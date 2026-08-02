use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt;
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rstorrent_protocol::bencode::MAX_BENCODE_INPUT_LENGTH;
use rstorrent_protocol::magnet::{Magnet, MagnetError, PeerHint, UdpTrackerUrl};
use rstorrent_protocol::metadata::{
    MetadataError, MetadataExtensionUpdate, MetadataInstant, MetadataMessage,
    TorrentMetadataDownload, TorrentMetadataEvent, UT_METADATA_LOCAL_ID,
    encode_extension_handshake, encode_metadata_reject, encode_metadata_request,
    parse_extension_handshake, parse_metadata_message,
};
use rstorrent_protocol::metainfo::{MAX_PIECES, Metainfo, MetainfoError};
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
use tokio::sync::mpsc;
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{Instant as TokioInstant, timeout, timeout_at};
use tokio_util::sync::CancellationToken;

use crate::dht::{DhtError, DhtHandle};
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
    DescriptorStorage, PreparedFileHash, ResumedStorage, SelectiveStorage, SelectiveStorageError,
    remove_selective_part_if_present, remove_selective_staging_if_present,
};
use crate::storage::{
    StagingFile, StorageError, VERIFICATION_CHUNK_LENGTH, remove_staging_if_present, staging_path,
};
use crate::swarm::{
    BlockKey, ConnectionId, ConnectionRemoval, ConnectionWindowPhaseSnapshot, NoRequestReason,
    PendingDialId, PieceHashFailure, PiecePlan, ReceiveDisposition, SwarmConfig, SwarmError,
    SwarmState,
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
const MAX_RESOLVED_ADDRESSES: usize = 32;
const UNKNOWN_MAGNET_LEFT: u64 = 16 * 1024;
const UDP_TRACKER_RECEIVE_LENGTH: usize = MAX_ANNOUNCE_RESPONSE_LENGTH + 1;
const SAFE_CANCEL_REQUESTED: usize = 1 << (usize::BITS - 1);
const SAFE_CANCEL_CRITICAL_MASK: usize = SAFE_CANCEL_REQUESTED - 1;
const DHT_RETRY_INITIAL_DELAY: Duration = Duration::from_secs(15);
const DHT_RETRY_MAX_DELAY: Duration = Duration::from_secs(5 * 60);
const DHT_SUCCESS_REQUERY_DELAY: Duration = Duration::from_secs(60);
const CONTENT_PEER_DIAGNOSTIC_INTERVAL: Duration = Duration::from_secs(1);
const PEER_OBSERVATION_INTERVAL: Duration = Duration::from_millis(100);
const MAX_METADATA_PEERS: usize = 8;
const METADATA_SCHEDULER_TICK: Duration = Duration::from_millis(100);
const MAX_RECENT_METADATA_ATTEMPTS: usize = 64;
const MAX_DIAGNOSTIC_ERROR_LENGTH: usize = 256;

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
    pub output_path: PathBuf,
    pub network: NetworkConfig,
    pub resource_limits: DownloadResourceLimits,
    pub skip_files: Vec<usize>,
    pub verified_info: Option<Vec<u8>>,
    pub verified_pieces: Vec<bool>,
    pub dht: Option<DhtHandle>,
}

pub trait DownloadCheckpointSink: Send + Sync {
    fn metadata_verified(&self, raw_info: &[u8]) -> Result<(), String>;
    fn storage_prepared(&self, storage: ResumedStorage) -> Result<(), String>;
    fn have_rechecked(&self, verified_pieces: &[bool]) -> Result<(), String>;
    fn piece_durable(&self, piece_index: usize) -> Result<(), String>;
    fn descriptor_prepared(&self, files: &[PreparedFileHash]) -> Result<(), String>;
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
    TrackerState(Box<TrackerRuntimeSnapshot>),
    SwarmState(Box<SwarmActivitySnapshot>),
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
    storage_hashes_started: AtomicUsize,
    storage_write_timing: StorageCommandTiming,
    storage_hash_timing: StorageCommandTiming,
    storage_write_blocks_started: AtomicUsize,
    storage_write_blocks_completed: AtomicUsize,
    storage_write_batch_blocks_high_water: AtomicUsize,
    storage_write_batch_bytes_high_water: AtomicUsize,
    storage_active_operation: AtomicUsize,
    storage_active_started_micros: AtomicU64,
    disk_runtime: Mutex<DiskRuntimeState>,
    activity_sink: Mutex<Option<Arc<dyn DownloadActivitySink>>>,
    last_swarm_activity: Mutex<Option<SwarmActivitySnapshot>>,
    last_content_peers: Mutex<(Option<Duration>, Vec<ContentPeerActivitySnapshot>)>,
    last_content_registry: Mutex<Option<PeerRegistryCounts>>,
    peer_connections: Mutex<PeerConnectionDiagnosticState>,
    metadata_diagnostics: Mutex<MetadataDiagnosticState>,
    safe_cancel_state: AtomicUsize,
}

#[derive(Debug, Default)]
struct PeerConnectionDiagnosticState {
    current: Vec<PeerConnectionObservation>,
    last_emitted: Vec<PeerConnectionObservation>,
    last_emitted_at: Option<Duration>,
}

#[derive(Debug, Default)]
struct StorageCommandTiming {
    started: AtomicUsize,
    completed: AtomicUsize,
    queue_wait_micros: AtomicU64,
    queue_wait_max_micros: AtomicU64,
    service_micros: AtomicU64,
    service_max_micros: AtomicU64,
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
    pub storage_hash_queue_wait_micros: u64,
    pub storage_hash_queue_wait_max_micros: u64,
    pub storage_hash_service_micros: u64,
    pub storage_hash_service_max_micros: u64,
    pub storage_active_write_micros: Option<u64>,
    pub storage_active_hash_micros: Option<u64>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiskPieceStage {
    Receiving,
    Queued,
    Writing,
    Stored,
    Hashing,
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
    started_at: Instant,
    stage_started_at: Instant,
    error: Option<String>,
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
                storage_hashes_started: AtomicUsize::new(0),
                storage_write_timing: StorageCommandTiming::default(),
                storage_hash_timing: StorageCommandTiming::default(),
                storage_write_blocks_started: AtomicUsize::new(0),
                storage_write_blocks_completed: AtomicUsize::new(0),
                storage_write_batch_blocks_high_water: AtomicUsize::new(0),
                storage_write_batch_bytes_high_water: AtomicUsize::new(0),
                storage_active_operation: AtomicUsize::new(0),
                storage_active_started_micros: AtomicU64::new(0),
                disk_runtime: Mutex::new(DiskRuntimeState::default()),
                activity_sink: Mutex::new(None),
                last_swarm_activity: Mutex::new(None),
                last_content_peers: Mutex::new((None, Vec::new())),
                last_content_registry: Mutex::new(None),
                peer_connections: Mutex::new(PeerConnectionDiagnosticState::default()),
                metadata_diagnostics: Mutex::new(MetadataDiagnosticState::default()),
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

    pub fn snapshot(&self) -> DownloadProgress {
        let write_timing = &self.inner.storage_write_timing;
        let hash_timing = &self.inner.storage_hash_timing;
        let (storage_active_write_micros, storage_active_hash_micros) = self.storage_active_ages();
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
            storage_hash_queue_wait_micros: hash_timing.queue_wait_micros.load(Ordering::Acquire),
            storage_hash_queue_wait_max_micros: hash_timing
                .queue_wait_max_micros
                .load(Ordering::Acquire),
            storage_hash_service_micros: hash_timing.service_micros.load(Ordering::Acquire),
            storage_hash_service_max_micros: hash_timing.service_max_micros.load(Ordering::Acquire),
            storage_active_write_micros,
            storage_active_hash_micros,
        }
    }

