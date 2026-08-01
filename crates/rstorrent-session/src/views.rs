use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tokio::time::Instant;
use ts_rs::TS;

use rstorrent_engine::peer::{PeerFailure, PeerSource, PeerSources};
use rstorrent_engine::{
    PeerConnectionDirection, PeerConnectionLifecycle, PeerConnectionObservation,
    PeerConnectionRole, PeerRequestWindowPhase, PeerTransport,
};

use crate::control::{ServiceSnapshot, StorageState, TorrentSnapshot, TorrentState};
use crate::view_sets::{DEFAULT_VIEW_SET_QUEUE_BYTES, ViewSetInner, ViewSetUpdate};

pub const VIEW_CONTRACT_VERSION: u16 = 2;
pub const MIN_SUBSCRIPTION_QUEUE_BYTES: u32 = 4 * 1024;
pub const MAX_SUBSCRIPTION_QUEUE_BYTES: u32 = 4 * 1024 * 1024;
pub const MAX_SUBSCRIPTION_INTERVAL_MILLIS: u32 = 60_000;
const MAX_DIAGNOSTIC_EVENTS: usize = 512;
const MAX_DIAGNOSTIC_BYTES: usize = 192 * 1024;
const MAX_DIAGNOSTIC_CONTEXT_FIELDS: usize = 8;
const MAX_DIAGNOSTIC_SUMMARY_CHARS: usize = 240;
const MAX_DIAGNOSTIC_KEY_CHARS: usize = 32;
const MAX_DIAGNOSTIC_VALUE_CHARS: usize = 160;

static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ViewSelector {
    TorrentList,
    Torrent { torrent_id: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum ViewProjection {
    Summary,
    PieceActivity,
    Peers,
    Diagnostics,
}

#[derive(
    Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize, JsonSchema, TS,
)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Trace,
    Debug,
    Info,
    Warning,
    Error,
}

