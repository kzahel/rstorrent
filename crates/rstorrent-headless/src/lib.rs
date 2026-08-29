#![forbid(unsafe_code)]

//! Linux headless-service configuration, runtime, and package ownership.

pub mod config;
pub mod installer;
pub mod remote_admin;
pub mod runtime;
pub mod updater;

pub const PACKAGE_ID: &str = "com.jstorrent.rstorrent.headless";
pub const PRODUCT_ID: &str = "rstorrent-headless";
pub const SERVICE_NAME: &str = "com.jstorrent.rstorrent.headless.service";
