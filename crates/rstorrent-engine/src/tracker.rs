use std::collections::BTreeSet;
use std::time::Duration;

use rstorrent_protocol::magnet::{MAX_TRACKER_URL_LENGTH, UdpTrackerUrl};
use rstorrent_protocol::udp_tracker::AnnounceEvent;

pub(crate) const TRACKER_BACKOFF_RATIO: u64 = 250;
pub(crate) const TRACKER_RETRY_MIN: Duration = Duration::from_secs(5);
pub(crate) const TRACKER_RETRY_MAX: Duration = Duration::from_secs(60 * 60);
pub(crate) const TRACKER_ANNOUNCE_MIN: Duration = Duration::from_secs(5 * 60);
pub(crate) const TRACKER_ANNOUNCE_MAX: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_TRACKER_FAILURES: u8 = 127;
pub(crate) const MAX_TRACKER_ERROR_LENGTH: usize = 256;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct TrackerId(u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackerSource {
    Magnet,
    Metainfo,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackerTransport {
    Udp,
    Http,
    Https,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TrackerHttpsAuthentication {
    #[default]
    SystemTrust,
    Disabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackerConnectionFamily {
    Ipv4,
    Ipv6,
}

impl TrackerConnectionFamily {
    pub(crate) const fn matches(self, address: std::net::SocketAddr) -> bool {
        matches!(
            (self, address),
            (Self::Ipv4, std::net::SocketAddr::V4(_)) | (Self::Ipv6, std::net::SocketAddr::V6(_))
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrackerEndpoint {
    Udp(UdpTrackerUrl),
    Http {
        target: url::Url,
        transport: TrackerTransport,
    },
}

impl TrackerEndpoint {
    pub fn from_http_url(value: &str) -> Option<Self> {
        if value.is_empty()
            || value.len() > MAX_TRACKER_URL_LENGTH
            || !value.is_ascii()
            || value
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte == b' ')
            || value.contains('#')
        {
            return None;
        }
        let (_, remainder) = value.split_once("://")?;
        let authority_end = remainder.find(['/', '?']).unwrap_or(remainder.len());
        let host_port = remainder[..authority_end]
            .rsplit_once('@')
            .map_or(&remainder[..authority_end], |(_, host_port)| host_port);
        if host_port.is_empty() {
            return None;
        }
        let target = url::Url::parse(value).ok()?;
        if target.host().is_none() || target.fragment().is_some() {
            return None;
        }
        let transport = match target.scheme() {
            "http" => TrackerTransport::Http,
            "https" => TrackerTransport::Https,
            _ => return None,
        };
        Some(Self::Http { target, transport })
    }

    pub const fn transport(&self) -> TrackerTransport {
        match self {
            Self::Udp(_) => TrackerTransport::Udp,
            Self::Http { transport, .. } => *transport,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackerRuntimeStatus {
    Inactive,
    Disabled,
    Idle,
    Announcing,
    RetryWait,
    ReannounceWait,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackerNextAction {
    Announce,
    Retry,
    Reannounce,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackerAnnounceEvent {
    Started,
    Update,
    Completed,
    Stopped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TrackerPriorityEvent {
    Update,
    Completed,
    Stopped,
}

impl TrackerPriorityEvent {
    const fn wire_event(self) -> AnnounceEvent {
        match self {
            Self::Update => AnnounceEvent::None,
            Self::Completed => AnnounceEvent::Completed,
            Self::Stopped => AnnounceEvent::Stopped,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrackerRuntimeRecordSnapshot {
    pub tracker_id: String,
    pub url: String,
    pub tier: u32,
    pub source: TrackerSource,
    pub transport: TrackerTransport,
    pub https_authentication: Option<TrackerHttpsAuthentication>,
    pub status: TrackerRuntimeStatus,
    pub announce_event: Option<TrackerAnnounceEvent>,
    pub total_attempts: u32,
    pub consecutive_failures: u8,
    pub last_connection_family: Option<TrackerConnectionFamily>,
    pub last_peer_count: Option<u32>,
    pub seeders: Option<u32>,
    pub leechers: Option<u32>,
    pub interval: Option<Duration>,
    pub next_action: Option<TrackerNextAction>,
    pub next_action_in: Option<Duration>,
    pub last_success_age: Option<Duration>,
    pub last_failure_age: Option<Duration>,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrackerConfig {
    pub url: String,
    pub endpoint: TrackerEndpoint,
    pub tier: u32,
    pub position: u32,
    pub source: TrackerSource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrackerRuntimeSnapshot {
    pub captured_at: Duration,
    pub active: bool,
    pub records: Vec<TrackerRuntimeRecordSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrackerRecord {
    id: TrackerId,
    endpoint: TrackerEndpoint,
    display_url: String,
    tier: u32,
    position: u32,
    source: TrackerSource,
    failures: u8,
    total_attempts: u32,
    start_acknowledged: bool,
    pending_event: Option<TrackerPriorityEvent>,
    inflight_event: Option<AnnounceEvent>,
    stopped: bool,
    disabled: bool,
    updating: bool,
    last_success: Option<Duration>,
    last_failure: Option<Duration>,
    last_connection_family: Option<TrackerConnectionFamily>,
    next_announce: Duration,
    interval: Option<Duration>,
    last_peer_count: Option<u32>,
    seeders: Option<u32>,
    leechers: Option<u32>,
    last_error: Option<String>,
    operation_https_authentication: Option<TrackerHttpsAuthentication>,
}

impl TrackerRecord {
    fn new(id: TrackerId, config: TrackerConfig) -> Self {
        Self {
            id,
            endpoint: config.endpoint,
            display_url: config.url,
            tier: config.tier,
            position: config.position,
            source: config.source,
            failures: 0,
            total_attempts: 0,
            start_acknowledged: false,
            pending_event: None,
            inflight_event: None,
            stopped: false,
            disabled: false,
            updating: false,
            last_success: None,
            last_failure: None,
            last_connection_family: None,
            next_announce: Duration::ZERO,
            interval: None,
            last_peer_count: None,
            seeders: None,
            leechers: None,
            last_error: None,
            operation_https_authentication: None,
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
        url: String,
        endpoint: TrackerEndpoint,
        tier: u32,
        source: TrackerSource,
        event: AnnounceEvent,
        attempt: u32,
        fallback: bool,
    },
    Wait {
        delay: Duration,
        url: String,
        endpoint: TrackerEndpoint,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TrackerAcceptedOutcome {
    pub requested_interval: Duration,
    pub peer_count: u32,
    pub seeders: Option<u32>,
    pub leechers: Option<u32>,
    pub connection_family: Option<TrackerConnectionFamily>,
}

#[derive(Debug)]
pub(crate) struct TrackerSchedule {
    records: Vec<TrackerRecord>,
    attempted: BTreeSet<TrackerId>,
    round_not_before: Duration,
    round_tracker: Option<TrackerId>,
    round_wait_kind: Option<TrackerWaitKind>,
    stopping: bool,
    https_authentication: TrackerHttpsAuthentication,
}

impl TrackerSchedule {
    #[cfg(test)]
    pub(crate) fn new(urls: Vec<UdpTrackerUrl>) -> Self {
        let configs = urls
            .into_iter()
            .enumerate()
            .map(|(position, endpoint)| TrackerConfig {
                url: tracker_label(&endpoint),
                endpoint: TrackerEndpoint::Udp(endpoint),
                tier: 0,
                position: position.try_into().unwrap_or(u32::MAX),
                source: TrackerSource::Magnet,
            })
            .collect();
        Self::from_configs(configs)
    }

    pub(crate) fn from_configs(configs: Vec<TrackerConfig>) -> Self {
        Self {
            records: configs
                .into_iter()
                .enumerate()
                .map(|(index, config)| {
                    TrackerRecord::new(
                        TrackerId(index.try_into().expect("tracker count fits u32")),
                        config,
                    )
                })
                .collect(),
            attempted: BTreeSet::new(),
            round_not_before: Duration::ZERO,
            round_tracker: None,
            round_wait_kind: None,
            stopping: false,
            https_authentication: TrackerHttpsAuthentication::SystemTrust,
        }
    }

    pub(crate) fn set_https_authentication(&mut self, authentication: TrackerHttpsAuthentication) {
        self.https_authentication = authentication;
    }

    pub(crate) fn request_update(&mut self) {
        if self.stopping {
            return;
        }
        for record in &mut self.records {
            if !record.stopped
                && (record.start_acknowledged
                    || record.inflight_event == Some(AnnounceEvent::Started))
            {
                record.pending_event = Some(TrackerPriorityEvent::Update);
                record.next_announce = Duration::ZERO;
            }
        }
        self.reset_round();
    }

    pub(crate) fn request_completed(&mut self) {
        if self.stopping {
            return;
        }
        for record in &mut self.records {
            if !record.stopped
                && (record.start_acknowledged
                    || record.inflight_event == Some(AnnounceEvent::Started))
            {
                record.pending_event = Some(TrackerPriorityEvent::Completed);
                record.next_announce = Duration::ZERO;
            }
        }
        self.reset_round();
    }

    pub(crate) fn request_stop(&mut self) {
        self.stopping = true;
        for record in &mut self.records {
            record.pending_event = record
                .start_acknowledged
                .then_some(TrackerPriorityEvent::Stopped);
            record.next_announce = Duration::ZERO;
        }
        self.reset_round();
    }

    pub(crate) fn cancel_inflight(&mut self) {
        for record in &mut self.records {
            record.updating = false;
            record.inflight_event = None;
        }
        self.reset_round();
    }

    pub(crate) fn stop_complete(&self) -> bool {
        self.stopping
            && self
                .records
                .iter()
                .all(|record| !record.updating && record.pending_event.is_none())
    }

    pub(crate) fn next_action(&mut self, now: Duration) -> TrackerAction {
        loop {
            if self.records.is_empty() {
                return TrackerAction::Exhausted;
            }
            if let Some(record) = self.records.iter_mut().find(|record| {
                !record.updating
                    && !record.stopped
                    && !record.disabled
                    && record.pending_event.is_some()
                    && !self.attempted.contains(&record.id)
            }) {
                let fallback = !self.attempted.is_empty();
                self.attempted.insert(record.id);
                record.total_attempts = record.total_attempts.saturating_add(1);
                record.updating = true;
                let event = record
                    .pending_event
                    .expect("priority selection retains its event")
                    .wire_event();
                record.inflight_event = Some(event);
                if record.endpoint.transport() == TrackerTransport::Https {
                    record.operation_https_authentication = Some(self.https_authentication);
                }
                return TrackerAction::Announce {
                    id: record.id,
                    url: record.display_url.clone(),
                    endpoint: record.endpoint.clone(),
                    tier: record.tier,
                    source: record.source,
                    event,
                    attempt: record.total_attempts,
                    fallback,
                };
            }
            if self.stopping {
                if self.records.iter().any(|record| record.updating) {
                    return TrackerAction::Pending;
                }
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
                    url: tracker.display_url.clone(),
                    endpoint: tracker.endpoint.clone(),
                    kind: self
                        .round_wait_kind
                        .expect("scheduled wait retains its reason"),
                };
            }
            if let Some(record) = self.records.iter_mut().find(|record| {
                !record.disabled
                    && !self.attempted.contains(&record.id)
                    && record.next_announce <= now
            }) {
                let fallback = !self.attempted.is_empty();
                self.attempted.insert(record.id);
                record.total_attempts = record.total_attempts.saturating_add(1);
                record.updating = true;
                let event = if record.start_acknowledged {
                    AnnounceEvent::None
                } else {
                    AnnounceEvent::Started
                };
                record.inflight_event = Some(event);
                if record.endpoint.transport() == TrackerTransport::Https {
                    record.operation_https_authentication = Some(self.https_authentication);
                }
                return TrackerAction::Announce {
                    id: record.id,
                    url: record.display_url.clone(),
                    endpoint: record.endpoint.clone(),
                    tier: record.tier,
                    source: record.source,
                    event,
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
                .filter(|record| !record.disabled && !self.attempted.contains(&record.id))
                .min_by_key(|record| record.next_announce)
            {
                return TrackerAction::Wait {
                    delay: record.next_announce.saturating_sub(now),
                    url: record.display_url.clone(),
                    endpoint: record.endpoint.clone(),
                    kind: TrackerWaitKind::FailureRetry,
                };
            }

            self.attempted.clear();
            let Some(earliest) = self
                .records
                .iter()
                .filter(|record| !record.disabled)
                .min_by_key(|record| record.next_announce)
            else {
                return TrackerAction::Exhausted;
            };
            self.round_not_before = earliest.next_announce;
            self.round_tracker = Some(earliest.id);
            self.round_wait_kind = Some(TrackerWaitKind::FailureRetry);
        }
    }

    pub(crate) fn failed(&mut self, id: TrackerId, now: Duration, detail: &str) -> TrackerFailure {
        let failures = self.record_mut(id).failures.saturating_add(1);
        let retry_in = tracker_failure_delay(failures);
        self.failed_with_retry(id, now, detail, retry_in)
    }

    pub(crate) fn failed_with_retry(
        &mut self,
        id: TrackerId,
        now: Duration,
        detail: &str,
        retry_in: Duration,
    ) -> TrackerFailure {
        let record = self.record_mut(id);
        record.updating = false;
        record.inflight_event = None;
        record.failures = record.failures.saturating_add(1).min(MAX_TRACKER_FAILURES);
        record.last_failure = Some(now);
        record.last_error = Some(bounded_tracker_error(detail));
        record.next_announce = now.saturating_add(retry_in);
        TrackerFailure {
            failures: record.failures,
            retry_in,
        }
    }

    pub(crate) fn disable(&mut self, id: TrackerId, now: Duration, detail: &str) {
        let record = self.record_mut(id);
        record.updating = false;
        record.inflight_event = None;
        record.pending_event = None;
        record.start_acknowledged = false;
        record.disabled = true;
        record.last_failure = Some(now);
        record.last_error = Some(bounded_tracker_error(detail));
    }

    pub(crate) fn supersede(&mut self, id: TrackerId) {
        let stopping = self.stopping;
        let record = self.record_mut(id);
        record.updating = false;
        record.inflight_event = None;
        record.next_announce = Duration::ZERO;
        if record.start_acknowledged && !stopping {
            record.pending_event = Some(TrackerPriorityEvent::Update);
        }
        self.reset_round();
    }

    pub(crate) fn succeeded(
        &mut self,
        id: TrackerId,
        now: Duration,
        interval_seconds: u32,
        peer_count: u32,
        seeders: u32,
        leechers: u32,
    ) -> TrackerSuccess {
        self.succeeded_outcome(
            id,
            now,
            TrackerAcceptedOutcome {
                requested_interval: Duration::from_secs(u64::from(interval_seconds)),
                peer_count,
                seeders: Some(seeders),
                leechers: Some(leechers),
                connection_family: None,
            },
        )
    }

    pub(crate) fn succeeded_outcome(
        &mut self,
        id: TrackerId,
        now: Duration,
        outcome: TrackerAcceptedOutcome,
    ) -> TrackerSuccess {
        let interval = outcome
            .requested_interval
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
            match record.inflight_event.take() {
                Some(AnnounceEvent::Started) => {
                    record.start_acknowledged = true;
                    if self.stopping {
                        record.pending_event = Some(TrackerPriorityEvent::Stopped);
                    }
                }
                Some(event @ (AnnounceEvent::Completed | AnnounceEvent::None)) => {
                    if record
                        .pending_event
                        .is_some_and(|pending| pending.wire_event() == event)
                    {
                        record.pending_event = None;
                    }
                }
                Some(AnnounceEvent::Stopped) => {
                    record.pending_event = None;
                    record.start_acknowledged = false;
                    record.stopped = true;
                }
                None => {}
            }
            record.last_success = Some(now);
            record.last_connection_family = outcome.connection_family;
            record.interval = Some(interval);
            record.last_peer_count = Some(outcome.peer_count);
            record.seeders = outcome.seeders;
            record.leechers = outcome.leechers;
            record.last_error = None;
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

    pub(crate) fn snapshot(&self, now: Duration, active: bool) -> TrackerRuntimeSnapshot {
        TrackerRuntimeSnapshot {
            captured_at: now,
            active,
            records: self
                .records
                .iter()
                .map(|record| record.snapshot(now, active, self.https_authentication))
                .collect(),
        }
    }

    fn record_mut(&mut self, id: TrackerId) -> &mut TrackerRecord {
        self.records
            .iter_mut()
            .find(|record| record.id == id)
            .expect("selected tracker record remains installed")
    }

    fn reset_round(&mut self) {
        self.attempted.clear();
        self.round_not_before = Duration::ZERO;
        self.round_tracker = None;
        self.round_wait_kind = None;
    }
}

impl TrackerRecord {
    fn snapshot(
        &self,
        now: Duration,
        active: bool,
        current_https_authentication: TrackerHttpsAuthentication,
    ) -> TrackerRuntimeRecordSnapshot {
        let (status, next_action, next_action_in) = if !active {
            (TrackerRuntimeStatus::Inactive, None, None)
        } else if self.updating {
            (TrackerRuntimeStatus::Announcing, None, None)
        } else if self.disabled {
            (TrackerRuntimeStatus::Disabled, None, None)
        } else if self.next_announce > now {
            let (status, action) = if self.failures != 0 {
                (TrackerRuntimeStatus::RetryWait, TrackerNextAction::Retry)
            } else {
                (
                    TrackerRuntimeStatus::ReannounceWait,
                    TrackerNextAction::Reannounce,
                )
            };
            (
                status,
                Some(action),
                Some(self.next_announce.saturating_sub(now)),
            )
        } else {
            (
                TrackerRuntimeStatus::Idle,
                Some(TrackerNextAction::Announce),
                Some(Duration::ZERO),
            )
        };
        let announce_event = (active && self.updating).then_some(match self.inflight_event {
            Some(AnnounceEvent::Started) => TrackerAnnounceEvent::Started,
            Some(AnnounceEvent::Completed) => TrackerAnnounceEvent::Completed,
            Some(AnnounceEvent::Stopped) => TrackerAnnounceEvent::Stopped,
            Some(AnnounceEvent::None) | None => TrackerAnnounceEvent::Update,
        });
        let url = self.display_url.clone();
        TrackerRuntimeRecordSnapshot {
            tracker_id: format!("{:06}:{:06}", self.tier, self.position),
            url,
            tier: self.tier,
            source: self.source,
            transport: self.endpoint.transport(),
            https_authentication: (self.endpoint.transport() == TrackerTransport::Https).then_some(
                self.operation_https_authentication
                    .unwrap_or(current_https_authentication),
            ),
            status,
            announce_event,
            total_attempts: self.total_attempts,
            consecutive_failures: self.failures,
            last_connection_family: self.last_connection_family,
            last_peer_count: self.last_peer_count,
            seeders: self.seeders,
            leechers: self.leechers,
            interval: self.interval,
            next_action,
            next_action_in,
            last_success_age: self.last_success.map(|instant| now.saturating_sub(instant)),
            last_failure_age: self.last_failure.map(|instant| now.saturating_sub(instant)),
            last_error: self.last_error.clone(),
        }
    }
}

fn bounded_tracker_error(detail: &str) -> String {
    let mut bounded = String::with_capacity(detail.len().min(MAX_TRACKER_ERROR_LENGTH));
    for character in detail.chars() {
        if bounded.len() + character.len_utf8() > MAX_TRACKER_ERROR_LENGTH {
            break;
        }
        bounded.push(character);
    }
    bounded
}

#[cfg(test)]
fn tracker_label(tracker: &UdpTrackerUrl) -> String {
    if tracker.host.contains(':') {
        format!("udp://[{}]:{}", tracker.host, tracker.port)
    } else {
        format!("udp://{}:{}", tracker.host, tracker.port)
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
        MAX_TRACKER_ERROR_LENGTH, TRACKER_ANNOUNCE_MAX, TRACKER_ANNOUNCE_MIN, TRACKER_RETRY_MAX,
        TrackerAcceptedOutcome, TrackerAction, TrackerAnnounceEvent, TrackerConfig,
        TrackerConnectionFamily, TrackerEndpoint, TrackerHttpsAuthentication, TrackerNextAction,
        TrackerRuntimeStatus, TrackerSchedule, TrackerSource, TrackerWaitKind,
        tracker_failure_delay,
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
    fn configured_schedule_preserves_large_tiers_without_u8_truncation() {
        let configs = (0_u32..300)
            .map(|position| {
                let endpoint = tracker(
                    &format!("tracker-{position}.example"),
                    u16::try_from(1_000 + position).expect("fixture port"),
                );
                TrackerConfig {
                    url: format!("udp://tracker-{position}.example:{}", 1_000 + position),
                    endpoint: TrackerEndpoint::Udp(endpoint),
                    tier: position / 100,
                    position: position % 100,
                    source: TrackerSource::Metainfo,
                }
            })
            .collect();
        let schedule = TrackerSchedule::from_configs(configs);
        let snapshot = schedule.snapshot(Duration::ZERO, false);

        assert_eq!(snapshot.records.len(), 300);
        assert_eq!(snapshot.records[299].tier, 2);
        assert_eq!(snapshot.records[299].source, TrackerSource::Metainfo);
        assert_eq!(snapshot.records[299].tracker_id, "000002:000099");
    }

    #[test]
    fn mixed_transport_schedule_preserves_endpoint_and_projection() {
        let configs = vec![
            TrackerConfig {
                url: "udp://tracker.example:6969".to_owned(),
                endpoint: TrackerEndpoint::Udp(tracker("tracker.example", 6969)),
                tier: 0,
                position: 0,
                source: TrackerSource::Metainfo,
            },
            TrackerConfig {
                url: "http://tracker.example/announce?passkey=abc".to_owned(),
                endpoint: TrackerEndpoint::from_http_url(
                    "http://tracker.example/announce?passkey=abc",
                )
                .expect("HTTP tracker URL"),
                tier: 0,
                position: 1,
                source: TrackerSource::Metainfo,
            },
            TrackerConfig {
                url: "https://tracker.example/announce".to_owned(),
                endpoint: TrackerEndpoint::from_http_url("https://tracker.example/announce")
                    .expect("HTTPS tracker URL"),
                tier: 1,
                position: 0,
                source: TrackerSource::Magnet,
            },
        ];
        let mut schedule = TrackerSchedule::from_configs(configs);
        let snapshot = schedule.snapshot(Duration::ZERO, false);
        assert_eq!(
            snapshot
                .records
                .iter()
                .map(|record| record.transport)
                .collect::<Vec<_>>(),
            [
                super::TrackerTransport::Udp,
                super::TrackerTransport::Http,
                super::TrackerTransport::Https,
            ]
        );

        let first = announce(&mut schedule, Duration::ZERO);
        schedule.failed(first, Duration::ZERO, "UDP unavailable");
        let TrackerAction::Announce { url, endpoint, .. } = schedule.next_action(Duration::ZERO)
        else {
            panic!("HTTP fallback should be eligible");
        };
        assert_eq!(url, "http://tracker.example/announce?passkey=abc");
        assert_eq!(endpoint.transport(), super::TrackerTransport::Http);
    }

    #[test]
    fn https_row_retains_operation_policy_across_live_policy_change() {
        let url = "https://tracker.example/announce";
        let mut schedule = TrackerSchedule::from_configs(vec![TrackerConfig {
            url: url.to_owned(),
            endpoint: TrackerEndpoint::from_http_url(url).expect("HTTPS tracker URL"),
            tier: 0,
            position: 0,
            source: TrackerSource::Metainfo,
        }]);
        schedule.set_https_authentication(TrackerHttpsAuthentication::Disabled);
        let id = announce(&mut schedule, Duration::ZERO);

        schedule.set_https_authentication(TrackerHttpsAuthentication::SystemTrust);
        assert_eq!(
            schedule.snapshot(Duration::ZERO, true).records[0].https_authentication,
            Some(TrackerHttpsAuthentication::Disabled)
        );
        schedule.failed(id, Duration::from_secs(1), "scripted TLS failure");
        assert_eq!(
            schedule.snapshot(Duration::from_secs(1), true).records[0].https_authentication,
            Some(TrackerHttpsAuthentication::Disabled)
        );
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
            let failure = schedule.failed(id, now, "timeout");
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
            endpoint,
            event,
            fallback,
            ..
        } = schedule.next_action(Duration::ZERO)
        else {
            panic!("first tracker should be eligible");
        };
        assert_eq!(endpoint, TrackerEndpoint::Udp(first));
        assert_eq!(event, AnnounceEvent::Started);
        assert!(!fallback);
        schedule.failed(first_id, Duration::ZERO, "first unavailable");

        let TrackerAction::Announce {
            id: second_id,
            endpoint,
            event,
            fallback,
            ..
        } = schedule.next_action(Duration::ZERO)
        else {
            panic!("second tracker should be the fallback");
        };
        assert_eq!(endpoint, TrackerEndpoint::Udp(second.clone()));
        assert_eq!(event, AnnounceEvent::Started);
        assert!(fallback);
        let success = schedule.succeeded(second_id, Duration::ZERO, 1, 12, 7, 5);
        assert_eq!(success.interval, TRACKER_ANNOUNCE_MIN);

        assert_eq!(
            schedule.next_action(TRACKER_ANNOUNCE_MIN - Duration::from_secs(1)),
            TrackerAction::Wait {
                delay: Duration::from_secs(1),
                url: "udp://second.example:81".to_owned(),
                endpoint: TrackerEndpoint::Udp(second.clone()),
                kind: TrackerWaitKind::Reannounce,
            }
        );
        assert!(matches!(
            schedule.next_action(TRACKER_ANNOUNCE_MIN),
            TrackerAction::Announce {
                endpoint,
                event: AnnounceEvent::None,
                fallback: false,
                ..
            } if endpoint == TrackerEndpoint::Udp(second)
        ));
    }

    #[test]
    fn snapshot_retains_only_the_last_successful_connection_family() {
        let mut schedule = TrackerSchedule::new(vec![tracker("tracker.example", 80)]);
        let id = announce(&mut schedule, Duration::ZERO);
        schedule.succeeded_outcome(
            id,
            Duration::from_secs(1),
            TrackerAcceptedOutcome {
                requested_interval: Duration::from_secs(600),
                peer_count: 0,
                seeders: None,
                leechers: None,
                connection_family: Some(TrackerConnectionFamily::Ipv6),
            },
        );
        assert_eq!(
            schedule.snapshot(Duration::from_secs(2), true).records[0].last_connection_family,
            Some(TrackerConnectionFamily::Ipv6)
        );

        let id = announce(&mut schedule, Duration::from_secs(601));
        schedule.failed(id, Duration::from_secs(601), "timeout");
        assert_eq!(
            schedule.snapshot(Duration::from_secs(602), true).records[0].last_connection_family,
            Some(TrackerConnectionFamily::Ipv6)
        );
    }

    #[test]
    fn explicit_retry_and_generation_scoped_disable_preserve_tier_fallback() {
        let mut schedule = TrackerSchedule::new(vec![
            tracker("first.example", 80),
            tracker("second.example", 81),
        ]);
        let first = announce(&mut schedule, Duration::ZERO);
        let failure = schedule.failed_with_retry(
            first,
            Duration::from_secs(2),
            "retry later",
            Duration::from_secs(600),
        );
        assert_eq!(failure.retry_in, Duration::from_secs(600));

        let second = announce(&mut schedule, Duration::from_secs(2));
        schedule.disable(second, Duration::from_secs(3), "never retry");
        let disabled = schedule.snapshot(Duration::from_secs(3), true);
        let record = disabled
            .records
            .iter()
            .find(|record| record.tracker_id == "000000:000001")
            .expect("disabled tracker row");
        assert_eq!(record.status, TrackerRuntimeStatus::Disabled);
        assert_eq!(record.next_action, None);
        assert_eq!(record.last_error.as_deref(), Some("never retry"));

        assert_eq!(
            schedule.next_action(Duration::from_secs(3)),
            TrackerAction::Wait {
                delay: Duration::from_secs(599),
                url: "udp://first.example:80".to_owned(),
                endpoint: TrackerEndpoint::Udp(tracker("first.example", 80)),
                kind: TrackerWaitKind::FailureRetry,
            }
        );

        let restarted = TrackerSchedule::new(vec![
            tracker("first.example", 80),
            tracker("second.example", 81),
        ]);
        assert!(
            restarted
                .snapshot(Duration::ZERO, true)
                .records
                .iter()
                .all(|record| record.status == TrackerRuntimeStatus::Idle)
        );
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

        schedule.failed(first, Duration::ZERO, "first timeout");
        assert_eq!(schedule.next_action(Duration::ZERO), TrackerAction::Pending);
        schedule.failed(second, Duration::ZERO, "second timeout");
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
            schedule.succeeded(id, Duration::ZERO, 0, 0, 0, 0).interval,
            TRACKER_ANNOUNCE_MIN
        );

        let id = announce(&mut schedule, TRACKER_ANNOUNCE_MIN);
        assert_eq!(
            schedule
                .succeeded(id, TRACKER_ANNOUNCE_MIN, u32::MAX, 0, 0, 0)
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
        schedule.succeeded(id, Duration::ZERO, 600, 0, 0, 0);

        assert_eq!(
            schedule.next_action(Duration::from_secs(10)),
            TrackerAction::Wait {
                delay: Duration::from_secs(590),
                url: "udp://first.example:80".to_owned(),
                endpoint: TrackerEndpoint::Udp(first),
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
            schedule.failed(slow_id, Duration::ZERO, "slow tracker");
        }
        let fast_id = announce(&mut schedule, Duration::ZERO);
        schedule.failed(fast_id, Duration::ZERO, "fast tracker");

        assert_eq!(
            schedule.next_action(Duration::ZERO),
            TrackerAction::Wait {
                delay: Duration::from_secs(17),
                url: "udp://fast.example:81".to_owned(),
                endpoint: TrackerEndpoint::Udp(fast),
                kind: TrackerWaitKind::FailureRetry,
            }
        );
    }

    #[test]
    fn snapshots_retain_bounded_failure_and_accepted_response_state() {
        let first = tracker("first.example", 80);
        let second = tracker("second.example", 81);
        let mut schedule = TrackerSchedule::new(vec![first, second]);

        let initial = schedule.snapshot(Duration::ZERO, false);
        assert_eq!(initial.records[0].status, TrackerRuntimeStatus::Inactive);
        assert_eq!(initial.records[0].next_action, None);
        assert_eq!(initial.records[0].interval, None);

        let first_id = announce(&mut schedule, Duration::from_secs(1));
        let announcing = schedule.snapshot(Duration::from_secs(1), true);
        assert_eq!(
            announcing.records[0].status,
            TrackerRuntimeStatus::Announcing
        );
        assert_eq!(
            announcing.records[0].announce_event,
            Some(TrackerAnnounceEvent::Started)
        );

        let long_error = "é".repeat(MAX_TRACKER_ERROR_LENGTH);
        schedule.failed(first_id, Duration::from_secs(2), &long_error);
        let failed = schedule.snapshot(Duration::from_secs(3), true);
        assert_eq!(failed.records[0].status, TrackerRuntimeStatus::RetryWait);
        assert_eq!(
            failed.records[0].next_action,
            Some(TrackerNextAction::Retry)
        );
        assert_eq!(
            failed.records[0].next_action_in,
            Some(Duration::from_secs(16))
        );
        assert_eq!(
            failed.records[0].last_failure_age,
            Some(Duration::from_secs(1))
        );
        assert_eq!(
            failed.records[0].last_error.as_ref().map(String::len),
            Some(MAX_TRACKER_ERROR_LENGTH)
        );

        let second_id = announce(&mut schedule, Duration::from_secs(3));
        schedule.succeeded(second_id, Duration::from_secs(4), 600, 23, 11, 12);
        let succeeded = schedule.snapshot(Duration::from_secs(9), true);
        let record = &succeeded.records[0];
        assert_eq!(record.url, "udp://second.example:81");
        assert_eq!(record.status, TrackerRuntimeStatus::ReannounceWait);
        assert_eq!(record.next_action, Some(TrackerNextAction::Reannounce));
        assert_eq!(record.next_action_in, Some(Duration::from_secs(595)));
        assert_eq!(record.last_success_age, Some(Duration::from_secs(5)));
        assert_eq!(record.last_peer_count, Some(23));
        assert_eq!(record.seeders, Some(11));
        assert_eq!(record.leechers, Some(12));
        assert_eq!(record.interval, Some(Duration::from_secs(600)));
        assert_eq!(record.last_error, None);

        let update_id = announce(&mut schedule, Duration::from_secs(604));
        assert_eq!(update_id, second_id);
        let updating = schedule.snapshot(Duration::from_secs(604), true);
        assert_eq!(
            updating.records[0].announce_event,
            Some(TrackerAnnounceEvent::Update)
        );
    }

    #[test]
    fn terminal_snapshot_clears_inflight_state_without_losing_history() {
        let mut schedule = TrackerSchedule::new(vec![tracker("tracker.example", 80)]);
        let id = announce(&mut schedule, Duration::ZERO);
        schedule.succeeded(id, Duration::from_secs(1), 900, 4, 3, 1);
        let _ = announce(&mut schedule, Duration::from_secs(901));

        let terminal = schedule.snapshot(Duration::from_secs(902), false);
        assert!(!terminal.active);
        assert_eq!(terminal.records[0].status, TrackerRuntimeStatus::Inactive);
        assert_eq!(terminal.records[0].announce_event, None);
        assert_eq!(terminal.records[0].next_action, None);
        assert_eq!(terminal.records[0].last_peer_count, Some(4));
    }

    #[test]
    fn lifecycle_priorities_order_completed_correction_and_stopped() {
        let mut schedule = TrackerSchedule::new(vec![tracker("tracker.example", 80)]);
        let started = announce(&mut schedule, Duration::ZERO);
        schedule.succeeded(started, Duration::from_secs(1), 900, 0, 0, 0);

        schedule.request_completed();
        let TrackerAction::Announce {
            id,
            event: AnnounceEvent::Completed,
            ..
        } = schedule.next_action(Duration::from_secs(2))
        else {
            panic!("completion transition must preempt the interval");
        };
        schedule.succeeded(id, Duration::from_secs(3), 900, 0, 0, 0);

        schedule.request_update();
        let TrackerAction::Announce {
            id,
            event: AnnounceEvent::None,
            ..
        } = schedule.next_action(Duration::from_secs(4))
        else {
            panic!("endpoint correction must preempt the interval");
        };
        schedule.succeeded(id, Duration::from_secs(5), 900, 0, 0, 0);

        schedule.request_stop();
        let TrackerAction::Announce {
            id,
            event: AnnounceEvent::Stopped,
            ..
        } = schedule.next_action(Duration::from_secs(6))
        else {
            panic!("stopped must supersede periodic work");
        };
        schedule.succeeded(id, Duration::from_secs(7), 900, 0, 0, 0);
        assert_eq!(
            schedule.next_action(Duration::from_secs(8)),
            TrackerAction::Exhausted
        );
    }

    #[test]
    fn correction_requested_during_started_is_not_lost() {
        let mut schedule = TrackerSchedule::new(vec![tracker("tracker.example", 80)]);
        let started = announce(&mut schedule, Duration::ZERO);
        schedule.request_update();
        schedule.succeeded(started, Duration::from_secs(1), 900, 0, 0, 0);

        assert!(matches!(
            schedule.next_action(Duration::from_secs(2)),
            TrackerAction::Announce {
                event: AnnounceEvent::None,
                ..
            }
        ));
    }

    #[test]
    fn imported_complete_start_does_not_fabricate_completed() {
        let mut schedule = TrackerSchedule::new(vec![tracker("tracker.example", 80)]);
        schedule.request_completed();
        assert!(matches!(
            schedule.next_action(Duration::ZERO),
            TrackerAction::Announce {
                event: AnnounceEvent::Started,
                ..
            }
        ));
    }

    #[test]
    fn stopped_is_only_due_after_a_successful_start() {
        let mut schedule = TrackerSchedule::new(vec![tracker("tracker.example", 80)]);
        schedule.request_stop();
        assert_eq!(
            schedule.next_action(Duration::ZERO),
            TrackerAction::Exhausted
        );
    }
}
