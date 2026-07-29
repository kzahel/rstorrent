#![forbid(unsafe_code)]

//! Runtime ownership for the first verified-piece diagnostic.

mod driver;
mod part_file;
mod selective_storage;
mod storage;

pub use driver::{DownloadConfig, DownloadError, DownloadReport, download_verified_piece};
pub use part_file::{PartFile, PartFileError, PartFileIdentity};
pub use selective_storage::{
    MaterializationReport, SelectiveStorage, SelectiveStorageError, SelectiveWriteStats,
    remove_selective_part_if_present, remove_selective_staging_if_present, selective_part_path,
    selective_staging_path,
};
