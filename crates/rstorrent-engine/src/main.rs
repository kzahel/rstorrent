use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rstorrent_engine::dht::{BootstrapNode, DhtConfig, DhtService};
use rstorrent_engine::peer::{PeerEndpoint, PeerObservation, PeerSource};
use rstorrent_engine::{
    DownloadActivityEvent, DownloadActivitySink, DownloadCheckpointSink, DownloadConfig,
    DownloadControl, DownloadError, DownloadProgress, DownloadResourceLimits, MagnetDownloadConfig,
    MseDhWorkOwner, NetworkConfig, NetworkPolicy, PeerBudget, PeerEncryptionPolicy,
    PeerEncryptionPolicyHandle, PreparedFileHash, ResumableMetainfoDownloadConfig,
    ResumeArtifactState, ResumeValidationIntent, ResumedStorage, TorrentId, TorrentIdentityContext,
    TorrentPeerHandle, download_magnet_with_control, download_verified_piece_with_control,
    resume_metainfo_with_control,
};
use rstorrent_protocol::content::{TorrentContent, TorrentContentProjection};
use rstorrent_protocol::identity::V1InfoHash;
use rstorrent_protocol::magnet::Magnet;
use rstorrent_protocol::metainfo::BEP9_METAINFO_LIMITS;
use rstorrent_protocol::piece::MIN_PAYLOAD_ALLOWANCE;

const DEFAULT_TIMEOUT_SECONDS: u64 = 15;
const MAX_TIMEOUT_SECONDS: u64 = 4 * 60 * 60;
const MAX_BUFFERED_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;
const DEFAULT_STORAGE_WRITE_CONCURRENCY: usize = 4;
const DEFAULT_STORAGE_HASH_CONCURRENCY: usize = 4;
const USAGE: &str = "\
Usage: rstorrent-download-piece \\
  --metainfo PATH --peer 127.0.0.1:PORT --output PATH \\
  [options]\n\
   or: rstorrent-download-piece \\
  --magnet 'magnet:?xt=urn:btih:...&tr=udp://127.0.0.1:PORT' --output PATH \\
  [options]\n\
\n\
Options:\n\
  [--timeout-seconds SECONDS] \\
  [--max-buffered-payload-bytes BYTES] \\
  [--encryption disabled|allow|prefer|required] \\
  [--dht-bootstrap IP:PORT] \\
  [--skip-file INDEX]... [--materialize-file INDEX]...";

#[derive(Debug)]
enum DownloadCommand {
    Metainfo(PendingMetainfoDownload),
    Magnet {
        config: MagnetDownloadConfig,
        dht_bootstrap: Option<SocketAddr>,
    },
}

#[derive(Debug)]
struct PendingMetainfoDownload {
    metainfo_path: PathBuf,
    peer: SocketAddr,
    output_path: PathBuf,
    network: NetworkConfig,
    resource_limits: DownloadResourceLimits,
    skip_files: Vec<usize>,
    materialize_files: Vec<usize>,
}

#[derive(Debug, Default)]
struct DiagnosticActivity {
    first_verified_piece: Mutex<Option<u32>>,
}

#[derive(Debug, Default)]
struct DiagnosticCheckpointSink;

impl DownloadCheckpointSink for DiagnosticCheckpointSink {
    fn metadata_verified(&self, _raw_info: &[u8]) -> Result<(), String> {
        Ok(())
    }

    fn storage_prepared(&self, _storage: ResumedStorage) -> Result<(), String> {
        Ok(())
    }

    fn recheck_started(&self) -> Result<u64, String> {
        Ok(1)
    }

    fn have_rechecked(&self, _verified_pieces: &[bool]) -> Result<(), String> {
        Ok(())
    }

    fn pieces_invalidated(&self, _piece_indices: &[usize]) -> Result<(), String> {
        Ok(())
    }

    fn pieces_durable(&self, _piece_indices: &[usize]) -> Result<(), String> {
        Ok(())
    }

    fn descriptor_prepared(&self, _files: &[PreparedFileHash]) -> Result<(), String> {
        Ok(())
    }

    fn publication_prepared(&self) -> Result<(), String> {
        Ok(())
    }

