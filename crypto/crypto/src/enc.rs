use {crate::SharedSecret, core::array, rand_core::CryptoRngCore};

#[derive(Clone)]
#[cfg_attr(feature = "codec", derive(codec::Codec))]
pub struct Ciphertext {
    pl: [u8; kyber::KYBER_CIPHERTEXTBYTES],
    #[cfg_attr(feature = "codec", codec(with = codec::unsafe_as_raw_bytes))]
    x: x25519_dalek::PublicKey,
}

#[derive(Clone)]
#[cfg_attr(feature = "codec", derive(codec::Codec))]
pub struct ChoosenCiphertext {
    xored_secret: SharedSecret,
    cp: Ciphertext,
}

#[derive(Clone)]
#[cfg_attr(feature = "codec", derive(codec::Codec))]
pub struct Keypair {
    #[cfg_attr(feature = "codec", codec(with = codec::unsafe_as_raw_bytes))]
    post: [u8; kyber::KYBER_SECRETKEYBYTES],
    #[allow(dead_code)]
    #[cfg_attr(feature = "codec", codec(with = codec::unsafe_as_raw_bytes))]
    pre: x25519_dalek::StaticSecret,
}

impl PartialEq for Keypair {
    fn eq(&self, other: &Self) -> bool {
        kyber::public(&self.post) == kyber::public(&other.post)
            && self.pre.to_bytes() == other.pre.to_bytes()
    }
}

impl Eq for Keypair {}

impl Keypair {
    pub fn new(mut rng: impl CryptoRngCore) -> Self {
        let post = kyber::Keypair::generate(&mut rng).unwrap().secret;
        let pre = x25519_dalek::StaticSecret::random_from_rng(rng);
        Self { post, pre }
    }

    #[must_use]
    pub fn public_key(&self) -> PublicKey {
        PublicKey { post: kyber::public(&self.post), pre: x25519_dalek::PublicKey::from(&self.pre) }
    }

    pub fn encapsulate(
        &self,
        public_key: &PublicKey,
        mut rng: impl CryptoRngCore,
    ) -> (Ciphertext, SharedSecret) {
        let (data, k_secret) = kyber::encapsulate(&public_key.post, &mut rng).unwrap();
        let x_secret = self.pre.diffie_hellman(&public_key.pre).to_bytes();
        let secret = array::from_fn(|i| k_secret[i] ^ x_secret[i]);
        (Ciphertext { pl: data, x: x25519_dalek::PublicKey::from(&self.pre) }, secret)
    }

    pub fn encapsulate_choosen(
        &self,
        public_key: &PublicKey,
        secret: SharedSecret,
        mut rng: impl CryptoRngCore,
    ) -> ChoosenCiphertext {
        let (cp, key) = self.encapsulate(public_key, &mut rng);
        ChoosenCiphertext { xored_secret: array::from_fn(|i| secret[i] ^ key[i]), cp }
    }

    pub fn decapsulate(&self, ciphertext: &Ciphertext) -> Result<SharedSecret, DecapsulationError> {
        let x_secret = self.pre.diffie_hellman(&ciphertext.x).to_bytes();
        let k_secret =
            kyber::decapsulate(&ciphertext.pl, &self.post).ok().ok_or(DecapsulationError::Kyber)?;
        Ok(array::from_fn(|i| k_secret[i] ^ x_secret[i]))
    }

    pub fn decapsulate_choosen(
        &self,
        ciphertext: &ChoosenCiphertext,
    ) -> Result<SharedSecret, DecapsulationError> {
        let secret = self.decapsulate(&ciphertext.cp)?;
        Ok(array::from_fn(|i| secret[i] ^ ciphertext.xored_secret[i]))
    }
}

#[derive(Debug)]
pub enum DecapsulationError {
    Kyber,
    Aes,
}

impl core::fmt::Display for DecapsulationError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Kyber => write!(f, "kyber error"),
            Self::Aes => write!(f, "aes error"),
        }
    }
}

impl core::error::Error for DecapsulationError {}

impl From<aes_gcm::Error> for DecapsulationError {
    fn from(_: aes_gcm::Error) -> Self {
        Self::Aes
    }
}

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "codec", derive(codec::Codec))]
pub struct PublicKey {
    #[allow(dead_code)]
    #[cfg_attr(feature = "codec", codec(with = codec::unsafe_as_raw_bytes))]
    post: kyber::PublicKey,
    #[allow(dead_code)]
    #[cfg_attr(feature = "codec", codec(with = codec::unsafe_as_raw_bytes))]
    pre: x25519_dalek::PublicKey,
}

impl AsRef<[u8]> for PublicKey {
    fn as_ref(&self) -> &[u8] {
        unsafe { &*(self as *const Self as *const [u8; core::mem::size_of::<PublicKey>()]) }
    }
}

#[cfg(test)]
mod tests {
    use {crate::SHARED_SECRET_SIZE, rand_core::OsRng};

    #[test]
    fn test_enc_dec() {
        use super::*;
        let alice = Keypair::new(OsRng);
        let bob = Keypair::new(OsRng);
        let (ciphertext, secret) = alice.encapsulate(&bob.public_key(), OsRng);
        let dec = bob.decapsulate(&ciphertext).unwrap();
        assert_eq!(secret, dec);
    }

    #[test]
    fn test_enc_dec_choosen() {
        use super::*;
        let alice = Keypair::new(OsRng);
        let bob = Keypair::new(OsRng);
        let secret = [42u8; SHARED_SECRET_SIZE];
        let ciphertext = alice.encapsulate_choosen(&bob.public_key(), secret, OsRng);
        let dec = bob.decapsulate_choosen(&ciphertext).unwrap();
        assert_eq!(secret, dec);
    }
}