    pub fn diagnostic_snapshot(&self) -> DownloadDiagnosticSnapshot {
        let progress = self.snapshot();
        let swarm = *self
            .inner
            .last_swarm_activity
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let content_registry = *self
            .inner
            .last_content_registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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

    fn observe_content_registry(&self, registry: &PeerRegistry, now: Duration) {
        *self
            .inner
            .last_content_registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
            Some(registry.counts(PeerSelectionContext { now }));
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
        self.emit_storage_state();
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
            let stage = if range_bytes(&piece.stored) >= piece.piece_length {
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
            state.hashing_bytes = piece_length as usize;
        }
        self.mutate_disk_piece(piece_index, piece_length, |piece, now| {
            set_disk_piece_stage(piece, DiskPieceStage::Hashing, now);
        });
        self.emit_storage_state();
    }

    fn disk_piece_verified(&self, piece_index: u32, piece_length: u32) {
        let mut state = self
            .inner
            .disk_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.hashing_bytes = 0;
        state.verified_bytes_total = state
            .verified_bytes_total
            .saturating_add(piece_length as usize);
        state.pieces.remove(&piece_index);
        let resident = self.inner.buffered_payload_bytes.load(Ordering::Acquire);
        drop(state);
        self.update_disk_pressure(resident);
        self.emit_storage_state();
    }

    fn disk_piece_failed(&self, piece_index: u32, piece_length: u32, detail: &str) {
        {
            let mut state = self
                .inner
                .disk_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.hashing_bytes = 0;
        }
        self.mutate_disk_piece(piece_index, piece_length, |piece, now| {
            piece.error = Some(bounded_diagnostic_detail(detail));
            set_disk_piece_stage(piece, DiskPieceStage::Failed, now);
        });
        self.emit_storage_state();
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
        self.emit_storage_state();
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
                started_at: now,
                stage_started_at: now,
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
        state.writing_bytes = bytes;
        for block in blocks {
            if let Some(piece) = state.pieces.get_mut(&block.piece) {
                set_disk_piece_stage(piece, DiskPieceStage::Writing, now);
            }
        }
        drop(state);
        self.emit_storage_state();
    }

    fn disk_write_batch_completed(&self) {
        self.inner
            .disk_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .writing_bytes = 0;
        self.emit_storage_state();
    }

    fn emit_storage_state(&self) {
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
        atomic_saturating_add(&timing.queue_wait_micros, queue_wait_micros);
        timing
            .queue_wait_max_micros
            .fetch_max(queue_wait_micros, Ordering::AcqRel);
        self.inner.storage_active_started_micros.store(
            duration_micros(
                started_at
                    .checked_duration_since(self.inner.started_at)
                    .unwrap_or_default(),
            ),
            Ordering::Release,
        );
        self.inner
            .storage_active_operation
            .store(kind as usize, Ordering::Release);
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
        self.clear_storage_active_operation();
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
        blocks: usize,
    ) {
        atomic_saturating_add_usize(&self.inner.storage_write_blocks_completed, blocks);
        self.storage_command_completed(StorageCommandKind::Write, started_at, completed_at);
        self.disk_write_batch_completed();
    }

    fn storage_timing(&self, kind: StorageCommandKind) -> &StorageCommandTiming {
        match kind {
            StorageCommandKind::Write => &self.inner.storage_write_timing,
            StorageCommandKind::Hash => &self.inner.storage_hash_timing,
        }
    }

    fn storage_active_ages(&self) -> (Option<u64>, Option<u64>) {
        let first_kind = self.inner.storage_active_operation.load(Ordering::Acquire);
        if first_kind == 0 {
            return (None, None);
        }
        let started_micros = self
            .inner
            .storage_active_started_micros
            .load(Ordering::Acquire);
        let second_kind = self.inner.storage_active_operation.load(Ordering::Acquire);
        if first_kind != second_kind {
            return (None, None);
        }
        let age = duration_micros(self.inner.started_at.elapsed()).saturating_sub(started_micros);
        match first_kind {
            value if value == StorageCommandKind::Write as usize => (Some(age), None),
            value if value == StorageCommandKind::Hash as usize => (None, Some(age)),
            _ => (None, None),
        }
    }

    fn clear_storage_active_operation(&self) {
        self.inner
            .storage_active_operation
            .store(0, Ordering::Release);
        self.inner
            .storage_active_started_micros
            .store(0, Ordering::Release);
    }

    fn clear_storage_jobs(&self) {
        self.inner.storage_jobs_pending.store(0, Ordering::Release);
        self.clear_storage_active_operation();
        let mut state = self
            .inner
            .disk_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.queued_write_bytes = 0;
        state.writing_bytes = 0;
        state.hashing_bytes = 0;
        state.pieces.clear();
        state.pressure = DiskPressure::Idle;
        if let Some(started) = state.backpressured_since.take() {
            state.backpressured_total = state
                .backpressured_total
                .saturating_add(Instant::now().saturating_duration_since(started));
        }
        drop(state);
        self.emit_storage_state();
    }

    fn clear_buffered_payload(&self) {
        self.inner
            .buffered_payload_bytes
            .store(0, Ordering::Release);
        self.update_disk_pressure(0);
        self.emit_storage_state();
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
    Storage(StorageError),
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
            Self::Storage(error) => write!(formatter, "storage: {error}"),
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
            Self::Storage(error) => Some(error),
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
    let staging = staging_path(&output_path).map_err(DownloadError::Storage)?;
    let result = run_magnet_download(config, control.clone()).await;
    let result = require_terminal_owner_cleanup(&control, result);
    control.clear_buffered_payload();

    match result {
        Ok(report) => Ok(report),
        Err(error) if preserves_existing_artifact(&error) => Err(error),
        Err(error) => {
            let cleanup = async {
                remove_staging_if_present(&staging).await?;
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

    let staging = staging_path(&config.output_path).map_err(DownloadError::Storage)?;
    let output_path = config.output_path.clone();
    let result = run_download(config, control.clone(), None).await;
    let result = require_terminal_owner_cleanup(&control, result);
    control.clear_buffered_payload();

    match result {
        Ok(report) => Ok(report),
        Err(error) if preserves_existing_artifact(&error) => Err(error),
        Err(error) => {
            let cleanup = async {
                remove_staging_if_present(&staging).await?;
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
        DownloadError::Storage(StorageError::ExistingOutput(_))
            | DownloadError::Storage(StorageError::ExistingStaging(_))
            | DownloadError::SelectiveStorage(SelectiveStorageError::ExistingOutput(_))
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
                if !self.haves.contains(&index) && self.haves.len() == MAX_PIECES {
                    return Err(DownloadError::InvalidPremetadataState(
                        "too many distinct HAVE indices",
                    ));
                }
                self.haves.insert(index);
            }
            PeerMessage::Bitfield(bitfield) => {
                if bitfield.len() > MAX_PIECES.div_ceil(8) {
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
        Ok(())
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
                if let Err(error) = sockets.begin_dial(attempt, info_hash, true, self.network) {
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
        let Some(tracker) = self.tracker.take() else {
            return Ok(());
        };
        tracker.shutdown().await
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
        initialize_descriptors: descriptors
            .as_ref()
            .is_some_and(|(_, initialize)| *initialize),
    };
    let descriptors = descriptors.map(|(descriptors, _)| descriptors);

    if let Some(raw_info) = config.verified_info {
        let metainfo = Metainfo::from_info_bytes(&raw_info).map_err(DownloadError::Metainfo)?;
        if metainfo.info_hash != magnet.info_hash {
            return Err(DownloadError::Checkpoint(
                "stored metadata does not match the magnet identity".to_owned(),
            ));
        }
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
        let content_config = ContentDownloadConfig {
            output_path: config.output_path,
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
        if let Err(message) = checkpoints.metadata_verified(&raw_info) {
            peers.close_current(None)?;
            return Err(DownloadError::Checkpoint(message));
        }
        let content_config = ContentDownloadConfig {
            output_path: config.output_path,
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
    let metainfo = Metainfo::from_info_bytes(&bytes).map_err(DownloadError::Metainfo)?;
    peer.prepend_messages(peer_state.validated_messages(&metainfo)?);
    Ok((bytes, metainfo))
}

async fn run_download(
    config: DownloadConfig,
    control: DownloadControl,
    descriptors: Option<DescriptorStorage>,
) -> Result<DownloadReport, DownloadError> {
    let metainfo_bytes = read_bounded_metainfo(&config.metainfo_path).await?;
    let metainfo = Metainfo::from_bytes(&metainfo_bytes).map_err(DownloadError::Metainfo)?;
    let mut peers =
        TorrentPeerCoordinator::from_endpoint(config.peer, PeerSource::Manual, config.network)?;
    let content_config = ContentDownloadConfig {
        output_path: config.output_path,
        max_buffered_payload_bytes: config.resource_limits.max_buffered_payload_bytes,
        swarm_config: config.resource_limits.swarm_config(),
        skip_files: config.skip_files,
        materialize_files: config.materialize_files,
    };
    run_content_download(
        content_config,
        metainfo,
        control,
        descriptors,
        &mut peers,
        None,
    )
    .await
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
    let result = match metainfo.mode {
        rstorrent_protocol::metainfo::MetainfoMode::SingleFile => {
            if resume.is_some() {
                Err(DownloadError::Metainfo(MetainfoError::Unsupported(
                    "resumable execution currently requires multi-file metainfo",
                )))
            } else if descriptors.is_some() {
                Err(DownloadError::Metainfo(MetainfoError::Unsupported(
                    "descriptor diagnostic execution requires multi-file metainfo",
                )))
            } else if !config.skip_files.is_empty() || !config.materialize_files.is_empty() {
                Err(DownloadError::Metainfo(MetainfoError::Unsupported(
                    "selected single-file diagnostic execution",
                )))
            } else {
                run_single_download(config, metainfo, control, peers).await
            }
        }
        rstorrent_protocol::metainfo::MetainfoMode::MultiFile => {
            run_selective_download(config, metainfo, control, descriptors, peers, resume).await
        }
    };
    peers.close_current(result.as_ref().err().and_then(content_peer_failure))?;
    result
}

async fn run_single_download(
    config: ContentDownloadConfig,
    metainfo: Metainfo,
    control: DownloadControl,
    peers: &mut TorrentPeerCoordinator,
) -> Result<DownloadReport, DownloadError> {
    u32::try_from(metainfo.total_length)
        .map_err(|_| DownloadError::Metainfo(MetainfoError::InvalidField("info.length")))?;
    let layout = TorrentLayout::from_metainfo(&metainfo);
    let selection = FileSelection::new(&layout, &[]).map_err(DownloadError::Layout)?;
    let piece_count = u32::try_from(layout.piece_count())
        .map_err(|_| DownloadError::Layout(LayoutError::ArithmeticOverflow))?;
    let mut plans = Vec::with_capacity(layout.piece_count());
    for piece in 0..piece_count {
        plans.push((
            piece,
            layout
                .request_ranges(piece, &selection)
                .map_err(DownloadError::Layout)?,
        ));
    }
    let storage = StagingFile::create(config.output_path.clone(), metainfo.total_length)
        .await
        .map_err(DownloadError::Storage)?;
    let download = ContentSwarmDownload::new(
        config.swarm_config,
        config.max_buffered_payload_bytes,
        plans,
        ContentStorage::Single(storage),
        ContentDownloadContext {
            metainfo: &metainfo,
            layout: &layout,
            resume: None,
            control: &control,
        },
    )?;
    let mut completed = download_content_swarm(peers, download).await?;
    let piece = completed
        .last_piece
        .expect("completed single-file swarm has a verified piece");
    let block_count = completed.total_blocks;
    let bytes_written = completed.total_bytes;
    let selected_written_bytes = completed.selected_written_bytes;
    let outstanding_request_high_water = completed
        .state
        .snapshot(peers.elapsed())
        .outstanding_request_high_water;
    let payload_high_water = control.snapshot().payload_high_water;
    let storage = completed.take_storage()?;
    drop(completed);
    let ContentStorage::Single(storage) = storage else {
        return Err(DownloadError::StorageTask(
            "single-file download returned selective storage".to_owned(),
        ));
    };
    storage.finalize().await.map_err(DownloadError::Storage)?;
    Ok(DownloadReport {
        info_hash: metainfo.info_hash,
        piece_hash: piece.hash,
        bytes_written,
        block_count,
        payload_limit: config.max_buffered_payload_bytes,
        payload_high_water,
        outstanding_request_limit: config.swarm_config.max_outstanding_request_bytes,
        outstanding_request_high_water,
        active_piece_limit: config.swarm_config.max_active_piece_bytes,
        verification_buffer: VERIFICATION_CHUNK_LENGTH,
        piece_count: layout.piece_count(),
        verified_piece_count: layout.piece_count(),
        skipped_piece_count: 0,
        selected_file_bytes: metainfo.total_length,
        skipped_file_bytes: 0,
        padding_bytes: 0,
        selected_written_bytes,
        part_written_bytes: 0,
        materialized_bytes: 0,
        part_slots_before_materialization: 0,
        part_slots_after_materialization: 0,
        part_reopened: false,
        part_path: None,
        prepared_files: Vec::new(),
    })
}

enum ContentStorage {
    Single(StagingFile),
    Selective(Box<SelectiveStorage>),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ContentWriteStats {
    selected_bytes: usize,
    part_bytes: usize,
}

enum ContentStorageCommand {
    Write {
        block: BlockKey,
        offset: u64,
        bytes: Vec<u8>,
    },
    Verify {
        piece: u32,
        offset: u64,
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
    offset: u64,
    bytes: Vec<u8>,
    stats: ContentWriteStats,
}

struct ContentWriteMember {
    block: BlockKey,
    stats: ContentWriteStats,
}

struct CoalescedContentWrite {
    piece: u32,
    begin: u32,
    offset: u64,
    bytes: Vec<u8>,
    members: Vec<ContentWriteMember>,
}

enum ContentStorageCompletion {
    Write {
        block: BlockKey,
        result: Result<ContentWriteStats, DownloadError>,
    },
    Verify {
        piece: u32,
        length: u32,
        result: Result<[u8; 20], DownloadError>,
    },
}

struct ContentStoragePipeline {
    commands: Option<mpsc::Sender<QueuedContentStorageCommand>>,
    completions: mpsc::Receiver<ContentStorageCompletion>,
    cancellation: CancellationToken,
    task: JoinHandle<ContentStorage>,
    pending_commands: VecDeque<QueuedContentStorageCommand>,
    control: DownloadControl,
    max_buffered_payload_bytes: usize,
    job_limit: usize,
    queue_capacity: usize,
}

impl ContentStoragePipeline {
    fn start(
        storage: ContentStorage,
        control: &DownloadControl,
        max_buffered_payload_bytes: usize,
    ) -> Self {
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
        Self {
            commands: Some(command_sender),
            completions: completion_receiver,
            cancellation,
            task,
            pending_commands: VecDeque::with_capacity(CONTENT_STORAGE_PENDING_QUEUE),
            control: control.clone(),
            max_buffered_payload_bytes,
            job_limit,
            queue_capacity,
        }
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
        self.completions.recv().await.ok_or_else(|| {
            DownloadError::StorageTask("storage completion channel closed".to_owned())
        })
    }

    fn completion_received(&self, completion: &ContentStorageCompletion) {
        self.control.storage_job_finished();
        if let ContentStorageCompletion::Write { block, .. } = completion {
            self.control.release_buffered_payload(block.length as usize);
        }
    }

    async fn shutdown(mut self, cancel: bool) -> Result<ContentStorage, DownloadError> {
        self.commands.take();
        if cancel {
            self.cancellation.cancel();
        }
        let result = self
            .task
            .await
            .map_err(|error| DownloadError::StorageTask(error.to_string()));
        self.control.clear_storage_jobs();
        self.control.clear_buffered_payload();
        result
    }
}

async fn run_content_storage_task(
    mut storage: ContentStorage,
    mut commands: mpsc::Receiver<QueuedContentStorageCommand>,
    completions: mpsc::Sender<ContentStorageCompletion>,
    cancellation: CancellationToken,
    control: DownloadControl,
    queue_capacity: usize,
) -> ContentStorage {
    let mut deferred = None;
    'storage: loop {
        let command = match deferred.take() {
            Some(command) => command,
            None => tokio::select! {
                biased;
                _ = cancellation.cancelled() => break,
                command = commands.recv() => match command {
                    Some(command) => command,
                    None => break,
                },
            },
        };

        let completions_to_send = if command.command.write_bytes().is_some() {
            let batch = collect_content_write_batch(command, &mut commands, &mut deferred);
            let blocks = batch.len();
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
            let completed = execute_content_storage_writes(&mut storage, batch, &control).await;
            control.storage_write_batch_completed(started_at, Instant::now(), blocks);
            completed
        } else {
            let kind = command.command.kind();
            debug_assert_eq!(kind, StorageCommandKind::Hash);
            if let ContentStorageCommand::Verify { piece, length, .. } = &command.command {
                control.disk_piece_hashing(*piece, *length);
                control.emit(DownloadActivityEvent::PieceHashing {
                    piece_index: *piece,
                });
            }
            let started_at = Instant::now();
            control.storage_command_started(kind, command.enqueued_at, started_at);
            let completion =
                execute_content_storage_verification(&mut storage, command.command, &control).await;
            control.storage_command_completed(kind, started_at, Instant::now());
            vec![completion]
        };

        for completion in completions_to_send {
            let projected_depth = queue_capacity
                .saturating_sub(completions.capacity())
                .saturating_add(1)
                .min(queue_capacity);
            control.observe_storage_completion_queue(projected_depth);
            let sent = tokio::select! {
                biased;
                _ = cancellation.cancelled() => false,
                result = completions.send(completion) => result.is_ok(),
            };
            if !sent {
                break 'storage;
            }
        }
    }
    storage
}

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

async fn execute_content_storage_writes(
    storage: &mut ContentStorage,
    commands: Vec<QueuedContentStorageCommand>,
    control: &DownloadControl,
) -> Vec<ContentStorageCompletion> {
    control.wait_before_storage().await;
    let mut prepared = Vec::with_capacity(commands.len());
    for command in commands {
        let ContentStorageCommand::Write {
            block,
            offset,
            bytes,
        } = command.command
        else {
            unreachable!("write batches contain only write commands");
        };
        let stats = match storage {
            ContentStorage::Single(_) => Ok(ContentWriteStats {
                selected_bytes: bytes.len(),
                part_bytes: 0,
            }),
            ContentStorage::Selective(storage) => storage
                .write_stats(block.piece, block.begin, bytes.len())
                .map(|stats| ContentWriteStats {
                    selected_bytes: stats.wanted_bytes,
                    part_bytes: stats.skipped_bytes,
                })
                .map_err(DownloadError::SelectiveStorage),
        };
        let stats = match stats {
            Ok(stats) => stats,
            Err(error) => {
                return failed_content_write_batch(block, error, prepared.into_iter());
            }
        };
        prepared.push(PreparedContentWrite {
            block,
            offset,
            bytes,
            stats,
        });
    }

    let writes = match coalesce_content_writes(prepared) {
        Ok(writes) => writes,
        Err((block, error)) => {
            return vec![ContentStorageCompletion::Write {
                block,
                result: Err(error),
            }];
        }
    };
    let mut completed = Vec::new();
    for write in writes {
        let result = match storage {
            ContentStorage::Single(storage) => storage
                .write_block(write.offset, write.bytes)
                .await
                .map_err(DownloadError::Storage),
            ContentStorage::Selective(storage) => storage
                .write_block(write.piece, write.begin, write.bytes)
                .await
                .map(|_| ())
                .map_err(DownloadError::SelectiveStorage),
        };
        if let Err(error) = result {
            let mut members = write.members.into_iter();
            let first = members
                .next()
                .expect("coalesced write retains at least one logical member");
            completed.push(ContentStorageCompletion::Write {
                block: first.block,
                result: Err(error),
            });
            completed.extend(members.map(|member| ContentStorageCompletion::Write {
                block: member.block,
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
                    result: Ok(member.stats),
                }),
        );
    }
    completed
}

fn failed_content_write_batch(
    failed_block: BlockKey,
    error: DownloadError,
    prepared: impl Iterator<Item = PreparedContentWrite>,
) -> Vec<ContentStorageCompletion> {
    let mut completions = vec![ContentStorageCompletion::Write {
        block: failed_block,
        result: Err(error),
    }];
    completions.extend(prepared.map(|write| ContentStorageCompletion::Write {
        block: write.block,
        result: Err(DownloadError::StorageTask(
            "coalesced write batch validation failed".to_owned(),
        )),
    }));
    completions
}

fn coalesce_content_writes(
    mut writes: Vec<PreparedContentWrite>,
) -> Result<Vec<CoalescedContentWrite>, (BlockKey, DownloadError)> {
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
                stats: write.stats,
            }],
        });
    }
    Ok(coalesced)
}

async fn execute_content_storage_verification(
    storage: &mut ContentStorage,
    command: ContentStorageCommand,
    control: &DownloadControl,
) -> ContentStorageCompletion {
    match command {
        ContentStorageCommand::Verify {
            piece,
            offset,
            length,
            expected,
            durable,
        } => {
            control.wait_before_storage_hash().await;
            let result = match storage {
                ContentStorage::Single(storage) => storage
                    .hash_piece(offset, length)
                    .await
                    .map_err(DownloadError::Storage),
                ContentStorage::Selective(storage) => {
                    async {
                        let actual = storage
                            .hash_piece(piece)
                            .await
                            .map_err(DownloadError::SelectiveStorage)?;
                        if actual == expected {
                            let piece_index = usize::try_from(piece).map_err(|_| {
                                DownloadError::Layout(LayoutError::ArithmeticOverflow)
                            })?;
                            if durable {
                                storage
                                    .sync_piece(piece)
                                    .await
                                    .map_err(DownloadError::SelectiveStorage)?;
                            }
                            storage
                                .record_verified(piece_index)
                                .map_err(DownloadError::SelectiveStorage)?;
                        }
                        Ok(actual)
                    }
                    .await
                }
            };
            ContentStorageCompletion::Verify {
                piece,
                length,
                result,
            }
        }
        ContentStorageCommand::Write { .. } => {
            unreachable!("write commands execute through the bounded batch path")
        }
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
    fn new(
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
        Ok(Self {
            state,
            storage_pipeline: Some(ContentStoragePipeline::start(
                storage,
                control,
                max_buffered_payload_bytes,
            )),
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

    fn is_complete(&self, now: Duration) -> bool {
        self.state.snapshot(now).no_request_reason == Some(NoRequestReason::Complete)
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
                    return Ok(ContentMessageDisposition::ClosePeer(PeerFailure::Protocol));
                };
                let Ok(key) = BlockKey::new(index, begin, length) else {
                    return Ok(ContentMessageDisposition::ClosePeer(PeerFailure::Protocol));
                };
                match self
                    .state
                    .receive_block(connection, key, now)
                    .map_err(DownloadError::Swarm)?
                {
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
                        self.control.emit(DownloadActivityEvent::BlockReceived {
                            piece_index: index,
                            begin,
                            length,
                        });
                        self.contributor_attempts.insert(connection, source_attempt);
                        let offset = single_file_offset(self.layout, index, begin)?;
                        self.storage_pipeline_mut()?
                            .enqueue(ContentStorageCommand::Write {
                                block: key,
                                offset,
                                bytes: block,
                            })?;
                    }
                    ReceiveDisposition::Redundant | ReceiveDisposition::Unsolicited => {}
                }
            }
            PeerMessage::KeepAlive
            | PeerMessage::Interested
            | PeerMessage::NotInterested
            | PeerMessage::Request(_)
            | PeerMessage::Cancel(_)
            | PeerMessage::Extended { .. } => {}
        }
        self.control.observe_swarm(&self.state, now);
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

    fn handle_storage_completion(
        &mut self,
        completion: ContentStorageCompletion,
        now: Duration,
    ) -> Result<ContentMessageDisposition, DownloadError> {
        self.storage_pipeline_mut()?
            .completion_received(&completion);
        let disposition = match completion {
            ContentStorageCompletion::Write { block, result } => {
                let stats = match result {
                    Ok(stats) => stats,
                    Err(error) => {
                        self.control.disk_storage_error(&error.to_string());
                        self.state
                            .finish_write(block, false, now)
                            .map_err(DownloadError::Swarm)?;
                        self.prune_contributor_attempts();
                        return Err(error);
                    }
                };
                self.state
                    .finish_write(block, true, now)
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
                self.control.emit(DownloadActivityEvent::BlockStored {
                    piece_index: block.piece,
                    begin: block.begin,
                    length: block.length,
                });
                if self
                    .state
                    .piece_ready(block.piece)
                    .map_err(DownloadError::Swarm)?
                {
                    let piece_index = usize::try_from(block.piece)
                        .map_err(|_| DownloadError::Layout(LayoutError::ArithmeticOverflow))?;
                    let length = self
                        .layout
                        .piece_length_at(block.piece)
                        .map_err(DownloadError::Layout)?;
                    let offset = single_file_offset(self.layout, block.piece, 0)?;
                    let expected = self.metainfo.piece_hashes[piece_index];
                    let durable = self.resume.is_some();
                    self.storage_pipeline_mut()?
                        .enqueue(ContentStorageCommand::Verify {
                            piece: block.piece,
                            offset,
                            length,
                            expected,
                            durable,
                        })?;
                }
                ContentMessageDisposition::Continue
            }
            ContentStorageCompletion::Verify {
                piece,
                length,
                result,
            } => {
                let actual = result?;
                let piece_index = usize::try_from(piece)
                    .map_err(|_| DownloadError::Layout(LayoutError::ArithmeticOverflow))?;
                let expected = self.metainfo.piece_hashes[piece_index];
                if actual != expected {
                    let failure = self
                        .state
                        .mark_piece_hash_failed(piece)
                        .map_err(DownloadError::Swarm)?;
                    self.control.emit(DownloadActivityEvent::PieceHashFailed {
                        piece_index: piece,
                        contributor_count: failure.contributors.len(),
                        failed_bytes: failure.failed_bytes,
                    });
                    self.control
                        .disk_piece_failed(piece, length, "piece hash failed; retrying");
                    ContentMessageDisposition::PieceHashFailed(failure)
                } else {
                    if let Some(resume) = self.resume {
                        resume
                            .checkpoints
                            .piece_durable(piece_index)
                            .map_err(DownloadError::Checkpoint)?;
                    }
                    let contributors = self
                        .state
                        .mark_piece_verified(piece)
                        .map_err(DownloadError::Swarm)?;
                    self.last_piece = Some(VerifiedPiece {
                        index: piece,
                        hash: actual,
                        length,
                    });
                    self.control
                        .emit(DownloadActivityEvent::PieceVerified { piece_index: piece });
                    self.control.disk_piece_verified(piece, length);
                    ContentMessageDisposition::PieceVerified(contributors)
                }
            }
        };
        self.control.observe_swarm(&self.state, now);
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

fn single_file_offset(
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
        if let Err(error) = sockets.begin_dial(attempt, info_hash, false, peers.network) {
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
        download
            .control
            .observe_content_registry(&peers.registry, now);
        let expired = if storage_backpressured {
            Vec::new()
        } else {
            download
                .state
                .expire_requests(now)
                .map_err(DownloadError::Swarm)?
        };
        for request in expired {
            let _ = request;
        }
        download.control.observe_swarm(&download.state, now);
        peers.observe_content_peers(&download.state)?;

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
        download
            .control
            .observe_swarm(&download.state, peers.elapsed());
        peers.observe_content_peers(&download.state)?;
        if download.is_complete(peers.elapsed()) {
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

        let snapshot = download.state.snapshot(peers.elapsed());
        let replacement_deadline = peers
            .selector
            .select(
                &peers.registry,
                PeerSelectionContext {
                    now: peers.elapsed(),
                },
            )
            .and(snapshot.next_replacement_at);
        let next_deadline = (!storage_backpressured)
            .then(|| {
                [snapshot.next_request_expiry, replacement_deadline]
                    .into_iter()
                    .flatten()
                    .min()
            })
            .flatten();
        let until_expiry = next_deadline.map(|deadline| deadline.saturating_sub(peers.elapsed()));
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
                let disposition =
                    download.handle_storage_completion(completion, peers.elapsed())?;
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
    let (mut storage, resumed_storage) = match (descriptors, &resume) {
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
            let (storage, resumed) = SelectiveStorage::resume(
                config.output_path.clone(),
                &metainfo,
                layout.clone(),
                selection,
                verified_pieces.clone(),
            )
            .await
            .map_err(DownloadError::SelectiveStorage)?;
            resume
                .checkpoints
                .storage_prepared(resumed)
                .map_err(DownloadError::Checkpoint)?;
            (storage, Some(resumed))
        }
        (None, None) => (
            SelectiveStorage::create(
                config.output_path.clone(),
                &metainfo,
                layout.clone(),
                selection,
            )
            .await
            .map_err(DownloadError::SelectiveStorage)?,
            None,
        ),
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
    };
    drop(storage_creation);

    if let (Some(resume), Some(resumed)) = (&resume, resumed_storage) {
        let previous = verified_pieces.clone();
        match resumed {
            ResumedStorage::Created => {
                verified_pieces.fill(false);
                for piece_index in 0..layout.piece_count() {
                    storage
                        .set_verified(piece_index, false)
                        .map_err(DownloadError::SelectiveStorage)?;
                }
            }
            ResumedStorage::Staging | ResumedStorage::Published => {
                for (piece_index, verified) in verified_pieces.iter_mut().enumerate() {
                    if !*verified {
                        continue;
                    }
                    let piece_index_u32 = u32::try_from(piece_index)
                        .map_err(|_| DownloadError::Layout(LayoutError::ArithmeticOverflow))?;
                    let actual = storage
                        .hash_piece(piece_index_u32)
                        .await
                        .map_err(DownloadError::SelectiveStorage)?;
                    if actual != metainfo.piece_hashes[piece_index] {
                        *verified = false;
                        storage
                            .set_verified(piece_index, false)
                            .map_err(DownloadError::SelectiveStorage)?;
                    }
                }
            }
        }
        if verified_pieces != previous {
            resume
                .checkpoints
                .have_rechecked(&verified_pieces)
                .map_err(DownloadError::Checkpoint)?;
        }
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
    let (
        total_blocks,
        total_bytes,
        selected_written_bytes,
        part_written_bytes,
        outstanding_request_high_water,
        last_piece,
    ) = if plans.is_empty() {
        (0, 0, 0, 0, 0, None)
    } else {
        let download = ContentSwarmDownload::new(
            config.swarm_config,
            config.max_buffered_payload_bytes,
            plans,
            ContentStorage::Selective(Box::new(storage)),
            ContentDownloadContext {
                metainfo: &metainfo,
                layout: &layout,
                resume: resume.as_ref(),
                control: &control,
            },
        )?;
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
            completed.last_piece,
        );
        let returned_storage = completed.take_storage()?;
        drop(completed);
        storage = match returned_storage {
            ContentStorage::Selective(storage) => *storage,
            ContentStorage::Single(_) => {
                return Err(DownloadError::StorageTask(
                    "selective download returned single-file storage".to_owned(),
                ));
            }
        };
        result
    };

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
    } else {
        storage
            .publish()
            .await
            .map_err(DownloadError::SelectiveStorage)?;
    }
    if !descriptor_backed && let Some(resume) = &resume {
        resume
            .checkpoints
            .published()
            .map_err(DownloadError::Checkpoint)?;
    }
    let part_slots_before_materialization = storage.part_slots();
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
    let prepared_files = if descriptor_backed {
        storage
            .finalize_descriptor_hashes()
            .await
            .map_err(DownloadError::SelectiveStorage)?
    } else {
        Vec::new()
    };
    if descriptor_backed && let Some(resume) = &resume {
        resume
            .checkpoints
            .descriptor_prepared(&prepared_files)
            .map_err(DownloadError::Checkpoint)?;
    }
    Ok(DownloadReport {
        info_hash: metainfo.info_hash,
        piece_hash: last_piece.map_or(metainfo.piece_hashes[last_wanted_piece], |piece| piece.hash),
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
        part_reopened: true,
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
    file.take((MAX_BENCODE_INPUT_LENGTH + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .map_err(|source| DownloadError::Io {
            operation: "read metainfo",
            source,
        })?;
    if bytes.len() > MAX_BENCODE_INPUT_LENGTH {
        return Err(DownloadError::MetainfoTooLarge {
            maximum: MAX_BENCODE_INPUT_LENGTH,
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
mod tests {
    use std::net::{IpAddr, SocketAddr};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use rstorrent_protocol::dht::{
        DhtEndpoint, DhtIp, Message as DhtMessage, NodeId, Query as DhtQuery, Want,
        decode_message as decode_dht, encode_response as encode_dht_response,
    };
    use rstorrent_protocol::magnet::Magnet;
    use rstorrent_protocol::metadata::{
        MetadataMessage, UT_METADATA_LOCAL_ID, encode_extension_handshake, encode_metadata_data,
        encode_metadata_reject, parse_metadata_message,
    };
    use rstorrent_protocol::metainfo::Metainfo;
    use rstorrent_protocol::peer_wire::{
        EXTENSION_PROTOCOL_RESERVED_BIT, EXTENSION_PROTOCOL_RESERVED_INDEX, HANDSHAKE_LENGTH,
        PeerMessage, decode_handshake, encode_handshake, encode_handshake_with_reserved,
        encode_message,
    };
    use rstorrent_protocol::piece::MIN_PAYLOAD_ALLOWANCE;
    use rstorrent_protocol::udp_tracker::AnnounceEvent;
    use sha1::{Digest, Sha1};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream, UdpSocket};
    use tokio::sync::{Barrier, Notify, Semaphore, mpsc};
    use tokio::time::{sleep, timeout};

    use super::{
        CLIENT_PEER_ID, CONTENT_STORAGE_WRITE_BATCH_BLOCKS, CONTENT_STORAGE_WRITE_BATCH_BYTES,
        CoalescedContentWrite, ContentDownloadConfig, ContentStorage, ContentStorageCommand,
        ContentStorageCompletion, ContentSupervisorOwner, ContentWriteStats,
        DEFAULT_ADVERTISED_PEER_PORT, DhtRetryTiming, DiskPressure, DownloadActivityEvent,
        DownloadActivitySink, DownloadConfig, DownloadControl, DownloadError,
        DownloadResourceLimits, MAX_DIAGNOSTIC_ERROR_LENGTH, MAX_METADATA_PEERS,
        MAX_RECENT_METADATA_ATTEMPTS, MagnetDownloadConfig, MetadataAcquisitionPhase,
        MetadataPeerStage, PeerConnection, PreparedContentWrite, QueuedContentStorageCommand,
        SwarmConfig, TorrentPeerCoordinator, TrackerManager, UdpTrackerAnnounce,
        UdpTrackerExchange, UdpTrackerTiming, UdpTrackerTokenCache, announce_udp_tracker_address,
        atomic_saturating_add, atomic_saturating_increment, coalesce_content_writes,
        collect_content_write_batch, content_dial_slot_available, content_storage_job_limit,
        download_magnet, download_magnet_metadata_with_control, download_magnet_metadata_with_dht,
        download_magnet_with_control, download_verified_piece,
        download_verified_piece_with_control, execute_content_storage_writes, next_peer_message,
        retrying_dht_lookup, run_content_download, run_magnet_download_with_peers, send_message,
    };
    use crate::dht::{BootstrapNode, DhtConfig, DhtService};
    use crate::network::{NetworkConfig, NetworkPolicy};
    use crate::peer::{
        DialAttempt, PeerEndpoint, PeerFailure, PeerObservation, PeerPhase, PeerRegistry,
        PeerRegistryConfig, PeerSelectionContext, PeerSelector, PeerSource,
    };
    use crate::peer_runtime::PeerConnectionLifecycle;
    use crate::selective_storage::{
        SelectiveStorageError, selective_part_path, selective_staging_path,
    };
    use crate::storage::{StagingFile, StorageError, staging_path};
    use crate::swarm::{
        BlockKey, DEFAULT_INITIAL_REQUESTS_PER_CONNECTION, DEFAULT_MAX_ESTABLISHED_CONNECTIONS,
        DEFAULT_MAX_PENDING_DIALS,
    };

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn storage_duration_counter_saturates() {
        let value = AtomicU64::new(u64::MAX - 2);
        atomic_saturating_add(&value, 3);
        assert_eq!(value.load(Ordering::Acquire), u64::MAX);
        atomic_saturating_add(&value, 1);
        assert_eq!(value.load(Ordering::Acquire), u64::MAX);

        let count = AtomicUsize::new(usize::MAX);
        atomic_saturating_increment(&count);
        assert_eq!(count.load(Ordering::Acquire), usize::MAX);
    }

    #[test]
    fn disk_pressure_uses_distinct_high_and_low_watermarks() {
        let control = DownloadControl::new();
        let limit = 4 * MIN_PAYLOAD_ALLOWANCE;
        control.configure_disk_runtime(limit);
        assert!(control.try_buffer_payload(2 * MIN_PAYLOAD_ALLOWANCE, limit));
        assert!(!control.disk_snapshot().intake_backpressured);
        assert!(control.try_buffer_payload(MIN_PAYLOAD_ALLOWANCE, limit));
        let high = control.disk_snapshot();
        assert_eq!(high.pressure, DiskPressure::Backpressured);
        assert!(high.intake_backpressured);
        assert_eq!(
            high.resident_high_watermark_bytes,
            3 * MIN_PAYLOAD_ALLOWANCE
        );
        assert_eq!(high.resident_low_watermark_bytes, 2 * MIN_PAYLOAD_ALLOWANCE);

        control.release_buffered_payload(MIN_PAYLOAD_ALLOWANCE / 2);
        assert!(control.disk_snapshot().intake_backpressured);
        control.release_buffered_payload(MIN_PAYLOAD_ALLOWANCE / 2);
        let recovered = control.disk_snapshot();
        assert_eq!(recovered.pressure, DiskPressure::Draining);
        assert!(!recovered.intake_backpressured);
        assert_eq!(recovered.pressure_transition_count, 2);
    }

    #[test]
    fn disk_piece_snapshot_counts_unique_ranges_and_retries() {
        let control = DownloadControl::new();
        control.configure_disk_runtime(8 * MIN_PAYLOAD_ALLOWANCE);
        let first = BlockKey::new(7, 0, MIN_PAYLOAD_ALLOWANCE as u32).expect("first block");
        control.disk_block_requested(first, 2 * MIN_PAYLOAD_ALLOWANCE as u32);
        control.disk_block_requested(first, 2 * MIN_PAYLOAD_ALLOWANCE as u32);
        control.disk_block_received(first, 2 * MIN_PAYLOAD_ALLOWANCE as u32);
        let active = control.disk_snapshot();
        assert_eq!(active.pieces.len(), 1);
        assert_eq!(
            active.pieces[0].requested_bytes,
            MIN_PAYLOAD_ALLOWANCE as u32
        );
        assert_eq!(
            active.pieces[0].received_bytes,
            MIN_PAYLOAD_ALLOWANCE as u32
        );
        assert_eq!(active.pieces[0].attempt, 1);

        control.disk_piece_failed(7, 2 * MIN_PAYLOAD_ALLOWANCE as u32, "piece hash failed");
        let second = BlockKey::new(
            7,
            MIN_PAYLOAD_ALLOWANCE as u32,
            MIN_PAYLOAD_ALLOWANCE as u32,
        )
        .expect("second block");
        control.disk_block_requested(second, 2 * MIN_PAYLOAD_ALLOWANCE as u32);
        let retry = control.disk_snapshot();
        assert_eq!(retry.pieces[0].attempt, 2);
        assert_eq!(
            retry.pieces[0].requested_bytes,
            MIN_PAYLOAD_ALLOWANCE as u32
        );
        assert_eq!(retry.pieces[0].received_bytes, 0);
        assert_eq!(retry.pieces[0].error, None);
    }

    fn prepared_write(piece: u32, begin: u32, bytes: &[u8]) -> PreparedContentWrite {
        PreparedContentWrite {
            block: BlockKey::new(piece, begin, bytes.len() as u32).expect("test block"),
            offset: u64::from(piece) * 1024 + u64::from(begin),
            bytes: bytes.to_vec(),
            stats: ContentWriteStats {
                selected_bytes: bytes.len(),
                part_bytes: 0,
            },
        }
    }

    fn queued_write(piece: u32, begin: u32, length: usize) -> QueuedContentStorageCommand {
        QueuedContentStorageCommand {
            enqueued_at: Instant::now(),
            command: ContentStorageCommand::Write {
                block: BlockKey::new(piece, begin, length as u32).expect("test block"),
                offset: u64::from(piece) * 1024 * 1024 + u64::from(begin),
                bytes: vec![piece as u8; length],
            },
        }
    }

    #[test]
    fn storage_write_batch_coalesces_only_exact_piece_ranges() {
        let writes = vec![
            prepared_write(0, 4, b"efgh"),
            prepared_write(1, 0, b"WXYZ"),
            prepared_write(0, 0, b"abcd"),
            prepared_write(0, 12, b"mnop"),
            prepared_write(0, 8, b"ijkl"),
        ];
        let coalesced = coalesce_content_writes(writes).expect("coalesce exact ranges");
        assert_eq!(coalesced.len(), 2);
        let CoalescedContentWrite {
            piece,
            begin,
            bytes,
            members,
            ..
        } = &coalesced[0];
        assert_eq!((*piece, *begin), (0, 0));
        assert_eq!(bytes, b"abcdefghijklmnop");
        assert_eq!(members.len(), 4);
        assert_eq!(coalesced[1].piece, 1);
        assert_eq!(coalesced[1].members.len(), 1);
    }

    #[test]
    fn storage_write_batch_rejects_overlap_and_keeps_gaps() {
        let gapped = coalesce_content_writes(vec![
            prepared_write(0, 0, b"abcd"),
            prepared_write(0, 8, b"ijkl"),
        ])
        .expect("gapped writes remain separate");
        assert_eq!(gapped.len(), 2);

        let overlap = coalesce_content_writes(vec![
            prepared_write(0, 0, b"abcdefgh"),
            prepared_write(0, 4, b"efgh"),
        ]);
        assert!(matches!(overlap, Err((_, DownloadError::StorageTask(_)))));
    }

    #[test]
    fn storage_write_batch_respects_exact_count_and_byte_caps() {
        let (sender, mut receiver) = mpsc::channel(CONTENT_STORAGE_WRITE_BATCH_BLOCKS);
        for piece in 1..=CONTENT_STORAGE_WRITE_BATCH_BLOCKS {
            sender
                .try_send(queued_write(piece as u32, 0, MIN_PAYLOAD_ALLOWANCE))
                .expect("queue test write");
        }
        let mut deferred = None;
        let batch = collect_content_write_batch(
            queued_write(0, 0, MIN_PAYLOAD_ALLOWANCE),
            &mut receiver,
            &mut deferred,
        );
        assert_eq!(batch.len(), CONTENT_STORAGE_WRITE_BATCH_BLOCKS);
        assert_eq!(
            batch
                .iter()
                .map(|queued| queued.command.write_bytes().expect("write bytes"))
                .sum::<usize>(),
            CONTENT_STORAGE_WRITE_BATCH_BYTES
        );
        assert!(deferred.is_none());
        assert_eq!(receiver.len(), 1);
    }

    #[tokio::test]
    async fn failed_coalesced_write_mutates_no_valid_prefix() {
        let output = test_path("coalesced-write-failure.bin");
        let staged = staging_path(&output).expect("staging path");
        let mut storage = ContentStorage::Single(
            StagingFile::create(output.clone(), 4)
                .await
                .expect("create staging file"),
        );
        let commands = vec![queued_write(0, 0, 4), queued_write(0, 4, 4)];

        let completions =
            execute_content_storage_writes(&mut storage, commands, &DownloadControl::new()).await;

        assert_eq!(completions.len(), 2);
        assert!(matches!(
            &completions[0],
            ContentStorageCompletion::Write {
                result: Err(DownloadError::Storage(StorageError::BlockOutOfRange { .. })),
                ..
            }
        ));
        assert_eq!(
            tokio::fs::read(&staged).await.expect("read staged file"),
            vec![0; 4]
        );
        drop(storage);
        let _ = tokio::fs::remove_file(staged).await;
    }

    fn loopback_network(timeout: Duration) -> NetworkConfig {
        NetworkConfig::new(NetworkPolicy::LoopbackOnly, timeout, timeout)
    }

    fn resource_limits(bytes: usize) -> DownloadResourceLimits {
        DownloadResourceLimits::new(bytes, bytes, bytes)
    }

    fn test_path(name: &str) -> PathBuf {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rstorrent-driver-test-{}-{sequence}-{name}",
            std::process::id()
        ))
    }

    fn test_dial_attempt() -> DialAttempt {
        let endpoint = PeerEndpoint::new("127.0.0.1:6881".parse().expect("test endpoint"))
            .expect("valid test endpoint");
        let mut registry =
            PeerRegistry::new(PeerRegistryConfig::default()).expect("test peer registry");
        registry
            .observe(
                PeerObservation::dialable(endpoint, PeerSource::Manual),
                Duration::ZERO,
            )
            .expect("test observation");
        let context = PeerSelectionContext {
            now: Duration::ZERO,
        };
        let candidate = PeerSelector
            .select(&registry, context)
            .expect("test candidate");
        registry
            .begin_dial(candidate, context)
            .expect("test dial attempt")
    }

    #[test]
    fn half_open_dials_do_not_consume_established_connection_slots() {
        let mut config = SwarmConfig::for_request_limit(MIN_PAYLOAD_ALLOWANCE);
        config.max_established_connections = 2;
        config.max_pending_dials = 2;

        assert!(content_dial_slot_available(1, 1, config, false));
        assert!(!content_dial_slot_available(1, 2, config, false));
        assert!(!content_dial_slot_available(2, 0, config, false));
        assert!(content_dial_slot_available(2, 0, config, true));
        assert!(!content_dial_slot_available(2, 1, config, true));
    }

    #[test]
    fn content_supervisor_owner_rotation_is_complete_and_stable() {
        let mut owner = ContentSupervisorOwner::Storage;
        let mut observed = Vec::new();
        for _ in 0..6 {
            observed.push(owner);
            owner = owner.next();
        }
        assert_eq!(
            observed,
            [
                ContentSupervisorOwner::Storage,
                ContentSupervisorOwner::Peer,
                ContentSupervisorOwner::Discovery,
                ContentSupervisorOwner::Storage,
                ContentSupervisorOwner::Peer,
                ContentSupervisorOwner::Discovery,
            ]
        );
    }

    #[test]
    fn product_profiles_are_generous_and_fill_every_initial_peer_window() {
        assert_eq!(
            DownloadResourceLimits::DESKTOP,
            DownloadResourceLimits::new(256 * 1024 * 1024, 32 * 1024 * 1024, 256 * 1024 * 1024)
        );
        assert_eq!(
            DownloadResourceLimits::ANDROID,
            DownloadResourceLimits::new(128 * 1024 * 1024, 16 * 1024 * 1024, 128 * 1024 * 1024)
        );
        let initial_window_bytes = DEFAULT_MAX_ESTABLISHED_CONNECTIONS
            * DEFAULT_INITIAL_REQUESTS_PER_CONNECTION
            * MIN_PAYLOAD_ALLOWANCE;
        for limits in [
            DownloadResourceLimits::DESKTOP,
            DownloadResourceLimits::ANDROID,
        ] {
            assert!(limits.max_outstanding_request_bytes >= initial_window_bytes);
            assert!(limits.max_buffered_payload_bytes >= MIN_PAYLOAD_ALLOWANCE);
            assert!(limits.max_active_piece_bytes >= initial_window_bytes);
            limits.validate().expect("valid product profile");
        }
    }

    #[test]
    fn metadata_diagnostic_history_and_error_detail_are_bounded() {
        let control = DownloadControl::new();
        control.metadata_started();
        let mut registry = PeerRegistry::new(PeerRegistryConfig::default()).expect("registry");
        for offset in 0..=MAX_RECENT_METADATA_ATTEMPTS {
            let endpoint = PeerEndpoint::new(SocketAddr::from((
                [127, 0, 0, 1],
                10_000 + u16::try_from(offset).expect("bounded port"),
            )))
            .expect("valid endpoint");
            registry
                .observe(
                    PeerObservation::dialable(endpoint, PeerSource::Tracker),
                    Duration::ZERO,
                )
                .expect("observation");
            let context = PeerSelectionContext {
                now: Duration::ZERO,
            };
            let candidate = PeerSelector
                .select(&registry, context)
                .expect("diagnostic candidate");
            let attempt = registry
                .begin_dial(candidate, context)
                .expect("diagnostic attempt");
            control.metadata_dial_started(attempt);
            control.metadata_peer_finished(
                attempt.id(),
                MetadataPeerStage::Failed,
                Some(&"x".repeat(MAX_DIAGNOSTIC_ERROR_LENGTH + 50)),
            );
            registry
                .dial_failed(attempt, Duration::ZERO, PeerFailure::Protocol)
                .expect("terminal attempt");
        }

        let snapshot = control.diagnostic_snapshot().metadata;
        assert_eq!(snapshot.recent_attempts.len(), MAX_RECENT_METADATA_ATTEMPTS);
        assert_eq!(snapshot.recent_attempts_dropped, 1);
        assert!(snapshot.active_attempts.is_empty());
        assert!(snapshot.recent_attempts.iter().all(|attempt| {
            attempt
                .terminal_detail
                .as_ref()
                .is_some_and(|detail| detail.len() == MAX_DIAGNOSTIC_ERROR_LENGTH)
        }));
    }

    #[tokio::test]
    async fn explicit_policies_gate_non_loopback_peers_and_offline_dns() {
        let public = "192.0.2.1:6881".parse().expect("documentation peer");
        let loopback = TorrentPeerCoordinator::from_endpoint(
            public,
            PeerSource::Manual,
            loopback_network(Duration::from_secs(1)),
        );
        assert!(matches!(
            loopback,
            Err(DownloadError::NetworkPolicyDenied {
                address,
                policy: NetworkPolicy::LoopbackOnly,
            }) if address == public
        ));

        let online = TorrentPeerCoordinator::from_endpoint(
            public,
            PeerSource::Manual,
            NetworkConfig::new(
                NetworkPolicy::Online,
                Duration::from_secs(1),
                Duration::from_secs(1),
            ),
        )
        .expect("online policy accepts valid public peer");
        assert_eq!(online.registry.len(), 1);

        let offline = download_magnet_metadata_with_control(
            "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213\
             &x.pe=must-not-resolve.invalid:6881"
                .to_owned(),
            NetworkConfig::new(
                NetworkPolicy::Offline,
                Duration::from_secs(1),
                Duration::from_secs(1),
            ),
            DownloadControl::new(),
        )
        .await;
        assert!(matches!(offline, Err(DownloadError::NetworkDisabled)));
    }

    #[tokio::test]
    async fn final_dial_rechecks_network_policy() {
        let public = "192.0.2.1:6881".parse().expect("documentation peer");
        let mut peers = TorrentPeerCoordinator::from_endpoint(
            public,
            PeerSource::Manual,
            NetworkConfig::new(
                NetworkPolicy::Online,
                Duration::from_secs(1),
                Duration::from_secs(1),
            ),
        )
        .expect("online peer session");
        peers.network.policy = NetworkPolicy::LoopbackOnly;

        let result = peers.connect_next([0; 20], false).await;
        assert!(matches!(
            result,
            Err(DownloadError::NetworkPolicyDenied {
                address,
                policy: NetworkPolicy::LoopbackOnly,
            }) if address == public
        ));
    }

    async fn connected_pair(io_timeout: Duration) -> (PeerConnection, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind peer message test");
        let address = listener.local_addr().expect("peer message address");
        let client = TcpStream::connect(address)
            .await
            .expect("connect peer message client");
        let (server, _) = listener.accept().await.expect("accept peer message client");
        (
            PeerConnection::for_test(test_dial_attempt(), client, io_timeout),
            server,
        )
    }

    #[tokio::test]
    async fn fragmented_bytes_cannot_extend_one_message_deadline() {
        let (mut peer, mut server) = connected_pair(Duration::from_millis(50)).await;
        let frame = encode_message(&PeerMessage::KeepAlive).expect("keepalive frame");
        let writer = tokio::spawn(async move {
            for byte in frame {
                if server.write_all(&[byte]).await.is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        });

        let result = next_peer_message(&mut peer).await;
        assert!(matches!(
            result,
            Err(DownloadError::PeerTimedOut {
                operation: "message read",
                ..
            })
        ));
        writer.await.expect("fragment writer");
    }

    #[tokio::test]
    async fn timely_messages_can_outlive_one_io_deadline() {
        let io_timeout = Duration::from_millis(150);
        let (mut peer, mut server) = connected_pair(io_timeout).await;
        let frame = encode_message(&PeerMessage::KeepAlive).expect("keepalive frame");
        let writer = tokio::spawn(async move {
            for _ in 0..4 {
                server
                    .write_all(&frame)
                    .await
                    .expect("write complete keepalive");
                tokio::time::sleep(Duration::from_millis(80)).await;
            }
        });

        for _ in 0..4 {
            assert_eq!(
                next_peer_message(&mut peer)
                    .await
                    .expect("timely complete message"),
                PeerMessage::KeepAlive
            );
        }
        writer.await.expect("timely message writer");
    }

    #[tokio::test]
    #[ignore = "uses changing public trackers and swarm state"]
    async fn live_big_buck_bunny_metadata_probe() {
        let magnet = "magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c\
&dn=Big+Buck+Bunny\
&tr=udp%3A%2F%2Fexplodie.org%3A6969\
&tr=udp%3A%2F%2Ftracker.coppersurfer.tk%3A6969\
&tr=udp%3A%2F%2Ftracker.empire-js.us%3A1337\
&tr=udp%3A%2F%2Ftracker.leechers-paradise.org%3A6969\
&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337";
        let control = DownloadControl::new();
        let activity = Arc::new(RecordingActivitySink::default());
        control.set_activity_sink(activity.clone());
        let task_control = control.clone();
        let mut task = tokio::spawn(download_magnet_metadata_with_control(
            magnet.to_owned(),
            NetworkConfig::new(
                NetworkPolicy::Online,
                Duration::from_secs(15),
                Duration::from_secs(30),
            ),
            task_control,
        ));
        let monitor_control = control.clone();
        let monitor = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(10)).await;
                let snapshot = monitor_control.diagnostic_snapshot().metadata;
                let registry = snapshot.registry.as_ref().map(|registry| registry.counts);
                eprintln!(
                    "public metadata probe: elapsed={:?} phase={:?} registry={registry:?} \
                     pending_dials={} active_workers={} attempts={} requests={} blocks={} bytes={} \
                     active={} recent={} dropped={}",
                    snapshot.captured_at,
                    snapshot.phase,
                    snapshot.pending_dials,
                    snapshot.active_workers,
                    snapshot.total_attempts,
                    snapshot.total_requests_sent,
                    snapshot.total_blocks_received,
                    snapshot.total_bytes_received,
                    snapshot.active_attempts.len(),
                    snapshot.recent_attempts.len(),
                    snapshot.recent_attempts_dropped,
                );
            }
        });

        let raw_info = match timeout(Duration::from_secs(90), &mut task).await {
            Ok(result) => {
                monitor.abort();
                let _ = monitor.await;
                let raw_info = result
                    .expect("join public metadata probe")
                    .expect("acquire public metadata");
                eprintln!(
                    "public metadata probe completed:\n{:#?}",
                    control.diagnostic_snapshot()
                );
                raw_info
            }
            Err(_) => {
                monitor.abort();
                let _ = monitor.await;
                let timeout_snapshot = control.diagnostic_snapshot();
                let events = activity
                    .events
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone();
                eprintln!("public metadata probe timeout snapshot:\n{timeout_snapshot:#?}");
                eprintln!("public metadata probe activity:\n{events:#?}");
                control.cancel();
                if timeout(Duration::from_secs(5), &mut task).await.is_err() {
                    task.abort();
                    let _ = task.await;
                }
                panic!("public metadata probe exceeded 90 seconds");
            }
        };
        let metainfo = Metainfo::from_info_bytes(&raw_info).expect("verified public metadata");
        assert_eq!(
            hex(&metainfo.info_hash),
            "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c"
        );
    }

    #[tokio::test]
    #[ignore = "uses changing public Mainline DHT and swarm state"]
    async fn live_big_buck_bunny_trackerless_dht_metadata_probe() {
        let expected_info_hash = "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c";
        let dht = DhtService::start(DhtConfig::for_network(NetworkPolicy::Online))
            .await
            .expect("start public DHT");
        let control = DownloadControl::new();
        let activity = Arc::new(RecordingActivitySink::default());
        control.set_activity_sink(activity.clone());
        let task_control = control.clone();
        let dht_handle = dht.handle();
        let mut task = tokio::spawn(async move {
            download_magnet_metadata_with_dht(
                format!("magnet:?xt=urn:btih:{expected_info_hash}"),
                NetworkConfig::new(
                    NetworkPolicy::Online,
                    Duration::from_secs(15),
                    Duration::from_secs(30),
                ),
                task_control,
                Some(dht_handle),
            )
            .await
        });
        let monitor_control = control.clone();
        let monitor = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(10)).await;
                let snapshot = monitor_control.diagnostic_snapshot().metadata;
                let registry = snapshot.registry.as_ref().map(|registry| registry.counts);
                eprintln!(
                    "public DHT metadata probe: elapsed={:?} phase={:?} registry={registry:?} \
                     pending_dials={} active_workers={} attempts={} requests={} blocks={} bytes={} \
                     active={} recent={} dropped={} last_error={:?}",
                    snapshot.captured_at,
                    snapshot.phase,
                    snapshot.pending_dials,
                    snapshot.active_workers,
                    snapshot.total_attempts,
                    snapshot.total_requests_sent,
                    snapshot.total_blocks_received,
                    snapshot.total_bytes_received,
                    snapshot.active_attempts.len(),
                    snapshot.recent_attempts.len(),
                    snapshot.recent_attempts_dropped,
                    snapshot.last_error,
                );
            }
        });

        let raw_info = match timeout(Duration::from_secs(120), &mut task).await {
            Ok(result) => {
                monitor.abort();
                let _ = monitor.await;
                let raw_info = result
                    .expect("join public DHT metadata probe")
                    .expect("acquire public DHT metadata");
                let stats = dht.handle().stats().await.ok();
                eprintln!(
                    "public DHT metadata probe completed; stats={stats:?}:\n{:#?}",
                    control.diagnostic_snapshot()
                );
                raw_info
            }
            Err(_) => {
                monitor.abort();
                let _ = monitor.await;
                let stats = dht.handle().stats().await.ok();
                let timeout_snapshot = control.diagnostic_snapshot();
                let events = activity
                    .events
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone();
                eprintln!("public DHT metadata timeout snapshot:\n{timeout_snapshot:#?}");
                eprintln!("public DHT metadata activity:\n{events:#?}");
                control.cancel();
                if timeout(Duration::from_secs(5), &mut task).await.is_err() {
                    task.abort();
                    let _ = task.await;
                }
                dht.shutdown().await.expect("DHT shutdown after timeout");
                panic!(
                    "public trackerless DHT metadata probe exceeded 120 seconds; stats={stats:?}"
                );
            }
        };
        dht.shutdown().await.expect("public DHT shutdown");
        let metainfo = Metainfo::from_info_bytes(&raw_info).expect("verified public metadata");
        assert_eq!(hex(&metainfo.info_hash), expected_info_hash);
    }

    #[test]
    fn safe_cancel_waits_for_storage_creation_boundary() {
        let control = DownloadControl::new();
        let storage_creation = control
            .enter_safe_cancel_critical()
            .expect("enter storage creation");

        control.cancel_when_safe();
        assert!(!control.is_cancelled());
        assert!(matches!(
            control.enter_safe_cancel_critical(),
            Err(DownloadError::Cancelled)
        ));

        drop(storage_creation);
        assert!(control.is_cancelled());

        let immediate = DownloadControl::new();
        immediate.cancel_when_safe();
        assert!(immediate.is_cancelled());
    }

    fn two_file_metainfo() -> Vec<u8> {
        let mut metainfo = b"d4:infod5:filesld6:lengthi1e4:pathl1:aee\
d6:lengthi32768e4:pathl1:beee4:name7:fixture12:piece lengthi32768e\
6:pieces40:"
            .to_vec();
        metainfo.extend_from_slice(&[1; 40]);
        metainfo.extend_from_slice(b"ee");
        metainfo
    }

    fn two_piece_metainfo(first: &[u8], second: &[u8]) -> Vec<u8> {
        assert_eq!(first.len(), 16 * 1024);
        assert_eq!(second.len(), 16 * 1024);
        let mut metainfo = format!(
            "d4:infod5:filesld6:lengthi{}e4:pathl1:aeed6:lengthi{}e4:pathl1:beee\
             4:name7:fixture12:piece lengthi16384e6:pieces40:",
            first.len(),
            second.len()
        )
        .into_bytes();
        metainfo.extend_from_slice(&Sha1::digest(first));
        metainfo.extend_from_slice(&Sha1::digest(second));
        metainfo.extend_from_slice(b"ee");
        metainfo
    }

    async fn serve_content_peer(
        listener: TcpListener,
        info_hash: [u8; 20],
        pieces: Arc<Vec<Vec<u8>>>,
        available: Vec<bool>,
    ) {
        serve_content_peer_with_timeout(
            listener,
            info_hash,
            pieces,
            available,
            Duration::from_secs(2),
        )
        .await;
    }

    async fn serve_content_peer_with_timeout(
        listener: TcpListener,
        info_hash: [u8; 20],
        pieces: Arc<Vec<Vec<u8>>>,
        available: Vec<bool>,
        io_timeout: Duration,
    ) {
        let (mut stream, _) = listener.accept().await.expect("accept content peer");
        let mut handshake = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake)
            .await
            .expect("read content handshake");
        decode_handshake(&handshake, info_hash).expect("valid content handshake");
        stream
            .write_all(&encode_handshake(info_hash, *b"-RS-SPLIT-0000000000"))
            .await
            .expect("send content handshake");
        let mut peer = PeerConnection::for_test(test_dial_attempt(), stream, io_timeout);
        let mut bitfield = vec![0_u8; available.len().div_ceil(8)];
        for (piece, present) in available.iter().enumerate() {
            if *present {
                bitfield[piece / 8] |= 1 << (7 - piece % 8);
            }
        }
        send_message(&mut peer, &PeerMessage::Bitfield(bitfield))
            .await
            .expect("send availability");
        send_message(&mut peer, &PeerMessage::Unchoke)
            .await
            .expect("send unchoke");
        loop {
            match next_peer_message(&mut peer).await {
                Ok(PeerMessage::Interested) => {}
                Ok(PeerMessage::Request(request)) => {
                    let piece = request.index as usize;
                    assert!(available[piece], "request sent to unavailable peer");
                    let begin = request.begin as usize;
                    let end = begin + request.length as usize;
                    send_message(
                        &mut peer,
                        &PeerMessage::Piece {
                            index: request.index,
                            begin: request.begin,
                            block: pieces[piece][begin..end].to_vec(),
                        },
                    )
                    .await
                    .expect("send content block");
                }
                Ok(PeerMessage::Cancel(_)) => {}
                Err(DownloadError::PeerClosed)
                | Err(DownloadError::Io {
                    operation: "read peer message",
                    ..
                }) => break,
                Ok(message) => panic!("unexpected content command {message:?}"),
                Err(error) => panic!("content peer failed: {error}"),
            }
        }
    }

    async fn serve_window_probe_peer(
        listener: TcpListener,
        info_hash: [u8; 20],
        payload: Arc<Vec<u8>>,
        max_pending: Arc<AtomicUsize>,
    ) {
        let (mut stream, _) = listener.accept().await.expect("accept window peer");
        let mut handshake = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake)
            .await
            .expect("read window handshake");
        decode_handshake(&handshake, info_hash).expect("valid window handshake");
        stream
            .write_all(&encode_handshake(info_hash, *b"-RS-WINDOW-000000000"))
            .await
            .expect("send window handshake");
        let mut peer =
            PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(2));
        send_message(&mut peer, &PeerMessage::Bitfield(vec![0x80]))
            .await
            .expect("send window availability");
        send_message(&mut peer, &PeerMessage::Unchoke)
            .await
            .expect("send window unchoke");

        let mut pending = Vec::new();
        while pending.len() < DEFAULT_INITIAL_REQUESTS_PER_CONNECTION {
            match next_peer_message(&mut peer).await {
                Ok(PeerMessage::Interested) => {}
                Ok(PeerMessage::Request(request)) => pending.push(request),
                Ok(PeerMessage::Cancel(_)) => {}
                Ok(message) => panic!("unexpected initial window command {message:?}"),
                Err(error) => panic!("window peer failed before initial requests: {error}"),
            }
        }
        max_pending.fetch_max(pending.len(), Ordering::AcqRel);

        let mut served_bytes = 0;
        while served_bytes < payload.len() {
            while pending.is_empty() {
                match next_peer_message(&mut peer).await {
                    Ok(PeerMessage::Request(request)) => pending.push(request),
                    Ok(PeerMessage::Interested) => {}
                    Ok(PeerMessage::Cancel(_)) => {}
                    Ok(message) => panic!("unexpected refill window command {message:?}"),
                    Err(error) => panic!("window peer failed while awaiting refill: {error}"),
                }
            }
            let request = pending.remove(0);
            let begin = request.begin as usize;
            let end = begin + request.length as usize;
            send_message(
                &mut peer,
                &PeerMessage::Piece {
                    index: request.index,
                    begin: request.begin,
                    block: payload[begin..end].to_vec(),
                },
            )
            .await
            .expect("send window payload");
            served_bytes += request.length as usize;

            loop {
                match timeout(Duration::from_millis(20), next_peer_message(&mut peer)).await {
                    Ok(Ok(PeerMessage::Request(request))) => pending.push(request),
                    Ok(Ok(PeerMessage::Interested)) => {}
                    Ok(Ok(PeerMessage::Cancel(_))) => {}
                    Ok(Err(DownloadError::PeerClosed))
                    | Ok(Err(DownloadError::Io {
                        operation: "read peer message",
                        ..
                    })) => return,
                    Ok(Ok(message)) => panic!("unexpected window command {message:?}"),
                    Ok(Err(error)) => panic!("window peer failed: {error}"),
                    Err(_) => break,
                }
            }
            max_pending.fetch_max(pending.len(), Ordering::AcqRel);
        }

        loop {
            match next_peer_message(&mut peer).await {
                Ok(PeerMessage::Request(_))
                | Ok(PeerMessage::Cancel(_))
                | Ok(PeerMessage::Interested) => {}
                Err(DownloadError::PeerClosed)
                | Err(DownloadError::Io {
                    operation: "read peer message",
                    ..
                }) => return,
                Ok(message) => panic!("unexpected final window command {message:?}"),
                Err(error) => panic!("window peer failed after queue drained: {error}"),
            }
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum AdverseRequestAction {
        Disconnect,
        Choke,
    }

    async fn serve_adverse_content_peer(
        listener: TcpListener,
        info_hash: [u8; 20],
        action: AdverseRequestAction,
    ) {
        let (mut stream, _) = listener.accept().await.expect("accept adverse peer");
        let mut handshake = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake)
            .await
            .expect("read adverse handshake");
        decode_handshake(&handshake, info_hash).expect("valid adverse handshake");
        stream
            .write_all(&encode_handshake(info_hash, *b"-RS-ADVERS-000000000"))
            .await
            .expect("send adverse handshake");
        let mut peer =
            PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(2));
        send_message(&mut peer, &PeerMessage::Bitfield(vec![0xc0]))
            .await
            .expect("send adverse availability");
        send_message(&mut peer, &PeerMessage::Unchoke)
            .await
            .expect("send adverse unchoke");
        loop {
            match next_peer_message(&mut peer).await {
                Ok(PeerMessage::Interested) => {}
                Ok(PeerMessage::Request(_)) => match action {
                    AdverseRequestAction::Disconnect => return,
                    AdverseRequestAction::Choke => {
                        send_message(&mut peer, &PeerMessage::Choke)
                            .await
                            .expect("send choke");
                        break;
                    }
                },
                Ok(message) => panic!("unexpected adverse command {message:?}"),
                Err(error) => panic!("adverse peer failed before request: {error}"),
            }
        }
        loop {
            match next_peer_message(&mut peer).await {
                Err(DownloadError::PeerClosed)
                | Err(DownloadError::Io {
                    operation: "read peer message",
                    ..
                }) => return,
                Ok(PeerMessage::Interested) => {}
                Ok(PeerMessage::Request(_)) => {
                    // Requests queued before the choke crossed the wire are harmless.
                }
                Ok(PeerMessage::Cancel(_)) => {}
                Ok(message) => panic!("choked peer received command {message:?}"),
                Err(error) => panic!("choked peer failed: {error}"),
            }
        }
    }

    async fn serve_one_block_then_choke_peer(
        listener: TcpListener,
        info_hash: [u8; 20],
        payload: Arc<Vec<u8>>,
    ) {
        let (mut stream, _) = listener.accept().await.expect("accept parole peer");
        let mut handshake = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake)
            .await
            .expect("read parole handshake");
        decode_handshake(&handshake, info_hash).expect("valid parole handshake");
        stream
            .write_all(&encode_handshake(info_hash, *b"-RS-PAROLE-000000000"))
            .await
            .expect("send parole handshake");
        let mut peer =
            PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(2));
        send_message(&mut peer, &PeerMessage::Bitfield(vec![0x80]))
            .await
            .expect("send parole availability");
        send_message(&mut peer, &PeerMessage::Unchoke)
            .await
            .expect("send parole unchoke");
        let request = loop {
            match next_peer_message(&mut peer).await {
                Ok(PeerMessage::Interested) => {}
                Ok(PeerMessage::Request(request)) => break request,
                Ok(message) => panic!("unexpected parole command {message:?}"),
                Err(error) => panic!("parole peer failed before request: {error}"),
            }
        };
        let begin = request.begin as usize;
        let end = begin + request.length as usize;
        send_message(
            &mut peer,
            &PeerMessage::Piece {
                index: request.index,
                begin: request.begin,
                block: payload[begin..end].to_vec(),
            },
        )
        .await
        .expect("send parole payload");
        send_message(&mut peer, &PeerMessage::Choke)
            .await
            .expect("send parole choke");
        loop {
            match next_peer_message(&mut peer).await {
                Ok(PeerMessage::Interested)
                | Ok(PeerMessage::Request(_))
                | Ok(PeerMessage::Cancel(_)) => {}
                Err(DownloadError::PeerClosed)
                | Err(DownloadError::Io {
                    operation: "read peer message",
                    ..
                }) => return,
                Ok(message) => panic!("unexpected post-choke command {message:?}"),
                Err(error) => panic!("parole peer failed after choke: {error}"),
            }
        }
    }

    async fn accept_handshake_without_reply(listener: TcpListener) {
        accept_handshake_without_reply_and_count(listener, None).await;
    }

    async fn accept_handshake_without_reply_and_count(
        listener: TcpListener,
        accepted: Option<Arc<AtomicUsize>>,
    ) {
        let (mut stream, _) = listener.accept().await.expect("accept silent peer");
        let mut handshake = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake)
            .await
            .expect("read silent handshake");
        if let Some(accepted) = accepted {
            accepted.fetch_add(1, Ordering::AcqRel);
        }
        let mut end = [0; 1];
        assert_eq!(stream.read(&mut end).await.expect("wait for close"), 0);
    }

    async fn serve_permanently_choked_peer(
        listener: TcpListener,
        info_hash: [u8; 20],
        bitfield: Vec<u8>,
    ) {
        let (mut stream, _) = listener.accept().await.expect("accept choked peer");
        let mut handshake = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake)
            .await
            .expect("read choked handshake");
        decode_handshake(&handshake, info_hash).expect("valid choked handshake");
        stream
            .write_all(&encode_handshake(info_hash, *b"-RS-CHOKED-000000000"))
            .await
            .expect("send choked handshake");
        let mut peer =
            PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(2));
        send_message(&mut peer, &PeerMessage::Bitfield(bitfield))
            .await
            .expect("send choked availability");
        loop {
            match next_peer_message(&mut peer).await {
                Ok(PeerMessage::Interested) => {}
                Err(DownloadError::PeerClosed)
                | Err(DownloadError::Io {
                    operation: "read peer message",
                    ..
                }) => return,
                Ok(message) => panic!("unexpected command for choked peer {message:?}"),
                Err(error) => panic!("choked peer failed: {error}"),
            }
        }
    }

    async fn prepare_endgame_peer(
        listener: TcpListener,
        info_hash: [u8; 20],
    ) -> (PeerConnection, rstorrent_protocol::peer_wire::BlockRequest) {
        let (mut stream, _) = listener.accept().await.expect("accept endgame peer");
        let mut handshake = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake)
            .await
            .expect("read endgame handshake");
        decode_handshake(&handshake, info_hash).expect("valid endgame handshake");
        stream
            .write_all(&encode_handshake(info_hash, *b"-RS-ENDGAME-00000000"))
            .await
            .expect("send endgame handshake");
        let mut peer =
            PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(2));
        send_message(&mut peer, &PeerMessage::Bitfield(vec![0x80]))
            .await
            .expect("send endgame availability");
        send_message(&mut peer, &PeerMessage::Unchoke)
            .await
            .expect("send endgame unchoke");
        let request = loop {
            match next_peer_message(&mut peer).await {
                Ok(PeerMessage::Interested) => {}
                Ok(PeerMessage::Request(request)) => break request,
                Ok(message) => panic!("unexpected endgame command {message:?}"),
                Err(error) => panic!("endgame peer failed before request: {error}"),
            }
        };
        (peer, request)
    }

