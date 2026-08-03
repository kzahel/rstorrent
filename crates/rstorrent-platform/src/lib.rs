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
    Unavailable,
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
            Self::Unavailable => formatter.write_str(
                "download folder picker requires Zenity or KDialog on this Linux desktop",
            ),
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

#[cfg(target_os = "linux")]
async fn choose_native_download_directory(
    starting_directory: &Path,
) -> Result<Option<PathBuf>, PickerError> {
    use std::io::ErrorKind;
    use tokio::process::Command;

    if !starting_directory.is_dir() {
        return Err(PickerError::InvalidStartingDirectory);
    }

    let mut zenity = Command::new("zenity");
    zenity.kill_on_drop(true).args([
        "--file-selection",
        "--directory",
        "--title=Choose a download folder",
        "--filename",
    ]);
    // Zenity interprets `--filename` as a selection and opens its parent.
    // A nonexistent child therefore opens the requested starting directory
    // while leaving that directory itself as the initial chooser result.
    zenity.arg(starting_directory.join("__rstorrent_folder_picker_start__"));
    match zenity.output().await {
        Ok(output) => return parse_linux_picker_output("zenity", output),
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(PickerError::Launch(error)),
    }

    let mut kdialog = Command::new("kdialog");
    kdialog
        .kill_on_drop(true)
        .arg("--getexistingdirectory")
        .arg(starting_directory)
        .args(["--title", "Choose a download folder"]);
    match kdialog.output().await {
        Ok(output) => parse_linux_picker_output("kdialog", output),
        Err(error) if error.kind() == ErrorKind::NotFound => Err(PickerError::Unavailable),
        Err(error) => Err(PickerError::Launch(error)),
    }
}

#[cfg(target_os = "linux")]
fn parse_linux_picker_output(
    program: &str,
    output: std::process::Output,
) -> Result<Option<PathBuf>, PickerError> {
    if output.status.success() {
        return parse_selected_path(&output.stdout);
    }
    if output.status.code() == Some(1) {
        return Ok(None);
    }

    let mut message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    message.truncate(message.floor_char_boundary(MAX_ERROR_BYTES));
    if message.is_empty() {
        message = format!("{program} exited with {}", output.status);
    }
    Err(PickerError::Failed(message))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
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

    #[cfg(target_os = "linux")]
    #[test]
    fn classifies_linux_selection_cancellation_and_failure() {
        use std::os::unix::process::ExitStatusExt;
        use std::process::{ExitStatus, Output};

        let output = |status, stdout: &[u8], stderr: &[u8]| Output {
            status: ExitStatus::from_raw(status),
            stdout: stdout.to_vec(),
            stderr: stderr.to_vec(),
        };

        assert_eq!(
            super::parse_linux_picker_output(
                "zenity",
                output(0, b"/tmp/selected folder\n", b"ignored warning"),
            )
            .expect("selection"),
            Some(PathBuf::from("/tmp/selected folder"))
        );
        assert_eq!(
            super::parse_linux_picker_output("zenity", output(1 << 8, b"", b""))
                .expect("cancellation"),
            None
        );
        assert!(matches!(
            super::parse_linux_picker_output(
                "zenity",
                output(5 << 8, b"", b"backend timed out"),
            ),
            Err(PickerError::Failed(message)) if message == "backend timed out"
        ));
    }
}
