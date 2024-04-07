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
    codec::{Codec, Reminder},
    crypto::proof::{Nonce, Proof},
    std::num::NonZeroUsize,
};

pub const REPLICATION_FACTOR: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(4) };

pub type BlockNumber = u64;
pub type Identity = crypto::Hash;
pub type ReplVec<T> = ArrayVec<T, { REPLICATION_FACTOR.get() }>;

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

pub use {chat::*, profile::*, rpc::CallId};

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
    #[error("invalid block: {0}")]
    InvalidBlock(InvalidBlockReason),
    #[error("no majority to confirm the request")]
    NoMajority,
    #[error("already voted")]
    AlreadyVoted,
    #[error("vote not found")]
    VoteNotFound,
    #[error("no permission")]
    NoPermission,
    #[error("rate limited for next {0}ms")]
    RateLimited(u64),
    #[error("connection dropped mid request")]
    ChannelClosed,
    #[error("invalid response")]
    InvalidResponse,
    #[error("timeout")]
    Timeout,
    #[error("context dropped")]
    ContextDropped,
    #[error("opposite party needs to send the message first")]
    OppositePartyFirst,
}

impl ChatError {
    pub fn recover(self) -> Result<(), Self> {
        match self {
            Self::SentDirectly => Ok(()),
            e => Err(e),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Codec, thiserror::Error)]
pub enum InvalidBlockReason {
    #[error("does not match majority")]
    ExtraMessages,
    #[error("is uotdated for us")]
    Outdated,
    #[error("not expected at this point")]
    NotExpected,
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

#[derive(Codec, Clone, Copy)]
pub struct Request<'a> {
    pub prefix: u8,
    pub id: CallId,
    pub topic: Option<Topic>,
    pub body: Reminder<'a>,
}

#[derive(Clone, Copy, Codec, Debug)]
pub struct Mail;
