//! Typed client settings and storage-settings transfer values.
//!
//! The application service and session store retain runtime and persistence
//! ownership. This subsystem keeps portable invariants and deterministic
//! conversion independent from those owners.

mod contract;
mod persistence;
mod runtime;

pub use contract::{
    ClientSettings, ClientSettingsError, ClientSettingsRuntimeView, ListenerBindFailureReason,
    ListenerPolicy, ListenerStatus, StorageRootAvailability, StorageRootSnapshot,
    StorageSettingsSnapshot,
};
pub(crate) use persistence::{
    SettingsPersistenceError, create_client_settings, read_client_settings, replace_client_settings,
};

#[cfg(test)]
mod tests;