    async fn serve_endgame_loser(
        listener: TcpListener,
        info_hash: [u8; 20],
        requests_ready: Arc<Barrier>,
    ) -> (
        rstorrent_protocol::peer_wire::BlockRequest,
        rstorrent_protocol::peer_wire::BlockRequest,
    ) {
        let (mut peer, request) = prepare_endgame_peer(listener, info_hash).await;
        requests_ready.wait().await;
        let cancel = match next_peer_message(&mut peer).await {
            Ok(PeerMessage::Cancel(cancel)) => cancel,
            Ok(message) => panic!("unexpected command before endgame cancel {message:?}"),
            Err(error) => panic!("endgame loser failed before cancel: {error}"),
        };
        (request, cancel)
    }

    async fn serve_endgame_winner(
        listener: TcpListener,
        info_hash: [u8; 20],
        payload: Vec<u8>,
        requests_ready: Arc<Barrier>,
    ) {
        let (mut peer, request) = prepare_endgame_peer(listener, info_hash).await;
        requests_ready.wait().await;
        let begin = request.begin as usize;
        let end = begin + request.length as usize;
        send_message(
            &mut peer,
            &PeerMessage::Piece {
                index: request.index,
                begin: request.begin,
                block: payload[begin..end].to_vec(),
            },
        )
        .await
        .expect("send winning endgame block");
        loop {
            match next_peer_message(&mut peer).await {
                Err(DownloadError::PeerClosed)
                | Err(DownloadError::Io {
                    operation: "read peer message",
                    ..
                }) => return,
                Ok(PeerMessage::Interested) => {}
                Ok(message) => panic!("unexpected post-win command {message:?}"),
                Err(error) => panic!("endgame winner failed after payload: {error}"),
            }
        }
    }

    async fn serve_delayed_block_peer(
        listener: TcpListener,
        info_hash: [u8; 20],
        payload: Vec<u8>,
        delay: Duration,
        keepalive_interval: Option<Duration>,
    ) {
        serve_delayed_block_peer_with_timeout(
            listener,
            info_hash,
            payload,
            delay,
            keepalive_interval,
            Duration::from_secs(2),
        )
        .await;
    }

