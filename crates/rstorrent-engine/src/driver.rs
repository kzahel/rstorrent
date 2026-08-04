use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt;
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rstorrent_protocol::magnet::{Magnet, MagnetError, PeerHint, UdpTrackerUrl};
use rstorrent_protocol::metadata::{
    MetadataError, MetadataExtensionUpdate, MetadataInstant, MetadataMessage,
    TorrentMetadataDownload, TorrentMetadataEvent, UT_METADATA_LOCAL_ID,
    encode_extension_handshake, encode_metadata_reject, encode_metadata_request,
    parse_extension_handshake, parse_metadata_message,
};
use rstorrent_protocol::metainfo::{
    BEP9_METAINFO_LIMITS, DURABLE_METAINFO_LIMITS, Metainfo, MetainfoError,
};
use rstorrent_protocol::peer_wire::{
    FrameError, Handshake, HandshakeError, MAX_REQUEST_BLOCK_LENGTH, PeerMessage,
};
use rstorrent_protocol::piece::{PieceError, VerifiedPiece};
use rstorrent_protocol::storage_layout::{FileSelection, LayoutError, TorrentLayout};
use rstorrent_protocol::udp_tracker::{
    AnnounceEvent, AnnounceRequest, AnnounceResponse, CompactPeer, MAX_ANNOUNCE_RESPONSE_LENGTH,
    MAX_COMPACT_PEERS, TrackerAddressFamily, TransactionId, UdpTrackerError,
    encode_announce_request, encode_connect_request, parse_announce_response,
    parse_connect_response,
};
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::net::{UdpSocket, lookup_host};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{Instant as TokioInstant, timeout, timeout_at};
use tokio_util::sync::CancellationToken;

use crate::checkpoint::{
    CheckpointAdmission, CheckpointBatch, CheckpointBatchState, CheckpointIntent, CheckpointPolicy,
    DurabilityTarget,
};
use crate::dht::{DhtError, DhtHandle};
use crate::metrics::{ByteMetric, ByteMetricSink, SharedByteMetricSink};
use crate::network::{NetworkConfig, NetworkPolicy};
use crate::peer::{
    DialAttempt, DialAttemptId, DialCandidate, PeerEndpoint, PeerFailure, PeerIntegrityAction,
    PeerObservation, PeerRegistry, PeerRegistryConfig, PeerRegistryCounts, PeerRegistryError,
    PeerRegistrySnapshot, PeerSelectionContext, PeerSelector, PeerSource,
};
use crate::peer_runtime::{
    PeerConnectionObservation, PeerConnectionRole, PeerContentActivity, PeerRequestWindowPhase,
    PeerRuntime, PeerRuntimeError, connection_id,
};
use crate::peer_socket::{
    self, PeerConnection, PeerSetError, PeerSetEvent, PeerSocketError, PeerSocketSet, PeerTaskEvent,
};
use crate::selective_storage::{
    CheckpointHandles, DescriptorStorage, PlatformStorageSpec, PreparedFileHash, PublicationShape,
    ResumeArtifactState, ResumedStorage, SelectiveHashPlan, SelectiveStorage,
    SelectiveStorageError, SelectiveWriteJob, VERIFICATION_CHUNK_LENGTH,
    remove_selective_part_if_present, remove_selective_staging_if_present,
    torrent_storage_paths_for_output_with_shape, validate_publication_name,
};
use crate::storage_file_pool::StorageFilePool;
use crate::swarm::{
    BlockKey, ConnectionId, ConnectionRemoval, ConnectionWindowPhaseSnapshot, NoRequestReason,
    PendingDialId, PieceGeneration, PieceHashFailure, PiecePlan, ReceiveDisposition, SwarmConfig,
    SwarmError, SwarmState,
};
use crate::tracker::{
    TrackerAction, TrackerId, TrackerRuntimeSnapshot, TrackerSchedule, TrackerWaitKind,
};

const CLIENT_PEER_ID: [u8; 20] = *b"-RS0001-000000000000";
const DEFAULT_ADVERTISED_PEER_PORT: u16 = 6881;
const NETWORK_RESOLUTION_TIMEOUT: Duration = Duration::from_secs(10);
const UDP_TRACKER_RETRANSMIT_AFTER: Duration = Duration::from_secs(15);
const UDP_TRACKER_COMPLETION_TIMEOUT: Duration = Duration::from_secs(30);
const UDP_TRACKER_TOKEN_LIFETIME: Duration = Duration::from_secs(60);
const MAX_UDP_TRACKER_TOKENS: usize = 64;
const MAX_CONCURRENT_TRACKER_OPERATIONS: usize = 8;
const TRACKER_RESULT_QUEUE: usize = 4;
const CONTENT_DISCOVERY_QUEUE: usize = 8;
const CONTENT_STORAGE_PENDING_QUEUE: usize = 2;
const CONTENT_STORAGE_WRITE_BATCH_BLOCKS: usize = 16;
const CONTENT_STORAGE_WRITE_BATCH_BYTES: usize = 256 * 1024;
const CONTENT_STORAGE_WRITE_CONCURRENCY: usize = 4;
const CONTENT_STORAGE_HASH_CONCURRENCY: usize = 4;
const CONTENT_STORAGE_MAX_DIAGNOSTIC_CONCURRENCY: usize = 8;
const CHECKPOINT_MAX_AGE: Duration = Duration::from_secs(2);
const CHECKPOINT_MAX_DIRTY_BYTES: u64 = 64 * 1024 * 1024;
const CHECKPOINT_MAX_PIECES: usize = 256;
const CHECKPOINT_INTENT_CAPACITY: usize = 256;
const CHECKPOINT_SYNC_CONCURRENCY: usize = 4;
const CHECKPOINT_BYTE_UNIT: u64 = MAX_REQUEST_BLOCK_LENGTH as u64;
const MAX_RESOLVED_ADDRESSES: usize = 32;
const UNKNOWN_MAGNET_LEFT: u64 = 16 * 1024;
const UDP_TRACKER_RECEIVE_LENGTH: usize = MAX_ANNOUNCE_RESPONSE_LENGTH + 1;
const SAFE_CANCEL_REQUESTED: usize = 1 << (usize::BITS - 1);
const SAFE_CANCEL_CRITICAL_MASK: usize = SAFE_CANCEL_REQUESTED - 1;
const DHT_RETRY_INITIAL_DELAY: Duration = Duration::from_secs(15);
const DHT_RETRY_MAX_DELAY: Duration = Duration::from_secs(5 * 60);
const DHT_SUCCESS_REQUERY_DELAY: Duration = Duration::from_secs(60);
const CONTENT_SWARM_MAINTENANCE_INTERVAL: Duration = Duration::from_millis(100);
const CONTENT_PEER_DIAGNOSTIC_INTERVAL: Duration = Duration::from_secs(1);
const PEER_OBSERVATION_INTERVAL: Duration = Duration::from_millis(100);
const STORAGE_OBSERVATION_INTERVAL: Duration = Duration::from_millis(100);
const MAX_METADATA_PEERS: usize = 8;
const METADATA_SCHEDULER_TICK: Duration = Duration::from_millis(100);
const MAX_RECENT_METADATA_ATTEMPTS: usize = 64;
const MAX_DIAGNOSTIC_ERROR_LENGTH: usize = 256;
const MAX_ENGINE_PIECES: usize = 52_428;

