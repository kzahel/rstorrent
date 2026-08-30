use serde::{Deserialize, Serialize};

use crate::{RemoteAccessError, Result};

pub const MAX_AUTHORIZED_CLIENTS: usize = 32;
pub const MAX_TOMBSTONES: usize = 128;
pub const MAX_SECURITY_EVENTS: usize = 1_024;
pub const MAX_FAILED_BUCKETS: usize = 256;
pub const IDLE_LIFETIME_MILLIS: u64 = 7 * 24 * 60 * 60 * 1_000;
pub const ABSOLUTE_LIFETIME_MILLIS: u64 = 30 * 24 * 60 * 60 * 1_000;
pub(crate) const TOUCH_INTERVAL_MILLIS: u64 = 60 * 60 * 1_000;
pub(crate) const SECURITY_RETENTION_MILLIS: u64 = 180 * 24 * 60 * 60 * 1_000;
pub(crate) const FAILED_RETENTION_MILLIS: u64 = 30 * 24 * 60 * 60 * 1_000;
pub(crate) const FAILED_BUCKET_MILLIS: u64 = 60 * 60 * 1_000;
pub(crate) const MAX_LABEL_BYTES: usize = 96;
pub(crate) const MAX_OBSERVATION_BYTES: usize = 160;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(u64);

impl Timestamp {
    pub const fn from_millis(value: u64) -> Self {
        Self(value)
    }

    pub const fn as_millis(self) -> u64 {
        self.0
    }

    pub(crate) const fn saturating_add(self, duration: u64) -> Self {
        Self(self.0.saturating_add(duration))
    }

    pub(crate) const fn saturating_sub(self, duration: u64) -> Self {
        Self(self.0.saturating_sub(duration))
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EventId([u8; 16]);

impl EventId {
    pub const fn new(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizationMetadata {
    label: String,
    client_build: Option<String>,
    route_observation: Option<String>,
    browser_observation: Option<String>,
}

impl AuthorizationMetadata {
    pub fn new(
        label: impl Into<String>,
        client_build: Option<String>,
        route_observation: Option<String>,
        browser_observation: Option<String>,
    ) -> Result<Self> {
        let value = Self {
            label: label.into(),
            client_build,
            route_observation,
            browser_observation,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn client_build(&self) -> Option<&str> {
        self.client_build.as_deref()
    }

    pub fn route_observation(&self) -> Option<&str> {
        self.route_observation.as_deref()
    }

    pub fn browser_observation(&self) -> Option<&str> {
        self.browser_observation.as_deref()
    }

    pub(crate) fn validate(&self) -> Result<()> {
        validate_bounded_text(&self.label, 1, MAX_LABEL_BYTES, "browser label")?;
        for (value, name) in [
            (&self.client_build, "client build"),
            (&self.route_observation, "route observation"),
            (&self.browser_observation, "browser observation"),
        ] {
            if let Some(value) = value {
                validate_bounded_text(value, 1, MAX_OBSERVATION_BYTES, name)?;
            }
        }
        Ok(())
    }

    pub(crate) fn rename(&mut self, label: String) -> Result<()> {
        validate_bounded_text(&label, 1, MAX_LABEL_BYTES, "browser label")?;
        self.label = label;
        Ok(())
    }

    pub(crate) fn into_label(self) -> String {
        self.label
    }
}

pub(crate) fn validate_bounded_text(
    value: &str,
    minimum: usize,
    maximum: usize,
    name: &'static str,
) -> Result<()> {
    let bytes = value.as_bytes();
    if !(minimum..=maximum).contains(&bytes.len())
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(RemoteAccessError::InvalidInput(name));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientState {
    Current,
    Revoked,
    Expired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthenticationMethod {
    Password,
    Resume,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Enabled,
    Disabled,
    PasswordChanged,
    AuthorizationCreated,
    AuthorizationRenamed,
    AuthorizationRevoked,
    AuthorizationExpired,
    FullLoginSucceeded,
    ResumeSucceeded,
    CircuitOpened,
    CircuitClosed,
    RequirePasswordEverywhere,
    RelayCredentialRotated,
    RecoveryReset,
    DirectFileSettingChanged,
    DirectFileStarted,
    DirectFileCompleted,
    DirectFileFailed,
    DirectFileCancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventResult {
    Succeeded,
    Rejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailedAttemptKind {
    Password,
    Resume,
    RateLimited,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizedClientView {
    pub client_id: String,
    pub label: String,
    pub fingerprint: String,
    pub created: Timestamp,
    pub last_full_login: Timestamp,
    pub last_resume: Option<Timestamp>,
    pub last_seen: Timestamp,
    pub idle_expires: Timestamp,
    pub absolute_expires: Timestamp,
    pub state: ClientState,
    pub client_build: Option<String>,
    pub route_observation: Option<String>,
    pub browser_observation: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TombstoneView {
    pub client_id: String,
    pub label: String,
    pub fingerprint: String,
    pub created: Timestamp,
    pub last_seen: Timestamp,
    pub ended: Timestamp,
    pub state: ClientState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityEventView {
    pub event_id: String,
    pub timestamp: Timestamp,
    pub kind: EventKind,
    pub result: EventResult,
    pub client_id: Option<String>,
    pub circuit_id: Option<String>,
    pub authentication_method: Option<AuthenticationMethod>,
    pub route: Option<String>,
    pub client_build: Option<String>,
    pub reason_class: Option<String>,
    pub direct_file: Option<DirectFileAuditView>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectFileAuditView {
    pub torrent_id: String,
    pub file_index: u32,
    pub byte_count: u64,
    pub candidate_class: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailedAttemptBucketView {
    pub bucket_start: Timestamp,
    pub kind: FailedAttemptKind,
    pub route_class: String,
    pub attempts: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecuritySnapshot {
    pub generation: u64,
    pub authorization_generation: u64,
    pub clients: Vec<AuthorizedClientView>,
    pub tombstones: Vec<TombstoneView>,
    pub events: Vec<SecurityEventView>,
    pub failed_attempts: Vec<FailedAttemptBucketView>,
}

pub(crate) fn encode_id(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub(crate) fn decode_fixed<const N: usize>(value: &str) -> Result<[u8; N]> {
    use base64::Engine as _;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| RemoteAccessError::Corrupt("identifier encoding"))?;
    decoded
        .try_into()
        .map_err(|_| RemoteAccessError::Corrupt("identifier length"))
}