    async fn serve_delayed_block_peer_with_timeout(
        listener: TcpListener,
        info_hash: [u8; 20],
        payload: Vec<u8>,
        delay: Duration,
        keepalive_interval: Option<Duration>,
        io_timeout: Duration,
    ) {
        let (mut stream, _) = listener.accept().await.expect("accept delayed peer");
        let mut handshake = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake)
            .await
            .expect("read delayed handshake");
        decode_handshake(&handshake, info_hash).expect("valid delayed handshake");
        stream
            .write_all(&encode_handshake(info_hash, *b"-RS-DELAY--000000000"))
            .await
            .expect("send delayed handshake");
        let mut peer = PeerConnection::for_test(test_dial_attempt(), stream, io_timeout);
        send_message(&mut peer, &PeerMessage::Bitfield(vec![0x80]))
            .await
            .expect("send delayed availability");
        send_message(&mut peer, &PeerMessage::Unchoke)
            .await
            .expect("send delayed unchoke");
        let request = loop {
            match next_peer_message(&mut peer).await {
                Ok(PeerMessage::Interested) => {}
                Ok(PeerMessage::Request(request)) => break request,
                Ok(message) => panic!("unexpected delayed command {message:?}"),
                Err(error) => panic!("delayed peer failed before request: {error}"),
            }
        };
        let started = tokio::time::Instant::now();
        if let Some(interval) = keepalive_interval {
            while started.elapsed().saturating_add(interval) < delay {
                tokio::time::sleep(interval).await;
                if send_message(&mut peer, &PeerMessage::KeepAlive)
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
        tokio::time::sleep(delay.saturating_sub(started.elapsed())).await;
        let begin = request.begin as usize;
        let end = begin + request.length as usize;
        if send_message(
            &mut peer,
            &PeerMessage::Piece {
                index: request.index,
                begin: request.begin,
                block: payload[begin..end].to_vec(),
            },
        )
        .await
        .is_err()
        {
            return;
        }
        loop {
            match next_peer_message(&mut peer).await {
                Err(DownloadError::PeerClosed)
                | Err(DownloadError::Io {
                    operation: "read peer message",
                    ..
                }) => return,
                Ok(PeerMessage::Request(_))
                | Ok(PeerMessage::Cancel(_))
                | Ok(PeerMessage::Interested) => {}
                Ok(message) => panic!("unexpected post-payload command {message:?}"),
                Err(error) => panic!("delayed peer failed after payload: {error}"),
            }
        }
    }

    async fn run_adverse_reassignment_case(action: AdverseRequestAction) {
        let first = vec![0x44; 16 * 1024];
        let second = vec![0x99; 16 * 1024];
        let metainfo =
            Metainfo::from_bytes(&two_piece_metainfo(&first, &second)).expect("two-piece metainfo");
        let payload = Arc::new(vec![first, second]);
        let adverse_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind adverse");
        let adverse_address = adverse_listener.local_addr().expect("adverse address");
        let useful_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind useful");
        let useful_address = useful_listener.local_addr().expect("useful address");
        let adverse = tokio::spawn(serve_adverse_content_peer(
            adverse_listener,
            metainfo.info_hash,
            action,
        ));
        let useful = tokio::spawn(serve_content_peer(
            useful_listener,
            metainfo.info_hash,
            payload,
            vec![true, true],
        ));
        let output = test_path(match action {
            AdverseRequestAction::Disconnect => "disconnect-reassignment",
            AdverseRequestAction::Choke => "choke-reassignment",
        });
        let mut peers = TorrentPeerCoordinator::from_endpoint(
            adverse_address,
            PeerSource::Manual,
            loopback_network(Duration::from_secs(2)),
        )
        .expect("peer session");
        peers
            .observe_address(useful_address, PeerSource::Manual)
            .expect("useful peer");
        let report = timeout(
            Duration::from_secs(3),
            run_content_download(
                ContentDownloadConfig {
                    output_path: output.clone(),
                    max_buffered_payload_bytes: 2 * MIN_PAYLOAD_ALLOWANCE,
                    swarm_config: SwarmConfig::for_request_limit(2 * MIN_PAYLOAD_ALLOWANCE),
                    skip_files: Vec::new(),
                    materialize_files: Vec::new(),
                },
                metainfo,
                DownloadControl::new(),
                None,
                &mut peers,
                None,
            ),
        )
        .await
        .expect("bounded reassignment")
        .expect("reassigned download");
        assert_eq!(report.verified_piece_count, 2);
        timeout(Duration::from_secs(1), adverse)
            .await
            .expect("adverse peer joined")
            .expect("adverse peer task");
        timeout(Duration::from_secs(1), useful)
            .await
            .expect("useful peer joined")
            .expect("useful peer task");
        let _ = tokio::fs::remove_dir_all(output).await;
    }

    #[tokio::test]
    async fn multi_piece_single_file_uses_torrent_offsets_and_publishes() {
        let payload = (0..(3 * 16 * 1024 + 731))
            .map(|index| ((index * 47 + index / 19) & 0xff) as u8)
            .collect::<Vec<_>>();
        let info = single_file_info_with_piece_length(&payload, 32 * 1024);
        let metainfo = Metainfo::from_info_bytes(&info).expect("multi-piece single-file metainfo");
        let pieces = payload
            .chunks(32 * 1024)
            .map(<[u8]>::to_vec)
            .collect::<Vec<_>>();
        assert_eq!(pieces.len(), 2);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind multi-piece peer");
        let address = listener.local_addr().expect("multi-piece peer address");
        let peer_task = tokio::spawn(serve_content_peer(
            listener,
            metainfo.info_hash,
            Arc::new(pieces),
            vec![true; metainfo.piece_count()],
        ));
        let output = test_path("multi-piece-single-file.bin");
        let mut peers = TorrentPeerCoordinator::from_endpoint(
            address,
            PeerSource::Manual,
            loopback_network(Duration::from_secs(2)),
        )
        .expect("peer session");

        let report = timeout(
            Duration::from_secs(3),
            run_content_download(
                ContentDownloadConfig {
                    output_path: output.clone(),
                    max_buffered_payload_bytes: 2 * MIN_PAYLOAD_ALLOWANCE,
                    swarm_config: SwarmConfig::for_request_limit(2 * MIN_PAYLOAD_ALLOWANCE),
                    skip_files: Vec::new(),
                    materialize_files: Vec::new(),
                },
                metainfo,
                DownloadControl::new(),
                None,
                &mut peers,
                None,
            ),
        )
        .await
        .expect("bounded multi-piece single-file download")
        .expect("multi-piece single-file completion");

        assert_eq!(report.piece_count, 2);
        assert_eq!(report.verified_piece_count, 2);
        assert_eq!(report.bytes_written, payload.len());
        assert_eq!(report.selected_written_bytes, payload.len());
        assert_eq!(
            tokio::fs::read(&output).await.expect("published file"),
            payload
        );
        timeout(Duration::from_secs(1), peer_task)
            .await
            .expect("multi-piece peer joined")
            .expect("multi-piece peer task");
        let _ = tokio::fs::remove_file(output).await;
    }

    #[tokio::test]
    async fn capable_peer_grows_pipeline_beyond_initial_request_window() {
        let payload = Arc::new(
            (0..(32 * MIN_PAYLOAD_ALLOWANCE))
                .map(|index| ((index * 31 + index / 23) & 0xff) as u8)
                .collect::<Vec<_>>(),
        );
        let info = single_file_info_with_piece_length(&payload, payload.len());
        let metainfo = Metainfo::from_info_bytes(&info).expect("window probe metainfo");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind window probe peer");
        let address = listener.local_addr().expect("window probe address");
        let max_pending = Arc::new(AtomicUsize::new(0));
        let peer_task = tokio::spawn(serve_window_probe_peer(
            listener,
            metainfo.info_hash,
            payload.clone(),
            max_pending.clone(),
        ));
        let output = test_path("adaptive-window.bin");
        let mut peers = TorrentPeerCoordinator::from_endpoint(
            address,
            PeerSource::Manual,
            loopback_network(Duration::from_secs(2)),
        )
        .expect("peer session");
        let control = DownloadControl::new();
        let payload_limit = payload.len();

        let report = timeout(
            Duration::from_secs(5),
            run_content_download(
                ContentDownloadConfig {
                    output_path: output.clone(),
                    max_buffered_payload_bytes: payload_limit,
                    swarm_config: SwarmConfig::for_request_limit(payload_limit),
                    skip_files: Vec::new(),
                    materialize_files: Vec::new(),
                },
                metainfo,
                control.clone(),
                None,
                &mut peers,
                None,
            ),
        )
        .await
        .expect("bounded window probe")
        .expect("window probe completion");

        assert_eq!(report.verified_piece_count, 1);
        assert_eq!(report.bytes_written, payload.len());
        assert!(
            max_pending.load(Ordering::Acquire) > DEFAULT_INITIAL_REQUESTS_PER_CONNECTION,
            "peer never observed the request window grow"
        );
        assert!(
            report.outstanding_request_high_water
                > DEFAULT_INITIAL_REQUESTS_PER_CONNECTION * MIN_PAYLOAD_ALLOWANCE
        );
        assert!(report.outstanding_request_high_water <= payload_limit);
        assert!(report.payload_high_water <= payload_limit);
        let swarm = control
            .diagnostic_snapshot()
            .swarm
            .expect("window diagnostics");
        assert!(swarm.request_target_max > DEFAULT_INITIAL_REQUESTS_PER_CONNECTION);
        assert_eq!(swarm.useful_payload_bytes, payload.len());
        assert_eq!(tokio::fs::read(&output).await.expect("output"), *payload);
        timeout(Duration::from_secs(1), peer_task)
            .await
            .expect("window peer joined")
            .expect("window peer task");
        let _ = tokio::fs::remove_file(output).await;
    }

    #[tokio::test]
    async fn request_pipeline_exceeds_independently_bounded_resident_payload() {
        let payload = Arc::new(
            (0..(8 * MIN_PAYLOAD_ALLOWANCE))
                .map(|index| ((index * 17 + index / 11) & 0xff) as u8)
                .collect::<Vec<_>>(),
        );
        let info = single_file_info_with_piece_length(&payload, payload.len());
        let metainfo = Metainfo::from_info_bytes(&info).expect("resource split metainfo");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind resource split peer");
        let address = listener.local_addr().expect("resource split address");
        let max_pending = Arc::new(AtomicUsize::new(0));
        let peer_task = tokio::spawn(serve_window_probe_peer(
            listener,
            metainfo.info_hash,
            payload.clone(),
            max_pending,
        ));
        let output = test_path("independent-resource-budgets.bin");
        let mut peers = TorrentPeerCoordinator::from_endpoint(
            address,
            PeerSource::Manual,
            loopback_network(Duration::from_secs(2)),
        )
        .expect("peer session");
        let control = DownloadControl::new();
        control.set_storage_write_delay(Duration::from_millis(25));

        let report = timeout(
            Duration::from_secs(5),
            run_content_download(
                ContentDownloadConfig {
                    output_path: output.clone(),
                    max_buffered_payload_bytes: 2 * MIN_PAYLOAD_ALLOWANCE,
                    swarm_config: SwarmConfig::for_request_limit(8 * MIN_PAYLOAD_ALLOWANCE),
                    skip_files: Vec::new(),
                    materialize_files: Vec::new(),
                },
                metainfo,
                control.clone(),
                None,
                &mut peers,
                None,
            ),
        )
        .await
        .expect("resource split deadline")
        .expect("resource split completion");

        assert_eq!(report.verified_piece_count, 1);
        assert!(report.payload_high_water <= 2 * MIN_PAYLOAD_ALLOWANCE);
        assert!(report.outstanding_request_high_water >= 4 * MIN_PAYLOAD_ALLOWANCE);
        assert!(report.outstanding_request_high_water > report.payload_high_water);
        assert_eq!(control.snapshot().buffered_payload_bytes, 0);
        timeout(Duration::from_secs(1), peer_task)
            .await
            .expect("resource split peer joined")
            .expect("resource split peer task");
        let _ = tokio::fs::remove_file(output).await;
    }

    #[tokio::test]
    async fn multi_peer_split_availability_completes_and_joins_every_socket() {
        let first = vec![0x31; 16 * 1024];
        let second = vec![0x72; 16 * 1024];
        let metainfo =
            Metainfo::from_bytes(&two_piece_metainfo(&first, &second)).expect("two-piece metainfo");
        let peers_payload = Arc::new(vec![first.clone(), second.clone()]);
        let first_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind A");
        let first_address = first_listener.local_addr().expect("address A");
        let second_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind B");
        let second_address = second_listener.local_addr().expect("address B");
        let peer_a = tokio::spawn(serve_content_peer(
            first_listener,
            metainfo.info_hash,
            peers_payload.clone(),
            vec![true, false],
        ));
        let peer_b = tokio::spawn(serve_content_peer(
            second_listener,
            metainfo.info_hash,
            peers_payload,
            vec![false, true],
        ));

        let control = DownloadControl::new();
        let output = test_path("split-availability");
        let mut peers = TorrentPeerCoordinator::from_endpoint(
            first_address,
            PeerSource::Manual,
            loopback_network(Duration::from_secs(2)),
        )
        .expect("peer session");
        peers
            .observe_address(second_address, PeerSource::Manual)
            .expect("second peer");
        let report = timeout(
            Duration::from_secs(3),
            run_content_download(
                ContentDownloadConfig {
                    output_path: output.clone(),
                    max_buffered_payload_bytes: 2 * MIN_PAYLOAD_ALLOWANCE,
                    swarm_config: SwarmConfig::for_request_limit(2 * MIN_PAYLOAD_ALLOWANCE),
                    skip_files: Vec::new(),
                    materialize_files: Vec::new(),
                },
                metainfo,
                control,
                None,
                &mut peers,
                None,
            ),
        )
        .await
        .expect("bounded multi-peer download")
        .expect("multi-peer download");

        assert_eq!(report.block_count, 2);
        assert_eq!(report.verified_piece_count, 2);
        assert!(report.payload_high_water <= 2 * MIN_PAYLOAD_ALLOWANCE);
        assert_eq!(
            tokio::fs::read(output.join("a")).await.expect("file A"),
            first
        );
        assert_eq!(
            tokio::fs::read(output.join("b")).await.expect("file B"),
            second
        );
        timeout(Duration::from_secs(1), peer_a)
            .await
            .expect("peer A joined")
            .expect("peer A task");
        timeout(Duration::from_secs(1), peer_b)
            .await
            .expect("peer B joined")
            .expect("peer B task");
        let _ = tokio::fs::remove_dir_all(output).await;
    }

    #[tokio::test]
    async fn slow_storage_preserves_multi_peer_resident_payload_bound() {
        let first = vec![0x29; 16 * 1024];
        let second = vec![0xe3; 16 * 1024];
        let metainfo =
            Metainfo::from_bytes(&two_piece_metainfo(&first, &second)).expect("two-piece metainfo");
        let payload = Arc::new(vec![first, second]);
        let mut addresses = Vec::new();
        let mut tasks = Vec::new();
        for _ in 0..2 {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind storage-pressure peer");
            addresses.push(listener.local_addr().expect("storage-pressure address"));
            tasks.push(tokio::spawn(serve_content_peer(
                listener,
                metainfo.info_hash,
                payload.clone(),
                vec![true, true],
            )));
        }
        let mut peers = TorrentPeerCoordinator::from_endpoint(
            addresses[0],
            PeerSource::Manual,
            loopback_network(Duration::from_secs(2)),
        )
        .expect("peer session");
        peers
            .observe_address(addresses[1], PeerSource::Manual)
            .expect("second peer");
        let control = DownloadControl::new();
        control.set_storage_write_delay(Duration::from_millis(250));
        let task_control = control.clone();
        let output = test_path("slow-storage-multi-peer");
        let task_output = output.clone();
        let mut download = tokio::spawn(async move {
            run_content_download(
                ContentDownloadConfig {
                    output_path: task_output,
                    max_buffered_payload_bytes: 2 * MIN_PAYLOAD_ALLOWANCE,
                    swarm_config: SwarmConfig::for_request_limit(2 * MIN_PAYLOAD_ALLOWANCE),
                    skip_files: Vec::new(),
                    materialize_files: Vec::new(),
                },
                metainfo,
                task_control,
                None,
                &mut peers,
                None,
            )
            .await
        });

        timeout(Duration::from_secs(2), async {
            loop {
                if control.snapshot().received_bytes >= MIN_PAYLOAD_ALLOWANCE {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("first payload reached the supervisor");
        timeout(Duration::from_millis(100), async {
            loop {
                if control.snapshot().received_bytes >= 2 * MIN_PAYLOAD_ALLOWANCE {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("second peer progressed while the first storage write was delayed");
        let active = control.snapshot();
        assert!(active.storage_active_write_micros.is_some());
        assert_eq!(active.storage_active_hash_micros, None);
        assert_eq!(active.storage_write_operations_started, 1);
        assert_eq!(active.storage_write_operations_completed, 0);

        let report = timeout(Duration::from_secs(3), &mut download)
            .await
            .expect("bounded slow-storage download")
            .expect("download task")
            .expect("slow-storage completion");

        assert_eq!(report.verified_piece_count, 2);
        assert!(report.payload_high_water <= 2 * MIN_PAYLOAD_ALLOWANCE);
        let progress = control.snapshot();
        assert_eq!(progress.storage_jobs_pending, 0);
        assert!(progress.storage_jobs_high_water >= 2);
        let job_limit = content_storage_job_limit(2 * MIN_PAYLOAD_ALLOWANCE);
        assert!(progress.storage_command_queue_high_water <= job_limit);
        assert!(progress.storage_completion_queue_high_water <= job_limit);
        assert!((1..=2).contains(&progress.storage_write_operations_started));
        assert_eq!(
            progress.storage_write_operations_started,
            progress.storage_write_operations_completed
        );
        assert_eq!(progress.storage_write_blocks_started, 2);
        assert_eq!(progress.storage_write_blocks_completed, 2);
        assert!((1..=2).contains(&progress.storage_write_batch_blocks_high_water));
        assert!(
            (MIN_PAYLOAD_ALLOWANCE..=2 * MIN_PAYLOAD_ALLOWANCE)
                .contains(&progress.storage_write_batch_bytes_high_water)
        );
        assert!(
            progress.storage_write_service_micros
                >= progress.storage_write_operations_started as u64 * 200_000
        );
        assert!(progress.storage_write_service_max_micros >= 200_000);
        assert_eq!(progress.storage_hash_operations_started, 2);
        assert_eq!(progress.storage_hash_operations_completed, 2);
        assert_eq!(progress.storage_active_write_micros, None);
        assert_eq!(progress.storage_active_hash_micros, None);
        for task in tasks {
            timeout(Duration::from_secs(1), task)
                .await
                .expect("storage-pressure peer joined")
                .expect("storage-pressure peer task");
        }
        let _ = tokio::fs::remove_dir_all(output).await;
    }

    #[tokio::test]
    async fn cancellation_joins_storage_with_queued_writes() {
        let payload = (0..(2 * MIN_PAYLOAD_ALLOWANCE))
            .map(|index| ((index * 17 + index / 13) & 0xff) as u8)
            .collect::<Vec<_>>();
        let payload_len = payload.len();
        let metainfo =
            Metainfo::from_info_bytes(&single_file_info_with_piece_length(&payload, payload.len()))
                .expect("queued-write metainfo");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind queued-write peer");
        let address = listener.local_addr().expect("queued-write address");
        let peer_task = tokio::spawn(serve_content_peer(
            listener,
            metainfo.info_hash,
            Arc::new(vec![payload.clone()]),
            vec![true],
        ));
        let mut peers = TorrentPeerCoordinator::from_endpoint(
            address,
            PeerSource::Manual,
            loopback_network(Duration::from_secs(2)),
        )
        .expect("peer session");
        let control = DownloadControl::new();
        control.set_storage_write_delay(Duration::from_millis(250));
        let task_control = control.clone();
        let output = test_path("cancel-queued-storage.bin");
        let task_output = output.clone();
        let mut download = tokio::spawn(async move {
            run_content_download(
                ContentDownloadConfig {
                    output_path: task_output,
                    max_buffered_payload_bytes: payload_len,
                    swarm_config: SwarmConfig::for_request_limit(payload_len),
                    skip_files: Vec::new(),
                    materialize_files: Vec::new(),
                },
                metainfo,
                task_control,
                None,
                &mut peers,
                None,
            )
            .await
        });

        timeout(Duration::from_secs(2), async {
            loop {
                let progress = control.snapshot();
                if progress.received_bytes == payload_len && progress.storage_jobs_pending >= 2 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("two writes entered bounded storage ownership");
        let active = control.snapshot();
        assert!(active.storage_active_write_micros.is_some());
        assert_eq!(active.storage_active_hash_micros, None);
        assert_eq!(active.storage_write_operations_started, 1);
        assert_eq!(active.storage_write_operations_completed, 0);
        control.cancel();
        let result = timeout(Duration::from_secs(1), &mut download)
            .await
            .expect("storage owner joined after queued-write cancellation")
            .expect("download task");
        assert!(matches!(result, Err(DownloadError::Cancelled)));
        let progress = control.snapshot();
        assert_eq!(progress.buffered_payload_bytes, 0);
        assert_eq!(progress.storage_jobs_pending, 0);
        assert_eq!(progress.storage_write_operations_started, 1);
        assert_eq!(progress.storage_write_operations_completed, 1);
        assert!((1..=2).contains(&progress.storage_write_blocks_started));
        assert_eq!(
            progress.storage_write_blocks_started,
            progress.storage_write_blocks_completed
        );
        assert_eq!(
            progress.storage_write_batch_blocks_high_water,
            progress.storage_write_blocks_started
        );
        assert!(progress.storage_write_service_micros >= 200_000);
        assert_eq!(progress.storage_hash_operations_started, 0);
        assert_eq!(progress.storage_active_write_micros, None);
        assert_eq!(progress.storage_active_hash_micros, None);
        assert!(!output.exists());
        timeout(Duration::from_secs(1), peer_task)
            .await
            .expect("queued-write peer joined")
            .expect("queued-write peer task");
        let _ = tokio::fs::remove_file(staging_path(&output).expect("staging path")).await;
    }

    #[tokio::test]
    async fn cancellation_joins_storage_during_piece_hash() {
        let payload = vec![0x4d; MIN_PAYLOAD_ALLOWANCE];
        let metainfo =
            Metainfo::from_info_bytes(&single_file_info(&payload)).expect("hash-cancel metainfo");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind hash-cancel peer");
        let address = listener.local_addr().expect("hash-cancel address");
        let peer_task = tokio::spawn(serve_content_peer(
            listener,
            metainfo.info_hash,
            Arc::new(vec![payload]),
            vec![true],
        ));
        let mut peers = TorrentPeerCoordinator::from_endpoint(
            address,
            PeerSource::Manual,
            loopback_network(Duration::from_secs(2)),
        )
        .expect("peer session");
        let control = DownloadControl::new();
        control.set_storage_hash_delay(Duration::from_millis(250));
        let task_control = control.clone();
        let output = test_path("cancel-storage-hash.bin");
        let task_output = output.clone();
        let mut download = tokio::spawn(async move {
            run_content_download(
                ContentDownloadConfig {
                    output_path: task_output,
                    max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
                    swarm_config: SwarmConfig::for_request_limit(MIN_PAYLOAD_ALLOWANCE),
                    skip_files: Vec::new(),
                    materialize_files: Vec::new(),
                },
                metainfo,
                task_control,
                None,
                &mut peers,
                None,
            )
            .await
        });

        timeout(Duration::from_secs(2), async {
            loop {
                if control.snapshot().storage_hashes_started == 1 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("piece hash entered storage owner");
        let active = control.snapshot();
        assert_eq!(active.storage_active_write_micros, None);
        assert!(active.storage_active_hash_micros.is_some());
        assert_eq!(active.storage_hash_operations_started, 1);
        assert_eq!(active.storage_hash_operations_completed, 0);
        control.cancel();
        let result = timeout(Duration::from_secs(1), &mut download)
            .await
            .expect("storage owner joined after hash cancellation")
            .expect("download task");
        assert!(matches!(result, Err(DownloadError::Cancelled)));
        let progress = control.snapshot();
        assert_eq!(progress.buffered_payload_bytes, 0);
        assert_eq!(progress.storage_jobs_pending, 0);
        assert_eq!(progress.storage_hash_operations_started, 1);
        assert_eq!(progress.storage_hash_operations_completed, 1);
        assert!(progress.storage_hash_service_micros >= 200_000);
        assert_eq!(progress.storage_active_write_micros, None);
        assert_eq!(progress.storage_active_hash_micros, None);
        assert!(!output.exists());
        timeout(Duration::from_secs(1), peer_task)
            .await
            .expect("hash-cancel peer joined")
            .expect("hash-cancel peer task");
        let _ = tokio::fs::remove_file(staging_path(&output).expect("staging path")).await;
    }

    #[tokio::test]
    async fn storage_command_backpressure_is_bounded_and_completes() {
        let payload = (0..(80 * MIN_PAYLOAD_ALLOWANCE))
            .map(|index| ((index * 23 + index / 29) & 0xff) as u8)
            .collect::<Vec<_>>();
        let metainfo =
            Metainfo::from_info_bytes(&single_file_info_with_piece_length(&payload, payload.len()))
                .expect("storage-pressure metainfo");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind storage-pressure peer");
        let address = listener.local_addr().expect("storage-pressure address");
        let peer_task = tokio::spawn(serve_content_peer(
            listener,
            metainfo.info_hash,
            Arc::new(vec![payload.clone()]),
            vec![true],
        ));
        let mut peers = TorrentPeerCoordinator::from_endpoint(
            address,
            PeerSource::Manual,
            loopback_network(Duration::from_secs(2)),
        )
        .expect("peer session");
        let control = DownloadControl::new();
        control.set_storage_write_delay(Duration::from_millis(3));
        let output = test_path("storage-command-pressure.bin");
        let job_limit = content_storage_job_limit(payload.len());

        let report = timeout(
            Duration::from_secs(5),
            run_content_download(
                ContentDownloadConfig {
                    output_path: output.clone(),
                    max_buffered_payload_bytes: payload.len(),
                    swarm_config: SwarmConfig::for_request_limit(payload.len()),
                    skip_files: Vec::new(),
                    materialize_files: Vec::new(),
                },
                metainfo,
                control.clone(),
                None,
                &mut peers,
                None,
            ),
        )
        .await
        .expect("bounded storage-pressure deadline")
        .expect("storage-pressure completion");

        assert_eq!(report.verified_piece_count, 1);
        assert_eq!(tokio::fs::read(&output).await.expect("output"), payload);
        let progress = control.snapshot();
        assert_eq!(progress.storage_jobs_pending, 0);
        assert!(progress.storage_jobs_high_water <= job_limit);
        assert!(progress.storage_jobs_high_water > CONTENT_STORAGE_WRITE_BATCH_BLOCKS);
        assert!(progress.storage_command_queue_high_water <= job_limit);
        assert!(progress.storage_command_queue_high_water > 0);
        assert!(progress.storage_completion_queue_high_water <= job_limit);
        assert_eq!(progress.storage_write_blocks_started, 80);
        assert_eq!(progress.storage_write_blocks_completed, 80);
        assert!(progress.storage_write_operations_started < 80);
        assert_eq!(
            progress.storage_write_operations_started,
            progress.storage_write_operations_completed
        );
        assert!(progress.storage_write_batch_blocks_high_water > 1);
        assert!(
            progress.storage_write_batch_blocks_high_water <= CONTENT_STORAGE_WRITE_BATCH_BLOCKS
        );
        assert!(progress.storage_write_batch_bytes_high_water <= CONTENT_STORAGE_WRITE_BATCH_BYTES);
        timeout(Duration::from_secs(1), peer_task)
            .await
            .expect("storage-pressure peer joined")
            .expect("storage-pressure peer task");
        let _ = tokio::fs::remove_file(output).await;
    }

    #[tokio::test]
    async fn storage_pressure_cannot_starve_dht_intake_or_dial_refill() {
        let payload = (0..(80 * MIN_PAYLOAD_ALLOWANCE))
            .map(|index| ((index * 31 + index / 37) & 0xff) as u8)
            .collect::<Vec<_>>();
        let metainfo =
            Metainfo::from_info_bytes(&single_file_info_with_piece_length(&payload, payload.len()))
                .expect("storage-pressure metainfo");
        let info_hash = metainfo.info_hash;
        let initial_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind initial peer");
        let initial_address = initial_listener.local_addr().expect("initial address");
        let initial_task = tokio::spawn(serve_content_peer(
            initial_listener,
            info_hash,
            Arc::new(vec![payload.clone()]),
            vec![true],
        ));
        let discovered_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind discovered peer");
        let discovered_address = discovered_listener
            .local_addr()
            .expect("discovered address");
        let discovered_task = tokio::spawn(serve_permanently_choked_peer(
            discovered_listener,
            info_hash,
            vec![0x80],
        ));
        let dht_socket = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind scripted DHT");
        let dht_address = dht_socket.local_addr().expect("DHT address");
        let release_dht = Arc::new(Notify::new());
        let dht_task = tokio::spawn(serve_dht_peer_after_signal(
            dht_socket,
            info_hash,
            discovered_address,
            release_dht.clone(),
        ));
        let dht = DhtService::start(dht_config(dht_address))
            .await
            .expect("start DHT client");
        let mut peers = TorrentPeerCoordinator::from_endpoint(
            initial_address,
            PeerSource::Manual,
            loopback_network(Duration::from_secs(2)),
        )
        .expect("peer session");
        peers.dht = Some(dht.handle());
        let control = DownloadControl::new();
        control.set_storage_write_delay(Duration::from_millis(5));
        let task_control = control.clone();
        let payload_limit = payload.len();
        let job_limit = content_storage_job_limit(payload_limit);
        let output = test_path("storage-pressure-dht-intake.bin");
        let task_output = output.clone();
        let download = tokio::spawn(async move {
            let result = run_content_download(
                ContentDownloadConfig {
                    output_path: task_output,
                    max_buffered_payload_bytes: payload_limit,
                    swarm_config: SwarmConfig::for_request_limit(payload_limit),
                    skip_files: Vec::new(),
                    materialize_files: Vec::new(),
                },
                metainfo,
                task_control,
                None,
                &mut peers,
                None,
            )
            .await;
            (result, peers)
        });

        timeout(Duration::from_secs(2), async {
            loop {
                let disk = control.disk_snapshot();
                if disk.intake_backpressured {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("storage queue saturated");
        release_dht.notify_one();
        let intake_progress = timeout(Duration::from_millis(300), async {
            loop {
                let diagnostics = control.diagnostic_snapshot();
                if diagnostics.content_registry.is_some_and(|registry| {
                    registry.total >= 2 && registry.dialing + registry.connected >= 2
                }) {
                    break diagnostics.progress;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("DHT peer entered registry and dial cohort during storage pressure");
        assert!(intake_progress.storage_jobs_pending > 0);

        let (result, peers) = timeout(Duration::from_secs(5), download)
            .await
            .expect("storage-pressure download joined")
            .expect("download task");
        let report = result.expect("storage-pressure download");
        assert_eq!(report.verified_piece_count, 1);
        assert_eq!(
            tokio::fs::read(&output).await.expect("published output"),
            payload
        );
        let progress = control.snapshot();
        assert_eq!(progress.storage_jobs_pending, 0);
        assert!(progress.storage_jobs_high_water <= job_limit);
        assert!(progress.storage_jobs_high_water > 0);
        assert!(progress.storage_command_queue_high_water <= job_limit);
        assert!(progress.storage_command_queue_high_water > 0);
        let disk = control.disk_snapshot();
        assert_eq!(disk.pressure, DiskPressure::Idle);
        assert!(!disk.intake_backpressured);
        assert!(disk.pressure_transition_count >= 2);
        let discovered = peers
            .registry
            .find_endpoint(PeerEndpoint::new(discovered_address).expect("DHT endpoint"))
            .expect("DHT peer retained");
        assert!(discovered.sources().contains(PeerSource::Dht));
        assert!(discovered.history().dial_attempts >= 1);
        for task in [initial_task, discovered_task, dht_task] {
            timeout(Duration::from_secs(1), task)
                .await
                .expect("scripted owner joined")
                .expect("scripted task");
        }
        dht.shutdown().await.expect("DHT shutdown");
        let _ = tokio::fs::remove_file(output).await;
    }

    #[tokio::test]
    async fn endgame_cancel_reaches_loser_before_slow_storage_completes() {
        let payload = vec![0x7d; MIN_PAYLOAD_ALLOWANCE];
        let metainfo =
            Metainfo::from_info_bytes(&single_file_info(&payload)).expect("endgame metainfo");
        let loser_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind endgame loser");
        let loser_address = loser_listener.local_addr().expect("loser address");
        let winner_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind endgame winner");
        let winner_address = winner_listener.local_addr().expect("winner address");
        let requests_ready = Arc::new(Barrier::new(2));
        let loser = tokio::spawn(serve_endgame_loser(
            loser_listener,
            metainfo.info_hash,
            requests_ready.clone(),
        ));
        let winner = tokio::spawn(serve_endgame_winner(
            winner_listener,
            metainfo.info_hash,
            payload.clone(),
            requests_ready,
        ));
        let mut peers = TorrentPeerCoordinator::from_endpoint(
            loser_address,
            PeerSource::Manual,
            loopback_network(Duration::from_secs(2)),
        )
        .expect("peer session");
        peers
            .observe_address(winner_address, PeerSource::Manual)
            .expect("winner peer");
        let control = DownloadControl::new();
        control.set_storage_write_delay(Duration::from_millis(250));
        let task_control = control.clone();
        let output = test_path("endgame-cancel.bin");
        let task_output = output.clone();
        let mut download = tokio::spawn(async move {
            run_content_download(
                ContentDownloadConfig {
                    output_path: task_output,
                    max_buffered_payload_bytes: 2 * MIN_PAYLOAD_ALLOWANCE,
                    swarm_config: SwarmConfig::for_request_limit(2 * MIN_PAYLOAD_ALLOWANCE),
                    skip_files: Vec::new(),
                    materialize_files: Vec::new(),
                },
                metainfo,
                task_control,
                None,
                &mut peers,
                None,
            )
            .await
        });

        let (request, cancel) = timeout(Duration::from_secs(2), loser)
            .await
            .expect("loser observed cancellation")
            .expect("loser task");
        assert_eq!(cancel, request);
        assert!(
            !download.is_finished(),
            "cancel must be emitted before the storage delay completes"
        );
        let report = timeout(Duration::from_secs(3), &mut download)
            .await
            .expect("endgame download deadline")
            .expect("download task")
            .expect("endgame completion");
        assert_eq!(report.verified_piece_count, 1);
        assert_eq!(report.payload_high_water, MIN_PAYLOAD_ALLOWANCE);
        assert_eq!(
            report.outstanding_request_high_water,
            2 * MIN_PAYLOAD_ALLOWANCE
        );
        assert_eq!(
            tokio::fs::read(&output).await.expect("endgame output"),
            payload
        );
        timeout(Duration::from_secs(1), winner)
            .await
            .expect("winner joined")
            .expect("winner task");
        let swarm = control
            .diagnostic_snapshot()
            .swarm
            .expect("terminal swarm diagnostics");
        assert_eq!(swarm.endgame_assignments, 1);
        assert_eq!(swarm.cancelled_request_attempts, 1);
        assert_eq!(swarm.active_request_attempts, 0);
        let _ = tokio::fs::remove_file(output).await;
    }

    #[tokio::test]
    async fn sole_corrupt_source_is_banned_and_clean_peer_retries_piece() {
        let payload = (0..MIN_PAYLOAD_ALLOWANCE)
            .map(|index| ((index * 29 + index / 11) & 0xff) as u8)
            .collect::<Vec<_>>();
        let mut corrupt = payload.clone();
        corrupt[37] ^= 0x80;
        let metainfo =
            Metainfo::from_info_bytes(&single_file_info(&payload)).expect("hash retry metainfo");
        let corrupt_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind corrupt peer");
        let corrupt_address = corrupt_listener.local_addr().expect("corrupt address");
        let clean_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind clean peer");
        let clean_address = clean_listener.local_addr().expect("clean address");
        let corrupt_task = tokio::spawn(serve_content_peer(
            corrupt_listener,
            metainfo.info_hash,
            Arc::new(vec![corrupt]),
            vec![true],
        ));
        let clean_task = tokio::spawn(serve_content_peer(
            clean_listener,
            metainfo.info_hash,
            Arc::new(vec![payload.clone()]),
            vec![true],
        ));
        let mut peers = TorrentPeerCoordinator::from_endpoint(
            corrupt_address,
            PeerSource::Manual,
            loopback_network(Duration::from_secs(2)),
        )
        .expect("peer session");
        peers
            .observe_address(clean_address, PeerSource::Manual)
            .expect("clean peer");
        let mut swarm_config = SwarmConfig::for_request_limit(MIN_PAYLOAD_ALLOWANCE);
        swarm_config.max_established_connections = 1;
        swarm_config.max_pending_dials = 1;
        let control = DownloadControl::new();
        let activity = Arc::new(RecordingActivitySink::default());
        control.set_activity_sink(activity.clone());
        let output = test_path("piece-hash-retry.bin");

        let report = timeout(
            Duration::from_secs(3),
            run_content_download(
                ContentDownloadConfig {
                    output_path: output.clone(),
                    max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
                    swarm_config,
                    skip_files: Vec::new(),
                    materialize_files: Vec::new(),
                },
                metainfo,
                control.clone(),
                None,
                &mut peers,
                None,
            ),
        )
        .await
        .expect("bounded hash recovery")
        .expect("clean peer completes failed piece");

        assert_eq!(report.verified_piece_count, 1);
        assert_eq!(report.selected_written_bytes, 2 * payload.len());
        assert_eq!(
            tokio::fs::read(&output).await.expect("published output"),
            payload
        );
        let snapshot = control
            .diagnostic_snapshot()
            .swarm
            .expect("hash failure diagnostics");
        assert_eq!(snapshot.piece_hash_failures, 1);
        assert_eq!(snapshot.failed_piece_bytes, MIN_PAYLOAD_ALLOWANCE);
        assert_eq!(snapshot.last_hash_failure_contributors, 1);
        assert_eq!(snapshot.active_request_attempts, 0);
        assert_eq!(snapshot.outstanding_request_bytes, 0);
        let corrupt_record = peers
            .registry
            .find_endpoint(PeerEndpoint::new(corrupt_address).expect("corrupt endpoint"))
            .expect("corrupt record");
        assert_eq!(corrupt_record.phase(), crate::peer::PeerPhase::Banned);
        assert_eq!(corrupt_record.integrity().trust_points, -2);
        assert_eq!(corrupt_record.integrity().hash_failures, 1);
        let clean_record = peers
            .registry
            .find_endpoint(PeerEndpoint::new(clean_address).expect("clean endpoint"))
            .expect("clean record");
        assert_eq!(clean_record.integrity().trust_points, 1);
        assert_eq!(clean_record.integrity().valid_pieces, 1);
        let events = activity
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadActivityEvent::PieceHashFailed {
                piece_index: 0,
                contributor_count: 1,
                failed_bytes: MIN_PAYLOAD_ALLOWANCE,
            }
        )));
        assert_eq!(
            events
                .iter()
                .filter_map(|event| match event {
                    DownloadActivityEvent::PieceStarted {
                        piece_index: 0,
                        attempt,
                        ..
                    } => Some(*attempt),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event,
                    DownloadActivityEvent::PieceHashing { piece_index: 0 }
                ))
                .count(),
            2
        );
        drop(events);
        for task in [corrupt_task, clean_task] {
            timeout(Duration::from_secs(1), task)
                .await
                .expect("hash recovery peer joined")
                .expect("hash recovery peer task");
        }
        let _ = tokio::fs::remove_file(output).await;
    }

    #[tokio::test]
    async fn ambiguous_corrupt_generation_records_suspects_without_false_bans() {
        let payload = (0..(2 * MIN_PAYLOAD_ALLOWANCE))
            .map(|index| ((index * 17 + index / 13) & 0xff) as u8)
            .collect::<Vec<_>>();
        let mut corrupt = payload.clone();
        corrupt[17] ^= 0x40;
        corrupt[MIN_PAYLOAD_ALLOWANCE + 17] ^= 0x40;
        let info = single_file_info_with_piece_length(&payload, payload.len());
        let metainfo = Metainfo::from_info_bytes(&info).expect("ambiguous hash metainfo");
        let first_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind first suspect");
        let first_address = first_listener.local_addr().expect("first suspect address");
        let second_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind second suspect");
        let second_address = second_listener
            .local_addr()
            .expect("second suspect address");
        let clean_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind clean generation");
        let clean_address = clean_listener.local_addr().expect("clean address");
        let first_task = tokio::spawn(serve_one_block_then_choke_peer(
            first_listener,
            metainfo.info_hash,
            Arc::new(corrupt),
        ));
        let second_task = tokio::spawn(serve_one_block_then_choke_peer(
            second_listener,
            metainfo.info_hash,
            Arc::new(payload.clone()),
        ));
        let clean_task = tokio::spawn(serve_content_peer(
            clean_listener,
            metainfo.info_hash,
            Arc::new(vec![payload.clone()]),
            vec![true],
        ));
        let mut peers = TorrentPeerCoordinator::from_endpoint(
            first_address,
            PeerSource::Manual,
            loopback_network(Duration::from_secs(2)),
        )
        .expect("peer session");
        peers
            .observe_address(second_address, PeerSource::Manual)
            .expect("second suspect");
        peers
            .observe_address(clean_address, PeerSource::Manual)
            .expect("clean peer");
        let mut swarm_config = SwarmConfig::for_request_limit(2 * MIN_PAYLOAD_ALLOWANCE);
        swarm_config.max_established_connections = 2;
        swarm_config.max_pending_dials = 1;
        swarm_config.unproductive_grace = Duration::from_millis(50);
        let control = DownloadControl::new();
        let output = test_path("ambiguous-piece-hash-retry.bin");

        let report = timeout(
            Duration::from_secs(3),
            run_content_download(
                ContentDownloadConfig {
                    output_path: output.clone(),
                    max_buffered_payload_bytes: 2 * MIN_PAYLOAD_ALLOWANCE,
                    swarm_config,
                    skip_files: Vec::new(),
                    materialize_files: Vec::new(),
                },
                metainfo,
                control.clone(),
                None,
                &mut peers,
                None,
            ),
        )
        .await
        .expect("bounded ambiguous recovery")
        .expect("clean generation completes");

        assert_eq!(report.verified_piece_count, 1);
        assert_eq!(report.selected_written_bytes, 2 * payload.len());
        assert_eq!(
            tokio::fs::read(&output).await.expect("published output"),
            payload
        );
        let snapshot = control
            .diagnostic_snapshot()
            .swarm
            .expect("ambiguous hash diagnostics");
        assert_eq!(snapshot.piece_hash_failures, 1);
        assert_eq!(snapshot.failed_piece_bytes, payload.len());
        assert_eq!(snapshot.last_hash_failure_contributors, 2);
        for address in [first_address, second_address] {
            let record = peers
                .registry
                .find_endpoint(PeerEndpoint::new(address).expect("suspect endpoint"))
                .expect("suspect record");
            assert_ne!(record.phase(), crate::peer::PeerPhase::Banned);
            assert_eq!(record.integrity().trust_points, -2);
            assert_eq!(record.integrity().hash_failures, 1);
            assert!(record.integrity().on_parole);
        }
        let clean_record = peers
            .registry
            .find_endpoint(PeerEndpoint::new(clean_address).expect("clean endpoint"))
            .expect("clean record");
        assert_eq!(clean_record.integrity().trust_points, 1);
        for task in [first_task, second_task, clean_task] {
            timeout(Duration::from_secs(1), task)
                .await
                .expect("ambiguous recovery peer joined")
                .expect("ambiguous recovery peer task");
        }
        let _ = tokio::fs::remove_file(output).await;
    }

    #[tokio::test]
    async fn disconnect_and_choke_reassign_only_their_outstanding_blocks() {
        run_adverse_reassignment_case(AdverseRequestAction::Disconnect).await;
        run_adverse_reassignment_case(AdverseRequestAction::Choke).await;
    }

    #[tokio::test]
    async fn useful_peer_at_end_of_full_pending_cohort_completes_promptly() {
        assert_eq!(DEFAULT_MAX_PENDING_DIALS, 30);
        let first = vec![0x13; 16 * 1024];
        let second = vec![0x57; 16 * 1024];
        let metainfo =
            Metainfo::from_bytes(&two_piece_metainfo(&first, &second)).expect("two-piece metainfo");
        let mut silent_addresses = Vec::new();
        let mut silent_tasks = Vec::new();
        for _ in 1..DEFAULT_MAX_PENDING_DIALS {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind silent peer");
            silent_addresses.push(listener.local_addr().expect("silent address"));
            silent_tasks.push(tokio::spawn(accept_handshake_without_reply(listener)));
        }
        let useful = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind useful peer");
        let useful_address = useful.local_addr().expect("useful address");
        let useful_task = tokio::spawn(serve_content_peer(
            useful,
            metainfo.info_hash,
            Arc::new(vec![first, second]),
            vec![true, true],
        ));
        let mut peers = TorrentPeerCoordinator::from_endpoint(
            silent_addresses[0],
            PeerSource::Manual,
            loopback_network(Duration::from_secs(5)),
        )
        .expect("peer session");
        for address in &silent_addresses[1..] {
            peers
                .observe_address(*address, PeerSource::Manual)
                .expect("silent peer");
        }
        peers
            .observe_address(useful_address, PeerSource::Manual)
            .expect("30th useful peer");
        let output = test_path("silent-handshake-parallel");
        let report = timeout(
            Duration::from_secs(2),
            run_content_download(
                ContentDownloadConfig {
                    output_path: output.clone(),
                    max_buffered_payload_bytes: 2 * MIN_PAYLOAD_ALLOWANCE,
                    swarm_config: SwarmConfig::for_request_limit(2 * MIN_PAYLOAD_ALLOWANCE),
                    skip_files: Vec::new(),
                    materialize_files: Vec::new(),
                },
                metainfo,
                DownloadControl::new(),
                None,
                &mut peers,
                None,
            ),
        )
        .await
        .expect("silent handshakes did not serialize progress")
        .expect("useful peer completed");
        assert_eq!(report.verified_piece_count, 2);
        silent_tasks.push(useful_task);
        for task in silent_tasks {
            timeout(Duration::from_secs(1), task)
                .await
                .expect("peer joined")
                .expect("peer task");
        }
        let _ = tokio::fs::remove_dir_all(output).await;
    }

    #[tokio::test]
    async fn cancellation_joins_a_full_silent_pending_cohort() {
        let payload = vec![0x31; 16 * 1024];
        let metainfo =
            Metainfo::from_info_bytes(&single_file_info(&payload)).expect("single-piece metainfo");
        let accepted = Arc::new(AtomicUsize::new(0));
        let mut addresses = Vec::new();
        let mut tasks = Vec::new();
        for _ in 0..DEFAULT_MAX_PENDING_DIALS {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind silent peer");
            addresses.push(listener.local_addr().expect("silent address"));
            tasks.push(tokio::spawn(accept_handshake_without_reply_and_count(
                listener,
                Some(accepted.clone()),
            )));
        }
        let mut peers = TorrentPeerCoordinator::from_endpoint(
            addresses[0],
            PeerSource::Manual,
            loopback_network(Duration::from_secs(5)),
        )
        .expect("peer session");
        for address in &addresses[1..] {
            peers
                .observe_address(*address, PeerSource::Manual)
                .expect("silent peer");
        }
        let output = test_path("full-silent-pending-cancel.bin");
        let control = DownloadControl::new();
        let task_output = output.clone();
        let task_control = control.clone();
        let download = tokio::spawn(async move {
            let result = run_content_download(
                ContentDownloadConfig {
                    output_path: task_output,
                    max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
                    swarm_config: SwarmConfig::for_request_limit(MIN_PAYLOAD_ALLOWANCE),
                    skip_files: Vec::new(),
                    materialize_files: Vec::new(),
                },
                metainfo,
                task_control,
                None,
                &mut peers,
                None,
            )
            .await;
            (result, peers)
        });
        let all_started = timeout(Duration::from_secs(2), async {
            while accepted.load(Ordering::Acquire) < DEFAULT_MAX_PENDING_DIALS {
                tokio::task::yield_now().await;
            }
        })
        .await;
        assert!(
            all_started.is_ok(),
            "only {} of {DEFAULT_MAX_PENDING_DIALS} pending handshakes started",
            accepted.load(Ordering::Acquire)
        );
        control.cancel();
        let (result, peers) = timeout(Duration::from_secs(1), download)
            .await
            .expect("download joined")
            .expect("download task");
        assert!(matches!(result, Err(DownloadError::Cancelled)));
        assert_eq!(accepted.load(Ordering::Acquire), DEFAULT_MAX_PENDING_DIALS);
        assert!(
            peers
                .registry
                .records()
                .all(|record| record.phase() == PeerPhase::Idle)
        );
        for task in tasks {
            timeout(Duration::from_secs(1), task)
                .await
                .expect("silent peer joined")
                .expect("silent peer task");
        }
        assert!(!output.exists());
        let staging = staging_path(&output).expect("staging path");
        assert!(
            staging.exists(),
            "direct content cancellation stays resumable"
        );
        tokio::fs::remove_file(staging)
            .await
            .expect("remove canceled staging file");
    }

    #[tokio::test]
    async fn full_choked_set_is_replaced_by_an_eligible_useful_peer() {
        let first = vec![0x21; 16 * 1024];
        let second = vec![0x84; 16 * 1024];
        let metainfo =
            Metainfo::from_bytes(&two_piece_metainfo(&first, &second)).expect("two-piece metainfo");
        let mut addresses = Vec::new();
        let mut tasks = Vec::new();
        for _ in 0..8 {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind choked peer");
            addresses.push(listener.local_addr().expect("choked address"));
            tasks.push(tokio::spawn(serve_permanently_choked_peer(
                listener,
                metainfo.info_hash,
                vec![0xc0],
            )));
        }
        let useful_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind replacement peer");
        let useful_address = useful_listener.local_addr().expect("replacement address");
        tasks.push(tokio::spawn(serve_content_peer(
            useful_listener,
            metainfo.info_hash,
            Arc::new(vec![first, second]),
            vec![true, true],
        )));
        let mut peers = TorrentPeerCoordinator::from_endpoint(
            addresses[0],
            PeerSource::Manual,
            loopback_network(Duration::from_secs(2)),
        )
        .expect("peer session");
        for address in addresses.into_iter().skip(1) {
            peers
                .observe_address(address, PeerSource::Manual)
                .expect("choked peer");
        }
        peers
            .observe_address(useful_address, PeerSource::Manual)
            .expect("replacement peer");
        let mut swarm_config = SwarmConfig::for_request_limit(2 * MIN_PAYLOAD_ALLOWANCE);
        swarm_config.unproductive_grace = Duration::from_millis(100);
        let output = test_path("choked-capacity-replacement");
        let report = timeout(
            Duration::from_secs(3),
            run_content_download(
                ContentDownloadConfig {
                    output_path: output.clone(),
                    max_buffered_payload_bytes: 2 * MIN_PAYLOAD_ALLOWANCE,
                    swarm_config,
                    skip_files: Vec::new(),
                    materialize_files: Vec::new(),
                },
                metainfo,
                DownloadControl::new(),
                None,
                &mut peers,
                None,
            ),
        )
        .await
        .expect("bounded replacement")
        .expect("replacement peer completed");
        assert_eq!(report.verified_piece_count, 2);
        for task in tasks {
            timeout(Duration::from_secs(1), task)
                .await
                .expect("peer joined")
                .expect("peer task");
        }
        let _ = tokio::fs::remove_dir_all(output).await;
    }

    #[tokio::test]
    async fn full_irrelevant_set_is_replaced_by_a_wanted_piece_peer() {
        let first = vec![0x18; 16 * 1024];
        let second = vec![0xa6; 16 * 1024];
        let metainfo =
            Metainfo::from_bytes(&two_piece_metainfo(&first, &second)).expect("two-piece metainfo");
        let payload = Arc::new(vec![first, second]);
        let mut addresses = Vec::new();
        let mut tasks = Vec::new();
        for _ in 0..8 {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind irrelevant peer");
            addresses.push(listener.local_addr().expect("irrelevant address"));
            tasks.push(tokio::spawn(serve_content_peer(
                listener,
                metainfo.info_hash,
                payload.clone(),
                vec![false, false],
            )));
        }
        let useful_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind wanted-piece peer");
        let useful_address = useful_listener.local_addr().expect("wanted-piece address");
        tasks.push(tokio::spawn(serve_content_peer(
            useful_listener,
            metainfo.info_hash,
            payload,
            vec![true, true],
        )));
        let mut peers = TorrentPeerCoordinator::from_endpoint(
            addresses[0],
            PeerSource::Manual,
            loopback_network(Duration::from_secs(2)),
        )
        .expect("peer session");
        for address in addresses.into_iter().skip(1) {
            peers
                .observe_address(address, PeerSource::Manual)
                .expect("irrelevant peer");
        }
        peers
            .observe_address(useful_address, PeerSource::Manual)
            .expect("wanted-piece peer");
        let mut swarm_config = SwarmConfig::for_request_limit(2 * MIN_PAYLOAD_ALLOWANCE);
        swarm_config.unproductive_grace = Duration::from_millis(50);
        let output = test_path("irrelevant-capacity-replacement");

        let report = timeout(
            Duration::from_secs(3),
            run_content_download(
                ContentDownloadConfig {
                    output_path: output.clone(),
                    max_buffered_payload_bytes: 2 * MIN_PAYLOAD_ALLOWANCE,
                    swarm_config,
                    skip_files: Vec::new(),
                    materialize_files: Vec::new(),
                },
                metainfo,
                DownloadControl::new(),
                None,
                &mut peers,
                None,
            ),
        )
        .await
        .expect("bounded wanted-piece replacement")
        .expect("wanted-piece peer completed");

        assert_eq!(report.verified_piece_count, 2);
        for task in tasks {
            timeout(Duration::from_secs(1), task)
                .await
                .expect("irrelevant peer joined")
                .expect("irrelevant peer task");
        }
        let _ = tokio::fs::remove_dir_all(output).await;
    }

    #[tokio::test]
    async fn full_choked_set_without_an_alternative_waits_without_churn() {
        let payload = vec![0x5b; 16 * 1024];
        let metainfo =
            Metainfo::from_info_bytes(&single_file_info(&payload)).expect("single-piece metainfo");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind choked peer");
        let address = listener.local_addr().expect("choked address");
        let peer_task = tokio::spawn(serve_permanently_choked_peer(
            listener,
            metainfo.info_hash,
            vec![0x80],
        ));
        let mut peers = TorrentPeerCoordinator::from_endpoint(
            address,
            PeerSource::Manual,
            loopback_network(Duration::from_secs(2)),
        )
        .expect("peer session");
        let mut swarm_config = SwarmConfig::for_request_limit(MIN_PAYLOAD_ALLOWANCE);
        swarm_config.max_established_connections = 1;
        swarm_config.unproductive_grace = Duration::from_millis(50);
        let output = test_path("choked-no-alternative.bin");
        let control = DownloadControl::new();
        let result = {
            let mut download = Box::pin(run_content_download(
                ContentDownloadConfig {
                    output_path: output.clone(),
                    max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
                    swarm_config,
                    skip_files: Vec::new(),
                    materialize_files: Vec::new(),
                },
                metainfo,
                control.clone(),
                None,
                &mut peers,
                None,
            ));
            assert!(
                timeout(Duration::from_millis(200), &mut download)
                    .await
                    .is_err(),
                "no-alternative state must wait"
            );
            control.cancel();
            download.await
        };
        assert!(matches!(result, Err(DownloadError::Cancelled)));
        let record = peers.registry.records().next().expect("retained peer");
        assert_eq!(record.history().dial_attempts, 1);
        assert_eq!(record.history().total_failures, 0);
        timeout(Duration::from_secs(1), peer_task)
            .await
            .expect("choked peer joined")
            .expect("choked peer task");
        let _ = tokio::fs::remove_file(staging_path(&output).expect("staging path")).await;
    }

    #[tokio::test]
    async fn unrelated_messages_do_not_prevent_expiry_and_late_payload_is_safe() {
        let payload = vec![0x6a; 16 * 1024];
        let metainfo =
            Metainfo::from_info_bytes(&single_file_info(&payload)).expect("single-piece metainfo");
        let old_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind old peer");
        let old_address = old_listener.local_addr().expect("old address");
        let replacement_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind replacement peer");
        let replacement_address = replacement_listener
            .local_addr()
            .expect("replacement address");
        let old_task = tokio::spawn(serve_delayed_block_peer(
            old_listener,
            metainfo.info_hash,
            payload.clone(),
            Duration::from_millis(130),
            Some(Duration::from_millis(25)),
        ));
        let replacement_task = tokio::spawn(serve_delayed_block_peer(
            replacement_listener,
            metainfo.info_hash,
            payload.clone(),
            Duration::from_millis(100),
            None,
        ));
        let mut peers = TorrentPeerCoordinator::from_endpoint(
            old_address,
            PeerSource::Manual,
            loopback_network(Duration::from_secs(2)),
        )
        .expect("peer session");
        peers
            .observe_address(replacement_address, PeerSource::Manual)
            .expect("replacement peer");
        let mut swarm_config = SwarmConfig::for_request_limit(MIN_PAYLOAD_ALLOWANCE);
        swarm_config.request_timeout = Duration::from_millis(75);
        let output = test_path("late-request-payload.bin");
        let control = DownloadControl::new();
        let report = timeout(
            Duration::from_secs(3),
            run_content_download(
                ContentDownloadConfig {
                    output_path: output.clone(),
                    max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
                    swarm_config,
                    skip_files: Vec::new(),
                    materialize_files: Vec::new(),
                },
                metainfo,
                control.clone(),
                None,
                &mut peers,
                None,
            ),
        )
        .await
        .expect("bounded expiry and late response")
        .expect("late response download");
        assert_eq!(report.verified_piece_count, 1);
        assert_eq!(report.payload_high_water, MIN_PAYLOAD_ALLOWANCE);
        assert!(control.snapshot().requested_bytes >= 2 * MIN_PAYLOAD_ALLOWANCE);
        assert_eq!(tokio::fs::read(&output).await.expect("output"), payload);
        for task in [old_task, replacement_task] {
            timeout(Duration::from_secs(1), task)
                .await
                .expect("delayed peer joined")
                .expect("delayed peer task");
        }
        let _ = tokio::fs::remove_file(output).await;
    }

    #[tokio::test]
    async fn sampled_stall_moves_a_burst_peers_window_to_a_healthy_peer() {
        let payload = (0..(8 * MIN_PAYLOAD_ALLOWANCE))
            .map(|index| ((index * 43 + index / 17) & 0xff) as u8)
            .collect::<Vec<_>>();
        let info = single_file_info_with_piece_length(&payload, payload.len());
        let metainfo = Metainfo::from_info_bytes(&info).expect("stall metainfo");
        let stalled_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind stalled peer");
        let stalled_address = stalled_listener.local_addr().expect("stalled address");
        let useful_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind useful peer");
        let useful_address = useful_listener.local_addr().expect("useful address");
        let stalled_task = tokio::spawn(serve_delayed_block_peer_with_timeout(
            stalled_listener,
            metainfo.info_hash,
            payload.clone(),
            Duration::ZERO,
            None,
            Duration::from_secs(10),
        ));
        let useful_task = tokio::spawn(serve_content_peer_with_timeout(
            useful_listener,
            metainfo.info_hash,
            Arc::new(vec![payload.clone()]),
            vec![true],
            Duration::from_secs(10),
        ));
        let mut peers = TorrentPeerCoordinator::from_endpoint(
            stalled_address,
            PeerSource::Manual,
            loopback_network(Duration::from_secs(10)),
        )
        .expect("peer session");
        peers
            .observe_address(useful_address, PeerSource::Manual)
            .expect("useful peer");
        let payload_limit = payload.len();
        let mut swarm_config = SwarmConfig::for_request_limit(payload_limit);
        swarm_config.request_timeout = Duration::from_secs(10);
        let control = DownloadControl::new();
        let output = test_path("sampled-stall.bin");

        let report = timeout(
            Duration::from_secs(7),
            run_content_download(
                ContentDownloadConfig {
                    output_path: output.clone(),
                    max_buffered_payload_bytes: payload_limit,
                    swarm_config,
                    skip_files: Vec::new(),
                    materialize_files: Vec::new(),
                },
                metainfo,
                control.clone(),
                None,
                &mut peers,
                None,
            ),
        )
        .await
        .expect("adaptive stall deadline")
        .expect("healthy peer completed stalled work");

        assert_eq!(report.verified_piece_count, 1);
        assert_eq!(tokio::fs::read(&output).await.expect("output"), payload);
        assert!(control.snapshot().requested_bytes > report.bytes_written);
        for task in [stalled_task, useful_task] {
            timeout(Duration::from_secs(1), task)
                .await
                .expect("peer joined")
                .expect("peer task");
        }
        let _ = tokio::fs::remove_file(output).await;
    }

    fn single_file_info(payload: &[u8]) -> Vec<u8> {
        single_file_info_with_piece_length(payload, 16 * 1024)
    }

    fn single_file_info_with_piece_length(payload: &[u8], piece_length: usize) -> Vec<u8> {
        assert!(piece_length > 0);
        let piece_hashes = payload
            .chunks(piece_length)
            .flat_map(|piece| Sha1::digest(piece).to_vec())
            .collect::<Vec<_>>();
        let mut info = format!(
            "d6:lengthi{}e4:name1:x12:piece lengthi{}e6:pieces{}:",
            payload.len(),
            piece_length,
            piece_hashes.len()
        )
        .into_bytes();
        info.extend_from_slice(&piece_hashes);
        info.push(b'e');
        info
    }

    fn private_single_file_info(payload: &[u8]) -> Vec<u8> {
        let mut info = single_file_info(payload);
        info.splice(
            info.len() - 1..info.len() - 1,
            b"7:privatei1e".iter().copied(),
        );
        info
    }

    fn dht_config(bootstrap: SocketAddr) -> DhtConfig {
        DhtConfig {
            network_policy: NetworkPolicy::LoopbackOnly,
            bind_address: "127.0.0.1:0".parse().expect("DHT bind"),
            bootstrap_nodes: vec![BootstrapNode::Address(bootstrap)],
            initial_snapshot: None,
            query_timeout: Duration::from_millis(500),
            lookup_timeout: Duration::from_secs(3),
            bootstrap_retry_interval: Duration::from_secs(1),
            routing_refresh_interval: Duration::from_secs(60),
            read_only: false,
        }
    }

    fn test_dht_endpoint(address: SocketAddr) -> DhtEndpoint {
        let port = address.port();
        match address.ip() {
            IpAddr::V4(address) => DhtEndpoint::new(DhtIp::V4(address.octets()), port),
            IpAddr::V6(address) => DhtEndpoint::new(DhtIp::V6(address.octets()), port),
        }
    }

    async fn serve_dht_peer(socket: UdpSocket, info_hash: [u8; 20], peer: SocketAddr) {
        let mut packet = [0_u8; 1024];
        loop {
            let (length, client) = socket.recv_from(&mut packet).await.expect("DHT query");
            let DhtMessage::Query(query) = decode_dht(&packet[..length]).expect("decode DHT query")
            else {
                continue;
            };
            let peers = match query.query {
                DhtQuery::FindNode { .. } => Vec::new(),
                DhtQuery::GetPeers {
                    info_hash: target,
                    want,
                } => {
                    assert_eq!(target, NodeId(info_hash));
                    assert!(want.is_empty() || want.contains(&Want::Ipv4));
                    vec![test_dht_endpoint(peer)]
                }
                _ => Vec::new(),
            };
            let done = !peers.is_empty();
            let response = encode_dht_response(
                &query.transaction,
                NodeId([6; 20]),
                &[],
                &peers,
                Some(b"fixture"),
                test_dht_endpoint(client),
            )
            .expect("encode DHT response");
            socket
                .send_to(&response, client)
                .await
                .expect("send DHT response");
            if done {
                break;
            }
        }
    }

    async fn serve_dht_peer_after_signal(
        socket: UdpSocket,
        info_hash: [u8; 20],
        peer: SocketAddr,
        release: Arc<Notify>,
    ) {
        let mut packet = [0_u8; 1024];
        loop {
            let (length, client) = socket.recv_from(&mut packet).await.expect("DHT query");
            let DhtMessage::Query(query) = decode_dht(&packet[..length]).expect("decode DHT query")
            else {
                continue;
            };
            let peers = match query.query {
                DhtQuery::FindNode { .. } => Vec::new(),
                DhtQuery::GetPeers {
                    info_hash: target,
                    want,
                } => {
                    assert_eq!(target, NodeId(info_hash));
                    assert!(want.is_empty() || want.contains(&Want::Ipv4));
                    release.notified().await;
                    vec![test_dht_endpoint(peer)]
                }
                _ => Vec::new(),
            };
            let done = !peers.is_empty();
            let response = encode_dht_response(
                &query.transaction,
                NodeId([6; 20]),
                &[],
                &peers,
                Some(b"fixture"),
                test_dht_endpoint(client),
            )
            .expect("encode DHT response");
            socket
                .send_to(&response, client)
                .await
                .expect("send DHT response");
            if done {
                break;
            }
        }
    }

    async fn serve_dht_peer_after_retry(socket: UdpSocket, info_hash: [u8; 20], peer: SocketAddr) {
        let mut packet = [0_u8; 1024];
        let mut peer_queries = 0_u8;
        loop {
            let (length, client) = socket.recv_from(&mut packet).await.expect("DHT query");
            let DhtMessage::Query(query) = decode_dht(&packet[..length]).expect("decode DHT query")
            else {
                continue;
            };
            let peers = match query.query {
                DhtQuery::GetPeers {
                    info_hash: target, ..
                } => {
                    assert_eq!(target, NodeId(info_hash));
                    peer_queries = peer_queries.saturating_add(1);
                    if peer_queries >= 2 {
                        vec![test_dht_endpoint(peer)]
                    } else {
                        Vec::new()
                    }
                }
                _ => Vec::new(),
            };
            let done = !peers.is_empty();
            let response = encode_dht_response(
                &query.transaction,
                NodeId([6; 20]),
                &[],
                &peers,
                Some(b"fixture"),
                test_dht_endpoint(client),
            )
            .expect("encode DHT response");
            socket
                .send_to(&response, client)
                .await
                .expect("send DHT response");
            if done {
                break;
            }
        }
    }

    fn hex(bytes: &[u8]) -> String {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut output = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            output.push(DIGITS[(byte >> 4) as usize] as char);
            output.push(DIGITS[(byte & 0x0f) as usize] as char);
        }
        output
    }

    async fn serve_metadata_then_piece(
        listener: TcpListener,
        info: Vec<u8>,
        payload: Vec<u8>,
        bitfield: Vec<u8>,
    ) {
        let (mut stream, _) = listener.accept().await.expect("accept magnet client");
        let info_hash: [u8; 20] = Sha1::digest(&info).into();
        let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake_bytes)
            .await
            .expect("read client handshake");
        let handshake =
            decode_handshake(&handshake_bytes, info_hash).expect("client handshake identity");
        assert!(handshake.supports_extensions());
        assert_eq!(handshake.peer_id, CLIENT_PEER_ID);
        let mut reserved = [0; 8];
        reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
        stream
            .write_all(&encode_handshake_with_reserved(
                info_hash,
                *b"-RS-TEST-00000000000",
                reserved,
            ))
            .await
            .expect("send server handshake");
        let mut peer =
            PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(5));

        let PeerMessage::Extended { id: 0, .. } = next_peer_message(&mut peer)
            .await
            .expect("client extension handshake")
        else {
            panic!("expected extension handshake");
        };
        send_message(&mut peer, &PeerMessage::Bitfield(bitfield))
            .await
            .expect("send early bitfield");
        send_message(&mut peer, &PeerMessage::Unchoke)
            .await
            .expect("send early unchoke");
        send_message(
            &mut peer,
            &PeerMessage::Extended {
                id: 0,
                payload: encode_extension_handshake(Some(info.len())),
            },
        )
        .await
        .expect("send extension handshake");

        let request = next_peer_message(&mut peer)
            .await
            .expect("metadata request");
        let PeerMessage::Extended {
            id: 1,
            payload: request,
        } = request
        else {
            panic!("expected metadata extension request");
        };
        assert_eq!(
            parse_metadata_message(&request).expect("parse metadata request"),
            MetadataMessage::Request { piece: 0 }
        );
        send_message(
            &mut peer,
            &PeerMessage::Extended {
                id: 1,
                payload: encode_metadata_data(0, info.len(), &info).expect("encode metadata block"),
            },
        )
        .await
        .expect("send metadata data");

        loop {
            match next_peer_message(&mut peer).await {
                Ok(PeerMessage::Interested) => {}
                Ok(PeerMessage::Request(request)) => {
                    assert_eq!(request.index, 0);
                    let begin = request.begin as usize;
                    let end = begin + request.length as usize;
                    send_message(
                        &mut peer,
                        &PeerMessage::Piece {
                            index: 0,
                            begin: request.begin,
                            block: payload[begin..end].to_vec(),
                        },
                    )
                    .await
                    .expect("send payload block");
                }
                Err(DownloadError::PeerClosed) => break,
                Ok(message) => panic!("unexpected content message {message:?}"),
                Err(error) => panic!("scripted peer failed: {error}"),
            }
        }
    }

    async fn serve_stalled_metadata_peer(
        listener: TcpListener,
        info_hash: [u8; 20],
        metadata_size: usize,
    ) {
        let (mut stream, _) = listener.accept().await.expect("accept magnet client");
        let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake_bytes)
            .await
            .expect("read client handshake");
        assert!(
            decode_handshake(&handshake_bytes, info_hash)
                .expect("client handshake identity")
                .supports_extensions()
        );
        let mut reserved = [0; 8];
        reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
        stream
            .write_all(&encode_handshake_with_reserved(
                info_hash,
                *b"-RS-STALL-0000000000",
                reserved,
            ))
            .await
            .expect("send server handshake");
        let mut peer =
            PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(6));
        assert!(matches!(
            next_peer_message(&mut peer).await,
            Ok(PeerMessage::Extended { id: 0, .. })
        ));
        send_message(
            &mut peer,
            &PeerMessage::Extended {
                id: 0,
                payload: encode_extension_handshake(Some(metadata_size)),
            },
        )
        .await
        .expect("send extension handshake");
        assert!(matches!(
            next_peer_message(&mut peer).await,
            Ok(PeerMessage::Extended { id: 1, .. })
        ));
        loop {
            match next_peer_message(&mut peer).await {
                Ok(PeerMessage::Extended { id: 1, .. }) => {}
                Err(DownloadError::PeerClosed | DownloadError::PeerTimedOut { .. }) => break,
                Ok(message) => panic!("unexpected stalled metadata message {message:?}"),
                Err(error) => panic!("stalled metadata peer failed: {error}"),
            }
        }
    }

    async fn serve_partial_metadata_peer(
        listener: TcpListener,
        info: Vec<u8>,
        reject_second_request: bool,
    ) {
        let (mut stream, _) = listener.accept().await.expect("accept metadata client");
        let info_hash: [u8; 20] = Sha1::digest(&info).into();
        let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake_bytes)
            .await
            .expect("read client handshake");
        assert!(
            decode_handshake(&handshake_bytes, info_hash)
                .expect("client handshake identity")
                .supports_extensions()
        );
        let mut reserved = [0; 8];
        reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
        stream
            .write_all(&encode_handshake_with_reserved(
                info_hash,
                *b"-RS-PARTIAL-00000000",
                reserved,
            ))
            .await
            .expect("send server handshake");
        let mut peer =
            PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(2));
        assert!(matches!(
            next_peer_message(&mut peer).await,
            Ok(PeerMessage::Extended { id: 0, .. })
        ));
        send_message(
            &mut peer,
            &PeerMessage::Extended {
                id: 0,
                payload: encode_extension_handshake(Some(info.len())),
            },
        )
        .await
        .expect("send metadata extension handshake");

        let mut request_count = 0;
        loop {
            match next_peer_message(&mut peer).await {
                Ok(PeerMessage::Extended { id: 1, payload }) => {
                    let MetadataMessage::Request { piece } =
                        parse_metadata_message(&payload).expect("parse metadata request")
                    else {
                        panic!("expected metadata request");
                    };
                    request_count += 1;
                    if reject_second_request && request_count == 2 {
                        send_message(
                            &mut peer,
                            &PeerMessage::Extended {
                                id: 1,
                                payload: encode_metadata_reject(piece),
                            },
                        )
                        .await
                        .expect("reject second metadata request");
                        continue;
                    }
                    let piece = usize::try_from(piece).expect("nonnegative metadata piece");
                    let begin = piece * rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH;
                    let end = (begin + rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH)
                        .min(info.len());
                    send_message(
                        &mut peer,
                        &PeerMessage::Extended {
                            id: 1,
                            payload: encode_metadata_data(
                                piece as u32,
                                info.len(),
                                &info[begin..end],
                            )
                            .expect("encode metadata block"),
                        },
                    )
                    .await
                    .expect("send metadata block");
                }
                Err(DownloadError::PeerClosed) => break,
                Ok(message) => panic!("unexpected partial metadata message {message:?}"),
                Err(error) => panic!("partial metadata peer failed: {error}"),
            }
        }
    }

    async fn serve_metadata_bytes_after_delay(
        listener: TcpListener,
        info_hash: [u8; 20],
        bytes: Vec<u8>,
        extension_delay: Duration,
    ) {
        let (mut stream, _) = listener.accept().await.expect("accept metadata client");
        let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake_bytes)
            .await
            .expect("read client handshake");
        assert!(
            decode_handshake(&handshake_bytes, info_hash)
                .expect("client handshake identity")
                .supports_extensions()
        );
        let mut reserved = [0; 8];
        reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
        stream
            .write_all(&encode_handshake_with_reserved(
                info_hash,
                *b"-RS-SCRIPT-000000000",
                reserved,
            ))
            .await
            .expect("send server handshake");
        let mut peer =
            PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(3));
        assert!(matches!(
            next_peer_message(&mut peer).await,
            Ok(PeerMessage::Extended { id: 0, .. })
        ));
        tokio::time::sleep(extension_delay).await;
        send_message(
            &mut peer,
            &PeerMessage::Extended {
                id: 0,
                payload: encode_extension_handshake(Some(bytes.len())),
            },
        )
        .await
        .expect("send metadata extension handshake");

        loop {
            match next_peer_message(&mut peer).await {
                Ok(PeerMessage::Extended { id: 1, payload }) => {
                    let MetadataMessage::Request { piece } =
                        parse_metadata_message(&payload).expect("parse metadata request")
                    else {
                        panic!("expected metadata request");
                    };
                    let piece = usize::try_from(piece).expect("nonnegative metadata piece");
                    let begin = piece * rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH;
                    let end = (begin + rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH)
                        .min(bytes.len());
                    send_message(
                        &mut peer,
                        &PeerMessage::Extended {
                            id: 1,
                            payload: encode_metadata_data(
                                piece as u32,
                                bytes.len(),
                                &bytes[begin..end],
                            )
                            .expect("encode metadata block"),
                        },
                    )
                    .await
                    .expect("send metadata block");
                }
                Err(DownloadError::PeerClosed) => break,
                Ok(message) => panic!("unexpected metadata message {message:?}"),
                Err(error) => panic!("scripted metadata peer failed: {error}"),
            }
        }
    }

    async fn serve_one_at_a_time_metadata_peer(listener: TcpListener, info: Vec<u8>) {
        let (mut stream, _) = listener.accept().await.expect("accept metadata client");
        let info_hash: [u8; 20] = Sha1::digest(&info).into();
        let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake_bytes)
            .await
            .expect("read client handshake");
        assert!(
            decode_handshake(&handshake_bytes, info_hash)
                .expect("client handshake identity")
                .supports_extensions()
        );
        let mut reserved = [0; 8];
        reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
        stream
            .write_all(&encode_handshake_with_reserved(
                info_hash,
                *b"-RS-ONE-AT-A-TIME000",
                reserved,
            ))
            .await
            .expect("send server handshake");
        let mut peer =
            PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(2));
        assert!(matches!(
            next_peer_message(&mut peer).await,
            Ok(PeerMessage::Extended { id: 0, .. })
        ));
        send_message(
            &mut peer,
            &PeerMessage::Extended {
                id: 0,
                payload: encode_extension_handshake(Some(info.len())),
            },
        )
        .await
        .expect("send metadata extension handshake");

        let first = next_peer_message(&mut peer)
            .await
            .expect("first metadata request");
        let PeerMessage::Extended {
            id: 1,
            payload: first,
        } = first
        else {
            panic!("expected first metadata request");
        };
        assert_eq!(
            parse_metadata_message(&first).expect("parse first request"),
            MetadataMessage::Request { piece: 0 }
        );
        assert!(
            timeout(Duration::from_millis(200), next_peer_message(&mut peer))
                .await
                .is_err(),
            "client must not pipeline a second metadata request immediately"
        );
        send_message(
            &mut peer,
            &PeerMessage::Extended {
                id: 1,
                payload: encode_metadata_data(
                    0,
                    info.len(),
                    &info[..rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH],
                )
                .expect("encode first metadata block"),
            },
        )
        .await
        .expect("send first metadata block");

        let second = next_peer_message(&mut peer)
            .await
            .expect("second metadata request after response");
        let PeerMessage::Extended {
            id: 1,
            payload: second,
        } = second
        else {
            panic!("expected second metadata request");
        };
        assert_eq!(
            parse_metadata_message(&second).expect("parse second request"),
            MetadataMessage::Request { piece: 1 }
        );
        send_message(
            &mut peer,
            &PeerMessage::Extended {
                id: 1,
                payload: encode_metadata_data(
                    1,
                    info.len(),
                    &info[rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH..],
                )
                .expect("encode second metadata block"),
            },
        )
        .await
        .expect("send second metadata block");
        assert!(matches!(
            next_peer_message(&mut peer).await,
            Err(DownloadError::PeerClosed)
        ));
    }

    async fn serve_metadata_peer_without_ut_metadata(listener: TcpListener, info_hash: [u8; 20]) {
        let (mut stream, _) = listener.accept().await.expect("accept magnet client");
        let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake_bytes)
            .await
            .expect("read client handshake");
        assert!(
            decode_handshake(&handshake_bytes, info_hash)
                .expect("client handshake identity")
                .supports_extensions()
        );
        let mut reserved = [0; 8];
        reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
        stream
            .write_all(&encode_handshake_with_reserved(
                info_hash,
                *b"-RS-STALL-0000000000",
                reserved,
            ))
            .await
            .expect("send server handshake");
        let mut peer =
            PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(1));
        assert!(matches!(
            next_peer_message(&mut peer).await,
            Ok(PeerMessage::Extended { id: 0, .. })
        ));
        send_message(
            &mut peer,
            &PeerMessage::Extended {
                id: 0,
                payload: b"d1:mdee".to_vec(),
            },
        )
        .await
        .expect("send extension handshake without ut_metadata");
        assert!(matches!(
            next_peer_message(&mut peer).await,
            Err(DownloadError::PeerClosed)
        ));
    }

    async fn serve_chattering_peer_without_extension_handshake(
        listener: TcpListener,
        info_hash: [u8; 20],
    ) {
        let (mut stream, _) = listener.accept().await.expect("accept magnet client");
        let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake_bytes)
            .await
            .expect("read client handshake");
        assert!(
            decode_handshake(&handshake_bytes, info_hash)
                .expect("client handshake identity")
                .supports_extensions()
        );
        let mut reserved = [0; 8];
        reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
        stream
            .write_all(&encode_handshake_with_reserved(
                info_hash,
                *b"-RS-STALL-0000000000",
                reserved,
            ))
            .await
            .expect("send server handshake");
        let mut peer =
            PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(1));
        assert!(matches!(
            next_peer_message(&mut peer).await,
            Ok(PeerMessage::Extended { id: 0, .. })
        ));
        loop {
            tokio::time::sleep(Duration::from_millis(10)).await;
            if send_message(&mut peer, &PeerMessage::KeepAlive)
                .await
                .is_err()
            {
                break;
            }
        }
    }

    async fn serve_metadata_rejecting_peer(
        listener: TcpListener,
        info_hash: [u8; 20],
        metadata_size: usize,
    ) {
        let (mut stream, _) = listener.accept().await.expect("accept magnet client");
        let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake_bytes)
            .await
            .expect("read client handshake");
        decode_handshake(&handshake_bytes, info_hash).expect("client handshake identity");
        let mut reserved = [0; 8];
        reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
        stream
            .write_all(&encode_handshake_with_reserved(
                info_hash,
                *b"-RS-STALL-0000000000",
                reserved,
            ))
            .await
            .expect("send server handshake");
        let mut peer =
            PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(1));
        assert!(matches!(
            next_peer_message(&mut peer).await,
            Ok(PeerMessage::Extended { id: 0, .. })
        ));
        send_message(
            &mut peer,
            &PeerMessage::Extended {
                id: 0,
                payload: encode_extension_handshake(Some(metadata_size)),
            },
        )
        .await
        .expect("send metadata extension handshake");
        let message = match next_peer_message(&mut peer).await {
            Ok(message) => message,
            Err(DownloadError::PeerClosed | DownloadError::PeerTimedOut { .. }) => return,
            Err(error) => panic!("rejecting metadata peer failed: {error}"),
        };
        let PeerMessage::Extended { id: 1, payload } = message else {
            panic!("expected metadata request");
        };
        let MetadataMessage::Request { piece } =
            parse_metadata_message(&payload).expect("parse metadata request")
        else {
            panic!("expected metadata request payload");
        };
        send_message(
            &mut peer,
            &PeerMessage::Extended {
                id: 1,
                payload: encode_metadata_reject(piece),
            },
        )
        .await
        .expect("reject metadata request");
        assert!(matches!(
            next_peer_message(&mut peer).await,
            Err(DownloadError::PeerClosed)
        ));
    }

    async fn serve_one_shot_udp_tracker(
        socket: UdpSocket,
        info_hash: [u8; 20],
        unreachable: SocketAddr,
        reachable: SocketAddr,
        announce_delay: Duration,
    ) {
        let mut request = [0; 2048];
        let (connect_length, client) = socket
            .recv_from(&mut request)
            .await
            .expect("receive tracker connect");
        assert_eq!(connect_length, 16);
        assert_eq!(
            u64::from_be_bytes(request[0..8].try_into().expect("protocol ID")),
            0x0417_2710_1980
        );
        assert_eq!(
            u32::from_be_bytes(request[8..12].try_into().expect("connect action")),
            0
        );
        let connect_transaction =
            u32::from_be_bytes(request[12..16].try_into().expect("connect transaction"));
        assert_ne!(connect_transaction, 0);

        let connection_id = 0x0102_0304_0506_0708_u64;
        socket
            .send_to(&[0, 1, 2, 3], client)
            .await
            .expect("send undersized unrelated response");
        let mut stale_connect = [0; 16];
        stale_connect[0..4].copy_from_slice(&0_u32.to_be_bytes());
        stale_connect[4..8].copy_from_slice(&connect_transaction.wrapping_add(1).to_be_bytes());
        stale_connect[8..16].copy_from_slice(&connection_id.to_be_bytes());
        socket
            .send_to(&stale_connect, client)
            .await
            .expect("send stale connect response");
        let mut connect_response = stale_connect;
        connect_response[4..8].copy_from_slice(&connect_transaction.to_be_bytes());
        socket
            .send_to(&connect_response, client)
            .await
            .expect("send connect response");

        let (announce_length, announce_client) = socket
            .recv_from(&mut request)
            .await
            .expect("receive tracker announce");
        assert_eq!(announce_client, client);
        assert_eq!(announce_length, 98);
        assert_eq!(
            u64::from_be_bytes(request[0..8].try_into().expect("connection ID")),
            connection_id
        );
        assert_eq!(
            u32::from_be_bytes(request[8..12].try_into().expect("announce action")),
            1
        );
        let announce_transaction =
            u32::from_be_bytes(request[12..16].try_into().expect("announce transaction"));
        assert_ne!(announce_transaction, 0);
        assert_ne!(announce_transaction, connect_transaction);
        assert_eq!(&request[16..36], &info_hash);
        tokio::time::sleep(announce_delay).await;
        assert_eq!(&request[36..56], &CLIENT_PEER_ID);
        assert_eq!(
            u64::from_be_bytes(request[56..64].try_into().expect("downloaded")),
            0
        );
        assert_eq!(
            u64::from_be_bytes(request[64..72].try_into().expect("left")),
            16 * 1024
        );
        assert_eq!(
            u64::from_be_bytes(request[72..80].try_into().expect("uploaded")),
            0
        );
        assert_eq!(
            u32::from_be_bytes(request[80..84].try_into().expect("event")),
            2
        );
        assert_eq!(
            u32::from_be_bytes(request[84..88].try_into().expect("IP address")),
            0
        );
        assert_ne!(
            u32::from_be_bytes(request[88..92].try_into().expect("key")),
            0
        );
        assert_eq!(
            i32::from_be_bytes(request[92..96].try_into().expect("num want")),
            200
        );
        assert_eq!(
            u16::from_be_bytes(request[96..98].try_into().expect("listen port")),
            DEFAULT_ADVERTISED_PEER_PORT
        );

        let mut response = Vec::new();
        response.extend_from_slice(&1_u32.to_be_bytes());
        response.extend_from_slice(&announce_transaction.to_be_bytes());
        response.extend_from_slice(&1800_u32.to_be_bytes());
        response.extend_from_slice(&1_u32.to_be_bytes());
        response.extend_from_slice(&1_u32.to_be_bytes());
        response.extend_from_slice(&[127, 0, 0, 1, 0, 0]);
        for address in [unreachable, reachable, reachable] {
            let SocketAddr::V4(address) = address else {
                panic!("scripted tracker uses IPv4 peers");
            };
            response.extend_from_slice(&address.ip().octets());
            response.extend_from_slice(&address.port().to_be_bytes());
        }
        response.extend_from_slice(&[192, 0, 2, 1, 0x1a, 0xe1]);

        let mut stale_response = response.clone();
        stale_response[4..8].copy_from_slice(&announce_transaction.wrapping_add(1).to_be_bytes());
        socket
            .send_to(&stale_response, client)
            .await
            .expect("send stale announce response");
        socket
            .send_to(&response, client)
            .await
            .expect("send announce response");
    }

    async fn serve_rejecting_udp_tracker(socket: UdpSocket) {
        let mut request = [0; 16];
        let (length, client) = socket
            .recv_from(&mut request)
            .await
            .expect("receive rejected tracker connect");
        assert_eq!(length, request.len());
        let transaction =
            u32::from_be_bytes(request[12..16].try_into().expect("connect transaction"));
        let mut response = Vec::from(3_u32.to_be_bytes());
        response.extend_from_slice(&transaction.to_be_bytes());
        response.extend_from_slice(b"controlled rejection");
        socket
            .send_to(&response, client)
            .await
            .expect("send tracker rejection");
    }

    #[tokio::test]
    async fn tracker_only_magnet_discovers_registry_peers_and_downloads() {
        let payload = b"tracker-discovered payload".to_vec();
        let info = single_file_info(&payload);
        let info_hash: [u8; 20] = Sha1::digest(&info).into();
        let output_path = test_path("tracker-magnet-output.bin");

        let unreachable_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind unreachable peer placeholder");
        let unreachable = unreachable_listener
            .local_addr()
            .expect("unreachable peer address");
        drop(unreachable_listener);

        let peer_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind tracker-discovered peer");
        let reachable = peer_listener.local_addr().expect("peer address");
        let peer_task = tokio::spawn(serve_metadata_then_piece(
            peer_listener,
            info,
            payload.clone(),
            vec![0x80],
        ));

        let tracker_socket = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind scripted UDP tracker");
        let tracker_address = tracker_socket.local_addr().expect("tracker address");
        let tracker_task = tokio::spawn(serve_one_shot_udp_tracker(
            tracker_socket,
            info_hash,
            unreachable,
            reachable,
            Duration::ZERO,
        ));
        let rejecting_tracker = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind rejecting UDP tracker");
        let rejecting_tracker_address = rejecting_tracker
            .local_addr()
            .expect("rejecting tracker address");
        let rejecting_tracker_task = tokio::spawn(serve_rejecting_udp_tracker(rejecting_tracker));

        let magnet = format!(
            "magnet:?xt=urn:btih:{}&tr=udp%3A%2F%2F{rejecting_tracker_address}&\
             tr=udp%3A%2F%2F{tracker_address}%2Fannounce",
            hex(&info_hash)
        );
        let parsed = Magnet::parse(&magnet).expect("parse tracker magnet");
        assert!(parsed.peer_hints.is_empty());
        assert_eq!(parsed.udp_trackers.len(), 2);
        let network = loopback_network(Duration::from_secs(2));
        let control = DownloadControl::new();
        let mut peers =
            TorrentPeerCoordinator::from_magnet(&parsed, network, control.clone(), None)
                .await
                .expect("prepare tracker discovery");
        assert!(peers.registry.is_empty());

        let report = run_magnet_download_with_peers(
            MagnetDownloadConfig {
                magnet,
                output_path: output_path.clone(),
                network,
                resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
                skip_files: Vec::new(),
                materialize_files: Vec::new(),
                dht: None,
            },
            control,
            parsed,
            &mut peers,
        )
        .await
        .expect("tracker-discovered magnet download");

        assert_eq!(peers.registry.len(), 2);
        let failed = peers
            .registry
            .find_endpoint(PeerEndpoint::new(unreachable).expect("failed endpoint"))
            .expect("failed tracker peer retained");
        assert_eq!(failed.history().total_failures, 1);
        assert_eq!(failed.history().last_failure, Some(PeerFailure::Connect));
        assert!(failed.sources().contains(PeerSource::Tracker));
        let succeeded = peers
            .registry
            .find_endpoint(PeerEndpoint::new(reachable).expect("successful endpoint"))
            .expect("successful tracker peer retained");
        assert_eq!(succeeded.history().total_failures, 0);
        assert!(succeeded.history().last_connected_at.is_some());
        assert!(succeeded.history().last_disconnected_at.is_some());
        assert!(succeeded.sources().contains(PeerSource::Tracker));

        assert_eq!(report.info_hash, info_hash);
        assert_eq!(
            tokio::fs::read(&output_path)
                .await
                .expect("published tracker output"),
            payload
        );
        peers
            .shutdown_tracker()
            .await
            .expect("stop tracker manager");
        if rejecting_tracker_task.is_finished() {
            rejecting_tracker_task
                .await
                .expect("rejecting tracker task");
        } else {
            rejecting_tracker_task.abort();
            let _ = rejecting_tracker_task.await;
        }
        tracker_task.await.expect("scripted tracker task");
        peer_task.await.expect("scripted peer task");
        let _ = tokio::fs::remove_file(output_path).await;
    }

    #[tokio::test]
    async fn tracker_peer_discovered_during_content_becomes_useful() {
        let payload = b"late tracker peer payload".to_vec();
        let info = single_file_info(&payload);
        let info_hash: [u8; 20] = Sha1::digest(&info).into();
        let metadata_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind metadata-only peer");
        let metadata_address = metadata_listener.local_addr().expect("metadata address");
        let metadata_task = tokio::spawn(serve_metadata_then_piece(
            metadata_listener,
            info,
            payload.clone(),
            vec![0x00],
        ));
        let useful_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind late useful peer");
        let useful_address = useful_listener.local_addr().expect("useful address");
        let useful_task = tokio::spawn(serve_content_peer(
            useful_listener,
            info_hash,
            Arc::new(vec![payload.clone()]),
            vec![true],
        ));
        let unavailable_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind unavailable placeholder");
        let unavailable = unavailable_listener
            .local_addr()
            .expect("unavailable address");
        drop(unavailable_listener);
        let tracker = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind delayed tracker");
        let tracker_address = tracker.local_addr().expect("tracker address");
        let tracker_task = tokio::spawn(serve_one_shot_udp_tracker(
            tracker,
            info_hash,
            unavailable,
            useful_address,
            Duration::from_millis(150),
        ));
        let output = test_path("late-tracker-content.bin");
        let result = timeout(
            Duration::from_secs(3),
            download_magnet(MagnetDownloadConfig {
                magnet: format!(
                    "magnet:?xt=urn:btih:{}&x.pe={metadata_address}&\
                     tr=udp%3A%2F%2F{tracker_address}%2Fannounce",
                    hex(&info_hash)
                ),
                output_path: output.clone(),
                network: loopback_network(Duration::from_secs(2)),
                resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
                skip_files: Vec::new(),
                materialize_files: Vec::new(),
                dht: None,
            }),
        )
        .await
        .expect("bounded late discovery")
        .expect("late discovered peer completed content");
        assert_eq!(result.verified_piece_count, 1);
        assert_eq!(
            tokio::fs::read(&output).await.expect("downloaded output"),
            payload
        );
        for task in [metadata_task, useful_task, tracker_task] {
            timeout(Duration::from_secs(1), task)
                .await
                .expect("scripted owner joined")
                .expect("scripted task");
        }
        let _ = tokio::fs::remove_file(output).await;
    }

    #[tokio::test]
    async fn dht_peer_discovered_during_content_becomes_useful() {
        let payload = b"late DHT peer payload".to_vec();
        let metainfo =
            Metainfo::from_info_bytes(&single_file_info(&payload)).expect("single-piece metainfo");
        let unavailable_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind unavailable content peer");
        let unavailable_address = unavailable_listener
            .local_addr()
            .expect("unavailable content address");
        let unavailable_task = tokio::spawn(serve_content_peer(
            unavailable_listener,
            metainfo.info_hash,
            Arc::new(vec![payload.clone()]),
            vec![false],
        ));
        let useful_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind DHT content peer");
        let useful_address = useful_listener.local_addr().expect("DHT content address");
        let useful_task = tokio::spawn(serve_content_peer(
            useful_listener,
            metainfo.info_hash,
            Arc::new(vec![payload.clone()]),
            vec![true],
        ));
        let dht_socket = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind scripted DHT");
        let dht_address = dht_socket.local_addr().expect("DHT address");
        let dht_task = tokio::spawn(serve_dht_peer(
            dht_socket,
            metainfo.info_hash,
            useful_address,
        ));
        let dht = DhtService::start(dht_config(dht_address))
            .await
            .expect("start DHT client");
        let mut peers = TorrentPeerCoordinator::from_endpoint(
            unavailable_address,
            PeerSource::Manual,
            loopback_network(Duration::from_secs(2)),
        )
        .expect("peer session");
        peers.dht = Some(dht.handle());
        let output = test_path("late-dht-content.bin");

        let report = timeout(
            Duration::from_secs(3),
            run_content_download(
                ContentDownloadConfig {
                    output_path: output.clone(),
                    max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
                    swarm_config: SwarmConfig::for_request_limit(MIN_PAYLOAD_ALLOWANCE),
                    skip_files: Vec::new(),
                    materialize_files: Vec::new(),
                },
                metainfo,
                DownloadControl::new(),
                None,
                &mut peers,
                None,
            ),
        )
        .await
        .expect("bounded late DHT discovery")
        .expect("late DHT peer completed content");

        assert_eq!(report.verified_piece_count, 1);
        assert_eq!(tokio::fs::read(&output).await.expect("output"), payload);
        let discovered = peers
            .registry
            .find_endpoint(PeerEndpoint::new(useful_address).expect("DHT endpoint"))
            .expect("DHT peer retained");
        assert!(discovered.sources().contains(PeerSource::Dht));
        for task in [unavailable_task, useful_task, dht_task] {
            timeout(Duration::from_secs(1), task)
                .await
                .expect("scripted DHT owner joined")
                .expect("scripted DHT task");
        }
        dht.shutdown().await.expect("DHT shutdown");
        let _ = tokio::fs::remove_file(output).await;
    }

    async fn assert_tracker_wait_cancels_without_socket_leaks() {
        let tracker = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind silent tracker");
        let tracker_address = tracker.local_addr().expect("tracker address");
        let output_path = test_path("cancelled-tracker-output.bin");
        let control = DownloadControl::new();
        let task_control = control.clone();
        let task = tokio::spawn(download_magnet_with_control(
            MagnetDownloadConfig {
                magnet: format!(
                    "magnet:?xt=urn:btih:{}&tr=udp%3A%2F%2F{tracker_address}",
                    "00".repeat(20)
                ),
                output_path: output_path.clone(),
                network: loopback_network(Duration::from_secs(2)),
                resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
                skip_files: Vec::new(),
                materialize_files: Vec::new(),
                dht: None,
            },
            task_control,
        ));

        let mut packet = [0; 32];
        let (length, client) = timeout(Duration::from_secs(1), tracker.recv_from(&mut packet))
            .await
            .expect("tracker connect deadline")
            .expect("receive tracker connect");
        assert_eq!(length, 16);
        control.cancel();
        let result = task.await.expect("join tracker-wait download");
        assert!(matches!(result, Err(DownloadError::Cancelled)));
        assert!(!tokio::fs::try_exists(&output_path).await.expect("output"));
        assert!(
            !tokio::fs::try_exists(staging_path(&output_path).expect("staging path"))
                .await
                .expect("staging")
        );

        UdpSocket::bind(client)
            .await
            .expect("tracker client socket released after terminal result");
    }

    #[derive(Debug, Default)]
    struct RecordingActivitySink {
        events: Mutex<Vec<DownloadActivityEvent>>,
    }

    impl DownloadActivitySink for RecordingActivitySink {
        fn record(&self, event: DownloadActivityEvent) {
            self.events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(event);
        }
    }

    async fn serve_empty_udp_tracker(socket: UdpSocket) {
        let mut packet = [0; 256];
        let (connect_length, client) = socket
            .recv_from(&mut packet)
            .await
            .expect("receive empty-tracker connect");
        assert_eq!(connect_length, 16);
        let connect_transaction =
            u32::from_be_bytes(packet[12..16].try_into().expect("connect transaction"));
        let connection_id = 0x0102_0304_0506_0708_u64;
        let mut connect_response = Vec::from(0_u32.to_be_bytes());
        connect_response.extend_from_slice(&connect_transaction.to_be_bytes());
        connect_response.extend_from_slice(&connection_id.to_be_bytes());
        socket
            .send_to(&connect_response, client)
            .await
            .expect("send empty-tracker connect response");

        let (announce_length, announce_client) = socket
            .recv_from(&mut packet)
            .await
            .expect("receive empty-tracker announce");
        assert_eq!(announce_length, 98);
        assert_eq!(announce_client, client);
        let announce_transaction =
            u32::from_be_bytes(packet[12..16].try_into().expect("announce transaction"));
        let mut announce_response = Vec::from(1_u32.to_be_bytes());
        announce_response.extend_from_slice(&announce_transaction.to_be_bytes());
        announce_response.extend_from_slice(&600_u32.to_be_bytes());
        announce_response.extend_from_slice(&0_u32.to_be_bytes());
        announce_response.extend_from_slice(&0_u32.to_be_bytes());
        socket
            .send_to(&announce_response, client)
            .await
            .expect("send valid zero-peer announce response");
    }

    async fn serve_barrier_udp_tracker(
        socket: UdpSocket,
        connect_barrier: Arc<Barrier>,
        peer_port: u16,
    ) {
        let mut packet = [0; 256];
        let (connect_length, client) = socket
            .recv_from(&mut packet)
            .await
            .expect("receive concurrent tracker connect");
        assert_eq!(connect_length, 16);
        let connect_transaction =
            u32::from_be_bytes(packet[12..16].try_into().expect("connect transaction"));
        connect_barrier.wait().await;

        let connection_id = 0x0102_0304_0506_0708_u64;
        let mut connect_response = Vec::from(0_u32.to_be_bytes());
        connect_response.extend_from_slice(&connect_transaction.to_be_bytes());
        connect_response.extend_from_slice(&connection_id.to_be_bytes());
        socket
            .send_to(&connect_response, client)
            .await
            .expect("send concurrent tracker connect response");

        let (announce_length, announce_client) = socket
            .recv_from(&mut packet)
            .await
            .expect("receive concurrent tracker announce");
        assert_eq!(announce_length, 98);
        assert_eq!(announce_client, client);
        let announce_transaction =
            u32::from_be_bytes(packet[12..16].try_into().expect("announce transaction"));
        let mut announce_response = Vec::from(1_u32.to_be_bytes());
        announce_response.extend_from_slice(&announce_transaction.to_be_bytes());
        announce_response.extend_from_slice(&600_u32.to_be_bytes());
        announce_response.extend_from_slice(&0_u32.to_be_bytes());
        announce_response.extend_from_slice(&0_u32.to_be_bytes());
        announce_response.extend_from_slice(&[127, 0, 0, 1]);
        announce_response.extend_from_slice(&peer_port.to_be_bytes());
        socket
            .send_to(&announce_response, client)
            .await
            .expect("send concurrent tracker announce response");
    }

    async fn serve_bounded_startup_tracker(
        socket: UdpSocket,
        started: Arc<AtomicUsize>,
        release: Arc<Semaphore>,
        peer_port: u16,
    ) -> bool {
        let mut packet = [0; 256];
        let (connect_length, client) = socket
            .recv_from(&mut packet)
            .await
            .expect("receive bounded tracker connect");
        assert_eq!(connect_length, 16);
        let ordinal = started.fetch_add(1, Ordering::AcqRel);
        let _permit = release.acquire().await.expect("startup release permit");
        let connect_transaction =
            u32::from_be_bytes(packet[12..16].try_into().expect("connect transaction"));
        if ordinal < super::MAX_CONCURRENT_TRACKER_OPERATIONS {
            let mut error_response = Vec::from(3_u32.to_be_bytes());
            error_response.extend_from_slice(&connect_transaction.to_be_bytes());
            error_response.extend_from_slice(b"scripted startup failure");
            socket
                .send_to(&error_response, client)
                .await
                .expect("send bounded tracker error");
            return false;
        }

        let connection_id = 0x0102_0304_0506_0708_u64;
        let mut connect_response = Vec::from(0_u32.to_be_bytes());
        connect_response.extend_from_slice(&connect_transaction.to_be_bytes());
        connect_response.extend_from_slice(&connection_id.to_be_bytes());
        socket
            .send_to(&connect_response, client)
            .await
            .expect("send bounded tracker connect response");
        let (announce_length, announce_client) = socket
            .recv_from(&mut packet)
            .await
            .expect("receive bounded tracker announce");
        assert_eq!(announce_length, 98);
        assert_eq!(announce_client, client);
        let announce_transaction =
            u32::from_be_bytes(packet[12..16].try_into().expect("announce transaction"));
        let mut announce_response = Vec::from(1_u32.to_be_bytes());
        announce_response.extend_from_slice(&announce_transaction.to_be_bytes());
        announce_response.extend_from_slice(&600_u32.to_be_bytes());
        announce_response.extend_from_slice(&0_u32.to_be_bytes());
        announce_response.extend_from_slice(&0_u32.to_be_bytes());
        announce_response.extend_from_slice(&[127, 0, 0, 1]);
        announce_response.extend_from_slice(&peer_port.to_be_bytes());
        socket
            .send_to(&announce_response, client)
            .await
            .expect("send bounded tracker announce response");
        true
    }

    #[tokio::test]
    async fn initial_tracker_operations_start_concurrently_and_merge_results() {
        let barrier = Arc::new(Barrier::new(3));
        let mut tracker_addresses = Vec::new();
        let mut servers = Vec::new();
        for offset in 0..3_u16 {
            let tracker = UdpSocket::bind("127.0.0.1:0")
                .await
                .expect("bind concurrent tracker");
            tracker_addresses.push(tracker.local_addr().expect("concurrent tracker address"));
            servers.push(tokio::spawn(serve_barrier_udp_tracker(
                tracker,
                barrier.clone(),
                41_000 + offset,
            )));
        }
        let trackers = tracker_addresses
            .iter()
            .map(|address| format!("&tr=udp%3A%2F%2F{address}"))
            .collect::<String>();
        let magnet = Magnet::parse(&format!(
            "magnet:?xt=urn:btih:{}{trackers}",
            "00".repeat(20)
        ))
        .expect("parse concurrent tracker magnet");
        let control = DownloadControl::new();
        let activity = Arc::new(RecordingActivitySink::default());
        control.set_activity_sink(activity.clone());
        let mut peers = TorrentPeerCoordinator::from_magnet(
            &magnet,
            loopback_network(Duration::from_secs(1)),
            control,
            None,
        )
        .await
        .expect("start concurrent trackers");

        timeout(Duration::from_secs(2), async {
            for _ in 0..3 {
                peers
                    .receive_tracker_peers()
                    .await
                    .expect("receive concurrent tracker peers");
            }
        })
        .await
        .expect("concurrent tracker result deadline");

        assert_eq!(peers.registry.len(), 3);
        let succeeded = {
            let events = activity
                .events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            events
                .iter()
                .filter(|event| {
                    matches!(
                        event,
                        DownloadActivityEvent::TrackerAnnounceSucceeded { peer_count: 1, .. }
                    )
                })
                .count()
        };
        assert_eq!(succeeded, 3);

        peers
            .shutdown_tracker()
            .await
            .expect("stop concurrent trackers");
        for server in servers {
            server.await.expect("concurrent tracker server");
        }
    }

    #[tokio::test]
    async fn initial_tracker_operations_hold_the_ceiling_and_advance_on_failure() {
        let tracker_count = super::MAX_CONCURRENT_TRACKER_OPERATIONS + 1;
        let started = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Semaphore::new(0));
        let mut tracker_addresses = Vec::new();
        let mut servers = Vec::new();
        for offset in 0..tracker_count {
            let tracker = UdpSocket::bind("127.0.0.1:0")
                .await
                .expect("bind bounded startup tracker");
            tracker_addresses.push(tracker.local_addr().expect("bounded tracker address"));
            servers.push(tokio::spawn(serve_bounded_startup_tracker(
                tracker,
                started.clone(),
                release.clone(),
                42_000 + u16::try_from(offset).expect("bounded peer port"),
            )));
        }
        let trackers = tracker_addresses
            .iter()
            .map(|address| format!("&tr=udp%3A%2F%2F{address}"))
            .collect::<String>();
        let magnet = Magnet::parse(&format!(
            "magnet:?xt=urn:btih:{}{trackers}",
            "00".repeat(20)
        ))
        .expect("parse bounded tracker magnet");
        let mut peers = TorrentPeerCoordinator::from_magnet(
            &magnet,
            loopback_network(Duration::from_secs(1)),
            DownloadControl::new(),
            None,
        )
        .await
        .expect("start bounded trackers");

        timeout(Duration::from_secs(1), async {
            while started.load(Ordering::Acquire) < super::MAX_CONCURRENT_TRACKER_OPERATIONS {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("fill tracker operation ceiling");
        sleep(Duration::from_millis(25)).await;
        assert_eq!(
            started.load(Ordering::Acquire),
            super::MAX_CONCURRENT_TRACKER_OPERATIONS
        );
        release.add_permits(tracker_count);

        timeout(Duration::from_secs(2), peers.receive_tracker_peers())
            .await
            .expect("bounded tracker result deadline")
            .expect("last startup tracker succeeds");
        assert_eq!(started.load(Ordering::Acquire), tracker_count);
        assert_eq!(peers.registry.len(), 1);

        peers
            .shutdown_tracker()
            .await
            .expect("stop bounded trackers");
        let mut successes = 0;
        for server in servers {
            successes += usize::from(server.await.expect("bounded tracker server"));
        }
        assert_eq!(successes, 1);
    }

    #[tokio::test]
    async fn concurrent_tracker_cancellation_joins_and_releases_every_socket() {
        let mut trackers = Vec::new();
        let mut tracker_addresses = Vec::new();
        for _ in 0..3 {
            let tracker = UdpSocket::bind("127.0.0.1:0")
                .await
                .expect("bind silent concurrent tracker");
            tracker_addresses.push(
                tracker
                    .local_addr()
                    .expect("silent concurrent tracker address"),
            );
            trackers.push(tracker);
        }
        let tracker_parameters = tracker_addresses
            .iter()
            .map(|address| format!("&tr=udp%3A%2F%2F{address}"))
            .collect::<String>();
        let magnet = Magnet::parse(&format!(
            "magnet:?xt=urn:btih:{}{tracker_parameters}",
            "00".repeat(20)
        ))
        .expect("parse silent concurrent trackers");
        let control = DownloadControl::new();
        let activity = Arc::new(RecordingActivitySink::default());
        control.set_activity_sink(activity.clone());
        let manager = TrackerManager::start(
            magnet.udp_trackers,
            magnet.info_hash,
            NetworkPolicy::LoopbackOnly,
            control,
        )
        .expect("start silent concurrent trackers");
        let mut client_addresses = Vec::new();
        for tracker in &trackers {
            let mut packet = [0; 32];
            let (length, client) = timeout(Duration::from_secs(1), tracker.recv_from(&mut packet))
                .await
                .expect("concurrent connect deadline")
                .expect("receive concurrent connect");
            assert_eq!(length, 16);
            client_addresses.push(client);
        }

        manager
            .shutdown()
            .await
            .expect("shutdown concurrent tracker manager");
        {
            let events = activity
                .events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            assert!(events.iter().any(|event| matches!(
                event,
                DownloadActivityEvent::TrackerState(snapshot)
                    if snapshot.active
                        && snapshot.records.iter().any(|record| matches!(
                            record.status,
                            crate::TrackerRuntimeStatus::Announcing
                        ))
            )));
            let terminal = events.iter().rev().find_map(|event| match event {
                DownloadActivityEvent::TrackerState(snapshot) => Some(snapshot),
                _ => None,
            });
            assert!(terminal.is_some_and(|snapshot| {
                !snapshot.active
                    && snapshot.records.iter().all(|record| {
                        matches!(record.status, crate::TrackerRuntimeStatus::Inactive)
                    })
            }));
        }
        for client in client_addresses {
            UdpSocket::bind(client)
                .await
                .expect("concurrent tracker client socket released");
        }
    }

    #[tokio::test]
    async fn zero_peer_success_waits_for_reannounce_without_tracker_failure() {
        let tracker = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind empty tracker");
        let tracker_address = tracker.local_addr().expect("empty tracker address");
        let server = tokio::spawn(serve_empty_udp_tracker(tracker));
        let magnet = Magnet::parse(&format!(
            "magnet:?xt=urn:btih:{}&tr=udp%3A%2F%2F{tracker_address}",
            "00".repeat(20)
        ))
        .expect("parse empty tracker magnet");
        let control = DownloadControl::new();
        let activity = Arc::new(RecordingActivitySink::default());
        control.set_activity_sink(activity.clone());
        let mut peers = TorrentPeerCoordinator::from_magnet(
            &magnet,
            loopback_network(Duration::from_secs(1)),
            control,
            None,
        )
        .await
        .expect("start empty tracker");

        timeout(Duration::from_secs(1), peers.receive_tracker_peers())
            .await
            .expect("empty tracker result deadline")
            .expect("valid empty tracker result");
        assert!(peers.registry.is_empty());
        timeout(Duration::from_secs(1), async {
            loop {
                let has_reannounce = activity
                    .events
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .iter()
                    .any(|event| {
                        matches!(
                            event,
                            DownloadActivityEvent::TrackerReannounceScheduled { .. }
                        )
                    });
                if has_reannounce {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("reannounce diagnostic deadline");
        {
            let events = activity
                .events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            assert!(events.iter().any(|event| matches!(
                event,
                DownloadActivityEvent::TrackerAnnounceSucceeded { peer_count: 0, .. }
            )));
            assert!(events.iter().any(|event| matches!(
                event,
                DownloadActivityEvent::TrackerPeersUnavailable { peer_count: 0, .. }
            )));
            assert!(
                !events.iter().any(|event| matches!(
                    event,
                    DownloadActivityEvent::TrackerAnnounceFailed { .. }
                ))
            );
        }
        peers.shutdown_tracker().await.expect("stop empty tracker");
        server.await.expect("empty tracker server");
    }

    #[test]
    fn udp_tracker_tokens_expire_after_the_protocol_lifetime() {
        let address = "127.0.0.1:6969".parse().expect("tracker address");
        let inserted_at = Instant::now();
        let mut tokens = UdpTrackerTokenCache::default();
        tokens.insert(address, 42, inserted_at);

        assert_eq!(
            tokens.get(address, inserted_at + Duration::from_secs(59)),
            Some(42)
        );
        assert_eq!(
            tokens.get(address, inserted_at + Duration::from_secs(60)),
            None
        );
    }

    #[tokio::test]
    async fn udp_tracker_retransmits_reuses_token_and_cancels_cleanly() {
        let tracker = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind scripted tracker");
        let tracker_address = tracker.local_addr().expect("tracker address");
        let announced_port = 41_234;
        let server = tokio::spawn(async move {
            let mut packet = [0; 256];

            let (first_connect, first_client) =
                timeout(Duration::from_secs(1), tracker.recv_from(&mut packet))
                    .await
                    .expect("first connect deadline")
                    .expect("first connect");
            assert_eq!(first_connect, 16);
            let connect_transaction =
                u32::from_be_bytes(packet[12..16].try_into().expect("connect transaction"));

            let (second_connect, second_client) =
                timeout(Duration::from_secs(1), tracker.recv_from(&mut packet))
                    .await
                    .expect("retransmitted connect deadline")
                    .expect("retransmitted connect");
            assert_eq!(second_connect, 16);
            assert_eq!(second_client, first_client);
            assert_eq!(
                u32::from_be_bytes(packet[12..16].try_into().expect("connect transaction")),
                connect_transaction
            );
            let connection_id = 0x0102_0304_0506_0708_u64;
            let mut connect_response = Vec::from(0_u32.to_be_bytes());
            connect_response.extend_from_slice(&connect_transaction.to_be_bytes());
            connect_response.extend_from_slice(&connection_id.to_be_bytes());
            tracker
                .send_to(&connect_response, first_client)
                .await
                .expect("connect response");

            let (first_announce, announce_client) =
                timeout(Duration::from_secs(1), tracker.recv_from(&mut packet))
                    .await
                    .expect("first announce deadline")
                    .expect("first announce");
            assert_eq!(first_announce, 98);
            assert_eq!(
                u32::from_be_bytes(packet[80..84].try_into().expect("started event")),
                AnnounceEvent::Started as u32
            );
            assert_eq!(
                u16::from_be_bytes(packet[96..98].try_into().expect("announced port")),
                announced_port
            );
            let announce_transaction =
                u32::from_be_bytes(packet[12..16].try_into().expect("announce transaction"));

            let (second_announce, second_announce_client) =
                timeout(Duration::from_secs(1), tracker.recv_from(&mut packet))
                    .await
                    .expect("retransmitted announce deadline")
                    .expect("retransmitted announce");
            assert_eq!(second_announce, 98);
            assert_eq!(second_announce_client, announce_client);
            assert_eq!(
                u32::from_be_bytes(packet[12..16].try_into().expect("announce transaction")),
                announce_transaction
            );
            assert_eq!(
                u16::from_be_bytes(packet[96..98].try_into().expect("announced port")),
                announced_port
            );
            let mut announce_response = Vec::from(1_u32.to_be_bytes());
            announce_response.extend_from_slice(&announce_transaction.to_be_bytes());
            announce_response.extend_from_slice(&600_u32.to_be_bytes());
            announce_response.extend_from_slice(&0_u32.to_be_bytes());
            announce_response.extend_from_slice(&0_u32.to_be_bytes());
            tracker
                .send_to(&announce_response, announce_client)
                .await
                .expect("first announce response");

            let (cached_announce, cached_client) =
                timeout(Duration::from_secs(1), tracker.recv_from(&mut packet))
                    .await
                    .expect("cached announce deadline")
                    .expect("cached announce");
            assert_eq!(cached_announce, 98, "cached token should skip connect");
            assert_eq!(
                u64::from_be_bytes(packet[0..8].try_into().expect("connection ID")),
                connection_id
            );
            assert_eq!(
                u32::from_be_bytes(packet[80..84].try_into().expect("ordinary event")),
                AnnounceEvent::None as u32
            );
            assert_eq!(
                u16::from_be_bytes(packet[96..98].try_into().expect("announced port")),
                announced_port
            );
            let cached_transaction =
                u32::from_be_bytes(packet[12..16].try_into().expect("announce transaction"));
            announce_response[4..8].copy_from_slice(&cached_transaction.to_be_bytes());
            tracker
                .send_to(&announce_response, cached_client)
                .await
                .expect("cached announce response");
        });

        let timing = UdpTrackerTiming {
            retransmit_after: Duration::from_millis(20),
            completion_timeout: Duration::from_millis(100),
        };
        let control = DownloadControl::new();
        let activity = Arc::new(RecordingActivitySink::default());
        control.set_activity_sink(activity.clone());
        let mut tokens = UdpTrackerTokenCache::default();
        let first = announce_udp_tracker_address(
            tracker_address,
            &mut tokens,
            UdpTrackerAnnounce {
                info_hash: [7; 20],
                key: 1,
                event: AnnounceEvent::Started,
                port: announced_port,
            },
            UdpTrackerExchange {
                timing,
                control: &control,
                tracker_label: "udp://127.0.0.1",
            },
        )
        .await
        .expect("loss-recovered announce");
        assert!(first.peers.is_empty());
        let second = announce_udp_tracker_address(
            tracker_address,
            &mut tokens,
            UdpTrackerAnnounce {
                info_hash: [7; 20],
                key: 1,
                event: AnnounceEvent::None,
                port: announced_port,
            },
            UdpTrackerExchange {
                timing: UdpTrackerTiming {
                    retransmit_after: Duration::from_millis(200),
                    completion_timeout: Duration::from_secs(1),
                },
                control: &control,
                tracker_label: "udp://127.0.0.1",
            },
        )
        .await
        .expect("cached-token announce");
        assert!(second.peers.is_empty());
        server.await.expect("scripted tracker");

        let retransmissions = {
            let events = activity
                .events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            events
                .iter()
                .filter(|event| {
                    matches!(event, DownloadActivityEvent::TrackerUdpRetransmitted { .. })
                })
                .count()
        };
        assert_eq!(retransmissions, 2);

        assert_tracker_wait_cancels_without_socket_leaks().await;
    }

    #[tokio::test]
    async fn stalled_metadata_peer_does_not_delay_useful_peer() {
        let payload = b"parallel verified metadata".to_vec();
        let info = single_file_info(&payload);
        let info_hash: [u8; 20] = Sha1::digest(&info).into();
        let stalled_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind stalled metadata peer");
        let stalled_address = stalled_listener
            .local_addr()
            .expect("stalled metadata address");
        let stalled_task = tokio::spawn(serve_stalled_metadata_peer(
            stalled_listener,
            info_hash,
            info.len(),
        ));
        let useful_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind useful metadata peer");
        let useful_address = useful_listener
            .local_addr()
            .expect("useful metadata address");
        let useful_task = tokio::spawn(serve_metadata_then_piece(
            useful_listener,
            info,
            payload.clone(),
            vec![0x80],
        ));
        let magnet = format!(
            "magnet:?xt=urn:btih:{}&x.pe={stalled_address}&x.pe={useful_address}",
            hex(&info_hash)
        );
        let parsed = Magnet::parse(&magnet).expect("parse parallel metadata magnet");
        let network = loopback_network(Duration::from_secs(5));
        let mut peers =
            TorrentPeerCoordinator::from_magnet(&parsed, network, DownloadControl::new(), None)
                .await
                .expect("resolve metadata peers");

        let (raw_info, metainfo) =
            timeout(Duration::from_secs(4), peers.acquire_metadata(info_hash))
                .await
                .expect("stalled metadata peer must not set the completion deadline")
                .expect("useful metadata peer supplies verified metadata");

        assert_eq!(raw_info, single_file_info(&payload));
        assert_eq!(metainfo.info_hash, info_hash);
        let stalled = peers
            .registry
            .find_endpoint(PeerEndpoint::new(stalled_address).expect("stalled endpoint"))
            .expect("stalled peer retained");
        assert_eq!(stalled.phase(), PeerPhase::Idle);
        assert_eq!(stalled.history().dial_attempts, 1);
        assert_eq!(stalled.history().total_failures, 0);
        peers.close_current(None).expect("close metadata winner");
        for task in [stalled_task, useful_task] {
            timeout(Duration::from_secs(1), task)
                .await
                .expect("metadata peer joined")
                .expect("metadata peer task");
        }
    }

    #[tokio::test]
    async fn metadata_cancellation_publishes_empty_peers_after_joined_cleanup() {
        let payload = b"cancelled metadata owner".to_vec();
        let info = single_file_info(&payload);
        let info_hash: [u8; 20] = Sha1::digest(&info).into();
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind cancelled metadata peer");
        let address = listener.local_addr().expect("metadata peer address");
        let peer_task = tokio::spawn(serve_stalled_metadata_peer(listener, info_hash, info.len()));
        let control = DownloadControl::new();
        let activity = Arc::new(RecordingActivitySink::default());
        control.set_activity_sink(activity.clone());
        let task_control = control.clone();
        let task = tokio::spawn(download_magnet_metadata_with_control(
            format!("magnet:?xt=urn:btih:{}&x.pe={address}", hex(&info_hash)),
            loopback_network(Duration::from_secs(5)),
            task_control,
        ));

        timeout(Duration::from_secs(1), async {
            loop {
                let diagnostics = control.diagnostic_snapshot();
                if diagnostics
                    .peer_connections
                    .iter()
                    .any(|peer| peer.lifecycle == PeerConnectionLifecycle::Connected)
                    && diagnostics.metadata.total_requests_sent > 0
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("metadata peer reached connected state");

        control.cancel();
        let result = timeout(Duration::from_secs(1), task)
            .await
            .expect("metadata cancellation joined")
            .expect("metadata task");
        assert!(matches!(result, Err(DownloadError::Cancelled)));
        timeout(Duration::from_secs(1), peer_task)
            .await
            .expect("metadata peer closed before terminal result")
            .expect("metadata peer task");

        let diagnostics = control.diagnostic_snapshot();
        assert!(diagnostics.peer_connections.is_empty());
        assert_eq!(diagnostics.metadata.pending_dials, 0);
        assert_eq!(diagnostics.metadata.active_workers, 0);
        let events = activity
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let peer_snapshots = events
            .iter()
            .filter_map(|event| match event {
                DownloadActivityEvent::PeerConnections { peers, .. } => Some(peers.as_slice()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(peer_snapshots.iter().any(|peers| {
            peers
                .iter()
                .any(|peer| peer.lifecycle == PeerConnectionLifecycle::Connected)
        }));
        assert!(peer_snapshots.iter().any(|peers| {
            peers
                .iter()
                .any(|peer| peer.lifecycle == PeerConnectionLifecycle::Disconnecting)
        }));
        assert!(peer_snapshots.last().is_some_and(|peers| peers.is_empty()));
    }

    #[tokio::test]
    async fn metadata_blocks_from_multiple_peers_complete_one_dictionary() {
        let payload = vec![0x5a; 1_700];
        let info = single_file_info_with_piece_length(&payload, 1);
        assert!(
            info.len() > 2 * rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH,
            "fixture must span three metadata blocks"
        );
        let info_hash: [u8; 20] = Sha1::digest(&info).into();
        let partial_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind partial metadata peer");
        let partial_address = partial_listener.local_addr().expect("partial address");
        let complete_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind complementary metadata peer");
        let complete_address = complete_listener.local_addr().expect("complete address");
        let partial_task = tokio::spawn(serve_partial_metadata_peer(
            partial_listener,
            info.clone(),
            true,
        ));
        let complete_task = tokio::spawn(serve_partial_metadata_peer(
            complete_listener,
            info.clone(),
            false,
        ));
        let magnet = format!(
            "magnet:?xt=urn:btih:{}&x.pe={partial_address}&x.pe={complete_address}",
            hex(&info_hash)
        );
        let parsed = Magnet::parse(&magnet).expect("parse multi-source metadata magnet");
        let control = DownloadControl::new();
        let mut peers = TorrentPeerCoordinator::from_magnet(
            &parsed,
            loopback_network(Duration::from_secs(2)),
            control.clone(),
            None,
        )
        .await
        .expect("resolve multi-source metadata peers");

        let (raw_info, metainfo) =
            timeout(Duration::from_secs(3), peers.acquire_metadata(info_hash))
                .await
                .expect("multi-source metadata completion bound")
                .expect("combine metadata blocks across peers");
        assert_eq!(raw_info, info);
        assert_eq!(metainfo.info_hash, info_hash);
        let snapshot = control.diagnostic_snapshot().metadata;
        assert_eq!(snapshot.total_blocks_received, 3);
        assert!(
            snapshot
                .recent_attempts
                .iter()
                .filter(|peer| peer.blocks_received > 0)
                .count()
                >= 2
        );

        peers.close_current(None).expect("close metadata winner");
        for task in [partial_task, complete_task] {
            timeout(Duration::from_secs(1), task)
                .await
                .expect("multi-source peer joined")
                .expect("multi-source peer task");
        }
    }

    #[tokio::test]
    async fn corrupt_metadata_generation_resets_before_clean_peer_completes() {
        let payload = vec![0x39; 1_700];
        let info = single_file_info_with_piece_length(&payload, 1);
        assert!(
            info.len() > 2 * rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH,
            "fixture must span three metadata blocks"
        );
        let info_hash: [u8; 20] = Sha1::digest(&info).into();
        let mut corrupt = info.clone();
        corrupt[rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH + 7] ^= 0x01;

        let corrupt_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind corrupt metadata peer");
        let corrupt_address = corrupt_listener.local_addr().expect("corrupt address");
        let clean_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind clean metadata peer");
        let clean_address = clean_listener.local_addr().expect("clean address");
        let corrupt_task = tokio::spawn(serve_metadata_bytes_after_delay(
            corrupt_listener,
            info_hash,
            corrupt,
            Duration::ZERO,
        ));
        let clean_task = tokio::spawn(serve_metadata_bytes_after_delay(
            clean_listener,
            info_hash,
            info.clone(),
            Duration::from_millis(200),
        ));
        let magnet = format!(
            "magnet:?xt=urn:btih:{}&x.pe={corrupt_address}&x.pe={clean_address}",
            hex(&info_hash)
        );
        let parsed = Magnet::parse(&magnet).expect("parse corrupt recovery magnet");
        let control = DownloadControl::new();
        let mut peers = TorrentPeerCoordinator::from_magnet(
            &parsed,
            loopback_network(Duration::from_secs(2)),
            control.clone(),
            None,
        )
        .await
        .expect("resolve corrupt recovery peers");

        let (raw_info, metainfo) =
            timeout(Duration::from_secs(3), peers.acquire_metadata(info_hash))
                .await
                .expect("corrupt metadata recovery bound")
                .expect("clean source completes after corrupt generation");
        assert_eq!(raw_info, info);
        assert_eq!(metainfo.info_hash, info_hash);
        let snapshot = control.diagnostic_snapshot().metadata;
        assert_eq!(snapshot.total_hash_failures, 1);
        assert_eq!(snapshot.last_hash_failure_contributors, 1);
        assert_eq!(snapshot.total_blocks_received, 6);

        peers
            .close_current(None)
            .expect("close clean metadata winner");
        for task in [corrupt_task, clean_task] {
            timeout(Duration::from_secs(1), task)
                .await
                .expect("corrupt recovery peer joined")
                .expect("corrupt recovery peer task");
        }
    }

    #[tokio::test]
    async fn metadata_requests_ramp_for_one_at_a_time_peer() {
        let payload = vec![0x71; 1_000];
        let info = single_file_info_with_piece_length(&payload, 1);
        assert!(
            info.len() > rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH
                && info.len() <= 2 * rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH,
            "fixture must span exactly two metadata blocks"
        );
        let info_hash: [u8; 20] = Sha1::digest(&info).into();
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind one-at-a-time metadata peer");
        let address = listener.local_addr().expect("one-at-a-time address");
        let server = tokio::spawn(serve_one_at_a_time_metadata_peer(listener, info.clone()));
        let magnet = format!("magnet:?xt=urn:btih:{}&x.pe={address}", hex(&info_hash));
        let parsed = Magnet::parse(&magnet).expect("parse one-at-a-time magnet");
        let control = DownloadControl::new();
        let mut peers = TorrentPeerCoordinator::from_magnet(
            &parsed,
            loopback_network(Duration::from_secs(2)),
            control.clone(),
            None,
        )
        .await
        .expect("resolve one-at-a-time peer");

        let (raw_info, metainfo) =
            timeout(Duration::from_secs(2), peers.acquire_metadata(info_hash))
                .await
                .expect("one-at-a-time metadata completion bound")
                .expect("pace requests until first response");
        assert_eq!(raw_info, info);
        assert_eq!(metainfo.info_hash, info_hash);
        let snapshot = control.diagnostic_snapshot().metadata;
        assert_eq!(snapshot.total_requests_sent, 2);
        assert_eq!(snapshot.total_blocks_received, 2);

        peers.close_current(None).expect("close metadata winner");
        timeout(Duration::from_secs(1), server)
            .await
            .expect("one-at-a-time peer joined")
            .expect("one-at-a-time peer task");
    }

    #[tokio::test]
    async fn peers_without_ut_metadata_release_slots_and_remain_diagnosable() {
        let payload = b"diagnosable metadata failover".to_vec();
        let info = single_file_info(&payload);
        let info_hash: [u8; 20] = Sha1::digest(&info).into();
        let mut missing_addresses = Vec::new();
        let mut missing_tasks = Vec::new();
        for _ in 0..MAX_METADATA_PEERS {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind metadata-incapable peer");
            missing_addresses.push(listener.local_addr().expect("missing metadata address"));
            missing_tasks.push(tokio::spawn(serve_metadata_peer_without_ut_metadata(
                listener, info_hash,
            )));
        }
        let useful_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind useful metadata peer");
        let useful_address = useful_listener
            .local_addr()
            .expect("useful metadata address");
        let useful_task = tokio::spawn(serve_metadata_then_piece(
            useful_listener,
            info.clone(),
            payload,
            vec![0x80],
        ));
        let mut magnet = format!("magnet:?xt=urn:btih:{}", hex(&info_hash));
        for address in &missing_addresses {
            magnet.push_str(&format!("&x.pe={address}"));
        }
        magnet.push_str(&format!("&x.pe={useful_address}"));
        let parsed = Magnet::parse(&magnet).expect("parse diagnostic metadata magnet");
        let control = DownloadControl::new();
        let mut peers = TorrentPeerCoordinator::from_magnet(
            &parsed,
            loopback_network(Duration::from_secs(1)),
            control.clone(),
            None,
        )
        .await
        .expect("resolve diagnostic metadata peers");

        let (raw_info, _) = timeout(Duration::from_secs(1), peers.acquire_metadata(info_hash))
            .await
            .expect("metadata-incapable peers must release all slots")
            .expect("later useful peer supplies metadata");
        assert_eq!(raw_info, info);

        let snapshot = control.diagnostic_snapshot().metadata;
        assert_eq!(snapshot.phase, MetadataAcquisitionPhase::Complete);
        assert_eq!(snapshot.total_attempts, MAX_METADATA_PEERS + 1);
        assert_eq!(snapshot.total_requests_sent, 1);
        assert_eq!(snapshot.total_blocks_received, 1);
        assert_eq!(snapshot.active_attempts, Vec::new());
        assert_eq!(
            snapshot
                .recent_attempts
                .iter()
                .filter(|peer| peer.stage == MetadataPeerStage::Failed)
                .count(),
            MAX_METADATA_PEERS
        );
        assert!(snapshot.recent_attempts.iter().any(|peer| {
            peer.stage == MetadataPeerStage::Complete
                && peer.remote_metadata_id == Some(UT_METADATA_LOCAL_ID)
                && peer.blocks_received == 1
        }));
        assert!(
            snapshot
                .recent_attempts
                .iter()
                .filter(|peer| {
                    peer.terminal_detail
                        .as_deref()
                        .is_some_and(|detail| detail.contains("does not advertise"))
                })
                .count()
                >= MAX_METADATA_PEERS
        );
        let registry = snapshot.registry.expect("peer registry snapshot");
        assert_eq!(registry.counts.total, MAX_METADATA_PEERS + 1);

        peers.close_current(None).expect("close metadata winner");
        for task in missing_tasks.into_iter().chain([useful_task]) {
            timeout(Duration::from_secs(1), task)
                .await
                .expect("metadata fixture joined")
                .expect("metadata fixture task");
        }
    }

    #[tokio::test]
    async fn unrelated_messages_cannot_hold_every_metadata_slot() {
        let payload = b"metadata after bounded chatter".to_vec();
        let info = single_file_info(&payload);
        let info_hash: [u8; 20] = Sha1::digest(&info).into();
        let mut chatter_addresses = Vec::new();
        let mut chatter_tasks = Vec::new();
        for _ in 0..MAX_METADATA_PEERS {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind chattering peer");
            chatter_addresses.push(listener.local_addr().expect("chattering peer address"));
            chatter_tasks.push(tokio::spawn(
                serve_chattering_peer_without_extension_handshake(listener, info_hash),
            ));
        }
        let useful_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind useful metadata peer");
        let useful_address = useful_listener
            .local_addr()
            .expect("useful metadata address");
        let useful_task = tokio::spawn(serve_metadata_then_piece(
            useful_listener,
            info.clone(),
            payload,
            vec![0x80],
        ));
        let mut magnet = format!("magnet:?xt=urn:btih:{}", hex(&info_hash));
        for address in &chatter_addresses {
            magnet.push_str(&format!("&x.pe={address}"));
        }
        magnet.push_str(&format!("&x.pe={useful_address}"));
        let parsed = Magnet::parse(&magnet).expect("parse chattering metadata magnet");
        let control = DownloadControl::new();
        let mut peers = TorrentPeerCoordinator::from_magnet(
            &parsed,
            loopback_network(Duration::from_millis(150)),
            control.clone(),
            None,
        )
        .await
        .expect("resolve chattering metadata peers");

        let (raw_info, _) = timeout(Duration::from_secs(2), peers.acquire_metadata(info_hash))
            .await
            .expect("metadata progress deadline releases chattering peers")
            .expect("later useful peer supplies metadata");
        assert_eq!(raw_info, info);
        let snapshot = control.diagnostic_snapshot().metadata;
        assert_eq!(snapshot.phase, MetadataAcquisitionPhase::Complete);
        assert_eq!(snapshot.total_attempts, MAX_METADATA_PEERS + 1);
        assert!(
            snapshot
                .recent_attempts
                .iter()
                .filter_map(|peer| peer.terminal_detail.as_deref())
                .filter(|detail| detail.contains("metadata progress timed out"))
                .count()
                >= MAX_METADATA_PEERS
        );
        assert!(snapshot.recent_attempts.iter().any(|peer| {
            peer.stage == MetadataPeerStage::Complete && peer.blocks_received == 1
        }));

        peers.close_current(None).expect("close metadata winner");
        for task in chatter_tasks.into_iter().chain([useful_task]) {
            timeout(Duration::from_secs(1), task)
                .await
                .expect("metadata fixture joined")
                .expect("metadata fixture task");
        }
    }

    #[tokio::test]
    async fn metadata_rejections_release_slots_and_are_counted() {
        let payload = b"metadata after explicit rejects".to_vec();
        let info = single_file_info(&payload);
        let info_hash: [u8; 20] = Sha1::digest(&info).into();
        let mut rejecting_addresses = Vec::new();
        let mut rejecting_tasks = Vec::new();
        for _ in 0..MAX_METADATA_PEERS {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind rejecting peer");
            rejecting_addresses.push(listener.local_addr().expect("rejecting peer address"));
            rejecting_tasks.push(tokio::spawn(serve_metadata_rejecting_peer(
                listener,
                info_hash,
                info.len(),
            )));
        }
        let useful_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind useful metadata peer");
        let useful_address = useful_listener
            .local_addr()
            .expect("useful metadata address");
        let useful_task = tokio::spawn(serve_metadata_then_piece(
            useful_listener,
            info.clone(),
            payload,
            vec![0x80],
        ));
        let mut magnet = format!("magnet:?xt=urn:btih:{}", hex(&info_hash));
        for address in &rejecting_addresses {
            magnet.push_str(&format!("&x.pe={address}"));
        }
        magnet.push_str(&format!("&x.pe={useful_address}"));
        let parsed = Magnet::parse(&magnet).expect("parse rejecting metadata magnet");
        let control = DownloadControl::new();
        let mut peers = TorrentPeerCoordinator::from_magnet(
            &parsed,
            loopback_network(Duration::from_secs(1)),
            control.clone(),
            None,
        )
        .await
        .expect("resolve rejecting metadata peers");

        let (raw_info, _) = timeout(Duration::from_secs(1), peers.acquire_metadata(info_hash))
            .await
            .expect("rejecting peers must release all slots")
            .expect("later useful peer supplies metadata");
        assert_eq!(raw_info, info);
        let snapshot = control.diagnostic_snapshot().metadata;
        assert_eq!(snapshot.phase, MetadataAcquisitionPhase::Complete);
        assert_eq!(snapshot.total_attempts, MAX_METADATA_PEERS + 1);
        let rejected_requests = snapshot
            .recent_attempts
            .iter()
            .map(|peer| peer.rejects_received)
            .sum::<usize>();
        assert!((1..=MAX_METADATA_PEERS).contains(&rejected_requests));
        assert!(snapshot.recent_attempts.iter().any(|peer| {
            peer.stage == MetadataPeerStage::Complete && peer.blocks_received == 1
        }));

        peers.close_current(None).expect("close metadata winner");
        for task in rejecting_tasks.into_iter().chain([useful_task]) {
            timeout(Duration::from_secs(1), task)
                .await
                .expect("metadata fixture joined")
                .expect("metadata fixture task");
        }
    }

    #[tokio::test]
    async fn tracker_discovery_continues_while_metadata_peer_stalls() {
        let payload = b"late tracker metadata".to_vec();
        let info = single_file_info(&payload);
        let info_hash: [u8; 20] = Sha1::digest(&info).into();
        let stalled_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind stalled metadata peer");
        let stalled_address = stalled_listener
            .local_addr()
            .expect("stalled metadata address");
        let stalled_task = tokio::spawn(serve_stalled_metadata_peer(
            stalled_listener,
            info_hash,
            info.len(),
        ));
        let useful_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind tracker metadata peer");
        let useful_address = useful_listener
            .local_addr()
            .expect("tracker metadata address");
        let useful_task = tokio::spawn(serve_metadata_then_piece(
            useful_listener,
            info,
            payload,
            vec![0x80],
        ));
        let unavailable_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind unavailable placeholder");
        let unavailable = unavailable_listener
            .local_addr()
            .expect("unavailable address");
        drop(unavailable_listener);
        let tracker = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind delayed tracker");
        let tracker_address = tracker.local_addr().expect("tracker address");
        let tracker_task = tokio::spawn(serve_one_shot_udp_tracker(
            tracker,
            info_hash,
            unavailable,
            useful_address,
            Duration::from_millis(100),
        ));
        let magnet = Magnet::parse(&format!(
            "magnet:?xt=urn:btih:{}&x.pe={stalled_address}&\
             tr=udp%3A%2F%2F{tracker_address}%2Fannounce",
            hex(&info_hash)
        ))
        .expect("parse late metadata discovery magnet");
        let mut peers = TorrentPeerCoordinator::from_magnet(
            &magnet,
            loopback_network(Duration::from_secs(2)),
            DownloadControl::new(),
            None,
        )
        .await
        .expect("start metadata discovery");

        let (_, metainfo) = timeout(Duration::from_secs(4), peers.acquire_metadata(info_hash))
            .await
            .expect("late tracker peer must be consumed during metadata work")
            .expect("tracker peer supplies metadata");

        assert_eq!(metainfo.info_hash, info_hash);
        let discovered = peers
            .registry
            .find_endpoint(PeerEndpoint::new(useful_address).expect("tracker endpoint"))
            .expect("tracker peer retained");
        assert!(discovered.sources().contains(PeerSource::Tracker));
        peers.close_current(None).expect("close metadata winner");
        peers
            .shutdown_tracker()
            .await
            .expect("shutdown metadata tracker");
        for task in [stalled_task, useful_task, tracker_task] {
            timeout(Duration::from_secs(1), task)
                .await
                .expect("metadata fixture joined")
                .expect("metadata fixture task");
        }
    }

    #[tokio::test]
    async fn magnet_registry_fails_over_and_hands_same_peer_to_content_download() {
        let payload = b"verified magnet payload".to_vec();
        let info = single_file_info(&payload);
        let info_hash: [u8; 20] = Sha1::digest(&info).into();
        let output_path = test_path("magnet-output.bin");
        let unreachable_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind unreachable peer placeholder");
        let unreachable = unreachable_listener
            .local_addr()
            .expect("unreachable peer address");
        drop(unreachable_listener);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind scripted metadata peer");
        let address = listener.local_addr().expect("listener address");
        let peer_task = tokio::spawn(serve_metadata_then_piece(
            listener,
            info,
            payload.clone(),
            vec![0x80],
        ));

        let magnet = format!(
            "magnet:?xt=urn:btih:{}&x.pe={unreachable}&x.pe={address}",
            hex(&info_hash)
        );
        let parsed = Magnet::parse(&magnet).expect("parse failover magnet");
        let network = loopback_network(Duration::from_secs(2));
        let mut peers =
            TorrentPeerCoordinator::from_magnet(&parsed, network, DownloadControl::new(), None)
                .await
                .expect("resolve failover peers");
        assert_eq!(peers.registry.len(), 2);

        let report = run_magnet_download_with_peers(
            MagnetDownloadConfig {
                magnet,
                output_path: output_path.clone(),
                network,
                resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
                skip_files: Vec::new(),
                materialize_files: Vec::new(),
                dht: None,
            },
            DownloadControl::new(),
            parsed,
            &mut peers,
        )
        .await
        .expect("magnet metadata and content after failover");

        let failed = peers
            .registry
            .find_endpoint(PeerEndpoint::new(unreachable).expect("failed endpoint"))
            .expect("failed peer record retained");
        assert_eq!(failed.phase(), PeerPhase::Idle);
        assert_eq!(failed.history().dial_attempts, 1);
        assert_eq!(failed.history().total_failures, 1);
        assert_eq!(failed.history().last_failure, Some(PeerFailure::Connect));
        assert!(failed.history().retry_at.is_some());
        assert!(failed.sources().contains(PeerSource::MagnetHint));

        let connected = peers
            .registry
            .find_endpoint(PeerEndpoint::new(address).expect("connected endpoint"))
            .expect("connected peer record retained");
        assert_eq!(connected.phase(), PeerPhase::Idle);
        assert_eq!(connected.history().dial_attempts, 1);
        assert_eq!(connected.history().total_failures, 0);
        assert!(connected.history().last_connected_at.is_some());
        assert!(connected.history().last_disconnected_at.is_some());
        assert!(connected.sources().contains(PeerSource::MagnetHint));

        assert_eq!(report.info_hash, info_hash);
        assert_eq!(
            tokio::fs::read(&output_path)
                .await
                .expect("published output"),
            payload
        );
        peer_task.await.expect("scripted peer task");
        let _ = tokio::fs::remove_file(output_path).await;
    }

    #[tokio::test]
    async fn public_magnet_entry_starts_tracker_and_uses_peer_registry_path() {
        let payload = b"public entry payload".to_vec();
        let info = single_file_info(&payload);
        let info_hash: [u8; 20] = Sha1::digest(&info).into();
        let output_path = test_path("public-magnet-output.bin");
        let unsupported_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind non-extension peer");
        let unsupported_address = unsupported_listener
            .local_addr()
            .expect("non-extension peer address");
        let unsupported_task = tokio::spawn(async move {
            let (mut stream, _) = unsupported_listener
                .accept()
                .await
                .expect("accept magnet client");
            let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
            stream
                .read_exact(&mut handshake_bytes)
                .await
                .expect("read magnet handshake");
            assert!(
                decode_handshake(&handshake_bytes, info_hash)
                    .expect("valid client handshake")
                    .supports_extensions()
            );
            stream
                .write_all(&encode_handshake_with_reserved(
                    info_hash,
                    *b"-RS-NOEXT-0000000000",
                    [0; 8],
                ))
                .await
                .expect("send non-extension handshake");
        });
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind scripted metadata peer");
        let address = listener.local_addr().expect("listener address");
        let peer_task = tokio::spawn(serve_metadata_then_piece(
            listener,
            info,
            payload.clone(),
            vec![0x80],
        ));
        let unused_tracker = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind unused tracker");
        let unused_tracker_address = unused_tracker.local_addr().expect("unused tracker address");

        let report = download_magnet(MagnetDownloadConfig {
            magnet: format!(
                "magnet:?xt=urn:btih:{}&x.pe={unsupported_address}&x.pe={address}&\
                 tr=udp%3A%2F%2F{unused_tracker_address}",
                hex(&info_hash)
            ),
            output_path: output_path.clone(),
            network: loopback_network(Duration::from_secs(2)),
            resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            materialize_files: Vec::new(),
            dht: None,
        })
        .await
        .expect("public magnet entry");

        assert_eq!(report.info_hash, info_hash);
        assert_eq!(
            tokio::fs::read(&output_path)
                .await
                .expect("published output"),
            payload
        );
        unsupported_task.await.expect("non-extension peer task");
        peer_task.await.expect("scripted peer task");
        let mut tracker_packet = [0; 16];
        let (tracker_length, _) = timeout(
            Duration::from_secs(1),
            unused_tracker.recv_from(&mut tracker_packet),
        )
        .await
        .expect("tracker lifecycle should start alongside explicit hints")
        .expect("receive initial tracker connect");
        assert_eq!(tracker_length, 16);
        let _ = tokio::fs::remove_file(output_path).await;
    }

    #[tokio::test]
    async fn transient_dht_miss_retries_without_becoming_terminal() {
        let info_hash = [8; 20];
        let peer = SocketAddr::from(([127, 0, 0, 1], 49_999));
        let dht_socket = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind scripted DHT");
        let dht_address = dht_socket.local_addr().expect("DHT address");
        let dht_task = tokio::spawn(serve_dht_peer_after_retry(dht_socket, info_hash, peer));
        let dht = DhtService::start(dht_config(dht_address))
            .await
            .expect("start DHT client");
        let control = DownloadControl::new();
        let activity = Arc::new(RecordingActivitySink::default());
        control.set_activity_sink(activity.clone());

        let peers = retrying_dht_lookup(
            dht.handle(),
            info_hash,
            control,
            DhtRetryTiming {
                initial_delay: Duration::from_millis(10),
                maximum_delay: Duration::from_millis(20),
            },
            Duration::ZERO,
        )
        .await
        .expect("retry DHT lookup");

        assert_eq!(peers, vec![peer]);
        {
            let events = activity
                .events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            assert!(
                events
                    .iter()
                    .any(|event| matches!(event, DownloadActivityEvent::DhtRetryScheduled { .. }))
            );
            assert!(events.iter().any(|event| matches!(
                event,
                DownloadActivityEvent::DhtLookupSucceeded { peer_count: 1 }
            )));
        }
        dht_task.await.expect("scripted DHT task");
        dht.shutdown().await.expect("DHT shutdown");
    }

    #[tokio::test]
    async fn trackerless_dht_peer_completes_metadata_and_content_path() {
        let payload = b"peer discovered through DHT".to_vec();
        let info = single_file_info(&payload);
        let info_hash: [u8; 20] = Sha1::digest(&info).into();
        let output_path = test_path("dht-magnet-output.bin");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind DHT-discovered peer");
        let peer_address = listener.local_addr().expect("peer address");
        let peer_task = tokio::spawn(serve_metadata_then_piece(
            listener,
            info,
            payload.clone(),
            vec![0x80],
        ));
        let dht_socket = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind scripted DHT");
        let dht_address = dht_socket.local_addr().expect("DHT address");
        let dht_task = tokio::spawn(serve_dht_peer(dht_socket, info_hash, peer_address));
        let dht = DhtService::start(dht_config(dht_address))
            .await
            .expect("start DHT client");

        let report = download_magnet(MagnetDownloadConfig {
            magnet: format!("magnet:?xt=urn:btih:{}", hex(&info_hash)),
            output_path: output_path.clone(),
            network: loopback_network(Duration::from_secs(2)),
            resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            materialize_files: Vec::new(),
            dht: Some(dht.handle()),
        })
        .await
        .expect("DHT-discovered download");

        assert_eq!(report.info_hash, info_hash);
        assert_eq!(
            tokio::fs::read(&output_path)
                .await
                .expect("published output"),
            payload
        );
        dht_task.await.expect("scripted DHT task");
        peer_task.await.expect("scripted peer task");
        dht.shutdown().await.expect("DHT shutdown");
        let _ = tokio::fs::remove_file(output_path).await;
    }

    #[tokio::test]
    async fn verified_private_metadata_purges_dht_only_peer_before_content() {
        let payload = b"must not be fetched from decentralized peer".to_vec();
        let info = private_single_file_info(&payload);
        let metainfo = Metainfo::from_info_bytes(&info).expect("private metadata");
        assert!(metainfo.private);
        let info_hash = metainfo.info_hash;
        let output_path = test_path("private-dht-output.bin");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind DHT-only peer");
        let peer_address = listener.local_addr().expect("peer address");
        let peer_task = tokio::spawn(serve_metadata_then_piece(
            listener,
            info,
            payload,
            vec![0x80],
        ));
        let dht_socket = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind scripted DHT");
        let dht_address = dht_socket.local_addr().expect("DHT address");
        let dht_task = tokio::spawn(serve_dht_peer(dht_socket, info_hash, peer_address));
        let dht = DhtService::start(dht_config(dht_address))
            .await
            .expect("start DHT client");

        let result = download_magnet(MagnetDownloadConfig {
            magnet: format!("magnet:?xt=urn:btih:{}", hex(&info_hash)),
            output_path: output_path.clone(),
            network: loopback_network(Duration::from_secs(2)),
            resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            materialize_files: Vec::new(),
            dht: Some(dht.handle()),
        })
        .await;

        assert!(matches!(result, Err(DownloadError::NoUsablePeer)));
        assert!(!tokio::fs::try_exists(&output_path).await.expect("output"));
        dht_task.await.expect("scripted DHT task");
        peer_task.await.expect("scripted peer task");
        dht.shutdown().await.expect("DHT shutdown");
    }

    #[tokio::test]
    async fn invalid_premetadata_bitfield_fails_before_storage_creation() {
        let payload = b"not written".to_vec();
        let info = single_file_info(&payload);
        let info_hash: [u8; 20] = Sha1::digest(&info).into();
        let output_path = test_path("bad-premetadata-output.bin");
        let staging = staging_path(&output_path).expect("staging path");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind scripted metadata peer");
        let address = listener.local_addr().expect("listener address");
        let peer_task = tokio::spawn(serve_metadata_then_piece(
            listener,
            info,
            payload,
            vec![0x80, 0],
        ));

        let result = download_magnet(MagnetDownloadConfig {
            magnet: format!("magnet:?xt=urn:btih:{}&x.pe={address}", hex(&info_hash)),
            output_path: output_path.clone(),
            network: loopback_network(Duration::from_secs(2)),
            resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            materialize_files: Vec::new(),
            dht: None,
        })
        .await;

        assert!(matches!(
            result,
            Err(DownloadError::InvalidPremetadataState(_))
        ));
        assert!(!tokio::fs::try_exists(&output_path).await.expect("output"));
        assert!(!tokio::fs::try_exists(&staging).await.expect("staging"));
        peer_task.abort();
        let _ = peer_task.await;
    }

    #[tokio::test]
    async fn magnet_peer_without_extension_support_fails_before_storage() {
        let info = single_file_info(b"not written");
        let info_hash: [u8; 20] = Sha1::digest(&info).into();
        let output_path = test_path("no-extension-output.bin");
        let staging = staging_path(&output_path).expect("staging path");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind non-extension peer");
        let address = listener.local_addr().expect("listener address");
        let peer_task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept magnet client");
            let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
            stream
                .read_exact(&mut handshake_bytes)
                .await
                .expect("read magnet handshake");
            assert!(
                decode_handshake(&handshake_bytes, info_hash)
                    .expect("valid client handshake")
                    .supports_extensions()
            );
            stream
                .write_all(&encode_handshake_with_reserved(
                    info_hash,
                    *b"-RS-NOEXT-0000000000",
                    [0; 8],
                ))
                .await
                .expect("send non-extension handshake");
        });

        let result = download_magnet(MagnetDownloadConfig {
            magnet: format!("magnet:?xt=urn:btih:{}&x.pe={address}", hex(&info_hash)),
            output_path: output_path.clone(),
            network: loopback_network(Duration::from_secs(2)),
            resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            materialize_files: Vec::new(),
            dht: None,
        })
        .await;

        assert!(matches!(
            result,
            Err(DownloadError::ExtensionProtocolUnsupported)
        ));
        assert!(!tokio::fs::try_exists(&output_path).await.expect("output"));
        assert!(!tokio::fs::try_exists(&staging).await.expect("staging"));
        peer_task.await.expect("non-extension peer task");
    }

    #[tokio::test]
    async fn magnet_peer_disconnect_during_metadata_fails_before_storage() {
        let info = single_file_info(b"not written");
        let info_hash: [u8; 20] = Sha1::digest(&info).into();
        let output_path = test_path("metadata-disconnect-output.bin");
        let staging = staging_path(&output_path).expect("staging path");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind disconnecting peer");
        let address = listener.local_addr().expect("listener address");
        let peer_task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept magnet client");
            let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
            stream
                .read_exact(&mut handshake_bytes)
                .await
                .expect("read magnet handshake");
            decode_handshake(&handshake_bytes, info_hash).expect("valid client handshake");
            let mut reserved = [0; 8];
            reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
            stream
                .write_all(&encode_handshake_with_reserved(
                    info_hash,
                    *b"-RS-DROP--0000000000",
                    reserved,
                ))
                .await
                .expect("send extension handshake");
        });

        let result = download_magnet(MagnetDownloadConfig {
            magnet: format!("magnet:?xt=urn:btih:{}&x.pe={address}", hex(&info_hash)),
            output_path: output_path.clone(),
            network: loopback_network(Duration::from_secs(2)),
            resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            materialize_files: Vec::new(),
            dht: None,
        })
        .await;

        assert!(
            matches!(
                &result,
                Err(DownloadError::PeerClosed)
                    | Err(DownloadError::Io {
                        operation: "read peer message",
                        ..
                    })
            ),
            "unexpected disconnect result: {result:?}"
        );
        assert!(!tokio::fs::try_exists(&output_path).await.expect("output"));
        assert!(!tokio::fs::try_exists(&staging).await.expect("staging"));
        peer_task.await.expect("disconnecting peer task");
    }

    #[tokio::test]
    async fn timeout_removes_unverified_staging_output() {
        let metainfo_path = test_path("fixture.torrent");
        let output_path = test_path("output.bin");
        let staging = staging_path(&output_path).expect("staging path");
        let mut metainfo =
            b"d4:infod6:lengthi1e4:name1:x12:piece lengthi16384e6:pieces20:".to_vec();
        metainfo.extend_from_slice(&[1; 20]);
        metainfo.extend_from_slice(b"ee");
        tokio::fs::write(&metainfo_path, metainfo)
            .await
            .expect("write metainfo");

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind scripted peer");
        let address = listener.local_addr().expect("listener address");
        let peer_task = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.expect("accept diagnostic");
            tokio::time::sleep(Duration::from_secs(1)).await;
        });

        let result = download_verified_piece(DownloadConfig {
            metainfo_path: metainfo_path.clone(),
            peer: address,
            output_path: output_path.clone(),
            network: loopback_network(Duration::from_millis(50)),
            resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            materialize_files: Vec::new(),
        })
        .await;

        assert!(matches!(result, Err(DownloadError::PeerTimedOut { .. })));
        assert!(
            !tokio::fs::try_exists(&output_path)
                .await
                .expect("output status")
        );
        assert!(
            !tokio::fs::try_exists(&staging)
                .await
                .expect("staging status")
        );

        peer_task.abort();
        let _ = peer_task.await;
        let _ = tokio::fs::remove_file(metainfo_path).await;
    }

    #[tokio::test]
    async fn selective_timeout_removes_owned_staging_and_part_paths() {
        let metainfo_path = test_path("selective-timeout.torrent");
        let output_path = test_path("selective-timeout");
        let staging = selective_staging_path(&output_path).expect("staging path");
        let part = selective_part_path(&output_path).expect("part path");
        tokio::fs::write(&metainfo_path, two_file_metainfo())
            .await
            .expect("write metainfo");

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind scripted peer");
        let address = listener.local_addr().expect("listener address");
        let peer_task = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.expect("accept diagnostic");
            tokio::time::sleep(Duration::from_secs(1)).await;
        });

        let result = download_verified_piece(DownloadConfig {
            metainfo_path: metainfo_path.clone(),
            peer: address,
            output_path: output_path.clone(),
            network: loopback_network(Duration::from_millis(50)),
            resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
            skip_files: vec![1],
            materialize_files: Vec::new(),
        })
        .await;

        assert!(matches!(result, Err(DownloadError::PeerTimedOut { .. })));
        assert!(!tokio::fs::try_exists(&output_path).await.expect("output"));
        assert!(!tokio::fs::try_exists(&staging).await.expect("staging"));
        assert!(!tokio::fs::try_exists(&part).await.expect("part"));

        peer_task.abort();
        let _ = peer_task.await;
        let _ = tokio::fs::remove_file(metainfo_path).await;
    }

    #[tokio::test]
    async fn cancellation_is_terminal_and_removes_owned_artifacts() {
        let metainfo_path = test_path("selective-cancel.torrent");
        let output_path = test_path("selective-cancel");
        let staging = selective_staging_path(&output_path).expect("staging path");
        let part = selective_part_path(&output_path).expect("part path");
        tokio::fs::write(&metainfo_path, two_file_metainfo())
            .await
            .expect("write metainfo");

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind scripted peer");
        let address = listener.local_addr().expect("listener address");
        let peer_task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept diagnostic");
            let mut handshake = [0; HANDSHAKE_LENGTH];
            stream
                .read_exact(&mut handshake)
                .await
                .expect("read diagnostic handshake");
            let mut end = [0; 1];
            assert_eq!(
                stream.read(&mut end).await.expect("wait for peer cleanup"),
                0
            );
        });

        let control = DownloadControl::new();
        let activity = Arc::new(RecordingActivitySink::default());
        control.set_activity_sink(activity.clone());
        let download_control = control.clone();
        let download_task = tokio::spawn(download_verified_piece_with_control(
            DownloadConfig {
                metainfo_path: metainfo_path.clone(),
                peer: address,
                output_path: output_path.clone(),
                network: loopback_network(Duration::from_secs(5)),
                resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
                skip_files: vec![1],
                materialize_files: Vec::new(),
            },
            download_control,
        ));

        timeout(Duration::from_secs(1), async {
            loop {
                if tokio::fs::try_exists(&staging).await.expect("staging")
                    && tokio::fs::try_exists(&part).await.expect("part")
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("engine created owned artifacts");
        timeout(Duration::from_secs(1), async {
            loop {
                if control
                    .diagnostic_snapshot()
                    .peer_connections
                    .iter()
                    .any(|peer| peer.lifecycle == PeerConnectionLifecycle::ProtocolHandshaking)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("engine published active diagnostic peer");

        control.cancel();
        control.cancel();
        let result = download_task.await.expect("download task");
        assert!(matches!(result, Err(DownloadError::Cancelled)));
        assert!(control.is_cancelled());
        let progress = control.snapshot();
        assert_eq!(progress.buffered_payload_bytes, 0);
        assert_eq!(progress.requested_bytes, 0);
        assert_eq!(progress.received_bytes, 0);
        assert_eq!(progress.stored_bytes, 0);
        assert_eq!(progress.storage_jobs_pending, 0);
        assert_eq!(progress.outstanding_request_bytes, 0);
        assert!(control.diagnostic_snapshot().peer_connections.is_empty());
        assert!(!tokio::fs::try_exists(&output_path).await.expect("output"));
        assert!(!tokio::fs::try_exists(&staging).await.expect("staging"));
        assert!(!tokio::fs::try_exists(&part).await.expect("part"));

        timeout(Duration::from_secs(1), peer_task)
            .await
            .expect("diagnostic peer joined before terminal result")
            .expect("diagnostic peer task");
        let events = activity
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let peer_snapshots = events
            .iter()
            .filter_map(|event| match event {
                DownloadActivityEvent::PeerConnections { peers, .. } => Some(peers.as_slice()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(peer_snapshots.iter().any(|peers| {
            peers
                .iter()
                .any(|peer| peer.lifecycle == PeerConnectionLifecycle::Disconnecting)
        }));
        assert!(peer_snapshots.last().is_some_and(|peers| peers.is_empty()));
        let _ = tokio::fs::remove_file(metainfo_path).await;
    }

    #[tokio::test]
    async fn preexisting_selective_part_file_is_preserved() {
        let metainfo_path = test_path("selective-existing.torrent");
        let output_path = test_path("selective-existing");
        let part = selective_part_path(&output_path).expect("part path");
        tokio::fs::write(&metainfo_path, two_file_metainfo())
            .await
            .expect("write metainfo");
        tokio::fs::write(&part, b"owned elsewhere")
            .await
            .expect("write existing part");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind unused peer");
        let address = listener.local_addr().expect("listener address");

        let result = download_verified_piece(DownloadConfig {
            metainfo_path: metainfo_path.clone(),
            peer: address,
            output_path: output_path.clone(),
            network: loopback_network(Duration::from_secs(1)),
            resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
            skip_files: vec![1],
            materialize_files: Vec::new(),
        })
        .await;
        assert!(matches!(
            result,
            Err(DownloadError::SelectiveStorage(
                SelectiveStorageError::ExistingPartFile(_)
            ))
        ));
        assert_eq!(
            tokio::fs::read(&part).await.expect("preserved part"),
            b"owned elsewhere"
        );

        let _ =
            tokio::fs::remove_dir_all(selective_staging_path(&output_path).expect("staging")).await;
        let _ = tokio::fs::remove_file(part).await;
        let _ = tokio::fs::remove_file(metainfo_path).await;
    }
}
