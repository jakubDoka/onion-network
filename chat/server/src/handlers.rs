#[macro_export]
macro_rules! ensure {
    ($cond:expr, Ok($resp:expr)) => {
        if !$cond {
            return Ok(Err($resp));
        }
    };

    (let $var:pat = $expr:expr, Ok($resp:expr)) => {
        let $var = $expr else {
            return Ok(Err($resp));
        };
    };

    ($cond:expr, $resp:expr) => {
        if !$cond {
            return Err($resp);
        }
    };

    (let $var:pat = $expr:expr, $resp:expr) => {
        let $var = $expr else {
            return Err($resp);
        };
    };
}

use {
    self::chat::Chat,
    crate::{Context, OnlineLocation},
    chat_spec::{ChatError, ChatName, Identity, ReplVec, Request, Topic, REPLICATION_FACTOR},
    codec::Codec,
    component_utils::arrayvec::ArrayVec,
    dht::U256,
    libp2p::{
        futures::{stream::FuturesUnordered, FutureExt, StreamExt},
        PeerId, Swarm,
    },
    rpc::CallId,
    std::{future::Future, pin::Pin},
    tokio::sync::OwnedRwLockWriteGuard,
};

pub mod chat;
pub mod profile;

type Result<T, E = ChatError> = std::result::Result<T, E>;

pub enum Response {
    Success(Vec<u8>),
    Failure(Vec<u8>),
    DontRespond,
}

pub struct State<'a> {
    pub swarm: &'a mut Swarm<crate::Behaviour>,
    pub location: OnlineLocation,
    pub context: Context,
    pub missing_topic: Option<MissingTopic>,
}

pub trait IntoResponse {
    fn into_response(self) -> Response;
}

impl IntoResponse for Response {
    fn into_response(self) -> Response {
        self
    }
}

impl IntoResponse for Vec<u8> {
    fn into_response(self) -> Response {
        Response::Success(self)
    }
}

impl<'a, O, E> IntoResponse for Result<O, E>
where
    O: Codec<'a>,
    E: Codec<'a>,
{
    fn into_response(self) -> Response {
        match self {
            Ok(_) => Response::Success(self.to_bytes()),
            Err(_) => Response::Failure(self.to_bytes()),
        }
    }
}

impl IntoResponse for () {
    fn into_response(self) -> Response {
        Response::DontRespond
    }
}

pub trait FromRequestOwned: Sized {
    fn from_request(req: Request, state: &mut State) -> Option<Self>;
}

macro_rules! impl_tuples_from_request {
    ($($ty:ident),*) => {
        impl<$($ty,)*> FromRequestOwned for ($($ty,)*)
        where
            $($ty: FromRequestOwned,)*
        {
            fn from_request(req: Request, state: &mut State) -> Option<Self> {
                Some(($($ty::from_request(req, state)?,)*))
            }
        }
    };
}

impl_tuples_from_request!(A);
impl_tuples_from_request!(A, B);
impl_tuples_from_request!(A, B, C);
impl_tuples_from_request!(A, B, C, D);
impl_tuples_from_request!(A, B, C, D, E);

impl FromRequestOwned for Context {
    fn from_request(_: Request, state: &mut State) -> Option<Self> {
        Some(state.context)
    }
}

pub type ReplGroup = ArrayVec<PeerId, { REPLICATION_FACTOR.get() }>;
impl FromRequestOwned for ReplGroup {
    fn from_request(req: Request, state: &mut State) -> Option<Self> {
        let us = *state.swarm.local_peer_id();
        Some(
            state
                .swarm
                .behaviour_mut()
                .dht
                .table
                .closest(req.topic?.as_bytes())
                .take(REPLICATION_FACTOR.get() + 1)
                .map(dht::Route::peer_id)
                .filter(|&p| p != us)
                .collect(),
        )
    }
}

pub type FullReplGroup = ArrayVec<U256, { REPLICATION_FACTOR.get() + 1 }>;
impl FromRequestOwned for FullReplGroup {
    fn from_request(req: Request, state: &mut State) -> Option<Self> {
        Some(
            state
                .swarm
                .behaviour_mut()
                .dht
                .table
                .closest(req.topic?.as_bytes())
                .take(REPLICATION_FACTOR.get() + 1)
                .map(|r| r.id)
                .collect(),
        )
    }
}

