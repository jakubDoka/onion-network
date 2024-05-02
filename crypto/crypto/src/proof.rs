use {
    codec::{Decode, Encode},
    rand_core::CryptoRngCore,
};

pub type Nonce = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConstContext<const N: usize>;

impl<const N: usize> ConstContext<N> {
    pub fn new() -> Self {
        Self
    }
}

impl<const N: usize> Encode for ConstContext<N> {
    fn encode(&self, b: &mut impl codec::Buffer) -> Option<()> {
        N.encode(b)
    }
}

impl<'a, const N: usize> Decode<'a> for ConstContext<N> {
    fn decode(b: &mut &[u8]) -> Option<Self> {
        usize::decode(b).filter(|&v| v == N).map(|_| Self)
    }
}

pub trait NonceInt {
    fn next(&mut self) -> Self;
    fn advance_to(&mut self, to: Self) -> bool;
}

impl NonceInt for Nonce {
    fn next(&mut self) -> Self {
        let old = *self;
        *self += 1;
        old
    }

    fn advance_to(&mut self, to: Self) -> bool {
        if *self < to {
            *self = to;
            true
        } else {
            false
        }
    }
}

#[derive(Clone, Copy, Debug, codec::Codec)]
pub struct Proof<T = ()> {
    pub pk: crate::sign::PublicKey,
    pub nonce: Nonce,
    pub signature: crate::sign::Signature,
    pub context: T,
}

impl<T> Proof<T> {
    pub fn new(
        kp: &crate::sign::Keypair,
        nonce: &mut Nonce,
        context: T,
        rng: impl CryptoRngCore,
    ) -> Self
    where
        T: Encode,
    {
        let signature = kp.sign(&Self::pack_payload(*nonce, &context), rng);
        Self { pk: kp.public_key(), nonce: nonce.next(), signature, context }
    }

    fn pack_payload(nonce: Nonce, context: &T) -> crate::Hash
    where
        T: Encode,
    {
        struct BlakeBuffer(blake3::Hasher);

        impl codec::Buffer for BlakeBuffer {
            fn extend_from_slice(&mut self, slice: &[u8]) -> Option<()> {
                self.0.update(slice);
                Some(())
            }

            fn push(&mut self, byte: u8) -> Option<()> {
                self.0.update(&[byte]);
                Some(())
            }
        }

        impl AsMut<[u8]> for BlakeBuffer {
            fn as_mut(&mut self) -> &mut [u8] {
                unimplemented!()
            }
        }

        let mut hasher = BlakeBuffer(blake3::Hasher::new());
        (nonce, context).encode(&mut hasher).unwrap();

        hasher.0.finalize().into()
    }

    pub fn verify(&self) -> bool
    where
        T: Encode,
    {
        self.pk.verify(&Self::pack_payload(self.nonce, &self.context), &self.signature).is_ok()
    }

    pub fn identity(&self) -> crate::hash::Hash {
        crate::hash::new(self.pk.as_ref())
    }
}
