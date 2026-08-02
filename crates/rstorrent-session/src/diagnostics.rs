use std::collections::VecDeque;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::{Config, TS};

pub const MAX_DIAGNOSTIC_EVENTS: usize = 2_048;
pub const MAX_DIAGNOSTIC_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_DIAGNOSTIC_RECORD_BYTES: usize = 4 * 1024;
pub const MAX_DIAGNOSTIC_PATCH_EVENTS: usize = 128;
pub const MAX_DIAGNOSTIC_PATCH_BYTES: usize = 128 * 1024;
pub const MAX_DIAGNOSTIC_SUBJECTS: usize = 4;
pub const MAX_DIAGNOSTIC_FIELDS: usize = 8;
pub const MAX_DIAGNOSTIC_MESSAGE_CHARS: usize = 320;
pub const MAX_DIAGNOSTIC_KEY_BYTES: usize = 48;
pub const MAX_DIAGNOSTIC_VALUE_CHARS: usize = 240;
pub const MAX_DIAGNOSTIC_CATEGORY_BYTES: usize = 64;
pub const MAX_DIAGNOSTIC_CATEGORY_SEGMENTS: usize = 4;

pub mod category {
    pub const LIFECYCLE_SESSION: &str = "lifecycle.session";
    pub const LIFECYCLE_TORRENT: &str = "lifecycle.torrent";
    pub const DISCOVERY_PEER: &str = "discovery.peer";
    pub const DISCOVERY_DHT: &str = "discovery.dht";
    pub const TRACKER_ANNOUNCE: &str = "tracker.announce";
    pub const PEER_CONNECTION: &str = "peer.connection";
    pub const PEER_PROTOCOL: &str = "peer.protocol";
    pub const METADATA_EXCHANGE: &str = "metadata.exchange";
    pub const SCHEDULER_REQUEST: &str = "scheduler.request";
    pub const PIECE_BLOCK: &str = "piece.block";
    pub const STORAGE_IO: &str = "storage.io";
    pub const INTEGRITY_HASH: &str = "integrity.hash";
    pub const PLATFORM_ADAPTER: &str = "platform.adapter";
    pub const PERFORMANCE_BACKPRESSURE: &str = "performance.backpressure";
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(transparent)]
pub struct DiagnosticCategory {
    pub value: String,
}

impl TS for DiagnosticCategory {
    type WithoutGenerics = Self;
    type OptionInnerType = Self;

    fn name(_: &Config) -> String {
        "DiagnosticCategory".to_owned()
    }

    fn decl(_: &Config) -> String {
        "type DiagnosticCategory = string;".to_owned()
    }

    fn decl_concrete(config: &Config) -> String {
        Self::decl(config)
    }

    fn inline(_: &Config) -> String {
        "string".to_owned()
    }
}

impl DiagnosticCategory {
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        valid_category(&value).then_some(Self { value })
    }

    pub fn from_static(value: &'static str) -> Self {
        debug_assert!(valid_category(value));
        Self {
            value: value.to_owned(),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.value
    }

    pub fn matches_prefix(&self, prefix: &Self) -> bool {
        self == prefix
            || self
                .value
                .strip_prefix(&prefix.value)
                .is_some_and(|suffix| suffix.starts_with('.'))
    }
}

