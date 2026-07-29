#![forbid(unsafe_code)]

//! Runtime ownership for the first verified-piece diagnostic.

mod driver;
mod storage;

pub use driver::{DownloadConfig, DownloadError, DownloadReport, download_verified_piece};
