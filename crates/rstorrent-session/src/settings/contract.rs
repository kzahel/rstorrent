use std::error::Error;
use std::fmt;

use rstorrent_engine::{DEFAULT_CONNECTION_LIMIT, DEFAULT_UNCHOKE_SLOTS};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const MIN_FIXED_LISTENER_PORT: u16 = 1_024;
pub const DEFAULT_PREFERRED_LISTEN_PORT: u16 = 6_881;
pub const MIN_PREFERRED_LISTEN_PORT: u16 = 1_024;
pub const MIN_PEER_CONNECTION_LIMIT: u32 = 1;
pub const MAX_PEER_CONNECTION_LIMIT: u32 = 2_000;
pub const MAX_UPLOAD_SLOTS: u16 = 50;
pub(crate) const MAX_RUNTIME_DETAIL_BYTES: usize = 512;
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ListenerPolicy {
    #[default]
    Disabled,
    AutomaticLoopback,
    FixedLoopback {
        #[schemars(range(min = 1_024))]
        port: u16,
    },
    AutomaticLocalNetwork,
    FixedLocalNetwork {
        #[schemars(range(min = 1_024))]
        port: u16,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum PortMappingPolicy {
    #[default]
    Disabled,
    Upnp,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(deny_unknown_fields)]
pub struct EffectiveListenerSettings {
    pub listener: ListenerPolicy,
    #[schemars(range(min = 1_024))]
    pub preferred_listen_port: u16,
}

impl EffectiveListenerSettings {
    pub(crate) fn from_settings(settings: &ClientSettings) -> Self {
        Self {
            listener: settings.listener,
            preferred_listen_port: settings.preferred_listen_port,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(deny_unknown_fields)]
pub struct ClientSettings {
    pub listener: ListenerPolicy,
    #[schemars(range(min = 1_024))]
    pub preferred_listen_port: u16,
    pub port_mapping: PortMappingPolicy,
    #[schemars(range(min = 1, max = 2_000))]
    pub peer_connection_limit: u32,
    #[schemars(range(min = 0, max = 50))]
    pub upload_slots: u16,
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            listener: ListenerPolicy::Disabled,
            preferred_listen_port: DEFAULT_PREFERRED_LISTEN_PORT,
            port_mapping: PortMappingPolicy::Disabled,
            peer_connection_limit: u32::try_from(DEFAULT_CONNECTION_LIMIT)
                .expect("engine connection default fits the settings contract"),
            upload_slots: u16::try_from(DEFAULT_UNCHOKE_SLOTS)
                .expect("engine upload-slot default fits the settings contract"),
        }
    }
}

impl ClientSettings {
    pub fn validate(&self) -> Result<(), ClientSettingsError> {
        if self.preferred_listen_port < MIN_PREFERRED_LISTEN_PORT {
            return Err(ClientSettingsError::PreferredListenerPort {
                port: self.preferred_listen_port,
            });
        }
        if let ListenerPolicy::FixedLoopback { port } | ListenerPolicy::FixedLocalNetwork { port } =
            self.listener
            && port < MIN_FIXED_LISTENER_PORT
        {
            return Err(ClientSettingsError::FixedListenerPort { port });
        }
        if !(MIN_PEER_CONNECTION_LIMIT..=MAX_PEER_CONNECTION_LIMIT)
            .contains(&self.peer_connection_limit)
        {
            return Err(ClientSettingsError::PeerConnectionLimit {
                value: self.peer_connection_limit,
            });
        }
        if self.upload_slots > MAX_UPLOAD_SLOTS {
            return Err(ClientSettingsError::UploadSlots {
                value: self.upload_slots,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientSettingsError {
    PreferredListenerPort { port: u16 },
    FixedListenerPort { port: u16 },
    PeerConnectionLimit { value: u32 },
    UploadSlots { value: u16 },
}

impl fmt::Display for ClientSettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PreferredListenerPort { .. } => write!(
                formatter,
                "preferred listener port must be {MIN_PREFERRED_LISTEN_PORT}..=65535"
            ),
            Self::FixedListenerPort { .. } => write!(
                formatter,
                "fixed listener port must be {MIN_FIXED_LISTENER_PORT}..=65535"
            ),
            Self::PeerConnectionLimit { .. } => write!(
                formatter,
                "peer connection limit must be {MIN_PEER_CONNECTION_LIMIT}..={MAX_PEER_CONNECTION_LIMIT}"
            ),
            Self::UploadSlots { .. } => {
                write!(formatter, "upload slots must be 0..={MAX_UPLOAD_SLOTS}")
            }
        }
    }
}

impl Error for ClientSettingsError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum ListenerBindFailureReason {
    AddressInUse,
    PermissionDenied,
    AddressUnavailable,
    Other,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ListenerStatus {
    #[default]
    Disabled,
    Listening {
        #[schemars(length(max = 64))]
        address: String,
        port: u16,
    },
    BindFailed {
        reason: ListenerBindFailureReason,
        #[schemars(length(max = 512))]
        detail: String,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionUdpStatus {
    #[default]
    Unavailable,
    Bound {
        #[schemars(length(max = 64))]
        address: String,
        port: u16,
        coordinated_with_tcp: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum PortMappingMechanism {
    UpnpIgdV2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum PortMappingFailureStage {
    Discovery,
    Description,
    ExternalAddress,
    Add,
    Verify,
    Renewal,
    Delete,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PortMappingStatus {
    #[default]
    Disabled,
    Ineligible,
    Discovering,
    Mapping,
    Mapped {
        mechanism: PortMappingMechanism,
        #[schemars(length(max = 64))]
        local_address: String,
        local_port: u16,
        #[schemars(length(max = 64))]
        external_address: String,
        external_port: u16,
        lease_seconds: u32,
    },
    Failed {
        stage: PortMappingFailureStage,
        #[schemars(length(max = 512))]
        detail: String,
    },
    RenewalFailed {
        #[schemars(length(max = 64))]
        external_address: String,
        external_port: u16,
        #[schemars(length(max = 512))]
        detail: String,
    },
    Stopping,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum AdvertisedPeerEndpointScope {
    Loopback,
    LocalNetwork,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum AdvertisedPeerEndpointUnavailableReason {
    ListenerDisabled,
    ListenerBindFailed,
}

/// Runtime truth about the TCP endpoint available for peer advertisement.
///
/// This is deliberately separate from [`PortMappingStatus`]: a live local
/// listener remains usable after mapping failure, while an active mapping is
/// not evidence that an outside peer has actually connected.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AdvertisedPeerEndpointStatus {
    #[default]
    Unavailable,
    OutboundOnly {
        generation: String,
        reason: AdvertisedPeerEndpointUnavailableReason,
    },
    Local {
        generation: String,
        #[schemars(length(max = 64))]
        address: String,
        port: u16,
        scope: AdvertisedPeerEndpointScope,
        incoming_observed: bool,
    },
    Mapped {
        generation: String,
        #[schemars(length(max = 64))]
        local_address: String,
        local_port: u16,
        #[schemars(length(max = 64))]
        external_address: String,
        external_port: u16,
        lease_seconds_remaining: u32,
        incoming_observed: bool,
    },
    RenewalUnhealthy {
        generation: String,
        #[schemars(length(max = 64))]
        local_address: String,
        local_port: u16,
        #[schemars(length(max = 64))]
        external_address: String,
        external_port: u16,
        lease_seconds_remaining: u32,
        #[schemars(length(max = 512))]
        detail: String,
        incoming_observed: bool,
    },
    Stopping {
        generation: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_port: Option<u16>,
        incoming_observed: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum ClientSettingsDegradedReason {
    TransportBindFailed,
    TransportHandoverFailed,
    PortMappingFailed,
    PortMappingCleanupFailed,
    PeerConnectionConvergenceFailed,
    UploadSlotConvergenceFailed,
    RuntimeStopped,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientSettingsApplicationState {
    Applying,
    #[default]
    Applied,
    Degraded {
        reason: ClientSettingsDegradedReason,
        #[schemars(length(max = 512))]
        detail: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(deny_unknown_fields)]
pub struct ClientSettingsRuntimeView {
    pub configured: ClientSettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_listener: Option<EffectiveListenerSettings>,
    pub effective_port_mapping: PortMappingPolicy,
    pub effective_peer_connection_limit: u32,
    pub effective_upload_slots: u16,
    pub transport_application: ClientSettingsApplicationState,
    pub port_mapping_application: ClientSettingsApplicationState,
    pub peer_connections_application: ClientSettingsApplicationState,
    pub upload_slots_application: ClientSettingsApplicationState,
    pub listener_status: ListenerStatus,
    pub session_udp_status: SessionUdpStatus,
    pub port_mapping_status: PortMappingStatus,
    pub advertised_peer_endpoint: AdvertisedPeerEndpointStatus,
}

impl Default for ClientSettingsRuntimeView {
    fn default() -> Self {
        let settings = ClientSettings::default();
        Self {
            effective_listener: Some(EffectiveListenerSettings::from_settings(&settings)),
            effective_port_mapping: settings.port_mapping,
            effective_peer_connection_limit: settings.peer_connection_limit,
            effective_upload_slots: settings.upload_slots,
            configured: settings.clone(),
            transport_application: ClientSettingsApplicationState::Applied,
            port_mapping_application: ClientSettingsApplicationState::Applied,
            peer_connections_application: ClientSettingsApplicationState::Applied,
            upload_slots_application: ClientSettingsApplicationState::Applied,
            listener_status: ListenerStatus::Disabled,
            session_udp_status: SessionUdpStatus::Unavailable,
            port_mapping_status: PortMappingStatus::Disabled,
            advertised_peer_endpoint: AdvertisedPeerEndpointStatus::Unavailable,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct StorageSettingsSnapshot {
    pub roots: Vec<StorageRootSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_root: Option<String>,
    pub show_add_options: bool,
}

impl Default for StorageSettingsSnapshot {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            default_root: None,
            show_add_options: true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct StorageRootSnapshot {
    pub root_id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_path: Option<String>,
    pub availability: StorageRootAvailability,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum StorageRootAvailability {
    Available,
    Unavailable,
}