#[derive(
    Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize, JsonSchema, TS,
)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Trace,
    Debug,
    Info,
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticProfile {
    Normal,
    Detailed,
    Trace,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct DiagnosticFilter {
    pub profile: DiagnosticProfile,
    pub minimum_severity: DiagnosticSeverity,
    pub categories: Vec<DiagnosticCategory>,
}

impl Default for DiagnosticFilter {
    fn default() -> Self {
        Self {
            profile: DiagnosticProfile::Normal,
            minimum_severity: DiagnosticSeverity::Info,
            categories: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DiagnosticSubject {
    PeerConnection {
        connection_id: String,
    },
    Tracker {
        tracker_id: String,
    },
    Piece {
        piece_index: u32,
        attempt: Option<u32>,
    },
    File {
        file_index: u32,
    },
    Task {
        kind: String,
        generation: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DiagnosticValue {
    Text { value: String },
    Boolean { value: bool },
    Count { value: String },
    Bytes { value: String },
    DurationMillis { value: String },
    Endpoint { value: String },
    ErrorCode { value: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct DiagnosticField {
    pub key: String,
    pub value: DiagnosticValue,
}

impl DiagnosticField {
    pub fn text(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: DiagnosticValue::Text {
                value: value.into(),
            },
        }
    }

    pub fn count(key: impl Into<String>, value: u64) -> Self {
        Self {
            key: key.into(),
            value: DiagnosticValue::Count {
                value: value.to_string(),
            },
        }
    }

    pub fn bytes(key: impl Into<String>, value: u64) -> Self {
        Self {
            key: key.into(),
            value: DiagnosticValue::Bytes {
                value: value.to_string(),
            },
        }
    }

    pub fn duration_millis(key: impl Into<String>, value: u64) -> Self {
        Self {
            key: key.into(),
            value: DiagnosticValue::DurationMillis {
                value: value.to_string(),
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct DiagnosticEvent {
    pub sequence: String,
    pub timestamp_millis: String,
    pub severity: DiagnosticSeverity,
    pub category: DiagnosticCategory,
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub torrent_id: Option<String>,
    pub message: String,
    pub subjects: Vec<DiagnosticSubject>,
    pub fields: Vec<DiagnosticField>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct DiagnosticRetention {
    pub source_evicted_count: String,
    pub retained_from_sequence: String,
}

#[derive(Clone, Debug)]
pub(crate) struct DiagnosticDraft {
    pub severity: DiagnosticSeverity,
    pub category: DiagnosticCategory,
    pub code: String,
    pub torrent_id: Option<String>,
    pub message: String,
    pub subjects: Vec<DiagnosticSubject>,
    pub fields: Vec<DiagnosticField>,
}

#[derive(Clone, Debug)]
struct StoredDiagnostic {
    event: DiagnosticEvent,
    encoded_bytes: usize,
}

#[derive(Debug)]
pub(crate) struct DiagnosticStore {
    events: VecDeque<StoredDiagnostic>,
    encoded_bytes: usize,
    evicted_count: u64,
    next_sequence: u64,
}

impl Default for DiagnosticStore {
    fn default() -> Self {
        Self {
            events: VecDeque::new(),
            encoded_bytes: 0,
            evicted_count: 0,
            next_sequence: 1,
        }
    }
}

impl DiagnosticStore {
    pub fn record(&mut self, draft: DiagnosticDraft, timestamp_millis: u128) -> DiagnosticEvent {
        let mut event = DiagnosticEvent {
            sequence: self.next_sequence.to_string(),
            timestamp_millis: timestamp_millis.to_string(),
            severity: draft.severity,
            category: draft.category,
            code: sanitize_ascii_identifier(&draft.code, MAX_DIAGNOSTIC_KEY_BYTES),
            torrent_id: draft
                .torrent_id
                .map(|value| sanitize_text(&value, MAX_DIAGNOSTIC_VALUE_CHARS)),
            message: sanitize_text(&draft.message, MAX_DIAGNOSTIC_MESSAGE_CHARS),
            subjects: draft
                .subjects
                .into_iter()
                .take(MAX_DIAGNOSTIC_SUBJECTS)
                .map(sanitize_subject)
                .collect(),
            fields: draft
                .fields
                .into_iter()
                .take(MAX_DIAGNOSTIC_FIELDS)
                .map(sanitize_field)
                .collect(),
        };
        self.next_sequence = self.next_sequence.saturating_add(1);
        let mut encoded_bytes = encoded_len(&event);
        while encoded_bytes > MAX_DIAGNOSTIC_RECORD_BYTES && !event.fields.is_empty() {
            event.fields.pop();
            encoded_bytes = encoded_len(&event);
        }
        while encoded_bytes > MAX_DIAGNOSTIC_RECORD_BYTES && !event.subjects.is_empty() {
            event.subjects.pop();
            encoded_bytes = encoded_len(&event);
        }
        if encoded_bytes > MAX_DIAGNOSTIC_RECORD_BYTES {
            event.message = sanitize_text(&event.message, 80);
            encoded_bytes = encoded_len(&event);
        }
        debug_assert!(encoded_bytes <= MAX_DIAGNOSTIC_RECORD_BYTES);
        self.events.push_back(StoredDiagnostic {
            event: event.clone(),
            encoded_bytes,
        });
        self.encoded_bytes = self.encoded_bytes.saturating_add(encoded_bytes);
        while self.events.len() > MAX_DIAGNOSTIC_EVENTS || self.encoded_bytes > MAX_DIAGNOSTIC_BYTES
        {
            let Some(evicted) = self.events.pop_front() else {
                break;
            };
            self.encoded_bytes = self.encoded_bytes.saturating_sub(evicted.encoded_bytes);
            self.evicted_count = self.evicted_count.saturating_add(1);
        }
        event
    }

    pub fn matching(
        &self,
        filter: &DiagnosticFilter,
        torrent_id: Option<&str>,
    ) -> Vec<DiagnosticEvent> {
        self.events
            .iter()
            .filter(|stored| diagnostic_matches(filter, torrent_id, &stored.event))
            .map(|stored| stored.event.clone())
            .collect()
    }

    pub fn retention(&self) -> DiagnosticRetention {
        DiagnosticRetention {
            source_evicted_count: self.evicted_count.to_string(),
            retained_from_sequence: self.events.front().map_or_else(
                || self.next_sequence.to_string(),
                |stored| stored.event.sequence.clone(),
            ),
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    #[cfg(test)]
    pub fn encoded_bytes(&self) -> usize {
        self.encoded_bytes
    }
}

pub(crate) fn diagnostic_matches(
    filter: &DiagnosticFilter,
    torrent_id: Option<&str>,
    event: &DiagnosticEvent,
) -> bool {
    if event.severity < filter.minimum_severity {
        return false;
    }
    if let Some(torrent_id) = torrent_id
        && let Some(event_torrent_id) = event.torrent_id.as_deref()
        && event_torrent_id != torrent_id
    {
        return false;
    }
    if !filter.categories.is_empty()
        && !filter
            .categories
            .iter()
            .any(|prefix| event.category.matches_prefix(prefix))
    {
        return filter.profile == DiagnosticProfile::Normal
            && event.severity >= DiagnosticSeverity::Warning;
    }
    profile_allows(filter.profile, event.severity, &event.category)
}

pub(crate) fn interest_matches(
    filter: &DiagnosticFilter,
    selected_torrent: Option<&str>,
    severity: DiagnosticSeverity,
    category: &DiagnosticCategory,
    event_torrent: Option<&str>,
) -> bool {
    if let Some(selected_torrent) = selected_torrent
        && event_torrent != Some(selected_torrent)
    {
        return false;
    }
    let event = DiagnosticEvent {
        sequence: String::new(),
        timestamp_millis: String::new(),
        severity,
        category: category.clone(),
        code: String::new(),
        torrent_id: event_torrent.map(ToOwned::to_owned),
        message: String::new(),
        subjects: Vec::new(),
        fields: Vec::new(),
    };
    diagnostic_matches(filter, None, &event)
}

pub(crate) fn valid_filter(filter: &DiagnosticFilter) -> bool {
    filter.categories.len() <= 16
        && filter
            .categories
            .iter()
            .all(|category| valid_category(category.as_str()))
        && filter
            .categories
            .iter()
            .enumerate()
            .all(|(index, category)| !filter.categories[..index].contains(category))
}

fn profile_allows(
    profile: DiagnosticProfile,
    severity: DiagnosticSeverity,
    category: &DiagnosticCategory,
) -> bool {
    if severity >= DiagnosticSeverity::Warning {
        return true;
    }
    match profile {
        DiagnosticProfile::Trace => true,
        DiagnosticProfile::Detailed => !matches_prefix(category, category::PIECE_BLOCK),
        DiagnosticProfile::Normal => [
            "lifecycle",
            "discovery",
            "tracker",
            "peer.connection",
            "storage",
            "integrity",
            "platform",
        ]
        .iter()
        .any(|prefix| matches_prefix(category, prefix)),
    }
}

fn matches_prefix(category: &DiagnosticCategory, prefix: &str) -> bool {
    category.as_str() == prefix
        || category
            .as_str()
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('.'))
}

pub fn valid_category(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_DIAGNOSTIC_CATEGORY_BYTES
        && value.split('.').count() <= MAX_DIAGNOSTIC_CATEGORY_SEGMENTS
        && value.split('.').all(|segment| {
            !segment.is_empty()
                && segment.len() <= MAX_DIAGNOSTIC_KEY_BYTES
                && segment.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'_' | b'-')
                })
        })
}

fn sanitize_subject(subject: DiagnosticSubject) -> DiagnosticSubject {
    match subject {
        DiagnosticSubject::PeerConnection { connection_id } => DiagnosticSubject::PeerConnection {
            connection_id: sanitize_text(&connection_id, MAX_DIAGNOSTIC_VALUE_CHARS),
        },
        DiagnosticSubject::Tracker { tracker_id } => DiagnosticSubject::Tracker {
            tracker_id: sanitize_text(&tracker_id, MAX_DIAGNOSTIC_VALUE_CHARS),
        },
        DiagnosticSubject::Piece {
            piece_index,
            attempt,
        } => DiagnosticSubject::Piece {
            piece_index,
            attempt,
        },
        DiagnosticSubject::File { file_index } => DiagnosticSubject::File { file_index },
        DiagnosticSubject::Task { kind, generation } => DiagnosticSubject::Task {
            kind: sanitize_ascii_identifier(&kind, MAX_DIAGNOSTIC_KEY_BYTES),
            generation: sanitize_text(&generation, MAX_DIAGNOSTIC_VALUE_CHARS),
        },
    }
}

fn sanitize_field(field: DiagnosticField) -> DiagnosticField {
    DiagnosticField {
        key: sanitize_ascii_identifier(&field.key, MAX_DIAGNOSTIC_KEY_BYTES),
        value: match field.value {
            DiagnosticValue::Text { value } => DiagnosticValue::Text {
                value: sanitize_text(&value, MAX_DIAGNOSTIC_VALUE_CHARS),
            },
            DiagnosticValue::Boolean { value } => DiagnosticValue::Boolean { value },
            DiagnosticValue::Count { value } => DiagnosticValue::Count {
                value: sanitize_decimal(&value),
            },
            DiagnosticValue::Bytes { value } => DiagnosticValue::Bytes {
                value: sanitize_decimal(&value),
            },
            DiagnosticValue::DurationMillis { value } => DiagnosticValue::DurationMillis {
                value: sanitize_decimal(&value),
            },
            DiagnosticValue::Endpoint { value } => DiagnosticValue::Endpoint {
                value: sanitize_text(&value, MAX_DIAGNOSTIC_VALUE_CHARS),
            },
            DiagnosticValue::ErrorCode { value } => DiagnosticValue::ErrorCode {
                value: sanitize_ascii_identifier(&value, MAX_DIAGNOSTIC_KEY_BYTES),
            },
        },
    }
}

fn sanitize_decimal(value: &str) -> String {
    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        value.to_owned()
    } else {
        "0".to_owned()
    }
}

fn sanitize_ascii_identifier(value: &str, maximum_bytes: usize) -> String {
    value
        .bytes()
        .filter(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-' | b'.')
        })
        .take(maximum_bytes)
        .map(char::from)
        .collect()
}

fn sanitize_text(value: &str, maximum_chars: usize) -> String {
    value
        .chars()
        .filter(|character| {
            !character.is_control()
                && !matches!(
                    *character,
                    '\u{061c}'
                        | '\u{200e}'
                        | '\u{200f}'
                        | '\u{202a}'..='\u{202e}'
                        | '\u{2066}'..='\u{2069}'
                )
        })
        .take(maximum_chars)
        .collect()
}

fn encoded_len(event: &DiagnosticEvent) -> usize {
    serde_json::to_vec(event).map_or(MAX_DIAGNOSTIC_RECORD_BYTES, |bytes| bytes.len())
}

pub(crate) fn patch_encoded_len(events: &[DiagnosticEvent]) -> usize {
    serde_json::to_vec(events).map_or(usize::MAX, |bytes| bytes.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(index: usize) -> DiagnosticDraft {
        DiagnosticDraft {
            severity: DiagnosticSeverity::Warning,
            category: DiagnosticCategory::from_static(category::TRACKER_ANNOUNCE),
            code: "tracker_timeout".to_owned(),
            torrent_id: Some("a".repeat(40)),
            message: format!("event {index}"),
            subjects: vec![DiagnosticSubject::Tracker {
                tracker_id: "udp://127.0.0.1:6881/announce".to_owned(),
            }],
            fields: vec![DiagnosticField::duration_millis("retry_in", 15_000)],
        }
    }

    #[test]
    fn categories_are_hierarchical_bounded_and_forward_compatible() {
        assert!(valid_category("peer.connection"));
        assert!(valid_category("dht.packet.outgoing"));
        assert!(!valid_category("Peer.Connection"));
        assert!(!valid_category("peer..connection"));
        assert!(!valid_category("a.b.c.d.e"));
        assert!(!valid_category(
            &"a".repeat(MAX_DIAGNOSTIC_CATEGORY_BYTES + 1)
        ));
        let category = DiagnosticCategory::new("peer.connection.retry").expect("category");
        let prefix = DiagnosticCategory::new("peer.connection").expect("prefix");
        assert!(category.matches_prefix(&prefix));
    }

    #[test]
    fn profiles_and_prefixes_filter_without_message_parsing() {
        let tracker = DiagnosticStore::default().record(draft(0), 1);
        let piece = DiagnosticEvent {
            category: DiagnosticCategory::from_static(category::PIECE_BLOCK),
            severity: DiagnosticSeverity::Debug,
            ..tracker.clone()
        };
        assert!(diagnostic_matches(
            &DiagnosticFilter::default(),
            None,
            &tracker
        ));
        assert!(!diagnostic_matches(
            &DiagnosticFilter::default(),
            None,
            &piece
        ));
        assert!(diagnostic_matches(
            &DiagnosticFilter {
                profile: DiagnosticProfile::Trace,
                minimum_severity: DiagnosticSeverity::Trace,
                categories: vec![DiagnosticCategory::from_static("piece")],
            },
            None,
            &piece,
        ));
    }

    #[test]
    fn selected_delivery_includes_global_but_interest_stays_torrent_scoped() {
        let filter = DiagnosticFilter {
            profile: DiagnosticProfile::Detailed,
            minimum_severity: DiagnosticSeverity::Debug,
            categories: Vec::new(),
        };
        let mut event = DiagnosticStore::default().record(draft(0), 1);
        event.severity = DiagnosticSeverity::Debug;
        event.category = DiagnosticCategory::from_static(category::SCHEDULER_REQUEST);
        event.torrent_id = None;
        assert!(diagnostic_matches(&filter, Some("selected"), &event));
        assert!(!interest_matches(
            &filter,
            Some("selected"),
            event.severity,
            &event.category,
            None,
        ));
        assert!(interest_matches(
            &filter,
            Some("selected"),
            event.severity,
            &event.category,
            Some("selected"),
        ));
        assert!(!interest_matches(
            &filter,
            Some("selected"),
            event.severity,
            &event.category,
            Some("other"),
        ));
    }

    #[test]
    fn store_bounds_history_by_count_and_reports_retained_boundary() {
        let mut store = DiagnosticStore::default();
        for index in 0..10_000 {
            store.record(draft(index), index as u128);
        }
        assert_eq!(store.len(), MAX_DIAGNOSTIC_EVENTS);
        assert!(store.encoded_bytes() <= MAX_DIAGNOSTIC_BYTES);
        assert_eq!(store.retention().source_evicted_count, "7952");
        assert_eq!(store.retention().retained_from_sequence, "7953");
    }

    #[test]
    fn hostile_fields_are_typed_sanitized_and_record_bounded() {
        let mut store = DiagnosticStore::default();
        let mut hostile = draft(0);
        hostile.message = format!("\u{202e}{}", "x".repeat(10_000));
        hostile.fields = (0..20)
            .map(|index| DiagnosticField::text(format!("key_{index}"), "y".repeat(1_000)))
            .collect();
        let event = store.record(hostile, 1);
        assert!(!event.message.contains('\u{202e}'));
        assert!(event.fields.len() <= MAX_DIAGNOSTIC_FIELDS);
        assert!(encoded_len(&event) <= MAX_DIAGNOSTIC_RECORD_BYTES);
        assert!(
            event
                .fields
                .iter()
                .all(|field| field.key.starts_with("key"))
        );
    }
}