    fn published(&self) -> Result<(), String> {
        Ok(())
    }
}

impl DiagnosticActivity {
    fn first_verified_piece(&self) -> Option<u32> {
        *self
            .first_verified_piece
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl DownloadActivitySink for DiagnosticActivity {
    fn record(&self, event: DownloadActivityEvent) {
        let DownloadActivityEvent::PieceVerified { piece_index } = event else {
            return;
        };
        let mut first = self
            .first_verified_piece
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        first.get_or_insert(piece_index);
    }
}

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

    let control = DownloadControl::new();
    let activity = Arc::new(DiagnosticActivity::default());
    control.set_activity_sink(activity.clone());
    if let Err(error) = configure_diagnostic_storage_execution(&control) {
        eprintln!("argument error: {error}");
        return ExitCode::from(2);
    }
    let result = match config {
        DownloadCommand::Metainfo(config) => {
            let bytes = match std::fs::read(&config.metainfo_path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    return report_result(
                        Err(DownloadError::Io {
                            operation: "read metainfo for identity",
                            source: error,
                        }),
                        control.snapshot(),
                        activity.first_verified_piece(),
                    );
                }
            };
            let projection = match TorrentContentProjection::from_bytes_with_limits(
                &bytes,
                BEP9_METAINFO_LIMITS,
            ) {
                Ok(projection) => projection,
                Err(error) => {
                    return report_result(
                        Err(DownloadError::Metainfo(error)),
                        control.snapshot(),
                        activity.first_verified_piece(),
                    );
                }
            };
            let identity = match TorrentId::generate() {
                Ok(torrent_id) => TorrentIdentityContext::new(
                    torrent_id,
                    projection.content.info_hashes(),
                    projection.content.swarm_key(),
                )
                .expect("complete metainfo selects its own matching wire identity"),
                Err(error) => {
                    eprintln!("identity allocation failed: {error}");
                    return ExitCode::from(1);
                }
            };
            match projection.content {
                TorrentContent::V1(v1) => {
                    let config = DownloadConfig {
                        identity: TorrentIdentityContext::v1(
                            identity.torrent_id(),
                            V1InfoHash::new(v1.metainfo.info_hash),
                        ),
                        metainfo_path: config.metainfo_path,
                        peer: config.peer,
                        output_path: config.output_path,
                        network: config.network,
                        resource_limits: config.resource_limits,
                        skip_files: config.skip_files,
                        materialize_files: config.materialize_files,
                    };
                    download_verified_piece_with_control(config, control.clone()).await
                }
                content @ (TorrentContent::V2(_) | TorrentContent::Hybrid(_)) => {
                    if !config.materialize_files.is_empty() {
                        return report_result(
                            Err(DownloadError::InvalidTorrentIdentity(
                                "v2 diagnostics do not use part-file materialization",
                            )),
                            control.snapshot(),
                            activity.first_verified_piece(),
                        );
                    }
                    let peers = match TorrentPeerHandle::new(Arc::new(control.clone())) {
                        Ok(peers) => peers,
                        Err(error) => {
                            return report_result(
                                Err(DownloadError::PeerTask(error.to_string())),
                                control.snapshot(),
                                activity.first_verified_piece(),
                            );
                        }
                    };
                    let endpoint = match PeerEndpoint::new(config.peer) {
                        Ok(endpoint) => endpoint,
                        Err(error) => {
                            return report_result(
                                Err(DownloadError::PeerRegistry(error)),
                                control.snapshot(),
                                activity.first_verified_piece(),
                            );
                        }
                    };
                    if let Err(error) = peers.observe_discovered_peer(PeerObservation::dialable(
                        endpoint,
                        PeerSource::Manual,
                    )) {
                        return report_result(
                            Err(DownloadError::PeerTask(error.to_string())),
                            control.snapshot(),
                            activity.first_verified_piece(),
                        );
                    }
                    let piece_count = content.piece_count();
                    let encryption = PeerEncryptionPolicyHandle::new(config.network.encryption);
                    resume_metainfo_with_control(
                        ResumableMetainfoDownloadConfig {
                            identity,
                            metainfo_source: bytes,
                            storage_root: config.output_path,
                            network: config.network,
                            peer_budget: PeerBudget::system_default(),
                            mse_dh: MseDhWorkOwner::new(),
                            encryption,
                            torrent_peers: Some(peers),
                            resource_limits: config.resource_limits,
                            skip_files: config.skip_files,
                            verified_pieces: vec![false; piece_count],
                            artifact_state: ResumeArtifactState::None,
                            resume_validation: ResumeValidationIntent::Full,
                            download_missing: true,
                            dht: None,
                            trackers: None,
                        },
                        Arc::new(DiagnosticCheckpointSink),
                        control.clone(),
                    )
                    .await
                }
            }
        }
        DownloadCommand::Magnet {
            mut config,
            dht_bootstrap,
        } => {
            let dht = if let Some(bootstrap) = dht_bootstrap {
                let mut dht_config = DhtConfig::for_network(NetworkPolicy::LoopbackOnly);
                dht_config.bootstrap_nodes = vec![BootstrapNode::Address(bootstrap)];
                match DhtService::start(dht_config).await {
                    Ok(service) => Some(service),
                    Err(error) => {
                        return report_result(
                            Err(DownloadError::Dht(error)),
                            control.snapshot(),
                            activity.first_verified_piece(),
                        );
                    }
                }
            } else {
                None
            };
            config.dht = dht.as_ref().map(DhtService::handle);
            let result = download_magnet_with_control(config, control.clone()).await;
            if let Some(dht) = dht {
                let shutdown = dht.shutdown().await.map_err(DownloadError::Dht);
                if result.is_ok()
                    && let Err(error) = shutdown
                {
                    return report_result(
                        Err(error),
                        control.snapshot(),
                        activity.first_verified_piece(),
                    );
                }
            }
            result
        }
    };
    report_result(result, control.snapshot(), activity.first_verified_piece())
}

