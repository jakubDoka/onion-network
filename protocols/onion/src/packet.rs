pub use crypto::{
    enc::{Keypair, PublicKey},
    SharedSecret,
};
use {
    aes_gcm::{
        aead::{generic_array::GenericArray, OsRng},
        aes::cipher::Unsigned,
        AeadCore, AeadInPlace, Aes256Gcm, KeyInit,
    },
    codec::Codec,
    crypto::enc::Ciphertext,
    libp2p::{core::multihash::Multihash, identity::PeerId},
    std::usize,
};

pub const OK: u8 = 0;
pub const MISSING_PEER: u8 = 1;
pub const OCCUPIED_PEER: u8 = 2;
pub const ASOC_DATA: &[u8] =
    concat!("asoc-", env!("CARGO_PKG_VERSION"), "-", env!("CARGO_PKG_NAME"),).as_bytes();
pub const TAG_SIZE: usize = <Aes256Gcm as AeadCore>::TagSize::USIZE;
pub const NONCE_SIZE: usize = <Aes256Gcm as AeadCore>::NonceSize::USIZE;
pub const CONFIRM_PACKET_SIZE: usize = TAG_SIZE + NONCE_SIZE;
pub const PATH_LEN: usize = 2;

pub fn write_confirm(key: &SharedSecret, buffer: &mut [u8]) {
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let cipher = Aes256Gcm::new(&GenericArray::from(*key));

    let tag = cipher
        .encrypt_in_place_detached(&nonce, ASOC_DATA, &mut [])
        .expect("we are certainly not that big");

    buffer[..tag.len()].copy_from_slice(&tag);
    buffer[tag.len()..].copy_from_slice(&nonce);
}

pub fn verify_confirm(key: &SharedSecret, buffer: &mut [u8]) -> bool {
    peel_wih_key(key, buffer).is_some()
}

pub fn wrap(client_kp: &Keypair, sender: &PublicKey, buffer: &mut Vec<u8>) {
    let (cp, key) = client_kp.encapsulate(sender, OsRng);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let cipher = Aes256Gcm::new(&GenericArray::from(key));

    let tag = cipher
        .encrypt_in_place_detached(&nonce, ASOC_DATA, buffer)
        .expect("we are certainly not that big");

    buffer.extend_from_slice(&tag);
    buffer.extend_from_slice(&nonce);
    cp.encode(buffer).unwrap();
}

pub fn new_initial(
    recipient: &PublicKey,
    path: [(PublicKey, PeerId); PATH_LEN],
    client_kp: &Keypair,
    buffer: &mut Vec<u8>,
) -> SharedSecret {
    let (cp, key) = client_kp.encapsulate(recipient, OsRng);
    cp.encode(buffer).unwrap();

    for (pk, id) in path {
        let prev_len = buffer.len();
        let mh = Multihash::from(id);
        mh.write(&mut *buffer).expect("write to vector cannot fail");
        buffer.push((buffer.len() - prev_len) as u8);

        wrap(client_kp, &pk, buffer);
    }

    client_kp.public_key().encode(buffer).unwrap();

    key
}

pub fn peel_wih_key(key: &SharedSecret, mut buffer: &mut [u8]) -> Option<usize> {
    if buffer.len() < TAG_SIZE + NONCE_SIZE {
        return None;
    }

    let mut tail;

    (buffer, tail) = buffer.split_at_mut(buffer.len() - NONCE_SIZE);
    let nonce = *GenericArray::from_slice(tail);
    (buffer, tail) = buffer.split_at_mut(buffer.len() - TAG_SIZE);
    let tag = *GenericArray::from_slice(tail);

    let cipher = Aes256Gcm::new(&GenericArray::from(*key));

    cipher.decrypt_in_place_detached(&nonce, ASOC_DATA, buffer, &tag).ok()?;

    Some(buffer.len())
}

pub fn peel_initial(
    node_kp: &Keypair,
    original_buffer: &mut [u8],
) -> Option<(Option<PeerId>, SharedSecret, usize)> {
    #[derive(codec::Codec)]
    struct PostPacket {
        ciphertext: Ciphertext,
        sender: PublicKey,
    }

    let mut buffer = &mut *original_buffer;
    let tail = buffer.take_mut(buffer.len() - std::mem::size_of::<PostPacket>()..)?;
    let PostPacket { sender, ciphertext } = PostPacket::decode(&mut &*tail)?;
    let ss = node_kp.decapsulate(&ciphertext).ok()?;

    if buffer.is_empty() {
        return Some((None, ss, 0));
    }

    let packet_len = peel_wih_key(&ss, buffer)?;

    let buffer = &mut buffer[..packet_len];
    let (len, buffer) = buffer.split_last_mut()?;
    let (buffer, tail) = buffer.split_at_mut(buffer.len() - *len as usize);
    let id = PeerId::from_bytes(tail).ok()?;

    let len = buffer.len();
    sender.encode(&mut &mut original_buffer[len..]).unwrap();
    Some((Some(id), ss, len + std::mem::size_of::<PublicKey>()))
}
