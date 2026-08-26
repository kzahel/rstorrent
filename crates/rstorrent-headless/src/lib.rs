#![forbid(unsafe_code)]

//! Linux headless-service configuration, runtime, and package ownership.

pub mod config;
pub mod runtime;

pub const PACKAGE_ID: &str = "com.jstorrent.rstorrent.headless";
pub const PRODUCT_ID: &str = "rstorrent-headless";
