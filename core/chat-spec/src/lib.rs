#![feature(iter_next_chunk)]
#![feature(slice_take)]
#![feature(iter_advance_by)]
#![feature(macro_metavar_expr)]
#![feature(associated_type_defaults)]
#![feature(impl_trait_in_assoc_type)]
#![feature(extract_if)]
#![feature(slice_from_ptr_range)]

use {
    component_utils::{arrayvec::ArrayVec, crypto::ToProofContext, Codec, Reminder},
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
        SEND_MESSAGE;
        BLOCK_PROPOSAL;
        SEND_BLOCK;
        FETCH_CHAT_DATA;
        FETCH_MESSAGES;
        SUBSCRIBE;
        GIVE_UP_BLOCK;

        FETCH_PROFILE;
        FETCH_VAULT;
        FETCH_PROFILE_FULL;
        CREATE_PROFILE;
        SEND_MAIL;
        READ_MAIL;
        SET_VAULT;
    }
}

pub use {
    chat::*,
    component_utils::crypto::{Nonce, Proof},
    profile::*,
    rpc::CallId,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Codec, thiserror::Error)]
pub enum ChatError {
    #[error("account not found")]
    NotFound,
    #[error("invalid proof")]
    InvalidProof,
    #[error("account already exists")]
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
    #[error("user already exists")]
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
pub enum PossibleTopic {
    Profile(Identity),
    Chat(ChatName),
}

impl PossibleTopic {
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Profile(i) => i.as_ref(),
            Self::Chat(c) => c.as_bytes(),
        }
    }
}

impl From<ChatName> for PossibleTopic {
    fn from(c: ChatName) -> Self {
        Self::Chat(c)
    }
}

impl From<Identity> for PossibleTopic {
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
    pub topic: Option<PossibleTopic>,
    pub body: Reminder<'a>,
}

#[derive(Clone, Copy, Codec, Debug)]
pub struct Mail;

impl ToProofContext for Mail {
    fn to_proof_context(self) -> crypto::Hash {
        [0xff - 1; CHAT_NAME_CAP]
    }
}
