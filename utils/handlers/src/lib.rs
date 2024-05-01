#![feature(impl_trait_in_assoc_type)]
#![feature(slice_flatten)]
#![feature(specialization)]
#![feature(macro_metavar_expr)]
#![allow(incomplete_features)]
#![feature(trait_alias)]

use {
    codec::{Codec, Decode, DecodeOwned, Encode, ReminderOwned},
    futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    std::{io, marker::PhantomData},
};

pub type CallId = [u8; 4];

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
macro_rules! rpcs {
    ($($name:ident;)*) => { $( pub const $name: u8 = ${index(0)}; )* };
}

#[macro_export]
macro_rules! quick_impl_from_request {
    ($ctx:ty => [$($ty:ty => |$state:ident| $expr:expr,)*]) => {$(
        impl $crate::FromContext<$ctx> for $ty {
            fn from_context($state: &mut $ctx) -> Option<Self> {
                Some($expr)
            }
        }
    )*};
}

pub trait FromContext<C>: Sized + Send + 'static {
    fn from_context(context: &mut C) -> Option<Self>;
}

impl<'a, C, T: FromContext<C>> FromContext<C> for Option<T> {
    fn from_context(state: &mut C) -> Option<Self> {
        Some(T::from_context(state))
    }
}

macro_rules! impl_tuples_from_request {
    ($($ty:ident),*) => {
        impl<Z, $($ty,)*> FromContext<Z> for ($($ty,)*)
        where
            $($ty: FromContext<Z>,)*
        {
            fn from_context(_context: &mut Z) -> Option<Self> {
                Some(($($ty::from_context(_context)?,)*))
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

pub trait FromStream<S: Stream>: Sized + Send + Sync {
    fn from_context(
        stream: &mut Option<S>,
        len: usize,
    ) -> impl std::future::Future<Output = io::Result<Self>> + Send + '_;
}

#[derive(Codec)]
pub struct DecFixed<T>(pub T);

impl<T: DecodeOwned + Send + Sync + 'static, S: Stream> FromStream<S> for DecFixed<T> {
    fn from_context(
        stream: &mut Option<S>,
        len: usize,
    ) -> impl std::future::Future<Output = io::Result<Self>> + Send + '_ {
        async move {
            let stream = stream.as_mut().ok_or(io::ErrorKind::InvalidInput)?;
            let mut result = std::mem::MaybeUninit::<T>::uninit();
            let bytes = codec::uninit_to_zeroed_slice(&mut result);
            debug_assert_eq!(bytes.len(), len);
            stream.read_exact(bytes).await?;
            Ok(DecFixed(T::decode(&mut &*bytes).ok_or(io::ErrorKind::InvalidData)?))
        }
    }
}

impl<S: Stream> FromStream<S> for () {
    fn from_context(
        _stream: &mut Option<S>,
        len: usize,
    ) -> impl std::future::Future<Output = io::Result<Self>> + Send + '_ {
        debug_assert_eq!(len, 0);
        async move { Ok(()) }
    }
}

#[derive(Codec)]
pub struct Dec<T>(pub T);

impl<T: DecodeOwned + Send + Sync, S: Stream> FromStream<S> for Dec<T> {
    fn from_context(
        stream: &mut Option<S>,
        len: usize,
    ) -> impl std::future::Future<Output = io::Result<Self>> + Send + '_ {
        async move {
            let ReminderOwned(buffer) = ReminderOwned::from_context(stream, len).await?;
            Ok(Dec(T::decode(&mut &*buffer).ok_or(io::ErrorKind::InvalidData)?))
        }
    }
}

impl<S: Stream> FromStream<S> for ReminderOwned {
    fn from_context(
        stream: &mut Option<S>,
        len: usize,
    ) -> impl std::future::Future<Output = io::Result<Self>> + Send + '_ {
        async move {
            let stream = stream.as_mut().ok_or(io::ErrorKind::InvalidInput)?;
            let mut buffer = vec![0u8; len];
            stream.read_exact(&mut buffer).await?;
            Ok(ReminderOwned(buffer))
        }
    }
}

#[derive(Codec)]
pub struct BorDec<'a, T>(ReminderOwned, PhantomData<(&'a (), T)>);

impl<'a, T: Decode<'a>> BorDec<'a, T> {
    pub fn get(&'a self) -> T {
        T::decode(&mut &*self.0 .0).unwrap()
    }
}

unsafe impl<'a, T> Send for BorDec<'a, T> {}

impl<'a, T: Decode<'a> + Sync, S: Stream> FromStream<S> for BorDec<'a, T> {
    fn from_context(
        stream: &mut Option<S>,
        len: usize,
    ) -> impl std::future::Future<Output = io::Result<Self>> + Send + '_ {
        async move {
            let ReminderOwned(buffer) = ReminderOwned::from_context(stream, len).await?;
            if T::decode(&mut unsafe { std::mem::transmute(buffer.as_slice()) }).is_none() {
                return Err(io::ErrorKind::InvalidData.into());
            }
            Ok(BorDec(ReminderOwned(buffer), PhantomData))
        }
    }
}

pub trait Stream = AsyncRead + AsyncWrite + Unpin + Send + 'static;

pub trait Handler<C, S, A, B>: Sized + Clone + Send + Sync + 'static {
    type Future: std::future::Future + Send;

    fn call(self, args: A, stream: B) -> Self::Future;

    fn handle(
        self,
        id: CallId,
        len: usize,
        context: &mut C,
        stream: S,
    ) -> impl std::future::Future<Output = HandlerRet<S>> + Send + 'static
    where
        <Self::Future as std::future::Future>::Output: Encode + Send,
        S: Stream,
        A: FromContext<C>,
        B: FromStream<S>,
    {
        let args = A::from_context(context);

        async move {
            let args = args.ok_or(io::ErrorKind::InvalidInput)?;
            let mut stream = Some(stream);
            let res = self.call(args, B::from_context(&mut stream, len).await?).await;
            if let Some(ref mut stream) = stream {
                let res = res.to_bytes();
                if !res.is_empty() {
                    let resp = (id, (res.len() as u32).to_be_bytes());
                    let resp: [[u8; 4]; 2] = resp.into();
                    stream.write_all(resp.flatten()).await?;
                    stream.write_all(&res).await?;
                }
            }
            Ok(stream)
        }
    }
}

macro_rules! impl_handler {
    (
        [$($ty:ident),*], $last:ident
    ) => {
        #[allow(non_snake_case, unused_mut)]
        impl<Z, F, S, Fut, $($ty,)* $last> Handler<Z, S, ($($ty,)*), $last> for F
        where
            F: FnOnce($($ty,)* $last) -> Fut + Send + 'static + Clone + Sync,
            Fut: std::future::Future + Send,
        {
            type Future = Fut;

            fn call(self, args: ($($ty,)*), stream: $last) -> Self::Future {
                let ($($ty,)*) = args;
                self($($ty,)* stream)
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

pub type HandlerRet<S> = io::Result<Option<S>>;

#[macro_export]
macro_rules! router {
    ($vis:vis $name:ident($state:ty): $($id:pat => $endpoint:expr;)*) => {
        $vis async fn $name<S>(id: CallId, prefix: u8, len: usize, mut context: $state, stream: S) -> Option<$crate::HandlerRet<S>>
            where
                S: $crate::Stream,
        {
            Some(match prefix {
                $($id => $crate::Handler::handle($endpoint, id, len, &mut context, stream).await,)*
                _ => return None,
            })
        }
    };
}
