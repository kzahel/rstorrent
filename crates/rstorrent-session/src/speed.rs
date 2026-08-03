use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::path::Path;
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rstorrent_engine::{ByteMetric, ByteMetricSink};
use rusqlite::{Connection, params};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;
use tokio::time::{MissedTickBehavior, interval};
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use crate::ViewHub;

const METRICS_DATABASE_FILENAME: &str = "metrics.db";
const METRICS_SCHEMA_VERSION: i64 = 1;
const METRICS_BUSY_TIMEOUT: Duration = Duration::from_secs(2);
pub const MAX_SPEED_SERIES: usize = 8;

#[derive(
    Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize, JsonSchema, TS,
)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum SpeedMetric {
    PayloadReceived,
    StagedWrite,
    PayloadVerified,
    PeerWireReceived,
    PeerWireSent,
    PeerProtocolReceived,
    PeerProtocolSent,
    MetadataPayloadReceived,
    MetadataPayloadSent,
    PeerUnclassifiedReceived,
    PeerUnclassifiedSent,
    DhtReceived,
    DhtSent,
    TrackerReceived,
    TrackerSent,
    LogicalHashRead,
    PayloadRedundant,
    PayloadHashFailed,
    PayloadUploaded,
}

impl SpeedMetric {
    pub const AVAILABLE: [Self; 18] = [
        Self::PayloadReceived,
        Self::StagedWrite,
        Self::PayloadVerified,
        Self::PeerWireReceived,
        Self::PeerWireSent,
        Self::PeerProtocolReceived,
        Self::PeerProtocolSent,
        Self::MetadataPayloadReceived,
        Self::MetadataPayloadSent,
        Self::PeerUnclassifiedReceived,
        Self::PeerUnclassifiedSent,
        Self::DhtReceived,
        Self::DhtSent,
        Self::TrackerReceived,
        Self::TrackerSent,
        Self::LogicalHashRead,
        Self::PayloadRedundant,
        Self::PayloadHashFailed,
    ];

    pub const DEFAULT: [Self; 3] = [
        Self::PayloadReceived,
        Self::StagedWrite,
        Self::PayloadVerified,
    ];

    const fn storage_name(self) -> &'static str {
        match self {
            Self::PayloadReceived => "payload_received",
            Self::StagedWrite => "staged_write",
            Self::PayloadVerified => "payload_verified",
            Self::PeerWireReceived => "peer_wire_received",
            Self::PeerWireSent => "peer_wire_sent",
            Self::PeerProtocolReceived => "peer_protocol_received",
            Self::PeerProtocolSent => "peer_protocol_sent",
            Self::MetadataPayloadReceived => "metadata_payload_received",
            Self::MetadataPayloadSent => "metadata_payload_sent",
            Self::PeerUnclassifiedReceived => "peer_unclassified_received",
            Self::PeerUnclassifiedSent => "peer_unclassified_sent",
            Self::DhtReceived => "dht_received",
            Self::DhtSent => "dht_sent",
            Self::TrackerReceived => "tracker_received",
            Self::TrackerSent => "tracker_sent",
            Self::LogicalHashRead => "logical_hash_read",
            Self::PayloadRedundant => "payload_redundant",
            Self::PayloadHashFailed => "payload_hash_failed",
            Self::PayloadUploaded => "payload_uploaded",
        }
    }

    fn from_storage_name(value: &str) -> Option<Self> {
        Self::AVAILABLE
            .into_iter()
            .find(|metric| metric.storage_name() == value)
    }
}

