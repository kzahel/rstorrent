use std::collections::{BTreeMap, BTreeSet, VecDeque};
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

use rstorrent_engine::peer::{
    DialEligibility, PeerFailure, PeerRegistrySnapshot, PeerSource, PeerSources,
};
use rstorrent_engine::{
    DiskCheckpointStage, DiskPieceRuntimeSnapshot, DiskPieceStage, DiskPressure,
    DiskRuntimeSnapshot, PeerConnectionDirection, PeerConnectionLifecycle,
    PeerConnectionObservation, PeerConnectionRole, PeerRequestWindowPhase, PeerTransport,
    TrackerRuntimeSnapshot,
};
use rstorrent_protocol::peer_id::identify_client;

use crate::control::{
    RemovalState, ServiceSnapshot, StorageSettingsSnapshot, StorageState, TorrentSnapshot,
    TorrentState,
};
use crate::diagnostics::{
    DiagnosticCategory, DiagnosticDraft, DiagnosticEvent, DiagnosticField, DiagnosticFilter,
    DiagnosticRetention, DiagnosticSeverity, DiagnosticStore, MAX_DIAGNOSTIC_PATCH_BYTES,
    MAX_DIAGNOSTIC_PATCH_EVENTS, diagnostic_matches, interest_matches, patch_encoded_len,
    valid_filter,
};
use crate::file_views::{FileCatalogState, FileProgressModel, FileView};
use crate::speed::{
    MAX_SPEED_SERIES, SessionRateHistory, SpeedHistoryView, SpeedMetric, SpeedRange,
};
use crate::tracker_views::{TrackerCatalogState, TrackerView, TrackerViewModel};

mod view_set;

pub use view_set::{
    API_VERSION, ApiEncoding, ApiHello, ApiLimits, ApiVersion, DeliveryMode, OpenViewSetOptions,
    OpenViewSetRequest, OpenViewSetResponse, UpdateBatch, UpdateViewSetRequest, ViewDeliveryPolicy,
    ViewSet, ViewSetError, ViewSetOwner, ViewSetStats, ViewSetUpdate, ViewSpec,
};
pub(crate) use view_set::{
    DEFAULT_VIEW_SET_QUEUE_BYTES, VIEW_SET_LEASE_MILLIS, VIEW_SET_REAPER_INTERVAL_MILLIS,
    ViewSetInner, ViewSetLeaseReaper,
};

pub const VIEW_CONTRACT_VERSION: u16 = 2;
pub const MIN_SUBSCRIPTION_QUEUE_BYTES: u32 = 4 * 1024;
pub const MAX_SUBSCRIPTION_QUEUE_BYTES: u32 = 4 * 1024 * 1024;
pub const MAX_SUBSCRIPTION_INTERVAL_MILLIS: u32 = 60_000;

