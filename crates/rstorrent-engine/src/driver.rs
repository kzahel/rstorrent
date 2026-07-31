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
    MetadataDownload, MetadataDownloadAction, MetadataError, MetadataExtensionUpdate,
    MetadataMessage, UT_METADATA_LOCAL_ID, encode_extension_handshake, encode_metadata_reject,
    encode_metadata_request, parse_extension_handshake, parse_metadata_message,
};
use rstorrent_protocol::metainfo::{MAX_PIECES, Metainfo, MetainfoError};
use rstorrent_protocol::peer_wire::{FrameError, Handshake, HandshakeError, PeerMessage};
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
    DialAttempt, DialAttemptId, PeerEndpoint, PeerFailure, PeerObservation, PeerRegistry,
    PeerRegistryConfig, PeerRegistryError, PeerSelectionContext, PeerSelector, PeerSource,
};
use crate::peer_socket::{
    self, PeerConnection, PeerSetError, PeerSetEvent, PeerSocketError, PeerSocketSet,
    PeerTaskEvent, connection_id,
};
use crate::selective_storage::{
    DescriptorStorage, PreparedFileHash, ResumedStorage, SelectiveStorage, SelectiveStorageError,
    remove_selective_part_if_present, remove_selective_staging_if_present,
};
use crate::storage::{
    StagingFile, StorageError, VERIFICATION_CHUNK_LENGTH, remove_staging_if_present, staging_path,
};
use crate::swarm::{
    BlockKey, ConnectionId, ConnectionRemoval, NoRequestReason, PendingDialId, PiecePlan,
    ReceiveDisposition, SwarmConfig, SwarmError, SwarmState,
};
use crate::tracker::{TrackerAction, TrackerSchedule, TrackerWaitKind};

const CLIENT_PEER_ID: [u8; 20] = *b"-RS0001-000000000000";
const NETWORK_RESOLUTION_TIMEOUT: Duration = Duration::from_secs(10);
const UDP_TRACKER_RETRANSMIT_AFTER: Duration = Duration::from_secs(15);
const UDP_TRACKER_COMPLETION_TIMEOUT: Duration = Duration::from_secs(30);
const UDP_TRACKER_TOKEN_LIFETIME: Duration = Duration::from_secs(60);
const MAX_UDP_TRACKER_TOKENS: usize = 64;
const TRACKER_RESULT_QUEUE: usize = 4;
const CONTENT_DISCOVERY_QUEUE: usize = 8;
const MAX_RESOLVED_ADDRESSES: usize = 32;
const UNKNOWN_MAGNET_LEFT: u64 = 16 * 1024;
const UDP_TRACKER_RECEIVE_LENGTH: usize = MAX_ANNOUNCE_RESPONSE_LENGTH + 1;
const SAFE_CANCEL_REQUESTED: usize = 1 << (usize::BITS - 1);
const SAFE_CANCEL_CRITICAL_MASK: usize = SAFE_CANCEL_REQUESTED - 1;
const DHT_RETRY_INITIAL_DELAY: Duration = Duration::from_secs(15);
const DHT_RETRY_MAX_DELAY: Duration = Duration::from_secs(5 * 60);
const DHT_SUCCESS_REQUERY_DELAY: Duration = Duration::from_secs(60);
const MAX_METADATA_PEERS: usize = 3;

#[derive(Clone, Debug)]
pub struct DownloadConfig {
    pub metainfo_path: PathBuf,
    pub peer: SocketAddr,
    pub output_path: PathBuf,
    pub network: NetworkConfig,
    pub max_buffered_payload_bytes: usize,
    pub skip_files: Vec<usize>,
    pub materialize_files: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct MagnetDownloadConfig {
    pub magnet: String,
    pub output_path: PathBuf,
    pub network: NetworkConfig,
    pub max_buffered_payload_bytes: usize,
    pub skip_files: Vec<usize>,
    pub materialize_files: Vec<usize>,
    pub dht: Option<DhtHandle>,
}

#[derive(Clone, Debug)]
pub struct ResumableMagnetDownloadConfig {
    pub magnet: String,
    pub output_path: PathBuf,
    pub network: NetworkConfig,
    pub max_buffered_payload_bytes: usize,
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
    SwarmState(SwarmActivitySnapshot),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SwarmActivitySnapshot {
    pub pending_dials: usize,
    pub connected_peers: usize,
    pub unchoked_peers: usize,
    pub missing_blocks: usize,
    pub requested_blocks: usize,
    pub writing_blocks: usize,
    pub received_blocks: usize,
    pub verified_blocks: usize,
    pub payload_reserved: usize,
    pub payload_high_water: usize,
    pub oldest_request_age_seconds: Option<u64>,
    pub next_request_expiry_seconds: Option<u64>,
    pub next_replacement_seconds: Option<u64>,
    pub no_request_reason: Option<NoRequestReason>,
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
    cancellation: CancellationToken,
    buffered_payload_bytes: AtomicUsize,
    payload_high_water: AtomicUsize,
    requested_bytes: AtomicUsize,
    received_bytes: AtomicUsize,
    stored_bytes: AtomicUsize,
    storage_write_delay_millis: AtomicU64,
    activity_sink: Mutex<Option<Arc<dyn DownloadActivitySink>>>,
    last_swarm_activity: Mutex<Option<SwarmActivitySnapshot>>,
    safe_cancel_state: AtomicUsize,
}

#[derive(Debug)]
struct SafeCancelGuard {
    control: DownloadControl,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DownloadProgress {
    pub buffered_payload_bytes: usize,
    pub payload_high_water: usize,
    pub requested_bytes: usize,
    pub received_bytes: usize,
    pub stored_bytes: usize,
}

impl DownloadControl {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DownloadControlInner {
                cancellation: CancellationToken::new(),
                buffered_payload_bytes: AtomicUsize::new(0),
                payload_high_water: AtomicUsize::new(0),
                requested_bytes: AtomicUsize::new(0),
                received_bytes: AtomicUsize::new(0),
                stored_bytes: AtomicUsize::new(0),
                storage_write_delay_millis: AtomicU64::new(0),
                activity_sink: Mutex::new(None),
                last_swarm_activity: Mutex::new(None),
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
        DownloadProgress {
            buffered_payload_bytes: self.inner.buffered_payload_bytes.load(Ordering::Acquire),
            payload_high_water: self.inner.payload_high_water.load(Ordering::Acquire),
            requested_bytes: self.inner.requested_bytes.load(Ordering::Acquire),
            received_bytes: self.inner.received_bytes.load(Ordering::Acquire),
            stored_bytes: self.inner.stored_bytes.load(Ordering::Acquire),
        }
    }

