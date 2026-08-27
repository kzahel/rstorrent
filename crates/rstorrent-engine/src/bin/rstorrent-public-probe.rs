use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rstorrent_engine::dht::{DhtConfig, DhtObservation, DhtService};
use rstorrent_engine::{
    AddressFamily, DownloadActivityEvent, DownloadActivitySink, DownloadCheckpointSink,
    DownloadConfig, DownloadControl, DownloadDiagnosticSnapshot, DownloadError, DownloadReport,
    DownloadResourceLimits, MseDhWorkOwner, NetworkConfig, NetworkPolicy, PeerBudget,
    PeerBudgetConfig, PeerConnectionLifecycle, PeerConnectionObservation, PeerEncryptionPolicy,
    PeerEncryptionPolicyHandle, PeerTransport, ResumableMagnetDownloadConfig, ResumeArtifactState,
    ResumeValidationIntent, ResumedStorage, SessionUdpService, SessionUdpSnapshot, TorrentId,
    TorrentIdentityContext, TorrentPeerActivitySink, TorrentPeerHandle, TrackerConfig,
    TrackerEndpoint, TrackerSource, UtpService, UtpServiceSnapshot, UtpTerminalEvidence,
    download_verified_piece_with_peer_state, resume_magnet_with_control, select_global_ipv6,
};
use rstorrent_protocol::identity::V1InfoHash;
use rstorrent_protocol::magnet::{Magnet, UdpTrackerUrl};
use rstorrent_protocol::metainfo::{
    EXPLICIT_IMPORT_METAINFO_LIMITS, Metainfo, MetainfoTrackerTransport,
};
use rstorrent_protocol::mse::MseMethod;
use serde::Serialize;
use tokio::net::UdpSocket;

const DEFAULT_TIMEOUT_SECONDS: u64 = 120;
const DEFAULT_CLEANUP_SECONDS: u64 = 10;
const DEFAULT_PAYLOAD_LIMIT: usize = 64 * 1024 * 1024;
const MAX_TIMEOUT_SECONDS: u64 = 24 * 60 * 60;
const MAX_CLEANUP_SECONDS: u64 = 60;
const UTILITY_SAMPLE_INTERVAL: Duration = Duration::from_secs(1);
const MAX_UTILITY_SAMPLES: usize = 1024;
const DHT_HEALTHY_ROUTING_NODES: u16 = 8;
const MAX_METAINFO_BYTES: u64 = 64 * 1024 * 1024;
const DEFAULT_WIRE_PAYLOAD_LIMIT: u64 = 512 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Target {
    Metadata,
    FirstPiece,
    TenPercent,
    FiftyPercent,
    NinetyPercent,
    NinetyFivePercent,
    NinetyNinePercent,
    Complete,
}

impl Target {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "metadata" => Ok(Self::Metadata),
            "first-piece" => Ok(Self::FirstPiece),
            "10-percent" => Ok(Self::TenPercent),
            "50-percent" => Ok(Self::FiftyPercent),
            "90-percent" => Ok(Self::NinetyPercent),
            "95-percent" => Ok(Self::NinetyFivePercent),
            "99-percent" => Ok(Self::NinetyNinePercent),
            "complete" => Ok(Self::Complete),
            _ => Err(format!("unknown target {value}")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Profile {
    MatchedPlain30,
    MatchedRc430,
    ProductDefault,
    ProductUtp,
    DhtOnly,
    WanTcp,
    WanUtp,
}

impl Profile {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "matched-plain-30" | "common" => Ok(Self::MatchedPlain30),
            "matched-rc4-30" => Ok(Self::MatchedRc430),
            "product-default" | "full-reference" => Ok(Self::ProductDefault),
            "product-utp" => Ok(Self::ProductUtp),
            "dht-only" | "dht" => Ok(Self::DhtOnly),
            "wan-tcp" => Ok(Self::WanTcp),
            "wan-utp" => Ok(Self::WanUtp),
            _ => Err(format!("unknown comparison profile {value}")),
        }
    }

    const fn discovery(self) -> Discovery {
        match self {
            Self::MatchedPlain30 | Self::MatchedRc430 => Discovery::Tracker,
            Self::ProductDefault | Self::ProductUtp => Discovery::Full,
            Self::DhtOnly => Discovery::Dht,
            Self::WanTcp | Self::WanUtp => Discovery::Direct,
        }
    }

    const fn encryption(self) -> PeerEncryptionPolicy {
        match self {
            Self::MatchedPlain30 => PeerEncryptionPolicy::Disabled,
            Self::MatchedRc430 => PeerEncryptionPolicy::Required,
            Self::ProductDefault | Self::ProductUtp | Self::DhtOnly => PeerEncryptionPolicy::Allow,
            Self::WanTcp | Self::WanUtp => PeerEncryptionPolicy::Disabled,
        }
    }

    const fn peer_exchange(self) -> bool {
        matches!(
            self,
            Self::ProductDefault | Self::ProductUtp | Self::DhtOnly
        )
    }

    const fn connection_limit(self) -> usize {
        match self {
            Self::MatchedPlain30 | Self::MatchedRc430 => 30,
            Self::ProductDefault | Self::DhtOnly => 200,
            Self::ProductUtp => 30,
            Self::WanTcp | Self::WanUtp => 1,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::MatchedPlain30 => "matched-plain-30",
            Self::MatchedRc430 => "matched-rc4-30",
            Self::ProductDefault => "product-default",
            Self::ProductUtp => "product-utp",
            Self::DhtOnly => "dht-only",
            Self::WanTcp => "wan-tcp",
            Self::WanUtp => "wan-utp",
        }
    }

    const fn enables_utp(self) -> bool {
        matches!(self, Self::ProductUtp | Self::WanUtp)
    }
}

#[derive(Debug)]
enum ProbeInput {
    Magnet(String),
    Metainfo(PathBuf),
}

