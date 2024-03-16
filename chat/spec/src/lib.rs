#![feature(iter_next_chunk)]
#![feature(slice_take)]
#![feature(iter_advance_by)]
#![feature(macro_metavar_expr)]
#![feature(associated_type_defaults)]
#![feature(impl_trait_in_assoc_type)]
#![feature(extract_if)]
#![feature(slice_from_ptr_range)]

use {
    codec::{Codec, Reminder},
    component_utils::arrayvec::{ArrayString, ArrayVec},
    rand_core::CryptoRngCore,
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
        FETCH_VAULT;
        FETCH_PROFILE_FULL;
        CREATE_PROFILE;
        SEND_MAIL;
        READ_MAIL;
        SET_VAULT;

        SUBSCRIBE;
        UNSUBSCRIBE;
    }
}

pub use {chat::*, profile::*, rpc::CallId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Codec, thiserror::Error)]
pub enum ChatError {
    #[error("outdated")]
    Outdated,
    #[error("not found")]
    NotFound,
    #[error("invalid proof")]
    InvalidProof,
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

pub type Nonce = u64;

#[derive(Clone, Copy, Debug, Codec)]
pub struct Proof<T> {
    pub pk: crypto::sign::PublicKey,
    pub nonce: Nonce,
    pub signature: crypto::sign::Signature,
    pub context: T,
}

const PAYLOAD_SIZE: usize = core::mem::size_of::<Nonce>() + crypto::hash::SIZE;

impl<T: ProofContext> Proof<T> {
    pub fn new(
        kp: &crypto::sign::Keypair,
        nonce: &mut Nonce,
        context: T,
        rng: impl CryptoRngCore,
    ) -> Self {
        let signature = kp.sign(&Self::pack_payload(*nonce, &context), rng);
        *nonce += 1;
        Self { pk: kp.public_key(), nonce: *nonce - 1, signature, context }
    }

    fn pack_payload(nonce: Nonce, context: &T) -> [u8; PAYLOAD_SIZE] {
        let mut buf = [0; PAYLOAD_SIZE];
        buf[..crypto::hash::SIZE].copy_from_slice(context.as_bytes());
        buf[crypto::hash::SIZE..].copy_from_slice(&nonce.to_be_bytes());
        buf
    }

    pub fn verify(&self) -> bool {
        let bytes = Self::pack_payload(self.nonce, &self.context);
        self.pk.verify(&bytes, &self.signature).is_ok()
    }
}

pub trait ProofContext {
    fn as_bytes(&self) -> &[u8];
}

impl<const SIZE: usize> ProofContext for [u8; SIZE] {
    fn as_bytes(&self) -> &[u8] {
        self
    }
}

impl ProofContext for &[u8] {
    fn as_bytes(&self) -> &[u8] {
        self
    }
}

impl ProofContext for str {
    fn as_bytes(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl<const SIZE: usize> ProofContext for ArrayString<SIZE> {
    fn as_bytes(&self) -> &[u8] {
        self.as_str().as_bytes()
    }
}

impl ProofContext for Reminder<'_> {
    fn as_bytes(&self) -> &[u8] {
        self.0
    }
}

#[derive(Clone, Copy, Codec, Debug)]
pub struct Mail;

impl ProofContext for Mail {
    fn as_bytes(&self) -> &[u8] {
        &[0xff - 1; CHAT_NAME_CAP]
    }
}
