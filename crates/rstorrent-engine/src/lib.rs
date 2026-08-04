#![forbid(unsafe_code)]

//! Runtime ownership for the first verified-piece diagnostic.

mod checkpoint;
pub mod dht;
mod driver;
mod incoming;
mod metadata_seed;
mod metrics;
mod network;
mod part_file;
pub mod peer;
mod peer_budget;
mod peer_io;
mod peer_runtime;
mod peer_socket;
mod positional_io;
mod seed_content;
mod selective_storage;
mod storage_file_pool;
pub mod swarm;
mod tracker;
mod upload;
mod upload_scheduler;

pub use driver::{
    ContentPeerActivitySnapshot, ContentRequestWindowPhase, DiskCheckpointStage,
    DiskPieceRuntimeSnapshot, DiskPieceStage, DiskPressure, DiskRuntimeSnapshot,
    DownloadActivityEvent, DownloadActivitySink, DownloadCheckpointSink, DownloadConfig,
    DownloadControl, DownloadDiagnosticSnapshot, DownloadError, DownloadProgress, DownloadReport,
    DownloadResourceLimits, MagnetDownloadConfig, MetadataAcquisitionPhase,
    MetadataAcquisitionSnapshot, MetadataPeerSnapshot, MetadataPeerStage, PathPublicationStage,
    ResumableMagnetDownloadConfig, SwarmActivitySnapshot, download_magnet,
    download_magnet_metadata_with_control, download_magnet_metadata_with_dht,
    download_magnet_with_control, download_verified_piece,
    download_verified_piece_to_descriptors_with_control, download_verified_piece_with_control,
    resume_magnet, resume_magnet_to_descriptors_with_control, resume_magnet_with_control,
};
pub use incoming::{
    DEFAULT_INCOMING_HANDSHAKE_TIMEOUT, IncomingPeerError, IncomingPeerHandle, IncomingPeerService,
    IncomingPeerServiceConfig, IncomingPeerServiceSnapshot, IncomingRejection,
    IncomingRejectionReason, IncomingTcpBootstrap, MAX_DEFERRED_METADATA_REQUESTS,
    MAX_INCOMING_ESTABLISHED, MAX_INCOMING_PENDING, MAX_SEED_REGISTRATIONS,
    METADATA_SEND_BUFFER_WATERMARK, SeedRegistration, SeedRegistrationToken,
};
pub use metadata_seed::{
    MetadataSeedConfig, MetadataSeedError, MetadataSeedReport, MetadataSeedServer,
    bind_metadata_seed,
};
pub use metrics::{ByteMetric, ByteMetricSink};
pub use network::{DEFAULT_PEER_ID, NetworkConfig, NetworkPolicy};
pub use part_file::{PartFile, PartFileError, PartFileIdentity};
pub use peer_budget::{
    DEFAULT_CONNECTION_LIMIT, DEFAULT_INCOMING_CONNECTION_SLACK, DEFAULT_LISTEN_BACKLOG,
    PeerBudget, PeerBudgetConfig, PeerBudgetDirection, PeerBudgetPermit, PeerBudgetPhase,
    PeerBudgetRejection, PeerBudgetSnapshot, effective_connection_limit,
};
pub use peer_runtime::{
    PeerConnectionDirection, PeerConnectionLifecycle, PeerConnectionObservation,
    PeerConnectionRole, PeerContentActivity, PeerRequestWindowPhase, PeerRuntimeError,
    PeerTransport,
};
pub use seed_content::{SeedContent, SeedContentError, SeedContentSnapshot};
pub use selective_storage::{
    DescriptorFile, DescriptorFileRole, DescriptorStorage, DescriptorStoragePlan,
    DescriptorStoragePlanFile, MaterializationReport, PlatformStorageSpec, PreparedFileHash,
    PublicationShape, ResumeArtifactState, ResumedStorage, SelectiveStorage, SelectiveStorageError,
    SelectiveWriteStats, TorrentStoragePaths, plan_descriptor_storage,
    remove_selective_part_if_present, remove_selective_staging_if_present, selective_part_path,
    selective_staging_path, torrent_storage_paths, torrent_storage_paths_for_metainfo,
    torrent_storage_paths_with_shape, validate_publication_name, verify_prepared_descriptors,
    verify_prepared_platform_files,
};
pub use storage_file_pool::{
    DEFAULT_STORAGE_FILE_LIMIT, PLATFORM_STORAGE_REQUEST_CAPACITY,
    PLATFORM_STORAGE_REQUEST_TIMEOUT, PlatformStorageBroker, PlatformStorageClient,
    PlatformStorageFailure, PlatformStorageFailureKind, PlatformStorageRequest,
    PlatformStorageTarget, StorageFileAccess, StorageFileHandle, StorageFileKey, StorageFileLease,
    StorageFileLocator, StorageFilePool, StorageFilePoolError, StorageFilePoolSnapshot,
    StorageFileReference, StorageFileRole, platform_storage_channel,
};
pub use tracker::{
    TrackerAnnounceEvent, TrackerNextAction, TrackerRuntimeRecordSnapshot, TrackerRuntimeSnapshot,
    TrackerRuntimeStatus, TrackerSource, TrackerTransport, UdpTrackerConfig,
};
pub use upload::{
    MAX_QUEUED_UPLOAD_BYTES, MAX_QUEUED_UPLOAD_REQUESTS, UploadAction, UploadCloseReason,
    UploadPeerSnapshot, UploadPeerState, UploadRead,
};
pub use upload_scheduler::{
    DEFAULT_OPTIMISTIC_UNCHOKE_INTERVAL, DEFAULT_SEEDING_PIECE_QUOTA, DEFAULT_UNCHOKE_INTERVAL,
    DEFAULT_UNCHOKE_SLOTS, UploadDecision, UploadGrant, UploadPeerId, UploadScheduler,
    UploadSchedulerConfig, UploadSchedulerPeer, UploadSchedulerSnapshot,
};
