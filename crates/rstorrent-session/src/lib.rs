#![forbid(unsafe_code)]

//! Durable application control and torrent-session ownership.

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

mod application;
mod control;
mod have;
mod store;
mod views;

pub use application::{
    ApplicationConfig, ApplicationError, ApplicationService, application_error_response,
};
pub use control::{
    CONTROL_VERSION, Command, ErrorCode, ErrorResponse, RequestEnvelope, ResponseEnvelope,
    ResponseOutcome, ServiceSnapshot, StorageState, TorrentSnapshot, TorrentState,
};
pub use have::{HaveError, HaveState};
pub use store::{
    ConfiguredStorageRoot, PreparedFileRecord, ResumeRecord, SessionStore, StorageRootLocation,
    StoreError,
};
pub use views::{
    ActivePiece, DeliveryPolicy, IndexRange, ResetReason, SubscriptionError, SubscriptionSpec,
    SubscriptionStats, TorrentView, VIEW_CONTRACT_VERSION, ViewHub, ViewPatch, ViewProjection,
    ViewSelector, ViewSnapshot, ViewSubscription, ViewUpdate, ViewUpdatePayload,
};
