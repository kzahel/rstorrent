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
const UTILITY_SAMPLE_INTERVAL: Duration = Duration::from_secs(1);
const MAX_UTILITY_SAMPLES: usize = 1024;

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

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
struct IntegerDistribution {
    count: usize,
    min: Option<usize>,
    median: Option<usize>,
    p90: Option<usize>,
    max: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct UtilitySample {
    elapsed_seconds: f64,
    verified_piece_count: usize,
    verified_bytes: u64,
    verified_rate: Option<u64>,
    tracker_response_batches: Option<u64>,
    tracker_reported_peers: Option<u64>,
    dht_response_batches: Option<u64>,
    dht_reported_peers: Option<u64>,
    dial_attempts: Option<u64>,
    known_peers: Option<usize>,
    eligible_peers: Option<usize>,
    connecting_peers: Option<usize>,
    connected_peers: Option<usize>,
    unchoked_peers: Option<usize>,
    wanted_peers: Option<usize>,
    ever_useful_peers: Option<usize>,
    active_payload_peers: Option<usize>,
    stalled_peers: Option<usize>,
    zero_payload_peers: Option<usize>,
    active_requests: Option<usize>,
    request_queue_bytes: Option<usize>,
    request_target: Option<usize>,
    writing_blocks: Option<usize>,
    storage_jobs: Option<usize>,
    pending_disk_bytes: Option<usize>,
    payload_rate: Option<usize>,
    peer_payload_rates: IntegerDistribution,
    peer_request_queues: IntegerDistribution,
}

#[derive(Debug, Default)]
struct UtilityTimeline {
    samples: Vec<UtilitySample>,
    coalesced_samples: usize,
    previous_verified: Option<(f64, u64)>,
}

impl UtilityTimeline {
    fn record(
        &mut self,
        elapsed: Duration,
        observation: &ObservationSnapshot,
        snapshot: &DownloadDiagnosticSnapshot,
    ) {
        let elapsed_seconds = elapsed.as_secs_f64();
        let verified_rate = self
            .previous_verified
            .and_then(|(previous_at, previous_bytes)| {
                let interval = elapsed_seconds - previous_at;
                (interval > 0.0).then(|| {
                    (observation.verified_bytes.saturating_sub(previous_bytes) as f64 / interval)
                        .round() as u64
                })
            });
        self.previous_verified = Some((elapsed_seconds, observation.verified_bytes));

        let registry = snapshot.content_registry.as_ref();
        let swarm = snapshot.swarm.as_ref();
        let peers = &snapshot.content_peers;
        let sample = UtilitySample {
            elapsed_seconds,
            verified_piece_count: observation.verified_piece_count,
            verified_bytes: observation.verified_bytes,
            verified_rate,
            tracker_response_batches: Some(observation.tracker_response_batches),
            tracker_reported_peers: Some(observation.tracker_reported_peers),
            dht_response_batches: Some(observation.dht_response_batches),
            dht_reported_peers: Some(observation.dht_reported_peers),
            dial_attempts: Some(observation.peer_dial_attempts),
            known_peers: registry.map(|value| value.total),
            eligible_peers: registry.map(|value| value.eligible),
            connecting_peers: registry.map(|value| value.dialing),
            connected_peers: swarm.map(|value| value.connected_peers),
            unchoked_peers: swarm.map(|value| value.unchoked_peers),
            wanted_peers: Some(
                peers
                    .iter()
                    .filter(|peer| peer.wanted_piece_count > 0)
                    .count(),
            ),
            ever_useful_peers: Some(
                peers
                    .iter()
                    .filter(|peer| peer.useful_payload_bytes > 0)
                    .count(),
            ),
            active_payload_peers: Some(
                peers
                    .iter()
                    .filter(|peer| peer.observed_payload_rate > 0)
                    .count(),
            ),
            stalled_peers: swarm.map(|value| value.stalled_peers),
            zero_payload_peers: Some(
                peers
                    .iter()
                    .filter(|peer| peer.useful_payload_bytes == 0)
                    .count(),
            ),
            active_requests: swarm.map(|value| value.active_request_attempts),
            request_queue_bytes: Some(peers.iter().map(|peer| peer.queued_payload_bytes).sum()),
            request_target: swarm.map(|value| value.request_target_total),
            writing_blocks: swarm.map(|value| value.writing_blocks),
            storage_jobs: Some(snapshot.progress.storage_jobs_pending),
            pending_disk_bytes: None,
            payload_rate: swarm.map(|value| value.observed_payload_rate),
            peer_payload_rates: integer_distribution(
                peers.iter().map(|peer| peer.observed_payload_rate),
            ),
            peer_request_queues: integer_distribution(
                peers.iter().map(|peer| peer.pending_requests),
            ),
        };
        self.push(sample);
    }

