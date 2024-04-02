#![feature(array_chunks)]
#![feature(slice_take)]
#![feature(array_windows)]
#![feature(portable_simd)]
#![allow(incomplete_features)]
#![feature(generic_const_exprs)]
#![feature(slice_flatten)]

mod fec;
mod galois;
mod math;

pub use fec::*;
