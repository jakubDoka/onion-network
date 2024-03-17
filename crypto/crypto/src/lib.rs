#![no_std]
#![feature(error_in_core)]

use aes_gcm::{
    aead::{generic_array::GenericArray, Tag},
    aes::cipher::Unsigned,
    AeadCore, AeadInPlace, Aes256Gcm, KeyInit, KeySizeUser, Nonce,
};

pub const ASOC_DATA: &[u8] = concat!("pqc-orion-crypto/enc/", env!("CARGO_PKG_VERSION")).as_bytes();

pub mod enc;
pub mod hash;
pub mod sign;

use rand_core::CryptoRngCore;
pub use {hash::Hash, rand_core};

pub fn new_secret(mut rng: impl CryptoRngCore) -> SharedSecret {
    let mut secret = [0; SHARED_SECRET_SIZE];
    rng.fill_bytes(&mut secret);
    secret
}

pub fn decrypt(data: &mut [u8], secret: SharedSecret) -> Option<&mut [u8]> {
    if data.len() < NONCE_SIZE + TAG_SIZE {
        return None;
    }

    let (data, postfix) = data.split_at_mut(data.len() - NONCE_SIZE - TAG_SIZE);
    let nonce = <Nonce<<Aes256Gcm as AeadCore>::NonceSize>>::from_slice(&postfix[TAG_SIZE..]);
    let tag = <Tag<Aes256Gcm>>::from_slice(&postfix[..TAG_SIZE]);
    let cipher = Aes256Gcm::new(&GenericArray::from(secret));
    cipher.decrypt_in_place_detached(nonce, crate::ASOC_DATA, data, tag).ok().map(|()| data)
}

#[must_use = "dont forget to append the array to the data"]
pub fn encrypt(
    data: &mut [u8],
    secret: SharedSecret,
    rng: impl CryptoRngCore,
) -> [u8; TAG_SIZE + NONCE_SIZE] {
    let nonce = Aes256Gcm::generate_nonce(rng);
    let cipher = Aes256Gcm::new(&GenericArray::from(secret));
    let tag = cipher
        .encrypt_in_place_detached(&nonce, crate::ASOC_DATA, data)
        .expect("data to not be that big");

    unsafe { core::mem::transmute((tag, nonce)) }
}

const NONCE_SIZE: usize = <<Aes256Gcm as AeadCore>::NonceSize as Unsigned>::USIZE;
const TAG_SIZE: usize = <<Aes256Gcm as AeadCore>::TagSize as Unsigned>::USIZE;

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "codec", derive(codec::Codec))]
pub struct FixedAesPayload<const SIZE: usize> {
    data: [u8; SIZE],
    #[cfg_attr(feature = "codec", codec(with = codec::unsafe_as_raw_bytes))]
    tag: Tag<Aes256Gcm>,
    #[cfg_attr(feature = "codec", codec(with = codec::unsafe_as_raw_bytes))]
    nonce: Nonce<<Aes256Gcm as AeadCore>::NonceSize>,
}

impl<const SIZE: usize> FixedAesPayload<SIZE> {
    pub fn new(
        mut data: [u8; SIZE],
        key: &SharedSecret,
        asoc_data: &[u8],
        rng: impl CryptoRngCore,
    ) -> Self {
        let nonce = Aes256Gcm::generate_nonce(rng);
        let cipher = Aes256Gcm::new(&GenericArray::from(*key));
        let tag = cipher
            .encrypt_in_place_detached(&nonce, asoc_data, &mut data)
            .expect("cannot fail from the implementation");
        Self { data, tag, nonce }
    }

    pub fn decrypt(
        self,
        key: SharedSecret,
        asoc_data: &[u8],
    ) -> Result<[u8; SIZE], aes_gcm::Error> {
        let mut data = self.data;
        let cipher = Aes256Gcm::new(&GenericArray::from(key));
        cipher
            .decrypt_in_place_detached(&self.nonce, asoc_data, &mut data, &self.tag)
            .map(|()| data)
    }
}

pub const SHARED_SECRET_SIZE: usize = <<Aes256Gcm as KeySizeUser>::KeySize as Unsigned>::USIZE;

pub type SharedSecret = [u8; SHARED_SECRET_SIZE];
