use std::env;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rstorrent_session::{
    ApplicationConfig, ApplicationService, BandwidthRuntimeView, Command, ConfiguredStorageRoot,
    DownloadResourceLimits, ErrorCode, NetworkConfig, NetworkPolicy, RequestEnvelope,
    ResponseEnvelope, application_error_response,
};
use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};

const MAX_ARGUMENTS: usize = 32;

#[derive(Serialize)]
struct DiagnosticResourceReport {
    download: rstorrent_engine::SessionDownloadResourceSnapshot,
    peer_budget: rstorrent_engine::PeerBudgetSnapshot,
    storage_files: rstorrent_engine::StorageFilePoolSnapshot,
    bandwidth: BandwidthRuntimeView,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("error={error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), DiagnosticError> {
    let (config, resource_report) = parse_arguments(env::args_os().skip(1))?;
    let service = Arc::new(tokio::sync::Mutex::new(
        ApplicationService::open(config).await?,
    ));
    ApplicationService::ensure_maintenance_owner(&service).await;
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut output = BufWriter::new(tokio::io::stdout());
    let mut peer_budget_before_shutdown = None;
    let mut bandwidth_before_shutdown = None;
    while let Some(line) = lines
        .next_line()
        .await
        .map_err(DiagnosticError::ReadInput)?
    {
        if line.len() > 64 * 1024 {
            let revision = service.lock().await.revision().unwrap_or(0);
            write_response(
                &mut output,
                &ResponseEnvelope::error(
                    String::new(),
                    revision,
                    ErrorCode::InvalidRequest,
                    "diagnostic request exceeds 65536 bytes",
                ),
            )
            .await?;
            continue;
        }
        let request = match serde_json::from_str::<RequestEnvelope>(&line) {
            Ok(request) => request,
            Err(error) => {
                let revision = service.lock().await.revision().unwrap_or(0);
                write_response(
                    &mut output,
                    &ResponseEnvelope::error(
                        String::new(),
                        revision,
                        ErrorCode::InvalidRequest,
                        error.to_string(),
                    ),
                )
                .await?;
                continue;
            }
        };
        let shutdown = matches!(request.command, Command::Shutdown);
        let request_id = request.request_id.clone();
        let mut service_guard = service.lock().await;
        if shutdown {
            peer_budget_before_shutdown = Some(service_guard.peer_budget_snapshot());
            bandwidth_before_shutdown = Some(service_guard.bandwidth_snapshot().into());
        }
        let response = match service_guard.dispatch(request).await {
            Ok(response) => response,
            Err(error) => application_error_response(
                request_id,
                service_guard.revision().unwrap_or(0),
                &error,
            ),
        };
        drop(service_guard);
        write_response(&mut output, &response).await?;
        if shutdown {
            break;
        }
    }
    let resources = {
        let service = service.lock().await;
        DiagnosticResourceReport {
            download: service.session_download_resource_snapshot(),
            peer_budget: peer_budget_before_shutdown
                .unwrap_or_else(|| service.peer_budget_snapshot()),
            storage_files: service.storage_file_pool_snapshot(),
            bandwidth: bandwidth_before_shutdown
                .unwrap_or_else(|| service.bandwidth_snapshot().into()),
        }
    };
    service.lock().await.shutdown().await?;
    if let Some(path) = resource_report {
        let bytes = serde_json::to_vec_pretty(&resources).map_err(DiagnosticError::Serialize)?;
        tokio::fs::write(path, bytes)
            .await
            .map_err(DiagnosticError::WriteReport)?;
    }
    Ok(())
}

async fn write_response(
    output: &mut BufWriter<tokio::io::Stdout>,
    response: &ResponseEnvelope,
) -> Result<(), DiagnosticError> {
    let mut bytes = serde_json::to_vec(response).map_err(DiagnosticError::Serialize)?;
    bytes.push(b'\n');
    output
        .write_all(&bytes)
        .await
        .map_err(DiagnosticError::WriteOutput)?;
    output.flush().await.map_err(DiagnosticError::WriteOutput)
}

fn parse_arguments(
    arguments: impl Iterator<Item = std::ffi::OsString>,
) -> Result<(ApplicationConfig, Option<PathBuf>), DiagnosticError> {
    let arguments = arguments.collect::<Vec<_>>();
    if arguments.len() > MAX_ARGUMENTS {
        return Err(DiagnosticError::Arguments(
            "too many diagnostic arguments".to_owned(),
        ));
    }
    let mut profile_root = None;
    let mut ephemeral = false;
    let mut profile_id = "default".to_owned();
    let mut resource_report = None;
    let mut storage_roots = Vec::new();
    let mut timeout = Duration::from_secs(120);
    let mut download_resource_limits = DownloadResourceLimits::DESKTOP;
    let mut storage_write_concurrency = 4_usize;
    let mut storage_hash_concurrency = 4_usize;
    let mut checkpoint_sync_delay = Duration::ZERO;
    let mut checkpoint_commit_delay = Duration::ZERO;
    let mut trace_checkpoint_stages = false;
    let mut index = 0;
    while index < arguments.len() {
        let name = arguments[index]
            .to_str()
            .ok_or_else(|| DiagnosticError::Arguments("argument name is not UTF-8".to_owned()))?;
        if name == "--ephemeral" {
            if ephemeral {
                return Err(DiagnosticError::Arguments(
                    "--ephemeral may appear only once".to_owned(),
                ));
            }
            ephemeral = true;
            index += 1;
            continue;
        }
        let value = arguments
            .get(index + 1)
            .ok_or_else(|| DiagnosticError::Arguments(format!("{name} requires one value")))?;
        match name {
            "--profile-root" => {
                set_once(&mut profile_root, PathBuf::from(value), "--profile-root")?;
            }
            "--profile-id" => {
                profile_id = value
                    .to_str()
                    .ok_or_else(|| {
                        DiagnosticError::Arguments("profile ID is not UTF-8".to_owned())
                    })?
                    .to_owned();
            }
            "--resource-report" => {
                set_once(
                    &mut resource_report,
                    PathBuf::from(value),
                    "--resource-report",
                )?;
            }
            "--storage-root" => {
                let value = value.to_str().ok_or_else(|| {
                    DiagnosticError::Arguments("storage root is not UTF-8".to_owned())
                })?;
                let (id, path) = value.split_once('=').ok_or_else(|| {
                    DiagnosticError::Arguments("storage root must have ID=PATH form".to_owned())
                })?;
                if id.is_empty() || path.is_empty() {
                    return Err(DiagnosticError::Arguments(
                        "storage root ID and path must be nonempty".to_owned(),
                    ));
                }
                storage_roots.push(ConfiguredStorageRoot::path(
                    id.to_owned(),
                    PathBuf::from(path),
                ));
            }
            "--timeout-seconds" => {
                timeout = Duration::from_secs(parse_positive_u64(value, name)?);
            }
            "--max-buffered-payload-bytes" => {
                download_resource_limits.max_buffered_payload_bytes =
                    usize::try_from(parse_positive_u64(value, name)?).map_err(|_| {
                        DiagnosticError::Arguments("payload allowance exceeds usize".to_owned())
                    })?;
                download_resource_limits.storage_intake_high_watermark_bytes =
                    DownloadResourceLimits::default_storage_intake_high_watermark(
                        download_resource_limits.max_buffered_payload_bytes,
                    );
            }
            "--storage-write-concurrency" => {
                storage_write_concurrency = parse_storage_concurrency(value, name)?;
            }
            "--storage-hash-concurrency" => {
                storage_hash_concurrency = parse_storage_concurrency(value, name)?;
            }
            "--checkpoint-sync-delay-millis" => {
                checkpoint_sync_delay = Duration::from_millis(parse_positive_u64(value, name)?);
            }
            "--checkpoint-commit-delay-millis" => {
                checkpoint_commit_delay = Duration::from_millis(parse_positive_u64(value, name)?);
            }
            "--trace-checkpoint-stages" => {
                trace_checkpoint_stages = match value.to_str() {
                    Some("true") => true,
                    Some("false") => false,
                    _ => {
                        return Err(DiagnosticError::Arguments(
                            "--trace-checkpoint-stages must be true or false".to_owned(),
                        ));
                    }
                };
            }
            _ => {
                return Err(DiagnosticError::Arguments(format!(
                    "unknown diagnostic argument {name}"
                )));
            }
        }
        index += 2;
    }
    if storage_roots.is_empty() {
        return Err(DiagnosticError::Arguments(
            "at least one --storage-root is required".to_owned(),
        ));
    }
    if ephemeral && profile_root.is_some() {
        return Err(DiagnosticError::Arguments(
            "--ephemeral and --profile-root are mutually exclusive".to_owned(),
        ));
    }
    let network = NetworkConfig::new(NetworkPolicy::LoopbackOnly, timeout, timeout);
    let mut config = if ephemeral {
        ApplicationConfig::ephemeral(profile_id, storage_roots, network)
    } else {
        ApplicationConfig::new(
            profile_root.ok_or_else(|| {
                DiagnosticError::Arguments(
                    "--profile-root is required unless --ephemeral is set".to_owned(),
                )
            })?,
            profile_id,
            storage_roots,
            network,
        )
    };
    config.download_resource_limits = download_resource_limits;
    config.storage_write_concurrency_for_testing = storage_write_concurrency;
    config.storage_hash_concurrency_for_testing = storage_hash_concurrency;
    config.checkpoint_sync_delay_for_testing = checkpoint_sync_delay;
    config.checkpoint_commit_delay_for_testing = checkpoint_commit_delay;
    config.checkpoint_stage_trace_for_testing = trace_checkpoint_stages;
    Ok((config, resource_report))
}

fn parse_storage_concurrency(
    value: &std::ffi::OsStr,
    name: &str,
) -> Result<usize, DiagnosticError> {
    let value = usize::try_from(parse_positive_u64(value, name)?)
        .map_err(|_| DiagnosticError::Arguments(format!("{name} exceeds usize")))?;
    if !(1..=8).contains(&value) {
        return Err(DiagnosticError::Arguments(format!(
            "{name} must be between 1 and 8"
        )));
    }
    Ok(value)
}

fn set_once<T>(target: &mut Option<T>, value: T, name: &str) -> Result<(), DiagnosticError> {
    if target.replace(value).is_some() {
        return Err(DiagnosticError::Arguments(format!(
            "{name} may appear only once"
        )));
    }
    Ok(())
}

fn parse_positive_u64(value: &std::ffi::OsStr, name: &str) -> Result<u64, DiagnosticError> {
    let value = value
        .to_str()
        .ok_or_else(|| DiagnosticError::Arguments(format!("{name} is not UTF-8")))?
        .parse::<u64>()
        .map_err(|_| DiagnosticError::Arguments(format!("{name} is not a positive integer")))?;
    if value == 0 {
        return Err(DiagnosticError::Arguments(format!(
            "{name} must be nonzero"
        )));
    }
    Ok(value)
}

#[derive(Debug)]
enum DiagnosticError {
    Arguments(String),
    Application(rstorrent_session::ApplicationError),
    ReadInput(std::io::Error),
    WriteOutput(std::io::Error),
    WriteReport(std::io::Error),
    Serialize(serde_json::Error),
}

impl fmt::Display for DiagnosticError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Arguments(message) => write!(formatter, "arguments: {message}"),
            Self::Application(error) => write!(formatter, "{error}"),
            Self::ReadInput(error) => write!(formatter, "read diagnostic input: {error}"),
            Self::WriteOutput(error) => write!(formatter, "write diagnostic output: {error}"),
            Self::WriteReport(error) => write!(formatter, "write resource report: {error}"),
            Self::Serialize(error) => write!(formatter, "serialize diagnostic output: {error}"),
        }
    }
}

