use std::collections::BTreeSet;
use std::time::Duration;

use rstorrent_protocol::magnet::UdpTrackerUrl;
use rstorrent_protocol::udp_tracker::AnnounceEvent;

pub(crate) const TRACKER_BACKOFF_RATIO: u64 = 250;
pub(crate) const TRACKER_RETRY_MIN: Duration = Duration::from_secs(5);
pub(crate) const TRACKER_RETRY_MAX: Duration = Duration::from_secs(60 * 60);
pub(crate) const TRACKER_ANNOUNCE_MIN: Duration = Duration::from_secs(5 * 60);
pub(crate) const TRACKER_ANNOUNCE_MAX: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_TRACKER_FAILURES: u8 = 127;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct TrackerId(u8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TrackerSource {
    Magnet,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrackerRecord {
    id: TrackerId,
    url: UdpTrackerUrl,
    tier: u8,
    source: TrackerSource,
    failures: u8,
    total_attempts: u32,
    start_acknowledged: bool,
    updating: bool,
    last_success: Option<Duration>,
    last_failure: Option<Duration>,
    next_announce: Duration,
    interval: Duration,
}

impl TrackerRecord {
    fn new(id: TrackerId, url: UdpTrackerUrl) -> Self {
        Self {
            id,
            url,
            tier: 0,
            source: TrackerSource::Magnet,
            failures: 0,
            total_attempts: 0,
            start_acknowledged: false,
            updating: false,
            last_success: None,
            last_failure: None,
            next_announce: Duration::ZERO,
            interval: TRACKER_ANNOUNCE_MIN,
        }
    }

    #[cfg(test)]
    fn failures(&self) -> u8 {
        self.failures
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TrackerAction {
    Announce {
        id: TrackerId,
        url: UdpTrackerUrl,
        tier: u8,
        source: TrackerSource,
        event: AnnounceEvent,
        attempt: u32,
        fallback: bool,
    },
    Wait {
        delay: Duration,
        url: UdpTrackerUrl,
        kind: TrackerWaitKind,
    },
    Pending,
    Exhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TrackerWaitKind {
    FailureRetry,
    Reannounce,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TrackerFailure {
    pub failures: u8,
    pub retry_in: Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TrackerSuccess {
    pub interval: Duration,
}

#[derive(Debug)]
pub(crate) struct TrackerSchedule {
    records: Vec<TrackerRecord>,
    attempted: BTreeSet<TrackerId>,
    round_not_before: Duration,
    round_tracker: Option<TrackerId>,
    round_wait_kind: Option<TrackerWaitKind>,
}

impl TrackerSchedule {
    pub(crate) fn new(urls: Vec<UdpTrackerUrl>) -> Self {
        debug_assert!(urls.len() <= usize::from(u8::MAX) + 1);
        Self {
            records: urls
                .into_iter()
                .enumerate()
                .map(|(index, url)| TrackerRecord::new(TrackerId(index as u8), url))
                .collect(),
            attempted: BTreeSet::new(),
            round_not_before: Duration::ZERO,
            round_tracker: None,
            round_wait_kind: None,
        }
    }

    pub(crate) fn next_action(&mut self, now: Duration) -> TrackerAction {
        loop {
            if self.records.is_empty() {
                return TrackerAction::Exhausted;
            }
            if now < self.round_not_before {
                let tracker = self
                    .records
                    .iter()
                    .find(|record| Some(record.id) == self.round_tracker)
                    .expect("scheduled wait retains its tracker");
                return TrackerAction::Wait {
                    delay: self.round_not_before - now,
                    url: tracker.url.clone(),
                    kind: self
                        .round_wait_kind
                        .expect("scheduled wait retains its reason"),
                };
            }
            if let Some(record) = self
                .records
                .iter_mut()
                .find(|record| !self.attempted.contains(&record.id) && record.next_announce <= now)
            {
                let fallback = !self.attempted.is_empty();
                self.attempted.insert(record.id);
                record.total_attempts = record.total_attempts.saturating_add(1);
                record.updating = true;
                return TrackerAction::Announce {
                    id: record.id,
                    url: record.url.clone(),
                    tier: record.tier,
                    source: record.source,
                    event: if record.start_acknowledged {
                        AnnounceEvent::None
                    } else {
                        AnnounceEvent::Started
                    },
                    attempt: record.total_attempts,
                    fallback,
                };
            }

            if self.records.iter().any(|record| record.updating) {
                return TrackerAction::Pending;
            }

            if let Some(record) = self
                .records
                .iter()
                .filter(|record| !self.attempted.contains(&record.id))
                .min_by_key(|record| record.next_announce)
            {
                return TrackerAction::Wait {
                    delay: record.next_announce.saturating_sub(now),
                    url: record.url.clone(),
                    kind: TrackerWaitKind::FailureRetry,
                };
            }

            self.attempted.clear();
            let earliest = self
                .records
                .iter()
                .min_by_key(|record| record.next_announce)
                .expect("nonempty schedule has an earliest tracker");
            self.round_not_before = earliest.next_announce;
            self.round_tracker = Some(earliest.id);
            self.round_wait_kind = Some(TrackerWaitKind::FailureRetry);
        }
    }

    pub(crate) fn failed(&mut self, id: TrackerId, now: Duration) -> TrackerFailure {
        let record = self.record_mut(id);
        record.updating = false;
        record.failures = record.failures.saturating_add(1).min(MAX_TRACKER_FAILURES);
        record.last_failure = Some(now);
        let retry_in = tracker_failure_delay(record.failures);
        record.next_announce = now.saturating_add(retry_in);
        TrackerFailure {
            failures: record.failures,
            retry_in,
        }
    }

    pub(crate) fn succeeded(
        &mut self,
        id: TrackerId,
        now: Duration,
        interval_seconds: u32,
    ) -> TrackerSuccess {
        let interval = Duration::from_secs(u64::from(interval_seconds))
            .clamp(TRACKER_ANNOUNCE_MIN, TRACKER_ANNOUNCE_MAX);
        let position = self
            .records
            .iter()
            .position(|record| record.id == id)
            .expect("selected tracker record remains installed");
        {
            let record = &mut self.records[position];
            record.updating = false;
            record.failures = 0;
            record.start_acknowledged = true;
            record.last_success = Some(now);
            record.interval = interval;
            record.next_announce = now.saturating_add(interval);
        }
        if position != 0 {
            let record = self.records.remove(position);
            self.records.insert(0, record);
        }
        self.attempted.clear();
        self.round_not_before = now.saturating_add(interval);
        self.round_tracker = Some(id);
        self.round_wait_kind = Some(TrackerWaitKind::Reannounce);
        TrackerSuccess { interval }
    }

    fn record_mut(&mut self, id: TrackerId) -> &mut TrackerRecord {
        self.records
            .iter_mut()
            .find(|record| record.id == id)
            .expect("selected tracker record remains installed")
    }
}

fn tracker_failure_delay(failures: u8) -> Duration {
    let failures = u64::from(failures);
    let delay = TRACKER_RETRY_MIN.as_secs().saturating_add(
        TRACKER_RETRY_MIN
            .as_secs()
            .saturating_mul(TRACKER_BACKOFF_RATIO)
            .saturating_mul(failures.saturating_mul(failures))
            / 100,
    );
    Duration::from_secs(delay.min(TRACKER_RETRY_MAX.as_secs()))
}

#[cfg(test)]
mod tests {
    use super::{
        TRACKER_ANNOUNCE_MAX, TRACKER_ANNOUNCE_MIN, TRACKER_RETRY_MAX, TrackerAction,
        TrackerSchedule, TrackerWaitKind, tracker_failure_delay,
    };
    use rstorrent_protocol::magnet::UdpTrackerUrl;
    use rstorrent_protocol::udp_tracker::AnnounceEvent;
    use std::time::Duration;

    fn tracker(name: &str, port: u16) -> UdpTrackerUrl {
        UdpTrackerUrl {
            host: name.to_owned(),
            port,
        }
    }

    fn announce(schedule: &mut TrackerSchedule, now: Duration) -> super::TrackerId {
        let TrackerAction::Announce { id, .. } = schedule.next_action(now) else {
            panic!("tracker should be eligible");
        };
        id
    }

    #[test]
    fn failure_backoff_is_quadratic_saturating_and_unlimited() {
        assert_eq!(tracker_failure_delay(1), Duration::from_secs(17));
        assert_eq!(tracker_failure_delay(2), Duration::from_secs(55));
        assert_eq!(tracker_failure_delay(3), Duration::from_secs(117));
        assert_eq!(tracker_failure_delay(4), Duration::from_secs(205));
        assert_eq!(tracker_failure_delay(127), TRACKER_RETRY_MAX);

        let mut schedule = TrackerSchedule::new(vec![tracker("tracker.example", 80)]);
        let mut now = Duration::ZERO;
        for _ in 0..140 {
            let id = announce(&mut schedule, now);
            let failure = schedule.failed(id, now);
            now += failure.retry_in;
        }
        assert_eq!(schedule.records[0].failures(), 127);
        assert!(matches!(
            schedule.next_action(now),
            TrackerAction::Announce { .. }
        ));
    }

    #[test]
    fn failure_falls_through_and_success_promotes_the_tracker() {
        let first = tracker("first.example", 80);
        let second = tracker("second.example", 81);
        let mut schedule = TrackerSchedule::new(vec![first.clone(), second.clone()]);

        let TrackerAction::Announce {
            id: first_id,
            url,
            event,
            fallback,
            ..
        } = schedule.next_action(Duration::ZERO)
        else {
            panic!("first tracker should be eligible");
        };
        assert_eq!(url, first);
        assert_eq!(event, AnnounceEvent::Started);
        assert!(!fallback);
        schedule.failed(first_id, Duration::ZERO);

        let TrackerAction::Announce {
            id: second_id,
            url,
            event,
            fallback,
            ..
        } = schedule.next_action(Duration::ZERO)
        else {
            panic!("second tracker should be the fallback");
        };
        assert_eq!(url, second);
        assert_eq!(event, AnnounceEvent::Started);
        assert!(fallback);
        let success = schedule.succeeded(second_id, Duration::ZERO, 1);
        assert_eq!(success.interval, TRACKER_ANNOUNCE_MIN);

        assert_eq!(
            schedule.next_action(TRACKER_ANNOUNCE_MIN - Duration::from_secs(1)),
            TrackerAction::Wait {
                delay: Duration::from_secs(1),
                url: second.clone(),
                kind: TrackerWaitKind::Reannounce,
            }
        );
        assert!(matches!(
            schedule.next_action(TRACKER_ANNOUNCE_MIN),
            TrackerAction::Announce {
                url,
                event: AnnounceEvent::None,
                fallback: false,
                ..
            } if url == second
        ));
    }

    #[test]
    fn concurrent_round_does_not_reselect_inflight_trackers() {
        let mut schedule = TrackerSchedule::new(vec![
            tracker("first.example", 80),
            tracker("second.example", 81),
        ]);
        let first = announce(&mut schedule, Duration::ZERO);
        let second = announce(&mut schedule, Duration::ZERO);
        assert_eq!(schedule.next_action(Duration::ZERO), TrackerAction::Pending);

        schedule.failed(first, Duration::ZERO);
        assert_eq!(schedule.next_action(Duration::ZERO), TrackerAction::Pending);
        schedule.failed(second, Duration::ZERO);
        assert!(matches!(
            schedule.next_action(Duration::ZERO),
            TrackerAction::Wait {
                kind: TrackerWaitKind::FailureRetry,
                ..
            }
        ));
    }

    #[test]
    fn successful_intervals_are_bounded() {
        let mut schedule = TrackerSchedule::new(vec![tracker("tracker.example", 80)]);
        let id = announce(&mut schedule, Duration::ZERO);
        assert_eq!(
            schedule.succeeded(id, Duration::ZERO, 0).interval,
            TRACKER_ANNOUNCE_MIN
        );

        let id = announce(&mut schedule, TRACKER_ANNOUNCE_MIN);
        assert_eq!(
            schedule
                .succeeded(id, TRACKER_ANNOUNCE_MIN, u32::MAX)
                .interval,
            TRACKER_ANNOUNCE_MAX
        );
    }

    #[test]
    fn a_successful_round_waits_instead_of_hammering_backups() {
        let first = tracker("first.example", 80);
        let second = tracker("second.example", 81);
        let mut schedule = TrackerSchedule::new(vec![first.clone(), second]);
        let id = announce(&mut schedule, Duration::ZERO);
        schedule.succeeded(id, Duration::ZERO, 600);

        assert_eq!(
            schedule.next_action(Duration::from_secs(10)),
            TrackerAction::Wait {
                delay: Duration::from_secs(590),
                url: first,
                kind: TrackerWaitKind::Reannounce,
            }
        );
    }

    #[test]
    fn retry_wait_names_the_earliest_eligible_tracker() {
        let slow = tracker("slow.example", 80);
        let fast = tracker("fast.example", 81);
        let mut schedule = TrackerSchedule::new(vec![slow, fast.clone()]);
        let slow_id = announce(&mut schedule, Duration::ZERO);
        for _ in 0..5 {
            schedule.failed(slow_id, Duration::ZERO);
        }
        let fast_id = announce(&mut schedule, Duration::ZERO);
        schedule.failed(fast_id, Duration::ZERO);

        assert_eq!(
            schedule.next_action(Duration::ZERO),
            TrackerAction::Wait {
                delay: Duration::from_secs(17),
                url: fast,
                kind: TrackerWaitKind::FailureRetry,
            }
        );
    }
}
