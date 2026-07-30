use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tokio::time::Instant;
use ts_rs::TS;

use crate::control::{ServiceSnapshot, StorageState, TorrentSnapshot, TorrentState};

pub const VIEW_CONTRACT_VERSION: u16 = 1;
pub const MIN_SUBSCRIPTION_QUEUE_BYTES: u32 = 4 * 1024;
pub const MAX_SUBSCRIPTION_QUEUE_BYTES: u32 = 4 * 1024 * 1024;
pub const MAX_SUBSCRIPTION_INTERVAL_MILLIS: u32 = 60_000;

static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ViewSelector {
    TorrentList,
    Torrent { torrent_id: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum ViewProjection {
    Summary,
    PieceActivity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct DeliveryPolicy {
    pub min_interval_millis: u32,
    pub max_queue_bytes: u32,
}

impl Default for DeliveryPolicy {
    fn default() -> Self {
        Self {
            min_interval_millis: 100,
            max_queue_bytes: 256 * 1024,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct SubscriptionSpec {
    pub selector: ViewSelector,
    pub projection: ViewProjection,
    pub delivery: DeliveryPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct IndexRange {
    pub start: u32,
    pub end_exclusive: u32,
}

impl IndexRange {
    pub fn new(start: u32, end_exclusive: u32) -> Option<Self> {
        (start < end_exclusive).then_some(Self {
            start,
            end_exclusive,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ActivePiece {
    pub piece_index: u32,
    pub piece_length: u32,
    pub requested: Vec<IndexRange>,
    pub received: Vec<IndexRange>,
    pub stored: Vec<IndexRange>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct TorrentView {
    pub torrent_id: String,
    pub state: TorrentState,
    pub storage_state: StorageState,
    pub metadata_available: bool,
    pub piece_count: u32,
    pub verified_piece_count: u32,
    pub requested_bytes: String,
    pub received_bytes: String,
    pub stored_bytes: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ViewSnapshot {
    TorrentList {
        torrents: Vec<TorrentView>,
    },
    Torrent {
        torrent: Option<TorrentView>,
    },
    PieceActivity {
        torrent_id: String,
        piece_count: u32,
        verified: Vec<IndexRange>,
        active: Option<ActivePiece>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ViewPatch {
    TorrentList {
        upsert: Vec<TorrentView>,
        removed: Vec<String>,
    },
    Torrent {
        torrent: Option<TorrentView>,
    },
    PieceActivity {
        torrent_id: String,
        piece_count: u32,
        verified: Vec<IndexRange>,
        cleared: Vec<IndexRange>,
        active: Option<ActivePiece>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ViewUpdatePayload {
    Snapshot { snapshot: ViewSnapshot },
    Patch { patch: ViewPatch },
    ResetRequired { reason: ResetReason },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum ResetReason {
    QueueOverflow,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ViewUpdate {
    pub contract_version: u16,
    pub stream_id: String,
    pub epoch: String,
    pub sequence: String,
    pub base_revision: String,
    pub revision: String,
    #[serde(flatten)]
    pub payload: ViewUpdatePayload,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SubscriptionStats {
    pub queued_bytes: usize,
    pub queue_high_water: usize,
    pub reset_count: u64,
}

#[derive(Clone, Debug)]
pub struct ViewHub {
    inner: Arc<Mutex<HubState>>,
}

#[derive(Debug)]
struct HubState {
    epoch: u64,
    revision: u64,
    torrents: BTreeMap<String, TorrentModel>,
    subscribers: BTreeMap<u64, Weak<SubscriberInner>>,
    next_stream_id: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TorrentModel {
    view: TorrentView,
    verified: Vec<IndexRange>,
    active: Option<ActivePiece>,
}

#[derive(Clone, Debug)]
pub struct ViewSubscription {
    inner: Arc<SubscriberInner>,
    hub: Weak<Mutex<HubState>>,
}

#[derive(Debug)]
struct SubscriberInner {
    stream_id: u64,
    epoch: u64,
    spec: SubscriptionSpec,
    queue: Mutex<QueueState>,
    notify: Notify,
}

#[derive(Debug)]
struct QueueState {
    entries: VecDeque<QueuedUpdate>,
    queued_bytes: usize,
    queue_high_water: usize,
    reset_count: u64,
    next_sequence: u64,
    tail_revision: u64,
    next_delivery: Instant,
    needs_resync: bool,
    closed: bool,
}

#[derive(Debug)]
struct QueuedUpdate {
    update: ViewUpdate,
    encoded_bytes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubscriptionError {
    InvalidInterval { maximum: u32 },
    InvalidQueueBound { minimum: u32, maximum: u32 },
    InvalidProjection,
    SnapshotExceedsQueue { snapshot: usize, maximum: u32 },
    Closed,
    Internal(String),
}

impl fmt::Display for SubscriptionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInterval { maximum } => {
                write!(
                    formatter,
                    "subscription interval exceeds {maximum} milliseconds"
                )
            }
            Self::InvalidQueueBound { minimum, maximum } => write!(
                formatter,
                "subscription queue must be within {minimum}..={maximum} bytes"
            ),
            Self::InvalidProjection => {
                write!(
                    formatter,
                    "the selected view does not support that projection"
                )
            }
            Self::SnapshotExceedsQueue { snapshot, maximum } => write!(
                formatter,
                "initial snapshot is {snapshot} bytes and exceeds the {maximum}-byte queue"
            ),
            Self::Closed => write!(formatter, "subscription is closed"),
            Self::Internal(message) => write!(formatter, "subscription internal error: {message}"),
        }
    }
}

impl Error for SubscriptionError {}

impl ViewHub {
    pub fn new(snapshot: &ServiceSnapshot) -> Result<Self, SubscriptionError> {
        let revision = parse_revision(&snapshot.revision)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(HubState {
                epoch: NEXT_EPOCH.fetch_add(1, Ordering::Relaxed),
                revision,
                torrents: snapshot
                    .torrents
                    .iter()
                    .map(|torrent| {
                        (
                            torrent.torrent_id.clone(),
                            TorrentModel::from_snapshot(torrent),
                        )
                    })
                    .collect(),
                subscribers: BTreeMap::new(),
                next_stream_id: 1,
            })),
        })
    }

    pub fn subscribe(&self, spec: SubscriptionSpec) -> Result<ViewSubscription, SubscriptionError> {
        validate_spec(&spec)?;
        let mut hub = self
            .inner
            .lock()
            .map_err(|_| SubscriptionError::Internal("view hub lock is poisoned".to_owned()))?;
        let stream_id = hub.next_stream_id;
        hub.next_stream_id = hub
            .next_stream_id
            .checked_add(1)
            .ok_or_else(|| SubscriptionError::Internal("stream ID overflow".to_owned()))?;
        let snapshot = hub.snapshot_for(&spec);
        let inner = Arc::new(SubscriberInner {
            stream_id,
            epoch: hub.epoch,
            spec,
            queue: Mutex::new(QueueState {
                entries: VecDeque::new(),
                queued_bytes: 0,
                queue_high_water: 0,
                reset_count: 0,
                next_sequence: 1,
                tail_revision: hub.revision,
                next_delivery: Instant::now(),
                needs_resync: false,
                closed: false,
            }),
            notify: Notify::new(),
        });
        inner.enqueue_snapshot(hub.revision, snapshot)?;
        hub.subscribers.insert(stream_id, Arc::downgrade(&inner));
        Ok(ViewSubscription {
            inner,
            hub: Arc::downgrade(&self.inner),
        })
    }

    pub(crate) fn replace_durable(
        &self,
        snapshot: &ServiceSnapshot,
        verified: &BTreeMap<String, Vec<IndexRange>>,
    ) -> Result<(), SubscriptionError> {
        let revision = parse_revision(&snapshot.revision)?;
        let mut hub = self
            .inner
            .lock()
            .map_err(|_| SubscriptionError::Internal("view hub lock is poisoned".to_owned()))?;
        let previous = hub.torrents.clone();
        let mut next = BTreeMap::new();
        for torrent in &snapshot.torrents {
            let mut model = TorrentModel::from_snapshot(torrent);
            if let Some(ranges) = verified.get(&torrent.torrent_id) {
                model.verified = ranges.clone();
            } else if let Some(old) = previous.get(&torrent.torrent_id) {
                model.verified = old.verified.clone();
            }
            if let Some(old) = previous.get(&torrent.torrent_id) {
                model.view.requested_bytes = old.view.requested_bytes.clone();
                model.view.received_bytes = old.view.received_bytes.clone();
                model.view.stored_bytes = old.view.stored_bytes.clone();
                model.active = old.active.clone();
            }
            next.insert(torrent.torrent_id.clone(), model);
        }
        hub.revision = revision;
        hub.torrents = next;
        hub.publish_changes(&previous)
    }

    pub(crate) fn record_activity(
        &self,
        torrent_id: &str,
        activity: TorrentActivity,
    ) -> Result<(), SubscriptionError> {
        let mut hub = self
            .inner
            .lock()
            .map_err(|_| SubscriptionError::Internal("view hub lock is poisoned".to_owned()))?;
        let previous = hub.torrents.clone();
        let Some(model) = hub.torrents.get_mut(torrent_id) else {
            return Ok(());
        };
        model.apply_activity(activity);
        hub.publish_changes(&previous)
    }
}

impl HubState {
    fn snapshot_for(&self, spec: &SubscriptionSpec) -> ViewSnapshot {
        match (&spec.selector, spec.projection) {
            (ViewSelector::TorrentList, ViewProjection::Summary) => ViewSnapshot::TorrentList {
                torrents: self
                    .torrents
                    .values()
                    .map(|torrent| torrent.view.clone())
                    .collect(),
            },
            (ViewSelector::Torrent { torrent_id }, ViewProjection::Summary) => {
                ViewSnapshot::Torrent {
                    torrent: self
                        .torrents
                        .get(torrent_id)
                        .map(|torrent| torrent.view.clone()),
                }
            }
            (ViewSelector::Torrent { torrent_id }, ViewProjection::PieceActivity) => {
                let torrent = self.torrents.get(torrent_id);
                ViewSnapshot::PieceActivity {
                    torrent_id: torrent_id.clone(),
                    piece_count: torrent.map_or(0, |torrent| torrent.view.piece_count),
                    verified: torrent.map_or_else(Vec::new, |torrent| torrent.verified.clone()),
                    active: torrent.and_then(|torrent| torrent.active.clone()),
                }
            }
            (ViewSelector::TorrentList, ViewProjection::PieceActivity) => {
                unreachable!("invalid projection is rejected before snapshot construction")
            }
        }
    }

    fn publish_changes(
        &mut self,
        previous: &BTreeMap<String, TorrentModel>,
    ) -> Result<(), SubscriptionError> {
        let revision = self.revision;
        let current = &self.torrents;
        self.subscribers.retain(|_, weak| weak.strong_count() != 0);
        let subscribers = self
            .subscribers
            .values()
            .filter_map(Weak::upgrade)
            .collect::<Vec<_>>();
        for subscriber in subscribers {
            let patch = patch_for(&subscriber.spec, previous, current);
            if let Some(patch) = patch {
                subscriber.enqueue_patch(revision, patch)?;
            }
        }
        Ok(())
    }
}

impl TorrentModel {
    fn from_snapshot(snapshot: &TorrentSnapshot) -> Self {
        Self {
            view: TorrentView {
                torrent_id: snapshot.torrent_id.clone(),
                state: snapshot.state,
                storage_state: snapshot.storage_state,
                metadata_available: snapshot.metadata_available,
                piece_count: snapshot.piece_count,
                verified_piece_count: snapshot.verified_piece_count,
                requested_bytes: "0".to_owned(),
                received_bytes: "0".to_owned(),
                stored_bytes: "0".to_owned(),
                error: snapshot.error.clone(),
            },
            verified: Vec::new(),
            active: None,
        }
    }

    fn apply_activity(&mut self, activity: TorrentActivity) {
        match activity {
            TorrentActivity::PieceStarted {
                piece_index,
                piece_length,
            } => {
                self.active = Some(ActivePiece {
                    piece_index,
                    piece_length,
                    requested: Vec::new(),
                    received: Vec::new(),
                    stored: Vec::new(),
                });
            }
            TorrentActivity::BlockRequested {
                piece_index,
                begin,
                length,
            } => {
                add_counter(&mut self.view.requested_bytes, u64::from(length));
                if let Some(active) = matching_active(&mut self.active, piece_index) {
                    insert_range(&mut active.requested, begin, length);
                }
            }
            TorrentActivity::BlockReceived {
                piece_index,
                begin,
                length,
            } => {
                add_counter(&mut self.view.received_bytes, u64::from(length));
                if let Some(active) = matching_active(&mut self.active, piece_index) {
                    remove_range(&mut active.requested, begin, length);
                    insert_range(&mut active.received, begin, length);
                }
            }
            TorrentActivity::BlockStored {
                piece_index,
                begin,
                length,
            } => {
                add_counter(&mut self.view.stored_bytes, u64::from(length));
                if let Some(active) = matching_active(&mut self.active, piece_index) {
                    remove_range(&mut active.received, begin, length);
                    insert_range(&mut active.stored, begin, length);
                }
            }
            TorrentActivity::PieceVerified { piece_index } => {
                insert_range(&mut self.verified, piece_index, 1);
                self.view.verified_piece_count =
                    range_cardinality(&self.verified).min(u64::from(u32::MAX)) as u32;
                if self
                    .active
                    .as_ref()
                    .is_some_and(|active| active.piece_index == piece_index)
                {
                    self.active = None;
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TorrentActivity {
    PieceStarted {
        piece_index: u32,
        piece_length: u32,
    },
    BlockRequested {
        piece_index: u32,
        begin: u32,
        length: u32,
    },
    BlockReceived {
        piece_index: u32,
        begin: u32,
        length: u32,
    },
    BlockStored {
        piece_index: u32,
        begin: u32,
        length: u32,
    },
    PieceVerified {
        piece_index: u32,
    },
}

impl ViewSubscription {
    pub fn stream_id(&self) -> String {
        self.inner.stream_id.to_string()
    }

    pub async fn next_update(&self) -> Option<ViewUpdate> {
        loop {
            let notified = self.inner.notify.notified();
            let wait = {
                let mut queue = self.inner.queue.lock().ok()?;
                if !queue.entries.is_empty() {
                    let now = Instant::now();
                    if now >= queue.next_delivery {
                        let queued = queue.entries.pop_front().expect("front was present");
                        queue.queued_bytes -= queued.encoded_bytes;
                        queue.next_delivery = now
                            + Duration::from_millis(u64::from(
                                self.inner.spec.delivery.min_interval_millis,
                            ));
                        return Some(queued.update);
                    }
                    Some(queue.next_delivery - now)
                } else if queue.closed {
                    return None;
                } else {
                    None
                }
            };
            if let Some(wait) = wait {
                tokio::select! {
                    () = tokio::time::sleep(wait) => {}
                    () = notified => {}
                }
            } else {
                notified.await;
            }
        }
    }

    pub fn resync(&self) -> Result<(), SubscriptionError> {
        let hub = self.hub.upgrade().ok_or(SubscriptionError::Closed)?;
        let hub = hub
            .lock()
            .map_err(|_| SubscriptionError::Internal("view hub lock is poisoned".to_owned()))?;
        self.inner
            .replace_with_snapshot(hub.revision, hub.snapshot_for(&self.inner.spec))
    }

    pub fn stats(&self) -> Result<SubscriptionStats, SubscriptionError> {
        let queue = self
            .inner
            .queue
            .lock()
            .map_err(|_| SubscriptionError::Internal("queue lock is poisoned".to_owned()))?;
        Ok(SubscriptionStats {
            queued_bytes: queue.queued_bytes,
            queue_high_water: queue.queue_high_water,
            reset_count: queue.reset_count,
        })
    }

    pub fn close(&self) {
        if let Ok(mut queue) = self.inner.queue.lock() {
            queue.closed = true;
            queue.entries.clear();
            queue.queued_bytes = 0;
        }
        self.inner.notify.notify_one();
    }
}

impl Drop for ViewSubscription {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) == 1 {
            self.close();
        }
    }
}

impl SubscriberInner {
    fn enqueue_snapshot(
        &self,
        revision: u64,
        snapshot: ViewSnapshot,
    ) -> Result<(), SubscriptionError> {
        self.replace_with_snapshot(revision, snapshot)
    }

    fn enqueue_patch(&self, revision: u64, patch: ViewPatch) -> Result<(), SubscriptionError> {
        self.enqueue(revision, ViewUpdatePayload::Patch { patch }, true)
    }

    fn replace_with_snapshot(
        &self,
        revision: u64,
        snapshot: ViewSnapshot,
    ) -> Result<(), SubscriptionError> {
        let mut queue = self
            .queue
            .lock()
            .map_err(|_| SubscriptionError::Internal("queue lock is poisoned".to_owned()))?;
        if queue.closed {
            return Err(SubscriptionError::Closed);
        }
        queue.entries.clear();
        queue.queued_bytes = 0;
        queue.needs_resync = false;
        let update = make_update(
            self,
            &mut queue,
            revision,
            ViewUpdatePayload::Snapshot { snapshot },
        );
        let encoded_bytes = encoded_len(&update)?;
        if encoded_bytes > self.spec.delivery.max_queue_bytes as usize {
            return Err(SubscriptionError::SnapshotExceedsQueue {
                snapshot: encoded_bytes,
                maximum: self.spec.delivery.max_queue_bytes,
            });
        }
        push_update(&mut queue, update, encoded_bytes);
        drop(queue);
        self.notify.notify_one();
        Ok(())
    }

    fn enqueue(
        &self,
        revision: u64,
        payload: ViewUpdatePayload,
        allow_coalesce: bool,
    ) -> Result<(), SubscriptionError> {
        let mut queue = self
            .queue
            .lock()
            .map_err(|_| SubscriptionError::Internal("queue lock is poisoned".to_owned()))?;
        if queue.closed || queue.needs_resync {
            return Ok(());
        }
        if allow_coalesce
            && let Some(back) = queue.entries.back_mut()
            && coalesce(&mut back.update, &payload)
        {
            let (previous_bytes, next_bytes) = {
                let previous_bytes = back.encoded_bytes;
                back.update.revision = revision.to_string();
                back.encoded_bytes = encoded_len(&back.update)?;
                (previous_bytes, back.encoded_bytes)
            };
            queue.tail_revision = revision;
            queue.queued_bytes = queue.queued_bytes - previous_bytes + next_bytes;
            if queue.queued_bytes <= self.spec.delivery.max_queue_bytes as usize {
                queue.queue_high_water = queue.queue_high_water.max(queue.queued_bytes);
                drop(queue);
                self.notify.notify_one();
                return Ok(());
            }
        } else {
            let update = make_update(self, &mut queue, revision, payload);
            let encoded_bytes = encoded_len(&update)?;
            if encoded_bytes <= self.spec.delivery.max_queue_bytes as usize
                && queue.queued_bytes + encoded_bytes <= self.spec.delivery.max_queue_bytes as usize
            {
                push_update(&mut queue, update, encoded_bytes);
                drop(queue);
                self.notify.notify_one();
                return Ok(());
            }
        }

        queue.entries.clear();
        queue.queued_bytes = 0;
        queue.needs_resync = true;
        queue.reset_count = queue.reset_count.saturating_add(1);
        let update = make_update(
            self,
            &mut queue,
            revision,
            ViewUpdatePayload::ResetRequired {
                reason: ResetReason::QueueOverflow,
            },
        );
        let encoded_bytes = encoded_len(&update)?;
        push_update(&mut queue, update, encoded_bytes);
        drop(queue);
        self.notify.notify_one();
        Ok(())
    }
}

fn validate_spec(spec: &SubscriptionSpec) -> Result<(), SubscriptionError> {
    if spec.delivery.min_interval_millis > MAX_SUBSCRIPTION_INTERVAL_MILLIS {
        return Err(SubscriptionError::InvalidInterval {
            maximum: MAX_SUBSCRIPTION_INTERVAL_MILLIS,
        });
    }
    if !(MIN_SUBSCRIPTION_QUEUE_BYTES..=MAX_SUBSCRIPTION_QUEUE_BYTES)
        .contains(&spec.delivery.max_queue_bytes)
    {
        return Err(SubscriptionError::InvalidQueueBound {
            minimum: MIN_SUBSCRIPTION_QUEUE_BYTES,
            maximum: MAX_SUBSCRIPTION_QUEUE_BYTES,
        });
    }
    if matches!(spec.selector, ViewSelector::TorrentList)
        && spec.projection == ViewProjection::PieceActivity
    {
        return Err(SubscriptionError::InvalidProjection);
    }
    Ok(())
}

fn parse_revision(value: &str) -> Result<u64, SubscriptionError> {
    value
        .parse()
        .map_err(|_| SubscriptionError::Internal("invalid snapshot revision".to_owned()))
}

fn make_update(
    subscriber: &SubscriberInner,
    queue: &mut QueueState,
    revision: u64,
    payload: ViewUpdatePayload,
) -> ViewUpdate {
    let sequence = queue.next_sequence;
    queue.next_sequence = queue.next_sequence.saturating_add(1);
    let base_revision = queue.tail_revision;
    queue.tail_revision = revision;
    ViewUpdate {
        contract_version: VIEW_CONTRACT_VERSION,
        stream_id: subscriber.stream_id.to_string(),
        epoch: subscriber.epoch.to_string(),
        sequence: sequence.to_string(),
        base_revision: base_revision.to_string(),
        revision: revision.to_string(),
        payload,
    }
}

fn encoded_len(update: &ViewUpdate) -> Result<usize, SubscriptionError> {
    serde_json::to_vec(update)
        .map(|bytes| bytes.len())
        .map_err(|error| SubscriptionError::Internal(error.to_string()))
}

fn push_update(queue: &mut QueueState, update: ViewUpdate, encoded_bytes: usize) {
    queue.queued_bytes += encoded_bytes;
    queue.queue_high_water = queue.queue_high_water.max(queue.queued_bytes);
    queue.entries.push_back(QueuedUpdate {
        update,
        encoded_bytes,
    });
}

fn patch_for(
    spec: &SubscriptionSpec,
    previous: &BTreeMap<String, TorrentModel>,
    current: &BTreeMap<String, TorrentModel>,
) -> Option<ViewPatch> {
    match (&spec.selector, spec.projection) {
        (ViewSelector::TorrentList, ViewProjection::Summary) => {
            let upsert = current
                .iter()
                .filter(|(id, model)| previous.get(*id).map(|old| &old.view) != Some(&model.view))
                .map(|(_, model)| model.view.clone())
                .collect::<Vec<_>>();
            let removed = previous
                .keys()
                .filter(|id| !current.contains_key(*id))
                .cloned()
                .collect::<Vec<_>>();
            (!upsert.is_empty() || !removed.is_empty())
                .then_some(ViewPatch::TorrentList { upsert, removed })
        }
        (ViewSelector::Torrent { torrent_id }, ViewProjection::Summary) => {
            let old = previous.get(torrent_id).map(|model| &model.view);
            let next = current.get(torrent_id).map(|model| &model.view);
            (old != next).then(|| ViewPatch::Torrent {
                torrent: next.cloned(),
            })
        }
        (ViewSelector::Torrent { torrent_id }, ViewProjection::PieceActivity) => {
            let old = previous.get(torrent_id);
            let next = current.get(torrent_id);
            if old == next {
                return None;
            }
            let old_verified = old.map_or(&[][..], |model| model.verified.as_slice());
            let next_verified = next.map_or(&[][..], |model| model.verified.as_slice());
            let verified = difference(next_verified, old_verified);
            let cleared = difference(old_verified, next_verified);
            Some(ViewPatch::PieceActivity {
                torrent_id: torrent_id.clone(),
                piece_count: next.map_or(0, |model| model.view.piece_count),
                verified,
                cleared,
                active: next.and_then(|model| model.active.clone()),
            })
        }
        (ViewSelector::TorrentList, ViewProjection::PieceActivity) => None,
    }
}

fn coalesce(update: &mut ViewUpdate, next: &ViewUpdatePayload) -> bool {
    let (ViewUpdatePayload::Patch { patch: current }, ViewUpdatePayload::Patch { patch: next }) =
        (&mut update.payload, next)
    else {
        return false;
    };
    match (current, next) {
        (
            ViewPatch::TorrentList { upsert, removed },
            ViewPatch::TorrentList {
                upsert: next_upsert,
                removed: next_removed,
            },
        ) => {
            let mut values = upsert
                .drain(..)
                .map(|torrent| (torrent.torrent_id.clone(), torrent))
                .collect::<BTreeMap<_, _>>();
            for id in next_removed {
                values.remove(id);
            }
            for torrent in next_upsert {
                values.insert(torrent.torrent_id.clone(), torrent.clone());
            }
            let mut removed_ids = removed.drain(..).collect::<std::collections::BTreeSet<_>>();
            for torrent in next_upsert {
                removed_ids.remove(&torrent.torrent_id);
            }
            removed_ids.extend(next_removed.iter().cloned());
            *upsert = values.into_values().collect();
            *removed = removed_ids.into_iter().collect();
            true
        }
        (ViewPatch::Torrent { torrent }, ViewPatch::Torrent { torrent: next }) => {
            *torrent = next.clone();
            true
        }
        (
            ViewPatch::PieceActivity {
                torrent_id,
                piece_count,
                verified,
                cleared,
                active,
            },
            ViewPatch::PieceActivity {
                torrent_id: next_id,
                piece_count: next_piece_count,
                verified: next_verified,
                cleared: next_cleared,
                active: next_active,
            },
        ) if torrent_id == next_id => {
            for range in next_cleared {
                remove_interval(verified, *range);
                insert_interval(cleared, *range);
            }
            for range in next_verified {
                remove_interval(cleared, *range);
                insert_interval(verified, *range);
            }
            *piece_count = *next_piece_count;
            *active = next_active.clone();
            true
        }
        _ => false,
    }
}

fn matching_active(active: &mut Option<ActivePiece>, piece_index: u32) -> Option<&mut ActivePiece> {
    active
        .as_mut()
        .filter(|active| active.piece_index == piece_index)
}

fn add_counter(counter: &mut String, increment: u64) {
    let value = counter
        .parse::<u64>()
        .unwrap_or(0)
        .saturating_add(increment);
    *counter = value.to_string();
}

fn insert_range(ranges: &mut Vec<IndexRange>, start: u32, length: u32) {
    if let Some(end) = start.checked_add(length)
        && let Some(range) = IndexRange::new(start, end)
    {
        insert_interval(ranges, range);
    }
}

fn insert_interval(ranges: &mut Vec<IndexRange>, mut inserted: IndexRange) {
    let mut output = Vec::with_capacity(ranges.len() + 1);
    let mut placed = false;
    for range in ranges.drain(..) {
        if range.end_exclusive < inserted.start {
            output.push(range);
        } else if inserted.end_exclusive < range.start {
            if !placed {
                output.push(inserted);
                placed = true;
            }
            output.push(range);
        } else {
            inserted.start = inserted.start.min(range.start);
            inserted.end_exclusive = inserted.end_exclusive.max(range.end_exclusive);
        }
    }
    if !placed {
        output.push(inserted);
    }
    *ranges = output;
}

fn remove_range(ranges: &mut Vec<IndexRange>, start: u32, length: u32) {
    if let Some(end) = start.checked_add(length)
        && let Some(range) = IndexRange::new(start, end)
    {
        remove_interval(ranges, range);
    }
}

fn remove_interval(ranges: &mut Vec<IndexRange>, removed: IndexRange) {
    let mut output = Vec::with_capacity(ranges.len() + 1);
    for range in ranges.drain(..) {
        if range.end_exclusive <= removed.start || range.start >= removed.end_exclusive {
            output.push(range);
            continue;
        }
        if range.start < removed.start {
            output.push(IndexRange {
                start: range.start,
                end_exclusive: removed.start,
            });
        }
        if range.end_exclusive > removed.end_exclusive {
            output.push(IndexRange {
                start: removed.end_exclusive,
                end_exclusive: range.end_exclusive,
            });
        }
    }
    *ranges = output;
}

fn difference(left: &[IndexRange], right: &[IndexRange]) -> Vec<IndexRange> {
    let mut output = left.to_vec();
    for range in right {
        remove_interval(&mut output, *range);
    }
    output
}

fn range_cardinality(ranges: &[IndexRange]) -> u64 {
    ranges
        .iter()
        .map(|range| u64::from(range.end_exclusive - range.start))
        .sum()
}

pub(crate) fn ranges_from_pieces(pieces: &[bool]) -> Vec<IndexRange> {
    let mut ranges = Vec::new();
    let mut start = None;
    for (index, present) in pieces
        .iter()
        .copied()
        .chain(std::iter::once(false))
        .enumerate()
    {
        if present && start.is_none() {
            start = Some(index);
        } else if !present && let Some(range_start) = start.take() {
            let Ok(range_start) = u32::try_from(range_start) else {
                break;
            };
            let Ok(end_exclusive) = u32::try_from(index) else {
                break;
            };
            ranges.push(IndexRange {
                start: range_start,
                end_exclusive,
            });
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        DeliveryPolicy, IndexRange, ResetReason, SubscriptionSpec, TorrentActivity, ViewHub,
        ViewProjection, ViewSelector, ViewSnapshot, ViewUpdatePayload, ranges_from_pieces,
    };
    use crate::{ServiceSnapshot, StorageState, TorrentSnapshot, TorrentState};

    fn snapshot(revision: u64, piece_count: u32) -> ServiceSnapshot {
        ServiceSnapshot {
            profile_id: "test".to_owned(),
            revision: revision.to_string(),
            torrents: vec![TorrentSnapshot {
                torrent_id: "000102030405060708090a0b0c0d0e0f10111213".to_owned(),
                storage_root: "downloads".to_owned(),
                state: TorrentState::Downloading,
                storage_state: StorageState::Staging,
                metadata_available: true,
                piece_count,
                verified_piece_count: 0,
                skip_files: Vec::new(),
                error: None,
            }],
        }
    }

    fn piece_spec(queue: u32) -> SubscriptionSpec {
        SubscriptionSpec {
            selector: ViewSelector::Torrent {
                torrent_id: "000102030405060708090a0b0c0d0e0f10111213".to_owned(),
            },
            projection: ViewProjection::PieceActivity,
            delivery: DeliveryPolicy {
                min_interval_millis: 0,
                max_queue_bytes: queue,
            },
        }
    }

    #[tokio::test]
    async fn starts_with_snapshot_and_keeps_large_indices() {
        let hub = ViewHub::new(&snapshot(7, 1_000_000)).expect("hub");
        let subscription = hub.subscribe(piece_spec(4096)).expect("subscribe");
        let update = subscription.next_update().await.expect("snapshot");
        assert_eq!(update.sequence, "1");
        assert_eq!(update.revision, "7");
        let ViewUpdatePayload::Snapshot {
            snapshot:
                ViewSnapshot::PieceActivity {
                    piece_count,
                    verified,
                    ..
                },
        } = update.payload
        else {
            panic!("expected piece snapshot");
        };
        assert_eq!(piece_count, 1_000_000);
        assert!(verified.is_empty());

        hub.record_activity(
            "000102030405060708090a0b0c0d0e0f10111213",
            TorrentActivity::PieceStarted {
                piece_index: 900_000,
                piece_length: 32 * 1024 * 1024,
            },
        )
        .expect("activity");
        let update = subscription.next_update().await.expect("patch");
        let ViewUpdatePayload::Patch { patch } = update.payload else {
            panic!("expected patch");
        };
        let serialized = serde_json::to_string(&patch).expect("serialize");
        assert!(serialized.contains("900000"));
        assert!(serialized.contains("33554432"));
    }

    #[tokio::test]
    async fn subscribers_have_independent_queues() {
        let hub = ViewHub::new(&snapshot(0, 100)).expect("hub");
        let fast = hub.subscribe(piece_spec(4096)).expect("fast");
        let slow = hub.subscribe(piece_spec(4096)).expect("slow");
        fast.next_update().await.expect("fast snapshot");
        slow.next_update().await.expect("slow snapshot");

        for piece_index in 0..20 {
            hub.record_activity(
                "000102030405060708090a0b0c0d0e0f10111213",
                TorrentActivity::PieceVerified { piece_index },
            )
            .expect("activity");
            fast.next_update().await.expect("fast patch");
        }
        assert_eq!(fast.stats().expect("fast stats").reset_count, 0);
        assert_eq!(slow.stats().expect("slow stats").reset_count, 0);
        let slow_update = slow.next_update().await.expect("coalesced slow patch");
        assert_eq!(slow_update.sequence, "2");
    }

    #[tokio::test]
    async fn overflow_requires_explicit_resync() {
        let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
        let reference = ViewHub::new(&snapshot(0, 2_000)).expect("reference hub");
        for piece_index in (0..2_000).step_by(2) {
            reference
                .record_activity(torrent_id, TorrentActivity::PieceVerified { piece_index })
                .expect("reference activity");
        }
        let reference_subscription = reference
            .subscribe(piece_spec(4 * 1024 * 1024))
            .expect("reference subscription");
        let final_snapshot = reference_subscription
            .next_update()
            .await
            .expect("reference snapshot");
        let queue_bound = u32::try_from(
            serde_json::to_vec(&final_snapshot)
                .expect("encode reference snapshot")
                .len(),
        )
        .expect("snapshot length fits u32")
        .max(4096);

        let hub = ViewHub::new(&snapshot(0, 2_000)).expect("hub");
        let subscription = hub
            .subscribe(piece_spec(queue_bound))
            .expect("subscription");
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            subscription.next_update(),
        )
        .await
        .expect("snapshot delivery timed out")
        .expect("snapshot");

        for piece_index in (0..2_000).step_by(2) {
            hub.record_activity(torrent_id, TorrentActivity::PieceVerified { piece_index })
                .expect("activity");
        }
        assert!(
            subscription.stats().expect("queued stats").queued_bytes > 0,
            "activity must enqueue either a patch or reset"
        );
        let update = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            subscription.next_update(),
        )
        .await
        .expect("reset delivery timed out")
        .expect("reset");
        assert_eq!(
            update.payload,
            ViewUpdatePayload::ResetRequired {
                reason: ResetReason::QueueOverflow
            }
        );
        assert_eq!(subscription.stats().expect("stats").reset_count, 1);
        subscription.resync().expect("resync");
        let replacement = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            subscription.next_update(),
        )
        .await
        .expect("replacement delivery timed out")
        .expect("replacement");
        assert!(matches!(
            replacement.payload,
            ViewUpdatePayload::Snapshot { .. }
        ));
    }

    #[test]
    fn piece_ranges_do_not_expand_indices() {
        let mut pieces = vec![false; 70_005];
        pieces[65_536..70_000].fill(true);
        assert_eq!(
            ranges_from_pieces(&pieces),
            vec![IndexRange {
                start: 65_536,
                end_exclusive: 70_000
            }]
        );
    }

    #[test]
    fn durable_replacement_preserves_exact_have_ranges() {
        let hub = ViewHub::new(&snapshot(0, 4)).expect("hub");
        hub.replace_durable(
            &snapshot(1, 4),
            &BTreeMap::from([(
                "000102030405060708090a0b0c0d0e0f10111213".to_owned(),
                vec![IndexRange {
                    start: 1,
                    end_exclusive: 3,
                }],
            )]),
        )
        .expect("replace");
    }
}
