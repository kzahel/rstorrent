//! Controlled interoperability owner for an application-backed completed seed.

use std::env;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use rstorrent_protocol::metainfo::{BEP9_METAINFO_LIMITS, Metainfo};
use rstorrent_session::{
    ApplicationConfig, ApplicationService, CONTROL_VERSION, Command, ConfiguredStorageRoot,
    IncomingTcpBootstrap, NetworkConfig, NetworkPolicy, RequestEnvelope, ResponseOutcome,
    SessionStore, StorageState, StoreError,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::time::timeout;

const READY_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("incoming seed failed: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), SeedHarnessError> {
    let arguments = Arguments::parse(env::args_os().skip(1))?;
    let outer = std::fs::read(&arguments.metainfo).map_err(|source| SeedHarnessError::Io {
        operation: "read metainfo",
        source,
    })?;
    if outer.len() > BEP9_METAINFO_LIMITS.max_outer_bytes {
        return Err(SeedHarnessError::Arguments(format!(
            "metainfo exceeds {} bytes",
            BEP9_METAINFO_LIMITS.max_outer_bytes
        )));
    }
    let metainfo = Metainfo::from_bytes_with_limits(&outer, BEP9_METAINFO_LIMITS)
        .map_err(|error| SeedHarnessError::Metainfo(error.to_string()))?;
    let raw_info = Metainfo::info_bytes_with_limits(&outer, BEP9_METAINFO_LIMITS)
        .map_err(|error| SeedHarnessError::Metainfo(error.to_string()))?
        .to_vec();
    std::fs::create_dir_all(&arguments.storage_root).map_err(|source| SeedHarnessError::Io {
        operation: "create storage root",
        source,
    })?;
    let storage_roots = vec![ConfiguredStorageRoot::path(
        "downloads",
        arguments.storage_root.clone(),
    )];
    initialize_catalog(
        &arguments.profile_root,
        &storage_roots,
        &metainfo,
        &raw_info,
    )?;

    let mut config = ApplicationConfig::new(
        arguments.profile_root,
        "incoming-interop".to_owned(),
        storage_roots,
        NetworkConfig::new(
            NetworkPolicy::LoopbackOnly,
            Duration::from_secs(5),
            Duration::from_secs(5),
        ),
    );
    config.incoming_tcp = IncomingTcpBootstrap::AutomaticLoopback;
    let mut service = ApplicationService::open(config).await?;
    let ready = timeout(READY_TIMEOUT, async {
        loop {
            if let Some(snapshot) = service.incoming_peer_snapshot()
                && snapshot.registrations == 1
            {
                break snapshot;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| SeedHarnessError::ReadinessTimeout)?;
    let ready_json = serde_json::json!({
        "event": "ready",
        "info_hash": hex(metainfo.info_hash),
        "listen": ready.listen_address.to_string(),
        "registrations": ready.registrations,
        "pending_high_water": ready.pending_high_water,
        "established_high_water": ready.established_high_water,
        "connection_high_water": ready.peer_budget.total_high_water,
        "upload_slots_high_water": ready.upload_slots_high_water,
        "queued_requests_high_water": ready.queued_requests_high_water,
        "queued_bytes_high_water": ready.queued_bytes_high_water,
        "read_high_water": ready.read_high_water,
        "read_bytes_high_water": ready.read_bytes_high_water,
        "payload_bytes_sent": ready.payload_bytes_sent,
    });
    let mut stdout = tokio::io::stdout();
    stdout
        .write_all(format!("{ready_json}\n").as_bytes())
        .await
        .map_err(|source| SeedHarnessError::Io {
            operation: "write ready observation",
            source,
        })?;
    stdout
        .flush()
        .await
        .map_err(|source| SeedHarnessError::Io {
            operation: "flush ready observation",
            source,
        })?;

    let mut shutdown = String::new();
    BufReader::new(tokio::io::stdin())
        .read_line(&mut shutdown)
        .await
        .map_err(|source| SeedHarnessError::Io {
            operation: "read shutdown request",
            source,
        })?;
    let final_snapshot = service
        .incoming_peer_snapshot()
        .expect("enabled incoming service remains owned before shutdown");
    service.shutdown().await?;
    let stopped_json = serde_json::json!({
        "event": "stopped",
        "pending_before_shutdown": final_snapshot.pending,
        "established_before_shutdown": final_snapshot.established,
        "reads_before_shutdown": final_snapshot.reads,
        "payload_bytes_sent": final_snapshot.payload_bytes_sent,
        "pending_high_water": final_snapshot.pending_high_water,
        "established_high_water": final_snapshot.established_high_water,
        "connection_high_water": final_snapshot.peer_budget.total_high_water,
        "upload_regular_high_water": final_snapshot.upload_regular_high_water,
        "upload_optimistic_high_water": final_snapshot.upload_optimistic_high_water,
        "upload_slots_high_water": final_snapshot.upload_slots_high_water,
        "queued_requests_high_water": final_snapshot.queued_requests_high_water,
        "queued_bytes_high_water": final_snapshot.queued_bytes_high_water,
        "read_high_water": final_snapshot.read_high_water,
        "read_bytes_high_water": final_snapshot.read_bytes_high_water,
        "writer_send_buffer_high_water": final_snapshot.writer_send_buffer_high_water,
    });
    stdout
        .write_all(format!("{stopped_json}\n").as_bytes())
        .await
        .map_err(|source| SeedHarnessError::Io {
            operation: "write stopped observation",
            source,
        })?;
    stdout
        .flush()
        .await
        .map_err(|source| SeedHarnessError::Io {
            operation: "flush stopped observation",
            source,
        })?;
    Ok(())
}

fn initialize_catalog(
    profile_root: &std::path::Path,
    storage_roots: &[ConfiguredStorageRoot],
    metainfo: &Metainfo,
    raw_info: &[u8],
) -> Result<(), SeedHarnessError> {
    let torrent_id = hex(metainfo.info_hash);
    let mut store = SessionStore::open(profile_root, "incoming-interop", storage_roots)?;
    match store.load_resume(&torrent_id) {
        Ok(resume)
            if resume.state == rstorrent_session::TorrentState::Complete
                && resume.storage_state == StorageState::Published =>
        {
            return Ok(());
        }
        Ok(_) => {
            return Err(SeedHarnessError::Catalog(
                "existing fixture catalog row is not complete and published".to_owned(),
            ));
        }
        Err(StoreError::UnknownTorrent(_)) => {}
        Err(error) => return Err(error.into()),
    }
    let response = store.handle_durable(&RequestEnvelope {
        version: CONTROL_VERSION,
        request_id: "initialize-incoming-seed".to_owned(),
        expected_revision: None,
        command: Command::AddMagnet {
            magnet: format!("magnet:?xt=urn:btih:{torrent_id}"),
            storage_root: "downloads".to_owned(),
            start_content: true,
            skip_files: Vec::new(),
        },
    })?;
    if !matches!(response.outcome, ResponseOutcome::Success { .. }) {
        return Err(SeedHarnessError::Catalog(
            "fixture add request was rejected".to_owned(),
        ));
    }
    store.record_metadata(&torrent_id, raw_info)?;
    store.record_pieces(
        &torrent_id,
        &(0..metainfo.piece_count()).collect::<Vec<_>>(),
    )?;
    store.mark_storage_prepared(&torrent_id, StorageState::Published)?;
    store.mark_complete(&torrent_id)?;
    Ok(())
}

#[derive(Debug)]
struct Arguments {
    profile_root: PathBuf,
    storage_root: PathBuf,
    metainfo: PathBuf,
}

impl Arguments {
    fn parse(
        arguments: impl Iterator<Item = std::ffi::OsString>,
    ) -> Result<Self, SeedHarnessError> {
        let arguments = arguments.collect::<Vec<_>>();
        if arguments.len() != 6 {
            return Err(SeedHarnessError::Arguments(
                "usage: rstorrent-incoming-seed --profile-root PATH --storage-root PATH --metainfo PATH"
                    .to_owned(),
            ));
        }
        let mut profile_root = None;
        let mut storage_root = None;
        let mut metainfo = None;
        for pair in arguments.chunks_exact(2) {
            let flag = pair[0]
                .to_str()
                .ok_or_else(|| SeedHarnessError::Arguments("flag is not UTF-8".to_owned()))?;
            let target = match flag {
                "--profile-root" => &mut profile_root,
                "--storage-root" => &mut storage_root,
                "--metainfo" => &mut metainfo,
                _ => {
                    return Err(SeedHarnessError::Arguments(format!(
                        "unknown argument {flag}"
                    )));
                }
            };
            if target.replace(PathBuf::from(&pair[1])).is_some() {
                return Err(SeedHarnessError::Arguments(format!(
                    "{flag} may appear only once"
                )));
            }
        }
        Ok(Self {
            profile_root: profile_root.ok_or_else(|| {
                SeedHarnessError::Arguments("--profile-root is required".to_owned())
            })?,
            storage_root: storage_root.ok_or_else(|| {
                SeedHarnessError::Arguments("--storage-root is required".to_owned())
            })?,
            metainfo: metainfo
                .ok_or_else(|| SeedHarnessError::Arguments("--metainfo is required".to_owned()))?,
        })
    }
}

fn hex(bytes: [u8; 20]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Debug)]
enum SeedHarnessError {
    Arguments(String),
    Metainfo(String),
    Catalog(String),
    ReadinessTimeout,
    Io {
        operation: &'static str,
        source: std::io::Error,
    },
    Store(StoreError),
    Application(rstorrent_session::ApplicationError),
}

impl fmt::Display for SeedHarnessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Arguments(error) | Self::Metainfo(error) | Self::Catalog(error) => {
                formatter.write_str(error)
            }
            Self::ReadinessTimeout => formatter.write_str("seed readiness timed out"),
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::Store(error) => write!(formatter, "{error}"),
            Self::Application(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for SeedHarnessError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Store(error) => Some(error),
            Self::Application(error) => Some(error),
            _ => None,
        }
    }
}

impl From<StoreError> for SeedHarnessError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<rstorrent_session::ApplicationError> for SeedHarnessError {
    fn from(error: rstorrent_session::ApplicationError) -> Self {
        Self::Application(error)
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::Arguments;

    #[test]
    fn requires_each_bounded_path_argument_once() {
        let parsed = Arguments::parse(
            [
                "--profile-root",
                "profile",
                "--storage-root",
                "storage",
                "--metainfo",
                "fixture.torrent",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .expect("parse harness arguments");
        assert_eq!(parsed.profile_root.to_string_lossy(), "profile");
        assert!(Arguments::parse(std::iter::empty()).is_err());
    }
}
