use std::collections::{BTreeSet, VecDeque};
use std::error::Error;
use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rstorrent_protocol::bencode::MAX_BENCODE_INPUT_LENGTH;
use rstorrent_protocol::magnet::{Magnet, MagnetError, PeerHint};
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
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, lookup_host};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::selective_storage::{
    DescriptorStorage, PreparedFileHash, ResumedStorage, SelectiveStorage, SelectiveStorageError,
    remove_selective_part_if_present, remove_selective_staging_if_present,
};
use crate::storage::{
    StagingFile, StorageError, VERIFICATION_CHUNK_LENGTH, remove_staging_if_present, staging_path,
};

const DIAGNOSTIC_PEER_ID: [u8; 20] = *b"-RS0001-000000000000";
const NETWORK_READ_LENGTH: usize = 16 * 1024;
const SAFE_CANCEL_REQUESTED: usize = 1 << (usize::BITS - 1);
const SAFE_CANCEL_CRITICAL_MASK: usize = SAFE_CANCEL_REQUESTED - 1;

#[derive(Debug)]
struct ConnectedPeer {
    stream: TcpStream,
    decoder: FrameDecoder,
    queued_messages: VecDeque<PeerMessage>,
}

#[derive(Clone, Debug)]
pub struct DownloadConfig {
    pub metainfo_path: PathBuf,
    pub peer: SocketAddr,
    pub output_path: PathBuf,
    pub timeout: Duration,
    pub max_buffered_payload_bytes: usize,
    pub skip_files: Vec<usize>,
    pub materialize_files: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct MagnetDownloadConfig {
    pub magnet: String,
    pub output_path: PathBuf,
    pub timeout: Duration,
    pub max_buffered_payload_bytes: usize,
    pub skip_files: Vec<usize>,
    pub materialize_files: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct ResumableMagnetDownloadConfig {
    pub magnet: String,
    pub output_path: PathBuf,
    pub timeout: Duration,
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
    peer: Option<SocketAddr>,
    peer_hints: Vec<PeerHint>,
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
    NonLoopbackPeer(SocketAddr),
    InvalidTimeout,
    MetainfoTooLarge {
        maximum: usize,
    },
    Io {
        operation: &'static str,
        source: io::Error,
    },
    Metainfo(MetainfoError),
    Magnet(MagnetError),
    Metadata(MetadataError),
    Handshake(HandshakeError),
    Frame(FrameError),
    Piece(PieceError),
    Layout(LayoutError),
    Storage(StorageError),
    SelectiveStorage(SelectiveStorageError),
    PeerClosed,
    NoUsablePeerHint,
    ExtensionProtocolUnsupported,
    MetadataExtensionDisabled,
    InvalidPremetadataState(&'static str),
    Checkpoint(String),
    Cancelled,
    TimedOut {
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
            Self::NonLoopbackPeer(peer) => {
                write!(
                    formatter,
                    "diagnostic peer {peer} is not a loopback address"
                )
            }
            Self::InvalidTimeout => write!(formatter, "diagnostic timeout must be nonzero"),
            Self::MetainfoTooLarge { maximum } => {
                write!(formatter, "metainfo exceeds input limit {maximum}")
            }
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::Metainfo(error) => write!(formatter, "metainfo: {error}"),
            Self::Magnet(error) => write!(formatter, "magnet: {error}"),
            Self::Metadata(error) => write!(formatter, "metadata: {error}"),
            Self::Handshake(error) => write!(formatter, "peer handshake: {error}"),
            Self::Frame(error) => write!(formatter, "peer frame: {error}"),
            Self::Piece(error) => write!(formatter, "piece state: {error}"),
            Self::Layout(error) => write!(formatter, "torrent layout: {error}"),
            Self::Storage(error) => write!(formatter, "storage: {error}"),
            Self::SelectiveStorage(error) => write!(formatter, "selective storage: {error}"),
            Self::PeerClosed => write!(formatter, "peer closed before piece verification"),
            Self::NoUsablePeerHint => {
                write!(formatter, "magnet has no reachable loopback peer hint")
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
            Self::TimedOut { timeout } => {
                write!(
                    formatter,
                    "diagnostic timed out after {}s",
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

pub async fn download_magnet_with_peer_hint(
    config: MagnetDownloadConfig,
) -> Result<DownloadReport, DownloadError> {
    download_magnet_with_peer_hint_with_control(config, DownloadControl::new()).await
}

pub async fn resume_magnet_with_peer_hint(
    config: ResumableMagnetDownloadConfig,
    checkpoints: Arc<dyn DownloadCheckpointSink>,
) -> Result<DownloadReport, DownloadError> {
    resume_magnet_with_peer_hint_with_control(config, checkpoints, DownloadControl::new()).await
}

pub async fn resume_magnet_with_peer_hint_with_control(
    config: ResumableMagnetDownloadConfig,
    checkpoints: Arc<dyn DownloadCheckpointSink>,
    control: DownloadControl,
) -> Result<DownloadReport, DownloadError> {
    resume_magnet_with_peer_hint_owned(config, checkpoints, control, None).await
}

pub async fn resume_magnet_to_descriptors_with_peer_hint_with_control(
    config: ResumableMagnetDownloadConfig,
    descriptors: DescriptorStorage,
    checkpoints: Arc<dyn DownloadCheckpointSink>,
    control: DownloadControl,
) -> Result<DownloadReport, DownloadError> {
    if config.verified_info.is_none() {
        return Err(DownloadError::Checkpoint(
            "descriptor storage requires verified metadata".to_owned(),
        ));
    }
    resume_magnet_with_peer_hint_owned(config, checkpoints, control, Some(descriptors)).await
}

async fn resume_magnet_with_peer_hint_owned(
    config: ResumableMagnetDownloadConfig,
    checkpoints: Arc<dyn DownloadCheckpointSink>,
    control: DownloadControl,
    descriptors: Option<DescriptorStorage>,
) -> Result<DownloadReport, DownloadError> {
    validate_resumable_magnet_download_config(&config)?;
    let configured_timeout = config.timeout;
    let result = tokio::select! {
        biased;
        _ = control.inner.cancellation.cancelled() => Err(DownloadError::Cancelled),
        result = timeout(
            configured_timeout,
            run_resumable_magnet_download(
                config,
                checkpoints,
                control.clone(),
                descriptors,
            ),
        ) => result
            .map_err(|_| DownloadError::TimedOut {
                timeout: configured_timeout,
            })
            .and_then(|result| result),
    };
    control.clear_buffered_payload();
    result
}

pub async fn download_magnet_metadata_with_peer_hint_with_control(
    magnet: String,
    configured_timeout: Duration,
    control: DownloadControl,
) -> Result<Vec<u8>, DownloadError> {
    if configured_timeout.is_zero() {
        return Err(DownloadError::InvalidTimeout);
    }
    let result = tokio::select! {
        biased;
        _ = control.inner.cancellation.cancelled() => Err(DownloadError::Cancelled),
        result = timeout(configured_timeout, run_magnet_metadata(magnet)) => result
            .map_err(|_| DownloadError::TimedOut {
                timeout: configured_timeout,
            })
            .and_then(|result| result),
    };
    control.clear_buffered_payload();
    result
}

pub async fn download_magnet_with_peer_hint_with_control(
    config: MagnetDownloadConfig,
    control: DownloadControl,
) -> Result<DownloadReport, DownloadError> {
    validate_magnet_download_config(&config)?;
    let configured_timeout = config.timeout;
    let output_path = config.output_path.clone();
    let staging = staging_path(&output_path).map_err(DownloadError::Storage)?;
    let result = tokio::select! {
        biased;
        _ = control.inner.cancellation.cancelled() => Err(DownloadError::Cancelled),
        result = timeout(
            configured_timeout,
            run_magnet_download(config, control.clone()),
        ) => result
            .map_err(|_| DownloadError::TimedOut {
                timeout: configured_timeout,
            })
            .and_then(|result| result),
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

    let configured_timeout = config.timeout;
    let staging = staging_path(&config.output_path).map_err(DownloadError::Storage)?;
    let output_path = config.output_path.clone();
    let result = tokio::select! {
        biased;
        _ = control.inner.cancellation.cancelled() => Err(DownloadError::Cancelled),
        result = timeout(
            configured_timeout,
            run_download(config, control.clone(), None),
        ) => result
            .map_err(|_| DownloadError::TimedOut {
                timeout: configured_timeout,
            })
            .and_then(|result| result),
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
    let configured_timeout = config.timeout;
    let result = tokio::select! {
        biased;
        _ = control.inner.cancellation.cancelled() => Err(DownloadError::Cancelled),
        result = timeout(
            configured_timeout,
            run_download(config, control.clone(), Some(descriptors)),
        ) => result
            .map_err(|_| DownloadError::TimedOut {
                timeout: configured_timeout,
            })
            .and_then(|result| result),
    };
    control.clear_buffered_payload();
    result
}

fn validate_download_config(config: &DownloadConfig) -> Result<(), DownloadError> {
    if !config.peer.ip().is_loopback() {
        return Err(DownloadError::NonLoopbackPeer(config.peer));
    }
    if config.timeout.is_zero() {
        return Err(DownloadError::InvalidTimeout);
    }
    Ok(())
}

fn validate_magnet_download_config(config: &MagnetDownloadConfig) -> Result<(), DownloadError> {
    if config.timeout.is_zero() {
        return Err(DownloadError::InvalidTimeout);
    }
    Ok(())
}

fn validate_resumable_magnet_download_config(
    config: &ResumableMagnetDownloadConfig,
) -> Result<(), DownloadError> {
    if config.timeout.is_zero() {
        return Err(DownloadError::InvalidTimeout);
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

async fn run_magnet_download(
    config: MagnetDownloadConfig,
    control: DownloadControl,
) -> Result<DownloadReport, DownloadError> {
    let magnet = Magnet::parse(&config.magnet).map_err(DownloadError::Magnet)?;
    if magnet.peer_hints.is_empty() {
        return Err(DownloadError::NoUsablePeerHint);
    }

    let mut last_error = None;
    for hint in &magnet.peer_hints {
        let addresses = match lookup_host((hint.host.as_str(), hint.port)).await {
            Ok(addresses) => addresses,
            Err(source) => {
                last_error = Some(DownloadError::Io {
                    operation: "resolve magnet peer hint",
                    source,
                });
                continue;
            }
        };
        for address in addresses {
            if !address.ip().is_loopback() {
                continue;
            }
            match acquire_metadata(address, magnet.info_hash).await {
                Ok((_raw_info, metainfo, connection)) => {
                    let content_config = ContentDownloadConfig {
                        peer: Some(address),
                        peer_hints: Vec::new(),
                        output_path: config.output_path,
                        max_buffered_payload_bytes: config.max_buffered_payload_bytes,
                        skip_files: config.skip_files,
                        materialize_files: config.materialize_files,
                    };
                    return run_content_download(
                        content_config,
                        metainfo,
                        control,
                        None,
                        Some(connection),
                        None,
                    )
                    .await;
                }
                Err(error) => last_error = Some(error),
            }
        }
    }
    Err(last_error.unwrap_or(DownloadError::NoUsablePeerHint))
}

async fn run_magnet_metadata(magnet: String) -> Result<Vec<u8>, DownloadError> {
    let magnet = Magnet::parse(&magnet).map_err(DownloadError::Magnet)?;
    if magnet.peer_hints.is_empty() {
        return Err(DownloadError::NoUsablePeerHint);
    }

    let mut last_error = None;
    for hint in &magnet.peer_hints {
        let addresses = match lookup_host((hint.host.as_str(), hint.port)).await {
            Ok(addresses) => addresses,
            Err(source) => {
                last_error = Some(DownloadError::Io {
                    operation: "resolve magnet peer hint",
                    source,
                });
                continue;
            }
        };
        for address in addresses {
            if !address.ip().is_loopback() {
                continue;
            }
            match acquire_metadata(address, magnet.info_hash).await {
                Ok((raw_info, _, _)) => return Ok(raw_info),
                Err(error) => last_error = Some(error),
            }
        }
    }
    Err(last_error.unwrap_or(DownloadError::NoUsablePeerHint))
}

#[derive(Clone)]
struct ResumeContext {
    verified_pieces: Vec<bool>,
    checkpoints: Arc<dyn DownloadCheckpointSink>,
}

async fn run_resumable_magnet_download(
    config: ResumableMagnetDownloadConfig,
    checkpoints: Arc<dyn DownloadCheckpointSink>,
    control: DownloadControl,
    descriptors: Option<DescriptorStorage>,
) -> Result<DownloadReport, DownloadError> {
    let magnet = Magnet::parse(&config.magnet).map_err(DownloadError::Magnet)?;
    if magnet.peer_hints.is_empty() {
        return Err(DownloadError::NoUsablePeerHint);
    }
    let resume = ResumeContext {
        verified_pieces: config.verified_pieces,
        checkpoints: checkpoints.clone(),
    };

    if let Some(raw_info) = config.verified_info {
        let metainfo = Metainfo::from_info_bytes(&raw_info).map_err(DownloadError::Metainfo)?;
        if metainfo.info_hash != magnet.info_hash {
            return Err(DownloadError::Checkpoint(
                "stored metadata does not match the magnet identity".to_owned(),
            ));
        }
        let content_config = ContentDownloadConfig {
            peer: None,
            peer_hints: magnet.peer_hints,
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
            None,
            Some(resume),
        )
        .await;
    }

    if descriptors.is_some() {
        return Err(DownloadError::Checkpoint(
            "descriptor storage requires verified metadata".to_owned(),
        ));
    }
    let mut last_error = None;
    for hint in &magnet.peer_hints {
        let addresses = match lookup_host((hint.host.as_str(), hint.port)).await {
            Ok(addresses) => addresses,
            Err(source) => {
                last_error = Some(DownloadError::Io {
                    operation: "resolve magnet peer hint",
                    source,
                });
                continue;
            }
        };
        for address in addresses {
            if !address.ip().is_loopback() {
                continue;
            }
            match acquire_metadata(address, magnet.info_hash).await {
                Ok((raw_info, metainfo, connection)) => {
                    checkpoints
                        .metadata_verified(&raw_info)
                        .map_err(DownloadError::Checkpoint)?;
                    let content_config = ContentDownloadConfig {
                        peer: Some(address),
                        peer_hints: Vec::new(),
                        output_path: config.output_path,
                        max_buffered_payload_bytes: config.max_buffered_payload_bytes,
                        skip_files: config.skip_files,
                        materialize_files: Vec::new(),
                    };
                    return run_content_download(
                        content_config,
                        metainfo,
                        control,
                        None,
                        Some(connection),
                        Some(resume),
                    )
                    .await;
                }
                Err(error) => last_error = Some(error),
            }
        }
    }
    Err(last_error.unwrap_or(DownloadError::NoUsablePeerHint))
}

async fn acquire_metadata(
    address: SocketAddr,
    info_hash: [u8; 20],
) -> Result<(Vec<u8>, Metainfo, ConnectedPeer), DownloadError> {
    let (mut peer, handshake) = connect_peer(address, info_hash, true).await?;
    if !handshake.supports_extensions() {
        return Err(DownloadError::ExtensionProtocolUnsupported);
    }
    send_message(
        &mut peer.stream,
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
        match next_peer_message(&mut peer).await? {
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
                    process_metadata_download_actions(&mut peer, remote_metadata_id, actions)
                        .await?
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
                            &mut peer.stream,
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
                    process_metadata_download_actions(&mut peer, remote_metadata_id, actions)
                        .await?
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
    peer: &mut ConnectedPeer,
    remote_metadata_id: Option<u8>,
    actions: Vec<MetadataDownloadAction>,
) -> Result<Option<Vec<u8>>, DownloadError> {
    for action in actions {
        match action {
            MetadataDownloadAction::Request(piece) => {
                let remote_id =
                    remote_metadata_id.ok_or(DownloadError::MetadataExtensionDisabled)?;
                send_message(
                    &mut peer.stream,
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
    mut peer: ConnectedPeer,
) -> Result<(Vec<u8>, Metainfo, ConnectedPeer), DownloadError> {
    let metainfo = Metainfo::from_info_bytes(&bytes).map_err(DownloadError::Metainfo)?;
    let mut queued_messages = peer_state.validated_messages(&metainfo)?;
    queued_messages.append(&mut peer.queued_messages);
    peer.queued_messages = queued_messages;
    Ok((bytes, metainfo, peer))
}

async fn run_download(
    config: DownloadConfig,
    control: DownloadControl,
    descriptors: Option<DescriptorStorage>,
) -> Result<DownloadReport, DownloadError> {
    let metainfo_bytes = read_bounded_metainfo(&config.metainfo_path).await?;
    let metainfo = Metainfo::from_bytes(&metainfo_bytes).map_err(DownloadError::Metainfo)?;
    let content_config = ContentDownloadConfig {
        peer: Some(config.peer),
        peer_hints: Vec::new(),
        output_path: config.output_path,
        max_buffered_payload_bytes: config.max_buffered_payload_bytes,
        skip_files: config.skip_files,
        materialize_files: config.materialize_files,
    };
    run_content_download(content_config, metainfo, control, descriptors, None, None).await
}

async fn run_content_download(
    config: ContentDownloadConfig,
    metainfo: Metainfo,
    control: DownloadControl,
    descriptors: Option<DescriptorStorage>,
    connection: Option<ConnectedPeer>,
    resume: Option<ResumeContext>,
) -> Result<DownloadReport, DownloadError> {
    match metainfo.mode {
        rstorrent_protocol::metainfo::MetainfoMode::SingleFile => {
            if resume.is_some() {
                return Err(DownloadError::Metainfo(MetainfoError::Unsupported(
                    "resumable execution currently requires multi-file metainfo",
                )));
            }
            if descriptors.is_some() {
                return Err(DownloadError::Metainfo(MetainfoError::Unsupported(
                    "descriptor diagnostic execution requires multi-file metainfo",
                )));
            }
            if metainfo.piece_count() != 1
                || !config.skip_files.is_empty()
                || !config.materialize_files.is_empty()
            {
                return Err(DownloadError::Metainfo(MetainfoError::Unsupported(
                    "multi-piece single-file or selected single-file diagnostic execution",
                )));
            }
            run_single_download(config, metainfo, control, connection).await
        }
        rstorrent_protocol::metainfo::MetainfoMode::MultiFile => {
            run_selective_download(config, metainfo, control, descriptors, connection, resume).await
        }
    }
}

async fn run_single_download(
    config: ContentDownloadConfig,
    metainfo: Metainfo,
    control: DownloadControl,
    connection: Option<ConnectedPeer>,
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

    let mut peer = match connection {
        Some(connection) => connection,
        None => {
            connect_peer(
                config.peer.ok_or(DownloadError::NoUsablePeerHint)?,
                metainfo.info_hash,
                false,
            )
            .await?
            .0
        }
    };
    loop {
        let message = match next_peer_message(&mut peer).await {
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
        if let Some(piece) = process_actions(
            &mut peer.stream,
            &mut storage,
            &mut download,
            actions,
            &control,
        )
        .await?
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
    connection: Option<ConnectedPeer>,
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
            let storage = SelectiveStorage::resume_with_descriptors(
                &metainfo,
                layout.clone(),
                selection,
                descriptors,
                verified_pieces.clone(),
            )
            .await
            .map_err(DownloadError::SelectiveStorage)?;
            resume
                .checkpoints
                .storage_prepared(ResumedStorage::Staging)
                .map_err(DownloadError::Checkpoint)?;
            (storage, Some(ResumedStorage::Staging))
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
    let mut peer = if plans.is_empty() {
        None
    } else {
        Some(match connection {
            Some(connection) => connection,
            None => {
                connect_content_peer(config.peer, &config.peer_hints, metainfo.info_hash).await?
            }
        })
    };
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
        let peer = peer
            .as_mut()
            .expect("missing pieces require a connected peer");
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
            &mut peer.stream,
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
                &mut peer.stream,
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

async fn connect_content_peer(
    address: Option<SocketAddr>,
    peer_hints: &[PeerHint],
    info_hash: [u8; 20],
) -> Result<ConnectedPeer, DownloadError> {
    if let Some(address) = address {
        return Ok(connect_peer(address, info_hash, false).await?.0);
    }
    let mut last_error = None;
    for hint in peer_hints {
        let addresses = match lookup_host((hint.host.as_str(), hint.port)).await {
            Ok(addresses) => addresses,
            Err(source) => {
                last_error = Some(DownloadError::Io {
                    operation: "resolve magnet peer hint",
                    source,
                });
                continue;
            }
        };
        for address in addresses {
            if !address.ip().is_loopback() {
                continue;
            }
            match connect_peer(address, info_hash, false).await {
                Ok((peer, _)) => return Ok(peer),
                Err(error) => last_error = Some(error),
            }
        }
    }
    Err(last_error.unwrap_or(DownloadError::NoUsablePeerHint))
}

async fn connect_peer(
    address: SocketAddr,
    info_hash: [u8; 20],
    advertise_extensions: bool,
) -> Result<(ConnectedPeer, Handshake), DownloadError> {
    let mut peer = TcpStream::connect(address)
        .await
        .map_err(|source| DownloadError::Io {
            operation: "connect to peer",
            source,
        })?;
    let handshake = if advertise_extensions {
        let mut reserved = [0; 8];
        reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
        encode_handshake_with_reserved(info_hash, DIAGNOSTIC_PEER_ID, reserved)
    } else {
        encode_handshake(info_hash, DIAGNOSTIC_PEER_ID)
    };
    peer.write_all(&handshake)
        .await
        .map_err(|source| DownloadError::Io {
            operation: "send peer handshake",
            source,
        })?;

    let mut handshake = [0_u8; HANDSHAKE_LENGTH];
    peer.read_exact(&mut handshake)
        .await
        .map_err(|source| DownloadError::Io {
            operation: "read peer handshake",
            source,
        })?;
    let handshake = decode_handshake(&handshake, info_hash).map_err(DownloadError::Handshake)?;
    Ok((
        ConnectedPeer {
            stream: peer,
            decoder: FrameDecoder::new(),
            queued_messages: VecDeque::new(),
        },
        handshake,
    ))
}

async fn next_peer_message(peer: &mut ConnectedPeer) -> Result<PeerMessage, DownloadError> {
    while peer.queued_messages.is_empty() {
        let mut network_buffer = [0_u8; NETWORK_READ_LENGTH];
        let read = peer
            .stream
            .read(&mut network_buffer)
            .await
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

async fn send_message(peer: &mut TcpStream, message: &PeerMessage) -> Result<(), DownloadError> {
    let frame = encode_message(message).map_err(DownloadError::Frame)?;
    peer.write_all(&frame)
        .await
        .map_err(|source| DownloadError::Io {
            operation: "send peer message",
            source,
        })
}

async fn process_actions(
    peer: &mut TcpStream,
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
    peer: &mut TcpStream,
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
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use rstorrent_protocol::metadata::{
        MetadataMessage, encode_extension_handshake, encode_metadata_data, parse_metadata_message,
    };
    use rstorrent_protocol::peer_wire::{
        EXTENSION_PROTOCOL_RESERVED_BIT, EXTENSION_PROTOCOL_RESERVED_INDEX, FrameDecoder,
        HANDSHAKE_LENGTH, PeerMessage, decode_handshake, encode_handshake_with_reserved,
    };
    use rstorrent_protocol::piece::MIN_PAYLOAD_ALLOWANCE;
    use sha1::{Digest, Sha1};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::time::timeout;

    use super::{
        ConnectedPeer, DIAGNOSTIC_PEER_ID, DownloadConfig, DownloadControl, DownloadError,
        MagnetDownloadConfig, download_magnet_with_peer_hint, download_verified_piece,
        download_verified_piece_with_control, next_peer_message, send_message,
    };
    use crate::selective_storage::{
        SelectiveStorageError, selective_part_path, selective_staging_path,
    };
    use crate::storage::staging_path;

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn test_path(name: &str) -> PathBuf {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rstorrent-driver-test-{}-{sequence}-{name}",
            std::process::id()
        ))
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
        assert_eq!(handshake.peer_id, DIAGNOSTIC_PEER_ID);
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
        let mut peer = ConnectedPeer {
            stream,
            decoder: FrameDecoder::new(),
            queued_messages: VecDeque::new(),
        };

        let PeerMessage::Extended { id: 0, .. } = next_peer_message(&mut peer)
            .await
            .expect("client extension handshake")
        else {
            panic!("expected extension handshake");
        };
        send_message(&mut peer.stream, &PeerMessage::Bitfield(bitfield))
            .await
            .expect("send early bitfield");
        send_message(&mut peer.stream, &PeerMessage::Unchoke)
            .await
            .expect("send early unchoke");
        send_message(
            &mut peer.stream,
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
            &mut peer.stream,
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
                        &mut peer.stream,
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

    #[tokio::test]
    async fn magnet_metadata_hands_the_same_peer_to_content_download() {
        let payload = b"verified magnet payload".to_vec();
        let info = single_file_info(&payload);
        let info_hash: [u8; 20] = Sha1::digest(&info).into();
        let output_path = test_path("magnet-output.bin");
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

        let report = download_magnet_with_peer_hint(MagnetDownloadConfig {
            magnet: format!("magnet:?xt=urn:btih:{}&x.pe={address}", hex(&info_hash)),
            output_path: output_path.clone(),
            timeout: Duration::from_secs(2),
            max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
            skip_files: Vec::new(),
            materialize_files: Vec::new(),
        })
        .await
        .expect("magnet metadata and content");

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

        let result = download_magnet_with_peer_hint(MagnetDownloadConfig {
            magnet: format!("magnet:?xt=urn:btih:{}&x.pe={address}", hex(&info_hash)),
            output_path: output_path.clone(),
            timeout: Duration::from_secs(2),
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

        let result = download_magnet_with_peer_hint(MagnetDownloadConfig {
            magnet: format!("magnet:?xt=urn:btih:{}&x.pe={address}", hex(&info_hash)),
            output_path: output_path.clone(),
            timeout: Duration::from_secs(2),
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

        let result = download_magnet_with_peer_hint(MagnetDownloadConfig {
            magnet: format!("magnet:?xt=urn:btih:{}&x.pe={address}", hex(&info_hash)),
            output_path: output_path.clone(),
            timeout: Duration::from_secs(2),
            max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
            skip_files: Vec::new(),
            materialize_files: Vec::new(),
        })
        .await;

        assert!(matches!(result, Err(DownloadError::PeerClosed)));
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
            timeout: Duration::from_millis(50),
            max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
            skip_files: Vec::new(),
            materialize_files: Vec::new(),
        })
        .await;

        assert!(matches!(result, Err(DownloadError::TimedOut { .. })));
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
            timeout: Duration::from_millis(50),
            max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
            skip_files: vec![1],
            materialize_files: Vec::new(),
        })
        .await;

        assert!(matches!(result, Err(DownloadError::TimedOut { .. })));
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
                timeout: Duration::from_secs(5),
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
            timeout: Duration::from_secs(1),
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
