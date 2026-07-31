use std::collections::{BTreeSet, VecDeque};
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
use rstorrent_protocol::peer_wire::{
    EXTENSION_PROTOCOL_RESERVED_BIT, EXTENSION_PROTOCOL_RESERVED_INDEX, FrameDecoder, FrameError,
    HANDSHAKE_LENGTH, Handshake, HandshakeError, PeerMessage, decode_handshake, encode_handshake,
    encode_handshake_with_reserved, encode_message,
};
use rstorrent_protocol::piece::{DownloadAction, OnePieceDownload, PieceError, VerifiedPiece};
use rstorrent_protocol::storage_layout::{FileSelection, LayoutError, TorrentLayout};
use rstorrent_protocol::udp_tracker::{
    AnnounceEvent, AnnounceRequest, AnnounceResponse, CompactPeer, MAX_ANNOUNCE_RESPONSE_LENGTH,
    MAX_COMPACT_PEERS, TrackerAddressFamily, TransactionId, UdpTrackerError,
    encode_announce_request, encode_connect_request, parse_announce_response,
    parse_connect_response,
};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket, lookup_host};
use tokio::time::{Instant as TokioInstant, timeout, timeout_at};
use tokio_util::sync::CancellationToken;

use crate::network::{NetworkConfig, NetworkPolicy};
use crate::peer::{
    DialAttempt, PeerEndpoint, PeerFailure, PeerObservation, PeerRegistry, PeerRegistryConfig,
    PeerRegistryError, PeerSelectionContext, PeerSelector, PeerSource,
};
use crate::selective_storage::{
    DescriptorStorage, PreparedFileHash, ResumedStorage, SelectiveStorage, SelectiveStorageError,
    remove_selective_part_if_present, remove_selective_staging_if_present,
};
use crate::storage::{
    StagingFile, StorageError, VERIFICATION_CHUNK_LENGTH, remove_staging_if_present, staging_path,
};

const CLIENT_PEER_ID: [u8; 20] = *b"-RS0001-000000000000";
const NETWORK_READ_LENGTH: usize = 16 * 1024;
const NETWORK_RESOLUTION_TIMEOUT: Duration = Duration::from_secs(10);
const UDP_TRACKER_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_RESOLVED_ADDRESSES: usize = 32;
const UNKNOWN_MAGNET_LEFT: u64 = 16 * 1024;
const UDP_TRACKER_RECEIVE_LENGTH: usize = MAX_ANNOUNCE_RESPONSE_LENGTH + 1;
const SAFE_CANCEL_REQUESTED: usize = 1 << (usize::BITS - 1);
const SAFE_CANCEL_CRITICAL_MASK: usize = SAFE_CANCEL_REQUESTED - 1;

#[derive(Debug)]
struct PeerConnection {
    attempt: DialAttempt,
    stream: TcpStream,
    decoder: FrameDecoder,
    queued_messages: VecDeque<PeerMessage>,
    io_timeout: Duration,
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
}

#[derive(Clone, Debug)]
struct ContentDownloadConfig {
    output_path: PathBuf,
    max_buffered_payload_bytes: usize,
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

