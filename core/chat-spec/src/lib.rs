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
    crypto::{enc, Serialized},
    std::{borrow::Cow, collections::HashMap, convert::Infallible, num::NonZeroUsize},
};

pub const REPLICATION_FACTOR: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(4) };

pub type BlockNumber = u64;
pub type Identity = crypto::Hash;
pub type ReplVec<T> = ArrayVec<T, { REPLICATION_FACTOR.get() }>;

mod chat;
mod profile;

pub use {
    chat::*,
    component_utils::{
        crypto::{Nonce, Proof},
        Protocol,
    },
    profile::*,
    rpc::CallId,
};

component_utils::compose_protocols! {
    fn Subscribe(PossibleTopic) -> Result<(), Infallible>;

    fn CreateChat(ChatName, Identity) -> Result<(), CreateChatError>;
    fn PerformChatAction<'a>(Proof<ChatName>, ChatAction<'a>) -> Result<(), ChatActionError>;
    fn FetchMessages<'a>(ChatName, Cursor) -> Result<(Cursor, Reminder<'a>), FetchMessagesError>;
    fn ProposeMsgBlock(ChatName, BlockNumber, crypto::Hash) -> Result<(), ProposeMsgBlockError>;
    fn SendBlock<'a>(ChatName, BlockNumber, Reminder<'a>) -> Result<(), SendBlockError>;
    fn FetchMinimalChatData<'a>(ChatName) -> Result<(BlockNumber, Cow<'a, HashMap<Identity, Member>>, Reminder<'a>), FetchLatestBlockError>;

    fn CreateProfile<'a>(Proof<&'a [u8]>, Serialized<enc::PublicKey>) -> Result<(), CreateAccountError>;
    fn SetVault<'a>(Proof<Reminder<'a>>) -> Result<(), SetVaultError>;
    fn FetchVault<'a>(Identity) -> Result<(Nonce, Nonce, Reminder<'a>), FetchVaultError>;
    fn ReadMail<'a>(Proof<Mail>) -> Result<Reminder<'a>, ReadMailError>;
    fn SendMail<'a>(Identity, Reminder<'a>) -> Result<(), SendMailError>;
    fn FetchProfile(Identity) -> Result<FetchProfileResp, FetchProfileError>;
    fn FetchFullProfile<'a>(Identity) -> Result<BorrowedProfile<'a>, FetchProfileError>;
}

pub struct Repl<T: Protocol>(T);

impl<T: Protocol> Protocol for Repl<T> {
    type Error = ReplError<T::Error>;
    type Request<'a> = T::Request<'a>;
    type Response<'a> = T::Response<'a>;

    const PREFIX: u8 = T::PREFIX;
}

#[derive(Debug, PartialEq, Eq, thiserror::Error, Codec)]
pub enum ReplError<T> {
    #[error("no majority")]
    NoMajority,
    #[error("invalid response from majority")]
    InvalidResponse,
    #[error("invalid topic")]
    InvalidTopic,
    #[error(transparent)]
    Inner(T),
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

pub trait ToPossibleTopic {
    fn to_possible_topic(&self) -> PossibleTopic;
}

impl<'a> ToPossibleTopic for Proof<&'a [u8]> {
    fn to_possible_topic(&self) -> PossibleTopic {
        PossibleTopic::Profile(crypto::hash::from_raw(&self.pk))
    }
}

impl<'a> ToPossibleTopic for Proof<Reminder<'a>> {
    fn to_possible_topic(&self) -> PossibleTopic {
        PossibleTopic::Profile(crypto::hash::from_raw(&self.pk))
    }
}

impl ToPossibleTopic for Proof<ChatName> {
    fn to_possible_topic(&self) -> PossibleTopic {
        PossibleTopic::Chat(self.context)
    }
}

impl ToPossibleTopic for Proof<Mail> {
    fn to_possible_topic(&self) -> PossibleTopic {
        PossibleTopic::Profile(crypto::hash::from_raw(&self.pk))
    }
}

impl ToPossibleTopic for ChatName {
    fn to_possible_topic(&self) -> PossibleTopic {
        PossibleTopic::Chat(*self)
    }
}

impl ToPossibleTopic for Identity {
    fn to_possible_topic(&self) -> PossibleTopic {
        PossibleTopic::Profile(*self)
    }
}

impl<A, B> ToPossibleTopic for (A, B)
where
    A: ToPossibleTopic,
{
    fn to_possible_topic(&self) -> PossibleTopic {
        self.0.to_possible_topic()
    }
}

impl<A, B, C> ToPossibleTopic for (A, B, C)
where
    A: ToPossibleTopic,
{
    fn to_possible_topic(&self) -> PossibleTopic {
        self.0.to_possible_topic()
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

pub type ProtocolResult<'a, P> = Result<<P as Protocol>::Response<'a>, <P as Protocol>::Error>;

pub trait Topic: for<'a> Codec<'a> + std::hash::Hash + Eq + 'static + Into<PossibleTopic> {
    type Event<'a>: Codec<'a>;
    type Record;
}

#[derive(Codec)]
pub struct Request<'a> {
    pub prefix: u8,
    pub id: CallId,
    pub body: Reminder<'a>,
}

#[derive(Clone, Copy, Codec)]
pub struct Mail;

impl ToProofContext for Mail {
    fn to_proof_context(self) -> crypto::Hash {
        [0xff - 1; CHAT_NAME_CAP]
    }
}
