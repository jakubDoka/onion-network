use {
    ed25519_dalek::{Signer, SigningKey, VerifyingKey},
    rand_core::CryptoRngCore,
};

pub type Pre = [u8; 32];

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "codec", derive(codec::Codec))]
pub struct Signature {
    post: [u8; falcon::BYTES],
    #[cfg_attr(feature = "codec", codec(with = codec::unsafe_as_raw_bytes))]
    pre: ed25519_dalek::Signature,
}

impl AsRef<[u8]> for Signature {
    fn as_ref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self as *const Self as *const u8,
                core::mem::size_of::<Self>(),
            )
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "codec", derive(codec::Codec))]
pub struct Keypair {
    #[cfg_attr(feature = "codec", codec(with = codec::unsafe_as_raw_bytes))]
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
    pub fn pre_quantum(&self) -> Pre {
        self.pre
    }

    pub fn identity(&self) -> crate::Hash {
        crate::hash::new(self.public_key())
    }
}

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "codec", derive(codec::Codec))]
pub struct PublicKey {
    #[cfg_attr(feature = "codec", codec(with = codec::unsafe_as_raw_bytes))]
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

impl AsRef<[u8]> for PublicKey {
    fn as_ref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self as *const Self as *const u8,
                core::mem::size_of::<Self>(),
            )
        }
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
