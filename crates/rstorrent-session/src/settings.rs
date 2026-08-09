//! Typed client settings and storage-settings transfer values.
//!
//! The application service and session store retain runtime and persistence
//! ownership. This subsystem keeps portable invariants and deterministic
//! conversion independent from those owners.

mod contract;
mod convergence;
mod persistence;
mod runtime;

pub(crate) use contract::MAX_RUNTIME_DETAIL_BYTES;
pub use contract::{
    AdvertisedPeerEndpointScope, AdvertisedPeerEndpointStatus,
    AdvertisedPeerEndpointUnavailableReason, ClientSettings, ClientSettingsApplicationState,
    ClientSettingsDegradedReason, ClientSettingsError, ClientSettingsRuntimeView,
    EffectiveListenerSettings, EncryptionPolicy, HttpsServerAuthenticationPolicy,
    ListenerBindFailureReason, ListenerPolicy, ListenerStatus, PortMappingFailureStage,
    PortMappingMechanism, PortMappingPolicy, PortMappingStatus, SessionUdpStatus,
    StorageRootAvailability, StorageRootSnapshot, StorageSettingsSnapshot,
};
pub(crate) use convergence::{
    SettingsAttempt, SettingsConvergenceModel, SettingsDomain, SettingsDomainGeneration,
};
pub(crate) use persistence::{
    SettingsPersistenceError, create_client_settings, migrate_client_settings_to_v10,
    migrate_client_settings_to_v11, migrate_client_settings_to_v12, migrate_client_settings_to_v15,
    read_client_settings, replace_client_settings,
};
pub(crate) use runtime::{bounded_utf8, classify_listener_bind_failure};

#[cfg(test)]
mod tests;
