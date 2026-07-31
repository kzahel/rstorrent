use std::collections::BTreeSet;
use std::env;
use std::fmt;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rstorrent_engine::dht::{DhtConfig, DhtService};
use rstorrent_engine::{
    DownloadActivityEvent, DownloadActivitySink, DownloadControl, DownloadDiagnosticSnapshot,
    DownloadError, DownloadReport, MagnetDownloadConfig, NetworkConfig, NetworkPolicy,
    download_magnet_with_control,
};
use serde::Serialize;

const DEFAULT_TIMEOUT_SECONDS: u64 = 120;
const DEFAULT_CLEANUP_SECONDS: u64 = 10;
const DEFAULT_PAYLOAD_LIMIT: usize = 64 * 1024 * 1024;
const MAX_TIMEOUT_SECONDS: u64 = 24 * 60 * 60;
const MAX_CLEANUP_SECONDS: u64 = 60;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Target {
    Metadata,
    FirstPiece,
    FiftyPercent,
    NinetyFivePercent,
    NinetyNinePercent,
    Complete,
}

impl Target {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "metadata" => Ok(Self::Metadata),
            "first-piece" => Ok(Self::FirstPiece),
            "50-percent" => Ok(Self::FiftyPercent),
            "95-percent" => Ok(Self::NinetyFivePercent),
            "99-percent" => Ok(Self::NinetyNinePercent),
            "complete" => Ok(Self::Complete),
            _ => Err(format!("unknown target {value}")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Discovery {
    Tracker,
    Dht,
    Full,
}

impl Discovery {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "tracker" => Ok(Self::Tracker),
            "dht" => Ok(Self::Dht),
            "full" => Ok(Self::Full),
            _ => Err(format!("unknown discovery profile {value}")),
        }
    }

    fn enables_dht(self) -> bool {
        matches!(self, Self::Dht | Self::Full)
    }
}

#[derive(Debug)]
struct Config {
    magnet: String,
    output: PathBuf,
    target: Target,
    discovery: Discovery,
    timeout: Duration,
    cleanup_grace: Duration,
    payload_limit: usize,
}

#[derive(Clone, Debug, Default, Serialize)]
struct Milestones {
    metadata_verified: Option<f64>,
    first_piece_verified: Option<f64>,
    #[serde(rename = "50_percent_verified")]
    fifty_percent_verified: Option<f64>,
    #[serde(rename = "95_percent_verified")]
    ninety_five_percent_verified: Option<f64>,
    #[serde(rename = "99_percent_verified")]
    ninety_nine_percent_verified: Option<f64>,
    all_pieces_verified: Option<f64>,
    published: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize)]
struct Geometry {
    total_length: Option<u64>,
    piece_length: Option<u32>,
    piece_count: Option<usize>,
    file_count: Option<usize>,
}

#[derive(Debug, Default)]
struct Observation {
    milestones: Milestones,
    geometry: Geometry,
    verified_pieces: BTreeSet<u32>,
    verified_bytes: u64,
}

#[derive(Debug)]
struct ProbeSink {
    started: Instant,
    observation: Mutex<Observation>,
}

impl ProbeSink {
    fn new(started: Instant) -> Self {
        Self {
            started,
            observation: Mutex::new(Observation::default()),
        }
    }

    fn snapshot(&self) -> ObservationSnapshot {
        let observation = self
            .observation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        ObservationSnapshot {
            milestones: observation.milestones.clone(),
            geometry: observation.geometry.clone(),
            verified_piece_count: observation.verified_pieces.len(),
            verified_bytes: observation.verified_bytes,
        }
    }

    fn reached(&self, target: Target) -> bool {
        let observation = self
            .observation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match target {
            Target::Metadata => observation.milestones.metadata_verified.is_some(),
            Target::FirstPiece => observation.milestones.first_piece_verified.is_some(),
            Target::FiftyPercent => observation.milestones.fifty_percent_verified.is_some(),
            Target::NinetyFivePercent => observation
                .milestones
                .ninety_five_percent_verified
                .is_some(),
            Target::NinetyNinePercent => observation
                .milestones
                .ninety_nine_percent_verified
                .is_some(),
            Target::Complete => observation.milestones.published.is_some(),
        }
    }

    fn mark_published(&self) {
        let elapsed = self.started.elapsed().as_secs_f64();
        let mut observation = self
            .observation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        observation
            .milestones
            .all_pieces_verified
            .get_or_insert(elapsed);
        observation.milestones.published.get_or_insert(elapsed);
    }
}

