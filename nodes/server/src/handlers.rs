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
    crate::{Context, OnlineLocation},
    chat_spec::{
        rpcs, BorrowedProfile, ChatError, Identity, PossibleTopic, Profile, ReplVec, Request,
        REPLICATION_FACTOR,
    },
    component_utils::{arrayvec::ArrayVec, Codec},
    libp2p::{
        futures::{stream::FuturesUnordered, StreamExt},
        PeerId, Swarm,
    },
    rpc::CallId,
    std::{future::Future, pin::Pin},
};

pub mod chat;
pub mod profile;
//mod retry;

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
    pub missing_topic: bool,
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
    O: component_utils::Codec<'a>,
    E: component_utils::Codec<'a>,
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

pub type ReplGroup = ArrayVec<PeerId, { REPLICATION_FACTOR.get() + 1 }>;

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

impl FromRequestOwned for PossibleTopic {
    fn from_request(req: Request, _: &mut State) -> Option<Self> {
        req.topic
    }
}

impl FromRequestOwned for Identity {
    fn from_request(req: Request, _: &mut State) -> Option<Self> {
        match req.topic? {
            PossibleTopic::Profile(i) => Some(i),
            _ => None,
        }
    }
}

pub struct MissingTopic(pub bool);

impl FromRequestOwned for MissingTopic {
    fn from_request(_: Request, s: &mut State) -> Option<Self> {
        Some(MissingTopic(s.missing_topic))
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

pub trait Handler<'a, T: FromRequestOwned + Send + 'static, R: component_utils::Codec<'a>>:
    Clone + Send + Sized + 'static
{
    type Future: Future + Send;

    fn call_computed(self, args: T, res: R) -> Self::Future;

    fn call(
        self,
        req: Request,
        mut state: State,
    ) -> Option<impl Future<Output = Response> + Send + 'static>
    where
        <Self::Future as Future>::Output: IntoResponse,
    {
        let args = T::from_request(req, &mut state)?;
        let body = req.body.0.to_vec();
        Some(async move {
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
            self.call_computed(args, r).await.into_response()
        })
    }

    fn repl(self) -> Repl<Self> {
        Repl { handler: self }
    }

    fn restore(self) -> Restore<Self> {
        Restore { handler: self }
    }
}

#[derive(Clone)]
pub struct Repl<H> {
    pub handler: H,
}

pub type ReplArgs<A> = (PossibleTopic, Context, Prefix, A);

impl<'a, H, A, R> Handler<'a, ReplArgs<A>, R> for Repl<H>
where
    H: Handler<'a, A, R>,
    A: FromRequestOwned + Send + 'static,
    R: component_utils::Codec<'a> + Send + Clone,
    <H::Future as Future>::Output: IntoResponse,
{
    type Future = impl Future<Output: IntoResponse> + Send;

    fn call_computed(self, (topic, cx, prefix, args): ReplArgs<A>, req: R) -> Self::Future {
        async move {
            let bytes = match self.handler.call_computed(args, req.clone()).await.into_response() {
                Response::Success(bytes) => bytes,
                resp => return resp,
            };

            let mut repl = cx.replicate_rpc(topic, prefix, req).await;
            let mut counter = ReplVec::<(crypto::Hash, u8)>::new();

            counter.push((crypto::hash::from_slice(&bytes), 1));

            while let Some((_, Ok(resp))) = repl.next().await {
                let hash = crypto::hash::from_slice(&resp);

                let Some((_, count)) = counter.iter_mut().find(|(h, _)| h == &hash) else {
                    counter.push((hash, 1));
                    continue;
                };

                *count += 1;
                if *count as usize > REPLICATION_FACTOR.get() / 2 {
                    if hash != crypto::hash::from_slice(&bytes) {
                        todo!("for some reason we have different opinion, this should initiate recovery");
                    }
                    return Response::Success(resp);
                }
            }

            Response::Success(bytes)
        }
    }
}

#[derive(Clone)]
pub struct Restore<H> {
    pub handler: H,
}

pub type RestoreArgsArgs<A> = (PossibleTopic, Context, MissingTopic, A);

impl<'a, H, A, R> Handler<'a, RestoreArgsArgs<A>, R> for Restore<H>
where
    H: Handler<'a, A, R>,
    A: FromRequestOwned + Send + 'static,
    R: component_utils::Codec<'a> + Send + Clone,
    <H::Future as Future>::Output: IntoResponse,
{
    type Future = impl Future<Output: IntoResponse> + Send;

    fn call_computed(
        self,
        (topic, cx, missing_topic, args): RestoreArgsArgs<A>,
        req: R,
    ) -> Self::Future {
        async move {
            if missing_topic.0 {
                let res = match topic {
                    PossibleTopic::Profile(identity) => profile::recover(cx, identity).await,
                    PossibleTopic::Chat(name) => chat::recover(cx, name).await,
                };

                if res.is_err() {
                    return Response::Failure(res.to_bytes());
                }
            }

            self.handler.call_computed(args, req).await.into_response()
        }
    }
}

macro_rules! impl_handler {
    (
        [$($ty:ident),*], $last:ident
    ) => {
        #[allow(non_snake_case, unused_mut)]
        impl<'a, F, Fut, $($ty,)* $last: component_utils::Codec<'a>> Handler<'a, ($($ty,)*), $last> for F
        where
            F: FnOnce($($ty,)* $last,) -> Fut + Clone + Send + 'static,
            Fut: Future + Send,
            Fut::Output: IntoResponse,
            $( $ty: FromRequestOwned + Send + 'static, )*
            $last: component_utils::Codec<'a> + Send,
        {
            type Future = Fut;

            fn call_computed(self, args: ($($ty,)*), req: $last) -> Self::Future {
                let ($($ty,)*) = args;
                self($($ty,)* req)
            }
        }
    };
}

impl_handler!([A], B);
impl_handler!([A, B], C);
impl_handler!([A, B, C], D);
impl_handler!([A, B, C, D], E);
impl_handler!([A, B, C, D, E], G);

pub type FinalResponse = (Response, OnlineLocation, CallId);
pub type RouteRet = Pin<Box<dyn Future<Output = FinalResponse>>>;
pub type Route = Box<dyn Fn(Request, State) -> RouteRet>;

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
        R: component_utils::Codec<'a>,
        <H::Future as Future>::Output: IntoResponse,
    {
        if self.routes.len() <= expected as usize {
            self.routes.resize_with(expected as usize + 1, || None);
        }
        self.routes[expected as usize] = Some(Box::new(move |req, state| {
            let local_self = route.clone();
            let origin = state.location;
            let f = local_self.call(req, state);
            Box::pin(async move {
                (
                    match f {
                        Some(f) => f.await.into_response(),
                        None => Response::DontRespond,
                    },
                    origin,
                    req.id,
                )
            })
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