    pub fn set_storage_write_delay(&self, delay: Duration) {
        let millis = delay.as_millis().try_into().unwrap_or(u64::MAX);
        self.inner
            .storage_write_delay_millis
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

    fn observe_swarm(&self, swarm: &SwarmState, now: Duration) {
        let snapshot = swarm.snapshot(now);
        self.inner
            .buffered_payload_bytes
            .store(snapshot.payload_reserved, Ordering::Release);
        self.inner
            .payload_high_water
            .fetch_max(snapshot.payload_high_water, Ordering::AcqRel);
        let activity = SwarmActivitySnapshot {
            pending_dials: snapshot.pending_dials,
            connected_peers: snapshot.connected_peers,
            unchoked_peers: snapshot.unchoked_peers,
            missing_blocks: snapshot.missing_blocks,
            requested_blocks: snapshot.requested_blocks,
            writing_blocks: snapshot.writing_blocks,
            received_blocks: snapshot.received_blocks,
            verified_blocks: snapshot.verified_blocks,
            payload_reserved: snapshot.payload_reserved,
            payload_high_water: snapshot.payload_high_water,
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
            self.emit(DownloadActivityEvent::SwarmState(activity));
        }
    }

    fn record_stored(&self, bytes: usize) {
        self.inner.stored_bytes.fetch_add(bytes, Ordering::AcqRel);
    }

    fn record_requested(&self, bytes: usize) {
        self.inner
            .requested_bytes
            .fetch_add(bytes, Ordering::AcqRel);
    }

    fn record_received(&self, bytes: usize) {
        self.inner.received_bytes.fetch_add(bytes, Ordering::AcqRel);
    }

    fn clear_buffered_payload(&self) {
        self.inner
            .buffered_payload_bytes
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
    PeerTask(String),
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
            Self::PeerTask(error) => write!(formatter, "peer task set: {error}"),
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
                write!(formatter, "peer disabled ut_metadata during acquisition")
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
    let result =
        run_resumable_magnet_download(config, checkpoints, control.clone(), descriptors).await;
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
    let result = run_magnet_metadata(magnet, network, control.clone(), dht).await;
    control.clear_buffered_payload();
    result
}

pub async fn download_magnet_with_control(
    config: MagnetDownloadConfig,
    control: DownloadControl,
) -> Result<DownloadReport, DownloadError> {
    validate_magnet_download_config(&config)?;
    let output_path = config.output_path.clone();
    let staging = staging_path(&output_path).map_err(DownloadError::Storage)?;
    let result = run_magnet_download(config, control.clone()).await;
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

    let staging = staging_path(&config.output_path).map_err(DownloadError::Storage)?;
    let output_path = config.output_path.clone();
    let result = tokio::select! {
        biased;
        _ = control.inner.cancellation.cancelled() => Err(DownloadError::Cancelled),
        result = run_download(config, control.clone(), None) => result,
    };
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
    let result = tokio::select! {
        biased;
        _ = control.inner.cancellation.cancelled() => Err(DownloadError::Cancelled),
        result = run_download(config, control.clone(), Some(descriptors)) => result,
    };
    control.clear_buffered_payload();
    result
}

fn validate_download_config(config: &DownloadConfig) -> Result<(), DownloadError> {
    validate_network_config(config.network)?;
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
    validate_network_config(config.network)
}

fn validate_resumable_magnet_download_config(
    config: &ResumableMagnetDownloadConfig,
) -> Result<(), DownloadError> {
    validate_network_config(config.network)
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
            PeerMessage::Request(_) | PeerMessage::Piece { .. } => {
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
    fn start(peers: &mut PeerSession, info_hash: [u8; 20]) -> Self {
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
    let mut token_cache = UdpTrackerTokenCache::default();
    loop {
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
                let result = tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => return,
                    result = announce_udp_tracker(
                        &url,
                        network_policy,
                        &mut token_cache,
                        UdpTrackerAnnounce {
                            info_hash,
                            key: tracker_key,
                            event,
                        },
                        UdpTrackerExchange {
                            timing: UdpTrackerTiming::PRODUCTION,
                            control: &control,
                            tracker_label: &tracker,
                        },
                    ) => result,
                };
                let now = started_at.elapsed();
                match result {
                    Ok(response) => {
                        let success = schedule.succeeded(id, now, response.interval);
                        control.emit(DownloadActivityEvent::TrackerAnnounceSucceeded {
                            tracker: tracker.clone(),
                            peer_count: response.peers.len().try_into().unwrap_or(u32::MAX),
                            interval_seconds: success.interval.as_secs(),
                        });
                        let send = sender.send(TrackerUpdate::Peers {
                            tracker,
                            peers: response.peers,
                        });
                        tokio::select! {
                            biased;
                            _ = cancellation.cancelled() => return,
                            result = send => {
                                if result.is_err() {
                                    return;
                                }
                            }
                        }
                    }
                    Err(error) => {
                        let failure = schedule.failed(id, now);
                        control.emit(DownloadActivityEvent::TrackerAnnounceFailed {
                            tracker,
                            failures: failure.failures,
                            retry_in_seconds: failure.retry_in.as_secs(),
                            detail: error.to_string(),
                        });
                    }
                }
            }
            TrackerAction::Wait { delay, url, kind } => {
                let tracker = udp_tracker_label(&url);
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
                tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => return,
                    _ = tokio::time::sleep(delay) => {}
                }
            }
            TrackerAction::Exhausted => return,
        }
    }
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
struct PeerSession {
    registry: PeerRegistry,
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

impl PeerSession {
    fn new(network: NetworkConfig, control: DownloadControl) -> Result<Self, DownloadError> {
        validate_network_config(network)?;
        Ok(Self {
            registry: PeerRegistry::new(PeerRegistryConfig::default())
                .map_err(DownloadError::PeerRegistry)?,
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
        if !network.policy.permits_dns() {
            return Err(DownloadError::NetworkDisabled);
        }
        peers.resolve_peer_hints(&magnet.peer_hints).await;
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
        Ok(())
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
            let attempt = self
                .registry
                .begin_dial(candidate, context)
                .map_err(DownloadError::PeerRegistry)?;
            match connect_peer(attempt, info_hash, advertise_extensions, self.network).await {
                Ok((connection, handshake)) => {
                    self.registry
                        .dial_succeeded(attempt, self.elapsed())
                        .map_err(DownloadError::PeerRegistry)?;
                    self.connection = Some(connection);
                    return Ok(handshake);
                }
                Err(error) => {
                    self.registry
                        .dial_failed(attempt, self.elapsed(), peer_failure(&error))
                        .map_err(DownloadError::PeerRegistry)?;
                    self.last_error = Some(error);
                }
            }
        }
    }

    async fn acquire_metadata(
        &mut self,
        info_hash: [u8; 20],
    ) -> Result<(Vec<u8>, Metainfo), DownloadError> {
        debug_assert!(self.connection.is_none());
        let mut sockets = PeerSocketSet::new();
        let mut workers = JoinSet::new();
        let mut worker_cancellations = BTreeMap::new();
        let mut discovery_failed_while_active = false;

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
                let attempt = self
                    .registry
                    .begin_dial(candidate, context)
                    .map_err(DownloadError::PeerRegistry)?;
                if let Err(error) = sockets.begin_dial(attempt, info_hash, true, self.network) {
                    self.registry
                        .dial_cancelled(attempt)
                        .map_err(DownloadError::PeerRegistry)?;
                    return Err(download_peer_set_error(error));
                }
            }

            if sockets.pending_len() == 0 && workers.is_empty() {
                self.receive_discovery_peers(info_hash).await?;
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
                MetadataSupervisorEvent::Discovery(Ok(())) => {
                    discovery_failed_while_active = false;
                }
                MetadataSupervisorEvent::Discovery(Err(error)) => {
                    self.last_error = Some(error);
                    discovery_failed_while_active = true;
                }
                MetadataSupervisorEvent::Cancelled => {
                    let now = self.elapsed();
                    cleanup_metadata_attempts(
                        &mut self.registry,
                        now,
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
                    self.registry
                        .dial_succeeded(attempt, self.elapsed())
                        .map_err(DownloadError::PeerRegistry)?;
                    let cancellation = CancellationToken::new();
                    worker_cancellations.insert(attempt.id(), (attempt, cancellation.clone()));
                    workers.spawn(async move {
                        run_metadata_peer(connection, handshake, info_hash, cancellation).await
                    });
                }
                MetadataSupervisorEvent::Socket(Ok(PeerSetEvent::DialCompleted {
                    attempt,
                    result: Err(error),
                })) => {
                    if matches!(error, PeerSocketError::Cancelled) {
                        self.registry
                            .dial_cancelled(attempt)
                            .map_err(DownloadError::PeerRegistry)?;
                    } else {
                        self.registry
                            .dial_failed(attempt, self.elapsed(), error.peer_failure())
                            .map_err(DownloadError::PeerRegistry)?;
                        self.last_error = Some(download_peer_socket_error(error));
                    }
                }
                MetadataSupervisorEvent::Socket(Ok(PeerSetEvent::Peer(_))) => {
                    let now = self.elapsed();
                    cleanup_metadata_attempts(
                        &mut self.registry,
                        now,
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
                    let now = self.elapsed();
                    cleanup_metadata_attempts(
                        &mut self.registry,
                        now,
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
                    let now = self.elapsed();
                    cleanup_metadata_attempts(
                        &mut self.registry,
                        now,
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
                    self.registry
                        .connection_closed(connection.attempt(), self.elapsed(), Some(failure))
                        .map_err(DownloadError::PeerRegistry)?;
                    self.last_error = Some(error);
                }
                MetadataSupervisorEvent::Worker(Some(Ok(MetadataPeerResult::Cancelled {
                    connection,
                }))) => {
                    worker_cancellations.remove(&connection.attempt().id());
                    self.registry
                        .connection_closed(connection.attempt(), self.elapsed(), None)
                        .map_err(DownloadError::PeerRegistry)?;
                }
                MetadataSupervisorEvent::Worker(Some(Err(error))) => {
                    let now = self.elapsed();
                    cleanup_metadata_attempts(
                        &mut self.registry,
                        now,
                        &mut sockets,
                        &mut workers,
                        &mut worker_cancellations,
                    )
                    .await?;
                    return Err(DownloadError::PeerTask(error.to_string()));
                }
                MetadataSupervisorEvent::Worker(None) => {
                    let now = self.elapsed();
                    cleanup_metadata_attempts(
                        &mut self.registry,
                        now,
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
        self.registry
            .connection_closed(connection.attempt(), self.elapsed(), failure)
            .map_err(DownloadError::PeerRegistry)
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
    info_hash: [u8; 20],
    cancellation: CancellationToken,
) -> MetadataPeerResult {
    let result = tokio::select! {
        biased;
        _ = cancellation.cancelled() => None,
        result = acquire_metadata_from_connection(&mut connection, handshake, info_hash) => {
            Some(result)
        }
    };
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
    registry: &mut PeerRegistry,
    now: Duration,
    sockets: &mut PeerSocketSet,
    workers: &mut JoinSet<MetadataPeerResult>,
    worker_cancellations: &mut BTreeMap<DialAttemptId, (DialAttempt, CancellationToken)>,
) -> Result<(), DownloadError> {
    for (_, cancellation) in worker_cancellations.values() {
        cancellation.cancel();
    }

    let mut first_error = None;
    match std::mem::take(sockets).shutdown().await {
        Ok(pending) => {
            for attempt in pending {
                if let Err(error) = registry.dial_cancelled(attempt)
                    && first_error.is_none()
                {
                    first_error = Some(DownloadError::PeerRegistry(error));
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
                if let Err(error) = registry.connection_closed(attempt, now, None)
                    && first_error.is_none()
                {
                    first_error = Some(DownloadError::PeerRegistry(error));
                }
            }
            Err(error) if first_error.is_none() => {
                first_error = Some(DownloadError::PeerTask(error.to_string()));
            }
            Err(_) => {}
        }
    }
    for (_, (attempt, _)) in std::mem::take(worker_cancellations) {
        if let Err(error) = registry.connection_closed(attempt, now, None)
            && first_error.is_none()
        {
            first_error = Some(DownloadError::PeerRegistry(error));
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
        port: 0,
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
    let mut peers = tokio::select! {
        biased;
        _ = control.inner.cancellation.cancelled() => return Err(DownloadError::Cancelled),
        peers = PeerSession::from_magnet(&magnet, config.network, control.clone(), config.dht.clone()) => peers?,
    };
    let operation_control = control.clone();
    let result = tokio::select! {
        biased;
        _ = control.inner.cancellation.cancelled() => Err(DownloadError::Cancelled),
        result = run_magnet_download_with_peers(
            config,
            operation_control,
            magnet,
            &mut peers,
        ) => result,
    };
    merge_tracker_shutdown(result, peers.shutdown_tracker().await)
}

async fn run_magnet_download_with_peers(
    config: MagnetDownloadConfig,
    control: DownloadControl,
    magnet: Magnet,
    peers: &mut PeerSession,
) -> Result<DownloadReport, DownloadError> {
    let (_raw_info, metainfo) = peers.acquire_metadata(magnet.info_hash).await?;
    let content_config = ContentDownloadConfig {
        output_path: config.output_path,
        max_buffered_payload_bytes: config.max_buffered_payload_bytes,
        swarm_config: SwarmConfig::for_payload_limit(config.max_buffered_payload_bytes),
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
    let mut peers = tokio::select! {
        biased;
        _ = control.inner.cancellation.cancelled() => return Err(DownloadError::Cancelled),
        peers = PeerSession::from_magnet(&magnet, network, control.clone(), dht) => peers?,
    };
    let result = tokio::select! {
        biased;
        _ = control.inner.cancellation.cancelled() => Err(DownloadError::Cancelled),
        result = async {
            let (raw_info, _) = peers.acquire_metadata(magnet.info_hash).await?;
            peers.close_current(None)?;
            Ok(raw_info)
        } => result,
    };
    merge_tracker_shutdown(result, peers.shutdown_tracker().await)
}

fn merge_tracker_shutdown<T>(
    result: Result<T, DownloadError>,
    shutdown: Result<(), DownloadError>,
) -> Result<T, DownloadError> {
    match (result, shutdown) {
        (Ok(value), Ok(())) => Ok(value),
        (Ok(_), Err(error)) | (Err(error), _) => Err(error),
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
        let mut peers = tokio::select! {
            biased;
            _ = control.inner.cancellation.cancelled() => return Err(DownloadError::Cancelled),
            peers = PeerSession::from_magnet(&magnet, config.network, control.clone(), content_dht) => peers?,
        };
        let content_config = ContentDownloadConfig {
            output_path: config.output_path,
            max_buffered_payload_bytes: config.max_buffered_payload_bytes,
            swarm_config: SwarmConfig::for_payload_limit(config.max_buffered_payload_bytes),
            skip_files: config.skip_files,
            materialize_files: Vec::new(),
        };
        let operation_control = control.clone();
        let result = tokio::select! {
            biased;
            _ = control.inner.cancellation.cancelled() => Err(DownloadError::Cancelled),
            result = run_content_download(
                content_config,
                metainfo,
                operation_control,
                descriptors,
                &mut peers,
                Some(resume),
            ) => result,
        };
        return merge_tracker_shutdown(result, peers.shutdown_tracker().await);
    }

    if descriptors.is_some() {
        return Err(DownloadError::Checkpoint(
            "descriptor storage requires verified metadata".to_owned(),
        ));
    }
    let mut peers = tokio::select! {
        biased;
        _ = control.inner.cancellation.cancelled() => return Err(DownloadError::Cancelled),
        peers = PeerSession::from_magnet(&magnet, config.network, control.clone(), dht) => peers?,
    };
    let operation_control = control.clone();
    let result = tokio::select! {
        biased;
        _ = control.inner.cancellation.cancelled() => Err(DownloadError::Cancelled),
        result = async {
            let (raw_info, metainfo) = peers.acquire_metadata(magnet.info_hash).await?;
            if let Err(message) = checkpoints.metadata_verified(&raw_info) {
                peers.close_current(None)?;
                return Err(DownloadError::Checkpoint(message));
            }
            let content_config = ContentDownloadConfig {
                output_path: config.output_path,
                max_buffered_payload_bytes: config.max_buffered_payload_bytes,
                swarm_config: SwarmConfig::for_payload_limit(config.max_buffered_payload_bytes),
                skip_files: config.skip_files,
                materialize_files: Vec::new(),
            };
            run_content_download(
                content_config,
                metainfo,
                operation_control,
                None,
                &mut peers,
                Some(resume),
            )
            .await
        } => result,
    };
    merge_tracker_shutdown(result, peers.shutdown_tracker().await)
}

async fn acquire_metadata_from_connection(
    peer: &mut PeerConnection,
    handshake: Handshake,
    info_hash: [u8; 20],
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
    let mut download = MetadataDownload::new(info_hash);
    let mut remote_metadata_id = None;
    let mut advertised_size = None;
    let mut started = false;
    loop {
        match next_peer_message(peer).await? {
            PeerMessage::Extended { id: 0, payload } => {
                let handshake =
                    parse_extension_handshake(&payload).map_err(DownloadError::Metadata)?;
                if let Some(size) = handshake.metadata_size {
                    if let Some(expected) = advertised_size
                        && expected != size
                    {
                        return Err(DownloadError::Metadata(MetadataError::SizeChanged {
                            expected,
                            actual: size,
                        }));
                    }
                    advertised_size = Some(size);
                }
                let actions = match handshake.metadata_extension {
                    MetadataExtensionUpdate::Disabled => {
                        return Err(DownloadError::MetadataExtensionDisabled);
                    }
                    MetadataExtensionUpdate::Enabled(id) => {
                        remote_metadata_id = Some(id);
                        if started {
                            match handshake.metadata_size {
                                Some(size) => download
                                    .accept_advertised_size(size)
                                    .map_err(DownloadError::Metadata)?,
                                None => Vec::new(),
                            }
                        } else {
                            started = true;
                            download
                                .start(advertised_size)
                                .map_err(DownloadError::Metadata)?
                        }
                    }
                    MetadataExtensionUpdate::Unchanged => {
                        if started {
                            match handshake.metadata_size {
                                Some(size) => download
                                    .accept_advertised_size(size)
                                    .map_err(DownloadError::Metadata)?,
                                None => Vec::new(),
                            }
                        } else {
                            Vec::new()
                        }
                    }
                };
                if let Some(bytes) =
                    process_metadata_download_actions(peer, remote_metadata_id, actions).await?
                {
                    return finish_metadata_acquisition(bytes, peer_state, peer);
                }
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
                let actions = download
                    .on_message(message)
                    .map_err(DownloadError::Metadata)?;
                if let Some(bytes) =
                    process_metadata_download_actions(peer, remote_metadata_id, actions).await?
                {
                    return finish_metadata_acquisition(bytes, peer_state, peer);
                }
            }
            PeerMessage::Extended { .. } => {}
            message => peer_state.observe(message)?,
        }
    }
}

async fn process_metadata_download_actions(
    peer: &mut PeerConnection,
    remote_metadata_id: Option<u8>,
    actions: Vec<MetadataDownloadAction>,
) -> Result<Option<Vec<u8>>, DownloadError> {
    for action in actions {
        match action {
            MetadataDownloadAction::Request(piece) => {
                let remote_id =
                    remote_metadata_id.ok_or(DownloadError::MetadataExtensionDisabled)?;
                send_message(
                    peer,
                    &PeerMessage::Extended {
                        id: remote_id,
                        payload: encode_metadata_request(piece),
                    },
                )
                .await?;
            }
            MetadataDownloadAction::Complete(bytes) => return Ok(Some(bytes)),
        }
    }
    Ok(None)
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
    let mut peers = PeerSession::from_endpoint(config.peer, PeerSource::Manual, config.network)?;
    let content_config = ContentDownloadConfig {
        output_path: config.output_path,
        max_buffered_payload_bytes: config.max_buffered_payload_bytes,
        swarm_config: SwarmConfig::for_payload_limit(config.max_buffered_payload_bytes),
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
    peers: &mut PeerSession,
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
    peers: &mut PeerSession,
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
    let mut storage = StagingFile::create(config.output_path.clone(), metainfo.total_length)
        .await
        .map_err(DownloadError::Storage)?;
    let download = ContentSwarmDownload::new(
        config.swarm_config,
        plans,
        ContentStorage::Single(&mut storage),
        &metainfo,
        &layout,
        None,
        &control,
    )?;
    let completed = download_content_swarm(peers, download).await?;
    let piece = completed
        .last_piece
        .expect("completed single-file swarm has a verified piece");
    let block_count = completed.total_blocks;
    let bytes_written = completed.total_bytes;
    let selected_written_bytes = completed.selected_written_bytes;
    let payload_high_water = completed.state.snapshot(peers.elapsed()).payload_high_water;
    drop(completed);
    storage.finalize().await.map_err(DownloadError::Storage)?;
    Ok(DownloadReport {
        info_hash: metainfo.info_hash,
        piece_hash: piece.hash,
        bytes_written,
        block_count,
        payload_limit: config.max_buffered_payload_bytes,
        payload_high_water,
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

struct ContentSwarmDownload<'a> {
    state: SwarmState,
    storage: ContentStorage<'a>,
    metainfo: &'a Metainfo,
    layout: &'a TorrentLayout,
    resume: Option<&'a ResumeContext>,
    control: &'a DownloadControl,
    total_blocks: usize,
    total_bytes: usize,
    selected_written_bytes: usize,
    part_written_bytes: usize,
    last_piece: Option<VerifiedPiece>,
}

enum ContentStorage<'a> {
    Single(&'a mut StagingFile),
    Selective(&'a mut SelectiveStorage),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ContentMessageDisposition {
    Continue,
    ClosePeer(PeerFailure),
}

impl<'a> ContentSwarmDownload<'a> {
    fn new(
        config: SwarmConfig,
        plans: Vec<(u32, Vec<rstorrent_protocol::storage_layout::RequestRange>)>,
        storage: ContentStorage<'a>,
        metainfo: &'a Metainfo,
        layout: &'a TorrentLayout,
        resume: Option<&'a ResumeContext>,
        control: &'a DownloadControl,
    ) -> Result<Self, DownloadError> {
        let mut total_blocks = 0;
        let mut total_bytes = 0;
        let mut swarm_plans = Vec::with_capacity(plans.len());
        for (piece, ranges) in plans {
            let piece_length = layout
                .piece_length_at(piece)
                .map_err(DownloadError::Layout)?;
            control.emit(DownloadActivityEvent::PieceStarted {
                piece_index: piece,
                piece_length,
            });
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
            storage,
            metainfo,
            layout,
            resume,
            control,
            total_blocks,
            total_bytes,
            selected_written_bytes: 0,
            part_written_bytes: 0,
            last_piece: None,
        })
    }

    fn is_complete(&self, now: Duration) -> bool {
        self.state.snapshot(now).no_request_reason == Some(NoRequestReason::Complete)
    }

    async fn handle_message(
        &mut self,
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
                    .receive_block(connection, key)
                    .map_err(DownloadError::Swarm)?
                {
                    ReceiveDisposition::Accept { .. } => {
                        self.control.record_received(block.len());
                        self.control.emit(DownloadActivityEvent::BlockReceived {
                            piece_index: index,
                            begin,
                            length,
                        });
                        self.control.wait_before_storage().await;
                        let block_length = block.len();
                        let write_result = match &mut self.storage {
                            ContentStorage::Single(storage) => {
                                let offset = single_file_offset(self.layout, index, begin)?;
                                storage
                                    .write_block(offset, block)
                                    .await
                                    .map(|()| (block_length, 0))
                                    .map_err(DownloadError::Storage)
                            }
                            ContentStorage::Selective(storage) => storage
                                .write_block(index, begin, block)
                                .await
                                .map(|stats| (stats.wanted_bytes, stats.skipped_bytes))
                                .map_err(DownloadError::SelectiveStorage),
                        };
                        let stats = match write_result {
                            Ok(stats) => stats,
                            Err(error) => {
                                self.state
                                    .finish_write(key, false, now)
                                    .map_err(DownloadError::Swarm)?;
                                return Err(error);
                            }
                        };
                        self.state
                            .finish_write(key, true, now)
                            .map_err(DownloadError::Swarm)?;
                        self.selected_written_bytes += stats.0;
                        self.part_written_bytes += stats.1;
                        self.control.record_stored(length as usize);
                        self.control.emit(DownloadActivityEvent::BlockStored {
                            piece_index: index,
                            begin,
                            length,
                        });
                        if self
                            .state
                            .piece_ready(index)
                            .map_err(DownloadError::Swarm)?
                        {
                            self.verify_piece(index).await?;
                        }
                    }
                    ReceiveDisposition::Redundant | ReceiveDisposition::Unsolicited => {}
                }
            }
            PeerMessage::KeepAlive
            | PeerMessage::Interested
            | PeerMessage::NotInterested
            | PeerMessage::Request(_)
            | PeerMessage::Extended { .. } => {}
        }
        self.control.observe_swarm(&self.state, now);
        Ok(ContentMessageDisposition::Continue)
    }

    async fn verify_piece(&mut self, piece: u32) -> Result<(), DownloadError> {
        let piece_index = usize::try_from(piece)
            .map_err(|_| DownloadError::Layout(LayoutError::ArithmeticOverflow))?;
        let piece_length = self
            .layout
            .piece_length_at(piece)
            .map_err(DownloadError::Layout)?;
        let actual = match &mut self.storage {
            ContentStorage::Single(storage) => {
                let offset = single_file_offset(self.layout, piece, 0)?;
                storage
                    .hash_piece(offset, piece_length)
                    .await
                    .map_err(DownloadError::Storage)?
            }
            ContentStorage::Selective(storage) => storage
                .hash_piece(piece)
                .await
                .map_err(DownloadError::SelectiveStorage)?,
        };
        let expected = self.metainfo.piece_hashes[piece_index];
        if actual != expected {
            return Err(DownloadError::Piece(PieceError::HashMismatch {
                expected,
                actual,
            }));
        }
        self.state
            .mark_piece_verified(piece)
            .map_err(DownloadError::Swarm)?;
        if let ContentStorage::Selective(storage) = &mut self.storage {
            if let Some(resume) = self.resume {
                storage
                    .sync_piece(piece)
                    .await
                    .map_err(DownloadError::SelectiveStorage)?;
                storage
                    .record_verified(piece_index)
                    .map_err(DownloadError::SelectiveStorage)?;
                resume
                    .checkpoints
                    .piece_durable(piece_index)
                    .map_err(DownloadError::Checkpoint)?;
            } else {
                storage
                    .record_verified(piece_index)
                    .map_err(DownloadError::SelectiveStorage)?;
            }
        }
        self.last_piece = Some(VerifiedPiece {
            index: piece,
            hash: actual,
            length: piece_length,
        });
        self.control
            .emit(DownloadActivityEvent::PieceVerified { piece_index: piece });
        Ok(())
    }
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

fn fill_content_dials(
    peers: &mut PeerSession,
    sockets: &mut PeerSocketSet,
    state: &mut SwarmState,
    info_hash: [u8; 20],
) -> Result<usize, DownloadError> {
    let mut started = 0;
    while sockets.pending_len() < state.config().max_pending_dials {
        let at_capacity = sockets.established_len() >= state.config().max_established_connections;
        if at_capacity {
            if sockets.pending_len() != 0 || state.replacement_candidate(peers.elapsed()).is_none()
            {
                break;
            }
        } else if sockets.established_len() + sockets.pending_len()
            >= state.config().max_established_connections
        {
            break;
        }
        let context = PeerSelectionContext {
            now: peers.elapsed(),
        };
        let Some(candidate) = peers.selector.select(&peers.registry, context) else {
            break;
        };
        peers.control.emit(DownloadActivityEvent::PeerDialStarted {
            peer: candidate.endpoint().to_string(),
        });
        let attempt = peers
            .registry
            .begin_dial(candidate, context)
            .map_err(DownloadError::PeerRegistry)?;
        state
            .begin_dial(pending_dial_id(attempt))
            .map_err(DownloadError::Swarm)?;
        if let Err(error) = sockets.begin_dial(attempt, info_hash, false, peers.network) {
            state
                .finish_dial(pending_dial_id(attempt))
                .map_err(DownloadError::Swarm)?;
            peers
                .registry
                .dial_cancelled(attempt)
                .map_err(DownloadError::PeerRegistry)?;
            return Err(download_peer_set_error(error));
        }
        started += 1;
    }
    Ok(started)
}

async fn close_content_connection(
    peers: &mut PeerSession,
    sockets: &mut PeerSocketSet,
    state: &mut SwarmState,
    connection: ConnectionId,
    failure: Option<PeerFailure>,
) -> Result<(), DownloadError> {
    if !sockets.contains(connection) {
        return Ok(());
    }
    let attempt = sockets
        .remove_connection(connection)
        .await
        .map_err(download_peer_set_error)?;
    state
        .remove_connection(connection, ConnectionRemoval::Disconnected)
        .map_err(DownloadError::Swarm)?;
    peers
        .registry
        .connection_closed(attempt, peers.elapsed(), failure)
        .map_err(DownloadError::PeerRegistry)
}

async fn replace_content_connection(
    peers: &mut PeerSession,
    sockets: &mut PeerSocketSet,
    state: &mut SwarmState,
    connection: ConnectionId,
) -> Result<(), DownloadError> {
    let attempt = sockets
        .remove_connection(connection)
        .await
        .map_err(download_peer_set_error)?;
    state
        .remove_connection(connection, ConnectionRemoval::Replaced)
        .map_err(DownloadError::Swarm)?;
    peers
        .registry
        .connection_closed(attempt, peers.elapsed(), None)
        .map_err(DownloadError::PeerRegistry)
}

async fn cleanup_content_connections(
    peers: &mut PeerSession,
    sockets: PeerSocketSet,
    state: &mut SwarmState,
    failure: Option<PeerFailure>,
) -> Result<(), DownloadError> {
    let active = sockets.connection_attempts();
    let pending = sockets.shutdown().await.map_err(download_peer_set_error)?;
    for attempt in active {
        peers
            .registry
            .connection_closed(attempt, peers.elapsed(), failure)
            .map_err(DownloadError::PeerRegistry)?;
    }
    for attempt in pending {
        peers
            .registry
            .dial_cancelled(attempt)
            .map_err(DownloadError::PeerRegistry)?;
    }
    state.cancel_all().map_err(DownloadError::Swarm)?;
    peers.control.clear_buffered_payload();
    Ok(())
}

enum ContentSupervisorEvent {
    Peer(PeerSetEvent),
    Discovery(Option<ContentDiscoveryEvent>),
    Deadline,
}

async fn next_content_supervisor_event(
    sockets: &mut PeerSocketSet,
    discovery: &mut ContentDiscovery,
    until_expiry: Option<Duration>,
    cancellation: &CancellationToken,
) -> Result<ContentSupervisorEvent, DownloadError> {
    if let Some(wait) = until_expiry {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(DownloadError::Cancelled),
            _ = tokio::time::sleep(wait) => Ok(ContentSupervisorEvent::Deadline),
            event = sockets.next_event() => event
                .map(ContentSupervisorEvent::Peer)
                .map_err(download_peer_set_error),
            event = discovery.next_event(), if discovery.is_active() => {
                Ok(ContentSupervisorEvent::Discovery(event))
            }
        }
    } else {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(DownloadError::Cancelled),
            event = sockets.next_event() => event
                .map(ContentSupervisorEvent::Peer)
                .map_err(download_peer_set_error),
            event = discovery.next_event(), if discovery.is_active() => {
                Ok(ContentSupervisorEvent::Discovery(event))
            }
        }
    }
}

async fn run_selective_swarm_loop(
    peers: &mut PeerSession,
    sockets: &mut PeerSocketSet,
    discovery: &mut ContentDiscovery,
    download: &mut ContentSwarmDownload<'_>,
) -> Result<(), DownloadError> {
    if let Some(connection) = peers.connection.take() {
        let attempt = connection.attempt();
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
        let expired = download
            .state
            .expire_requests(now)
            .map_err(DownloadError::Swarm)?;
        for request in expired {
            let _ = request;
        }
        download.control.observe_swarm(&download.state, now);

        let assignments = download.state.schedule(now).map_err(DownloadError::Swarm)?;
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
            download
                .control
                .record_requested(assignment.block.length as usize);
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
        if download.is_complete(peers.elapsed()) {
            return Ok(());
        }

        fill_content_dials(
            peers,
            sockets,
            &mut download.state,
            download.metainfo.info_hash,
        )?;
        if sockets.established_len() == 0 && sockets.pending_len() == 0 && !discovery.is_active() {
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
        let next_deadline = [snapshot.next_request_expiry, replacement_deadline]
            .into_iter()
            .flatten()
            .min();
        let until_expiry = next_deadline.map(|deadline| deadline.saturating_sub(peers.elapsed()));
        let event = next_content_supervisor_event(
            sockets,
            discovery,
            until_expiry,
            &peers.control.inner.cancellation,
        )
        .await?;

        match event {
            ContentSupervisorEvent::Deadline => continue,
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
            ContentSupervisorEvent::Peer(PeerSetEvent::DialCompleted { attempt, result }) => {
                download
                    .state
                    .finish_dial(pending_dial_id(attempt))
                    .map_err(DownloadError::Swarm)?;
                match result {
                    Ok((connection, _handshake)) => {
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
                                peers
                                    .registry
                                    .dial_succeeded(attempt, peers.elapsed())
                                    .map_err(DownloadError::PeerRegistry)?;
                                peers
                                    .registry
                                    .connection_closed(attempt, peers.elapsed(), None)
                                    .map_err(DownloadError::PeerRegistry)?;
                                continue;
                            }
                        }
                        peers
                            .registry
                            .dial_succeeded(attempt, peers.elapsed())
                            .map_err(DownloadError::PeerRegistry)?;
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
                        peers
                            .registry
                            .dial_failed(attempt, peers.elapsed(), failure)
                            .map_err(DownloadError::PeerRegistry)?;
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
                if download
                    .handle_message(id, message, peers.elapsed())
                    .await?
                    == ContentMessageDisposition::ClosePeer(PeerFailure::Protocol)
                {
                    close_content_connection(
                        peers,
                        sockets,
                        &mut download.state,
                        id,
                        Some(PeerFailure::Protocol),
                    )
                    .await?;
                }
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

async fn download_content_swarm<'a>(
    peers: &mut PeerSession,
    mut download: ContentSwarmDownload<'a>,
) -> Result<ContentSwarmDownload<'a>, DownloadError> {
    let mut sockets = PeerSocketSet::new();
    let mut discovery = ContentDiscovery::start(peers, download.metainfo.info_hash);
    let result = run_selective_swarm_loop(peers, &mut sockets, &mut discovery, &mut download).await;
    let failure = result.as_ref().err().and_then(content_peer_failure);
    let discovery_cleanup = discovery.shutdown().await;
    let peer_cleanup =
        cleanup_content_connections(peers, sockets, &mut download.state, failure).await;
    let cleanup = match (discovery_cleanup, peer_cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(first), Err(second)) => Err(DownloadError::PeerTask(format!(
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
    peers: &mut PeerSession,
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
        payload_high_water,
        last_piece,
    ) = if plans.is_empty() {
        (0, 0, 0, 0, 0, None)
    } else {
        let download = ContentSwarmDownload::new(
            config.swarm_config,
            plans,
            ContentStorage::Selective(&mut storage),
            &metainfo,
            &layout,
            resume.as_ref(),
            &control,
        )?;
        let completed = download_content_swarm(peers, download).await?;
        let result = (
            completed.total_blocks,
            completed.total_bytes,
            completed.selected_written_bytes,
            completed.part_written_bytes,
            completed.state.snapshot(peers.elapsed()).payload_high_water,
            completed.last_piece,
        );
        drop(completed);
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
        payload_high_water,
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
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use rstorrent_protocol::dht::{
        DhtEndpoint, DhtIp, Message as DhtMessage, NodeId, Query as DhtQuery, Want,
        decode_message as decode_dht, encode_response as encode_dht_response,
    };
    use rstorrent_protocol::magnet::Magnet;
    use rstorrent_protocol::metadata::{
        MetadataMessage, encode_extension_handshake, encode_metadata_data, parse_metadata_message,
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
    use tokio::time::timeout;

    use super::{
        CLIENT_PEER_ID, ContentDownloadConfig, DhtRetryTiming, DownloadActivityEvent,
        DownloadActivitySink, DownloadConfig, DownloadControl, DownloadError, MagnetDownloadConfig,
        PeerConnection, PeerSession, SwarmConfig, UdpTrackerAnnounce, UdpTrackerExchange,
        UdpTrackerTiming, UdpTrackerTokenCache, announce_udp_tracker_address, download_magnet,
        download_magnet_metadata_with_control, download_magnet_metadata_with_dht,
        download_magnet_with_control, download_verified_piece,
        download_verified_piece_with_control, next_peer_message, retrying_dht_lookup,
        run_content_download, run_magnet_download_with_peers, send_message,
    };
    use crate::dht::{BootstrapNode, DhtConfig, DhtService};
    use crate::network::{NetworkConfig, NetworkPolicy};
    use crate::peer::{
        DialAttempt, PeerEndpoint, PeerFailure, PeerObservation, PeerPhase, PeerRegistry,
        PeerRegistryConfig, PeerSelectionContext, PeerSelector, PeerSource,
    };
    use crate::selective_storage::{
        SelectiveStorageError, selective_part_path, selective_staging_path,
    };
    use crate::storage::staging_path;

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn loopback_network(timeout: Duration) -> NetworkConfig {
        NetworkConfig::new(NetworkPolicy::LoopbackOnly, timeout, timeout)
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

    #[tokio::test]
    async fn explicit_policies_gate_non_loopback_peers_and_offline_dns() {
        let public = "192.0.2.1:6881".parse().expect("documentation peer");
        let loopback = PeerSession::from_endpoint(
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

        let online = PeerSession::from_endpoint(
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
        let mut peers = PeerSession::from_endpoint(
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

        let raw_info = match timeout(Duration::from_secs(90), &mut task).await {
            Ok(result) => result
                .expect("join public metadata probe")
                .expect("acquire public metadata"),
            Err(_) => {
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

        let raw_info = match timeout(Duration::from_secs(120), &mut task).await {
            Ok(result) => result
                .expect("join public DHT metadata probe")
                .expect("acquire public DHT metadata"),
            Err(_) => {
                let stats = dht.handle().stats().await.ok();
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
        let mut peer =
            PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(2));
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
                Ok(message) => panic!("choked peer received command {message:?}"),
                Err(error) => panic!("choked peer failed: {error}"),
            }
        }
    }

    async fn accept_handshake_without_reply(listener: TcpListener) {
        let (mut stream, _) = listener.accept().await.expect("accept silent peer");
        let mut handshake = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake)
            .await
            .expect("read silent handshake");
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

    async fn serve_delayed_block_peer(
        listener: TcpListener,
        info_hash: [u8; 20],
        payload: Vec<u8>,
        delay: Duration,
        keepalive_interval: Option<Duration>,
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
        let mut peer =
            PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(2));
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
                Ok(PeerMessage::Request(_)) | Ok(PeerMessage::Interested) => {}
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
        let mut peers = PeerSession::from_endpoint(
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
                    swarm_config: SwarmConfig::for_payload_limit(2 * MIN_PAYLOAD_ALLOWANCE),
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
        let mut peers = PeerSession::from_endpoint(
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
                    swarm_config: SwarmConfig::for_payload_limit(2 * MIN_PAYLOAD_ALLOWANCE),
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
        let mut peers = PeerSession::from_endpoint(
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
                    swarm_config: SwarmConfig::for_payload_limit(2 * MIN_PAYLOAD_ALLOWANCE),
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
    async fn disconnect_and_choke_reassign_only_their_outstanding_blocks() {
        run_adverse_reassignment_case(AdverseRequestAction::Disconnect).await;
        run_adverse_reassignment_case(AdverseRequestAction::Choke).await;
    }

    #[tokio::test]
    async fn silent_handshakes_do_not_block_a_parallel_useful_peer() {
        let first = vec![0x13; 16 * 1024];
        let second = vec![0x57; 16 * 1024];
        let metainfo =
            Metainfo::from_bytes(&two_piece_metainfo(&first, &second)).expect("two-piece metainfo");
        let silent_a = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind silent A");
        let address_a = silent_a.local_addr().expect("silent A address");
        let silent_b = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind silent B");
        let address_b = silent_b.local_addr().expect("silent B address");
        let useful = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind useful peer");
        let useful_address = useful.local_addr().expect("useful address");
        let silent_task_a = tokio::spawn(accept_handshake_without_reply(silent_a));
        let silent_task_b = tokio::spawn(accept_handshake_without_reply(silent_b));
        let useful_task = tokio::spawn(serve_content_peer(
            useful,
            metainfo.info_hash,
            Arc::new(vec![first, second]),
            vec![true, true],
        ));
        let mut peers = PeerSession::from_endpoint(
            address_a,
            PeerSource::Manual,
            loopback_network(Duration::from_secs(5)),
        )
        .expect("peer session");
        peers
            .observe_address(address_b, PeerSource::Manual)
            .expect("silent B");
        peers
            .observe_address(useful_address, PeerSource::Manual)
            .expect("useful peer");
        let output = test_path("silent-handshake-parallel");
        let report = timeout(
            Duration::from_secs(2),
            run_content_download(
                ContentDownloadConfig {
                    output_path: output.clone(),
                    max_buffered_payload_bytes: 2 * MIN_PAYLOAD_ALLOWANCE,
                    swarm_config: SwarmConfig::for_payload_limit(2 * MIN_PAYLOAD_ALLOWANCE),
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
        for task in [silent_task_a, silent_task_b, useful_task] {
            timeout(Duration::from_secs(1), task)
                .await
                .expect("peer joined")
                .expect("peer task");
        }
        let _ = tokio::fs::remove_dir_all(output).await;
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
        let mut peers = PeerSession::from_endpoint(
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
        let mut swarm_config = SwarmConfig::for_payload_limit(2 * MIN_PAYLOAD_ALLOWANCE);
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
        let mut peers = PeerSession::from_endpoint(
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
        let mut swarm_config = SwarmConfig::for_payload_limit(2 * MIN_PAYLOAD_ALLOWANCE);
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
        let mut peers = PeerSession::from_endpoint(
            address,
            PeerSource::Manual,
            loopback_network(Duration::from_secs(2)),
        )
        .expect("peer session");
        let mut swarm_config = SwarmConfig::for_payload_limit(MIN_PAYLOAD_ALLOWANCE);
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
        let mut peers = PeerSession::from_endpoint(
            old_address,
            PeerSource::Manual,
            loopback_network(Duration::from_secs(2)),
        )
        .expect("peer session");
        peers
            .observe_address(replacement_address, PeerSource::Manual)
            .expect("replacement peer");
        let mut swarm_config = SwarmConfig::for_payload_limit(MIN_PAYLOAD_ALLOWANCE);
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
            PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(1));

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
            PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(2));
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
            0
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
        let mut peers = PeerSession::from_magnet(&parsed, network, control.clone(), None)
            .await
            .expect("prepare tracker discovery");
        assert!(peers.registry.is_empty());

        let report = run_magnet_download_with_peers(
            MagnetDownloadConfig {
                magnet,
                output_path: output_path.clone(),
                network,
                max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
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
                max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
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
        let mut peers = PeerSession::from_endpoint(
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
                    swarm_config: SwarmConfig::for_payload_limit(MIN_PAYLOAD_ALLOWANCE),
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
                max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
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
        let mut peers = PeerSession::from_magnet(
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
            },
            UdpTrackerExchange {
                timing,
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
        let network = loopback_network(Duration::from_secs(2));
        let mut peers = PeerSession::from_magnet(&parsed, network, DownloadControl::new(), None)
            .await
            .expect("resolve metadata peers");

        let (raw_info, metainfo) =
            timeout(Duration::from_secs(1), peers.acquire_metadata(info_hash))
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
        let mut peers = PeerSession::from_magnet(
            &magnet,
            loopback_network(Duration::from_secs(2)),
            DownloadControl::new(),
            None,
        )
        .await
        .expect("start metadata discovery");

        let (_, metainfo) = timeout(Duration::from_secs(1), peers.acquire_metadata(info_hash))
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
        let mut peers = PeerSession::from_magnet(&parsed, network, DownloadControl::new(), None)
            .await
            .expect("resolve failover peers");
        assert_eq!(peers.registry.len(), 2);

        let report = run_magnet_download_with_peers(
            MagnetDownloadConfig {
                magnet,
                output_path: output_path.clone(),
                network,
                max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
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
            max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
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
            max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
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
            max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
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
            max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
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
            max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
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
            max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
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
            max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
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
            max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
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
            let (_stream, _) = listener.accept().await.expect("accept diagnostic");
            tokio::time::sleep(Duration::from_secs(1)).await;
        });

        let control = DownloadControl::new();
        let download_control = control.clone();
        let download_task = tokio::spawn(download_verified_piece_with_control(
            DownloadConfig {
                metainfo_path: metainfo_path.clone(),
                peer: address,
                output_path: output_path.clone(),
                network: loopback_network(Duration::from_secs(5)),
                max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
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
        assert!(!tokio::fs::try_exists(&output_path).await.expect("output"));
        assert!(!tokio::fs::try_exists(&staging).await.expect("staging"));
        assert!(!tokio::fs::try_exists(&part).await.expect("part"));

        peer_task.abort();
        let _ = peer_task.await;
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
            max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
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
