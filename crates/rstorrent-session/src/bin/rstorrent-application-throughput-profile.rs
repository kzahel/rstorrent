#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use rstorrent_session::{
    ApplicationConfig, ApplicationService, CONTROL_VERSION, Command, ConfiguredStorageRoot,
    DiagnosticFilter, DiagnosticProfile, DiagnosticSeverity, NetworkConfig, NetworkPolicy,
    OpenViewSetOptions, OpenViewSetRequest, RequestEnvelope, ResponseOutcome, TorrentState,
    UpdateBatch, ViewDeliveryPolicy, ViewSet, ViewSetOwner, ViewSetUpdate, ViewSpec,
};
use serde::Serialize;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

const MAX_ARGUMENTS: usize = 24;
const MAX_PAYLOAD_BYTES: u64 = 10 * 1024 * 1024 * 1024;
const MAX_TIMEOUT_SECONDS: u64 = 4 * 60 * 60;
const MAX_CONSUMER_DELAY_MILLIS: u64 = 60_000;
const STATUS_POLL_INTERVAL: Duration = Duration::from_secs(1);
const FILE_POLL_INTERVAL: Duration = Duration::from_millis(10);
const DELIVERY_WAIT_MILLIS: u32 = 20_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ViewMode {
    Idle,
    Library,
    General,
    Peers,
    Files,
    Trackers,
    Pieces,
    Disk,
    LogsNormal,
    LogsDetailed,
    LogsTrace,
    All,
    SlowAll,
}

impl ViewMode {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "idle" => Some(Self::Idle),
            "library" => Some(Self::Library),
            "general" => Some(Self::General),
            "peers" => Some(Self::Peers),
            "files" => Some(Self::Files),
            "trackers" => Some(Self::Trackers),
            "pieces" => Some(Self::Pieces),
            "disk" => Some(Self::Disk),
            "logs-normal" => Some(Self::LogsNormal),
            "logs-detailed" => Some(Self::LogsDetailed),
            "logs-trace" => Some(Self::LogsTrace),
            "all" => Some(Self::All),
            "slow-all" => Some(Self::SlowAll),
            _ => None,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Library => "library",
            Self::General => "general",
            Self::Peers => "peers",
            Self::Files => "files",
            Self::Trackers => "trackers",
            Self::Pieces => "pieces",
            Self::Disk => "disk",
            Self::LogsNormal => "logs-normal",
            Self::LogsDetailed => "logs-detailed",
            Self::LogsTrace => "logs-trace",
            Self::All => "all",
            Self::SlowAll => "slow-all",
        }
    }

    fn specs(self, torrent_id: &str) -> Vec<ViewSpec> {
        if self == Self::Idle {
            return Vec::new();
        }
        let mut specs = vec![torrent_list()];
        if self != Self::Library {
            specs.push(torrent_summary(torrent_id));
        }
        match self {
            Self::Idle | Self::Library | Self::General => {}
            Self::Peers => specs.push(torrent_peers(torrent_id)),
            Self::Files => specs.push(torrent_files(torrent_id)),
            Self::Trackers => specs.push(torrent_trackers(torrent_id)),
            Self::Pieces => specs.push(piece_activity(torrent_id)),
            Self::Disk => specs.push(session_disk()),
            Self::LogsNormal => specs.push(diagnostics(
                DiagnosticProfile::Normal,
                DiagnosticSeverity::Info,
            )),
            Self::LogsDetailed => specs.push(diagnostics(
                DiagnosticProfile::Detailed,
                DiagnosticSeverity::Debug,
            )),
            Self::LogsTrace => specs.push(diagnostics(
                DiagnosticProfile::Trace,
                DiagnosticSeverity::Trace,
            )),
            Self::All | Self::SlowAll => {
                specs.extend([
                    torrent_peers(torrent_id),
                    torrent_files(torrent_id),
                    torrent_trackers(torrent_id),
                    piece_activity(torrent_id),
                    session_disk(),
                    diagnostics(DiagnosticProfile::Trace, DiagnosticSeverity::Trace),
                ]);
            }
        }
        specs
    }
}

#[derive(Debug)]
struct Arguments {
    profile_root: PathBuf,
    payload_root: PathBuf,
    magnet: String,
    torrent_id: String,
    payload_bytes: u64,
    mode: ViewMode,
    timeout: Duration,
    consumer_delay: Duration,
    write_concurrency: usize,
    hash_concurrency: usize,
}

