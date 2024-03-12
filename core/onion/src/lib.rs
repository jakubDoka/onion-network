#![feature(iter_map_windows)]
#![feature(if_let_guard)]
#![feature(type_alias_impl_trait)]
#![feature(array_windows)]
#![feature(let_chains)]
#![feature(array_chunks)]
#![feature(impl_trait_in_assoc_type)]
#![feature(unwrap_infallible)]
#![feature(extract_if)]

mod behaviour;
mod packet;

#[cfg(test)]
mod tests;

pub use {
    behaviour::*,
    packet::{KeyPair, PublicKey, SharedSecret},
};