    fn push(&mut self, sample: UtilitySample) {
        if self.samples.len() >= MAX_UTILITY_SAMPLES {
            let previous_len = self.samples.len();
            self.samples = self
                .samples
                .drain(..)
                .enumerate()
                .filter_map(|(index, sample)| (index == 0 || index % 2 == 1).then_some(sample))
                .collect();
            self.coalesced_samples = self
                .coalesced_samples
                .saturating_add(previous_len.saturating_sub(self.samples.len()));
        }
        self.samples.push(sample);
    }
}

fn integer_distribution(values: impl IntoIterator<Item = usize>) -> IntegerDistribution {
    let mut values = values.into_iter().collect::<Vec<_>>();
    if values.is_empty() {
        return IntegerDistribution::default();
    }
    values.sort_unstable();
    let count = values.len();
    let p90 = count.saturating_mul(9).div_ceil(10).saturating_sub(1);
    IntegerDistribution {
        count,
        min: values.first().copied(),
        median: values.get((count - 1) / 2).copied(),
        p90: values.get(p90).copied(),
        max: values.last().copied(),
    }
}

#[derive(Debug, Default)]
struct Observation {
    milestones: Milestones,
    geometry: Geometry,
    verified_pieces: BTreeSet<u32>,
    verified_bytes: u64,
    tracker_response_batches: u64,
    tracker_reported_peers: u64,
    dht_response_batches: u64,
    dht_reported_peers: u64,
    peer_dial_attempts: u64,
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
            tracker_response_batches: observation.tracker_response_batches,
            tracker_reported_peers: observation.tracker_reported_peers,
            dht_response_batches: observation.dht_response_batches,
            dht_reported_peers: observation.dht_reported_peers,
            peer_dial_attempts: observation.peer_dial_attempts,
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
            DownloadActivityEvent::TrackerAnnounceSucceeded { peer_count, .. } => {
                observation.tracker_response_batches =
                    observation.tracker_response_batches.saturating_add(1);
                observation.tracker_reported_peers = observation
                    .tracker_reported_peers
                    .saturating_add(u64::from(peer_count));
            }
            DownloadActivityEvent::PeerDialStarted { .. } => {
                observation.peer_dial_attempts = observation.peer_dial_attempts.saturating_add(1);
            }
            DownloadActivityEvent::DhtLookupSucceeded { peer_count } => {
                observation.dht_response_batches =
                    observation.dht_response_batches.saturating_add(1);
                observation.dht_reported_peers = observation
                    .dht_reported_peers
                    .saturating_add(u64::from(peer_count));
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
    tracker_response_batches: u64,
    tracker_reported_peers: u64,
    dht_response_batches: u64,
    dht_reported_peers: u64,
    peer_dial_attempts: u64,
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
    utility_timeline: Vec<UtilitySample>,
    utility_timeline_coalesced_samples: usize,
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
    tracker_response_batches: u64,
    tracker_reported_peers: u64,
    peer_dial_attempts: u64,
    content_candidate_count: Option<usize>,
    content_eligible_candidates: Option<usize>,
    content_dialing_candidates: Option<usize>,
    content_backed_off_candidates: Option<usize>,
    content_failure_limited_candidates: Option<usize>,
    content_peers_captured_at_seconds: Option<f64>,
    content_peers: Vec<ContentPeerDiagnostics>,
    connected_peers: Option<usize>,
    unchoked_peers: Option<usize>,
    missing_blocks: Option<usize>,
    requested_blocks: Option<usize>,
    active_request_attempts: Option<usize>,
    active_duplicate_attempts: Option<usize>,
    writing_blocks: Option<usize>,
    request_target_total: Option<usize>,
    request_target_max: Option<usize>,
    slow_start_peers: Option<usize>,
    stalled_peers: Option<usize>,
    useful_payload_bytes: Option<usize>,
    observed_payload_rate: Option<usize>,
    endgame_assignments: Option<usize>,
    cancelled_request_attempts: Option<usize>,
    redundant_payload_bytes: Option<usize>,
    piece_hash_failures: Option<usize>,
    failed_piece_bytes: Option<usize>,
    last_hash_failure_contributors: Option<usize>,
    request_timeout_min_seconds: Option<u64>,
    request_timeout_max_seconds: Option<u64>,
    no_request_reason: Option<String>,
    requested_bytes: usize,
    received_bytes: usize,
    stored_bytes: usize,
    payload_high_water: usize,
    storage_jobs_pending: usize,
    storage_jobs_high_water: usize,
    storage_command_queue_high_water: usize,
    storage_completion_queue_high_water: usize,
    storage_hashes_started: usize,
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
struct ContentPeerDiagnostics {
    connection_id: u64,
    choking: bool,
    wanted_piece_count: usize,
    pending_requests: usize,
    target_requests: usize,
    queued_payload_bytes: usize,
    window_phase: String,
    useful_payload_bytes: usize,
    observed_payload_rate: usize,
    connected_age_seconds: u64,
    last_useful_age_seconds: Option<u64>,
    last_payload_age_seconds: Option<u64>,
    request_timeout_seconds: u64,
    oldest_request_age_seconds: Option<u64>,
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
    let mut utility_timeline = UtilityTimeline::default();

    let mut dht = if config.discovery.enables_dht() {
        match DhtService::start(DhtConfig::for_network(NetworkPolicy::Online)).await {
            Ok(service) => Some(service),
            Err(error) => {
                return result(
                    &config,
                    started,
                    &sink,
                    &control.diagnostic_snapshot(),
                    &utility_timeline,
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
    let mut next_utility_sample = Duration::ZERO;
    let mut joined: Option<Result<Result<DownloadReport, DownloadError>, tokio::task::JoinError>> =
        None;

    loop {
        let elapsed = started.elapsed();
        if sink.reached(Target::Metadata)
            && (utility_timeline.samples.is_empty() || elapsed >= next_utility_sample)
        {
            utility_timeline.record(elapsed, &sink.snapshot(), &control.diagnostic_snapshot());
            next_utility_sample = elapsed.saturating_add(UTILITY_SAMPLE_INTERVAL);
        }
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
    if sink.reached(Target::Metadata) {
        utility_timeline.record(
            started.elapsed(),
            &sink.snapshot(),
            &control.diagnostic_snapshot(),
        );
    }
    result(
        &config,
        started,
        &sink,
        &control.diagnostic_snapshot(),
        &utility_timeline,
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
    utility_timeline: &UtilityTimeline,
    terminal: TerminalState,
) -> ProbeResult {
    let observation = sink.snapshot();
    let diagnostics = diagnostic_result(diagnostics, &observation, utility_timeline);
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
        diagnostics,
    }
}

fn diagnostic_result(
    snapshot: &DownloadDiagnosticSnapshot,
    observation: &ObservationSnapshot,
    utility_timeline: &UtilityTimeline,
) -> Diagnostics {
    let registry = snapshot.metadata.registry.as_ref();
    let content_registry = snapshot.content_registry.as_ref();
    let swarm = snapshot.swarm.as_ref();
    Diagnostics {
        utility_timeline: utility_timeline.samples.clone(),
        utility_timeline_coalesced_samples: utility_timeline.coalesced_samples,
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
        tracker_response_batches: observation.tracker_response_batches,
        tracker_reported_peers: observation.tracker_reported_peers,
        peer_dial_attempts: observation.peer_dial_attempts,
        content_candidate_count: content_registry.map(|value| value.total),
        content_eligible_candidates: content_registry.map(|value| value.eligible),
        content_dialing_candidates: content_registry.map(|value| value.dialing),
        content_backed_off_candidates: content_registry.map(|value| value.backed_off),
        content_failure_limited_candidates: content_registry.map(|value| value.failure_limited),
        content_peers_captured_at_seconds: snapshot
            .content_peers_captured_at
            .map(|value| value.as_secs_f64()),
        content_peers: snapshot
            .content_peers
            .iter()
            .map(|peer| ContentPeerDiagnostics {
                connection_id: peer.connection_id,
                choking: peer.choking,
                wanted_piece_count: peer.wanted_piece_count,
                pending_requests: peer.pending_requests,
                target_requests: peer.target_requests,
                queued_payload_bytes: peer.queued_payload_bytes,
                window_phase: peer.window_phase.as_str().to_owned(),
                useful_payload_bytes: peer.useful_payload_bytes,
                observed_payload_rate: peer.observed_payload_rate,
                connected_age_seconds: peer.connected_age_seconds,
                last_useful_age_seconds: peer.last_useful_age_seconds,
                last_payload_age_seconds: peer.last_payload_age_seconds,
                request_timeout_seconds: peer.request_timeout_seconds,
                oldest_request_age_seconds: peer.oldest_request_age_seconds,
            })
            .collect(),
        connected_peers: swarm.map(|value| value.connected_peers),
        unchoked_peers: swarm.map(|value| value.unchoked_peers),
        missing_blocks: swarm.map(|value| value.missing_blocks),
        requested_blocks: swarm.map(|value| value.requested_blocks),
        active_request_attempts: swarm.map(|value| value.active_request_attempts),
        active_duplicate_attempts: swarm.map(|value| value.active_duplicate_attempts),
        writing_blocks: swarm.map(|value| value.writing_blocks),
        request_target_total: swarm.map(|value| value.request_target_total),
        request_target_max: swarm.map(|value| value.request_target_max),
        slow_start_peers: swarm.map(|value| value.slow_start_peers),
        stalled_peers: swarm.map(|value| value.stalled_peers),
        useful_payload_bytes: swarm.map(|value| value.useful_payload_bytes),
        observed_payload_rate: swarm.map(|value| value.observed_payload_rate),
        endgame_assignments: swarm.map(|value| value.endgame_assignments),
        cancelled_request_attempts: swarm.map(|value| value.cancelled_request_attempts),
        redundant_payload_bytes: swarm.map(|value| value.redundant_payload_bytes),
        piece_hash_failures: swarm.map(|value| value.piece_hash_failures),
        failed_piece_bytes: swarm.map(|value| value.failed_piece_bytes),
        last_hash_failure_contributors: swarm.map(|value| value.last_hash_failure_contributors),
        request_timeout_min_seconds: swarm.and_then(|value| value.request_timeout_min_seconds),
        request_timeout_max_seconds: swarm.and_then(|value| value.request_timeout_max_seconds),
        no_request_reason: swarm
            .and_then(|value| value.no_request_reason)
            .map(|value| format!("{value:?}").to_ascii_lowercase()),
        requested_bytes: snapshot.progress.requested_bytes,
        received_bytes: snapshot.progress.received_bytes,
        stored_bytes: snapshot.progress.stored_bytes,
        payload_high_water: snapshot.progress.payload_high_water,
        storage_jobs_pending: snapshot.progress.storage_jobs_pending,
        storage_jobs_high_water: snapshot.progress.storage_jobs_high_water,
        storage_command_queue_high_water: snapshot.progress.storage_command_queue_high_water,
        storage_completion_queue_high_water: snapshot.progress.storage_completion_queue_high_water,
        storage_hashes_started: snapshot.progress.storage_hashes_started,
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
    use std::time::{Duration, Instant};

    use rstorrent_engine::{
        DownloadDiagnosticSnapshot, DownloadProgress, MetadataAcquisitionSnapshot,
    };

    use super::{
        DownloadActivityEvent, DownloadActivitySink, Geometry, IntegerDistribution,
        MAX_UTILITY_SAMPLES, Milestones, ObservationSnapshot, ProbeSink, UtilitySample,
        UtilityTimeline, crosses, integer_distribution,
    };

    #[test]
    fn percentage_thresholds_do_not_round_down() {
        assert!(!crosses(49, 100, 50));
        assert!(crosses(1, 1, 50));
        assert!(crosses(95, 100, 95));
        assert!(!crosses(94, 100, 95));
    }

    #[test]
    fn discovery_diagnostics_accumulate_without_endpoint_retention() {
        let sink = ProbeSink::new(Instant::now());
        for peer_count in [3, 7] {
            sink.record(DownloadActivityEvent::TrackerAnnounceSucceeded {
                tracker: "redacted by aggregate".to_owned(),
                peer_count,
                interval_seconds: 600,
            });
        }
        sink.record(DownloadActivityEvent::PeerDialStarted {
            peer: "redacted by aggregate".to_owned(),
        });
        sink.record(DownloadActivityEvent::DhtLookupSucceeded { peer_count: 5 });

        let snapshot = sink.snapshot();
        assert_eq!(snapshot.tracker_response_batches, 2);
        assert_eq!(snapshot.tracker_reported_peers, 10);
        assert_eq!(snapshot.dht_response_batches, 1);
        assert_eq!(snapshot.dht_reported_peers, 5);
        assert_eq!(snapshot.peer_dial_attempts, 1);
    }

    #[test]
    fn utility_distribution_uses_bounded_nearest_rank_values() {
        assert_eq!(
            integer_distribution([9, 0, 5, 1, 8, 3, 2, 7, 6, 4]),
            IntegerDistribution {
                count: 10,
                min: Some(0),
                median: Some(4),
                p90: Some(8),
                max: Some(9),
            }
        );
        assert_eq!(
            integer_distribution(std::iter::empty()),
            IntegerDistribution::default()
        );
    }

    #[test]
    fn utility_timeline_rates_and_coalescing_are_bounded() {
        let mut timeline = UtilityTimeline::default();
        let diagnostics = DownloadDiagnosticSnapshot {
            progress: DownloadProgress::default(),
            swarm: None,
            content_peers_captured_at: None,
            content_peers: Vec::new(),
            content_registry: None,
            metadata: MetadataAcquisitionSnapshot::default(),
        };
        timeline.record(Duration::ZERO, &observation_snapshot(0, 0), &diagnostics);
        timeline.record(
            Duration::from_secs(2),
            &observation_snapshot(1, 200),
            &diagnostics,
        );
        assert_eq!(timeline.samples[0].verified_rate, None);
        assert_eq!(timeline.samples[1].verified_rate, Some(100));

        for ordinal in 2..=MAX_UTILITY_SAMPLES {
            timeline.push(utility_sample(ordinal as f64));
        }
        assert!(timeline.samples.len() <= MAX_UTILITY_SAMPLES);
        assert_eq!(
            timeline
                .samples
                .first()
                .map(|sample| sample.elapsed_seconds),
            Some(0.0)
        );
        assert_eq!(
            timeline.samples.last().map(|sample| sample.elapsed_seconds),
            Some(MAX_UTILITY_SAMPLES as f64)
        );
        assert!(timeline.coalesced_samples > 0);
    }

    fn observation_snapshot(
        verified_piece_count: usize,
        verified_bytes: u64,
    ) -> ObservationSnapshot {
        ObservationSnapshot {
            milestones: Milestones::default(),
            geometry: Geometry::default(),
            verified_piece_count,
            verified_bytes,
            tracker_response_batches: 0,
            tracker_reported_peers: 0,
            dht_response_batches: 0,
            dht_reported_peers: 0,
            peer_dial_attempts: 0,
        }
    }

    fn utility_sample(elapsed_seconds: f64) -> UtilitySample {
        UtilitySample {
            elapsed_seconds,
            verified_piece_count: 0,
            verified_bytes: 0,
            verified_rate: None,
            tracker_response_batches: None,
            tracker_reported_peers: None,
            dht_response_batches: None,
            dht_reported_peers: None,
            dial_attempts: None,
            known_peers: None,
            eligible_peers: None,
            connecting_peers: None,
            connected_peers: None,
            unchoked_peers: None,
            wanted_peers: None,
            ever_useful_peers: None,
            active_payload_peers: None,
            stalled_peers: None,
            zero_payload_peers: None,
            active_requests: None,
            request_queue_bytes: None,
            request_target: None,
            writing_blocks: None,
            storage_jobs: None,
            pending_disk_bytes: None,
            payload_rate: None,
            peer_payload_rates: IntegerDistribution::default(),
            peer_request_queues: IntegerDistribution::default(),
        }
    }
}