#[derive(Default, Serialize)]
struct DeliveryEvidence {
    batches: u64,
    empty_batches: u64,
    updates: u64,
    snapshot_updates: u64,
    patch_updates: u64,
    view_removed_updates: u64,
    reset_updates: u64,
    serialized_bytes: u64,
    per_view_updates: BTreeMap<String, u64>,
    queue_bytes_at_end: usize,
    queue_high_water_bytes: usize,
    view_set_reset_count: u64,
}

#[derive(Serialize)]
struct RunReport {
    schema_version: u16,
    scenario: &'static str,
    mode: &'static str,
    payload_bytes: u64,
    transfer_seconds: f64,
    throughput_mib_s: f64,
    completion_polls: u64,
    piece_count: u32,
    verified_piece_count: u32,
    views: Vec<ViewSpec>,
    consumer_delay_millis: u64,
    delivery: DeliveryEvidence,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("application throughput profile failed: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn Error>> {
    let arguments = parse_arguments(env::args_os().skip(1))?;
    let mut config = ApplicationConfig::new(
        arguments.profile_root.clone(),
        "throughput-profile".to_owned(),
        vec![ConfiguredStorageRoot::path(
            "downloads".to_owned(),
            arguments.payload_root.clone(),
        )],
        NetworkConfig::new(
            NetworkPolicy::LoopbackOnly,
            arguments.timeout,
            arguments.timeout,
        ),
    );
    config.storage_write_concurrency_for_testing = arguments.write_concurrency;
    config.storage_hash_concurrency_for_testing = arguments.hash_concurrency;
    let mut service = ApplicationService::open(config).await?;

    let specs = arguments.mode.specs(&arguments.torrent_id);
    let owner = ViewSetOwner::trusted("application-throughput-profile");
    let cancellation = CancellationToken::new();
    let mut view_set = None;
    let mut consumer = None;
    if !specs.is_empty() {
        let opened = service.open_view_set(
            owner.clone(),
            OpenViewSetRequest {
                views: specs.clone(),
                options: OpenViewSetOptions::default(),
            },
        )?;
        let retained = service.view_set(&owner, &opened.view_set_id)?;
        consumer = Some(spawn_consumer(
            retained.clone(),
            opened.initial,
            arguments.consumer_delay,
            cancellation.clone(),
        ));
        view_set = Some(retained);
    }

    let started = Instant::now();
    let response = service
        .dispatch(RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: "profile-add".to_owned(),
            expected_revision: None,
            command: Command::AddMagnet {
                magnet: arguments.magnet.clone(),
                storage_root: "downloads".to_owned(),
                start_content: true,
                skip_files: Vec::new(),
            },
        })
        .await?;
    ensure_success(response)?;

    let final_root = arguments.payload_root.join(&arguments.torrent_id);
    let deadline = started + arguments.timeout;
    let mut next_status_poll = started;
    let mut completion_polls = 0_u64;
    let final_torrent = loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(invalid_input(format!(
                "mode {} exceeded {} seconds",
                arguments.mode.as_str(),
                arguments.timeout.as_secs()
            ))
            .into());
        }
        if final_root.is_dir() || now >= next_status_poll {
            completion_polls = completion_polls.saturating_add(1);
            let response = service
                .dispatch(RequestEnvelope {
                    version: CONTROL_VERSION,
                    request_id: format!("profile-snapshot-{completion_polls}"),
                    expected_revision: None,
                    command: Command::Snapshot,
                })
                .await?;
            let snapshot = ensure_success(response)?;
            let torrent = snapshot
                .torrents
                .into_iter()
                .find(|torrent| torrent.torrent_id == arguments.torrent_id)
                .ok_or_else(|| invalid_input("application snapshot lost the test torrent"))?;
            match torrent.state {
                TorrentState::Complete => break torrent,
                TorrentState::NeedsRepair | TorrentState::Error => {
                    return Err(invalid_input(format!(
                        "application entered {:?}: {}",
                        torrent.state,
                        torrent.error.as_deref().unwrap_or("no error detail")
                    ))
                    .into());
                }
                _ => {}
            }
            next_status_poll = now + STATUS_POLL_INTERVAL;
        }
        tokio::time::sleep(FILE_POLL_INTERVAL).await;
    };
    let transfer_seconds = started.elapsed().as_secs_f64();

    if consumer.is_some() {
        tokio::time::sleep(arguments.consumer_delay + Duration::from_millis(300)).await;
    }
    cancellation.cancel();
    let mut delivery = match consumer {
        Some(task) => task
            .await
            .map_err(|error| invalid_input(error.to_string()))??,
        None => DeliveryEvidence::default(),
    };
    if let Some(retained) = &view_set {
        let stats = retained.stats()?;
        delivery.queue_bytes_at_end = stats.queued_bytes;
        delivery.queue_high_water_bytes = stats.queue_high_water;
        delivery.view_set_reset_count = stats.reset_count;
        service.close_view_set(&owner, retained.id())?;
    }
    service.shutdown().await?;

    if final_torrent.verified_piece_count != final_torrent.piece_count
        || final_torrent.piece_count == 0
    {
        return Err(invalid_input("complete application snapshot has incomplete pieces").into());
    }
    let report = RunReport {
        schema_version: 1,
        scenario: "sqlite-application-view-throughput",
        mode: arguments.mode.as_str(),
        payload_bytes: arguments.payload_bytes,
        transfer_seconds,
        throughput_mib_s: arguments.payload_bytes as f64 / (1024.0 * 1024.0) / transfer_seconds,
        completion_polls,
        piece_count: final_torrent.piece_count,
        verified_piece_count: final_torrent.verified_piece_count,
        views: specs,
        consumer_delay_millis: u64::try_from(arguments.consumer_delay.as_millis())
            .unwrap_or(u64::MAX),
        delivery,
    };
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}

