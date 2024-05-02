pub const SIZE: usize = 32;

pub type Hash = [u8; SIZE];

pub fn new(data: impl AsRef<[u8]>) -> Hash {
    blake3::hash(data.as_ref()).into()
}

#[must_use]
pub fn combine(left: Hash, right: Hash) -> Hash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(left.as_ref());
    hasher.update(right.as_ref());
    hasher.finalize().into()
}

#[must_use]
pub fn with_nonce(data: &[u8], nonce: u64) -> Hash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(data);
    hasher.update(&nonce.to_be_bytes());
    hasher.finalize().into()
}

pub fn kv(key: &[u8], value: &[u8]) -> Hash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(key);
    hasher.update(value);
    hasher.finalize().into()
}
