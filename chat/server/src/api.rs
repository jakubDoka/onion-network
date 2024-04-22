use {
    self::chat::Chat,
    crate::{reputation::Rated, Context, OnlineLocation},
    arrayvec::ArrayVec,
    chat_spec::{ChatError, ChatName, Identity, Request, Topic, REPLICATION_FACTOR},
    codec::Codec,
    dht::U256,
    handlers::{FromRequestOwned, IntoResponse, Response},
    libp2p::{futures::StreamExt, PeerId, Swarm},
    opfusk::PeerIdExt,
    rpc::CallId,
    std::future::Future,
    tokio::sync::OwnedRwLockWriteGuard,
};

pub mod chat;
pub mod profile;

type Result<T, E = ChatError> = std::result::Result<T, E>;
pub type ReplGroup = ArrayVec<PeerId, { REPLICATION_FACTOR.get() }>;
pub type FullReplGroup = ArrayVec<U256, { REPLICATION_FACTOR.get() + 1 }>;
pub type Prefix = u8;
pub type Origin = PeerId;

pub struct State<'a> {
    pub req: Request<'a>,
    pub swarm: &'a mut Swarm<crate::Behaviour>,
    pub location: OnlineLocation,
    pub context: Context,
    pub missing_topic: Option<MissingTopic>,
}

impl handlers::Context for State<'_> {
    fn request_body(&self) -> &[u8] {
        self.req.body.0
    }
}

pub enum RouterContext {}

impl handlers::RouterContext for RouterContext {
    type RequestMeta = (OnlineLocation, CallId);
    type State<'a> = State<'a>;

    fn prefix(state: &Self::State<'_>) -> usize {
        state.req.prefix as usize
    }

    fn meta(state: &Self::State<'_>) -> Self::RequestMeta {
        (state.location, state.req.id)
    }
}

pub enum MissingTopic {
    Chat { name: ChatName, lock: OwnedRwLockWriteGuard<Chat> },
    Profile(Identity),
}

handlers::quick_impl_from_request! {State<'_> => [
    Context => |state| state.context,
    FullReplGroup => |state| state
        .swarm
        .behaviour_mut()
        .dht
        .table
        .closest(state.req.topic?.as_bytes())
        .take(REPLICATION_FACTOR.get() + 1)
        .map(|r| r.id)
        .collect(),
    ReplGroup => |state| {
        let us = *state.swarm.local_peer_id();
            state
                .swarm
                .behaviour_mut()
                .dht
                .table
                .closest(state.req.topic?.as_bytes())
                .take(REPLICATION_FACTOR.get() + 1)
                .map(dht::Route::peer_id)
                .filter(|&p| p != us)
                .collect()
    },
    Topic => |state| state.req.topic?,
    Identity => |state| match state.req.topic? {
        Topic::Profile(i) => i,
        _ => return None,
    },
    ChatName => |state| match state.req.topic? {
        Topic::Chat(c) => c,
        _ => return None,
    },
    MissingTopic => |state| state.missing_topic.take()?,
    Prefix => |state| state.req.prefix,
    OnlineLocation => |state| state.location,
    Origin => |state| match state.location {
        OnlineLocation::Local(_) => return None,
        OnlineLocation::Remote(p) => p,
    },
]}

pub trait Handler<'a, C: handlers::Context, T: FromRequestOwned<C>, R: Codec<'a>>:
    handlers::Handler<'a, C, T, R>
{
    fn repl(self) -> Repl<Self> {
        Repl { handler: self }
    }

    fn restore(self) -> Restore<Self> {
        Restore { handler: self }
    }

    fn rated<F>(self, rater: F) -> Rated<Self, F>
    where
        F: Fn(&<Self::Future as Future>::Output) -> i64 + Send + Sync + 'static + Clone,
    {
        Rated::new(self, rater)
    }
}

impl<'a, H, C, T, R> Handler<'a, C, T, R> for H
where
    H: handlers::Handler<'a, C, T, R>,
    C: handlers::Context,
    T: FromRequestOwned<C> + Send + 'static,
    R: Codec<'a>,
{
}

#[derive(Clone)]
pub struct Repl<H> {
    pub handler: H,
}

pub type ReplArgs<A> = (Topic, Context, Prefix, A);

impl<'a, H, A, R> handlers::Handler<'a, State<'a>, ReplArgs<A>, R> for Repl<H>
where
    H: Handler<'a, State<'a>, A, R>,
    A: FromRequestOwned<State<'a>> + Send + 'static,
    R: Codec<'a> + Send + Clone,
    <H::Future as Future>::Output: IntoResponse,
{
    type Future = impl Future<Output = Response> + Send;

    fn call_computed(self, (topic, cx, prefix, args): ReplArgs<A>, req: R) -> Self::Future {
        async move {
            let bytes = match self.handler.call_computed(args, req.clone()).await.into_response() {
                Response::Success(bytes) => bytes,
                resp => return resp,
            };

            let mut repl = cx.repl_rpc(topic, prefix, req).await;
            let mut responses =
                ArrayVec::<(Vec<u8>, PeerId), { REPLICATION_FACTOR.get() + 1 }>::new();

            responses.push((bytes, cx.local_peer_id));

            while let Some((peer, resp)) = repl.next().await {
                responses.push((resp.unwrap_or_default(), peer));
            }

            responses.sort_unstable();

            let Some(winning_response) = responses
                .chunk_by_mut(|(a, _), (b, _)| a == b)
                .find(|group| group.len() > REPLICATION_FACTOR.get() / 2)
                .map(|wr| std::mem::take(&mut wr[0].0))
            else {
                return Err::<(), _>(ChatError::NoMajority).into_response();
            };

            for group in responses.chunk_by(|(a, _), (b, _)| a == b) {
                if group.len() > REPLICATION_FACTOR.get() {
                    continue;
                }

                for (resp, peer) in group {
                    log::warn!(
                        "peer {:?} disagrees with majority. call: {}, resp bytes: {:?}, possible error: {:?}",
                        peer,
                        std::any::type_name::<H>(),
                        resp.as_slice(),
                        Result::<(), ChatError>::decode(&mut resp.as_slice()),
                    );
                    crate::reputation::Rep::get().rate(peer.to_hash(), 10);
                }
            }

            Response::Success(winning_response)
        }
    }
}

#[derive(Clone)]
pub struct Restore<H> {
    pub handler: H,
}

pub type RestoreArgsArgs<A> = (Context, Option<MissingTopic>, A);

impl<'a, H, A, R> handlers::Handler<'a, State<'a>, RestoreArgsArgs<A>, R> for Restore<H>
where
    H: Handler<'a, State<'a>, A, R>,
    A: FromRequestOwned<State<'a>> + Send + 'static,
    R: Codec<'a> + Send + Clone,
    <H::Future as Future>::Output: IntoResponse,
{
    type Future = impl Future<Output = Response> + Send;

    fn call_computed(self, (cx, missing_topic, args): RestoreArgsArgs<A>, req: R) -> Self::Future {
        async move {
            let res = match missing_topic {
                Some(MissingTopic::Chat { name, lock }) => chat::recover(cx, name, lock).await,
                Some(MissingTopic::Profile(identity)) => profile::recover(cx, identity).await,
                _ => Ok(()),
            };

            if res.is_err() {
                return Response::Failure(res.to_bytes());
            }

            self.handler.call_computed(args, req).await.into_response()
        }
    }
}