const fn content_storage_job_limit(max_buffered_payload_bytes: usize) -> usize {
    max_buffered_payload_bytes / MAX_REQUEST_BLOCK_LENGTH as usize
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DownloadResourceLimits {
    pub max_outstanding_request_bytes: usize,
    pub max_buffered_payload_bytes: usize,
    pub max_active_piece_bytes: usize,
}

impl DownloadResourceLimits {
    pub const DESKTOP: Self = Self {
        max_outstanding_request_bytes: 256 * 1024 * 1024,
        max_buffered_payload_bytes: 32 * 1024 * 1024,
        max_active_piece_bytes: 256 * 1024 * 1024,
    };

    pub const ANDROID: Self = Self {
        max_outstanding_request_bytes: 128 * 1024 * 1024,
        max_buffered_payload_bytes: 16 * 1024 * 1024,
        max_active_piece_bytes: 128 * 1024 * 1024,
    };

    pub const fn new(
        max_outstanding_request_bytes: usize,
        max_buffered_payload_bytes: usize,
        max_active_piece_bytes: usize,
    ) -> Self {
        Self {
            max_outstanding_request_bytes,
            max_buffered_payload_bytes,
            max_active_piece_bytes,
        }
    }

    fn validate(self) -> Result<Self, DownloadError> {
        if self.max_outstanding_request_bytes < rstorrent_protocol::piece::MIN_PAYLOAD_ALLOWANCE {
            return Err(DownloadError::InvalidResourceLimit(
                "outstanding request allowance must fit one request block",
            ));
        }
        if self.max_buffered_payload_bytes < rstorrent_protocol::piece::MIN_PAYLOAD_ALLOWANCE {
            return Err(DownloadError::InvalidResourceLimit(
                "buffered payload allowance must fit one request block",
            ));
        }
        if self.max_active_piece_bytes < rstorrent_protocol::piece::MIN_PAYLOAD_ALLOWANCE {
            return Err(DownloadError::InvalidResourceLimit(
                "active piece working set must fit one request block",
            ));
        }
        Ok(self)
    }

    fn swarm_config(self) -> SwarmConfig {
        let mut config = SwarmConfig::for_request_limit(self.max_outstanding_request_bytes);
        config.max_active_piece_bytes = self.max_active_piece_bytes;
        config
    }
}

#[derive(Clone, Debug)]
pub struct DownloadConfig {
    pub metainfo_path: PathBuf,
    pub peer: SocketAddr,
    pub output_path: PathBuf,
    pub network: NetworkConfig,
    pub resource_limits: DownloadResourceLimits,
    pub skip_files: Vec<usize>,
    pub materialize_files: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct MagnetDownloadConfig {
    pub magnet: String,
    pub output_path: PathBuf,
    pub network: NetworkConfig,
    pub resource_limits: DownloadResourceLimits,
    pub skip_files: Vec<usize>,
    pub materialize_files: Vec<usize>,
    pub dht: Option<DhtHandle>,
}

#[derive(Clone, Debug)]
pub struct ResumableMagnetDownloadConfig {
    pub magnet: String,
    /// Selected containing directory. Verified multi-file metadata supplies
    /// the recognizable publication directory beneath this root.
    pub storage_root: PathBuf,
    pub network: NetworkConfig,
    pub resource_limits: DownloadResourceLimits,
    pub skip_files: Vec<usize>,
    pub verified_info: Option<Vec<u8>>,
    pub verified_pieces: Vec<bool>,
    pub artifact_state: ResumeArtifactState,
    pub download_missing: bool,
    pub dht: Option<DhtHandle>,
}

pub trait DownloadCheckpointSink: Send + Sync {
    fn metadata_verified(&self, raw_info: &[u8]) -> Result<(), String>;
    fn storage_prepared(&self, storage: ResumedStorage) -> Result<(), String>;
    fn recheck_started(&self) -> Result<(), String>;
    fn have_rechecked(&self, verified_pieces: &[bool]) -> Result<(), String>;
    fn pieces_durable(&self, piece_indices: &[usize]) -> Result<(), String>;
    fn piece_durable(&self, piece_index: usize) -> Result<(), String> {
        self.pieces_durable(&[piece_index])
    }
    fn descriptor_prepared(&self, files: &[PreparedFileHash]) -> Result<(), String>;
    fn publication_prepared(&self) -> Result<(), String>;
    fn published(&self) -> Result<(), String>;
}

pub trait DownloadActivitySink: Send + Sync + fmt::Debug {
    fn record(&self, event: DownloadActivityEvent);
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DownloadActivityEvent {
    MetadataVerified {
        total_length: u64,
        piece_length: u32,
        piece_count: usize,
        file_count: usize,
    },
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
    PieceHashing {
        piece_index: u32,
    },
    PieceVerified {
        piece_index: u32,
    },
    PieceHashFailed {
        piece_index: u32,
        contributor_count: usize,
        failed_bytes: usize,
    },
    PathPublicationStage(PathPublicationStage),
    StorageState(Box<DiskRuntimeSnapshot>),
    TrackerAnnounceStarted {
        tracker: String,
        tier: u8,
        attempt: u32,
        event: AnnounceEvent,
    },
    TrackerUdpRetransmitted {
        tracker: String,
        operation: &'static str,
    },
    TrackerAnnounceFailed {
        tracker: String,
        failures: u8,
        retry_in_seconds: u64,
        detail: String,
    },
    TrackerFallbackSelected {
        tracker: String,
        tier: u8,
    },
    TrackerRetryScheduled {
        tracker: String,
        retry_in_seconds: u64,
    },
    TrackerReannounceScheduled {
        tracker: String,
        announce_in_seconds: u64,
    },
    TrackerAnnounceSucceeded {
        tracker: String,
        peer_count: u32,
        interval_seconds: u64,
    },
    TrackerPeersUnavailable {
        tracker: String,
        peer_count: u32,
    },
    DhtLookupStarted,
    DhtLookupSucceeded {
        peer_count: u32,
    },
    DhtLookupFailed {
        detail: String,
    },
    DhtRetryScheduled {
        retry_in_seconds: u64,
    },
    DhtDisabledForPrivateTorrent,
    PeerDialStarted {
        peer: String,
    },
    PeerConnections {
        captured_at: Duration,
        peers: Box<Vec<PeerConnectionObservation>>,
    },
    PeerRegistryState {
        active: bool,
        snapshot: Box<PeerRegistrySnapshot>,
    },
    TrackerState(Box<TrackerRuntimeSnapshot>),
    SwarmState(Box<SwarmActivitySnapshot>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathPublicationStage {
    IntentDurable = 1,
    Renamed = 2,
    NamespaceDurable = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SwarmActivitySnapshot {
    pub pending_dials: usize,
    pub connected_peers: usize,
    pub unchoked_peers: usize,
    pub missing_blocks: usize,
    pub requested_blocks: usize,
    pub active_request_attempts: usize,
    pub active_duplicate_attempts: usize,
    pub writing_blocks: usize,
    pub received_blocks: usize,
    pub verified_blocks: usize,
    pub active_piece_count: usize,
    pub active_piece_bytes: usize,
    pub outstanding_request_bytes: usize,
    pub outstanding_request_high_water: usize,
    pub request_target_total: usize,
    pub request_target_max: usize,
    pub slow_start_peers: usize,
    pub stalled_peers: usize,
    pub useful_payload_bytes: usize,
    pub observed_payload_rate: usize,
    pub endgame_assignments: usize,
    pub cancelled_request_attempts: usize,
    pub redundant_payload_bytes: usize,
    pub piece_hash_failures: usize,
    pub failed_piece_bytes: usize,
    pub last_hash_failure_contributors: usize,
    pub request_timeout_min_seconds: Option<u64>,
    pub request_timeout_max_seconds: Option<u64>,
    pub oldest_request_age_seconds: Option<u64>,
    pub next_request_expiry_seconds: Option<u64>,
    pub next_replacement_seconds: Option<u64>,
    pub no_request_reason: Option<NoRequestReason>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentRequestWindowPhase {
    SlowStart,
    Steady,
    Stalled,
}

impl ContentRequestWindowPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SlowStart => "slow_start",
            Self::Steady => "steady",
            Self::Stalled => "stalled",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContentPeerActivitySnapshot {
    pub connection_id: u64,
    pub choking: bool,
    pub wanted_piece_count: usize,
    pub pending_requests: usize,
    pub target_requests: usize,
    pub queued_payload_bytes: usize,
    pub window_phase: ContentRequestWindowPhase,
    pub useful_payload_bytes: usize,
    pub observed_payload_rate: usize,
    pub connected_age_seconds: u64,
    pub last_useful_age_seconds: Option<u64>,
    pub last_payload_age_seconds: Option<u64>,
    pub request_timeout_seconds: u64,
    pub oldest_request_age_seconds: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MetadataAcquisitionPhase {
    #[default]
    Idle,
    Discovering,
    Acquiring,
    Complete,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MetadataPeerStage {
    Dialing,
    AwaitingExtensionHandshake,
    Requesting,
    Complete,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetadataPeerSnapshot {
    pub record_id: u64,
    pub attempt_id: u64,
    pub endpoint: SocketAddr,
    pub stage: MetadataPeerStage,
    pub started_at: Duration,
    pub last_activity_at: Duration,
    pub last_progress_at: Duration,
    pub supports_extensions: Option<bool>,
    pub remote_metadata_id: Option<u8>,
    pub metadata_size: Option<usize>,
    pub metadata_blocks: Option<usize>,
    pub requests_sent: usize,
    pub pending_requests: usize,
    pub blocks_received: usize,
    pub bytes_received: usize,
    pub messages_received: usize,
    pub rejects_received: usize,
    pub terminal_detail: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MetadataAcquisitionSnapshot {
    pub captured_at: Duration,
    pub phase: MetadataAcquisitionPhase,
    pub registry: Option<PeerRegistrySnapshot>,
    pub pending_dials: usize,
    pub active_workers: usize,
    pub total_attempts: usize,
    pub total_requests_sent: usize,
    pub total_blocks_received: usize,
    pub total_bytes_received: usize,
    pub total_hash_failures: usize,
    pub last_hash_failure_contributors: usize,
    pub recent_attempts_dropped: usize,
    pub last_error: Option<String>,
    pub active_attempts: Vec<MetadataPeerSnapshot>,
    pub recent_attempts: Vec<MetadataPeerSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadDiagnosticSnapshot {
    pub progress: DownloadProgress,
    pub swarm: Option<SwarmActivitySnapshot>,
    pub content_peers_captured_at: Option<Duration>,
    pub content_peers: Vec<ContentPeerActivitySnapshot>,
    pub content_registry: Option<PeerRegistryCounts>,
    pub peer_connections: Vec<PeerConnectionObservation>,
    pub metadata: MetadataAcquisitionSnapshot,
}

#[derive(Clone, Debug)]
struct ContentDownloadConfig {
    output_path: PathBuf,
    max_buffered_payload_bytes: usize,
    swarm_config: SwarmConfig,
    skip_files: Vec<usize>,
    materialize_files: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct DownloadControl {
    inner: Arc<DownloadControlInner>,
}

#[derive(Debug)]
struct DownloadControlInner {
    started_at: Instant,
    cancellation: CancellationToken,
    buffered_payload_bytes: AtomicUsize,
    payload_high_water: AtomicUsize,
    outstanding_request_bytes: AtomicUsize,
    outstanding_request_high_water: AtomicUsize,
    requested_bytes: AtomicUsize,
    received_bytes: AtomicUsize,
    stored_bytes: AtomicUsize,
    storage_jobs_pending: AtomicUsize,
    storage_jobs_high_water: AtomicUsize,
    storage_command_queue_high_water: AtomicUsize,
    storage_completion_queue_high_water: AtomicUsize,
    storage_write_delay_millis: AtomicU64,
    storage_hash_delay_millis: AtomicU64,
    checkpoint_sync_delay_millis: AtomicU64,
    checkpoint_commit_delay_millis: AtomicU64,
    checkpoint_sync_failures: AtomicUsize,
    storage_hashes_started: AtomicUsize,
    storage_write_concurrency: AtomicUsize,
    storage_hash_concurrency: AtomicUsize,
    storage_write_timing: StorageCommandTiming,
    storage_hash_timing: StorageCommandTiming,
    storage_write_blocks_started: AtomicUsize,
    storage_write_blocks_completed: AtomicUsize,
    storage_write_batch_blocks_high_water: AtomicUsize,
    storage_write_batch_bytes_high_water: AtomicUsize,
    storage_active: Mutex<StorageActiveOperations>,
    disk_runtime: Mutex<DiskRuntimeState>,
    last_storage_emitted_at: Mutex<Option<Instant>>,
    activity_sink: Mutex<Option<Arc<dyn DownloadActivitySink>>>,
    byte_metric_sink: Mutex<Option<SharedByteMetricSink>>,
    last_swarm_activity: Mutex<Option<SwarmActivitySnapshot>>,
    last_content_peers: Mutex<(Option<Duration>, Vec<ContentPeerActivitySnapshot>)>,
    peer_registry_activity: Mutex<PeerRegistryActivityState>,
    peer_connections: Mutex<PeerConnectionDiagnosticState>,
    metadata_diagnostics: Mutex<MetadataDiagnosticState>,
    storage_file_pool: Mutex<Option<StorageFilePool>>,
    platform_storage: Mutex<Option<PlatformStorageSpec>>,
    safe_cancel_state: AtomicUsize,
}

#[derive(Debug, Default)]
struct PeerConnectionDiagnosticState {
    current: Vec<PeerConnectionObservation>,
    last_emitted: Vec<PeerConnectionObservation>,
    last_emitted_at: Option<Duration>,
}

#[derive(Debug, Default)]
struct PeerRegistryActivityState {
    active: bool,
    last_emitted: Option<PeerRegistrySnapshot>,
    next_transition_at: Option<Duration>,
}

#[derive(Debug, Default)]
struct StorageCommandTiming {
    started: AtomicUsize,
    completed: AtomicUsize,
    active: AtomicUsize,
    active_high_water: AtomicUsize,
    queue_wait_micros: AtomicU64,
    queue_wait_max_micros: AtomicU64,
    service_micros: AtomicU64,
    service_max_micros: AtomicU64,
}

#[derive(Debug, Default)]
struct StorageActiveOperations {
    writes: Vec<Instant>,
    hashes: Vec<Instant>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StorageCommandKind {
    Write = 1,
    Hash = 2,
}

#[derive(Debug, Default)]
struct MetadataDiagnosticState {
    phase: MetadataAcquisitionPhase,
    registry: Option<PeerRegistrySnapshot>,
    pending_dials: usize,
    active_workers: usize,
    total_attempts: usize,
    total_requests_sent: usize,
    total_blocks_received: usize,
    total_bytes_received: usize,
    total_hash_failures: usize,
    last_hash_failure_contributors: usize,
    recent_attempts_dropped: usize,
    last_error: Option<String>,
    active_attempts: BTreeMap<DialAttemptId, MetadataPeerSnapshot>,
    recent_attempts: VecDeque<MetadataPeerSnapshot>,
}

#[derive(Debug)]
struct SafeCancelGuard {
    control: DownloadControl,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DownloadProgress {
    pub buffered_payload_bytes: usize,
    pub payload_high_water: usize,
    pub outstanding_request_bytes: usize,
    pub outstanding_request_high_water: usize,
    pub requested_bytes: usize,
    pub received_bytes: usize,
    pub stored_bytes: usize,
    pub storage_jobs_pending: usize,
    pub storage_jobs_high_water: usize,
    pub storage_command_queue_high_water: usize,
    pub storage_completion_queue_high_water: usize,
    pub storage_hashes_started: usize,
    pub storage_write_operations_started: usize,
    pub storage_write_operations_completed: usize,
    pub storage_write_operations_active: usize,
    pub storage_write_operations_active_high_water: usize,
    pub storage_write_queue_wait_micros: u64,
    pub storage_write_queue_wait_max_micros: u64,
    pub storage_write_service_micros: u64,
    pub storage_write_service_max_micros: u64,
    pub storage_write_blocks_started: usize,
    pub storage_write_blocks_completed: usize,
    pub storage_write_batch_blocks_high_water: usize,
    pub storage_write_batch_bytes_high_water: usize,
    pub storage_hash_operations_started: usize,
    pub storage_hash_operations_completed: usize,
    pub storage_hash_operations_active: usize,
    pub storage_hash_operations_active_high_water: usize,
    pub storage_hash_queue_wait_micros: u64,
    pub storage_hash_queue_wait_max_micros: u64,
    pub storage_hash_service_micros: u64,
    pub storage_hash_service_max_micros: u64,
    pub storage_active_write_micros: Option<u64>,
    pub storage_active_hash_micros: Option<u64>,
    pub checkpoint_stage: DiskCheckpointStage,
    pub checkpoint_dirty_pieces: usize,
    pub checkpoint_dirty_bytes: usize,
    pub checkpoint_dirty_piece_high_water: usize,
    pub checkpoint_dirty_byte_high_water: usize,
    pub checkpoint_oldest_dirty_millis: u64,
    pub checkpoint_batches_started: usize,
    pub checkpoint_batches_completed: usize,
    pub checkpoint_pieces_completed: usize,
    pub checkpoint_sync_operations_completed: usize,
    pub checkpoint_sync_service_micros: u64,
    pub checkpoint_sync_service_max_micros: u64,
    pub checkpoint_commit_service_micros: u64,
    pub checkpoint_commit_service_max_micros: u64,
    pub checkpoint_active_micros: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DiskPressure {
    #[default]
    Idle,
    Normal,
    Backpressured,
    Draining,
    Error,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DiskCheckpointStage {
    #[default]
    Idle,
    Syncing,
    Committing,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiskPieceStage {
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiskPieceRuntimeSnapshot {
    pub piece_index: u32,
    pub piece_length: u32,
    pub attempt: u32,
    pub stage: DiskPieceStage,
    pub requested_bytes: u32,
    pub received_bytes: u32,
    pub stored_bytes: u32,
    pub age_millis: u64,
    pub stage_age_millis: u64,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DiskRuntimeSnapshot {
    pub captured_at_millis: u64,
    pub pressure: DiskPressure,
    pub intake_backpressured: bool,
    pub resident_limit_bytes: usize,
    pub resident_high_watermark_bytes: usize,
    pub resident_low_watermark_bytes: usize,
    pub requested_bytes: usize,
    pub resident_bytes: usize,
    pub queued_write_bytes: usize,
    pub writing_bytes: usize,
    pub hashing_bytes: usize,
    pub checkpoint_stage: DiskCheckpointStage,
    pub checkpoint_dirty_pieces: usize,
    pub checkpoint_dirty_bytes: usize,
    pub checkpoint_dirty_piece_high_water: usize,
    pub checkpoint_dirty_byte_high_water: usize,
    pub checkpoint_oldest_dirty_millis: u64,
    pub checkpoint_batches_started: usize,
    pub checkpoint_batches_completed: usize,
    pub checkpoint_pieces_completed: usize,
    pub checkpoint_sync_operations_completed: usize,
    pub checkpoint_sync_service_micros: u64,
    pub checkpoint_sync_service_max_micros: u64,
    pub checkpoint_commit_service_micros: u64,
    pub checkpoint_commit_service_max_micros: u64,
    pub checkpoint_active_micros: Option<u64>,
    pub storage_jobs_pending: usize,
    pub received_bytes_total: usize,
    pub stored_bytes_total: usize,
    pub verified_bytes_total: usize,
    pub write_operations_started: usize,
    pub write_operations_completed: usize,
    pub hash_operations_started: usize,
    pub hash_operations_completed: usize,
    pub write_queue_wait_micros: u64,
    pub write_queue_wait_max_micros: u64,
    pub write_service_micros: u64,
    pub write_service_max_micros: u64,
    pub hash_queue_wait_micros: u64,
    pub hash_queue_wait_max_micros: u64,
    pub hash_service_micros: u64,
    pub hash_service_max_micros: u64,
    pub pressure_transition_count: u64,
    pub backpressured_millis_total: u64,
    pub last_error: Option<String>,
    pub pieces: Vec<DiskPieceRuntimeSnapshot>,
}

#[derive(Debug, Default)]
struct DiskRuntimeState {
    resident_limit_bytes: usize,
    high_watermark_bytes: usize,
    low_watermark_bytes: usize,
    pressure: DiskPressure,
    pressure_transition_count: u64,
    backpressured_since: Option<Instant>,
    backpressured_total: Duration,
    queued_write_bytes: usize,
    writing_bytes: usize,
    hashing_bytes: usize,
    checkpoint_stage: DiskCheckpointStage,
    checkpoint_active_started_at: Option<Instant>,
    checkpoint_dirty_pieces: usize,
    checkpoint_dirty_bytes: usize,
    checkpoint_dirty_piece_high_water: usize,
    checkpoint_dirty_byte_high_water: usize,
    checkpoint_batches_started: usize,
    checkpoint_batches_completed: usize,
    checkpoint_pieces_completed: usize,
    checkpoint_sync_operations_completed: usize,
    checkpoint_sync_service: Duration,
    checkpoint_sync_service_max: Duration,
    checkpoint_commit_service: Duration,
    checkpoint_commit_service_max: Duration,
    verified_bytes_total: usize,
    last_error: Option<String>,
    pieces: BTreeMap<u32, DiskPieceRuntimeState>,
}

#[derive(Debug)]
struct DiskPieceRuntimeState {
    piece_length: u32,
    attempt: u32,
    stage: DiskPieceStage,
    requested: Vec<(u32, u32)>,
    received: Vec<(u32, u32)>,
    stored: Vec<(u32, u32)>,
    active_write_jobs: usize,
    started_at: Instant,
    stage_started_at: Instant,
    checkpoint_dirty_since: Option<Instant>,
    error: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct CheckpointProgressSnapshot {
    stage: DiskCheckpointStage,
    dirty_pieces: usize,
    dirty_bytes: usize,
    dirty_piece_high_water: usize,
    dirty_byte_high_water: usize,
    oldest_dirty_millis: u64,
    batches_started: usize,
    batches_completed: usize,
    pieces_completed: usize,
    sync_operations_completed: usize,
    sync_service_micros: u64,
    sync_service_max_micros: u64,
    commit_service_micros: u64,
    commit_service_max_micros: u64,
    active_micros: Option<u64>,
}

impl DownloadControl {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DownloadControlInner {
                started_at: Instant::now(),
                cancellation: CancellationToken::new(),
                buffered_payload_bytes: AtomicUsize::new(0),
                payload_high_water: AtomicUsize::new(0),
                outstanding_request_bytes: AtomicUsize::new(0),
                outstanding_request_high_water: AtomicUsize::new(0),
                requested_bytes: AtomicUsize::new(0),
                received_bytes: AtomicUsize::new(0),
                stored_bytes: AtomicUsize::new(0),
                storage_jobs_pending: AtomicUsize::new(0),
                storage_jobs_high_water: AtomicUsize::new(0),
                storage_command_queue_high_water: AtomicUsize::new(0),
                storage_completion_queue_high_water: AtomicUsize::new(0),
                storage_write_delay_millis: AtomicU64::new(0),
                storage_hash_delay_millis: AtomicU64::new(0),
                checkpoint_sync_delay_millis: AtomicU64::new(0),
                checkpoint_commit_delay_millis: AtomicU64::new(0),
                checkpoint_sync_failures: AtomicUsize::new(0),
                storage_hashes_started: AtomicUsize::new(0),
                storage_write_concurrency: AtomicUsize::new(CONTENT_STORAGE_WRITE_CONCURRENCY),
                storage_hash_concurrency: AtomicUsize::new(CONTENT_STORAGE_HASH_CONCURRENCY),
                storage_write_timing: StorageCommandTiming::default(),
                storage_hash_timing: StorageCommandTiming::default(),
                storage_write_blocks_started: AtomicUsize::new(0),
                storage_write_blocks_completed: AtomicUsize::new(0),
                storage_write_batch_blocks_high_water: AtomicUsize::new(0),
                storage_write_batch_bytes_high_water: AtomicUsize::new(0),
                storage_active: Mutex::new(StorageActiveOperations::default()),
                disk_runtime: Mutex::new(DiskRuntimeState::default()),
                last_storage_emitted_at: Mutex::new(None),
                activity_sink: Mutex::new(None),
                byte_metric_sink: Mutex::new(None),
                last_swarm_activity: Mutex::new(None),
                last_content_peers: Mutex::new((None, Vec::new())),
                peer_registry_activity: Mutex::new(PeerRegistryActivityState::default()),
                peer_connections: Mutex::new(PeerConnectionDiagnosticState::default()),
                metadata_diagnostics: Mutex::new(MetadataDiagnosticState::default()),
                storage_file_pool: Mutex::new(None),
                platform_storage: Mutex::new(None),
                safe_cancel_state: AtomicUsize::new(0),
            }),
        }
    }

    pub fn cancel(&self) {
        self.inner.cancellation.cancel();
    }

    pub fn cancel_when_safe(&self) {
        let previous = self
            .inner
            .safe_cancel_state
            .fetch_or(SAFE_CANCEL_REQUESTED, Ordering::AcqRel);
        if previous & SAFE_CANCEL_CRITICAL_MASK == 0 {
            self.cancel();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancellation.is_cancelled()
    }

    pub fn set_storage_file_pool(&self, pool: StorageFilePool) {
        *self
            .inner
            .storage_file_pool
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(pool);
    }

    pub fn set_platform_storage(&self, storage: PlatformStorageSpec) {
        *self
            .inner
            .platform_storage
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(storage);
    }

    fn storage_file_pool(&self) -> Option<StorageFilePool> {
        self.inner
            .storage_file_pool
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn platform_storage(&self) -> Option<PlatformStorageSpec> {
        self.inner
            .platform_storage
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn snapshot(&self) -> DownloadProgress {
        let write_timing = &self.inner.storage_write_timing;
        let hash_timing = &self.inner.storage_hash_timing;
        let (storage_active_write_micros, storage_active_hash_micros) = self.storage_active_ages();
        let checkpoint = self.checkpoint_progress_snapshot(Instant::now());
        DownloadProgress {
            buffered_payload_bytes: self.inner.buffered_payload_bytes.load(Ordering::Acquire),
            payload_high_water: self.inner.payload_high_water.load(Ordering::Acquire),
            outstanding_request_bytes: self.inner.outstanding_request_bytes.load(Ordering::Acquire),
            outstanding_request_high_water: self
                .inner
                .outstanding_request_high_water
                .load(Ordering::Acquire),
            requested_bytes: self.inner.requested_bytes.load(Ordering::Acquire),
            received_bytes: self.inner.received_bytes.load(Ordering::Acquire),
            stored_bytes: self.inner.stored_bytes.load(Ordering::Acquire),
            storage_jobs_pending: self.inner.storage_jobs_pending.load(Ordering::Acquire),
            storage_jobs_high_water: self.inner.storage_jobs_high_water.load(Ordering::Acquire),
            storage_command_queue_high_water: self
                .inner
                .storage_command_queue_high_water
                .load(Ordering::Acquire),
            storage_completion_queue_high_water: self
                .inner
                .storage_completion_queue_high_water
                .load(Ordering::Acquire),
            storage_hashes_started: self.inner.storage_hashes_started.load(Ordering::Acquire),
            storage_write_operations_started: write_timing.started.load(Ordering::Acquire),
            storage_write_operations_completed: write_timing.completed.load(Ordering::Acquire),
            storage_write_operations_active: write_timing.active.load(Ordering::Acquire),
            storage_write_operations_active_high_water: write_timing
                .active_high_water
                .load(Ordering::Acquire),
            storage_write_queue_wait_micros: write_timing.queue_wait_micros.load(Ordering::Acquire),
            storage_write_queue_wait_max_micros: write_timing
                .queue_wait_max_micros
                .load(Ordering::Acquire),
            storage_write_service_micros: write_timing.service_micros.load(Ordering::Acquire),
            storage_write_service_max_micros: write_timing
                .service_max_micros
                .load(Ordering::Acquire),
            storage_write_blocks_started: self
                .inner
                .storage_write_blocks_started
                .load(Ordering::Acquire),
            storage_write_blocks_completed: self
                .inner
                .storage_write_blocks_completed
                .load(Ordering::Acquire),
            storage_write_batch_blocks_high_water: self
                .inner
                .storage_write_batch_blocks_high_water
                .load(Ordering::Acquire),
            storage_write_batch_bytes_high_water: self
                .inner
                .storage_write_batch_bytes_high_water
                .load(Ordering::Acquire),
            storage_hash_operations_started: hash_timing.started.load(Ordering::Acquire),
            storage_hash_operations_completed: hash_timing.completed.load(Ordering::Acquire),
            storage_hash_operations_active: hash_timing.active.load(Ordering::Acquire),
            storage_hash_operations_active_high_water: hash_timing
                .active_high_water
                .load(Ordering::Acquire),
            storage_hash_queue_wait_micros: hash_timing.queue_wait_micros.load(Ordering::Acquire),
            storage_hash_queue_wait_max_micros: hash_timing
                .queue_wait_max_micros
                .load(Ordering::Acquire),
            storage_hash_service_micros: hash_timing.service_micros.load(Ordering::Acquire),
            storage_hash_service_max_micros: hash_timing.service_max_micros.load(Ordering::Acquire),
            storage_active_write_micros,
            storage_active_hash_micros,
            checkpoint_stage: checkpoint.stage,
            checkpoint_dirty_pieces: checkpoint.dirty_pieces,
            checkpoint_dirty_bytes: checkpoint.dirty_bytes,
            checkpoint_dirty_piece_high_water: checkpoint.dirty_piece_high_water,
            checkpoint_dirty_byte_high_water: checkpoint.dirty_byte_high_water,
            checkpoint_oldest_dirty_millis: checkpoint.oldest_dirty_millis,
            checkpoint_batches_started: checkpoint.batches_started,
            checkpoint_batches_completed: checkpoint.batches_completed,
            checkpoint_pieces_completed: checkpoint.pieces_completed,
            checkpoint_sync_operations_completed: checkpoint.sync_operations_completed,
            checkpoint_sync_service_micros: checkpoint.sync_service_micros,
            checkpoint_sync_service_max_micros: checkpoint.sync_service_max_micros,
            checkpoint_commit_service_micros: checkpoint.commit_service_micros,
            checkpoint_commit_service_max_micros: checkpoint.commit_service_max_micros,
            checkpoint_active_micros: checkpoint.active_micros,
        }
    }

    fn checkpoint_progress_snapshot(&self, now: Instant) -> CheckpointProgressSnapshot {
        let state = self
            .inner
            .disk_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let oldest_dirty_millis = state
            .pieces
            .values()
            .filter_map(|piece| piece.checkpoint_dirty_since)
            .map(|started| duration_millis(now.saturating_duration_since(started)))
            .max()
            .unwrap_or(0);
        CheckpointProgressSnapshot {
            stage: state.checkpoint_stage,
            dirty_pieces: state.checkpoint_dirty_pieces,
            dirty_bytes: state.checkpoint_dirty_bytes,
            dirty_piece_high_water: state.checkpoint_dirty_piece_high_water,
            dirty_byte_high_water: state.checkpoint_dirty_byte_high_water,
            oldest_dirty_millis,
            batches_started: state.checkpoint_batches_started,
            batches_completed: state.checkpoint_batches_completed,
            pieces_completed: state.checkpoint_pieces_completed,
            sync_operations_completed: state.checkpoint_sync_operations_completed,
            sync_service_micros: duration_micros(state.checkpoint_sync_service),
            sync_service_max_micros: duration_micros(state.checkpoint_sync_service_max),
            commit_service_micros: duration_micros(state.checkpoint_commit_service),
            commit_service_max_micros: duration_micros(state.checkpoint_commit_service_max),
            active_micros: state
                .checkpoint_active_started_at
                .map(|started| duration_micros(now.saturating_duration_since(started))),
        }
    }

    pub fn diagnostic_snapshot(&self) -> DownloadDiagnosticSnapshot {
        let progress = self.snapshot();
        let swarm = *self
            .inner
            .last_swarm_activity
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let content_registry = self
            .inner
            .peer_registry_activity
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .last_emitted
            .as_ref()
            .map(|snapshot| snapshot.counts);
        let (content_peers_captured_at, content_peers) = {
            let state = self
                .inner
                .last_content_peers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (state.0, state.1.clone())
        };
        let captured_at = self.diagnostic_elapsed();
        let peer_connections = self
            .inner
            .peer_connections
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .current
            .clone();
        let metadata = {
            let state = self
                .inner
                .metadata_diagnostics
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            MetadataAcquisitionSnapshot {
                captured_at,
                phase: state.phase,
                registry: state.registry.clone(),
                pending_dials: state.pending_dials,
                active_workers: state.active_workers,
                total_attempts: state.total_attempts,
                total_requests_sent: state.total_requests_sent,
                total_blocks_received: state.total_blocks_received,
                total_bytes_received: state.total_bytes_received,
                total_hash_failures: state.total_hash_failures,
                last_hash_failure_contributors: state.last_hash_failure_contributors,
                recent_attempts_dropped: state.recent_attempts_dropped,
                last_error: state.last_error.clone(),
                active_attempts: state.active_attempts.values().cloned().collect(),
                recent_attempts: state.recent_attempts.iter().cloned().collect(),
            }
        };
        DownloadDiagnosticSnapshot {
            progress,
            swarm,
            content_peers_captured_at,
            content_peers,
            content_registry,
            peer_connections,
            metadata,
        }
    }

    pub fn disk_snapshot(&self) -> DiskRuntimeSnapshot {
        let now = Instant::now();
        let progress = self.snapshot();
        let state = self
            .inner
            .disk_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let active_backpressure = state.backpressured_since.map_or(Duration::ZERO, |started| {
            now.saturating_duration_since(started)
        });
        let pieces = state
            .pieces
            .iter()
            .map(|(&piece_index, piece)| DiskPieceRuntimeSnapshot {
                piece_index,
                piece_length: piece.piece_length,
                attempt: piece.attempt,
                stage: piece.stage,
                requested_bytes: range_bytes(&piece.requested),
                received_bytes: range_bytes(&piece.received),
                stored_bytes: range_bytes(&piece.stored),
                age_millis: duration_millis(now.saturating_duration_since(piece.started_at)),
                stage_age_millis: duration_millis(
                    now.saturating_duration_since(piece.stage_started_at),
                ),
                error: piece.error.clone(),
            })
            .collect();
        DiskRuntimeSnapshot {
            captured_at_millis: duration_millis(self.inner.started_at.elapsed()),
            pressure: state.pressure,
            intake_backpressured: state.backpressured_since.is_some(),
            resident_limit_bytes: state.resident_limit_bytes,
            resident_high_watermark_bytes: state.high_watermark_bytes,
            resident_low_watermark_bytes: state.low_watermark_bytes,
            requested_bytes: progress.outstanding_request_bytes,
            resident_bytes: progress.buffered_payload_bytes,
            queued_write_bytes: state.queued_write_bytes,
            writing_bytes: state.writing_bytes,
            hashing_bytes: state.hashing_bytes,
            checkpoint_stage: progress.checkpoint_stage,
            checkpoint_dirty_pieces: progress.checkpoint_dirty_pieces,
            checkpoint_dirty_bytes: progress.checkpoint_dirty_bytes,
            checkpoint_dirty_piece_high_water: progress.checkpoint_dirty_piece_high_water,
            checkpoint_dirty_byte_high_water: progress.checkpoint_dirty_byte_high_water,
            checkpoint_oldest_dirty_millis: progress.checkpoint_oldest_dirty_millis,
            checkpoint_batches_started: progress.checkpoint_batches_started,
            checkpoint_batches_completed: progress.checkpoint_batches_completed,
            checkpoint_pieces_completed: progress.checkpoint_pieces_completed,
            checkpoint_sync_operations_completed: progress.checkpoint_sync_operations_completed,
            checkpoint_sync_service_micros: progress.checkpoint_sync_service_micros,
            checkpoint_sync_service_max_micros: progress.checkpoint_sync_service_max_micros,
            checkpoint_commit_service_micros: progress.checkpoint_commit_service_micros,
            checkpoint_commit_service_max_micros: progress.checkpoint_commit_service_max_micros,
            checkpoint_active_micros: progress.checkpoint_active_micros,
            storage_jobs_pending: progress.storage_jobs_pending,
            received_bytes_total: progress.received_bytes,
            stored_bytes_total: progress.stored_bytes,
            verified_bytes_total: state.verified_bytes_total,
            write_operations_started: progress.storage_write_operations_started,
            write_operations_completed: progress.storage_write_operations_completed,
            hash_operations_started: progress.storage_hash_operations_started,
            hash_operations_completed: progress.storage_hash_operations_completed,
            write_queue_wait_micros: progress.storage_write_queue_wait_micros,
            write_queue_wait_max_micros: progress.storage_write_queue_wait_max_micros,
            write_service_micros: progress.storage_write_service_micros,
            write_service_max_micros: progress.storage_write_service_max_micros,
            hash_queue_wait_micros: progress.storage_hash_queue_wait_micros,
            hash_queue_wait_max_micros: progress.storage_hash_queue_wait_max_micros,
            hash_service_micros: progress.storage_hash_service_micros,
            hash_service_max_micros: progress.storage_hash_service_max_micros,
            pressure_transition_count: state.pressure_transition_count,
            backpressured_millis_total: duration_millis(
                state
                    .backpressured_total
                    .saturating_add(active_backpressure),
            ),
            last_error: state.last_error.clone(),
            pieces,
        }
    }

    pub fn set_storage_write_delay(&self, delay: Duration) {
        let millis = delay.as_millis().try_into().unwrap_or(u64::MAX);
        self.inner
            .storage_write_delay_millis
            .store(millis, Ordering::Release);
    }

    #[cfg(test)]
    pub fn set_storage_hash_delay(&self, delay: Duration) {
        let millis = delay.as_millis().try_into().unwrap_or(u64::MAX);
        self.inner
            .storage_hash_delay_millis
            .store(millis, Ordering::Release);
    }

    #[doc(hidden)]
    pub fn set_storage_execution_limits_for_testing(
        &self,
        writes: usize,
        hashes: usize,
    ) -> Result<(), DownloadError> {
        if !(1..=CONTENT_STORAGE_MAX_DIAGNOSTIC_CONCURRENCY).contains(&writes)
            || !(1..=CONTENT_STORAGE_MAX_DIAGNOSTIC_CONCURRENCY).contains(&hashes)
        {
            return Err(DownloadError::StorageTask(format!(
                "diagnostic storage concurrency must be between 1 and {CONTENT_STORAGE_MAX_DIAGNOSTIC_CONCURRENCY}"
            )));
        }
        self.inner
            .storage_write_concurrency
            .store(writes, Ordering::Release);
        self.inner
            .storage_hash_concurrency
            .store(hashes, Ordering::Release);
        Ok(())
    }

    fn storage_execution_limits(&self) -> (usize, usize) {
        (
            self.inner.storage_write_concurrency.load(Ordering::Acquire),
            self.inner.storage_hash_concurrency.load(Ordering::Acquire),
        )
    }

    #[doc(hidden)]
    pub fn set_checkpoint_sync_delay_for_testing(&self, delay: Duration) {
        let millis = delay.as_millis().try_into().unwrap_or(u64::MAX);
        self.inner
            .checkpoint_sync_delay_millis
            .store(millis, Ordering::Release);
    }

    #[doc(hidden)]
    pub fn set_checkpoint_commit_delay_for_testing(&self, delay: Duration) {
        let millis = delay.as_millis().try_into().unwrap_or(u64::MAX);
        self.inner
            .checkpoint_commit_delay_millis
            .store(millis, Ordering::Release);
    }

    async fn enter_path_publication_stage(&self, stage: PathPublicationStage) {
        self.emit(DownloadActivityEvent::PathPublicationStage(stage));
    }

    #[cfg(test)]
    fn fail_next_checkpoint_sync(&self) {
        atomic_saturating_increment(&self.inner.checkpoint_sync_failures);
    }

    fn enter_safe_cancel_critical(&self) -> Result<SafeCancelGuard, DownloadError> {
        let mut state = self.inner.safe_cancel_state.load(Ordering::Acquire);
        loop {
            if state & SAFE_CANCEL_REQUESTED != 0 {
                return Err(DownloadError::Cancelled);
            }
            let critical_count = state & SAFE_CANCEL_CRITICAL_MASK;
            let next = critical_count
                .checked_add(1)
                .filter(|count| *count <= SAFE_CANCEL_CRITICAL_MASK)
                .ok_or_else(|| {
                    DownloadError::Checkpoint(
                        "safe-cancellation critical-section overflow".to_owned(),
                    )
                })?;
            match self.inner.safe_cancel_state.compare_exchange_weak(
                state,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Ok(SafeCancelGuard {
                        control: self.clone(),
                    });
                }
                Err(actual) => state = actual,
            }
        }
    }

    pub fn set_activity_sink(&self, sink: Arc<dyn DownloadActivitySink>) {
        *self
            .inner
            .activity_sink
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(sink);
    }

    pub fn set_byte_metric_sink(&self, sink: Arc<dyn ByteMetricSink>) {
        *self
            .inner
            .byte_metric_sink
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(sink);
    }

    fn byte_metric_sink(&self) -> Option<SharedByteMetricSink> {
        self.inner
            .byte_metric_sink
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn record_bytes(&self, metric: ByteMetric, bytes: usize) {
        if bytes == 0 {
            return;
        }
        if let Some(sink) = self.byte_metric_sink() {
            sink.record(metric, bytes.try_into().unwrap_or(u64::MAX));
        }
    }

    fn emit(&self, event: DownloadActivityEvent) {
        let sink = self
            .inner
            .activity_sink
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if let Some(sink) = sink {
            sink.record(event);
        }
    }

    fn diagnostic_elapsed(&self) -> Duration {
        self.inner.started_at.elapsed()
    }

    fn metadata_started(&self) {
        let mut state = self
            .inner
            .metadata_diagnostics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *state = MetadataDiagnosticState {
            phase: MetadataAcquisitionPhase::Discovering,
            ..MetadataDiagnosticState::default()
        };
    }

    fn observe_metadata_supervisor(
        &self,
        registry: PeerRegistrySnapshot,
        pending_dials: usize,
        active_workers: usize,
        last_error: Option<&DownloadError>,
    ) {
        let mut state = self
            .inner
            .metadata_diagnostics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.registry = Some(registry);
        state.pending_dials = pending_dials;
        state.active_workers = active_workers;
        if let Some(error) = last_error {
            state.last_error = Some(bounded_diagnostic_detail(&error.to_string()));
        }
        if pending_dials != 0 || active_workers != 0 {
            state.phase = MetadataAcquisitionPhase::Acquiring;
        } else if !matches!(
            state.phase,
            MetadataAcquisitionPhase::Complete
                | MetadataAcquisitionPhase::Failed
                | MetadataAcquisitionPhase::Cancelled
        ) {
            state.phase = MetadataAcquisitionPhase::Discovering;
        }
    }

    fn metadata_dial_started(&self, attempt: DialAttempt) {
        let now = self.diagnostic_elapsed();
        let mut state = self
            .inner
            .metadata_diagnostics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.total_attempts = state.total_attempts.saturating_add(1);
        state.active_attempts.insert(
            attempt.id(),
            MetadataPeerSnapshot {
                record_id: attempt.record_id().get(),
                attempt_id: attempt.id().get(),
                endpoint: attempt.endpoint().address(),
                stage: MetadataPeerStage::Dialing,
                started_at: now,
                last_activity_at: now,
                last_progress_at: now,
                supports_extensions: None,
                remote_metadata_id: None,
                metadata_size: None,
                metadata_blocks: None,
                requests_sent: 0,
                pending_requests: 0,
                blocks_received: 0,
                bytes_received: 0,
                messages_received: 0,
                rejects_received: 0,
                terminal_detail: None,
            },
        );
    }

    fn metadata_peer_connected(&self, attempt: DialAttempt, supports_extensions: bool) {
        let now = self.diagnostic_elapsed();
        self.update_metadata_peer(attempt.id(), |peer| {
            peer.stage = MetadataPeerStage::AwaitingExtensionHandshake;
            peer.supports_extensions = Some(supports_extensions);
            peer.last_activity_at = now;
            peer.last_progress_at = now;
        });
    }

    fn metadata_peer_message(&self, attempt_id: DialAttemptId) {
        let now = self.diagnostic_elapsed();
        self.update_metadata_peer(attempt_id, |peer| {
            peer.messages_received = peer.messages_received.saturating_add(1);
            peer.last_activity_at = now;
        });
    }

    fn metadata_extension_handshake(
        &self,
        attempt_id: DialAttemptId,
        remote_metadata_id: Option<u8>,
        download: &TorrentMetadataDownload,
        requests_sent: usize,
    ) {
        let now = self.diagnostic_elapsed();
        let mut state = self
            .inner
            .metadata_diagnostics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.total_requests_sent = state.total_requests_sent.saturating_add(requests_sent);
        if let Some(peer) = state.active_attempts.get_mut(&attempt_id) {
            peer.stage = MetadataPeerStage::Requesting;
            peer.remote_metadata_id = remote_metadata_id;
            peer.metadata_size = download.metadata_size();
            peer.metadata_blocks = metadata_block_count_for_diagnostics(download);
            peer.pending_requests = download
                .pending_requests_for_peer(attempt_id.get())
                .unwrap_or(0);
            peer.requests_sent = peer.requests_sent.saturating_add(requests_sent);
            peer.last_activity_at = now;
            peer.last_progress_at = now;
        }
    }

    fn metadata_block_received(
        &self,
        attempt_id: DialAttemptId,
        block_bytes: usize,
        download: &TorrentMetadataDownload,
        requests_sent: usize,
    ) {
        let now = self.diagnostic_elapsed();
        let mut state = self
            .inner
            .metadata_diagnostics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.total_requests_sent = state.total_requests_sent.saturating_add(requests_sent);
        state.total_blocks_received = state.total_blocks_received.saturating_add(1);
        state.total_bytes_received = state.total_bytes_received.saturating_add(block_bytes);
        if let Some(peer) = state.active_attempts.get_mut(&attempt_id) {
            peer.stage = MetadataPeerStage::Requesting;
            peer.metadata_size = download.metadata_size();
            peer.metadata_blocks = metadata_block_count_for_diagnostics(download);
            peer.pending_requests = download
                .pending_requests_for_peer(attempt_id.get())
                .unwrap_or(0);
            peer.requests_sent = peer.requests_sent.saturating_add(requests_sent);
            peer.blocks_received = peer.blocks_received.saturating_add(1);
            peer.bytes_received = peer.bytes_received.saturating_add(block_bytes);
            peer.last_activity_at = now;
            peer.last_progress_at = now;
        }
    }

    fn metadata_requests_sent(
        &self,
        attempt_id: DialAttemptId,
        download: &TorrentMetadataDownload,
        requests_sent: usize,
    ) {
        if requests_sent == 0 {
            return;
        }
        let now = self.diagnostic_elapsed();
        let mut state = self
            .inner
            .metadata_diagnostics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.total_requests_sent = state.total_requests_sent.saturating_add(requests_sent);
        if let Some(peer) = state.active_attempts.get_mut(&attempt_id) {
            peer.stage = MetadataPeerStage::Requesting;
            peer.metadata_size = download.metadata_size();
            peer.metadata_blocks = metadata_block_count_for_diagnostics(download);
            peer.pending_requests = download
                .pending_requests_for_peer(attempt_id.get())
                .unwrap_or(0);
            peer.requests_sent = peer.requests_sent.saturating_add(requests_sent);
            peer.last_activity_at = now;
        }
    }

    fn metadata_rejected(&self, attempt_id: DialAttemptId) {
        let now = self.diagnostic_elapsed();
        self.update_metadata_peer(attempt_id, |peer| {
            peer.rejects_received = peer.rejects_received.saturating_add(1);
            peer.last_activity_at = now;
        });
    }

    fn metadata_hash_failed(&self, contributors: usize) {
        let mut state = self
            .inner
            .metadata_diagnostics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.total_hash_failures = state.total_hash_failures.saturating_add(1);
        state.last_hash_failure_contributors = contributors;
    }

    fn metadata_peer_finished(
        &self,
        attempt_id: DialAttemptId,
        stage: MetadataPeerStage,
        detail: Option<&str>,
    ) {
        let now = self.diagnostic_elapsed();
        let mut state = self
            .inner
            .metadata_diagnostics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(mut peer) = state.active_attempts.remove(&attempt_id) else {
            return;
        };
        peer.stage = stage;
        peer.last_activity_at = now;
        peer.terminal_detail = detail.map(bounded_diagnostic_detail);
        if stage == MetadataPeerStage::Failed {
            state.last_error = peer.terminal_detail.clone();
        }
        push_recent_metadata_attempt(&mut state, peer);
    }

    fn metadata_finished(&self, result: &Result<(Vec<u8>, Metainfo), DownloadError>) {
        let now = self.diagnostic_elapsed();
        let mut state = self
            .inner
            .metadata_diagnostics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (phase, detail, peer_stage) = match result {
            Ok(_) => (
                MetadataAcquisitionPhase::Complete,
                None,
                MetadataPeerStage::Cancelled,
            ),
            Err(DownloadError::Cancelled) => (
                MetadataAcquisitionPhase::Cancelled,
                Some("download cancelled".to_owned()),
                MetadataPeerStage::Cancelled,
            ),
            Err(error) => (
                MetadataAcquisitionPhase::Failed,
                Some(bounded_diagnostic_detail(&error.to_string())),
                MetadataPeerStage::Failed,
            ),
        };
        state.phase = phase;
        state.last_error = detail.clone();
        state.pending_dials = 0;
        state.active_workers = 0;
        let active = std::mem::take(&mut state.active_attempts);
        for (_, mut peer) in active {
            peer.stage = peer_stage;
            peer.last_activity_at = now;
            peer.terminal_detail = detail.clone();
            push_recent_metadata_attempt(&mut state, peer);
        }
    }

    fn update_metadata_peer(
        &self,
        attempt_id: DialAttemptId,
        update: impl FnOnce(&mut MetadataPeerSnapshot),
    ) {
        let mut state = self
            .inner
            .metadata_diagnostics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(peer) = state.active_attempts.get_mut(&attempt_id) {
            update(peer);
        }
    }

    fn observe_swarm(&self, swarm: &SwarmState, now: Duration) {
        let snapshot = swarm.snapshot(now);
        {
            let mut state = self
                .inner
                .last_content_peers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let due = state.0.is_none_or(|captured| {
                now.saturating_sub(captured) >= CONTENT_PEER_DIAGNOSTIC_INTERVAL
            });
            if due || snapshot.no_request_reason == Some(NoRequestReason::Complete) {
                state.0 = Some(now);
                state.1 = swarm
                    .connection_activity(now)
                    .into_iter()
                    .map(|peer| ContentPeerActivitySnapshot {
                        connection_id: peer.id.get(),
                        choking: peer.choking,
                        wanted_piece_count: peer.wanted_piece_count,
                        pending_requests: peer.pending_requests,
                        target_requests: peer.target_requests,
                        queued_payload_bytes: peer.queued_payload_bytes,
                        window_phase: match peer.window_phase {
                            ConnectionWindowPhaseSnapshot::SlowStart => {
                                ContentRequestWindowPhase::SlowStart
                            }
                            ConnectionWindowPhaseSnapshot::Steady => {
                                ContentRequestWindowPhase::Steady
                            }
                            ConnectionWindowPhaseSnapshot::Stalled => {
                                ContentRequestWindowPhase::Stalled
                            }
                        },
                        useful_payload_bytes: peer.useful_payload_bytes,
                        observed_payload_rate: peer.observed_payload_rate,
                        connected_age_seconds: peer.connected_age.as_secs(),
                        last_useful_age_seconds: peer.last_useful_age.map(|value| value.as_secs()),
                        last_payload_age_seconds: peer
                            .last_payload_age
                            .map(|value| value.as_secs()),
                        request_timeout_seconds: peer.request_timeout.as_secs(),
                        oldest_request_age_seconds: peer
                            .oldest_request_age
                            .map(|value| value.as_secs()),
                    })
                    .collect();
            }
        }
        self.inner
            .outstanding_request_bytes
            .store(snapshot.outstanding_request_bytes, Ordering::Release);
        self.inner
            .outstanding_request_high_water
            .fetch_max(snapshot.outstanding_request_high_water, Ordering::AcqRel);
        let activity = SwarmActivitySnapshot {
            pending_dials: snapshot.pending_dials,
            connected_peers: snapshot.connected_peers,
            unchoked_peers: snapshot.unchoked_peers,
            missing_blocks: snapshot.missing_blocks,
            requested_blocks: snapshot.requested_blocks,
            active_request_attempts: snapshot.active_request_attempts,
            active_duplicate_attempts: snapshot.active_duplicate_attempts,
            writing_blocks: snapshot.writing_blocks,
            received_blocks: snapshot.received_blocks,
            verified_blocks: snapshot.verified_blocks,
            active_piece_count: snapshot.active_piece_count,
            active_piece_bytes: snapshot.active_piece_bytes,
            outstanding_request_bytes: snapshot.outstanding_request_bytes,
            outstanding_request_high_water: snapshot.outstanding_request_high_water,
            request_target_total: snapshot.request_target_total,
            request_target_max: snapshot.request_target_max,
            slow_start_peers: snapshot.slow_start_peers,
            stalled_peers: snapshot.stalled_peers,
            useful_payload_bytes: snapshot.useful_payload_bytes,
            observed_payload_rate: snapshot.observed_payload_rate,
            endgame_assignments: snapshot.endgame_assignments,
            cancelled_request_attempts: snapshot.cancelled_request_attempts,
            redundant_payload_bytes: snapshot.redundant_payload_bytes,
            piece_hash_failures: snapshot.piece_hash_failures,
            failed_piece_bytes: snapshot.failed_piece_bytes,
            last_hash_failure_contributors: snapshot.last_hash_failure_contributors,
            request_timeout_min_seconds: snapshot.request_timeout_min.map(|value| value.as_secs()),
            request_timeout_max_seconds: snapshot.request_timeout_max.map(|value| value.as_secs()),
            oldest_request_age_seconds: snapshot.oldest_request_age.map(|age| age.as_secs()),
            next_request_expiry_seconds: snapshot
                .next_request_expiry
                .map(|deadline| deadline.saturating_sub(now).as_secs()),
            next_replacement_seconds: snapshot
                .next_replacement_at
                .map(|deadline| deadline.saturating_sub(now).as_secs()),
            no_request_reason: snapshot.no_request_reason,
        };
        let changed = {
            let mut previous = self
                .inner
                .last_swarm_activity
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if previous.as_ref() == Some(&activity) {
                false
            } else {
                *previous = Some(activity);
                true
            }
        };
        if changed {
            self.emit(DownloadActivityEvent::SwarmState(Box::new(activity)));
        }
    }

    fn observe_peer_registry(
        &self,
        registry: &PeerRegistry,
        now: Duration,
        active: bool,
        force: bool,
    ) {
        let event = {
            let mut state = self
                .inner
                .peer_registry_activity
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if !force
                && state.active == active
                && state
                    .next_transition_at
                    .is_none_or(|deadline| now < deadline)
            {
                return;
            }

            let mut snapshot = registry.snapshot(PeerSelectionContext { now });
            if !active {
                snapshot.records.clear();
                snapshot.counts = PeerRegistryCounts::default();
            }
            state.next_transition_at = active
                .then(|| {
                    snapshot
                        .records
                        .iter()
                        .filter_map(|record| match record.eligibility {
                            crate::peer::DialEligibility::Backoff { retry_at }
                                if retry_at > now =>
                            {
                                Some(retry_at)
                            }
                            _ => None,
                        })
                        .min()
                })
                .flatten();
            let changed = state.active != active
                || state.last_emitted.as_ref().is_none_or(|previous| {
                    previous.maximum_records != snapshot.maximum_records
                        || previous.counts != snapshot.counts
                        || previous.records != snapshot.records
                });
            state.active = active;
            if changed {
                state.last_emitted = Some(snapshot.clone());
                Some(DownloadActivityEvent::PeerRegistryState {
                    active,
                    snapshot: Box::new(snapshot),
                })
            } else {
                None
            }
        };
        if let Some(event) = event {
            self.emit(event);
        }
    }

    fn observe_peer_runtime(&self, runtime: &PeerRuntime, captured_at: Duration, force: bool) {
        let current = runtime.snapshot();
        let emit = {
            let mut state = self
                .inner
                .peer_connections
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.current = current.clone();
            let due = force
                || state.last_emitted_at.is_none_or(|previous| {
                    captured_at.saturating_sub(previous) >= PEER_OBSERVATION_INTERVAL
                });
            if due && state.last_emitted != current {
                state.last_emitted = current.clone();
                state.last_emitted_at = Some(captured_at);
                true
            } else {
                false
            }
        };
        if emit {
            self.emit(DownloadActivityEvent::PeerConnections {
                captured_at,
                peers: Box::new(current),
            });
        }
    }

    fn configure_disk_runtime(&self, resident_limit_bytes: usize) {
        let mut state = self
            .inner
            .disk_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.resident_limit_bytes = resident_limit_bytes;
        state.high_watermark_bytes = resident_limit_bytes
            .saturating_mul(3)
            .checked_div(4)
            .unwrap_or(resident_limit_bytes)
            .max(1);
        state.low_watermark_bytes = resident_limit_bytes.checked_div(2).unwrap_or_default();
        state.pressure = DiskPressure::Normal;
        drop(state);
        self.emit_storage_state_force();
    }

    fn storage_backpressured(&self) -> bool {
        self.inner
            .disk_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .backpressured_since
            .is_some()
    }

    fn update_disk_pressure(&self, resident_bytes: usize) {
        let now = Instant::now();
        let mut state = self
            .inner
            .disk_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.resident_limit_bytes == 0 || state.pressure == DiskPressure::Error {
            return;
        }
        let was_backpressured = state.backpressured_since.is_some();
        if !was_backpressured && resident_bytes >= state.high_watermark_bytes {
            state.backpressured_since = Some(now);
            state.pressure = DiskPressure::Backpressured;
            state.pressure_transition_count = state.pressure_transition_count.saturating_add(1);
        } else if was_backpressured && resident_bytes <= state.low_watermark_bytes {
            if let Some(started) = state.backpressured_since.take() {
                state.backpressured_total = state
                    .backpressured_total
                    .saturating_add(now.saturating_duration_since(started));
            }
            state.pressure = if resident_bytes == 0 && state.pieces.is_empty() {
                DiskPressure::Idle
            } else {
                DiskPressure::Draining
            };
            state.pressure_transition_count = state.pressure_transition_count.saturating_add(1);
        } else if !was_backpressured {
            state.pressure = if resident_bytes == 0 && state.pieces.is_empty() {
                DiskPressure::Idle
            } else {
                DiskPressure::Normal
            };
        }
    }

    fn disk_block_requested(&self, block: BlockKey, piece_length: u32) -> (u32, bool) {
        self.inner
            .requested_bytes
            .fetch_add(block.length as usize, Ordering::AcqRel);
        let attempt = self.mutate_disk_piece(block.piece, piece_length, |piece, now| {
            if piece.stage == DiskPieceStage::Failed {
                piece.attempt = piece.attempt.saturating_add(1);
                piece.requested.clear();
                piece.received.clear();
                piece.stored.clear();
                piece.started_at = now;
                piece.checkpoint_dirty_since = None;
                piece.error = None;
            }
            let started =
                piece.requested.is_empty() && piece.received.is_empty() && piece.stored.is_empty();
            insert_disk_range(&mut piece.requested, block.begin, block.length);
            set_disk_piece_stage(piece, DiskPieceStage::Receiving, now);
            (piece.attempt, started)
        });
        self.emit_storage_state();
        attempt
    }

    fn disk_block_received(&self, block: BlockKey, piece_length: u32) {
        self.inner
            .received_bytes
            .fetch_add(block.length as usize, Ordering::AcqRel);
        self.mutate_disk_piece(block.piece, piece_length, |piece, now| {
            insert_disk_range(&mut piece.received, block.begin, block.length);
            set_disk_piece_stage(piece, DiskPieceStage::Queued, now);
        });
        self.emit_storage_state();
    }

    fn disk_block_stored(&self, block: BlockKey, piece_length: u32) {
        self.inner
            .stored_bytes
            .fetch_add(block.length as usize, Ordering::AcqRel);
        self.mutate_disk_piece(block.piece, piece_length, |piece, now| {
            insert_disk_range(&mut piece.stored, block.begin, block.length);
            let stage = if piece.active_write_jobs > 0 {
                DiskPieceStage::Writing
            } else if range_bytes(&piece.stored) >= piece.piece_length {
                DiskPieceStage::Stored
            } else {
                DiskPieceStage::Receiving
            };
            set_disk_piece_stage(piece, stage, now);
        });
        self.emit_storage_state();
    }

    fn disk_piece_hashing(&self, piece_index: u32, piece_length: u32) {
        {
            let mut state = self
                .inner
                .disk_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.hashing_bytes = state.hashing_bytes.saturating_add(piece_length as usize);
        }
        self.mutate_disk_piece(piece_index, piece_length, |piece, now| {
            set_disk_piece_stage(piece, DiskPieceStage::Hashing, now);
        });
        self.emit_storage_state();
    }

    fn disk_piece_hash_verified(
        &self,
        piece_index: u32,
        piece_length: u32,
        requires_checkpoint: bool,
    ) {
        let now = Instant::now();
        let mut state = self
            .inner
            .disk_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.hashing_bytes = state.hashing_bytes.saturating_sub(piece_length as usize);
        state.verified_bytes_total = state
            .verified_bytes_total
            .saturating_add(piece_length as usize);
        if requires_checkpoint {
            let newly_dirty = state.pieces.get_mut(&piece_index).is_some_and(|piece| {
                let newly_dirty = piece.checkpoint_dirty_since.is_none();
                piece.checkpoint_dirty_since.get_or_insert(now);
                set_disk_piece_stage(piece, DiskPieceStage::CheckpointDirty, now);
                newly_dirty
            });
            if newly_dirty {
                state.checkpoint_dirty_pieces = state.checkpoint_dirty_pieces.saturating_add(1);
                state.checkpoint_dirty_bytes = state
                    .checkpoint_dirty_bytes
                    .saturating_add(piece_length as usize);
                state.checkpoint_dirty_piece_high_water = state
                    .checkpoint_dirty_piece_high_water
                    .max(state.checkpoint_dirty_pieces);
                state.checkpoint_dirty_byte_high_water = state
                    .checkpoint_dirty_byte_high_water
                    .max(state.checkpoint_dirty_bytes);
            }
        } else {
            state.pieces.remove(&piece_index);
        }
        let resident = self.inner.buffered_payload_bytes.load(Ordering::Acquire);
        drop(state);
        self.update_disk_pressure(resident);
        self.emit_storage_state();
    }

    fn disk_checkpoint_sync_started(&self, batch: &CheckpointBatch) {
        let now = Instant::now();
        let mut state = self
            .inner
            .disk_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.checkpoint_stage = DiskCheckpointStage::Syncing;
        state.checkpoint_active_started_at = Some(now);
        state.checkpoint_batches_started = state.checkpoint_batches_started.saturating_add(1);
        for intent in &batch.intents {
            if let Ok(piece_index) = u32::try_from(intent.piece_index)
                && let Some(piece) = state.pieces.get_mut(&piece_index)
            {
                set_disk_piece_stage(piece, DiskPieceStage::CheckpointSyncing, now);
            }
        }
        drop(state);
        self.emit_storage_state_force();
    }

    fn disk_checkpoint_sync_completed(&self, batch: &CheckpointBatch, elapsed: Duration) {
        let now = Instant::now();
        let mut state = self
            .inner
            .disk_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.checkpoint_sync_operations_completed = state
            .checkpoint_sync_operations_completed
            .saturating_add(batch.targets.len());
        state.checkpoint_sync_service = state.checkpoint_sync_service.saturating_add(elapsed);
        state.checkpoint_sync_service_max = state.checkpoint_sync_service_max.max(elapsed);
        state.checkpoint_stage = DiskCheckpointStage::Committing;
        state.checkpoint_active_started_at = Some(now);
        for intent in &batch.intents {
            if let Ok(piece_index) = u32::try_from(intent.piece_index)
                && let Some(piece) = state.pieces.get_mut(&piece_index)
            {
                set_disk_piece_stage(piece, DiskPieceStage::CheckpointCommitting, now);
            }
        }
        drop(state);
        self.emit_storage_state_force();
    }

    fn disk_checkpoint_completed(&self, batch: &CheckpointBatch, elapsed: Duration) {
        let mut state = self
            .inner
            .disk_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.checkpoint_commit_service = state.checkpoint_commit_service.saturating_add(elapsed);
        state.checkpoint_commit_service_max = state.checkpoint_commit_service_max.max(elapsed);
        state.checkpoint_batches_completed = state.checkpoint_batches_completed.saturating_add(1);
        state.checkpoint_pieces_completed = state
            .checkpoint_pieces_completed
            .saturating_add(batch.intents.len());
        state.checkpoint_dirty_pieces = state
            .checkpoint_dirty_pieces
            .saturating_sub(batch.intents.len());
        let batch_bytes = usize::try_from(batch.dirty_bytes).unwrap_or(usize::MAX);
        state.checkpoint_dirty_bytes = state.checkpoint_dirty_bytes.saturating_sub(batch_bytes);
        for intent in &batch.intents {
            if let Ok(piece_index) = u32::try_from(intent.piece_index) {
                state.pieces.remove(&piece_index);
            }
        }
        state.checkpoint_stage = DiskCheckpointStage::Idle;
        state.checkpoint_active_started_at = None;
        let resident = self.inner.buffered_payload_bytes.load(Ordering::Acquire);
        drop(state);
        self.update_disk_pressure(resident);
        self.emit_storage_state_force();
    }

    fn disk_checkpoint_failed(&self, batch: &CheckpointBatch, elapsed: Duration, detail: &str) {
        let now = Instant::now();
        let mut state = self
            .inner
            .disk_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match state.checkpoint_stage {
            DiskCheckpointStage::Syncing => {
                state.checkpoint_sync_service =
                    state.checkpoint_sync_service.saturating_add(elapsed);
                state.checkpoint_sync_service_max = state.checkpoint_sync_service_max.max(elapsed);
            }
            DiskCheckpointStage::Committing => {
                state.checkpoint_commit_service =
                    state.checkpoint_commit_service.saturating_add(elapsed);
                state.checkpoint_commit_service_max =
                    state.checkpoint_commit_service_max.max(elapsed);
            }
            DiskCheckpointStage::Idle | DiskCheckpointStage::Error => {}
        }
        let detail = bounded_diagnostic_detail(detail);
        state.checkpoint_stage = DiskCheckpointStage::Error;
        state.checkpoint_active_started_at = None;
        state.pressure = DiskPressure::Error;
        state.last_error = Some(detail.clone());
        for intent in &batch.intents {
            if let Ok(piece_index) = u32::try_from(intent.piece_index)
                && let Some(piece) = state.pieces.get_mut(&piece_index)
            {
                piece.error = Some(detail.clone());
                set_disk_piece_stage(piece, DiskPieceStage::Failed, now);
            }
        }
        drop(state);
        self.emit_storage_state_force();
    }

    fn disk_piece_failed(&self, piece_index: u32, piece_length: u32, detail: &str) {
        {
            let mut state = self
                .inner
                .disk_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.hashing_bytes = state.hashing_bytes.saturating_sub(piece_length as usize);
        }
        self.mutate_disk_piece(piece_index, piece_length, |piece, now| {
            piece.error = Some(bounded_diagnostic_detail(detail));
            set_disk_piece_stage(piece, DiskPieceStage::Failed, now);
        });
        self.emit_storage_state_force();
    }

    fn disk_storage_error(&self, detail: &str) {
        let mut state = self
            .inner
            .disk_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.pressure = DiskPressure::Error;
        state.last_error = Some(bounded_diagnostic_detail(detail));
        if let Some(started) = state.backpressured_since.take() {
            state.backpressured_total = state
                .backpressured_total
                .saturating_add(Instant::now().saturating_duration_since(started));
        }
        drop(state);
        self.emit_storage_state_force();
    }

    fn mutate_disk_piece<T>(
        &self,
        piece_index: u32,
        piece_length: u32,
        update: impl FnOnce(&mut DiskPieceRuntimeState, Instant) -> T,
    ) -> T {
        let now = Instant::now();
        let mut state = self
            .inner
            .disk_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let piece = state
            .pieces
            .entry(piece_index)
            .or_insert_with(|| DiskPieceRuntimeState {
                piece_length,
                attempt: 1,
                stage: DiskPieceStage::Receiving,
                requested: Vec::new(),
                received: Vec::new(),
                stored: Vec::new(),
                active_write_jobs: 0,
                started_at: now,
                stage_started_at: now,
                checkpoint_dirty_since: None,
                error: None,
            });
        piece.piece_length = piece_length;
        update(piece, now)
    }

    fn disk_write_batch_started(&self, blocks: &[BlockKey], bytes: usize) {
        let now = Instant::now();
        let mut state = self
            .inner
            .disk_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.queued_write_bytes = state.queued_write_bytes.saturating_sub(bytes);
        state.writing_bytes = state.writing_bytes.saturating_add(bytes);
        let mut pieces = BTreeSet::new();
        for block in blocks {
            if pieces.insert(block.piece)
                && let Some(piece) = state.pieces.get_mut(&block.piece)
            {
                piece.active_write_jobs = piece.active_write_jobs.saturating_add(1);
                set_disk_piece_stage(piece, DiskPieceStage::Writing, now);
            }
        }
        drop(state);
        self.emit_storage_state();
    }

    fn disk_write_batch_completed(&self, blocks: &[BlockKey], bytes: usize) {
        let mut state = self
            .inner
            .disk_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.writing_bytes = state.writing_bytes.saturating_sub(bytes);
        let mut pieces = BTreeSet::new();
        for block in blocks {
            if pieces.insert(block.piece)
                && let Some(piece) = state.pieces.get_mut(&block.piece)
            {
                piece.active_write_jobs = piece.active_write_jobs.saturating_sub(1);
            }
        }
        drop(state);
        self.emit_storage_state();
    }

    fn emit_storage_state(&self) {
        self.emit_storage_state_inner(false);
    }

    fn emit_storage_state_force(&self) {
        self.emit_storage_state_inner(true);
    }

    fn emit_storage_state_inner(&self, force: bool) {
        let now = Instant::now();
        let mut last_emitted_at = self
            .inner
            .last_storage_emitted_at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !force
            && last_emitted_at.is_some_and(|previous| {
                now.saturating_duration_since(previous) < STORAGE_OBSERVATION_INTERVAL
            })
        {
            return;
        }
        *last_emitted_at = Some(now);
        self.emit(DownloadActivityEvent::StorageState(Box::new(
            self.disk_snapshot(),
        )));
    }

    fn storage_job_started(&self) {
        let pending = self
            .inner
            .storage_jobs_pending
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        self.inner
            .storage_jobs_high_water
            .fetch_max(pending, Ordering::AcqRel);
    }

    fn storage_job_finished(&self) {
        let previous = self
            .inner
            .storage_jobs_pending
            .fetch_sub(1, Ordering::AcqRel);
        debug_assert_ne!(previous, 0);
    }

    fn storage_jobs_at_limit(&self, limit: usize) -> bool {
        self.inner.storage_jobs_pending.load(Ordering::Acquire) >= limit
    }

    fn try_buffer_payload(&self, bytes: usize, limit: usize) -> bool {
        let mut current = self.inner.buffered_payload_bytes.load(Ordering::Acquire);
        loop {
            let Some(next) = current.checked_add(bytes) else {
                return false;
            };
            if next > limit {
                return false;
            }
            match self.inner.buffered_payload_bytes.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    self.inner
                        .payload_high_water
                        .fetch_max(next, Ordering::AcqRel);
                    {
                        let mut state = self
                            .inner
                            .disk_runtime
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        state.queued_write_bytes = state.queued_write_bytes.saturating_add(bytes);
                    }
                    self.update_disk_pressure(next);
                    self.emit_storage_state();
                    return true;
                }
                Err(actual) => current = actual,
            }
        }
    }

    fn release_buffered_payload(&self, bytes: usize) {
        let previous = self
            .inner
            .buffered_payload_bytes
            .fetch_sub(bytes, Ordering::AcqRel);
        debug_assert!(previous >= bytes);
        self.update_disk_pressure(previous.saturating_sub(bytes));
        self.emit_storage_state();
    }

    fn abandon_queued_payload(&self, bytes: usize) {
        {
            let mut state = self
                .inner
                .disk_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.queued_write_bytes = state.queued_write_bytes.saturating_sub(bytes);
        }
        self.release_buffered_payload(bytes);
    }

    fn observe_storage_command_queue(&self, depth: usize) {
        self.inner
            .storage_command_queue_high_water
            .fetch_max(depth, Ordering::AcqRel);
    }

    fn observe_storage_completion_queue(&self, depth: usize) {
        self.inner
            .storage_completion_queue_high_water
            .fetch_max(depth, Ordering::AcqRel);
    }

    fn storage_command_started(
        &self,
        kind: StorageCommandKind,
        enqueued_at: Instant,
        started_at: Instant,
    ) {
        let timing = self.storage_timing(kind);
        let queue_wait_micros = duration_micros(
            started_at
                .checked_duration_since(enqueued_at)
                .unwrap_or_default(),
        );
        atomic_saturating_increment(&timing.started);
        let active = timing
            .active
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        timing.active_high_water.fetch_max(active, Ordering::AcqRel);
        atomic_saturating_add(&timing.queue_wait_micros, queue_wait_micros);
        timing
            .queue_wait_max_micros
            .fetch_max(queue_wait_micros, Ordering::AcqRel);
        let mut active = self
            .inner
            .storage_active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match kind {
            StorageCommandKind::Write => active.writes.push(started_at),
            StorageCommandKind::Hash => active.hashes.push(started_at),
        }
    }

    fn storage_command_completed(
        &self,
        kind: StorageCommandKind,
        started_at: Instant,
        completed_at: Instant,
    ) {
        let timing = self.storage_timing(kind);
        let service_micros = duration_micros(
            completed_at
                .checked_duration_since(started_at)
                .unwrap_or_default(),
        );
        atomic_saturating_add(&timing.service_micros, service_micros);
        timing
            .service_max_micros
            .fetch_max(service_micros, Ordering::AcqRel);
        atomic_saturating_increment(&timing.completed);
        let previous = timing.active.fetch_sub(1, Ordering::AcqRel);
        debug_assert_ne!(previous, 0);
        let mut active = self
            .inner
            .storage_active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let operations = match kind {
            StorageCommandKind::Write => &mut active.writes,
            StorageCommandKind::Hash => &mut active.hashes,
        };
        if let Some(index) = operations
            .iter()
            .position(|candidate| *candidate == started_at)
        {
            operations.swap_remove(index);
        } else {
            debug_assert!(false, "completed storage operation was not active");
        }
    }

    fn storage_write_batch_started(
        &self,
        enqueued_at: Instant,
        started_at: Instant,
        blocks: &[BlockKey],
        bytes: usize,
    ) {
        self.storage_command_started(StorageCommandKind::Write, enqueued_at, started_at);
        atomic_saturating_add_usize(&self.inner.storage_write_blocks_started, blocks.len());
        self.inner
            .storage_write_batch_blocks_high_water
            .fetch_max(blocks.len(), Ordering::AcqRel);
        self.inner
            .storage_write_batch_bytes_high_water
            .fetch_max(bytes, Ordering::AcqRel);
        self.disk_write_batch_started(blocks, bytes);
    }

    fn storage_write_batch_completed(
        &self,
        started_at: Instant,
        completed_at: Instant,
        blocks: &[BlockKey],
        bytes: usize,
    ) {
        atomic_saturating_add_usize(&self.inner.storage_write_blocks_completed, blocks.len());
        self.storage_command_completed(StorageCommandKind::Write, started_at, completed_at);
        self.disk_write_batch_completed(blocks, bytes);
    }

    fn storage_timing(&self, kind: StorageCommandKind) -> &StorageCommandTiming {
        match kind {
            StorageCommandKind::Write => &self.inner.storage_write_timing,
            StorageCommandKind::Hash => &self.inner.storage_hash_timing,
        }
    }

    fn storage_active_ages(&self) -> (Option<u64>, Option<u64>) {
        let active = self
            .inner
            .storage_active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = Instant::now();
        let oldest_age = |operations: &[Instant]| {
            operations
                .iter()
                .min()
                .map(|started| duration_micros(now.saturating_duration_since(*started)))
        };
        (oldest_age(&active.writes), oldest_age(&active.hashes))
    }

    fn clear_storage_active_operations(&self) {
        self.inner
            .storage_write_timing
            .active
            .store(0, Ordering::Release);
        self.inner
            .storage_hash_timing
            .active
            .store(0, Ordering::Release);
        let mut active = self
            .inner
            .storage_active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        active.writes.clear();
        active.hashes.clear();
    }

    fn clear_storage_jobs(&self) {
        self.inner.storage_jobs_pending.store(0, Ordering::Release);
        self.clear_storage_active_operations();
        let mut state = self
            .inner
            .disk_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.queued_write_bytes = 0;
        state.writing_bytes = 0;
        state.hashing_bytes = 0;
        state.checkpoint_stage = DiskCheckpointStage::Idle;
        state.checkpoint_active_started_at = None;
        state.checkpoint_dirty_pieces = 0;
        state.checkpoint_dirty_bytes = 0;
        state.pieces.clear();
        state.pressure = DiskPressure::Idle;
        if let Some(started) = state.backpressured_since.take() {
            state.backpressured_total = state
                .backpressured_total
                .saturating_add(Instant::now().saturating_duration_since(started));
        }
        drop(state);
        self.emit_storage_state_force();
    }

    fn clear_buffered_payload(&self) {
        self.inner
            .buffered_payload_bytes
            .store(0, Ordering::Release);
        self.update_disk_pressure(0);
        self.emit_storage_state_force();
    }

    fn clear_outstanding_requests(&self) {
        self.inner
            .outstanding_request_bytes
            .store(0, Ordering::Release);
    }

    async fn wait_before_storage(&self) {
        let millis = self
            .inner
            .storage_write_delay_millis
            .load(Ordering::Acquire);
        if millis != 0 {
            tokio::time::sleep(Duration::from_millis(millis)).await;
        }
    }

    async fn wait_before_storage_hash(&self) {
        self.inner
            .storage_hashes_started
            .fetch_add(1, Ordering::AcqRel);
        let millis = self.inner.storage_hash_delay_millis.load(Ordering::Acquire);
        if millis != 0 {
            tokio::time::sleep(Duration::from_millis(millis)).await;
        }
    }

    async fn wait_before_checkpoint_sync(&self) {
        let millis = self
            .inner
            .checkpoint_sync_delay_millis
            .load(Ordering::Acquire);
        if millis != 0 {
            tokio::time::sleep(Duration::from_millis(millis)).await;
        }
    }

    async fn wait_before_checkpoint_commit(&self) {
        let millis = self
            .inner
            .checkpoint_commit_delay_millis
            .load(Ordering::Acquire);
        if millis != 0 {
            tokio::time::sleep(Duration::from_millis(millis)).await;
        }
    }

    fn take_checkpoint_sync_failure(&self) -> bool {
        self.inner
            .checkpoint_sync_failures
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |failures| {
                failures.checked_sub(1)
            })
            .is_ok()
    }
}

fn duration_micros(duration: Duration) -> u64 {
    duration.as_micros().try_into().unwrap_or(u64::MAX)
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

fn set_disk_piece_stage(piece: &mut DiskPieceRuntimeState, stage: DiskPieceStage, now: Instant) {
    if piece.stage != stage {
        piece.stage = stage;
        piece.stage_started_at = now;
    }
}

fn insert_disk_range(ranges: &mut Vec<(u32, u32)>, begin: u32, length: u32) {
    let Some(mut end) = begin.checked_add(length) else {
        return;
    };
    let mut start = begin;
    let mut index = 0;
    while index < ranges.len() && ranges[index].1 < start {
        index += 1;
    }
    while index < ranges.len() && ranges[index].0 <= end {
        start = start.min(ranges[index].0);
        end = end.max(ranges[index].1);
        ranges.remove(index);
    }
    ranges.insert(index, (start, end));
}

fn range_bytes(ranges: &[(u32, u32)]) -> u32 {
    ranges.iter().fold(0_u32, |total, (start, end)| {
        total.saturating_add(end.saturating_sub(*start))
    })
}

fn atomic_saturating_add(value: &AtomicU64, amount: u64) {
    let _ = value.fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
        Some(current.saturating_add(amount))
    });
}

fn atomic_saturating_increment(value: &AtomicUsize) {
    atomic_saturating_add_usize(value, 1);
}

fn atomic_saturating_add_usize(value: &AtomicUsize, amount: usize) {
    let _ = value.fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
        Some(current.saturating_add(amount))
    });
}

impl Drop for SafeCancelGuard {
    fn drop(&mut self) {
        let previous = self
            .control
            .inner
            .safe_cancel_state
            .fetch_sub(1, Ordering::AcqRel);
        debug_assert_ne!(previous & SAFE_CANCEL_CRITICAL_MASK, 0);
        if previous == SAFE_CANCEL_REQUESTED | 1 {
            self.control.cancel();
        }
    }
}

fn bounded_diagnostic_detail(detail: &str) -> String {
    let mut bounded = String::with_capacity(detail.len().min(MAX_DIAGNOSTIC_ERROR_LENGTH));
    for character in detail.chars() {
        if bounded.len() + character.len_utf8() > MAX_DIAGNOSTIC_ERROR_LENGTH {
            break;
        }
        bounded.push(character);
    }
    bounded
}

fn push_recent_metadata_attempt(state: &mut MetadataDiagnosticState, peer: MetadataPeerSnapshot) {
    if state.recent_attempts.len() == MAX_RECENT_METADATA_ATTEMPTS {
        state.recent_attempts.pop_front();
        state.recent_attempts_dropped = state.recent_attempts_dropped.saturating_add(1);
    }
    state.recent_attempts.push_back(peer);
}

impl Default for DownloadControl {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadReport {
    pub info_hash: [u8; 20],
    pub piece_hash: [u8; 20],
    pub bytes_written: usize,
    pub block_count: usize,
    pub payload_limit: usize,
    pub payload_high_water: usize,
    pub outstanding_request_limit: usize,
    pub outstanding_request_high_water: usize,
    pub active_piece_limit: usize,
    pub verification_buffer: usize,
    pub piece_count: usize,
    pub verified_piece_count: usize,
    pub skipped_piece_count: usize,
    pub selected_file_bytes: u64,
    pub skipped_file_bytes: u64,
    pub padding_bytes: u64,
    pub selected_written_bytes: usize,
    pub part_written_bytes: usize,
    pub materialized_bytes: u64,
    pub part_slots_before_materialization: usize,
    pub part_slots_after_materialization: usize,
    pub part_reopened: bool,
    pub part_path: Option<PathBuf>,
    pub prepared_files: Vec<PreparedFileHash>,
}

#[derive(Debug)]
pub enum DownloadError {
    NetworkDisabled,
    NetworkPolicyDenied {
        address: SocketAddr,
        policy: NetworkPolicy,
    },
    InvalidNetworkTimeout {
        operation: &'static str,
    },
    InvalidResourceLimit(&'static str),
    MetainfoTooLarge {
        maximum: usize,
    },
    Io {
        operation: &'static str,
        source: io::Error,
    },
    Metainfo(MetainfoError),
    Magnet(MagnetError),
    Entropy(getrandom::Error),
    Dht(DhtError),
    UdpTracker(UdpTrackerError),
    PeerRegistry(PeerRegistryError),
    PeerRuntime(PeerRuntimeError),
    PeerTask(String),
    StorageTask(String),
    Swarm(SwarmError),
    Metadata(MetadataError),
    Handshake(HandshakeError),
    Frame(FrameError),
    Piece(PieceError),
    Layout(LayoutError),
    SelectiveStorage(SelectiveStorageError),
    PeerClosed,
    NoUsablePeer,
    NoUsableTrackerAddress,
    UdpTrackerResponseTooLarge {
        maximum: usize,
    },
    UdpTrackerTimedOut {
        operation: &'static str,
        timeout: Duration,
    },
    ExtensionProtocolUnsupported,
    MetadataExtensionDisabled,
    InvalidPremetadataState(&'static str),
    Checkpoint(String),
    Cancelled,
    PeerTimedOut {
        operation: &'static str,
        timeout: Duration,
    },
    NetworkTimedOut {
        operation: &'static str,
        timeout: Duration,
    },
    TrackerTask(String),
    PeerCleanup {
        failure: String,
        cleanup: String,
    },
    CleanupAfterFailure {
        failure: String,
        source: io::Error,
    },
}

impl fmt::Display for DownloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NetworkDisabled => write!(formatter, "outbound networking is disabled"),
            Self::NetworkPolicyDenied { address, policy } => {
                write!(
                    formatter,
                    "outbound address {address} is denied by network policy {policy}"
                )
            }
            Self::InvalidNetworkTimeout { operation } => {
                write!(formatter, "{operation} timeout must be nonzero")
            }
            Self::InvalidResourceLimit(message) => formatter.write_str(message),
            Self::MetainfoTooLarge { maximum } => {
                write!(formatter, "metainfo exceeds input limit {maximum}")
            }
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::Metainfo(error) => write!(formatter, "metainfo: {error}"),
            Self::Magnet(error) => write!(formatter, "magnet: {error}"),
            Self::Entropy(error) => write!(formatter, "operating-system randomness: {error}"),
            Self::Dht(error) => write!(formatter, "DHT: {error}"),
            Self::UdpTracker(error) => write!(formatter, "UDP tracker: {error}"),
            Self::PeerRegistry(error) => write!(formatter, "peer registry: {error}"),
            Self::PeerRuntime(error) => write!(formatter, "peer runtime: {error}"),
            Self::PeerTask(error) => write!(formatter, "peer task set: {error}"),
            Self::StorageTask(error) => write!(formatter, "content storage task: {error}"),
            Self::Swarm(error) => write!(formatter, "swarm state: {error}"),
            Self::Metadata(error) => write!(formatter, "metadata: {error}"),
            Self::Handshake(error) => write!(formatter, "peer handshake: {error}"),
            Self::Frame(error) => write!(formatter, "peer frame: {error}"),
            Self::Piece(error) => write!(formatter, "piece state: {error}"),
            Self::Layout(error) => write!(formatter, "torrent layout: {error}"),
            Self::SelectiveStorage(error) => write!(formatter, "selective storage: {error}"),
            Self::PeerClosed => write!(formatter, "peer closed before piece verification"),
            Self::NoUsablePeer => {
                write!(formatter, "magnet discovery produced no eligible peer")
            }
            Self::NoUsableTrackerAddress => {
                write!(
                    formatter,
                    "UDP tracker has no resolvable address allowed by network policy"
                )
            }
            Self::UdpTrackerResponseTooLarge { maximum } => {
                write!(
                    formatter,
                    "UDP tracker response exceeds the {maximum}-byte limit"
                )
            }
            Self::UdpTrackerTimedOut { operation, timeout } => {
                write!(
                    formatter,
                    "UDP tracker {operation} timed out after {}s",
                    timeout.as_secs()
                )
            }
            Self::ExtensionProtocolUnsupported => {
                write!(formatter, "peer does not advertise the extension protocol")
            }
            Self::MetadataExtensionDisabled => {
                write!(
                    formatter,
                    "peer does not advertise an enabled ut_metadata extension"
                )
            }
            Self::InvalidPremetadataState(reason) => {
                write!(
                    formatter,
                    "peer sent invalid state before metadata: {reason}"
                )
            }
            Self::Checkpoint(message) => write!(formatter, "durable checkpoint: {message}"),
            Self::Cancelled => write!(formatter, "download cancelled"),
            Self::PeerTimedOut { operation, timeout } => {
                write!(
                    formatter,
                    "peer {operation} timed out after {}s",
                    timeout.as_secs()
                )
            }
            Self::NetworkTimedOut { operation, timeout } => {
                write!(
                    formatter,
                    "{operation} timed out after {}s",
                    timeout.as_secs()
                )
            }
            Self::TrackerTask(message) => write!(formatter, "tracker task: {message}"),
            Self::PeerCleanup { failure, cleanup } => {
                write!(
                    formatter,
                    "{failure}; additionally failed to stop peer tasks: {cleanup}"
                )
            }
            Self::CleanupAfterFailure { failure, source } => write!(
                formatter,
                "{failure}; additionally failed to remove staging output: {source}"
            ),
        }
    }
}

impl Error for DownloadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Metainfo(error) => Some(error),
            Self::Magnet(error) => Some(error),
            Self::Dht(error) => Some(error),
            Self::UdpTracker(error) => Some(error),
            Self::PeerRegistry(error) => Some(error),
            Self::PeerRuntime(error) => Some(error),
            Self::Swarm(error) => Some(error),
            Self::Metadata(error) => Some(error),
            Self::Handshake(error) => Some(error),
            Self::Frame(error) => Some(error),
            Self::Piece(error) => Some(error),
            Self::Layout(error) => Some(error),
            Self::SelectiveStorage(error) => Some(error),
            Self::CleanupAfterFailure { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl DownloadError {
    pub fn is_existing_artifact(&self) -> bool {
        preserves_existing_artifact(self)
    }
}

pub async fn download_verified_piece(
    config: DownloadConfig,
) -> Result<DownloadReport, DownloadError> {
    download_verified_piece_with_control(config, DownloadControl::new()).await
}

pub async fn download_magnet(
    config: MagnetDownloadConfig,
) -> Result<DownloadReport, DownloadError> {
    download_magnet_with_control(config, DownloadControl::new()).await
}

pub async fn resume_magnet(
    config: ResumableMagnetDownloadConfig,
    checkpoints: Arc<dyn DownloadCheckpointSink>,
) -> Result<DownloadReport, DownloadError> {
    resume_magnet_with_control(config, checkpoints, DownloadControl::new()).await
}

pub async fn resume_magnet_with_control(
    config: ResumableMagnetDownloadConfig,
    checkpoints: Arc<dyn DownloadCheckpointSink>,
    control: DownloadControl,
) -> Result<DownloadReport, DownloadError> {
    resume_magnet_owned(config, checkpoints, control, None).await
}

pub async fn resume_magnet_to_descriptors_with_control(
    config: ResumableMagnetDownloadConfig,
    descriptors: DescriptorStorage,
    initialize_storage: bool,
    checkpoints: Arc<dyn DownloadCheckpointSink>,
    control: DownloadControl,
) -> Result<DownloadReport, DownloadError> {
    if config.verified_info.is_none() {
        return Err(DownloadError::Checkpoint(
            "descriptor storage requires verified metadata".to_owned(),
        ));
    }
    resume_magnet_owned(
        config,
        checkpoints,
        control,
        Some((descriptors, initialize_storage)),
    )
    .await
}

async fn resume_magnet_owned(
    config: ResumableMagnetDownloadConfig,
    checkpoints: Arc<dyn DownloadCheckpointSink>,
    control: DownloadControl,
    descriptors: Option<(DescriptorStorage, bool)>,
) -> Result<DownloadReport, DownloadError> {
    validate_resumable_magnet_download_config(&config)?;
    if control.is_cancelled() {
        return Err(DownloadError::Cancelled);
    }
    let result =
        run_resumable_magnet_download(config, checkpoints, control.clone(), descriptors).await;
    let result = require_terminal_owner_cleanup(&control, result);
    control.clear_buffered_payload();
    result
}

pub async fn download_magnet_metadata_with_control(
    magnet: String,
    network: NetworkConfig,
    control: DownloadControl,
) -> Result<Vec<u8>, DownloadError> {
    download_magnet_metadata_with_dht(magnet, network, control, None).await
}

pub async fn download_magnet_metadata_with_dht(
    magnet: String,
    network: NetworkConfig,
    control: DownloadControl,
    dht: Option<DhtHandle>,
) -> Result<Vec<u8>, DownloadError> {
    validate_network_config(network)?;
    if control.is_cancelled() {
        return Err(DownloadError::Cancelled);
    }
    let result = run_magnet_metadata(magnet, network, control.clone(), dht).await;
    let result = require_terminal_owner_cleanup(&control, result);
    control.clear_buffered_payload();
    result
}

pub async fn download_magnet_with_control(
    config: MagnetDownloadConfig,
    control: DownloadControl,
) -> Result<DownloadReport, DownloadError> {
    validate_magnet_download_config(&config)?;
    if control.is_cancelled() {
        return Err(DownloadError::Cancelled);
    }
    let output_path = config.output_path.clone();
    let result = run_magnet_download(config, control.clone()).await;
    let result = require_terminal_owner_cleanup(&control, result);
    control.clear_buffered_payload();

    match result {
        Ok(report) => Ok(report),
        Err(error) if preserves_existing_artifact(&error) => Err(error),
        Err(error) => {
            let cleanup = async {
                remove_selective_staging_if_present(&output_path).await?;
                remove_selective_part_if_present(&output_path).await
            }
            .await;
            match cleanup {
                Ok(()) => Err(error),
                Err(source) => Err(DownloadError::CleanupAfterFailure {
                    failure: error.to_string(),
                    source,
                }),
            }
        }
    }
}

pub async fn download_verified_piece_with_control(
    config: DownloadConfig,
    control: DownloadControl,
) -> Result<DownloadReport, DownloadError> {
    validate_download_config(&config)?;
    if control.is_cancelled() {
        return Err(DownloadError::Cancelled);
    }

    let output_path = config.output_path.clone();
    let result = run_download(config, control.clone(), None).await;
    let result = require_terminal_owner_cleanup(&control, result);
    control.clear_buffered_payload();

    match result {
        Ok(report) => Ok(report),
        Err(error) if preserves_existing_artifact(&error) => Err(error),
        Err(error) => {
            let cleanup = async {
                remove_selective_staging_if_present(&output_path).await?;
                remove_selective_part_if_present(&output_path).await
            }
            .await;
            match cleanup {
                Ok(()) => Err(error),
                Err(source) => Err(DownloadError::CleanupAfterFailure {
                    failure: error.to_string(),
                    source,
                }),
            }
        }
    }
}

pub async fn download_verified_piece_to_descriptors_with_control(
    config: DownloadConfig,
    descriptors: DescriptorStorage,
    control: DownloadControl,
) -> Result<DownloadReport, DownloadError> {
    validate_download_config(&config)?;
    if control.is_cancelled() {
        return Err(DownloadError::Cancelled);
    }
    let result = run_download(config, control.clone(), Some(descriptors)).await;
    let result = require_terminal_owner_cleanup(&control, result);
    control.clear_buffered_payload();
    result
}

fn require_terminal_owner_cleanup<T>(
    control: &DownloadControl,
    result: Result<T, DownloadError>,
) -> Result<T, DownloadError> {
    let diagnostics = control.diagnostic_snapshot();
    let progress = diagnostics.progress;
    let mut active = Vec::new();
    if !diagnostics.peer_connections.is_empty() {
        active.push(format!(
            "{} peer connection(s)",
            diagnostics.peer_connections.len()
        ));
    }
    if diagnostics.metadata.pending_dials != 0 {
        active.push(format!(
            "{} metadata dial(s)",
            diagnostics.metadata.pending_dials
        ));
    }
    if diagnostics.metadata.active_workers != 0 {
        active.push(format!(
            "{} metadata worker(s)",
            diagnostics.metadata.active_workers
        ));
    }
    if progress.storage_jobs_pending != 0 {
        active.push(format!("{} storage job(s)", progress.storage_jobs_pending));
    }
    if progress.outstanding_request_bytes != 0 {
        active.push(format!(
            "{} outstanding request byte(s)",
            progress.outstanding_request_bytes
        ));
    }
    if progress.buffered_payload_bytes != 0 {
        active.push(format!(
            "{} buffered payload byte(s)",
            progress.buffered_payload_bytes
        ));
    }
    if active.is_empty() {
        return result;
    }

    let cleanup = format!(
        "download operation returned with active owners: {}",
        active.join(", ")
    );
    match result {
        Ok(_) => Err(DownloadError::PeerCleanup {
            failure: "download operation completed before owner cleanup".to_owned(),
            cleanup,
        }),
        Err(error) => Err(DownloadError::PeerCleanup {
            failure: error.to_string(),
            cleanup,
        }),
    }
}

fn validate_download_config(config: &DownloadConfig) -> Result<(), DownloadError> {
    validate_network_config(config.network)?;
    config.resource_limits.validate()?;
    if matches!(config.network.policy, NetworkPolicy::Offline) {
        return Err(DownloadError::NetworkDisabled);
    }
    if !config.network.policy.allows(config.peer) {
        return Err(DownloadError::NetworkPolicyDenied {
            address: config.peer,
            policy: config.network.policy,
        });
    }
    Ok(())
}

fn validate_magnet_download_config(config: &MagnetDownloadConfig) -> Result<(), DownloadError> {
    validate_network_config(config.network)?;
    config.resource_limits.validate()?;
    Ok(())
}

fn validate_resumable_magnet_download_config(
    config: &ResumableMagnetDownloadConfig,
) -> Result<(), DownloadError> {
    validate_network_config(config.network)?;
    config.resource_limits.validate()?;
    Ok(())
}

fn validate_network_config(config: NetworkConfig) -> Result<(), DownloadError> {
    if config.peer_connect_timeout.is_zero() {
        return Err(DownloadError::InvalidNetworkTimeout {
            operation: "peer connect",
        });
    }
    if config.peer_io_timeout.is_zero() {
        return Err(DownloadError::InvalidNetworkTimeout {
            operation: "peer I/O",
        });
    }
    Ok(())
}

fn preserves_existing_artifact(error: &DownloadError) -> bool {
    matches!(
        error,
        DownloadError::SelectiveStorage(SelectiveStorageError::ExistingOutput(_))
            | DownloadError::SelectiveStorage(SelectiveStorageError::ExistingStaging(_))
            | DownloadError::SelectiveStorage(SelectiveStorageError::ExistingPartFile(_))
            | DownloadError::SelectiveStorage(SelectiveStorageError::PartFile(
                crate::part_file::PartFileError::Existing(_)
            ))
    )
}

#[derive(Debug)]
struct PremetadataPeerState {
    choking: bool,
    bitfield: Option<Vec<u8>>,
    haves: BTreeSet<u32>,
}

impl PremetadataPeerState {
    fn new() -> Self {
        Self {
            choking: true,
            bitfield: None,
            haves: BTreeSet::new(),
        }
    }

    fn observe(&mut self, message: PeerMessage) -> Result<(), DownloadError> {
        match message {
            PeerMessage::KeepAlive | PeerMessage::Interested | PeerMessage::NotInterested => {}
            PeerMessage::Choke => self.choking = true,
            PeerMessage::Unchoke => self.choking = false,
            PeerMessage::Have(index) => {
                if !self.haves.contains(&index) && self.haves.len() == MAX_ENGINE_PIECES {
                    return Err(DownloadError::InvalidPremetadataState(
                        "too many distinct HAVE indices",
                    ));
                }
                self.haves.insert(index);
            }
            PeerMessage::Bitfield(bitfield) => {
                if bitfield.len() > MAX_ENGINE_PIECES.div_ceil(8) {
                    return Err(DownloadError::InvalidPremetadataState(
                        "bitfield exceeds the supported piece-count bound",
                    ));
                }
                self.bitfield = Some(bitfield);
            }
            PeerMessage::Request(_) | PeerMessage::Cancel(_) | PeerMessage::Piece { .. } => {
                return Err(DownloadError::InvalidPremetadataState(
                    "payload message arrived before verified metadata",
                ));
            }
            PeerMessage::Extended { .. } => {
                return Err(DownloadError::InvalidPremetadataState(
                    "extension message was dispatched as core peer state",
                ));
            }
        }
        Ok(())
    }

    fn validated_messages(
        self,
        metainfo: &Metainfo,
    ) -> Result<VecDeque<PeerMessage>, DownloadError> {
        let piece_count = metainfo.piece_count();
        let mut messages = VecDeque::new();
        if let Some(bitfield) = self.bitfield {
            let expected_length = piece_count.div_ceil(8);
            if bitfield.len() != expected_length {
                return Err(DownloadError::InvalidPremetadataState(
                    "bitfield length does not match verified metadata",
                ));
            }
            let remainder = piece_count % 8;
            if remainder != 0 {
                let unused_mask = (1_u8 << (8 - remainder)) - 1;
                if bitfield.last().is_some_and(|byte| byte & unused_mask != 0) {
                    return Err(DownloadError::InvalidPremetadataState(
                        "bitfield sets unused trailing bits",
                    ));
                }
            }
            messages.push_back(PeerMessage::Bitfield(bitfield));
        }
        for index in self.haves {
            if index as usize >= piece_count {
                return Err(DownloadError::InvalidPremetadataState(
                    "HAVE index is outside verified metadata",
                ));
            }
            messages.push_back(PeerMessage::Have(index));
        }
        if !self.choking {
            messages.push_back(PeerMessage::Unchoke);
        }
        Ok(messages)
    }
}

#[derive(Clone, Copy, Debug)]
struct UdpTrackerTiming {
    retransmit_after: Duration,
    completion_timeout: Duration,
}

impl UdpTrackerTiming {
    const PRODUCTION: Self = Self {
        retransmit_after: UDP_TRACKER_RETRANSMIT_AFTER,
        completion_timeout: UDP_TRACKER_COMPLETION_TIMEOUT,
    };
}

#[derive(Clone, Copy, Debug)]
struct UdpTrackerAnnounce {
    info_hash: [u8; 20],
    key: u32,
    event: AnnounceEvent,
    port: u16,
}

#[derive(Clone, Copy, Debug)]
struct UdpTrackerExchange<'a> {
    timing: UdpTrackerTiming,
    control: &'a DownloadControl,
    tracker_label: &'a str,
}

#[derive(Clone, Copy, Debug)]
struct UdpTrackerToken {
    connection_id: u64,
    expires_at: Instant,
}

#[derive(Debug, Default)]
struct UdpTrackerTokenCache {
    tokens: BTreeMap<SocketAddr, UdpTrackerToken>,
}

impl UdpTrackerTokenCache {
    fn get(&mut self, address: SocketAddr, now: Instant) -> Option<u64> {
        self.prune(now);
        self.tokens
            .get(&address)
            .filter(|token| token.expires_at > now)
            .map(|token| token.connection_id)
    }

    fn insert(&mut self, address: SocketAddr, connection_id: u64, now: Instant) {
        self.prune(now);
        if self.tokens.len() == MAX_UDP_TRACKER_TOKENS && !self.tokens.contains_key(&address) {
            let first = self.tokens.keys().next().copied();
            if let Some(first) = first {
                self.tokens.remove(&first);
            }
        }
        self.tokens.insert(
            address,
            UdpTrackerToken {
                connection_id,
                expires_at: now + UDP_TRACKER_TOKEN_LIFETIME,
            },
        );
    }

    fn remove(&mut self, address: SocketAddr) {
        self.tokens.remove(&address);
    }

    fn prune(&mut self, now: Instant) {
        self.tokens.retain(|_, token| token.expires_at > now);
    }
}

#[derive(Debug)]
enum TrackerUpdate {
    Peers {
        tracker: String,
        peers: Vec<CompactPeer>,
    },
}

#[derive(Debug)]
struct TrackerOperationResult {
    id: TrackerId,
    tracker: String,
    token_cache: UdpTrackerTokenCache,
    result: Result<AnnounceResponse, DownloadError>,
}

#[derive(Debug)]
struct TrackerManager {
    receiver: mpsc::Receiver<TrackerUpdate>,
    cancellation: CancellationToken,
    task: Option<JoinHandle<()>>,
}

impl TrackerManager {
    fn start(
        mut trackers: Vec<UdpTrackerUrl>,
        info_hash: [u8; 20],
        network_policy: NetworkPolicy,
        control: DownloadControl,
    ) -> Result<Self, DownloadError> {
        shuffle_tracker_urls(&mut trackers)?;
        let tracker_key = random_nonzero_u32()?;
        let cancellation = CancellationToken::new();
        let task_cancellation = cancellation.clone();
        let (sender, receiver) = mpsc::channel(TRACKER_RESULT_QUEUE);
        let task = tokio::spawn(run_tracker_manager(
            TrackerSchedule::new(trackers),
            info_hash,
            tracker_key,
            network_policy,
            control,
            task_cancellation,
            sender,
        ));
        Ok(Self {
            receiver,
            cancellation,
            task: Some(task),
        })
    }

    async fn next_peers(&mut self) -> Result<(String, Vec<CompactPeer>), DownloadError> {
        enum Outcome {
            Update(Option<TrackerUpdate>),
            Task(Result<(), tokio::task::JoinError>),
        }
        let outcome = {
            let Some(task) = self.task.as_mut() else {
                return Err(DownloadError::TrackerTask(
                    "tracker owner terminated unexpectedly".to_owned(),
                ));
            };
            tokio::select! {
                update = self.receiver.recv() => Outcome::Update(update),
                result = task => Outcome::Task(result),
            }
        };
        match outcome {
            Outcome::Update(Some(TrackerUpdate::Peers { tracker, peers })) => Ok((tracker, peers)),
            Outcome::Update(None) => Err(DownloadError::TrackerTask(
                "tracker result channel closed unexpectedly".to_owned(),
            )),
            Outcome::Task(result) => {
                self.task.take();
                match result {
                    Ok(()) => Err(DownloadError::TrackerTask(
                        "tracker owner terminated unexpectedly".to_owned(),
                    )),
                    Err(error) => Err(DownloadError::TrackerTask(error.to_string())),
                }
            }
        }
    }

    async fn shutdown(mut self) -> Result<(), DownloadError> {
        self.cancellation.cancel();
        self.receiver.close();
        let Some(task) = self.task.take() else {
            return Ok(());
        };
        task.await
            .map_err(|error| DownloadError::TrackerTask(error.to_string()))
    }
}

#[derive(Debug)]
enum ContentDiscoveryEvent {
    Peers {
        source: PeerSource,
        tracker: Option<String>,
        addresses: Vec<SocketAddr>,
    },
    Failed(DownloadError),
}

#[derive(Debug)]
struct ContentDiscovery {
    receiver: mpsc::Receiver<ContentDiscoveryEvent>,
    cancellation: CancellationToken,
    tasks: Vec<JoinHandle<Result<(), DownloadError>>>,
}

impl ContentDiscovery {
    fn start(peers: &mut TorrentPeerCoordinator, info_hash: [u8; 20]) -> Self {
        let (sender, receiver) = mpsc::channel(CONTENT_DISCOVERY_QUEUE);
        let cancellation = CancellationToken::new();
        let mut tasks = Vec::new();
        if let Some(tracker) = peers.tracker.take() {
            tasks.push(tokio::spawn(run_content_tracker_discovery(
                tracker,
                sender.clone(),
                cancellation.clone(),
            )));
        }
        if let Some(dht) = peers.dht.clone() {
            tasks.push(tokio::spawn(run_content_dht_discovery(
                dht,
                info_hash,
                peers.control.clone(),
                sender.clone(),
                cancellation.clone(),
            )));
        }
        drop(sender);
        Self {
            receiver,
            cancellation,
            tasks,
        }
    }

    fn is_active(&self) -> bool {
        !self.receiver.is_closed()
    }

    async fn next_event(&mut self) -> Option<ContentDiscoveryEvent> {
        self.receiver.recv().await
    }

    async fn shutdown(mut self) -> Result<(), DownloadError> {
        self.cancellation.cancel();
        self.receiver.close();
        for task in self.tasks {
            task.await
                .map_err(|error| DownloadError::PeerTask(error.to_string()))??;
        }
        Ok(())
    }
}

async fn run_content_tracker_discovery(
    mut tracker: TrackerManager,
    sender: mpsc::Sender<ContentDiscoveryEvent>,
    cancellation: CancellationToken,
) -> Result<(), DownloadError> {
    loop {
        let result = tokio::select! {
            biased;
            _ = cancellation.cancelled() => break,
            result = tracker.next_peers() => result,
        };
        let event = match result {
            Ok((tracker, peers)) => ContentDiscoveryEvent::Peers {
                source: PeerSource::Tracker,
                tracker: Some(tracker),
                addresses: peers.into_iter().map(compact_peer_address).collect(),
            },
            Err(error) => {
                send_content_discovery_event(
                    &sender,
                    ContentDiscoveryEvent::Failed(error),
                    &cancellation,
                )
                .await;
                break;
            }
        };
        if !send_content_discovery_event(&sender, event, &cancellation).await {
            break;
        }
    }
    tracker.shutdown().await
}

async fn run_content_dht_discovery(
    dht: DhtHandle,
    info_hash: [u8; 20],
    control: DownloadControl,
    sender: mpsc::Sender<ContentDiscoveryEvent>,
    cancellation: CancellationToken,
) -> Result<(), DownloadError> {
    let mut retry_delay = DHT_RETRY_INITIAL_DELAY;
    loop {
        control.emit(DownloadActivityEvent::DhtLookupStarted);
        let result = tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                let _ = dht.cancel_lookup(info_hash).await;
                return Ok(());
            }
            _ = control.inner.cancellation.cancelled() => {
                let _ = dht.cancel_lookup(info_hash).await;
                return Ok(());
            }
            result = dht.lookup(info_hash) => result,
        };
        match result {
            Ok(addresses) => {
                control.emit(DownloadActivityEvent::DhtLookupSucceeded {
                    peer_count: addresses.len().try_into().unwrap_or(u32::MAX),
                });
                if !send_content_discovery_event(
                    &sender,
                    ContentDiscoveryEvent::Peers {
                        source: PeerSource::Dht,
                        tracker: None,
                        addresses,
                    },
                    &cancellation,
                )
                .await
                {
                    return Ok(());
                }
                retry_delay = DHT_RETRY_INITIAL_DELAY;
                tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => return Ok(()),
                    _ = control.inner.cancellation.cancelled() => return Ok(()),
                    _ = tokio::time::sleep(DHT_SUCCESS_REQUERY_DELAY) => {}
                }
            }
            Err(
                error @ (DhtError::LookupTimedOut
                | DhtError::NoReachableNodes
                | DhtError::LookupCapacity),
            ) => {
                control.emit(DownloadActivityEvent::DhtLookupFailed {
                    detail: error.to_string(),
                });
                control.emit(DownloadActivityEvent::DhtRetryScheduled {
                    retry_in_seconds: retry_delay.as_secs(),
                });
                tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => return Ok(()),
                    _ = control.inner.cancellation.cancelled() => return Ok(()),
                    _ = tokio::time::sleep(retry_delay) => {}
                }
                retry_delay = retry_delay.saturating_mul(2).min(DHT_RETRY_MAX_DELAY);
            }
            Err(error) => {
                control.emit(DownloadActivityEvent::DhtLookupFailed {
                    detail: error.to_string(),
                });
                send_content_discovery_event(
                    &sender,
                    ContentDiscoveryEvent::Failed(DownloadError::Dht(error)),
                    &cancellation,
                )
                .await;
                return Ok(());
            }
        }
    }
}

async fn send_content_discovery_event(
    sender: &mpsc::Sender<ContentDiscoveryEvent>,
    event: ContentDiscoveryEvent,
    cancellation: &CancellationToken,
) -> bool {
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => false,
        result = sender.send(event) => result.is_ok(),
    }
}

impl Drop for TrackerManager {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn run_tracker_manager(
    mut schedule: TrackerSchedule,
    info_hash: [u8; 20],
    tracker_key: u32,
    network_policy: NetworkPolicy,
    control: DownloadControl,
    cancellation: CancellationToken,
    sender: mpsc::Sender<TrackerUpdate>,
) {
    let started_at = Instant::now();
    control.emit(DownloadActivityEvent::TrackerState(Box::new(
        schedule.snapshot(started_at.elapsed(), true),
    )));
    run_active_tracker_manager(
        &mut schedule,
        info_hash,
        tracker_key,
        network_policy,
        &control,
        &cancellation,
        &sender,
        started_at,
    )
    .await;
    control.emit(DownloadActivityEvent::TrackerState(Box::new(
        schedule.snapshot(started_at.elapsed(), false),
    )));
}

#[allow(clippy::too_many_arguments)]
async fn run_active_tracker_manager(
    schedule: &mut TrackerSchedule,
    info_hash: [u8; 20],
    tracker_key: u32,
    network_policy: NetworkPolicy,
    control: &DownloadControl,
    cancellation: &CancellationToken,
    sender: &mpsc::Sender<TrackerUpdate>,
    started_at: Instant,
) {
    let mut token_caches = BTreeMap::new();
    let mut operations = JoinSet::new();
    loop {
        let mut pending_action = None;
        while operations.len() < MAX_CONCURRENT_TRACKER_OPERATIONS {
            match schedule.next_action(started_at.elapsed()) {
                TrackerAction::Announce {
                    id,
                    url,
                    tier,
                    source: _,
                    event,
                    attempt,
                    fallback,
                } => {
                    let tracker = udp_tracker_label(&url);
                    if fallback {
                        control.emit(DownloadActivityEvent::TrackerFallbackSelected {
                            tracker: tracker.clone(),
                            tier,
                        });
                    }
                    control.emit(DownloadActivityEvent::TrackerAnnounceStarted {
                        tracker: tracker.clone(),
                        tier,
                        attempt,
                        event,
                    });
                    control.emit(DownloadActivityEvent::TrackerState(Box::new(
                        schedule.snapshot(started_at.elapsed(), true),
                    )));
                    let operation_control = control.clone();
                    let mut token_cache = token_caches.remove(&id).unwrap_or_default();
                    operations.spawn(async move {
                        let result = announce_udp_tracker(
                            &url,
                            network_policy,
                            &mut token_cache,
                            UdpTrackerAnnounce {
                                info_hash,
                                key: tracker_key,
                                event,
                                port: DEFAULT_ADVERTISED_PEER_PORT,
                            },
                            UdpTrackerExchange {
                                timing: UdpTrackerTiming::PRODUCTION,
                                control: &operation_control,
                                tracker_label: &tracker,
                            },
                        )
                        .await;
                        TrackerOperationResult {
                            id,
                            tracker,
                            token_cache,
                            result,
                        }
                    });
                }
                action @ TrackerAction::Wait { .. }
                | action @ TrackerAction::Pending
                | action @ TrackerAction::Exhausted => {
                    pending_action = Some(action);
                    break;
                }
            }
        }

        if !operations.is_empty() {
            let joined = tokio::select! {
                biased;
                _ = cancellation.cancelled() => {
                    shutdown_tracker_operations(&mut operations).await;
                    return;
                }
                joined = operations.join_next() => joined,
            };
            let Some(joined) = joined else {
                continue;
            };
            let operation = match joined {
                Ok(operation) => operation,
                Err(_) => {
                    shutdown_tracker_operations(&mut operations).await;
                    return;
                }
            };
            token_caches.insert(operation.id, operation.token_cache);
            let now = started_at.elapsed();
            match operation.result {
                Ok(response) => {
                    let peer_count = response.peers.len().try_into().unwrap_or(u32::MAX);
                    let success = schedule.succeeded(
                        operation.id,
                        now,
                        response.interval,
                        peer_count,
                        response.seeders,
                        response.leechers,
                    );
                    control.emit(DownloadActivityEvent::TrackerAnnounceSucceeded {
                        tracker: operation.tracker.clone(),
                        peer_count,
                        interval_seconds: success.interval.as_secs(),
                    });
                    control.emit(DownloadActivityEvent::TrackerState(Box::new(
                        schedule.snapshot(now, true),
                    )));
                    let send = sender.send(TrackerUpdate::Peers {
                        tracker: operation.tracker,
                        peers: response.peers,
                    });
                    let sent = tokio::select! {
                        biased;
                        _ = cancellation.cancelled() => false,
                        result = send => result.is_ok(),
                    };
                    if !sent {
                        shutdown_tracker_operations(&mut operations).await;
                        return;
                    }
                }
                Err(error) => {
                    let detail = error.to_string();
                    let failure = schedule.failed(operation.id, now, &detail);
                    control.emit(DownloadActivityEvent::TrackerAnnounceFailed {
                        tracker: operation.tracker,
                        failures: failure.failures,
                        retry_in_seconds: failure.retry_in.as_secs(),
                        detail,
                    });
                    control.emit(DownloadActivityEvent::TrackerState(Box::new(
                        schedule.snapshot(now, true),
                    )));
                }
            }
            continue;
        }

        match pending_action.unwrap_or_else(|| schedule.next_action(started_at.elapsed())) {
            TrackerAction::Wait { delay, url, kind } => {
                let tracker = udp_tracker_label(&url);
                emit_tracker_wait(control, tracker, kind, delay);
                tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => return,
                    _ = tokio::time::sleep(delay) => {}
                }
            }
            TrackerAction::Exhausted => return,
            TrackerAction::Pending => return,
            TrackerAction::Announce { .. } => continue,
        }
    }
}

fn emit_tracker_wait(
    control: &DownloadControl,
    tracker: String,
    kind: TrackerWaitKind,
    delay: Duration,
) {
    match kind {
        TrackerWaitKind::FailureRetry => {
            control.emit(DownloadActivityEvent::TrackerRetryScheduled {
                tracker,
                retry_in_seconds: delay.as_secs(),
            });
        }
        TrackerWaitKind::Reannounce => {
            control.emit(DownloadActivityEvent::TrackerReannounceScheduled {
                tracker,
                announce_in_seconds: delay.as_secs(),
            });
        }
    }
}

async fn shutdown_tracker_operations(operations: &mut JoinSet<TrackerOperationResult>) {
    operations.abort_all();
    while operations.join_next().await.is_some() {}
}

fn shuffle_tracker_urls(trackers: &mut [UdpTrackerUrl]) -> Result<(), DownloadError> {
    for last in (1..trackers.len()).rev() {
        let selected = usize::try_from(random_nonzero_u32()?).unwrap_or(usize::MAX) % (last + 1);
        trackers.swap(last, selected);
    }
    Ok(())
}

fn udp_tracker_label(tracker: &UdpTrackerUrl) -> String {
    if tracker.host.contains(':') {
        format!("udp://[{}]:{}", tracker.host, tracker.port)
    } else {
        format!("udp://{}:{}", tracker.host, tracker.port)
    }
}

#[derive(Debug)]
struct TorrentPeerCoordinator {
    registry: PeerRegistry,
    runtime: PeerRuntime,
    selector: PeerSelector,
    started_at: Instant,
    network: NetworkConfig,
    tracker: Option<TrackerManager>,
    dht: Option<DhtHandle>,
    control: DownloadControl,
    connection: Option<PeerConnection>,
    last_error: Option<DownloadError>,
    next_dht_lookup: Instant,
}

#[derive(Debug)]
enum MetadataPeerResult {
    Complete {
        connection: PeerConnection,
        raw_info: Vec<u8>,
        metainfo: Metainfo,
    },
    Failed {
        connection: PeerConnection,
        error: DownloadError,
    },
    Cancelled {
        connection: PeerConnection,
    },
}

impl MetadataPeerResult {
    fn attempt(&self) -> DialAttempt {
        match self {
            Self::Complete { connection, .. }
            | Self::Failed { connection, .. }
            | Self::Cancelled { connection } => connection.attempt(),
        }
    }
}

#[derive(Debug)]
enum MetadataSupervisorEvent {
    Cancelled,
    Discovery(Result<(), DownloadError>),
    Socket(Result<PeerSetEvent, PeerSetError>),
    Worker(Option<Result<MetadataPeerResult, tokio::task::JoinError>>),
}

#[derive(Clone, Copy, Debug)]
struct DhtRetryTiming {
    initial_delay: Duration,
    maximum_delay: Duration,
}

impl DhtRetryTiming {
    const PRODUCTION: Self = Self {
        initial_delay: DHT_RETRY_INITIAL_DELAY,
        maximum_delay: DHT_RETRY_MAX_DELAY,
    };
}

async fn retrying_dht_lookup(
    dht: DhtHandle,
    info_hash: [u8; 20],
    control: DownloadControl,
    timing: DhtRetryTiming,
    initial_wait: Duration,
) -> Result<Vec<SocketAddr>, DownloadError> {
    if !initial_wait.is_zero() {
        tokio::select! {
            _ = control.inner.cancellation.cancelled() => {
                return Err(DownloadError::Cancelled);
            }
            _ = tokio::time::sleep(initial_wait) => {}
        }
    }
    let mut retry_delay = timing.initial_delay;
    loop {
        control.emit(DownloadActivityEvent::DhtLookupStarted);
        let result = tokio::select! {
            _ = control.inner.cancellation.cancelled() => {
                let _ = dht.cancel_lookup(info_hash).await;
                return Err(DownloadError::Cancelled);
            }
            result = dht.lookup(info_hash) => result,
        };
        match result {
            Ok(peers) => {
                control.emit(DownloadActivityEvent::DhtLookupSucceeded {
                    peer_count: peers.len().try_into().unwrap_or(u32::MAX),
                });
                return Ok(peers);
            }
            Err(
                error @ (DhtError::LookupTimedOut
                | DhtError::NoReachableNodes
                | DhtError::LookupCapacity),
            ) => {
                control.emit(DownloadActivityEvent::DhtLookupFailed {
                    detail: error.to_string(),
                });
                control.emit(DownloadActivityEvent::DhtRetryScheduled {
                    retry_in_seconds: retry_delay.as_secs(),
                });
                tokio::select! {
                    _ = control.inner.cancellation.cancelled() => {
                        return Err(DownloadError::Cancelled);
                    }
                    _ = tokio::time::sleep(retry_delay) => {}
                }
                retry_delay = retry_delay.saturating_mul(2).min(timing.maximum_delay);
            }
            Err(error) => {
                control.emit(DownloadActivityEvent::DhtLookupFailed {
                    detail: error.to_string(),
                });
                return Err(DownloadError::Dht(error));
            }
        }
    }
}

impl TorrentPeerCoordinator {
    fn new(network: NetworkConfig, control: DownloadControl) -> Result<Self, DownloadError> {
        validate_network_config(network)?;
        Ok(Self {
            registry: PeerRegistry::new(PeerRegistryConfig::default())
                .map_err(DownloadError::PeerRegistry)?,
            runtime: PeerRuntime::default(),
            selector: PeerSelector,
            started_at: Instant::now(),
            network,
            tracker: None,
            dht: None,
            control,
            connection: None,
            last_error: None,
            next_dht_lookup: Instant::now(),
        })
    }

    fn begin_dial(
        &mut self,
        candidate: DialCandidate,
        role: PeerConnectionRole,
    ) -> Result<DialAttempt, DownloadError> {
        let context = PeerSelectionContext {
            now: self.elapsed(),
        };
        let attempt = self
            .registry
            .begin_dial(candidate, context)
            .map_err(DownloadError::PeerRegistry)?;
        if let Err(error) = self.runtime.begin_outgoing(attempt, role, context.now) {
            let _ = self.registry.dial_cancelled(attempt);
            return Err(DownloadError::PeerRuntime(error));
        }
        self.publish_peer_runtime(true)?;
        Ok(attempt)
    }

    fn transport_connected(&mut self, attempt: DialAttempt) -> Result<(), DownloadError> {
        let connection = connection_id(attempt);
        let Some(peer) = self.runtime.observation(connection) else {
            return Ok(());
        };
        if peer.lifecycle != crate::peer_runtime::PeerConnectionLifecycle::TransportConnecting {
            return Ok(());
        }
        self.runtime
            .transport_connected(connection, self.elapsed())
            .map_err(DownloadError::PeerRuntime)?;
        self.publish_peer_runtime(true)
    }

    fn dial_succeeded(
        &mut self,
        attempt: DialAttempt,
        handshake: &Handshake,
    ) -> Result<(), DownloadError> {
        self.registry
            .dial_succeeded(attempt, self.elapsed())
            .map_err(DownloadError::PeerRegistry)?;
        self.runtime
            .handshake_completed(connection_id(attempt), handshake, self.elapsed())
            .map_err(DownloadError::PeerRuntime)?;
        self.publish_peer_runtime(true)
    }

    fn dial_failed(
        &mut self,
        attempt: DialAttempt,
        failure: PeerFailure,
    ) -> Result<(), DownloadError> {
        let connection = connection_id(attempt);
        self.runtime
            .begin_disconnect(connection, Some(failure), self.elapsed())
            .map_err(DownloadError::PeerRuntime)?;
        self.publish_peer_runtime(true)?;
        self.registry
            .dial_failed(attempt, self.elapsed(), failure)
            .map_err(DownloadError::PeerRegistry)?;
        self.runtime
            .remove(connection)
            .map_err(DownloadError::PeerRuntime)?;
        self.publish_peer_runtime(true)
    }

    fn dial_cancelled(&mut self, attempt: DialAttempt) -> Result<(), DownloadError> {
        let connection = connection_id(attempt);
        self.runtime
            .begin_disconnect(connection, None, self.elapsed())
            .map_err(DownloadError::PeerRuntime)?;
        self.publish_peer_runtime(true)?;
        self.registry
            .dial_cancelled(attempt)
            .map_err(DownloadError::PeerRegistry)?;
        self.runtime
            .remove(connection)
            .map_err(DownloadError::PeerRuntime)?;
        self.publish_peer_runtime(true)
    }

    fn begin_disconnect(
        &mut self,
        attempt: DialAttempt,
        failure: Option<PeerFailure>,
    ) -> Result<(), DownloadError> {
        self.runtime
            .begin_disconnect(connection_id(attempt), failure, self.elapsed())
            .map_err(DownloadError::PeerRuntime)?;
        self.publish_peer_runtime(true)
    }

    fn connection_closed(
        &mut self,
        attempt: DialAttempt,
        failure: Option<PeerFailure>,
    ) -> Result<(), DownloadError> {
        let connection = connection_id(attempt);
        if self.runtime.observation(connection).is_some_and(|peer| {
            peer.lifecycle != crate::peer_runtime::PeerConnectionLifecycle::Disconnecting
        }) {
            self.begin_disconnect(attempt, failure)?;
        }
        self.registry
            .connection_closed(attempt, self.elapsed(), failure)
            .map_err(DownloadError::PeerRegistry)?;
        self.runtime
            .remove(connection)
            .map_err(DownloadError::PeerRuntime)?;
        self.publish_peer_runtime(true)
    }

    fn handoff_to_content(&mut self, attempt: DialAttempt) -> Result<(), DownloadError> {
        self.runtime
            .set_role(connection_id(attempt), PeerConnectionRole::Content)
            .map_err(DownloadError::PeerRuntime)?;
        self.publish_peer_runtime(true)
    }

    fn observe_content_peers(&mut self, state: &SwarmState) -> Result<(), DownloadError> {
        for peer in state.connection_activity(self.elapsed()) {
            self.runtime
                .set_content_activity(
                    peer.id,
                    PeerContentActivity {
                        choking: peer.choking,
                        wanted_piece_count: peer.wanted_piece_count,
                        pending_requests: peer.pending_requests,
                        target_requests: peer.target_requests,
                        queued_payload_bytes: peer.queued_payload_bytes,
                        useful_payload_bytes: peer.useful_payload_bytes,
                        observed_payload_rate: peer.observed_payload_rate,
                        connected_age: peer.connected_age,
                        last_useful_age: peer.last_useful_age,
                        last_payload_age: peer.last_payload_age,
                        request_timeout: peer.request_timeout,
                        oldest_request_age: peer.oldest_request_age,
                        request_window_phase: match peer.window_phase {
                            ConnectionWindowPhaseSnapshot::SlowStart => {
                                PeerRequestWindowPhase::SlowStart
                            }
                            ConnectionWindowPhaseSnapshot::Steady => PeerRequestWindowPhase::Steady,
                            ConnectionWindowPhaseSnapshot::Stalled => {
                                PeerRequestWindowPhase::Stalled
                            }
                        },
                    },
                )
                .map_err(DownloadError::PeerRuntime)?;
        }
        self.publish_peer_runtime(false)
    }

    fn publish_peer_runtime(&mut self, force: bool) -> Result<(), DownloadError> {
        let connections = self
            .runtime
            .snapshot()
            .into_iter()
            .map(|peer| peer.connection_id)
            .collect::<Vec<_>>();
        for connection in connections {
            let record_id = self
                .runtime
                .observation(connection)
                .and_then(|peer| peer.record_id);
            if let Some(sources) = record_id
                .and_then(|record_id| self.registry.get(record_id))
                .map(|record| record.sources())
            {
                self.runtime
                    .set_sources(connection, sources)
                    .map_err(DownloadError::PeerRuntime)?;
            }
        }
        self.control
            .observe_peer_runtime(&self.runtime, self.elapsed(), force);
        self.publish_peer_registry(force);
        Ok(())
    }

    fn publish_peer_registry(&self, force: bool) {
        self.control
            .observe_peer_registry(&self.registry, self.elapsed(), true, force);
    }

    fn from_endpoint(
        address: SocketAddr,
        source: PeerSource,
        network: NetworkConfig,
    ) -> Result<Self, DownloadError> {
        let mut peers = Self::new(network, DownloadControl::new())?;
        if matches!(network.policy, NetworkPolicy::Offline) {
            return Err(DownloadError::NetworkDisabled);
        }
        peers.observe_address(address, source)?;
        Ok(peers)
    }

    async fn from_magnet(
        magnet: &Magnet,
        network: NetworkConfig,
        control: DownloadControl,
        dht: Option<DhtHandle>,
    ) -> Result<Self, DownloadError> {
        let mut peers = Self::new(network, control)?;
        peers.publish_peer_registry(true);
        peers.dht = dht;
        if peers.control.is_cancelled() {
            return Err(DownloadError::Cancelled);
        }
        if !network.policy.permits_dns() {
            return Err(DownloadError::NetworkDisabled);
        }
        peers.resolve_peer_hints(&magnet.peer_hints).await;
        if peers.control.is_cancelled() {
            return Err(DownloadError::Cancelled);
        }
        if !magnet.udp_trackers.is_empty() {
            peers.tracker = Some(TrackerManager::start(
                magnet.udp_trackers.clone(),
                magnet.info_hash,
                network.policy,
                peers.control.clone(),
            )?);
        }
        if peers.registry.is_empty() && peers.tracker.is_none() && peers.dht.is_none() {
            return Err(peers
                .last_error
                .take()
                .unwrap_or(DownloadError::NoUsablePeer));
        }
        Ok(peers)
    }

    async fn resolve_peer_hints(&mut self, hints: &[PeerHint]) {
        for hint in hints {
            let addresses =
                match resolve_host(&hint.host, hint.port, "resolve magnet peer hint").await {
                    Ok(addresses) => addresses,
                    Err(error) => {
                        self.last_error = Some(error);
                        continue;
                    }
                };
            for address in addresses {
                if let Err(error) = self.observe_address(address, PeerSource::MagnetHint) {
                    self.last_error = Some(error);
                }
            }
        }
    }

    fn observe_address(
        &mut self,
        address: SocketAddr,
        source: PeerSource,
    ) -> Result<(), DownloadError> {
        if !self.network.policy.allows(address) {
            return Err(DownloadError::NetworkPolicyDenied {
                address,
                policy: self.network.policy,
            });
        }
        let endpoint = PeerEndpoint::new(address).map_err(DownloadError::PeerRegistry)?;
        self.registry
            .observe(PeerObservation::dialable(endpoint, source), self.elapsed())
            .map_err(DownloadError::PeerRegistry)?;
        self.publish_peer_runtime(true)
    }

    fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    async fn receive_tracker_peers(&mut self) -> Result<(), DownloadError> {
        let Some(tracker) = self.tracker.as_mut() else {
            return Err(self
                .last_error
                .take()
                .unwrap_or(DownloadError::NoUsablePeer));
        };
        let (tracker, compact_peers) = tracker.next_peers().await?;
        let peer_count = compact_peers.len().try_into().unwrap_or(u32::MAX);
        for compact_peer in compact_peers {
            let address = compact_peer_address(compact_peer);
            if let Err(error) = self.observe_address(address, PeerSource::Tracker) {
                self.last_error = Some(error);
            }
        }
        if self
            .selector
            .select(
                &self.registry,
                PeerSelectionContext {
                    now: self.elapsed(),
                },
            )
            .is_none()
        {
            self.control
                .emit(DownloadActivityEvent::TrackerPeersUnavailable {
                    tracker,
                    peer_count,
                });
        }
        Ok(())
    }

    async fn receive_dht_peers(&mut self, info_hash: [u8; 20]) -> Result<(), DownloadError> {
        let dht = self.dht.clone().ok_or(DownloadError::NoUsablePeer)?;
        let peers = retrying_dht_lookup(
            dht,
            info_hash,
            self.control.clone(),
            DhtRetryTiming::PRODUCTION,
            self.dht_requery_wait(),
        )
        .await?;
        self.next_dht_lookup = Instant::now() + DHT_SUCCESS_REQUERY_DELAY;
        for address in peers {
            if let Err(error) = self.observe_address(address, PeerSource::Dht) {
                self.last_error = Some(error);
            }
        }
        Ok(())
    }

    async fn receive_discovery_peers(&mut self, info_hash: [u8; 20]) -> Result<(), DownloadError> {
        match (self.tracker.is_some(), self.dht.is_some()) {
            (true, true) => {
                let dht = self.dht.clone().expect("DHT presence checked");
                let dht_wait = self.dht_requery_wait();
                let dht_control = self.control.clone();
                let tracker = self.tracker.as_mut().expect("tracker presence checked");
                enum Discovery {
                    Tracker(Result<(String, Vec<CompactPeer>), DownloadError>),
                    Dht(Result<Vec<SocketAddr>, DownloadError>),
                }
                let dht_lookup = retrying_dht_lookup(
                    dht,
                    info_hash,
                    dht_control,
                    DhtRetryTiming::PRODUCTION,
                    dht_wait,
                );
                let discovered = tokio::select! {
                    tracker = tracker.next_peers() => Discovery::Tracker(tracker),
                    dht = dht_lookup => Discovery::Dht(dht),
                };
                match discovered {
                    Discovery::Tracker(result) => {
                        let (tracker, compact_peers) = result?;
                        let peer_count = compact_peers.len().try_into().unwrap_or(u32::MAX);
                        for peer in compact_peers {
                            if let Err(error) = self
                                .observe_address(compact_peer_address(peer), PeerSource::Tracker)
                            {
                                self.last_error = Some(error);
                            }
                        }
                        if self.registry.is_empty() {
                            self.control
                                .emit(DownloadActivityEvent::TrackerPeersUnavailable {
                                    tracker,
                                    peer_count,
                                });
                        }
                        Ok(())
                    }
                    Discovery::Dht(Ok(peers)) => {
                        self.next_dht_lookup = Instant::now() + DHT_SUCCESS_REQUERY_DELAY;
                        for address in peers {
                            if let Err(error) = self.observe_address(address, PeerSource::Dht) {
                                self.last_error = Some(error);
                            }
                        }
                        Ok(())
                    }
                    Discovery::Dht(Err(error)) => {
                        self.last_error = Some(error);
                        self.receive_tracker_peers().await
                    }
                }
            }
            (true, false) => self.receive_tracker_peers().await,
            (false, true) => self.receive_dht_peers(info_hash).await,
            (false, false) => Err(self
                .last_error
                .take()
                .unwrap_or(DownloadError::NoUsablePeer)),
        }
    }

    fn dht_requery_wait(&self) -> Duration {
        self.next_dht_lookup
            .saturating_duration_since(Instant::now())
    }

    #[cfg(test)]
    async fn connect_next(
        &mut self,
        info_hash: [u8; 20],
        advertise_extensions: bool,
    ) -> Result<Handshake, DownloadError> {
        debug_assert!(self.connection.is_none());
        loop {
            let context = PeerSelectionContext {
                now: self.elapsed(),
            };
            let candidate = match self.selector.select(&self.registry, context) {
                Some(candidate) => candidate,
                None => {
                    self.receive_discovery_peers(info_hash).await?;
                    continue;
                }
            };
            self.control.emit(DownloadActivityEvent::PeerDialStarted {
                peer: candidate.endpoint().to_string(),
            });
            let attempt = self.begin_dial(candidate, PeerConnectionRole::Content)?;
            match connect_peer(attempt, info_hash, advertise_extensions, self.network).await {
                Ok((connection, handshake)) => {
                    self.dial_succeeded(attempt, &handshake)?;
                    self.connection = Some(connection);
                    return Ok(handshake);
                }
                Err(error) => {
                    self.dial_failed(attempt, peer_failure(&error))?;
                    self.last_error = Some(error);
                }
            }
        }
    }

    async fn acquire_metadata(
        &mut self,
        info_hash: [u8; 20],
    ) -> Result<(Vec<u8>, Metainfo), DownloadError> {
        self.control.metadata_started();
        let result = self.acquire_metadata_inner(info_hash).await;
        self.control.observe_metadata_supervisor(
            self.registry.snapshot(PeerSelectionContext {
                now: self.elapsed(),
            }),
            0,
            0,
            self.last_error.as_ref(),
        );
        if let Ok((_, metainfo)) = &result {
            self.control.emit(DownloadActivityEvent::MetadataVerified {
                total_length: metainfo.total_length,
                piece_length: metainfo.piece_length,
                piece_count: metainfo.piece_hashes.len(),
                file_count: metainfo.files.len(),
            });
        }
        self.control.metadata_finished(&result);
        result
    }

    async fn acquire_metadata_inner(
        &mut self,
        info_hash: [u8; 20],
    ) -> Result<(Vec<u8>, Metainfo), DownloadError> {
        debug_assert!(self.connection.is_none());
        let mut sockets = PeerSocketSet::new();
        let mut workers = JoinSet::new();
        let mut worker_cancellations = BTreeMap::new();
        let mut discovery_failed_while_active = false;
        let metadata = Arc::new(Mutex::new(TorrentMetadataDownload::new(info_hash)));

        loop {
            while sockets.pending_len() + workers.len() < MAX_METADATA_PEERS {
                let context = PeerSelectionContext {
                    now: self.elapsed(),
                };
                let Some(candidate) = self.selector.select(&self.registry, context) else {
                    break;
                };
                self.control.emit(DownloadActivityEvent::PeerDialStarted {
                    peer: candidate.endpoint().to_string(),
                });
                let attempt = self.begin_dial(candidate, PeerConnectionRole::Metadata)?;
                self.control.metadata_dial_started(attempt);
                if let Err(error) = sockets.begin_dial(
                    attempt,
                    info_hash,
                    true,
                    self.network,
                    self.control.byte_metric_sink(),
                ) {
                    self.dial_cancelled(attempt)?;
                    return Err(download_peer_set_error(error));
                }
            }

            self.control.observe_metadata_supervisor(
                self.registry.snapshot(PeerSelectionContext {
                    now: self.elapsed(),
                }),
                sockets.pending_len(),
                workers.len(),
                self.last_error.as_ref(),
            );

            if sockets.pending_len() == 0 && workers.is_empty() {
                let cancellation = self.control.inner.cancellation.clone();
                tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => {
                        return Err(DownloadError::Cancelled);
                    }
                    result = self.receive_discovery_peers(info_hash) => result?,
                }
                discovery_failed_while_active = false;
                continue;
            }

            let can_discover = !discovery_failed_while_active
                && sockets.pending_len() + workers.len() < MAX_METADATA_PEERS
                && (self.tracker.is_some() || self.dht.is_some());
            let cancellation = self.control.inner.cancellation.clone();
            let event = tokio::select! {
                biased;
                _ = cancellation.cancelled() => {
                    MetadataSupervisorEvent::Cancelled
                }
                result = self.receive_discovery_peers(info_hash), if can_discover => {
                    MetadataSupervisorEvent::Discovery(result)
                }
                event = sockets.next_event() => MetadataSupervisorEvent::Socket(event),
                joined = workers.join_next(), if !workers.is_empty() => {
                    MetadataSupervisorEvent::Worker(joined)
                }
            };
            match event {
                MetadataSupervisorEvent::Socket(Ok(PeerSetEvent::DialPhase { attempt })) => {
                    self.transport_connected(attempt)?;
                }
                MetadataSupervisorEvent::Discovery(Ok(())) => {
                    discovery_failed_while_active = false;
                }
                MetadataSupervisorEvent::Discovery(Err(error)) => {
                    self.last_error = Some(error);
                    discovery_failed_while_active = true;
                }
                MetadataSupervisorEvent::Cancelled => {
                    cleanup_metadata_attempts(
                        self,
                        &mut sockets,
                        &mut workers,
                        &mut worker_cancellations,
                    )
                    .await?;
                    return Err(DownloadError::Cancelled);
                }
                MetadataSupervisorEvent::Socket(Ok(PeerSetEvent::DialCompleted {
                    attempt,
                    result: Ok((connection, handshake)),
                })) => {
                    self.dial_succeeded(attempt, &handshake)?;
                    self.control
                        .metadata_peer_connected(attempt, handshake.supports_extensions());
                    let cancellation = CancellationToken::new();
                    worker_cancellations.insert(attempt.id(), (attempt, cancellation.clone()));
                    let control = self.control.clone();
                    let metadata = metadata.clone();
                    workers.spawn(async move {
                        run_metadata_peer(connection, handshake, cancellation, control, metadata)
                            .await
                    });
                }
                MetadataSupervisorEvent::Socket(Ok(PeerSetEvent::DialCompleted {
                    attempt,
                    result: Err(error),
                })) => {
                    if matches!(&error, PeerSocketError::Cancelled) {
                        self.dial_cancelled(attempt)?;
                        self.control.metadata_peer_finished(
                            attempt.id(),
                            MetadataPeerStage::Cancelled,
                            Some("metadata dial cancelled"),
                        );
                    } else {
                        let detail = error.to_string();
                        self.dial_failed(attempt, error.peer_failure())?;
                        self.last_error = Some(download_peer_socket_error(error));
                        self.control.metadata_peer_finished(
                            attempt.id(),
                            MetadataPeerStage::Failed,
                            Some(&detail),
                        );
                    }
                }
                MetadataSupervisorEvent::Socket(Ok(PeerSetEvent::Peer(_))) => {
                    cleanup_metadata_attempts(
                        self,
                        &mut sockets,
                        &mut workers,
                        &mut worker_cancellations,
                    )
                    .await?;
                    return Err(DownloadError::PeerTask(
                        "metadata socket set produced an impossible peer event".to_owned(),
                    ));
                }
                MetadataSupervisorEvent::Socket(Err(error)) => {
                    cleanup_metadata_attempts(
                        self,
                        &mut sockets,
                        &mut workers,
                        &mut worker_cancellations,
                    )
                    .await?;
                    return Err(download_peer_set_error(error));
                }
                MetadataSupervisorEvent::Worker(Some(Ok(MetadataPeerResult::Complete {
                    connection,
                    raw_info,
                    metainfo,
                }))) => {
                    worker_cancellations.remove(&connection.attempt().id());
                    cleanup_metadata_attempts(
                        self,
                        &mut sockets,
                        &mut workers,
                        &mut worker_cancellations,
                    )
                    .await?;
                    self.connection = Some(connection);
                    if metainfo.private {
                        self.disable_dht_for_private(info_hash).await?;
                    }
                    return Ok((raw_info, metainfo));
                }
                MetadataSupervisorEvent::Worker(Some(Ok(MetadataPeerResult::Failed {
                    connection,
                    error,
                }))) => {
                    worker_cancellations.remove(&connection.attempt().id());
                    let failure = peer_failure(&error);
                    self.connection_closed(connection.attempt(), Some(failure))?;
                    self.last_error = Some(error);
                }
                MetadataSupervisorEvent::Worker(Some(Ok(MetadataPeerResult::Cancelled {
                    connection,
                }))) => {
                    worker_cancellations.remove(&connection.attempt().id());
                    self.connection_closed(connection.attempt(), None)?;
                }
                MetadataSupervisorEvent::Worker(Some(Err(error))) => {
                    cleanup_metadata_attempts(
                        self,
                        &mut sockets,
                        &mut workers,
                        &mut worker_cancellations,
                    )
                    .await?;
                    return Err(DownloadError::PeerTask(error.to_string()));
                }
                MetadataSupervisorEvent::Worker(None) => {
                    cleanup_metadata_attempts(
                        self,
                        &mut sockets,
                        &mut workers,
                        &mut worker_cancellations,
                    )
                    .await?;
                    return Err(DownloadError::PeerTask(
                        "metadata worker set ended unexpectedly".to_owned(),
                    ));
                }
            }
        }
    }

    fn close_current(&mut self, failure: Option<PeerFailure>) -> Result<(), DownloadError> {
        let Some(connection) = self.connection.take() else {
            return Ok(());
        };
        self.begin_disconnect(connection.attempt(), failure)?;
        self.connection_closed(connection.attempt(), failure)
    }

    async fn shutdown_tracker(&mut self) -> Result<(), DownloadError> {
        let result = match self.tracker.take() {
            Some(tracker) => tracker.shutdown().await,
            None => Ok(()),
        };
        self.control
            .observe_peer_registry(&self.registry, self.elapsed(), false, true);
        result
    }

    async fn disable_dht_for_private(&mut self, info_hash: [u8; 20]) -> Result<(), DownloadError> {
        let current_is_dht_only = self.connection.as_ref().is_some_and(|connection| {
            self.registry
                .get(connection.attempt().record_id())
                .is_some_and(|record| {
                    record.sources().contains(PeerSource::Dht) && record.sources().len() == 1
                })
        });
        if current_is_dht_only {
            self.close_current(None)?;
        }
        if let Some(dht) = self.dht.take() {
            dht.cancel_lookup(info_hash)
                .await
                .map_err(DownloadError::Dht)?;
        }
        self.registry.remove_source(PeerSource::Dht);
        self.control
            .emit(DownloadActivityEvent::DhtDisabledForPrivateTorrent);
        Ok(())
    }
}

async fn run_metadata_peer(
    mut connection: PeerConnection,
    handshake: Handshake,
    cancellation: CancellationToken,
    control: DownloadControl,
    metadata: Arc<Mutex<TorrentMetadataDownload>>,
) -> MetadataPeerResult {
    let attempt = connection.attempt();
    let result = tokio::select! {
        biased;
        _ = cancellation.cancelled() => None,
        result = acquire_metadata_from_connection(
            &mut connection,
            handshake,
            &control,
            &metadata,
        ) => {
            Some(result)
        }
    };
    metadata
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove_peer(attempt.id().get());
    match &result {
        Some(Ok(_)) => {
            control.metadata_peer_finished(attempt.id(), MetadataPeerStage::Complete, None)
        }
        Some(Err(error)) => {
            let detail = error.to_string();
            control.metadata_peer_finished(attempt.id(), MetadataPeerStage::Failed, Some(&detail));
        }
        None => control.metadata_peer_finished(
            attempt.id(),
            MetadataPeerStage::Cancelled,
            Some("metadata attempt cancelled"),
        ),
    }
    match result {
        Some(Ok((raw_info, metainfo))) => MetadataPeerResult::Complete {
            connection,
            raw_info,
            metainfo,
        },
        Some(Err(error)) => MetadataPeerResult::Failed { connection, error },
        None => MetadataPeerResult::Cancelled { connection },
    }
}

async fn cleanup_metadata_attempts(
    peers: &mut TorrentPeerCoordinator,
    sockets: &mut PeerSocketSet,
    workers: &mut JoinSet<MetadataPeerResult>,
    worker_cancellations: &mut BTreeMap<DialAttemptId, (DialAttempt, CancellationToken)>,
) -> Result<(), DownloadError> {
    let mut first_error = None;
    for attempt in sockets
        .pending_attempts()
        .into_iter()
        .chain(worker_cancellations.values().map(|(attempt, _)| *attempt))
    {
        if let Err(error) = peers.begin_disconnect(attempt, None)
            && first_error.is_none()
        {
            first_error = Some(error);
        }
    }
    for (_, cancellation) in worker_cancellations.values() {
        cancellation.cancel();
    }

    match std::mem::take(sockets).shutdown().await {
        Ok(pending) => {
            for attempt in pending {
                if let Err(error) = peers.dial_cancelled(attempt)
                    && first_error.is_none()
                {
                    first_error = Some(error);
                }
            }
        }
        Err(error) => first_error = Some(download_peer_set_error(error)),
    }

    while let Some(joined) = workers.join_next().await {
        match joined {
            Ok(result) => {
                let attempt = result.attempt();
                worker_cancellations.remove(&attempt.id());
                if let Err(error) = peers.connection_closed(attempt, None)
                    && first_error.is_none()
                {
                    first_error = Some(error);
                }
            }
            Err(error) if first_error.is_none() => {
                first_error = Some(DownloadError::PeerTask(error.to_string()));
            }
            Err(_) => {}
        }
    }
    for (_, (attempt, _)) in std::mem::take(worker_cancellations) {
        if let Err(error) = peers.connection_closed(attempt, None)
            && first_error.is_none()
        {
            first_error = Some(error);
        }
    }

    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn random_nonzero_u32() -> Result<u32, DownloadError> {
    let mut bytes = [0; 4];
    getrandom::fill(&mut bytes).map_err(DownloadError::Entropy)?;
    Ok(u32::from_ne_bytes(bytes).max(1))
}

fn compact_peer_address(peer: CompactPeer) -> SocketAddr {
    match peer {
        CompactPeer::Ipv4 { address, port } => SocketAddr::from((Ipv4Addr::from(address), port)),
        CompactPeer::Ipv6 { address, port } => SocketAddr::from((Ipv6Addr::from(address), port)),
    }
}

async fn resolve_host(
    host: &str,
    port: u16,
    operation: &'static str,
) -> Result<Vec<SocketAddr>, DownloadError> {
    timeout(NETWORK_RESOLUTION_TIMEOUT, lookup_host((host, port)))
        .await
        .map_err(|_| DownloadError::NetworkTimedOut {
            operation,
            timeout: NETWORK_RESOLUTION_TIMEOUT,
        })?
        .map(|addresses| addresses.take(MAX_RESOLVED_ADDRESSES).collect())
        .map_err(|source| DownloadError::Io { operation, source })
}

async fn announce_udp_tracker(
    tracker: &UdpTrackerUrl,
    network_policy: NetworkPolicy,
    token_cache: &mut UdpTrackerTokenCache,
    announce: UdpTrackerAnnounce,
    exchange: UdpTrackerExchange<'_>,
) -> Result<AnnounceResponse, DownloadError> {
    if !network_policy.permits_dns() {
        return Err(DownloadError::NetworkDisabled);
    }
    let addresses = resolve_host(&tracker.host, tracker.port, "resolve UDP tracker").await?;
    let mut last_error = None;
    let mut found_allowed = false;
    for address in addresses {
        if !network_policy.allows(address) {
            continue;
        }
        found_allowed = true;
        match announce_udp_tracker_address(address, token_cache, announce, exchange).await {
            Ok(response) => return Ok(response),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or({
        if found_allowed {
            DownloadError::NoUsablePeer
        } else {
            DownloadError::NoUsableTrackerAddress
        }
    }))
}

async fn announce_udp_tracker_address(
    address: SocketAddr,
    token_cache: &mut UdpTrackerTokenCache,
    announce: UdpTrackerAnnounce,
    exchange: UdpTrackerExchange<'_>,
) -> Result<AnnounceResponse, DownloadError> {
    let bind_address = match address {
        SocketAddr::V4(_) => SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)),
        SocketAddr::V6(_) => SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0)),
    };
    let socket = UdpSocket::bind(bind_address)
        .await
        .map_err(|source| DownloadError::Io {
            operation: "bind UDP tracker socket",
            source,
        })?;
    socket
        .connect(address)
        .await
        .map_err(|source| DownloadError::Io {
            operation: "connect UDP tracker socket",
            source,
        })?;

    let connection_id = match token_cache.get(address, Instant::now()) {
        Some(connection_id) => connection_id,
        None => {
            let connect_transaction = TransactionId::new(random_nonzero_u32()?);
            let connect_request = encode_connect_request(connect_transaction);
            let connection_id = exchange_udp_tracker_packet(
                &socket,
                &connect_request,
                "connect response",
                exchange.timing,
                exchange.control,
                exchange.tracker_label,
                |bytes| parse_connect_response(bytes, connect_transaction),
            )
            .await?;
            token_cache.insert(address, connection_id, Instant::now());
            connection_id
        }
    };

    let announce_transaction = TransactionId::new(random_nonzero_u32()?);
    let request = encode_announce_request(AnnounceRequest {
        connection_id,
        transaction_id: announce_transaction,
        info_hash: announce.info_hash,
        peer_id: CLIENT_PEER_ID,
        downloaded: 0,
        left: UNKNOWN_MAGNET_LEFT,
        uploaded: 0,
        event: announce.event,
        ip_address: 0,
        key: announce.key,
        num_want: MAX_COMPACT_PEERS as i32,
        port: announce.port,
    });
    let family = match address {
        SocketAddr::V4(_) => TrackerAddressFamily::Ipv4,
        SocketAddr::V6(_) => TrackerAddressFamily::Ipv6,
    };
    let result = exchange_udp_tracker_packet(
        &socket,
        &request,
        "announce response",
        exchange.timing,
        exchange.control,
        exchange.tracker_label,
        |bytes| parse_announce_response(bytes, announce_transaction, family),
    )
    .await;
    if result.is_err() {
        token_cache.remove(address);
    }
    result
}

async fn send_udp_tracker_packet(
    socket: &UdpSocket,
    packet: &[u8],
    operation: &'static str,
    timeout_duration: Duration,
) -> Result<(), DownloadError> {
    let sent = socket.send(packet);
    let sent = timeout(timeout_duration, sent)
        .await
        .map_err(|_| DownloadError::NetworkTimedOut {
            operation,
            timeout: timeout_duration,
        })?
        .map_err(|source| DownloadError::Io { operation, source })?;
    if sent != packet.len() {
        return Err(DownloadError::Io {
            operation,
            source: io::Error::new(io::ErrorKind::WriteZero, "short UDP tracker send"),
        });
    }
    Ok(())
}

async fn exchange_udp_tracker_packet<T>(
    socket: &UdpSocket,
    packet: &[u8],
    operation: &'static str,
    timing: UdpTrackerTiming,
    control: &DownloadControl,
    tracker_label: &str,
    parse: impl Fn(&[u8]) -> Result<T, UdpTrackerError>,
) -> Result<T, DownloadError> {
    send_udp_tracker_packet(
        socket,
        packet,
        "send UDP tracker request",
        timing.completion_timeout,
    )
    .await?;
    control.record_bytes(ByteMetric::TrackerSent, packet.len());
    let started = TokioInstant::now();
    let retransmit_at = started + timing.retransmit_after;
    let deadline = started + timing.completion_timeout;
    let mut retransmitted = false;
    let mut buffer = [0; UDP_TRACKER_RECEIVE_LENGTH];
    loop {
        let next_deadline = if retransmitted {
            deadline
        } else {
            retransmit_at.min(deadline)
        };
        let received = match timeout_at(next_deadline, socket.recv(&mut buffer)).await {
            Ok(result) => result.map_err(|source| DownloadError::Io {
                operation: "receive UDP tracker response",
                source,
            })?,
            Err(_) if !retransmitted && next_deadline < deadline => {
                send_udp_tracker_packet(
                    socket,
                    packet,
                    "retransmit UDP tracker request",
                    timing.completion_timeout,
                )
                .await?;
                control.record_bytes(ByteMetric::TrackerSent, packet.len());
                retransmitted = true;
                control.emit(DownloadActivityEvent::TrackerUdpRetransmitted {
                    tracker: tracker_label.to_owned(),
                    operation,
                });
                continue;
            }
            Err(_) => {
                return Err(DownloadError::UdpTrackerTimedOut {
                    operation,
                    timeout: timing.completion_timeout,
                });
            }
        };
        control.record_bytes(ByteMetric::TrackerReceived, received);
        if received > MAX_ANNOUNCE_RESPONSE_LENGTH {
            return Err(DownloadError::UdpTrackerResponseTooLarge {
                maximum: MAX_ANNOUNCE_RESPONSE_LENGTH,
            });
        }
        if received < 8 {
            continue;
        }
        match parse(&buffer[..received]) {
            Err(UdpTrackerError::UnexpectedTransaction { .. }) => {}
            result => return result.map_err(DownloadError::UdpTracker),
        }
    }
}

fn peer_failure(error: &DownloadError) -> PeerFailure {
    match error {
        DownloadError::Io {
            operation: "connect to peer",
            ..
        }
        | DownloadError::PeerTimedOut {
            operation: "connect",
            ..
        } => PeerFailure::Connect,
        DownloadError::PeerTimedOut {
            operation: "handshake read" | "handshake write",
            ..
        } => PeerFailure::Handshake,
        DownloadError::Handshake(_) => PeerFailure::Handshake,
        DownloadError::PeerClosed => PeerFailure::RemoteClosed,
        _ => PeerFailure::Protocol,
    }
}

fn content_peer_failure(error: &DownloadError) -> Option<PeerFailure> {
    match error {
        DownloadError::PeerClosed => Some(PeerFailure::RemoteClosed),
        DownloadError::Frame(_)
        | DownloadError::Piece(_)
        | DownloadError::Handshake(_)
        | DownloadError::InvalidPremetadataState(_)
        | DownloadError::Metadata(_)
        | DownloadError::MetadataExtensionDisabled
        | DownloadError::ExtensionProtocolUnsupported
        | DownloadError::PeerTimedOut {
            operation: "message read" | "message write",
            ..
        } => Some(peer_failure(error)),
        DownloadError::Io {
            operation: "read peer message" | "send peer message",
            ..
        } => Some(PeerFailure::RemoteClosed),
        _ => None,
    }
}

async fn run_magnet_download(
    config: MagnetDownloadConfig,
    control: DownloadControl,
) -> Result<DownloadReport, DownloadError> {
    let magnet = Magnet::parse(&config.magnet).map_err(DownloadError::Magnet)?;
    let mut peers = TorrentPeerCoordinator::from_magnet(
        &magnet,
        config.network,
        control.clone(),
        config.dht.clone(),
    )
    .await?;
    let result = run_magnet_download_with_peers(config, control, magnet, &mut peers).await;
    merge_tracker_shutdown(result, peers.shutdown_tracker().await)
}

async fn run_magnet_download_with_peers(
    config: MagnetDownloadConfig,
    control: DownloadControl,
    magnet: Magnet,
    peers: &mut TorrentPeerCoordinator,
) -> Result<DownloadReport, DownloadError> {
    let (_raw_info, metainfo) = peers.acquire_metadata(magnet.info_hash).await?;
    let content_config = ContentDownloadConfig {
        output_path: config.output_path,
        max_buffered_payload_bytes: config.resource_limits.max_buffered_payload_bytes,
        swarm_config: config.resource_limits.swarm_config(),
        skip_files: config.skip_files,
        materialize_files: config.materialize_files,
    };
    run_content_download(content_config, metainfo, control, None, peers, None).await
}

async fn run_magnet_metadata(
    magnet: String,
    network: NetworkConfig,
    control: DownloadControl,
    dht: Option<DhtHandle>,
) -> Result<Vec<u8>, DownloadError> {
    let magnet = Magnet::parse(&magnet).map_err(DownloadError::Magnet)?;
    let mut peers = TorrentPeerCoordinator::from_magnet(&magnet, network, control, dht).await?;
    let result = async {
        let (raw_info, _) = peers.acquire_metadata(magnet.info_hash).await?;
        peers.close_current(None)?;
        Ok(raw_info)
    }
    .await;
    merge_tracker_shutdown(result, peers.shutdown_tracker().await)
}

fn merge_tracker_shutdown<T>(
    result: Result<T, DownloadError>,
    shutdown: Result<(), DownloadError>,
) -> Result<T, DownloadError> {
    match (result, shutdown) {
        (Ok(value), Ok(())) => Ok(value),
        (Ok(_), Err(error)) | (Err(error), Ok(())) => Err(error),
        (Err(error), Err(cleanup)) => Err(DownloadError::PeerCleanup {
            failure: error.to_string(),
            cleanup: cleanup.to_string(),
        }),
    }
}

#[derive(Clone)]
struct ResumeContext {
    verified_pieces: Vec<bool>,
    checkpoints: Arc<dyn DownloadCheckpointSink>,
    initialize_descriptors: bool,
    artifact_state: ResumeArtifactState,
    download_missing: bool,
}

async fn run_resumable_magnet_download(
    config: ResumableMagnetDownloadConfig,
    checkpoints: Arc<dyn DownloadCheckpointSink>,
    control: DownloadControl,
    descriptors: Option<(DescriptorStorage, bool)>,
) -> Result<DownloadReport, DownloadError> {
    let magnet = Magnet::parse(&config.magnet).map_err(DownloadError::Magnet)?;
    let dht = config.dht.clone();
    let resume = ResumeContext {
        verified_pieces: config.verified_pieces,
        checkpoints: checkpoints.clone(),
        artifact_state: config.artifact_state,
        download_missing: config.download_missing,
        initialize_descriptors: descriptors
            .as_ref()
            .is_some_and(|(_, initialize)| *initialize),
    };
    let descriptors = descriptors.map(|(descriptors, _)| descriptors);

    if let Some(raw_info) = config.verified_info {
        let metainfo = Metainfo::from_info_bytes_with_limits(&raw_info, DURABLE_METAINFO_LIMITS)
            .map_err(DownloadError::Metainfo)?;
        if metainfo.info_hash != magnet.info_hash {
            return Err(DownloadError::Checkpoint(
                "stored metadata does not match the magnet identity".to_owned(),
            ));
        }
        validate_publication_name(&metainfo.name).map_err(DownloadError::SelectiveStorage)?;
        let content_dht = if metainfo.private {
            control.emit(DownloadActivityEvent::DhtDisabledForPrivateTorrent);
            None
        } else {
            dht.clone()
        };
        let mut peers = TorrentPeerCoordinator::from_magnet(
            &magnet,
            config.network,
            control.clone(),
            content_dht,
        )
        .await?;
        let output_path = if descriptors.is_some() {
            PathBuf::new()
        } else {
            config.storage_root.join(&metainfo.name)
        };
        let content_config = ContentDownloadConfig {
            output_path,
            max_buffered_payload_bytes: config.resource_limits.max_buffered_payload_bytes,
            swarm_config: config.resource_limits.swarm_config(),
            skip_files: config.skip_files,
            materialize_files: Vec::new(),
        };
        let result = run_content_download(
            content_config,
            metainfo,
            control,
            descriptors,
            &mut peers,
            Some(resume),
        )
        .await;
        return merge_tracker_shutdown(result, peers.shutdown_tracker().await);
    }

    if descriptors.is_some() {
        return Err(DownloadError::Checkpoint(
            "descriptor storage requires verified metadata".to_owned(),
        ));
    }
    let mut peers =
        TorrentPeerCoordinator::from_magnet(&magnet, config.network, control.clone(), dht).await?;
    let result = async {
        let (raw_info, metainfo) = peers.acquire_metadata(magnet.info_hash).await?;
        validate_publication_name(&metainfo.name).map_err(DownloadError::SelectiveStorage)?;
        if let Err(message) = checkpoints.metadata_verified(&raw_info) {
            peers.close_current(None)?;
            return Err(DownloadError::Checkpoint(message));
        }
        let content_config = ContentDownloadConfig {
            output_path: config.storage_root.join(&metainfo.name),
            max_buffered_payload_bytes: config.resource_limits.max_buffered_payload_bytes,
            swarm_config: config.resource_limits.swarm_config(),
            skip_files: config.skip_files,
            materialize_files: Vec::new(),
        };
        run_content_download(
            content_config,
            metainfo,
            control,
            None,
            &mut peers,
            Some(resume),
        )
        .await
    }
    .await;
    merge_tracker_shutdown(result, peers.shutdown_tracker().await)
}

async fn acquire_metadata_from_connection(
    peer: &mut PeerConnection,
    handshake: Handshake,
    control: &DownloadControl,
    metadata: &Arc<Mutex<TorrentMetadataDownload>>,
) -> Result<(Vec<u8>, Metainfo), DownloadError> {
    if !handshake.supports_extensions() {
        return Err(DownloadError::ExtensionProtocolUnsupported);
    }
    send_message(
        peer,
        &PeerMessage::Extended {
            id: 0,
            payload: encode_extension_handshake(None),
        },
    )
    .await?;

    let mut peer_state = PremetadataPeerState::new();
    let mut remote_metadata_id = None;
    let mut received_extension_handshake = false;
    let metadata_progress_timeout = peer.io_timeout();
    let mut progress_deadline = TokioInstant::now() + metadata_progress_timeout;
    loop {
        if TokioInstant::now() >= progress_deadline {
            return Err(DownloadError::PeerTimedOut {
                operation: "metadata progress",
                timeout: metadata_progress_timeout,
            });
        }
        if let Some(remote_id) = remote_metadata_id {
            let requests_sent = send_torrent_metadata_requests(
                peer,
                remote_id,
                metadata_instant(control.diagnostic_elapsed()),
                metadata,
            )
            .await?;
            let download = metadata
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            control.metadata_requests_sent(peer.attempt().id(), &download, requests_sent);
        }

        let scheduler_wake = TokioInstant::now() + METADATA_SCHEDULER_TICK;
        let message = tokio::select! {
            result = next_peer_message(peer) => Some(result?),
            _ = tokio::time::sleep_until(scheduler_wake) => None,
        };
        let Some(message) = message else {
            if TokioInstant::now() >= progress_deadline {
                return Err(DownloadError::PeerTimedOut {
                    operation: "metadata progress",
                    timeout: metadata_progress_timeout,
                });
            }
            continue;
        };
        control.metadata_peer_message(peer.attempt().id());
        match message {
            PeerMessage::Extended { id: 0, payload } => {
                let handshake =
                    parse_extension_handshake(&payload).map_err(DownloadError::Metadata)?;
                if !received_extension_handshake
                    && handshake.metadata_extension == MetadataExtensionUpdate::Unchanged
                {
                    return Err(DownloadError::MetadataExtensionDisabled);
                }
                received_extension_handshake = true;
                match handshake.metadata_extension {
                    MetadataExtensionUpdate::Disabled => {
                        return Err(DownloadError::MetadataExtensionDisabled);
                    }
                    MetadataExtensionUpdate::Enabled(id) => {
                        remote_metadata_id = Some(id);
                        metadata
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .register_peer(peer.attempt().id().get(), handshake.metadata_size)
                            .map_err(DownloadError::Metadata)?;
                    }
                    MetadataExtensionUpdate::Unchanged => {
                        if let Some(size) = handshake.metadata_size {
                            metadata
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner())
                                .accept_peer_size(peer.attempt().id().get(), size)
                                .map_err(DownloadError::Metadata)?;
                        }
                    }
                }
                let requests_sent = match remote_metadata_id {
                    Some(remote_id) => {
                        send_torrent_metadata_requests(
                            peer,
                            remote_id,
                            metadata_instant(control.diagnostic_elapsed()),
                            metadata,
                        )
                        .await?
                    }
                    None => 0,
                };
                let download = metadata
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                control.metadata_extension_handshake(
                    peer.attempt().id(),
                    remote_metadata_id,
                    &download,
                    requests_sent,
                );
                progress_deadline = TokioInstant::now() + metadata_progress_timeout;
            }
            PeerMessage::Extended {
                id: UT_METADATA_LOCAL_ID,
                payload,
            } => {
                let message = parse_metadata_message(&payload).map_err(DownloadError::Metadata)?;
                if let MetadataMessage::Request { piece } = message {
                    if let Some(remote_id) = remote_metadata_id {
                        send_message(
                            peer,
                            &PeerMessage::Extended {
                                id: remote_id,
                                payload: encode_metadata_reject(piece),
                            },
                        )
                        .await?;
                    }
                    continue;
                }
                match message {
                    MetadataMessage::Data {
                        piece,
                        total_size,
                        block,
                    } => {
                        let block_bytes = block.len();
                        let event = metadata
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .on_data(
                                peer.attempt().id().get(),
                                piece,
                                total_size,
                                block,
                                metadata_instant(control.diagnostic_elapsed()),
                            )
                            .map_err(DownloadError::Metadata)?;
                        let completed = match event {
                            TorrentMetadataEvent::Complete(bytes) => Some(bytes),
                            TorrentMetadataEvent::HashMismatch { contributors } => {
                                control.metadata_hash_failed(contributors.len());
                                None
                            }
                            TorrentMetadataEvent::BlockAccepted { .. }
                            | TorrentMetadataEvent::Duplicate { .. } => None,
                        };
                        if let Some(bytes) = completed {
                            let download = metadata
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            control.metadata_block_received(
                                peer.attempt().id(),
                                block_bytes,
                                &download,
                                0,
                            );
                            drop(download);
                            return finish_metadata_acquisition(bytes, peer_state, peer);
                        }
                        let requests_sent = match remote_metadata_id {
                            Some(remote_id) => {
                                send_torrent_metadata_requests(
                                    peer,
                                    remote_id,
                                    metadata_instant(control.diagnostic_elapsed()),
                                    metadata,
                                )
                                .await?
                            }
                            None => 0,
                        };
                        let download = metadata
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        control.metadata_block_received(
                            peer.attempt().id(),
                            block_bytes,
                            &download,
                            requests_sent,
                        );
                        progress_deadline = TokioInstant::now() + metadata_progress_timeout;
                    }
                    MetadataMessage::Reject { piece } => {
                        metadata
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .on_reject(
                                peer.attempt().id().get(),
                                piece,
                                metadata_instant(control.diagnostic_elapsed()),
                            )
                            .map_err(DownloadError::Metadata)?;
                        control.metadata_rejected(peer.attempt().id());
                        return Err(DownloadError::Metadata(MetadataError::Rejected {
                            piece: u32::try_from(piece).expect("validated metadata piece"),
                        }));
                    }
                    MetadataMessage::Unknown { .. } => {}
                    MetadataMessage::Request { .. } => unreachable!("request returned above"),
                }
            }
            PeerMessage::Extended { .. } => {}
            message => peer_state.observe(message)?,
        }
    }
}

async fn send_torrent_metadata_requests(
    peer: &mut PeerConnection,
    remote_metadata_id: u8,
    now: MetadataInstant,
    metadata: &Arc<Mutex<TorrentMetadataDownload>>,
) -> Result<usize, DownloadError> {
    let requests = metadata
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .requests_for_peer(peer.attempt().id().get(), now)
        .map_err(DownloadError::Metadata)?;
    for piece in &requests {
        send_message(
            peer,
            &PeerMessage::Extended {
                id: remote_metadata_id,
                payload: encode_metadata_request(*piece),
            },
        )
        .await?;
    }
    Ok(requests.len())
}

fn metadata_instant(elapsed: Duration) -> MetadataInstant {
    MetadataInstant::from_millis(u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
}

fn metadata_block_count_for_diagnostics(download: &TorrentMetadataDownload) -> Option<usize> {
    let blocks = download.allocated_blocks();
    (blocks != 0).then_some(blocks)
}

fn finish_metadata_acquisition(
    bytes: Vec<u8>,
    peer_state: PremetadataPeerState,
    peer: &mut PeerConnection,
) -> Result<(Vec<u8>, Metainfo), DownloadError> {
    let metainfo = Metainfo::from_info_bytes_with_limits(&bytes, BEP9_METAINFO_LIMITS)
        .map_err(DownloadError::Metainfo)?;
    peer.prepend_messages(peer_state.validated_messages(&metainfo)?);
    Ok((bytes, metainfo))
}

async fn run_download(
    config: DownloadConfig,
    control: DownloadControl,
    descriptors: Option<DescriptorStorage>,
) -> Result<DownloadReport, DownloadError> {
    let metainfo_bytes = read_bounded_metainfo(&config.metainfo_path).await?;
    let metainfo = Metainfo::from_bytes_with_limits(&metainfo_bytes, BEP9_METAINFO_LIMITS)
        .map_err(DownloadError::Metainfo)?;
    let mut peers =
        TorrentPeerCoordinator::from_endpoint(config.peer, PeerSource::Manual, config.network)?;
    let content_config = ContentDownloadConfig {
        output_path: config.output_path,
        max_buffered_payload_bytes: config.resource_limits.max_buffered_payload_bytes,
        swarm_config: config.resource_limits.swarm_config(),
        skip_files: config.skip_files,
        materialize_files: config.materialize_files,
    };
    let result = run_content_download(
        content_config,
        metainfo,
        control,
        descriptors,
        &mut peers,
        None,
    )
    .await;
    merge_tracker_shutdown(result, peers.shutdown_tracker().await)
}

async fn run_content_download(
    config: ContentDownloadConfig,
    metainfo: Metainfo,
    control: DownloadControl,
    descriptors: Option<DescriptorStorage>,
    peers: &mut TorrentPeerCoordinator,
    resume: Option<ResumeContext>,
) -> Result<DownloadReport, DownloadError> {
    peers.control = control.clone();
    peers.publish_peer_registry(true);
    let result =
        run_selective_download(config, metainfo, control, descriptors, peers, resume).await;
    peers.close_current(result.as_ref().err().and_then(content_peer_failure))?;
    result
}

struct ContentStorage(Box<SelectiveStorage>);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ContentWriteStats {
    selected_bytes: usize,
    part_bytes: usize,
}

enum ContentStorageCommand {
    Write {
        block: BlockKey,
        generation: PieceGeneration,
        offset: u64,
        bytes: Vec<u8>,
    },
    Verify {
        piece: u32,
        generation: PieceGeneration,
        length: u32,
        expected: [u8; 20],
        durable: bool,
    },
}

impl ContentStorageCommand {
    fn kind(&self) -> StorageCommandKind {
        match self {
            Self::Write { .. } => StorageCommandKind::Write,
            Self::Verify { .. } => StorageCommandKind::Hash,
        }
    }

    fn write_bytes(&self) -> Option<usize> {
        match self {
            Self::Write { bytes, .. } => Some(bytes.len()),
            Self::Verify { .. } => None,
        }
    }
}

struct QueuedContentStorageCommand {
    enqueued_at: Instant,
    command: ContentStorageCommand,
}

struct PreparedContentWrite {
    block: BlockKey,
    generation: PieceGeneration,
    offset: u64,
    bytes: Vec<u8>,
    stats: ContentWriteStats,
}

struct ContentWriteMember {
    block: BlockKey,
    generation: PieceGeneration,
    stats: ContentWriteStats,
}

struct CoalescedContentWrite {
    piece: u32,
    begin: u32,
    offset: u64,
    bytes: Vec<u8>,
    members: Vec<ContentWriteMember>,
}

struct ContentWriteOperation(SelectiveWriteJob);

impl ContentWriteOperation {
    async fn execute(self) -> Result<(), DownloadError> {
        self.0
            .execute()
            .await
            .map(|_| ())
            .map_err(DownloadError::SelectiveStorage)
    }
}

struct PreparedPhysicalContentWrite {
    operation: ContentWriteOperation,
    members: Vec<ContentWriteMember>,
}

struct ContentWriteJob {
    writes: Vec<PreparedPhysicalContentWrite>,
}

struct ContentHashOperation(SelectiveHashPlan);

struct ContentHashJob {
    piece: u32,
    generation: PieceGeneration,
    length: u32,
    expected: [u8; 20],
    durable: bool,
    durability_targets: Vec<DurabilityTarget>,
    operation: ContentHashOperation,
}

struct ContentHashJobResult {
    piece: u32,
    generation: PieceGeneration,
    length: u32,
    expected: [u8; 20],
    durable: bool,
    durability_targets: Vec<DurabilityTarget>,
    result: Result<[u8; 20], DownloadError>,
}

enum ContentStorageJobResult {
    Write {
        started_at: Instant,
        blocks: Vec<BlockKey>,
        bytes: usize,
        completions: Vec<ContentStorageCompletion>,
    },
    Hash {
        started_at: Instant,
        result: ContentHashJobResult,
    },
}

enum ContentStorageCompletion {
    Write {
        block: BlockKey,
        generation: PieceGeneration,
        result: Result<ContentWriteStats, DownloadError>,
    },
    Verify {
        piece: u32,
        generation: PieceGeneration,
        length: u32,
        result: Result<ContentVerification, DownloadError>,
    },
}

struct ContentVerification {
    actual: [u8; 20],
    durability_targets: Vec<DurabilityTarget>,
}

struct PendingCheckpointIntent {
    intent: CheckpointIntent,
    permit: CheckpointPermit,
}

struct CheckpointPermit {
    _item: OwnedSemaphorePermit,
    _bytes: OwnedSemaphorePermit,
}

struct ContentCheckpointPipeline {
    intents: Option<mpsc::Sender<PendingCheckpointIntent>>,
    item_capacity: Arc<Semaphore>,
    byte_capacity: Arc<Semaphore>,
    failures: mpsc::Receiver<String>,
    task: JoinHandle<Result<(), DownloadError>>,
    started_at: Instant,
}

impl ContentCheckpointPipeline {
    fn start(
        handles: CheckpointHandles,
        checkpoints: Arc<dyn DownloadCheckpointSink>,
        control: DownloadControl,
    ) -> Result<Self, DownloadError> {
        let policy = CheckpointPolicy::new(
            CHECKPOINT_MAX_AGE,
            CHECKPOINT_MAX_DIRTY_BYTES,
            CHECKPOINT_MAX_PIECES,
        )
        .map_err(|error| DownloadError::StorageTask(error.to_string()))?;
        let (intent_sender, intent_receiver) = mpsc::channel(CHECKPOINT_INTENT_CAPACITY);
        let (failure_sender, failure_receiver) = mpsc::channel(1);
        let item_capacity = Arc::new(Semaphore::new(CHECKPOINT_INTENT_CAPACITY));
        let byte_permits = CHECKPOINT_MAX_DIRTY_BYTES / CHECKPOINT_BYTE_UNIT;
        let byte_capacity = Arc::new(Semaphore::new(
            usize::try_from(byte_permits)
                .map_err(|_| DownloadError::StorageTask("checkpoint byte bound overflow".into()))?,
        ));
        let started_at = Instant::now();
        let task_started_at = started_at;
        let task = tokio::spawn(async move {
            let result = run_content_checkpoint_task(
                intent_receiver,
                handles,
                checkpoints,
                policy,
                task_started_at,
                control,
            )
            .await;
            if let Err(error) = &result {
                let _ = failure_sender.try_send(error.to_string());
            }
            result
        });
        Ok(Self {
            intents: Some(intent_sender),
            item_capacity,
            byte_capacity,
            failures: failure_receiver,
            task,
            started_at,
        })
    }

    async fn enqueue(
        &self,
        piece_index: usize,
        length: u32,
        targets: Vec<DurabilityTarget>,
    ) -> Result<(), DownloadError> {
        let item = self
            .item_capacity
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| DownloadError::StorageTask("checkpoint item owner closed".to_owned()))?;
        let length_u64 = u64::from(length);
        let units = length_u64.div_ceil(CHECKPOINT_BYTE_UNIT);
        let maximum_units = CHECKPOINT_MAX_DIRTY_BYTES / CHECKPOINT_BYTE_UNIT;
        let units = units.min(maximum_units);
        let units = u32::try_from(units)
            .map_err(|_| DownloadError::StorageTask("checkpoint byte charge overflow".into()))?;
        let bytes = self
            .byte_capacity
            .clone()
            .acquire_many_owned(units)
            .await
            .map_err(|_| DownloadError::StorageTask("checkpoint byte owner closed".to_owned()))?;
        let intent =
            CheckpointIntent::new(piece_index, length_u64, self.started_at.elapsed(), targets)
                .map_err(|error| DownloadError::StorageTask(error.to_string()))?;
        let sender = self.intents.as_ref().ok_or_else(|| {
            DownloadError::StorageTask("checkpoint intent owner is stopped".to_owned())
        })?;
        sender
            .send(PendingCheckpointIntent {
                intent,
                permit: CheckpointPermit {
                    _item: item,
                    _bytes: bytes,
                },
            })
            .await
            .map_err(|_| DownloadError::StorageTask("checkpoint intent channel closed".to_owned()))
    }
}

struct ContentStoragePipeline {
    commands: Option<mpsc::Sender<QueuedContentStorageCommand>>,
    completions: mpsc::Receiver<ContentStorageCompletion>,
    cancellation: CancellationToken,
    task: JoinHandle<Result<ContentStorage, DownloadError>>,
    pending_commands: VecDeque<QueuedContentStorageCommand>,
    control: DownloadControl,
    max_buffered_payload_bytes: usize,
    job_limit: usize,
    queue_capacity: usize,
    checkpoint: Option<ContentCheckpointPipeline>,
}

impl ContentStoragePipeline {
    async fn start(
        mut storage: ContentStorage,
        control: &DownloadControl,
        max_buffered_payload_bytes: usize,
        checkpoints: Option<Arc<dyn DownloadCheckpointSink>>,
    ) -> Result<Self, DownloadError> {
        let checkpoint = match checkpoints {
            Some(checkpoints) => {
                let handles = storage
                    .0
                    .checkpoint_handles()
                    .await
                    .map_err(DownloadError::SelectiveStorage)?;
                Some(ContentCheckpointPipeline::start(
                    handles,
                    checkpoints,
                    control.clone(),
                )?)
            }
            None => None,
        };
        control.configure_disk_runtime(max_buffered_payload_bytes);
        let job_limit = content_storage_job_limit(max_buffered_payload_bytes);
        debug_assert_ne!(job_limit, 0);
        let queue_capacity = job_limit;
        let (command_sender, command_receiver) = mpsc::channel(queue_capacity);
        let (completion_sender, completion_receiver) = mpsc::channel(queue_capacity);
        let cancellation = CancellationToken::new();
        let task = tokio::spawn(run_content_storage_task(
            storage,
            command_receiver,
            completion_sender,
            cancellation.clone(),
            control.clone(),
            queue_capacity,
        ));
        Ok(Self {
            commands: Some(command_sender),
            completions: completion_receiver,
            cancellation,
            task,
            pending_commands: VecDeque::with_capacity(CONTENT_STORAGE_PENDING_QUEUE),
            control: control.clone(),
            max_buffered_payload_bytes,
            job_limit,
            queue_capacity,
            checkpoint,
        })
    }

    fn enqueue(&mut self, command: ContentStorageCommand) -> Result<(), DownloadError> {
        let buffered_bytes = command.write_bytes();
        if buffered_bytes.is_some_and(|bytes| {
            !self
                .control
                .try_buffer_payload(bytes, self.max_buffered_payload_bytes)
        }) {
            return Err(DownloadError::Swarm(SwarmError::Invariant(
                "received payload exceeded the storage buffer allowance",
            )));
        }
        let command = QueuedContentStorageCommand {
            enqueued_at: Instant::now(),
            command,
        };
        if !self.pending_commands.is_empty() {
            if self.pending_commands.len() >= CONTENT_STORAGE_PENDING_QUEUE {
                if let Some(bytes) = buffered_bytes {
                    self.control.abandon_queued_payload(bytes);
                }
                return Err(DownloadError::Swarm(SwarmError::Invariant(
                    "storage pending-command bound exceeded",
                )));
            }
            self.control.storage_job_started();
            self.pending_commands.push_back(command);
            return Ok(());
        }
        self.control.storage_job_started();
        let Some(sender) = &self.commands else {
            self.control.storage_job_finished();
            if let Some(bytes) = buffered_bytes {
                self.control.abandon_queued_payload(bytes);
            }
            return Err(DownloadError::StorageTask(
                "storage command owner is stopped".to_owned(),
            ));
        };
        match sender.try_send(command) {
            Ok(()) => {
                self.control.observe_storage_command_queue(
                    self.queue_capacity.saturating_sub(sender.capacity()),
                );
                Ok(())
            }
            Err(mpsc::error::TrySendError::Full(command)) => {
                self.pending_commands.push_back(command);
                Ok(())
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.control.storage_job_finished();
                if let Some(bytes) = buffered_bytes {
                    self.control.abandon_queued_payload(bytes);
                }
                Err(DownloadError::StorageTask(
                    "storage command channel closed".to_owned(),
                ))
            }
        }
    }

    fn flush_pending(&mut self) -> Result<bool, DownloadError> {
        while let Some(command) = self.pending_commands.pop_front() {
            let Some(sender) = &self.commands else {
                self.pending_commands.push_front(command);
                return Err(DownloadError::StorageTask(
                    "storage command owner is stopped".to_owned(),
                ));
            };
            match sender.try_send(command) {
                Ok(()) => {
                    self.control.observe_storage_command_queue(
                        self.queue_capacity.saturating_sub(sender.capacity()),
                    );
                }
                Err(mpsc::error::TrySendError::Full(command)) => {
                    self.pending_commands.push_front(command);
                    return Ok(false);
                }
                Err(mpsc::error::TrySendError::Closed(command)) => {
                    self.pending_commands.push_front(command);
                    return Err(DownloadError::StorageTask(
                        "storage command channel closed".to_owned(),
                    ));
                }
            }
        }
        Ok(true)
    }

    fn is_backpressured(&self) -> bool {
        self.control.storage_backpressured()
            || !self.pending_commands.is_empty()
            || self.control.storage_jobs_at_limit(self.job_limit)
    }

    async fn next_completion(&mut self) -> Result<ContentStorageCompletion, DownloadError> {
        let Some(checkpoint) = self.checkpoint.as_mut() else {
            return self.completions.recv().await.ok_or_else(|| {
                DownloadError::StorageTask("storage completion channel closed".to_owned())
            });
        };
        tokio::select! {
            completion = self.completions.recv() => completion.ok_or_else(|| {
                DownloadError::StorageTask("storage completion channel closed".to_owned())
            }),
            failure = checkpoint.failures.recv() => Err(DownloadError::Checkpoint(
                failure.unwrap_or_else(|| "checkpoint task stopped unexpectedly".to_owned())
            )),
        }
    }

    fn completion_received(&self, completion: &ContentStorageCompletion) {
        self.control.storage_job_finished();
        if let ContentStorageCompletion::Write { block, .. } = completion {
            self.control.release_buffered_payload(block.length as usize);
        }
    }

    async fn shutdown(mut self, cancel: bool) -> Result<ContentStorage, DownloadError> {
        self.commands.take();
        if let Some(checkpoint) = self.checkpoint.as_mut() {
            checkpoint.intents.take();
        }
        if cancel {
            self.cancellation.cancel();
        }
        let storage_result = self
            .task
            .await
            .map_err(|error| DownloadError::StorageTask(error.to_string()))
            .and_then(|result| result);
        let checkpoint_result = match self.checkpoint {
            Some(checkpoint) => checkpoint
                .task
                .await
                .map_err(|error| DownloadError::StorageTask(error.to_string()))?,
            None => Ok(()),
        };
        self.control.clear_storage_jobs();
        self.control.clear_buffered_payload();
        match (storage_result, checkpoint_result) {
            (Ok(storage), Ok(())) => Ok(storage),
            (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
            (Err(error), Err(checkpoint)) => Err(DownloadError::StorageTask(format!(
                "{error}; additionally {checkpoint}"
            ))),
        }
    }
}

async fn run_content_checkpoint_task(
    mut intents: mpsc::Receiver<PendingCheckpointIntent>,
    handles: CheckpointHandles,
    checkpoints: Arc<dyn DownloadCheckpointSink>,
    policy: CheckpointPolicy,
    started_at: Instant,
    control: DownloadControl,
) -> Result<(), DownloadError> {
    let mut state = CheckpointBatchState::new(policy);
    let mut permits = BTreeMap::new();
    let mut pending = None;
    loop {
        let next = match pending.take() {
            Some(intent) => Some(intent),
            None if state.len() == 0 => intents.recv().await,
            None => {
                let wait = state.next_flush_in(started_at.elapsed()).ok_or_else(|| {
                    DownloadError::StorageTask(
                        "nonempty checkpoint batch has no age deadline".to_owned(),
                    )
                })?;
                match timeout(wait, intents.recv()).await {
                    Ok(intent) => intent,
                    Err(_) => {
                        flush_content_checkpoint(
                            &mut state,
                            &mut permits,
                            &handles,
                            &checkpoints,
                            &control,
                        )
                        .await?;
                        continue;
                    }
                }
            }
        };
        let Some(mut next) = next else {
            flush_content_checkpoint(&mut state, &mut permits, &handles, &checkpoints, &control)
                .await?;
            return Ok(());
        };
        let piece_index = next.intent.piece_index;
        match state
            .admit(next.intent, started_at.elapsed())
            .map_err(|error| DownloadError::StorageTask(error.to_string()))?
        {
            CheckpointAdmission::Accumulating => {
                if permits.insert(piece_index, next.permit).is_some() {
                    return Err(DownloadError::StorageTask(
                        "checkpoint permit piece is duplicated".to_owned(),
                    ));
                }
            }
            CheckpointAdmission::Ready(_) => {
                if permits.insert(piece_index, next.permit).is_some() {
                    return Err(DownloadError::StorageTask(
                        "checkpoint permit piece is duplicated".to_owned(),
                    ));
                }
                flush_content_checkpoint(
                    &mut state,
                    &mut permits,
                    &handles,
                    &checkpoints,
                    &control,
                )
                .await?;
            }
            CheckpointAdmission::FlushBefore { intent, .. } => {
                flush_content_checkpoint(
                    &mut state,
                    &mut permits,
                    &handles,
                    &checkpoints,
                    &control,
                )
                .await?;
                next.intent = intent;
                pending = Some(next);
            }
        }
    }
}

async fn flush_content_checkpoint(
    state: &mut CheckpointBatchState,
    permits: &mut BTreeMap<usize, CheckpointPermit>,
    handles: &CheckpointHandles,
    checkpoints: &Arc<dyn DownloadCheckpointSink>,
    control: &DownloadControl,
) -> Result<(), DownloadError> {
    let expected_dirty_bytes = state.dirty_bytes();
    let Some(batch) = state.take() else {
        return Ok(());
    };
    let actual_dirty_bytes = batch.intents.iter().try_fold(0_u64, |total, intent| {
        total.checked_add(intent.length).ok_or_else(|| {
            DownloadError::StorageTask("checkpoint batch byte sum overflow".to_owned())
        })
    })?;
    let oldest_verified_at = batch
        .intents
        .iter()
        .map(|intent| intent.verified_at)
        .min()
        .ok_or_else(|| DownloadError::StorageTask("checkpoint batch is empty".to_owned()))?;
    if actual_dirty_bytes != batch.dirty_bytes
        || actual_dirty_bytes != expected_dirty_bytes
        || oldest_verified_at != batch.oldest_verified_at
    {
        return Err(DownloadError::StorageTask(
            "checkpoint batch accounting diverged".to_owned(),
        ));
    }
    let mut batch_permits = Vec::with_capacity(batch.intents.len());
    let mut piece_indices = Vec::with_capacity(batch.intents.len());
    for intent in &batch.intents {
        piece_indices.push(intent.piece_index);
        batch_permits.push(permits.remove(&intent.piece_index).ok_or_else(|| {
            DownloadError::StorageTask(format!(
                "checkpoint piece {} has no capacity permit",
                intent.piece_index
            ))
        })?);
    }
    if !permits.is_empty() {
        return Err(DownloadError::StorageTask(
            "checkpoint permits escaped their batch".to_owned(),
        ));
    }
    control.disk_checkpoint_sync_started(&batch);
    let sync_started = Instant::now();
    control.wait_before_checkpoint_sync().await;
    let sync_result = if control.take_checkpoint_sync_failure() {
        Err(DownloadError::Checkpoint(
            "injected checkpoint sync failure".to_owned(),
        ))
    } else {
        sync_checkpoint_targets(handles, &batch).await
    };
    if let Err(error) = sync_result {
        control.disk_checkpoint_failed(&batch, sync_started.elapsed(), &error.to_string());
        return Err(error);
    }
    control.disk_checkpoint_sync_completed(&batch, sync_started.elapsed());
    let commit_started = Instant::now();
    control.wait_before_checkpoint_commit().await;
    let checkpoints = checkpoints.clone();
    let commit_result =
        tokio::task::spawn_blocking(move || checkpoints.pieces_durable(&piece_indices))
            .await
            .map_err(|error| DownloadError::StorageTask(error.to_string()))
            .and_then(|result| result.map_err(DownloadError::Checkpoint));
    if let Err(error) = commit_result {
        control.disk_checkpoint_failed(&batch, commit_started.elapsed(), &error.to_string());
        return Err(error);
    }
    control.disk_checkpoint_completed(&batch, commit_started.elapsed());
    drop(batch_permits);
    Ok(())
}

async fn sync_checkpoint_targets(
    handles: &CheckpointHandles,
    batch: &CheckpointBatch,
) -> Result<(), DownloadError> {
    let references = batch
        .targets
        .iter()
        .copied()
        .map(|target| {
            handles
                .get(&target)
                .and_then(|handle| handle.get())
                .cloned()
                .map_or_else(
                    || {
                        Err(DownloadError::StorageTask(format!(
                            "checkpoint target {target:?} has no sync handle"
                        )))
                    },
                    |reference| Ok((target, reference)),
                )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut targets = Vec::with_capacity(references.len());
    for (target, reference) in references {
        let file = reference
            .acquire()
            .await
            .map_err(DownloadError::SelectiveStorage)?;
        targets.push((target, file));
    }
    let mut targets = targets.into_iter();
    let mut running = JoinSet::new();
    let mut first_error = None;
    loop {
        while first_error.is_none() && running.len() < CHECKPOINT_SYNC_CONCURRENCY {
            let Some((target, file)) = targets.next() else {
                break;
            };
            running.spawn_blocking(move || (target, file.file().sync_data()));
        }
        let Some(result) = running.join_next().await else {
            break;
        };
        match result {
            Ok((_target, Ok(()))) => {}
            Ok((target, Err(error))) => {
                first_error.get_or_insert_with(|| {
                    DownloadError::StorageTask(format!(
                        "checkpoint target {target:?} sync failed: {error}"
                    ))
                });
            }
            Err(error) => {
                first_error.get_or_insert_with(|| DownloadError::StorageTask(error.to_string()));
            }
        }
    }
    first_error.map_or(Ok(()), Err)
}

async fn run_content_storage_task(
    mut storage: ContentStorage,
    mut commands: mpsc::Receiver<QueuedContentStorageCommand>,
    completions: mpsc::Sender<ContentStorageCompletion>,
    cancellation: CancellationToken,
    control: DownloadControl,
    queue_capacity: usize,
) -> Result<ContentStorage, DownloadError> {
    let mut ready_writes = VecDeque::new();
    let mut ready_hashes = VecDeque::new();
    let mut pending_completions = VecDeque::new();
    let mut running = JoinSet::new();
    let mut active_writes = 0_usize;
    let mut active_hashes = 0_usize;
    let mut commands_closed = false;
    let mut cancelled = false;
    let (write_concurrency, hash_concurrency) = control.storage_execution_limits();

    loop {
        if !cancelled {
            loop {
                match commands.try_recv() {
                    Ok(command) => {
                        queue_content_storage_command(command, &mut ready_writes, &mut ready_hashes)
                    }
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        commands_closed = true;
                        break;
                    }
                }
            }
            flush_ready_content_storage_completions(
                &completions,
                &mut pending_completions,
                &control,
                queue_capacity,
            )?;
        }

        while !cancelled && active_writes < write_concurrency && !ready_writes.is_empty() {
            let batch = collect_ready_content_write_batch(&mut ready_writes);
            let block_keys = batch
                .iter()
                .filter_map(|command| match &command.command {
                    ContentStorageCommand::Write { block, .. } => Some(*block),
                    ContentStorageCommand::Verify { .. } => None,
                })
                .collect::<Vec<_>>();
            let bytes = batch.iter().fold(0_usize, |total, command| {
                total.saturating_add(command.command.write_bytes().unwrap_or(0))
            });
            let enqueued_at = batch[0].enqueued_at;
            let started_at = Instant::now();
            control.storage_write_batch_started(enqueued_at, started_at, &block_keys, bytes);
            match prepare_content_storage_writes(&mut storage, batch).await {
                Ok(job) => {
                    active_writes += 1;
                    let job_control = control.clone();
                    running.spawn(async move {
                        job_control.wait_before_storage().await;
                        ContentStorageJobResult::Write {
                            started_at,
                            blocks: block_keys,
                            bytes,
                            completions: execute_content_write_job(job).await,
                        }
                    });
                }
                Err(failed) => {
                    control.storage_write_batch_completed(
                        started_at,
                        Instant::now(),
                        &block_keys,
                        bytes,
                    );
                    pending_completions.extend(failed);
                }
            }
        }

        while !cancelled && active_hashes < hash_concurrency && !ready_hashes.is_empty() {
            let command = ready_hashes
                .pop_front()
                .expect("nonempty hash-ready queue has a command");
            let ContentStorageCommand::Verify { piece, length, .. } = &command.command else {
                unreachable!("hash-ready queue contains only verify commands");
            };
            control.disk_piece_hashing(*piece, *length);
            control.emit(DownloadActivityEvent::PieceHashing {
                piece_index: *piece,
            });
            let started_at = Instant::now();
            control.storage_command_started(
                StorageCommandKind::Hash,
                command.enqueued_at,
                started_at,
            );
            match prepare_content_storage_hash(&storage, command.command) {
                Ok(job) => {
                    active_hashes += 1;
                    let job_control = control.clone();
                    running.spawn(async move {
                        job_control.wait_before_storage_hash().await;
                        ContentStorageJobResult::Hash {
                            started_at,
                            result: execute_content_hash_job(job).await,
                        }
                    });
                }
                Err(failed) => {
                    control.storage_command_completed(
                        StorageCommandKind::Hash,
                        started_at,
                        Instant::now(),
                    );
                    pending_completions.push_back(failed);
                }
            }
        }

        if cancelled {
            ready_writes.clear();
            ready_hashes.clear();
            pending_completions.clear();
            while let Some(joined) = running.join_next().await {
                complete_cancelled_content_storage_job(
                    joined.map_err(|error| DownloadError::StorageTask(error.to_string()))?,
                    &control,
                );
            }
            return Ok(storage);
        }

        if commands_closed
            && ready_writes.is_empty()
            && ready_hashes.is_empty()
            && running.is_empty()
            && pending_completions.is_empty()
        {
            return Ok(storage);
        }

        tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                cancelled = true;
                commands.close();
            }
            joined = running.join_next(), if !running.is_empty() => {
                let result = joined
                    .expect("nonempty storage job set has a completion")
                    .map_err(|error| DownloadError::StorageTask(error.to_string()))?;
                match result {
                    ContentStorageJobResult::Write {
                        started_at,
                        blocks,
                        bytes,
                        completions: completed,
                    } => {
                        active_writes = active_writes.checked_sub(1).ok_or_else(|| {
                            DownloadError::StorageTask("active write job underflow".to_owned())
                        })?;
                        control.storage_write_batch_completed(
                            started_at,
                            Instant::now(),
                            &blocks,
                            bytes,
                        );
                        pending_completions.extend(completed);
                    }
                    ContentStorageJobResult::Hash { started_at, result } => {
                        active_hashes = active_hashes.checked_sub(1).ok_or_else(|| {
                            DownloadError::StorageTask("active hash job underflow".to_owned())
                        })?;
                        control.storage_command_completed(
                            StorageCommandKind::Hash,
                            started_at,
                            Instant::now(),
                        );
                        pending_completions.push_back(finish_content_hash_job(
                            &mut storage,
                            result,
                            &control,
                        ));
                    }
                }
            }
            permit = completions.reserve(), if !pending_completions.is_empty() => {
                let permit = permit.map_err(|_| {
                    DownloadError::StorageTask("storage completion channel closed".to_owned())
                })?;
                let projected_depth = queue_capacity
                    .saturating_sub(completions.capacity())
                    .saturating_add(1)
                    .min(queue_capacity);
                control.observe_storage_completion_queue(projected_depth);
                permit.send(
                    pending_completions
                        .pop_front()
                        .expect("reserved completion has a pending value"),
                );
            }
            command = commands.recv(), if !commands_closed => {
                match command {
                    Some(command) => queue_content_storage_command(
                        command,
                        &mut ready_writes,
                        &mut ready_hashes,
                    ),
                    None => commands_closed = true,
                }
            }
        }
    }
}

fn queue_content_storage_command(
    command: QueuedContentStorageCommand,
    writes: &mut VecDeque<QueuedContentStorageCommand>,
    hashes: &mut VecDeque<QueuedContentStorageCommand>,
) {
    match command.command.kind() {
        StorageCommandKind::Write => writes.push_back(command),
        StorageCommandKind::Hash => hashes.push_back(command),
    }
}

fn flush_ready_content_storage_completions(
    completions: &mpsc::Sender<ContentStorageCompletion>,
    pending: &mut VecDeque<ContentStorageCompletion>,
    control: &DownloadControl,
    queue_capacity: usize,
) -> Result<(), DownloadError> {
    while let Some(completion) = pending.pop_front() {
        match completions.try_send(completion) {
            Ok(()) => {
                let depth = queue_capacity
                    .saturating_sub(completions.capacity())
                    .min(queue_capacity);
                control.observe_storage_completion_queue(depth);
            }
            Err(mpsc::error::TrySendError::Full(completion)) => {
                pending.push_front(completion);
                break;
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Err(DownloadError::StorageTask(
                    "storage completion channel closed".to_owned(),
                ));
            }
        }
    }
    Ok(())
}

fn collect_ready_content_write_batch(
    writes: &mut VecDeque<QueuedContentStorageCommand>,
) -> Vec<QueuedContentStorageCommand> {
    let first = writes
        .pop_front()
        .expect("nonempty write-ready queue has a command");
    let mut bytes = first.command.write_bytes().unwrap_or(0);
    let mut batch = Vec::with_capacity(CONTENT_STORAGE_WRITE_BATCH_BLOCKS);
    batch.push(first);
    while batch.len() < CONTENT_STORAGE_WRITE_BATCH_BLOCKS {
        let Some(next_bytes) = writes
            .front()
            .and_then(|command| command.command.write_bytes())
        else {
            break;
        };
        let Some(projected) = bytes.checked_add(next_bytes) else {
            break;
        };
        if projected > CONTENT_STORAGE_WRITE_BATCH_BYTES {
            break;
        }
        bytes = projected;
        batch.push(
            writes
                .pop_front()
                .expect("inspected write-ready command remains present"),
        );
    }
    batch
}

fn complete_cancelled_content_storage_job(
    result: ContentStorageJobResult,
    control: &DownloadControl,
) {
    match result {
        ContentStorageJobResult::Write {
            started_at,
            blocks,
            bytes,
            ..
        } => control.storage_write_batch_completed(started_at, Instant::now(), &blocks, bytes),
        ContentStorageJobResult::Hash { started_at, .. } => {
            control.storage_command_completed(StorageCommandKind::Hash, started_at, Instant::now())
        }
    }
}

#[cfg(test)]
fn collect_content_write_batch(
    first: QueuedContentStorageCommand,
    commands: &mut mpsc::Receiver<QueuedContentStorageCommand>,
    deferred: &mut Option<QueuedContentStorageCommand>,
) -> Vec<QueuedContentStorageCommand> {
    debug_assert!(first.command.write_bytes().is_some());
    let mut bytes = first.command.write_bytes().unwrap_or(0);
    let mut batch = Vec::with_capacity(CONTENT_STORAGE_WRITE_BATCH_BLOCKS);
    batch.push(first);

    while batch.len() < CONTENT_STORAGE_WRITE_BATCH_BLOCKS {
        let next = match commands.try_recv() {
            Ok(command) => command,
            Err(mpsc::error::TryRecvError::Empty | mpsc::error::TryRecvError::Disconnected) => {
                break;
            }
        };
        let Some(next_bytes) = next.command.write_bytes() else {
            *deferred = Some(next);
            break;
        };
        let Some(projected) = bytes.checked_add(next_bytes) else {
            *deferred = Some(next);
            break;
        };
        if projected > CONTENT_STORAGE_WRITE_BATCH_BYTES {
            *deferred = Some(next);
            break;
        }
        bytes = projected;
        batch.push(next);
    }
    batch
}

#[cfg(test)]
async fn execute_content_storage_writes(
    storage: &mut ContentStorage,
    commands: Vec<QueuedContentStorageCommand>,
    control: &DownloadControl,
) -> Vec<ContentStorageCompletion> {
    control.wait_before_storage().await;
    match prepare_content_storage_writes(storage, commands).await {
        Ok(job) => execute_content_write_job(job).await,
        Err(completions) => completions,
    }
}

async fn prepare_content_storage_writes(
    storage: &mut ContentStorage,
    commands: Vec<QueuedContentStorageCommand>,
) -> Result<ContentWriteJob, Vec<ContentStorageCompletion>> {
    let mut prepared = Vec::with_capacity(commands.len());
    for command in commands {
        let ContentStorageCommand::Write {
            block,
            generation,
            offset,
            bytes,
        } = command.command
        else {
            unreachable!("write batches contain only write commands");
        };
        let stats = storage
            .0
            .write_stats(block.piece, block.begin, bytes.len())
            .map(|stats| ContentWriteStats {
                selected_bytes: stats.wanted_bytes,
                part_bytes: stats.skipped_bytes,
            })
            .map_err(DownloadError::SelectiveStorage);
        let stats = match stats {
            Ok(stats) => stats,
            Err(error) => {
                return Err(failed_content_write_batch(
                    block,
                    generation,
                    error,
                    prepared.into_iter(),
                ));
            }
        };
        prepared.push(PreparedContentWrite {
            block,
            generation,
            offset,
            bytes,
            stats,
        });
    }

    let writes = match coalesce_content_writes(prepared) {
        Ok(writes) => writes,
        Err((block, generation, error)) => {
            return Err(vec![ContentStorageCompletion::Write {
                block,
                generation,
                result: Err(error),
            }]);
        }
    };
    let mut physical = Vec::with_capacity(writes.len());
    let mut writes = writes.into_iter();
    while let Some(write) = writes.next() {
        let operation = storage
            .0
            .prepare_write(write.piece, write.begin, write.bytes)
            .await
            .map(ContentWriteOperation)
            .map_err(DownloadError::SelectiveStorage);
        match operation {
            Ok(operation) => physical.push(PreparedPhysicalContentWrite {
                operation,
                members: write.members,
            }),
            Err(error) => {
                let mut members = write.members.into_iter();
                let first = members
                    .next()
                    .expect("coalesced write retains at least one logical member");
                let mut completions = vec![ContentStorageCompletion::Write {
                    block: first.block,
                    generation: first.generation,
                    result: Err(error),
                }];
                completions.extend(members.map(failed_prepared_content_write));
                completions.extend(
                    physical
                        .into_iter()
                        .flat_map(|write| write.members)
                        .map(failed_prepared_content_write),
                );
                completions.extend(
                    writes
                        .flat_map(|write| write.members)
                        .map(failed_prepared_content_write),
                );
                return Err(completions);
            }
        }
    }
    Ok(ContentWriteJob { writes: physical })
}

async fn execute_content_write_job(job: ContentWriteJob) -> Vec<ContentStorageCompletion> {
    let mut completed = Vec::new();
    for write in job.writes {
        let result = write.operation.execute().await;
        if let Err(error) = result {
            let mut members = write.members.into_iter();
            let first = members
                .next()
                .expect("coalesced write retains at least one logical member");
            completed.push(ContentStorageCompletion::Write {
                block: first.block,
                generation: first.generation,
                result: Err(error),
            });
            completed.extend(members.map(|member| ContentStorageCompletion::Write {
                block: member.block,
                generation: member.generation,
                result: Err(DownloadError::StorageTask(
                    "coalesced physical write failed".to_owned(),
                )),
            }));
            return completed;
        }
        completed.extend(
            write
                .members
                .into_iter()
                .map(|member| ContentStorageCompletion::Write {
                    block: member.block,
                    generation: member.generation,
                    result: Ok(member.stats),
                }),
        );
    }
    completed
}

fn failed_prepared_content_write(member: ContentWriteMember) -> ContentStorageCompletion {
    ContentStorageCompletion::Write {
        block: member.block,
        generation: member.generation,
        result: Err(DownloadError::StorageTask(
            "coalesced write batch preparation failed".to_owned(),
        )),
    }
}

fn failed_content_write_batch(
    failed_block: BlockKey,
    failed_generation: PieceGeneration,
    error: DownloadError,
    prepared: impl Iterator<Item = PreparedContentWrite>,
) -> Vec<ContentStorageCompletion> {
    let mut completions = vec![ContentStorageCompletion::Write {
        block: failed_block,
        generation: failed_generation,
        result: Err(error),
    }];
    completions.extend(prepared.map(|write| ContentStorageCompletion::Write {
        block: write.block,
        generation: write.generation,
        result: Err(DownloadError::StorageTask(
            "coalesced write batch validation failed".to_owned(),
        )),
    }));
    completions
}

fn coalesce_content_writes(
    mut writes: Vec<PreparedContentWrite>,
) -> Result<Vec<CoalescedContentWrite>, (BlockKey, PieceGeneration, DownloadError)> {
    writes.sort_unstable_by_key(|write| (write.block.piece, write.block.begin));
    let mut coalesced: Vec<CoalescedContentWrite> = Vec::with_capacity(writes.len());
    for write in writes {
        if let Some(previous) = coalesced.last_mut()
            && previous.piece == write.block.piece
        {
            let previous_piece_end = previous
                .begin
                .checked_add(u32::try_from(previous.bytes.len()).unwrap_or(u32::MAX));
            if previous_piece_end.is_some_and(|end| write.block.begin < end) {
                return Err((
                    write.block,
                    write.generation,
                    DownloadError::StorageTask(
                        "overlapping logical writes entered one storage batch".to_owned(),
                    ),
                ));
            }
            let previous_file_end = previous.offset.checked_add(previous.bytes.len() as u64);
            if previous_piece_end == Some(write.block.begin)
                && previous_file_end == Some(write.offset)
            {
                previous.bytes.extend_from_slice(&write.bytes);
                previous.members.push(ContentWriteMember {
                    block: write.block,
                    generation: write.generation,
                    stats: write.stats,
                });
                continue;
            }
        }
        coalesced.push(CoalescedContentWrite {
            piece: write.block.piece,
            begin: write.block.begin,
            offset: write.offset,
            bytes: write.bytes,
            members: vec![ContentWriteMember {
                block: write.block,
                generation: write.generation,
                stats: write.stats,
            }],
        });
    }
    Ok(coalesced)
}

#[cfg(test)]
async fn execute_content_storage_verification(
    storage: &mut ContentStorage,
    command: ContentStorageCommand,
    control: &DownloadControl,
) -> ContentStorageCompletion {
    let job = match prepare_content_storage_hash(storage, command) {
        Ok(job) => job,
        Err(completion) => return completion,
    };
    control.wait_before_storage_hash().await;
    let result = execute_content_hash_job(job).await;
    finish_content_hash_job(storage, result, control)
}

fn prepare_content_storage_hash(
    storage: &ContentStorage,
    command: ContentStorageCommand,
) -> Result<ContentHashJob, ContentStorageCompletion> {
    let ContentStorageCommand::Verify {
        piece,
        generation,
        length,
        expected,
        durable,
    } = command
    else {
        unreachable!("write commands execute through the bounded batch path");
    };
    let durability_targets = if durable {
        storage
            .0
            .durability_targets(piece)
            .map_err(DownloadError::SelectiveStorage)
    } else {
        Ok(Vec::new())
    };
    let prepared = durability_targets.and_then(|durability_targets| {
        storage
            .0
            .prepare_hash(piece)
            .map(|operation| (ContentHashOperation(operation), durability_targets))
            .map_err(DownloadError::SelectiveStorage)
    });
    match prepared {
        Ok((operation, durability_targets)) => Ok(ContentHashJob {
            piece,
            generation,
            length,
            expected,
            durable,
            durability_targets,
            operation,
        }),
        Err(error) => Err(ContentStorageCompletion::Verify {
            piece,
            generation,
            length,
            result: Err(error),
        }),
    }
}

async fn execute_content_hash_job(job: ContentHashJob) -> ContentHashJobResult {
    let result = job
        .operation
        .0
        .execute()
        .await
        .map_err(DownloadError::SelectiveStorage);
    ContentHashJobResult {
        piece: job.piece,
        generation: job.generation,
        length: job.length,
        expected: job.expected,
        durable: job.durable,
        durability_targets: job.durability_targets,
        result,
    }
}

fn finish_content_hash_job(
    storage: &mut ContentStorage,
    result: ContentHashJobResult,
    control: &DownloadControl,
) -> ContentStorageCompletion {
    let verification = result.result.and_then(|actual| {
        if actual == result.expected {
            let piece_index = usize::try_from(result.piece)
                .map_err(|_| DownloadError::Layout(LayoutError::ArithmeticOverflow))?;
            storage
                .0
                .record_verified(piece_index)
                .map_err(DownloadError::SelectiveStorage)?;
        }
        Ok(ContentVerification {
            actual,
            durability_targets: if actual == result.expected {
                result.durability_targets
            } else {
                Vec::new()
            },
        })
    });
    if verification
        .as_ref()
        .is_ok_and(|verification| verification.actual == result.expected)
    {
        control.disk_piece_hash_verified(result.piece, result.length, result.durable);
    }
    ContentStorageCompletion::Verify {
        piece: result.piece,
        generation: result.generation,
        length: result.length,
        result: verification,
    }
}

struct ContentSwarmDownload<'a> {
    state: SwarmState,
    storage_pipeline: Option<ContentStoragePipeline>,
    completed_storage: Option<ContentStorage>,
    metainfo: &'a Metainfo,
    layout: &'a TorrentLayout,
    resume: Option<&'a ResumeContext>,
    control: &'a DownloadControl,
    total_blocks: usize,
    total_bytes: usize,
    selected_written_bytes: usize,
    part_written_bytes: usize,
    last_piece: Option<VerifiedPiece>,
    contributor_attempts: BTreeMap<ConnectionId, DialAttempt>,
}

struct ContentDownloadContext<'a> {
    metainfo: &'a Metainfo,
    layout: &'a TorrentLayout,
    resume: Option<&'a ResumeContext>,
    control: &'a DownloadControl,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ContentMessageDisposition {
    Continue,
    ClosePeer(PeerFailure),
    PieceVerified(Vec<ConnectionId>),
    PieceHashFailed(PieceHashFailure),
}

impl<'a> ContentSwarmDownload<'a> {
    async fn new(
        config: SwarmConfig,
        max_buffered_payload_bytes: usize,
        plans: Vec<(u32, Vec<rstorrent_protocol::storage_layout::RequestRange>)>,
        storage: ContentStorage,
        context: ContentDownloadContext<'a>,
    ) -> Result<Self, DownloadError> {
        let ContentDownloadContext {
            metainfo,
            layout,
            resume,
            control,
        } = context;
        let mut total_blocks = 0;
        let mut total_bytes = 0;
        let mut swarm_plans = Vec::with_capacity(plans.len());
        for (piece, ranges) in plans {
            total_blocks += ranges.len();
            total_bytes += ranges
                .iter()
                .map(|range| range.length as usize)
                .sum::<usize>();
            let ranges = ranges
                .into_iter()
                .map(|range| (range.begin, range.length))
                .collect::<Vec<_>>();
            swarm_plans.push(PiecePlan::new(piece, &ranges).map_err(DownloadError::Swarm)?);
        }
        let state = SwarmState::new(config, layout.piece_count(), swarm_plans)
            .map_err(DownloadError::Swarm)?;
        let checkpoints = resume.map(|resume| resume.checkpoints.clone());
        Ok(Self {
            state,
            storage_pipeline: Some(
                ContentStoragePipeline::start(
                    storage,
                    control,
                    max_buffered_payload_bytes,
                    checkpoints,
                )
                .await?,
            ),
            completed_storage: None,
            metainfo,
            layout,
            resume,
            control,
            total_blocks,
            total_bytes,
            selected_written_bytes: 0,
            part_written_bytes: 0,
            last_piece: None,
            contributor_attempts: BTreeMap::new(),
        })
    }

    fn is_complete(&self) -> bool {
        self.state.is_complete()
    }

    async fn handle_message(
        &mut self,
        sockets: &PeerSocketSet,
        connection: ConnectionId,
        message: PeerMessage,
        now: Duration,
    ) -> Result<ContentMessageDisposition, DownloadError> {
        match message {
            PeerMessage::Choke => {
                self.state
                    .set_choking(connection, true)
                    .map_err(DownloadError::Swarm)?;
            }
            PeerMessage::Unchoke => {
                self.state
                    .set_choking(connection, false)
                    .map_err(DownloadError::Swarm)?;
            }
            PeerMessage::Have(piece) => {
                if self.state.peer_has(connection, piece).is_err() {
                    return Ok(ContentMessageDisposition::ClosePeer(PeerFailure::Protocol));
                }
            }
            PeerMessage::Bitfield(bitfield) => {
                let Some(availability) =
                    decode_validated_availability(&bitfield, self.layout.piece_count())
                else {
                    return Ok(ContentMessageDisposition::ClosePeer(PeerFailure::Protocol));
                };
                self.state
                    .set_bitfield(connection, availability)
                    .map_err(DownloadError::Swarm)?;
            }
            PeerMessage::Piece {
                index,
                begin,
                block,
            } => {
                let Ok(length) = u32::try_from(block.len()) else {
                    self.control
                        .record_bytes(ByteMetric::PeerUnclassifiedReceived, block.len());
                    return Ok(ContentMessageDisposition::ClosePeer(PeerFailure::Protocol));
                };
                let Ok(key) = BlockKey::new(index, begin, length) else {
                    self.control
                        .record_bytes(ByteMetric::PeerUnclassifiedReceived, block.len());
                    return Ok(ContentMessageDisposition::ClosePeer(PeerFailure::Protocol));
                };
                let disposition = match self.state.receive_block(connection, key, now) {
                    Ok(disposition) => disposition,
                    Err(error) => {
                        self.control
                            .record_bytes(ByteMetric::PeerUnclassifiedReceived, block.len());
                        return Err(DownloadError::Swarm(error));
                    }
                };
                match disposition {
                    ReceiveDisposition::Accept { cancellations, .. } => {
                        let source_attempt = sockets.attempt(connection).ok_or({
                            DownloadError::Swarm(SwarmError::Invariant(
                                "accepted block source socket is missing",
                            ))
                        })?;
                        for cancellation in cancellations {
                            let _ = sockets
                                .send(
                                    cancellation.connection,
                                    PeerMessage::Cancel(cancellation.block.request()),
                                )
                                .await;
                        }
                        let piece_length = self
                            .layout
                            .piece_length_at(index)
                            .map_err(DownloadError::Layout)?;
                        self.control.disk_block_received(key, piece_length);
                        self.control
                            .record_bytes(ByteMetric::PayloadReceived, block.len());
                        self.control.emit(DownloadActivityEvent::BlockReceived {
                            piece_index: index,
                            begin,
                            length,
                        });
                        self.contributor_attempts.insert(connection, source_attempt);
                        let offset = torrent_payload_offset(self.layout, index, begin)?;
                        let generation = self
                            .state
                            .piece_generation(index)
                            .map_err(DownloadError::Swarm)?;
                        self.storage_pipeline_mut()?
                            .enqueue(ContentStorageCommand::Write {
                                block: key,
                                generation,
                                offset,
                                bytes: block,
                            })?;
                    }
                    ReceiveDisposition::Redundant | ReceiveDisposition::Unsolicited => {
                        self.control
                            .record_bytes(ByteMetric::PayloadRedundant, block.len());
                        self.control
                            .record_bytes(ByteMetric::PeerUnclassifiedReceived, block.len());
                    }
                }
            }
            PeerMessage::KeepAlive
            | PeerMessage::Interested
            | PeerMessage::NotInterested
            | PeerMessage::Request(_)
            | PeerMessage::Cancel(_)
            | PeerMessage::Extended { .. } => {}
        }
        Ok(ContentMessageDisposition::Continue)
    }

    fn storage_pipeline_mut(&mut self) -> Result<&mut ContentStoragePipeline, DownloadError> {
        self.storage_pipeline.as_mut().ok_or_else(|| {
            DownloadError::StorageTask("content storage owner is not running".to_owned())
        })
    }

    fn flush_pending_storage(&mut self) -> Result<bool, DownloadError> {
        self.storage_pipeline_mut()?.flush_pending()
    }

    fn storage_is_backpressured(&self) -> bool {
        self.storage_pipeline
            .as_ref()
            .is_some_and(ContentStoragePipeline::is_backpressured)
    }

    async fn handle_storage_completion(
        &mut self,
        completion: ContentStorageCompletion,
        now: Duration,
    ) -> Result<ContentMessageDisposition, DownloadError> {
        self.storage_pipeline_mut()?
            .completion_received(&completion);
        let disposition = match completion {
            ContentStorageCompletion::Write {
                block,
                generation,
                result,
            } => {
                if self
                    .state
                    .piece_generation(block.piece)
                    .map_err(DownloadError::Swarm)?
                    != generation
                {
                    return Ok(ContentMessageDisposition::Continue);
                }
                let stats = match result {
                    Ok(stats) => stats,
                    Err(error) => {
                        self.control.disk_storage_error(&error.to_string());
                        self.state
                            .finish_write_for_generation(block, generation, false, now)
                            .map_err(DownloadError::Swarm)?;
                        self.prune_contributor_attempts();
                        return Err(error);
                    }
                };
                self.state
                    .finish_write_for_generation(block, generation, true, now)
                    .map_err(DownloadError::Swarm)?;
                self.selected_written_bytes = self
                    .selected_written_bytes
                    .saturating_add(stats.selected_bytes);
                self.part_written_bytes = self.part_written_bytes.saturating_add(stats.part_bytes);
                let piece_length = self
                    .layout
                    .piece_length_at(block.piece)
                    .map_err(DownloadError::Layout)?;
                self.control.disk_block_stored(block, piece_length);
                self.control
                    .record_bytes(ByteMetric::StagedWrite, block.length as usize);
                self.control.emit(DownloadActivityEvent::BlockStored {
                    piece_index: block.piece,
                    begin: block.begin,
                    length: block.length,
                });
                if self
                    .state
                    .piece_ready_for_generation(block.piece, generation)
                    .map_err(DownloadError::Swarm)?
                {
                    let piece_index = usize::try_from(block.piece)
                        .map_err(|_| DownloadError::Layout(LayoutError::ArithmeticOverflow))?;
                    let length = self
                        .layout
                        .piece_length_at(block.piece)
                        .map_err(DownloadError::Layout)?;
                    let expected = self.metainfo.piece_hashes[piece_index];
                    let durable = self.resume.is_some();
                    self.state
                        .begin_piece_hash(block.piece, generation)
                        .map_err(DownloadError::Swarm)?;
                    self.storage_pipeline_mut()?
                        .enqueue(ContentStorageCommand::Verify {
                            piece: block.piece,
                            generation,
                            length,
                            expected,
                            durable,
                        })?;
                }
                ContentMessageDisposition::Continue
            }
            ContentStorageCompletion::Verify {
                piece,
                generation,
                length,
                result,
            } => {
                if self
                    .state
                    .piece_generation(piece)
                    .map_err(DownloadError::Swarm)?
                    != generation
                {
                    return Ok(ContentMessageDisposition::Continue);
                }
                let verification = result?;
                self.control
                    .record_bytes(ByteMetric::LogicalHashRead, length as usize);
                let actual = verification.actual;
                let piece_index = usize::try_from(piece)
                    .map_err(|_| DownloadError::Layout(LayoutError::ArithmeticOverflow))?;
                let expected = self.metainfo.piece_hashes[piece_index];
                self.state
                    .finish_piece_hash(piece, generation, actual == expected)
                    .map_err(DownloadError::Swarm)?;
                if actual != expected {
                    let failure = self
                        .state
                        .mark_piece_hash_failed_for_generation(piece, generation)
                        .map_err(DownloadError::Swarm)?;
                    self.control.emit(DownloadActivityEvent::PieceHashFailed {
                        piece_index: piece,
                        contributor_count: failure.contributors.len(),
                        failed_bytes: failure.failed_bytes,
                    });
                    self.control
                        .record_bytes(ByteMetric::PayloadHashFailed, failure.failed_bytes);
                    self.control
                        .disk_piece_failed(piece, length, "piece hash failed; retrying");
                    ContentMessageDisposition::PieceHashFailed(failure)
                } else {
                    if self.resume.is_some() {
                        self.storage_pipeline_mut()?
                            .checkpoint
                            .as_ref()
                            .ok_or_else(|| {
                                DownloadError::StorageTask(
                                    "resumable storage has no checkpoint owner".to_owned(),
                                )
                            })?
                            .enqueue(piece_index, length, verification.durability_targets)
                            .await?;
                    }
                    let contributors = self
                        .state
                        .mark_piece_verified_for_generation(piece, generation)
                        .map_err(DownloadError::Swarm)?;
                    self.last_piece = Some(VerifiedPiece {
                        index: piece,
                        hash: actual,
                        length,
                    });
                    self.control
                        .record_bytes(ByteMetric::PayloadVerified, length as usize);
                    self.control
                        .emit(DownloadActivityEvent::PieceVerified { piece_index: piece });
                    ContentMessageDisposition::PieceVerified(contributors)
                }
            }
        };
        Ok(disposition)
    }

    async fn stop_storage(&mut self, cancel: bool) -> Result<(), DownloadError> {
        let pipeline = self.storage_pipeline.take().ok_or_else(|| {
            DownloadError::StorageTask("content storage owner is not running".to_owned())
        })?;
        self.completed_storage = Some(pipeline.shutdown(cancel).await?);
        Ok(())
    }

    fn take_storage(&mut self) -> Result<ContentStorage, DownloadError> {
        self.completed_storage.take().ok_or_else(|| {
            DownloadError::StorageTask("content storage owner did not return storage".to_owned())
        })
    }

    fn contributor_attempt(&self, connection: ConnectionId) -> Option<DialAttempt> {
        self.contributor_attempts.get(&connection).copied()
    }

    fn prune_contributor_attempts(&mut self) {
        let retained = self.state.unverified_contributors();
        self.contributor_attempts
            .retain(|connection, _| retained.contains(connection));
    }
}

fn record_verified_piece_contributors(
    peers: &mut TorrentPeerCoordinator,
    download: &mut ContentSwarmDownload<'_>,
    contributors: &[ConnectionId],
) -> Result<(), DownloadError> {
    for &connection in contributors {
        let attempt = download.contributor_attempt(connection).ok_or({
            DownloadError::Swarm(SwarmError::Invariant(
                "verified piece contributor attempt is missing",
            ))
        })?;
        match peers.registry.record_piece_passed(attempt) {
            Ok(())
            | Err(PeerRegistryError::StaleAttempt(_))
            | Err(PeerRegistryError::UnknownRecord(_)) => {}
            Err(error) => return Err(DownloadError::PeerRegistry(error)),
        }
    }
    download.prune_contributor_attempts();
    Ok(())
}

async fn record_failed_piece_contributors(
    peers: &mut TorrentPeerCoordinator,
    sockets: &mut PeerSocketSet,
    download: &mut ContentSwarmDownload<'_>,
    failure: &PieceHashFailure,
) -> Result<(), DownloadError> {
    let known_bad = failure.contributors.len() == 1;
    let mut banned = Vec::new();
    for &connection in &failure.contributors {
        let attempt = download.contributor_attempt(connection).ok_or({
            DownloadError::Swarm(SwarmError::Invariant(
                "failed piece contributor attempt is missing",
            ))
        })?;
        match peers.registry.record_piece_failed(attempt, known_bad) {
            Ok(PeerIntegrityAction::Retain) => {}
            Ok(PeerIntegrityAction::Ban) => banned.push((connection, attempt)),
            Err(PeerRegistryError::StaleAttempt(_)) | Err(PeerRegistryError::UnknownRecord(_)) => {}
            Err(error) => return Err(DownloadError::PeerRegistry(error)),
        }
    }
    for (connection, attempt) in banned {
        if sockets.contains(connection) {
            close_content_connection(peers, sockets, &mut download.state, connection, None).await?;
        }
        peers
            .registry
            .ban(attempt.record_id())
            .map_err(DownloadError::PeerRegistry)?;
    }
    download.prune_contributor_attempts();
    Ok(())
}

fn torrent_payload_offset(
    layout: &TorrentLayout,
    piece: u32,
    begin: u32,
) -> Result<u64, DownloadError> {
    u64::from(piece)
        .checked_mul(u64::from(layout.piece_length()))
        .and_then(|offset| offset.checked_add(u64::from(begin)))
        .ok_or(DownloadError::Layout(LayoutError::ArithmeticOverflow))
}

fn decode_validated_availability(bitfield: &[u8], piece_count: usize) -> Option<Vec<bool>> {
    if bitfield.len() != piece_count.div_ceil(8) {
        return None;
    }
    let remainder = piece_count % 8;
    if remainder != 0 {
        let unused_mask = (1_u8 << (8 - remainder)) - 1;
        if bitfield.last().is_some_and(|byte| byte & unused_mask != 0) {
            return None;
        }
    }
    let mut availability = vec![false; piece_count];
    decode_availability(bitfield, &mut availability);
    Some(availability)
}

fn pending_dial_id(attempt: DialAttempt) -> PendingDialId {
    PendingDialId::new(attempt.id().get()).expect("dial attempt identifiers are nonzero")
}

fn content_dial_slot_available(
    established: usize,
    pending: usize,
    config: SwarmConfig,
    replacement_available: bool,
) -> bool {
    if pending >= config.max_pending_dials {
        return false;
    }
    established < config.max_established_connections || (pending == 0 && replacement_available)
}

fn fill_content_dials(
    peers: &mut TorrentPeerCoordinator,
    sockets: &mut PeerSocketSet,
    state: &mut SwarmState,
    info_hash: [u8; 20],
) -> Result<usize, DownloadError> {
    let mut started = 0;
    while content_dial_slot_available(
        sockets.established_len(),
        sockets.pending_len(),
        state.config(),
        state.replacement_candidate(peers.elapsed()).is_some(),
    ) {
        let context = PeerSelectionContext {
            now: peers.elapsed(),
        };
        let Some(candidate) = peers.selector.select(&peers.registry, context) else {
            break;
        };
        peers.control.emit(DownloadActivityEvent::PeerDialStarted {
            peer: candidate.endpoint().to_string(),
        });
        let attempt = peers.begin_dial(candidate, PeerConnectionRole::Content)?;
        if let Err(error) = state.begin_dial(pending_dial_id(attempt)) {
            peers.dial_cancelled(attempt)?;
            return Err(DownloadError::Swarm(error));
        }
        if let Err(error) = sockets.begin_dial(
            attempt,
            info_hash,
            false,
            peers.network,
            peers.control.byte_metric_sink(),
        ) {
            state
                .finish_dial(pending_dial_id(attempt))
                .map_err(DownloadError::Swarm)?;
            peers.dial_cancelled(attempt)?;
            return Err(download_peer_set_error(error));
        }
        started += 1;
    }
    Ok(started)
}

async fn close_content_connection(
    peers: &mut TorrentPeerCoordinator,
    sockets: &mut PeerSocketSet,
    state: &mut SwarmState,
    connection: ConnectionId,
    failure: Option<PeerFailure>,
) -> Result<(), DownloadError> {
    if !sockets.contains(connection) {
        return Ok(());
    }
    let attempt =
        sockets
            .attempt(connection)
            .ok_or(DownloadError::Swarm(SwarmError::Invariant(
                "active peer socket has no dial attempt",
            )))?;
    peers.begin_disconnect(attempt, failure)?;
    let attempt = sockets
        .remove_connection(connection)
        .await
        .map_err(download_peer_set_error)?;
    state
        .remove_connection(connection, ConnectionRemoval::Disconnected)
        .map_err(DownloadError::Swarm)?;
    peers.connection_closed(attempt, failure)
}

async fn replace_content_connection(
    peers: &mut TorrentPeerCoordinator,
    sockets: &mut PeerSocketSet,
    state: &mut SwarmState,
    connection: ConnectionId,
) -> Result<(), DownloadError> {
    let attempt =
        sockets
            .attempt(connection)
            .ok_or(DownloadError::Swarm(SwarmError::Invariant(
                "replacement peer socket has no dial attempt",
            )))?;
    peers.begin_disconnect(attempt, None)?;
    let attempt = sockets
        .remove_connection(connection)
        .await
        .map_err(download_peer_set_error)?;
    state
        .remove_connection(connection, ConnectionRemoval::Replaced)
        .map_err(DownloadError::Swarm)?;
    peers.connection_closed(attempt, None)
}

async fn cleanup_content_connections(
    peers: &mut TorrentPeerCoordinator,
    sockets: PeerSocketSet,
    state: &mut SwarmState,
    failure: Option<PeerFailure>,
) -> Result<(), DownloadError> {
    let active = sockets.connection_attempts();
    let pending_before_shutdown = sockets.pending_attempts();
    let mut first_error = None;
    for attempt in active.iter().chain(&pending_before_shutdown).copied() {
        if let Err(error) = peers.begin_disconnect(attempt, failure)
            && first_error.is_none()
        {
            first_error = Some(error);
        }
    }
    let pending = match sockets.shutdown().await {
        Ok(pending) => pending,
        Err(error) => {
            if first_error.is_none() {
                first_error = Some(download_peer_set_error(error));
            }
            pending_before_shutdown
        }
    };
    for attempt in active {
        if let Err(error) = peers.connection_closed(attempt, failure)
            && first_error.is_none()
        {
            first_error = Some(error);
        }
    }
    for attempt in pending {
        if let Err(error) = peers.dial_cancelled(attempt)
            && first_error.is_none()
        {
            first_error = Some(error);
        }
    }
    if let Err(error) = state.cancel_all().map_err(DownloadError::Swarm)
        && first_error.is_none()
    {
        first_error = Some(error);
    }
    peers.control.clear_buffered_payload();
    peers.control.clear_outstanding_requests();
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

enum ContentSupervisorEvent {
    Peer(PeerSetEvent),
    Discovery(Option<ContentDiscoveryEvent>),
    Storage(ContentStorageCompletion),
    Deadline,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ContentSupervisorOwner {
    Storage,
    Peer,
    Discovery,
}

impl ContentSupervisorOwner {
    const fn next(self) -> Self {
        match self {
            Self::Storage => Self::Peer,
            Self::Peer => Self::Discovery,
            Self::Discovery => Self::Storage,
        }
    }
}

impl ContentSupervisorEvent {
    const fn owner(&self) -> Option<ContentSupervisorOwner> {
        match self {
            Self::Peer(_) => Some(ContentSupervisorOwner::Peer),
            Self::Discovery(_) => Some(ContentSupervisorOwner::Discovery),
            Self::Storage(_) => Some(ContentSupervisorOwner::Storage),
            Self::Deadline => None,
        }
    }
}

async fn next_content_supervisor_event(
    sockets: &mut PeerSocketSet,
    discovery: &mut ContentDiscovery,
    storage: &mut ContentStoragePipeline,
    storage_backpressured: bool,
    until_expiry: Option<Duration>,
    cancellation: &CancellationToken,
    priority: ContentSupervisorOwner,
) -> Result<ContentSupervisorEvent, DownloadError> {
    if until_expiry.is_some_and(|wait| wait.is_zero()) {
        return Ok(ContentSupervisorEvent::Deadline);
    }
    if storage_backpressured {
        return match priority {
            ContentSupervisorOwner::Storage => tokio::select! {
                biased;
                _ = cancellation.cancelled() => Err(DownloadError::Cancelled),
                completion = storage.next_completion() => {
                    completion.map(ContentSupervisorEvent::Storage)
                }
                event = discovery.next_event(), if discovery.is_active() => {
                    Ok(ContentSupervisorEvent::Discovery(event))
                }
            },
            ContentSupervisorOwner::Peer | ContentSupervisorOwner::Discovery => tokio::select! {
                biased;
                _ = cancellation.cancelled() => Err(DownloadError::Cancelled),
                event = discovery.next_event(), if discovery.is_active() => {
                    Ok(ContentSupervisorEvent::Discovery(event))
                }
                completion = storage.next_completion() => {
                    completion.map(ContentSupervisorEvent::Storage)
                }
            },
        };
    }
    let wait = until_expiry.unwrap_or(Duration::ZERO);
    match priority {
        ContentSupervisorOwner::Storage => tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(DownloadError::Cancelled),
            completion = storage.next_completion() => {
                completion.map(ContentSupervisorEvent::Storage)
            }
            event = sockets.next_event() => event
                .map(ContentSupervisorEvent::Peer)
                .map_err(download_peer_set_error),
            event = discovery.next_event(), if discovery.is_active() => {
                Ok(ContentSupervisorEvent::Discovery(event))
            }
            _ = tokio::time::sleep(wait), if until_expiry.is_some() => {
                Ok(ContentSupervisorEvent::Deadline)
            }
        },
        ContentSupervisorOwner::Peer => tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(DownloadError::Cancelled),
            event = sockets.next_event() => event
                .map(ContentSupervisorEvent::Peer)
                .map_err(download_peer_set_error),
            event = discovery.next_event(), if discovery.is_active() => {
                Ok(ContentSupervisorEvent::Discovery(event))
            }
            completion = storage.next_completion() => {
                completion.map(ContentSupervisorEvent::Storage)
            }
            _ = tokio::time::sleep(wait), if until_expiry.is_some() => {
                Ok(ContentSupervisorEvent::Deadline)
            }
        },
        ContentSupervisorOwner::Discovery => tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(DownloadError::Cancelled),
            event = discovery.next_event(), if discovery.is_active() => {
                Ok(ContentSupervisorEvent::Discovery(event))
            }
            completion = storage.next_completion() => {
                completion.map(ContentSupervisorEvent::Storage)
            }
            event = sockets.next_event() => event
                .map(ContentSupervisorEvent::Peer)
                .map_err(download_peer_set_error),
            _ = tokio::time::sleep(wait), if until_expiry.is_some() => {
                Ok(ContentSupervisorEvent::Deadline)
            }
        },
    }
}

async fn run_selective_swarm_loop(
    peers: &mut TorrentPeerCoordinator,
    sockets: &mut PeerSocketSet,
    discovery: &mut ContentDiscovery,
    download: &mut ContentSwarmDownload<'_>,
) -> Result<(), DownloadError> {
    let mut next_owner = ContentSupervisorOwner::Storage;
    let mut storage_pressure_started = None;
    let mut next_maintenance_at = Duration::ZERO;
    if let Some(connection) = peers.connection.take() {
        let attempt = connection.attempt();
        peers.handoff_to_content(attempt)?;
        let id = sockets
            .add_connection(connection)
            .map_err(download_peer_set_error)?;
        download
            .state
            .add_connection(id, peers.elapsed())
            .map_err(DownloadError::Swarm)?;
        if sockets.send(id, PeerMessage::Interested).await.is_err() {
            close_content_connection(
                peers,
                sockets,
                &mut download.state,
                id,
                Some(PeerFailure::RemoteClosed),
            )
            .await?;
        } else {
            debug_assert_eq!(id, connection_id(attempt));
        }
    }

    loop {
        let now = peers.elapsed();
        let storage_ready = download.flush_pending_storage()?;
        let storage_backpressured = download.storage_is_backpressured();
        match (storage_pressure_started, storage_backpressured) {
            (None, true) => storage_pressure_started = Some(now),
            (Some(started), false) => {
                download
                    .state
                    .defer_peer_deadlines(now.saturating_sub(started));
                storage_pressure_started = None;
            }
            _ => {}
        }
        peers.publish_peer_registry(false);
        if now >= next_maintenance_at {
            if !storage_backpressured {
                download
                    .state
                    .expire_requests(now)
                    .map_err(DownloadError::Swarm)?;
            }
            download.control.observe_swarm(&download.state, now);
            download.control.emit_storage_state();
            peers.observe_content_peers(&download.state)?;
            next_maintenance_at = now.saturating_add(CONTENT_SWARM_MAINTENANCE_INTERVAL);
        }

        let assignments = if storage_ready && !storage_backpressured {
            download.state.schedule(now).map_err(DownloadError::Swarm)?
        } else {
            Vec::new()
        };
        let mut failed_connections = BTreeSet::new();
        for assignment in assignments {
            if sockets
                .send(
                    assignment.connection,
                    PeerMessage::Request(assignment.block.request()),
                )
                .await
                .is_err()
            {
                failed_connections.insert(assignment.connection);
                continue;
            }
            let piece_length = download
                .layout
                .piece_length_at(assignment.block.piece)
                .map_err(DownloadError::Layout)?;
            let (attempt, started) = download
                .control
                .disk_block_requested(assignment.block, piece_length);
            if started {
                download.control.emit(DownloadActivityEvent::PieceStarted {
                    piece_index: assignment.block.piece,
                    piece_length,
                    attempt,
                });
            }
            download
                .control
                .emit(DownloadActivityEvent::BlockRequested {
                    piece_index: assignment.block.piece,
                    begin: assignment.block.begin,
                    length: assignment.block.length,
                });
        }
        for connection in failed_connections {
            close_content_connection(
                peers,
                sockets,
                &mut download.state,
                connection,
                Some(PeerFailure::RemoteClosed),
            )
            .await?;
        }
        if download.is_complete() {
            let now = peers.elapsed();
            download.control.observe_swarm(&download.state, now);
            download.control.emit_storage_state_force();
            peers.observe_content_peers(&download.state)?;
            return Ok(());
        }

        if !storage_backpressured {
            fill_content_dials(
                peers,
                sockets,
                &mut download.state,
                download.metainfo.info_hash,
            )?;
        }
        if sockets.established_len() == 0
            && sockets.pending_len() == 0
            && !discovery.is_active()
            && download.control.snapshot().storage_jobs_pending == 0
        {
            return Err(peers
                .last_error
                .take()
                .unwrap_or(DownloadError::NoUsablePeer));
        }

        let until_expiry =
            (!storage_backpressured).then(|| next_maintenance_at.saturating_sub(peers.elapsed()));
        let cancellation = peers.control.inner.cancellation.clone();
        let event = {
            let storage = download.storage_pipeline_mut()?;
            next_content_supervisor_event(
                sockets,
                discovery,
                storage,
                storage_backpressured,
                until_expiry,
                &cancellation,
                next_owner,
            )
            .await?
        };
        if let Some(owner) = event.owner() {
            next_owner = owner.next();
        }

        match event {
            ContentSupervisorEvent::Deadline => continue,
            ContentSupervisorEvent::Storage(completion) => {
                let disposition = download
                    .handle_storage_completion(completion, peers.elapsed())
                    .await?;
                apply_content_disposition(peers, sockets, download, None, disposition).await?;
            }
            ContentSupervisorEvent::Discovery(Some(ContentDiscoveryEvent::Peers {
                source,
                tracker,
                addresses,
            })) => {
                let peer_count = addresses.len().try_into().unwrap_or(u32::MAX);
                for address in addresses {
                    if let Err(error) = peers.observe_address(address, source) {
                        peers.last_error = Some(error);
                    }
                }
                if let Some(tracker) = tracker
                    && peers
                        .selector
                        .select(
                            &peers.registry,
                            PeerSelectionContext {
                                now: peers.elapsed(),
                            },
                        )
                        .is_none()
                {
                    peers
                        .control
                        .emit(DownloadActivityEvent::TrackerPeersUnavailable {
                            tracker,
                            peer_count,
                        });
                }
            }
            ContentSupervisorEvent::Discovery(Some(ContentDiscoveryEvent::Failed(error))) => {
                peers.last_error = Some(error);
            }
            ContentSupervisorEvent::Discovery(None) => {}
            ContentSupervisorEvent::Peer(PeerSetEvent::DialPhase { attempt }) => {
                peers.transport_connected(attempt)?;
            }
            ContentSupervisorEvent::Peer(PeerSetEvent::DialCompleted { attempt, result }) => {
                download
                    .state
                    .finish_dial(pending_dial_id(attempt))
                    .map_err(DownloadError::Swarm)?;
                match result {
                    Ok((connection, handshake)) => {
                        if sockets.established_len()
                            >= download.state.config().max_established_connections
                        {
                            if let Some(replaced) =
                                download.state.replacement_candidate(peers.elapsed())
                            {
                                replace_content_connection(
                                    peers,
                                    sockets,
                                    &mut download.state,
                                    replaced,
                                )
                                .await?;
                            } else {
                                peers.dial_succeeded(attempt, &handshake)?;
                                peers.begin_disconnect(attempt, None)?;
                                peers.connection_closed(attempt, None)?;
                                continue;
                            }
                        }
                        peers.dial_succeeded(attempt, &handshake)?;
                        let id = sockets
                            .add_connection(connection)
                            .map_err(download_peer_set_error)?;
                        download
                            .state
                            .add_connection(id, peers.elapsed())
                            .map_err(DownloadError::Swarm)?;
                        if sockets.send(id, PeerMessage::Interested).await.is_err() {
                            close_content_connection(
                                peers,
                                sockets,
                                &mut download.state,
                                id,
                                Some(PeerFailure::RemoteClosed),
                            )
                            .await?;
                        }
                    }
                    Err(error) => {
                        let failure = error.peer_failure();
                        peers.dial_failed(attempt, failure)?;
                        peers.last_error = Some(download_peer_socket_error(error));
                    }
                }
            }
            ContentSupervisorEvent::Peer(PeerSetEvent::Peer(PeerTaskEvent::Message {
                attempt,
                message,
            })) => {
                let id = connection_id(attempt);
                if !sockets.contains(id) {
                    continue;
                }
                let disposition = download
                    .handle_message(sockets, id, message, peers.elapsed())
                    .await?;
                apply_content_disposition(peers, sockets, download, Some(id), disposition).await?;
            }
            ContentSupervisorEvent::Peer(PeerSetEvent::Peer(PeerTaskEvent::Stopped {
                attempt,
                result,
            })) => {
                let id = connection_id(attempt);
                if !sockets.contains(id) {
                    continue;
                }
                let failure = result.as_ref().err().map(PeerSocketError::peer_failure);
                if let Err(error) = result {
                    peers.last_error = Some(download_peer_socket_error(error));
                }
                close_content_connection(peers, sockets, &mut download.state, id, failure).await?;
            }
        }
    }
}

async fn apply_content_disposition(
    peers: &mut TorrentPeerCoordinator,
    sockets: &mut PeerSocketSet,
    download: &mut ContentSwarmDownload<'_>,
    connection: Option<ConnectionId>,
    disposition: ContentMessageDisposition,
) -> Result<(), DownloadError> {
    match disposition {
        ContentMessageDisposition::Continue => {}
        ContentMessageDisposition::ClosePeer(failure) => {
            let connection = connection.ok_or(DownloadError::Swarm(SwarmError::Invariant(
                "storage completion cannot close a peer",
            )))?;
            close_content_connection(
                peers,
                sockets,
                &mut download.state,
                connection,
                Some(failure),
            )
            .await?;
        }
        ContentMessageDisposition::PieceVerified(contributors) => {
            record_verified_piece_contributors(peers, download, &contributors)?;
        }
        ContentMessageDisposition::PieceHashFailed(failure) => {
            record_failed_piece_contributors(peers, sockets, download, &failure).await?;
        }
    }
    Ok(())
}

async fn download_content_swarm<'a>(
    peers: &mut TorrentPeerCoordinator,
    mut download: ContentSwarmDownload<'a>,
) -> Result<ContentSwarmDownload<'a>, DownloadError> {
    let mut sockets = PeerSocketSet::new();
    let mut discovery = ContentDiscovery::start(peers, download.metainfo.info_hash);
    let result = run_selective_swarm_loop(peers, &mut sockets, &mut discovery, &mut download).await;
    let failure = result.as_ref().err().and_then(content_peer_failure);
    let discovery_cleanup = discovery.shutdown().await;
    let peer_cleanup =
        cleanup_content_connections(peers, sockets, &mut download.state, failure).await;
    let connection_cleanup = match (discovery_cleanup, peer_cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(first), Err(second)) => Err(DownloadError::PeerTask(format!(
            "{first}; additionally {second}"
        ))),
    };
    let storage_cleanup = download
        .stop_storage(result.is_err() || connection_cleanup.is_err())
        .await;
    let cleanup = match (connection_cleanup, storage_cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(first), Err(second)) => Err(DownloadError::StorageTask(format!(
            "{first}; additionally {second}"
        ))),
    };
    match (result, cleanup) {
        (Ok(()), Ok(())) => Ok(download),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => Err(DownloadError::PeerCleanup {
            failure: error.to_string(),
            cleanup: cleanup.to_string(),
        }),
    }
}

