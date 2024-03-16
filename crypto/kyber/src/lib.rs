#![no_std]
#![feature(array_chunks)]
#![feature(iter_array_chunks)]

mod barrett;
mod cbd;
mod indcpa;
mod kem;
mod mondgomery;
mod ntt;
mod params;
mod poly;
mod polyvec;
mod symetric;
mod verify;

use self::params::{INDCPA_SECRETKEYBYTES, SYMBYTES};
pub use params::{CIPHERTEXTBYTES, PUBLICKEYBYTES, SECRETKEYBYTES, SSBYTES};

pub const ENC_SEEDBYTES: usize = SYMBYTES;
pub const KEY_SEEDBYTES: usize = SYMBYTES * 2;

#[derive(Clone, Copy)]
pub struct Keypair([u8; SECRETKEYBYTES]);

impl Keypair {
    #[must_use]
    pub fn new(seed: &[u8; KEY_SEEDBYTES]) -> Self {
        Self(kem::keypair_derand(seed))
    }

    #[must_use]
    pub fn publickey(&self) -> PublicKey {
        PublicKey(
            self.0[INDCPA_SECRETKEYBYTES..INDCPA_SECRETKEYBYTES + PUBLICKEYBYTES]
                .try_into()
                .unwrap(),
        )
    }

    #[must_use]
    pub fn dec(&self, ct: &[u8; CIPHERTEXTBYTES]) -> Option<[u8; SSBYTES]> {
        kem::dec(*ct, self.0)
    }

    #[must_use]
    pub const fn to_bytes(&self) -> [u8; SECRETKEYBYTES] {
        self.0
    }

    #[must_use]
    pub const fn from_bytes(bytes: &[u8; SECRETKEYBYTES]) -> Self {
        Self(*bytes)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublicKey([u8; PUBLICKEYBYTES]);

impl PublicKey {
    #[must_use]
    pub fn enc(&self, seed: &[u8; ENC_SEEDBYTES]) -> ([u8; CIPHERTEXTBYTES], [u8; SSBYTES]) {
        kem::enc_derand(self.0, seed)
    }

    #[must_use]
    pub const fn to_bytes(&self) -> [u8; PUBLICKEYBYTES] {
        self.0
    }

    #[must_use]
    pub const fn from_bytes(bytes: &[u8; PUBLICKEYBYTES]) -> Self {
        Self(*bytes)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_kem() {
        let keypair = super::Keypair::new(&[6u8; super::KEY_SEEDBYTES]);
        let (ct, ss) = keypair.publickey().enc(&[7u8; super::ENC_SEEDBYTES]);
        let ss2 = keypair.dec(&ct).unwrap();
        assert_eq!(ss, ss2);
    }
}
