use {
    crate::params::SYMBYTES,
    sha3::{
        digest::{ExtendableOutput, Update, XofReader},
        Digest,
    },
};

pub type XofState = sha3::Shake128;

pub fn hash_h(input: &[u8]) -> [u8; SYMBYTES] {
    sha3::Sha3_256::digest(input).into()
}

pub fn hash_g(input: &[u8]) -> [u8; SYMBYTES * 2] {
    sha3::Sha3_512::digest(input).into()
}

pub fn xof_absorb(state: &mut XofState, seed: &[u8; SYMBYTES], x: u8, y: u8) {
    state.update(seed);
    state.update(&[x, y]);
}

pub fn prf(out: &mut [u8], key: &[u8; SYMBYTES], nonce: u8) {
    let mut state = sha3::Shake256::default();
    state.update(key);
    state.update(&[nonce]);
    state.finalize_xof().read(out);
}