struct FullRecheckResult {
    verified: Vec<bool>,
    recovered: Vec<u32>,
}

async fn inventory_recheck_sources(
    storage: &mut SelectiveStorage,
    layout: &TorrentLayout,
    piece_indices: &[u32],
    control: &DownloadControl,
) -> Result<Vec<(u32, u32, bool)>, DownloadError> {
    let mut inventory = Vec::with_capacity(piece_indices.len());
    for &piece_index in piece_indices {
        if control.is_cancelled() {
            return Err(DownloadError::Cancelled);
        }
        let piece_length = layout
            .piece_length_at(piece_index)
            .map_err(DownloadError::Layout)?;
        let available = storage
            .has_piece_sources(piece_index)
            .await
            .map_err(DownloadError::SelectiveStorage)?;
        inventory.push((piece_index, piece_length, available));
    }
    Ok(inventory)
}

async fn full_recheck_managed_storage(
    storage: &mut SelectiveStorage,
    metainfo: &Metainfo,
    layout: &TorrentLayout,
    inventory: &[(u32, u32, bool)],
    previous: &[bool],
    control: &DownloadControl,
) -> Result<FullRecheckResult, DownloadError> {
    let mut verified = vec![false; layout.piece_count()];
    for piece_index in 0..layout.piece_count() {
        storage
            .set_verified(piece_index, false)
            .map_err(DownloadError::SelectiveStorage)?;
    }

    let mut candidates = VecDeque::new();
    for &(piece_index, piece_length, available) in inventory {
        if available {
            candidates.push_back((piece_index, piece_length));
        } else {
            control.disk_piece_hashing(piece_index, piece_length);
            control.disk_piece_failed(
                piece_index,
                piece_length,
                "recheck source is missing or short",
            );
        }
    }

    let hash_concurrency = control.storage_execution_limits().1;
    let mut running = JoinSet::new();
    let mut recovered = Vec::new();
    let mut cancelled = false;
    let mut first_error = None;

    loop {
        cancelled |= control.is_cancelled();
        while !cancelled && first_error.is_none() && running.len() < hash_concurrency {
            let Some((piece_index, piece_length)) = candidates.pop_front() else {
                break;
            };
            control.disk_piece_hashing(piece_index, piece_length);
            control.emit(DownloadActivityEvent::PieceHashing { piece_index });
            let operation = match storage.prepare_hash(piece_index) {
                Ok(operation) => operation,
                Err(error) if error.is_missing_or_short_source() => {
                    control.disk_piece_failed(piece_index, piece_length, &error.to_string());
                    continue;
                }
                Err(error) => {
                    control.disk_piece_failed(piece_index, piece_length, &error.to_string());
                    first_error = Some(DownloadError::SelectiveStorage(error));
                    break;
                }
            };
            let started_at = Instant::now();
            control.storage_command_started(StorageCommandKind::Hash, started_at, started_at);
            let job_control = control.clone();
            running.spawn(async move {
                job_control.wait_before_storage_hash().await;
                (
                    piece_index,
                    piece_length,
                    started_at,
                    operation.execute().await,
                )
            });
        }

        let Some(result) = running.join_next().await else {
            break;
        };
        match result {
            Ok((piece_index, piece_length, started_at, result)) => {
                control.storage_command_completed(
                    StorageCommandKind::Hash,
                    started_at,
                    Instant::now(),
                );
                let piece_index_usize = match usize::try_from(piece_index) {
                    Ok(piece_index) => piece_index,
                    Err(_) => {
                        first_error
                            .get_or_insert(DownloadError::Layout(LayoutError::ArithmeticOverflow));
                        continue;
                    }
                };
                let matches = match result {
                    Ok(actual) => actual == metainfo.piece_hashes[piece_index_usize],
                    Err(error) if error.is_missing_or_short_source() => false,
                    Err(error) => {
                        control.disk_piece_failed(piece_index, piece_length, &error.to_string());
                        first_error.get_or_insert(DownloadError::SelectiveStorage(error));
                        continue;
                    }
                };
                if !matches {
                    control.disk_piece_failed(piece_index, piece_length, "recheck hash mismatch");
                    continue;
                }
                verified[piece_index_usize] = true;
                if let Err(error) = storage.set_verified(piece_index_usize, true) {
                    first_error.get_or_insert(DownloadError::SelectiveStorage(error));
                    continue;
                }
                if !previous[piece_index_usize] {
                    recovered.push(piece_index);
                }
                control.disk_piece_hash_verified(piece_index, piece_length, false);
            }
            Err(error) => {
                first_error.get_or_insert_with(|| DownloadError::StorageTask(error.to_string()));
            }
        }
    }

    if let Some(error) = first_error {
        return Err(error);
    }
    if cancelled {
        return Err(DownloadError::Cancelled);
    }
    Ok(FullRecheckResult {
        verified,
        recovered,
    })
}

