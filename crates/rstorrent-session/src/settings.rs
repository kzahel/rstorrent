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
    ListenerPolicy, ListenerStatus, PortMappingFailureStage, PortMappingMechanism,
    PortMappingPolicy, PortMappingStatus, SessionUdpStatus, StorageRootAvailability,
    StorageRootSnapshot, StorageSettingsSnapshot,
};
pub(crate) use persistence::{
    SettingsPersistenceError, create_client_settings, migrate_client_settings_to_v10,
    migrate_client_settings_to_v11, read_client_settings, replace_client_settings,
};
pub(crate) use runtime::classify_listener_bind_failure;

#[cfg(test)]
mod tests;
