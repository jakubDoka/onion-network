#![feature(impl_trait_in_assoc_type)]
#![feature(specialization)]
#![feature(macro_metavar_expr)]
#![allow(incomplete_features)]

#[macro_export]
macro_rules! blocking {
    ($expr:expr) => {
        tokio::task::spawn_blocking(move || $expr).await.unwrap()
    };
}

pub async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    tokio::task::spawn_blocking(f).await.unwrap()
}

pub async fn async_blocking<F: Future + Send + 'static>(f: F) -> F::Output
where
    F::Output: Send,
{
    tokio::task::spawn_blocking(move || futures::executor::block_on(f)).await.unwrap()
}

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
    ($($module:path => {$($id:expr => $endpoint:expr;)*};)*) => {{
        let mut router = $crate::Router::default();
        $( {
            use $module::*;
            $(
                router.register($id, $endpoint);
            )*
        } )*
        router
    }};
}

#[macro_export]
macro_rules! rpcs {
    ($($name:ident;)*) => { $( pub const $name: u8 = ${index(0)}; )* };
}

#[macro_export]
macro_rules! quick_impl_from_request {
    ($ctx:ty => [$($ty:ty => |$state:ident| $expr:expr,)*]) => {$(
        impl $crate::FromRequestOwned<$ctx> for $ty {
            fn from_request($state: &mut $ctx) -> Option<Self> {
                Some($expr)
            }
        }
    )*};
}

use {
    codec::Codec,
    futures::{stream::FuturesUnordered, FutureExt, Stream, StreamExt},
    std::{future::Future, pin::Pin, usize},
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

pub trait FromRequestOwned<C>: Sized + Send + 'static {
    fn from_request(context: &mut C) -> Option<Self>;
}

impl<'a, C, T: FromRequestOwned<C>> FromRequestOwned<C> for Option<T> {
    fn from_request(state: &mut C) -> Option<Self> {
        Some(T::from_request(state))
    }
}

macro_rules! impl_tuples_from_request {
    ($($ty:ident),*) => {
        impl<Z, $($ty,)*> FromRequestOwned<Z> for ($($ty,)*)
        where
            $($ty: for<'a> FromRequestOwned<Z>,)*
        {
            fn from_request(_context: &mut Z) -> Option<Self> {
                Some(($($ty::from_request(_context)?,)*))
            }
        }
    };
}

impl_tuples_from_request!();
impl_tuples_from_request!(A);
impl_tuples_from_request!(A, B);
impl_tuples_from_request!(A, B, C);
impl_tuples_from_request!(A, B, C, D);
impl_tuples_from_request!(A, B, C, D, E);

pub trait Context {
    fn request_body(&self) -> &[u8];
}

pub trait Handler<'a, C: Context, T: FromRequestOwned<C> + Send + 'static, R: Codec<'a>>:
    Clone + Send + Sized + 'static
{
    type Future: Future<Output = Response> + Send;

    fn call_computed(self, args: T, res: R) -> Self::Future;

    fn call(self, context: &mut C) -> impl Future<Output = Response> + Send + 'static {
        let args = T::from_request(context);
        let body = context.request_body().to_vec();
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
        impl<'a, Z: Context, F, Fut, $($ty,)* $last: Codec<'a>> Handler<'a, Z, ($($ty,)*), $last> for F
        where
            F: FnOnce($($ty,)* $last,) -> Fut + Clone + Send + 'static,
            Fut: Future + Send,
            Fut::Output: IntoResponse,
            $( $ty: for<'b> FromRequestOwned<Z> + Send + 'static, )*
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

pub type FinalResponse<M> = (Response, M);
pub type RouteRet<M> = Pin<Box<dyn Future<Output = FinalResponse<M>> + Send>>;
pub type Route<C> =
    dyn Fn(<C as RouterContext>::State<'_>) -> RouteRet<<C as RouterContext>::RequestMeta> + Send;

pub trait RouterContext: 'static + Send {
    type State<'a>: Context;
    type RequestMeta: Send + 'static;

    fn prefix(state: &Self::State<'_>) -> usize;
    fn meta(state: &Self::State<'_>) -> Self::RequestMeta;
}

pub struct Router<C>
where
    C: RouterContext,
{
    routes: Vec<Option<Box<Route<C>>>>,
    requests: FuturesUnordered<RouteRet<C::RequestMeta>>,
}

impl<C> Default for Router<C>
where
    C: RouterContext,
{
    fn default() -> Self {
        Self { routes: Vec::new(), requests: FuturesUnordered::new() }
    }
}

impl<C> Router<C>
where
    C: RouterContext,
{
    pub fn register<'a, A, H, R>(&mut self, expected: u8, route: H)
    where
        H: Handler<'a, C::State<'a>, A, R>,
        A: FromRequestOwned<C::State<'a>> + Send + 'static,
        R: Codec<'a>,
    {
        if self.routes.len() <= expected as usize {
            self.routes.resize_with(expected as usize + 1, || None);
        }
        self.routes[expected as usize] = Some(Box::new(move |mut ctx| {
            let local_self = route.clone();
            let meta = C::meta(&ctx);
            Box::pin(
                local_self.call(unsafe { std::mem::transmute(&mut ctx) }).map(move |r| (r, meta)),
            )
        }));
    }

    pub fn handle(&mut self, context: C::State<'_>) {
        let Some(Some(route)) = self.routes.get(C::prefix(&context)) else {
            return;
        };

        self.requests.push(route(context));
    }
}

impl<C> Stream for Router<C>
where
    C: RouterContext,
{
    type Item = FinalResponse<C::RequestMeta>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.requests.poll_next_unpin(cx)
    }
}