async fn run_selective_download(
    config: ContentDownloadConfig,
    metainfo: Metainfo,
    control: DownloadControl,
    descriptors: Option<DescriptorStorage>,
    peers: &mut TorrentPeerCoordinator,
    resume: Option<ResumeContext>,
) -> Result<DownloadReport, DownloadError> {
    let layout = TorrentLayout::from_metainfo(&metainfo);
    let selection =
        FileSelection::new(&layout, &config.skip_files).map_err(DownloadError::Layout)?;
    for &file_index in &config.materialize_files {
        let file = layout.files().get(file_index).ok_or(DownloadError::Layout(
            LayoutError::InvalidFileIndex {
                index: file_index,
                file_count: layout.files().len(),
            },
        ))?;
        if file.padding || selection.is_wanted(file_index) {
            return Err(DownloadError::Metainfo(MetainfoError::Unsupported(
                "materialized files must be initially skipped non-padding files",
            )));
        }
    }

    let mut plans = Vec::new();
    let mut skipped_piece_count = 0;
    for piece_index in 0..layout.piece_count() {
        let piece_index_u32 = u32::try_from(piece_index)
            .map_err(|_| DownloadError::Layout(LayoutError::ArithmeticOverflow))?;
        let ranges = layout
            .request_ranges(piece_index_u32, &selection)
            .map_err(DownloadError::Layout)?;
        if ranges.is_empty() {
            skipped_piece_count += 1;
        } else {
            plans.push((piece_index_u32, ranges));
        }
    }
    if plans.is_empty() {
        return Err(DownloadError::Metainfo(MetainfoError::Unsupported(
            "selection with no wanted pieces",
        )));
    }
    let last_wanted_piece = usize::try_from(
        plans
            .last()
            .expect("at least one wanted piece was planned")
            .0,
    )
    .map_err(|_| DownloadError::Layout(LayoutError::ArithmeticOverflow))?;

    let descriptor_backed = descriptors.is_some();
    let platform_storage = control.platform_storage();
    let platform_backed = platform_storage.is_some();
    if descriptor_backed && platform_backed {
        return Err(DownloadError::StorageTask(
            "descriptor and platform storage cannot be combined".to_owned(),
        ));
    }
    let mut verified_pieces = match &resume {
        Some(resume) if resume.verified_pieces.is_empty() => vec![false; layout.piece_count()],
        Some(resume) if resume.verified_pieces.len() == layout.piece_count() => {
            resume.verified_pieces.clone()
        }
        Some(resume) => {
            return Err(DownloadError::Checkpoint(format!(
                "have state has {} pieces, expected {}",
                resume.verified_pieces.len(),
                layout.piece_count()
            )));
        }
        None => vec![false; layout.piece_count()],
    };
    let storage_creation = control.enter_safe_cancel_critical()?;
    let (mut storage, resumed_storage) = if let Some(platform) = platform_storage {
        let (storage, resumed) = SelectiveStorage::create_with_platform(
            platform,
            &metainfo,
            layout.clone(),
            selection,
            verified_pieces.clone(),
        )
        .await
        .map_err(DownloadError::SelectiveStorage)?;
        if let Some(resume) = &resume {
            resume
                .checkpoints
                .storage_prepared(resumed)
                .map_err(DownloadError::Checkpoint)?;
        }
        (storage, Some(resumed))
    } else {
        match (descriptors, &resume) {
            (Some(descriptors), None) => (
                SelectiveStorage::create_with_descriptors(
                    &metainfo,
                    layout.clone(),
                    selection,
                    &config.materialize_files,
                    descriptors,
                )
                .await
                .map_err(DownloadError::SelectiveStorage)?,
                None,
            ),
            (None, Some(resume)) => {
                let paths = torrent_storage_paths_for_output_with_shape(
                    config.output_path.clone(),
                    metainfo.info_hash,
                    PublicationShape::from_metainfo(&metainfo),
                )
                .map_err(DownloadError::SelectiveStorage)?;
                let (storage, resumed) = match control.storage_file_pool() {
                    Some(pool) => {
                        SelectiveStorage::resume_with_paths_and_pool_expected(
                            paths,
                            &metainfo,
                            layout.clone(),
                            selection,
                            verified_pieces.clone(),
                            pool,
                            Some(resume.artifact_state),
                        )
                        .await
                    }
                    None => {
                        SelectiveStorage::resume_with_paths_expected(
                            paths,
                            &metainfo,
                            layout.clone(),
                            selection,
                            verified_pieces.clone(),
                            resume.artifact_state,
                        )
                        .await
                    }
                }
                .map_err(DownloadError::SelectiveStorage)?;
                resume
                    .checkpoints
                    .storage_prepared(resumed)
                    .map_err(DownloadError::Checkpoint)?;
                (storage, Some(resumed))
            }
            (None, None) => {
                let storage = match control.storage_file_pool() {
                    Some(pool) => {
                        SelectiveStorage::create_with_pool(
                            config.output_path.clone(),
                            &metainfo,
                            layout.clone(),
                            selection,
                            pool,
                        )
                        .await
                    }
                    None => {
                        SelectiveStorage::create(
                            config.output_path.clone(),
                            &metainfo,
                            layout.clone(),
                            selection,
                        )
                        .await
                    }
                }
                .map_err(DownloadError::SelectiveStorage)?;
                (storage, None)
            }
            (Some(descriptors), Some(resume)) => {
                let descriptor_is_empty = descriptors
                    .part_file
                    .metadata()
                    .map_err(|source| DownloadError::Io {
                        operation: "inspect resumable descriptor part file",
                        source,
                    })?
                    .len()
                    == 0;
                let initialize = resume.initialize_descriptors && descriptor_is_empty;
                let storage = if initialize {
                    SelectiveStorage::create_with_descriptors(
                        &metainfo,
                        layout.clone(),
                        selection,
                        &[],
                        descriptors,
                    )
                    .await
                    .map_err(DownloadError::SelectiveStorage)?
                } else {
                    SelectiveStorage::resume_with_descriptors(
                        &metainfo,
                        layout.clone(),
                        selection,
                        descriptors,
                        verified_pieces.clone(),
                    )
                    .await
                    .map_err(DownloadError::SelectiveStorage)?
                };
                let resumed = if initialize {
                    ResumedStorage::Created
                } else {
                    ResumedStorage::Staging
                };
                resume
                    .checkpoints
                    .storage_prepared(resumed)
                    .map_err(DownloadError::Checkpoint)?;
                (storage, Some(resumed))
            }
        }
    };
    drop(storage_creation);

    if let (Some(resume), Some(resumed)) = (&resume, resumed_storage) {
        let piece_indices = plans
            .iter()
            .map(|(piece_index, _)| *piece_index)
            .collect::<Vec<_>>();
        let inventory = if resumed == ResumedStorage::Created {
            Vec::new()
        } else {
            inventory_recheck_sources(&mut storage, &layout, &piece_indices, &control).await?
        };
        resume
            .checkpoints
            .recheck_started()
            .map_err(DownloadError::Checkpoint)?;
        let previous = verified_pieces.clone();
        let checked = if resumed == ResumedStorage::Created {
            FullRecheckResult {
                verified: vec![false; layout.piece_count()],
                recovered: Vec::new(),
            }
        } else {
            full_recheck_managed_storage(
                &mut storage,
                &metainfo,
                &layout,
                &inventory,
                &previous,
                &control,
            )
            .await?
        };
        verified_pieces = checked.verified;
        storage
            .reconcile_after_recheck()
            .await
            .map_err(DownloadError::SelectiveStorage)?;
        if resumed == ResumedStorage::Staging {
            storage
                .sync_pieces(&checked.recovered)
                .await
                .map_err(DownloadError::SelectiveStorage)?;
        }
        resume
            .checkpoints
            .have_rechecked(&verified_pieces)
            .map_err(DownloadError::Checkpoint)?;
    }

    plans.retain(|(piece_index, _)| {
        usize::try_from(*piece_index)
            .ok()
            .and_then(|piece_index| verified_pieces.get(piece_index))
            .is_none_or(|verified| !*verified)
    });
    let selected_file_bytes = storage.selected_bytes();
    let skipped_file_bytes = storage.skipped_bytes();
    let padding_bytes = storage.padding_bytes();
    let part_path = storage.part_path().map(Path::to_path_buf);
    if resume
        .as_ref()
        .is_some_and(|resume| !resume.download_missing)
        && !plans.is_empty()
    {
        return Ok(DownloadReport {
            info_hash: metainfo.info_hash,
            piece_hash: metainfo.piece_hashes[last_wanted_piece],
            bytes_written: 0,
            block_count: 0,
            payload_limit: config.max_buffered_payload_bytes,
            payload_high_water: control.snapshot().payload_high_water,
            outstanding_request_limit: config.swarm_config.max_outstanding_request_bytes,
            outstanding_request_high_water: 0,
            active_piece_limit: config.swarm_config.max_active_piece_bytes,
            verification_buffer: VERIFICATION_CHUNK_LENGTH,
            piece_count: layout.piece_count(),
            verified_piece_count: verified_pieces.iter().filter(|verified| **verified).count(),
            skipped_piece_count,
            selected_file_bytes,
            skipped_file_bytes,
            padding_bytes,
            selected_written_bytes: 0,
            part_written_bytes: 0,
            materialized_bytes: 0,
            part_slots_before_materialization: storage.part_slots(),
            part_slots_after_materialization: storage.part_slots(),
            part_reopened: storage.has_part_file(),
            part_path,
            prepared_files: Vec::new(),
        });
    }
    let (
        total_blocks,
        total_bytes,
        selected_written_bytes,
        part_written_bytes,
        outstanding_request_high_water,
    ) = if plans.is_empty() {
        (0, 0, 0, 0, 0)
    } else {
        let download = ContentSwarmDownload::new(
            config.swarm_config,
            config.max_buffered_payload_bytes,
            plans,
            ContentStorage(Box::new(storage)),
            ContentDownloadContext {
                metainfo: &metainfo,
                layout: &layout,
                resume: resume.as_ref(),
                control: &control,
            },
        )
        .await?;
        let mut completed = download_content_swarm(peers, download).await?;
        let result = (
            completed.total_blocks,
            completed.total_bytes,
            completed.selected_written_bytes,
            completed.part_written_bytes,
            completed
                .state
                .snapshot(peers.elapsed())
                .outstanding_request_high_water,
        );
        let returned_storage = completed.take_storage()?;
        drop(completed);
        storage = *returned_storage.0;
        result
    };

    let completed_existing_publication = storage.is_published();
    if descriptor_backed {
        storage
            .prepare_descriptors()
            .await
            .map_err(DownloadError::SelectiveStorage)?;
    } else if storage.is_published() {
        storage
            .finish_published()
            .await
            .map_err(DownloadError::SelectiveStorage)?;
    } else if platform_backed {
        storage
            .prepare_platform()
            .await
            .map_err(DownloadError::SelectiveStorage)?;
    } else {
        storage
            .prepare_path_publication()
            .await
            .map_err(DownloadError::SelectiveStorage)?;
        if let Some(resume) = &resume {
            resume
                .checkpoints
                .publication_prepared()
                .map_err(DownloadError::Checkpoint)?;
        }
        control
            .enter_path_publication_stage(PathPublicationStage::IntentDurable)
            .await;
        storage
            .rename_path_publication()
            .await
            .map_err(DownloadError::SelectiveStorage)?;
        control
            .enter_path_publication_stage(PathPublicationStage::Renamed)
            .await;
        storage
            .sync_path_publication_namespace()
            .await
            .map_err(DownloadError::SelectiveStorage)?;
        control
            .enter_path_publication_stage(PathPublicationStage::NamespaceDurable)
            .await;
        storage
            .finish_path_publication()
            .map_err(DownloadError::SelectiveStorage)?;
    }
    if ((!descriptor_backed && !platform_backed) || completed_existing_publication)
        && let Some(resume) = &resume
    {
        resume
            .checkpoints
            .published()
            .map_err(DownloadError::Checkpoint)?;
    }
    let part_slots_before_materialization = storage.part_slots();
    let part_reopened = storage.has_part_file();
    storage
        .reopen_part_file()
        .await
        .map_err(DownloadError::SelectiveStorage)?;
    let mut materialized_bytes = 0_u64;
    for file_index in config.materialize_files {
        materialized_bytes += storage
            .materialize_file(file_index)
            .await
            .map_err(DownloadError::SelectiveStorage)?
            .bytes;
    }
    let part_slots_after_materialization = storage.part_slots();
    let requires_provider_publication =
        descriptor_backed || (platform_backed && !completed_existing_publication);
    let prepared_files = if requires_provider_publication {
        storage
            .finalize_descriptor_hashes()
            .await
            .map_err(DownloadError::SelectiveStorage)?
    } else {
        Vec::new()
    };
    if requires_provider_publication && let Some(resume) = &resume {
        resume
            .checkpoints
            .descriptor_prepared(&prepared_files)
            .map_err(DownloadError::Checkpoint)?;
    }
    Ok(DownloadReport {
        info_hash: metainfo.info_hash,
        // Selective pieces may complete in any order. Keep the diagnostic
        // report stable by naming the highest-index wanted piece rather than
        // whichever verification completion happened to arrive last.
        piece_hash: metainfo.piece_hashes[last_wanted_piece],
        bytes_written: total_bytes,
        block_count: total_blocks,
        payload_limit: config.max_buffered_payload_bytes,
        payload_high_water: control.snapshot().payload_high_water,
        outstanding_request_limit: config.swarm_config.max_outstanding_request_bytes,
        outstanding_request_high_water,
        active_piece_limit: config.swarm_config.max_active_piece_bytes,
        verification_buffer: VERIFICATION_CHUNK_LENGTH,
        piece_count: layout.piece_count(),
        verified_piece_count: layout.piece_count() - skipped_piece_count,
        skipped_piece_count,
        selected_file_bytes,
        skipped_file_bytes,
        padding_bytes,
        selected_written_bytes,
        part_written_bytes,
        materialized_bytes,
        part_slots_before_materialization,
        part_slots_after_materialization,
        part_reopened,
        part_path,
        prepared_files,
    })
}

