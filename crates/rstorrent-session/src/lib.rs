#![forbid(unsafe_code)]

//! Durable application control and torrent-session ownership.

mod control;
mod have;
mod store;

pub use control::{
    CONTROL_VERSION, Command, ErrorCode, ErrorResponse, RequestEnvelope, ResponseEnvelope,
    ResponseOutcome, ServiceSnapshot, StorageState, TorrentSnapshot, TorrentState,
};
pub use have::{HaveError, HaveState};
pub use store::{ConfiguredStorageRoot, SessionStore, StoreError};
