#![forbid(unsafe_code)]

//! Narrow native-platform operations shared by first-party application adapters.

use std::error::Error;
use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

const MAX_SELECTED_PATH_BYTES: usize = 4096;
const MAX_ERROR_BYTES: usize = 1024;

pub trait DownloadDirectoryPicker: Send + Sync + 'static {
    fn choose<'a>(&'a self, starting_directory: &'a Path) -> PickerFuture<'a>;
}

pub type PickerFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Option<PathBuf>, PickerError>> + Send + 'a>>;

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeDownloadDirectoryPicker;

impl DownloadDirectoryPicker for NativeDownloadDirectoryPicker {
    fn choose<'a>(&'a self, starting_directory: &'a Path) -> PickerFuture<'a> {
        Box::pin(choose_native_download_directory(starting_directory))
    }
}

#[derive(Debug)]
pub enum PickerError {
    Unsupported,
    InvalidStartingDirectory,
    Launch(std::io::Error),
    Failed(String),
    InvalidOutput,
}

impl fmt::Display for PickerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => {
                formatter.write_str("download folder picker is not implemented on this platform")
            }
            Self::InvalidStartingDirectory => formatter
                .write_str("download folder picker requires an existing starting directory"),
            Self::Launch(error) => write!(formatter, "launch download folder picker: {error}"),
            Self::Failed(message) => write!(formatter, "download folder picker failed: {message}"),
            Self::InvalidOutput => {
                formatter.write_str("download folder picker returned an invalid path")
            }
        }
    }
}

impl Error for PickerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Launch(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(target_os = "macos")]
async fn choose_native_download_directory(
    starting_directory: &Path,
) -> Result<Option<PathBuf>, PickerError> {
    use tokio::process::Command;

    const SCRIPT: &str = r#"
on run argv
    try
        set startingFolder to POSIX file (item 1 of argv)
        set selectedFolder to choose folder with prompt "Choose a download folder" default location startingFolder
        return POSIX path of selectedFolder
    on error number -128
        return ""
    end try
end run
"#;

    if !starting_directory.is_dir() {
        return Err(PickerError::InvalidStartingDirectory);
    }
    let mut command = Command::new("/usr/bin/osascript");
    command
        .kill_on_drop(true)
        .args(["-e", SCRIPT, "--"])
        .arg(starting_directory);
    let output = command.output().await.map_err(PickerError::Launch)?;
    if !output.status.success() {
        let mut message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        message.truncate(message.floor_char_boundary(MAX_ERROR_BYTES));
        if message.is_empty() {
            message = format!("osascript exited with {}", output.status);
        }
        return Err(PickerError::Failed(message));
    }
    parse_selected_path(&output.stdout)
}

#[cfg(not(target_os = "macos"))]
async fn choose_native_download_directory(
    starting_directory: &Path,
) -> Result<Option<PathBuf>, PickerError> {
    if !starting_directory.is_dir() {
        return Err(PickerError::InvalidStartingDirectory);
    }
    Err(PickerError::Unsupported)
}

fn parse_selected_path(output: &[u8]) -> Result<Option<PathBuf>, PickerError> {
    let path = std::str::from_utf8(output)
        .map_err(|_| PickerError::InvalidOutput)?
        .trim_end_matches(['\r', '\n']);
    if path.is_empty() {
        return Ok(None);
    }
    if path.len() > MAX_SELECTED_PATH_BYTES || path.contains('\0') {
        return Err(PickerError::InvalidOutput);
    }
    Ok(Some(PathBuf::from(path)))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{PickerError, parse_selected_path};

    #[test]
    fn parses_selection_and_cancellation_without_trimming_folder_names() {
        assert_eq!(
            parse_selected_path(b"/Users/test/Downloads/\n").expect("selection"),
            Some(PathBuf::from("/Users/test/Downloads/"))
        );
        assert_eq!(parse_selected_path(b"\n").expect("cancel"), None);
        assert_eq!(
            parse_selected_path(b"/tmp/folder with space \n").expect("space"),
            Some(PathBuf::from("/tmp/folder with space "))
        );
    }

    #[test]
    fn rejects_unbounded_or_non_utf8_output() {
        assert!(matches!(
            parse_selected_path(&[0xff]),
            Err(PickerError::InvalidOutput)
        ));
        assert!(matches!(
            parse_selected_path(&vec![b'x'; 4097]),
            Err(PickerError::InvalidOutput)
        ));
    }
}
