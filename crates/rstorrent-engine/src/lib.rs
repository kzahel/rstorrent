#![forbid(unsafe_code)]

//! Runtime ownership for the first verified-piece diagnostic.

mod advertisement;
mod artifact_layout;
mod checkpoint;
pub mod dht;
mod driver;
mod http_tracker;
mod incoming;
mod metadata_seed;
mod metrics;
mod mse;
mod namespace_transition;
mod network;
mod part_file;
pub mod peer;
mod peer_budget;
mod peer_io;
mod peer_runtime;
mod peer_socket;
mod pex;
mod piece_picker;
pub mod port_mapping;
mod positional_io;
mod resume_validation;
mod seed_content;
mod selective_storage;
mod session_resources;
mod session_socket;
mod session_udp;
mod storage_file_pool;
pub mod swarm;
mod torrent_peer;
mod tracker;
mod upload;
mod upload_scheduler;

pub use advertisement::{
    DHT_ANNOUNCE_INTERVAL, DHT_LOOKUP_INTERVAL, DISCOVERY_ADVERTISEMENT_COMMAND_CAPACITY,
    DiscoveryAdvertisementError, DiscoveryAdvertisementHandle, DiscoveryAdvertisementOwnerCounts,
    DiscoveryAdvertisementRegistration, DiscoveryAdvertisementRuntimeSnapshot,
    DiscoveryAdvertisementService, MAX_TRACKER_OPERATIONS, OUTBOUND_ONLY_TRACKER_PORT,
    PeerAdvertisementEndpoint, PeerAdvertisementEndpointScope, PeerAdvertisementFamilyEndpoint,
    TRACKER_STOP_TIMEOUT, TorrentPrivacy, TrackerCounterSnapshot, TrackerCounters,
    UNKNOWN_METADATA_LEFT_BYTES,
};
pub use artifact_layout::{
    ArtifactLayoutError, LogicalPayloadArtifact, PublicationShape, PublishedArtifactLayout,
};
pub use driver::{
    CheckerPhase, CheckerProgress, ContentPeerActivitySnapshot, ContentRequestWindowPhase,
    DiskCheckpointStage, DiskPieceRuntimeSnapshot, DiskPieceStage, DiskPressure,
    DiskRuntimeSnapshot, DownloadActivityEvent, DownloadActivitySink, DownloadCheckpointSink,
    DownloadConfig, DownloadControl, DownloadDiagnosticSnapshot, DownloadError, DownloadProgress,
    DownloadReport, DownloadResourceLimits, FileSelectionUpdate, MagnetDownloadConfig,
    MetadataAcquisitionPhase, MetadataAcquisitionSnapshot, MetadataPeerSnapshot, MetadataPeerStage,
    PathPublicationStage, ResumableMagnetDownloadConfig, SwarmActivitySnapshot, download_magnet,
    download_magnet_metadata_with_control, download_magnet_metadata_with_dht,
    download_magnet_metadata_with_dht_and_peers, download_magnet_metadata_with_external_discovery,
    download_magnet_with_control, download_verified_piece, download_verified_piece_with_control,
    download_verified_piece_with_peer_state, resume_magnet, resume_magnet_with_control,
};
#[cfg(feature = "descriptor-storage-diagnostics")]
#[doc(hidden)]
pub use driver::{
    download_verified_piece_to_descriptors_with_control, resume_magnet_to_descriptors_with_control,
};
#[cfg(feature = "test-platform-root")]
#[doc(hidden)]
pub use http_tracker::install_test_platform_root;
pub use incoming::{
    DEFAULT_INCOMING_HANDSHAKE_TIMEOUT, DEFAULT_INCOMING_INACTIVITY_TIMEOUT,
    DEFAULT_INCOMING_KEEPALIVE_INTERVAL, DEFAULT_INCOMING_NO_REQUEST_TIMEOUT,
    DEFAULT_INCOMING_PEER_ACTIVITY_TIMEOUT, DEFAULT_UPLOAD_READ_JOBS,
    INCOMING_WRITER_NO_PROGRESS_TIMEOUT, IncomingPeerAcceptor, IncomingPeerError,
    IncomingPeerHandle, IncomingPeerRuntime, IncomingPeerService, IncomingPeerServiceConfig,
    IncomingPeerServiceSnapshot, IncomingRejection, IncomingRejectionReason, IncomingTcpBootstrap,
    MAX_CONFIGURED_UPLOAD_READ_JOBS, MAX_DEFERRED_METADATA_REQUESTS, MAX_INCOMING_PENDING,
    MAX_INCOMING_WRITER_BYTES, MAX_SEED_REGISTRATIONS, METADATA_SEND_BUFFER_WATERMARK,
    PeerUploadSnapshot, SeedRegistration, SeedRegistrationToken, TorrentUploadSnapshot,
    UploadTrafficSnapshot,
};
pub use metadata_seed::{
    MetadataSeedConfig, MetadataSeedError, MetadataSeedReport, MetadataSeedServer,
    bind_metadata_seed,
};
pub use metrics::{ByteMetric, ByteMetricSink};
pub use mse::{
    MAX_MSE_DH_JOBS, MseDhWorkError, MseDhWorkOwner, MseDhWorkSnapshot, MseHandshakeFailure,
    MseHandshakeObservation, MseHandshakeOutcome, MseHandshakeSink,
};
pub use namespace_transition::{
    NamespaceAction, NamespaceDisposition, NamespaceState, NamespaceTransitionError,
    NamespaceTransitionInput, NamespaceTransitionOutcome, decide_namespace_transition,
};
pub use network::{
    AddressFamily, AddressFamilyPolicy, AddressFamilyPolicyHandle, DEFAULT_PEER_ID, NetworkConfig,
    NetworkPolicy, PeerEncryptionPolicy, PeerEncryptionPolicyHandle,
};
pub use part_file::{PartFile, PartFileError, PartFileIdentity};
pub use peer_budget::{
    DEFAULT_CONNECTION_LIMIT, DEFAULT_INCOMING_CONNECTION_SLACK, DEFAULT_LISTEN_BACKLOG,
    PeerBudget, PeerBudgetConfig, PeerBudgetDirection, PeerBudgetPermit, PeerBudgetPhase,
    PeerBudgetReconfiguration, PeerBudgetRejection, PeerBudgetSnapshot, effective_connection_limit,
};
pub use peer_runtime::{
    PeerConnectionDirection, PeerConnectionLifecycle, PeerConnectionObservation,
    PeerConnectionRole, PeerContentActivity, PeerRequestWindowPhase, PeerRuntimeError,
    PeerTransport, PeerUploadActivity, PeerUploadGrant,
};
pub use pex::PexError;
pub use piece_picker::PieceActivationPolicy;
pub use resume_validation::{
    ResumeAdmissionOutcome, ResumeStorageEvidence, ResumeValidationIntent,
    ResumeValidationRejectReason, decide_resume_admission,
};
pub use seed_content::{SeedContent, SeedContentError, SeedContentSnapshot};
#[cfg(feature = "descriptor-storage-diagnostics")]
#[doc(hidden)]
pub use selective_storage::{
    DescriptorFile, DescriptorFileRole, DescriptorStorage, DescriptorStoragePlan,
    DescriptorStoragePlanFile, plan_descriptor_storage, verify_prepared_descriptors,
};
pub use selective_storage::{
    MaterializationReport, PlatformStorageSpec, PreparedFileHash, ResumeArtifactState,
    ResumedStorage, SelectionReconcileReport, SelectiveStorage, SelectiveStorageError,
    SelectiveWriteStats, TorrentStoragePaths, remove_selective_part_if_present,
    remove_selective_staging_if_present, selective_part_path, selective_staging_path,
    torrent_storage_paths, torrent_storage_paths_for_metainfo, torrent_storage_paths_with_shape,
    validate_publication_name, verify_prepared_platform_files,
};
pub use session_resources::{
    SessionDownloadResourceSnapshot, SessionDownloadResources, SessionStorageRootResourceSnapshot,
    SessionTorrentResources,
};
pub use session_socket::{
    MAX_LISTEN_PORT_RETRIES, SessionSocketConfig, SessionSocketError, SessionSocketFamilySet,
    SessionSocketFamilyState, SessionSocketSet, SessionSocketTransport, eligible_global_ipv6,
    select_global_ipv6,
};
pub use session_udp::{
    SESSION_UDP_DHT_QUEUE, SessionUdpError, SessionUdpHandle, SessionUdpService,
    SessionUdpSnapshot, SessionUdpTransport,
};
pub use storage_file_pool::{
    DEFAULT_STORAGE_FILE_LIMIT, MAX_STORAGE_OBSERVATION_TOKEN_BYTES,
    PLATFORM_STORAGE_REQUEST_CAPACITY, PLATFORM_STORAGE_REQUEST_TIMEOUT, PlatformStorageBroker,
    PlatformStorageClient, PlatformStorageFailure, PlatformStorageFailureKind,
    PlatformStorageOperation, PlatformStorageRequest, PlatformStorageTarget, StorageFileAccess,
    StorageFileHandle, StorageFileKey, StorageFileLease, StorageFileLocator, StorageFilePool,
    StorageFilePoolError, StorageFilePoolSnapshot, StorageFileReference, StorageFileRole,
    StorageObjectKind, StorageObservation, platform_storage_channel,
};
pub use torrent_peer::{
    IncomingPeerAttachment, TorrentPeerActivitySink, TorrentPeerError, TorrentPeerHandle,
};
pub use tracker::{
    TrackerAnnounceEvent, TrackerConfig, TrackerConnectionFamily, TrackerEndpoint,
    TrackerHttpsAuthentication, TrackerNextAction, TrackerRuntimeRecordSnapshot,
    TrackerRuntimeSnapshot, TrackerRuntimeStatus, TrackerSource, TrackerTransport,
};
pub use upload::{
    MAX_GENERATED_ALLOWED_FAST_PIECES, MAX_QUEUED_UPLOAD_BYTES, MAX_QUEUED_UPLOAD_REQUESTS,
    UploadAction, UploadCloseReason, UploadPeerSnapshot, UploadPeerState, UploadRead,
    generate_allowed_fast_set,
};
pub use upload_scheduler::{
    DEFAULT_OPTIMISTIC_UNCHOKE_INTERVAL, DEFAULT_SEEDING_PIECE_QUOTA, DEFAULT_UNCHOKE_INTERVAL,
    DEFAULT_UNCHOKE_SLOTS, UploadDecision, UploadGrant, UploadPeerId, UploadScheduler,
    UploadSchedulerConfig, UploadSchedulerPeer, UploadSchedulerSnapshot,
};
