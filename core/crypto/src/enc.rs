use {
    crate::{FixedAesPayload, SharedSecret, ASOC_DATA},
    core::array,
    rand_core::CryptoRngCore,
};

impl_transmute! {
    Keypair,
    PublicKey,
    Ciphertext,
    ChoosenCiphertext,
}

#[derive(Clone)]
pub struct Ciphertext {
    pl: [u8; kyber::CIPHERTEXTBYTES],
    x: x25519_dalek::PublicKey,
}

#[derive(Clone)]
pub struct ChoosenCiphertext {
    pl: FixedAesPayload<32>,
    cp: Ciphertext,
}

#[derive(Clone)]
pub struct Keypair {
    post: kyber::Keypair,
    #[allow(dead_code)]
    pre: x25519_dalek::StaticSecret,
}

impl PartialEq for Keypair {
    fn eq(&self, other: &Self) -> bool {
        self.post.publickey() == other.post.publickey()
            && self.pre.to_bytes() == other.pre.to_bytes()
    }
}

impl Eq for Keypair {}

impl Keypair {
    pub fn new(mut rng: impl CryptoRngCore) -> Self {
        let mut seed = [0; kyber::KEY_SEEDBYTES];
        rng.fill_bytes(&mut seed);
        let post = kyber::Keypair::new(&seed);
        let pre = x25519_dalek::StaticSecret::random_from_rng(rng);
        Self { post, pre }
    }

    #[must_use]
    pub fn public_key(&self) -> PublicKey {
        PublicKey { post: self.post.publickey(), pre: x25519_dalek::PublicKey::from(&self.pre) }
    }

    pub fn encapsulate(
        &self,
        public_key: &PublicKey,
        mut rng: impl CryptoRngCore,
    ) -> (Ciphertext, SharedSecret) {
        let mut seed = [0; kyber::ENC_SEEDBYTES];
        rng.fill_bytes(&mut seed);
        let (data, k_secret) = public_key.post.enc(&seed);
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
        ChoosenCiphertext { pl: FixedAesPayload::new(secret, &key, ASOC_DATA, rng), cp }
    }

    pub fn decapsulate(&self, ciphertext: &Ciphertext) -> Result<SharedSecret, DecapsulationError> {
        let x_secret = self.pre.diffie_hellman(&ciphertext.x).to_bytes();
        let k_secret = self.post.dec(&ciphertext.pl).ok_or(DecapsulationError::Kyber)?;
        Ok(array::from_fn(|i| k_secret[i] ^ x_secret[i]))
    }

    pub fn decapsulate_choosen(
        &self,
        ciphertext: &ChoosenCiphertext,
    ) -> Result<SharedSecret, DecapsulationError> {
        let secret = self.decapsulate(&ciphertext.cp)?;
        Ok(ciphertext.pl.decrypt(secret, ASOC_DATA)?)
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
pub struct PublicKey {
    #[allow(dead_code)]
    post: kyber::PublicKey,
    #[allow(dead_code)]
    pre: x25519_dalek::PublicKey,
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
