#![forbid(unsafe_code)]

//! Runtime ownership for the first verified-piece diagnostic.

mod checkpoint;
pub mod dht;
mod driver;
mod metadata_seed;
mod network;
mod part_file;
pub mod peer;
mod peer_runtime;
mod peer_socket;
mod positional_io;
mod selective_storage;
mod storage;
mod storage_file_pool;
pub mod swarm;
mod tracker;

pub use driver::{
    ContentPeerActivitySnapshot, ContentRequestWindowPhase, DiskCheckpointStage,
    DiskPieceRuntimeSnapshot, DiskPieceStage, DiskPressure, DiskRuntimeSnapshot,
    DownloadActivityEvent, DownloadActivitySink, DownloadCheckpointSink, DownloadConfig,
    DownloadControl, DownloadDiagnosticSnapshot, DownloadError, DownloadProgress, DownloadReport,
    DownloadResourceLimits, MagnetDownloadConfig, MetadataAcquisitionPhase,
    MetadataAcquisitionSnapshot, MetadataPeerSnapshot, MetadataPeerStage,
    ResumableMagnetDownloadConfig, SwarmActivitySnapshot, download_magnet,
    download_magnet_metadata_with_control, download_magnet_metadata_with_dht,
    download_magnet_with_control, download_verified_piece,
    download_verified_piece_to_descriptors_with_control, download_verified_piece_with_control,
    resume_magnet, resume_magnet_to_descriptors_with_control, resume_magnet_with_control,
};
pub use metadata_seed::{
    MetadataSeedConfig, MetadataSeedError, MetadataSeedReport, MetadataSeedServer,
    bind_metadata_seed,
};
pub use network::{NetworkConfig, NetworkPolicy};
pub use part_file::{PartFile, PartFileError, PartFileIdentity};
pub use peer_runtime::{
    PeerConnectionDirection, PeerConnectionLifecycle, PeerConnectionObservation,
    PeerConnectionRole, PeerContentActivity, PeerRequestWindowPhase, PeerRuntimeError,
    PeerTransport,
};
pub use selective_storage::{
    DescriptorFile, DescriptorFileRole, DescriptorStorage, DescriptorStoragePlan,
    DescriptorStoragePlanFile, MaterializationReport, PreparedFileHash, ResumedStorage,
    SelectiveStorage, SelectiveStorageError, SelectiveWriteStats, TorrentStoragePaths,
    plan_descriptor_storage, remove_selective_part_if_present, remove_selective_staging_if_present,
    selective_part_path, selective_staging_path, torrent_storage_paths, validate_publication_name,
    verify_prepared_descriptors,
};
pub use storage_file_pool::{
    DEFAULT_STORAGE_FILE_LIMIT, PLATFORM_STORAGE_REQUEST_CAPACITY,
    PLATFORM_STORAGE_REQUEST_TIMEOUT, PlatformStorageBroker, PlatformStorageClient,
    PlatformStorageFailure, PlatformStorageFailureKind, PlatformStorageRequest,
    PlatformStorageTarget, StorageFileAccess, StorageFileHandle, StorageFileKey,
    StorageFileLocator, StorageFilePool, StorageFilePoolError, StorageFilePoolSnapshot,
    StorageFileReference, StorageFileRole, platform_storage_channel,
};
pub use tracker::{
    TrackerAnnounceEvent, TrackerNextAction, TrackerRuntimeRecordSnapshot, TrackerRuntimeSnapshot,
    TrackerRuntimeStatus, TrackerSource, TrackerTransport,
};
