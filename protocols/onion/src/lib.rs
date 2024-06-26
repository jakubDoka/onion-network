#![feature(iter_map_windows)]
#![feature(slice_take)]
#![feature(if_let_guard)]
#![feature(type_alias_impl_trait)]
#![feature(array_windows)]
#![feature(let_chains)]
#![feature(array_chunks)]
#![feature(impl_trait_in_assoc_type)]
#![feature(unwrap_infallible)]
#![feature(extract_if)]
#![feature(never_type)]

mod behaviour;
pub mod key_share;
mod packet;

#[cfg(test)]
mod tests;

pub use {
    behaviour::*,
    packet::{Keypair, PublicKey, SharedSecret},
};
