#![feature(array_chunks)]
#![feature(macro_metavar_expr)]
#![feature(slice_take)]
#![feature(slice_from_ptr_range)]
#![feature(slice_split_at_unchecked)]

#[macro_export]
macro_rules! build_env {
    ($vis:vis $name:ident) => {
        #[cfg(feature = "building")]
        $vis const $name: &str = env!(stringify!($name));
        #[cfg(not(feature = "building"))]
        $vis const $name: &str = "";
    };
}

#[macro_export]
macro_rules! decl_stream_protocol {
    ($decl_name:ident = $name:literal) => {
        pub const $decl_name: libp2p::StreamProtocol = libp2p::StreamProtocol::new(concat!(
            "/",
            env!("CARGO_PKG_NAME"),
            "/",
            $name,
            "/",
            env!("CARGO_PKG_VERSION")
        ));
    };
}

#[macro_export]
macro_rules! gen_config {
    (
        $($(#[$comment:meta])* $required_field:ident: $required_ty:ty,)*
        ;;
        $($(#[$comment2:meta])* $field:ident: $ty:ty = $default:expr,)*
    ) => {
        pub struct Config {
            $(
                $(#[$comment])*
                pub $required_field: $required_ty,
            )*
            $(
                $field: $ty,
            )*
        }

        impl Config {
            pub fn new($($required_field: $required_ty),*) -> Self {
                Self {
                    $($required_field,)*
                    $($field: $default,)*
                }
            }

            $(
                $(#[$comment2])*
                #[doc = concat!("Defaults to ", stringify!($default))]
                pub fn $field(mut self, $field: $ty) -> Self {
                    self.$field = $field;
                    self
                }
            )*
        }
    };
}

#[macro_export]
macro_rules! gen_unique_id {
    ($vis:vis $ty:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Codec)]
        $vis struct $ty(usize);

        impl $ty {
            $vis fn new() -> Self {
                static COUNTER: std::sync::atomic::AtomicUsize =
                    std::sync::atomic::AtomicUsize::new(0);
                Self(COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
            }

            $vis fn whatever() -> Self {
                Self(usize::MAX)
            }
        }
    };
}

pub mod codec;
pub mod proof;
pub mod proximity;
pub mod stream;

use core::task::Waker;
pub use {arrayvec, codec::*, codec_derive::Codec, futures, stream::*, thiserror};

pub struct DropFn<F: FnOnce()>(Option<F>);

impl<F: FnOnce()> DropFn<F> {
    pub fn new(f: F) -> Self {
        Self(Some(f))
    }
}

impl<F: FnOnce()> Drop for DropFn<F> {
    fn drop(&mut self) {
        self.0.take().expect("we drop only once")();
    }
}

#[must_use]
pub fn arrstr_to_array<const SIZE: usize>(s: arrayvec::ArrayString<SIZE>) -> [u8; SIZE] {
    let mut arr = [0xff; SIZE];
    arr[..s.len()].copy_from_slice(s.as_bytes());
    arr
}

#[must_use]
pub fn array_to_arrstr<const SIZE: usize>(arr: [u8; SIZE]) -> Option<arrayvec::ArrayString<SIZE>> {
    let mut s = arrayvec::ArrayString::<SIZE>::new();
    let len = arr.iter().rposition(|&x| x != 0xff).map_or(0, |x| x + 1);
    s.push_str(core::str::from_utf8(&arr[..len]).ok()?);
    Some(s)
}

pub trait FindAndRemove<T> {
    fn find_and_remove(&mut self, q: impl FnMut(&T) -> bool) -> Option<T>;
    fn find_and_remove_value(&mut self, value: &T) -> Option<T>
    where
        T: PartialEq,
    {
        self.find_and_remove(|x| x == value)
    }
}

impl<T> FindAndRemove<T> for Vec<T> {
    fn find_and_remove(&mut self, q: impl FnMut(&T) -> bool) -> Option<T> {
        Some(self.swap_remove(self.iter().position(q)?))
    }
}

impl<T, const N: usize> FindAndRemove<T> for arrayvec::ArrayVec<T, N> {
    fn find_and_remove(&mut self, q: impl FnMut(&T) -> bool) -> Option<T> {
        Some(self.swap_remove(self.iter().position(q)?))
    }
}

pub fn set_waker(old: &mut Option<Waker>, new: &Waker) {
    if let Some(old) = old {
        old.clone_from(new);
    } else {
        *old = Some(new.clone());
    }
}