#[derive(
    Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize, JsonSchema, TS,
)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCategory {
    Lifecycle,
    Discovery,
    Tracker,
    Peer,
    Metadata,
    Protocol,
    Scheduler,
    Piece,
    Storage,
    Integrity,
    Platform,
    Performance,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticProfile {
    Normal,
    Detailed,
    Trace,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct DiagnosticFilter {
    pub profile: DiagnosticProfile,
    pub minimum_severity: DiagnosticSeverity,
    pub categories: Vec<DiagnosticCategory>,
}

impl Default for DiagnosticFilter {
    fn default() -> Self {
        Self {
            profile: DiagnosticProfile::Normal,
            minimum_severity: DiagnosticSeverity::Info,
            categories: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct DiagnosticField {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct DiagnosticEvent {
    pub sequence: String,
    pub timestamp_millis: String,
    pub severity: DiagnosticSeverity,
    pub category: DiagnosticCategory,
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub torrent_id: Option<String>,
    pub summary: String,
    pub context: Vec<DiagnosticField>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum ProgressDisposition {
    Active,
    Waiting,
    Blocked,
    Inactive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum ProgressPhase {
    Discovery,
    Metadata,
    Storage,
    Transfer,
    Verification,
    Publication,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum ProgressReason {
    NetworkDisabled,
    DiscoveringPeers,
    WaitingForDiscovery,
    NoEnabledDiscoverySource,
    AcquiringMetadata,
    PreparingStorage,
    WaitingForStorage,
    TransferringPieces,
    VerifyingPieces,
    WaitingForPublication,
    Paused,
    Complete,
    NeedsRepair,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum ProgressAction {
    EnableNetwork,
    EnableDiscovery,
    SelectStorage,
    Resume,
    RepairStorage,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ProgressAssessment {
    pub disposition: ProgressDisposition,
    pub phase: ProgressPhase,
    pub reason: ProgressReason,
    pub actions: Vec<ProgressAction>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProgressInputs {
    pub task_active: bool,
    pub network_disabled: bool,
    pub discovery_exhausted: bool,
    pub discovery_active: bool,
    pub discovery_retry_scheduled: bool,
    pub dht_enabled: bool,
}

pub fn assess_progress(snapshot: &TorrentSnapshot, inputs: ProgressInputs) -> ProgressAssessment {
    use ProgressAction::{EnableDiscovery, EnableNetwork, RepairStorage, Resume, SelectStorage};
    use ProgressDisposition::{Active, Blocked, Inactive, Waiting};
    use ProgressPhase::{Discovery, Publication, Storage, Transfer, Verification};
    use ProgressReason::{
        AcquiringMetadata, Complete, DiscoveringPeers, Failed, NeedsRepair, NetworkDisabled,
        NoEnabledDiscoverySource, Paused, PreparingStorage, TransferringPieces, VerifyingPieces,
        WaitingForDiscovery, WaitingForPublication, WaitingForStorage,
    };

    match snapshot.state {
        TorrentState::Paused => ProgressAssessment {
            disposition: Inactive,
            phase: phase_for(snapshot),
            reason: Paused,
            actions: vec![Resume],
        },
        TorrentState::Complete => ProgressAssessment {
            disposition: Inactive,
            phase: Publication,
            reason: Complete,
            actions: Vec::new(),
        },
        TorrentState::NeedsRepair => ProgressAssessment {
            disposition: Blocked,
            phase: Storage,
            reason: NeedsRepair,
            actions: vec![RepairStorage],
        },
        TorrentState::Error => ProgressAssessment {
            disposition: Inactive,
            phase: phase_for(snapshot),
            reason: Failed,
            actions: Vec::new(),
        },
        TorrentState::AwaitingStorage if inputs.task_active => ProgressAssessment {
            disposition: Active,
            phase: Storage,
            reason: PreparingStorage,
            actions: Vec::new(),
        },
        TorrentState::AwaitingStorage => ProgressAssessment {
            disposition: Blocked,
            phase: Storage,
            reason: WaitingForStorage,
            actions: vec![SelectStorage],
        },
        TorrentState::AwaitingPublication => ProgressAssessment {
            disposition: Waiting,
            phase: Publication,
            reason: WaitingForPublication,
            actions: Vec::new(),
        },
        TorrentState::Checking => ProgressAssessment {
            disposition: if inputs.task_active { Active } else { Waiting },
            phase: Verification,
            reason: VerifyingPieces,
            actions: Vec::new(),
        },
        TorrentState::Downloading => ProgressAssessment {
            disposition: if inputs.task_active { Active } else { Waiting },
            phase: Transfer,
            reason: TransferringPieces,
            actions: Vec::new(),
        },
        TorrentState::AwaitingMetadata if inputs.network_disabled => ProgressAssessment {
            disposition: Blocked,
            phase: Discovery,
            reason: NetworkDisabled,
            actions: vec![EnableNetwork],
        },
        TorrentState::AwaitingMetadata if inputs.task_active || inputs.discovery_active => {
            ProgressAssessment {
                disposition: Active,
                phase: Discovery,
                reason: if inputs.task_active {
                    AcquiringMetadata
                } else {
                    DiscoveringPeers
                },
                actions: Vec::new(),
            }
        }
        TorrentState::AwaitingMetadata
            if inputs.discovery_retry_scheduled || inputs.dht_enabled =>
        {
            ProgressAssessment {
                disposition: Waiting,
                phase: Discovery,
                reason: WaitingForDiscovery,
                actions: Vec::new(),
            }
        }
        TorrentState::AwaitingMetadata if inputs.discovery_exhausted => ProgressAssessment {
            disposition: Blocked,
            phase: Discovery,
            reason: NoEnabledDiscoverySource,
            actions: vec![EnableDiscovery],
        },
        TorrentState::AwaitingMetadata => ProgressAssessment {
            disposition: Waiting,
            phase: Discovery,
            reason: WaitingForDiscovery,
            actions: Vec::new(),
        },
    }
}

fn phase_for(snapshot: &TorrentSnapshot) -> ProgressPhase {
    match snapshot.state {
        TorrentState::AwaitingMetadata => ProgressPhase::Discovery,
        TorrentState::AwaitingStorage | TorrentState::NeedsRepair => ProgressPhase::Storage,
        TorrentState::Checking => ProgressPhase::Verification,
        TorrentState::Downloading => ProgressPhase::Transfer,
        TorrentState::AwaitingPublication | TorrentState::Complete => ProgressPhase::Publication,
        TorrentState::Paused | TorrentState::Error if snapshot.metadata_available => {
            ProgressPhase::Transfer
        }
        TorrentState::Paused | TorrentState::Error => ProgressPhase::Discovery,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct DeliveryPolicy {
    pub min_interval_millis: u32,
    pub max_queue_bytes: u32,
}

impl Default for DeliveryPolicy {
    fn default() -> Self {
        Self {
            min_interval_millis: 100,
            max_queue_bytes: 256 * 1024,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct SubscriptionSpec {
    pub selector: ViewSelector,
    pub projection: ViewProjection,
    pub delivery: DeliveryPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<DiagnosticFilter>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct IndexRange {
    pub start: u32,
    pub end_exclusive: u32,
}

impl IndexRange {
    pub fn new(start: u32, end_exclusive: u32) -> Option<Self> {
        (start < end_exclusive).then_some(Self {
            start,
            end_exclusive,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ActivePiece {
    pub piece_index: u32,
    pub piece_length: u32,
    pub requested: Vec<IndexRange>,
    pub received: Vec<IndexRange>,
    pub stored: Vec<IndexRange>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct TorrentView {
    pub torrent_id: String,
    pub state: TorrentState,
    pub storage_state: StorageState,
    pub metadata_available: bool,
    pub piece_count: u32,
    pub verified_piece_count: u32,
    pub requested_bytes: String,
    pub received_bytes: String,
    pub stored_bytes: String,
    pub active_peer_connections: u32,
    pub payload_download_rate_bytes: String,
    pub progress: ProgressAssessment,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStatus {
    Available,
    Unavailable,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum PeerDirection {
    Incoming,
    Outgoing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum PeerTransportKind {
    Tcp,
    Utp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum PeerLifecycle {
    TransportConnecting,
    ProtocolHandshaking,
    Connected,
    Disconnecting,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum PeerRole {
    Metadata,
    Content,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum PeerRequestPhase {
    SlowStart,
    Steady,
    Stalled,
}

#[derive(
    Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize, JsonSchema, TS,
)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum PeerSourceView {
    Tracker,
    PeerExchange,
    Dht,
    LocalDiscovery,
    Incoming,
    Manual,
    MagnetHint,
    Cache,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum PeerDisconnectReason {
    Connect,
    Handshake,
    Protocol,
    RemoteClosed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct PeerFieldCapabilities {
    pub local_endpoint: CapabilityStatus,
    pub client_name: CapabilityStatus,
    pub ut_metadata: CapabilityStatus,
    pub interest_directions: CapabilityStatus,
    pub local_choke: CapabilityStatus,
    pub piece_availability: CapabilityStatus,
    pub protocol_rates: CapabilityStatus,
    pub upload: CapabilityStatus,
    pub metadata_stage: CapabilityStatus,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct PeerView {
    pub connection_id: String,
    pub torrent_id: String,
    pub peer_record_id: Option<String>,
    pub direction: PeerDirection,
    pub transport: PeerTransportKind,
    pub lifecycle: PeerLifecycle,
    pub role: PeerRole,
    pub lifecycle_age_millis: String,
    pub remote_endpoint: String,
    pub local_endpoint: Option<String>,
    pub sources: Vec<PeerSourceView>,
    pub peer_id: Option<String>,
    pub client_name: Option<String>,
    pub supports_extensions: Option<bool>,
    pub supports_ut_metadata: Option<bool>,
    pub local_interested: Option<bool>,
    pub remote_interested: Option<bool>,
    pub remote_choking: Option<bool>,
    pub local_choking: Option<bool>,
    pub available_piece_count: Option<u32>,
    pub wanted_piece_count: Option<u32>,
    pub payload_download_rate_bytes: Option<String>,
    pub payload_downloaded_bytes: Option<String>,
    pub protocol_download_rate_bytes: Option<String>,
    pub protocol_downloaded_bytes: Option<String>,
    pub payload_upload_rate_bytes: Option<String>,
    pub payload_uploaded_bytes: Option<String>,
    pub pending_requests: Option<u32>,
    pub target_requests: Option<u32>,
    pub queued_payload_bytes: Option<String>,
    pub oldest_request_age_millis: Option<String>,
    pub request_timeout_millis: Option<String>,
    pub request_phase: Option<PeerRequestPhase>,
    pub connected_age_millis: Option<String>,
    pub last_useful_age_millis: Option<String>,
    pub last_payload_age_millis: Option<String>,
    pub disconnect_reason: Option<PeerDisconnectReason>,
    pub capabilities: PeerFieldCapabilities,
}

impl PeerView {
    fn from_observation(
        torrent_id: &str,
        captured_at: Duration,
        peer: &PeerConnectionObservation,
    ) -> Self {
        let content = peer.content.as_ref();
        Self {
            connection_id: peer.connection_id.get().to_string(),
            torrent_id: torrent_id.to_owned(),
            peer_record_id: peer.record_id.map(|id| id.get().to_string()),
            direction: match peer.direction {
                PeerConnectionDirection::Incoming => PeerDirection::Incoming,
                PeerConnectionDirection::Outgoing => PeerDirection::Outgoing,
            },
            transport: match peer.transport {
                PeerTransport::Tcp => PeerTransportKind::Tcp,
                PeerTransport::Utp => PeerTransportKind::Utp,
            },
            lifecycle: match peer.lifecycle {
                PeerConnectionLifecycle::TransportConnecting => PeerLifecycle::TransportConnecting,
                PeerConnectionLifecycle::ProtocolHandshaking => PeerLifecycle::ProtocolHandshaking,
                PeerConnectionLifecycle::Connected => PeerLifecycle::Connected,
                PeerConnectionLifecycle::Disconnecting => PeerLifecycle::Disconnecting,
            },
            role: match peer.role {
                PeerConnectionRole::Metadata => PeerRole::Metadata,
                PeerConnectionRole::Content => PeerRole::Content,
            },
            lifecycle_age_millis: duration_millis_string(
                captured_at.saturating_sub(peer.lifecycle_changed_at),
            ),
            remote_endpoint: peer.endpoint.to_string(),
            local_endpoint: None,
            sources: peer_sources(peer.sources),
            peer_id: peer.peer_id.map(hex_peer_id),
            client_name: None,
            supports_extensions: peer.supports_extensions,
            supports_ut_metadata: None,
            local_interested: content.map(|_| true),
            remote_interested: None,
            remote_choking: content.map(|activity| activity.choking),
            local_choking: None,
            available_piece_count: None,
            wanted_piece_count: content.map(|activity| bounded_u32(activity.wanted_piece_count)),
            payload_download_rate_bytes: content
                .map(|activity| activity.observed_payload_rate.to_string()),
            payload_downloaded_bytes: content
                .map(|activity| activity.useful_payload_bytes.to_string()),
            protocol_download_rate_bytes: None,
            protocol_downloaded_bytes: None,
            payload_upload_rate_bytes: None,
            payload_uploaded_bytes: None,
            pending_requests: content.map(|activity| bounded_u32(activity.pending_requests)),
            target_requests: content.map(|activity| bounded_u32(activity.target_requests)),
            queued_payload_bytes: content.map(|activity| activity.queued_payload_bytes.to_string()),
            oldest_request_age_millis: content
                .and_then(|activity| activity.oldest_request_age)
                .map(duration_millis_string),
            request_timeout_millis: content
                .map(|activity| duration_millis_string(activity.request_timeout)),
            request_phase: content.map(|activity| match activity.request_window_phase {
                PeerRequestWindowPhase::SlowStart => PeerRequestPhase::SlowStart,
                PeerRequestWindowPhase::Steady => PeerRequestPhase::Steady,
                PeerRequestWindowPhase::Stalled => PeerRequestPhase::Stalled,
            }),
            connected_age_millis: content
                .map(|activity| duration_millis_string(activity.connected_age)),
            last_useful_age_millis: content
                .and_then(|activity| activity.last_useful_age)
                .map(duration_millis_string),
            last_payload_age_millis: content
                .and_then(|activity| activity.last_payload_age)
                .map(duration_millis_string),
            disconnect_reason: peer.close_reason.map(|reason| match reason {
                PeerFailure::Connect => PeerDisconnectReason::Connect,
                PeerFailure::Handshake => PeerDisconnectReason::Handshake,
                PeerFailure::Protocol => PeerDisconnectReason::Protocol,
                PeerFailure::RemoteClosed => PeerDisconnectReason::RemoteClosed,
            }),
            capabilities: PeerFieldCapabilities {
                local_endpoint: CapabilityStatus::Unsupported,
                client_name: CapabilityStatus::Unsupported,
                ut_metadata: CapabilityStatus::Unavailable,
                interest_directions: CapabilityStatus::Unavailable,
                local_choke: CapabilityStatus::Unsupported,
                piece_availability: CapabilityStatus::Unavailable,
                protocol_rates: CapabilityStatus::Unsupported,
                upload: CapabilityStatus::Unsupported,
                metadata_stage: CapabilityStatus::Unavailable,
            },
        }
    }
}

fn bounded_u32(value: usize) -> u32 {
    value.try_into().unwrap_or(u32::MAX)
}

fn duration_millis_string(value: Duration) -> String {
    value.as_millis().to_string()
}

fn hex_peer_id(peer_id: [u8; 20]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(40);
    for byte in peer_id {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn peer_sources(sources: PeerSources) -> Vec<PeerSourceView> {
    [
        (PeerSource::Tracker, PeerSourceView::Tracker),
        (PeerSource::PeerExchange, PeerSourceView::PeerExchange),
        (PeerSource::Dht, PeerSourceView::Dht),
        (PeerSource::LocalDiscovery, PeerSourceView::LocalDiscovery),
        (PeerSource::Incoming, PeerSourceView::Incoming),
        (PeerSource::Manual, PeerSourceView::Manual),
        (PeerSource::MagnetHint, PeerSourceView::MagnetHint),
        (PeerSource::Cache, PeerSourceView::Cache),
    ]
    .into_iter()
    .filter_map(|(source, view)| sources.contains(source).then_some(view))
    .collect()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ViewSnapshot {
    TorrentList {
        torrents: Vec<TorrentView>,
    },
    Torrent {
        torrent: Option<TorrentView>,
    },
    PieceActivity {
        torrent_id: String,
        piece_count: u32,
        verified: Vec<IndexRange>,
        active: Option<ActivePiece>,
    },
    Peers {
        torrent_id: String,
        peers: Vec<PeerView>,
    },
    Diagnostics {
        events: Vec<DiagnosticEvent>,
        dropped_count: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ViewPatch {
    TorrentList {
        upsert: Vec<TorrentView>,
        removed: Vec<String>,
    },
    Torrent {
        torrent: Option<TorrentView>,
    },
    PieceActivity {
        torrent_id: String,
        piece_count: u32,
        verified: Vec<IndexRange>,
        cleared: Vec<IndexRange>,
        active: Option<ActivePiece>,
    },
    Peers {
        torrent_id: String,
        upsert: Vec<PeerView>,
        removed: Vec<String>,
    },
    Diagnostics {
        events: Vec<DiagnosticEvent>,
        dropped_count: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ViewUpdatePayload {
    Snapshot { snapshot: ViewSnapshot },
    Patch { patch: ViewPatch },
    ResetRequired { reason: ResetReason },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum ResetReason {
    QueueOverflow,
    CursorMismatch,
    CursorExpired,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ViewUpdate {
    pub contract_version: u16,
    pub stream_id: String,
    pub epoch: String,
    pub sequence: String,
    pub base_revision: String,
    pub revision: String,
    #[serde(flatten)]
    pub payload: ViewUpdatePayload,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SubscriptionStats {
    pub queued_bytes: usize,
    pub queue_high_water: usize,
    pub reset_count: u64,
}

#[derive(Clone, Debug)]
pub struct ViewHub {
    pub(crate) inner: Arc<Mutex<HubState>>,
}

#[derive(Debug)]
pub(crate) struct HubState {
    pub(crate) epoch: u64,
    pub(crate) revision: u64,
    torrents: BTreeMap<String, TorrentModel>,
    diagnostics: VecDeque<StoredDiagnostic>,
    diagnostic_bytes: usize,
    diagnostic_dropped: u64,
    next_diagnostic_sequence: u64,
    subscribers: BTreeMap<u64, Weak<SubscriberInner>>,
    next_stream_id: u64,
    pub(crate) view_sets: BTreeMap<String, Arc<ViewSetInner>>,
    pub(crate) view_set_lease: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TorrentModel {
    view: TorrentView,
    snapshot: TorrentSnapshot,
    progress_inputs: ProgressInputs,
    verified: Vec<IndexRange>,
    active: Option<ActivePiece>,
    peers: BTreeMap<String, PeerView>,
}

#[derive(Clone, Debug)]
struct StoredDiagnostic {
    event: DiagnosticEvent,
    encoded_bytes: usize,
}

#[derive(Clone, Debug)]
pub struct ViewSubscription {
    inner: Arc<SubscriberInner>,
    hub: Weak<Mutex<HubState>>,
}

#[derive(Debug)]
struct SubscriberInner {
    stream_id: u64,
    epoch: u64,
    spec: SubscriptionSpec,
    queue: Mutex<QueueState>,
    notify: Notify,
}

#[derive(Debug)]
struct QueueState {
    entries: VecDeque<QueuedUpdate>,
    queued_bytes: usize,
    queue_high_water: usize,
    reset_count: u64,
    next_sequence: u64,
    tail_revision: u64,
    next_delivery: Instant,
    needs_resync: bool,
    closed: bool,
}

#[derive(Debug)]
struct QueuedUpdate {
    update: ViewUpdate,
    encoded_bytes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubscriptionError {
    InvalidInterval { maximum: u32 },
    InvalidQueueBound { minimum: u32, maximum: u32 },
    InvalidProjection,
    SnapshotExceedsQueue { snapshot: usize, maximum: u32 },
    Closed,
    Internal(String),
}

impl fmt::Display for SubscriptionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInterval { maximum } => {
                write!(
                    formatter,
                    "subscription interval exceeds {maximum} milliseconds"
                )
            }
            Self::InvalidQueueBound { minimum, maximum } => write!(
                formatter,
                "subscription queue must be within {minimum}..={maximum} bytes"
            ),
            Self::InvalidProjection => {
                write!(
                    formatter,
                    "the selected view does not support that projection"
                )
            }
            Self::SnapshotExceedsQueue { snapshot, maximum } => write!(
                formatter,
                "initial snapshot is {snapshot} bytes and exceeds the {maximum}-byte queue"
            ),
            Self::Closed => write!(formatter, "subscription is closed"),
            Self::Internal(message) => write!(formatter, "subscription internal error: {message}"),
        }
    }
}

impl Error for SubscriptionError {}

impl From<crate::ViewSetError> for SubscriptionError {
    fn from(error: crate::ViewSetError) -> Self {
        Self::Internal(error.to_string())
    }
}

impl ViewHub {
    pub fn new(snapshot: &ServiceSnapshot) -> Result<Self, SubscriptionError> {
        Self::new_with_view_set_lease(
            snapshot,
            Duration::from_millis(crate::view_sets::VIEW_SET_LEASE_MILLIS),
        )
    }

    pub(crate) fn new_with_view_set_lease(
        snapshot: &ServiceSnapshot,
        view_set_lease: Duration,
    ) -> Result<Self, SubscriptionError> {
        let revision = parse_revision(&snapshot.revision)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(HubState {
                epoch: NEXT_EPOCH.fetch_add(1, Ordering::Relaxed),
                revision,
                torrents: snapshot
                    .torrents
                    .iter()
                    .map(|torrent| {
                        (
                            torrent.torrent_id.clone(),
                            TorrentModel::from_snapshot(torrent),
                        )
                    })
                    .collect(),
                diagnostics: VecDeque::new(),
                diagnostic_bytes: 0,
                diagnostic_dropped: 0,
                next_diagnostic_sequence: 1,
                subscribers: BTreeMap::new(),
                next_stream_id: 1,
                view_sets: BTreeMap::new(),
                view_set_lease,
            })),
        })
    }

    pub(crate) fn view_set_lease(&self) -> Duration {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .view_set_lease
    }

    pub fn subscribe(&self, spec: SubscriptionSpec) -> Result<ViewSubscription, SubscriptionError> {
        validate_spec(&spec)?;
        let mut hub = self
            .inner
            .lock()
            .map_err(|_| SubscriptionError::Internal("view hub lock is poisoned".to_owned()))?;
        let stream_id = hub.next_stream_id;
        hub.next_stream_id = hub
            .next_stream_id
            .checked_add(1)
            .ok_or_else(|| SubscriptionError::Internal("stream ID overflow".to_owned()))?;
        let snapshot = hub.snapshot_for(&spec);
        let inner = Arc::new(SubscriberInner {
            stream_id,
            epoch: hub.epoch,
            spec,
            queue: Mutex::new(QueueState {
                entries: VecDeque::new(),
                queued_bytes: 0,
                queue_high_water: 0,
                reset_count: 0,
                next_sequence: 1,
                tail_revision: hub.revision,
                next_delivery: Instant::now(),
                needs_resync: false,
                closed: false,
            }),
            notify: Notify::new(),
        });
        inner.enqueue_snapshot(hub.revision, snapshot)?;
        hub.subscribers.insert(stream_id, Arc::downgrade(&inner));
        Ok(ViewSubscription {
            inner,
            hub: Arc::downgrade(&self.inner),
        })
    }

    pub(crate) fn replace_durable(
        &self,
        snapshot: &ServiceSnapshot,
        verified: &BTreeMap<String, Vec<IndexRange>>,
    ) -> Result<(), SubscriptionError> {
        let revision = parse_revision(&snapshot.revision)?;
        let mut hub = self
            .inner
            .lock()
            .map_err(|_| SubscriptionError::Internal("view hub lock is poisoned".to_owned()))?;
        let previous = hub.torrents.clone();
        let mut next = BTreeMap::new();
        for torrent in &snapshot.torrents {
            let mut model = TorrentModel::from_snapshot(torrent);
            if let Some(ranges) = verified.get(&torrent.torrent_id) {
                model.verified = ranges.clone();
            } else if let Some(old) = previous.get(&torrent.torrent_id) {
                model.verified = old.verified.clone();
            }
            if let Some(old) = previous.get(&torrent.torrent_id) {
                model.view.requested_bytes = old.view.requested_bytes.clone();
                model.view.received_bytes = old.view.received_bytes.clone();
                model.view.stored_bytes = old.view.stored_bytes.clone();
                model.view.active_peer_connections = old.view.active_peer_connections;
                model.view.payload_download_rate_bytes =
                    old.view.payload_download_rate_bytes.clone();
                model.progress_inputs = old.progress_inputs;
                model.view.progress = assess_progress(torrent, model.progress_inputs);
                model.active = old.active.clone();
                model.peers = old.peers.clone();
            }
            next.insert(torrent.torrent_id.clone(), model);
        }
        hub.revision = revision;
        hub.torrents = next;
        hub.publish_changes(&previous)
    }

    pub(crate) fn record_activity(
        &self,
        torrent_id: &str,
        activity: TorrentActivity,
    ) -> Result<(), SubscriptionError> {
        let mut hub = self
            .inner
            .lock()
            .map_err(|_| SubscriptionError::Internal("view hub lock is poisoned".to_owned()))?;
        let previous = hub.torrents.clone();
        let Some(model) = hub.torrents.get_mut(torrent_id) else {
            return Ok(());
        };
        model.apply_activity(activity);
        hub.publish_changes(&previous)
    }

    pub(crate) fn set_progress_inputs(
        &self,
        torrent_id: &str,
        inputs: ProgressInputs,
    ) -> Result<(), SubscriptionError> {
        let mut hub = self
            .inner
            .lock()
            .map_err(|_| SubscriptionError::Internal("view hub lock is poisoned".to_owned()))?;
        let previous = hub.torrents.clone();
        let Some(model) = hub.torrents.get_mut(torrent_id) else {
            return Ok(());
        };
        model.progress_inputs = inputs;
        model.view.progress = assess_progress(&model.snapshot, inputs);
        hub.publish_changes(&previous)
    }

    pub(crate) fn set_discovery_activity(
        &self,
        torrent_id: &str,
        active: bool,
        retry_scheduled: bool,
    ) -> Result<(), SubscriptionError> {
        let mut hub = self
            .inner
            .lock()
            .map_err(|_| SubscriptionError::Internal("view hub lock is poisoned".to_owned()))?;
        let previous = hub.torrents.clone();
        let Some(model) = hub.torrents.get_mut(torrent_id) else {
            return Ok(());
        };
        if model.snapshot.state != TorrentState::AwaitingMetadata {
            return Ok(());
        }
        model.progress_inputs.task_active = active;
        model.progress_inputs.discovery_active = active;
        model.progress_inputs.discovery_retry_scheduled = retry_scheduled;
        model.progress_inputs.discovery_exhausted = false;
        model.view.progress = assess_progress(&model.snapshot, model.progress_inputs);
        hub.publish_changes(&previous)
    }

    pub(crate) fn record_peer_connections(
        &self,
        torrent_id: &str,
        captured_at: Duration,
        peers: &[PeerConnectionObservation],
    ) -> Result<(), SubscriptionError> {
        let mut hub = self
            .inner
            .lock()
            .map_err(|_| SubscriptionError::Internal("view hub lock is poisoned".to_owned()))?;
        let Some(model) = hub.torrents.get_mut(torrent_id) else {
            return Ok(());
        };
        let previous_view = model.view.clone();
        let previous_peers = std::mem::take(&mut model.peers);
        model.peers = peers
            .iter()
            .map(|peer| {
                let view = PeerView::from_observation(torrent_id, captured_at, peer);
                (view.connection_id.clone(), view)
            })
            .collect();
        model.view.active_peer_connections = model.peers.len().try_into().unwrap_or(u32::MAX);
        model.view.payload_download_rate_bytes = peers
            .iter()
            .filter_map(|peer| peer.content.as_ref())
            .fold(0_u64, |total, content| {
                total.saturating_add(content.observed_payload_rate.try_into().unwrap_or(u64::MAX))
            })
            .to_string();
        let next_view = model.view.clone();
        let next_peers = model.peers.clone();
        hub.publish_peer_changes(
            torrent_id,
            &previous_view,
            &next_view,
            &previous_peers,
            &next_peers,
        )
    }

    pub fn record_diagnostic(
        &self,
        severity: DiagnosticSeverity,
        category: DiagnosticCategory,
        code: &str,
        torrent_id: Option<&str>,
        summary: &str,
        context: &[(&str, &str)],
    ) -> Result<(), SubscriptionError> {
        let mut hub = self
            .inner
            .lock()
            .map_err(|_| SubscriptionError::Internal("view hub lock is poisoned".to_owned()))?;
        let event = DiagnosticEvent {
            sequence: hub.next_diagnostic_sequence.to_string(),
            timestamp_millis: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .to_string(),
            severity,
            category,
            code: sanitize_text(code, MAX_DIAGNOSTIC_KEY_CHARS),
            torrent_id: torrent_id.map(|value| sanitize_text(value, 40)),
            summary: sanitize_text(summary, MAX_DIAGNOSTIC_SUMMARY_CHARS),
            context: context
                .iter()
                .take(MAX_DIAGNOSTIC_CONTEXT_FIELDS)
                .map(|(key, value)| DiagnosticField {
                    key: sanitize_text(key, MAX_DIAGNOSTIC_KEY_CHARS),
                    value: sanitize_text(value, MAX_DIAGNOSTIC_VALUE_CHARS),
                })
                .collect(),
        };
        hub.next_diagnostic_sequence = hub.next_diagnostic_sequence.saturating_add(1);
        let encoded_bytes = serde_json::to_vec(&event)
            .map_err(|error| SubscriptionError::Internal(error.to_string()))?
            .len();
        hub.diagnostics.push_back(StoredDiagnostic {
            event: event.clone(),
            encoded_bytes,
        });
        hub.diagnostic_bytes = hub.diagnostic_bytes.saturating_add(encoded_bytes);
        while hub.diagnostics.len() > MAX_DIAGNOSTIC_EVENTS
            || hub.diagnostic_bytes > MAX_DIAGNOSTIC_BYTES
        {
            let Some(dropped) = hub.diagnostics.pop_front() else {
                break;
            };
            hub.diagnostic_bytes = hub.diagnostic_bytes.saturating_sub(dropped.encoded_bytes);
            hub.diagnostic_dropped = hub.diagnostic_dropped.saturating_add(1);
        }
        hub.publish_diagnostic(event)
    }
}

impl HubState {
    pub(crate) fn snapshot_for(&self, spec: &SubscriptionSpec) -> ViewSnapshot {
        match (&spec.selector, spec.projection) {
            (ViewSelector::TorrentList, ViewProjection::Summary) => ViewSnapshot::TorrentList {
                torrents: self
                    .torrents
                    .values()
                    .map(|torrent| torrent.view.clone())
                    .collect(),
            },
            (ViewSelector::Torrent { torrent_id }, ViewProjection::Summary) => {
                ViewSnapshot::Torrent {
                    torrent: self
                        .torrents
                        .get(torrent_id)
                        .map(|torrent| torrent.view.clone()),
                }
            }
            (ViewSelector::Torrent { torrent_id }, ViewProjection::PieceActivity) => {
                let torrent = self.torrents.get(torrent_id);
                ViewSnapshot::PieceActivity {
                    torrent_id: torrent_id.clone(),
                    piece_count: torrent.map_or(0, |torrent| torrent.view.piece_count),
                    verified: torrent.map_or_else(Vec::new, |torrent| torrent.verified.clone()),
                    active: torrent.and_then(|torrent| torrent.active.clone()),
                }
            }
            (ViewSelector::Torrent { torrent_id }, ViewProjection::Peers) => ViewSnapshot::Peers {
                torrent_id: torrent_id.clone(),
                peers: self
                    .torrents
                    .get(torrent_id)
                    .map_or_else(Vec::new, |torrent| {
                        torrent.peers.values().cloned().collect()
                    }),
            },
            (selector, ViewProjection::Diagnostics) => {
                let filter = spec.diagnostics.clone().unwrap_or_default();
                ViewSnapshot::Diagnostics {
                    events: self
                        .diagnostics
                        .iter()
                        .map(|stored| &stored.event)
                        .filter(|event| diagnostic_matches(selector, &filter, event))
                        .cloned()
                        .collect(),
                    dropped_count: self.diagnostic_dropped.to_string(),
                }
            }
            (ViewSelector::TorrentList, ViewProjection::PieceActivity | ViewProjection::Peers) => {
                unreachable!("invalid projection is rejected before snapshot construction")
            }
        }
    }

    fn publish_changes(
        &mut self,
        previous: &BTreeMap<String, TorrentModel>,
    ) -> Result<(), SubscriptionError> {
        let revision = self.revision;
        self.retain_live_view_sets();
        let current = &self.torrents;
        self.subscribers.retain(|_, weak| weak.strong_count() != 0);
        let subscribers = self
            .subscribers
            .values()
            .filter_map(Weak::upgrade)
            .collect::<Vec<_>>();
        for subscriber in subscribers {
            let patch = patch_for(&subscriber.spec, previous, current);
            if let Some(patch) = patch {
                subscriber.enqueue_patch(revision, patch)?;
            }
        }
        let view_sets = self.view_sets.values().cloned().collect::<Vec<_>>();
        for view_set in view_sets {
            for spec in view_set.view_specs()? {
                let subscription = spec.subscription_spec(DEFAULT_VIEW_SET_QUEUE_BYTES);
                if let Some(patch) = patch_for(&subscription, previous, current) {
                    view_set.enqueue_patch(spec.view_id(), patch, revision)?;
                }
            }
        }
        Ok(())
    }

    fn publish_diagnostic(&mut self, event: DiagnosticEvent) -> Result<(), SubscriptionError> {
        let revision = self.revision;
        let dropped_count = self.diagnostic_dropped.to_string();
        self.subscribers.retain(|_, weak| weak.strong_count() != 0);
        let subscribers = self
            .subscribers
            .values()
            .filter_map(Weak::upgrade)
            .collect::<Vec<_>>();
        for subscriber in subscribers {
            if subscriber.spec.projection != ViewProjection::Diagnostics {
                continue;
            }
            let filter = subscriber.spec.diagnostics.clone().unwrap_or_default();
            if diagnostic_matches(&subscriber.spec.selector, &filter, &event) {
                subscriber.enqueue_diagnostic_patch(
                    revision,
                    ViewPatch::Diagnostics {
                        events: vec![event.clone()],
                        dropped_count: dropped_count.clone(),
                    },
                )?;
            }
        }
        self.retain_live_view_sets();
        let view_sets = self.view_sets.values().cloned().collect::<Vec<_>>();
        for view_set in view_sets {
            for spec in view_set.view_specs()? {
                let subscription = spec.subscription_spec(DEFAULT_VIEW_SET_QUEUE_BYTES);
                if subscription.projection != ViewProjection::Diagnostics {
                    continue;
                }
                let filter = subscription.diagnostics.clone().unwrap_or_default();
                if diagnostic_matches(&subscription.selector, &filter, &event) {
                    view_set.enqueue_patch(
                        spec.view_id(),
                        ViewPatch::Diagnostics {
                            events: vec![event.clone()],
                            dropped_count: dropped_count.clone(),
                        },
                        revision,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn publish_peer_changes(
        &mut self,
        torrent_id: &str,
        previous_view: &TorrentView,
        next_view: &TorrentView,
        previous_peers: &BTreeMap<String, PeerView>,
        next_peers: &BTreeMap<String, PeerView>,
    ) -> Result<(), SubscriptionError> {
        let revision = self.revision;
        self.subscribers.retain(|_, weak| weak.strong_count() != 0);
        let subscribers = self
            .subscribers
            .values()
            .filter_map(Weak::upgrade)
            .collect::<Vec<_>>();
        for subscriber in subscribers {
            if let Some(patch) = targeted_peer_patch(
                &subscriber.spec,
                torrent_id,
                previous_view,
                next_view,
                previous_peers,
                next_peers,
            ) {
                subscriber.enqueue_patch(revision, patch)?;
            }
        }
        self.retain_live_view_sets();
        let view_sets = self.view_sets.values().cloned().collect::<Vec<_>>();
        for view_set in view_sets {
            for spec in view_set.view_specs()? {
                if let Some(patch) = targeted_peer_patch(
                    &spec.subscription_spec(DEFAULT_VIEW_SET_QUEUE_BYTES),
                    torrent_id,
                    previous_view,
                    next_view,
                    previous_peers,
                    next_peers,
                ) {
                    view_set.enqueue_patch(spec.view_id(), patch, revision)?;
                }
            }
        }
        Ok(())
    }

    fn retain_live_view_sets(&mut self) {
        let now = std::time::Instant::now();
        self.view_sets.retain(|_, view_set| {
            let retain = !view_set.is_expired(now);
            if !retain {
                view_set.close();
            }
            retain
        });
    }

    pub(crate) fn snapshots_for_view_set(
        &self,
        view_set: &ViewSetInner,
    ) -> Result<(u64, Vec<ViewSetUpdate>), crate::ViewSetError> {
        let snapshots = view_set
            .view_specs()?
            .into_iter()
            .map(|spec| ViewSetUpdate::Snapshot {
                view_id: spec.view_id().to_owned(),
                snapshot: self.snapshot_for(&spec.subscription_spec(DEFAULT_VIEW_SET_QUEUE_BYTES)),
            })
            .collect();
        Ok((self.revision, snapshots))
    }
}

impl TorrentModel {
    fn from_snapshot(snapshot: &TorrentSnapshot) -> Self {
        let progress_inputs = ProgressInputs::default();
        Self {
            view: TorrentView {
                torrent_id: snapshot.torrent_id.clone(),
                state: snapshot.state,
                storage_state: snapshot.storage_state,
                metadata_available: snapshot.metadata_available,
                piece_count: snapshot.piece_count,
                verified_piece_count: snapshot.verified_piece_count,
                requested_bytes: "0".to_owned(),
                received_bytes: "0".to_owned(),
                stored_bytes: "0".to_owned(),
                active_peer_connections: 0,
                payload_download_rate_bytes: "0".to_owned(),
                progress: assess_progress(snapshot, progress_inputs),
                error: snapshot.error.clone(),
            },
            snapshot: snapshot.clone(),
            progress_inputs,
            verified: Vec::new(),
            active: None,
            peers: BTreeMap::new(),
        }
    }

    fn apply_activity(&mut self, activity: TorrentActivity) {
        match activity {
            TorrentActivity::PieceStarted {
                piece_index,
                piece_length,
            } => {
                self.active = Some(ActivePiece {
                    piece_index,
                    piece_length,
                    requested: Vec::new(),
                    received: Vec::new(),
                    stored: Vec::new(),
                });
            }
            TorrentActivity::BlockRequested {
                piece_index,
                begin,
                length,
            } => {
                add_counter(&mut self.view.requested_bytes, u64::from(length));
                if let Some(active) = matching_active(&mut self.active, piece_index) {
                    insert_range(&mut active.requested, begin, length);
                }
            }
            TorrentActivity::BlockReceived {
                piece_index,
                begin,
                length,
            } => {
                add_counter(&mut self.view.received_bytes, u64::from(length));
                if let Some(active) = matching_active(&mut self.active, piece_index) {
                    remove_range(&mut active.requested, begin, length);
                    insert_range(&mut active.received, begin, length);
                }
            }
            TorrentActivity::BlockStored {
                piece_index,
                begin,
                length,
            } => {
                add_counter(&mut self.view.stored_bytes, u64::from(length));
                if let Some(active) = matching_active(&mut self.active, piece_index) {
                    remove_range(&mut active.received, begin, length);
                    insert_range(&mut active.stored, begin, length);
                }
            }
            TorrentActivity::PieceVerified { piece_index } => {
                insert_range(&mut self.verified, piece_index, 1);
                self.view.verified_piece_count =
                    range_cardinality(&self.verified).min(u64::from(u32::MAX)) as u32;
                if self
                    .active
                    .as_ref()
                    .is_some_and(|active| active.piece_index == piece_index)
                {
                    self.active = None;
                }
            }
            TorrentActivity::PieceHashFailed { piece_index } => {
                if let Some(active) = matching_active(&mut self.active, piece_index) {
                    active.requested.clear();
                    active.received.clear();
                    active.stored.clear();
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TorrentActivity {
    PieceStarted {
        piece_index: u32,
        piece_length: u32,
    },
    BlockRequested {
        piece_index: u32,
        begin: u32,
        length: u32,
    },
    BlockReceived {
        piece_index: u32,
        begin: u32,
        length: u32,
    },
    BlockStored {
        piece_index: u32,
        begin: u32,
        length: u32,
    },
    PieceVerified {
        piece_index: u32,
    },
    PieceHashFailed {
        piece_index: u32,
    },
}

impl ViewSubscription {
    pub fn stream_id(&self) -> String {
        self.inner.stream_id.to_string()
    }

    pub async fn next_update(&self) -> Option<ViewUpdate> {
        loop {
            let notified = self.inner.notify.notified();
            let wait = {
                let mut queue = self.inner.queue.lock().ok()?;
                if !queue.entries.is_empty() {
                    let now = Instant::now();
                    if now >= queue.next_delivery {
                        let queued = queue.entries.pop_front().expect("front was present");
                        queue.queued_bytes -= queued.encoded_bytes;
                        queue.next_delivery = now
                            + Duration::from_millis(u64::from(
                                self.inner.spec.delivery.min_interval_millis,
                            ));
                        return Some(queued.update);
                    }
                    Some(queue.next_delivery - now)
                } else if queue.closed {
                    return None;
                } else {
                    None
                }
            };
            if let Some(wait) = wait {
                tokio::select! {
                    () = tokio::time::sleep(wait) => {}
                    () = notified => {}
                }
            } else {
                notified.await;
            }
        }
    }

    pub fn resync(&self) -> Result<(), SubscriptionError> {
        let hub = self.hub.upgrade().ok_or(SubscriptionError::Closed)?;
        let hub = hub
            .lock()
            .map_err(|_| SubscriptionError::Internal("view hub lock is poisoned".to_owned()))?;
        self.inner
            .replace_with_snapshot(hub.revision, hub.snapshot_for(&self.inner.spec))
    }

    pub fn stats(&self) -> Result<SubscriptionStats, SubscriptionError> {
        let queue = self
            .inner
            .queue
            .lock()
            .map_err(|_| SubscriptionError::Internal("queue lock is poisoned".to_owned()))?;
        Ok(SubscriptionStats {
            queued_bytes: queue.queued_bytes,
            queue_high_water: queue.queue_high_water,
            reset_count: queue.reset_count,
        })
    }

    pub fn close(&self) {
        if let Ok(mut queue) = self.inner.queue.lock() {
            queue.closed = true;
            queue.entries.clear();
            queue.queued_bytes = 0;
        }
        self.inner.notify.notify_one();
    }
}

impl Drop for ViewSubscription {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) == 1 {
            self.close();
        }
    }
}

impl SubscriberInner {
    fn enqueue_snapshot(
        &self,
        revision: u64,
        snapshot: ViewSnapshot,
    ) -> Result<(), SubscriptionError> {
        self.replace_with_snapshot(revision, snapshot)
    }

    fn enqueue_patch(&self, revision: u64, patch: ViewPatch) -> Result<(), SubscriptionError> {
        self.enqueue(revision, ViewUpdatePayload::Patch { patch }, true)
    }

    fn enqueue_diagnostic_patch(
        &self,
        revision: u64,
        patch: ViewPatch,
    ) -> Result<(), SubscriptionError> {
        self.enqueue(revision, ViewUpdatePayload::Patch { patch }, false)
    }

    fn replace_with_snapshot(
        &self,
        revision: u64,
        snapshot: ViewSnapshot,
    ) -> Result<(), SubscriptionError> {
        let mut queue = self
            .queue
            .lock()
            .map_err(|_| SubscriptionError::Internal("queue lock is poisoned".to_owned()))?;
        if queue.closed {
            return Err(SubscriptionError::Closed);
        }
        queue.entries.clear();
        queue.queued_bytes = 0;
        queue.needs_resync = false;
        let update = make_update(
            self,
            &mut queue,
            revision,
            ViewUpdatePayload::Snapshot { snapshot },
        );
        let encoded_bytes = encoded_len(&update)?;
        if encoded_bytes > self.spec.delivery.max_queue_bytes as usize {
            return Err(SubscriptionError::SnapshotExceedsQueue {
                snapshot: encoded_bytes,
                maximum: self.spec.delivery.max_queue_bytes,
            });
        }
        push_update(&mut queue, update, encoded_bytes);
        drop(queue);
        self.notify.notify_one();
        Ok(())
    }

    fn enqueue(
        &self,
        revision: u64,
        payload: ViewUpdatePayload,
        allow_coalesce: bool,
    ) -> Result<(), SubscriptionError> {
        let mut queue = self
            .queue
            .lock()
            .map_err(|_| SubscriptionError::Internal("queue lock is poisoned".to_owned()))?;
        if queue.closed || queue.needs_resync {
            return Ok(());
        }
        if allow_coalesce
            && let Some(back) = queue.entries.back_mut()
            && coalesce(&mut back.update, &payload)
        {
            let (previous_bytes, next_bytes) = {
                let previous_bytes = back.encoded_bytes;
                back.update.revision = revision.to_string();
                back.encoded_bytes = encoded_len(&back.update)?;
                (previous_bytes, back.encoded_bytes)
            };
            queue.tail_revision = revision;
            queue.queued_bytes = queue.queued_bytes - previous_bytes + next_bytes;
            if queue.queued_bytes <= self.spec.delivery.max_queue_bytes as usize {
                queue.queue_high_water = queue.queue_high_water.max(queue.queued_bytes);
                drop(queue);
                self.notify.notify_one();
                return Ok(());
            }
        } else {
            let update = make_update(self, &mut queue, revision, payload);
            let encoded_bytes = encoded_len(&update)?;
            if encoded_bytes <= self.spec.delivery.max_queue_bytes as usize
                && queue.queued_bytes + encoded_bytes <= self.spec.delivery.max_queue_bytes as usize
            {
                push_update(&mut queue, update, encoded_bytes);
                drop(queue);
                self.notify.notify_one();
                return Ok(());
            }
        }

        queue.entries.clear();
        queue.queued_bytes = 0;
        queue.needs_resync = true;
        queue.reset_count = queue.reset_count.saturating_add(1);
        let update = make_update(
            self,
            &mut queue,
            revision,
            ViewUpdatePayload::ResetRequired {
                reason: ResetReason::QueueOverflow,
            },
        );
        let encoded_bytes = encoded_len(&update)?;
        push_update(&mut queue, update, encoded_bytes);
        drop(queue);
        self.notify.notify_one();
        Ok(())
    }
}

pub(crate) fn validate_spec(spec: &SubscriptionSpec) -> Result<(), SubscriptionError> {
    if spec.delivery.min_interval_millis > MAX_SUBSCRIPTION_INTERVAL_MILLIS {
        return Err(SubscriptionError::InvalidInterval {
            maximum: MAX_SUBSCRIPTION_INTERVAL_MILLIS,
        });
    }
    if !(MIN_SUBSCRIPTION_QUEUE_BYTES..=MAX_SUBSCRIPTION_QUEUE_BYTES)
        .contains(&spec.delivery.max_queue_bytes)
    {
        return Err(SubscriptionError::InvalidQueueBound {
            minimum: MIN_SUBSCRIPTION_QUEUE_BYTES,
            maximum: MAX_SUBSCRIPTION_QUEUE_BYTES,
        });
    }
    if matches!(spec.selector, ViewSelector::TorrentList)
        && matches!(
            spec.projection,
            ViewProjection::PieceActivity | ViewProjection::Peers
        )
    {
        return Err(SubscriptionError::InvalidProjection);
    }
    if spec.projection != ViewProjection::Diagnostics && spec.diagnostics.is_some() {
        return Err(SubscriptionError::InvalidProjection);
    }
    if let Some(filter) = &spec.diagnostics
        && (filter.categories.len() > 12
            || filter
                .categories
                .iter()
                .enumerate()
                .any(|(index, category)| filter.categories[..index].contains(category)))
    {
        return Err(SubscriptionError::InvalidProjection);
    }
    Ok(())
}

fn parse_revision(value: &str) -> Result<u64, SubscriptionError> {
    value
        .parse()
        .map_err(|_| SubscriptionError::Internal("invalid snapshot revision".to_owned()))
}

fn make_update(
    subscriber: &SubscriberInner,
    queue: &mut QueueState,
    revision: u64,
    payload: ViewUpdatePayload,
) -> ViewUpdate {
    let sequence = queue.next_sequence;
    queue.next_sequence = queue.next_sequence.saturating_add(1);
    let base_revision = queue.tail_revision;
    queue.tail_revision = revision;
    ViewUpdate {
        contract_version: VIEW_CONTRACT_VERSION,
        stream_id: subscriber.stream_id.to_string(),
        epoch: subscriber.epoch.to_string(),
        sequence: sequence.to_string(),
        base_revision: base_revision.to_string(),
        revision: revision.to_string(),
        payload,
    }
}

fn encoded_len(update: &ViewUpdate) -> Result<usize, SubscriptionError> {
    serde_json::to_vec(update)
        .map(|bytes| bytes.len())
        .map_err(|error| SubscriptionError::Internal(error.to_string()))
}

fn push_update(queue: &mut QueueState, update: ViewUpdate, encoded_bytes: usize) {
    queue.queued_bytes += encoded_bytes;
    queue.queue_high_water = queue.queue_high_water.max(queue.queued_bytes);
    queue.entries.push_back(QueuedUpdate {
        update,
        encoded_bytes,
    });
}

fn patch_for(
    spec: &SubscriptionSpec,
    previous: &BTreeMap<String, TorrentModel>,
    current: &BTreeMap<String, TorrentModel>,
) -> Option<ViewPatch> {
    match (&spec.selector, spec.projection) {
        (ViewSelector::TorrentList, ViewProjection::Summary) => {
            let upsert = current
                .iter()
                .filter(|(id, model)| previous.get(*id).map(|old| &old.view) != Some(&model.view))
                .map(|(_, model)| model.view.clone())
                .collect::<Vec<_>>();
            let removed = previous
                .keys()
                .filter(|id| !current.contains_key(*id))
                .cloned()
                .collect::<Vec<_>>();
            (!upsert.is_empty() || !removed.is_empty())
                .then_some(ViewPatch::TorrentList { upsert, removed })
        }
        (ViewSelector::Torrent { torrent_id }, ViewProjection::Summary) => {
            let old = previous.get(torrent_id).map(|model| &model.view);
            let next = current.get(torrent_id).map(|model| &model.view);
            (old != next).then(|| ViewPatch::Torrent {
                torrent: next.cloned(),
            })
        }
        (ViewSelector::Torrent { torrent_id }, ViewProjection::PieceActivity) => {
            let old = previous.get(torrent_id);
            let next = current.get(torrent_id);
            if old == next {
                return None;
            }
            let old_verified = old.map_or(&[][..], |model| model.verified.as_slice());
            let next_verified = next.map_or(&[][..], |model| model.verified.as_slice());
            let verified = difference(next_verified, old_verified);
            let cleared = difference(old_verified, next_verified);
            Some(ViewPatch::PieceActivity {
                torrent_id: torrent_id.clone(),
                piece_count: next.map_or(0, |model| model.view.piece_count),
                verified,
                cleared,
                active: next.and_then(|model| model.active.clone()),
            })
        }
        (ViewSelector::Torrent { torrent_id }, ViewProjection::Peers) => {
            let empty = BTreeMap::new();
            let old = previous
                .get(torrent_id)
                .map_or(&empty, |model| &model.peers);
            let next = current.get(torrent_id).map_or(&empty, |model| &model.peers);
            peer_collection_patch(torrent_id, old, next)
        }
        (ViewSelector::TorrentList, ViewProjection::PieceActivity | ViewProjection::Peers) => None,
        (_, ViewProjection::Diagnostics) => None,
    }
}

fn targeted_peer_patch(
    spec: &SubscriptionSpec,
    torrent_id: &str,
    previous_view: &TorrentView,
    next_view: &TorrentView,
    previous_peers: &BTreeMap<String, PeerView>,
    next_peers: &BTreeMap<String, PeerView>,
) -> Option<ViewPatch> {
    match (&spec.selector, spec.projection) {
        (ViewSelector::TorrentList, ViewProjection::Summary) => {
            (previous_view != next_view).then(|| ViewPatch::TorrentList {
                upsert: vec![next_view.clone()],
                removed: Vec::new(),
            })
        }
        (
            ViewSelector::Torrent {
                torrent_id: selected,
            },
            ViewProjection::Summary,
        ) if selected == torrent_id => (previous_view != next_view).then(|| ViewPatch::Torrent {
            torrent: Some(next_view.clone()),
        }),
        (
            ViewSelector::Torrent {
                torrent_id: selected,
            },
            ViewProjection::Peers,
        ) if selected == torrent_id => {
            peer_collection_patch(torrent_id, previous_peers, next_peers)
        }
        _ => None,
    }
}

fn peer_collection_patch(
    torrent_id: &str,
    previous: &BTreeMap<String, PeerView>,
    current: &BTreeMap<String, PeerView>,
) -> Option<ViewPatch> {
    let upsert = current
        .iter()
        .filter(|(id, peer)| previous.get(*id) != Some(*peer))
        .map(|(_, peer)| peer.clone())
        .collect::<Vec<_>>();
    let removed = previous
        .keys()
        .filter(|id| !current.contains_key(*id))
        .cloned()
        .collect::<Vec<_>>();
    (!upsert.is_empty() || !removed.is_empty()).then(|| ViewPatch::Peers {
        torrent_id: torrent_id.to_owned(),
        upsert,
        removed,
    })
}

fn coalesce(update: &mut ViewUpdate, next: &ViewUpdatePayload) -> bool {
    let (ViewUpdatePayload::Patch { patch: current }, ViewUpdatePayload::Patch { patch: next }) =
        (&mut update.payload, next)
    else {
        return false;
    };
    coalesce_patch(current, next)
}

pub(crate) fn coalesce_patch(current: &mut ViewPatch, next: &ViewPatch) -> bool {
    match (current, next) {
        (
            ViewPatch::TorrentList { upsert, removed },
            ViewPatch::TorrentList {
                upsert: next_upsert,
                removed: next_removed,
            },
        ) => {
            let mut values = upsert
                .drain(..)
                .map(|torrent| (torrent.torrent_id.clone(), torrent))
                .collect::<BTreeMap<_, _>>();
            for id in next_removed {
                values.remove(id);
            }
            for torrent in next_upsert {
                values.insert(torrent.torrent_id.clone(), torrent.clone());
            }
            let mut removed_ids = removed.drain(..).collect::<std::collections::BTreeSet<_>>();
            for torrent in next_upsert {
                removed_ids.remove(&torrent.torrent_id);
            }
            removed_ids.extend(next_removed.iter().cloned());
            *upsert = values.into_values().collect();
            *removed = removed_ids.into_iter().collect();
            true
        }
        (ViewPatch::Torrent { torrent }, ViewPatch::Torrent { torrent: next }) => {
            *torrent = next.clone();
            true
        }
        (
            ViewPatch::PieceActivity {
                torrent_id,
                piece_count,
                verified,
                cleared,
                active,
            },
            ViewPatch::PieceActivity {
                torrent_id: next_id,
                piece_count: next_piece_count,
                verified: next_verified,
                cleared: next_cleared,
                active: next_active,
            },
        ) if torrent_id == next_id => {
            for range in next_cleared {
                remove_interval(verified, *range);
                insert_interval(cleared, *range);
            }
            for range in next_verified {
                remove_interval(cleared, *range);
                insert_interval(verified, *range);
            }
            *piece_count = *next_piece_count;
            *active = next_active.clone();
            true
        }
        (
            ViewPatch::Peers {
                torrent_id,
                upsert,
                removed,
            },
            ViewPatch::Peers {
                torrent_id: next_id,
                upsert: next_upsert,
                removed: next_removed,
            },
        ) if torrent_id == next_id => {
            let mut values = upsert
                .drain(..)
                .map(|peer| (peer.connection_id.clone(), peer))
                .collect::<BTreeMap<_, _>>();
            for id in next_removed {
                values.remove(id);
            }
            for peer in next_upsert {
                values.insert(peer.connection_id.clone(), peer.clone());
            }
            let mut removed_ids = removed.drain(..).collect::<std::collections::BTreeSet<_>>();
            for peer in next_upsert {
                removed_ids.remove(&peer.connection_id);
            }
            removed_ids.extend(next_removed.iter().cloned());
            *upsert = values.into_values().collect();
            *removed = removed_ids.into_iter().collect();
            true
        }
        (
            ViewPatch::Diagnostics {
                events,
                dropped_count,
            },
            ViewPatch::Diagnostics {
                events: next_events,
                dropped_count: next_dropped_count,
            },
        ) => {
            events.extend(next_events.iter().cloned());
            *dropped_count = next_dropped_count.clone();
            true
        }
        _ => false,
    }
}

fn diagnostic_matches(
    selector: &ViewSelector,
    filter: &DiagnosticFilter,
    event: &DiagnosticEvent,
) -> bool {
    if event.severity < filter.minimum_severity {
        return false;
    }
    if let ViewSelector::Torrent { torrent_id } = selector
        && event.torrent_id.as_deref() != Some(torrent_id.as_str())
    {
        return false;
    }
    if !filter.categories.is_empty() && !filter.categories.contains(&event.category) {
        return filter.profile == DiagnosticProfile::Normal
            && event.severity >= DiagnosticSeverity::Warning;
    }
    match filter.profile {
        DiagnosticProfile::Trace => true,
        DiagnosticProfile::Detailed => event.category != DiagnosticCategory::Piece,
        DiagnosticProfile::Normal => {
            event.severity >= DiagnosticSeverity::Warning
                || matches!(
                    event.category,
                    DiagnosticCategory::Lifecycle
                        | DiagnosticCategory::Discovery
                        | DiagnosticCategory::Tracker
                        | DiagnosticCategory::Peer
                        | DiagnosticCategory::Storage
                        | DiagnosticCategory::Integrity
                        | DiagnosticCategory::Platform
                )
        }
    }
}

fn sanitize_text(value: &str, maximum_chars: usize) -> String {
    value
        .chars()
        .filter(|character| {
            !character.is_control()
                && !matches!(
                    *character,
                    '\u{061c}'
                        | '\u{200e}'
                        | '\u{200f}'
                        | '\u{202a}'..='\u{202e}'
                        | '\u{2066}'..='\u{2069}'
                )
        })
        .take(maximum_chars)
        .collect()
}

fn matching_active(active: &mut Option<ActivePiece>, piece_index: u32) -> Option<&mut ActivePiece> {
    active
        .as_mut()
        .filter(|active| active.piece_index == piece_index)
}

fn add_counter(counter: &mut String, increment: u64) {
    let value = counter
        .parse::<u64>()
        .unwrap_or(0)
        .saturating_add(increment);
    *counter = value.to_string();
}

fn insert_range(ranges: &mut Vec<IndexRange>, start: u32, length: u32) {
    if let Some(end) = start.checked_add(length)
        && let Some(range) = IndexRange::new(start, end)
    {
        insert_interval(ranges, range);
    }
}

fn insert_interval(ranges: &mut Vec<IndexRange>, mut inserted: IndexRange) {
    let mut output = Vec::with_capacity(ranges.len() + 1);
    let mut placed = false;
    for range in ranges.drain(..) {
        if range.end_exclusive < inserted.start {
            output.push(range);
        } else if inserted.end_exclusive < range.start {
            if !placed {
                output.push(inserted);
                placed = true;
            }
            output.push(range);
        } else {
            inserted.start = inserted.start.min(range.start);
            inserted.end_exclusive = inserted.end_exclusive.max(range.end_exclusive);
        }
    }
    if !placed {
        output.push(inserted);
    }
    *ranges = output;
}

fn remove_range(ranges: &mut Vec<IndexRange>, start: u32, length: u32) {
    if let Some(end) = start.checked_add(length)
        && let Some(range) = IndexRange::new(start, end)
    {
        remove_interval(ranges, range);
    }
}

fn remove_interval(ranges: &mut Vec<IndexRange>, removed: IndexRange) {
    let mut output = Vec::with_capacity(ranges.len() + 1);
    for range in ranges.drain(..) {
        if range.end_exclusive <= removed.start || range.start >= removed.end_exclusive {
            output.push(range);
            continue;
        }
        if range.start < removed.start {
            output.push(IndexRange {
                start: range.start,
                end_exclusive: removed.start,
            });
        }
        if range.end_exclusive > removed.end_exclusive {
            output.push(IndexRange {
                start: removed.end_exclusive,
                end_exclusive: range.end_exclusive,
            });
        }
    }
    *ranges = output;
}

fn difference(left: &[IndexRange], right: &[IndexRange]) -> Vec<IndexRange> {
    let mut output = left.to_vec();
    for range in right {
        remove_interval(&mut output, *range);
    }
    output
}

fn range_cardinality(ranges: &[IndexRange]) -> u64 {
    ranges
        .iter()
        .map(|range| u64::from(range.end_exclusive - range.start))
        .sum()
}

pub(crate) fn ranges_from_pieces(pieces: &[bool]) -> Vec<IndexRange> {
    let mut ranges = Vec::new();
    let mut start = None;
    for (index, present) in pieces
        .iter()
        .copied()
        .chain(std::iter::once(false))
        .enumerate()
    {
        if present && start.is_none() {
            start = Some(index);
        } else if !present && let Some(range_start) = start.take() {
            let Ok(range_start) = u32::try_from(range_start) else {
                break;
            };
            let Ok(end_exclusive) = u32::try_from(index) else {
                break;
            };
            ranges.push(IndexRange {
                start: range_start,
                end_exclusive,
            });
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        DeliveryPolicy, DiagnosticCategory, DiagnosticFilter, DiagnosticProfile,
        DiagnosticSeverity, IndexRange, ProgressAction, ProgressDisposition, ProgressInputs,
        ProgressReason, ResetReason, SubscriptionSpec, TorrentActivity, ViewHub, ViewPatch,
        ViewProjection, ViewSelector, ViewSnapshot, ViewUpdatePayload, assess_progress,
        ranges_from_pieces,
    };
    use crate::{ServiceSnapshot, StorageState, TorrentSnapshot, TorrentState};

    fn snapshot(revision: u64, piece_count: u32) -> ServiceSnapshot {
        ServiceSnapshot {
            profile_id: "test".to_owned(),
            revision: revision.to_string(),
            torrents: vec![TorrentSnapshot {
                torrent_id: "000102030405060708090a0b0c0d0e0f10111213".to_owned(),
                storage_root: "downloads".to_owned(),
                state: TorrentState::Downloading,
                storage_state: StorageState::Staging,
                metadata_available: true,
                piece_count,
                verified_piece_count: 0,
                skip_files: Vec::new(),
                error: None,
            }],
        }
    }

    fn piece_spec(queue: u32) -> SubscriptionSpec {
        SubscriptionSpec {
            selector: ViewSelector::Torrent {
                torrent_id: "000102030405060708090a0b0c0d0e0f10111213".to_owned(),
            },
            projection: ViewProjection::PieceActivity,
            delivery: DeliveryPolicy {
                min_interval_millis: 0,
                max_queue_bytes: queue,
            },
            diagnostics: None,
        }
    }

    #[tokio::test]
    async fn starts_with_snapshot_and_keeps_large_indices() {
        let hub = ViewHub::new(&snapshot(7, 1_000_000)).expect("hub");
        let subscription = hub.subscribe(piece_spec(4096)).expect("subscribe");
        let update = subscription.next_update().await.expect("snapshot");
        assert_eq!(update.sequence, "1");
        assert_eq!(update.revision, "7");
        let ViewUpdatePayload::Snapshot {
            snapshot:
                ViewSnapshot::PieceActivity {
                    piece_count,
                    verified,
                    ..
                },
        } = update.payload
        else {
            panic!("expected piece snapshot");
        };
        assert_eq!(piece_count, 1_000_000);
        assert!(verified.is_empty());

        hub.record_activity(
            "000102030405060708090a0b0c0d0e0f10111213",
            TorrentActivity::PieceStarted {
                piece_index: 900_000,
                piece_length: 32 * 1024 * 1024,
            },
        )
        .expect("activity");
        let update = subscription.next_update().await.expect("patch");
        let ViewUpdatePayload::Patch { patch } = update.payload else {
            panic!("expected patch");
        };
        let serialized = serde_json::to_string(&patch).expect("serialize");
        assert!(serialized.contains("900000"));
        assert!(serialized.contains("33554432"));
    }

    #[tokio::test]
    async fn piece_hash_failure_clears_unverified_active_ranges() {
        let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
        let hub = ViewHub::new(&snapshot(0, 1)).expect("hub");
        let subscription = hub.subscribe(piece_spec(4096)).expect("subscribe");
        subscription.next_update().await.expect("snapshot");
        hub.record_activity(
            torrent_id,
            TorrentActivity::PieceStarted {
                piece_index: 0,
                piece_length: 16 * 1024,
            },
        )
        .expect("start piece");
        subscription.next_update().await.expect("start patch");
        hub.record_activity(
            torrent_id,
            TorrentActivity::BlockStored {
                piece_index: 0,
                begin: 0,
                length: 16 * 1024,
            },
        )
        .expect("stored block");
        subscription.next_update().await.expect("stored patch");
        hub.record_activity(
            torrent_id,
            TorrentActivity::PieceHashFailed { piece_index: 0 },
        )
        .expect("failed piece");
        let update = subscription.next_update().await.expect("reset patch");
        let ViewUpdatePayload::Patch {
            patch:
                ViewPatch::PieceActivity {
                    active: Some(active),
                    ..
                },
        } = update.payload
        else {
            panic!("expected active-piece reset patch");
        };
        assert!(active.requested.is_empty());
        assert!(active.received.is_empty());
        assert!(active.stored.is_empty());
    }

    #[tokio::test]
    async fn subscribers_have_independent_queues() {
        let hub = ViewHub::new(&snapshot(0, 100)).expect("hub");
        let fast = hub.subscribe(piece_spec(4096)).expect("fast");
        let slow = hub.subscribe(piece_spec(4096)).expect("slow");
        fast.next_update().await.expect("fast snapshot");
        slow.next_update().await.expect("slow snapshot");

        for piece_index in 0..20 {
            hub.record_activity(
                "000102030405060708090a0b0c0d0e0f10111213",
                TorrentActivity::PieceVerified { piece_index },
            )
            .expect("activity");
            fast.next_update().await.expect("fast patch");
        }
        assert_eq!(fast.stats().expect("fast stats").reset_count, 0);
        assert_eq!(slow.stats().expect("slow stats").reset_count, 0);
        let slow_update = slow.next_update().await.expect("coalesced slow patch");
        assert_eq!(slow_update.sequence, "2");
    }

    #[tokio::test]
    async fn overflow_requires_explicit_resync() {
        let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
        let reference = ViewHub::new(&snapshot(0, 2_000)).expect("reference hub");
        for piece_index in (0..2_000).step_by(2) {
            reference
                .record_activity(torrent_id, TorrentActivity::PieceVerified { piece_index })
                .expect("reference activity");
        }
        let reference_subscription = reference
            .subscribe(piece_spec(4 * 1024 * 1024))
            .expect("reference subscription");
        let final_snapshot = reference_subscription
            .next_update()
            .await
            .expect("reference snapshot");
        let queue_bound = u32::try_from(
            serde_json::to_vec(&final_snapshot)
                .expect("encode reference snapshot")
                .len(),
        )
        .expect("snapshot length fits u32")
        .checked_add(1)
        .expect("small snapshot headroom fits u32")
        .max(4096);

        let hub = ViewHub::new(&snapshot(0, 2_000)).expect("hub");
        let subscription = hub
            .subscribe(piece_spec(queue_bound))
            .expect("subscription");
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            subscription.next_update(),
        )
        .await
        .expect("snapshot delivery timed out")
        .expect("snapshot");

        for piece_index in (0..2_000).step_by(2) {
            hub.record_activity(torrent_id, TorrentActivity::PieceVerified { piece_index })
                .expect("activity");
        }
        assert!(
            subscription.stats().expect("queued stats").queued_bytes > 0,
            "activity must enqueue either a patch or reset"
        );
        let update = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            subscription.next_update(),
        )
        .await
        .expect("reset delivery timed out")
        .expect("reset");
        assert_eq!(
            update.payload,
            ViewUpdatePayload::ResetRequired {
                reason: ResetReason::QueueOverflow
            }
        );
        assert_eq!(subscription.stats().expect("stats").reset_count, 1);
        subscription.resync().expect("resync");
        let replacement = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            subscription.next_update(),
        )
        .await
        .expect("replacement delivery timed out")
        .expect("replacement");
        assert!(matches!(
            replacement.payload,
            ViewUpdatePayload::Snapshot { .. }
        ));
    }

    #[test]
    fn piece_ranges_do_not_expand_indices() {
        let mut pieces = vec![false; 70_005];
        pieces[65_536..70_000].fill(true);
        assert_eq!(
            ranges_from_pieces(&pieces),
            vec![IndexRange {
                start: 65_536,
                end_exclusive: 70_000
            }]
        );
    }

    #[test]
    fn durable_replacement_preserves_exact_have_ranges() {
        let hub = ViewHub::new(&snapshot(0, 4)).expect("hub");
        hub.replace_durable(
            &snapshot(1, 4),
            &BTreeMap::from([(
                "000102030405060708090a0b0c0d0e0f10111213".to_owned(),
                vec![IndexRange {
                    start: 1,
                    end_exclusive: 3,
                }],
            )]),
        )
        .expect("replace");
    }

    #[test]
    fn discovery_exhaustion_waits_when_another_mechanism_can_act() {
        let mut torrent = snapshot(0, 0).torrents.remove(0);
        torrent.state = TorrentState::AwaitingMetadata;
        torrent.metadata_available = false;
        let blocked = assess_progress(
            &torrent,
            ProgressInputs {
                discovery_exhausted: true,
                ..ProgressInputs::default()
            },
        );
        assert_eq!(blocked.disposition, ProgressDisposition::Blocked);
        assert_eq!(blocked.reason, ProgressReason::NoEnabledDiscoverySource);

        let waiting = assess_progress(
            &torrent,
            ProgressInputs {
                discovery_exhausted: true,
                dht_enabled: true,
                ..ProgressInputs::default()
            },
        );
        assert_eq!(waiting.disposition, ProgressDisposition::Waiting);
        assert_eq!(waiting.reason, ProgressReason::WaitingForDiscovery);
    }

    #[test]
    fn disabled_network_is_blocked_without_changing_torrent_intent() {
        let mut torrent = snapshot(0, 0).torrents.remove(0);
        torrent.state = TorrentState::AwaitingMetadata;
        torrent.metadata_available = false;
        let assessment = assess_progress(
            &torrent,
            ProgressInputs {
                network_disabled: true,
                discovery_exhausted: true,
                ..ProgressInputs::default()
            },
        );

        assert_eq!(assessment.disposition, ProgressDisposition::Blocked);
        assert_eq!(assessment.reason, ProgressReason::NetworkDisabled);
        assert_eq!(assessment.actions, vec![ProgressAction::EnableNetwork]);
    }

    #[tokio::test]
    async fn diagnostics_filter_before_queue_and_report_ring_drops() {
        let hub = ViewHub::new(&snapshot(0, 1)).expect("hub");
        let filtered = hub
            .subscribe(SubscriptionSpec {
                selector: ViewSelector::TorrentList,
                projection: ViewProjection::Diagnostics,
                delivery: DeliveryPolicy {
                    min_interval_millis: 0,
                    max_queue_bytes: 4 * 1024 * 1024,
                },
                diagnostics: Some(DiagnosticFilter {
                    profile: DiagnosticProfile::Normal,
                    minimum_severity: DiagnosticSeverity::Trace,
                    categories: vec![DiagnosticCategory::Piece],
                }),
            })
            .expect("subscribe");
        filtered.next_update().await.expect("snapshot");
        hub.record_diagnostic(
            DiagnosticSeverity::Trace,
            DiagnosticCategory::Piece,
            "block_received",
            None,
            "trace",
            &[],
        )
        .expect("record trace");
        assert_eq!(filtered.stats().expect("stats").queued_bytes, 0);

        hub.record_diagnostic(
            DiagnosticSeverity::Warning,
            DiagnosticCategory::Tracker,
            "tracker_unavailable",
            None,
            "warning",
            &[],
        )
        .expect("record warning");
        let warning = filtered.next_update().await.expect("warning patch");
        assert!(
            serde_json::to_string(&warning)
                .expect("encode")
                .contains("tracker_unavailable")
        );

        for index in 0..600 {
            hub.record_diagnostic(
                DiagnosticSeverity::Warning,
                DiagnosticCategory::Tracker,
                "bounded",
                None,
                &format!("event {index}"),
                &[("hostile", "\u{202e}<script>")],
            )
            .expect("record bounded event");
        }
        let snapshot = hub
            .subscribe(SubscriptionSpec {
                selector: ViewSelector::TorrentList,
                projection: ViewProjection::Diagnostics,
                delivery: DeliveryPolicy {
                    min_interval_millis: 0,
                    max_queue_bytes: 4 * 1024 * 1024,
                },
                diagnostics: Some(DiagnosticFilter {
                    profile: DiagnosticProfile::Normal,
                    minimum_severity: DiagnosticSeverity::Info,
                    categories: Vec::new(),
                }),
            })
            .expect("bounded subscription")
            .next_update()
            .await
            .expect("bounded snapshot");
        let ViewUpdatePayload::Snapshot {
            snapshot:
                ViewSnapshot::Diagnostics {
                    events,
                    dropped_count,
                },
        } = snapshot.payload
        else {
            panic!("expected diagnostic snapshot");
        };
        assert_eq!(events.len(), super::MAX_DIAGNOSTIC_EVENTS);
        assert_ne!(dropped_count, "0");
        assert!(
            events
                .iter()
                .all(|event| !event.summary.contains('\u{202e}'))
        );
        assert!(
            events
                .iter()
                .flat_map(|event| &event.context)
                .all(|field| !field.value.contains('\u{202e}'))
        );
    }
}