impl DownloadActivitySink for ProbeSink {
    fn record(&self, event: DownloadActivityEvent) {
        let elapsed = self.started.elapsed().as_secs_f64();
        let mut observation = self
            .observation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match event {
            DownloadActivityEvent::MetadataVerified {
                total_length,
                piece_length,
                piece_count,
                file_count,
            } => {
                observation.geometry = Geometry {
                    total_length: Some(total_length),
                    piece_length: Some(piece_length),
                    piece_count: Some(piece_count),
                    file_count: Some(file_count),
                };
                observation
                    .milestones
                    .metadata_verified
                    .get_or_insert(elapsed);
            }
            DownloadActivityEvent::PieceVerified { piece_index } => {
                if !observation.verified_pieces.insert(piece_index) {
                    return;
                }
                let (Some(total), Some(piece_length), Some(piece_count)) = (
                    observation.geometry.total_length,
                    observation.geometry.piece_length,
                    observation.geometry.piece_count,
                ) else {
                    return;
                };
                let piece_start = u64::from(piece_index).saturating_mul(u64::from(piece_length));
                let piece_bytes = total
                    .saturating_sub(piece_start)
                    .min(u64::from(piece_length));
                observation.verified_bytes = observation.verified_bytes.saturating_add(piece_bytes);
                observation
                    .milestones
                    .first_piece_verified
                    .get_or_insert(elapsed);
                if crosses(observation.verified_bytes, total, 50) {
                    observation
                        .milestones
                        .fifty_percent_verified
                        .get_or_insert(elapsed);
                }
                if crosses(observation.verified_bytes, total, 95) {
                    observation
                        .milestones
                        .ninety_five_percent_verified
                        .get_or_insert(elapsed);
                }
                if crosses(observation.verified_bytes, total, 99) {
                    observation
                        .milestones
                        .ninety_nine_percent_verified
                        .get_or_insert(elapsed);
                }
                if observation.verified_pieces.len() == piece_count {
                    observation
                        .milestones
                        .all_pieces_verified
                        .get_or_insert(elapsed);
                }
            }
            _ => {}
        }
    }
}

fn crosses(done: u64, total: u64, percent: u64) -> bool {
    total > 0 && u128::from(done) * 100 >= u128::from(total) * u128::from(percent)
}

#[derive(Clone, Debug, Serialize)]
struct ObservationSnapshot {
    milestones: Milestones,
    geometry: Geometry,
    verified_piece_count: usize,
    verified_bytes: u64,
}

#[derive(Debug, Serialize)]
struct Capabilities {
    network_policy: &'static str,
    udp_trackers: bool,
    dht: bool,
    incoming_connections: bool,
    tcp_outgoing: bool,
    utp_outgoing: bool,
    web_seed: bool,
    websocket_trackers: bool,
}

#[derive(Debug, Serialize)]
struct Diagnostics {
    metadata_phase: String,
    candidate_count: Option<usize>,
    eligible_candidates: Option<usize>,
    pending_dials: usize,
    active_metadata_workers: usize,
    metadata_attempts: usize,
    metadata_requests: usize,
    metadata_blocks: usize,
    metadata_bytes: usize,
    metadata_hash_failures: usize,
    metadata_hash_failure_contributors: usize,
    metadata_attempt_details: Vec<MetadataAttemptDiagnostics>,
    connected_peers: Option<usize>,
    unchoked_peers: Option<usize>,
    missing_blocks: Option<usize>,
    requested_blocks: Option<usize>,
    writing_blocks: Option<usize>,
    request_target_total: Option<usize>,
    request_target_max: Option<usize>,
    slow_start_peers: Option<usize>,
    stalled_peers: Option<usize>,
    useful_payload_bytes: Option<usize>,
    observed_payload_rate: Option<usize>,
    no_request_reason: Option<String>,
    requested_bytes: usize,
    received_bytes: usize,
    stored_bytes: usize,
    payload_high_water: usize,
}

#[derive(Debug, Serialize)]
struct MetadataAttemptDiagnostics {
    stage: String,
    started_seconds: f64,
    last_activity_seconds: f64,
    last_progress_seconds: f64,
    supports_extensions: Option<bool>,
    remote_metadata_id: Option<u8>,
    metadata_size: Option<usize>,
    metadata_blocks: Option<usize>,
    requests_sent: usize,
    pending_requests: usize,
    blocks_received: usize,
    bytes_received: usize,
    messages_received: usize,
    rejects_received: usize,
    terminal_detail: Option<String>,
}

