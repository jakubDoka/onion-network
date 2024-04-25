use {
    crate::{reputation::Rated, Context, OnlineLocation},
    arrayvec::ArrayVec,
    chain_api::NodeIdentity,
    chat_spec::{ChatError, ChatName, GroupVec, Identity, ReplVec, Topic, REPLICATION_FACTOR},
    codec::{DecodeOwned, Encode},
    dht::U256,
    handlers::CallId,
    libp2p::PeerId,
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

pub struct State {
    pub location: OnlineLocation,
    pub context: Context,
    pub topic: Topic,
    pub id: CallId,
    pub prefix: Prefix,
}

pub enum RouterContext {}

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
    pub handler: H,
}

pub type ReplArgs<A> = (Topic, Context, Prefix, A);

impl<C, H, S, A, B> handlers::Handler<C, S, ReplArgs<A>, B> for Repl<H>
where
    H: handlers::Handler<C, S, A, B>,
    S: handlers::Stream,
    ReplArgs<A>: handlers::FromContext<C>,
    A: handlers::FromContext<C>,
    B: handlers::FromStream<S> + Encode,
    <H::Future as Future>::Output: DecodeOwned + Send + Sync + Eq,
{
    type Future = impl Future<Output = Result<<H::Future as Future>::Output, ChatError>> + Send;

    fn call(self, (topic, cx, prefix, args): ReplArgs<A>, req: B) -> Self::Future {
        async move {
            let others = cx.repl_rpc::<<H::Future as Future>::Output>(topic, prefix, &req).await?;
            let us = self.handler.call(args, req).await;
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
                let count = resps.len() - prev_len;
                if count > REPLICATION_FACTOR.get() / 2 {
                    pick = Some(base);
                    crate::reputation::Rep::get().rate(peer, -1);
                } else {
                    crate::reputation::Rep::get().rate(peer, 10);
                }
            }

            pick.ok_or(ChatError::NoMajority)
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
