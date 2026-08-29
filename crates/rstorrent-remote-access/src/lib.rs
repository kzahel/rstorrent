#![forbid(unsafe_code)]
//! Durable, runtime-independent ownership for remote browser authorization.
//!
//! This crate deliberately owns no socket, async task, application DTO, or
//! product log. It turns authenticated protocol results into bounded durable
//! state, while callers own transport and lifecycle composition.

mod authority;
mod error;
mod model;
mod store;

pub use authority::{AuthorizationRequest, PendingResume, ProvisioningMaterial, RemoteAuthority};
pub use error::{RemoteAccessError, Result};
pub use model::{
    ABSOLUTE_LIFETIME_MILLIS, AuthenticationMethod, AuthorizationMetadata, AuthorizedClientView,
    ClientState, EventId, EventKind, EventResult, FailedAttemptKind, IDLE_LIFETIME_MILLIS,
    MAX_AUTHORIZED_CLIENTS, MAX_FAILED_BUCKETS, MAX_SECURITY_EVENTS, MAX_TOMBSTONES,
    SecurityEventView, SecuritySnapshot, Timestamp, TombstoneView,
};
pub use store::{AuthorityStore, CommitCrashPoint, DisableOutcome};
