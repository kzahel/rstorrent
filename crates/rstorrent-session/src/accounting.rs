//! One bounded, task-free durable torrent-accounting accumulator.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rstorrent_engine::{ByteMetric, ByteMetricSink, TorrentId};
use tokio::sync::Notify;

use crate::store::{MAX_ACCOUNTING_BATCH, TorrentAccounting, TorrentAccountingUpdate};

const PAYLOAD_FLUSH_THRESHOLD: u64 = 1024 * 1024;
const TIMER_FLUSH_INTERVAL: Duration = Duration::from_secs(5);
const MAX_DURABLE_VALUE: u64 = i64::MAX as u64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AccountingActivity {
    pub(crate) torrent_id: TorrentId,
    pub(crate) generation: u64,
    pub(crate) active: bool,
    pub(crate) finished: bool,
    pub(crate) seeding: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ActivityState {
    active: bool,
    finished: bool,
    seeding: bool,
}

impl From<AccountingActivity> for ActivityState {
    fn from(activity: AccountingActivity) -> Self {
        Self {
            active: activity.active,
            finished: activity.finished,
            seeding: activity.seeding,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AccountingEntry {
    generation: u64,
    current: TorrentAccounting,
    persisted: TorrentAccounting,
    activity: ActivityState,
    last_observed: Duration,
    pending_active: Duration,
    pending_finished: Duration,
    pending_seeding: Duration,
    dirty_since: Option<Duration>,
}

impl AccountingEntry {
    fn new(generation: u64, durable: TorrentAccounting, now: Duration) -> Self {
        Self {
            generation,
            current: durable,
            persisted: durable,
            activity: ActivityState::default(),
            last_observed: now,
            pending_active: Duration::ZERO,
            pending_finished: Duration::ZERO,
            pending_seeding: Duration::ZERO,
            dirty_since: None,
        }
    }

    fn advance(&mut self, now: Duration) -> Result<u64, AccountingError> {
        let previous_observed = self.last_observed;
        let elapsed = now
            .checked_sub(previous_observed)
            .ok_or(AccountingError::MonotonicTimeRegressed)?;
        self.last_observed = now;
        if self.activity.active {
            self.pending_active = self.pending_active.saturating_add(elapsed);
        }
        if self.activity.finished {
            self.pending_finished = self.pending_finished.saturating_add(elapsed);
        }
        if self.activity.seeding {
            self.pending_seeding = self.pending_seeding.saturating_add(elapsed);
        }
        let active = take_whole_seconds(&mut self.pending_active);
        let finished = take_whole_seconds(&mut self.pending_finished);
        let seeding = take_whole_seconds(&mut self.pending_seeding);
        let mut saturation_events = 0_u64;
        if active != 0 || finished != 0 || seeding != 0 {
            let (active_seconds, active_saturated) =
                bounded_add(self.current.active_seconds, active);
            let (finished_seconds, finished_saturated) =
                bounded_add(self.current.finished_seconds, finished);
            let (seeding_seconds, seeding_saturated) =
                bounded_add(self.current.seeding_seconds, seeding);
            self.current.active_seconds = active_seconds;
            self.current.finished_seconds = finished_seconds;
            self.current.seeding_seconds = seeding_seconds;
            saturation_events = u64::from(active_saturated)
                + u64::from(finished_saturated)
                + u64::from(seeding_saturated);
            self.dirty_since.get_or_insert(previous_observed);
        }
        Ok(saturation_events)
    }

    fn set_activity(
        &mut self,
        generation: u64,
        activity: ActivityState,
        now: Duration,
    ) -> Result<u64, AccountingError> {
        if generation != self.generation {
            return Ok(0);
        }
        if activity.seeding && !activity.finished || activity.finished && !activity.active {
            return Err(AccountingError::MalformedActivity);
        }
        let saturation_events = self.advance(now)?;
        self.activity = activity;
        Ok(saturation_events)
    }

    fn dirty(&self) -> bool {
        self.current != self.persisted
    }
}

fn take_whole_seconds(duration: &mut Duration) -> u64 {
    let seconds = duration.as_secs();
    *duration = duration.saturating_sub(Duration::from_secs(seconds));
    seconds
}

fn bounded_add(current: u64, delta: u64) -> (u64, bool) {
    match current.checked_add(delta) {
        Some(total) if total <= MAX_DURABLE_VALUE => (total, false),
        _ => (MAX_DURABLE_VALUE, true),
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct AccountingHighWater {
    pub(crate) tracked_rows: usize,
    pub(crate) uncommitted_payload_bytes: u64,
    pub(crate) uncommitted_timer_seconds: u64,
    pub(crate) dirty_rows: usize,
    pub(crate) saturation_events: u64,
}

#[derive(Debug, Default)]
struct AccountingModel {
    entries: BTreeMap<TorrentId, AccountingEntry>,
    high_water: AccountingHighWater,
}

impl AccountingModel {
    fn register(
        &mut self,
        torrent_id: TorrentId,
        generation: u64,
        durable: TorrentAccounting,
        now: Duration,
    ) -> Result<(), AccountingError> {
        if generation == 0 {
            return Err(AccountingError::ZeroGeneration);
        }
        if durable.finished_seconds > durable.active_seconds
            || durable.seeding_seconds > durable.finished_seconds
        {
            return Err(AccountingError::MalformedDurableTimers);
        }
        let saturation_events = match self.entries.get_mut(&torrent_id) {
            Some(entry) => {
                let saturation_events = entry.advance(now)?;
                if durable.total_uploaded > entry.current.total_uploaded
                    || durable.total_downloaded > entry.current.total_downloaded
                    || durable.active_seconds > entry.current.active_seconds
                    || durable.finished_seconds > entry.current.finished_seconds
                    || durable.seeding_seconds > entry.current.seeding_seconds
                {
                    entry.current = durable;
                    entry.persisted = durable;
                    entry.dirty_since = None;
                }
                entry.generation = generation;
                entry.activity = ActivityState::default();
                saturation_events
            }
            None => {
                self.entries
                    .insert(torrent_id, AccountingEntry::new(generation, durable, now));
                0
            }
        };
        self.high_water.saturation_events = self
            .high_water
            .saturation_events
            .saturating_add(saturation_events);
        self.high_water.tracked_rows = self.high_water.tracked_rows.max(self.entries.len());
        Ok(())
    }

    fn observe_payload(
        &mut self,
        torrent_id: TorrentId,
        generation: u64,
        downloaded: u64,
        uploaded: u64,
        now: Duration,
    ) -> Result<bool, AccountingError> {
        let before_threshold = self.uncommitted_payload_bytes() >= PAYLOAD_FLUSH_THRESHOLD;
        let entry = self
            .entries
            .get_mut(&torrent_id)
            .ok_or(AccountingError::UnknownTorrent)?;
        if entry.generation != generation {
            return Ok(false);
        }
        let timer_saturations = entry.advance(now)?;
        let (next_downloaded, download_saturated) =
            bounded_add(entry.current.total_downloaded, downloaded);
        let (next_uploaded, upload_saturated) = bounded_add(entry.current.total_uploaded, uploaded);
        entry.current.total_downloaded = next_downloaded;
        entry.current.total_uploaded = next_uploaded;
        if downloaded != 0 || uploaded != 0 {
            entry.dirty_since.get_or_insert(now);
        }
        let payload_saturations = u64::from(download_saturated || upload_saturated);
        self.high_water.saturation_events = self
            .high_water
            .saturation_events
            .saturating_add(timer_saturations.saturating_add(payload_saturations));
        self.update_high_water();
        Ok(!before_threshold && self.uncommitted_payload_bytes() >= PAYLOAD_FLUSH_THRESHOLD)
    }

    fn observe_tracker_counts(
        &mut self,
        torrent_id: TorrentId,
        generation: u64,
        complete: Option<u32>,
        incomplete: Option<u32>,
        now: Duration,
    ) -> Result<(), AccountingError> {
        let entry = self
            .entries
            .get_mut(&torrent_id)
            .ok_or(AccountingError::UnknownTorrent)?;
        if entry.generation != generation {
            return Ok(());
        }
        let saturation_events = entry.advance(now)?;
        let mut changed = false;
        if let Some(complete) = complete
            && entry.current.tracker_complete != Some(complete)
        {
            entry.current.tracker_complete = Some(complete);
            changed = true;
        }
        if let Some(incomplete) = incomplete
            && entry.current.tracker_incomplete != Some(incomplete)
        {
            entry.current.tracker_incomplete = Some(incomplete);
            changed = true;
        }
        if changed {
            entry.dirty_since.get_or_insert(now);
            self.update_high_water();
        }
        self.high_water.saturation_events = self
            .high_water
            .saturation_events
            .saturating_add(saturation_events);
        Ok(())
    }

    fn observe_activities(
        &mut self,
        activities: &[AccountingActivity],
        now: Duration,
    ) -> Result<(), AccountingError> {
        for activity in activities {
            let entry = self
                .entries
                .get_mut(&activity.torrent_id)
                .ok_or(AccountingError::UnknownTorrent)?;
            let saturation_events =
                entry.set_activity(activity.generation, (*activity).into(), now)?;
            self.high_water.saturation_events = self
                .high_water
                .saturation_events
                .saturating_add(saturation_events);
        }
        self.update_high_water();
        Ok(())
    }

    fn current(
        &mut self,
        torrent_id: TorrentId,
        generation: u64,
        now: Duration,
    ) -> Result<TorrentAccounting, AccountingError> {
        let entry = self
            .entries
            .get_mut(&torrent_id)
            .ok_or(AccountingError::UnknownTorrent)?;
        if entry.generation != generation {
            return Err(AccountingError::UnknownTorrent);
        }
        let saturation_events = entry.advance(now)?;
        let current = entry.current;
        self.high_water.saturation_events = self
            .high_water
            .saturation_events
            .saturating_add(saturation_events);
        self.update_high_water();
        Ok(current)
    }

    fn flush_due(&self, now: Duration, force: bool) -> bool {
        if !self.entries.values().any(AccountingEntry::dirty) {
            return false;
        }
        force
            || self.uncommitted_payload_bytes() >= PAYLOAD_FLUSH_THRESHOLD
            || self.entries.values().any(|entry| {
                entry
                    .dirty_since
                    .and_then(|dirty_since| now.checked_sub(dirty_since))
                    .is_some_and(|age| age >= TIMER_FLUSH_INTERVAL)
            })
    }

    fn prepare_flush(&self) -> Vec<TorrentAccountingUpdate> {
        self.entries
            .iter()
            .filter(|(_, entry)| entry.dirty())
            .take(MAX_ACCOUNTING_BATCH)
            .map(|(torrent_id, entry)| TorrentAccountingUpdate {
                torrent_id: *torrent_id,
                accounting: entry.current,
            })
            .collect()
    }

    fn acknowledge(&mut self, updates: &[TorrentAccountingUpdate], now: Duration) {
        for update in updates {
            let Some(entry) = self.entries.get_mut(&update.torrent_id) else {
                continue;
            };
            entry.persisted = update.accounting;
            if !entry.dirty() {
                entry.dirty_since = None;
            } else {
                entry.dirty_since.get_or_insert(now);
            }
        }
        self.update_high_water();
    }

    fn discard(&mut self, torrent_id: TorrentId) {
        self.entries.remove(&torrent_id);
    }

    fn uncommitted_payload_bytes(&self) -> u64 {
        self.entries.values().fold(0_u64, |total, entry| {
            total
                .saturating_add(
                    entry
                        .current
                        .total_uploaded
                        .saturating_sub(entry.persisted.total_uploaded),
                )
                .saturating_add(
                    entry
                        .current
                        .total_downloaded
                        .saturating_sub(entry.persisted.total_downloaded),
                )
        })
    }

    fn uncommitted_timer_seconds(&self) -> u64 {
        self.entries.values().fold(0_u64, |maximum, entry| {
            maximum.max(
                entry
                    .current
                    .active_seconds
                    .saturating_sub(entry.persisted.active_seconds),
            )
        })
    }

    fn update_high_water(&mut self) {
        self.high_water.uncommitted_payload_bytes = self
            .high_water
            .uncommitted_payload_bytes
            .max(self.uncommitted_payload_bytes());
        self.high_water.uncommitted_timer_seconds = self
            .high_water
            .uncommitted_timer_seconds
            .max(self.uncommitted_timer_seconds());
        self.high_water.dirty_rows = self
            .high_water
            .dirty_rows
            .max(self.entries.values().filter(|entry| entry.dirty()).count());
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TorrentAccountingOwner {
    started_at: std::time::Instant,
    model: Arc<Mutex<AccountingModel>>,
    wake: Arc<Notify>,
}

impl TorrentAccountingOwner {
    pub(crate) fn new(wake: Arc<Notify>) -> Self {
        Self {
            started_at: std::time::Instant::now(),
            model: Arc::new(Mutex::new(AccountingModel::default())),
            wake,
        }
    }

    fn now(&self) -> Duration {
        self.started_at.elapsed()
    }

    pub(crate) fn register(
        &self,
        torrent_id: TorrentId,
        generation: u64,
        durable: TorrentAccounting,
    ) -> Result<(), AccountingError> {
        self.model()?
            .register(torrent_id, generation, durable, self.now())
    }

    pub(crate) fn observe_payload(
        &self,
        torrent_id: TorrentId,
        generation: u64,
        downloaded: u64,
        uploaded: u64,
    ) -> Result<(), AccountingError> {
        let notify = self.model()?.observe_payload(
            torrent_id,
            generation,
            downloaded,
            uploaded,
            self.now(),
        )?;
        if notify {
            self.wake.notify_one();
        }
        Ok(())
    }

    pub(crate) fn metric_sink(
        &self,
        torrent_id: TorrentId,
        generation: u64,
        upstream: Option<Arc<dyn ByteMetricSink>>,
    ) -> Arc<dyn ByteMetricSink> {
        Arc::new(TorrentAccountingMetricSink {
            torrent_id,
            generation,
            accounting: self.clone(),
            upstream,
        })
    }

    pub(crate) fn observe_tracker_counts(
        &self,
        torrent_id: TorrentId,
        generation: u64,
        complete: Option<u32>,
        incomplete: Option<u32>,
    ) -> Result<(), AccountingError> {
        self.model()?.observe_tracker_counts(
            torrent_id,
            generation,
            complete,
            incomplete,
            self.now(),
        )
    }

    pub(crate) fn observe_activities(
        &self,
        activities: &[AccountingActivity],
    ) -> Result<(), AccountingError> {
        self.model()?.observe_activities(activities, self.now())
    }

    pub(crate) fn current(
        &self,
        torrent_id: TorrentId,
        generation: u64,
    ) -> Result<TorrentAccounting, AccountingError> {
        self.model()?.current(torrent_id, generation, self.now())
    }

    pub(crate) fn prepare_flush(
        &self,
        force: bool,
    ) -> Result<Vec<TorrentAccountingUpdate>, AccountingError> {
        let model = self.model()?;
        if model.flush_due(self.now(), force) {
            Ok(model.prepare_flush())
        } else {
            Ok(Vec::new())
        }
    }

    pub(crate) fn acknowledge(
        &self,
        updates: &[TorrentAccountingUpdate],
    ) -> Result<(), AccountingError> {
        self.model()?.acknowledge(updates, self.now());
        Ok(())
    }

    pub(crate) fn discard(&self, torrent_id: TorrentId) -> Result<(), AccountingError> {
        self.model()?.discard(torrent_id);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn high_water(&self) -> Result<AccountingHighWater, AccountingError> {
        Ok(self.model()?.high_water)
    }

    fn model(&self) -> Result<std::sync::MutexGuard<'_, AccountingModel>, AccountingError> {
        self.model.lock().map_err(|_| AccountingError::Poisoned)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AccountingError {
    Poisoned,
    ZeroGeneration,
    UnknownTorrent,
    MonotonicTimeRegressed,
    MalformedDurableTimers,
    MalformedActivity,
}

impl fmt::Display for AccountingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Poisoned => formatter.write_str("torrent accounting lock is poisoned"),
            Self::ZeroGeneration => formatter.write_str("torrent accounting generation is zero"),
            Self::UnknownTorrent => formatter.write_str("torrent accounting owner is unknown"),
            Self::MonotonicTimeRegressed => {
                formatter.write_str("torrent accounting monotonic time regressed")
            }
            Self::MalformedDurableTimers => {
                formatter.write_str("durable torrent accounting timers are malformed")
            }
            Self::MalformedActivity => {
                formatter.write_str("torrent accounting activity is malformed")
            }
        }
    }
}

impl std::error::Error for AccountingError {}

#[derive(Clone, Debug)]
struct TorrentAccountingMetricSink {
    torrent_id: TorrentId,
    generation: u64,
    accounting: TorrentAccountingOwner,
    upstream: Option<Arc<dyn ByteMetricSink>>,
}

impl ByteMetricSink for TorrentAccountingMetricSink {
    fn record(&self, metric: ByteMetric, bytes: u64) {
        if let Some(upstream) = &self.upstream {
            upstream.record(metric, bytes);
        }
        let (downloaded, uploaded) = match metric {
            ByteMetric::PayloadReceived => (bytes, 0),
            ByteMetric::PayloadUploaded => (0, bytes),
            _ => return,
        };
        let _ =
            self.accounting
                .observe_payload(self.torrent_id, self.generation, downloaded, uploaded);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn torrent(byte: u8) -> TorrentId {
        TorrentId::new([byte; 16]).unwrap()
    }

    #[test]
    fn absolute_generation_deltas_and_late_generations_are_exact() {
        let id = torrent(1);
        let mut model = AccountingModel::default();
        model
            .register(id, 7, TorrentAccounting::default(), Duration::ZERO)
            .unwrap();
        model
            .observe_payload(id, 7, 10, 20, Duration::from_secs(1))
            .unwrap();
        model
            .observe_payload(id, 6, 100, 100, Duration::from_secs(2))
            .unwrap();
        assert_eq!(model.entries[&id].current.total_downloaded, 10);
        assert_eq!(model.entries[&id].current.total_uploaded, 20);
    }

    #[test]
    fn timers_accrue_only_for_nested_unpaused_activity() {
        let id = torrent(2);
        let mut model = AccountingModel::default();
        model
            .register(id, 1, TorrentAccounting::default(), Duration::ZERO)
            .unwrap();
        model
            .observe_activities(
                &[AccountingActivity {
                    torrent_id: id,
                    generation: 1,
                    active: true,
                    finished: false,
                    seeding: false,
                }],
                Duration::ZERO,
            )
            .unwrap();
        model
            .observe_activities(
                &[AccountingActivity {
                    torrent_id: id,
                    generation: 1,
                    active: true,
                    finished: true,
                    seeding: true,
                }],
                Duration::from_millis(1_500),
            )
            .unwrap();
        model
            .observe_activities(
                &[AccountingActivity {
                    torrent_id: id,
                    generation: 1,
                    active: false,
                    finished: false,
                    seeding: false,
                }],
                Duration::from_secs(4),
            )
            .unwrap();
        let current = model.entries[&id].current;
        assert_eq!(current.active_seconds, 4);
        assert_eq!(current.finished_seconds, 2);
        assert_eq!(current.seeding_seconds, 2);
        assert_eq!(model.high_water.tracked_rows, 1);
        assert_eq!(model.high_water.uncommitted_timer_seconds, 4);
        assert_eq!(model.high_water.dirty_rows, 1);
    }

    #[test]
    fn one_mebibyte_and_five_seconds_trigger_bounded_flushes() {
        let id = torrent(3);
        let mut model = AccountingModel::default();
        model
            .register(id, 1, TorrentAccounting::default(), Duration::ZERO)
            .unwrap();
        assert!(
            !model
                .observe_payload(id, 1, PAYLOAD_FLUSH_THRESHOLD - 1, 0, Duration::ZERO,)
                .unwrap()
        );
        assert!(!model.flush_due(Duration::from_secs(4), false));
        assert!(model.flush_due(Duration::from_secs(5), false));
        assert!(
            model
                .observe_payload(id, 1, 1, 0, Duration::from_secs(5))
                .unwrap()
        );
        assert_eq!(model.high_water.tracked_rows, 1);
        assert_eq!(
            model.high_water.uncommitted_payload_bytes,
            PAYLOAD_FLUSH_THRESHOLD
        );
        assert_eq!(model.high_water.uncommitted_timer_seconds, 0);
        assert_eq!(model.high_water.dirty_rows, 1);
        let prepared = model.prepare_flush();
        assert_eq!(prepared.len(), 1);
        model.acknowledge(&prepared, Duration::from_secs(5));
        assert!(!model.flush_due(Duration::from_secs(10), false));
    }

    #[test]
    fn saturation_is_bounded_and_observable() {
        let id = torrent(4);
        let mut model = AccountingModel::default();
        model
            .register(
                id,
                1,
                TorrentAccounting {
                    total_uploaded: MAX_DURABLE_VALUE - 1,
                    ..TorrentAccounting::default()
                },
                Duration::ZERO,
            )
            .unwrap();
        model.observe_payload(id, 1, 0, 2, Duration::ZERO).unwrap();
        assert_eq!(model.entries[&id].current.total_uploaded, MAX_DURABLE_VALUE);
        assert_eq!(model.high_water.saturation_events, 1);
    }

    #[test]
    fn timer_saturation_is_bounded_and_observable() {
        let id = torrent(6);
        let mut model = AccountingModel::default();
        model
            .register(
                id,
                1,
                TorrentAccounting {
                    active_seconds: MAX_DURABLE_VALUE,
                    ..TorrentAccounting::default()
                },
                Duration::ZERO,
            )
            .unwrap();
        model
            .observe_activities(
                &[AccountingActivity {
                    torrent_id: id,
                    generation: 1,
                    active: true,
                    finished: false,
                    seeding: false,
                }],
                Duration::ZERO,
            )
            .unwrap();
        model
            .observe_activities(
                &[AccountingActivity {
                    torrent_id: id,
                    generation: 1,
                    active: false,
                    finished: false,
                    seeding: false,
                }],
                Duration::from_secs(1),
            )
            .unwrap();
        assert_eq!(model.entries[&id].current.active_seconds, MAX_DURABLE_VALUE);
        assert_eq!(model.high_water.saturation_events, 1);
    }

    #[test]
    fn tracker_counts_keep_the_latest_known_values() {
        let id = torrent(7);
        let mut model = AccountingModel::default();
        model
            .register(id, 1, TorrentAccounting::default(), Duration::ZERO)
            .unwrap();
        model
            .observe_tracker_counts(id, 1, Some(9), Some(4), Duration::ZERO)
            .unwrap();
        model
            .observe_tracker_counts(id, 1, None, Some(3), Duration::from_secs(1))
            .unwrap();
        assert_eq!(model.entries[&id].current.tracker_complete, Some(9));
        assert_eq!(model.entries[&id].current.tracker_incomplete, Some(3));
        assert!(model.entries[&id].dirty());
    }

    #[test]
    fn malformed_activity_and_time_regression_fail_closed() {
        let id = torrent(5);
        let mut model = AccountingModel::default();
        model
            .register(id, 1, TorrentAccounting::default(), Duration::from_secs(2))
            .unwrap();
        assert_eq!(
            model.observe_activities(
                &[AccountingActivity {
                    torrent_id: id,
                    generation: 1,
                    active: false,
                    finished: true,
                    seeding: true,
                }],
                Duration::from_secs(2),
            ),
            Err(AccountingError::MalformedActivity)
        );
        assert_eq!(
            model.observe_payload(id, 1, 1, 0, Duration::from_secs(1)),
            Err(AccountingError::MonotonicTimeRegressed)
        );
    }
}
