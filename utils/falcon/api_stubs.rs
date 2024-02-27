use rand_core::CryptoRngCore;

pub const SECRETKEYBYTES: usize = 1281;
pub const PUBLICKEYBYTES: usize = 897;
pub const BYTES: usize = 668;
pub const SEED_BYTES: usize = 48;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Keypair {
    pk: PublicKey,
    sk: [u8; SECRETKEYBYTES],
}

impl Keypair {
    pub fn new(seed: &[u8; SEED_BYTES]) -> Option<Self> {
        let mut pk = [0u8; PUBLICKEYBYTES];
        let mut sk = [0u8; SECRETKEYBYTES];
        unsafe {
            let res = src::pqclean::PQCLEAN_FALCON512_CLEAN_crypto_sign_keypair(
                |ptr, len| {
                    assert_eq!(len as usize, SEED_BYTES);
                    ptr.copy_from_nonoverlapping(seed.as_ptr(), len as usize);
                    0
                },
                pk.as_mut_ptr(),
                sk.as_mut_ptr(),
            );

            if res != 0 {
                return None;
            }
        }
        Some(Self {
            pk: PublicKey(pk),
            sk,
        })
    }

    pub fn sign(&self, message: &[u8], mut rng: impl CryptoRngCore) -> Option<[u8; BYTES]> {
        let mut sig = [0u8; BYTES];
        let mut len = 0;
        unsafe {
            let res = src::pqclean::PQCLEAN_FALCON512_CLEAN_crypto_sign_signature(
                |addr, len| {
                    let slice = core::slice::from_raw_parts_mut(addr, len as usize);
                    rng.fill_bytes(slice);
                    0
                },
                sig.as_mut_ptr(),
                &mut len,
                message.as_ptr(),
                message.len() as _,
                self.sk.as_ptr(),
            );
            if res != 0 {
                return None;
            }
        }
        sig[BYTES - 2..].copy_from_slice(&(len as u16).to_le_bytes());
        Some(sig)
    }

    pub fn public_key(&self) -> &PublicKey {
        &self.pk
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PublicKey([u8; PUBLICKEYBYTES]);

impl PublicKey {
    pub fn verify(&self, message: &[u8], sig: &[u8; BYTES]) -> bool {
        let len = u16::from_le_bytes([sig[BYTES - 2], sig[BYTES - 1]]);
        unsafe {
            src::pqclean::PQCLEAN_FALCON512_CLEAN_crypto_sign_verify(
                sig.as_ptr(),
                len as _,
                message.as_ptr(),
                message.len() as _,
                self.0.as_ptr(),
            ) == 0
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        rand_core::{CryptoRng, RngCore},
    };

    struct TotallyRandom;

    impl RngCore for TotallyRandom {
        fn next_u32(&mut self) -> u32 {
            todo!()
        }

        fn next_u64(&mut self) -> u64 {
            todo!()
        }

        fn fill_bytes(&mut self, _: &mut [u8]) {}

        fn try_fill_bytes(&mut self, _: &mut [u8]) -> Result<(), rand_core::Error> {
            todo!()
        }
    }

    impl CryptoRng for TotallyRandom {}

    #[test]
    fn test_sign_verify() {
        let keypair = Keypair::new(&[0u8; SEED_BYTES]).unwrap();
        let message = b"Hello, world!";
        let sig = keypair.sign(message, TotallyRandom).unwrap();
        assert!(keypair.public_key().verify(message, &sig));
    }
}
