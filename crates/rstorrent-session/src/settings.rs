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
    DEFAULT_ACTIVE_DOWNLOADS, EffectiveListenerSettings, EncryptionPolicy,
    HttpsServerAuthenticationPolicy, Ipv6PinholeFailureStage, Ipv6PinholeStatus,
    ListenerBindFailureReason, ListenerPolicy, ListenerStatus, MAX_ACTIVE_DOWNLOADS,
    MIN_ACTIVE_DOWNLOADS, PortMappingFailureStage, PortMappingMechanism, PortMappingPolicy,
    PortMappingStatus, SessionUdpStatus, StorageRootAvailability, StorageRootSnapshot,
    StorageSettingsSnapshot, TransportAddressFamily, TransportFamilyRuntimeView,
};
pub(crate) use convergence::{
    SettingsAttempt, SettingsConvergenceModel, SettingsDomain, SettingsDomainGeneration,
};
pub(crate) use persistence::{
    SettingsPersistenceError, create_client_settings, migrate_client_settings_to_v10,
    migrate_client_settings_to_v11, migrate_client_settings_to_v12, migrate_client_settings_to_v15,
    migrate_client_settings_to_v16, migrate_client_settings_to_v17, read_client_settings,
    replace_client_settings,
};
pub(crate) use runtime::{bounded_utf8, classify_listener_bind_failure};

#[cfg(test)]
mod tests;