#[cfg(test)]
async fn connect_peer(
    attempt: DialAttempt,
    info_hash: [u8; 20],
    advertise_extensions: bool,
    network: NetworkConfig,
) -> Result<(PeerConnection, Handshake), DownloadError> {
    peer_socket::connect(attempt, info_hash, advertise_extensions, network)
        .await
        .map_err(download_peer_socket_error)
}

async fn next_peer_message(peer: &mut PeerConnection) -> Result<PeerMessage, DownloadError> {
    peer_socket::next_message(peer)
        .await
        .map_err(download_peer_socket_error)
}

fn decode_availability(bitfield: &[u8], availability: &mut [bool]) {
    for (index, available) in availability.iter_mut().enumerate() {
        *available = bitfield[index / 8] & (1 << (7 - index % 8)) != 0;
    }
}

async fn read_bounded_metainfo(path: &Path) -> Result<Vec<u8>, DownloadError> {
    let file = File::open(path).await.map_err(|source| DownloadError::Io {
        operation: "open metainfo",
        source,
    })?;
    let mut bytes = Vec::new();
    file.take((BEP9_METAINFO_LIMITS.max_outer_bytes + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .map_err(|source| DownloadError::Io {
            operation: "read metainfo",
            source,
        })?;
    if bytes.len() > BEP9_METAINFO_LIMITS.max_outer_bytes {
        return Err(DownloadError::MetainfoTooLarge {
            maximum: BEP9_METAINFO_LIMITS.max_outer_bytes,
        });
    }
    Ok(bytes)
}

async fn send_message(
    peer: &mut PeerConnection,
    message: &PeerMessage,
) -> Result<(), DownloadError> {
    peer_socket::send_message(peer, message)
        .await
        .map_err(download_peer_socket_error)
}

fn download_peer_socket_error(error: PeerSocketError) -> DownloadError {
    match error {
        PeerSocketError::Cancelled => DownloadError::Cancelled,
        PeerSocketError::NetworkPolicyDenied { address, policy } => {
            DownloadError::NetworkPolicyDenied { address, policy }
        }
        PeerSocketError::Io { operation, source } => DownloadError::Io { operation, source },
        PeerSocketError::TimedOut { operation, timeout } => {
            DownloadError::PeerTimedOut { operation, timeout }
        }
        PeerSocketError::Closed => DownloadError::PeerClosed,
        PeerSocketError::Handshake(error) => DownloadError::Handshake(error),
        PeerSocketError::Frame(error) => DownloadError::Frame(error),
    }
}

fn download_peer_set_error(error: PeerSetError) -> DownloadError {
    DownloadError::PeerTask(error.to_string())
}

#[cfg(test)]
mod tests;
