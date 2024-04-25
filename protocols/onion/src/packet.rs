pub use crypto::{
    enc::{Keypair, PublicKey},
    SharedSecret,
};
use {
    aes_gcm::aead::OsRng,
    arrayvec::ArrayVec,
    codec::{Codec, Decode, Encode},
    crypto::enc::Ciphertext,
    libp2p::identity::PeerId,
};

pub const OK: u8 = 0;
pub const MISSING_PEER: u8 = 1;
pub const OCCUPIED_PEER: u8 = 2;
pub const PATH_LEN: usize = 2;

const MAX_INNER_PACKET_SIZE: usize =
    std::mem::size_of::<Ciphertext>() + std::mem::size_of::<PeerId>();

#[derive(Codec)]
struct InnerMostPacket {
    #[codec(with = peer_id_codec)]
    to: PeerId,
    cp: Ciphertext,
}

#[derive(Codec)]
struct MiddlePacket {
    cp: Ciphertext,
    tag: [u8; crypto::TAG_SIZE],
    bytes: ArrayVec<u8, MAX_INNER_PACKET_SIZE>,
}

#[derive(Codec)]
pub struct OuterPacket {
    #[codec(with = peer_id_codec)]
    to: PeerId,
    mid: MiddlePacket,
}

impl OuterPacket {
    pub fn new(
        recipient: PublicKey,
        recipient_id: PeerId,
        mid_recipient: PublicKey,
        mid_id: PeerId,
        client_kp: &Keypair,
    ) -> (Self, SharedSecret) {
        let (cp, key) = client_kp.encapsulate(&recipient, OsRng);
        let inner = InnerMostPacket { to: recipient_id, cp };

        let (cp, mid_key) = client_kp.encapsulate(&mid_recipient, OsRng);
        let mut bytes = ArrayVec::new();
        inner.encode(&mut bytes).unwrap();
        let tag = crypto::encrypt(&mut bytes, mid_key, OsRng);
        let mid = MiddlePacket { cp, tag, bytes };

        (Self { to: mid_id, mid }, key)
    }
}

mod peer_id_codec {
    use {codec::WritableBuffer, libp2p::multihash::Multihash};

    pub fn encode(
        peer_id: &libp2p::identity::PeerId,
        buffer: &mut impl codec::Buffer,
    ) -> Option<()> {
        let mh = Multihash::<64>::from(*peer_id);
        mh.write(WritableBuffer { buffer }).ok()?;
        Some(())
    }

    pub fn decode(buffer: &mut &[u8]) -> Option<libp2p::identity::PeerId> {
        let mh = Multihash::<64>::read(buffer).ok()?;
        libp2p::identity::PeerId::from_multihash(mh).ok()
    }
}

pub fn peel(
    node_kp: &Keypair,
    mut original_buffer: &mut [u8],
) -> Option<(Result<PeerId, SharedSecret>, usize)> {
    let prev_len = original_buffer.len();

    if let Some(op) = OuterPacket::decode(&mut &*original_buffer) {
        op.mid.encode(&mut original_buffer).unwrap();
        return Some((Ok(op.to), prev_len - original_buffer.len()));
    }

    if let Some(mp) = MiddlePacket::decode(&mut &*original_buffer) {
        let ss = node_kp.decapsulate(&mp.cp).ok()?;
        original_buffer[..mp.bytes.len()].copy_from_slice(&mp.bytes);
        crypto::decrypt_separate_tag(&mut original_buffer[..mp.bytes.len()], ss, mp.tag)
            .then_some(())?;
        let ip = InnerMostPacket::decode(&mut &original_buffer[..mp.bytes.len()])?;
        ip.cp.encode(&mut original_buffer)?;
        return Some((Ok(ip.to), std::mem::size_of::<Ciphertext>()));
    }

    let cp = Ciphertext::decode(&mut &*original_buffer)?;
    let ss = node_kp.decapsulate(&cp).ok()?;
    Some((Err(ss), 0))
}
