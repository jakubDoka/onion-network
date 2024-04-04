#![feature(impl_trait_in_assoc_type)]
#![feature(slice_take)]
#![feature(macro_metavar_expr)]
#![feature(array_windows)]
#![feature(slice_flatten)]
#![feature(portable_simd)]
#![allow(internal_features)]
#![feature(ptr_internals)]
#![feature(trait_alias)]
#![feature(inline_const)]

pub use berkleamp_welch::{DecodeError, RebuildError, ResourcesError, Share as ReconstructPiece};
use {
    arrayvec::ArrayVec,
    codec::{Codec, Reminder},
};

pub mod handler;
pub mod protocol;
pub mod sorted_compact_vec;
pub mod rpcs {
    macro_rules! rpcs {
        ($($name:ident;)*) => { $( pub const $name: u8 = ${index(0)}; )* };
    }

    rpcs! {
        // client to node
        STORE_BLOCK;
        READ_BLOCK;

        // satelite to node
        ALLOCATE_BLOCK;
        GET_PIECE_PROOF;

        // node to satellite
        REGISTER_NODE;
        GET_GC_META;

        // client to satellite
        REGISTER_CLIENT;
        ALLOCATE_FILE;
        DELETE_FILE;
        ALLOCAET_BANDWIDTH;
        GET_FILE_HOLDERS;
    }
}

pub const PIECE_SIZE: usize = 1024;
pub const DATA_PIECES: usize = if cfg!(debug_assertions) { 4 } else { 16 };
pub const PARITY_PIECES: usize = if cfg!(debug_assertions) { 4 } else { 16 };
pub const MAX_PIECES: usize = DATA_PIECES + PARITY_PIECES;
pub const BLOCK_FRAGMENT_SIZE: usize = 1024 * 1024 * 8;
pub const BLOCK_SIZE: usize = MAX_PIECES * BLOCK_FRAGMENT_SIZE;
pub const BLOCK_PIECES: usize = BLOCK_FRAGMENT_SIZE / PIECE_SIZE;
pub const DATA_PER_BLOCK: usize = DATA_PIECES * BLOCK_FRAGMENT_SIZE;

pub type ReconstructBundle<'data> = [ReconstructPiece<'data>; DATA_PIECES];
pub type Piece = [u8; PIECE_SIZE];
pub type Data = [Piece; DATA_PIECES];
pub type Parity = [Piece; PARITY_PIECES];
pub type NodeIdentity = crypto::Hash;
pub type UserIdentity = crypto::Hash;
pub type BlockId = crypto::Hash;
pub type NodeId = u16;
pub type FreeSpace = u32;
pub type Holders = [NodeId; MAX_PIECES];
pub type ExpandedHolders = [NodeIdentity; MAX_PIECES];

#[repr(packed)]
#[derive(Clone, Copy)]
pub struct Address {
    pub starting_block: BlockId,
    pub block_offset: u16,
    pub size: u64,
}

#[derive(Codec, Clone, Copy)]
pub struct File {
    #[codec(with = codec::unsafe_as_raw_bytes)]
    pub address: Address,
    pub holders: ExpandedHolders,
}

#[repr(packed)]
#[derive(Clone, Copy)]
pub struct FileMeta {
    pub holders: Holders,
    pub owner: UserIdentity,
}

pub struct Encoding {
    inner: berkleamp_welch::Fec,
    buffer: Vec<u8>,
}

impl Default for Encoding {
    fn default() -> Self {
        Self { inner: berkleamp_welch::Fec::new(DATA_PIECES, PARITY_PIECES), buffer: vec![] }
    }
}

impl Encoding {
    pub fn encode(&self, data: &Data, parity: &mut Parity) {
        self.inner.encode(data.flatten(), parity.flatten_mut()).unwrap();
    }

    pub fn reconstruct(
        &mut self,
        shards: &mut ReconstructBundle,
    ) -> Result<(), berkleamp_welch::RebuildError> {
        self.inner.rebuild(shards, &mut self.buffer).map(drop)
    }

    pub fn find_cheaters(
        &mut self,
        shards: &mut ArrayVec<ReconstructPiece, MAX_PIECES>,
    ) -> Result<(), berkleamp_welch::DecodeError> {
        self.inner.decode(shards, true, &mut self.buffer).map(drop)
    }
}

#[derive(Codec)]
pub struct Request<'a> {
    pub prefix: u8,
    pub body: Reminder<'a>,
}
