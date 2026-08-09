//! Download cancellation, bounded accounting, diagnostics, and activity observation.
//!
//! `DownloadControl` is task-free shared control state. The driver and its
//! existing child owners mutate it, while cancellation and safe-cancel
//! critical sections preserve the driver's established shutdown order.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rstorrent_protocol::metadata::TorrentMetadataDownload;
use rstorrent_protocol::metainfo::Metainfo;
use rstorrent_protocol::udp_tracker::AnnounceEvent;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use super::DownloadError;
use crate::checkpoint::CheckpointBatch;
use crate::metrics::{ByteMetric, ByteMetricSink, SharedByteMetricSink};
use crate::mse::MseHandshakeSink;
use crate::peer::{DialAttempt, DialAttemptId, PeerRegistryCounts, PeerRegistrySnapshot};
#[cfg(test)]
use crate::peer::{PeerRegistry, PeerSelectionContext};
use crate::peer_runtime::PeerConnectionObservation;
use crate::piece_picker::PieceActivationPolicy;
use crate::selective_storage::PlatformStorageSpec;
use crate::session_resources::{
    SessionExecutionPermit, SessionSemaphorePermit, SessionTorrentResources,
};
use crate::storage_file_pool::StorageFilePool;
use crate::swarm::{BlockKey, ConnectionWindowPhaseSnapshot, NoRequestReason, SwarmState};
use crate::torrent_peer::TorrentPeerActivitySink;
use crate::tracker::TrackerRuntimeSnapshot;