static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ViewSelector {
    TorrentList,
    SessionDht,
    Torrent {
        torrent_id: String,
    },
    SessionSpeed {
        range: SpeedRange,
        metrics: Vec<SpeedMetric>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum ViewProjection {
    Summary,
    PieceActivity,
    Disk,
    Dht,
    Speed,
    Peers,
    Swarm,
    Files,
    Trackers,
    Diagnostics,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum DhtLifecycleView {
    Offline,
    BootstrapEmpty,
    Participating,
    Inactive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum DhtNetworkPolicyView {
    Offline,
    LoopbackOnly,
    Online,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct DhtBucketView {
    pub bucket_index: u16,
    pub good_nodes: u8,
    pub questionable_nodes: u8,
    pub replacement_candidates: u8,
    pub oldest_live_response_age_millis: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct DhtLookupView {
    pub lookup_id: String,
    pub target_id: String,
    pub age_millis: String,
    pub deadline_in_millis: String,
    pub unqueried_candidates: u16,
    pub in_flight_candidates: u16,
    pub responded_candidates: u16,
    pub failed_candidates: u16,
    pub discovered_peers: u16,
    pub closest_responded_prefix_bits: Option<u16>,
    pub last_convergence_improvement_age_millis: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct DhtInspectionView {
    pub lifecycle: DhtLifecycleView,
    pub network_policy: DhtNetworkPolicyView,
    pub local_node_id: String,
    pub captured_millis: String,
    pub routing_nodes_v4: u16,
    pub occupied_buckets_v4: u16,
    pub deepest_shared_prefix_bits_v4: Option<u16>,
    pub active_transactions: u32,
    pub active_lookups: u32,
    pub queries_sent: String,
    pub responses_received: String,
    pub queries_received: String,
    pub malformed_received: String,
    pub rate_limited: String,
    pub discovered_peers: String,
    pub bootstrap_attempts: String,
    pub routing_refreshes: String,
    pub datagram_bytes_sent: String,
    pub datagram_bytes_received: String,
    pub buckets_v4: Vec<DhtBucketView>,
    pub lookups: Vec<DhtLookupView>,
}

impl DhtInspectionView {
    fn inactive() -> Self {
        Self {
            lifecycle: DhtLifecycleView::Inactive,
            network_policy: DhtNetworkPolicyView::Offline,
            local_node_id: "0000000000000000000000000000000000000000".to_owned(),
            captured_millis: "0".to_owned(),
            routing_nodes_v4: 0,
            occupied_buckets_v4: 0,
            deepest_shared_prefix_bits_v4: None,
            active_transactions: 0,
            active_lookups: 0,
            queries_sent: "0".to_owned(),
            responses_received: "0".to_owned(),
            queries_received: "0".to_owned(),
            malformed_received: "0".to_owned(),
            rate_limited: "0".to_owned(),
            discovered_peers: "0".to_owned(),
            bootstrap_attempts: "0".to_owned(),
            routing_refreshes: "0".to_owned(),
            datagram_bytes_sent: "0".to_owned(),
            datagram_bytes_received: "0".to_owned(),
            buckets_v4: (0..160)
                .map(|bucket_index| DhtBucketView {
                    bucket_index,
                    good_nodes: 0,
                    questionable_nodes: 0,
                    replacement_candidates: 0,
                    oldest_live_response_age_millis: None,
                })
                .collect(),
            lookups: Vec::new(),
        }
    }
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
    pub piece_id: String,
    pub piece_index: u32,
    pub attempt: u32,
    pub piece_length: u32,
    pub stage: ActivePieceStageView,
    pub requested: Vec<IndexRange>,
    pub received: Vec<IndexRange>,
    pub stored: Vec<IndexRange>,
    pub age_millis: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum ActivePieceStageView {
    Requested,
    Received,
    Stored,
    Hashing,
    CheckpointDirty,
    CheckpointSyncing,
    CheckpointCommitting,
    Failed,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum DiskPressureView {
    #[default]
    Idle,
    Normal,
    Backpressured,
    Draining,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum DiskPieceStageView {
    Receiving,
    Queued,
    Writing,
    Stored,
    Hashing,
    CheckpointDirty,
    CheckpointSyncing,
    CheckpointCommitting,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum DiskCheckpointStageView {
    Idle,
    Syncing,
    Committing,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct DiskPipelineView {
    pub pressure: DiskPressureView,
    pub checkpoint_stage: DiskCheckpointStageView,
    pub intake_backpressured: bool,
    pub sample_millis: String,
    pub resident_limit_bytes: String,
    pub resident_high_watermark_bytes: String,
    pub resident_low_watermark_bytes: String,
    pub requested_bytes: String,
    pub resident_bytes: String,
    pub queued_write_bytes: String,
    pub writing_bytes: String,
    pub hashing_bytes: String,
    pub checkpoint_dirty_pieces: String,
    pub checkpoint_dirty_bytes: String,
    pub checkpoint_dirty_piece_high_water: String,
    pub checkpoint_dirty_byte_high_water: String,
    pub checkpoint_oldest_dirty_millis: String,
    pub checkpoint_batches_started: String,
    pub checkpoint_batches_completed: String,
    pub checkpoint_pieces_completed: String,
    pub checkpoint_sync_operations_completed: String,
    pub checkpoint_sync_service_micros: String,
    pub checkpoint_sync_service_max_micros: String,
    pub checkpoint_commit_service_micros: String,
    pub checkpoint_commit_service_max_micros: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_active_micros: Option<String>,
    pub storage_jobs_pending: String,
    pub received_bytes_total: String,
    pub stored_bytes_total: String,
    pub verified_bytes_total: String,
    pub receive_rate_bytes: String,
    pub write_rate_bytes: String,
    pub hash_rate_bytes: String,
    pub write_operations_started: String,
    pub write_operations_completed: String,
    pub hash_operations_started: String,
    pub hash_operations_completed: String,
    pub write_queue_wait_micros: String,
    pub write_queue_wait_max_micros: String,
    pub write_service_micros: String,
    pub write_service_max_micros: String,
    pub hash_queue_wait_micros: String,
    pub hash_queue_wait_max_micros: String,
    pub hash_service_micros: String,
    pub hash_service_max_micros: String,
    pub pressure_transition_count: String,
    pub backpressured_millis_total: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl Default for DiskPipelineView {
    fn default() -> Self {
        Self {
            pressure: DiskPressureView::Idle,
            checkpoint_stage: DiskCheckpointStageView::Idle,
            intake_backpressured: false,
            sample_millis: "0".to_owned(),
            resident_limit_bytes: "0".to_owned(),
            resident_high_watermark_bytes: "0".to_owned(),
            resident_low_watermark_bytes: "0".to_owned(),
            requested_bytes: "0".to_owned(),
            resident_bytes: "0".to_owned(),
            queued_write_bytes: "0".to_owned(),
            writing_bytes: "0".to_owned(),
            hashing_bytes: "0".to_owned(),
            checkpoint_dirty_pieces: "0".to_owned(),
            checkpoint_dirty_bytes: "0".to_owned(),
            checkpoint_dirty_piece_high_water: "0".to_owned(),
            checkpoint_dirty_byte_high_water: "0".to_owned(),
            checkpoint_oldest_dirty_millis: "0".to_owned(),
            checkpoint_batches_started: "0".to_owned(),
            checkpoint_batches_completed: "0".to_owned(),
            checkpoint_pieces_completed: "0".to_owned(),
            checkpoint_sync_operations_completed: "0".to_owned(),
            checkpoint_sync_service_micros: "0".to_owned(),
            checkpoint_sync_service_max_micros: "0".to_owned(),
            checkpoint_commit_service_micros: "0".to_owned(),
            checkpoint_commit_service_max_micros: "0".to_owned(),
            checkpoint_active_micros: None,
            storage_jobs_pending: "0".to_owned(),
            received_bytes_total: "0".to_owned(),
            stored_bytes_total: "0".to_owned(),
            verified_bytes_total: "0".to_owned(),
            receive_rate_bytes: "0".to_owned(),
            write_rate_bytes: "0".to_owned(),
            hash_rate_bytes: "0".to_owned(),
            write_operations_started: "0".to_owned(),
            write_operations_completed: "0".to_owned(),
            hash_operations_started: "0".to_owned(),
            hash_operations_completed: "0".to_owned(),
            write_queue_wait_micros: "0".to_owned(),
            write_queue_wait_max_micros: "0".to_owned(),
            write_service_micros: "0".to_owned(),
            write_service_max_micros: "0".to_owned(),
            hash_queue_wait_micros: "0".to_owned(),
            hash_queue_wait_max_micros: "0".to_owned(),
            hash_service_micros: "0".to_owned(),
            hash_service_max_micros: "0".to_owned(),
            pressure_transition_count: "0".to_owned(),
            backpressured_millis_total: "0".to_owned(),
            last_error: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct DiskPieceView {
    pub row_id: String,
    pub torrent_id: String,
    pub torrent_name: String,
    pub piece_index: u32,
    pub piece_length: u32,
    pub attempt: u32,
    pub stage: DiskPieceStageView,
    pub requested_bytes: String,
    pub received_bytes: String,
    pub stored_bytes: String,
    pub age_millis: String,
    pub stage_age_millis: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct TorrentView {
    pub torrent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub state: TorrentState,
    pub storage_state: StorageState,
    pub metadata_available: bool,
    pub piece_count: u32,
    pub verified_piece_count: u32,
    pub requested_bytes: String,
    pub received_bytes: String,
    pub stored_bytes: String,
    pub active_peer_connections: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configured_tracker_count: Option<u32>,
    pub payload_download_rate_bytes: String,
    pub progress: ProgressAssessment,
    pub archived: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removal_state: Option<RemovalState>,
    pub delete_managed_data_supported: bool,
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

#[derive(
    Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize, JsonSchema, TS,
)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum PeerFlagView {
    Incoming,
    Encrypted,
    DownloadAllowed,
    DownloadChoked,
    UploadAllowed,
    UploadChoked,
    ExtensionProtocol,
    MetadataExtension,
    Utp,
    HolePunched,
    OnParole,
    OptimisticUnchoke,
    Snubbed,
    UploadOnly,
    Endgame,
    Seed,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum SwarmCatalogState {
    Active,
    Inactive,
    TorrentMissing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum SwarmPeerState {
    Eligible,
    NotConnectable,
    Dialing,
    Connected,
    BackedOff,
    FailureLimited,
    Banned,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct SwarmCountsView {
    pub total: u32,
    pub eligible: u32,
    pub not_connectable: u32,
    pub dialing: u32,
    pub connected: u32,
    pub backed_off: u32,
    pub failure_limited: u32,
    pub banned: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct SwarmPeerView {
    pub peer_record_id: String,
    pub torrent_id: String,
    pub endpoint: String,
    pub sources: Vec<PeerSourceView>,
    pub state: SwarmPeerState,
    pub connectable: bool,
    pub first_observed_age_millis: String,
    pub last_observed_age_millis: String,
    pub retry_in_millis: Option<String>,
    pub dial_attempts: u32,
    pub consecutive_failures: u32,
    pub total_failures: u32,
    pub last_dial_age_millis: Option<String>,
    pub last_connected_age_millis: Option<String>,
    pub last_failure: Option<PeerDisconnectReason>,
    pub last_failure_age_millis: Option<String>,
    pub trust_points: i8,
    pub hash_failures: u8,
    pub valid_pieces: u32,
    pub on_parole: bool,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub peer_flags: Vec<PeerFlagView>,
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
        let client_name = peer.peer_id.as_ref().and_then(identify_client);
        let client_name_capability = if client_name.is_some() {
            CapabilityStatus::Available
        } else {
            CapabilityStatus::Unavailable
        };
        let mut view = Self {
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
            peer_flags: Vec::new(),
            lifecycle_age_millis: duration_millis_string(
                captured_at.saturating_sub(peer.lifecycle_changed_at),
            ),
            remote_endpoint: peer.endpoint.to_string(),
            local_endpoint: None,
            sources: peer_sources(peer.sources),
            peer_id: peer.peer_id.map(hex_peer_id),
            client_name,
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
                client_name: client_name_capability,
                ut_metadata: CapabilityStatus::Unavailable,
                interest_directions: CapabilityStatus::Unavailable,
                local_choke: CapabilityStatus::Unsupported,
                piece_availability: CapabilityStatus::Unavailable,
                protocol_rates: CapabilityStatus::Unsupported,
                upload: CapabilityStatus::Unsupported,
                metadata_stage: CapabilityStatus::Unavailable,
            },
        };
        view.peer_flags = derive_peer_flags(&view);
        view
    }
}

fn derive_peer_flags(peer: &PeerView) -> Vec<PeerFlagView> {
    let mut flags = Vec::with_capacity(6);

    if peer.direction == PeerDirection::Incoming {
        flags.push(PeerFlagView::Incoming);
    }
    if peer.local_interested == Some(true) {
        match peer.remote_choking {
            Some(false) => flags.push(PeerFlagView::DownloadAllowed),
            Some(true) => flags.push(PeerFlagView::DownloadChoked),
            None => {}
        }
    }
    if peer.remote_interested == Some(true) {
        match peer.local_choking {
            Some(false) => flags.push(PeerFlagView::UploadAllowed),
            Some(true) => flags.push(PeerFlagView::UploadChoked),
            None => {}
        }
    }
    if peer.supports_extensions == Some(true) {
        flags.push(PeerFlagView::ExtensionProtocol);
    }
    if peer.supports_ut_metadata == Some(true) {
        flags.push(PeerFlagView::MetadataExtension);
    }
    if peer.transport == PeerTransportKind::Utp {
        flags.push(PeerFlagView::Utp);
    }

    flags
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

const MAX_SWARM_RECORDS: usize = 1_000;

fn peer_failure_view(failure: PeerFailure) -> PeerDisconnectReason {
    match failure {
        PeerFailure::Connect => PeerDisconnectReason::Connect,
        PeerFailure::Handshake => PeerDisconnectReason::Handshake,
        PeerFailure::Protocol => PeerDisconnectReason::Protocol,
        PeerFailure::RemoteClosed => PeerDisconnectReason::RemoteClosed,
    }
}

fn swarm_model(
    torrent_id: &str,
    active: bool,
    snapshot: &PeerRegistrySnapshot,
) -> Result<SwarmModel, SubscriptionError> {
    if snapshot.maximum_records > MAX_SWARM_RECORDS
        || snapshot.records.len() > snapshot.maximum_records
        || snapshot.counts.total != snapshot.records.len()
    {
        return Err(SubscriptionError::Internal(
            "peer registry snapshot exceeds its bounded contract".to_owned(),
        ));
    }
    let counts_total = snapshot.counts.eligible
        + snapshot.counts.not_connectable
        + snapshot.counts.dialing
        + snapshot.counts.connected
        + snapshot.counts.banned
        + snapshot.counts.backed_off
        + snapshot.counts.failure_limited;
    if counts_total != snapshot.counts.total {
        return Err(SubscriptionError::Internal(
            "peer registry snapshot counts are inconsistent".to_owned(),
        ));
    }
    let captured_at = snapshot.captured_at;
    let peers = if active {
        snapshot
            .records
            .iter()
            .map(|record| {
                let (state, retry_in_millis) = match record.eligibility {
                    DialEligibility::Eligible => (SwarmPeerState::Eligible, None),
                    DialEligibility::NotConnectable => (SwarmPeerState::NotConnectable, None),
                    DialEligibility::Dialing => (SwarmPeerState::Dialing, None),
                    DialEligibility::Connected => (SwarmPeerState::Connected, None),
                    DialEligibility::Backoff { retry_at } => (
                        SwarmPeerState::BackedOff,
                        Some(duration_millis_string(retry_at.saturating_sub(captured_at))),
                    ),
                    DialEligibility::FailureLimit { .. } => (SwarmPeerState::FailureLimited, None),
                    DialEligibility::Banned => (SwarmPeerState::Banned, None),
                };
                let history = record.history;
                let view = SwarmPeerView {
                    peer_record_id: record.id.get().to_string(),
                    torrent_id: torrent_id.to_owned(),
                    endpoint: record.endpoint.to_string(),
                    sources: peer_sources(record.sources),
                    state,
                    connectable: record.connectable,
                    first_observed_age_millis: duration_millis_string(
                        captured_at.saturating_sub(record.first_observed_at),
                    ),
                    last_observed_age_millis: duration_millis_string(
                        captured_at.saturating_sub(record.last_observed_at),
                    ),
                    retry_in_millis,
                    dial_attempts: history.dial_attempts,
                    consecutive_failures: history.consecutive_failures,
                    total_failures: history.total_failures,
                    last_dial_age_millis: history
                        .last_dial_at
                        .map(|at| duration_millis_string(captured_at.saturating_sub(at))),
                    last_connected_age_millis: history
                        .last_connected_at
                        .map(|at| duration_millis_string(captured_at.saturating_sub(at))),
                    last_failure: history.last_failure.map(peer_failure_view),
                    last_failure_age_millis: history.last_failure.and_then(|_| {
                        history
                            .last_disconnected_at
                            .map(|at| duration_millis_string(captured_at.saturating_sub(at)))
                    }),
                    trust_points: record.integrity.trust_points,
                    hash_failures: record.integrity.hash_failures,
                    valid_pieces: record.integrity.valid_pieces,
                    on_parole: record.integrity.on_parole,
                };
                (view.peer_record_id.clone(), view)
            })
            .collect()
    } else {
        BTreeMap::new()
    };
    Ok(SwarmModel {
        state: if active {
            SwarmCatalogState::Active
        } else {
            SwarmCatalogState::Inactive
        },
        captured_millis: duration_millis_string(captured_at),
        maximum_records: bounded_u32(snapshot.maximum_records),
        counts: if active {
            SwarmCountsView {
                total: bounded_u32(snapshot.counts.total),
                eligible: bounded_u32(snapshot.counts.eligible),
                not_connectable: bounded_u32(snapshot.counts.not_connectable),
                dialing: bounded_u32(snapshot.counts.dialing),
                connected: bounded_u32(snapshot.counts.connected),
                backed_off: bounded_u32(snapshot.counts.backed_off),
                failure_limited: bounded_u32(snapshot.counts.failure_limited),
                banned: bounded_u32(snapshot.counts.banned),
            }
        } else {
            SwarmCountsView::default()
        },
        peers,
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
// UniFFI does not lower boxed record fields. These DTO variants are bounded
// transport values, not retained hot-path engine state.
#[allow(clippy::large_enum_variant)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ViewSnapshot {
    TorrentList {
        torrents: Vec<TorrentView>,
        storage: StorageSettingsSnapshot,
    },
    Torrent {
        torrent: Option<TorrentView>,
    },
    PieceActivity {
        torrent_id: String,
        piece_count: u32,
        verified: Vec<IndexRange>,
        active: Vec<ActivePiece>,
    },
    SessionDisk {
        pipeline: DiskPipelineView,
        pieces: Vec<DiskPieceView>,
    },
    SessionDht {
        inspection: DhtInspectionView,
    },
    SessionSpeed {
        history: SpeedHistoryView,
    },
    Peers {
        torrent_id: String,
        peers: Vec<PeerView>,
    },
    Swarm {
        torrent_id: String,
        state: SwarmCatalogState,
        captured_millis: String,
        maximum_records: u32,
        counts: SwarmCountsView,
        peers: Vec<SwarmPeerView>,
    },
    Files {
        torrent_id: String,
        state: FileCatalogState,
        filesystem_content_base: Option<String>,
        files: Vec<FileView>,
    },
    Trackers {
        torrent_id: String,
        state: TrackerCatalogState,
        trackers: Vec<TrackerView>,
    },
    Diagnostics {
        events: Vec<DiagnosticEvent>,
        retention: DiagnosticRetention,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
// Keep this wire enum aligned with ViewSnapshot; UniFFI cannot lower a boxed
// DiskPipelineView field.
#[allow(clippy::large_enum_variant)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ViewPatch {
    TorrentList {
        upsert: Vec<TorrentView>,
        removed: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        storage: Option<StorageSettingsSnapshot>,
    },
    Torrent {
        torrent: Option<TorrentView>,
    },
    PieceActivity {
        torrent_id: String,
        piece_count: u32,
        verified: Vec<IndexRange>,
        cleared: Vec<IndexRange>,
        active_upsert: Vec<ActivePiece>,
        active_removed: Vec<String>,
    },
    SessionDisk {
        pipeline: DiskPipelineView,
        upsert: Vec<DiskPieceView>,
        removed: Vec<String>,
    },
    SessionDht {
        inspection: DhtInspectionView,
    },
    SessionSpeed {
        history: SpeedHistoryView,
    },
    Peers {
        torrent_id: String,
        upsert: Vec<PeerView>,
        removed: Vec<String>,
    },
    Swarm {
        torrent_id: String,
        state: SwarmCatalogState,
        captured_millis: String,
        maximum_records: u32,
        counts: SwarmCountsView,
        upsert: Vec<SwarmPeerView>,
        removed: Vec<String>,
    },
    Files {
        torrent_id: String,
        upsert: Vec<FileView>,
        removed: Vec<String>,
    },
    Trackers {
        torrent_id: String,
        upsert: Vec<TrackerView>,
        removed: Vec<String>,
    },
    Diagnostics {
        events: Vec<DiagnosticEvent>,
        retention: DiagnosticRetention,
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
    pub(crate) speed_interest: Arc<Notify>,
}

#[derive(Debug)]
pub(crate) struct HubState {
    pub(crate) epoch: u64,
    pub(crate) revision: u64,
    torrents: BTreeMap<String, TorrentModel>,
    storage: StorageSettingsSnapshot,
    disk: DiskSessionModel,
    dht: DhtInspectionView,
    speed: Arc<Mutex<SessionRateHistory>>,
    diagnostics: DiagnosticStore,
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
    active: BTreeMap<u32, ActivePiece>,
    peers: BTreeMap<String, PeerView>,
    swarm: SwarmModel,
    files: Option<FileProgressModel>,
    trackers: TrackerViewModel,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SwarmModel {
    state: SwarmCatalogState,
    captured_millis: String,
    maximum_records: u32,
    counts: SwarmCountsView,
    peers: BTreeMap<String, SwarmPeerView>,
}

impl Default for SwarmModel {
    fn default() -> Self {
        Self {
            state: SwarmCatalogState::Inactive,
            captured_millis: "0".to_owned(),
            maximum_records: 1_000,
            counts: SwarmCountsView::default(),
            peers: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Default)]
struct DiskSessionModel {
    torrents: BTreeMap<String, DiskTorrentRuntime>,
}

#[derive(Clone, Debug)]
struct DiskTorrentRuntime {
    snapshot: DiskRuntimeSnapshot,
    sample_millis: u64,
    receive_rate_bytes: u64,
    write_rate_bytes: u64,
    hash_rate_bytes: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct DiskSessionView {
    pipeline: DiskPipelineView,
    pieces: BTreeMap<String, DiskPieceView>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DurableTorrentViewState {
    pub(crate) display_name: Option<String>,
    pub(crate) verified: Vec<IndexRange>,
    pub(crate) files: Option<FileProgressModel>,
    pub(crate) trackers: TrackerViewModel,
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
        Self::new_with_view_set_lease(snapshot, Duration::from_millis(VIEW_SET_LEASE_MILLIS))
    }

    pub(crate) fn new_with_view_set_lease(
        snapshot: &ServiceSnapshot,
        view_set_lease: Duration,
    ) -> Result<Self, SubscriptionError> {
        Self::new_with_speed_history(
            snapshot,
            view_set_lease,
            Arc::new(Mutex::new(SessionRateHistory::new())),
        )
    }

    pub(crate) fn new_with_speed_history(
        snapshot: &ServiceSnapshot,
        view_set_lease: Duration,
        speed: Arc<Mutex<SessionRateHistory>>,
    ) -> Result<Self, SubscriptionError> {
        Self::new_with_runtime_views(
            snapshot,
            view_set_lease,
            speed,
            DhtInspectionView::inactive(),
        )
    }

    pub(crate) fn new_with_runtime_views(
        snapshot: &ServiceSnapshot,
        view_set_lease: Duration,
        speed: Arc<Mutex<SessionRateHistory>>,
        dht: DhtInspectionView,
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
                storage: snapshot.storage.clone(),
                disk: DiskSessionModel::default(),
                dht,
                speed,
                diagnostics: DiagnosticStore::default(),
                subscribers: BTreeMap::new(),
                next_stream_id: 1,
                view_sets: BTreeMap::new(),
                view_set_lease,
            })),
            speed_interest: Arc::new(Notify::new()),
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
        self.speed_interest.notify_one();
        Ok(ViewSubscription {
            inner,
            hub: Arc::downgrade(&self.inner),
        })
    }

    pub(crate) fn speed_tick_interval(&self) -> Option<Duration> {
        let Ok(mut hub) = self.inner.lock() else {
            return None;
        };
        hub.subscribers.retain(|_, weak| weak.strong_count() != 0);
        let direct = hub
            .subscribers
            .values()
            .filter_map(Weak::upgrade)
            .filter_map(|subscriber| {
                if subscriber.spec.projection != ViewProjection::Speed {
                    return None;
                }
                match subscriber.spec.selector {
                    ViewSelector::SessionSpeed { range, .. } => range.tick_millis(),
                    _ => None,
                }
            })
            .min();
        hub.retain_live_view_sets();
        let leased = hub
            .view_sets
            .values()
            .filter_map(|view_set| {
                view_set.view_specs().ok().and_then(|specs| {
                    specs
                        .iter()
                        .filter_map(|spec| match spec {
                            crate::ViewSpec::SessionSpeed { range, .. } => range.tick_millis(),
                            _ => None,
                        })
                        .min()
                })
            })
            .min();
        direct
            .into_iter()
            .chain(leased)
            .min()
            .map(Duration::from_millis)
    }

    pub(crate) fn speed_interest_notify(&self) -> Arc<Notify> {
        self.speed_interest.clone()
    }

    pub(crate) fn publish_speed_tick(&self) -> Result<(), SubscriptionError> {
        let mut hub = self
            .inner
            .lock()
            .map_err(|_| SubscriptionError::Internal("view hub lock is poisoned".to_owned()))?;
        let now_millis = {
            let mut speed = hub
                .speed
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let now_millis = speed.now_millis();
            speed.advance_to(now_millis);
            now_millis
        };
        let revision = hub.revision;
        hub.subscribers.retain(|_, weak| weak.strong_count() != 0);
        let subscribers = hub
            .subscribers
            .values()
            .filter_map(Weak::upgrade)
            .collect::<Vec<_>>();
        for subscriber in subscribers {
            let ViewSelector::SessionSpeed { range, metrics } = &subscriber.spec.selector else {
                continue;
            };
            let history = hub
                .speed
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .view(*range, metrics, now_millis);
            subscriber.enqueue_patch(revision, ViewPatch::SessionSpeed { history })?;
        }
        hub.retain_live_view_sets();
        let view_sets = hub.view_sets.values().cloned().collect::<Vec<_>>();
        for view_set in view_sets {
            for spec in view_set.view_specs()? {
                let crate::ViewSpec::SessionSpeed { range, metrics, .. } = &spec else {
                    continue;
                };
                let history = hub
                    .speed
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .view(*range, metrics, now_millis);
                view_set.enqueue_patch(
                    spec.view_id(),
                    ViewPatch::SessionSpeed { history },
                    revision,
                )?;
            }
        }
        Ok(())
    }

    pub(crate) fn publish_dht(
        &self,
        inspection: DhtInspectionView,
    ) -> Result<(), SubscriptionError> {
        let mut hub = self
            .inner
            .lock()
            .map_err(|_| SubscriptionError::Internal("view hub lock is poisoned".to_owned()))?;
        if hub.dht == inspection {
            return Ok(());
        }
        hub.dht = inspection.clone();
        let revision = hub.revision;
        hub.subscribers.retain(|_, weak| weak.strong_count() != 0);
        let subscribers = hub
            .subscribers
            .values()
            .filter_map(Weak::upgrade)
            .collect::<Vec<_>>();
        for subscriber in subscribers {
            if matches!(subscriber.spec.selector, ViewSelector::SessionDht) {
                subscriber.enqueue_patch(
                    revision,
                    ViewPatch::SessionDht {
                        inspection: inspection.clone(),
                    },
                )?;
            }
        }
        hub.retain_live_view_sets();
        let view_sets = hub.view_sets.values().cloned().collect::<Vec<_>>();
        for view_set in view_sets {
            for spec in view_set.view_specs()? {
                if matches!(spec, crate::ViewSpec::SessionDht { .. }) {
                    view_set.enqueue_patch(
                        spec.view_id(),
                        ViewPatch::SessionDht {
                            inspection: inspection.clone(),
                        },
                        revision,
                    )?;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn replace_durable(
        &self,
        snapshot: &ServiceSnapshot,
        durable: &BTreeMap<String, DurableTorrentViewState>,
    ) -> Result<(), SubscriptionError> {
        let revision = parse_revision(&snapshot.revision)?;
        let mut hub = self
            .inner
            .lock()
            .map_err(|_| SubscriptionError::Internal("view hub lock is poisoned".to_owned()))?;
        let previous = hub.torrents.clone();
        let previous_storage = hub.storage.clone();
        let previous_disk = hub.disk.view(&hub.torrents);
        let mut next = BTreeMap::new();
        for torrent in &snapshot.torrents {
            let mut model = TorrentModel::from_snapshot(torrent);
            if let Some(state) = durable.get(&torrent.torrent_id) {
                model.view.display_name = state.display_name.clone();
                model.verified = state.verified.clone();
                model.files = state.files.clone();
                model.trackers = state.trackers.clone();
            } else if let Some(old) = previous.get(&torrent.torrent_id) {
                model.view.display_name = old.view.display_name.clone();
                model.verified = old.verified.clone();
                model.files = old.files.clone();
                model.trackers = old.trackers.clone();
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
                model.swarm = old.swarm.clone();
                if let (Some(old_files), Some(durable_files)) = (&old.files, model.files.as_ref())
                    && old_files.catalog_matches(durable_files)
                {
                    let mut reconciled = old_files.clone();
                    reconciled
                        .reconcile_verified(&durable_files.verified_piece_indices())
                        .map_err(|error| SubscriptionError::Internal(error.to_string()))?;
                    model.files = Some(reconciled);
                }
                if old.trackers.catalog_matches(&model.trackers) {
                    model.trackers = old.trackers.clone();
                }
            }
            model.view.configured_tracker_count = Some(model.trackers.count());
            next.insert(torrent.torrent_id.clone(), model);
        }
        hub.revision = revision;
        hub.torrents = next;
        hub.storage = snapshot.storage.clone();
        let current_torrent_ids = hub.torrents.keys().cloned().collect::<BTreeSet<_>>();
        hub.disk.retain(&current_torrent_ids);
        let current_disk = hub.disk.view(&hub.torrents);
        hub.publish_changes(&previous, Some(&previous_storage))?;
        if previous_disk != current_disk {
            hub.publish_disk_changes(&previous_disk, &current_disk)?;
        }
        Ok(())
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
        let Some(model) = hub.torrents.get_mut(torrent_id) else {
            return Ok(());
        };
        let previous_view = model.view.clone();
        let previous_verified = model.verified.clone();
        let previous_active = model.active.clone();
        let file_upsert = model
            .apply_activity(activity)
            .map_err(|error| SubscriptionError::Internal(error.to_string()))?;
        let next_view = model.view.clone();
        let next_verified = model.verified.clone();
        let next_active = model.active.clone();
        hub.publish_activity_changes(
            torrent_id,
            &previous_view,
            &next_view,
            &previous_verified,
            &next_verified,
            &previous_active,
            &next_active,
            &file_upsert,
        )
    }

    pub(crate) fn record_disk_runtime(
        &self,
        torrent_id: &str,
        snapshot: &DiskRuntimeSnapshot,
    ) -> Result<(), SubscriptionError> {
        let mut hub = self
            .inner
            .lock()
            .map_err(|_| SubscriptionError::Internal("view hub lock is poisoned".to_owned()))?;
        if !hub.torrents.contains_key(torrent_id) {
            return Ok(());
        }
        let previous = hub.disk.view(&hub.torrents);
        hub.disk.update(torrent_id, snapshot);
        let current = hub.disk.view(&hub.torrents);
        if previous != current {
            hub.publish_disk_changes(&previous, &current)?;
        }
        Ok(())
    }

    pub(crate) fn record_piece_runtime(
        &self,
        torrent_id: &str,
        pieces: &[DiskPieceRuntimeSnapshot],
    ) -> Result<(), SubscriptionError> {
        let mut hub = self
            .inner
            .lock()
            .map_err(|_| SubscriptionError::Internal("view hub lock is poisoned".to_owned()))?;
        let Some(model) = hub.torrents.get_mut(torrent_id) else {
            return Ok(());
        };
        let previous_view = model.view.clone();
        let previous_verified = model.verified.clone();
        let previous_active = model.active.clone();
        model.reconcile_piece_runtime(pieces);
        let next_view = model.view.clone();
        let next_verified = model.verified.clone();
        let next_active = model.active.clone();
        if previous_active != next_active {
            hub.publish_activity_changes(
                torrent_id,
                &previous_view,
                &next_view,
                &previous_verified,
                &next_verified,
                &previous_active,
                &next_active,
                &[],
            )?;
        }
        Ok(())
    }

    pub(crate) fn clear_disk_runtime(&self, torrent_id: &str) -> Result<(), SubscriptionError> {
        let mut hub = self
            .inner
            .lock()
            .map_err(|_| SubscriptionError::Internal("view hub lock is poisoned".to_owned()))?;
        let previous = hub.disk.view(&hub.torrents);
        hub.disk.torrents.remove(torrent_id);
        let current = hub.disk.view(&hub.torrents);
        if previous != current {
            hub.publish_disk_changes(&previous, &current)?;
        }
        Ok(())
    }

    pub(crate) fn clear_piece_runtime(&self, torrent_id: &str) -> Result<(), SubscriptionError> {
        self.record_piece_runtime(torrent_id, &[])
    }

    pub(crate) fn record_pieces_durable(
        &self,
        torrent_id: &str,
        piece_indices: &[u32],
        revision: u64,
    ) -> Result<(), SubscriptionError> {
        if piece_indices.is_empty() {
            return Ok(());
        }
        let mut hub = self
            .inner
            .lock()
            .map_err(|_| SubscriptionError::Internal("view hub lock is poisoned".to_owned()))?;
        let Some(model) = hub.torrents.get_mut(torrent_id) else {
            return Ok(());
        };
        let previous_view = model.view.clone();
        let previous_verified = model.verified.clone();
        let previous_active = model.active.clone();
        let mut next = model.clone();
        let mut file_upsert = BTreeMap::new();
        for &piece_index in piece_indices {
            if piece_index >= next.view.piece_count {
                return Err(SubscriptionError::Internal(format!(
                    "durable piece {piece_index} is outside {} pieces",
                    next.view.piece_count
                )));
            }
            insert_range(&mut next.verified, piece_index, 1);
            if let Some(files) = next.files.as_mut() {
                for file in files
                    .piece_verified(piece_index)
                    .map_err(|error| SubscriptionError::Internal(error.to_string()))?
                {
                    file_upsert.insert(file.file_id.clone(), file);
                }
            }
        }
        next.view.verified_piece_count =
            range_cardinality(&next.verified).min(u64::from(u32::MAX)) as u32;
        next.snapshot.verified_piece_count = next.view.verified_piece_count;
        next.view.storage_state = StorageState::Staging;
        next.snapshot.storage_state = StorageState::Staging;
        let next_view = next.view.clone();
        let next_verified = next.verified.clone();
        let next_active = next.active.clone();
        *model = next;
        hub.revision = revision;
        hub.publish_activity_changes(
            torrent_id,
            &previous_view,
            &next_view,
            &previous_verified,
            &next_verified,
            &previous_active,
            &next_active,
            &file_upsert.into_values().collect::<Vec<_>>(),
        )
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
        hub.publish_changes(&previous, None)
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
        hub.publish_changes(&previous, None)
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

    pub(crate) fn record_peer_registry_state(
        &self,
        torrent_id: &str,
        active: bool,
        snapshot: &PeerRegistrySnapshot,
    ) -> Result<(), SubscriptionError> {
        let mut hub = self
            .inner
            .lock()
            .map_err(|_| SubscriptionError::Internal("view hub lock is poisoned".to_owned()))?;
        let Some(model) = hub.torrents.get_mut(torrent_id) else {
            return Ok(());
        };
        let previous = model.swarm.clone();
        model.swarm = swarm_model(torrent_id, active, snapshot)?;
        let current = model.swarm.clone();
        hub.publish_swarm_changes(torrent_id, &previous, &current)
    }

    pub(crate) fn record_tracker_state(
        &self,
        torrent_id: &str,
        snapshot: &TrackerRuntimeSnapshot,
    ) -> Result<(), SubscriptionError> {
        let mut hub = self
            .inner
            .lock()
            .map_err(|_| SubscriptionError::Internal("view hub lock is poisoned".to_owned()))?;
        let Some(model) = hub.torrents.get_mut(torrent_id) else {
            return Ok(());
        };
        let previous_view = model.view.clone();
        let previous = model.trackers.row_map().clone();
        model.trackers.apply_snapshot(snapshot);
        model.view.configured_tracker_count = Some(model.trackers.count());
        let next_view = model.view.clone();
        let current = model.trackers.row_map().clone();
        hub.publish_tracker_changes(torrent_id, &previous_view, &next_view, &previous, &current)
    }

    pub fn record_diagnostic(
        &self,
        severity: DiagnosticSeverity,
        category: &str,
        code: &str,
        torrent_id: Option<&str>,
        message: &str,
        context: &[(&str, &str)],
    ) -> Result<(), SubscriptionError> {
        let category = DiagnosticCategory::new(category)
            .ok_or_else(|| SubscriptionError::Internal("invalid diagnostic category".to_owned()))?;
        self.record_structured_diagnostic(DiagnosticDraft {
            severity,
            category,
            code: code.to_owned(),
            torrent_id: torrent_id.map(ToOwned::to_owned),
            message: message.to_owned(),
            subjects: Vec::new(),
            fields: context
                .iter()
                .map(|(key, value)| DiagnosticField::text(*key, *value))
                .collect(),
        })
    }

    pub(crate) fn record_structured_diagnostic(
        &self,
        draft: DiagnosticDraft,
    ) -> Result<(), SubscriptionError> {
        let mut hub = self
            .inner
            .lock()
            .map_err(|_| SubscriptionError::Internal("view hub lock is poisoned".to_owned()))?;
        if !hub.diagnostic_enabled(draft.severity, &draft.category, draft.torrent_id.as_deref())? {
            return Ok(());
        }
        let timestamp_millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let event = hub.diagnostics.record(draft, timestamp_millis);
        hub.publish_diagnostic(event)
    }

    pub(crate) fn record_diagnostic_lazy<F>(
        &self,
        severity: DiagnosticSeverity,
        category: &'static str,
        torrent_id: Option<&str>,
        build: F,
    ) -> Result<(), SubscriptionError>
    where
        F: FnOnce() -> DiagnosticDraft,
    {
        let category = DiagnosticCategory::from_static(category);
        let enabled = {
            let mut hub = self
                .inner
                .lock()
                .map_err(|_| SubscriptionError::Internal("view hub lock is poisoned".to_owned()))?;
            hub.diagnostic_enabled(severity, &category, torrent_id)?
        };
        if !enabled {
            return Ok(());
        }
        self.record_structured_diagnostic(build())
    }
}

impl HubState {
    fn diagnostic_enabled(
        &mut self,
        severity: DiagnosticSeverity,
        category: &DiagnosticCategory,
        torrent_id: Option<&str>,
    ) -> Result<bool, SubscriptionError> {
        if interest_matches(
            &DiagnosticFilter::default(),
            None,
            severity,
            category,
            torrent_id,
        ) {
            return Ok(true);
        }
        self.subscribers.retain(|_, weak| weak.strong_count() != 0);
        if self
            .subscribers
            .values()
            .filter_map(Weak::upgrade)
            .any(|subscriber| {
                let filter = subscriber.spec.diagnostics.clone().unwrap_or_default();
                subscriber.spec.projection == ViewProjection::Diagnostics
                    && interest_matches(
                        &filter,
                        selector_torrent_id(&subscriber.spec.selector),
                        severity,
                        category,
                        torrent_id,
                    )
            })
        {
            return Ok(true);
        }
        self.retain_live_view_sets();
        for view_set in self.view_sets.values() {
            for spec in view_set.view_specs()? {
                let subscription = spec.subscription_spec(DEFAULT_VIEW_SET_QUEUE_BYTES);
                if subscription.projection == ViewProjection::Diagnostics {
                    let filter = subscription.diagnostics.clone().unwrap_or_default();
                    if interest_matches(
                        &filter,
                        selector_torrent_id(&subscription.selector),
                        severity,
                        category,
                        torrent_id,
                    ) {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }

    pub(crate) fn snapshot_for(&self, spec: &SubscriptionSpec) -> ViewSnapshot {
        match (&spec.selector, spec.projection) {
            (ViewSelector::TorrentList, ViewProjection::Summary) => ViewSnapshot::TorrentList {
                torrents: self
                    .torrents
                    .values()
                    .map(|torrent| torrent.view.clone())
                    .collect(),
                storage: self.storage.clone(),
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
                    active: torrent.map_or_else(Vec::new, |torrent| {
                        torrent.active.values().cloned().collect()
                    }),
                }
            }
            (ViewSelector::TorrentList, ViewProjection::Disk) => {
                let disk = self.disk.view(&self.torrents);
                ViewSnapshot::SessionDisk {
                    pipeline: disk.pipeline,
                    pieces: disk.pieces.into_values().collect(),
                }
            }
            (ViewSelector::SessionDht, ViewProjection::Dht) => ViewSnapshot::SessionDht {
                inspection: self.dht.clone(),
            },
            (ViewSelector::SessionSpeed { range, metrics }, ViewProjection::Speed) => {
                let mut history = self
                    .speed
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let now_millis = history.now_millis();
                ViewSnapshot::SessionSpeed {
                    history: history.view(*range, metrics, now_millis),
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
            (ViewSelector::Torrent { torrent_id }, ViewProjection::Swarm) => {
                let swarm = self.torrents.get(torrent_id).map(|torrent| &torrent.swarm);
                ViewSnapshot::Swarm {
                    torrent_id: torrent_id.clone(),
                    state: swarm.map_or(SwarmCatalogState::TorrentMissing, |swarm| swarm.state),
                    captured_millis: swarm
                        .map_or_else(|| "0".to_owned(), |swarm| swarm.captured_millis.clone()),
                    maximum_records: swarm.map_or(1_000, |swarm| swarm.maximum_records),
                    counts: swarm
                        .map_or_else(SwarmCountsView::default, |swarm| swarm.counts.clone()),
                    peers: swarm
                        .map_or_else(Vec::new, |swarm| swarm.peers.values().cloned().collect()),
                }
            }
            (ViewSelector::Torrent { torrent_id }, ViewProjection::Files) => {
                match self.torrents.get(torrent_id) {
                    Some(torrent) => ViewSnapshot::Files {
                        torrent_id: torrent_id.clone(),
                        state: if torrent.files.is_some() {
                            FileCatalogState::Available
                        } else {
                            FileCatalogState::MetadataPending
                        },
                        filesystem_content_base: torrent
                            .files
                            .as_ref()
                            .and_then(FileProgressModel::filesystem_content_base)
                            .map(str::to_owned),
                        files: torrent
                            .files
                            .as_ref()
                            .map_or_else(Vec::new, FileProgressModel::rows),
                    },
                    None => ViewSnapshot::Files {
                        torrent_id: torrent_id.clone(),
                        state: FileCatalogState::TorrentMissing,
                        filesystem_content_base: None,
                        files: Vec::new(),
                    },
                }
            }
            (ViewSelector::Torrent { torrent_id }, ViewProjection::Trackers) => {
                match self.torrents.get(torrent_id) {
                    Some(torrent) => ViewSnapshot::Trackers {
                        torrent_id: torrent_id.clone(),
                        state: TrackerCatalogState::Available,
                        trackers: torrent.trackers.rows(),
                    },
                    None => ViewSnapshot::Trackers {
                        torrent_id: torrent_id.clone(),
                        state: TrackerCatalogState::TorrentMissing,
                        trackers: Vec::new(),
                    },
                }
            }
            (selector, ViewProjection::Diagnostics) => {
                let filter = spec.diagnostics.clone().unwrap_or_default();
                let torrent_id = selector_torrent_id(selector);
                ViewSnapshot::Diagnostics {
                    events: self.diagnostics.matching(&filter, torrent_id),
                    retention: self.diagnostics.retention(),
                }
            }
            (
                ViewSelector::TorrentList,
                ViewProjection::PieceActivity
                | ViewProjection::Dht
                | ViewProjection::Speed
                | ViewProjection::Peers
                | ViewProjection::Swarm
                | ViewProjection::Files
                | ViewProjection::Trackers,
            ) => {
                unreachable!("invalid projection is rejected before snapshot construction")
            }
            (ViewSelector::Torrent { .. }, ViewProjection::Disk | ViewProjection::Dht) => {
                unreachable!("invalid projection is rejected before snapshot construction")
            }
            (ViewSelector::Torrent { .. }, ViewProjection::Speed) => {
                unreachable!("invalid projection is rejected before snapshot construction")
            }
            (ViewSelector::SessionSpeed { .. }, _) => {
                unreachable!("invalid projection is rejected before snapshot construction")
            }
            (ViewSelector::SessionDht, _) => {
                unreachable!("invalid projection is rejected before snapshot construction")
            }
        }
    }

    fn publish_changes(
        &mut self,
        previous: &BTreeMap<String, TorrentModel>,
        previous_storage: Option<&StorageSettingsSnapshot>,
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
            if projection_requires_snapshot(&subscriber.spec, previous, current) {
                subscriber.enqueue_snapshot(revision, self.snapshot_for(&subscriber.spec))?;
                continue;
            }
            let patch = patch_for(
                &subscriber.spec,
                previous,
                current,
                previous_storage.filter(|storage| *storage != &self.storage),
                &self.storage,
            );
            if let Some(patch) = patch {
                subscriber.enqueue_patch(revision, patch)?;
            }
        }
        let view_sets = self.view_sets.values().cloned().collect::<Vec<_>>();
        for view_set in view_sets {
            for spec in view_set.view_specs()? {
                let subscription = spec.subscription_spec(DEFAULT_VIEW_SET_QUEUE_BYTES);
                if projection_requires_snapshot(&subscription, previous, current) {
                    view_set.enqueue_snapshot(
                        spec.view_id(),
                        self.snapshot_for(&subscription),
                        revision,
                    )?;
                    continue;
                }
                if let Some(patch) = patch_for(
                    &subscription,
                    previous,
                    current,
                    previous_storage.filter(|storage| *storage != &self.storage),
                    &self.storage,
                ) {
                    view_set.enqueue_patch(spec.view_id(), patch, revision)?;
                }
            }
        }
        Ok(())
    }

    fn publish_disk_changes(
        &mut self,
        previous: &DiskSessionView,
        current: &DiskSessionView,
    ) -> Result<(), SubscriptionError> {
        let Some(patch) = disk_patch(previous, current) else {
            return Ok(());
        };
        let revision = self.revision;
        self.subscribers.retain(|_, weak| weak.strong_count() != 0);
        let subscribers = self
            .subscribers
            .values()
            .filter_map(Weak::upgrade)
            .collect::<Vec<_>>();
        for subscriber in subscribers {
            if subscriber.spec.projection == ViewProjection::Disk {
                subscriber.enqueue_patch(revision, patch.clone())?;
            }
        }
        self.retain_live_view_sets();
        let view_sets = self.view_sets.values().cloned().collect::<Vec<_>>();
        for view_set in view_sets {
            for spec in view_set.view_specs()? {
                if matches!(spec, crate::ViewSpec::SessionDisk { .. }) {
                    view_set.enqueue_patch(spec.view_id(), patch.clone(), revision)?;
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn publish_activity_changes(
        &mut self,
        torrent_id: &str,
        previous_view: &TorrentView,
        next_view: &TorrentView,
        previous_verified: &[IndexRange],
        next_verified: &[IndexRange],
        previous_active: &BTreeMap<u32, ActivePiece>,
        next_active: &BTreeMap<u32, ActivePiece>,
        file_upsert: &[FileView],
    ) -> Result<(), SubscriptionError> {
        let revision = self.revision;
        self.subscribers.retain(|_, weak| weak.strong_count() != 0);
        let subscribers = self
            .subscribers
            .values()
            .filter_map(Weak::upgrade)
            .collect::<Vec<_>>();
        for subscriber in subscribers {
            if let Some(patch) = targeted_activity_patch(
                &subscriber.spec,
                torrent_id,
                previous_view,
                next_view,
                previous_verified,
                next_verified,
                previous_active,
                next_active,
                file_upsert,
            ) {
                subscriber.enqueue_patch(revision, patch)?;
            }
        }
        self.retain_live_view_sets();
        let view_sets = self.view_sets.values().cloned().collect::<Vec<_>>();
        for view_set in view_sets {
            for spec in view_set.view_specs()? {
                let subscription = spec.subscription_spec(DEFAULT_VIEW_SET_QUEUE_BYTES);
                if let Some(patch) = targeted_activity_patch(
                    &subscription,
                    torrent_id,
                    previous_view,
                    next_view,
                    previous_verified,
                    next_verified,
                    previous_active,
                    next_active,
                    file_upsert,
                ) {
                    view_set.enqueue_patch(spec.view_id(), patch, revision)?;
                }
            }
        }
        Ok(())
    }

    fn publish_diagnostic(&mut self, event: DiagnosticEvent) -> Result<(), SubscriptionError> {
        let revision = self.revision;
        let retention = self.diagnostics.retention();
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
            if diagnostic_matches(
                &filter,
                selector_torrent_id(&subscriber.spec.selector),
                &event,
            ) {
                subscriber.enqueue_diagnostic_patch(
                    revision,
                    ViewPatch::Diagnostics {
                        events: vec![event.clone()],
                        retention: retention.clone(),
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
                if diagnostic_matches(&filter, selector_torrent_id(&subscription.selector), &event)
                {
                    view_set.enqueue_patch(
                        spec.view_id(),
                        ViewPatch::Diagnostics {
                            events: vec![event.clone()],
                            retention: retention.clone(),
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

    fn publish_swarm_changes(
        &mut self,
        torrent_id: &str,
        previous: &SwarmModel,
        current: &SwarmModel,
    ) -> Result<(), SubscriptionError> {
        if previous == current {
            return Ok(());
        }
        let revision = self.revision;
        self.subscribers.retain(|_, weak| weak.strong_count() != 0);
        let subscribers = self
            .subscribers
            .values()
            .filter_map(Weak::upgrade)
            .collect::<Vec<_>>();
        for subscriber in subscribers {
            if let Some(patch) =
                targeted_swarm_patch(&subscriber.spec, torrent_id, previous, current)
            {
                subscriber.enqueue_patch(revision, patch)?;
            }
        }
        self.retain_live_view_sets();
        let view_sets = self.view_sets.values().cloned().collect::<Vec<_>>();
        for view_set in view_sets {
            for spec in view_set.view_specs()? {
                if let Some(patch) = targeted_swarm_patch(
                    &spec.subscription_spec(DEFAULT_VIEW_SET_QUEUE_BYTES),
                    torrent_id,
                    previous,
                    current,
                ) {
                    view_set.enqueue_patch(spec.view_id(), patch, revision)?;
                }
            }
        }
        Ok(())
    }

    fn publish_tracker_changes(
        &mut self,
        torrent_id: &str,
        previous_view: &TorrentView,
        next_view: &TorrentView,
        previous: &BTreeMap<String, TrackerView>,
        current: &BTreeMap<String, TrackerView>,
    ) -> Result<(), SubscriptionError> {
        let revision = self.revision;
        self.subscribers.retain(|_, weak| weak.strong_count() != 0);
        let subscribers = self
            .subscribers
            .values()
            .filter_map(Weak::upgrade)
            .collect::<Vec<_>>();
        for subscriber in subscribers {
            if let Some(patch) = targeted_tracker_patch(
                &subscriber.spec,
                torrent_id,
                previous_view,
                next_view,
                previous,
                current,
            ) {
                subscriber.enqueue_patch(revision, patch)?;
            }
        }
        self.retain_live_view_sets();
        let view_sets = self.view_sets.values().cloned().collect::<Vec<_>>();
        for view_set in view_sets {
            for spec in view_set.view_specs()? {
                let subscription = spec.subscription_spec(DEFAULT_VIEW_SET_QUEUE_BYTES);
                if let Some(patch) = targeted_tracker_patch(
                    &subscription,
                    torrent_id,
                    previous_view,
                    next_view,
                    previous,
                    current,
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
                display_name: None,
                state: snapshot.state,
                storage_state: snapshot.storage_state,
                metadata_available: snapshot.metadata_available,
                piece_count: snapshot.piece_count,
                verified_piece_count: snapshot.verified_piece_count,
                requested_bytes: "0".to_owned(),
                received_bytes: "0".to_owned(),
                stored_bytes: "0".to_owned(),
                active_peer_connections: 0,
                configured_tracker_count: None,
                payload_download_rate_bytes: "0".to_owned(),
                progress: assess_progress(snapshot, progress_inputs),
                archived: snapshot.archived,
                removal_state: snapshot.removal_state,
                delete_managed_data_supported: snapshot.delete_managed_data_supported,
                error: snapshot.error.clone(),
            },
            snapshot: snapshot.clone(),
            progress_inputs,
            verified: Vec::new(),
            active: BTreeMap::new(),
            peers: BTreeMap::new(),
            swarm: SwarmModel::default(),
            files: None,
            trackers: TrackerViewModel::default(),
        }
    }

    fn apply_activity(
        &mut self,
        activity: TorrentActivity,
    ) -> Result<Vec<FileView>, crate::file_views::FileProgressError> {
        let mut file_upsert = Vec::new();
        match activity {
            TorrentActivity::PieceStarted {
                piece_index,
                piece_length,
                attempt,
            } => {
                self.active
                    .entry(piece_index)
                    .or_insert_with(|| ActivePiece {
                        piece_id: active_piece_id(piece_index, attempt),
                        piece_index,
                        attempt,
                        piece_length,
                        stage: ActivePieceStageView::Requested,
                        requested: Vec::new(),
                        received: Vec::new(),
                        stored: Vec::new(),
                        age_millis: "0".to_owned(),
                        error: None,
                    });
            }
            TorrentActivity::BlockRequested {
                piece_index,
                begin,
                length,
            } => {
                add_counter(&mut self.view.requested_bytes, u64::from(length));
                if let Some(active) = self.active.get_mut(&piece_index) {
                    insert_range(&mut active.requested, begin, length);
                    active.stage = ActivePieceStageView::Requested;
                }
            }
            TorrentActivity::BlockReceived {
                piece_index,
                begin,
                length,
            } => {
                add_counter(&mut self.view.received_bytes, u64::from(length));
                if let Some(active) = self.active.get_mut(&piece_index) {
                    remove_range(&mut active.requested, begin, length);
                    insert_range(&mut active.received, begin, length);
                    active.stage = ActivePieceStageView::Received;
                }
            }
            TorrentActivity::BlockStored {
                piece_index,
                begin,
                length,
            } => {
                add_counter(&mut self.view.stored_bytes, u64::from(length));
                if let Some(active) = self.active.get_mut(&piece_index) {
                    remove_range(&mut active.received, begin, length);
                    insert_range(&mut active.stored, begin, length);
                    active.stage =
                        if range_cardinality(&active.stored) >= u64::from(active.piece_length) {
                            ActivePieceStageView::Stored
                        } else {
                            ActivePieceStageView::Received
                        };
                }
                if let Some(files) = &mut self.files {
                    file_upsert = files.stored_block(piece_index, begin, length)?;
                }
            }
            TorrentActivity::PieceVerified { piece_index } => {
                insert_range(&mut self.verified, piece_index, 1);
                self.view.verified_piece_count =
                    range_cardinality(&self.verified).min(u64::from(u32::MAX)) as u32;
                self.active.remove(&piece_index);
                if let Some(files) = &mut self.files {
                    file_upsert = files.piece_verified(piece_index)?;
                }
            }
            TorrentActivity::PieceHashFailed { piece_index } => {
                if let Some(active) = self.active.get_mut(&piece_index) {
                    active.requested.clear();
                    active.received.clear();
                    active.stored.clear();
                    active.stage = ActivePieceStageView::Failed;
                    active.error = Some("Piece hash failed; retrying".to_owned());
                }
                if let Some(files) = &mut self.files {
                    file_upsert = files.piece_hash_failed(piece_index)?;
                }
            }
            TorrentActivity::PieceHashing { piece_index } => {
                if let Some(active) = self.active.get_mut(&piece_index) {
                    active.stage = ActivePieceStageView::Hashing;
                }
            }
        }
        Ok(file_upsert)
    }

    fn reconcile_piece_runtime(&mut self, pieces: &[DiskPieceRuntimeSnapshot]) {
        let mut retained = BTreeSet::new();
        for runtime in pieces {
            if runtime.piece_index >= self.view.piece_count {
                continue;
            }
            retained.insert(runtime.piece_index);
            let active = self
                .active
                .entry(runtime.piece_index)
                .or_insert_with(|| active_piece_from_runtime(runtime));
            if active.attempt != runtime.attempt {
                *active = active_piece_from_runtime(runtime);
            } else {
                active.piece_length = runtime.piece_length;
                active.stage = active_stage_from_runtime(runtime);
                active.age_millis = runtime.age_millis.to_string();
                active.error = runtime.error.clone();
            }
        }
        self.active
            .retain(|piece_index, _| retained.contains(piece_index));
    }
}

impl DiskSessionModel {
    fn update(&mut self, torrent_id: &str, snapshot: &DiskRuntimeSnapshot) {
        let (sample_millis, receive_rate_bytes, write_rate_bytes, hash_rate_bytes) = self
            .torrents
            .get(torrent_id)
            .and_then(|previous| {
                let elapsed = snapshot
                    .captured_at_millis
                    .checked_sub(previous.snapshot.captured_at_millis)?;
                (elapsed != 0).then(|| {
                    (
                        elapsed,
                        sampled_rate(
                            snapshot.received_bytes_total,
                            previous.snapshot.received_bytes_total,
                            elapsed,
                        ),
                        sampled_rate(
                            snapshot.stored_bytes_total,
                            previous.snapshot.stored_bytes_total,
                            elapsed,
                        ),
                        sampled_rate(
                            snapshot.verified_bytes_total,
                            previous.snapshot.verified_bytes_total,
                            elapsed,
                        ),
                    )
                })
            })
            .unwrap_or_default();
        self.torrents.insert(
            torrent_id.to_owned(),
            DiskTorrentRuntime {
                snapshot: snapshot.clone(),
                sample_millis,
                receive_rate_bytes,
                write_rate_bytes,
                hash_rate_bytes,
            },
        );
    }

    fn retain(&mut self, torrent_ids: &BTreeSet<String>) {
        self.torrents
            .retain(|torrent_id, _| torrent_ids.contains(torrent_id));
    }

    fn view(&self, torrents: &BTreeMap<String, TorrentModel>) -> DiskSessionView {
        let mut view = DiskSessionView::default();
        let mut pressure_rank = 0_u8;
        let mut checkpoint_rank = 0_u8;
        for (torrent_id, runtime) in &self.torrents {
            let snapshot = &runtime.snapshot;
            let rank = disk_pressure_rank(snapshot.pressure);
            if rank >= pressure_rank {
                pressure_rank = rank;
                view.pipeline.pressure = map_disk_pressure(snapshot.pressure);
            }
            let rank = disk_checkpoint_rank(snapshot.checkpoint_stage);
            if rank >= checkpoint_rank {
                checkpoint_rank = rank;
                view.pipeline.checkpoint_stage =
                    map_disk_checkpoint_stage(snapshot.checkpoint_stage);
            }
            view.pipeline.intake_backpressured |= snapshot.intake_backpressured;
            view.pipeline.sample_millis =
                max_decimal(&view.pipeline.sample_millis, runtime.sample_millis);
            add_decimal(
                &mut view.pipeline.resident_limit_bytes,
                usize_to_u64(snapshot.resident_limit_bytes),
            );
            add_decimal(
                &mut view.pipeline.resident_high_watermark_bytes,
                usize_to_u64(snapshot.resident_high_watermark_bytes),
            );
            add_decimal(
                &mut view.pipeline.resident_low_watermark_bytes,
                usize_to_u64(snapshot.resident_low_watermark_bytes),
            );
            add_decimal(
                &mut view.pipeline.requested_bytes,
                usize_to_u64(snapshot.requested_bytes),
            );
            add_decimal(
                &mut view.pipeline.resident_bytes,
                usize_to_u64(snapshot.resident_bytes),
            );
            add_decimal(
                &mut view.pipeline.queued_write_bytes,
                usize_to_u64(snapshot.queued_write_bytes),
            );
            add_decimal(
                &mut view.pipeline.writing_bytes,
                usize_to_u64(snapshot.writing_bytes),
            );
            add_decimal(
                &mut view.pipeline.hashing_bytes,
                usize_to_u64(snapshot.hashing_bytes),
            );
            add_decimal(
                &mut view.pipeline.checkpoint_dirty_pieces,
                usize_to_u64(snapshot.checkpoint_dirty_pieces),
            );
            add_decimal(
                &mut view.pipeline.checkpoint_dirty_bytes,
                usize_to_u64(snapshot.checkpoint_dirty_bytes),
            );
            add_decimal(
                &mut view.pipeline.checkpoint_dirty_piece_high_water,
                usize_to_u64(snapshot.checkpoint_dirty_piece_high_water),
            );
            add_decimal(
                &mut view.pipeline.checkpoint_dirty_byte_high_water,
                usize_to_u64(snapshot.checkpoint_dirty_byte_high_water),
            );
            view.pipeline.checkpoint_oldest_dirty_millis = max_decimal(
                &view.pipeline.checkpoint_oldest_dirty_millis,
                snapshot.checkpoint_oldest_dirty_millis,
            );
            add_decimal(
                &mut view.pipeline.checkpoint_batches_started,
                usize_to_u64(snapshot.checkpoint_batches_started),
            );
            add_decimal(
                &mut view.pipeline.checkpoint_batches_completed,
                usize_to_u64(snapshot.checkpoint_batches_completed),
            );
            add_decimal(
                &mut view.pipeline.checkpoint_pieces_completed,
                usize_to_u64(snapshot.checkpoint_pieces_completed),
            );
            add_decimal(
                &mut view.pipeline.checkpoint_sync_operations_completed,
                usize_to_u64(snapshot.checkpoint_sync_operations_completed),
            );
            add_decimal(
                &mut view.pipeline.checkpoint_sync_service_micros,
                snapshot.checkpoint_sync_service_micros,
            );
            view.pipeline.checkpoint_sync_service_max_micros = max_decimal(
                &view.pipeline.checkpoint_sync_service_max_micros,
                snapshot.checkpoint_sync_service_max_micros,
            );
            add_decimal(
                &mut view.pipeline.checkpoint_commit_service_micros,
                snapshot.checkpoint_commit_service_micros,
            );
            view.pipeline.checkpoint_commit_service_max_micros = max_decimal(
                &view.pipeline.checkpoint_commit_service_max_micros,
                snapshot.checkpoint_commit_service_max_micros,
            );
            if let Some(active) = snapshot.checkpoint_active_micros {
                view.pipeline.checkpoint_active_micros = Some(max_decimal(
                    view.pipeline
                        .checkpoint_active_micros
                        .as_deref()
                        .unwrap_or("0"),
                    active,
                ));
            }
            add_decimal(
                &mut view.pipeline.storage_jobs_pending,
                usize_to_u64(snapshot.storage_jobs_pending),
            );
            add_decimal(
                &mut view.pipeline.received_bytes_total,
                usize_to_u64(snapshot.received_bytes_total),
            );
            add_decimal(
                &mut view.pipeline.stored_bytes_total,
                usize_to_u64(snapshot.stored_bytes_total),
            );
            add_decimal(
                &mut view.pipeline.verified_bytes_total,
                usize_to_u64(snapshot.verified_bytes_total),
            );
            add_decimal(
                &mut view.pipeline.receive_rate_bytes,
                runtime.receive_rate_bytes,
            );
            add_decimal(
                &mut view.pipeline.write_rate_bytes,
                runtime.write_rate_bytes,
            );
            add_decimal(&mut view.pipeline.hash_rate_bytes, runtime.hash_rate_bytes);
            add_decimal(
                &mut view.pipeline.write_operations_started,
                usize_to_u64(snapshot.write_operations_started),
            );
            add_decimal(
                &mut view.pipeline.write_operations_completed,
                usize_to_u64(snapshot.write_operations_completed),
            );
            add_decimal(
                &mut view.pipeline.hash_operations_started,
                usize_to_u64(snapshot.hash_operations_started),
            );
            add_decimal(
                &mut view.pipeline.hash_operations_completed,
                usize_to_u64(snapshot.hash_operations_completed),
            );
            add_decimal(
                &mut view.pipeline.write_queue_wait_micros,
                snapshot.write_queue_wait_micros,
            );
            view.pipeline.write_queue_wait_max_micros = max_decimal(
                &view.pipeline.write_queue_wait_max_micros,
                snapshot.write_queue_wait_max_micros,
            );
            add_decimal(
                &mut view.pipeline.write_service_micros,
                snapshot.write_service_micros,
            );
            view.pipeline.write_service_max_micros = max_decimal(
                &view.pipeline.write_service_max_micros,
                snapshot.write_service_max_micros,
            );
            add_decimal(
                &mut view.pipeline.hash_queue_wait_micros,
                snapshot.hash_queue_wait_micros,
            );
            view.pipeline.hash_queue_wait_max_micros = max_decimal(
                &view.pipeline.hash_queue_wait_max_micros,
                snapshot.hash_queue_wait_max_micros,
            );
            add_decimal(
                &mut view.pipeline.hash_service_micros,
                snapshot.hash_service_micros,
            );
            view.pipeline.hash_service_max_micros = max_decimal(
                &view.pipeline.hash_service_max_micros,
                snapshot.hash_service_max_micros,
            );
            add_decimal(
                &mut view.pipeline.pressure_transition_count,
                snapshot.pressure_transition_count,
            );
            add_decimal(
                &mut view.pipeline.backpressured_millis_total,
                snapshot.backpressured_millis_total,
            );
            if snapshot.last_error.is_some() {
                view.pipeline.last_error = snapshot.last_error.clone();
            }

            let torrent_name = torrents
                .get(torrent_id)
                .and_then(|torrent| torrent.view.display_name.clone())
                .unwrap_or_else(|| format!("Torrent {}", &torrent_id[..torrent_id.len().min(12)]));
            for piece in &snapshot.pieces {
                let row_id = format!("{torrent_id}:{}:{}", piece.piece_index, piece.attempt);
                view.pieces.insert(
                    row_id.clone(),
                    DiskPieceView {
                        row_id,
                        torrent_id: torrent_id.clone(),
                        torrent_name: torrent_name.clone(),
                        piece_index: piece.piece_index,
                        piece_length: piece.piece_length,
                        attempt: piece.attempt,
                        stage: map_disk_piece_stage(piece.stage),
                        requested_bytes: piece.requested_bytes.to_string(),
                        received_bytes: piece.received_bytes.to_string(),
                        stored_bytes: piece.stored_bytes.to_string(),
                        age_millis: piece.age_millis.to_string(),
                        stage_age_millis: piece.stage_age_millis.to_string(),
                        error: piece.error.clone(),
                    },
                );
            }
        }
        view
    }
}

fn sampled_rate(current: usize, previous: usize, elapsed_millis: u64) -> u64 {
    let bytes = current.saturating_sub(previous) as u128;
    let rate = bytes
        .saturating_mul(1_000)
        .checked_div(u128::from(elapsed_millis))
        .unwrap_or_default();
    u64::try_from(rate).unwrap_or(u64::MAX)
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn add_decimal(value: &mut String, amount: u64) {
    let current = value.parse::<u64>().unwrap_or_default();
    *value = current.saturating_add(amount).to_string();
}

fn max_decimal(value: &str, candidate: u64) -> String {
    value
        .parse::<u64>()
        .unwrap_or_default()
        .max(candidate)
        .to_string()
}

const fn disk_pressure_rank(pressure: DiskPressure) -> u8 {
    match pressure {
        DiskPressure::Idle => 0,
        DiskPressure::Normal => 1,
        DiskPressure::Draining => 2,
        DiskPressure::Backpressured => 3,
        DiskPressure::Error => 4,
    }
}

const fn disk_checkpoint_rank(stage: DiskCheckpointStage) -> u8 {
    match stage {
        DiskCheckpointStage::Idle => 0,
        DiskCheckpointStage::Syncing => 1,
        DiskCheckpointStage::Committing => 2,
        DiskCheckpointStage::Error => 3,
    }
}

const fn map_disk_pressure(pressure: DiskPressure) -> DiskPressureView {
    match pressure {
        DiskPressure::Idle => DiskPressureView::Idle,
        DiskPressure::Normal => DiskPressureView::Normal,
        DiskPressure::Backpressured => DiskPressureView::Backpressured,
        DiskPressure::Draining => DiskPressureView::Draining,
        DiskPressure::Error => DiskPressureView::Error,
    }
}

const fn map_disk_piece_stage(stage: DiskPieceStage) -> DiskPieceStageView {
    match stage {
        DiskPieceStage::Receiving => DiskPieceStageView::Receiving,
        DiskPieceStage::Queued => DiskPieceStageView::Queued,
        DiskPieceStage::Writing => DiskPieceStageView::Writing,
        DiskPieceStage::Stored => DiskPieceStageView::Stored,
        DiskPieceStage::Hashing => DiskPieceStageView::Hashing,
        DiskPieceStage::CheckpointDirty => DiskPieceStageView::CheckpointDirty,
        DiskPieceStage::CheckpointSyncing => DiskPieceStageView::CheckpointSyncing,
        DiskPieceStage::CheckpointCommitting => DiskPieceStageView::CheckpointCommitting,
        DiskPieceStage::Failed => DiskPieceStageView::Failed,
    }
}

const fn map_disk_checkpoint_stage(stage: DiskCheckpointStage) -> DiskCheckpointStageView {
    match stage {
        DiskCheckpointStage::Idle => DiskCheckpointStageView::Idle,
        DiskCheckpointStage::Syncing => DiskCheckpointStageView::Syncing,
        DiskCheckpointStage::Committing => DiskCheckpointStageView::Committing,
        DiskCheckpointStage::Error => DiskCheckpointStageView::Error,
    }
}

fn active_piece_id(piece_index: u32, attempt: u32) -> String {
    format!("{piece_index}:{attempt}")
}

fn active_piece_from_runtime(runtime: &DiskPieceRuntimeSnapshot) -> ActivePiece {
    ActivePiece {
        piece_id: active_piece_id(runtime.piece_index, runtime.attempt),
        piece_index: runtime.piece_index,
        attempt: runtime.attempt,
        piece_length: runtime.piece_length,
        stage: active_stage_from_runtime(runtime),
        requested: Vec::new(),
        received: Vec::new(),
        stored: Vec::new(),
        age_millis: runtime.age_millis.to_string(),
        error: runtime.error.clone(),
    }
}

const fn active_stage_from_runtime(runtime: &DiskPieceRuntimeSnapshot) -> ActivePieceStageView {
    match runtime.stage {
        DiskPieceStage::Receiving => {
            if runtime.received_bytes > runtime.stored_bytes {
                ActivePieceStageView::Received
            } else {
                ActivePieceStageView::Requested
            }
        }
        DiskPieceStage::Queued | DiskPieceStage::Writing => ActivePieceStageView::Received,
        DiskPieceStage::Stored => ActivePieceStageView::Stored,
        DiskPieceStage::Hashing => ActivePieceStageView::Hashing,
        DiskPieceStage::CheckpointDirty => ActivePieceStageView::CheckpointDirty,
        DiskPieceStage::CheckpointSyncing => ActivePieceStageView::CheckpointSyncing,
        DiskPieceStage::CheckpointCommitting => ActivePieceStageView::CheckpointCommitting,
        DiskPieceStage::Failed => ActivePieceStageView::Failed,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TorrentActivity {
    PieceStarted {
        piece_index: u32,
        piece_length: u32,
        attempt: u32,
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
    PieceHashing {
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
            ViewProjection::PieceActivity
                | ViewProjection::Dht
                | ViewProjection::Peers
                | ViewProjection::Swarm
                | ViewProjection::Files
                | ViewProjection::Trackers
        )
    {
        return Err(SubscriptionError::InvalidProjection);
    }
    if matches!(spec.selector, ViewSelector::Torrent { .. })
        && matches!(
            spec.projection,
            ViewProjection::Disk | ViewProjection::Dht | ViewProjection::Speed
        )
    {
        return Err(SubscriptionError::InvalidProjection);
    }
    match (&spec.selector, spec.projection) {
        (ViewSelector::SessionDht, ViewProjection::Dht) => {}
        (ViewSelector::SessionDht, _) | (_, ViewProjection::Dht) => {
            return Err(SubscriptionError::InvalidProjection);
        }
        (ViewSelector::SessionSpeed { metrics, .. }, ViewProjection::Speed)
            if !metrics.is_empty()
                && metrics.len() <= MAX_SPEED_SERIES
                && metrics
                    .iter()
                    .all(|metric| SpeedMetric::AVAILABLE.contains(metric))
                && metrics.iter().copied().collect::<BTreeSet<_>>().len() == metrics.len() => {}
        (ViewSelector::SessionSpeed { .. }, _) | (_, ViewProjection::Speed) => {
            return Err(SubscriptionError::InvalidProjection);
        }
        _ => {}
    }
    if spec.projection != ViewProjection::Diagnostics && spec.diagnostics.is_some() {
        return Err(SubscriptionError::InvalidProjection);
    }
    if let Some(filter) = &spec.diagnostics
        && !valid_filter(filter)
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
    previous_storage: Option<&StorageSettingsSnapshot>,
    current_storage: &StorageSettingsSnapshot,
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
            let storage = previous_storage.map(|_| current_storage.clone());
            (!upsert.is_empty() || !removed.is_empty() || storage.is_some()).then_some(
                ViewPatch::TorrentList {
                    upsert,
                    removed,
                    storage,
                },
            )
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
            let empty = BTreeMap::new();
            let old_active = old.map_or(&empty, |model| &model.active);
            let next_active = next.map_or(&empty, |model| &model.active);
            let (active_upsert, active_removed) = active_piece_patch(old_active, next_active);
            Some(ViewPatch::PieceActivity {
                torrent_id: torrent_id.clone(),
                piece_count: next.map_or(0, |model| model.view.piece_count),
                verified,
                cleared,
                active_upsert,
                active_removed,
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
        (ViewSelector::Torrent { torrent_id }, ViewProjection::Swarm) => {
            match (previous.get(torrent_id), current.get(torrent_id)) {
                (Some(old), Some(next)) => {
                    swarm_collection_patch(torrent_id, &old.swarm, &next.swarm)
                }
                _ => None,
            }
        }
        (ViewSelector::Torrent { torrent_id }, ViewProjection::Files) => {
            let old = previous
                .get(torrent_id)
                .and_then(|model| model.files.as_ref());
            let next = current
                .get(torrent_id)
                .and_then(|model| model.files.as_ref());
            match (old, next) {
                (Some(old), Some(next)) if old.catalog_matches(next) => {
                    let upsert = next.rows_changed_since(old);
                    (!upsert.is_empty()).then(|| ViewPatch::Files {
                        torrent_id: torrent_id.clone(),
                        upsert,
                        removed: Vec::new(),
                    })
                }
                _ => None,
            }
        }
        (ViewSelector::Torrent { torrent_id }, ViewProjection::Trackers) => {
            let empty = BTreeMap::new();
            let old = previous
                .get(torrent_id)
                .map_or(&empty, |model| model.trackers.row_map());
            let next = current
                .get(torrent_id)
                .map_or(&empty, |model| model.trackers.row_map());
            tracker_collection_patch(torrent_id, old, next)
        }
        (
            ViewSelector::TorrentList,
            ViewProjection::PieceActivity
            | ViewProjection::Peers
            | ViewProjection::Swarm
            | ViewProjection::Files
            | ViewProjection::Trackers,
        ) => None,
        (
            _,
            ViewProjection::Disk
            | ViewProjection::Dht
            | ViewProjection::Speed
            | ViewProjection::Diagnostics,
        ) => None,
        (ViewSelector::SessionDht, _) => None,
        (ViewSelector::SessionSpeed { .. }, _) => None,
    }
}

fn projection_requires_snapshot(
    spec: &SubscriptionSpec,
    previous: &BTreeMap<String, TorrentModel>,
    current: &BTreeMap<String, TorrentModel>,
) -> bool {
    if let (ViewSelector::Torrent { torrent_id }, ViewProjection::Swarm) =
        (&spec.selector, spec.projection)
    {
        return previous.contains_key(torrent_id) != current.contains_key(torrent_id);
    }
    if let (ViewSelector::Torrent { torrent_id }, ViewProjection::Trackers) =
        (&spec.selector, spec.projection)
    {
        return previous.contains_key(torrent_id) != current.contains_key(torrent_id);
    }
    let (ViewSelector::Torrent { torrent_id }, ViewProjection::Files) =
        (&spec.selector, spec.projection)
    else {
        return false;
    };
    match (previous.get(torrent_id), current.get(torrent_id)) {
        (None, None) => false,
        (Some(old), Some(next)) => match (&old.files, &next.files) {
            (None, None) => false,
            (Some(old), Some(next)) => !old.catalog_matches(next),
            _ => true,
        },
        _ => true,
    }
}

#[allow(clippy::too_many_arguments)]
fn targeted_activity_patch(
    spec: &SubscriptionSpec,
    torrent_id: &str,
    previous_view: &TorrentView,
    next_view: &TorrentView,
    previous_verified: &[IndexRange],
    next_verified: &[IndexRange],
    previous_active: &BTreeMap<u32, ActivePiece>,
    next_active: &BTreeMap<u32, ActivePiece>,
    file_upsert: &[FileView],
) -> Option<ViewPatch> {
    match (&spec.selector, spec.projection) {
        (ViewSelector::TorrentList, ViewProjection::Summary) => {
            (previous_view != next_view).then(|| ViewPatch::TorrentList {
                upsert: vec![next_view.clone()],
                removed: Vec::new(),
                storage: None,
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
            ViewProjection::PieceActivity,
        ) if selected == torrent_id => {
            let verified = difference(next_verified, previous_verified);
            let cleared = difference(previous_verified, next_verified);
            let (active_upsert, active_removed) = active_piece_patch(previous_active, next_active);
            (!verified.is_empty()
                || !cleared.is_empty()
                || !active_upsert.is_empty()
                || !active_removed.is_empty())
            .then(|| ViewPatch::PieceActivity {
                torrent_id: torrent_id.to_owned(),
                piece_count: next_view.piece_count,
                verified,
                cleared,
                active_upsert,
                active_removed,
            })
        }
        (
            ViewSelector::Torrent {
                torrent_id: selected,
            },
            ViewProjection::Files,
        ) if selected == torrent_id && !file_upsert.is_empty() => Some(ViewPatch::Files {
            torrent_id: torrent_id.to_owned(),
            upsert: file_upsert.to_vec(),
            removed: Vec::new(),
        }),
        _ => None,
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
                storage: None,
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

fn targeted_swarm_patch(
    spec: &SubscriptionSpec,
    torrent_id: &str,
    previous: &SwarmModel,
    current: &SwarmModel,
) -> Option<ViewPatch> {
    match (&spec.selector, spec.projection) {
        (
            ViewSelector::Torrent {
                torrent_id: selected,
            },
            ViewProjection::Swarm,
        ) if selected == torrent_id => swarm_collection_patch(torrent_id, previous, current),
        _ => None,
    }
}

fn targeted_tracker_patch(
    spec: &SubscriptionSpec,
    torrent_id: &str,
    previous_view: &TorrentView,
    next_view: &TorrentView,
    previous_trackers: &BTreeMap<String, TrackerView>,
    next_trackers: &BTreeMap<String, TrackerView>,
) -> Option<ViewPatch> {
    match (&spec.selector, spec.projection) {
        (ViewSelector::TorrentList, ViewProjection::Summary) => {
            (previous_view != next_view).then(|| ViewPatch::TorrentList {
                upsert: vec![next_view.clone()],
                removed: Vec::new(),
                storage: None,
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
            ViewProjection::Trackers,
        ) if selected == torrent_id => {
            tracker_collection_patch(torrent_id, previous_trackers, next_trackers)
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

fn swarm_collection_patch(
    torrent_id: &str,
    previous: &SwarmModel,
    current: &SwarmModel,
) -> Option<ViewPatch> {
    if previous == current {
        return None;
    }
    let upsert = current
        .peers
        .iter()
        .filter(|(id, peer)| previous.peers.get(*id) != Some(*peer))
        .map(|(_, peer)| peer.clone())
        .collect();
    let removed = previous
        .peers
        .keys()
        .filter(|id| !current.peers.contains_key(*id))
        .cloned()
        .collect();
    Some(ViewPatch::Swarm {
        torrent_id: torrent_id.to_owned(),
        state: current.state,
        captured_millis: current.captured_millis.clone(),
        maximum_records: current.maximum_records,
        counts: current.counts.clone(),
        upsert,
        removed,
    })
}

fn tracker_collection_patch(
    torrent_id: &str,
    previous: &BTreeMap<String, TrackerView>,
    current: &BTreeMap<String, TrackerView>,
) -> Option<ViewPatch> {
    let upsert = current
        .iter()
        .filter(|(id, tracker)| previous.get(*id) != Some(*tracker))
        .map(|(_, tracker)| tracker.clone())
        .collect::<Vec<_>>();
    let removed = previous
        .keys()
        .filter(|id| !current.contains_key(*id))
        .cloned()
        .collect::<Vec<_>>();
    (!upsert.is_empty() || !removed.is_empty()).then(|| ViewPatch::Trackers {
        torrent_id: torrent_id.to_owned(),
        upsert,
        removed,
    })
}

fn disk_patch(previous: &DiskSessionView, current: &DiskSessionView) -> Option<ViewPatch> {
    let upsert = current
        .pieces
        .iter()
        .filter(|(id, piece)| previous.pieces.get(*id) != Some(*piece))
        .map(|(_, piece)| piece.clone())
        .collect::<Vec<_>>();
    let removed = previous
        .pieces
        .keys()
        .filter(|id| !current.pieces.contains_key(*id))
        .cloned()
        .collect::<Vec<_>>();
    (previous.pipeline != current.pipeline || !upsert.is_empty() || !removed.is_empty()).then(
        || ViewPatch::SessionDisk {
            pipeline: current.pipeline.clone(),
            upsert,
            removed,
        },
    )
}

fn active_piece_patch(
    previous: &BTreeMap<u32, ActivePiece>,
    current: &BTreeMap<u32, ActivePiece>,
) -> (Vec<ActivePiece>, Vec<String>) {
    let upsert = current
        .iter()
        .filter(|(piece_index, piece)| previous.get(*piece_index) != Some(*piece))
        .map(|(_, piece)| piece.clone())
        .collect();
    let removed = previous
        .iter()
        .filter(|(piece_index, piece)| {
            current
                .get(*piece_index)
                .is_none_or(|current| current.piece_id != piece.piece_id)
        })
        .map(|(_, piece)| piece.piece_id.clone())
        .collect();
    (upsert, removed)
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
            ViewPatch::TorrentList {
                upsert,
                removed,
                storage,
            },
            ViewPatch::TorrentList {
                upsert: next_upsert,
                removed: next_removed,
                storage: next_storage,
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
            if next_storage.is_some() {
                *storage = next_storage.clone();
            }
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
                active_upsert,
                active_removed,
            },
            ViewPatch::PieceActivity {
                torrent_id: next_id,
                piece_count: next_piece_count,
                verified: next_verified,
                cleared: next_cleared,
                active_upsert: next_active_upsert,
                active_removed: next_active_removed,
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
            let mut values = active_upsert
                .drain(..)
                .map(|piece| (piece.piece_id.clone(), piece))
                .collect::<BTreeMap<_, _>>();
            for id in next_active_removed {
                values.remove(id);
            }
            for piece in next_active_upsert {
                values.insert(piece.piece_id.clone(), piece.clone());
            }
            let mut removed_ids = active_removed.drain(..).collect::<BTreeSet<_>>();
            for piece in next_active_upsert {
                removed_ids.remove(&piece.piece_id);
            }
            removed_ids.extend(next_active_removed.iter().cloned());
            *active_upsert = values.into_values().collect();
            *active_removed = removed_ids.into_iter().collect();
            true
        }
        (
            ViewPatch::SessionDisk {
                pipeline,
                upsert,
                removed,
            },
            ViewPatch::SessionDisk {
                pipeline: next_pipeline,
                upsert: next_upsert,
                removed: next_removed,
            },
        ) => {
            let mut values = upsert
                .drain(..)
                .map(|piece| (piece.row_id.clone(), piece))
                .collect::<BTreeMap<_, _>>();
            for id in next_removed {
                values.remove(id);
            }
            for piece in next_upsert {
                values.insert(piece.row_id.clone(), piece.clone());
            }
            let mut removed_ids = removed.drain(..).collect::<BTreeSet<_>>();
            for piece in next_upsert {
                removed_ids.remove(&piece.row_id);
            }
            removed_ids.extend(next_removed.iter().cloned());
            *pipeline = next_pipeline.clone();
            *upsert = values.into_values().collect();
            *removed = removed_ids.into_iter().collect();
            true
        }
        (
            ViewPatch::SessionDht { inspection },
            ViewPatch::SessionDht {
                inspection: next_inspection,
            },
        ) => {
            *inspection = next_inspection.clone();
            true
        }
        (
            ViewPatch::SessionSpeed { history },
            ViewPatch::SessionSpeed {
                history: next_history,
            },
        ) => {
            *history = next_history.clone();
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
            ViewPatch::Swarm {
                torrent_id,
                state,
                captured_millis,
                maximum_records,
                counts,
                upsert,
                removed,
            },
            ViewPatch::Swarm {
                torrent_id: next_id,
                state: next_state,
                captured_millis: next_captured_millis,
                maximum_records: next_maximum_records,
                counts: next_counts,
                upsert: next_upsert,
                removed: next_removed,
            },
        ) if torrent_id == next_id => {
            let mut values = upsert
                .drain(..)
                .map(|peer| (peer.peer_record_id.clone(), peer))
                .collect::<BTreeMap<_, _>>();
            for id in next_removed {
                values.remove(id);
            }
            for peer in next_upsert {
                values.insert(peer.peer_record_id.clone(), peer.clone());
            }
            let mut removed_ids = removed.drain(..).collect::<BTreeSet<_>>();
            for peer in next_upsert {
                removed_ids.remove(&peer.peer_record_id);
            }
            removed_ids.extend(next_removed.iter().cloned());
            *state = *next_state;
            *captured_millis = next_captured_millis.clone();
            *maximum_records = *next_maximum_records;
            *counts = next_counts.clone();
            *upsert = values.into_values().collect();
            *removed = removed_ids.into_iter().collect();
            true
        }
        (
            ViewPatch::Files {
                torrent_id,
                upsert,
                removed,
            },
            ViewPatch::Files {
                torrent_id: next_id,
                upsert: next_upsert,
                removed: next_removed,
            },
        ) if torrent_id == next_id => {
            let mut values = upsert
                .drain(..)
                .map(|file| (file.file_id.clone(), file))
                .collect::<BTreeMap<_, _>>();
            for id in next_removed {
                values.remove(id);
            }
            for file in next_upsert {
                values.insert(file.file_id.clone(), file.clone());
            }
            let mut removed_ids = removed.drain(..).collect::<std::collections::BTreeSet<_>>();
            for file in next_upsert {
                removed_ids.remove(&file.file_id);
            }
            removed_ids.extend(next_removed.iter().cloned());
            *upsert = values.into_values().collect();
            *removed = removed_ids.into_iter().collect();
            true
        }
        (
            ViewPatch::Trackers {
                torrent_id,
                upsert,
                removed,
            },
            ViewPatch::Trackers {
                torrent_id: next_id,
                upsert: next_upsert,
                removed: next_removed,
            },
        ) if torrent_id == next_id => {
            let mut values = upsert
                .drain(..)
                .map(|tracker| (tracker.tracker_id.clone(), tracker))
                .collect::<BTreeMap<_, _>>();
            for id in next_removed {
                values.remove(id);
            }
            for tracker in next_upsert {
                values.insert(tracker.tracker_id.clone(), tracker.clone());
            }
            let mut removed_ids = removed.drain(..).collect::<std::collections::BTreeSet<_>>();
            for tracker in next_upsert {
                removed_ids.remove(&tracker.tracker_id);
            }
            removed_ids.extend(next_removed.iter().cloned());
            *upsert = values.into_values().collect();
            *removed = removed_ids.into_iter().collect();
            true
        }
        (
            ViewPatch::Diagnostics { events, retention },
            ViewPatch::Diagnostics {
                events: next_events,
                retention: next_retention,
            },
        ) => {
            if events.len().saturating_add(next_events.len()) > MAX_DIAGNOSTIC_PATCH_EVENTS
                || patch_encoded_len(events).saturating_add(patch_encoded_len(next_events))
                    > MAX_DIAGNOSTIC_PATCH_BYTES
            {
                return false;
            }
            events.extend(next_events.iter().cloned());
            *retention = next_retention.clone();
            true
        }
        _ => false,
    }
}

fn selector_torrent_id(selector: &ViewSelector) -> Option<&str> {
    match selector {
        ViewSelector::TorrentList
        | ViewSelector::SessionDht
        | ViewSelector::SessionSpeed { .. } => None,
        ViewSelector::Torrent { torrent_id } => Some(torrent_id),
    }
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
#[path = "views/tests/mod.rs"]
mod tests;
