#![cfg_attr(not(feature = "std"), no_std)]
#![feature(maybe_uninit_uninit_array_transpose)]
#![feature(maybe_uninit_as_bytes)]
#![feature(slice_take)]

#[cfg(feature = "derive")]
pub use codec_derive::Codec;
use core::mem::MaybeUninit;

mod impls;
#[cfg(feature = "std")]
mod std_impls;

#[cfg(feature = "std")]
pub use std_impls::*;

pub mod unsafe_as_raw_bytes {
    pub fn encode<T>(value: &T, buffer: &mut impl super::Buffer) -> Option<()> {
        buffer.extend_from_slice(unsafe {
            core::slice::from_raw_parts(value as *const T as *const u8, core::mem::size_of::<T>())
        })
    }

    pub fn decode<T>(buffer: &mut &[u8]) -> Option<T> {
        let bytes = buffer.take(..core::mem::size_of::<T>())?;
        Some(unsafe { core::ptr::read(bytes.as_ptr() as *const T) })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Reminder<'a>(pub &'a [u8]);

impl Reminder<'_> {
    pub fn to_owned(&self) -> ReminderOwned {
        ReminderOwned(self.0.to_vec())
    }
}

impl<'a> AsRef<[u8]> for Reminder<'a> {
    fn as_ref(&self) -> &[u8] {
        self.0
    }
}

pub trait Buffer {
    #[must_use = "handle the error"]
    fn push(&mut self, byte: u8) -> Option<()>;
    #[must_use = "handle the error"]
    fn extend_from_slice(&mut self, slice: &[u8]) -> Option<()> {
        slice.iter().try_for_each(|&byte| self.push(byte))
    }
}

pub trait Encode {
    #[must_use = "handle the error"]
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()>;

    #[cfg(feature = "std")]
    fn to_bytes(&self) -> Vec<u8> {
        let mut buffer = Vec::new();
        self.encode(&mut buffer).expect("to encode");
        buffer
    }

    fn encoded_len(&self) -> usize {
        struct LenCounter(usize);

        impl Buffer for LenCounter {
            fn extend_from_slice(&mut self, slice: &[u8]) -> Option<()> {
                self.0 += slice.len();
                Some(())
            }

            fn push(&mut self, _: u8) -> Option<()> {
                self.0 += 1;
                Some(())
            }
        }

        let mut counter = LenCounter(0);
        self.encode(&mut counter).expect("to encode");
        counter.0
    }
}

pub trait Decode<'a>: Sized {
    fn decode(buffer: &mut &'a [u8]) -> Option<Self>;
}

pub trait DecodeOwned: for<'a> Decode<'a> {}
impl<T: for<'a> Decode<'a>> DecodeOwned for T {}

pub fn uninit_to_zeroed_slice<T>(uninit: &mut MaybeUninit<T>) -> &mut [u8] {
    let bytes = uninit.as_bytes_mut();
    bytes.iter_mut().for_each(|byte| _ = byte.write(0));
    unsafe { std::mem::transmute(bytes) }
}
