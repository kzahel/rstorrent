use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use rstorrent_engine::{
    TrackerAnnounceEvent, TrackerNextAction, TrackerRuntimeRecordSnapshot, TrackerRuntimeSnapshot,
    TrackerRuntimeStatus, TrackerSource, TrackerTransport,
};
use rstorrent_protocol::magnet::Magnet;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum TrackerCatalogState {
    Available,
    TorrentMissing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum TrackerTransportView {
    Udp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum TrackerSourceView {
    Magnet,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum TrackerStatusView {
    Inactive,
    Idle,
    Announcing,
    RetryWait,
    ReannounceWait,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum TrackerAnnounceEventView {
    Started,
    Update,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum TrackerNextActionView {
    Announce,
    Retry,
    Reannounce,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct TrackerView {
    pub tracker_id: String,
    pub url: String,
    pub transport: TrackerTransportView,
    pub source: TrackerSourceView,
    pub tier: u32,
    pub status: TrackerStatusView,
    pub announce_event: Option<TrackerAnnounceEventView>,
    pub total_attempts: u32,
    pub consecutive_failures: u32,
    pub last_peer_count: Option<u32>,
    pub seeders: Option<u32>,
    pub leechers: Option<u32>,
    pub interval_seconds: Option<u32>,
    pub next_action: Option<TrackerNextActionView>,
    pub next_action_in_millis: Option<String>,
    pub last_success_age_millis: Option<String>,
    pub last_failure_age_millis: Option<String>,
    pub last_error: Option<String>,
}

impl TrackerView {
    fn inactive(url: String) -> Self {
        Self {
            tracker_id: url.clone(),
            url,
            transport: TrackerTransportView::Udp,
            source: TrackerSourceView::Magnet,
            tier: 0,
            status: TrackerStatusView::Inactive,
            announce_event: None,
            total_attempts: 0,
            consecutive_failures: 0,
            last_peer_count: None,
            seeders: None,
            leechers: None,
            interval_seconds: None,
            next_action: None,
            next_action_in_millis: None,
            last_success_age_millis: None,
            last_failure_age_millis: None,
            last_error: None,
        }
    }
}

impl From<&TrackerRuntimeRecordSnapshot> for TrackerView {
    fn from(record: &TrackerRuntimeRecordSnapshot) -> Self {
        Self {
            tracker_id: record.tracker_id.clone(),
            url: record.url.clone(),
            transport: match record.transport {
                TrackerTransport::Udp => TrackerTransportView::Udp,
            },
            source: match record.source {
                TrackerSource::Magnet => TrackerSourceView::Magnet,
            },
            tier: u32::from(record.tier),
            status: match record.status {
                TrackerRuntimeStatus::Inactive => TrackerStatusView::Inactive,
                TrackerRuntimeStatus::Idle => TrackerStatusView::Idle,
                TrackerRuntimeStatus::Announcing => TrackerStatusView::Announcing,
                TrackerRuntimeStatus::RetryWait => TrackerStatusView::RetryWait,
                TrackerRuntimeStatus::ReannounceWait => TrackerStatusView::ReannounceWait,
            },
            announce_event: record.announce_event.map(|event| match event {
                TrackerAnnounceEvent::Started => TrackerAnnounceEventView::Started,
                TrackerAnnounceEvent::Update => TrackerAnnounceEventView::Update,
            }),
            total_attempts: record.total_attempts,
            consecutive_failures: u32::from(record.consecutive_failures),
            last_peer_count: record.last_peer_count,
            seeders: record.seeders,
            leechers: record.leechers,
            interval_seconds: record
                .interval
                .map(|interval| interval.as_secs().try_into().unwrap_or(u32::MAX)),
            next_action: record.next_action.map(|action| match action {
                TrackerNextAction::Announce => TrackerNextActionView::Announce,
                TrackerNextAction::Retry => TrackerNextActionView::Retry,
                TrackerNextAction::Reannounce => TrackerNextActionView::Reannounce,
            }),
            next_action_in_millis: record
                .next_action_in
                .map(|duration| duration.as_millis().to_string()),
            last_success_age_millis: record
                .last_success_age
                .map(|duration| duration.as_millis().to_string()),
            last_failure_age_millis: record
                .last_failure_age
                .map(|duration| duration.as_millis().to_string()),
            last_error: record.last_error.clone(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct TrackerViewModel {
    rows: BTreeMap<String, TrackerView>,
}

impl TrackerViewModel {
    pub(crate) fn from_magnet(value: &str) -> Self {
        let rows = Magnet::parse(value)
            .map(|magnet| {
                magnet
                    .udp_trackers
                    .into_iter()
                    .map(|tracker| {
                        let url = tracker_url(&tracker.host, tracker.port);
                        (url.clone(), TrackerView::inactive(url))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self { rows }
    }

    pub(crate) fn catalog_matches(&self, other: &Self) -> bool {
        self.rows.keys().eq(other.rows.keys())
    }

    pub(crate) fn apply_snapshot(&mut self, snapshot: &TrackerRuntimeSnapshot) {
        self.rows = snapshot
            .records
            .iter()
            .map(|record| {
                let view = TrackerView::from(record);
                (view.tracker_id.clone(), view)
            })
            .collect();
    }

    pub(crate) fn rows(&self) -> Vec<TrackerView> {
        self.rows.values().cloned().collect()
    }

    pub(crate) fn row_map(&self) -> &BTreeMap<String, TrackerView> {
        &self.rows
    }
}

fn tracker_url(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("udp://[{host}]:{port}")
    } else {
        format!("udp://{host}:{port}")
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rstorrent_engine::{
        TrackerNextAction, TrackerRuntimeRecordSnapshot, TrackerRuntimeSnapshot,
        TrackerRuntimeStatus, TrackerSource, TrackerTransport,
    };

    use super::{TrackerNextActionView, TrackerStatusView, TrackerViewModel};

    #[test]
    fn durable_magnet_builds_deduplicated_inactive_catalog() {
        let model = TrackerViewModel::from_magnet(
            "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213\
             &tr=udp%3A%2F%2Ftracker.example%3A6969\
             &tr=udp%3A%2F%2Ftracker.example%3A6969",
        );
        let rows = model.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].url, "udp://tracker.example:6969");
        assert_eq!(rows[0].status, TrackerStatusView::Inactive);
    }

    #[test]
    fn runtime_snapshot_replaces_catalog_and_maps_monotonic_values() {
        let mut model = TrackerViewModel::default();
        model.apply_snapshot(&TrackerRuntimeSnapshot {
            captured_at: Duration::from_secs(9),
            active: true,
            records: vec![TrackerRuntimeRecordSnapshot {
                tracker_id: "udp://tracker.example:6969".to_owned(),
                url: "udp://tracker.example:6969".to_owned(),
                tier: 0,
                source: TrackerSource::Magnet,
                transport: TrackerTransport::Udp,
                status: TrackerRuntimeStatus::RetryWait,
                announce_event: None,
                total_attempts: 2,
                consecutive_failures: 1,
                last_peer_count: Some(20),
                seeders: Some(12),
                leechers: Some(8),
                interval: Some(Duration::from_secs(600)),
                next_action: Some(TrackerNextAction::Retry),
                next_action_in: Some(Duration::from_millis(1250)),
                last_success_age: Some(Duration::from_secs(8)),
                last_failure_age: Some(Duration::from_millis(250)),
                last_error: Some("timeout".to_owned()),
            }],
        });
        let row = &model.rows()[0];
        assert_eq!(row.status, TrackerStatusView::RetryWait);
        assert_eq!(row.next_action, Some(TrackerNextActionView::Retry));
        assert_eq!(row.next_action_in_millis.as_deref(), Some("1250"));
        assert_eq!(row.last_success_age_millis.as_deref(), Some("8000"));
        assert_eq!(row.last_failure_age_millis.as_deref(), Some("250"));
        assert_eq!(row.interval_seconds, Some(600));
    }
}