#[derive(Debug, Serialize)]
struct ProbeResult {
    schema_version: u32,
    implementation: &'static str,
    outcome: &'static str,
    target: Target,
    wall_seconds: f64,
    milestones: Milestones,
    geometry: Geometry,
    verified_piece_count: usize,
    verified_bytes: u64,
    integrity_verified: bool,
    cleanup_succeeded: bool,
    terminal_detail: Option<String>,
    capabilities: Capabilities,
    diagnostics: Diagnostics,
}

#[derive(Debug)]
struct TerminalState {
    outcome: &'static str,
    integrity_verified: bool,
    cleanup_succeeded: bool,
    detail: Option<String>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let config = match parse_args(env::args().skip(1).collect()) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("argument error: {error}");
            return ExitCode::from(2);
        }
    };
    let result = run(config).await;
    match serde_json::to_string(&result) {
        Ok(json) => println!("{json}"),
        Err(error) => {
            eprintln!("could not serialize probe result: {error}");
            return ExitCode::FAILURE;
        }
    }
    if matches!(result.outcome, "milestone_reached") && result.cleanup_succeeded {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

async fn run(config: Config) -> ProbeResult {
    let started = Instant::now();
    let control = DownloadControl::new();
    let sink = Arc::new(ProbeSink::new(started));
    control.set_activity_sink(sink.clone());

    let mut dht = if config.discovery.enables_dht() {
        match DhtService::start(DhtConfig::for_network(NetworkPolicy::Online)).await {
            Ok(service) => Some(service),
            Err(error) => {
                return result(
                    &config,
                    started,
                    &sink,
                    &control.diagnostic_snapshot(),
                    TerminalState {
                        outcome: "error",
                        integrity_verified: false,
                        cleanup_succeeded: false,
                        detail: Some(format!("DHT startup failed: {error}")),
                    },
                );
            }
        }
    } else {
        None
    };

    let download_config = MagnetDownloadConfig {
        magnet: config.magnet.clone(),
        output_path: config.output.clone(),
        network: NetworkConfig::new(
            NetworkPolicy::Online,
            Duration::from_secs(15),
            Duration::from_secs(15),
        ),
        max_buffered_payload_bytes: config.payload_limit,
        skip_files: Vec::new(),
        materialize_files: Vec::new(),
        dht: dht.as_ref().map(DhtService::handle),
    };
    let task_control = control.clone();
    let mut task =
        tokio::spawn(
            async move { download_magnet_with_control(download_config, task_control).await },
        );
    let deadline = tokio::time::sleep(config.timeout);
    tokio::pin!(deadline);
    let mut reached = false;
    let mut timed_out = false;
    let mut joined: Option<Result<Result<DownloadReport, DownloadError>, tokio::task::JoinError>> =
        None;

    loop {
        if config.target != Target::Complete && sink.reached(config.target) {
            reached = true;
            control.cancel_when_safe();
            break;
        }
        tokio::select! {
            result = &mut task => {
                joined = Some(result);
                break;
            }
            _ = &mut deadline => {
                timed_out = true;
                control.cancel_when_safe();
                break;
            }
            _ = tokio::time::sleep(Duration::from_millis(20)) => {}
        }
    }

    let mut cleanup_succeeded = true;
    if joined.is_none() {
        match tokio::time::timeout(config.cleanup_grace, &mut task).await {
            Ok(result) => joined = Some(result),
            Err(_) => {
                task.abort();
                let _ = task.await;
                cleanup_succeeded = false;
            }
        }
    }
    if let Some(service) = dht.take()
        && tokio::time::timeout(config.cleanup_grace, service.shutdown())
            .await
            .map_or(true, |result| result.is_err())
    {
        cleanup_succeeded = false;
    }

    let terminal = classify_terminal(
        config.target,
        reached,
        timed_out,
        joined,
        &sink,
        cleanup_succeeded,
    );
    result(
        &config,
        started,
        &sink,
        &control.diagnostic_snapshot(),
        terminal,
    )
}

fn classify_terminal(
    target: Target,
    reached: bool,
    timed_out: bool,
    joined: Option<Result<Result<DownloadReport, DownloadError>, tokio::task::JoinError>>,
    sink: &ProbeSink,
    cleanup_succeeded: bool,
) -> TerminalState {
    if !cleanup_succeeded {
        return TerminalState {
            outcome: "harness_error",
            integrity_verified: false,
            cleanup_succeeded: false,
            detail: Some("owned task cleanup failed".to_owned()),
        };
    }
    if timed_out {
        return TerminalState {
            outcome: "timeout",
            integrity_verified: false,
            cleanup_succeeded: true,
            detail: Some("target deadline expired".to_owned()),
        };
    }
    let (outcome, integrity_verified, detail) = match joined {
        Some(Ok(Ok(report))) => {
            if report.verified_piece_count != report.piece_count {
                return TerminalState {
                    outcome: "integrity_failure",
                    integrity_verified: false,
                    cleanup_succeeded: true,
                    detail: Some("download report did not verify every piece".to_owned()),
                };
            }
            sink.mark_published();
            if target == Target::Complete || sink.reached(target) {
                ("milestone_reached", true, None)
            } else {
                (
                    "error",
                    false,
                    Some("download completed without the requested milestone".to_owned()),
                )
            }
        }
        Some(Ok(Err(DownloadError::Cancelled))) if reached && sink.reached(target) => {
            ("milestone_reached", true, None)
        }
        Some(Ok(Err(error))) => ("error", false, Some(error.to_string())),
        Some(Err(error)) => ("harness_error", false, Some(error.to_string())),
        None => (
            "harness_error",
            false,
            Some("download task result missing".to_owned()),
        ),
    };
    TerminalState {
        outcome,
        integrity_verified,
        cleanup_succeeded: true,
        detail,
    }
}

fn result(
    config: &Config,
    started: Instant,
    sink: &ProbeSink,
    diagnostics: &DownloadDiagnosticSnapshot,
    terminal: TerminalState,
) -> ProbeResult {
    let observation = sink.snapshot();
    ProbeResult {
        schema_version: 1,
        implementation: "rstorrent",
        outcome: terminal.outcome,
        target: config.target,
        wall_seconds: started.elapsed().as_secs_f64(),
        milestones: observation.milestones,
        geometry: observation.geometry,
        verified_piece_count: observation.verified_piece_count,
        verified_bytes: observation.verified_bytes,
        integrity_verified: terminal.integrity_verified,
        cleanup_succeeded: terminal.cleanup_succeeded,
        terminal_detail: terminal.detail,
        capabilities: Capabilities {
            network_policy: "online",
            udp_trackers: !matches!(config.discovery, Discovery::Dht),
            dht: config.discovery.enables_dht(),
            incoming_connections: false,
            tcp_outgoing: true,
            utp_outgoing: false,
            web_seed: false,
            websocket_trackers: false,
        },
        diagnostics: diagnostic_result(diagnostics),
    }
}

fn diagnostic_result(snapshot: &DownloadDiagnosticSnapshot) -> Diagnostics {
    let registry = snapshot.metadata.registry.as_ref();
    let swarm = snapshot.swarm.as_ref();
    Diagnostics {
        metadata_phase: format!("{:?}", snapshot.metadata.phase).to_ascii_lowercase(),
        candidate_count: registry.map(|value| value.counts.total),
        eligible_candidates: registry.map(|value| value.counts.eligible),
        pending_dials: snapshot.metadata.pending_dials,
        active_metadata_workers: snapshot.metadata.active_workers,
        metadata_attempts: snapshot.metadata.total_attempts,
        metadata_requests: snapshot.metadata.total_requests_sent,
        metadata_blocks: snapshot.metadata.total_blocks_received,
        metadata_bytes: snapshot.metadata.total_bytes_received,
        metadata_hash_failures: snapshot.metadata.total_hash_failures,
        metadata_hash_failure_contributors: snapshot.metadata.last_hash_failure_contributors,
        metadata_attempt_details: snapshot
            .metadata
            .recent_attempts
            .iter()
            .chain(snapshot.metadata.active_attempts.iter())
            .map(|attempt| MetadataAttemptDiagnostics {
                stage: format!("{:?}", attempt.stage).to_ascii_lowercase(),
                started_seconds: attempt.started_at.as_secs_f64(),
                last_activity_seconds: attempt.last_activity_at.as_secs_f64(),
                last_progress_seconds: attempt.last_progress_at.as_secs_f64(),
                supports_extensions: attempt.supports_extensions,
                remote_metadata_id: attempt.remote_metadata_id,
                metadata_size: attempt.metadata_size,
                metadata_blocks: attempt.metadata_blocks,
                requests_sent: attempt.requests_sent,
                pending_requests: attempt.pending_requests,
                blocks_received: attempt.blocks_received,
                bytes_received: attempt.bytes_received,
                messages_received: attempt.messages_received,
                rejects_received: attempt.rejects_received,
                terminal_detail: attempt.terminal_detail.clone(),
            })
            .collect(),
        connected_peers: swarm.map(|value| value.connected_peers),
        unchoked_peers: swarm.map(|value| value.unchoked_peers),
        missing_blocks: swarm.map(|value| value.missing_blocks),
        requested_blocks: swarm.map(|value| value.requested_blocks),
        writing_blocks: swarm.map(|value| value.writing_blocks),
        request_target_total: swarm.map(|value| value.request_target_total),
        request_target_max: swarm.map(|value| value.request_target_max),
        slow_start_peers: swarm.map(|value| value.slow_start_peers),
        stalled_peers: swarm.map(|value| value.stalled_peers),
        useful_payload_bytes: swarm.map(|value| value.useful_payload_bytes),
        observed_payload_rate: swarm.map(|value| value.observed_payload_rate),
        no_request_reason: swarm
            .and_then(|value| value.no_request_reason)
            .map(|value| format!("{value:?}").to_ascii_lowercase()),
        requested_bytes: snapshot.progress.requested_bytes,
        received_bytes: snapshot.progress.received_bytes,
        stored_bytes: snapshot.progress.stored_bytes,
        payload_high_water: snapshot.progress.payload_high_water,
    }
}

fn parse_args(arguments: Vec<String>) -> Result<Config, String> {
    let mut magnet = None;
    let mut output = None;
    let mut target = Target::Complete;
    let mut discovery = Discovery::Tracker;
    let mut timeout_seconds = DEFAULT_TIMEOUT_SECONDS;
    let mut cleanup_seconds = DEFAULT_CLEANUP_SECONDS;
    let mut payload_limit = DEFAULT_PAYLOAD_LIMIT;
    let mut index = 0;
    while index < arguments.len() {
        let flag = &arguments[index];
        index += 1;
        let value = arguments
            .get(index)
            .ok_or_else(|| format!("{flag} requires a value"))?;
        index += 1;
        match flag.as_str() {
            "--magnet" => set_once(&mut magnet, value.clone(), flag)?,
            "--output" => set_once(&mut output, PathBuf::from(value), flag)?,
            "--target" => target = Target::parse(value)?,
            "--discovery" => discovery = Discovery::parse(value)?,
            "--timeout-seconds" => {
                timeout_seconds = bounded_u64(value, flag, 1, MAX_TIMEOUT_SECONDS)?;
            }
            "--cleanup-seconds" => {
                cleanup_seconds = bounded_u64(value, flag, 1, MAX_CLEANUP_SECONDS)?;
            }
            "--max-buffered-payload-bytes" => {
                payload_limit = value
                    .parse()
                    .map_err(|_| format!("{flag} must be an integer"))?;
                if !(16 * 1024..=1024 * 1024 * 1024).contains(&payload_limit) {
                    return Err(format!("{flag} is outside the supported range"));
                }
            }
            _ => return Err(format!("unknown argument {flag}")),
        }
    }
    Ok(Config {
        magnet: magnet.ok_or_else(|| "--magnet is required".to_owned())?,
        output: output.ok_or_else(|| "--output is required".to_owned())?,
        target,
        discovery,
        timeout: Duration::from_secs(timeout_seconds),
        cleanup_grace: Duration::from_secs(cleanup_seconds),
        payload_limit,
    })
}

fn bounded_u64(value: &str, flag: &str, minimum: u64, maximum: u64) -> Result<u64, String> {
    let value = value
        .parse::<u64>()
        .map_err(|_| format!("{flag} must be an integer"))?;
    if !(minimum..=maximum).contains(&value) {
        return Err(format!("{flag} must be between {minimum} and {maximum}"));
    }
    Ok(value)
}

fn set_once<T>(slot: &mut Option<T>, value: T, flag: &str) -> Result<(), String> {
    if slot.replace(value).is_some() {
        Err(format!("{flag} may only be specified once"))
    } else {
        Ok(())
    }
}

impl fmt::Display for Target {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::crosses;

    #[test]
    fn percentage_thresholds_do_not_round_down() {
        assert!(!crosses(49, 100, 50));
        assert!(crosses(1, 1, 50));
        assert!(crosses(95, 100, 95));
        assert!(!crosses(94, 100, 95));
    }
}
