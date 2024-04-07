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
    crypto::{proof::Proof, sign::Signature},
    rpc::CallId,
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
        STORE_FILE;
        READ_FILE;

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
pub type FreeSpace = u64;
pub type Holders = [NodeId; MAX_PIECES];
pub type ExpandedHolders = [NodeIdentity; MAX_PIECES];
pub type ClientResult<T, E = ClientError> = Result<T, E>;
pub type NodeResult<T, E = NodeError> = Result<T, E>;

#[derive(Codec, thiserror::Error, Debug)]
pub enum ClientError {
    #[error("not enough nodes to store the file")]
    NotEnoughtNodes,
    #[error("internal error, actual message is logged")]
    InternalError,
    #[error("not allowed")]
    NotAllowed,
    #[error("already registered")]
    AlreadyRegistered,
    #[error("invalid proof")]
    InvalidProof,
    #[error("you won the lottery, now try again")]
    YouWonTheLottery,
    #[error("not found")]
    NotFound,
}

impl From<anyhow::Error> for ClientError {
    fn from(err: anyhow::Error) -> Self {
        log::error!("{:#?}", err);
        ClientError::InternalError
    }
}

#[derive(Codec, thiserror::Error, Debug)]
pub enum NodeError {
    #[error("invalid proof")]
    InvalidProof,
    #[error("already registered")]
    AlreadyRegistered,
    #[error("not registered")]
    NotRegistered,
    #[error("invalid nonce, expected: {0}")]
    InvalidNonce(u64),
    #[error("store thrown unexpected error, actual message is logged")]
    StoreError,
}

impl From<anyhow::Error> for NodeError {
    fn from(err: anyhow::Error) -> Self {
        log::error!("lmdb error: {:?}", err);
        NodeError::StoreError
    }
}

#[repr(packed)]
#[derive(Clone, Copy, Codec)]
pub struct Address {
    pub id: BlockId,
    pub size: u64,
}

#[repr(packed)]
#[derive(Clone, Copy, Codec)]
pub struct StoreContext {
    pub address: Address,
    pub dest: NodeIdentity,
}

#[repr(packed)]
#[derive(Clone, Copy, Codec)]
pub struct BandwidthContext {
    pub dest: NodeIdentity,
    pub issuer: UserIdentity,
    pub amount: u64,
}

#[derive(Codec, Clone, Copy)]
pub struct CompactBandwidthUse {
    pub signature: Signature,
    pub amount: u64,
}

#[derive(Codec, Clone, Copy)]
pub struct BandwidthUse {
    pub proof: Proof<BandwidthContext>,
    pub amount: u64,
}
impl BandwidthUse {
    pub fn is_exhaused(&self) -> bool {
        self.amount == self.proof.context.amount
    }
}

#[derive(Codec, Clone, Copy)]
pub struct File {
    pub address: Address,
    pub holders: ExpandedHolders,
}

#[repr(packed)]
#[derive(Clone, Copy, Codec)]
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

#[derive(Codec)]
pub enum StreamKind {
    RequestStream(CallId),
}
