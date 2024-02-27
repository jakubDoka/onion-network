use {
    ed25519_dalek::{Signer, SigningKey, VerifyingKey},
    rand_core::CryptoRngCore,
};

impl_transmute! {
    Signature,
    Keypair,
    PublicKey,
}

pub type Ed = [u8; 32];

#[derive(Clone, Copy)]
pub struct Signature {
    post: [u8; falcon::BYTES],
    pre: ed25519_dalek::Signature,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Keypair {
    post: falcon::Keypair,
    pre: ed25519_dalek::SecretKey,
}

impl Keypair {
    pub fn new(mut rng: impl CryptoRngCore) -> Self {
        let mut seed = [0u8; falcon::SEED_BYTES];
        rng.fill_bytes(&mut seed);
        let post = falcon::Keypair::new(&seed).expect("yea, whatever");
        let pre = SigningKey::generate(&mut rng).to_bytes();
        Self { post, pre }
    }

    #[must_use]
    pub fn public_key(&self) -> PublicKey {
        PublicKey {
            post: *self.post.public_key(),
            pre: SigningKey::from_bytes(&self.pre).verifying_key().to_bytes(),
        }
    }

    pub fn sign(&self, data: &[u8], rng: impl CryptoRngCore) -> Signature {
        let post = self.post.sign(data, rng).expect("really now?");
        let pre = SigningKey::from(&self.pre)
            .try_sign(data)
            .expect("cannot fail from the implementation");
        Signature { post, pre }
    }

    #[must_use]
    pub fn pre_quantum(&self) -> Ed {
        self.pre
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PublicKey {
    pub post: falcon::PublicKey,
    pub pre: [u8; ed25519_dalek::PUBLIC_KEY_LENGTH],
}

impl PublicKey {
    pub fn verify(&self, data: &[u8], signature: &Signature) -> Result<(), SignatureError> {
        VerifyingKey::from_bytes(&self.pre)
            .and_then(|vk| vk.verify_strict(data, &signature.pre))
            .map_err(|_| SignatureError::PreQuantum)?;
        self.post.verify(data, &signature.post).then_some(()).ok_or(SignatureError::PostQuantum)?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum SignatureError {
    PostQuantum,
    PreQuantum,
}

impl core::fmt::Display for SignatureError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::PostQuantum => write!(f, "post quantum signature verification failed"),
            Self::PreQuantum => write!(f, "pre quantum signature verification failed"),
        }
    }
}

impl core::error::Error for SignatureError {}

#[cfg(test)]
mod test {
    use rand_core::OsRng;

    #[test]
    fn test_sign_verify() {
        use super::*;
        let keypair = Keypair::new(OsRng);
        let data = b"hello world";
        let signature = keypair.sign(data, OsRng);
        let public_key = keypair.public_key();
        public_key.verify(data, &signature).unwrap();
        public_key.verify(b"deez nust", &signature).expect_err("invalid signature");
    }
}