fn spawn_consumer(
    view_set: ViewSet,
    initial: UpdateBatch,
    delay: Duration,
    cancellation: CancellationToken,
) -> JoinHandle<Result<DeliveryEvidence, io::Error>> {
    tokio::spawn(async move {
        let mut evidence = DeliveryEvidence::default();
        record_batch(&mut evidence, &initial)?;
        let mut cursor = initial.cursor;
        loop {
            if !delay.is_zero() {
                tokio::select! {
                    () = cancellation.cancelled() => break,
                    () = tokio::time::sleep(delay) => {}
                }
            }
            let batch = tokio::select! {
                () = cancellation.cancelled() => break,
                result = view_set.next_updates(&cursor, DELIVERY_WAIT_MILLIS) => {
                    result.map_err(|error| invalid_input(error.to_string()))?
                }
            };
            if batch.base_cursor != cursor {
                return Err(invalid_input(format!(
                    "view batch base {} did not match cursor {}",
                    batch.base_cursor, cursor
                )));
            }
            cursor = batch.cursor.clone();
            record_batch(&mut evidence, &batch)?;
        }
        Ok(evidence)
    })
}

fn record_batch(evidence: &mut DeliveryEvidence, batch: &UpdateBatch) -> Result<(), io::Error> {
    evidence.batches = evidence.batches.saturating_add(1);
    if batch.updates.is_empty() {
        evidence.empty_batches = evidence.empty_batches.saturating_add(1);
    }
    let encoded = serde_json::to_vec(batch).map_err(invalid_data)?;
    evidence.serialized_bytes = evidence
        .serialized_bytes
        .saturating_add(u64::try_from(encoded.len()).unwrap_or(u64::MAX));
    for update in &batch.updates {
        evidence.updates = evidence.updates.saturating_add(1);
        let view_id = match update {
            ViewSetUpdate::Snapshot { view_id, .. } => {
                evidence.snapshot_updates = evidence.snapshot_updates.saturating_add(1);
                Some(view_id)
            }
            ViewSetUpdate::Patch { view_id, .. } => {
                evidence.patch_updates = evidence.patch_updates.saturating_add(1);
                Some(view_id)
            }
            ViewSetUpdate::ViewRemoved { view_id } => {
                evidence.view_removed_updates = evidence.view_removed_updates.saturating_add(1);
                Some(view_id)
            }
            ViewSetUpdate::ResetRequired { view_id, .. } => {
                evidence.reset_updates = evidence.reset_updates.saturating_add(1);
                view_id.as_ref()
            }
        };
        if let Some(view_id) = view_id {
            let count = evidence
                .per_view_updates
                .entry(view_id.clone())
                .or_default();
            *count = count.saturating_add(1);
        }
    }
    Ok(())
}

