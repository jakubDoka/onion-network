#![feature(iter_next_chunk)]
#![feature(let_chains)]
#![feature(slice_take)]
#![feature(iter_advance_by)]
#![feature(macro_metavar_expr)]
#![feature(associated_type_defaults)]
#![feature(impl_trait_in_assoc_type)]
#![feature(extract_if)]
#![feature(slice_from_ptr_range)]

use {
    arrayvec::ArrayVec,
    codec::Codec,
    crypto::proof::{Nonce, Proof},
    std::{io, num::NonZeroUsize},
};

pub const REPLICATION_FACTOR: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(4) };

pub type BlockNumber = u64;
pub type Identity = crypto::Hash;
pub type ReplVec<T> = ArrayVec<T, { REPLICATION_FACTOR.get() }>;
pub type GroupVec<T> = ArrayVec<T, { REPLICATION_FACTOR.get() + 1 }>;

mod chat;
mod profile;
pub mod rpcs {
    macro_rules! rpcs {
        ($($name:ident;)*) => { $( pub const $name: u8 = ${index(0)}; )* };
    }

    rpcs! {
        CREATE_CHAT;
        ADD_MEMBER;
        KICK_MEMBER;
        SEND_MESSAGE;
        SEND_BLOCK;
        FETCH_CHAT_DATA;
        FETCH_MESSAGES;
        VOTE_BLOCK;
        FETCH_MEMBERS;

        FETCH_PROFILE;
        FETCH_PROFILE_FULL;
        CREATE_PROFILE;
        SEND_MAIL;
        READ_MAIL;
        INSERT_TO_VAULT;
        REMOVE_FROM_VAULT;

        FETCH_VAULT;
        FETCH_VAULT_KEY;

        SUBSCRIBE;
        UNSUBSCRIBE;
    }
}

pub use {chat::*, profile::*};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Codec, thiserror::Error)]
pub enum ChatError {
    #[error("invalid proof")]
    InvalidProof,
    #[error("not found")]
    NotFound,
    #[error("outdated")]
    Outdated,
    #[error("already exists")]
    AlreadyExists,
    #[error("invalid action")]
    InvalidAction,
    #[error("sent directly")]
    SentDirectly,
    #[error("sending to self is not allowed")]
    SendingToSelf,
    #[error("mailbox full (limit: {MAIL_BOX_CAP})")]
    MailboxFull,
    #[error("you are not a member")]
    NotMember,
    #[error("member already exists")]
    AlreadyMember,
    #[error("invalid action, expected nonce higher then {0}")]
    InvalidChatAction(Nonce),
    #[error("message too large")]
    MessageTooLarge,
    #[error("latest message block is still being finalized")]
    MessageOverload,
    #[error("no blocks even though past block was proposed")]
    NoBlocks,
    #[error("The sending node is not among replicators")]
    NoReplicator,
    #[error("block contains unexpected messages")]
    BlockUnexpectedMessages,
    #[error("block is not expected")]
    BlockNotExpected,
    #[error("no majority to confirm the request")]
    NoMajority,
    #[error("already voted")]
    AlreadyVoted,
    #[error("vote not found")]
    VoteNotFound,
    #[error("no permission")]
    NoPermission,
    #[error("rate limited for next {0}")]
    RateLimited(MsTilEnd),
    #[error("invalid response")]
    InvalidResponse,
    #[error("timeout")]
    Timeout,
}

impl From<io::Error> for ChatError {
    fn from(e: io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::TimedOut => Self::Timeout,
            _ => Self::InvalidResponse,
        }
    }
}

#[derive(Clone, Copy, Debug, Codec)]
struct MsTilEnd(u64);

impl PartialEq<MsTilEnd> for MsTilEnd {
    fn eq(&self, other: &Self) -> bool {
        self.0.abs_diff(other.0) < 3000
    }
}

impl Eq for MsTilEnd {}

impl std::fmt::Display for MsTilEnd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}ms", self.0)
    }
}

impl ChatError {
    pub fn recover(self) -> Result<(), Self> {
        match self {
            Self::SentDirectly => Ok(()),
            e => Err(e),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Codec)]
pub enum Topic {
    Profile(Identity),
    Chat(ChatName),
}

impl Topic {
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Profile(i) => i.as_ref(),
            Self::Chat(c) => c.as_bytes(),
        }
    }

    pub fn decompress(bytes: [u8; 32]) -> Self {
        let len = 32 - bytes.iter().rev().take_while(|&&b| b == 0xff).count();
        let bts = &bytes[..len];
        std::str::from_utf8(bts)
            .ok()
            .and_then(|s| ChatName::from(s).ok())
            .map(Self::Chat)
            .unwrap_or(Self::Profile(bytes))
    }

    pub fn compress(&self) -> [u8; 32] {
        let mut bytes = [0xff; 32];
        let bts = self.as_bytes();
        bytes[..bts.len()].copy_from_slice(bts);
        bytes
    }
}

pub trait ComputeTopic {
    fn topic(&self) -> Topic;
}

impl<T> ComputeTopic for Proof<T> {
    fn topic(&self) -> Topic {
        crypto::hash::new(self.pk).into()
    }
}

impl From<ChatName> for Topic {
    fn from(c: ChatName) -> Self {
        Self::Chat(c)
    }
}

impl From<Identity> for Topic {
    fn from(i: Identity) -> Self {
        Self::Profile(i)
    }
}

pub fn advance_nonce(current: &mut Nonce, new: Nonce) -> bool {
    let valid = new > *current;
    if valid {
        *current = new;
    }
    valid
}

#[repr(packed)]
#[derive(Clone, Copy)]
pub struct RequestHeader {
    pub prefix: u8,
    pub call_id: [u8; 4],
    pub topic: [u8; 32],
    pub len: [u8; 4],
}

impl RequestHeader {
    pub fn as_bytes(&self) -> &[u8; std::mem::size_of::<Self>()] {
        unsafe { std::mem::transmute(self) }
    }

    pub fn from_array(arr: [u8; std::mem::size_of::<Self>()]) -> Self {
        unsafe { std::mem::transmute(arr) }
    }

    pub fn get_len(&self) -> usize {
        u32::from_be_bytes(self.len) as usize
    }
}

#[repr(packed)]
#[derive(Clone, Copy)]
pub struct ResponseHeader {
    pub call_id: [u8; 4],
    pub len: [u8; 4],
}

impl ResponseHeader {
    pub fn as_bytes(&self) -> &[u8; std::mem::size_of::<Self>()] {
        unsafe { std::mem::transmute(self) }
    }

    pub fn from_array(arr: [u8; std::mem::size_of::<Self>()]) -> Self {
        unsafe { std::mem::transmute(arr) }
    }

    pub fn get_len(&self) -> usize {
        u32::from_be_bytes(self.len) as usize
    }
}

#[repr(packed)]
pub struct InternalRequestHeader {
    pub prefix: u8,
    pub topic: [u8; 32],
}

#[derive(Clone, Copy, Codec, Debug)]
pub struct Mail;
