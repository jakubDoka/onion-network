use crate::TransmutationCircle;

pub const SIZE: usize = 32;

pub type Hash = [u8; SIZE];

pub fn new<T: TransmutationCircle>(data: &T) -> Hash {
    blake3::hash(data.as_bytes().as_ref()).into()
}

#[must_use]
pub fn from_raw(data: &[u8]) -> Hash {
    blake3::hash(data).into()
}

#[must_use]
pub fn combine(left: Hash, right: Hash) -> Hash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(left.as_ref());
    hasher.update(right.as_ref());
    hasher.finalize().into()
}

#[must_use]
pub fn from_slice(slice: &[u8]) -> Hash {
    blake3::hash(slice).into()
}

#[must_use]
pub fn with_nonce(data: &[u8], nonce: u64) -> Hash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(data);
    hasher.update(&nonce.to_be_bytes());
    hasher.finalize().into()
}
