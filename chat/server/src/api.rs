use {
    crate::{rate_map, reputation::Rated, Context, OnlineLocation},
    chain_api::NodeIdentity,
    chat_spec::{
        rpcs, ChatError, ChatName, GroupVec, Identity, ReplVec, Topic, REPLICATION_FACTOR,
    },
    codec::{DecodeOwned, Encode},
    dht::U256,
    handlers::CallId,
    libp2p::PeerId,
    onion::PathId,
    opfusk::ToPeerId,
    std::future::Future,
};

pub mod chat;
pub mod profile;

type Result<T, E = ChatError> = std::result::Result<T, E>;
pub type ReplGroup = ReplVec<U256>;
pub type FullReplGroup = GroupVec<U256>;
pub type Prefix = u8;
pub type Origin = PeerId;

pub async fn subscribe(
    cx: Context,
    topic: Topic,
    user: PathId,
    cid: CallId,
    _: (),
) -> Result<(), ChatError> {
    match topic {
        Topic::Chat(name) if cx.chats.contains_key(&name) => {
            cx.chat_subs.entry(name).or_default().insert(user, cid);
            Ok(())
        }
        Topic::Profile(id) => {
            cx.profile_subs.insert(id, (user, cid));
            Ok(())
        }
        _ => Err(ChatError::NotFound),
    }
}

pub async fn unsubscribe(cx: Context, topic: Topic, user: PathId, _: ()) -> Result<(), ChatError> {
    match topic {
        Topic::Chat(name) => {
            let mut subs = cx.chat_subs.get_mut(&name).ok_or(ChatError::NotFound)?;
            subs.remove(&user).ok_or(ChatError::NotFound).map(drop)
        }
        // TODO: perform access control with signature
        Topic::Profile(id) => cx.profile_subs.remove(&id).ok_or(ChatError::NotFound).map(drop),
    }
}

handlers::router! { pub client_router(State):
    rpcs::CREATE_CHAT => chat::create.repl();
    rpcs::ADD_MEMBER => chat::add_member.repl().restore();
    rpcs::KICK_MEMBER => chat::kick_member.repl().restore();
    rpcs::SEND_MESSAGE => chat::send_message.repl().restore();
    rpcs::FETCH_MESSAGES => chat::fetch_messages.restore();
    rpcs::FETCH_MEMBERS => chat::fetch_members.restore();
    rpcs::CREATE_PROFILE => profile::create.repl();
    rpcs::SEND_MAIL => profile::send_mail.repl().restore();
    rpcs::READ_MAIL => profile::read_mail.repl().restore();
    rpcs::INSERT_TO_VAULT => profile::insert_to_vault.repl().restore();
    rpcs::REMOVE_FROM_VAULT => profile::remove_from_vault.repl().restore();
    rpcs::FETCH_PROFILE => profile::fetch_keys.restore();
    rpcs::FETCH_VAULT => profile::fetch_vault.restore();
    rpcs::FETCH_VAULT_KEY => profile::fetch_vault_key.restore();
    rpcs::SUBSCRIBE => subscribe.restore();
    rpcs::UNSUBSCRIBE => unsubscribe.restore();
}

handlers::router! { pub server_router(State):
     rpcs::CREATE_CHAT => chat::create;
     rpcs::ADD_MEMBER => chat::add_member.restore();
     rpcs::KICK_MEMBER => chat::kick_member.restore();
     rpcs::SEND_BLOCK => chat::handle_message_block
         .rated(rate_map! { BlockNotExpected => 10, BlockUnexpectedMessages => 100, Outdated => 5 })
         .restore();
     rpcs::FETCH_CHAT_DATA => chat::fetch_chat_data.rated(rate_map!(20));
     rpcs::SEND_MESSAGE => chat::send_message.restore();
     rpcs::VOTE_BLOCK => chat::vote
         .rated(rate_map! { NoReplicator => 50, NotFound => 5, AlreadyVoted => 30 })
         .restore();
     rpcs::CREATE_PROFILE => profile::create;
     rpcs::SEND_MAIL => profile::send_mail.restore();
     rpcs::READ_MAIL => profile::read_mail.restore();
     rpcs::INSERT_TO_VAULT => profile::insert_to_vault.restore();
     rpcs::REMOVE_FROM_VAULT => profile::remove_from_vault.restore();
     rpcs::FETCH_PROFILE_FULL => profile::fetch_full.rated(rate_map!(20));
}

pub struct State {
    pub location: OnlineLocation,
    pub context: Context,
    pub topic: Topic,
    pub id: CallId,
    pub prefix: Prefix,
}

