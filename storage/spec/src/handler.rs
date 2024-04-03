#[macro_export]
macro_rules! ensure {
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

#[macro_export]
macro_rules! router {
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

use {
    codec::Codec,
    futures::{stream::FuturesUnordered, FutureExt, StreamExt},
    std::{future::Future, marker::PhantomData, pin::Pin},
};

pub enum Response {
    Success(Vec<u8>),
    Failure(Vec<u8>),
    DontRespond(DontRespondReason),
}

pub enum DontRespondReason {
    NoResponse,
    InvalidRequestContext,
    InvalidRequestCodec,
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
        Response::DontRespond(DontRespondReason::NoResponse)
    }
}

pub trait FromRequestOwned: Sized + Send + 'static {
    type Request: Copy + AsRef<[u8]> + AsRef<u8> + Send;
    type State: Send;

    fn from_request(req: Self::Request, state: &mut Self::State) -> Option<Self>;
}

macro_rules! impl_tuples_from_request {
    ($($ty:ident),*) => {
        impl<R: Copy + AsRef<[u8]> + AsRef<u8> + Send, S: Send, $($ty,)*> FromRequestOwned for ($($ty,)*)
        where
            $($ty: FromRequestOwned<Request = R, State = S>,)*
        {
            type Request = R;
            type State = S;
            fn from_request(req: Self::Request, state: &mut Self::State) -> Option<Self> {
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

pub trait Handler<'a, T: FromRequestOwned + Send + 'static, R: Codec<'a>>:
    Clone + Send + Sized + 'static
{
    type Future: Future<Output = Response> + Send;

    fn call_computed(self, args: T, res: R) -> Self::Future;

    fn call(
        self,
        req: T::Request,
        mut state: T::State,
    ) -> impl Future<Output = Response> + Send + 'static {
        let args = T::from_request(req, &mut state);
        let body = (req.as_ref() as &[u8]).to_vec();
        async move {
            let Some(args) = args else {
                return Response::DontRespond(DontRespondReason::InvalidRequestContext);
            };

            // SAFETY: the lifetime of `body` is bound to the scope of the future, `r` does not
            // escape its scope. Extending lifetime here is only needed due to limitations of
            // compiler.
            let Some(r) = R::decode(unsafe { std::mem::transmute(&mut body.as_slice()) }) else {
                return Response::DontRespond(DontRespondReason::InvalidRequestCodec);
            };
            self.call_computed(args, r).await
        }
    }
}

macro_rules! impl_handler {
    (
        [$($ty:ident),*], $last:ident
    ) => {
        #[allow(non_snake_case, unused_mut)]
        impl<'a, R: Copy + AsRef<[u8]> + AsRef<u8> + Send, S: Send, F, Fut, $($ty,)* $last: Codec<'a>> Handler<'a, ($($ty,)*), $last> for F
        where
            F: FnOnce($($ty,)* $last,) -> Fut + Clone + Send + 'static,
            Fut: Future + Send,
            Fut::Output: IntoResponse,
            $( $ty: FromRequestOwned<Request = R, State = S> + Send + 'static, )*
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

impl_handler!([A], B);
impl_handler!([A, B], C);
impl_handler!([A, B, C], D);
impl_handler!([A, B, C, D], E);
impl_handler!([A, B, C, D, E], G);

pub type FinalResponse<C> = (Response, C);
pub type RouteRet<C> = Pin<Box<dyn Future<Output = FinalResponse<C>> + Send>>;
pub type Route<A, C> = Box<
    dyn Fn(<A as FromRequestOwned>::Request, <A as FromRequestOwned>::State) -> RouteRet<C> + Send,
>;

#[derive(Default)]
pub struct Router<A, C>
where
    A: FromRequestOwned,
{
    routes: Vec<Option<Route<A, C>>>,
    requests: FuturesUnordered<RouteRet<C>>,
    context: PhantomData<fn(C) -> C>,
}

impl<A, C> Router<A, C>
where
    A: FromRequestOwned,
    C: FromRequestOwned<Request = A::Request, State = A::State>,
{
    pub fn register<'a, H, R>(&mut self, expected: u8, route: H)
    where
        H: Handler<'a, A, R>,
        A: FromRequestOwned + Send + 'static,
        R: Codec<'a>,
    {
        if self.routes.len() <= expected as usize {
            self.routes.resize_with(expected as usize + 1, || None);
        }
        self.routes[expected as usize] = Some(Box::new(move |req, mut state| {
            let local_self = route.clone();
            let ctx = C::from_request(req, &mut state).unwrap();
            Box::pin(local_self.call(req, state).map(move |r| (r, ctx)))
        }));
    }

    pub fn handle(&mut self, request: A::Request, state: A::State) {
        let Some(Some(route)) = self.routes.get(*(request.as_ref() as &u8) as usize) else {
            return;
        };

        self.requests.push(route(request, state));
    }

    pub fn poll(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<FinalResponse<C>> {
        match self.requests.poll_next_unpin(cx) {
            std::task::Poll::Ready(Some(res)) => std::task::Poll::Ready(res),
            _ => std::task::Poll::Pending,
        }
    }
}
