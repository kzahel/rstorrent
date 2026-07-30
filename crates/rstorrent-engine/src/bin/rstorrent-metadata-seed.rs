use std::ffi::OsString;
use std::io::{self, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use rstorrent_engine::{MetadataSeedConfig, bind_metadata_seed};

const DEFAULT_TIMEOUT_SECONDS: u64 = 15;
const MAX_TIMEOUT_SECONDS: u64 = 300;
const USAGE: &str = "\
Usage: rstorrent-metadata-seed \\
  --metainfo PATH --listen 127.0.0.1:PORT \\
  [--timeout-seconds SECONDS]";

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
    let server = match bind_metadata_seed(config).await {
        Ok(server) => server,
        Err(error) => {
            eprintln!("metadata seed failed before listen: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "listening address={} info_hash={} metadata_size={}",
        server.listen_address(),
        hex(&server.info_hash()),
        server.metadata_size()
    );
    if let Err(error) = io::stdout().flush() {
        eprintln!("metadata seed failed to flush listener report: {error}");
        return ExitCode::FAILURE;
    }

    match server.serve().await {
        Ok(report) => {
            println!(
                "served address={} info_hash={} metadata_size={} blocks={} requests={}",
                report.listen_address,
                hex(&report.info_hash),
                report.metadata_size,
                report.block_count,
                report.request_count
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("metadata seed failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_arguments(arguments: Vec<OsString>) -> Result<MetadataSeedConfig, String> {
    let mut metainfo_path = None;
    let mut listen = None;
    let mut timeout_seconds = DEFAULT_TIMEOUT_SECONDS;
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
            "--listen" => {
                let value = value
                    .to_str()
                    .ok_or_else(|| "--listen must be valid UTF-8".to_owned())?;
                let address = value
                    .parse::<SocketAddr>()
                    .map_err(|_| "--listen must be an IP address and port".to_owned())?;
                set_once(&mut listen, address, flag)?;
            }
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
            _ => return Err(format!("unknown argument {flag}")),
        }
    }
    Ok(MetadataSeedConfig {
        metainfo_path: metainfo_path.ok_or_else(|| "--metainfo is required".to_owned())?,
        listen: listen.ok_or_else(|| "--listen is required".to_owned())?,
        timeout: Duration::from_secs(timeout_seconds),
    })
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

    use super::{DEFAULT_TIMEOUT_SECONDS, parse_arguments};

    fn strings(arguments: &[&str]) -> Vec<OsString> {
        arguments.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_bounded_loopback_server_arguments() {
        let config = parse_arguments(strings(&[
            "--metainfo",
            "fixture.torrent",
            "--listen",
            "127.0.0.1:0",
        ]))
        .expect("valid arguments");

        assert_eq!(config.metainfo_path.to_string_lossy(), "fixture.torrent");
        assert!(config.listen.ip().is_loopback());
        assert_eq!(config.timeout, Duration::from_secs(DEFAULT_TIMEOUT_SECONDS));
        assert!(parse_arguments(strings(&[])).is_err());
        assert!(
            parse_arguments(strings(&[
                "--metainfo",
                "a",
                "--listen",
                "127.0.0.1:0",
                "--timeout-seconds",
                "301",
            ]))
            .is_err()
        );
    }
}
