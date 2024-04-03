use rand_core::CryptoRngCore;

pub type Nonce = u64;

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "codec", derive(codec::Codec))]
pub struct Proof<T> {
    pub pk: crate::sign::PublicKey,
    pub nonce: Nonce,
    pub signature: crate::sign::Signature,
    pub context: T,
}

const PAYLOAD_SIZE: usize = core::mem::size_of::<Nonce>() + crate::hash::SIZE;

impl<T: ProofContext> Proof<T> {
    pub fn new(
        kp: &crate::sign::Keypair,
        nonce: &mut Nonce,
        context: T,
        rng: impl CryptoRngCore,
    ) -> Self {
        let signature = kp.sign(&Self::pack_payload(*nonce, &context), rng);
        *nonce += 1;
        Self { pk: kp.public_key(), nonce: *nonce - 1, signature, context }
    }

    fn pack_payload(nonce: Nonce, context: &T) -> [u8; PAYLOAD_SIZE] {
        let mut buf = [0; PAYLOAD_SIZE];
        buf[..crate::hash::SIZE].copy_from_slice(&crate::hash::new(context.as_bytes()));
        buf[crate::hash::SIZE..].copy_from_slice(&nonce.to_be_bytes());
        buf
    }

    pub fn verify(&self) -> bool {
        let bytes = Self::pack_payload(self.nonce, &self.context);
        self.pk.verify(&bytes, &self.signature).is_ok()
    }

    pub fn identity(&self) -> crate::hash::Hash {
        crate::hash::new(self.pk.as_ref())
    }
}

pub trait ProofContext {
    fn as_bytes(&self) -> &[u8];
}

impl<const SIZE: usize> ProofContext for [u8; SIZE] {
    fn as_bytes(&self) -> &[u8] {
        self
    }
}

impl ProofContext for &[u8] {
    fn as_bytes(&self) -> &[u8] {
        self
    }
}

impl ProofContext for str {
    fn as_bytes(&self) -> &[u8] {
        self.as_bytes()
    }
}

#[cfg(feature = "arrayvec")]
impl<const SIZE: usize> ProofContext for arrayvec::ArrayString<SIZE> {
    fn as_bytes(&self) -> &[u8] {
        self.as_str().as_bytes()
    }
}

#[cfg(feature = "codec")]
impl ProofContext for codec::Reminder<'_> {
    fn as_bytes(&self) -> &[u8] {
        self.0
    }
}
