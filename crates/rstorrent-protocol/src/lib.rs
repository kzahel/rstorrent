#![forbid(unsafe_code)]

//! Runtime-independent BitTorrent protocol values and state transitions.

pub mod bencode;
pub mod metainfo;
pub mod peer_wire;
pub mod piece;
