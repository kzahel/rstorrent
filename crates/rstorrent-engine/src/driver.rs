use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rstorrent_protocol::extension::{
    ExtensionAdvertisement, ExtensionMap, PexFlags, UT_PEX_LOCAL_ID,
    encode_extension_handshake as encode_recognized_extension_handshake,
    parse_extension_handshake as parse_recognized_extension_handshake,
};
use rstorrent_protocol::magnet::{Magnet, MagnetError, PeerHint, TrackerUrl, UdpTrackerUrl};
use rstorrent_protocol::metadata::{
    MetadataError, MetadataExtensionUpdate, MetadataInstant, MetadataMessage,
    TorrentMetadataDownload, TorrentMetadataEvent, UT_METADATA_LOCAL_ID,
    encode_extension_handshake, encode_metadata_reject, encode_metadata_request,
    parse_extension_handshake, parse_metadata_message,
};
use rstorrent_protocol::metainfo::{
    BEP9_METAINFO_LIMITS, DURABLE_METAINFO_LIMITS, MAX_METAINFO_PIECES, Metainfo, MetainfoError,
};
use rstorrent_protocol::peer_wire::{
    FrameError, Handshake, HandshakeError, NegotiatedPeerCapabilities, PeerMessage,
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
use tokio::sync::{mpsc, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{Instant as TokioInstant, timeout, timeout_at};
use tokio_util::sync::CancellationToken;

use crate::dht::{DhtError, DhtHandle};
use crate::metrics::ByteMetric;
use crate::mse::MseDhWorkOwner;
#[cfg(test)]
use crate::network::DEFAULT_PEER_ID;
use crate::network::{NetworkConfig, NetworkPolicy, PeerEncryptionPolicyHandle};
use crate::peer::{
    DialAttempt, DialAttemptId, DialCandidate, PeerEndpoint, PeerFailure, PeerIntegrityAction,
    PeerObservation, PeerRegistryError, PeerSelectionContext, PeerSelector, PeerSource,
};
use crate::peer_budget::PeerBudget;
use crate::peer_runtime::{
    PeerAdmissionOutcome, PeerAdmissionRejection, PeerConnectionRole, PeerContentActivity,
    PeerRequestWindowPhase, PeerRuntimeError, connection_id,
};
use crate::peer_socket::{
    self, PeerConnection, PeerSetError, PeerSetEvent, PeerSocketError, PeerSocketSet, PeerTaskEvent,
};
use crate::pex::{PexError, PexReceiveContext, PexReceiveDisposition};
use crate::piece_picker::picker_seed;
use crate::selective_storage::{
    DescriptorStorage, PreparedFileHash, PublicationShape, ResumeArtifactState, ResumedStorage,
    SelectiveStorage, SelectiveStorageError, VERIFICATION_CHUNK_LENGTH,
    remove_selective_part_if_present, remove_selective_staging_if_present,
    torrent_storage_paths_for_output_with_shape, validate_publication_name,
};
use crate::swarm::{
    BlockKey, ConnectionId, ConnectionRemoval, ConnectionWindowPhaseSnapshot, PendingDialId,
    PieceHashFailure, PiecePlan, ReceiveDisposition, RejectDisposition, RequestAssignment,
    SwarmConfig, SwarmError, SwarmState,
};
use crate::torrent_peer::{TorrentPeerError, TorrentPeerHandle};
use crate::tracker::{
    TrackerAction, TrackerConfig, TrackerConnectionFamily, TrackerEndpoint, TrackerId,
    TrackerSchedule, TrackerWaitKind,
};

mod control;
mod storage_pipeline;

#[cfg(test)]
const CLIENT_PEER_ID: [u8; 20] = DEFAULT_PEER_ID;
const NETWORK_RESOLUTION_TIMEOUT: Duration = Duration::from_secs(10);
const UDP_TRACKER_RETRANSMIT_AFTER: Duration = Duration::from_secs(15);
const UDP_TRACKER_COMPLETION_TIMEOUT: Duration = Duration::from_secs(30);
const UDP_TRACKER_TOKEN_LIFETIME: Duration = Duration::from_secs(60);

fn admission_rejection_failure(outcome: PeerAdmissionOutcome) -> Option<PeerFailure> {
    match outcome {
        PeerAdmissionOutcome::Admitted { .. } => None,
        PeerAdmissionOutcome::Rejected(PeerAdmissionRejection::SelfConnection) => {
            Some(PeerFailure::SelfConnection)
        }
        PeerAdmissionOutcome::Rejected(PeerAdmissionRejection::DuplicatePeerId { .. }) => {
            Some(PeerFailure::DuplicatePeerId)
        }
    }
}

const MAX_UDP_TRACKER_TOKENS: usize = 64;
const MAX_CONCURRENT_TRACKER_OPERATIONS: usize = 8;
const TRACKER_RESULT_QUEUE: usize = 4;
const CONTENT_DISCOVERY_QUEUE: usize = 8;
const MAX_RESOLVED_ADDRESSES: usize = 32;
const UNKNOWN_MAGNET_LEFT: u64 = 16 * 1024;
const UDP_TRACKER_RECEIVE_LENGTH: usize = MAX_ANNOUNCE_RESPONSE_LENGTH + 1;
const DHT_RETRY_INITIAL_DELAY: Duration = Duration::from_secs(15);
const DHT_RETRY_MAX_DELAY: Duration = Duration::from_secs(5 * 60);
const DHT_SUCCESS_REQUERY_DELAY: Duration = Duration::from_secs(60);
const CONTENT_SWARM_MAINTENANCE_INTERVAL: Duration = Duration::from_millis(100);
const MAX_METADATA_PEERS: usize = 8;
const METADATA_SCHEDULER_TICK: Duration = Duration::from_millis(100);
const MAX_ENGINE_PIECES: usize = MAX_METAINFO_PIECES;
const MAX_PLANNED_CONTENT_PIECES: usize = 256;

fn public_pex_extension_handshake() -> Vec<u8> {
    encode_recognized_extension_handshake(ExtensionAdvertisement {
        pex_id: Some(UT_PEX_LOCAL_ID),
        ..ExtensionAdvertisement::default()
    })
    .expect("the stable local PEX extension ID is valid")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DownloadResourceLimits {
    pub max_outstanding_request_bytes: usize,
    pub max_buffered_payload_bytes: usize,
    pub max_active_piece_bytes: usize,
    pub max_active_pieces: usize,
}

impl DownloadResourceLimits {
    pub const DESKTOP: Self = Self {
        max_outstanding_request_bytes: 256 * 1024 * 1024,
        max_buffered_payload_bytes: 32 * 1024 * 1024,
        max_active_piece_bytes: 256 * 1024 * 1024,
        max_active_pieces: crate::swarm::DEFAULT_MAX_ACTIVE_PIECES,
    };

    pub const ANDROID: Self = Self {
        max_outstanding_request_bytes: 128 * 1024 * 1024,
        max_buffered_payload_bytes: 16 * 1024 * 1024,
        max_active_piece_bytes: 128 * 1024 * 1024,
        max_active_pieces: crate::swarm::DEFAULT_MAX_ACTIVE_PIECES,
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
            max_active_pieces: crate::swarm::DEFAULT_MAX_ACTIVE_PIECES,
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
        if self.max_active_pieces == 0 {
            return Err(DownloadError::InvalidResourceLimit(
                "active piece count must be nonzero",
            ));
        }
        Ok(self)
    }

    fn swarm_config(self) -> SwarmConfig {
        let mut config = SwarmConfig::for_request_limit(self.max_outstanding_request_bytes);
        config.max_active_piece_bytes = self.max_active_piece_bytes;
        config.max_active_pieces = self.max_active_pieces;
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
    /// Session-wide connection ownership shared with the incoming listener.
    pub peer_budget: PeerBudget,
    /// Session-wide owner for bounded MSE Diffie-Hellman work.
    pub mse_dh: MseDhWorkOwner,
    /// Live session policy sampled when each future handshake starts.
    pub encryption: PeerEncryptionPolicyHandle,
    /// Long-lived per-torrent peer state supplied by the application session.
    /// Diagnostic and standalone callers leave this unset.
    pub torrent_peers: Option<TorrentPeerHandle>,
    pub resource_limits: DownloadResourceLimits,
    pub skip_files: Vec<usize>,
    pub verified_info: Option<Vec<u8>>,
    pub verified_pieces: Vec<bool>,
    pub artifact_state: ResumeArtifactState,
    pub download_missing: bool,
    pub dht: Option<DhtHandle>,
    /// Authoritative operational UDP tracker catalog. `None` uses the
    /// independently bounded trackers parsed from the magnet URI.
    pub udp_trackers: Option<Vec<TrackerConfig>>,
}

pub trait DownloadCheckpointSink: Send + Sync {
    fn metadata_verified(&self, raw_info: &[u8]) -> Result<(), String>;
    fn storage_prepared(&self, storage: ResumedStorage) -> Result<(), String>;
    fn recheck_started(&self) -> Result<u64, String>;
    fn have_rechecked(&self, verified_pieces: &[bool]) -> Result<(), String>;
    fn pieces_invalidated(&self, piece_indices: &[usize]) -> Result<(), String>;
    fn pieces_durable(&self, piece_indices: &[usize]) -> Result<(), String>;
    fn piece_durable(&self, piece_index: usize) -> Result<(), String> {
        self.pieces_durable(&[piece_index])
    }
    fn descriptor_prepared(&self, files: &[PreparedFileHash]) -> Result<(), String>;
    fn publication_prepared(&self) -> Result<(), String>;
    fn published(&self) -> Result<(), String>;
}

#[derive(Clone, Debug)]
struct ContentDownloadConfig {
    output_path: PathBuf,
    max_buffered_payload_bytes: usize,
    swarm_config: SwarmConfig,
    skip_files: Vec<usize>,
    materialize_files: Vec<usize>,
}

#[cfg(test)]
use control::{
    CONTENT_STORAGE_HASH_CONCURRENCY, CONTENT_STORAGE_WRITE_CONCURRENCY,
    MAX_DIAGNOSTIC_ERROR_LENGTH, MAX_RECENT_METADATA_ATTEMPTS, atomic_saturating_add,
    atomic_saturating_increment,
};
pub use control::{
    CheckerPhase, CheckerProgress, ContentPeerActivitySnapshot, ContentRequestWindowPhase,
    DiskCheckpointStage, DiskPieceRuntimeSnapshot, DiskPieceStage, DiskPressure,
    DiskRuntimeSnapshot, DownloadActivityEvent, DownloadActivitySink, DownloadControl,
    DownloadDiagnosticSnapshot, DownloadProgress, FileSelectionUpdate, MetadataAcquisitionPhase,
    MetadataAcquisitionSnapshot, MetadataPeerSnapshot, MetadataPeerStage, PathPublicationStage,
    SwarmActivitySnapshot,
};
use control::{CheckerPieceOutcome, StorageCommandKind};

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
    Pex(PexError),
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
            Self::Pex(error) => write!(formatter, "peer exchange: {error}"),
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
            Self::Pex(error) => Some(error),
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

fn map_torrent_peer_error(error: TorrentPeerError) -> DownloadError {
    match error {
        TorrentPeerError::Registry(error) => DownloadError::PeerRegistry(error),
        TorrentPeerError::Runtime(error) => DownloadError::PeerRuntime(error),
        TorrentPeerError::Pex(error) => DownloadError::Pex(error),
        TorrentPeerError::AddressFamilyDenied(_) => DownloadError::NoUsablePeer,
        TorrentPeerError::ConnectionIdentifierOverflow => {
            DownloadError::PeerRegistry(PeerRegistryError::IdentifierOverflow("peer connection"))
        }
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
    download_magnet_metadata_with_dht(magnet, network, control, None, PeerBudget::system_default())
        .await
}

pub async fn download_magnet_metadata_with_dht(
    magnet: String,
    network: NetworkConfig,
    control: DownloadControl,
    dht: Option<DhtHandle>,
    peer_budget: PeerBudget,
) -> Result<Vec<u8>, DownloadError> {
    download_magnet_metadata_with_dht_and_peers(magnet, network, control, dht, peer_budget, None)
        .await
}

pub async fn download_magnet_metadata_with_dht_and_peers(
    magnet: String,
    network: NetworkConfig,
    control: DownloadControl,
    dht: Option<DhtHandle>,
    peer_budget: PeerBudget,
    torrent_peers: Option<TorrentPeerHandle>,
) -> Result<Vec<u8>, DownloadError> {
    validate_network_config(network)?;
    if control.is_cancelled() {
        return Err(DownloadError::Cancelled);
    }
    let result = run_magnet_metadata(
        magnet,
        network,
        control.clone(),
        dht,
        None,
        TorrentPeerResources {
            peer_budget,
            torrent_peers,
            mse_dh: MseDhWorkOwner::new(),
            encryption: PeerEncryptionPolicyHandle::new(network.encryption),
        },
    )
    .await;
    let result = require_terminal_owner_cleanup(&control, result);
    control.clear_buffered_payload();
    result
}

pub async fn download_magnet_metadata_with_external_discovery(
    magnet: String,
    network: NetworkConfig,
    control: DownloadControl,
    peer_budget: PeerBudget,
    mse_dh: MseDhWorkOwner,
    encryption: PeerEncryptionPolicyHandle,
    torrent_peers: TorrentPeerHandle,
) -> Result<Vec<u8>, DownloadError> {
    validate_network_config(network)?;
    if control.is_cancelled() {
        return Err(DownloadError::Cancelled);
    }
    let result = run_magnet_metadata(
        magnet,
        network,
        control.clone(),
        None,
        Some(Vec::new()),
        TorrentPeerResources {
            peer_budget,
            torrent_peers: Some(torrent_peers),
            mse_dh,
            encryption,
        },
    )
    .await;
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
    let result = run_download(config, control.clone(), None, None).await;
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

#[doc(hidden)]
pub async fn download_verified_piece_with_peer_state(
    config: DownloadConfig,
    control: DownloadControl,
    peer_budget: PeerBudget,
    torrent_peers: TorrentPeerHandle,
) -> Result<DownloadReport, DownloadError> {
    validate_download_config(&config)?;
    if control.is_cancelled() {
        return Err(DownloadError::Cancelled);
    }
    let output_path = config.output_path.clone();
    let result = run_download(
        config,
        control.clone(),
        None,
        Some((peer_budget, torrent_peers)),
    )
    .await;
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
    let result = run_download(config, control.clone(), Some(descriptors), None).await;
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
    fast_extension: bool,
    initial_availability_received: bool,
    choking: bool,
    bitfield: Option<Vec<u8>>,
    have_all: bool,
    have_none: bool,
    haves: Vec<u8>,
    fast_advisories: VecDeque<PeerMessage>,
}

impl PremetadataPeerState {
    fn new(fast_extension: bool) -> Self {
        Self {
            fast_extension,
            initial_availability_received: false,
            choking: true,
            bitfield: None,
            have_all: false,
            have_none: false,
            haves: Vec::new(),
            fast_advisories: VecDeque::new(),
        }
    }

    fn observe(&mut self, message: PeerMessage) -> Result<(), DownloadError> {
        let fast_message = matches!(
            message,
            PeerMessage::SuggestPiece(_)
                | PeerMessage::HaveAll
                | PeerMessage::HaveNone
                | PeerMessage::RejectRequest(_)
                | PeerMessage::AllowedFast(_)
        );
        if fast_message && !self.fast_extension {
            return Err(DownloadError::InvalidPremetadataState(
                "Fast message arrived without negotiated support",
            ));
        }
        if self.fast_extension {
            let initial = matches!(
                message,
                PeerMessage::Bitfield(_) | PeerMessage::HaveAll | PeerMessage::HaveNone
            );
            if !self.initial_availability_received && !initial {
                return Err(DownloadError::InvalidPremetadataState(
                    "Fast initial availability is missing",
                ));
            }
            if self.initial_availability_received && initial {
                return Err(DownloadError::InvalidPremetadataState(
                    "Fast initial availability was repeated",
                ));
            }
            if initial {
                self.initial_availability_received = true;
            }
        }
        match message {
            PeerMessage::KeepAlive | PeerMessage::Interested | PeerMessage::NotInterested => {}
            PeerMessage::Choke => self.choking = true,
            PeerMessage::Unchoke => self.choking = false,
            PeerMessage::Have(index) => {
                let index = usize::try_from(index).map_err(|_| {
                    DownloadError::InvalidPremetadataState("HAVE index does not fit usize")
                })?;
                if index >= MAX_ENGINE_PIECES {
                    return Err(DownloadError::InvalidPremetadataState(
                        "HAVE index exceeds the supported piece-count bound",
                    ));
                }
                let byte = index / 8;
                if self.haves.len() <= byte {
                    self.haves.resize(byte + 1, 0);
                }
                self.haves[byte] |= 1 << (7 - index % 8);
            }
            PeerMessage::Bitfield(bitfield) => {
                if bitfield.len() > MAX_ENGINE_PIECES.div_ceil(8) {
                    return Err(DownloadError::InvalidPremetadataState(
                        "bitfield exceeds the supported piece-count bound",
                    ));
                }
                self.bitfield = Some(bitfield);
            }
            PeerMessage::HaveAll => self.have_all = true,
            PeerMessage::HaveNone => self.have_none = true,
            PeerMessage::SuggestPiece(piece) | PeerMessage::AllowedFast(piece) => {
                if usize::try_from(piece).map_or(true, |piece| piece >= MAX_ENGINE_PIECES) {
                    return Err(DownloadError::InvalidPremetadataState(
                        "Fast advisory index exceeds the supported piece-count bound",
                    ));
                }
                if self.fast_advisories.len() < crate::swarm::MAX_FAST_ADVISORY_PIECES {
                    self.fast_advisories.push_back(message);
                }
            }
            PeerMessage::RejectRequest(_) => {
                return Err(DownloadError::InvalidPremetadataState(
                    "Fast reject arrived before any payload request",
                ));
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
        let expected_length = piece_count.div_ceil(8);
        let had_availability =
            self.bitfield.is_some() || self.have_all || self.have_none || !self.haves.is_empty();
        let mut bitfield = if self.have_all {
            vec![u8::MAX; expected_length]
        } else {
            self.bitfield.unwrap_or_else(|| vec![0; expected_length])
        };
        if self.have_all && !piece_count.is_multiple_of(8) {
            let used = piece_count % 8;
            if let Some(last) = bitfield.last_mut() {
                *last &= u8::MAX << (8 - used);
            }
        }
        if had_availability {
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
            if self.haves.len() > expected_length
                && self.haves[expected_length..].iter().any(|byte| *byte != 0)
            {
                return Err(DownloadError::InvalidPremetadataState(
                    "HAVE index is outside verified metadata",
                ));
            }
            for (target, haves) in bitfield.iter_mut().zip(self.haves) {
                *target |= haves;
            }
            let remainder = piece_count % 8;
            if remainder != 0 {
                let unused_mask = (1_u8 << (8 - remainder)) - 1;
                if bitfield.last().is_some_and(|byte| byte & unused_mask != 0) {
                    return Err(DownloadError::InvalidPremetadataState(
                        "availability sets unused trailing bits",
                    ));
                }
            }
            messages.push_back(PeerMessage::Bitfield(bitfield));
        }
        for message in self.fast_advisories {
            let piece = match message {
                PeerMessage::SuggestPiece(piece) | PeerMessage::AllowedFast(piece) => piece,
                _ => unreachable!("only Fast advisories are retained"),
            };
            if usize::try_from(piece).map_or(true, |piece| piece >= piece_count) {
                return Err(DownloadError::InvalidPremetadataState(
                    "Fast advisory index is outside verified metadata",
                ));
            }
            messages.push_back(message);
        }
        if !self.choking {
            messages.push_back(PeerMessage::Unchoke);
        }
        Ok(messages)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct UdpTrackerTiming {
    pub(crate) retransmit_after: Duration,
    pub(crate) completion_timeout: Duration,
}

impl UdpTrackerTiming {
    pub(crate) const PRODUCTION: Self = Self {
        retransmit_after: UDP_TRACKER_RETRANSMIT_AFTER,
        completion_timeout: UDP_TRACKER_COMPLETION_TIMEOUT,
    };
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct UdpTrackerAnnounce {
    pub(crate) info_hash: [u8; 20],
    pub(crate) peer_id: [u8; 20],
    pub(crate) key: u32,
    pub(crate) downloaded: u64,
    pub(crate) left: u64,
    pub(crate) uploaded: u64,
    pub(crate) event: AnnounceEvent,
    pub(crate) num_want: i32,
    pub(crate) port: u16,
    pub(crate) ipv6_port: u16,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct UdpTrackerExchange<'a> {
    pub(crate) timing: UdpTrackerTiming,
    pub(crate) control: &'a DownloadControl,
    pub(crate) tracker_label: &'a str,
    pub(crate) source_ipv4: Option<IpAddr>,
    pub(crate) source_ipv6: Option<IpAddr>,
}

#[derive(Clone, Copy, Debug)]
struct UdpTrackerToken {
    connection_id: u64,
    expires_at: Instant,
}

#[derive(Debug, Default)]
pub(crate) struct UdpTrackerTokenCache {
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
    result: Result<UdpTrackerAnnounceResult, DownloadError>,
}

#[derive(Debug)]
pub(crate) struct UdpTrackerAnnounceResult {
    pub response: AnnounceResponse,
    pub connection_family: TrackerConnectionFamily,
}

#[derive(Debug)]
struct TrackerManager {
    receiver: mpsc::Receiver<TrackerUpdate>,
    cancellation: CancellationToken,
    task: Option<JoinHandle<()>>,
}

impl TrackerManager {
    #[cfg(test)]
    fn start(
        trackers: Vec<UdpTrackerUrl>,
        info_hash: [u8; 20],
        network: NetworkConfig,
        control: DownloadControl,
    ) -> Result<Self, DownloadError> {
        Self::start_with_configs(
            configured_udp_trackers(&trackers),
            info_hash,
            network,
            control,
        )
    }

    fn start_with_configs(
        mut trackers: Vec<TrackerConfig>,
        info_hash: [u8; 20],
        network: NetworkConfig,
        control: DownloadControl,
    ) -> Result<Self, DownloadError> {
        shuffle_tracker_configs(&mut trackers)?;
        let tracker_key = random_nonzero_u32()?;
        let cancellation = CancellationToken::new();
        let task_cancellation = cancellation.clone();
        let (sender, receiver) = mpsc::channel(TRACKER_RESULT_QUEUE);
        let task = tokio::spawn(run_tracker_manager(
            TrackerSchedule::from_configs(trackers),
            info_hash,
            tracker_key,
            network,
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
        if peers.external_discovery {
            tasks.push(tokio::spawn(keep_content_external_discovery_alive(
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

async fn keep_content_external_discovery_alive(
    _sender: mpsc::Sender<ContentDiscoveryEvent>,
    cancellation: CancellationToken,
) -> Result<(), DownloadError> {
    cancellation.cancelled().await;
    Ok(())
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
            _ = control.cancelled() => {
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
                    _ = control.cancelled() => return Ok(()),
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
                    _ = control.cancelled() => return Ok(()),
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
    network: NetworkConfig,
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
        network,
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
    network: NetworkConfig,
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
                    endpoint,
                    tier,
                    source: _,
                    event,
                    attempt,
                    fallback,
                } => {
                    let tracker = url;
                    let TrackerEndpoint::Udp(url) = endpoint else {
                        let _ = schedule.failed(
                            id,
                            started_at.elapsed(),
                            "HTTP tracker transport is unavailable in the direct engine manager",
                        );
                        continue;
                    };
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
                    let session_permit = tokio::select! {
                        biased;
                        _ = cancellation.cancelled() => {
                            shutdown_tracker_operations(&mut operations).await;
                            return;
                        }
                        _ = control.cancelled() => {
                            shutdown_tracker_operations(&mut operations).await;
                            return;
                        }
                        permit = control.acquire_tracker_operation() => permit,
                    };
                    operations.spawn(async move {
                        let _session_permit = session_permit;
                        let result = announce_udp_tracker(
                            &url,
                            network.policy,
                            network.address_families,
                            &mut token_cache,
                            UdpTrackerAnnounce {
                                info_hash,
                                peer_id: network.peer_id,
                                key: tracker_key,
                                downloaded: 0,
                                left: UNKNOWN_MAGNET_LEFT,
                                uploaded: 0,
                                event,
                                num_want: MAX_COMPACT_PEERS as i32,
                                port: 1,
                                ipv6_port: 1,
                            },
                            UdpTrackerExchange {
                                timing: UdpTrackerTiming::PRODUCTION,
                                control: &operation_control,
                                tracker_label: &tracker,
                                source_ipv4: None,
                                source_ipv6: None,
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
                Ok(result) => {
                    let response = result.response;
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
            TrackerAction::Wait {
                delay, url, kind, ..
            } => {
                let tracker = url;
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

fn shuffle_tracker_configs(trackers: &mut [TrackerConfig]) -> Result<(), DownloadError> {
    let mut first = 0;
    while first < trackers.len() {
        let tier = trackers[first].tier;
        let mut end = first + 1;
        while end < trackers.len() && trackers[end].tier == tier {
            end += 1;
        }
        shuffle_tracker_tier(&mut trackers[first..end])?;
        first = end;
    }
    Ok(())
}

fn shuffle_tracker_tier<T>(trackers: &mut [T]) -> Result<(), DownloadError> {
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
    peers: TorrentPeerHandle,
    owns_peer_sink: bool,
    external_discovery: bool,
    selector: PeerSelector,
    network: NetworkConfig,
    peer_budget: PeerBudget,
    mse_dh: MseDhWorkOwner,
    encryption: PeerEncryptionPolicyHandle,
    tracker: Option<TrackerManager>,
    dht: Option<DhtHandle>,
    control: DownloadControl,
    connection: Option<PeerConnection>,
    last_error: Option<DownloadError>,
    next_dht_lookup: Instant,
}

struct TorrentPeerResources {
    peer_budget: PeerBudget,
    torrent_peers: Option<TorrentPeerHandle>,
    mse_dh: MseDhWorkOwner,
    encryption: PeerEncryptionPolicyHandle,
}

impl TorrentPeerResources {
    fn standalone(network: NetworkConfig) -> Self {
        Self {
            peer_budget: PeerBudget::system_default(),
            torrent_peers: None,
            mse_dh: MseDhWorkOwner::new(),
            encryption: PeerEncryptionPolicyHandle::new(network.encryption),
        }
    }
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
    PolicyCheck,
    Discovery(Result<(), DownloadError>),
    Socket(Result<PeerSetEvent, PeerSetError>),
    Worker(Box<Option<Result<MetadataPeerResult, tokio::task::JoinError>>>),
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
            _ = control.cancelled() => {
                return Err(DownloadError::Cancelled);
            }
            _ = tokio::time::sleep(initial_wait) => {}
        }
    }
    let mut retry_delay = timing.initial_delay;
    loop {
        control.emit(DownloadActivityEvent::DhtLookupStarted);
        let result = tokio::select! {
            _ = control.cancelled() => {
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
                    _ = control.cancelled() => {
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
        Self::new_with_peer_budget(network, control, PeerBudget::system_default())
    }

    fn new_with_peer_budget(
        network: NetworkConfig,
        control: DownloadControl,
        peer_budget: PeerBudget,
    ) -> Result<Self, DownloadError> {
        Self::new_with_peer_state(
            network,
            control,
            peer_budget,
            None,
            MseDhWorkOwner::new(),
            PeerEncryptionPolicyHandle::new(network.encryption),
        )
    }

    fn new_with_peer_state(
        network: NetworkConfig,
        control: DownloadControl,
        peer_budget: PeerBudget,
        peers: Option<TorrentPeerHandle>,
        mse_dh: MseDhWorkOwner,
        encryption: PeerEncryptionPolicyHandle,
    ) -> Result<Self, DownloadError> {
        validate_network_config(network)?;
        let owns_peer_sink = peers.is_none();
        let external_discovery = peers.is_some();
        let peers = match peers {
            Some(peers) => peers,
            None => {
                TorrentPeerHandle::new(Arc::new(control.clone())).map_err(map_torrent_peer_error)?
            }
        };
        if owns_peer_sink {
            peers
                .enforce_address_families(network.address_families)
                .map_err(map_torrent_peer_error)?;
        }
        Ok(Self {
            peers,
            owns_peer_sink,
            external_discovery,
            selector: PeerSelector,
            network,
            peer_budget,
            mse_dh,
            encryption,
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
            .peers
            .with_state(|state| state.begin_dial(candidate, role, context.now))
            .map_err(map_torrent_peer_error)?;
        self.publish_peer_runtime(true)?;
        Ok(attempt)
    }

    fn connection_network(&self) -> NetworkConfig {
        self.network
            .with_encryption(self.encryption.load())
            .with_address_families(self.peers.address_family_policy())
    }

    fn transport_connected(&mut self, attempt: DialAttempt) -> Result<(), DownloadError> {
        let connection = connection_id(attempt);
        let Some(peer) = self
            .peers
            .with_state(|state| state.runtime.observation(connection).cloned())
        else {
            return Ok(());
        };
        if peer.lifecycle != crate::peer_runtime::PeerConnectionLifecycle::TransportConnecting {
            return Ok(());
        }
        let now = self.elapsed();
        self.peers
            .with_state(|state| state.runtime.transport_connected(connection, now))
            .map_err(DownloadError::PeerRuntime)?;
        self.publish_peer_runtime(true)
    }

    fn dial_succeeded(
        &mut self,
        attempt: DialAttempt,
        connection: &PeerConnection,
        handshake: &Handshake,
    ) -> Result<PeerAdmissionOutcome, DownloadError> {
        let connection_id = connection_id(attempt);
        if let Some(cancellation) = connection.budget_cancellation() {
            self.peers
                .register_connection_cancellation(connection_id, cancellation);
        }
        let now = self.elapsed();
        let outcome = self
            .peers
            .with_state(|state| {
                if let Some(endpoint_state) = connection.mse_endpoint_update() {
                    state
                        .registry
                        .update_mse_endpoint(attempt, endpoint_state)?;
                }
                state
                    .runtime
                    .set_mse_method(connection_id, connection.mse_method())
                    .map_err(TorrentPeerError::Runtime)?;
                state.registry.dial_succeeded(attempt, now)?;
                let outcome = state
                    .runtime
                    .handshake_completed(connection_id, handshake, self.network.peer_id, now)
                    .map_err(TorrentPeerError::Runtime)?;
                if matches!(outcome, PeerAdmissionOutcome::Admitted { .. }) {
                    state.pex.peer_established(
                        attempt.endpoint(),
                        PexFlags::from_bits(PexFlags::OUTGOING),
                    );
                }
                Ok::<_, TorrentPeerError>(outcome)
            })
            .map_err(map_torrent_peer_error);
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => {
                self.peers.unregister_connection_cancellation(connection_id);
                return Err(error);
            }
        };
        self.peers.apply_admission(connection_id, outcome);
        self.publish_peer_runtime(true)?;
        Ok(outcome)
    }

    fn dial_failed(
        &mut self,
        attempt: DialAttempt,
        failure: PeerFailure,
    ) -> Result<(), DownloadError> {
        let connection = connection_id(attempt);
        let now = self.elapsed();
        self.peers
            .with_state(|state| {
                state
                    .runtime
                    .begin_disconnect(connection, Some(failure), now)
            })
            .map_err(DownloadError::PeerRuntime)?;
        self.publish_peer_runtime(true)?;
        self.peers
            .with_state(|state| {
                state.registry.dial_failed(attempt, now, failure)?;
                state
                    .runtime
                    .remove(connection)
                    .map_err(TorrentPeerError::Runtime)?;
                Ok::<_, TorrentPeerError>(())
            })
            .map_err(map_torrent_peer_error)?;
        self.peers.unregister_connection_cancellation(connection);
        self.publish_peer_runtime(true)
    }

    fn update_mse_endpoint(
        &mut self,
        attempt: DialAttempt,
        endpoint: crate::peer::MseEndpointState,
    ) -> Result<(), DownloadError> {
        self.peers
            .with_state(|state| state.registry.update_mse_endpoint(attempt, endpoint))
            .map_err(DownloadError::PeerRegistry)
    }

    fn dial_cancelled(&mut self, attempt: DialAttempt) -> Result<(), DownloadError> {
        let connection = connection_id(attempt);
        let now = self.elapsed();
        self.peers
            .with_state(|state| state.runtime.begin_disconnect(connection, None, now))
            .map_err(DownloadError::PeerRuntime)?;
        self.publish_peer_runtime(true)?;
        self.peers
            .with_state(|state| {
                state.registry.dial_cancelled(attempt)?;
                state
                    .runtime
                    .remove(connection)
                    .map_err(TorrentPeerError::Runtime)?;
                Ok::<_, TorrentPeerError>(())
            })
            .map_err(map_torrent_peer_error)?;
        self.peers.unregister_connection_cancellation(connection);
        self.publish_peer_runtime(true)
    }

    fn begin_disconnect(
        &mut self,
        attempt: DialAttempt,
        failure: Option<PeerFailure>,
    ) -> Result<(), DownloadError> {
        let now = self.elapsed();
        self.peers
            .with_state(|state| {
                state
                    .runtime
                    .begin_disconnect(connection_id(attempt), failure, now)
            })
            .map_err(DownloadError::PeerRuntime)?;
        self.publish_peer_runtime(true)
    }

    fn connection_closed(
        &mut self,
        attempt: DialAttempt,
        failure: Option<PeerFailure>,
    ) -> Result<(), DownloadError> {
        let connection = connection_id(attempt);
        if self.peers.with_state(|state| {
            state.runtime.observation(connection).is_some_and(|peer| {
                peer.lifecycle != crate::peer_runtime::PeerConnectionLifecycle::Disconnecting
            })
        }) {
            self.begin_disconnect(attempt, failure)?;
        }
        let now = self.elapsed();
        self.peers
            .with_state(|state| {
                state.pex.remove_source(connection, &mut state.registry);
                state.pex.peer_dropped(attempt.endpoint());
                state.registry.connection_closed(attempt, now, failure)?;
                state
                    .runtime
                    .remove(connection)
                    .map_err(TorrentPeerError::Runtime)?;
                Ok::<_, TorrentPeerError>(())
            })
            .map_err(map_torrent_peer_error)?;
        self.peers.unregister_connection_cancellation(connection);
        self.publish_peer_runtime(true)
    }

    fn apply_extension_handshake(
        &mut self,
        connection: ConnectionId,
        payload: &[u8],
    ) -> Result<ExtensionMap, DownloadError> {
        let handshake = parse_recognized_extension_handshake(payload)
            .map_err(|error| DownloadError::Pex(PexError::Extension(error)))?;
        Ok(self
            .peers
            .with_state(|state| state.pex.apply_extension_handshake(connection, handshake)))
    }

    fn install_extension_map(&mut self, connection: ConnectionId, map: ExtensionMap) {
        self.peers
            .with_state(|state| state.pex.install_extension_map(connection, map));
    }

    fn receive_pex(
        &mut self,
        connection: ConnectionId,
        payload: &[u8],
        verified_public: bool,
    ) -> Result<PexReceiveDisposition, DownloadError> {
        let source_endpoint = self
            .peers
            .with_state(|state| {
                state
                    .runtime
                    .observation(connection)
                    .map(|peer| peer.endpoint)
            })
            .ok_or(DownloadError::PeerRuntime(
                PeerRuntimeError::UnknownConnection(connection),
            ))?;
        let now = self.elapsed();
        let disposition = self
            .peers
            .with_state(|state| {
                state.pex.receive(
                    connection,
                    payload,
                    PexReceiveContext {
                        source_endpoint,
                        now,
                        verified_public,
                        network_policy: self.network.policy,
                        address_families: self.peers.address_family_policy(),
                        self_endpoints: &[],
                    },
                    &mut state.registry,
                )
            })
            .map_err(DownloadError::Pex)?;
        self.publish_peer_runtime(true)?;
        Ok(disposition)
    }

    fn next_pex(
        &mut self,
        connection: ConnectionId,
    ) -> Result<Option<(u8, Vec<u8>)>, DownloadError> {
        let now = self.elapsed();
        self.peers.with_state(|state| {
            let remote_id = state.pex.extension_map(connection).pex_id();
            let receiving_peer = state
                .runtime
                .observation(connection)
                .and_then(|peer| PeerEndpoint::new(peer.endpoint).ok());
            match (remote_id, receiving_peer) {
                (Some(remote_id), Some(receiving_peer)) => state
                    .pex
                    .next_outbound(connection, receiving_peer, now)
                    .map(|payload| payload.map(|payload| (remote_id, payload)))
                    .map_err(DownloadError::Pex),
                _ => Ok(None),
            }
        })
    }

    fn purge_pex_for_private(&mut self) -> Result<(), DownloadError> {
        self.peers
            .with_state(|state| state.pex.purge(&mut state.registry));
        self.publish_peer_runtime(true)
    }

    fn handoff_to_content(&mut self, attempt: DialAttempt) -> Result<(), DownloadError> {
        self.peers
            .with_state(|state| {
                state
                    .runtime
                    .set_role(connection_id(attempt), PeerConnectionRole::Content)
            })
            .map_err(DownloadError::PeerRuntime)?;
        self.publish_peer_runtime(true)
    }

    fn observe_content_peers(&mut self, state: &SwarmState) -> Result<(), DownloadError> {
        for peer in state.connection_activity(self.elapsed()) {
            self.peers
                .with_state(|state| {
                    state.runtime.set_content_activity(
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
                                ConnectionWindowPhaseSnapshot::Steady => {
                                    PeerRequestWindowPhase::Steady
                                }
                                ConnectionWindowPhaseSnapshot::Stalled => {
                                    PeerRequestWindowPhase::Stalled
                                }
                            },
                        },
                    )
                })
                .map_err(DownloadError::PeerRuntime)?;
        }
        self.publish_peer_runtime(false)
    }

    fn publish_peer_runtime(&mut self, force: bool) -> Result<(), DownloadError> {
        self.peers
            .publish(true, force)
            .map_err(map_torrent_peer_error)
    }

    fn publish_peer_registry(&self, force: bool) {
        let _ = self.peers.publish(true, force);
    }

    #[cfg(test)]
    fn from_endpoint(
        address: SocketAddr,
        source: PeerSource,
        network: NetworkConfig,
    ) -> Result<Self, DownloadError> {
        Self::from_endpoint_with_control(address, source, network, DownloadControl::new())
    }

    fn from_endpoint_with_control(
        address: SocketAddr,
        source: PeerSource,
        network: NetworkConfig,
        control: DownloadControl,
    ) -> Result<Self, DownloadError> {
        let mut peers = Self::new(network, control)?;
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
        Self::from_magnet_with_trackers(
            magnet,
            None,
            network,
            control,
            dht,
            TorrentPeerResources::standalone(network),
        )
        .await
    }

    async fn from_magnet_with_trackers(
        magnet: &Magnet,
        configured_trackers: Option<Vec<TrackerConfig>>,
        network: NetworkConfig,
        control: DownloadControl,
        dht: Option<DhtHandle>,
        resources: TorrentPeerResources,
    ) -> Result<Self, DownloadError> {
        let mut peers = Self::new_with_peer_state(
            network,
            control,
            resources.peer_budget,
            resources.torrent_peers,
            resources.mse_dh,
            resources.encryption,
        )?;
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
        let trackers =
            configured_trackers.unwrap_or_else(|| configured_magnet_trackers(&magnet.trackers));
        if !trackers.is_empty() {
            peers.tracker = Some(TrackerManager::start_with_configs(
                trackers,
                magnet.info_hash,
                network,
                peers.control.clone(),
            )?);
        }
        if peers.registry_is_empty()
            && peers.tracker.is_none()
            && peers.dht.is_none()
            && !peers.external_discovery
        {
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
        if !self.peers.address_family_policy().permits(address.ip()) {
            return Err(DownloadError::NoUsablePeer);
        }
        let endpoint = PeerEndpoint::new(address).map_err(DownloadError::PeerRegistry)?;
        let now = self.elapsed();
        self.peers
            .with_state(|state| {
                state
                    .registry
                    .observe(PeerObservation::dialable(endpoint, source), now)
            })
            .map_err(DownloadError::PeerRegistry)?;
        self.publish_peer_runtime(true)
    }

    fn elapsed(&self) -> Duration {
        self.peers.elapsed()
    }

    fn registry_is_empty(&self) -> bool {
        self.peers.with_state(|state| state.registry.is_empty())
    }

    #[cfg(test)]
    fn registry_len(&self) -> usize {
        self.peers.with_state(|state| state.registry.len())
    }

    fn select_candidate(&self, context: PeerSelectionContext) -> Option<DialCandidate> {
        let address_families = self.peers.address_family_policy();
        self.peers.with_state(|state| {
            self.selector
                .select_with_address_families(&state.registry, context, address_families)
        })
    }

    fn registry_snapshot(&self) -> crate::peer::PeerRegistrySnapshot {
        self.peers.registry_snapshot(true)
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
            .select_candidate(PeerSelectionContext {
                now: self.elapsed(),
            })
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
                        if self.registry_is_empty() {
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
            (false, false) if self.external_discovery => {
                tokio::select! {
                    _ = self.control.cancelled() => Err(DownloadError::Cancelled),
                    _ = tokio::time::sleep(Duration::from_millis(100)) => Ok(()),
                }
            }
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
            let candidate = match self.select_candidate(context) {
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
            match connect_peer(
                attempt,
                info_hash,
                advertise_extensions,
                self.connection_network(),
            )
            .await
            {
                Ok((connection, handshake)) => {
                    let admission = self.dial_succeeded(attempt, &connection, &handshake)?;
                    if let Some(failure) = admission_rejection_failure(admission) {
                        self.connection_closed(attempt, Some(failure))?;
                        continue;
                    }
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
            self.registry_snapshot(),
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
        let mut sockets = PeerSocketSet::with_owners(self.peer_budget.clone(), self.mse_dh.clone());
        let mut workers = JoinSet::new();
        let mut worker_cancellations: BTreeMap<DialAttemptId, (DialAttempt, CancellationToken)> =
            BTreeMap::new();
        let mut discovery_failed_while_active = false;
        let metadata = Arc::new(Mutex::new(TorrentMetadataDownload::new(info_hash)));

        loop {
            let address_families = self.peers.address_family_policy();
            sockets.cancel_disallowed(address_families);
            for (attempt, cancellation) in worker_cancellations.values() {
                if !address_families.permits(attempt.endpoint().address().ip()) {
                    cancellation.cancel();
                }
            }
            self.peers
                .enforce_address_families(address_families)
                .map_err(map_torrent_peer_error)?;
            while sockets.pending_len() + workers.len() < MAX_METADATA_PEERS {
                if !self.control.try_acquire_outbound_turn() {
                    break;
                }
                let context = PeerSelectionContext {
                    now: self.elapsed(),
                };
                let Some(candidate) = self.select_candidate(context) else {
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
                    self.connection_network(),
                    self.control.byte_metric_sink(),
                    self.control.mse_handshake_sink(),
                ) {
                    self.dial_cancelled(attempt)?;
                    if matches!(error, PeerSetError::ConnectionLimit(_)) {
                        break;
                    }
                    return Err(download_peer_set_error(error));
                }
            }

            self.control.observe_metadata_supervisor(
                self.registry_snapshot(),
                sockets.pending_len(),
                workers.len(),
                self.last_error.as_ref(),
            );

            if sockets.pending_len() == 0 && workers.is_empty() {
                let cancellation = self.control.cancellation_token();
                tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => {
                        return Err(DownloadError::Cancelled);
                    }
                    result = self.receive_discovery_peers(info_hash) => result?,
                    _ = tokio::time::sleep(CONTENT_SWARM_MAINTENANCE_INTERVAL) => {},
                }
                discovery_failed_while_active = false;
                continue;
            }

            let can_discover = !discovery_failed_while_active
                && sockets.pending_len() + workers.len() < MAX_METADATA_PEERS
                && (self.tracker.is_some() || self.dht.is_some());
            let cancellation = self.control.cancellation_token();
            let event = tokio::select! {
                biased;
                _ = cancellation.cancelled() => {
                    MetadataSupervisorEvent::Cancelled
                }
                result = self.receive_discovery_peers(info_hash), if can_discover => {
                    MetadataSupervisorEvent::Discovery(result)
                }
                _ = tokio::time::sleep(CONTENT_SWARM_MAINTENANCE_INTERVAL) => {
                    MetadataSupervisorEvent::PolicyCheck
                }
                event = sockets.next_event() => MetadataSupervisorEvent::Socket(event),
                joined = workers.join_next(), if !workers.is_empty() => {
                    MetadataSupervisorEvent::Worker(Box::new(joined))
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
                MetadataSupervisorEvent::PolicyCheck => {}
                MetadataSupervisorEvent::Socket(Ok(PeerSetEvent::DialCompleted {
                    attempt,
                    result,
                })) => match *result {
                    Ok((connection, handshake)) => {
                        let admission = self.dial_succeeded(attempt, &connection, &handshake)?;
                        if let Some(failure) = admission_rejection_failure(admission) {
                            self.connection_closed(attempt, Some(failure))?;
                            continue;
                        }
                        self.control
                            .metadata_peer_connected(attempt, handshake.supports_extensions());
                        let cancellation = CancellationToken::new();
                        worker_cancellations.insert(attempt.id(), (attempt, cancellation.clone()));
                        let control = self.control.clone();
                        let metadata = metadata.clone();
                        let admission_cancellation = connection.budget_cancellation();
                        workers.spawn(async move {
                            run_metadata_peer(
                                connection,
                                handshake,
                                cancellation,
                                admission_cancellation,
                                control,
                                metadata,
                            )
                            .await
                        });
                    }
                    Err(error) => {
                        if matches!(&error, PeerSocketError::Cancelled) {
                            self.dial_cancelled(attempt)?;
                            self.control.metadata_peer_finished(
                                attempt.id(),
                                MetadataPeerStage::Cancelled,
                                Some("metadata dial cancelled"),
                            );
                        } else {
                            let detail = error.to_string();
                            if let Some(endpoint) = error.mse_endpoint_update() {
                                self.update_mse_endpoint(attempt, endpoint)?;
                            }
                            self.dial_failed(attempt, error.peer_failure())?;
                            self.last_error = Some(download_peer_socket_error(error));
                            self.control.metadata_peer_finished(
                                attempt.id(),
                                MetadataPeerStage::Failed,
                                Some(&detail),
                            );
                        }
                    }
                },
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
                MetadataSupervisorEvent::Worker(joined) => match *joined {
                    Some(Ok(MetadataPeerResult::Complete {
                        connection,
                        raw_info,
                        metainfo,
                    })) => {
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
                    Some(Ok(MetadataPeerResult::Failed { connection, error })) => {
                        worker_cancellations.remove(&connection.attempt().id());
                        let failure = peer_failure(&error);
                        self.connection_closed(connection.attempt(), Some(failure))?;
                        self.last_error = Some(error);
                    }
                    Some(Ok(MetadataPeerResult::Cancelled { connection })) => {
                        worker_cancellations.remove(&connection.attempt().id());
                        self.connection_closed(connection.attempt(), None)?;
                    }
                    Some(Err(error)) => {
                        cleanup_metadata_attempts(
                            self,
                            &mut sockets,
                            &mut workers,
                            &mut worker_cancellations,
                        )
                        .await?;
                        return Err(DownloadError::PeerTask(error.to_string()));
                    }
                    None => {
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
                },
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
        self.peers
            .publish(!self.owns_peer_sink, true)
            .map_err(map_torrent_peer_error)?;
        result
    }

    async fn disable_dht_for_private(&mut self, info_hash: [u8; 20]) -> Result<(), DownloadError> {
        self.purge_pex_for_private()?;
        let current_is_dht_only = self.connection.as_ref().is_some_and(|connection| {
            self.peers.with_state(|state| {
                state
                    .registry
                    .get(connection.attempt().record_id())
                    .is_some_and(|record| {
                        record.sources().contains(PeerSource::Dht) && record.sources().len() == 1
                    })
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
        self.peers
            .with_state(|state| state.registry.remove_source(PeerSource::Dht));
        self.control
            .emit(DownloadActivityEvent::DhtDisabledForPrivateTorrent);
        Ok(())
    }
}

fn configured_udp_trackers(trackers: &[UdpTrackerUrl]) -> Vec<TrackerConfig> {
    trackers
        .iter()
        .cloned()
        .enumerate()
        .map(|(position, endpoint)| TrackerConfig {
            url: udp_tracker_label(&endpoint),
            endpoint: TrackerEndpoint::Udp(endpoint),
            tier: 0,
            position: position.try_into().unwrap_or(u32::MAX),
            source: crate::tracker::TrackerSource::Magnet,
        })
        .collect()
}

fn configured_magnet_trackers(trackers: &[TrackerUrl]) -> Vec<TrackerConfig> {
    configured_udp_trackers(
        &trackers
            .iter()
            .filter_map(|tracker| tracker.udp_endpoint().cloned())
            .collect::<Vec<_>>(),
    )
}

async fn run_metadata_peer(
    mut connection: PeerConnection,
    handshake: Handshake,
    cancellation: CancellationToken,
    admission_cancellation: Option<CancellationToken>,
    control: DownloadControl,
    metadata: Arc<Mutex<TorrentMetadataDownload>>,
) -> MetadataPeerResult {
    let attempt = connection.attempt();
    let result = tokio::select! {
        biased;
        _ = cancellation.cancelled() => None,
        _ = async {
            if let Some(cancellation) = &admission_cancellation {
                cancellation.cancelled().await;
            }
        }, if admission_cancellation.is_some() => None,
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

pub(crate) fn random_nonzero_u32() -> Result<u32, DownloadError> {
    let mut bytes = [0; 4];
    getrandom::fill(&mut bytes).map_err(DownloadError::Entropy)?;
    Ok(u32::from_ne_bytes(bytes).max(1))
}

pub(crate) fn compact_peer_address(peer: CompactPeer) -> SocketAddr {
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

pub(crate) async fn announce_udp_tracker(
    tracker: &UdpTrackerUrl,
    network_policy: NetworkPolicy,
    address_families: crate::network::AddressFamilyPolicy,
    token_cache: &mut UdpTrackerTokenCache,
    announce: UdpTrackerAnnounce,
    exchange: UdpTrackerExchange<'_>,
) -> Result<UdpTrackerAnnounceResult, DownloadError> {
    if !network_policy.permits_dns() {
        return Err(DownloadError::NetworkDisabled);
    }
    let addresses = resolve_host(&tracker.host, tracker.port, "resolve UDP tracker").await?;
    let mut last_error = None;
    let mut found_allowed = false;
    for address in addresses {
        if !address_families.permits(address.ip()) || !network_policy.allows(address) {
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
    mut announce: UdpTrackerAnnounce,
    exchange: UdpTrackerExchange<'_>,
) -> Result<UdpTrackerAnnounceResult, DownloadError> {
    let bind_address = match address {
        SocketAddr::V4(_) => exchange
            .source_ipv4
            .filter(IpAddr::is_ipv4)
            .map_or(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)), |source| {
                SocketAddr::new(source, 0)
            }),
        SocketAddr::V6(_) => {
            announce.port = announce.ipv6_port;
            exchange
                .source_ipv6
                .filter(IpAddr::is_ipv6)
                .map_or(SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0)), |source| {
                    SocketAddr::new(source, 0)
                })
        }
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
        peer_id: announce.peer_id,
        downloaded: announce.downloaded,
        left: announce.left,
        uploaded: announce.uploaded,
        event: announce.event,
        ip_address: 0,
        key: announce.key,
        num_want: announce.num_want,
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
    result.map(|response| UdpTrackerAnnounceResult {
        response,
        connection_family: match family {
            TrackerAddressFamily::Ipv4 => TrackerConnectionFamily::Ipv4,
            TrackerAddressFamily::Ipv6 => TrackerConnectionFamily::Ipv6,
        },
    })
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
    configured_trackers: Option<Vec<TrackerConfig>>,
    resources: TorrentPeerResources,
) -> Result<Vec<u8>, DownloadError> {
    let magnet = Magnet::parse(&magnet).map_err(DownloadError::Magnet)?;
    let mut peers = TorrentPeerCoordinator::from_magnet_with_trackers(
        &magnet,
        configured_trackers,
        network,
        control,
        dht,
        resources,
    )
    .await?;
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
    let configured_trackers = config.udp_trackers.clone();
    let torrent_peers = config.torrent_peers.clone();
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
        let mut peers = TorrentPeerCoordinator::from_magnet_with_trackers(
            &magnet,
            configured_trackers,
            config.network,
            control.clone(),
            content_dht,
            TorrentPeerResources {
                peer_budget: config.peer_budget.clone(),
                torrent_peers: torrent_peers.clone(),
                mse_dh: config.mse_dh.clone(),
                encryption: config.encryption.clone(),
            },
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
    let mut peers = TorrentPeerCoordinator::from_magnet_with_trackers(
        &magnet,
        configured_trackers,
        config.network,
        control.clone(),
        dht,
        TorrentPeerResources {
            peer_budget: config.peer_budget.clone(),
            torrent_peers,
            mse_dh: config.mse_dh,
            encryption: config.encryption,
        },
    )
    .await?;
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
    if peer.supports_fast_extension() {
        send_message(peer, &PeerMessage::HaveNone).await?;
    }
    send_message(
        peer,
        &PeerMessage::Extended {
            id: 0,
            payload: encode_extension_handshake(None),
        },
    )
    .await?;

    let fast_extension = NegotiatedPeerCapabilities::negotiate(
        peer_socket::advertised_reserved_bits(true),
        &handshake,
    )
    .fast_extension;
    let mut peer_state = PremetadataPeerState::new(fast_extension);
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
                let recognized = parse_recognized_extension_handshake(&payload)
                    .map_err(|error| DownloadError::Metadata(error.into()))?;
                peer.apply_extension_handshake(recognized);
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
    peer_state: Option<(PeerBudget, TorrentPeerHandle)>,
) -> Result<DownloadReport, DownloadError> {
    let metainfo_bytes = read_bounded_metainfo(&config.metainfo_path).await?;
    let metainfo = Metainfo::from_bytes_with_limits(&metainfo_bytes, BEP9_METAINFO_LIMITS)
        .map_err(DownloadError::Metainfo)?;
    let mut peers = match peer_state {
        Some((peer_budget, torrent_peers)) => {
            let mut peers = TorrentPeerCoordinator::new_with_peer_state(
                config.network,
                control.clone(),
                peer_budget,
                Some(torrent_peers),
                MseDhWorkOwner::new(),
                PeerEncryptionPolicyHandle::new(config.network.encryption),
            )?;
            peers.observe_address(config.peer, PeerSource::Manual)?;
            peers
        }
        None => TorrentPeerCoordinator::from_endpoint_with_control(
            config.peer,
            PeerSource::Manual,
            config.network,
            control.clone(),
        )?,
    };
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
    if peers.owns_peer_sink {
        peers.peers.set_sink(Arc::new(control.clone()));
    }
    peers.publish_peer_registry(true);
    let result =
        run_selective_download(config, metainfo, control, descriptors, peers, resume).await;
    peers.close_current(result.as_ref().err().and_then(content_peer_failure))?;
    result
}

#[cfg(test)]
use storage_pipeline::{
    CHECKPOINT_MAX_DIRTY_BYTES, CONTENT_STORAGE_WRITE_BATCH_BLOCKS,
    CONTENT_STORAGE_WRITE_BATCH_BYTES, CoalescedContentWrite, ContentCheckpointPipeline,
    ContentWriteStats, PreparedContentWrite, QueuedContentStorageCommand, coalesce_content_writes,
    collect_content_write_batch, content_storage_job_limit, execute_content_storage_verification,
    execute_content_storage_writes,
};
use storage_pipeline::{
    ContentStorage, ContentStorageCommand, ContentStorageCompletion, ContentStoragePipeline,
};

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
    selection: FileSelection,
    maximum_planned_bytes: usize,
    max_buffered_payload_bytes: usize,
    selection_revision: u64,
}

struct AppliedFileSelection {
    selection: FileSelection,
    revision: u64,
}

struct ContentDownloadContext<'a> {
    metainfo: &'a Metainfo,
    layout: &'a TorrentLayout,
    resume: Option<&'a ResumeContext>,
    control: &'a DownloadControl,
}

#[cfg(test)]
fn build_content_plan_window(
    layout: &TorrentLayout,
    selection: &FileSelection,
    pieces: &mut std::vec::IntoIter<u32>,
    maximum_pieces: usize,
    maximum_bytes: usize,
    permit_first_over_limit: bool,
) -> Result<(Vec<PiecePlan>, usize, usize), DownloadError> {
    let mut plans = Vec::with_capacity(maximum_pieces);
    let mut total_blocks = 0_usize;
    let mut total_bytes = 0_usize;
    while plans.len() < maximum_pieces {
        let Some(&piece) = pieces.as_slice().first() else {
            break;
        };
        let (plan, block_count, piece_bytes) = build_content_piece_plan(layout, selection, piece)?;
        let next_total_bytes = total_bytes
            .checked_add(piece_bytes)
            .ok_or(DownloadError::Layout(LayoutError::ArithmeticOverflow))?;
        if next_total_bytes > maximum_bytes && (!plans.is_empty() || !permit_first_over_limit) {
            break;
        }
        pieces.next();
        total_blocks = total_blocks
            .checked_add(block_count)
            .ok_or(DownloadError::Layout(LayoutError::ArithmeticOverflow))?;
        total_bytes = next_total_bytes;
        plans.push(plan);
    }
    Ok((plans, total_blocks, total_bytes))
}

fn build_content_piece_plan(
    layout: &TorrentLayout,
    selection: &FileSelection,
    piece: u32,
) -> Result<(PiecePlan, usize, usize), DownloadError> {
    let ranges = layout
        .request_ranges(piece, selection)
        .map_err(DownloadError::Layout)?;
    let block_count = ranges.len();
    let piece_bytes = ranges.iter().try_fold(0_usize, |total, range| {
        total.checked_add(range.length as usize)
    });
    let piece_bytes = piece_bytes.ok_or(DownloadError::Layout(LayoutError::ArithmeticOverflow))?;
    let ranges = ranges
        .into_iter()
        .map(|range| (range.begin, range.length))
        .collect::<Vec<_>>();
    let plan = PiecePlan::new(piece, &ranges).map_err(DownloadError::Swarm)?;
    Ok((plan, block_count, piece_bytes))
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
        wanted_pieces: Vec<u32>,
        picker_seed: u64,
        selection: AppliedFileSelection,
        storage: ContentStorage,
        context: ContentDownloadContext<'a>,
    ) -> Result<Self, DownloadError> {
        let ContentDownloadContext {
            metainfo,
            layout,
            resume,
            control,
        } = context;
        let maximum_planned_bytes = config.max_active_piece_bytes;
        let mut state = SwarmState::new_with_wanted(
            config,
            layout.piece_count(),
            wanted_pieces,
            Vec::new(),
            picker_seed,
        )
        .map_err(DownloadError::Swarm)?;
        state.set_session_resources(control.session_resources());
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
            total_blocks: 0,
            total_bytes: 0,
            selected_written_bytes: 0,
            part_written_bytes: 0,
            last_piece: None,
            contributor_attempts: BTreeMap::new(),
            selection: selection.selection,
            maximum_planned_bytes,
            max_buffered_payload_bytes,
            selection_revision: selection.revision,
        })
    }

    fn is_complete(&self) -> bool {
        self.state.is_complete()
    }

    fn advance_plan_window(&mut self, verified_piece: u32) -> Result<(), DownloadError> {
        self.state
            .retire_verified_piece(verified_piece)
            .map_err(DownloadError::Swarm)?;
        Ok(())
    }

    fn prepare_next_piece(&mut self) -> Result<bool, DownloadError> {
        let Some(piece) = self
            .state
            .reserve_piece_for_planning(MAX_PLANNED_CONTENT_PIECES)
        else {
            return Ok(false);
        };
        let (plan, blocks, bytes) =
            match build_content_piece_plan(self.layout, &self.selection, piece) {
                Ok(plan) => plan,
                Err(error) => {
                    self.state
                        .cancel_piece_planning(piece)
                        .map_err(DownloadError::Swarm)?;
                    return Err(error);
                }
            };
        let planned_bytes = self.state.planned_piece_bytes();
        let fits = planned_bytes
            .checked_add(bytes)
            .is_some_and(|total| total <= self.maximum_planned_bytes);
        if !fits && planned_bytes != 0 {
            self.state
                .cancel_piece_planning(piece)
                .map_err(DownloadError::Swarm)?;
            return Ok(false);
        }
        self.state
            .append_piece_plans(vec![plan])
            .map_err(DownloadError::Swarm)?;
        self.total_blocks = self.total_blocks.saturating_add(blocks);
        self.total_bytes = self.total_bytes.saturating_add(bytes);
        Ok(true)
    }

    fn schedule(&mut self, now: Duration) -> Result<Vec<RequestAssignment>, DownloadError> {
        while self.state.planned_piece_count() < MAX_PLANNED_CONTENT_PIECES {
            if !self.prepare_next_piece()? {
                break;
            }
        }
        self.state.schedule(now).map_err(DownloadError::Swarm)
    }

    async fn handle_message(
        &mut self,
        peers: &mut TorrentPeerCoordinator,
        sockets: &PeerSocketSet,
        connection: ConnectionId,
        message: PeerMessage,
        now: Duration,
    ) -> Result<ContentMessageDisposition, DownloadError> {
        if self
            .state
            .observe_fast_message(connection, &message)
            .is_err()
        {
            return Ok(ContentMessageDisposition::ClosePeer(PeerFailure::Protocol));
        }
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
                    validated_compact_availability(bitfield, self.layout.piece_count())
                else {
                    return Ok(ContentMessageDisposition::ClosePeer(PeerFailure::Protocol));
                };
                self.state
                    .set_compact_bitfield(connection, availability)
                    .map_err(DownloadError::Swarm)?;
            }
            PeerMessage::HaveAll => {
                self.state
                    .set_have_all(connection)
                    .map_err(DownloadError::Swarm)?;
            }
            PeerMessage::HaveNone => {
                self.state
                    .set_have_none(connection)
                    .map_err(DownloadError::Swarm)?;
            }
            PeerMessage::RejectRequest(request) => {
                let Ok(block) = BlockKey::new(request.index, request.begin, request.length) else {
                    return Ok(ContentMessageDisposition::ClosePeer(PeerFailure::Protocol));
                };
                match self
                    .state
                    .reject_request(connection, block)
                    .map_err(DownloadError::Swarm)?
                {
                    RejectDisposition::Accepted { .. } | RejectDisposition::Stale => {}
                    RejectDisposition::NeverRequested => {
                        return Ok(ContentMessageDisposition::ClosePeer(PeerFailure::Protocol));
                    }
                }
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
                    ReceiveDisposition::Redundant => {
                        self.control
                            .record_bytes(ByteMetric::PayloadRedundant, block.len());
                        self.control
                            .record_bytes(ByteMetric::PeerUnclassifiedReceived, block.len());
                    }
                    ReceiveDisposition::Unsolicited => {
                        self.control
                            .record_bytes(ByteMetric::PayloadRedundant, block.len());
                        self.control
                            .record_bytes(ByteMetric::PeerUnclassifiedReceived, block.len());
                        if self
                            .state
                            .fast_peer_snapshot(connection)
                            .map_err(DownloadError::Swarm)?
                            .negotiated
                        {
                            return Ok(ContentMessageDisposition::ClosePeer(PeerFailure::Protocol));
                        }
                    }
                }
            }
            PeerMessage::Extended { id: 0, payload } => {
                if peers
                    .apply_extension_handshake(connection, &payload)
                    .is_err()
                {
                    return Ok(ContentMessageDisposition::ClosePeer(PeerFailure::Protocol));
                }
            }
            PeerMessage::Extended {
                id: UT_PEX_LOCAL_ID,
                payload,
            } => match peers.receive_pex(connection, &payload, !self.metainfo.private) {
                Ok(PexReceiveDisposition::RateLimited { close: true, .. }) | Err(_) => {
                    return Ok(ContentMessageDisposition::ClosePeer(PeerFailure::Protocol));
                }
                Ok(_) => {}
            },
            PeerMessage::KeepAlive
            | PeerMessage::Interested
            | PeerMessage::NotInterested
            | PeerMessage::Request(_)
            | PeerMessage::Cancel(_)
            | PeerMessage::Extended { .. } => {}
            PeerMessage::SuggestPiece(_) | PeerMessage::AllowedFast(_) => {}
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
                            .enqueue_checkpoint(
                                piece_index,
                                length,
                                verification.durability_targets,
                            )
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
                    self.advance_plan_window(piece)?;
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

    async fn restart_storage(&mut self, storage: ContentStorage) -> Result<(), DownloadError> {
        let checkpoints = self.resume.map(|resume| resume.checkpoints.clone());
        self.storage_pipeline = Some(
            ContentStoragePipeline::start(
                storage,
                self.control,
                self.max_buffered_payload_bytes,
                checkpoints,
            )
            .await?,
        );
        self.completed_storage = None;
        Ok(())
    }

    async fn reconcile_file_selection(
        &mut self,
        sockets: &PeerSocketSet,
        update: FileSelectionUpdate,
    ) -> Result<(), DownloadError> {
        if update.revision <= self.selection_revision {
            return Ok(());
        }
        let next_selection =
            FileSelection::new(self.layout, &update.skip_files).map_err(DownloadError::Layout)?;
        self.stop_storage(false).await?;
        let mut storage = self.take_storage()?;
        let reconcile = match storage.0.reconcile_selection(next_selection.clone()).await {
            Ok(reconcile) => reconcile,
            Err(error) => {
                self.restart_storage(storage).await?;
                return Err(DownloadError::SelectiveStorage(error));
            }
        };
        if !reconcile.invalidated_pieces.is_empty()
            && let Some(resume) = self.resume
        {
            resume
                .checkpoints
                .pieces_invalidated(&reconcile.invalidated_pieces)
                .map_err(DownloadError::Checkpoint)?;
        }
        let mut wanted = Vec::new();
        for piece_index in 0..self.layout.piece_count() {
            let piece_index_u32 = match u32::try_from(piece_index) {
                Ok(piece_index) => piece_index,
                Err(_) => {
                    self.restart_storage(storage).await?;
                    return Err(DownloadError::Layout(LayoutError::ArithmeticOverflow));
                }
            };
            let ranges = match self.layout.request_ranges(piece_index_u32, &next_selection) {
                Ok(ranges) => ranges,
                Err(error) => {
                    self.restart_storage(storage).await?;
                    return Err(DownloadError::Layout(error));
                }
            };
            if !ranges.is_empty() && !storage.0.verified_pieces()[piece_index] {
                wanted.push(piece_index_u32);
            }
        }
        let cancellations = match self.state.replace_wanted_pieces(wanted) {
            Ok(cancellations) => cancellations,
            Err(error) => {
                self.restart_storage(storage).await?;
                return Err(DownloadError::Swarm(error));
            }
        };
        for cancellation in cancellations {
            let _ = sockets
                .send(
                    cancellation.connection,
                    PeerMessage::Cancel(cancellation.block.request()),
                )
                .await;
        }
        self.control.clear_outstanding_requests();
        self.contributor_attempts.clear();
        self.selection = next_selection;
        self.selection_revision = update.revision;
        self.control.file_selection_applied(update.revision);
        self.restart_storage(storage).await
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
        match peers
            .peers
            .with_state(|state| state.registry.record_piece_passed(attempt))
        {
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
        match peers
            .peers
            .with_state(|state| state.registry.record_piece_failed(attempt, known_bad))
        {
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
            .peers
            .with_state(|state| state.registry.ban(attempt.record_id()))
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

fn validated_compact_availability(bitfield: Vec<u8>, piece_count: usize) -> Option<Vec<u8>> {
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
    Some(bitfield)
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
        if !peers.control.try_acquire_outbound_turn() {
            break;
        }
        let context = PeerSelectionContext {
            now: peers.elapsed(),
        };
        let Some(candidate) = peers.select_candidate(context) else {
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
            true,
            peers.connection_network(),
            peers.control.byte_metric_sink(),
            peers.control.mse_handshake_sink(),
        ) {
            state
                .finish_dial(pending_dial_id(attempt))
                .map_err(DownloadError::Swarm)?;
            peers.dial_cancelled(attempt)?;
            if matches!(error, PeerSetError::ConnectionLimit(_)) {
                break;
            }
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

async fn send_due_pex(
    peers: &mut TorrentPeerCoordinator,
    sockets: &PeerSocketSet,
) -> Result<Vec<ConnectionId>, DownloadError> {
    let mut failed = Vec::new();
    for attempt in sockets.connection_attempts() {
        let connection = connection_id(attempt);
        let Some((remote_id, payload)) = peers.next_pex(connection)? else {
            continue;
        };
        if sockets
            .send(
                connection,
                PeerMessage::Extended {
                    id: remote_id,
                    payload,
                },
            )
            .await
            .is_err()
        {
            failed.push(connection);
        }
    }
    Ok(failed)
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

// This stack-local event is consumed immediately; boxing every peer event would
// add a heap allocation to the peer hot path solely to equalize enum variants.
#[allow(clippy::large_enum_variant)]
enum ContentSupervisorEvent {
    Peer(PeerSetEvent),
    Discovery(Option<ContentDiscoveryEvent>),
    Storage(ContentStorageCompletion),
    Selection(FileSelectionUpdate),
    Deadline,
}

struct ContentSupervisorWait<'a> {
    storage_backpressured: bool,
    until_expiry: Option<Duration>,
    cancellation: &'a CancellationToken,
    selection_updates: &'a mut watch::Receiver<Option<FileSelectionUpdate>>,
    priority: ContentSupervisorOwner,
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
            Self::Selection(_) | Self::Deadline => None,
        }
    }
}

async fn next_content_supervisor_event(
    sockets: &mut PeerSocketSet,
    discovery: &mut ContentDiscovery,
    storage: &mut ContentStoragePipeline,
    wait: ContentSupervisorWait<'_>,
) -> Result<ContentSupervisorEvent, DownloadError> {
    let ContentSupervisorWait {
        storage_backpressured,
        until_expiry,
        cancellation,
        selection_updates,
        priority,
    } = wait;
    if selection_updates.has_changed().map_err(|_| {
        DownloadError::StorageTask("file-selection controller stopped unexpectedly".to_owned())
    })? {
        let update = selection_updates
            .borrow_and_update()
            .clone()
            .ok_or_else(|| {
                DownloadError::StorageTask("file-selection update is missing".to_owned())
            })?;
        return Ok(ContentSupervisorEvent::Selection(update));
    }
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
                _ = tokio::time::sleep(CONTENT_SWARM_MAINTENANCE_INTERVAL) => {
                    Ok(ContentSupervisorEvent::Deadline)
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
                _ = tokio::time::sleep(CONTENT_SWARM_MAINTENANCE_INTERVAL) => {
                    Ok(ContentSupervisorEvent::Deadline)
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
            _ = tokio::time::sleep(CONTENT_SWARM_MAINTENANCE_INTERVAL) => {
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
            _ = tokio::time::sleep(CONTENT_SWARM_MAINTENANCE_INTERVAL) => {
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
            _ = tokio::time::sleep(CONTENT_SWARM_MAINTENANCE_INTERVAL) => {
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
    let mut selection_updates = download.control.selection_updates();
    let mut storage_pressure_started = None;
    let mut next_maintenance_at = Duration::ZERO;
    if let Some(connection) = peers.connection.take() {
        let attempt = connection.attempt();
        let fast_extension = connection.supports_fast_extension();
        let extension_map = connection.extension_map();
        peers.handoff_to_content(attempt)?;
        let id = sockets
            .add_connection(connection)
            .map_err(download_peer_set_error)?;
        download
            .state
            .add_connection(id, peers.elapsed())
            .map_err(DownloadError::Swarm)?;
        peers.install_extension_map(id, extension_map);
        if !download.metainfo.private
            && sockets
                .send(
                    id,
                    PeerMessage::Extended {
                        id: 0,
                        payload: public_pex_extension_handshake(),
                    },
                )
                .await
                .is_err()
        {
            close_content_connection(
                peers,
                sockets,
                &mut download.state,
                id,
                Some(PeerFailure::RemoteClosed),
            )
            .await?;
        }
        download
            .state
            .set_fast_extension(id, fast_extension)
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
        let address_families = peers.peers.address_family_policy();
        sockets.cancel_disallowed(address_families);
        peers
            .peers
            .enforce_address_families(address_families)
            .map_err(map_torrent_peer_error)?;
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
            for connection in send_due_pex(peers, sockets).await? {
                close_content_connection(
                    peers,
                    sockets,
                    &mut download.state,
                    connection,
                    Some(PeerFailure::RemoteClosed),
                )
                .await?;
            }
            next_maintenance_at = now.saturating_add(CONTENT_SWARM_MAINTENANCE_INTERVAL);
        }

        let assignments = if storage_ready && !storage_backpressured {
            download.schedule(now)?
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
        let cancellation = peers.control.cancellation_token();
        let event = {
            let storage = download.storage_pipeline_mut()?;
            next_content_supervisor_event(
                sockets,
                discovery,
                storage,
                ContentSupervisorWait {
                    storage_backpressured,
                    until_expiry,
                    cancellation: &cancellation,
                    selection_updates: &mut selection_updates,
                    priority: next_owner,
                },
            )
            .await?
        };
        if let Some(owner) = event.owner() {
            next_owner = owner.next();
        }

        match event {
            ContentSupervisorEvent::Deadline => continue,
            ContentSupervisorEvent::Selection(update) => {
                while download.control.snapshot().storage_jobs_pending != 0 {
                    download.flush_pending_storage()?;
                    let completion = download.storage_pipeline_mut()?.next_completion().await?;
                    let disposition = download
                        .handle_storage_completion(completion, peers.elapsed())
                        .await?;
                    apply_content_disposition(peers, sockets, download, None, disposition).await?;
                }
                download.reconcile_file_selection(sockets, update).await?;
            }
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
                        .select_candidate(PeerSelectionContext {
                            now: peers.elapsed(),
                        })
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
                match *result {
                    Ok((connection, handshake)) => {
                        let admission = peers.dial_succeeded(attempt, &connection, &handshake)?;
                        if let Some(failure) = admission_rejection_failure(admission) {
                            peers.connection_closed(attempt, Some(failure))?;
                            continue;
                        }
                        if let PeerAdmissionOutcome::Admitted {
                            evicted: Some(evicted),
                        } = admission
                            && sockets.contains(evicted)
                        {
                            close_content_connection(
                                peers,
                                sockets,
                                &mut download.state,
                                evicted,
                                Some(PeerFailure::DuplicatePeerId),
                            )
                            .await?;
                        }
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
                                peers.begin_disconnect(attempt, None)?;
                                peers.connection_closed(attempt, None)?;
                                continue;
                            }
                        }
                        let id = sockets
                            .add_connection(connection)
                            .map_err(download_peer_set_error)?;
                        download
                            .state
                            .add_connection(id, peers.elapsed())
                            .map_err(DownloadError::Swarm)?;
                        let capabilities = NegotiatedPeerCapabilities::negotiate(
                            peer_socket::advertised_reserved_bits(true),
                            &handshake,
                        );
                        download
                            .state
                            .set_fast_extension(id, capabilities.fast_extension)
                            .map_err(DownloadError::Swarm)?;
                        if handshake.supports_extensions()
                            && !download.metainfo.private
                            && sockets
                                .send(
                                    id,
                                    PeerMessage::Extended {
                                        id: 0,
                                        payload: public_pex_extension_handshake(),
                                    },
                                )
                                .await
                                .is_err()
                        {
                            close_content_connection(
                                peers,
                                sockets,
                                &mut download.state,
                                id,
                                Some(PeerFailure::RemoteClosed),
                            )
                            .await?;
                            continue;
                        }
                        if capabilities.fast_extension
                            && sockets.send(id, PeerMessage::HaveNone).await.is_err()
                        {
                            close_content_connection(
                                peers,
                                sockets,
                                &mut download.state,
                                id,
                                Some(PeerFailure::RemoteClosed),
                            )
                            .await?;
                            continue;
                        }
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
                        if let Some(endpoint) = error.mse_endpoint_update() {
                            peers.update_mse_endpoint(attempt, endpoint)?;
                        }
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
                    .handle_message(peers, sockets, id, message, peers.elapsed())
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
    let mut sockets = PeerSocketSet::with_owners(peers.peer_budget.clone(), peers.mse_dh.clone());
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

async fn wait_for_checking_resume(
    control: &DownloadControl,
    paused: &mut watch::Receiver<bool>,
) -> Result<(), DownloadError> {
    loop {
        let pause_requested = *paused.borrow_and_update();
        if !pause_requested {
            return Ok(());
        }
        control.checker_set_phase(CheckerPhase::Paused);
        tokio::select! {
            _ = control.cancelled() => return Err(DownloadError::Cancelled),
            changed = paused.changed() => {
                if changed.is_err() {
                    return Err(DownloadError::Cancelled);
                }
            }
        }
    }
}

async fn full_recheck_managed_storage(
    storage: &mut SelectiveStorage,
    metainfo: &Metainfo,
    layout: &TorrentLayout,
    previous: &[bool],
    selection: &mut AppliedFileSelection,
    control: &DownloadControl,
) -> Result<FullRecheckResult, DownloadError> {
    let mut verified = vec![false; layout.piece_count()];
    for piece_index in 0..layout.piece_count() {
        storage
            .set_verified(piece_index, false)
            .map_err(DownloadError::SelectiveStorage)?;
    }

    let hash_concurrency = control.storage_execution_limits().1;
    let mut running = JoinSet::new();
    let mut recovered = Vec::new();
    let mut cancelled = false;
    let mut first_error = None;
    let mut next_piece_index = 0_usize;
    let mut pause_updates = control.checking_pause_updates();
    let mut heartbeat = tokio::time::interval_at(
        TokioInstant::now() + Duration::from_secs(1),
        Duration::from_secs(1),
    );
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    control.checker_set_phase(CheckerPhase::Hashing);

    loop {
        cancelled |= control.is_cancelled();
        let pause_requested = *pause_updates.borrow_and_update();
        let pending_selection = control
            .latest_file_selection()
            .filter(|update| update.revision > selection.revision);
        if pending_selection.is_some() {
            control.checker_set_phase(CheckerPhase::ReconcilingStorage);
        }
        if running.is_empty()
            && let Some(update) = pending_selection.as_ref()
        {
            let next_selection =
                FileSelection::new(layout, &update.skip_files).map_err(DownloadError::Layout)?;
            let reconcile = storage
                .reconcile_selection(next_selection.clone())
                .await
                .map_err(DownloadError::SelectiveStorage)?;
            for piece_index in reconcile.invalidated_pieces {
                verified[piece_index] = false;
                let piece_index = u32::try_from(piece_index)
                    .map_err(|_| DownloadError::Layout(LayoutError::ArithmeticOverflow))?;
                recovered.retain(|recovered| *recovered != piece_index);
            }
            selection.selection = next_selection;
            selection.revision = update.revision;
            control.file_selection_applied(update.revision);
            control.checker_set_phase(CheckerPhase::Hashing);
            continue;
        }
        if running.is_empty() && pause_requested {
            wait_for_checking_resume(control, &mut pause_updates).await?;
            control.checker_set_phase(CheckerPhase::Hashing);
            continue;
        }
        while !cancelled
            && first_error.is_none()
            && pending_selection.is_none()
            && !pause_requested
            && running.len() < hash_concurrency
        {
            if next_piece_index == layout.piece_count() {
                break;
            }
            let piece_index = u32::try_from(next_piece_index)
                .map_err(|_| DownloadError::Layout(LayoutError::ArithmeticOverflow))?;
            next_piece_index += 1;
            let piece_length = layout
                .piece_length_at(piece_index)
                .map_err(DownloadError::Layout)?;
            let bytes_hashed = layout
                .file_segments(piece_index, 0, piece_length)
                .map_err(DownloadError::Layout)?
                .into_iter()
                .filter(|segment| !segment.padding)
                .try_fold(0_u64, |total, segment| {
                    total
                        .checked_add(segment.length as u64)
                        .ok_or(DownloadError::Layout(LayoutError::ArithmeticOverflow))
                })?;
            let available = match storage.has_piece_sources(piece_index).await {
                Ok(available) => available,
                Err(error) => {
                    control.disk_storage_error(&error.to_string());
                    first_error = Some(DownloadError::SelectiveStorage(error));
                    break;
                }
            };
            if !available {
                control.checker_piece_processed(piece_index, 0, CheckerPieceOutcome::Absent);
                continue;
            }
            let operation = match storage.prepare_hash(piece_index) {
                Ok(operation) => operation,
                Err(error) if error.is_missing_or_short_source() => {
                    control.checker_piece_processed(piece_index, 0, CheckerPieceOutcome::Absent);
                    continue;
                }
                Err(error) => {
                    control.disk_storage_error(&error.to_string());
                    first_error = Some(DownloadError::SelectiveStorage(error));
                    break;
                }
            };
            control.disk_piece_hashing(piece_index, piece_length);
            control.checker_hash_started(piece_index);
            control.emit(DownloadActivityEvent::PieceHashing { piece_index });
            let started_at = Instant::now();
            control.storage_command_started(StorageCommandKind::Hash, started_at, started_at);
            let job_control = control.clone();
            running.spawn(async move {
                let _session_permit = job_control.wait_before_storage_hash().await;
                (
                    piece_index,
                    piece_length,
                    bytes_hashed,
                    started_at,
                    operation.execute().await,
                )
            });
        }

        if running.is_empty() {
            break;
        }
        let result = tokio::select! {
            result = running.join_next() => result,
            _ = heartbeat.tick() => {
                control.checker_heartbeat();
                continue;
            }
        };
        let Some(result) = result else {
            break;
        };
        match result {
            Ok((piece_index, piece_length, bytes_hashed, started_at, result)) => {
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
                let outcome = match result {
                    Ok(actual) if actual == metainfo.piece_hashes[piece_index_usize] => {
                        CheckerPieceOutcome::Matched
                    }
                    Ok(_) => CheckerPieceOutcome::Mismatched,
                    Err(error) if error.is_missing_or_short_source() => CheckerPieceOutcome::Absent,
                    Err(error) => {
                        control.disk_piece_failed(piece_index, piece_length, &error.to_string());
                        control.checker_hash_stopped(piece_index);
                        first_error.get_or_insert(DownloadError::SelectiveStorage(error));
                        continue;
                    }
                };
                control.checker_piece_processed(piece_index, bytes_hashed, outcome);
                if outcome != CheckerPieceOutcome::Matched {
                    control.disk_piece_check_unverified(piece_index, piece_length);
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
        control.checker_set_phase(CheckerPhase::Paused);
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

    let mut wanted_pieces = Vec::new();
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
            wanted_pieces.push(piece_index_u32);
        }
    }
    let mut last_wanted_piece = wanted_pieces
        .last()
        .copied()
        .map_or(Ok(0), usize::try_from)
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
            selection.clone(),
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
                    selection.clone(),
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
                            selection.clone(),
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
                            selection.clone(),
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
                            selection.clone(),
                            pool,
                        )
                        .await
                    }
                    None => {
                        SelectiveStorage::create(
                            config.output_path.clone(),
                            &metainfo,
                            layout.clone(),
                            selection.clone(),
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
                        selection.clone(),
                        &[],
                        descriptors,
                    )
                    .await
                    .map_err(DownloadError::SelectiveStorage)?
                } else {
                    SelectiveStorage::resume_with_descriptors(
                        &metainfo,
                        layout.clone(),
                        selection.clone(),
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

    let mut applied_selection = AppliedFileSelection {
        selection,
        revision: 0,
    };
    if let (Some(resume), Some(resumed)) = (&resume, resumed_storage) {
        let generation = resume
            .checkpoints
            .recheck_started()
            .map_err(DownloadError::Checkpoint)?;
        control.checker_started(generation, layout.piece_count());
        let previous = verified_pieces.clone();
        let checked = if resumed == ResumedStorage::Created {
            let mut pause_updates = control.checking_pause_updates();
            control.checker_set_phase(CheckerPhase::Hashing);
            for piece_index in 0..layout.piece_count() {
                wait_for_checking_resume(&control, &mut pause_updates).await?;
                control.checker_set_phase(CheckerPhase::Hashing);
                if control.is_cancelled() {
                    control.checker_set_phase(CheckerPhase::Paused);
                    return Err(DownloadError::Cancelled);
                }
                let piece_index = u32::try_from(piece_index)
                    .map_err(|_| DownloadError::Layout(LayoutError::ArithmeticOverflow))?;
                control.checker_piece_processed(piece_index, 0, CheckerPieceOutcome::Absent);
            }
            FullRecheckResult {
                verified: vec![false; layout.piece_count()],
                recovered: Vec::new(),
            }
        } else {
            match full_recheck_managed_storage(
                &mut storage,
                &metainfo,
                &layout,
                &previous,
                &mut applied_selection,
                &control,
            )
            .await
            {
                Ok(checked) => checked,
                Err(error) => {
                    if !matches!(error, DownloadError::Cancelled) {
                        control.checker_finished(generation);
                    }
                    return Err(error);
                }
            }
        };
        let mut pause_updates = control.checking_pause_updates();
        wait_for_checking_resume(&control, &mut pause_updates).await?;
        control.checker_set_phase(CheckerPhase::ReconcilingStorage);
        verified_pieces = checked.verified;
        if let Err(error) = storage.reconcile_after_recheck().await {
            control.checker_finished(generation);
            return Err(DownloadError::SelectiveStorage(error));
        }
        if resumed == ResumedStorage::Staging
            && let Err(error) = storage.sync_pieces(&checked.recovered).await
        {
            control.checker_finished(generation);
            return Err(DownloadError::SelectiveStorage(error));
        }
        wait_for_checking_resume(&control, &mut pause_updates).await?;
        control.checker_set_phase(CheckerPhase::Finalizing);
        if let Err(error) = resume.checkpoints.have_rechecked(&verified_pieces) {
            control.checker_finished(generation);
            return Err(DownloadError::Checkpoint(error));
        }
        control.checker_finished(generation);
        wait_for_checking_resume(&control, &mut pause_updates).await?;
    }

    let AppliedFileSelection {
        mut selection,
        revision: mut selection_revision,
    } = applied_selection;

    if let Some(update) = control
        .latest_file_selection()
        .filter(|update| update.revision > selection_revision)
    {
        let next_selection =
            FileSelection::new(&layout, &update.skip_files).map_err(DownloadError::Layout)?;
        let reconcile = storage
            .reconcile_selection(next_selection.clone())
            .await
            .map_err(DownloadError::SelectiveStorage)?;
        if !reconcile.invalidated_pieces.is_empty() {
            if let Some(resume) = &resume {
                resume
                    .checkpoints
                    .pieces_invalidated(&reconcile.invalidated_pieces)
                    .map_err(DownloadError::Checkpoint)?;
            }
            for piece_index in reconcile.invalidated_pieces {
                verified_pieces[piece_index] = false;
            }
        }
        selection = next_selection;
        selection_revision = update.revision;
        control.file_selection_applied(update.revision);
        wanted_pieces.clear();
        skipped_piece_count = 0;
        for piece_index in 0..layout.piece_count() {
            let piece_index_u32 = u32::try_from(piece_index)
                .map_err(|_| DownloadError::Layout(LayoutError::ArithmeticOverflow))?;
            if layout
                .request_ranges(piece_index_u32, &selection)
                .map_err(DownloadError::Layout)?
                .is_empty()
            {
                skipped_piece_count += 1;
            } else {
                wanted_pieces.push(piece_index_u32);
            }
        }
        last_wanted_piece = wanted_pieces
            .last()
            .copied()
            .map_or(Ok(0), usize::try_from)
            .map_err(|_| DownloadError::Layout(LayoutError::ArithmeticOverflow))?;
    }

    wanted_pieces.retain(|piece_index| {
        usize::try_from(*piece_index)
            .ok()
            .and_then(|piece_index| verified_pieces.get(piece_index))
            .is_none_or(|verified| !*verified)
    });
    let selected_file_bytes = storage.selected_bytes();
    let skipped_file_bytes = storage.skipped_bytes();
    let padding_bytes = storage.padding_bytes();
    let part_path = storage.part_path().map(Path::to_path_buf);
    let plan_selection = selection.clone();
    if resume
        .as_ref()
        .is_some_and(|resume| !resume.download_missing)
        && !wanted_pieces.is_empty()
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
    ) = if wanted_pieces.is_empty() {
        (0, 0, 0, 0, 0)
    } else {
        let download = ContentSwarmDownload::new(
            config.swarm_config,
            config.max_buffered_payload_bytes,
            wanted_pieces,
            picker_seed(metainfo.info_hash, peers.network.peer_id),
            AppliedFileSelection {
                selection: plan_selection,
                revision: selection_revision,
            },
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
        selection = completed.selection.clone();
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

    skipped_piece_count = 0;
    last_wanted_piece = 0;
    for piece_index in 0..layout.piece_count() {
        let piece_index_u32 = u32::try_from(piece_index)
            .map_err(|_| DownloadError::Layout(LayoutError::ArithmeticOverflow))?;
        if layout
            .request_ranges(piece_index_u32, &selection)
            .map_err(DownloadError::Layout)?
            .is_empty()
        {
            skipped_piece_count += 1;
        } else {
            last_wanted_piece = piece_index;
        }
    }
    let selected_file_bytes = storage.selected_bytes();
    let skipped_file_bytes = storage.skipped_bytes();
    let padding_bytes = storage.padding_bytes();
    let part_path = storage.part_path().map(Path::to_path_buf);
    if selected_file_bytes == 0 {
        let part_slots = storage.part_slots();
        return Ok(DownloadReport {
            info_hash: metainfo.info_hash,
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
            verified_piece_count: 0,
            skipped_piece_count,
            selected_file_bytes,
            skipped_file_bytes,
            padding_bytes,
            selected_written_bytes,
            part_written_bytes,
            materialized_bytes: 0,
            part_slots_before_materialization: part_slots,
            part_slots_after_materialization: part_slots,
            part_reopened: storage.has_part_file(),
            part_path,
            prepared_files: Vec::new(),
        });
    }

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
        error @ (PeerSocketError::MseHandshake(_)
        | PeerSocketError::MseDh(_)
        | PeerSocketError::Entropy(_)) => DownloadError::PeerTask(error.to_string()),
        PeerSocketError::MseEndpointUpdate { source, .. } => download_peer_socket_error(*source),
        PeerSocketError::Frame(error) => DownloadError::Frame(error),
    }
}

fn download_peer_set_error(error: PeerSetError) -> DownloadError {
    DownloadError::PeerTask(error.to_string())
}

#[cfg(test)]
mod tests;