fn report_result(
    result: Result<rstorrent_engine::DownloadReport, DownloadError>,
    progress: DownloadProgress,
    first_verified_piece: Option<u32>,
) -> ExitCode {
    match result {
        Ok(report) => {
            println!(
                "verified pieces={}/{} skipped_pieces={} first_verified_piece={} bytes={} sha1={} info_hash={} blocks={} \
payload_limit={} payload_high_water={} outstanding_request_limit={} \
outstanding_request_high_water={} active_piece_limit={} verification_buffer={} selected_file_bytes={} \
skipped_file_bytes={} padding_bytes={} selected_written_bytes={} part_written_bytes={} \
materialized_bytes={} part_slots_before={} part_slots_after={} part_reopened={} part_path={} \
storage_write_operations={} storage_write_blocks={} storage_write_batch_blocks_high_water={} \
storage_write_batch_bytes_high_water={} storage_write_service_micros={} \
storage_write_active_high_water={} storage_hash_operations={} \
storage_hash_service_micros={} storage_hash_active_high_water={}",
                report.verified_piece_count,
                report.piece_count,
                report.skipped_piece_count,
                first_verified_piece.map_or_else(|| "-".to_owned(), |piece| piece.to_string()),
                report.bytes_written,
                hex(&report.piece_hash),
                hex(&report.info_hash),
                report.block_count,
                report.payload_limit,
                report.payload_high_water,
                report.outstanding_request_limit,
                report.outstanding_request_high_water,
                report.active_piece_limit,
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
                    .map_or_else(|| "-".to_owned(), |path| path.display().to_string()),
                progress.storage_write_operations_completed,
                progress.storage_write_blocks_completed,
                progress.storage_write_batch_blocks_high_water,
                progress.storage_write_batch_bytes_high_water,
                progress.storage_write_service_micros,
                progress.storage_write_operations_active_high_water,
                progress.storage_hash_operations_completed,
                progress.storage_hash_service_micros,
                progress.storage_hash_operations_active_high_water,
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("download failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn configure_diagnostic_storage_execution(control: &DownloadControl) -> Result<(), String> {
    let writes = parse_diagnostic_concurrency(
        "RSTORRENT_TEST_STORAGE_WRITE_CONCURRENCY",
        DEFAULT_STORAGE_WRITE_CONCURRENCY,
    )?;
    let hashes = parse_diagnostic_concurrency(
        "RSTORRENT_TEST_STORAGE_HASH_CONCURRENCY",
        DEFAULT_STORAGE_HASH_CONCURRENCY,
    )?;
    control
        .set_storage_execution_limits_for_testing(writes, hashes)
        .map_err(|error| error.to_string())
}

fn parse_diagnostic_concurrency(name: &str, default: usize) -> Result<usize, String> {
    let Some(value) = std::env::var_os(name) else {
        return Ok(default);
    };
    value
        .to_str()
        .ok_or_else(|| format!("{name} must be valid UTF-8"))?
        .parse()
        .map_err(|_| format!("{name} must be an integer"))
}

fn parse_arguments(arguments: Vec<OsString>) -> Result<DownloadCommand, String> {
    let mut metainfo_path = None;
    let mut magnet = None;
    let mut peer = None;
    let mut output_path = None;
    let mut dht_bootstrap = None;
    let mut timeout_seconds = DEFAULT_TIMEOUT_SECONDS;
    let mut encryption = PeerEncryptionPolicy::Allow;
    let mut resource_limits = DownloadResourceLimits::DESKTOP;
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
            "--magnet" => {
                let value = value
                    .to_str()
                    .ok_or_else(|| "--magnet must be valid UTF-8".to_owned())?;
                set_once(&mut magnet, value.to_owned(), flag)?;
            }
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
            "--dht-bootstrap" => {
                let value = value
                    .to_str()
                    .ok_or_else(|| "--dht-bootstrap must be valid UTF-8".to_owned())?;
                let address = value
                    .parse::<SocketAddr>()
                    .map_err(|_| "--dht-bootstrap must be an IP address and port".to_owned())?;
                set_once(&mut dht_bootstrap, address, flag)?;
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
            "--max-buffered-payload-bytes" => {
                let value = value
                    .to_str()
                    .ok_or_else(|| "--max-buffered-payload-bytes must be valid UTF-8".to_owned())?;
                resource_limits.max_buffered_payload_bytes = value
                    .parse()
                    .map_err(|_| "--max-buffered-payload-bytes must be an integer".to_owned())?;
                if !(MIN_PAYLOAD_ALLOWANCE..=MAX_BUFFERED_PAYLOAD_BYTES)
                    .contains(&resource_limits.max_buffered_payload_bytes)
                {
                    return Err(format!(
                        "--max-buffered-payload-bytes must be between \
{MIN_PAYLOAD_ALLOWANCE} and {MAX_BUFFERED_PAYLOAD_BYTES}"
                    ));
                }
                resource_limits.storage_intake_high_watermark_bytes =
                    DownloadResourceLimits::default_storage_intake_high_watermark(
                        resource_limits.max_buffered_payload_bytes,
                    );
            }
            "--encryption" => {
                encryption = match value
                    .to_str()
                    .ok_or_else(|| "--encryption must be valid UTF-8".to_owned())?
                {
                    "disabled" => PeerEncryptionPolicy::Disabled,
                    "allow" => PeerEncryptionPolicy::Allow,
                    "prefer" => PeerEncryptionPolicy::Prefer,
                    "required" => PeerEncryptionPolicy::Required,
                    _ => {
                        return Err(
                            "--encryption must be disabled, allow, prefer, or required".to_owned()
                        );
                    }
                };
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

    let output_path = output_path.ok_or_else(|| "--output is required".to_owned())?;
    if metainfo_path.is_some() && dht_bootstrap.is_some() {
        return Err("--dht-bootstrap is only valid with --magnet".to_owned());
    }
    let peer_timeout = Duration::from_secs(timeout_seconds);
    let network = NetworkConfig::new(NetworkPolicy::LoopbackOnly, peer_timeout, peer_timeout)
        .with_encryption(encryption);
    match (metainfo_path, magnet, peer) {
        (Some(metainfo_path), None, Some(peer)) => {
            Ok(DownloadCommand::Metainfo(PendingMetainfoDownload {
                metainfo_path,
                peer,
                output_path,
                network,
                resource_limits,
                skip_files,
                materialize_files,
            }))
        }
        (None, Some(magnet), None) => {
            let parsed = Magnet::parse(&magnet).map_err(|error| error.to_string())?;
            Ok(DownloadCommand::Magnet {
                config: MagnetDownloadConfig {
                    identity: TorrentIdentityContext::for_full(
                        TorrentId::generate().map_err(|error| error.to_string())?,
                        parsed.identity,
                    ),
                    magnet,
                    output_path,
                    network,
                    resource_limits,
                    skip_files,
                    materialize_files,
                    dht: None,
                },
                dht_bootstrap,
            })
        }
        (Some(_), Some(_), _) => Err("--metainfo and --magnet are mutually exclusive".to_owned()),
        (None, None, _) => Err("exactly one of --metainfo or --magnet is required".to_owned()),
        (Some(_), None, None) => Err("--peer is required with --metainfo".to_owned()),
        (None, Some(_), Some(_)) => Err(
            "--peer cannot be supplied with --magnet; use magnet discovery parameters".to_owned(),
        ),
    }
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

    use super::{
        DEFAULT_TIMEOUT_SECONDS, DownloadCommand, DownloadResourceLimits, NetworkConfig,
        NetworkPolicy, PeerEncryptionPolicy, parse_arguments,
    };

    fn strings(arguments: &[&str]) -> Vec<OsString> {
        arguments.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_required_diagnostic_arguments() {
        let command = parse_arguments(strings(&[
            "--metainfo",
            "fixture.torrent",
            "--peer",
            "127.0.0.1:6881",
            "--output",
            "payload.bin",
        ]))
        .expect("valid arguments");
        let DownloadCommand::Metainfo(config) = command else {
            panic!("expected metainfo command");
        };

        assert_eq!(config.metainfo_path.to_string_lossy(), "fixture.torrent");
        assert!(config.peer.ip().is_loopback());
        assert_eq!(config.peer.port(), 6881);
        assert_eq!(config.output_path.to_string_lossy(), "payload.bin");
        assert_eq!(
            config.network,
            NetworkConfig::new(
                NetworkPolicy::LoopbackOnly,
                Duration::from_secs(DEFAULT_TIMEOUT_SECONDS),
                Duration::from_secs(DEFAULT_TIMEOUT_SECONDS),
            )
        );
        assert_eq!(config.resource_limits, DownloadResourceLimits::DESKTOP);
        assert!(config.skip_files.is_empty());
        assert!(config.materialize_files.is_empty());
    }

    #[test]
    fn parses_magnet_without_an_out_of_band_peer() {
        let command = parse_arguments(strings(&[
            "--magnet",
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&tr=udp://127.0.0.1:1",
            "--output",
            "payload",
        ]))
        .expect("valid magnet arguments");
        let DownloadCommand::Magnet {
            config,
            dht_bootstrap,
        } = command
        else {
            panic!("expected magnet command");
        };

        assert!(config.magnet.starts_with("magnet:?"));
        assert_eq!(config.output_path.to_string_lossy(), "payload");
        assert_eq!(dht_bootstrap, None);
        assert_eq!(
            config.network,
            NetworkConfig::new(
                NetworkPolicy::LoopbackOnly,
                Duration::from_secs(DEFAULT_TIMEOUT_SECONDS),
                Duration::from_secs(DEFAULT_TIMEOUT_SECONDS),
            )
        );
    }

    #[test]
    fn parses_closed_encryption_policy() {
        let command = parse_arguments(strings(&[
            "--metainfo",
            "fixture.torrent",
            "--peer",
            "127.0.0.1:6881",
            "--output",
            "payload.bin",
            "--encryption",
            "required",
        ]))
        .expect("valid encryption policy");
        let DownloadCommand::Metainfo(config) = command else {
            panic!("expected metainfo command");
        };
        assert_eq!(config.network.encryption, PeerEncryptionPolicy::Required);
        assert!(
            parse_arguments(strings(&[
                "--metainfo",
                "fixture.torrent",
                "--peer",
                "127.0.0.1:6881",
                "--output",
                "payload.bin",
                "--encryption",
                "sometimes",
            ]))
            .is_err()
        );
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
                "14401",
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
        let DownloadCommand::Metainfo(selected) = selected else {
            panic!("expected metainfo command");
        };
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
