use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use rstorrent_engine::{DownloadConfig, download_verified_piece};
use rstorrent_protocol::piece::MIN_PAYLOAD_ALLOWANCE;

const DEFAULT_TIMEOUT_SECONDS: u64 = 15;
const MAX_TIMEOUT_SECONDS: u64 = 300;
const DEFAULT_MAX_BUFFERED_PAYLOAD_BYTES: usize = 256 * 1024;
const MAX_BUFFERED_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;
const USAGE: &str = "\
Usage: rstorrent-download-piece \\
  --metainfo PATH --peer 127.0.0.1:PORT --output PATH \\
  [--timeout-seconds SECONDS] \\
  [--max-buffered-payload-bytes BYTES] \\
  [--skip-file INDEX]... [--materialize-file INDEX]...";

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    if arguments.len() == 1 && (arguments[0] == "--help" || arguments[0] == "-h") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    let config = match parse_arguments(arguments) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("argument error: {error}\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    match download_verified_piece(config).await {
        Ok(report) => {
            println!(
                "verified pieces={}/{} skipped_pieces={} bytes={} sha1={} info_hash={} blocks={} \
payload_limit={} payload_high_water={} verification_buffer={} selected_file_bytes={} \
skipped_file_bytes={} padding_bytes={} selected_written_bytes={} part_written_bytes={} \
materialized_bytes={} part_slots_before={} part_slots_after={} part_reopened={} part_path={}",
                report.verified_piece_count,
                report.piece_count,
                report.skipped_piece_count,
                report.bytes_written,
                hex(&report.piece_hash),
                hex(&report.info_hash),
                report.block_count,
                report.payload_limit,
                report.payload_high_water,
                report.verification_buffer,
                report.selected_file_bytes,
                report.skipped_file_bytes,
                report.padding_bytes,
                report.selected_written_bytes,
                report.part_written_bytes,
                report.materialized_bytes,
                report.part_slots_before_materialization,
                report.part_slots_after_materialization,
                report.part_reopened,
                report
                    .part_path
                    .as_ref()
                    .map_or_else(|| "-".to_owned(), |path| path.display().to_string())
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("download failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_arguments(arguments: Vec<OsString>) -> Result<DownloadConfig, String> {
    let mut metainfo_path = None;
    let mut peer = None;
    let mut output_path = None;
    let mut timeout_seconds = DEFAULT_TIMEOUT_SECONDS;
    let mut max_buffered_payload_bytes = DEFAULT_MAX_BUFFERED_PAYLOAD_BYTES;
    let mut skip_files = Vec::new();
    let mut materialize_files = Vec::new();
    let mut index = 0;

    while index < arguments.len() {
        let flag = arguments[index]
            .to_str()
            .ok_or_else(|| "flags must be valid UTF-8".to_owned())?;
        index += 1;
        let value = arguments
            .get(index)
            .ok_or_else(|| format!("{flag} requires a value"))?;
        index += 1;

        match flag {
            "--metainfo" => set_once(&mut metainfo_path, PathBuf::from(value), flag)?,
            "--peer" => {
                let value = value
                    .to_str()
                    .ok_or_else(|| "--peer must be valid UTF-8".to_owned())?;
                let address = value
                    .parse::<SocketAddr>()
                    .map_err(|_| "--peer must be an IP address and port".to_owned())?;
                set_once(&mut peer, address, flag)?;
            }
            "--output" => set_once(&mut output_path, PathBuf::from(value), flag)?,
            "--timeout-seconds" => {
                let value = value
                    .to_str()
                    .ok_or_else(|| "--timeout-seconds must be valid UTF-8".to_owned())?;
                timeout_seconds = value
                    .parse()
                    .map_err(|_| "--timeout-seconds must be an integer".to_owned())?;
                if !(1..=MAX_TIMEOUT_SECONDS).contains(&timeout_seconds) {
                    return Err(format!(
                        "--timeout-seconds must be between 1 and {MAX_TIMEOUT_SECONDS}"
                    ));
                }
            }
            "--max-buffered-payload-bytes" => {
                let value = value
                    .to_str()
                    .ok_or_else(|| "--max-buffered-payload-bytes must be valid UTF-8".to_owned())?;
                max_buffered_payload_bytes = value
                    .parse()
                    .map_err(|_| "--max-buffered-payload-bytes must be an integer".to_owned())?;
                if !(MIN_PAYLOAD_ALLOWANCE..=MAX_BUFFERED_PAYLOAD_BYTES)
                    .contains(&max_buffered_payload_bytes)
                {
                    return Err(format!(
                        "--max-buffered-payload-bytes must be between \
{MIN_PAYLOAD_ALLOWANCE} and {MAX_BUFFERED_PAYLOAD_BYTES}"
                    ));
                }
            }
            "--skip-file" => {
                let file_index = parse_file_index(value, flag)?;
                push_unique(&mut skip_files, file_index, flag)?;
            }
            "--materialize-file" => {
                let file_index = parse_file_index(value, flag)?;
                push_unique(&mut materialize_files, file_index, flag)?;
            }
            _ => return Err(format!("unknown argument {flag}")),
        }
    }

    Ok(DownloadConfig {
        metainfo_path: metainfo_path.ok_or_else(|| "--metainfo is required".to_owned())?,
        peer: peer.ok_or_else(|| "--peer is required".to_owned())?,
        output_path: output_path.ok_or_else(|| "--output is required".to_owned())?,
        timeout: Duration::from_secs(timeout_seconds),
        max_buffered_payload_bytes,
        skip_files,
        materialize_files,
    })
}

fn parse_file_index(value: &OsString, flag: &str) -> Result<usize, String> {
    value
        .to_str()
        .ok_or_else(|| format!("{flag} must be valid UTF-8"))?
        .parse()
        .map_err(|_| format!("{flag} must be a nonnegative integer"))
}

fn push_unique(values: &mut Vec<usize>, value: usize, flag: &str) -> Result<(), String> {
    if values.contains(&value) {
        return Err(format!("{flag} index {value} may only be provided once"));
    }
    values.push(value);
    Ok(())
}

fn set_once<T>(target: &mut Option<T>, value: T, flag: &str) -> Result<(), String> {
    if target.replace(value).is_some() {
        return Err(format!("{flag} may only be provided once"));
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(DIGITS[(byte >> 4) as usize] as char);
        result.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    result
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::time::Duration;

    use super::{DEFAULT_MAX_BUFFERED_PAYLOAD_BYTES, DEFAULT_TIMEOUT_SECONDS, parse_arguments};

    fn strings(arguments: &[&str]) -> Vec<OsString> {
        arguments.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_required_diagnostic_arguments() {
        let config = parse_arguments(strings(&[
            "--metainfo",
            "fixture.torrent",
            "--peer",
            "127.0.0.1:6881",
            "--output",
            "payload.bin",
        ]))
        .expect("valid arguments");

        assert_eq!(config.metainfo_path.to_string_lossy(), "fixture.torrent");
        assert!(config.peer.ip().is_loopback());
        assert_eq!(config.peer.port(), 6881);
        assert_eq!(config.output_path.to_string_lossy(), "payload.bin");
        assert_eq!(config.timeout, Duration::from_secs(DEFAULT_TIMEOUT_SECONDS));
        assert_eq!(
            config.max_buffered_payload_bytes,
            DEFAULT_MAX_BUFFERED_PAYLOAD_BYTES
        );
        assert!(config.skip_files.is_empty());
        assert!(config.materialize_files.is_empty());
    }

    #[test]
    fn rejects_missing_duplicate_and_unbounded_arguments() {
        assert!(parse_arguments(strings(&[])).is_err());
        assert!(
            parse_arguments(strings(&[
                "--metainfo",
                "a",
                "--metainfo",
                "b",
                "--peer",
                "127.0.0.1:1",
                "--output",
                "c",
            ]))
            .is_err()
        );
        assert!(
            parse_arguments(strings(&[
                "--metainfo",
                "a",
                "--peer",
                "127.0.0.1:1",
                "--output",
                "c",
                "--timeout-seconds",
                "301",
            ]))
            .is_err()
        );
        assert!(
            parse_arguments(strings(&[
                "--metainfo",
                "a",
                "--peer",
                "127.0.0.1:1",
                "--output",
                "c",
                "--max-buffered-payload-bytes",
                "1",
            ]))
            .is_err()
        );
        let selected = parse_arguments(strings(&[
            "--metainfo",
            "a",
            "--peer",
            "127.0.0.1:1",
            "--output",
            "c",
            "--skip-file",
            "3",
            "--skip-file",
            "7",
            "--materialize-file",
            "7",
        ]))
        .expect("selected arguments");
        assert_eq!(selected.skip_files, [3, 7]);
        assert_eq!(selected.materialize_files, [7]);
        assert!(
            parse_arguments(strings(&[
                "--metainfo",
                "a",
                "--peer",
                "127.0.0.1:1",
                "--output",
                "c",
                "--skip-file",
                "3",
                "--skip-file",
                "3",
            ]))
            .is_err()
        );
    }
}
