use std::error::Error;
use std::fmt;

use rstorrent_engine::{DEFAULT_CONNECTION_LIMIT, DEFAULT_UNCHOKE_SLOTS};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const MIN_FIXED_LISTENER_PORT: u16 = 1_024;
pub const MIN_PEER_CONNECTION_LIMIT: u32 = 1;
pub const MAX_PEER_CONNECTION_LIMIT: u32 = 2_000;
pub const MAX_UPLOAD_SLOTS: u16 = 50;
pub(crate) const MAX_LISTENER_BIND_DETAIL_BYTES: usize = 512;
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
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(deny_unknown_fields)]
pub struct ClientSettings {
    pub listener: ListenerPolicy,
    #[schemars(range(min = 1, max = 2_000))]
    pub peer_connection_limit: u32,
    #[schemars(range(min = 0, max = 50))]
    pub upload_slots: u16,
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            listener: ListenerPolicy::Disabled,
            peer_connection_limit: u32::try_from(DEFAULT_CONNECTION_LIMIT)
                .expect("engine connection default fits the settings contract"),
            upload_slots: u16::try_from(DEFAULT_UNCHOKE_SLOTS)
                .expect("engine upload-slot default fits the settings contract"),
        }
    }
}

impl ClientSettings {
    pub fn validate(&self) -> Result<(), ClientSettingsError> {
        if let ListenerPolicy::FixedLoopback { port } = self.listener
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
    FixedListenerPort { port: u16 },
    PeerConnectionLimit { value: u32 },
    UploadSlots { value: u16 },
}

impl fmt::Display for ClientSettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(deny_unknown_fields)]
pub struct ClientSettingsRuntimeView {
    pub configured: ClientSettings,
    pub active: ClientSettings,
    pub restart_required: bool,
    pub effective_peer_connection_limit: u32,
    pub listener_status: ListenerStatus,
}

impl Default for ClientSettingsRuntimeView {
    fn default() -> Self {
        let settings = ClientSettings::default();
        Self {
            effective_peer_connection_limit: settings.peer_connection_limit,
            configured: settings.clone(),
            active: settings,
            restart_required: false,
            listener_status: ListenerStatus::Disabled,
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