pub(super) const CONTENT_STORAGE_WRITE_CONCURRENCY: usize = 4;
pub(super) const CONTENT_STORAGE_HASH_CONCURRENCY: usize = 4;
const CONTENT_STORAGE_MAX_DIAGNOSTIC_CONCURRENCY: usize = 8;
const SAFE_CANCEL_REQUESTED: usize = 1 << (usize::BITS - 1);
const SAFE_CANCEL_CRITICAL_MASK: usize = SAFE_CANCEL_REQUESTED - 1;
const CONTENT_PEER_DIAGNOSTIC_INTERVAL: Duration = Duration::from_secs(1);
const STORAGE_OBSERVATION_INTERVAL: Duration = Duration::from_millis(100);
const CHECKER_OBSERVATION_INTERVAL: Duration = Duration::from_secs(1);
pub(super) const MAX_RECENT_METADATA_ATTEMPTS: usize = 64;
pub(super) const MAX_DIAGNOSTIC_ERROR_LENGTH: usize = 256;

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
    CheckerProgress(Box<CheckerProgress>),
    CheckerFinished {
        generation: u64,
    },
    PathPublicationStage(PathPublicationStage),
    StorageState(Box<DiskRuntimeSnapshot>),
    TrackerAnnounceStarted {
        tracker: String,
        tier: u32,
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
        tier: u32,
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
    TrackerWarning {
        tracker: String,
        detail: String,
    },
    TrackerPeersUnavailable {
        tracker: String,
        peer_count: u32,
    },
    DhtLookupStarted,
    DhtLookupSucceeded {
        peer_count: u32,
    },
    DhtAnnounceCompleted {
        port: u16,
        token_nodes: u8,
        announces_sent: u8,
        announces_succeeded: u8,
        announces_failed: u8,
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
    pub max_active_pieces: usize,
    pub piece_activation_policy: PieceActivationPolicy,
    pub availability_seed_count: usize,
    pub picker_retained_bytes: usize,
    pub picker_rank_comparisons: u64,
    pub picker_bulk_rebuilds: u64,
    pub picker_candidate_inspections: u64,
    pub active_piece_visits: u64,
    pub inactive_planned_piece_visits: u64,
    pub last_activated_piece: Option<u32>,
    pub last_activated_availability: Option<u32>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckerPhase {
    Queued,
    Preparing,
    Hashing,
    ReconcilingStorage,
    Paused,
    Finalizing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckerProgress {
    pub generation: u64,
    pub phase: CheckerPhase,
    pub pieces_total: usize,
    pub pieces_processed: usize,
    pub pieces_matched: usize,
    pub pieces_absent: usize,
    pub pieces_mismatched: usize,
    pub bytes_hashed: u64,
    pub active_hash_jobs: usize,
    pub queued_hash_jobs: usize,
    pub elapsed_millis: u64,
    pub last_advance_age_millis: u64,
    pub oldest_active_job_age_millis: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileSelectionUpdate {
    pub revision: u64,
    pub skip_files: Vec<usize>,
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
    checker: Mutex<Option<CheckerProgressState>>,
    activity_sink: Mutex<Option<Arc<dyn DownloadActivitySink>>>,
    mse_handshake_sink: Mutex<Option<Arc<dyn MseHandshakeSink>>>,
    byte_metric_sink: Mutex<Option<SharedByteMetricSink>>,
    last_swarm_activity: Mutex<Option<SwarmActivitySnapshot>>,
    last_content_peers: Mutex<(Option<Duration>, Vec<ContentPeerActivitySnapshot>)>,
    peer_registry_activity: Mutex<PeerRegistryActivityState>,
    peer_connections: Mutex<PeerConnectionDiagnosticState>,
    metadata_diagnostics: Mutex<MetadataDiagnosticState>,
    storage_file_pool: Mutex<Option<StorageFilePool>>,
    platform_storage: Mutex<Option<PlatformStorageSpec>>,
    session_resources: Mutex<Option<SessionTorrentResources>>,
    selection_updates: watch::Sender<Option<FileSelectionUpdate>>,
    checking_paused: watch::Sender<bool>,
    selection_applied_revision: AtomicU64,
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
    #[cfg(test)]
    active: bool,
    last_emitted: Option<PeerRegistrySnapshot>,
    #[cfg(test)]
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

#[derive(Debug)]
struct CheckerProgressState {
    generation: u64,
    phase: CheckerPhase,
    pieces_total: usize,
    pieces_processed: usize,
    pieces_matched: usize,
    pieces_absent: usize,
    pieces_mismatched: usize,
    bytes_hashed: u64,
    active_hash_jobs: BTreeMap<u32, Instant>,
    started_at: Instant,
    last_advance_at: Instant,
    last_emitted_at: Option<Instant>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CheckerPieceOutcome {
    Matched,
    Absent,
    Mismatched,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StorageCommandKind {
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
pub(super) struct SafeCancelGuard {
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
        let (selection_updates, _) = watch::channel(None);
        let (checking_paused, _) = watch::channel(false);
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
                checker: Mutex::new(None),
                activity_sink: Mutex::new(None),
                mse_handshake_sink: Mutex::new(None),
                byte_metric_sink: Mutex::new(None),
                last_swarm_activity: Mutex::new(None),
                last_content_peers: Mutex::new((None, Vec::new())),
                peer_registry_activity: Mutex::new(PeerRegistryActivityState::default()),
                peer_connections: Mutex::new(PeerConnectionDiagnosticState::default()),
                metadata_diagnostics: Mutex::new(MetadataDiagnosticState::default()),
                storage_file_pool: Mutex::new(None),
                platform_storage: Mutex::new(None),
                session_resources: Mutex::new(None),
                selection_updates,
                checking_paused,
                selection_applied_revision: AtomicU64::new(0),
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

    pub(super) async fn cancelled(&self) {
        self.inner.cancellation.cancelled().await;
    }

    pub(super) fn cancellation_token(&self) -> CancellationToken {
        self.inner.cancellation.clone()
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

    pub fn set_session_resources(&self, resources: SessionTorrentResources) {
        *self
            .inner
            .session_resources
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(resources);
    }

    pub(super) fn session_resources(&self) -> Option<SessionTorrentResources> {
        self.inner
            .session_resources
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub(super) fn storage_file_pool(&self) -> Option<StorageFilePool> {
        self.inner
            .storage_file_pool
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub(super) fn platform_storage(&self) -> Option<PlatformStorageSpec> {
        self.inner
            .platform_storage
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn update_file_selection(&self, update: FileSelectionUpdate) {
        self.inner.selection_updates.send_replace(Some(update));
    }

    pub(super) fn selection_updates(&self) -> watch::Receiver<Option<FileSelectionUpdate>> {
        self.inner.selection_updates.subscribe()
    }

    pub(super) fn latest_file_selection(&self) -> Option<FileSelectionUpdate> {
        self.inner.selection_updates.borrow().clone()
    }

    pub fn applied_file_selection_revision(&self) -> u64 {
        self.inner
            .selection_applied_revision
            .load(Ordering::Acquire)
    }

    pub(super) fn file_selection_applied(&self, revision: u64) {
        self.inner
            .selection_applied_revision
            .fetch_max(revision, Ordering::AcqRel);
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

    pub fn checker_snapshot(&self) -> Option<CheckerProgress> {
        let now = Instant::now();
        self.inner
            .checker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .map(|state| checker_progress_snapshot(state, now))
    }

    /// Requests an in-progress checker to drain admitted hashes and retain its
    /// storage owner, generation, and cursor until resumed.
    ///
    /// Returning `false` means no checker was active while the request was
    /// serialized, so callers must use ordinary task cancellation instead.
    pub fn pause_checking(&self) -> bool {
        let checker = self
            .inner
            .checker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if checker.is_none() {
            return false;
        }
        self.inner.checking_paused.send_replace(true);
        true
    }

    pub fn resume_checking(&self) {
        self.inner.checking_paused.send_replace(false);
    }

    pub(super) fn checking_pause_updates(&self) -> watch::Receiver<bool> {
        self.inner.checking_paused.subscribe()
    }

    pub(super) fn checker_started(&self, generation: u64, pieces_total: usize) {
        let now = Instant::now();
        *self
            .inner
            .checker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(CheckerProgressState {
            generation,
            phase: CheckerPhase::Preparing,
            pieces_total,
            pieces_processed: 0,
            pieces_matched: 0,
            pieces_absent: 0,
            pieces_mismatched: 0,
            bytes_hashed: 0,
            active_hash_jobs: BTreeMap::new(),
            started_at: now,
            last_advance_at: now,
            last_emitted_at: None,
        });
        self.emit_checker_progress(true);
    }

    pub(super) fn checker_set_phase(&self, phase: CheckerPhase) {
        let changed = {
            let mut checker = self
                .inner
                .checker
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            checker.as_mut().is_some_and(|state| {
                if state.phase == phase {
                    false
                } else {
                    state.phase = phase;
                    true
                }
            })
        };
        if changed {
            self.emit_checker_progress(true);
        }
    }

    pub(super) fn checker_hash_started(&self, piece_index: u32) {
        let now = Instant::now();
        let changed_phase = {
            let mut checker = self
                .inner
                .checker
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(state) = checker.as_mut() else {
                return;
            };
            let changed_phase = state.phase != CheckerPhase::Hashing;
            state.phase = CheckerPhase::Hashing;
            state.active_hash_jobs.insert(piece_index, now);
            changed_phase
        };
        self.emit_checker_progress(changed_phase);
    }

    pub(super) fn checker_piece_processed(
        &self,
        piece_index: u32,
        bytes_hashed: u64,
        outcome: CheckerPieceOutcome,
    ) {
        {
            let mut checker = self
                .inner
                .checker
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(state) = checker.as_mut() else {
                return;
            };
            state.active_hash_jobs.remove(&piece_index);
            state.pieces_processed = state.pieces_processed.saturating_add(1);
            match outcome {
                CheckerPieceOutcome::Matched => {
                    state.pieces_matched = state.pieces_matched.saturating_add(1);
                    state.bytes_hashed = state.bytes_hashed.saturating_add(bytes_hashed);
                }
                CheckerPieceOutcome::Absent => {
                    state.pieces_absent = state.pieces_absent.saturating_add(1);
                }
                CheckerPieceOutcome::Mismatched => {
                    state.pieces_mismatched = state.pieces_mismatched.saturating_add(1);
                    state.bytes_hashed = state.bytes_hashed.saturating_add(bytes_hashed);
                }
            }
            state.last_advance_at = Instant::now();
            debug_assert_eq!(
                state.pieces_processed,
                state
                    .pieces_matched
                    .saturating_add(state.pieces_absent)
                    .saturating_add(state.pieces_mismatched)
            );
            debug_assert!(state.pieces_processed <= state.pieces_total);
        }
        self.emit_checker_progress(false);
    }

    pub(super) fn checker_hash_stopped(&self, piece_index: u32) {
        let removed = self
            .inner
            .checker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_mut()
            .is_some_and(|state| state.active_hash_jobs.remove(&piece_index).is_some());
        if removed {
            self.emit_checker_progress(true);
        }
    }

    pub(super) fn checker_heartbeat(&self) {
        self.emit_checker_progress(false);
    }

    pub(super) fn checker_finished(&self, generation: u64) {
        let removed = {
            let mut checker = self
                .inner
                .checker
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if checker
                .as_ref()
                .is_some_and(|state| state.generation == generation)
            {
                checker.take();
                true
            } else {
                false
            }
        };
        if removed {
            self.emit(DownloadActivityEvent::CheckerFinished { generation });
        }
    }

    fn emit_checker_progress(&self, force: bool) {
        let now = Instant::now();
        let snapshot = {
            let mut checker = self
                .inner
                .checker
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(state) = checker.as_mut() else {
                return;
            };
            if !force
                && state.last_emitted_at.is_some_and(|last| {
                    now.saturating_duration_since(last) < CHECKER_OBSERVATION_INTERVAL
                })
            {
                return;
            }
            state.last_emitted_at = Some(now);
            checker_progress_snapshot(state, now)
        };
        self.emit(DownloadActivityEvent::CheckerProgress(Box::new(snapshot)));
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

    #[doc(hidden)]
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

    pub(super) fn storage_execution_limits(&self) -> (usize, usize) {
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

    pub(super) async fn enter_path_publication_stage(&self, stage: PathPublicationStage) {
        self.emit(DownloadActivityEvent::PathPublicationStage(stage));
    }

    #[cfg(test)]
    pub(super) fn fail_next_checkpoint_sync(&self) {
        atomic_saturating_increment(&self.inner.checkpoint_sync_failures);
    }

    pub(super) fn enter_safe_cancel_critical(&self) -> Result<SafeCancelGuard, DownloadError> {
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

    pub fn set_mse_handshake_sink(&self, sink: Arc<dyn MseHandshakeSink>) {
        *self
            .inner
            .mse_handshake_sink
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(sink);
    }

    pub(super) fn mse_handshake_sink(&self) -> Option<Arc<dyn MseHandshakeSink>> {
        self.inner
            .mse_handshake_sink
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub(super) fn byte_metric_sink(&self) -> Option<SharedByteMetricSink> {
        self.inner
            .byte_metric_sink
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub(super) fn record_bytes(&self, metric: ByteMetric, bytes: usize) {
        if bytes == 0 {
            return;
        }
        if let Some(sink) = self.byte_metric_sink() {
            sink.record(metric, bytes.try_into().unwrap_or(u64::MAX));
        }
    }

    pub(crate) fn emit(&self, event: DownloadActivityEvent) {
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

    pub(crate) fn diagnostic_elapsed(&self) -> Duration {
        self.inner.started_at.elapsed()
    }

    pub(super) fn metadata_started(&self) {
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

    pub(super) fn observe_metadata_supervisor(
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

    pub(super) fn metadata_dial_started(&self, attempt: DialAttempt) {
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

    pub(super) fn metadata_peer_connected(&self, attempt: DialAttempt, supports_extensions: bool) {
        let now = self.diagnostic_elapsed();
        self.update_metadata_peer(attempt.id(), |peer| {
            peer.stage = MetadataPeerStage::AwaitingExtensionHandshake;
            peer.supports_extensions = Some(supports_extensions);
            peer.last_activity_at = now;
            peer.last_progress_at = now;
        });
    }

    pub(super) fn metadata_peer_message(&self, attempt_id: DialAttemptId) {
        let now = self.diagnostic_elapsed();
        self.update_metadata_peer(attempt_id, |peer| {
            peer.messages_received = peer.messages_received.saturating_add(1);
            peer.last_activity_at = now;
        });
    }

    pub(super) fn metadata_extension_handshake(
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

    pub(super) fn metadata_block_received(
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

    pub(super) fn metadata_requests_sent(
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

    pub(super) fn metadata_rejected(&self, attempt_id: DialAttemptId) {
        let now = self.diagnostic_elapsed();
        self.update_metadata_peer(attempt_id, |peer| {
            peer.rejects_received = peer.rejects_received.saturating_add(1);
            peer.last_activity_at = now;
        });
    }

    pub(super) fn metadata_hash_failed(&self, contributors: usize) {
        let mut state = self
            .inner
            .metadata_diagnostics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.total_hash_failures = state.total_hash_failures.saturating_add(1);
        state.last_hash_failure_contributors = contributors;
    }

    pub(super) fn metadata_peer_finished(
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

    pub(super) fn metadata_finished(&self, result: &Result<(Vec<u8>, Metainfo), DownloadError>) {
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

    pub(super) fn update_metadata_peer(
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

    pub(super) fn observe_swarm(&self, swarm: &SwarmState, now: Duration) {
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
            max_active_pieces: snapshot.max_active_pieces,
            piece_activation_policy: snapshot.piece_activation_policy,
            availability_seed_count: snapshot.availability_seed_count,
            picker_retained_bytes: snapshot.picker_retained_bytes,
            picker_rank_comparisons: snapshot.picker_rank_comparisons,
            picker_bulk_rebuilds: snapshot.picker_bulk_rebuilds,
            picker_candidate_inspections: snapshot.picker_candidate_inspections,
            active_piece_visits: snapshot.active_piece_visits,
            inactive_planned_piece_visits: snapshot.inactive_planned_piece_visits,
            last_activated_piece: snapshot.last_activated_piece,
            last_activated_availability: snapshot.last_activated_availability,
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

    #[cfg(test)]
    pub(super) fn observe_peer_registry(
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

    pub(super) fn configure_disk_runtime(&self, resident_limit_bytes: usize) {
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

    pub(super) fn storage_backpressured(&self) -> bool {
        self.inner
            .disk_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .backpressured_since
            .is_some()
    }

    pub(super) fn update_disk_pressure(&self, resident_bytes: usize) {
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

    pub(super) fn disk_block_requested(&self, block: BlockKey, piece_length: u32) -> (u32, bool) {
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

    pub(super) fn disk_block_received(&self, block: BlockKey, piece_length: u32) {
        self.inner
            .received_bytes
            .fetch_add(block.length as usize, Ordering::AcqRel);
        self.mutate_disk_piece(block.piece, piece_length, |piece, now| {
            insert_disk_range(&mut piece.received, block.begin, block.length);
            set_disk_piece_stage(piece, DiskPieceStage::Queued, now);
        });
        self.emit_storage_state();
    }

    pub(super) fn disk_block_stored(&self, block: BlockKey, piece_length: u32) {
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

    pub(super) fn disk_piece_hashing(&self, piece_index: u32, piece_length: u32) {
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

    pub(super) fn disk_piece_hash_verified(
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

    pub(super) fn disk_piece_check_unverified(&self, piece_index: u32, piece_length: u32) {
        let mut state = self
            .inner
            .disk_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.hashing_bytes = state.hashing_bytes.saturating_sub(piece_length as usize);
        state.pieces.remove(&piece_index);
        let resident = self.inner.buffered_payload_bytes.load(Ordering::Acquire);
        drop(state);
        self.update_disk_pressure(resident);
        self.emit_storage_state();
    }

    pub(super) fn disk_checkpoint_sync_started(&self, batch: &CheckpointBatch) {
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

    pub(super) fn disk_checkpoint_sync_completed(
        &self,
        batch: &CheckpointBatch,
        elapsed: Duration,
    ) {
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

    pub(super) fn disk_checkpoint_completed(&self, batch: &CheckpointBatch, elapsed: Duration) {
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

    pub(super) fn disk_checkpoint_failed(
        &self,
        batch: &CheckpointBatch,
        elapsed: Duration,
        detail: &str,
    ) {
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

    pub(super) fn disk_piece_failed(&self, piece_index: u32, piece_length: u32, detail: &str) {
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

    pub(super) fn disk_storage_error(&self, detail: &str) {
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

    pub(super) fn disk_write_batch_started(&self, blocks: &[BlockKey], bytes: usize) {
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

    pub(super) fn disk_write_batch_completed(&self, blocks: &[BlockKey], bytes: usize) {
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

    pub(super) fn emit_storage_state(&self) {
        self.emit_storage_state_inner(false);
    }

    pub(super) fn emit_storage_state_force(&self) {
        self.emit_storage_state_inner(true);
    }

    pub(super) fn emit_storage_state_inner(&self, force: bool) {
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

    pub(super) fn storage_job_started(&self) {
        let pending = self
            .inner
            .storage_jobs_pending
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        self.inner
            .storage_jobs_high_water
            .fetch_max(pending, Ordering::AcqRel);
    }

    pub(super) fn storage_job_finished(&self) {
        let previous = self
            .inner
            .storage_jobs_pending
            .fetch_sub(1, Ordering::AcqRel);
        debug_assert_ne!(previous, 0);
    }

    pub(super) fn storage_jobs_at_limit(&self, limit: usize) -> bool {
        self.inner.storage_jobs_pending.load(Ordering::Acquire) >= limit
    }

    pub(super) fn try_buffer_payload(&self, bytes: usize, limit: usize) -> bool {
        let mut session_reservation = match self.session_resources() {
            Some(resources) => match resources.try_reserve_payload_bytes(bytes) {
                Some(reservation) => Some(reservation),
                None => return false,
            },
            None => None,
        };
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
                    if let Some(reservation) = session_reservation.take() {
                        reservation.commit();
                    }
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

    pub(super) fn release_buffered_payload(&self, bytes: usize) {
        let previous = self
            .inner
            .buffered_payload_bytes
            .fetch_sub(bytes, Ordering::AcqRel);
        debug_assert!(previous >= bytes);
        if let Some(resources) = self.session_resources() {
            resources.release_payload_bytes(bytes);
        }
        self.update_disk_pressure(previous.saturating_sub(bytes));
        self.emit_storage_state();
    }

    pub(super) fn abandon_queued_payload(&self, bytes: usize) {
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

    pub(super) fn observe_storage_command_queue(&self, depth: usize) {
        self.inner
            .storage_command_queue_high_water
            .fetch_max(depth, Ordering::AcqRel);
    }

    pub(super) fn observe_storage_completion_queue(&self, depth: usize) {
        self.inner
            .storage_completion_queue_high_water
            .fetch_max(depth, Ordering::AcqRel);
    }

    pub(super) fn storage_command_started(
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

    pub(super) fn storage_command_completed(
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

    pub(super) fn storage_write_batch_started(
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

    pub(super) fn storage_write_batch_completed(
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

    pub(super) fn storage_active_ages(&self) -> (Option<u64>, Option<u64>) {
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

    pub(super) fn clear_storage_active_operations(&self) {
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

    pub(super) fn clear_storage_jobs(&self) {
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

    pub(super) fn clear_buffered_payload(&self) {
        let buffered = self.inner.buffered_payload_bytes.swap(0, Ordering::AcqRel);
        if let Some(resources) = self.session_resources()
            && buffered != 0
        {
            resources.release_payload_bytes(buffered);
        }
        self.update_disk_pressure(0);
        self.emit_storage_state_force();
    }

    pub(super) fn clear_outstanding_requests(&self) {
        self.inner
            .outstanding_request_bytes
            .store(0, Ordering::Release);
    }

    pub(super) async fn wait_before_storage(&self) -> Option<SessionExecutionPermit> {
        let permit = match self.session_resources() {
            Some(resources) => Some(resources.acquire_storage_write().await),
            None => None,
        };
        let millis = self
            .inner
            .storage_write_delay_millis
            .load(Ordering::Acquire);
        if millis != 0 {
            tokio::time::sleep(Duration::from_millis(millis)).await;
        }
        permit
    }

    pub(super) async fn wait_before_storage_hash(&self) -> Option<SessionExecutionPermit> {
        let permit = match self.session_resources() {
            Some(resources) => Some(resources.acquire_storage_hash().await),
            None => None,
        };
        self.inner
            .storage_hashes_started
            .fetch_add(1, Ordering::AcqRel);
        let millis = self.inner.storage_hash_delay_millis.load(Ordering::Acquire);
        if millis != 0 {
            tokio::time::sleep(Duration::from_millis(millis)).await;
        }
        permit
    }

    pub(super) fn try_acquire_outbound_turn(&self) -> bool {
        self.session_resources()
            .is_none_or(|resources| resources.try_acquire_outbound_turn())
    }

    pub(super) async fn acquire_tracker_operation(&self) -> Option<SessionSemaphorePermit> {
        match self.session_resources() {
            Some(resources) => Some(resources.acquire_tracker_operation().await),
            None => None,
        }
    }

    pub(super) async fn wait_before_checkpoint_sync(&self) -> Option<SessionExecutionPermit> {
        let permit = match self.session_resources() {
            Some(resources) => Some(resources.acquire_storage_write().await),
            None => None,
        };
        let millis = self
            .inner
            .checkpoint_sync_delay_millis
            .load(Ordering::Acquire);
        if millis != 0 {
            tokio::time::sleep(Duration::from_millis(millis)).await;
        }
        permit
    }

    pub(super) async fn wait_before_checkpoint_commit(&self) {
        let millis = self
            .inner
            .checkpoint_commit_delay_millis
            .load(Ordering::Acquire);
        if millis != 0 {
            tokio::time::sleep(Duration::from_millis(millis)).await;
        }
    }

    pub(super) fn take_checkpoint_sync_failure(&self) -> bool {
        self.inner
            .checkpoint_sync_failures
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |failures| {
                failures.checked_sub(1)
            })
            .is_ok()
    }
}

impl TorrentPeerActivitySink for DownloadControl {
    fn record_peer_connections(
        &self,
        captured_at: Duration,
        peers: Vec<PeerConnectionObservation>,
    ) {
        {
            let mut state = self
                .inner
                .peer_connections
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.current = peers.clone();
            state.last_emitted = peers.clone();
            state.last_emitted_at = Some(captured_at);
        }
        self.emit(DownloadActivityEvent::PeerConnections {
            captured_at,
            peers: Box::new(peers),
        });
    }

    fn record_peer_registry(&self, active: bool, snapshot: PeerRegistrySnapshot) {
        self.inner
            .peer_registry_activity
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .last_emitted = Some(snapshot.clone());
        self.emit(DownloadActivityEvent::PeerRegistryState {
            active,
            snapshot: Box::new(snapshot),
        });
    }
}

fn duration_micros(duration: Duration) -> u64 {
    duration.as_micros().try_into().unwrap_or(u64::MAX)
}

fn metadata_block_count_for_diagnostics(download: &TorrentMetadataDownload) -> Option<usize> {
    let blocks = download.allocated_blocks();
    (blocks != 0).then_some(blocks)
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

fn checker_progress_snapshot(state: &CheckerProgressState, now: Instant) -> CheckerProgress {
    let active_hash_jobs = state.active_hash_jobs.len();
    CheckerProgress {
        generation: state.generation,
        phase: state.phase,
        pieces_total: state.pieces_total,
        pieces_processed: state.pieces_processed,
        pieces_matched: state.pieces_matched,
        pieces_absent: state.pieces_absent,
        pieces_mismatched: state.pieces_mismatched,
        bytes_hashed: state.bytes_hashed,
        active_hash_jobs,
        queued_hash_jobs: state
            .pieces_total
            .saturating_sub(state.pieces_processed)
            .saturating_sub(active_hash_jobs),
        elapsed_millis: duration_millis(now.saturating_duration_since(state.started_at)),
        last_advance_age_millis: duration_millis(
            now.saturating_duration_since(state.last_advance_at),
        ),
        oldest_active_job_age_millis: state
            .active_hash_jobs
            .values()
            .map(|started| duration_millis(now.saturating_duration_since(*started)))
            .max(),
    }
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

pub(super) fn atomic_saturating_add(value: &AtomicU64, amount: u64) {
    let _ = value.fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
        Some(current.saturating_add(amount))
    });
}

pub(super) fn atomic_saturating_increment(value: &AtomicUsize) {
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
