//! Typed client settings and storage-settings transfer values.
//!
//! The application service and session store retain runtime and persistence
//! ownership. This subsystem keeps portable invariants and deterministic
//! conversion independent from those owners.

mod contract;
mod runtime;

pub use contract::{
    ClientSettings, ClientSettingsError, ClientSettingsRuntimeView, ListenerBindFailureReason,
    ListenerPolicy, ListenerStatus, StorageRootAvailability, StorageRootSnapshot,
    StorageSettingsSnapshot,
};

#[cfg(test)]
mod tests;