    fn observe(&self, download: &OnePieceDownload) {
        let budget = download.payload_budget();
        self.inner
            .buffered_payload_bytes
            .store(budget.reserved, Ordering::Release);
        self.inner
            .payload_high_water
            .fetch_max(budget.high_water, Ordering::AcqRel);
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
    UdpTracker(UdpTrackerError),
    PeerRegistry(PeerRegistryError),
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
            Self::UdpTracker(error) => write!(formatter, "UDP tracker: {error}"),
            Self::PeerRegistry(error) => write!(formatter, "peer registry: {error}"),
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
            Self::UdpTracker(error) => Some(error),
            Self::PeerRegistry(error) => Some(error),
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
    let result = tokio::select! {
        biased;
        _ = control.inner.cancellation.cancelled() => Err(DownloadError::Cancelled),
        result = run_resumable_magnet_download(
            config,
            checkpoints,
            control.clone(),
            descriptors,
        ) => result,
    };
    control.clear_buffered_payload();
    result
}

pub async fn download_magnet_metadata_with_control(
    magnet: String,
    network: NetworkConfig,
    control: DownloadControl,
) -> Result<Vec<u8>, DownloadError> {
    validate_network_config(network)?;
    let result = tokio::select! {
        biased;
        _ = control.inner.cancellation.cancelled() => Err(DownloadError::Cancelled),
        result = run_magnet_metadata(magnet, network) => result,
    };
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
    let result = tokio::select! {
        biased;
        _ = control.inner.cancellation.cancelled() => Err(DownloadError::Cancelled),
        result = run_magnet_download(config, control.clone()) => result,
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

#[derive(Debug)]
struct PeerSession {
    registry: PeerRegistry,
    selector: PeerSelector,
    started_at: Instant,
    network: NetworkConfig,
    udp_trackers: Vec<UdpTrackerUrl>,
    next_udp_tracker: usize,
    tracker_key: Option<u32>,
    connection: Option<PeerConnection>,
    last_error: Option<DownloadError>,
}

impl PeerSession {
    fn new(network: NetworkConfig) -> Result<Self, DownloadError> {
        validate_network_config(network)?;
        Ok(Self {
            registry: PeerRegistry::new(PeerRegistryConfig::default())
                .map_err(DownloadError::PeerRegistry)?,
            selector: PeerSelector,
            started_at: Instant::now(),
            network,
            udp_trackers: Vec::new(),
            next_udp_tracker: 0,
            tracker_key: None,
            connection: None,
            last_error: None,
        })
    }

    fn from_endpoint(
        address: SocketAddr,
        source: PeerSource,
        network: NetworkConfig,
    ) -> Result<Self, DownloadError> {
        let mut peers = Self::new(network)?;
        if matches!(network.policy, NetworkPolicy::Offline) {
            return Err(DownloadError::NetworkDisabled);
        }
        peers.observe_address(address, source)?;
        Ok(peers)
    }

    async fn from_magnet(magnet: &Magnet, network: NetworkConfig) -> Result<Self, DownloadError> {
        let mut peers = Self::new(network)?;
        if !network.policy.permits_dns() {
            return Err(DownloadError::NetworkDisabled);
        }
        peers.udp_trackers.clone_from(&magnet.udp_trackers);
        peers.resolve_peer_hints(&magnet.peer_hints).await;
        if peers.registry.is_empty() && peers.udp_trackers.is_empty() {
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

    async fn discover_next_tracker(&mut self, info_hash: [u8; 20]) -> bool {
        let tracker_key = match self.tracker_key {
            Some(tracker_key) => tracker_key,
            None => match random_nonzero_u32() {
                Ok(tracker_key) => {
                    self.tracker_key = Some(tracker_key);
                    tracker_key
                }
                Err(error) => {
                    self.last_error = Some(error);
                    return false;
                }
            },
        };
        while let Some(tracker) = self.udp_trackers.get(self.next_udp_tracker).cloned() {
            self.next_udp_tracker += 1;
            match announce_udp_tracker(&tracker, info_hash, tracker_key, self.network.policy).await
            {
                Ok(response) => {
                    for compact_peer in response.peers {
                        let address = compact_peer_address(compact_peer);
                        if let Err(error) = self.observe_address(address, PeerSource::Tracker) {
                            self.last_error = Some(error);
                        }
                    }
                    let context = PeerSelectionContext {
                        now: self.elapsed(),
                    };
                    if self.selector.select(&self.registry, context).is_some() {
                        return true;
                    }
                    self.last_error = Some(DownloadError::NoUsablePeer);
                }
                Err(error) => self.last_error = Some(error),
            }
        }
        false
    }

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
                    if self.discover_next_tracker(info_hash).await {
                        continue;
                    }
                    return Err(self
                        .last_error
                        .take()
                        .unwrap_or(DownloadError::NoUsablePeer));
                }
            };
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

    async fn ensure_content_connection(
        &mut self,
        info_hash: [u8; 20],
    ) -> Result<(), DownloadError> {
        if self.connection.is_none() {
            self.connect_next(info_hash, false).await?;
        }
        Ok(())
    }

    async fn acquire_metadata(
        &mut self,
        info_hash: [u8; 20],
    ) -> Result<(Vec<u8>, Metainfo), DownloadError> {
        loop {
            let handshake = self.connect_next(info_hash, true).await?;
            let result = acquire_metadata_from_connection(
                self.connection
                    .as_mut()
                    .expect("successful dial installs peer connection"),
                handshake,
                info_hash,
            )
            .await;
            match result {
                Ok(metadata) => return Ok(metadata),
                Err(error) => {
                    let failure = peer_failure(&error);
                    self.close_current(Some(failure))?;
                    self.last_error = Some(error);
                }
            }
        }
    }

    fn connection_mut(&mut self) -> Result<&mut PeerConnection, DownloadError> {
        self.connection.as_mut().ok_or(DownloadError::NoUsablePeer)
    }

    fn close_current(&mut self, failure: Option<PeerFailure>) -> Result<(), DownloadError> {
        let Some(connection) = self.connection.take() else {
            return Ok(());
        };
        self.registry
            .connection_closed(connection.attempt, self.elapsed(), failure)
            .map_err(DownloadError::PeerRegistry)
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
    info_hash: [u8; 20],
    key: u32,
    network_policy: NetworkPolicy,
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
        match announce_udp_tracker_address(address, info_hash, key).await {
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
    info_hash: [u8; 20],
    key: u32,
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

    let connect_transaction = TransactionId::new(random_nonzero_u32()?);
    send_udp_tracker_packet(
        &socket,
        &encode_connect_request(connect_transaction),
        "send UDP tracker connect",
    )
    .await?;
    let connection_id = receive_udp_tracker_response(&socket, "connect response", |bytes| {
        parse_connect_response(bytes, connect_transaction)
    })
    .await?;

    let mut announce_transaction = TransactionId::new(random_nonzero_u32()?);
    if announce_transaction == connect_transaction {
        announce_transaction = TransactionId::new(connect_transaction.get().wrapping_add(1).max(1));
    }
    let request = encode_announce_request(AnnounceRequest {
        connection_id,
        transaction_id: announce_transaction,
        info_hash,
        peer_id: CLIENT_PEER_ID,
        downloaded: 0,
        left: UNKNOWN_MAGNET_LEFT,
        uploaded: 0,
        event: AnnounceEvent::Started,
        ip_address: 0,
        key,
        num_want: MAX_COMPACT_PEERS as i32,
        port: 0,
    });
    send_udp_tracker_packet(&socket, &request, "send UDP tracker announce").await?;
    let family = match address {
        SocketAddr::V4(_) => TrackerAddressFamily::Ipv4,
        SocketAddr::V6(_) => TrackerAddressFamily::Ipv6,
    };
    receive_udp_tracker_response(&socket, "announce response", |bytes| {
        parse_announce_response(bytes, announce_transaction, family)
    })
    .await
}

async fn send_udp_tracker_packet(
    socket: &UdpSocket,
    packet: &[u8],
    operation: &'static str,
) -> Result<(), DownloadError> {
    let sent = socket.send(packet);
    let sent = timeout(UDP_TRACKER_RESPONSE_TIMEOUT, sent)
        .await
        .map_err(|_| DownloadError::NetworkTimedOut {
            operation,
            timeout: UDP_TRACKER_RESPONSE_TIMEOUT,
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

async fn receive_udp_tracker_response<T>(
    socket: &UdpSocket,
    operation: &'static str,
    parse: impl Fn(&[u8]) -> Result<T, UdpTrackerError>,
) -> Result<T, DownloadError> {
    let deadline = TokioInstant::now() + UDP_TRACKER_RESPONSE_TIMEOUT;
    let mut buffer = [0; UDP_TRACKER_RECEIVE_LENGTH];
    loop {
        let received = timeout_at(deadline, socket.recv(&mut buffer))
            .await
            .map_err(|_| DownloadError::UdpTrackerTimedOut {
                operation,
                timeout: UDP_TRACKER_RESPONSE_TIMEOUT,
            })?
            .map_err(|source| DownloadError::Io {
                operation: "receive UDP tracker response",
                source,
            })?;
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
    let mut peers = PeerSession::from_magnet(&magnet, config.network).await?;
    run_magnet_download_with_peers(config, control, magnet, &mut peers).await
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
        skip_files: config.skip_files,
        materialize_files: config.materialize_files,
    };
    run_content_download(content_config, metainfo, control, None, peers, None).await
}

async fn run_magnet_metadata(
    magnet: String,
    network: NetworkConfig,
) -> Result<Vec<u8>, DownloadError> {
    let magnet = Magnet::parse(&magnet).map_err(DownloadError::Magnet)?;
    let mut peers = PeerSession::from_magnet(&magnet, network).await?;
    let (raw_info, _) = peers.acquire_metadata(magnet.info_hash).await?;
    peers.close_current(None)?;
    Ok(raw_info)
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
        let mut peers = PeerSession::from_magnet(&magnet, config.network).await?;
        let content_config = ContentDownloadConfig {
            output_path: config.output_path,
            max_buffered_payload_bytes: config.max_buffered_payload_bytes,
            skip_files: config.skip_files,
            materialize_files: Vec::new(),
        };
        return run_content_download(
            content_config,
            metainfo,
            control,
            descriptors,
            &mut peers,
            Some(resume),
        )
        .await;
    }

    if descriptors.is_some() {
        return Err(DownloadError::Checkpoint(
            "descriptor storage requires verified metadata".to_owned(),
        ));
    }
    let mut peers = PeerSession::from_magnet(&magnet, config.network).await?;
    let (raw_info, metainfo) = peers.acquire_metadata(magnet.info_hash).await?;
    if let Err(message) = checkpoints.metadata_verified(&raw_info) {
        peers.close_current(None)?;
        return Err(DownloadError::Checkpoint(message));
    }
    let content_config = ContentDownloadConfig {
        output_path: config.output_path,
        max_buffered_payload_bytes: config.max_buffered_payload_bytes,
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
    let mut queued_messages = peer_state.validated_messages(&metainfo)?;
    queued_messages.append(&mut peer.queued_messages);
    peer.queued_messages = queued_messages;
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
            } else if metainfo.piece_count() != 1
                || !config.skip_files.is_empty()
                || !config.materialize_files.is_empty()
            {
                Err(DownloadError::Metainfo(MetainfoError::Unsupported(
                    "multi-piece single-file or selected single-file diagnostic execution",
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
    let piece_length = u32::try_from(metainfo.total_length)
        .map_err(|_| DownloadError::Metainfo(MetainfoError::InvalidField("info.length")))?;
    let mut download = OnePieceDownload::new(
        0,
        piece_length,
        metainfo.piece_hashes[0],
        config.max_buffered_payload_bytes,
    )
    .map_err(DownloadError::Piece)?;
    control.emit(DownloadActivityEvent::PieceStarted {
        piece_index: 0,
        piece_length,
    });
    let mut storage = StagingFile::create(config.output_path.clone(), metainfo.total_length)
        .await
        .map_err(DownloadError::Storage)?;

    peers.ensure_content_connection(metainfo.info_hash).await?;
    let peer = peers.connection_mut()?;
    loop {
        let message = match next_peer_message(peer).await {
            Ok(message) => message,
            Err(error) => {
                download.cancel_pending();
                control.observe(&download);
                return Err(error);
            }
        };
        if matches!(message, PeerMessage::Extended { .. }) {
            continue;
        }
        let actions = download.on_message(message).map_err(DownloadError::Piece)?;
        control.observe(&download);
        if let Some(piece) =
            process_actions(peer, &mut storage, &mut download, actions, &control).await?
        {
            let budget = download.payload_budget();
            let block_count = download.block_count();
            storage.finalize().await.map_err(DownloadError::Storage)?;
            return Ok(DownloadReport {
                info_hash: metainfo.info_hash,
                piece_hash: piece.hash,
                bytes_written: piece.length as usize,
                block_count,
                payload_limit: budget.limit,
                payload_high_water: budget.high_water,
                verification_buffer: VERIFICATION_CHUNK_LENGTH,
                piece_count: 1,
                verified_piece_count: 1,
                skipped_piece_count: 0,
                selected_file_bytes: metainfo.total_length,
                skipped_file_bytes: 0,
                padding_bytes: 0,
                selected_written_bytes: piece.length as usize,
                part_written_bytes: 0,
                materialized_bytes: 0,
                part_slots_before_materialization: 0,
                part_slots_after_materialization: 0,
                part_reopened: false,
                part_path: None,
                prepared_files: Vec::new(),
            });
        }
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
    if !plans.is_empty() {
        peers.ensure_content_connection(metainfo.info_hash).await?;
    }
    let mut availability = vec![false; layout.piece_count()];
    let mut availability_known = false;
    let mut peer_choking = true;
    let mut total_blocks = 0;
    let mut total_bytes = 0;
    let mut selected_written_bytes = 0;
    let mut part_written_bytes = 0;
    let mut payload_high_water = 0;
    let mut last_piece = None;

    for (piece_index, ranges) in plans {
        let peer = peers.connection_mut()?;
        let piece_index_usize = usize::try_from(piece_index)
            .map_err(|_| DownloadError::Layout(LayoutError::ArithmeticOverflow))?;
        let piece_length = layout
            .piece_length_at(piece_index)
            .map_err(DownloadError::Layout)?;
        let mut download = OnePieceDownload::new_for_torrent(
            piece_index,
            piece_length,
            metainfo.piece_hashes[piece_index_usize],
            config.max_buffered_payload_bytes,
            layout.piece_count(),
            &ranges,
        )
        .map_err(DownloadError::Piece)?;
        control.emit(DownloadActivityEvent::PieceStarted {
            piece_index,
            piece_length,
        });
        total_blocks += download.block_count();
        total_bytes += ranges
            .iter()
            .map(|range| range.length as usize)
            .sum::<usize>();

        let mut initial_actions = Vec::new();
        if availability_known {
            initial_actions.extend(
                download
                    .on_message(PeerMessage::Bitfield(encode_availability(&availability)))
                    .map_err(DownloadError::Piece)?,
            );
        }
        if !peer_choking {
            initial_actions.extend(
                download
                    .on_message(PeerMessage::Unchoke)
                    .map_err(DownloadError::Piece)?,
            );
        }
        control.observe(&download);
        if let Some(piece) = process_selective_actions(
            peer,
            &mut storage,
            &mut download,
            initial_actions,
            &mut selected_written_bytes,
            &mut part_written_bytes,
            &control,
        )
        .await?
        {
            if let Some(resume) = &resume {
                storage
                    .sync_piece(piece.index)
                    .await
                    .map_err(DownloadError::SelectiveStorage)?;
                storage
                    .record_verified(piece.index as usize)
                    .map_err(DownloadError::SelectiveStorage)?;
                resume
                    .checkpoints
                    .piece_durable(piece.index as usize)
                    .map_err(DownloadError::Checkpoint)?;
            } else {
                storage
                    .record_verified(piece.index as usize)
                    .map_err(DownloadError::SelectiveStorage)?;
            }
            payload_high_water = payload_high_water.max(download.payload_budget().high_water);
            last_piece = Some(piece);
            continue;
        }

        loop {
            let message = match next_peer_message(peer).await {
                Ok(message) => message,
                Err(error) => {
                    download.cancel_pending();
                    control.observe(&download);
                    return Err(error);
                }
            };
            if matches!(message, PeerMessage::Extended { .. }) {
                continue;
            }
            let availability_update = availability_update(&message);
            let actions = download.on_message(message).map_err(DownloadError::Piece)?;
            control.observe(&download);
            match availability_update {
                AvailabilityUpdate::None => {}
                AvailabilityUpdate::Choke(choking) => peer_choking = choking,
                AvailabilityUpdate::Have(index) => {
                    availability[index as usize] = true;
                    availability_known = true;
                }
                AvailabilityUpdate::Bitfield(bitfield) => {
                    decode_availability(&bitfield, &mut availability);
                    availability_known = true;
                }
            }

            if let Some(piece) = process_selective_actions(
                peer,
                &mut storage,
                &mut download,
                actions,
                &mut selected_written_bytes,
                &mut part_written_bytes,
                &control,
            )
            .await?
            {
                if let Some(resume) = &resume {
                    storage
                        .sync_piece(piece.index)
                        .await
                        .map_err(DownloadError::SelectiveStorage)?;
                    storage
                        .record_verified(piece.index as usize)
                        .map_err(DownloadError::SelectiveStorage)?;
                    resume
                        .checkpoints
                        .piece_durable(piece.index as usize)
                        .map_err(DownloadError::Checkpoint)?;
                } else {
                    storage
                        .record_verified(piece.index as usize)
                        .map_err(DownloadError::SelectiveStorage)?;
                }
                payload_high_water = payload_high_water.max(download.payload_budget().high_water);
                last_piece = Some(piece);
                break;
            }
        }
    }

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

async fn connect_peer(
    attempt: DialAttempt,
    info_hash: [u8; 20],
    advertise_extensions: bool,
    network: NetworkConfig,
) -> Result<(PeerConnection, Handshake), DownloadError> {
    let address = attempt.endpoint().address();
    if !network.policy.allows(address) {
        return Err(DownloadError::NetworkPolicyDenied {
            address,
            policy: network.policy,
        });
    }
    let mut peer = timeout(network.peer_connect_timeout, TcpStream::connect(address))
        .await
        .map_err(|_| DownloadError::PeerTimedOut {
            operation: "connect",
            timeout: network.peer_connect_timeout,
        })?
        .map_err(|source| DownloadError::Io {
            operation: "connect to peer",
            source,
        })?;
    let handshake = if advertise_extensions {
        let mut reserved = [0; 8];
        reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
        encode_handshake_with_reserved(info_hash, CLIENT_PEER_ID, reserved)
    } else {
        encode_handshake(info_hash, CLIENT_PEER_ID)
    };
    timeout(network.peer_io_timeout, peer.write_all(&handshake))
        .await
        .map_err(|_| DownloadError::PeerTimedOut {
            operation: "handshake write",
            timeout: network.peer_io_timeout,
        })?
        .map_err(|source| DownloadError::Io {
            operation: "send peer handshake",
            source,
        })?;

    let mut handshake = [0_u8; HANDSHAKE_LENGTH];
    timeout(network.peer_io_timeout, peer.read_exact(&mut handshake))
        .await
        .map_err(|_| DownloadError::PeerTimedOut {
            operation: "handshake read",
            timeout: network.peer_io_timeout,
        })?
        .map_err(|source| DownloadError::Io {
            operation: "read peer handshake",
            source,
        })?;
    let handshake = decode_handshake(&handshake, info_hash).map_err(DownloadError::Handshake)?;
    Ok((
        PeerConnection {
            attempt,
            stream: peer,
            decoder: FrameDecoder::new(),
            queued_messages: VecDeque::new(),
            io_timeout: network.peer_io_timeout,
        },
        handshake,
    ))
}

async fn next_peer_message(peer: &mut PeerConnection) -> Result<PeerMessage, DownloadError> {
    let deadline = TokioInstant::now() + peer.io_timeout;
    while peer.queued_messages.is_empty() {
        let mut network_buffer = [0_u8; NETWORK_READ_LENGTH];
        let read = timeout_at(deadline, peer.stream.read(&mut network_buffer))
            .await
            .map_err(|_| DownloadError::PeerTimedOut {
                operation: "message read",
                timeout: peer.io_timeout,
            })?
            .map_err(|source| DownloadError::Io {
                operation: "read peer message",
                source,
            })?;
        if read == 0 {
            return Err(DownloadError::PeerClosed);
        }
        peer.queued_messages.extend(
            peer.decoder
                .push(&network_buffer[..read])
                .map_err(DownloadError::Frame)?,
        );
    }
    Ok(peer
        .queued_messages
        .pop_front()
        .expect("peer message queue is nonempty after receive loop"))
}

#[derive(Debug)]
enum AvailabilityUpdate {
    None,
    Choke(bool),
    Have(u32),
    Bitfield(Vec<u8>),
}

fn availability_update(message: &PeerMessage) -> AvailabilityUpdate {
    match message {
        PeerMessage::Choke => AvailabilityUpdate::Choke(true),
        PeerMessage::Unchoke => AvailabilityUpdate::Choke(false),
        PeerMessage::Have(index) => AvailabilityUpdate::Have(*index),
        PeerMessage::Bitfield(bitfield) => AvailabilityUpdate::Bitfield(bitfield.clone()),
        _ => AvailabilityUpdate::None,
    }
}

fn encode_availability(availability: &[bool]) -> Vec<u8> {
    let mut bitfield = vec![0_u8; availability.len().div_ceil(8)];
    for (index, available) in availability.iter().enumerate() {
        if *available {
            bitfield[index / 8] |= 1 << (7 - index % 8);
        }
    }
    bitfield
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
    let frame = encode_message(message).map_err(DownloadError::Frame)?;
    timeout(peer.io_timeout, peer.stream.write_all(&frame))
        .await
        .map_err(|_| DownloadError::PeerTimedOut {
            operation: "message write",
            timeout: peer.io_timeout,
        })?
        .map_err(|source| DownloadError::Io {
            operation: "send peer message",
            source,
        })
}

async fn process_actions(
    peer: &mut PeerConnection,
    storage: &mut StagingFile,
    download: &mut OnePieceDownload,
    actions: Vec<DownloadAction>,
    control: &DownloadControl,
) -> Result<Option<VerifiedPiece>, DownloadError> {
    let mut pending = VecDeque::from(actions);
    while let Some(action) = pending.pop_front() {
        match action {
            DownloadAction::SendInterested => {
                send_message(peer, &PeerMessage::Interested).await?;
            }
            DownloadAction::Request(request) => {
                send_message(peer, &PeerMessage::Request(request)).await?;
                control.record_requested(request.length as usize);
                control.emit(DownloadActivityEvent::BlockRequested {
                    piece_index: request.index,
                    begin: request.begin,
                    length: request.length,
                });
            }
            DownloadAction::StoreBlock(block) => {
                let index = block.index;
                let begin = block.begin;
                let length = block.bytes.len();
                control.record_received(length);
                control.emit(DownloadActivityEvent::BlockReceived {
                    piece_index: index,
                    begin,
                    length: u32::try_from(length).expect("peer block length is bounded by u32"),
                });
                control.wait_before_storage().await;
                if let Err(error) = storage.write_block(u64::from(begin), block.bytes).await {
                    download
                        .on_block_write_failed(index, begin)
                        .map_err(DownloadError::Piece)?;
                    control.observe(download);
                    return Err(DownloadError::Storage(error));
                }
                control.record_stored(length);
                control.emit(DownloadActivityEvent::BlockStored {
                    piece_index: index,
                    begin,
                    length: u32::try_from(length).expect("peer block length is bounded by u32"),
                });
                pending.extend(
                    download
                        .on_block_stored(index, begin)
                        .map_err(DownloadError::Piece)?,
                );
                control.observe(download);
            }
            DownloadAction::VerifyPiece { index, length } => {
                let actual_hash = storage
                    .hash_piece(0, length)
                    .await
                    .map_err(DownloadError::Storage)?;
                pending.push_back(
                    download
                        .finish_verification(index, actual_hash)
                        .map_err(DownloadError::Piece)?,
                );
            }
            DownloadAction::Verified(piece) => {
                control.emit(DownloadActivityEvent::PieceVerified {
                    piece_index: piece.index,
                });
                return Ok(Some(piece));
            }
        }
    }
    Ok(None)
}

async fn process_selective_actions(
    peer: &mut PeerConnection,
    storage: &mut SelectiveStorage,
    download: &mut OnePieceDownload,
    actions: Vec<DownloadAction>,
    selected_written_bytes: &mut usize,
    part_written_bytes: &mut usize,
    control: &DownloadControl,
) -> Result<Option<VerifiedPiece>, DownloadError> {
    let mut pending = VecDeque::from(actions);
    while let Some(action) = pending.pop_front() {
        match action {
            DownloadAction::SendInterested => {
                send_message(peer, &PeerMessage::Interested).await?;
            }
            DownloadAction::Request(request) => {
                send_message(peer, &PeerMessage::Request(request)).await?;
                control.record_requested(request.length as usize);
                control.emit(DownloadActivityEvent::BlockRequested {
                    piece_index: request.index,
                    begin: request.begin,
                    length: request.length,
                });
            }
            DownloadAction::StoreBlock(block) => {
                let index = block.index;
                let begin = block.begin;
                let length = block.bytes.len();
                control.record_received(length);
                control.emit(DownloadActivityEvent::BlockReceived {
                    piece_index: index,
                    begin,
                    length: u32::try_from(length).expect("peer block length is bounded by u32"),
                });
                control.wait_before_storage().await;
                let stats = match storage.write_block(index, begin, block.bytes).await {
                    Ok(stats) => stats,
                    Err(error) => {
                        download
                            .on_block_write_failed(index, begin)
                            .map_err(DownloadError::Piece)?;
                        control.observe(download);
                        return Err(DownloadError::SelectiveStorage(error));
                    }
                };
                control.record_stored(length);
                control.emit(DownloadActivityEvent::BlockStored {
                    piece_index: index,
                    begin,
                    length: u32::try_from(length).expect("peer block length is bounded by u32"),
                });
                *selected_written_bytes += stats.wanted_bytes;
                *part_written_bytes += stats.skipped_bytes;
                pending.extend(
                    download
                        .on_block_stored(index, begin)
                        .map_err(DownloadError::Piece)?,
                );
                control.observe(download);
            }
            DownloadAction::VerifyPiece { index, .. } => {
                let actual_hash = storage
                    .hash_piece(index)
                    .await
                    .map_err(DownloadError::SelectiveStorage)?;
                pending.push_back(
                    download
                        .finish_verification(index, actual_hash)
                        .map_err(DownloadError::Piece)?,
                );
            }
            DownloadAction::Verified(piece) => {
                control.emit(DownloadActivityEvent::PieceVerified {
                    piece_index: piece.index,
                });
                return Ok(Some(piece));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::net::SocketAddr;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use rstorrent_protocol::magnet::Magnet;
    use rstorrent_protocol::metadata::{
        MetadataMessage, encode_extension_handshake, encode_metadata_data, parse_metadata_message,
    };
    use rstorrent_protocol::metainfo::Metainfo;
    use rstorrent_protocol::peer_wire::{
        EXTENSION_PROTOCOL_RESERVED_BIT, EXTENSION_PROTOCOL_RESERVED_INDEX, FrameDecoder,
        HANDSHAKE_LENGTH, PeerMessage, decode_handshake, encode_handshake_with_reserved,
        encode_message,
    };
    use rstorrent_protocol::piece::MIN_PAYLOAD_ALLOWANCE;
    use sha1::{Digest, Sha1};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream, UdpSocket};
    use tokio::time::timeout;

    use super::{
        CLIENT_PEER_ID, DownloadConfig, DownloadControl, DownloadError, MagnetDownloadConfig,
        PeerConnection, PeerSession, download_magnet, download_magnet_metadata_with_control,
        download_magnet_with_control, download_verified_piece,
        download_verified_piece_with_control, next_peer_message, run_magnet_download_with_peers,
        send_message,
    };
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
            PeerConnection {
                attempt: test_dial_attempt(),
                stream: client,
                decoder: FrameDecoder::new(),
                queued_messages: VecDeque::new(),
                io_timeout,
            },
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

    fn single_file_info(payload: &[u8]) -> Vec<u8> {
        let piece_hash: [u8; 20] = Sha1::digest(payload).into();
        let mut info = format!(
            "d6:lengthi{}e4:name1:x12:piece lengthi16384e6:pieces20:",
            payload.len()
        )
        .into_bytes();
        info.extend_from_slice(&piece_hash);
        info.push(b'e');
        info
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
        let mut peer = PeerConnection {
            attempt: test_dial_attempt(),
            stream,
            decoder: FrameDecoder::new(),
            queued_messages: VecDeque::new(),
            io_timeout: Duration::from_secs(1),
        };

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

    async fn serve_one_shot_udp_tracker(
        socket: UdpSocket,
        info_hash: [u8; 20],
        unreachable: SocketAddr,
        reachable: SocketAddr,
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
        let mut peers = PeerSession::from_magnet(&parsed, network)
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
            },
            DownloadControl::new(),
            parsed,
            &mut peers,
        )
        .await
        .expect("tracker-discovered magnet download");

        assert_eq!(peers.next_udp_tracker, 2);
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
        rejecting_tracker_task
            .await
            .expect("rejecting tracker task");
        tracker_task.await.expect("scripted tracker task");
        peer_task.await.expect("scripted peer task");
        let _ = tokio::fs::remove_file(output_path).await;
    }

    async fn assert_tracker_wait_terminates(cancel: bool) {
        let tracker = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind silent tracker");
        let tracker_address = tracker.local_addr().expect("tracker address");
        let output_path = test_path(if cancel {
            "cancelled-tracker-output.bin"
        } else {
            "timed-out-tracker-output.bin"
        });
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
            },
            task_control,
        ));

        let mut packet = [0; 32];
        let (length, client) = timeout(Duration::from_secs(1), tracker.recv_from(&mut packet))
            .await
            .expect("tracker connect deadline")
            .expect("receive tracker connect");
        assert_eq!(length, 16);
        if cancel {
            control.cancel();
        }
        let result = task.await.expect("join tracker-wait download");
        if cancel {
            assert!(matches!(result, Err(DownloadError::Cancelled)));
        } else {
            assert!(matches!(
                result,
                Err(DownloadError::UdpTrackerTimedOut { .. })
            ));
        }
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

    #[tokio::test]
    async fn tracker_wait_honors_timeout_and_cancellation_without_socket_leaks() {
        assert_tracker_wait_terminates(false).await;
        assert_tracker_wait_terminates(true).await;
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
        let mut peers = PeerSession::from_magnet(&parsed, network)
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
    async fn public_magnet_entry_uses_peer_registry_path() {
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
        let mut tracker_packet = [0; 1];
        assert!(
            timeout(
                Duration::from_millis(25),
                unused_tracker.recv_from(&mut tracker_packet)
            )
            .await
            .is_err(),
            "eligible explicit hints should not trigger tracker discovery"
        );
        let _ = tokio::fs::remove_file(output_path).await;
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
