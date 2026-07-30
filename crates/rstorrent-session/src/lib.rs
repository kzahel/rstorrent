#![forbid(unsafe_code)]

//! Durable application control and torrent-session ownership.

mod application;
mod control;
mod have;
mod store;

pub use application::{
    ApplicationConfig, ApplicationError, ApplicationService, application_error_response,
};
pub use control::{
    CONTROL_VERSION, Command, ErrorCode, ErrorResponse, RequestEnvelope, ResponseEnvelope,
    ResponseOutcome, ServiceSnapshot, StorageState, TorrentSnapshot, TorrentState,
};
pub use have::{HaveError, HaveState};
pub use store::{ConfiguredStorageRoot, ResumeRecord, SessionStore, StoreError};