impl ProbeInput {
    const fn mode(&self) -> &'static str {
        match self {
            Self::Magnet(_) => "magnet",
            Self::Metainfo(_) => "metainfo",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Discovery {
    Tracker,
    Dht,
    Full,
    Direct,
}

impl Discovery {
    const fn enables_dht(self) -> bool {
        matches!(self, Self::Dht | Self::Full)
    }

    const fn enables_trackers(self) -> bool {
        matches!(self, Self::Tracker | Self::Full)
    }
}

#[derive(Debug)]
struct Config {
    input: ProbeInput,
    expected_info_hash: [u8; 20],
    peer_hints: Vec<SocketAddr>,
    output: PathBuf,
    target: Target,
    profile: Profile,
    profile_sha256: String,
    timeout: Duration,
    cleanup_grace: Duration,
    payload_limit: usize,
    storage_intake_high_watermark: usize,
    wire_payload_limit: u64,
    checkpoint_sync_bypassed: bool,
    summary_activity_observation: bool,
    nonresumable_execution: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
struct Milestones {
    process_ready: Option<f64>,
    torrent_admitted: Option<f64>,
    metadata_verified: Option<f64>,
    first_candidate: Option<f64>,
    first_connection: Option<f64>,
    first_payload_byte: Option<f64>,
    last_payload_byte: Option<f64>,
    last_block_stored: Option<f64>,
    first_piece_verified: Option<f64>,
    #[serde(rename = "10_percent_verified")]
    ten_percent_verified: Option<f64>,
    #[serde(rename = "50_percent_verified")]
    fifty_percent_verified: Option<f64>,
    #[serde(rename = "90_percent_verified")]
    ninety_percent_verified: Option<f64>,
    #[serde(rename = "95_percent_verified")]
    ninety_five_percent_verified: Option<f64>,
    #[serde(rename = "99_percent_verified")]
    ninety_nine_percent_verified: Option<f64>,
    all_pieces_verified: Option<f64>,
    published: Option<f64>,
    owner_stopped: Option<f64>,
    shutdown_joined: Option<f64>,
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
    backed_off_peers: Option<usize>,
    failure_limited_peers: Option<usize>,
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
    storage_queue_wait_micros: Option<u64>,
    storage_write_service_micros: Option<u64>,
    storage_hash_service_micros: Option<u64>,
    storage_write_blocks_completed: Option<usize>,
    storage_write_batch_blocks_high_water: Option<usize>,
    storage_write_batch_bytes_high_water: Option<usize>,
    storage_active_kind: Option<&'static str>,
    storage_active_age_micros: Option<u64>,
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
            backed_off_peers: registry.map(|value| value.backed_off),
            failure_limited_peers: registry.map(|value| value.failure_limited),
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
            storage_queue_wait_micros: Some(
                snapshot
                    .progress
                    .storage_write_queue_wait_micros
                    .saturating_add(snapshot.progress.storage_hash_queue_wait_micros),
            ),
            storage_write_service_micros: Some(snapshot.progress.storage_write_service_micros),
            storage_hash_service_micros: Some(snapshot.progress.storage_hash_service_micros),
            storage_write_blocks_completed: Some(snapshot.progress.storage_write_blocks_completed),
            storage_write_batch_blocks_high_water: Some(
                snapshot.progress.storage_write_batch_blocks_high_water,
            ),
            storage_write_batch_bytes_high_water: Some(
                snapshot.progress.storage_write_batch_bytes_high_water,
            ),
            storage_active_kind: if snapshot.progress.storage_active_write_micros.is_some() {
                Some("write")
            } else if snapshot.progress.storage_active_hash_micros.is_some() {
                Some("hash")
            } else {
                None
            },
            storage_active_age_micros: snapshot
                .progress
                .storage_active_write_micros
                .or(snapshot.progress.storage_active_hash_micros),
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
    received_payload_bytes: u64,
    stored_payload_bytes: u64,
    tracker_response_batches: u64,
    tracker_reported_peers: u64,
    dht_response_batches: u64,
    dht_reported_peers: u64,
    peer_dial_attempts: u64,
}

#[derive(Debug)]
struct ProbeSink {
    started: Instant,
    summary_activity_observation: bool,
    observation: Mutex<Observation>,
}

impl ProbeSink {
    fn new(started: Instant, summary_activity_observation: bool) -> Self {
        Self {
            started,
            summary_activity_observation,
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
            Target::TenPercent => observation.milestones.ten_percent_verified.is_some(),
            Target::FiftyPercent => observation.milestones.fifty_percent_verified.is_some(),
            Target::NinetyPercent => observation.milestones.ninety_percent_verified.is_some(),
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

    fn mark_process_ready(&self) {
        self.observation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .milestones
            .process_ready = Some(self.started.elapsed().as_secs_f64());
    }

    fn mark_admitted(&self) {
        self.observation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .milestones
            .torrent_admitted = Some(self.started.elapsed().as_secs_f64());
    }

    fn mark_metainfo(&self, metainfo: &Metainfo) {
        let elapsed = self.started.elapsed().as_secs_f64();
        let mut observation = self
            .observation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        observation.geometry = Geometry {
            total_length: Some(metainfo.total_length),
            piece_length: Some(metainfo.piece_length),
            piece_count: Some(metainfo.piece_count()),
            file_count: Some(metainfo.files.len()),
        };
        observation.milestones.metadata_verified = Some(elapsed);
    }

    fn mark_stopped(&self, joined: bool) {
        let elapsed = self.started.elapsed().as_secs_f64();
        let mut observation = self
            .observation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        observation.milestones.owner_stopped = Some(elapsed);
        observation.milestones.shutdown_joined = joined.then_some(elapsed);
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
        if self.summary_activity_observation
            && matches!(
                event,
                DownloadActivityEvent::BlockReceived { .. }
                    | DownloadActivityEvent::BlockStored { .. }
            )
        {
            return;
        }
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
                if crosses(observation.verified_bytes, total, 10) {
                    observation
                        .milestones
                        .ten_percent_verified
                        .get_or_insert(elapsed);
                }
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
                if crosses(observation.verified_bytes, total, 90) {
                    observation
                        .milestones
                        .ninety_percent_verified
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
                if peer_count > 0 {
                    observation
                        .milestones
                        .first_candidate
                        .get_or_insert(elapsed);
                }
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
                if peer_count > 0 {
                    observation
                        .milestones
                        .first_candidate
                        .get_or_insert(elapsed);
                }
                observation.dht_response_batches =
                    observation.dht_response_batches.saturating_add(1);
                observation.dht_reported_peers = observation
                    .dht_reported_peers
                    .saturating_add(u64::from(peer_count));
            }
            DownloadActivityEvent::PeerConnections { peers, .. } => {
                if peers
                    .iter()
                    .any(|peer| peer.lifecycle == PeerConnectionLifecycle::Connected)
                {
                    observation
                        .milestones
                        .first_connection
                        .get_or_insert(elapsed);
                }
            }
            DownloadActivityEvent::BlockReceived { length, .. } if length > 0 => {
                observation
                    .milestones
                    .first_payload_byte
                    .get_or_insert(elapsed);
                observation.received_payload_bytes = observation
                    .received_payload_bytes
                    .saturating_add(u64::from(length));
                if observation
                    .geometry
                    .total_length
                    .is_some_and(|total| observation.received_payload_bytes >= total)
                {
                    observation
                        .milestones
                        .last_payload_byte
                        .get_or_insert(elapsed);
                }
            }
            DownloadActivityEvent::BlockStored { length, .. } if length > 0 => {
                observation.stored_payload_bytes = observation
                    .stored_payload_bytes
                    .saturating_add(u64::from(length));
                if observation
                    .geometry
                    .total_length
                    .is_some_and(|total| observation.stored_payload_bytes >= total)
                {
                    observation
                        .milestones
                        .last_block_stored
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
    tracker_response_batches: u64,
    tracker_reported_peers: u64,
    dht_response_batches: u64,
    dht_reported_peers: u64,
    peer_dial_attempts: u64,
}

#[derive(Debug, Serialize)]
struct Capabilities {
    network_policy: &'static str,
    tracker: bool,
    dht: bool,
    pex: bool,
    incoming_connections: bool,
    tcp_outgoing: bool,
    utp_outgoing: bool,
    web_seed: bool,
    websocket_trackers: bool,
    address_families: [&'static str; 2],
    encryption: &'static str,
    incomplete_upload: bool,
    upload_slots: usize,
}

#[derive(Debug, Serialize)]
struct EffectiveSettings {
    network_policy: &'static str,
    address_families: [&'static str; 2],
    tracker: bool,
    dht: bool,
    pex: bool,
    lsd: bool,
    upnp: bool,
    natpmp: bool,
    web_seed: bool,
    incoming_connections: bool,
    outgoing_tcp: bool,
    outgoing_utp: bool,
    outgoing_tcp_fallback: bool,
    session_connection_limit: usize,
    torrent_connection_limit: usize,
    pending_dial_limit: usize,
    connection_attempts_per_second: usize,
    peer_connect_timeout_seconds: u64,
    request_timeout_seconds: u64,
    request_queue_time_seconds: u64,
    max_outgoing_request_queue: usize,
    download_rate_limit_bytes_per_second: u64,
    upload_rate_limit_bytes_per_second: u64,
    upload_slots: usize,
    encryption: &'static str,
}

impl EffectiveSettings {
    const fn for_profile(profile: Profile) -> Self {
        Self {
            network_policy: "online",
            address_families: ["ipv4", "ipv6"],
            tracker: profile.discovery().enables_trackers(),
            dht: profile.discovery().enables_dht(),
            pex: profile.peer_exchange(),
            lsd: false,
            upnp: false,
            natpmp: false,
            web_seed: false,
            incoming_connections: false,
            outgoing_tcp: true,
            outgoing_utp: profile.enables_utp(),
            outgoing_tcp_fallback: matches!(profile, Profile::WanUtp),
            session_connection_limit: profile.connection_limit(),
            torrent_connection_limit: 30,
            pending_dial_limit: 30,
            connection_attempts_per_second: 30,
            peer_connect_timeout_seconds: 15,
            request_timeout_seconds: 60,
            request_queue_time_seconds: 3,
            max_outgoing_request_queue: 500,
            download_rate_limit_bytes_per_second: 0,
            upload_rate_limit_bytes_per_second: 0,
            upload_slots: 8,
            encryption: match profile {
                Profile::MatchedPlain30 | Profile::WanTcp | Profile::WanUtp => "disabled",
                Profile::MatchedRc430 => "required-rc4",
                Profile::ProductDefault | Profile::ProductUtp | Profile::DhtOnly => "allow",
            },
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
struct PeerMethodEvidence {
    snapshots: u64,
    connected_high_water: usize,
    tcp_high_water: usize,
    utp_high_water: usize,
    plaintext_stream_high_water: usize,
    plaintext_payload_high_water: usize,
    rc4_high_water: usize,
    payload_contributor_plaintext_stream: bool,
    payload_contributor_plaintext_payload: bool,
    payload_contributor_rc4: bool,
    useful_payload_bytes_high_water: u64,
    uploaded_payload_bytes_high_water: u64,
    utp_endpoint_snapshots: u64,
    utp_unknown_high_water: usize,
    utp_advertised_high_water: usize,
    utp_confirmed_high_water: usize,
    utp_suppressed_high_water: usize,
    utp_suppression_failures_high_water: u8,
    peer_failure_high_water: u32,
    last_peer_failure: Option<String>,
}

#[derive(Debug, Default)]
struct ProbeTorrentPeerSink {
    evidence: Mutex<PeerMethodEvidence>,
    methods: Mutex<BTreeMap<u64, Option<MseMethod>>>,
}

impl ProbeTorrentPeerSink {
    fn snapshot(&self, diagnostics: &DownloadDiagnosticSnapshot) -> PeerMethodEvidence {
        let mut evidence = self
            .evidence
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let methods = self
            .methods
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for peer in &diagnostics.content_peers {
            if peer.useful_payload_bytes == 0 {
                continue;
            }
            match methods.get(&peer.connection_id).copied().flatten() {
                None => evidence.payload_contributor_plaintext_stream = true,
                Some(MseMethod::PlaintextPayload) => {
                    evidence.payload_contributor_plaintext_payload = true;
                }
                Some(MseMethod::Rc4) => evidence.payload_contributor_rc4 = true,
            }
            evidence.useful_payload_bytes_high_water = evidence
                .useful_payload_bytes_high_water
                .max(peer.useful_payload_bytes as u64);
        }
        evidence
    }
}

impl TorrentPeerActivitySink for ProbeTorrentPeerSink {
    fn record_peer_connections(
        &self,
        _captured_at: Duration,
        peers: Vec<PeerConnectionObservation>,
    ) {
        let connected = peers
            .iter()
            .filter(|peer| peer.lifecycle == PeerConnectionLifecycle::Connected)
            .collect::<Vec<_>>();
        let tcp = connected
            .iter()
            .filter(|peer| peer.transport == PeerTransport::Tcp)
            .count();
        let utp = connected
            .iter()
            .filter(|peer| peer.transport == PeerTransport::Utp)
            .count();
        let plaintext_stream = connected
            .iter()
            .filter(|peer| peer.mse_method.is_none())
            .count();
        let plaintext_payload = connected
            .iter()
            .filter(|peer| peer.mse_method == Some(MseMethod::PlaintextPayload))
            .count();
        let rc4 = connected
            .iter()
            .filter(|peer| peer.mse_method == Some(MseMethod::Rc4))
            .count();
        let useful_payload_bytes = connected
            .iter()
            .filter_map(|peer| {
                peer.content
                    .map(|content| content.useful_payload_bytes as u64)
            })
            .sum();
        let uploaded_payload_bytes = connected
            .iter()
            .filter_map(|peer| peer.upload.map(|upload| upload.payload_bytes))
            .sum();
        let mut evidence = self
            .evidence
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        evidence.snapshots = evidence.snapshots.saturating_add(1);
        evidence.connected_high_water = evidence.connected_high_water.max(connected.len());
        evidence.tcp_high_water = evidence.tcp_high_water.max(tcp);
        evidence.utp_high_water = evidence.utp_high_water.max(utp);
        evidence.plaintext_stream_high_water =
            evidence.plaintext_stream_high_water.max(plaintext_stream);
        evidence.plaintext_payload_high_water =
            evidence.plaintext_payload_high_water.max(plaintext_payload);
        evidence.rc4_high_water = evidence.rc4_high_water.max(rc4);
        evidence.useful_payload_bytes_high_water = evidence
            .useful_payload_bytes_high_water
            .max(useful_payload_bytes);
        evidence.uploaded_payload_bytes_high_water = evidence
            .uploaded_payload_bytes_high_water
            .max(uploaded_payload_bytes);
        for peer in connected {
            if peer
                .content
                .is_some_and(|content| content.useful_payload_bytes > 0)
            {
                match peer.mse_method {
                    None => evidence.payload_contributor_plaintext_stream = true,
                    Some(MseMethod::PlaintextPayload) => {
                        evidence.payload_contributor_plaintext_payload = true;
                    }
                    Some(MseMethod::Rc4) => evidence.payload_contributor_rc4 = true,
                }
            }
        }
        drop(evidence);
        let mut methods = self
            .methods
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for peer in peers {
            if peer.lifecycle == PeerConnectionLifecycle::Connected {
                methods.insert(peer.connection_id.get(), peer.mse_method);
            }
        }
    }

    fn record_peer_registry(
        &self,
        _active: bool,
        snapshot: rstorrent_engine::peer::PeerRegistrySnapshot,
    ) {
        use rstorrent_engine::peer::{PeerFailure, UtpEndpointState};

        let mut unknown = 0;
        let mut advertised = 0;
        let mut confirmed = 0;
        let mut suppressed = 0;
        let mut suppression_failures = 0;
        let mut last_failure = None;
        for record in snapshot.records {
            if last_failure
                .as_ref()
                .is_none_or(|(failures, _)| record.history.total_failures >= *failures)
                && let Some(failure) = record.history.last_failure
            {
                last_failure = Some((
                    record.history.total_failures,
                    match failure {
                        PeerFailure::Connect => "connect",
                        PeerFailure::Handshake => "handshake",
                        PeerFailure::SelfConnection => "self_connection",
                        PeerFailure::DuplicatePeerId => "duplicate_peer_id",
                        PeerFailure::Protocol => "protocol",
                        PeerFailure::RemoteClosed => "remote_closed",
                    }
                    .to_owned(),
                ));
            }
            match record.history.utp_endpoint {
                UtpEndpointState::Unknown => unknown += 1,
                UtpEndpointState::Advertised => advertised += 1,
                UtpEndpointState::Confirmed => confirmed += 1,
                UtpEndpointState::Suppressed { failures, .. } => {
                    suppressed += 1;
                    suppression_failures = suppression_failures.max(failures);
                }
            }
        }
        let mut evidence = self
            .evidence
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((failures, failure)) = last_failure
            && failures >= evidence.peer_failure_high_water
        {
            evidence.peer_failure_high_water = failures;
            evidence.last_peer_failure = Some(failure);
        }
        evidence.utp_endpoint_snapshots = evidence.utp_endpoint_snapshots.saturating_add(1);
        evidence.utp_unknown_high_water = evidence.utp_unknown_high_water.max(unknown);
        evidence.utp_advertised_high_water = evidence.utp_advertised_high_water.max(advertised);
        evidence.utp_confirmed_high_water = evidence.utp_confirmed_high_water.max(confirmed);
        evidence.utp_suppressed_high_water = evidence.utp_suppressed_high_water.max(suppressed);
        evidence.utp_suppression_failures_high_water = evidence
            .utp_suppression_failures_high_water
            .max(suppression_failures);
    }
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
    content_last_error: Option<String>,
    content_peers_captured_at_seconds: Option<f64>,
    content_peers: Vec<ContentPeerDiagnostics>,
    connected_peers: Option<usize>,
    unchoked_peers: Option<usize>,
    missing_blocks: Option<usize>,
    requested_blocks: Option<usize>,
    active_request_attempts: Option<usize>,
    active_duplicate_attempts: Option<usize>,
    writing_blocks: Option<usize>,
    active_piece_count: Option<usize>,
    active_piece_bytes: Option<usize>,
    outstanding_request_bytes: usize,
    outstanding_request_high_water: usize,
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
    buffered_payload_bytes: usize,
    payload_high_water: usize,
    resident_payload_limit_bytes: usize,
    storage_intake_high_watermark_bytes: usize,
    storage_intake_low_watermark_bytes: usize,
    storage_jobs_pending: usize,
    storage_jobs_high_water: usize,
    storage_command_queue_high_water: usize,
    storage_completion_queue_high_water: usize,
    storage_hashes_started: usize,
    storage_write_operations_started: usize,
    storage_write_operations_completed: usize,
    storage_write_queue_wait_micros: u64,
    storage_write_queue_wait_max_micros: u64,
    storage_write_service_micros: u64,
    storage_write_service_max_micros: u64,
    storage_write_blocks_started: usize,
    storage_write_blocks_completed: usize,
    storage_write_batch_blocks_high_water: usize,
    storage_write_batch_bytes_high_water: usize,
    storage_hash_operations_started: usize,
    storage_hash_operations_completed: usize,
    storage_hash_queue_wait_micros: u64,
    storage_hash_queue_wait_max_micros: u64,
    storage_hash_service_micros: u64,
    storage_hash_service_max_micros: u64,
    checkpoint_batches_started: usize,
    checkpoint_batches_completed: usize,
    checkpoint_pieces_completed: usize,
    checkpoint_sync_operations_completed: usize,
    checkpoint_sync_service_micros: u64,
    checkpoint_sync_service_max_micros: u64,
    checkpoint_commit_service_micros: u64,
    checkpoint_commit_service_max_micros: u64,
    checkpoint_sync_bypassed: bool,
    summary_activity_observation: bool,
    nonresumable_execution: bool,
    storage_active_kind: Option<&'static str>,
    storage_active_age_micros: Option<u64>,
    peer_methods: PeerMethodEvidence,
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
struct DhtFamilyEvidence {
    family: &'static str,
    local_bound: bool,
    external_address_observed: bool,
    lifecycle: String,
    routing_nodes: u16,
    time_to_first_valid_response_seconds: Option<f64>,
    time_to_routing_threshold_seconds: Option<f64>,
    queries_sent: u64,
    responses_received: u64,
    discovered_peers: u64,
    datagram_bytes_sent: u64,
    datagram_bytes_received: u64,
}

#[derive(Debug, Serialize)]
struct DhtEvidence {
    healthy_routing_threshold: u16,
    ipv6_startup_error: Option<String>,
    families: Vec<DhtFamilyEvidence>,
}

#[derive(Debug, Serialize)]
struct UtpEvidence {
    path_mtu_profile: String,
    active_connections_after_shutdown: usize,
    connections_started: u64,
    connection_high_water: usize,
    incoming_half_open_after_shutdown: usize,
    incoming_half_open_high_water: usize,
    incoming_stream_queue_high_water: usize,
    connection_datagram_queue_high_water: usize,
    malformed_datagrams: u64,
    unknown_connection_datagrams: u64,
    stale_generation_datagrams: u64,
    connection_datagrams_dropped: u64,
    datagrams_sent: u64,
    datagram_bytes_sent: u64,
    data_datagrams_sent: u64,
    state_datagrams_sent: u64,
    retransmission_datagrams_sent: u64,
    retransmission_bytes_sent: u64,
    retransmission_queue_high_water: usize,
    in_flight_packet_high_water: usize,
    in_flight_byte_high_water: usize,
    congestion_control_acknowledgements_high_water: u64,
    congestion_control_acknowledged_bytes_high_water: u64,
    congestion_limited_acknowledgements_high_water: u64,
    sender_underfilled_acknowledgements_high_water: u64,
    remote_window_limited_acknowledgements_high_water: u64,
    window_growth_acknowledgements_high_water: u64,
    slow_start_active_observed: bool,
    slow_start_threshold_byte_high_water: usize,
    slow_start_acknowledgements_high_water: u64,
    slow_start_exits_high_water: u64,
    pending_ack_packet_high_water: usize,
    loss_reduction_high_water: u64,
    timeout_collapse_high_water: u64,
    delivered_byte_high_water: usize,
    receive_reorder_packet_high_water: usize,
    receive_buffered_byte_high_water: usize,
    receive_window_drop_high_water: u64,
    unsent_byte_high_water: usize,
    sent_byte_high_water: usize,
    application_coalesce_byte_high_water: usize,
    smoothed_rtt_min_micros: Option<u64>,
    smoothed_rtt_max_micros: Option<u64>,
    effective_rto_min_micros: Option<u64>,
    effective_rto_max_micros: Option<u64>,
    base_delay_min_micros: Option<u64>,
    base_delay_max_micros: Option<u64>,
    queue_delay_min_micros: Option<u64>,
    queue_delay_max_micros: Option<u64>,
    congestion_window_min_bytes: Option<usize>,
    congestion_window_max_bytes: Option<usize>,
    advertised_receive_window_min_bytes: Option<usize>,
    advertised_receive_window_max_bytes: Option<usize>,
    selected_mtu_min_bytes: Option<usize>,
    selected_mtu_max_bytes: Option<usize>,
    mtu_candidate_min_bytes: Option<usize>,
    mtu_candidate_max_bytes: Option<usize>,
    mtu_probes_started_high_water: u64,
    mtu_probes_acknowledged_high_water: u64,
    mtu_probes_failed_high_water: u64,
    mtu_revalidations_started_high_water: u64,
    mtu_revalidations_acknowledged_high_water: u64,
    mtu_revalidations_failed_high_water: u64,
    mtu_downward_recoveries_high_water: u64,
    mtu_probe_datagrams_sent: u64,
    mtu_fragmentable_retry_datagrams_sent: u64,
    retry_exhausted_connections: u64,
    graceful_connections: u64,
    reset_connections: u64,
    consumer_dropped_connections: u64,
    generation_changed_connections: u64,
    service_cancelled_connections: u64,
    protocol_error_connections: u64,
    io_error_connections: u64,
    worker_panics: u64,
    first_terminal: Option<UtpFailureEvidence>,
    last_failure: Option<UtpFailureEvidence>,
}

#[derive(Debug, Serialize)]
struct UtpFailureEvidence {
    kind: String,
    detail: String,
    new_data_datagrams_sent: u64,
    retransmission_data_datagrams_sent: u64,
    data_datagrams_received: u64,
    sent_sequence_cycles: u64,
    received_sequence_cycles: u64,
    last_data_sequence_sent: Option<u16>,
    last_retransmission_sequence_sent: Option<u16>,
    last_data_sequence_received: Option<u16>,
    loss_signals_received: u64,
    duplicate_acknowledgements: u64,
    stale_acknowledgements: u64,
    future_acknowledgements: u64,
    ambiguous_acknowledgements: u64,
    duplicate_data_datagrams: u64,
    too_far_ahead_data_datagrams: u64,
    ambiguous_data_datagrams: u64,
    fin_datagrams_received: u64,
    reset_datagrams_received: u64,
    outstanding_packets: usize,
    outstanding_bytes: usize,
    in_flight_packets: usize,
    in_flight_bytes: usize,
    pending_retransmissions: usize,
    congestion_window_bytes: usize,
    remote_window_bytes: usize,
    smoothed_rtt_micros: Option<u64>,
    effective_rto_micros: u64,
    consecutive_timeouts: u8,
    loss_reductions: u64,
    timeout_collapses: u64,
}

impl From<UtpTerminalEvidence> for UtpFailureEvidence {
    fn from(failure: UtpTerminalEvidence) -> Self {
        Self {
            kind: failure.kind.as_str().to_owned(),
            detail: failure.detail,
            new_data_datagrams_sent: failure.new_data_datagrams_sent,
            retransmission_data_datagrams_sent: failure.retransmission_data_datagrams_sent,
            data_datagrams_received: failure.data_datagrams_received,
            sent_sequence_cycles: failure.sent_sequence_cycles,
            received_sequence_cycles: failure.received_sequence_cycles,
            last_data_sequence_sent: failure.last_data_sequence_sent,
            last_retransmission_sequence_sent: failure.last_retransmission_sequence_sent,
            last_data_sequence_received: failure.last_data_sequence_received,
            loss_signals_received: failure.loss_signals_received,
            duplicate_acknowledgements: failure.duplicate_acknowledgements,
            stale_acknowledgements: failure.stale_acknowledgements,
            future_acknowledgements: failure.future_acknowledgements,
            ambiguous_acknowledgements: failure.ambiguous_acknowledgements,
            duplicate_data_datagrams: failure.duplicate_data_datagrams,
            too_far_ahead_data_datagrams: failure.too_far_ahead_data_datagrams,
            ambiguous_data_datagrams: failure.ambiguous_data_datagrams,
            fin_datagrams_received: failure.fin_datagrams_received,
            reset_datagrams_received: failure.reset_datagrams_received,
            outstanding_packets: failure.outstanding_packets,
            outstanding_bytes: failure.outstanding_bytes,
            in_flight_packets: failure.in_flight_packets,
            in_flight_bytes: failure.in_flight_bytes,
            pending_retransmissions: failure.pending_retransmissions,
            congestion_window_bytes: failure.congestion_window_bytes,
            remote_window_bytes: failure.remote_window_bytes,
            smoothed_rtt_micros: failure.smoothed_rtt_micros,
            effective_rto_micros: failure.effective_rto_micros,
            consecutive_timeouts: failure.consecutive_timeouts,
            loss_reductions: failure.loss_reductions,
            timeout_collapses: failure.timeout_collapses,
        }
    }
}

impl From<UtpServiceSnapshot> for UtpEvidence {
    fn from(snapshot: UtpServiceSnapshot) -> Self {
        let first_terminal = snapshot.first_terminal.map(Into::into);
        let last_failure = snapshot.last_failure.map(Into::into);
        Self {
            path_mtu_profile: snapshot.path_mtu_profile.as_str().to_owned(),
            active_connections_after_shutdown: snapshot.active_connections,
            connections_started: snapshot.connections_started,
            connection_high_water: snapshot.connection_high_water,
            incoming_half_open_after_shutdown: snapshot.incoming_half_open,
            incoming_half_open_high_water: snapshot.incoming_half_open_high_water,
            incoming_stream_queue_high_water: snapshot.incoming_stream_queue_high_water,
            connection_datagram_queue_high_water: snapshot.connection_datagram_queue_high_water,
            malformed_datagrams: snapshot.malformed_datagrams,
            unknown_connection_datagrams: snapshot.unknown_connection_datagrams,
            stale_generation_datagrams: snapshot.stale_generation_datagrams,
            connection_datagrams_dropped: snapshot.connection_datagrams_dropped,
            datagrams_sent: snapshot.datagrams_sent,
            datagram_bytes_sent: snapshot.datagram_bytes_sent,
            data_datagrams_sent: snapshot.data_datagrams_sent,
            state_datagrams_sent: snapshot.state_datagrams_sent,
            retransmission_datagrams_sent: snapshot.retransmission_datagrams_sent,
            retransmission_bytes_sent: snapshot.retransmission_bytes_sent,
            retransmission_queue_high_water: snapshot.retransmission_queue_high_water,
            in_flight_packet_high_water: snapshot.in_flight_packet_high_water,
            in_flight_byte_high_water: snapshot.in_flight_byte_high_water,
            congestion_control_acknowledgements_high_water: snapshot
                .congestion_control_acknowledgements_high_water,
            congestion_control_acknowledged_bytes_high_water: snapshot
                .congestion_control_acknowledged_bytes_high_water,
            congestion_limited_acknowledgements_high_water: snapshot
                .congestion_limited_acknowledgements_high_water,
            sender_underfilled_acknowledgements_high_water: snapshot
                .sender_underfilled_acknowledgements_high_water,
            remote_window_limited_acknowledgements_high_water: snapshot
                .remote_window_limited_acknowledgements_high_water,
            window_growth_acknowledgements_high_water: snapshot
                .window_growth_acknowledgements_high_water,
            slow_start_active_observed: snapshot.slow_start_active_observed,
            slow_start_threshold_byte_high_water: snapshot.slow_start_threshold_byte_high_water,
            slow_start_acknowledgements_high_water: snapshot.slow_start_acknowledgements_high_water,
            slow_start_exits_high_water: snapshot.slow_start_exits_high_water,
            pending_ack_packet_high_water: snapshot.pending_ack_packet_high_water,
            loss_reduction_high_water: snapshot.loss_reduction_high_water,
            timeout_collapse_high_water: snapshot.timeout_collapse_high_water,
            delivered_byte_high_water: snapshot.delivered_byte_high_water,
            receive_reorder_packet_high_water: snapshot.receive_reorder_packet_high_water,
            receive_buffered_byte_high_water: snapshot.receive_buffered_byte_high_water,
            receive_window_drop_high_water: snapshot.receive_window_drop_high_water,
            unsent_byte_high_water: snapshot.unsent_byte_high_water,
            sent_byte_high_water: snapshot.sent_byte_high_water,
            application_coalesce_byte_high_water: snapshot.application_coalesce_byte_high_water,
            smoothed_rtt_min_micros: snapshot.smoothed_rtt_min_micros,
            smoothed_rtt_max_micros: snapshot.smoothed_rtt_max_micros,
            effective_rto_min_micros: snapshot.effective_rto_min_micros,
            effective_rto_max_micros: snapshot.effective_rto_max_micros,
            base_delay_min_micros: snapshot.base_delay_min_micros,
            base_delay_max_micros: snapshot.base_delay_max_micros,
            queue_delay_min_micros: snapshot.queue_delay_min_micros,
            queue_delay_max_micros: snapshot.queue_delay_max_micros,
            congestion_window_min_bytes: snapshot.congestion_window_min_bytes,
            congestion_window_max_bytes: snapshot.congestion_window_max_bytes,
            advertised_receive_window_min_bytes: snapshot.advertised_receive_window_min_bytes,
            advertised_receive_window_max_bytes: snapshot.advertised_receive_window_max_bytes,
            selected_mtu_min_bytes: snapshot.selected_mtu_min_bytes,
            selected_mtu_max_bytes: snapshot.selected_mtu_max_bytes,
            mtu_candidate_min_bytes: snapshot.mtu_candidate_min_bytes,
            mtu_candidate_max_bytes: snapshot.mtu_candidate_max_bytes,
            mtu_probes_started_high_water: snapshot.mtu_probes_started_high_water,
            mtu_probes_acknowledged_high_water: snapshot.mtu_probes_acknowledged_high_water,
            mtu_probes_failed_high_water: snapshot.mtu_probes_failed_high_water,
            mtu_revalidations_started_high_water: snapshot.mtu_revalidations_started_high_water,
            mtu_revalidations_acknowledged_high_water: snapshot
                .mtu_revalidations_acknowledged_high_water,
            mtu_revalidations_failed_high_water: snapshot.mtu_revalidations_failed_high_water,
            mtu_downward_recoveries_high_water: snapshot.mtu_downward_recoveries_high_water,
            mtu_probe_datagrams_sent: snapshot.mtu_probe_datagrams_sent,
            mtu_fragmentable_retry_datagrams_sent: snapshot.mtu_fragmentable_retry_datagrams_sent,
            retry_exhausted_connections: snapshot.retry_exhausted_connections,
            graceful_connections: snapshot.graceful_connections,
            reset_connections: snapshot.reset_connections,
            consumer_dropped_connections: snapshot.consumer_dropped_connections,
            generation_changed_connections: snapshot.generation_changed_connections,
            service_cancelled_connections: snapshot.service_cancelled_connections,
            protocol_error_connections: snapshot.protocol_error_connections,
            io_error_connections: snapshot.io_error_connections,
            worker_panics: snapshot.worker_panics,
            first_terminal,
            last_failure,
        }
    }
}

#[derive(Debug, Serialize)]
struct UdpEvidence {
    tasks_after_shutdown: usize,
    task_high_water: usize,
    queued_after_shutdown: usize,
    queue_high_water: usize,
    datagrams_received: u64,
    datagram_bytes_received: u64,
    datagrams_dropped: u64,
    dht_datagrams_dropped: u64,
    utp_queued_after_shutdown: usize,
    utp_queue_high_water: usize,
    utp_datagrams_classified: u64,
    utp_datagram_bytes_classified: u64,
    utp_datagrams_dropped: u64,
    protected_sends_attempted: u64,
    protected_sends_sent: u64,
    protected_sends_message_too_large: u64,
    protected_sends_failed: u64,
    fragmentation_restore_failures: u64,
    fragmentation_repairs_succeeded: u64,
    maximum_datagram_bytes_sent: usize,
    ipv4_fragmentation_protection: String,
}

impl From<SessionUdpSnapshot> for UdpEvidence {
    fn from(snapshot: SessionUdpSnapshot) -> Self {
        Self {
            tasks_after_shutdown: snapshot.tasks,
            task_high_water: snapshot.task_high_water,
            queued_after_shutdown: snapshot.queued,
            queue_high_water: snapshot.queue_high_water,
            datagrams_received: snapshot.datagrams_received,
            datagram_bytes_received: snapshot.datagram_bytes_received,
            datagrams_dropped: snapshot.datagrams_dropped,
            dht_datagrams_dropped: snapshot.dht_datagrams_dropped,
            utp_queued_after_shutdown: snapshot.utp_queued,
            utp_queue_high_water: snapshot.utp_queue_high_water,
            utp_datagrams_classified: snapshot.utp_datagrams_classified,
            utp_datagram_bytes_classified: snapshot.utp_datagram_bytes_classified,
            utp_datagrams_dropped: snapshot.utp_datagrams_dropped,
            protected_sends_attempted: snapshot.protected_sends_attempted,
            protected_sends_sent: snapshot.protected_sends_sent,
            protected_sends_message_too_large: snapshot.protected_sends_message_too_large,
            protected_sends_failed: snapshot.protected_sends_failed,
            fragmentation_restore_failures: snapshot.fragmentation_restore_failures,
            fragmentation_repairs_succeeded: snapshot.fragmentation_repairs_succeeded,
            maximum_datagram_bytes_sent: snapshot.maximum_datagram_bytes_sent,
            ipv4_fragmentation_protection: format!("{:?}", snapshot.ipv4_fragmentation_protection),
        }
    }
}

#[derive(Debug, Default)]
struct ProbeCheckpointSink;

impl DownloadCheckpointSink for ProbeCheckpointSink {
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

    fn descriptor_prepared(
        &self,
        _files: &[rstorrent_engine::PreparedFileHash],
    ) -> Result<(), String> {
        Ok(())
    }

    fn publication_prepared(&self) -> Result<(), String> {
        Ok(())
    }

    fn published(&self) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug, Default)]
struct DhtEvidenceTracker {
    ipv6_startup_error: Option<String>,
    first_response: BTreeMap<AddressFamily, Duration>,
    routing_threshold: BTreeMap<AddressFamily, Duration>,
}

impl DhtEvidenceTracker {
    fn sample(&mut self, observation: &DhtObservation, elapsed: Duration) {
        for family in &observation.families {
            if family.stats.responses_received > 0 {
                self.first_response.entry(family.family).or_insert(elapsed);
            }
            if family.routing_nodes >= DHT_HEALTHY_ROUTING_NODES {
                self.routing_threshold
                    .entry(family.family)
                    .or_insert(elapsed);
            }
        }
    }

    fn finish(mut self, observation: &DhtObservation, elapsed: Duration) -> DhtEvidence {
        self.sample(observation, elapsed);
        let families = observation
            .families
            .iter()
            .map(|family| DhtFamilyEvidence {
                family: match family.family {
                    AddressFamily::Ipv4 => "ipv4",
                    AddressFamily::Ipv6 => "ipv6",
                },
                local_bound: family.local_address.port() != 0,
                external_address_observed: family.observed_external_address.is_some(),
                lifecycle: format!("{:?}", family.lifecycle).to_ascii_lowercase(),
                routing_nodes: family.routing_nodes,
                time_to_first_valid_response_seconds: self
                    .first_response
                    .get(&family.family)
                    .map(Duration::as_secs_f64),
                time_to_routing_threshold_seconds: self
                    .routing_threshold
                    .get(&family.family)
                    .map(Duration::as_secs_f64),
                queries_sent: family.stats.queries_sent,
                responses_received: family.stats.responses_received,
                discovered_peers: family.stats.discovered_peers,
                datagram_bytes_sent: family.stats.datagram_bytes_sent,
                datagram_bytes_received: family.stats.datagram_bytes_received,
            })
            .collect();
        DhtEvidence {
            healthy_routing_threshold: DHT_HEALTHY_ROUTING_NODES,
            ipv6_startup_error: self.ipv6_startup_error,
            families,
        }
    }
}

#[derive(Debug, Serialize)]
struct ProbeResult {
    schema_version: u32,
    implementation: &'static str,
    profile: &'static str,
    profile_sha256: String,
    input_mode: &'static str,
    info_hash: String,
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
    effective_settings: EffectiveSettings,
    capabilities: Capabilities,
    dht_evidence: Option<DhtEvidence>,
    utp_evidence: Option<UtpEvidence>,
    udp_evidence: Option<UdpEvidence>,
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

struct LiveUdpRuntime {
    dht: Option<DhtService>,
    utp: Option<UtpService>,
    udp: SessionUdpService,
    evidence: DhtEvidenceTracker,
}

async fn start_live_udp(enable_dht: bool, enable_utp: bool) -> Result<LiveUdpRuntime, String> {
    let ipv4 = UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))
        .await
        .map_err(|error| format!("bind IPv4 session UDP socket: {error}"))?;
    let (mut udp, transport) = SessionUdpService::start(ipv4)
        .map_err(|error| format!("start session UDP owner: {error}"))?;
    let mut evidence = DhtEvidenceTracker::default();
    if enable_dht {
        match select_global_ipv6().await {
            Ok(address) => {
                let ipv6 = match UdpSocket::bind(SocketAddr::from((address, 0))).await {
                    Ok(socket) => socket,
                    Err(error) => {
                        let _ = udp.shutdown().await;
                        return Err(format!("bind selected IPv6 DHT socket: {error}"));
                    }
                };
                if let Err(error) = udp.replace_socket(ipv6).await {
                    let _ = udp.shutdown().await;
                    return Err(format!("start IPv6 session UDP generation: {error}"));
                }
            }
            Err(_) => {
                evidence.ipv6_startup_error =
                    Some("global IPv6 session generation unavailable".to_owned());
            }
        }
    }
    let mut utp = if enable_utp {
        match UtpService::start(&mut udp) {
            Ok(service) => Some(service),
            Err(error) => {
                let _ = udp.shutdown().await;
                return Err(format!("start fixed uTP service: {error}"));
            }
        }
    } else {
        None
    };
    let dht = if enable_dht {
        let config = DhtConfig::for_network(NetworkPolicy::Online);
        match DhtService::start_with_transport(config, transport).await {
            Ok(service) => Some(service),
            Err(error) => {
                if let Some(utp) = utp.take() {
                    let _ = utp.shutdown().await;
                }
                let _ = udp.shutdown().await;
                return Err(error.to_string());
            }
        }
    } else {
        None
    };
    Ok(LiveUdpRuntime {
        dht,
        utp,
        udp,
        evidence,
    })
}

async fn run(config: Config) -> ProbeResult {
    let started = Instant::now();
    let control = DownloadControl::new();
    control.set_checkpoint_sync_bypassed_for_testing(config.checkpoint_sync_bypassed);
    let sink = Arc::new(ProbeSink::new(started, config.summary_activity_observation));
    sink.mark_process_ready();
    control.set_activity_sink(sink.clone());
    let peer_sink = Arc::new(ProbeTorrentPeerSink::default());
    let mut utility_timeline = UtilityTimeline::default();

    let prepared = match prepare_input(&config, &sink) {
        Ok(prepared) => prepared,
        Err(error) => {
            return result(
                &config,
                started,
                ProbeResultSources {
                    sink: sink.as_ref(),
                    peer_sink: peer_sink.as_ref(),
                    diagnostics: &control.diagnostic_snapshot(),
                    utility_timeline: &utility_timeline,
                },
                None,
                None,
                None,
                TerminalState {
                    outcome: "error",
                    integrity_verified: false,
                    cleanup_succeeded: true,
                    detail: Some(error),
                },
            );
        }
    };

    let torrent_id = match TorrentId::generate() {
        Ok(torrent_id) => torrent_id,
        Err(error) => {
            return result(
                &config,
                started,
                ProbeResultSources {
                    sink: sink.as_ref(),
                    peer_sink: peer_sink.as_ref(),
                    diagnostics: &control.diagnostic_snapshot(),
                    utility_timeline: &utility_timeline,
                },
                None,
                None,
                None,
                TerminalState {
                    outcome: "harness_error",
                    integrity_verified: false,
                    cleanup_succeeded: true,
                    detail: Some(format!("create torrent owner: {error}")),
                },
            );
        }
    };
    let identity =
        TorrentIdentityContext::v1(torrent_id, V1InfoHash::new(config.expected_info_hash));

    let torrent_peers = match TorrentPeerHandle::new(peer_sink.clone()) {
        Ok(handle) => handle,
        Err(error) => {
            return result(
                &config,
                started,
                ProbeResultSources {
                    sink: sink.as_ref(),
                    peer_sink: peer_sink.as_ref(),
                    diagnostics: &control.diagnostic_snapshot(),
                    utility_timeline: &utility_timeline,
                },
                None,
                None,
                None,
                TerminalState {
                    outcome: "harness_error",
                    integrity_verified: false,
                    cleanup_succeeded: true,
                    detail: Some(format!("create torrent peer owner: {error}")),
                },
            );
        }
    };

    let mut live_udp = if config.profile.discovery().enables_dht() || config.profile.enables_utp() {
        match start_live_udp(
            config.profile.discovery().enables_dht(),
            config.profile.enables_utp(),
        )
        .await
        {
            Ok(runtime) => Some(runtime),
            Err(error) => {
                return result(
                    &config,
                    started,
                    ProbeResultSources {
                        sink: sink.as_ref(),
                        peer_sink: peer_sink.as_ref(),
                        diagnostics: &control.diagnostic_snapshot(),
                        utility_timeline: &utility_timeline,
                    },
                    None,
                    None,
                    None,
                    TerminalState {
                        outcome: "error",
                        integrity_verified: false,
                        cleanup_succeeded: false,
                        detail: Some(format!("session UDP startup failed: {error}")),
                    },
                );
            }
        }
    } else {
        None
    };
    if let Some(handle) = live_udp
        .as_ref()
        .and_then(|runtime| runtime.utp.as_ref().map(UtpService::handle))
    {
        control.set_utp_handle(handle);
    }

    let mut resource_limits = DownloadResourceLimits::DESKTOP;
    resource_limits.max_buffered_payload_bytes = config.payload_limit;
    resource_limits.storage_intake_high_watermark_bytes = config.storage_intake_high_watermark;
    let mut budget_config = PeerBudgetConfig::system_default();
    budget_config.configured_limit = config.profile.connection_limit();
    budget_config.incoming_slack = 0;
    let network = NetworkConfig::new(
        NetworkPolicy::Online,
        Duration::from_secs(15),
        Duration::from_secs(15),
    )
    .with_encryption(config.profile.encryption())
    .with_mse_rc4_only(matches!(config.profile, Profile::MatchedRc430))
    .with_peer_exchange(config.profile.peer_exchange());
    sink.mark_admitted();
    let task_control = control.clone();
    let mut task = if config.nonresumable_execution {
        let direct_config = DownloadConfig {
            identity,
            metainfo_path: prepared
                .metainfo_path
                .expect("nonresumable diagnostic input is gated to metainfo"),
            peer: config.peer_hints[0],
            output_path: config.output.join(
                prepared
                    .publication_name
                    .expect("metainfo input has a publication name"),
            ),
            network,
            resource_limits,
            skip_files: Vec::new(),
            high_priority_files: Vec::new(),
            materialize_files: Vec::new(),
        };
        tokio::spawn(download_verified_piece_with_peer_state(
            direct_config,
            task_control,
            PeerBudget::new(budget_config),
            torrent_peers,
        ))
    } else {
        let download_config = ResumableMagnetDownloadConfig {
            identity,
            magnet: prepared.magnet,
            storage_root: config.output.clone(),
            network,
            peer_budget: PeerBudget::new(budget_config),
            mse_dh: MseDhWorkOwner::new(),
            encryption: PeerEncryptionPolicyHandle::new(config.profile.encryption()),
            torrent_peers: Some(torrent_peers),
            resource_limits,
            skip_files: Vec::new(),
            high_priority_files: Vec::new(),
            verified_info: prepared.verified_info,
            verified_pieces: Vec::new(),
            artifact_state: ResumeArtifactState::None,
            resume_validation: ResumeValidationIntent::FastEligible,
            download_missing: true,
            dht: live_udp
                .as_ref()
                .and_then(|runtime| runtime.dht.as_ref().map(DhtService::handle)),
            trackers: prepared.trackers,
        };
        let checkpoints = Arc::new(ProbeCheckpointSink);
        tokio::spawn(async move {
            resume_magnet_with_control(download_config, checkpoints, task_control).await
        })
    };
    let deadline = tokio::time::sleep(config.timeout);
    tokio::pin!(deadline);
    let mut reached = false;
    let mut timed_out = false;
    let mut resource_limited = false;
    let mut next_utility_sample = Duration::ZERO;
    let mut joined: Option<Result<Result<DownloadReport, DownloadError>, tokio::task::JoinError>> =
        None;

    loop {
        let elapsed = started.elapsed();
        if let Some(runtime) = live_udp.as_mut()
            && let Some(service) = runtime.dht.as_ref()
        {
            let observations = service.subscribe_observations();
            runtime.evidence.sample(&observations.borrow(), elapsed);
        }
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
        if control.diagnostic_snapshot().progress.received_bytes as u64 > config.wire_payload_limit
        {
            resource_limited = true;
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
    let cleanup_deadline = Instant::now() + config.cleanup_grace;
    let remaining_cleanup = || cleanup_deadline.saturating_duration_since(Instant::now());
    if joined.is_none() {
        match tokio::time::timeout(remaining_cleanup(), &mut task).await {
            Ok(result) => joined = Some(result),
            Err(_) => {
                task.abort();
                let _ = task.await;
                cleanup_succeeded = false;
            }
        }
    }
    let dht_evidence = live_udp.as_mut().and_then(|runtime| {
        runtime.dht.as_ref().map(|service| {
            let observations = service.subscribe_observations();
            std::mem::take(&mut runtime.evidence).finish(&observations.borrow(), started.elapsed())
        })
    });
    let mut utp_evidence = None;
    let mut udp_evidence = None;
    if let Some(mut runtime) = live_udp.take() {
        if let Some(utp) = runtime.utp.take() {
            match tokio::time::timeout(remaining_cleanup(), utp.shutdown()).await {
                Ok(Ok(snapshot)) => utp_evidence = Some(snapshot.into()),
                Ok(Err(_)) | Err(_) => cleanup_succeeded = false,
            }
        }
        if let Some(service) = runtime.dht.take()
            && tokio::time::timeout(remaining_cleanup(), service.shutdown())
                .await
                .map_or(true, |result| result.is_err())
        {
            cleanup_succeeded = false;
        }
        match tokio::time::timeout(remaining_cleanup(), runtime.udp.shutdown()).await {
            Ok(Ok(snapshot)) => udp_evidence = Some(snapshot.into()),
            Ok(Err(_)) | Err(_) => cleanup_succeeded = false,
        }
    }

    sink.mark_stopped(cleanup_succeeded);

    let terminal = classify_terminal(
        config.target,
        reached,
        timed_out,
        resource_limited,
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
        ProbeResultSources {
            sink: sink.as_ref(),
            peer_sink: peer_sink.as_ref(),
            diagnostics: &control.diagnostic_snapshot(),
            utility_timeline: &utility_timeline,
        },
        dht_evidence,
        utp_evidence,
        udp_evidence,
        terminal,
    )
}

#[derive(Debug)]
struct PreparedInput {
    magnet: String,
    verified_info: Option<Vec<u8>>,
    trackers: Option<Vec<TrackerConfig>>,
    metainfo_path: Option<PathBuf>,
    publication_name: Option<String>,
}

fn prepare_input(config: &Config, sink: &ProbeSink) -> Result<PreparedInput, String> {
    match &config.input {
        ProbeInput::Magnet(magnet) => {
            let parsed = Magnet::parse(magnet).map_err(|error| format!("parse magnet: {error}"))?;
            if parsed.identity
                != rstorrent_protocol::identity::FullInfoHash::V1(
                    rstorrent_protocol::identity::V1InfoHash::new(config.expected_info_hash),
                )
            {
                return Err("magnet identity does not match --expected-info-hash".to_owned());
            }
            Ok(PreparedInput {
                magnet: magnet.clone(),
                verified_info: None,
                trackers: (!matches!(
                    config.profile.discovery(),
                    Discovery::Tracker | Discovery::Full
                ))
                .then(Vec::new),
                metainfo_path: None,
                publication_name: None,
            })
        }
        ProbeInput::Metainfo(path) => {
            let metadata = std::fs::metadata(path)
                .map_err(|error| format!("inspect metainfo {}: {error}", path.display()))?;
            if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_METAINFO_BYTES {
                return Err(format!(
                    "metainfo size must be between 1 and {MAX_METAINFO_BYTES} bytes"
                ));
            }
            let bytes = std::fs::read(path)
                .map_err(|error| format!("read metainfo {}: {error}", path.display()))?;
            let projection =
                Metainfo::project_bytes_with_limits(&bytes, EXPLICIT_IMPORT_METAINFO_LIMITS)
                    .map_err(|error| format!("parse metainfo: {error}"))?;
            if projection.metainfo.info_hash != config.expected_info_hash {
                return Err("metainfo identity does not match --expected-info-hash".to_owned());
            }
            let raw_info = bytes[projection.info_span.clone()].to_vec();
            sink.mark_metainfo(&projection.metainfo);
            let publication_name = projection.metainfo.name.clone();
            let magnet = format!(
                "magnet:?xt=urn:btih:{}",
                hex_info_hash(&projection.metainfo.info_hash)
            );
            let magnet = config.peer_hints.iter().fold(magnet, |mut magnet, peer| {
                magnet.push_str("&x.pe=");
                magnet.push_str(&peer.to_string());
                magnet
            });
            let trackers = if !config.profile.discovery().enables_trackers() {
                Vec::new()
            } else {
                projection
                    .trackers
                    .into_iter()
                    .map(|tracker| {
                        let endpoint = match tracker.transport {
                            MetainfoTrackerTransport::Udp => {
                                UdpTrackerUrl::from_metainfo_url(&tracker.url)
                                    .map(TrackerEndpoint::Udp)
                            }
                            MetainfoTrackerTransport::Http | MetainfoTrackerTransport::Https => {
                                TrackerEndpoint::from_http_url(&tracker.url)
                            }
                        }
                        .ok_or_else(|| "unsupported tracker URL in metainfo".to_owned())?;
                        Ok(TrackerConfig {
                            url: tracker.url,
                            endpoint,
                            tier: tracker.tier,
                            position: tracker.position,
                            source: TrackerSource::Metainfo,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?
            };
            Ok(PreparedInput {
                magnet,
                verified_info: Some(raw_info),
                trackers: Some(trackers),
                metainfo_path: Some(path.clone()),
                publication_name: Some(publication_name),
            })
        }
    }
}

fn hex_info_hash(info_hash: &[u8; 20]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(40);
    for byte in info_hash {
        output.push(HEX[usize::from(byte >> 4)] as char);
        output.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    output
}

fn classify_terminal(
    target: Target,
    reached: bool,
    timed_out: bool,
    resource_limited: bool,
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
    if resource_limited {
        return TerminalState {
            outcome: "resource_bound",
            integrity_verified: false,
            cleanup_succeeded: true,
            detail: Some("wire payload ceiling exceeded".to_owned()),
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

struct ProbeResultSources<'a> {
    sink: &'a ProbeSink,
    peer_sink: &'a ProbeTorrentPeerSink,
    diagnostics: &'a DownloadDiagnosticSnapshot,
    utility_timeline: &'a UtilityTimeline,
}

fn result(
    config: &Config,
    started: Instant,
    sources: ProbeResultSources<'_>,
    dht_evidence: Option<DhtEvidence>,
    utp_evidence: Option<UtpEvidence>,
    udp_evidence: Option<UdpEvidence>,
    terminal: TerminalState,
) -> ProbeResult {
    let ProbeResultSources {
        sink,
        peer_sink,
        diagnostics,
        utility_timeline,
    } = sources;
    let observation = sink.snapshot();
    let diagnostics = diagnostic_result(
        diagnostics,
        &observation,
        utility_timeline,
        peer_sink.snapshot(diagnostics),
        config,
    );
    ProbeResult {
        schema_version: 2,
        implementation: "rstorrent",
        profile: config.profile.name(),
        profile_sha256: config.profile_sha256.clone(),
        input_mode: config.input.mode(),
        info_hash: hex_info_hash(&config.expected_info_hash),
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
        effective_settings: EffectiveSettings::for_profile(config.profile),
        capabilities: Capabilities {
            network_policy: "online",
            tracker: config.profile.discovery().enables_trackers(),
            dht: config.profile.discovery().enables_dht(),
            pex: config.profile.peer_exchange(),
            incoming_connections: false,
            tcp_outgoing: true,
            utp_outgoing: config.profile.enables_utp(),
            web_seed: false,
            websocket_trackers: false,
            address_families: ["ipv4", "ipv6"],
            encryption: EffectiveSettings::for_profile(config.profile).encryption,
            incomplete_upload: true,
            upload_slots: 8,
        },
        dht_evidence,
        utp_evidence,
        udp_evidence,
        diagnostics,
    }
}

fn diagnostic_result(
    snapshot: &DownloadDiagnosticSnapshot,
    observation: &ObservationSnapshot,
    utility_timeline: &UtilityTimeline,
    peer_methods: PeerMethodEvidence,
    config: &Config,
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
        content_last_error: snapshot.content_last_error.clone(),
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
        active_piece_count: swarm.map(|value| value.active_piece_count),
        active_piece_bytes: swarm.map(|value| value.active_piece_bytes),
        outstanding_request_bytes: snapshot.progress.outstanding_request_bytes,
        outstanding_request_high_water: snapshot.progress.outstanding_request_high_water,
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
        buffered_payload_bytes: snapshot.progress.buffered_payload_bytes,
        payload_high_water: snapshot.progress.payload_high_water,
        resident_payload_limit_bytes: config.payload_limit,
        storage_intake_high_watermark_bytes: config.storage_intake_high_watermark,
        storage_intake_low_watermark_bytes: config.storage_intake_high_watermark.saturating_mul(2)
            / 3,
        storage_jobs_pending: snapshot.progress.storage_jobs_pending,
        storage_jobs_high_water: snapshot.progress.storage_jobs_high_water,
        storage_command_queue_high_water: snapshot.progress.storage_command_queue_high_water,
        storage_completion_queue_high_water: snapshot.progress.storage_completion_queue_high_water,
        storage_hashes_started: snapshot.progress.storage_hashes_started,
        storage_write_operations_started: snapshot.progress.storage_write_operations_started,
        storage_write_operations_completed: snapshot.progress.storage_write_operations_completed,
        storage_write_queue_wait_micros: snapshot.progress.storage_write_queue_wait_micros,
        storage_write_queue_wait_max_micros: snapshot.progress.storage_write_queue_wait_max_micros,
        storage_write_service_micros: snapshot.progress.storage_write_service_micros,
        storage_write_service_max_micros: snapshot.progress.storage_write_service_max_micros,
        storage_write_blocks_started: snapshot.progress.storage_write_blocks_started,
        storage_write_blocks_completed: snapshot.progress.storage_write_blocks_completed,
        storage_write_batch_blocks_high_water: snapshot
            .progress
            .storage_write_batch_blocks_high_water,
        storage_write_batch_bytes_high_water: snapshot
            .progress
            .storage_write_batch_bytes_high_water,
        storage_hash_operations_started: snapshot.progress.storage_hash_operations_started,
        storage_hash_operations_completed: snapshot.progress.storage_hash_operations_completed,
        storage_hash_queue_wait_micros: snapshot.progress.storage_hash_queue_wait_micros,
        storage_hash_queue_wait_max_micros: snapshot.progress.storage_hash_queue_wait_max_micros,
        storage_hash_service_micros: snapshot.progress.storage_hash_service_micros,
        storage_hash_service_max_micros: snapshot.progress.storage_hash_service_max_micros,
        checkpoint_batches_started: snapshot.progress.checkpoint_batches_started,
        checkpoint_batches_completed: snapshot.progress.checkpoint_batches_completed,
        checkpoint_pieces_completed: snapshot.progress.checkpoint_pieces_completed,
        checkpoint_sync_operations_completed: snapshot
            .progress
            .checkpoint_sync_operations_completed,
        checkpoint_sync_service_micros: snapshot.progress.checkpoint_sync_service_micros,
        checkpoint_sync_service_max_micros: snapshot.progress.checkpoint_sync_service_max_micros,
        checkpoint_commit_service_micros: snapshot.progress.checkpoint_commit_service_micros,
        checkpoint_commit_service_max_micros: snapshot
            .progress
            .checkpoint_commit_service_max_micros,
        checkpoint_sync_bypassed: config.checkpoint_sync_bypassed,
        summary_activity_observation: config.summary_activity_observation,
        nonresumable_execution: config.nonresumable_execution,
        storage_active_kind: if snapshot.progress.storage_active_write_micros.is_some() {
            Some("write")
        } else if snapshot.progress.storage_active_hash_micros.is_some() {
            Some("hash")
        } else {
            None
        },
        storage_active_age_micros: snapshot
            .progress
            .storage_active_write_micros
            .or(snapshot.progress.storage_active_hash_micros),
        peer_methods,
    }
}

fn parse_args(arguments: Vec<String>) -> Result<Config, String> {
    let mut magnet = None;
    let mut metainfo = None;
    let mut output = None;
    let mut expected_info_hash = None;
    let mut peer_hints = Vec::new();
    let mut target = Target::Complete;
    let mut profile = Profile::MatchedPlain30;
    let mut profile_sha256 = None;
    let mut timeout_seconds = DEFAULT_TIMEOUT_SECONDS;
    let mut cleanup_seconds = DEFAULT_CLEANUP_SECONDS;
    let mut payload_limit = DEFAULT_PAYLOAD_LIMIT;
    let mut storage_intake_high_watermark = None;
    let mut wire_payload_limit = DEFAULT_WIRE_PAYLOAD_LIMIT;
    let mut checkpoint_sync_bypassed = false;
    let mut summary_activity_observation = false;
    let mut nonresumable_execution = false;
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
            "--metainfo" => set_once(&mut metainfo, PathBuf::from(value), flag)?,
            "--output" => set_once(&mut output, PathBuf::from(value), flag)?,
            "--expected-info-hash" => {
                set_once(&mut expected_info_hash, parse_info_hash(value)?, flag)?;
            }
            "--peer-hint" => {
                if peer_hints.len() >= 8 {
                    return Err("--peer-hint may be specified at most eight times".to_owned());
                }
                peer_hints.push(
                    value
                        .parse::<SocketAddr>()
                        .map_err(|_| "--peer-hint must be an IP socket address".to_owned())?,
                );
            }
            "--target" => target = Target::parse(value)?,
            "--profile" => profile = Profile::parse(value)?,
            "--profile-sha256" => set_once(&mut profile_sha256, value.clone(), flag)?,
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
            "--storage-intake-high-watermark-bytes" => {
                let parsed = value
                    .parse()
                    .map_err(|_| format!("{flag} must be an integer"))?;
                if !(16 * 1024..=1024 * 1024 * 1024).contains(&parsed) {
                    return Err(format!("{flag} is outside the supported range"));
                }
                set_once(&mut storage_intake_high_watermark, parsed, flag)?;
            }
            "--wire-payload-ceiling-bytes" => {
                wire_payload_limit = bounded_u64(value, flag, 1, u64::MAX)?;
            }
            "--diagnostic-checkpoint-sync" => {
                checkpoint_sync_bypassed = match value.as_str() {
                    "enabled" => false,
                    "bypass" => true,
                    _ => return Err(format!("{flag} must be enabled or bypass")),
                };
            }
            "--diagnostic-activity-observation" => {
                summary_activity_observation = match value.as_str() {
                    "detailed" => false,
                    "summary" => true,
                    _ => return Err(format!("{flag} must be detailed or summary")),
                };
            }
            "--diagnostic-execution" => {
                nonresumable_execution = match value.as_str() {
                    "resumable" => false,
                    "nonresumable" => true,
                    _ => return Err(format!("{flag} must be resumable or nonresumable")),
                };
            }
            _ => return Err(format!("unknown argument {flag}")),
        }
    }
    let input = match (magnet, metainfo) {
        (Some(magnet), None) => ProbeInput::Magnet(magnet),
        (None, Some(path)) => ProbeInput::Metainfo(path),
        (None, None) => return Err("exactly one of --magnet or --metainfo is required".to_owned()),
        (Some(_), Some(_)) => {
            return Err("--magnet and --metainfo are mutually exclusive".to_owned());
        }
    };
    let profile_sha256 = profile_sha256.ok_or_else(|| "--profile-sha256 is required".to_owned())?;
    if profile_sha256.len() != 64 || !profile_sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("--profile-sha256 must be a 64-character hexadecimal digest".to_owned());
    }
    if checkpoint_sync_bypassed
        && (!matches!(&input, ProbeInput::Metainfo(_))
            || target != Target::Complete
            || !matches!(profile, Profile::MatchedPlain30 | Profile::MatchedRc430))
    {
        return Err(
            "checkpoint sync bypass requires direct metainfo, complete target, and a matched profile"
                .to_owned(),
        );
    }
    if summary_activity_observation
        && (!matches!(&input, ProbeInput::Metainfo(_))
            || target != Target::Complete
            || !matches!(profile, Profile::MatchedPlain30 | Profile::MatchedRc430))
    {
        return Err(
            "summary activity observation requires direct metainfo, complete target, and a matched profile"
                .to_owned(),
        );
    }
    if nonresumable_execution
        && (!matches!(&input, ProbeInput::Metainfo(_))
            || peer_hints.len() != 1
            || target != Target::Complete
            || !matches!(profile, Profile::MatchedPlain30 | Profile::MatchedRc430))
    {
        return Err(
            "nonresumable execution requires direct metainfo, one peer hint, complete target, and a matched profile"
                .to_owned(),
        );
    }
    let storage_intake_high_watermark = storage_intake_high_watermark.unwrap_or_else(|| {
        DownloadResourceLimits::default_storage_intake_high_watermark(payload_limit)
    });
    if storage_intake_high_watermark > payload_limit {
        return Err(
            "--storage-intake-high-watermark-bytes must not exceed the buffered payload allowance"
                .to_owned(),
        );
    }
    Ok(Config {
        input,
        expected_info_hash: expected_info_hash
            .ok_or_else(|| "--expected-info-hash is required".to_owned())?,
        peer_hints,
        output: output.ok_or_else(|| "--output is required".to_owned())?,
        target,
        profile,
        profile_sha256: profile_sha256.to_ascii_lowercase(),
        timeout: Duration::from_secs(timeout_seconds),
        cleanup_grace: Duration::from_secs(cleanup_seconds),
        payload_limit,
        storage_intake_high_watermark,
        wire_payload_limit,
        checkpoint_sync_bypassed,
        summary_activity_observation,
        nonresumable_execution,
    })
}

fn parse_info_hash(value: &str) -> Result<[u8; 20], String> {
    if value.len() != 40 {
        return Err("--expected-info-hash must contain 40 hexadecimal characters".to_owned());
    }
    let mut output = [0_u8; 20];
    for (index, byte) in output.iter_mut().enumerate() {
        let start = index * 2;
        *byte = u8::from_str_radix(&value[start..start + 2], 16)
            .map_err(|_| "--expected-info-hash must contain lowercase hexadecimal".to_owned())?;
    }
    if value.bytes().any(|byte| byte.is_ascii_uppercase()) {
        return Err("--expected-info-hash must contain lowercase hexadecimal".to_owned());
    }
    Ok(output)
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
        DownloadDiagnosticSnapshot, DownloadProgress, DownloadResourceLimits,
        MetadataAcquisitionSnapshot,
    };

    use super::{
        Discovery, DownloadActivityEvent, DownloadActivitySink, EffectiveSettings, Geometry,
        IntegerDistribution, MAX_UTILITY_SAMPLES, Milestones, ObservationSnapshot,
        PeerEncryptionPolicy, ProbeInput, ProbeSink, Profile, UtilitySample, UtilityTimeline,
        crosses, integer_distribution, parse_args,
    };

    fn required_arguments() -> Vec<String> {
        vec![
            "--magnet".to_owned(),
            format!("magnet:?xt=urn:btih:{}", "11".repeat(20)),
            "--expected-info-hash".to_owned(),
            "11".repeat(20),
            "--output".to_owned(),
            "out".to_owned(),
            "--profile-sha256".to_owned(),
            "22".repeat(32),
        ]
    }

    #[test]
    fn arguments_require_exactly_one_bounded_input() {
        let config = parse_args(required_arguments()).expect("valid magnet arguments");
        assert!(matches!(config.input, ProbeInput::Magnet(_)));
        assert_eq!(config.profile, Profile::MatchedPlain30);
        assert_eq!(
            config.storage_intake_high_watermark,
            DownloadResourceLimits::default_storage_intake_high_watermark(config.payload_limit)
        );

        let mut both = required_arguments();
        both.extend(["--metainfo".to_owned(), "fixture.torrent".to_owned()]);
        assert!(parse_args(both).is_err());

        let missing = required_arguments().into_iter().skip(2).collect::<Vec<_>>();
        assert!(parse_args(missing).is_err());
    }

    #[test]
    fn storage_intake_watermark_is_independent_and_bounded_by_resident_payload() {
        let mut arguments = required_arguments();
        arguments.extend([
            "--max-buffered-payload-bytes".to_owned(),
            (64 * 1024 * 1024).to_string(),
            "--storage-intake-high-watermark-bytes".to_owned(),
            (2 * 1024 * 1024).to_string(),
        ]);
        let config = parse_args(arguments).expect("independent intake watermark");
        assert_eq!(config.payload_limit, 64 * 1024 * 1024);
        assert_eq!(config.storage_intake_high_watermark, 2 * 1024 * 1024);

        let mut invalid = required_arguments();
        invalid.extend([
            "--max-buffered-payload-bytes".to_owned(),
            (1024 * 1024).to_string(),
            "--storage-intake-high-watermark-bytes".to_owned(),
            (2 * 1024 * 1024).to_string(),
        ]);
        assert!(parse_args(invalid).is_err());
    }

    #[test]
    fn arguments_select_forced_rc4_and_bounded_peer_hints() {
        let mut arguments = required_arguments();
        arguments.extend(["--profile".to_owned(), "matched-rc4-30".to_owned()]);
        arguments.extend(["--peer-hint".to_owned(), "127.0.0.1:6881".to_owned()]);
        let config = parse_args(arguments).expect("forced RC4 arguments");
        assert_eq!(config.profile, Profile::MatchedRc430);
        assert_eq!(config.peer_hints.len(), 1);
    }

    #[test]
    fn product_utp_profile_is_explicit_and_fixed_to_thirty_peers() {
        let mut arguments = required_arguments();
        arguments.extend(["--profile".to_owned(), "product-utp".to_owned()]);
        let config = parse_args(arguments).expect("product uTP arguments");
        assert_eq!(config.profile, Profile::ProductUtp);
        assert!(config.profile.enables_utp());
        assert_eq!(config.profile.connection_limit(), 30);
        let settings = EffectiveSettings::for_profile(config.profile);
        assert!(settings.outgoing_utp);
        assert!(settings.outgoing_tcp);
        assert!(!settings.incoming_connections);
        assert!(!settings.upnp);
        assert!(!EffectiveSettings::for_profile(Profile::ProductDefault).outgoing_utp);
    }

    #[test]
    fn wan_profiles_are_direct_single_peer_and_transport_explicit() {
        for (name, expected) in [("wan-tcp", Profile::WanTcp), ("wan-utp", Profile::WanUtp)] {
            let mut arguments = required_arguments();
            arguments.extend(["--profile".to_owned(), name.to_owned()]);
            let config = parse_args(arguments).expect("direct WAN profile arguments");
            assert_eq!(config.profile, expected);
            assert_eq!(config.profile.discovery(), Discovery::Direct);
            assert_eq!(config.profile.connection_limit(), 1);
            assert!(!config.profile.peer_exchange());
            assert_eq!(config.profile.encryption(), PeerEncryptionPolicy::Disabled);
        }
        let tcp = EffectiveSettings::for_profile(Profile::WanTcp);
        assert!(tcp.outgoing_tcp);
        assert!(!tcp.outgoing_utp);
        assert!(!tcp.outgoing_tcp_fallback);
        assert!(!tcp.tracker);
        assert!(!tcp.dht);
        let utp = EffectiveSettings::for_profile(Profile::WanUtp);
        assert!(utp.outgoing_tcp);
        assert!(utp.outgoing_utp);
        assert!(utp.outgoing_tcp_fallback);
        assert!(!utp.tracker);
        assert!(!utp.dht);
    }

    #[test]
    fn checkpoint_sync_bypass_is_narrowly_diagnostic() {
        let mut metainfo = required_arguments();
        metainfo[0] = "--metainfo".to_owned();
        metainfo[1] = "fixture.torrent".to_owned();
        metainfo.extend([
            "--diagnostic-checkpoint-sync".to_owned(),
            "bypass".to_owned(),
        ]);
        let config = parse_args(metainfo).expect("matched complete metainfo bypass");
        assert!(config.checkpoint_sync_bypassed);

        let mut magnet = required_arguments();
        magnet.extend([
            "--diagnostic-checkpoint-sync".to_owned(),
            "bypass".to_owned(),
        ]);
        assert!(parse_args(magnet).is_err());
    }

    #[test]
    fn summary_activity_observation_is_narrowly_diagnostic() {
        let mut metainfo = required_arguments();
        metainfo[0] = "--metainfo".to_owned();
        metainfo[1] = "fixture.torrent".to_owned();
        metainfo.extend([
            "--diagnostic-activity-observation".to_owned(),
            "summary".to_owned(),
        ]);
        let config = parse_args(metainfo).expect("matched complete metainfo summary");
        assert!(config.summary_activity_observation);

        let mut magnet = required_arguments();
        magnet.extend([
            "--diagnostic-activity-observation".to_owned(),
            "summary".to_owned(),
        ]);
        assert!(parse_args(magnet).is_err());
    }

    #[test]
    fn nonresumable_execution_is_narrowly_diagnostic() {
        let mut metainfo = required_arguments();
        metainfo[0] = "--metainfo".to_owned();
        metainfo[1] = "fixture.torrent".to_owned();
        metainfo.extend([
            "--peer-hint".to_owned(),
            "127.0.0.1:6881".to_owned(),
            "--diagnostic-execution".to_owned(),
            "nonresumable".to_owned(),
        ]);
        let config = parse_args(metainfo).expect("matched complete direct execution");
        assert!(config.nonresumable_execution);

        let mut magnet = required_arguments();
        magnet.extend([
            "--peer-hint".to_owned(),
            "127.0.0.1:6881".to_owned(),
            "--diagnostic-execution".to_owned(),
            "nonresumable".to_owned(),
        ]);
        assert!(parse_args(magnet).is_err());
    }

    #[test]
    fn percentage_thresholds_do_not_round_down() {
        assert!(!crosses(49, 100, 50));
        assert!(crosses(1, 1, 50));
        assert!(crosses(95, 100, 95));
        assert!(!crosses(94, 100, 95));
    }

    #[test]
    fn discovery_diagnostics_accumulate_without_endpoint_retention() {
        let sink = ProbeSink::new(Instant::now(), false);
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
    fn payload_milestones_separate_receive_store_and_verify() {
        let sink = ProbeSink::new(Instant::now(), false);
        sink.record(DownloadActivityEvent::MetadataVerified {
            total_length: 32,
            piece_length: 32,
            piece_count: 1,
            file_count: 1,
        });
        sink.record(DownloadActivityEvent::BlockReceived {
            piece_index: 0,
            begin: 0,
            length: 16,
        });
        sink.record(DownloadActivityEvent::BlockReceived {
            piece_index: 0,
            begin: 16,
            length: 16,
        });
        sink.record(DownloadActivityEvent::BlockStored {
            piece_index: 0,
            begin: 0,
            length: 16,
        });
        let partial = sink.snapshot();
        assert!(partial.milestones.first_payload_byte.is_some());
        assert!(partial.milestones.last_payload_byte.is_some());
        assert!(partial.milestones.last_block_stored.is_none());
        assert!(partial.milestones.first_piece_verified.is_none());

        sink.record(DownloadActivityEvent::BlockStored {
            piece_index: 0,
            begin: 16,
            length: 16,
        });
        sink.record(DownloadActivityEvent::PieceVerified { piece_index: 0 });
        let complete = sink.snapshot();
        assert!(complete.milestones.last_block_stored.is_some());
        assert!(complete.milestones.first_piece_verified.is_some());
        assert!(complete.milestones.all_pieces_verified.is_some());
    }

    #[test]
    fn summary_activity_observation_omits_per_block_milestones() {
        let sink = ProbeSink::new(Instant::now(), true);
        sink.record(DownloadActivityEvent::MetadataVerified {
            total_length: 16,
            piece_length: 16,
            piece_count: 1,
            file_count: 1,
        });
        sink.record(DownloadActivityEvent::BlockReceived {
            piece_index: 0,
            begin: 0,
            length: 16,
        });
        sink.record(DownloadActivityEvent::BlockStored {
            piece_index: 0,
            begin: 0,
            length: 16,
        });
        sink.record(DownloadActivityEvent::PieceVerified { piece_index: 0 });

        let complete = sink.snapshot();
        assert!(complete.milestones.first_payload_byte.is_none());
        assert!(complete.milestones.last_block_stored.is_none());
        assert!(complete.milestones.all_pieces_verified.is_some());
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
            progress: DownloadProgress {
                storage_write_queue_wait_micros: 7,
                storage_hash_queue_wait_micros: 11,
                storage_write_service_micros: 13,
                storage_hash_service_micros: 17,
                storage_write_blocks_completed: 23,
                storage_write_batch_blocks_high_water: 5,
                storage_write_batch_bytes_high_water: 65_536,
                storage_active_hash_micros: Some(19),
                ..DownloadProgress::default()
            },
            swarm: None,
            content_peers_captured_at: None,
            content_peers: Vec::new(),
            content_registry: None,
            content_last_error: None,
            peer_connections: Vec::new(),
            metadata: MetadataAcquisitionSnapshot::default(),
        };
        timeline.record(Duration::ZERO, &observation_snapshot(0, 0), &diagnostics);
        assert_eq!(timeline.samples[0].storage_queue_wait_micros, Some(18));
        assert_eq!(timeline.samples[0].storage_write_service_micros, Some(13));
        assert_eq!(timeline.samples[0].storage_hash_service_micros, Some(17));
        assert_eq!(timeline.samples[0].storage_write_blocks_completed, Some(23));
        assert_eq!(
            timeline.samples[0].storage_write_batch_blocks_high_water,
            Some(5)
        );
        assert_eq!(
            timeline.samples[0].storage_write_batch_bytes_high_water,
            Some(65_536)
        );
        assert_eq!(timeline.samples[0].storage_active_kind, Some("hash"));
        assert_eq!(timeline.samples[0].storage_active_age_micros, Some(19));
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
            backed_off_peers: None,
            failure_limited_peers: None,
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
            storage_queue_wait_micros: None,
            storage_write_service_micros: None,
            storage_hash_service_micros: None,
            storage_write_blocks_completed: None,
            storage_write_batch_blocks_high_water: None,
            storage_write_batch_bytes_high_water: None,
            storage_active_kind: None,
            storage_active_age_micros: None,
            pending_disk_bytes: None,
            payload_rate: None,
            peer_payload_rates: IntegerDistribution::default(),
            peer_request_queues: IntegerDistribution::default(),
        }
    }
}