fn ensure_success(
    response: rstorrent_session::ResponseEnvelope,
) -> Result<rstorrent_session::ServiceSnapshot, io::Error> {
    match response.outcome {
        ResponseOutcome::Success { snapshot } => Ok(snapshot),
        ResponseOutcome::Error { error } => Err(invalid_input(format!(
            "application command failed with {:?}: {}",
            error.code, error.message
        ))),
    }
}

fn parse_arguments(
    arguments: impl Iterator<Item = std::ffi::OsString>,
) -> Result<Arguments, io::Error> {
    let arguments = arguments.collect::<Vec<_>>();
    if arguments.len() > MAX_ARGUMENTS || arguments.len() % 2 != 0 {
        return Err(invalid_input("invalid number of diagnostic arguments"));
    }
    let mut profile_root = None;
    let mut payload_root = None;
    let mut magnet = None;
    let mut torrent_id = None;
    let mut payload_bytes = None;
    let mut mode = None;
    let mut timeout = Duration::from_secs(600);
    let mut consumer_delay = Duration::from_millis(1_000);
    let mut write_concurrency = 4_usize;
    let mut hash_concurrency = 4_usize;
    for pair in arguments.chunks_exact(2) {
        let name = pair[0]
            .to_str()
            .ok_or_else(|| invalid_input("argument name is not UTF-8"))?;
        let value = &pair[1];
        match name {
            "--profile-root" => {
                set_once(&mut profile_root, PathBuf::from(value), "--profile-root")?
            }
            "--payload-root" => {
                set_once(&mut payload_root, PathBuf::from(value), "--payload-root")?
            }
            "--magnet" => set_once(&mut magnet, utf8(value, name)?.to_owned(), name)?,
            "--torrent-id" => {
                let value = utf8(value, name)?;
                if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return Err(invalid_input("--torrent-id must be 40 hexadecimal bytes"));
                }
                set_once(&mut torrent_id, value.to_ascii_lowercase(), name)?;
            }
            "--payload-bytes" => {
                let value = parse_u64(value, name, 1, MAX_PAYLOAD_BYTES)?;
                set_once(&mut payload_bytes, value, name)?;
            }
            "--mode" => {
                let value = utf8(value, name)?;
                let parsed = ViewMode::parse(value)
                    .ok_or_else(|| invalid_input(format!("unknown view mode {value}")))?;
                set_once(&mut mode, parsed, name)?;
            }
            "--timeout-seconds" => {
                timeout = Duration::from_secs(parse_u64(value, name, 1, MAX_TIMEOUT_SECONDS)?);
            }
            "--consumer-delay-millis" => {
                consumer_delay =
                    Duration::from_millis(parse_u64(value, name, 1, MAX_CONSUMER_DELAY_MILLIS)?);
            }
            "--write-concurrency" => {
                write_concurrency = parse_usize(value, name, 1, 8)?;
            }
            "--hash-concurrency" => {
                hash_concurrency = parse_usize(value, name, 1, 8)?;
            }
            _ => return Err(invalid_input(format!("unknown argument {name}"))),
        }
    }
    let mode = mode.ok_or_else(|| invalid_input("--mode is required"))?;
    Ok(Arguments {
        profile_root: profile_root.ok_or_else(|| invalid_input("--profile-root is required"))?,
        payload_root: payload_root.ok_or_else(|| invalid_input("--payload-root is required"))?,
        magnet: magnet.ok_or_else(|| invalid_input("--magnet is required"))?,
        torrent_id: torrent_id.ok_or_else(|| invalid_input("--torrent-id is required"))?,
        payload_bytes: payload_bytes.ok_or_else(|| invalid_input("--payload-bytes is required"))?,
        mode,
        timeout,
        consumer_delay: if mode == ViewMode::SlowAll {
            consumer_delay
        } else {
            Duration::ZERO
        },
        write_concurrency,
        hash_concurrency,
    })
}

fn torrent_list() -> ViewSpec {
    ViewSpec::TorrentList {
        view_id: "library".to_owned(),
        delivery: delivery(100),
    }
}

fn torrent_summary(torrent_id: &str) -> ViewSpec {
    ViewSpec::TorrentSummary {
        view_id: "torrent-summary".to_owned(),
        torrent_id: torrent_id.to_owned(),
        delivery: delivery(100),
    }
}

fn torrent_peers(torrent_id: &str) -> ViewSpec {
    ViewSpec::TorrentPeers {
        view_id: "torrent-peers".to_owned(),
        torrent_id: torrent_id.to_owned(),
        delivery: delivery(100),
    }
}