impl Error for DiagnosticError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Application(error) => Some(error),
            Self::ReadInput(error) | Self::WriteOutput(error) | Self::WriteReport(error) => {
                Some(error)
            }
            Self::Serialize(error) => Some(error),
            Self::Arguments(_) => None,
        }
    }
}

impl From<rstorrent_session::ApplicationError> for DiagnosticError {
    fn from(error: rstorrent_session::ApplicationError) -> Self {
        Self::Application(error)
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use rstorrent_session::ApplicationPersistence;

    use super::{NetworkPolicy, parse_arguments};

    #[test]
    fn parses_profile_and_storage_root() {
        let (config, report) = parse_arguments(
            [
                "--profile-root",
                "/tmp/profile",
                "--profile-id",
                "test",
                "--storage-root",
                "downloads=/tmp/payload",
                "--timeout-seconds",
                "9",
                "--resource-report",
                "/tmp/resources.json",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .expect("parse diagnostic arguments");
        assert_eq!(report, Some(PathBuf::from("/tmp/resources.json")));
        assert_eq!(
            config.persistence,
            ApplicationPersistence::Durable {
                profile_root: PathBuf::from("/tmp/profile")
            }
        );
        assert_eq!(config.profile_id, "test");
        assert_eq!(config.storage_roots[0].id, "downloads");
        assert_eq!(config.network.peer_connect_timeout.as_secs(), 9);
        assert_eq!(config.network.peer_io_timeout.as_secs(), 9);
        assert_eq!(config.network.policy, NetworkPolicy::LoopbackOnly);
    }

    #[test]
    fn requires_root_and_rejects_unbounded_arguments() {
        assert!(parse_arguments(std::iter::empty()).is_err());
        assert!(
            parse_arguments((0..34).map(|index| OsString::from(format!("argument-{index}"))))
                .is_err()
        );
    }

    #[test]
    fn parses_ephemeral_without_a_profile_root_and_rejects_conflict() {
        let (config, report) = parse_arguments(
            ["--ephemeral", "--storage-root", "downloads=/tmp/payload"]
                .into_iter()
                .map(OsString::from),
        )
        .expect("parse ephemeral diagnostic arguments");
        assert!(report.is_none());
        assert_eq!(config.persistence, ApplicationPersistence::Ephemeral);

        assert!(
            parse_arguments(
                [
                    "--ephemeral",
                    "--profile-root",
                    "/tmp/profile",
                    "--storage-root",
                    "downloads=/tmp/payload",
                ]
                .into_iter()
                .map(OsString::from),
            )
            .is_err()
        );
    }
}