impl From<ByteMetric> for SpeedMetric {
    fn from(metric: ByteMetric) -> Self {
        match metric {
            ByteMetric::PayloadReceived => Self::PayloadReceived,
            ByteMetric::StagedWrite => Self::StagedWrite,
            ByteMetric::PayloadVerified => Self::PayloadVerified,
            ByteMetric::PeerWireReceived => Self::PeerWireReceived,
            ByteMetric::PeerWireSent => Self::PeerWireSent,
            ByteMetric::PeerProtocolReceived => Self::PeerProtocolReceived,
            ByteMetric::PeerProtocolSent => Self::PeerProtocolSent,
            ByteMetric::MetadataPayloadReceived => Self::MetadataPayloadReceived,
            ByteMetric::MetadataPayloadSent => Self::MetadataPayloadSent,
            ByteMetric::PeerUnclassifiedReceived => Self::PeerUnclassifiedReceived,
            ByteMetric::PeerUnclassifiedSent => Self::PeerUnclassifiedSent,
            ByteMetric::DhtReceived => Self::DhtReceived,
            ByteMetric::DhtSent => Self::DhtSent,
            ByteMetric::TrackerReceived => Self::TrackerReceived,
            ByteMetric::TrackerSent => Self::TrackerSent,
            ByteMetric::LogicalHashRead => Self::LogicalHashRead,
            ByteMetric::PayloadRedundant => Self::PayloadRedundant,
            ByteMetric::PayloadHashFailed => Self::PayloadHashFailed,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum SpeedRange {
    Seconds30,
    Minutes2,
    Minutes10,
    Hour1,
    Hours24,
    Days30,
    Years2,
}

impl SpeedRange {
    const fn tier(self) -> TierConfig {
        match self {
            Self::Seconds30 => TIERS[0],
            Self::Minutes2 => TIERS[1],
            Self::Minutes10 => TIERS[2],
            Self::Hour1 => TIERS[3],
            Self::Hours24 => TIERS[4],
            Self::Days30 => TIERS[5],
            Self::Years2 => TIERS[6],
        }
    }

    pub const fn is_live(self) -> bool {
        matches!(
            self,
            Self::Seconds30 | Self::Minutes2 | Self::Minutes10 | Self::Hour1
        )
    }

    pub(crate) const fn tick_millis(self) -> Option<u64> {
        match self {
            Self::Seconds30 => Some(100),
            Self::Minutes2 => Some(500),
            Self::Minutes10 | Self::Hour1 => Some(1_000),
            Self::Hours24 | Self::Days30 | Self::Years2 => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct SpeedSeriesView {
    pub metric: SpeedMetric,
    pub current_rate_bytes: String,
    pub values: Vec<Option<String>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct SpeedMetricAvailability {
    pub metric: SpeedMetric,
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct SpeedCurrentRate {
    pub metric: SpeedMetric,
    pub bytes: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum SpeedPersistenceState {
    Healthy,
    Degraded,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct SpeedHistoryView {
    pub captured_millis: String,
    pub history_epoch: String,
    pub range: SpeedRange,
    pub bucket_millis: String,
    pub start_millis: String,
    pub complete_through_millis: String,
    pub live: bool,
    pub persistence: SpeedPersistenceState,
    pub current: Vec<SpeedCurrentRate>,
    pub series: Vec<SpeedSeriesView>,
    pub catalog: Vec<SpeedMetricAvailability>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TierConfig {
    bucket_millis: u64,
    count: usize,
    persistent: bool,
}

const TIERS: [TierConfig; 7] = [
    TierConfig {
        bucket_millis: 100,
        count: 300,
        persistent: false,
    },
    TierConfig {
        bucket_millis: 500,
        count: 240,
        persistent: false,
    },
    TierConfig {
        bucket_millis: 2_000,
        count: 300,
        persistent: false,
    },
    TierConfig {
        bucket_millis: 10_000,
        count: 360,
        persistent: false,
    },
    TierConfig {
        bucket_millis: 60_000,
        count: 1_440,
        persistent: true,
    },
    TierConfig {
        bucket_millis: 15 * 60_000,
        count: 2_880,
        persistent: true,
    },
    TierConfig {
        bucket_millis: 24 * 60 * 60_000,
        count: 730,
        persistent: true,
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Bucket {
    start_millis: u64,
    bytes: u64,
}

#[derive(Clone, Debug)]
struct TierHistory {
    config: TierConfig,
    buckets: Vec<Option<Bucket>>,
    last_advanced_start: Option<u64>,
}

impl TierHistory {
    fn new(config: TierConfig) -> Self {
        Self {
            config,
            buckets: vec![None; config.count],
            last_advanced_start: None,
        }
    }

    fn bucket_start(&self, timestamp_millis: u64) -> u64 {
        timestamp_millis / self.config.bucket_millis * self.config.bucket_millis
    }

    fn index(&self, start_millis: u64) -> usize {
        ((start_millis / self.config.bucket_millis) % self.config.count as u64) as usize
    }

    fn insert(&mut self, start_millis: u64, bytes: u64) {
        let index = self.index(start_millis);
        match &mut self.buckets[index] {
            Some(bucket) if bucket.start_millis == start_millis => {
                bucket.bytes = bucket.bytes.saturating_add(bytes);
            }
            slot => {
                *slot = Some(Bucket {
                    start_millis,
                    bytes,
                });
            }
        }
    }

    fn restore(&mut self, start_millis: u64, bytes: u64) {
        let index = self.index(start_millis);
        self.buckets[index] = Some(Bucket {
            start_millis,
            bytes,
        });
        self.last_advanced_start = Some(
            self.last_advanced_start
                .map_or(start_millis, |last| last.max(start_millis)),
        );
    }

    fn advance_to(&mut self, timestamp_millis: u64) -> Option<(u64, u64)> {
        let target = self.bucket_start(timestamp_millis);
        let first = self.last_advanced_start.map_or(target, |last| {
            last.saturating_add(self.config.bucket_millis)
        });
        if first > target {
            return None;
        }
        let elapsed = (target - first) / self.config.bucket_millis;
        let skip = elapsed.saturating_sub(self.config.count as u64 - 1);
        let mut start = first.saturating_add(skip.saturating_mul(self.config.bucket_millis));
        let retained_first = start;
        while start <= target {
            let index = self.index(start);
            if self.buckets[index].is_none_or(|bucket| bucket.start_millis != start) {
                self.buckets[index] = Some(Bucket {
                    start_millis: start,
                    bytes: 0,
                });
            }
            let Some(next) = start.checked_add(self.config.bucket_millis) else {
                break;
            };
            start = next;
        }
        self.last_advanced_start = Some(target);
        Some((retained_first, target))
    }

    fn resume_at(&mut self, timestamp_millis: u64) {
        let start = self.bucket_start(timestamp_millis);
        let index = self.index(start);
        if self.buckets[index].is_none_or(|bucket| bucket.start_millis != start) {
            self.buckets[index] = Some(Bucket {
                start_millis: start,
                bytes: 0,
            });
        }
        self.last_advanced_start = Some(start);
    }

    fn value(&self, start_millis: u64) -> Option<u64> {
        self.buckets[self.index(start_millis)]
            .filter(|bucket| bucket.start_millis == start_millis)
            .map(|bucket| bucket.bytes)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SessionRateHistory {
    tiers: BTreeMap<SpeedMetric, Vec<TierHistory>>,
    dirty_persistent: BTreeSet<(SpeedMetric, u64, u64)>,
    history_epoch: String,
    clock: HistoryClock,
    persistence_degraded: bool,
}

impl SessionRateHistory {
    pub(crate) fn new() -> Self {
        let clock = HistoryClock::new();
        Self {
            tiers: SpeedMetric::AVAILABLE
                .into_iter()
                .map(|metric| (metric, TIERS.into_iter().map(TierHistory::new).collect()))
                .collect(),
            dirty_persistent: BTreeSet::new(),
            history_epoch: format!("{}-{}", clock.anchor_millis, std::process::id(),),
            clock,
            persistence_degraded: false,
        }
    }

    pub(crate) fn now_millis(&self) -> u64 {
        self.clock.now_millis()
    }

    pub(crate) fn record_at(&mut self, metric: SpeedMetric, bytes: u64, now_millis: u64) {
        let Some(tiers) = self.tiers.get_mut(&metric) else {
            return;
        };
        let mut dirty = Vec::with_capacity(3);
        for tier in tiers {
            let bucket_start = tier.bucket_start(now_millis);
            if let Some((first, last)) = tier.advance_to(now_millis)
                && tier.config.persistent
            {
                dirty.extend(
                    bucket_starts(tier.config.bucket_millis, first, last)
                        .map(|start| (metric, tier.config.bucket_millis, start)),
                );
            }
            tier.insert(bucket_start, bytes);
            if tier.config.persistent {
                dirty.push((metric, tier.config.bucket_millis, bucket_start));
            }
        }
        self.dirty_persistent.extend(dirty);
    }

    pub(crate) fn advance_to(&mut self, now_millis: u64) {
        let mut dirty = Vec::new();
        for (metric, tiers) in &mut self.tiers {
            for tier in tiers {
                if let Some((first, last)) = tier.advance_to(now_millis)
                    && tier.config.persistent
                {
                    dirty.extend(
                        bucket_starts(tier.config.bucket_millis, first, last)
                            .map(|start| (*metric, tier.config.bucket_millis, start)),
                    );
                }
            }
        }
        self.dirty_persistent.extend(dirty);
    }

    fn resume_at(&mut self, now_millis: u64) {
        let mut dirty = Vec::with_capacity(SpeedMetric::AVAILABLE.len() * 3);
        for (metric, tiers) in &mut self.tiers {
            for tier in tiers {
                tier.resume_at(now_millis);
                if tier.config.persistent {
                    dirty.push((
                        *metric,
                        tier.config.bucket_millis,
                        tier.bucket_start(now_millis),
                    ));
                }
            }
        }
        self.dirty_persistent.extend(dirty);
    }

    pub(crate) fn view(
        &mut self,
        range: SpeedRange,
        metrics: &[SpeedMetric],
        now_millis: u64,
    ) -> SpeedHistoryView {
        self.advance_to(now_millis);
        let config = range.tier();
        let tier_index = TIERS
            .iter()
            .position(|tier| *tier == config)
            .expect("range tier belongs to fixed catalog");
        let current_start = now_millis / config.bucket_millis * config.bucket_millis;
        let complete_through = current_start.saturating_sub(config.bucket_millis);
        let start = complete_through.saturating_sub(
            config
                .bucket_millis
                .saturating_mul(config.count.saturating_sub(1) as u64),
        );
        let series = metrics
            .iter()
            .filter_map(|metric| {
                let tiers = self.tiers.get(metric)?;
                let tier = &tiers[tier_index];
                let values = (0..config.count)
                    .map(|index| {
                        let timestamp =
                            start.saturating_add(config.bucket_millis.saturating_mul(index as u64));
                        tier.value(timestamp).map(|value| value.to_string())
                    })
                    .collect();
                Some(SpeedSeriesView {
                    metric: *metric,
                    current_rate_bytes: self.current_rate(*metric, now_millis).to_string(),
                    values,
                })
            })
            .collect();
        SpeedHistoryView {
            captured_millis: now_millis.to_string(),
            history_epoch: self.history_epoch.clone(),
            range,
            bucket_millis: config.bucket_millis.to_string(),
            start_millis: start.to_string(),
            complete_through_millis: complete_through.to_string(),
            live: range.is_live(),
            persistence: if self.persistence_degraded {
                SpeedPersistenceState::Degraded
            } else {
                SpeedPersistenceState::Healthy
            },
            current: SpeedMetric::AVAILABLE
                .into_iter()
                .map(|metric| SpeedCurrentRate {
                    metric,
                    bytes: self.current_rate(metric, now_millis).to_string(),
                })
                .collect(),
            series,
            catalog: metric_catalog(),
        }
    }

    fn current_rate(&self, metric: SpeedMetric, now_millis: u64) -> u64 {
        let Some(tier) = self.tiers.get(&metric).and_then(|tiers| tiers.first()) else {
            return 0;
        };
        let current_start = now_millis / 100 * 100;
        let complete = current_start.saturating_sub(100);
        let total = (0..10_u64)
            .filter_map(|offset| tier.value(complete.saturating_sub(offset * 100)))
            .fold(0_u64, u64::saturating_add);
        total
    }

    fn restore(&mut self, row: PersistRow) {
        let Some(metric_tiers) = self.tiers.get_mut(&row.metric) else {
            return;
        };
        let Some(tier) = metric_tiers
            .iter_mut()
            .find(|tier| tier.config.bucket_millis == row.bucket_millis)
        else {
            return;
        };
        tier.restore(row.start_millis, row.bytes);
    }

    pub(crate) fn drain_persistent_rows(
        &mut self,
        now_millis: u64,
        include_active: bool,
    ) -> Vec<PersistRow> {
        let mut output = Vec::new();
        let mut drained = Vec::new();
        for &(metric, bucket_millis, start_millis) in &self.dirty_persistent {
            let current = now_millis / bucket_millis * bucket_millis;
            if start_millis > current || (!include_active && start_millis == current) {
                continue;
            }
            let Some(tier) = self.tiers.get(&metric).and_then(|tiers| {
                tiers
                    .iter()
                    .find(|tier| tier.config.bucket_millis == bucket_millis)
            }) else {
                drained.push((metric, bucket_millis, start_millis));
                continue;
            };
            if let Some(bytes) = tier.value(start_millis) {
                output.push(PersistRow {
                    bucket_millis,
                    start_millis,
                    metric,
                    bytes,
                    complete: start_millis < current,
                });
            }
            drained.push((metric, bucket_millis, start_millis));
        }
        for key in drained {
            self.dirty_persistent.remove(&key);
        }
        output.sort_by_key(|row| (row.bucket_millis, row.start_millis, row.metric));
        output
    }

    pub(crate) fn set_persistence_degraded(&mut self, degraded: bool) {
        self.persistence_degraded = degraded;
    }
}

fn bucket_starts(bucket_millis: u64, first: u64, last: u64) -> impl Iterator<Item = u64> {
    (0..=((last - first) / bucket_millis))
        .map(move |offset| first.saturating_add(offset.saturating_mul(bucket_millis)))
}

#[derive(Clone, Debug)]
struct HistoryClock {
    anchor: Instant,
    anchor_millis: u64,
}

impl HistoryClock {
    fn new() -> Self {
        Self {
            anchor: Instant::now(),
            anchor_millis: unix_millis(),
        }
    }

    fn now_millis(&self) -> u64 {
        self.anchor_millis.saturating_add(
            self.anchor
                .elapsed()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX),
        )
    }
}

fn metric_catalog() -> Vec<SpeedMetricAvailability> {
    SpeedMetric::AVAILABLE
        .into_iter()
        .map(|metric| SpeedMetricAvailability {
            metric,
            available: true,
            reason: None,
        })
        .chain([SpeedMetricAvailability {
            metric: SpeedMetric::PayloadUploaded,
            available: false,
            reason: Some("upload and seeding are not implemented".to_owned()),
        }])
        .collect()
}

#[derive(Clone, Debug)]
pub(crate) struct SessionSpeedRecorder {
    history: Arc<Mutex<SessionRateHistory>>,
}

impl SessionSpeedRecorder {
    fn new(history: Arc<Mutex<SessionRateHistory>>) -> Self {
        Self { history }
    }
}

#[derive(Debug)]
pub(crate) struct PreparedSpeedHistory {
    pub(crate) history: Arc<Mutex<SessionRateHistory>>,
    pub(crate) recorder: Arc<SessionSpeedRecorder>,
    store: Option<MetricsStore>,
}

impl PreparedSpeedHistory {
    pub(crate) fn open(profile_root: &Path) -> Self {
        let mut history = SessionRateHistory::new();
        let now = history.now_millis();
        let store = match MetricsStore::open(profile_root) {
            Ok(store) => match store.load(now) {
                Ok(rows) => {
                    for row in rows {
                        let current = now / row.bucket_millis * row.bucket_millis;
                        if row.complete || row.start_millis == current {
                            history.restore(row);
                        }
                    }
                    Some(store)
                }
                Err(_) => {
                    history.set_persistence_degraded(true);
                    None
                }
            },
            Err(_) => {
                history.set_persistence_degraded(true);
                None
            }
        };
        history.resume_at(now);
        let history = Arc::new(Mutex::new(history));
        let recorder = Arc::new(SessionSpeedRecorder::new(history.clone()));
        Self {
            history,
            recorder,
            store,
        }
    }

    pub(crate) fn start(self, views: ViewHub) -> SpeedHistoryRuntime {
        SpeedHistoryRuntime::start(self, views)
    }
}

#[derive(Debug)]
enum WriterCommand {
    Write(Vec<PersistRow>),
    Shutdown,
}

#[derive(Debug)]
pub(crate) struct SpeedHistoryRuntime {
    cancellation: CancellationToken,
    task: Option<JoinHandle<()>>,
}

impl SpeedHistoryRuntime {
    fn start(prepared: PreparedSpeedHistory, views: ViewHub) -> Self {
        let cancellation = CancellationToken::new();
        let task_cancellation = cancellation.clone();
        let history = prepared.history;
        let writer = prepared.store.map(|mut store| {
            let (sender, receiver) = sync_channel::<WriterCommand>(1);
            let writer_history = history.clone();
            let join = thread::Builder::new()
                .name("rstorrent-metrics-writer".to_owned())
                .spawn(move || {
                    while let Ok(command) = receiver.recv() {
                        match command {
                            WriterCommand::Write(rows) => {
                                let degraded = store.write(&rows).is_err();
                                if let Ok(mut history) = writer_history.lock() {
                                    history.set_persistence_degraded(degraded);
                                }
                            }
                            WriterCommand::Shutdown => break,
                        }
                    }
                })
                .ok();
            (sender, join)
        });
        let notify = views.speed_interest_notify();
        let task = tokio::spawn(async move {
            let mut persistence = interval(Duration::from_secs(60));
            persistence.set_missed_tick_behavior(MissedTickBehavior::Skip);
            persistence.tick().await;
            loop {
                if let Some(tick) = views.speed_tick_interval() {
                    tokio::select! {
                        biased;
                        _ = task_cancellation.cancelled() => break,
                        _ = persistence.tick() => persist_history(&history, writer.as_ref()),
                        _ = tokio::time::sleep(tick) => {
                            let _ = views.publish_speed_tick();
                        }
                    }
                } else {
                    tokio::select! {
                        biased;
                        _ = task_cancellation.cancelled() => break,
                        _ = persistence.tick() => persist_history(&history, writer.as_ref()),
                        () = notify.notified() => {}
                    }
                }
            }
            if let Some((sender, join)) = writer {
                let rows = {
                    let mut history = history
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let now = history.now_millis();
                    history.advance_to(now);
                    history.drain_persistent_rows(now, true)
                };
                let _ = tokio::task::spawn_blocking(move || {
                    let _ = sender.send(WriterCommand::Write(rows));
                    let _ = sender.send(WriterCommand::Shutdown);
                    if let Some(join) = join {
                        let _ = join.join();
                    }
                })
                .await;
            }
        });
        Self {
            cancellation,
            task: Some(task),
        }
    }

    pub(crate) async fn shutdown(mut self) -> Result<(), tokio::task::JoinError> {
        self.cancellation.cancel();
        let Some(task) = self.task.take() else {
            return Ok(());
        };
        task.await
    }
}

impl Drop for SpeedHistoryRuntime {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

fn persist_history(
    history: &Arc<Mutex<SessionRateHistory>>,
    writer: Option<&(SyncSender<WriterCommand>, Option<thread::JoinHandle<()>>)>,
) {
    let rows = {
        let mut history = history
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = history.now_millis();
        history.advance_to(now);
        history.drain_persistent_rows(now, false)
    };
    let Some((sender, _)) = writer else {
        return;
    };
    if sender.try_send(WriterCommand::Write(rows)).is_err()
        && let Ok(mut history) = history.lock()
    {
        history.set_persistence_degraded(true);
    }
}

impl ByteMetricSink for SessionSpeedRecorder {
    fn record(&self, metric: ByteMetric, bytes: u64) {
        if let Ok(mut history) = self.history.lock() {
            let now = history.now_millis();
            history.record_at(metric.into(), bytes, now);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PersistRow {
    bucket_millis: u64,
    start_millis: u64,
    metric: SpeedMetric,
    bytes: u64,
    complete: bool,
}

#[derive(Debug)]
pub(crate) struct MetricsStore {
    connection: Connection,
}

impl MetricsStore {
    pub(crate) fn open(profile_root: &Path) -> Result<Self, MetricsStoreError> {
        std::fs::create_dir_all(profile_root).map_err(MetricsStoreError::Io)?;
        let path = profile_root.join(METRICS_DATABASE_FILENAME);
        let mut connection = Connection::open(&path)?;
        connection.busy_timeout(METRICS_BUSY_TIMEOUT)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        migrate_metrics(&mut connection)?;
        Ok(Self { connection })
    }

    pub(crate) fn load(&self, now_millis: u64) -> Result<Vec<PersistRow>, MetricsStoreError> {
        let oldest = now_millis.saturating_sub(730 * 24 * 60 * 60_000);
        let mut statement = self.connection.prepare(
            "SELECT bucket_millis, start_millis, metric, bytes, complete
             FROM rate_buckets WHERE start_millis >= ?1
             ORDER BY bucket_millis, start_millis, metric",
        )?;
        let rows = statement.query_map([to_i64(oldest)], |row| {
            let metric: String = row.get(2)?;
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                metric,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;
        let mut output = Vec::new();
        for row in rows {
            let (bucket_millis, start_millis, metric, bytes, complete) = row?;
            let Some(metric) = SpeedMetric::from_storage_name(&metric) else {
                continue;
            };
            let (Ok(bucket_millis), Ok(start_millis), Ok(bytes)) = (
                u64::try_from(bucket_millis),
                u64::try_from(start_millis),
                u64::try_from(bytes),
            ) else {
                continue;
            };
            output.push(PersistRow {
                bucket_millis,
                start_millis,
                metric,
                bytes,
                complete: complete != 0,
            });
        }
        Ok(output)
    }

    pub(crate) fn write(&mut self, rows: &[PersistRow]) -> Result<(), MetricsStoreError> {
        let transaction = self.connection.transaction()?;
        {
            let mut statement = transaction.prepare(
                "INSERT INTO rate_buckets
                    (bucket_millis, start_millis, metric, bytes, complete)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(bucket_millis, start_millis, metric)
                 DO UPDATE SET bytes = excluded.bytes,
                               complete = excluded.complete",
            )?;
            for row in rows {
                statement.execute(params![
                    to_i64(row.bucket_millis),
                    to_i64(row.start_millis),
                    row.metric.storage_name(),
                    to_i64(row.bytes),
                    i64::from(row.complete),
                ])?;
            }
        }
        for tier in TIERS.into_iter().filter(|tier| tier.persistent) {
            let newest = rows
                .iter()
                .filter(|row| row.bucket_millis == tier.bucket_millis)
                .map(|row| row.start_millis)
                .max();
            if let Some(newest) = newest {
                let oldest = newest.saturating_sub(
                    tier.bucket_millis
                        .saturating_mul(tier.count.saturating_sub(1) as u64),
                );
                transaction.execute(
                    "DELETE FROM rate_buckets
                     WHERE bucket_millis = ?1 AND start_millis < ?2",
                    params![to_i64(tier.bucket_millis), to_i64(oldest)],
                )?;
            }
        }
        transaction.commit()?;
        Ok(())
    }
}

fn migrate_metrics(connection: &mut Connection) -> Result<(), MetricsStoreError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS metrics_meta (
            singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
            schema_version INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS rate_buckets (
            bucket_millis INTEGER NOT NULL CHECK (bucket_millis > 0),
            start_millis INTEGER NOT NULL CHECK (start_millis >= 0),
            metric TEXT NOT NULL,
            bytes INTEGER NOT NULL CHECK (bytes >= 0),
            complete INTEGER NOT NULL CHECK (complete IN (0, 1)),
            PRIMARY KEY (bucket_millis, start_millis, metric)
         ) WITHOUT ROWID;",
    )?;
    let version = connection.query_row(
        "SELECT schema_version FROM metrics_meta WHERE singleton = 1",
        [],
        |row| row.get::<_, i64>(0),
    );
    match version {
        Ok(version) if version == METRICS_SCHEMA_VERSION => Ok(()),
        Ok(_) => Err(MetricsStoreError::UnsupportedSchema),
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            connection.execute(
                "INSERT INTO metrics_meta (singleton, schema_version) VALUES (1, ?1)",
                [METRICS_SCHEMA_VERSION],
            )?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

pub(crate) fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[derive(Debug)]
pub(crate) enum MetricsStoreError {
    Io(std::io::Error),
    Sql(rusqlite::Error),
    UnsupportedSchema,
}

impl fmt::Display for MetricsStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "metrics store I/O: {error}"),
            Self::Sql(error) => write!(formatter, "metrics store SQLite: {error}"),
            Self::UnsupportedSchema => formatter.write_str("unsupported metrics store schema"),
        }
    }
}

impl Error for MetricsStoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Sql(error) => Some(error),
            Self::UnsupportedSchema => None,
        }
    }
}

impl From<rusqlite::Error> for MetricsStoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sql(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_bytes_are_additive_and_failed_bytes_do_not_reduce_verified() {
        let mut history = SessionRateHistory::new();
        let now = 1_000_000;
        history.record_at(SpeedMetric::PayloadReceived, 16_384, now - 900);
        history.record_at(SpeedMetric::StagedWrite, 16_384, now - 800);
        history.record_at(SpeedMetric::PayloadHashFailed, 16_384, now - 700);
        history.record_at(SpeedMetric::PayloadReceived, 16_384, now - 600);
        history.record_at(SpeedMetric::StagedWrite, 16_384, now - 500);
        history.record_at(SpeedMetric::PayloadVerified, 16_384, now - 400);
        let view = history.view(
            SpeedRange::Seconds30,
            &[
                SpeedMetric::PayloadReceived,
                SpeedMetric::StagedWrite,
                SpeedMetric::PayloadVerified,
                SpeedMetric::PayloadHashFailed,
            ],
            now,
        );
        let totals = view
            .series
            .iter()
            .map(|series| {
                series
                    .values
                    .iter()
                    .filter_map(|value| value.as_deref())
                    .map(|value| value.parse::<u64>().expect("bytes"))
                    .sum::<u64>()
            })
            .collect::<Vec<_>>();
        assert_eq!(totals, [32_768, 32_768, 16_384, 16_384]);
    }

    #[test]
    fn tier_rollups_conserve_bytes_and_leave_prestart_gaps_null() {
        let mut history = SessionRateHistory::new();
        let now = 3_600_000;
        for offset in 0..100_u64 {
            history.record_at(
                SpeedMetric::PayloadReceived,
                10,
                now - 10_000 + offset * 100,
            );
        }
        let live = history.view(SpeedRange::Seconds30, &[SpeedMetric::PayloadReceived], now);
        let hourly = history.view(SpeedRange::Hour1, &[SpeedMetric::PayloadReceived], now);
        let total = |view: &SpeedHistoryView| {
            view.series[0]
                .values
                .iter()
                .filter_map(|value| value.as_deref())
                .map(|value| value.parse::<u64>().expect("bytes"))
                .sum::<u64>()
        };
        assert_eq!(total(&live), 1_000);
        assert_eq!(total(&hourly), 1_000);
        assert!(hourly.series[0].values.iter().any(Option::is_none));
    }

    #[test]
    fn persistent_tiers_round_trip_in_separate_database() {
        let root = std::env::temp_dir().join(format!(
            "rstorrent-speed-{}-{}",
            std::process::id(),
            unix_millis()
        ));
        let now = 2_000_000_000;
        let mut history = SessionRateHistory::new();
        history.record_at(SpeedMetric::PayloadReceived, 4096, now - 60_000);
        history.advance_to(now);
        let rows = history.drain_persistent_rows(now, false);
        assert!(rows.len() <= 54, "one close batch stays bounded");
        let mut store = MetricsStore::open(&root).expect("open metrics");
        assert!(root.join("metrics.db").is_file());
        store.write(&rows).expect("write metrics");
        let loaded = store.load(now).expect("load metrics");
        assert!(loaded.iter().any(|row| {
            row.metric == SpeedMetric::PayloadReceived
                && row.bucket_millis == 60_000
                && row.bytes == 4096
        }));
        drop(store);
        std::fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn clean_flush_includes_active_coarse_accumulators_once() {
        let now = 2_000_012_345;
        let mut history = SessionRateHistory::new();
        history.record_at(SpeedMetric::PayloadReceived, 2048, now);
        let rows = history.drain_persistent_rows(now, true);
        assert_eq!(
            rows.iter()
                .filter(|row| row.metric == SpeedMetric::PayloadReceived)
                .count(),
            3,
        );
        assert!(rows.iter().all(|row| !row.complete));
        assert!(history.drain_persistent_rows(now, true).is_empty());
    }
}
