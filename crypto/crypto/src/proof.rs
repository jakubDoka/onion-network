use {codec::Codec, rand_core::CryptoRngCore};

pub type Nonce = u64;

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
        if *self + 1 == to {
            *self = to;
            true
        } else {
            false
        }
    }
}

#[derive(Clone, Copy, Debug, codec::Codec)]
pub struct Proof<T> {
    pub pk: crate::sign::PublicKey,
    pub nonce: Nonce,
    pub signature: crate::sign::Signature,
    pub context: T,
}

impl<T> Proof<T> {
    pub fn new<'a>(
        kp: &crate::sign::Keypair,
        nonce: &mut Nonce,
        context: T,
        rng: impl CryptoRngCore,
    ) -> Self
    where
        T: Codec<'a>,
    {
        let signature = kp.sign(&Self::pack_payload(*nonce, &context), rng);
        Self { pk: kp.public_key(), nonce: nonce.next(), signature, context }
    }

    fn pack_payload<'a>(nonce: Nonce, context: &T) -> crate::Hash
    where
        T: Codec<'a>,
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

    pub fn verify<'a>(&self) -> bool
    where
        T: Codec<'a>,
    {
        self.pk.verify(&Self::pack_payload(self.nonce, &self.context), &self.signature).is_ok()
    }

    pub fn identity(&self) -> crate::hash::Hash {
        crate::hash::new(self.pk.as_ref())
    }
}
