use std::env;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use rstorrent_session::{
    ApplicationConfig, ApplicationService, Command, ConfiguredStorageRoot, ErrorCode,
    NetworkConfig, NetworkPolicy, RequestEnvelope, ResponseEnvelope, application_error_response,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};

const MAX_ARGUMENTS: usize = 32;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("error={error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), DiagnosticError> {
    let config = parse_arguments(env::args_os().skip(1))?;
    let mut service = ApplicationService::open(config).await?;
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut output = BufWriter::new(tokio::io::stdout());
    while let Some(line) = lines
        .next_line()
        .await
        .map_err(DiagnosticError::ReadInput)?
    {
        if line.len() > 64 * 1024 {
            write_response(
                &mut output,
                &ResponseEnvelope::error(
                    String::new(),
                    service.revision().unwrap_or(0),
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
                write_response(
                    &mut output,
                    &ResponseEnvelope::error(
                        String::new(),
                        service.revision().unwrap_or(0),
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
        let response = match service.dispatch(request).await {
            Ok(response) => response,
            Err(error) => {
                application_error_response(request_id, service.revision().unwrap_or(0), &error)
            }
        };
        write_response(&mut output, &response).await?;
        if shutdown {
            break;
        }
    }
    service.shutdown().await?;
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
) -> Result<ApplicationConfig, DiagnosticError> {
    let arguments = arguments.collect::<Vec<_>>();
    if arguments.len() > MAX_ARGUMENTS {
        return Err(DiagnosticError::Arguments(
            "too many diagnostic arguments".to_owned(),
        ));
    }
    let mut profile_root = None;
    let mut profile_id = "default".to_owned();
    let mut storage_roots = Vec::new();
    let mut timeout = Duration::from_secs(120);
    let mut max_buffered_payload_bytes = 32 * 1024;
    let mut index = 0;
    while index < arguments.len() {
        let name = arguments[index]
            .to_str()
            .ok_or_else(|| DiagnosticError::Arguments("argument name is not UTF-8".to_owned()))?;
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
                max_buffered_payload_bytes = usize::try_from(parse_positive_u64(value, name)?)
                    .map_err(|_| {
                        DiagnosticError::Arguments("payload allowance exceeds usize".to_owned())
                    })?;
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
    let mut config = ApplicationConfig::new(
        profile_root
            .ok_or_else(|| DiagnosticError::Arguments("--profile-root is required".to_owned()))?,
        profile_id,
        storage_roots,
        NetworkConfig::new(NetworkPolicy::LoopbackOnly, timeout, timeout),
    );
    config.max_buffered_payload_bytes = max_buffered_payload_bytes;
    Ok(config)
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
    Serialize(serde_json::Error),
}

impl fmt::Display for DiagnosticError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Arguments(message) => write!(formatter, "arguments: {message}"),
            Self::Application(error) => write!(formatter, "{error}"),
            Self::ReadInput(error) => write!(formatter, "read diagnostic input: {error}"),
            Self::WriteOutput(error) => write!(formatter, "write diagnostic output: {error}"),
            Self::Serialize(error) => write!(formatter, "serialize diagnostic output: {error}"),
        }
    }
}

impl Error for DiagnosticError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Application(error) => Some(error),
            Self::ReadInput(error) | Self::WriteOutput(error) => Some(error),
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

    use super::{NetworkPolicy, parse_arguments};

    #[test]
    fn parses_profile_and_storage_root() {
        let config = parse_arguments(
            [
                "--profile-root",
                "/tmp/profile",
                "--profile-id",
                "test",
                "--storage-root",
                "downloads=/tmp/payload",
                "--timeout-seconds",
                "9",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .expect("parse diagnostic arguments");
        assert_eq!(config.profile_root, PathBuf::from("/tmp/profile"));
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
}
