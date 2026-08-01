use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use ts_rs::TS;

use crate::views::{
    DeliveryPolicy, DiagnosticFilter, HubState, ResetReason, SubscriptionSpec, ViewHub, ViewPatch,
    ViewProjection, ViewSelector, ViewSnapshot, coalesce_patch,
};

pub const API_VERSION: u16 = 1;
pub const MAX_VIEW_SETS: usize = 32;
pub const MAX_VIEW_SETS_PER_OWNER: usize = 8;
pub const MAX_VIEWS_PER_SET: usize = 16;
pub const MAX_VIEW_ID_BYTES: usize = 64;
pub const MIN_VIEW_SET_QUEUE_BYTES: u32 = 16 * 1024;
pub const DEFAULT_VIEW_SET_QUEUE_BYTES: u32 = 256 * 1024;
pub const MAX_VIEW_SET_QUEUE_BYTES: u32 = 512 * 1024;
pub const MAX_VIEW_SET_WAIT_MILLIS: u32 = 20_000;
pub const VIEW_SET_LEASE_MILLIS: u64 = 5 * 60 * 1_000;
pub const MAX_VIEW_DELIVERY_INTERVAL_MILLIS: u32 = 60_000;

static NEXT_VIEW_SET_EPOCH: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum ApiEncoding {
    Json,
    Cbor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryMode {
    Poll,
    LongPoll,
    Stream,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
pub struct ApiVersion {
    pub current: u16,
    pub minimum: u16,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
pub struct ApiLimits {
    pub max_view_sets_per_owner: u16,
    pub max_views_per_set: u16,
    pub max_view_id_bytes: u16,
    pub min_queue_bytes: u32,
    pub default_queue_bytes: u32,
    pub max_queue_bytes: u32,
    pub max_wait_millis: u32,
    pub lease_millis: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
pub struct ApiHello {
    pub api: ApiVersion,
    pub encodings: Vec<ApiEncoding>,
    pub deliveries: Vec<DeliveryMode>,
    pub capabilities: Vec<String>,
    pub limits: ApiLimits,
}

impl Default for ApiHello {
    fn default() -> Self {
        Self {
            api: ApiVersion {
                current: API_VERSION,
                minimum: API_VERSION,
            },
            encodings: vec![ApiEncoding::Json],
            deliveries: vec![DeliveryMode::Poll, DeliveryMode::LongPoll],
            capabilities: vec![
                "torrent_list".to_owned(),
                "torrent_summary".to_owned(),
                "piece_activity".to_owned(),
                "diagnostics".to_owned(),
            ],
            limits: ApiLimits {
                max_view_sets_per_owner: MAX_VIEW_SETS_PER_OWNER as u16,
                max_views_per_set: MAX_VIEWS_PER_SET as u16,
                max_view_id_bytes: MAX_VIEW_ID_BYTES as u16,
                min_queue_bytes: MIN_VIEW_SET_QUEUE_BYTES,
                default_queue_bytes: DEFAULT_VIEW_SET_QUEUE_BYTES,
                max_queue_bytes: MAX_VIEW_SET_QUEUE_BYTES,
                max_wait_millis: MAX_VIEW_SET_WAIT_MILLIS,
                lease_millis: VIEW_SET_LEASE_MILLIS.to_string(),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
pub struct ViewDeliveryPolicy {
    #[serde(default)]
    pub min_interval_millis: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ViewSpec {
    TorrentList {
        view_id: String,
        #[serde(default)]
        delivery: ViewDeliveryPolicy,
    },
    TorrentSummary {
        view_id: String,
        torrent_id: String,
        #[serde(default)]
        delivery: ViewDeliveryPolicy,
    },
    PieceActivity {
        view_id: String,
        torrent_id: String,
        #[serde(default)]
        delivery: ViewDeliveryPolicy,
    },
    Diagnostics {
        view_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        torrent_id: Option<String>,
        #[serde(default)]
        filter: DiagnosticFilter,
        #[serde(default)]
        delivery: ViewDeliveryPolicy,
    },
}

impl ViewSpec {
    pub fn view_id(&self) -> &str {
        match self {
            Self::TorrentList { view_id, .. }
            | Self::TorrentSummary { view_id, .. }
            | Self::PieceActivity { view_id, .. }
            | Self::Diagnostics { view_id, .. } => view_id,
        }
    }

    fn delivery(&self) -> ViewDeliveryPolicy {
        match self {
            Self::TorrentList { delivery, .. }
            | Self::TorrentSummary { delivery, .. }
            | Self::PieceActivity { delivery, .. }
            | Self::Diagnostics { delivery, .. } => *delivery,
        }
    }

    pub(crate) fn subscription_spec(&self, queue_bytes: u32) -> SubscriptionSpec {
        let (selector, projection, diagnostics) = match self {
            Self::TorrentList { .. } => (ViewSelector::TorrentList, ViewProjection::Summary, None),
            Self::TorrentSummary { torrent_id, .. } => (
                ViewSelector::Torrent {
                    torrent_id: torrent_id.clone(),
                },
                ViewProjection::Summary,
                None,
            ),
            Self::PieceActivity { torrent_id, .. } => (
                ViewSelector::Torrent {
                    torrent_id: torrent_id.clone(),
                },
                ViewProjection::PieceActivity,
                None,
            ),
            Self::Diagnostics {
                torrent_id, filter, ..
            } => (
                torrent_id
                    .as_ref()
                    .map_or(ViewSelector::TorrentList, |id| ViewSelector::Torrent {
                        torrent_id: id.clone(),
                    }),
                ViewProjection::Diagnostics,
                Some(filter.clone()),
            ),
        };
        SubscriptionSpec {
            selector,
            projection,
            delivery: DeliveryPolicy {
                min_interval_millis: self.delivery().min_interval_millis,
                max_queue_bytes: queue_bytes,
            },
            diagnostics,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
pub struct OpenViewSetOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_queue_bytes: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
pub struct OpenViewSetRequest {
    pub views: Vec<ViewSpec>,
    #[serde(default)]
    pub options: OpenViewSetOptions,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
pub struct UpdateViewSetRequest {
    pub views: Vec<ViewSpec>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ViewSetUpdate {
    Snapshot {
        view_id: String,
        snapshot: ViewSnapshot,
    },
    Patch {
        view_id: String,
        patch: ViewPatch,
    },
    ViewRemoved {
        view_id: String,
    },
    ResetRequired {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        view_id: Option<String>,
        reason: ResetReason,
    },
}

impl ViewSetUpdate {
    fn view_id(&self) -> Option<&str> {
        match self {
            Self::Snapshot { view_id, .. }
            | Self::Patch { view_id, .. }
            | Self::ViewRemoved { view_id } => Some(view_id),
            Self::ResetRequired { view_id, .. } => view_id.as_deref(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
pub struct UpdateBatch {
    pub api_version: u16,
    pub view_set_id: String,
    pub epoch: String,
    pub base_cursor: String,
    pub cursor: String,
    pub durable_revision: String,
    pub updates: Vec<ViewSetUpdate>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
pub struct OpenViewSetResponse {
    pub view_set_id: String,
    pub lease_millis: String,
    pub effective_queue_bytes: u32,
    pub effective_views: Vec<ViewSpec>,
    pub initial: UpdateBatch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ViewSetOwner(Arc<str>);

impl ViewSetOwner {
    pub fn trusted(value: impl Into<Arc<str>>) -> Self {
        Self(value.into())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ViewSetStats {
    pub queued_bytes: usize,
    pub queue_high_water: usize,
    pub reset_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ViewSetError {
    InvalidViewCount { maximum: usize },
    InvalidViewId,
    DuplicateViewId(String),
    InvalidDeliveryInterval { maximum: u32 },
    InvalidQueueBound { minimum: u32, maximum: u32 },
    InvalidView(String),
    ResourceLimit,
    UnknownViewSet,
    SnapshotExceedsQueue { snapshot: usize, maximum: u32 },
    Closed,
    Internal(String),
}

impl fmt::Display for ViewSetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidViewCount { maximum } => {
                write!(formatter, "view set must contain 1..={maximum} views")
            }
            Self::InvalidViewId => write!(formatter, "view ID is invalid"),
            Self::DuplicateViewId(id) => write!(formatter, "view ID {id} is duplicated"),
            Self::InvalidDeliveryInterval { maximum } => {
                write!(
                    formatter,
                    "view delivery interval exceeds {maximum} milliseconds"
                )
            }
            Self::InvalidQueueBound { minimum, maximum } => {
                write!(
                    formatter,
                    "view-set queue must be within {minimum}..={maximum} bytes"
                )
            }
            Self::InvalidView(message) => write!(formatter, "invalid view: {message}"),
            Self::ResourceLimit => write!(formatter, "view-set resource limit reached"),
            Self::UnknownViewSet => write!(formatter, "view set is unavailable"),
            Self::SnapshotExceedsQueue { snapshot, maximum } => write!(
                formatter,
                "view-set snapshot is {snapshot} bytes and exceeds {maximum} bytes"
            ),
            Self::Closed => write!(formatter, "view set is closed"),
            Self::Internal(message) => write!(formatter, "view set internal error: {message}"),
        }
    }
}

impl Error for ViewSetError {}

#[derive(Clone, Debug)]
pub struct ViewSet {
    pub(crate) inner: Arc<ViewSetInner>,
    pub(crate) hub: Weak<Mutex<HubState>>,
}

#[derive(Debug)]
pub(crate) struct ViewSetInner {
    id: String,
    owner: ViewSetOwner,
    state: Mutex<ViewSetState>,
    notify: Notify,
}

#[derive(Debug)]
struct ViewSetState {
    epoch: u64,
    acknowledged_cursor: u64,
    next_cursor: u64,
    durable_revision: u64,
    views: BTreeMap<String, ViewSpec>,
    queue_bytes_limit: u32,
    pending: VecDeque<QueuedViewSetUpdate>,
    pending_bytes: usize,
    in_flight: Option<StoredBatch>,
    queue_high_water: usize,
    reset_count: u64,
    reset_pending: Option<ResetReason>,
    last_activity: Instant,
    closed: bool,
}

#[derive(Clone, Debug)]
struct QueuedViewSetUpdate {
    update: ViewSetUpdate,
    encoded_bytes: usize,
}

#[derive(Clone, Debug)]
struct StoredBatch {
    batch: UpdateBatch,
    encoded_bytes: usize,
}

enum PollState {
    Ready(UpdateBatch),
    Wait,
    Reset(ResetReason),
    Closed,
}

impl ViewSet {
    pub fn id(&self) -> &str {
        &self.inner.id
    }

    pub async fn next_updates(
        &self,
        after: &str,
        max_wait_millis: u32,
    ) -> Result<UpdateBatch, ViewSetError> {
        if max_wait_millis > MAX_VIEW_SET_WAIT_MILLIS {
            return Err(ViewSetError::InvalidDeliveryInterval {
                maximum: MAX_VIEW_SET_WAIT_MILLIS,
            });
        }
        let after = parse_decimal(after)?;
        let deadline =
            tokio::time::Instant::now() + Duration::from_millis(u64::from(max_wait_millis));
        loop {
            let notified = self.inner.notify.notified();
            match self.inner.poll_state(after, Instant::now())? {
                PollState::Ready(batch) => return Ok(batch),
                PollState::Reset(reason) => return self.reset_from_hub(reason),
                PollState::Closed => return Err(ViewSetError::Closed),
                PollState::Wait if max_wait_millis == 0 => {
                    return self.inner.empty_batch(after, Instant::now());
                }
                PollState::Wait => {
                    if tokio::time::Instant::now() >= deadline {
                        return self.inner.empty_batch(after, Instant::now());
                    }
                    tokio::select! {
                        () = notified => {}
                        () = tokio::time::sleep_until(deadline) => {
                            return self.inner.empty_batch(after, Instant::now());
                        }
                    }
                }
            }
        }
    }

    pub fn stats(&self) -> Result<ViewSetStats, ViewSetError> {
        self.inner.stats()
    }

    fn reset_from_hub(&self, reason: ResetReason) -> Result<UpdateBatch, ViewSetError> {
        let hub = self.hub.upgrade().ok_or(ViewSetError::Closed)?;
        let hub = hub
            .lock()
            .map_err(|_| ViewSetError::Internal("view hub lock is poisoned".to_owned()))?;
        let (revision, snapshots) = hub.snapshots_for_view_set(&self.inner)?;
        self.inner
            .reset_with_snapshots(reason, revision, snapshots, Instant::now())
    }
}

impl ViewHub {
    pub fn open_view_set(
        &self,
        owner: ViewSetOwner,
        request: OpenViewSetRequest,
    ) -> Result<OpenViewSetResponse, ViewSetError> {
        self.open_view_set_at(owner, request, Instant::now())
    }

    fn open_view_set_at(
        &self,
        owner: ViewSetOwner,
        request: OpenViewSetRequest,
        now: Instant,
    ) -> Result<OpenViewSetResponse, ViewSetError> {
        let (views, queue_bytes) = validated_open(&request)?;
        let mut hub = self
            .inner
            .lock()
            .map_err(|_| ViewSetError::Internal("view hub lock is poisoned".to_owned()))?;
        prune_expired(&mut hub, now);
        if hub.view_sets.len() >= MAX_VIEW_SETS
            || hub
                .view_sets
                .values()
                .filter(|view_set| view_set.owner_matches(&owner))
                .count()
                >= MAX_VIEW_SETS_PER_OWNER
        {
            return Err(ViewSetError::ResourceLimit);
        }
        let snapshots = snapshots_for_specs(&hub, &views, queue_bytes);
        let id = loop {
            let candidate = generate_view_set_id()?;
            if !hub.view_sets.contains_key(&candidate) {
                break candidate;
            }
        };
        let inner = ViewSetInner::new(
            id.clone(),
            owner,
            hub.revision,
            views,
            queue_bytes,
            snapshots,
            now,
        )?;
        let response = inner.open_response()?;
        hub.view_sets.insert(id, inner);
        Ok(response)
    }

    pub fn update_view_set(
        &self,
        owner: &ViewSetOwner,
        id: &str,
        request: UpdateViewSetRequest,
    ) -> Result<(), ViewSetError> {
        let views = validated_update(&request)?;
        let now = Instant::now();
        let mut hub = self
            .inner
            .lock()
            .map_err(|_| ViewSetError::Internal("view hub lock is poisoned".to_owned()))?;
        prune_expired(&mut hub, now);
        let view_set = owned_view_set(&hub, owner, id)?;
        let previous = view_set
            .view_specs()?
            .into_iter()
            .map(|spec| (spec.view_id().to_owned(), spec))
            .collect::<BTreeMap<_, _>>();
        let queue_bytes = view_set.queue_bytes_limit()?;
        let mut updates = previous
            .keys()
            .filter(|view_id| !views.contains_key(*view_id))
            .map(|view_id| ViewSetUpdate::ViewRemoved {
                view_id: view_id.clone(),
            })
            .collect::<Vec<_>>();
        for (view_id, spec) in &views {
            if previous.get(view_id) != Some(spec) {
                updates.push(ViewSetUpdate::Snapshot {
                    view_id: view_id.clone(),
                    snapshot: hub.snapshot_for(&spec.subscription_spec(queue_bytes)),
                });
            }
        }
        view_set.replace_views(views, updates, hub.revision, now)
    }

    pub fn view_set(&self, owner: &ViewSetOwner, id: &str) -> Result<ViewSet, ViewSetError> {
        let now = Instant::now();
        let mut hub = self
            .inner
            .lock()
            .map_err(|_| ViewSetError::Internal("view hub lock is poisoned".to_owned()))?;
        prune_expired(&mut hub, now);
        let inner = owned_view_set(&hub, owner, id)?;
        inner.touch(now)?;
        Ok(ViewSet {
            inner,
            hub: Arc::downgrade(&self.inner),
        })
    }

    pub fn close_view_set(&self, owner: &ViewSetOwner, id: &str) -> Result<(), ViewSetError> {
        let mut hub = self
            .inner
            .lock()
            .map_err(|_| ViewSetError::Internal("view hub lock is poisoned".to_owned()))?;
        let view_set = owned_view_set(&hub, owner, id)?;
        hub.view_sets.remove(id);
        view_set.close();
        Ok(())
    }

    pub fn close_all_view_sets(&self) {
        if let Ok(mut hub) = self.inner.lock() {
            for (_, view_set) in std::mem::take(&mut hub.view_sets) {
                view_set.close();
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn expire_view_sets_at(&self, now: Instant) {
        if let Ok(mut hub) = self.inner.lock() {
            prune_expired(&mut hub, now);
        }
    }
}

impl ViewSetInner {
    pub(crate) fn new(
        id: String,
        owner: ViewSetOwner,
        revision: u64,
        views: BTreeMap<String, ViewSpec>,
        queue_bytes_limit: u32,
        snapshots: Vec<ViewSetUpdate>,
        now: Instant,
    ) -> Result<Arc<Self>, ViewSetError> {
        let inner = Arc::new(Self {
            id,
            owner,
            state: Mutex::new(ViewSetState {
                epoch: next_epoch(),
                acknowledged_cursor: 0,
                next_cursor: 1,
                durable_revision: revision,
                views,
                queue_bytes_limit,
                pending: VecDeque::new(),
                pending_bytes: 0,
                in_flight: None,
                queue_high_water: 0,
                reset_count: 0,
                reset_pending: None,
                last_activity: now,
                closed: false,
            }),
            notify: Notify::new(),
        });
        inner.install_initial(snapshots)?;
        Ok(inner)
    }

    pub(crate) fn owner_matches(&self, owner: &ViewSetOwner) -> bool {
        self.owner == *owner
    }

    pub(crate) fn view_specs(&self) -> Result<Vec<ViewSpec>, ViewSetError> {
        let state = self.state()?;
        Ok(state.views.values().cloned().collect())
    }

    fn queue_bytes_limit(&self) -> Result<u32, ViewSetError> {
        Ok(self.state()?.queue_bytes_limit)
    }

    fn open_response(&self) -> Result<OpenViewSetResponse, ViewSetError> {
        let state = self.state()?;
        let initial = state
            .in_flight
            .as_ref()
            .ok_or_else(|| ViewSetError::Internal("initial batch is absent".to_owned()))?
            .batch
            .clone();
        Ok(OpenViewSetResponse {
            view_set_id: self.id.clone(),
            lease_millis: VIEW_SET_LEASE_MILLIS.to_string(),
            effective_queue_bytes: state.queue_bytes_limit,
            effective_views: state.views.values().cloned().collect(),
            initial,
        })
    }

    pub(crate) fn is_expired(&self, now: Instant) -> bool {
        self.state.lock().map_or(true, |state| {
            state.closed
                || now.saturating_duration_since(state.last_activity)
                    >= Duration::from_millis(VIEW_SET_LEASE_MILLIS)
        })
    }

    pub(crate) fn touch(&self, now: Instant) -> Result<(), ViewSetError> {
        let mut state = self.state()?;
        if state.closed {
            return Err(ViewSetError::Closed);
        }
        state.last_activity = now;
        Ok(())
    }

    pub(crate) fn close(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.closed = true;
            state.pending.clear();
            state.pending_bytes = 0;
            state.in_flight = None;
        }
        self.notify.notify_waiters();
    }

    pub(crate) fn replace_views(
        &self,
        views: BTreeMap<String, ViewSpec>,
        updates: Vec<ViewSetUpdate>,
        revision: u64,
        now: Instant,
    ) -> Result<(), ViewSetError> {
        let mut state = self.state()?;
        if state.closed {
            return Err(ViewSetError::Closed);
        }
        state.views = views;
        state.durable_revision = revision;
        state.last_activity = now;
        for update in updates {
            enqueue_update(&mut state, update)?;
        }
        drop(state);
        self.notify.notify_waiters();
        Ok(())
    }

    pub(crate) fn enqueue_patch(
        &self,
        view_id: &str,
        patch: ViewPatch,
        revision: u64,
    ) -> Result<(), ViewSetError> {
        let mut state = self.state()?;
        if state.closed || state.reset_pending.is_some() {
            return Ok(());
        }
        state.durable_revision = revision;
        enqueue_update(
            &mut state,
            ViewSetUpdate::Patch {
                view_id: view_id.to_owned(),
                patch,
            },
        )?;
        drop(state);
        self.notify.notify_waiters();
        Ok(())
    }

    fn install_initial(&self, snapshots: Vec<ViewSetUpdate>) -> Result<(), ViewSetError> {
        let mut state = self.state()?;
        let batch = make_batch(&self.id, &mut state, 0, snapshots)?;
        let encoded_bytes = encoded_batch_len(&batch)?;
        if encoded_bytes > state.queue_bytes_limit as usize {
            return Err(ViewSetError::SnapshotExceedsQueue {
                snapshot: encoded_bytes,
                maximum: state.queue_bytes_limit,
            });
        }
        state.queue_high_water = encoded_bytes;
        state.in_flight = Some(StoredBatch {
            batch,
            encoded_bytes,
        });
        Ok(())
    }

    fn poll_state(&self, after: u64, now: Instant) -> Result<PollState, ViewSetError> {
        let mut state = self.state()?;
        if state.closed {
            return Ok(PollState::Closed);
        }
        state.last_activity = now;
        if let Some(in_flight) = &state.in_flight {
            let base = parse_decimal(&in_flight.batch.base_cursor)?;
            let cursor = parse_decimal(&in_flight.batch.cursor)?;
            if after == base {
                return Ok(PollState::Ready(in_flight.batch.clone()));
            }
            if after == cursor {
                state.acknowledged_cursor = cursor;
                state.in_flight = None;
            } else {
                return Ok(PollState::Reset(ResetReason::CursorMismatch));
            }
        } else if after != state.acknowledged_cursor {
            return Ok(PollState::Reset(ResetReason::CursorMismatch));
        }
        if let Some(reason) = state.reset_pending {
            return Ok(PollState::Reset(reason));
        }
        if state.pending.is_empty() {
            return Ok(PollState::Wait);
        }
        let updates = state
            .pending
            .drain(..)
            .map(|queued| queued.update)
            .collect::<Vec<_>>();
        state.pending_bytes = 0;
        let base = state.acknowledged_cursor;
        let batch = make_batch(&self.id, &mut state, base, updates)?;
        let encoded_bytes = encoded_batch_len(&batch)?;
        if encoded_bytes > state.queue_bytes_limit as usize {
            state.reset_pending = Some(ResetReason::QueueOverflow);
            return Ok(PollState::Reset(ResetReason::QueueOverflow));
        }
        state.queue_high_water = state.queue_high_water.max(encoded_bytes);
        state.in_flight = Some(StoredBatch {
            batch: batch.clone(),
            encoded_bytes,
        });
        Ok(PollState::Ready(batch))
    }

    fn empty_batch(&self, after: u64, now: Instant) -> Result<UpdateBatch, ViewSetError> {
        let mut state = self.state()?;
        if state.closed {
            return Err(ViewSetError::Closed);
        }
        state.last_activity = now;
        Ok(UpdateBatch {
            api_version: API_VERSION,
            view_set_id: self.id.clone(),
            epoch: state.epoch.to_string(),
            base_cursor: after.to_string(),
            cursor: after.to_string(),
            durable_revision: state.durable_revision.to_string(),
            updates: Vec::new(),
        })
    }

    fn reset_with_snapshots(
        &self,
        reason: ResetReason,
        revision: u64,
        mut snapshots: Vec<ViewSetUpdate>,
        now: Instant,
    ) -> Result<UpdateBatch, ViewSetError> {
        let mut state = self.state()?;
        if state.closed {
            return Err(ViewSetError::Closed);
        }
        state.epoch = next_epoch();
        state.acknowledged_cursor = 0;
        state.next_cursor = 1;
        state.durable_revision = revision;
        state.pending.clear();
        state.pending_bytes = 0;
        state.in_flight = None;
        state.reset_pending = None;
        state.reset_count = state.reset_count.saturating_add(1);
        state.last_activity = now;
        let mut updates = vec![ViewSetUpdate::ResetRequired {
            view_id: None,
            reason,
        }];
        updates.append(&mut snapshots);
        let batch = make_batch(&self.id, &mut state, 0, updates)?;
        let encoded_bytes = encoded_batch_len(&batch)?;
        if encoded_bytes > state.queue_bytes_limit as usize {
            return Err(ViewSetError::SnapshotExceedsQueue {
                snapshot: encoded_bytes,
                maximum: state.queue_bytes_limit,
            });
        }
        state.queue_high_water = state.queue_high_water.max(encoded_bytes);
        state.in_flight = Some(StoredBatch {
            batch: batch.clone(),
            encoded_bytes,
        });
        Ok(batch)
    }

    fn stats(&self) -> Result<ViewSetStats, ViewSetError> {
        let state = self.state()?;
        let in_flight = state
            .in_flight
            .as_ref()
            .map_or(0, |batch| batch.encoded_bytes);
        Ok(ViewSetStats {
            queued_bytes: state.pending_bytes.saturating_add(in_flight),
            queue_high_water: state.queue_high_water,
            reset_count: state.reset_count,
        })
    }

    fn state(&self) -> Result<std::sync::MutexGuard<'_, ViewSetState>, ViewSetError> {
        self.state
            .lock()
            .map_err(|_| ViewSetError::Internal("view-set lock is poisoned".to_owned()))
    }
}

fn validate_specs(views: &[ViewSpec]) -> Result<BTreeMap<String, ViewSpec>, ViewSetError> {
    if views.is_empty() || views.len() > MAX_VIEWS_PER_SET {
        return Err(ViewSetError::InvalidViewCount {
            maximum: MAX_VIEWS_PER_SET,
        });
    }
    let mut output = BTreeMap::new();
    for view in views {
        let id = view.view_id();
        if id.is_empty()
            || id.len() > MAX_VIEW_ID_BYTES
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(ViewSetError::InvalidViewId);
        }
        if view.delivery().min_interval_millis > MAX_VIEW_DELIVERY_INTERVAL_MILLIS {
            return Err(ViewSetError::InvalidDeliveryInterval {
                maximum: MAX_VIEW_DELIVERY_INTERVAL_MILLIS,
            });
        }
        let spec = view.subscription_spec(DEFAULT_VIEW_SET_QUEUE_BYTES);
        super::views::validate_spec(&spec)
            .map_err(|error| ViewSetError::InvalidView(error.to_string()))?;
        if output.insert(id.to_owned(), view.clone()).is_some() {
            return Err(ViewSetError::DuplicateViewId(id.to_owned()));
        }
    }
    Ok(output)
}

fn owned_view_set(
    hub: &HubState,
    owner: &ViewSetOwner,
    id: &str,
) -> Result<Arc<ViewSetInner>, ViewSetError> {
    hub.view_sets
        .get(id)
        .filter(|view_set| view_set.owner_matches(owner))
        .cloned()
        .ok_or(ViewSetError::UnknownViewSet)
}

fn prune_expired(hub: &mut HubState, now: Instant) {
    hub.view_sets.retain(|_, view_set| {
        let retain = !view_set.is_expired(now);
        if !retain {
            view_set.close();
        }
        retain
    });
}

fn snapshots_for_specs(
    hub: &HubState,
    views: &BTreeMap<String, ViewSpec>,
    queue_bytes: u32,
) -> Vec<ViewSetUpdate> {
    views
        .iter()
        .map(|(view_id, spec)| ViewSetUpdate::Snapshot {
            view_id: view_id.clone(),
            snapshot: hub.snapshot_for(&spec.subscription_spec(queue_bytes)),
        })
        .collect()
}

pub(crate) fn validated_open(
    request: &OpenViewSetRequest,
) -> Result<(BTreeMap<String, ViewSpec>, u32), ViewSetError> {
    let views = validate_specs(&request.views)?;
    let queue_bytes = request
        .options
        .requested_queue_bytes
        .unwrap_or(DEFAULT_VIEW_SET_QUEUE_BYTES);
    if !(MIN_VIEW_SET_QUEUE_BYTES..=MAX_VIEW_SET_QUEUE_BYTES).contains(&queue_bytes) {
        return Err(ViewSetError::InvalidQueueBound {
            minimum: MIN_VIEW_SET_QUEUE_BYTES,
            maximum: MAX_VIEW_SET_QUEUE_BYTES,
        });
    }
    Ok((views, queue_bytes))
}

pub(crate) fn validated_update(
    request: &UpdateViewSetRequest,
) -> Result<BTreeMap<String, ViewSpec>, ViewSetError> {
    validate_specs(&request.views)
}

pub(crate) fn generate_view_set_id() -> Result<String, ViewSetError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| ViewSetError::Internal(error.to_string()))?;
    let mut output = String::with_capacity(35);
    output.push_str("vs_");
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}")
            .map_err(|error| ViewSetError::Internal(error.to_string()))?;
    }
    Ok(output)
}

fn enqueue_update(state: &mut ViewSetState, update: ViewSetUpdate) -> Result<(), ViewSetError> {
    if matches!(
        update,
        ViewSetUpdate::Snapshot { .. } | ViewSetUpdate::ViewRemoved { .. }
    ) && let Some(view_id) = update.view_id()
    {
        let mut kept = VecDeque::with_capacity(state.pending.len());
        while let Some(queued) = state.pending.pop_front() {
            if queued.update.view_id() == Some(view_id) {
                state.pending_bytes = state.pending_bytes.saturating_sub(queued.encoded_bytes);
            } else {
                kept.push_back(queued);
            }
        }
        state.pending = kept;
    }
    if let ViewSetUpdate::Patch {
        view_id: next_id,
        patch: next_patch,
    } = &update
    {
        let replacement = if let Some(queued) = state.pending.back_mut()
            && let ViewSetUpdate::Patch { view_id, patch } = &mut queued.update
            && view_id == next_id
            && coalesce_patch(patch, next_patch)
        {
            let previous = queued.encoded_bytes;
            queued.encoded_bytes = encoded_update_len(&queued.update)?;
            Some((previous, queued.encoded_bytes))
        } else {
            None
        };
        if let Some((previous, replacement)) = replacement {
            state.pending_bytes = state.pending_bytes - previous + replacement;
            return enforce_bound(state);
        }
    }
    let encoded_bytes = encoded_update_len(&update)?;
    state.pending_bytes = state.pending_bytes.saturating_add(encoded_bytes);
    state.pending.push_back(QueuedViewSetUpdate {
        update,
        encoded_bytes,
    });
    enforce_bound(state)
}

fn enforce_bound(state: &mut ViewSetState) -> Result<(), ViewSetError> {
    let in_flight = state
        .in_flight
        .as_ref()
        .map_or(0, |batch| batch.encoded_bytes);
    let used = in_flight.saturating_add(state.pending_bytes);
    state.queue_high_water = state
        .queue_high_water
        .max(used.min(state.queue_bytes_limit as usize));
    if used <= state.queue_bytes_limit as usize {
        return Ok(());
    }
    state.pending.clear();
    state.pending_bytes = 0;
    state.reset_pending = Some(ResetReason::QueueOverflow);
    Ok(())
}

fn make_batch(
    id: &str,
    state: &mut ViewSetState,
    base_cursor: u64,
    updates: Vec<ViewSetUpdate>,
) -> Result<UpdateBatch, ViewSetError> {
    let cursor = state.next_cursor;
    state.next_cursor = state
        .next_cursor
        .checked_add(1)
        .ok_or_else(|| ViewSetError::Internal("view-set cursor overflow".to_owned()))?;
    Ok(UpdateBatch {
        api_version: API_VERSION,
        view_set_id: id.to_owned(),
        epoch: state.epoch.to_string(),
        base_cursor: base_cursor.to_string(),
        cursor: cursor.to_string(),
        durable_revision: state.durable_revision.to_string(),
        updates,
    })
}

fn encoded_update_len(update: &ViewSetUpdate) -> Result<usize, ViewSetError> {
    serde_json::to_vec(update)
        .map(|bytes| bytes.len())
        .map_err(|error| ViewSetError::Internal(error.to_string()))
}

fn encoded_batch_len(batch: &UpdateBatch) -> Result<usize, ViewSetError> {
    serde_json::to_vec(batch)
        .map(|bytes| bytes.len())
        .map_err(|error| ViewSetError::Internal(error.to_string()))
}

fn parse_decimal(value: &str) -> Result<u64, ViewSetError> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(ViewSetError::InvalidView(
            "cursor is not canonical decimal".to_owned(),
        ));
    }
    value
        .parse()
        .map_err(|_| ViewSetError::InvalidView("cursor is out of range".to_owned()))
}

fn next_epoch() -> u64 {
    NEXT_VIEW_SET_EPOCH.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DiagnosticCategory, DiagnosticEvent, DiagnosticSeverity, ProgressAction,
        ProgressAssessment, ProgressDisposition, ProgressPhase, ProgressReason, ServiceSnapshot,
        StorageState, TorrentSnapshot, TorrentState, TorrentView,
    };

    const TORRENT_ID: &str = "000102030405060708090a0b0c0d0e0f10111213";

    fn torrent_view(id: &str, verified: u32) -> TorrentView {
        TorrentView {
            torrent_id: id.to_owned(),
            state: TorrentState::Downloading,
            storage_state: StorageState::Staging,
            metadata_available: true,
            piece_count: 3,
            verified_piece_count: verified,
            requested_bytes: "0".to_owned(),
            received_bytes: "0".to_owned(),
            stored_bytes: "0".to_owned(),
            progress: ProgressAssessment {
                disposition: ProgressDisposition::Active,
                phase: ProgressPhase::Transfer,
                reason: ProgressReason::TransferringPieces,
                actions: Vec::<ProgressAction>::new(),
            },
            error: None,
        }
    }

    fn spec() -> ViewSpec {
        ViewSpec::TorrentList {
            view_id: "library".to_owned(),
            delivery: ViewDeliveryPolicy::default(),
        }
    }

    fn service_snapshot(revision: u64, verified: u32) -> ServiceSnapshot {
        ServiceSnapshot {
            profile_id: "test".to_owned(),
            revision: revision.to_string(),
            torrents: vec![TorrentSnapshot {
                torrent_id: TORRENT_ID.to_owned(),
                storage_root: "downloads".to_owned(),
                state: TorrentState::Downloading,
                storage_state: StorageState::Staging,
                metadata_available: true,
                piece_count: 3,
                verified_piece_count: verified,
                skip_files: Vec::new(),
                error: None,
            }],
        }
    }

    fn open_request(views: Vec<ViewSpec>) -> OpenViewSetRequest {
        OpenViewSetRequest {
            views,
            options: OpenViewSetOptions::default(),
        }
    }

    fn inner(now: Instant) -> Arc<ViewSetInner> {
        let views = BTreeMap::from([("library".to_owned(), spec())]);
        ViewSetInner::new(
            "vs_test".to_owned(),
            ViewSetOwner::trusted("owner"),
            7,
            views,
            DEFAULT_VIEW_SET_QUEUE_BYTES,
            vec![ViewSetUpdate::Snapshot {
                view_id: "library".to_owned(),
                snapshot: ViewSnapshot::TorrentList {
                    torrents: vec![torrent_view("aa", 0)],
                },
            }],
            now,
        )
        .expect("view set")
    }

    #[test]
    fn validates_ids_counts_and_queue_bounds() {
        let duplicate = OpenViewSetRequest {
            views: vec![spec(), spec()],
            options: OpenViewSetOptions::default(),
        };
        assert!(matches!(
            validated_open(&duplicate),
            Err(ViewSetError::DuplicateViewId(_))
        ));
        let invalid = OpenViewSetRequest {
            views: vec![ViewSpec::TorrentList {
                view_id: "bad id".to_owned(),
                delivery: ViewDeliveryPolicy::default(),
            }],
            options: OpenViewSetOptions::default(),
        };
        assert_eq!(validated_open(&invalid), Err(ViewSetError::InvalidViewId));
        let queue = OpenViewSetRequest {
            views: vec![spec()],
            options: OpenViewSetOptions {
                requested_queue_bytes: Some(1),
            },
        };
        assert!(matches!(
            validated_open(&queue),
            Err(ViewSetError::InvalidQueueBound { .. })
        ));
    }

    #[test]
    fn replays_until_acknowledged_then_emits_accumulated_patch() {
        let now = Instant::now();
        let inner = inner(now);
        let first = match inner.poll_state(0, now).expect("poll") {
            PollState::Ready(batch) => batch,
            _ => panic!("initial batch missing"),
        };
        assert_eq!(first.cursor, "1");
        let replay = match inner.poll_state(0, now).expect("poll") {
            PollState::Ready(batch) => batch,
            _ => panic!("replay missing"),
        };
        assert_eq!(replay, first);
        inner
            .enqueue_patch(
                "library",
                ViewPatch::TorrentList {
                    upsert: vec![torrent_view("aa", 1)],
                    removed: Vec::new(),
                },
                8,
            )
            .expect("patch");
        let next = match inner.poll_state(1, now).expect("poll") {
            PollState::Ready(batch) => batch,
            _ => panic!("next batch missing"),
        };
        assert_eq!(next.base_cursor, "1");
        assert_eq!(next.cursor, "2");
        assert_eq!(next.durable_revision, "8");
        assert_eq!(next.updates.len(), 1);
    }

    #[test]
    fn mismatched_cursor_requests_reset() {
        let now = Instant::now();
        let inner = inner(now);
        assert!(matches!(
            inner.poll_state(99, now).expect("poll"),
            PollState::Reset(ResetReason::CursorMismatch)
        ));
    }

    #[test]
    fn expiry_and_close_are_observable() {
        let now = Instant::now();
        let inner = inner(now);
        assert!(!inner.is_expired(now));
        assert!(inner.is_expired(now + Duration::from_millis(VIEW_SET_LEASE_MILLIS)));
        inner.close();
        assert!(matches!(
            inner.poll_state(0, now).expect("poll"),
            PollState::Closed
        ));
    }

    #[tokio::test]
    async fn hub_publishes_independent_replayable_batches() {
        let hub = ViewHub::new(&service_snapshot(0, 0)).expect("hub");
        let owner = ViewSetOwner::trusted("owner");
        let first = hub
            .open_view_set(owner.clone(), open_request(vec![spec()]))
            .expect("first view set");
        let second = hub
            .open_view_set(owner.clone(), open_request(vec![spec()]))
            .expect("second view set");
        let first_set = hub
            .view_set(&owner, &first.view_set_id)
            .expect("first handle");
        let second_set = hub
            .view_set(&owner, &second.view_set_id)
            .expect("second handle");

        hub.replace_durable(&service_snapshot(1, 1), &BTreeMap::new())
            .expect("replace durable state");
        let first_batch = first_set
            .next_updates(&first.initial.cursor, 0)
            .await
            .expect("first patch");
        let replay = first_set
            .next_updates(&first.initial.cursor, 0)
            .await
            .expect("replay");
        let second_batch = second_set
            .next_updates(&second.initial.cursor, 0)
            .await
            .expect("second patch");

        assert_eq!(replay, first_batch);
        assert_eq!(first_batch.durable_revision, "1");
        assert_eq!(second_batch.durable_revision, "1");
        assert_ne!(first_batch.view_set_id, second_batch.view_set_id);
        assert!(matches!(
            first_batch.updates.as_slice(),
            [ViewSetUpdate::Patch { view_id, .. }] if view_id == "library"
        ));
    }

    #[tokio::test]
    async fn view_replacement_is_atomic_and_reports_removal() {
        let hub = ViewHub::new(&service_snapshot(0, 0)).expect("hub");
        let owner = ViewSetOwner::trusted("owner");
        let opened = hub
            .open_view_set(owner.clone(), open_request(vec![spec()]))
            .expect("view set");
        let details = ViewSpec::TorrentSummary {
            view_id: "details".to_owned(),
            torrent_id: TORRENT_ID.to_owned(),
            delivery: ViewDeliveryPolicy::default(),
        };
        hub.update_view_set(
            &owner,
            &opened.view_set_id,
            UpdateViewSetRequest {
                views: vec![details.clone()],
            },
        )
        .expect("replace views");
        let view_set = hub
            .view_set(&owner, &opened.view_set_id)
            .expect("view set handle");
        let batch = view_set
            .next_updates(&opened.initial.cursor, 0)
            .await
            .expect("replacement batch");

        assert_eq!(batch.updates.len(), 2);
        assert!(batch.updates.contains(&ViewSetUpdate::ViewRemoved {
            view_id: "library".to_owned(),
        }));
        assert!(matches!(
            batch.updates.as_slice(),
            [_, ViewSetUpdate::Snapshot { view_id, .. }] if view_id == details.view_id()
        ));
    }

    #[test]
    fn owners_cannot_observe_or_mutate_each_others_sets() {
        let hub = ViewHub::new(&service_snapshot(0, 0)).expect("hub");
        let owner = ViewSetOwner::trusted("owner-a");
        let stranger = ViewSetOwner::trusted("owner-b");
        let opened = hub
            .open_view_set(owner, open_request(vec![spec()]))
            .expect("view set");

        assert!(matches!(
            hub.view_set(&stranger, &opened.view_set_id),
            Err(ViewSetError::UnknownViewSet)
        ));
        assert!(matches!(
            hub.close_view_set(&stranger, &opened.view_set_id),
            Err(ViewSetError::UnknownViewSet)
        ));
    }

    #[tokio::test]
    async fn queue_overflow_rotates_epoch_and_restores_snapshots() {
        let hub = ViewHub::new(&service_snapshot(0, 0)).expect("hub");
        let owner = ViewSetOwner::trusted("owner");
        let opened = hub
            .open_view_set(
                owner.clone(),
                OpenViewSetRequest {
                    views: vec![ViewSpec::Diagnostics {
                        view_id: "logs".to_owned(),
                        torrent_id: None,
                        filter: DiagnosticFilter::default(),
                        delivery: ViewDeliveryPolicy::default(),
                    }],
                    options: OpenViewSetOptions {
                        requested_queue_bytes: Some(MIN_VIEW_SET_QUEUE_BYTES),
                    },
                },
            )
            .expect("view set");
        let view_set = hub
            .view_set(&owner, &opened.view_set_id)
            .expect("view set handle");
        view_set
            .inner
            .enqueue_patch(
                "logs",
                ViewPatch::Diagnostics {
                    events: vec![DiagnosticEvent {
                        sequence: "1".to_owned(),
                        timestamp_millis: "1".to_owned(),
                        severity: DiagnosticSeverity::Info,
                        category: DiagnosticCategory::Lifecycle,
                        code: "oversized".to_owned(),
                        torrent_id: None,
                        summary: "x".repeat(MIN_VIEW_SET_QUEUE_BYTES as usize),
                        context: Vec::new(),
                    }],
                    dropped_count: "0".to_owned(),
                },
                1,
            )
            .expect("overflow is converted to reset");

        let reset = view_set
            .next_updates(&opened.initial.cursor, 0)
            .await
            .expect("reset batch");
        assert_ne!(reset.epoch, opened.initial.epoch);
        assert!(matches!(
            reset.updates.first(),
            Some(ViewSetUpdate::ResetRequired {
                reason: ResetReason::QueueOverflow,
                ..
            })
        ));
        assert!(matches!(
            reset.updates.get(1),
            Some(ViewSetUpdate::Snapshot { view_id, .. }) if view_id == "logs"
        ));
        assert_eq!(view_set.stats().expect("stats").reset_count, 1);
    }

    #[test]
    fn expired_sets_are_pruned_from_owner_capacity() {
        let hub = ViewHub::new(&service_snapshot(0, 0)).expect("hub");
        let owner = ViewSetOwner::trusted("owner");
        let opened = hub
            .open_view_set(owner.clone(), open_request(vec![spec()]))
            .expect("view set");
        hub.expire_view_sets_at(Instant::now() + Duration::from_millis(VIEW_SET_LEASE_MILLIS));

        assert!(matches!(
            hub.view_set(&owner, &opened.view_set_id),
            Err(ViewSetError::UnknownViewSet)
        ));
        hub.open_view_set(owner, open_request(vec![spec()]))
            .expect("expired capacity is reclaimed");
    }

    #[tokio::test]
    async fn close_wakes_a_waiting_long_poll() {
        let hub = ViewHub::new(&service_snapshot(0, 0)).expect("hub");
        let owner = ViewSetOwner::trusted("owner");
        let opened = hub
            .open_view_set(owner.clone(), open_request(vec![spec()]))
            .expect("view set");
        let view_set = hub
            .view_set(&owner, &opened.view_set_id)
            .expect("view set handle");
        let cursor = opened.initial.cursor.clone();
        let waiter = tokio::spawn(async move { view_set.next_updates(&cursor, 20_000).await });
        tokio::task::yield_now().await;
        hub.close_view_set(&owner, &opened.view_set_id)
            .expect("close");
        let result = tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .expect("waiter timed out")
            .expect("waiter task");
        assert_eq!(result, Err(ViewSetError::Closed));
    }
}