handlers::quick_impl_from_request! {State => [
    Context => |state| state.context,
    FullReplGroup => |state| state.context.dht
        .read()
        .closest::<{REPLICATION_FACTOR.get() + 1}>(state.topic.as_bytes()),
    ReplGroup => |state| state.context.dht
        .read()
        .closest::<{REPLICATION_FACTOR.get() + 1}>(state.topic.as_bytes())
        .into_iter()
        .filter(|&r| NodeIdentity::from(r) != state.context.local_peer_id)
        .collect(),
    Topic => |state| state.topic,
    Identity => |state| match state.topic {
        Topic::Profile(i) => i,
        _ => return None,
    },
    ChatName => |state| match state.topic {
        Topic::Chat(c) => c,
        _ => return None,
    },
    Prefix => |state| state.prefix,
    OnlineLocation => |state| state.location,
    Origin => |state| match state.location {
        OnlineLocation::Local(_) => return None,
        OnlineLocation::Remote(p) => p.to_peer_id(),
    },
    PathId => |state| match state.location {
        OnlineLocation::Local(p) => p,
        _ => return None,
    },
    CallId => |state| state.id,
]}

pub trait Handler: Sized {
    fn repl(self) -> Repl<Self> {
        Repl { handler: self }
    }

    fn restore(self) -> Restore<Self> {
        Restore { handler: self }
    }

    fn rated<F>(self, rater: F) -> Rated<Self, F> {
        Rated::new(self, rater)
    }
}

impl<H> Handler for H {}

#[derive(Clone)]
pub struct Repl<H> {
    handler: H,
}

pub type ReplArgs<A> = (Topic, Context, Prefix, A);

impl<C, H, S, A, B, O> handlers::Handler<C, S, ReplArgs<A>, B> for Repl<H>
where
    H: handlers::Handler<C, S, A, B>,
    S: handlers::Stream,
    H::Future: Future<Output = Result<O>>,
    ReplArgs<A>: handlers::FromContext<C>,
    A: handlers::FromContext<C>,
    B: handlers::FromStream<S> + Encode,
    <H::Future as Future>::Output: DecodeOwned + Send + Sync + Eq,
{
    type Future = impl Future<Output = Result<O, ChatError>> + Send;

    fn call(self, (topic, cx, prefix, args): ReplArgs<A>, req: B) -> Self::Future {
        async move {
            let req_bytes = req.to_bytes();
            let us = self.handler.call(args, req).await;

            if matches!(us, Err(ChatError::SentDirectly)) {
                return us;
            }

            let others =
                cx.repl_rpc_low::<<H::Future as Future>::Output>(topic, prefix, &req_bytes).await?;
            let us = (cx.local_peer_id, us);

            let mut resps = others.into_iter().chain(std::iter::once(us)).collect::<GroupVec<_>>();

            let mut pick = None;
            while resps.len() > REPLICATION_FACTOR.get() / 2 {
                let prev_len = resps.len();
                let (peer, base) = resps.pop().unwrap();
                let mut other_peers = GroupVec::new();
                resps.retain(|(_, other)| {
                    let matches = &base == other;
                    if matches {
                        other_peers.push(peer);
                    }
                    !matches
                });
                let count = prev_len - resps.len();
                if count > REPLICATION_FACTOR.get() / 2 {
                    pick = Some(base);
                    crate::reputation::Rep::get().rate(peer, -1);
                } else {
                    crate::reputation::Rep::get().rate(peer, 10);
                }
            }

            pick.ok_or(ChatError::NoMajority)?
        }
    }
}

#[derive(Clone)]
pub struct Restore<H> {
    pub handler: H,
}

pub type RestoreArgsArgs<A> = (Context, Topic, A);

impl<C, H, S, A, B, O> handlers::Handler<C, S, RestoreArgsArgs<A>, B> for Restore<H>
where
    H: handlers::Handler<C, S, A, B>,
    H::Future: Future<Output = Result<O>>,
    S: handlers::Stream,
    RestoreArgsArgs<A>: handlers::FromContext<C>,
    A: handlers::FromContext<C>,
    B: handlers::FromStream<S>,
{
    type Future = impl Future<Output = <H::Future as Future>::Output> + Send;

    fn call(self, (cx, topic, args): RestoreArgsArgs<A>, req: B) -> Self::Future {
        async move {
            match topic {
                Topic::Chat(name) if !cx.chats.contains_key(&name) => chat::recover(cx, name).await,
                Topic::Profile(identity) if !cx.profiles.contains_key(&identity) => {
                    profile::recover(cx, identity).await
                }
                _ => Ok(()),
            }?;

            self.handler.call(args, req).await
        }
    }
}