fn torrent_files(torrent_id: &str) -> ViewSpec {
    ViewSpec::TorrentFiles {
        view_id: "torrent-files".to_owned(),
        torrent_id: torrent_id.to_owned(),
        delivery: delivery(250),
    }
}

fn torrent_trackers(torrent_id: &str) -> ViewSpec {
    ViewSpec::TorrentTrackers {
        view_id: "torrent-trackers".to_owned(),
        torrent_id: torrent_id.to_owned(),
        delivery: delivery(250),
    }
}

fn piece_activity(torrent_id: &str) -> ViewSpec {
    ViewSpec::PieceActivity {
        view_id: "torrent-pieces".to_owned(),
        torrent_id: torrent_id.to_owned(),
        delivery: delivery(100),
    }
}

fn session_disk() -> ViewSpec {
    ViewSpec::SessionDisk {
        view_id: "session-disk".to_owned(),
        delivery: delivery(100),
    }
}

fn diagnostics(profile: DiagnosticProfile, minimum_severity: DiagnosticSeverity) -> ViewSpec {
    ViewSpec::Diagnostics {
        view_id: "logs".to_owned(),
        torrent_id: None,
        filter: DiagnosticFilter {
            profile,
            minimum_severity,
            categories: Vec::new(),
        },
        delivery: delivery(100),
    }
}

const fn delivery(min_interval_millis: u32) -> ViewDeliveryPolicy {
    ViewDeliveryPolicy {
        min_interval_millis,
    }
}

fn utf8<'a>(value: &'a std::ffi::OsStr, name: &str) -> Result<&'a str, io::Error> {
    value
        .to_str()
        .ok_or_else(|| invalid_input(format!("{name} is not UTF-8")))
}

fn parse_u64(
    value: &std::ffi::OsStr,
    name: &str,
    minimum: u64,
    maximum: u64,
) -> Result<u64, io::Error> {
    let parsed = utf8(value, name)?
        .parse::<u64>()
        .map_err(|_| invalid_input(format!("{name} must be an integer")))?;
    if !(minimum..=maximum).contains(&parsed) {
        return Err(invalid_input(format!(
            "{name} must be between {minimum} and {maximum}"
        )));
    }
    Ok(parsed)
}

fn parse_usize(
    value: &std::ffi::OsStr,
    name: &str,
    minimum: usize,
    maximum: usize,
) -> Result<usize, io::Error> {
    let parsed = parse_u64(value, name, minimum as u64, maximum as u64)?;
    usize::try_from(parsed).map_err(|_| invalid_input(format!("{name} exceeds usize")))
}

fn set_once<T>(target: &mut Option<T>, value: T, name: &str) -> Result<(), io::Error> {
    if target.replace(value).is_some() {
        return Err(invalid_input(format!("{name} may appear only once")));
    }
    Ok(())
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn invalid_data(error: serde_json::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TORRENT_ID: &str = "000102030405060708090a0b0c0d0e0f10111213";

    #[test]
    fn production_modes_keep_real_common_and_delivery_intervals() {
        let peers = ViewMode::Peers.specs(TORRENT_ID);
        assert_eq!(peers.len(), 3);
        assert!(matches!(peers[0], ViewSpec::TorrentList { .. }));
        assert!(matches!(peers[1], ViewSpec::TorrentSummary { .. }));
        assert!(matches!(peers[2], ViewSpec::TorrentPeers { .. }));

        let files = ViewMode::Files.specs(TORRENT_ID);
        assert!(matches!(
            files.last(),
            Some(ViewSpec::TorrentFiles {
                delivery: ViewDeliveryPolicy {
                    min_interval_millis: 250
                },
                ..
            })
        ));
    }

    #[test]
    fn adversarial_modes_request_every_view_once() {
        for mode in [ViewMode::All, ViewMode::SlowAll] {
            let specs = mode.specs(TORRENT_ID);
            assert_eq!(specs.len(), 8);
            let ids = specs
                .iter()
                .map(ViewSpec::view_id)
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(ids.len(), specs.len());
            assert!(specs.iter().any(|spec| matches!(
                spec,
                ViewSpec::Diagnostics {
                    filter: DiagnosticFilter {
                        profile: DiagnosticProfile::Trace,
                        ..
                    },
                    ..
                }
            )));
        }
    }
}
