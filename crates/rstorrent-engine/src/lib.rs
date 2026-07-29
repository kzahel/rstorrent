#![forbid(unsafe_code)]

//! Runtime ownership for the first verified-piece diagnostic.

mod driver;
mod part_file;
mod storage;

pub use driver::{DownloadConfig, DownloadError, DownloadReport, download_verified_piece};
pub use part_file::{PartFile, PartFileError, PartFileIdentity};
