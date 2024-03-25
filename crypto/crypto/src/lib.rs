#![no_std]
#![feature(error_in_core)]

use {
    aes_gcm::{
        aead::{generic_array::GenericArray, Tag},
        aes::cipher::Unsigned,
        AeadCore, AeadInPlace, Aes256Gcm, KeyInit, KeySizeUser, Nonce,
    },
    rand_core::CryptoRngCore,
};
pub use {hash::Hash, rand_core};

pub mod enc;
pub mod hash;
pub mod sign;

const NONCE_SIZE: usize = <<Aes256Gcm as AeadCore>::NonceSize as Unsigned>::USIZE;
const _TAG_SIZE: usize = <<Aes256Gcm as AeadCore>::TagSize as Unsigned>::USIZE;

pub const ASOC_DATA: &[u8] = concat!("pqc-orion-crypto/enc/", env!("CARGO_PKG_VERSION")).as_bytes();
pub const TAG_SIZE: usize = _TAG_SIZE + NONCE_SIZE;
pub const SHARED_SECRET_SIZE: usize = <<Aes256Gcm as KeySizeUser>::KeySize as Unsigned>::USIZE;

pub type SharedSecret = [u8; SHARED_SECRET_SIZE];

pub fn new_secret(mut rng: impl CryptoRngCore) -> SharedSecret {
    let mut secret = [0; SHARED_SECRET_SIZE];
    rng.fill_bytes(&mut secret);
    secret
}

pub fn decrypt(data: &mut [u8], secret: SharedSecret) -> Option<&mut [u8]> {
    if data.len() < NONCE_SIZE + _TAG_SIZE {
        return None;
    }
    let (data, postfix) = data.split_at_mut(data.len() - NONCE_SIZE - _TAG_SIZE);
    decrypt_separate_tag(data, secret, postfix.try_into().unwrap()).then_some(data)
}

pub fn decrypt_separate_tag(
    data: &mut [u8],
    secret: SharedSecret,
    postfix: [u8; TAG_SIZE],
) -> bool {
    let nonce = <Nonce<<Aes256Gcm as AeadCore>::NonceSize>>::from_slice(&postfix[_TAG_SIZE..]);
    let tag = <Tag<Aes256Gcm>>::from_slice(&postfix[.._TAG_SIZE]);
    let cipher = Aes256Gcm::new(&GenericArray::from(secret));
    cipher.decrypt_in_place_detached(nonce, crate::ASOC_DATA, data, tag).is_ok()
}

#[must_use = "dont forget to append the array to the data"]
pub fn encrypt(data: &mut [u8], secret: SharedSecret, rng: impl CryptoRngCore) -> [u8; TAG_SIZE] {
    let nonce = Aes256Gcm::generate_nonce(rng);
    let cipher = Aes256Gcm::new(&GenericArray::from(secret));
    let tag = cipher
        .encrypt_in_place_detached(&nonce, crate::ASOC_DATA, data)
        .expect("data to not be that big");

    unsafe { core::mem::transmute((tag, nonce)) }
}