impl FromRequestOwned for Topic {
    fn from_request(req: Request, _: &mut State) -> Option<Self> {
        req.topic
    }
}

impl FromRequestOwned for Identity {
    fn from_request(req: Request, _: &mut State) -> Option<Self> {
        match req.topic? {
            Topic::Profile(i) => Some(i),
            _ => None,
        }
    }
}

impl FromRequestOwned for ChatName {
    fn from_request(req: Request, _: &mut State) -> Option<Self> {
        match req.topic? {
            Topic::Chat(c) => Some(c),
            _ => None,
        }
    }
}

impl<T: FromRequestOwned> FromRequestOwned for Option<T> {
    fn from_request(req: Request, state: &mut State) -> Option<Self> {
        Some(T::from_request(req, state))
    }
}

pub enum MissingTopic {
    Chat { name: ChatName, lock: OwnedRwLockWriteGuard<Chat> },
    Profile(Identity),
}

impl FromRequestOwned for MissingTopic {
    fn from_request(_: Request, s: &mut State) -> Option<Self> {
        s.missing_topic.take()
    }
}

pub type Prefix = u8;

impl FromRequestOwned for Prefix {
    fn from_request(req: Request, _: &mut State) -> Option<Self> {
        Some(req.prefix)
    }
}

impl FromRequestOwned for OnlineLocation {
    fn from_request(_: Request, state: &mut State) -> Option<Self> {
        Some(state.location)
    }
}

pub type Origin = PeerId;

impl FromRequestOwned for Origin {
    fn from_request(_: Request, state: &mut State) -> Option<Self> {
        match state.location {
            OnlineLocation::Local(_) => None,
            OnlineLocation::Remote(p) => Some(p),
        }
    }
}

impl FromRequestOwned for () {
    fn from_request(_: Request, _: &mut State) -> Option<Self> {
        Some(())
    }
}

pub trait Handler<'a, T: FromRequestOwned + Send + 'static, R: Codec<'a>>:
    Clone + Send + Sized + 'static
{
    type Future: Future<Output = Response> + Send;

    fn call_computed(self, args: T, res: R) -> Self::Future;

    fn call(
        self,
        req: Request,
        mut state: State,
    ) -> impl Future<Output = Response> + Send + 'static {
        let args = T::from_request(req, &mut state);
        let body = req.body.0.to_vec();
        async move {
            let Some(args) = args else {
                log::warn!(
                    "invalid request from {:?} for handler {:?}",
                    state.location,
                    std::any::type_name::<Self>()
                );
                return Response::DontRespond;
            };

            // SAFETY: the lifetime of `body` is bound to the scope of the future, `r` does not
            // escape its scope. Extending lifetime here is only needed due to limitations of
            // compiler.
            let Some(r) = R::decode(unsafe { std::mem::transmute(&mut body.as_slice()) }) else {
                log::warn!(
                    "invalid encoding from {:?} for handler {:?}",
                    state.location,
                    std::any::type_name::<Self>()
                );
                return Response::DontRespond;
            };
            self.call_computed(args, r).await
        }
    }

    fn repl(self) -> Repl<Self> {
        Repl { handler: self }
    }

    fn restore(self) -> Restore<Self> {
        Restore { handler: self }
    }

    fn no_resp(self) -> NoResp<Self> {
        NoResp { handler: self }
    }
}

#[derive(Clone)]
pub struct Repl<H> {
    pub handler: H,
}

pub type ReplArgs<A> = (Topic, Context, Prefix, A);

impl<'a, H, A, R> Handler<'a, ReplArgs<A>, R> for Repl<H>
where
    H: Handler<'a, A, R>,
    A: FromRequestOwned + Send + 'static,
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
            let mut counter = ReplVec::<(crypto::Hash, u8)>::new();

            counter.push((crypto::hash::new(&bytes), 1));

            while let Some((peer, Ok(resp))) = repl.next().await {
                let hash = crypto::hash::new(&resp);

                let Some((_, count)) = counter.iter_mut().find(|(h, _)| h == &hash) else {
                    log::warn!(
                        "unexpected response from {:?} {:?} {:?} {} {}",
                        std::any::type_name::<<H::Future as Future>::Output>(),
                        Result::<(), ChatError>::decode(&mut resp.as_slice()),
                        resp,
                        peer,
                        cx.local_peer_id,
                    );
                    counter.push((hash, 1));
                    continue;
                };

                *count += 1;
                if *count as usize > REPLICATION_FACTOR.get() / 2 {
                    while repl.next().await.is_some() {}
                    return Response::Success(resp);
                }
            }

            Err::<(), _>(ChatError::NoMajority).into_response()
        }
    }
}

