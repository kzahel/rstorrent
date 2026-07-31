#![forbid(unsafe_code)]

//! Runtime ownership for the first verified-piece diagnostic.

pub mod dht;
mod driver;
mod metadata_seed;
mod network;
mod part_file;
pub mod peer;
mod peer_socket;
mod selective_storage;
mod storage;
pub mod swarm;
mod tracker;

pub use driver::{
    DownloadActivityEvent, DownloadActivitySink, DownloadCheckpointSink, DownloadConfig,
    DownloadControl, DownloadDiagnosticSnapshot, DownloadError, DownloadProgress, DownloadReport,
    MagnetDownloadConfig, MetadataAcquisitionPhase, MetadataAcquisitionSnapshot,
    MetadataPeerSnapshot, MetadataPeerStage, ResumableMagnetDownloadConfig, SwarmActivitySnapshot,
    download_magnet, download_magnet_metadata_with_control, download_magnet_metadata_with_dht,
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
pub use selective_storage::{
    DescriptorFile, DescriptorFileRole, DescriptorStorage, DescriptorStoragePlan,
    DescriptorStoragePlanFile, MaterializationReport, PreparedFileHash, ResumedStorage,
    SelectiveStorage, SelectiveStorageError, SelectiveWriteStats, plan_descriptor_storage,
    remove_selective_part_if_present, remove_selective_staging_if_present, selective_part_path,
    selective_staging_path, verify_prepared_descriptors,
};
