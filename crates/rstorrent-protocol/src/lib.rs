#![forbid(unsafe_code)]

//! Runtime-independent BitTorrent protocol values and state transitions.

pub mod bencode;
pub mod dht;
pub mod extension;
pub mod identity;
pub mod magnet;
pub mod metadata;
pub mod metainfo;
pub mod mse;
pub mod peer_id;
pub mod peer_wire;
pub mod piece;
pub mod storage_layout;
pub mod udp_tracker;
pub mod utp;