#[derive(Clone)]
pub struct Restore<H> {
    pub handler: H,
}

pub type RestoreArgsArgs<A> = (Context, Option<MissingTopic>, A);

impl<'a, H, A, R> Handler<'a, RestoreArgsArgs<A>, R> for Restore<H>
where
    H: Handler<'a, A, R>,
    A: FromRequestOwned + Send + 'static,
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

#[derive(Clone)]
pub struct NoResp<H> {
    handler: H,
}

impl<'a, H, T, R> Handler<'a, T, R> for NoResp<H>
where
    H: Handler<'a, T, R>,
    T: FromRequestOwned + Send + 'static,
    R: Codec<'a>,
{
    type Future = impl Future<Output = Response> + Send;

    fn call_computed(self, args: T, req: R) -> Self::Future {
        self.handler.call_computed(args, req).map(|e| {
            if let Response::Failure(e) = e {
                log::warn!(
                    "no response from {:?} {:?}",
                    std::any::type_name::<Self>(),
                    <Result<()>>::decode(&mut e.as_slice())
                );
            }
            Response::DontRespond
        })
    }
}

macro_rules! impl_handler {
    (
        [$($ty:ident),*], $last:ident
    ) => {
        #[allow(non_snake_case, unused_mut)]
        impl<'a, F, Fut, $($ty,)* $last: Codec<'a>> Handler<'a, ($($ty,)*), $last> for F
        where
            F: FnOnce($($ty,)* $last,) -> Fut + Clone + Send + 'static,
            Fut: Future + Send,
            Fut::Output: IntoResponse,
            $( $ty: FromRequestOwned + Send + 'static, )*
            $last: Codec<'a> + Send,
        {
            type Future = impl Future<Output = Response> + Send;

            fn call_computed(self, args: ($($ty,)*), req: $last) -> Self::Future {
                let ($($ty,)*) = args;
                self($($ty,)* req).map(|r| r.into_response())
            }
        }
    };
}

impl_handler!([], B);
impl_handler!([A], B);
impl_handler!([A, B], C);
impl_handler!([A, B, C], D);
impl_handler!([A, B, C, D], E);
impl_handler!([A, B, C, D, E], G);

pub type FinalResponse = (Response, OnlineLocation, CallId);
pub type RouteRet = Pin<Box<dyn Future<Output = FinalResponse> + Send>>;
pub type Route = Box<dyn Fn(Request, State) -> RouteRet + Send>;

#[derive(Default)]
pub struct Router {
    routes: Vec<Option<Route>>,
    requests: FuturesUnordered<RouteRet>,
}

impl Router {
    pub fn register<'a, H, A, R>(&mut self, expected: u8, route: H)
    where
        H: Handler<'a, A, R>,
        A: FromRequestOwned + Send + 'static,
        R: Codec<'a>,
    {
        if self.routes.len() <= expected as usize {
            self.routes.resize_with(expected as usize + 1, || None);
        }
        self.routes[expected as usize] = Some(Box::new(move |req, state| {
            let local_self = route.clone();
            let origin = state.location;
            Box::pin(local_self.call(req, state).map(move |r| (r, origin, req.id)))
        }));
    }

    pub fn handle(&mut self, request: Request, state: State) {
        let Some(Some(route)) = self.routes.get(request.prefix as usize) else {
            return;
        };

        self.requests.push(route(request, state));
    }

    pub fn poll(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<FinalResponse> {
        match self.requests.poll_next_unpin(cx) {
            std::task::Poll::Ready(Some(res)) => std::task::Poll::Ready(res),
            _ => std::task::Poll::Pending,
        }
    }
}

macro_rules! __routes {
    ($($module:ident::{$($id:expr => $endpoint:expr;)*};)*) => {{
        let mut router = Router::default();
        $( {
            use handlers::$module::*;
            $(
                router.register($id, $endpoint);
            )*
        } )*
        router
    }};
}

pub(crate) use __routes as router;
