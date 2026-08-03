use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tokio::task::{JoinError, JoinHandle};
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use crate::diagnostics::DiagnosticFilter;
use crate::views::{
    DeliveryPolicy, HubState, ResetReason, SubscriptionSpec, ViewHub, ViewPatch, ViewProjection,
    ViewSelector, ViewSnapshot, coalesce_patch,
};

pub const API_VERSION: u16 = 1;
pub const MAX_VIEW_SETS: usize = 32;
pub const MAX_VIEW_SETS_PER_OWNER: usize = 8;
pub const MAX_VIEWS_PER_SET: usize = 16;
pub const MAX_VIEW_ID_BYTES: usize = 64;
pub const MIN_VIEW_SET_QUEUE_BYTES: u32 = 16 * 1024;
pub const DEFAULT_VIEW_SET_QUEUE_BYTES: u32 = 256 * 1024;
pub const MAX_VIEW_SET_QUEUE_BYTES: u32 = 512 * 1024;
pub const MAX_VIEW_SET_SNAPSHOT_BYTES: u32 = 16 * 1024 * 1024;
pub const MAX_VIEW_SET_WAIT_MILLIS: u32 = 20_000;
pub const VIEW_SET_LEASE_MILLIS: u64 = 5 * 60 * 1_000;
pub const VIEW_SET_REAPER_INTERVAL_MILLIS: u64 = 5_000;
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
    pub max_snapshot_bytes: u32,
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
                "torrent_peers".to_owned(),
                "torrent_files".to_owned(),
                "torrent_trackers".to_owned(),
                "session_disk".to_owned(),
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
                max_snapshot_bytes: MAX_VIEW_SET_SNAPSHOT_BYTES,
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
    SessionDisk {
        view_id: String,
        #[serde(default)]
        delivery: ViewDeliveryPolicy,
    },
    TorrentPeers {
        view_id: String,
        torrent_id: String,
        #[serde(default)]
        delivery: ViewDeliveryPolicy,
    },
    TorrentFiles {
        view_id: String,
        torrent_id: String,
        #[serde(default)]
        delivery: ViewDeliveryPolicy,
    },
    TorrentTrackers {
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
            | Self::SessionDisk { view_id, .. }
            | Self::TorrentPeers { view_id, .. }
            | Self::TorrentFiles { view_id, .. }
            | Self::TorrentTrackers { view_id, .. }
            | Self::Diagnostics { view_id, .. } => view_id,
        }
    }

    fn delivery(&self) -> ViewDeliveryPolicy {
        match self {
            Self::TorrentList { delivery, .. }
            | Self::TorrentSummary { delivery, .. }
            | Self::PieceActivity { delivery, .. }
            | Self::SessionDisk { delivery, .. }
            | Self::TorrentPeers { delivery, .. }
            | Self::TorrentFiles { delivery, .. }
            | Self::TorrentTrackers { delivery, .. }
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
            Self::SessionDisk { .. } => (ViewSelector::TorrentList, ViewProjection::Disk, None),
            Self::TorrentPeers { torrent_id, .. } => (
                ViewSelector::Torrent {
                    torrent_id: torrent_id.clone(),
                },
                ViewProjection::Peers,
                None,
            ),
            Self::TorrentFiles { torrent_id, .. } => (
                ViewSelector::Torrent {
                    torrent_id: torrent_id.clone(),
                },
                ViewProjection::Files,
                None,
            ),
            Self::TorrentTrackers { torrent_id, .. } => (
                ViewSelector::Torrent {
                    torrent_id: torrent_id.clone(),
                },
                ViewProjection::Trackers,
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
    ConsumerBusy,
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
            Self::ConsumerBusy => write!(formatter, "view set already has an active consumer"),
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
    polling: AtomicBool,
    lease: Duration,
}

#[derive(Debug)]
pub(crate) struct ViewSetLeaseReaper {
    cancellation: CancellationToken,
    task: Option<JoinHandle<()>>,
}

impl ViewSetLeaseReaper {
    pub(crate) fn start(hub: ViewHub, interval: Duration) -> Self {
        let cancellation = CancellationToken::new();
        let task_cancellation = cancellation.clone();
        let task = tokio::spawn(async move {
            let mut timer = tokio::time::interval(interval);
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    biased;
                    _ = task_cancellation.cancelled() => break,
                    _ = timer.tick() => {
                        hub.reap_expired_view_sets();
                    }
                }
            }
        });
        Self {
            cancellation,
            task: Some(task),
        }
    }

    pub(crate) async fn shutdown(&mut self) -> Result<(), JoinError> {
        self.cancellation.cancel();
        if let Some(task) = self.task.take() {
            task.await?;
        }
        Ok(())
    }
}

impl Drop for ViewSetLeaseReaper {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
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
    last_delivered: BTreeMap<String, Instant>,
    in_flight: Option<StoredBatch>,
    queue_high_water: usize,
    reset_count: u64,
    reset_pending: Option<ResetReason>,
    last_client_activity: Instant,
    closed: bool,
}

#[derive(Clone, Debug)]
struct QueuedViewSetUpdate {
    update: ViewSetUpdate,
    encoded_bytes: usize,
    ready_at: Instant,
}

#[derive(Clone, Debug)]
struct StoredBatch {
    batch: UpdateBatch,
    encoded_bytes: usize,
}

struct ViewSetInitialState {
    revision: u64,
    views: BTreeMap<String, ViewSpec>,
    queue_bytes_limit: u32,
    snapshots: Vec<ViewSetUpdate>,
    now: Instant,
    lease: Duration,
}

enum PollState {
    Ready(UpdateBatch),
    Wait(Option<Instant>),
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
        let _poll = self.inner.start_poll()?;
        self.inner.touch(Instant::now())?;
        let deadline =
            tokio::time::Instant::now() + Duration::from_millis(u64::from(max_wait_millis));
        loop {
            let notified = self.inner.notify.notified();
            match self.inner.poll_state(after, Instant::now())? {
                PollState::Ready(batch) => return Ok(batch),
                PollState::Reset(reason) => return self.reset_from_hub(reason),
                PollState::Closed => return Err(ViewSetError::Closed),
                PollState::Wait(_) if max_wait_millis == 0 => {
                    return self.inner.empty_batch(after, Instant::now());
                }
                PollState::Wait(ready_at) => {
                    if tokio::time::Instant::now() >= deadline {
                        return self.inner.empty_batch(after, Instant::now());
                    }
                    let wake_at = ready_at.map_or(deadline, |ready_at| {
                        tokio::time::Instant::from_std(ready_at).min(deadline)
                    });
                    tokio::select! {
                        () = notified => {}
                        () = tokio::time::sleep_until(wake_at) => {
                            if wake_at == deadline {
                                return self.inner.empty_batch(after, Instant::now());
                            }
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
            ViewSetInitialState {
                revision: hub.revision,
                views,
                queue_bytes_limit: queue_bytes,
                snapshots,
                now,
                lease: hub.view_set_lease,
            },
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

    pub(crate) fn reap_expired_view_sets(&self) -> usize {
        let Ok(mut hub) = self.inner.lock() else {
            return 0;
        };
        let before = hub.view_sets.len();
        prune_expired(&mut hub, Instant::now());
        before.saturating_sub(hub.view_sets.len())
    }

    #[cfg(test)]
    pub(crate) fn expire_view_sets_at(&self, now: Instant) {
        if let Ok(mut hub) = self.inner.lock() {
            prune_expired(&mut hub, now);
        }
    }
}

impl ViewSetInner {
    fn new(
        id: String,
        owner: ViewSetOwner,
        initial: ViewSetInitialState,
    ) -> Result<Arc<Self>, ViewSetError> {
        let ViewSetInitialState {
            revision,
            views,
            queue_bytes_limit,
            snapshots,
            now,
            lease,
        } = initial;
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
                last_delivered: BTreeMap::new(),
                in_flight: None,
                queue_high_water: 0,
                reset_count: 0,
                reset_pending: None,
                last_client_activity: now,
                closed: false,
            }),
            notify: Notify::new(),
            polling: AtomicBool::new(false),
            lease,
        });
        inner.install_initial(snapshots, now)?;
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
            lease_millis: self.lease.as_millis().to_string(),
            effective_queue_bytes: state.queue_bytes_limit,
            effective_views: state.views.values().cloned().collect(),
            initial,
        })
    }

    pub(crate) fn is_expired(&self, now: Instant) -> bool {
        self.state.lock().map_or(true, |state| {
            state.closed || now.saturating_duration_since(state.last_client_activity) >= self.lease
        })
    }

    pub(crate) fn touch(&self, now: Instant) -> Result<(), ViewSetError> {
        let mut state = self.state()?;
        if state.closed {
            return Err(ViewSetError::Closed);
        }
        state.last_client_activity = now;
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
        state
            .last_delivered
            .retain(|view_id, _| views.contains_key(view_id));
        state.views = views;
        state.durable_revision = revision;
        state.last_client_activity = now;
        for update in updates {
            enqueue_update(&mut state, update, now)?;
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
        let now = Instant::now();
        let ready_at = state
            .views
            .get(view_id)
            .and_then(|spec| {
                state.last_delivered.get(view_id).map(|last| {
                    *last + Duration::from_millis(u64::from(spec.delivery().min_interval_millis))
                })
            })
            .unwrap_or(now)
            .max(now);
        enqueue_update(
            &mut state,
            ViewSetUpdate::Patch {
                view_id: view_id.to_owned(),
                patch,
            },
            ready_at,
        )?;
        drop(state);
        self.notify.notify_waiters();
        Ok(())
    }

    pub(crate) fn enqueue_snapshot(
        &self,
        view_id: &str,
        snapshot: ViewSnapshot,
        revision: u64,
    ) -> Result<(), ViewSetError> {
        let mut state = self.state()?;
        if state.closed || state.reset_pending.is_some() {
            return Ok(());
        }
        state.durable_revision = revision;
        enqueue_update(
            &mut state,
            ViewSetUpdate::Snapshot {
                view_id: view_id.to_owned(),
                snapshot,
            },
            Instant::now(),
        )?;
        drop(state);
        self.notify.notify_waiters();
        Ok(())
    }

    fn install_initial(
        &self,
        snapshots: Vec<ViewSetUpdate>,
        now: Instant,
    ) -> Result<(), ViewSetError> {
        let mut state = self.state()?;
        for snapshot in &snapshots {
            if let Some(view_id) = snapshot.view_id() {
                state.last_delivered.insert(view_id.to_owned(), now);
            }
        }
        let batch = make_batch(&self.id, &mut state, 0, snapshots)?;
        let encoded_bytes = encoded_batch_len(&batch)?;
        if encoded_bytes > MAX_VIEW_SET_SNAPSHOT_BYTES as usize {
            return Err(ViewSetError::SnapshotExceedsQueue {
                snapshot: encoded_bytes,
                maximum: MAX_VIEW_SET_SNAPSHOT_BYTES,
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
            return Ok(PollState::Wait(None));
        }
        let mut updates = Vec::new();
        let mut retained = VecDeque::with_capacity(state.pending.len());
        let mut next_ready = None::<Instant>;
        while let Some(queued) = state.pending.pop_front() {
            if queued.ready_at <= now {
                state.pending_bytes = state.pending_bytes.saturating_sub(queued.encoded_bytes);
                updates.push(queued.update);
            } else {
                next_ready =
                    Some(next_ready.map_or(queued.ready_at, |ready| ready.min(queued.ready_at)));
                retained.push_back(queued);
            }
        }
        state.pending = retained;
        if updates.is_empty() {
            return Ok(PollState::Wait(next_ready));
        }
        for update in &updates {
            if let Some(view_id) = update.view_id() {
                state.last_delivered.insert(view_id.to_owned(), now);
            }
        }
        let base = state.acknowledged_cursor;
        let batch = make_batch(&self.id, &mut state, base, updates)?;
        let encoded_bytes = encoded_batch_len(&batch)?;
        let batch_limit = if batch_has_snapshot(&batch) {
            MAX_VIEW_SET_SNAPSHOT_BYTES
        } else {
            state.queue_bytes_limit
        };
        if encoded_bytes > batch_limit as usize {
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

    fn empty_batch(&self, after: u64, _now: Instant) -> Result<UpdateBatch, ViewSetError> {
        let state = self.state()?;
        if state.closed {
            return Err(ViewSetError::Closed);
        }
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
        let base_cursor = state.acknowledged_cursor;
        state.durable_revision = revision;
        state.pending.clear();
        state.pending_bytes = 0;
        state.last_delivered.clear();
        state.in_flight = None;
        state.reset_pending = None;
        state.reset_count = state.reset_count.saturating_add(1);
        let mut updates = vec![ViewSetUpdate::ResetRequired {
            view_id: None,
            reason,
        }];
        updates.append(&mut snapshots);
        for update in &updates {
            if let Some(view_id) = update.view_id() {
                state.last_delivered.insert(view_id.to_owned(), now);
            }
        }
        let batch = make_batch(&self.id, &mut state, base_cursor, updates)?;
        let encoded_bytes = encoded_batch_len(&batch)?;
        if encoded_bytes > MAX_VIEW_SET_SNAPSHOT_BYTES as usize {
            return Err(ViewSetError::SnapshotExceedsQueue {
                snapshot: encoded_bytes,
                maximum: MAX_VIEW_SET_SNAPSHOT_BYTES,
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

    fn start_poll(&self) -> Result<ActivePoll<'_>, ViewSetError> {
        self.polling
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| ViewSetError::ConsumerBusy)?;
        Ok(ActivePoll { inner: self })
    }
}

struct ActivePoll<'a> {
    inner: &'a ViewSetInner,
}

impl Drop for ActivePoll<'_> {
    fn drop(&mut self) {
        self.inner.polling.store(false, Ordering::Release);
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

fn enqueue_update(
    state: &mut ViewSetState,
    update: ViewSetUpdate,
    ready_at: Instant,
) -> Result<(), ViewSetError> {
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
            queued.ready_at = queued.ready_at.max(ready_at);
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
        ready_at,
    });
    enforce_bound(state)
}

fn enforce_bound(state: &mut ViewSetState) -> Result<(), ViewSetError> {
    let in_flight = state.in_flight.as_ref().map_or(0, |batch| {
        if batch_has_snapshot(&batch.batch) {
            0
        } else {
            batch.encoded_bytes
        }
    });
    let used = in_flight.saturating_add(state.pending_bytes);
    state.queue_high_water = state.queue_high_water.max(used);
    let pending_snapshot = state
        .pending
        .iter()
        .any(|queued| matches!(queued.update, ViewSetUpdate::Snapshot { .. }));
    let limit = if pending_snapshot {
        MAX_VIEW_SET_SNAPSHOT_BYTES
    } else {
        state.queue_bytes_limit
    };
    if used <= limit as usize {
        return Ok(());
    }
    state.pending.clear();
    state.pending_bytes = 0;
    state.reset_pending = Some(ResetReason::QueueOverflow);
    Ok(())
}

fn batch_has_snapshot(batch: &UpdateBatch) -> bool {
    batch
        .updates
        .iter()
        .any(|update| matches!(update, ViewSetUpdate::Snapshot { .. }))
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
    use crate::diagnostics::category;
    use crate::{
        DiagnosticCategory, DiagnosticEvent, DiagnosticRetention, DiagnosticSeverity,
        FileCatalogState, FileSelectionView, FileView, ProgressAction, ProgressAssessment,
        ProgressDisposition, ProgressPhase, ProgressReason, ServiceSnapshot, StorageState,
        TorrentSnapshot, TorrentState, TorrentView,
    };
    use rstorrent_engine::peer::{PeerSource, PeerSources};
    use rstorrent_engine::swarm::ConnectionId;
    use rstorrent_engine::{
        PeerConnectionDirection, PeerConnectionLifecycle, PeerConnectionObservation,
        PeerConnectionRole, PeerContentActivity, PeerRequestWindowPhase, PeerTransport,
    };

    const TORRENT_ID: &str = "000102030405060708090a0b0c0d0e0f10111213";

    fn torrent_view(id: &str, verified: u32) -> TorrentView {
        TorrentView {
            torrent_id: id.to_owned(),
            display_name: Some("Fixture torrent".to_owned()),
            state: TorrentState::Downloading,
            storage_state: StorageState::Staging,
            metadata_available: true,
            piece_count: 3,
            verified_piece_count: verified,
            requested_bytes: "0".to_owned(),
            received_bytes: "0".to_owned(),
            stored_bytes: "0".to_owned(),
            active_peer_connections: 0,
            configured_tracker_count: Some(2),
            payload_download_rate_bytes: "0".to_owned(),
            progress: ProgressAssessment {
                disposition: ProgressDisposition::Active,
                phase: ProgressPhase::Transfer,
                reason: ProgressReason::TransferringPieces,
                actions: Vec::<ProgressAction>::new(),
            },
            archived: false,
            removal_state: None,
            delete_managed_data_supported: true,
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
                archived: false,
                removal_state: None,
                delete_managed_data_supported: true,
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
            ViewSetInitialState {
                revision: 7,
                views,
                queue_bytes_limit: DEFAULT_VIEW_SET_QUEUE_BYTES,
                snapshots: vec![ViewSetUpdate::Snapshot {
                    view_id: "library".to_owned(),
                    snapshot: ViewSnapshot::TorrentList {
                        torrents: vec![torrent_view("aa", 0)],
                    },
                }],
                now,
                lease: Duration::from_millis(VIEW_SET_LEASE_MILLIS),
            },
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
    fn full_legal_file_snapshot_is_separate_from_steady_queue_pressure() {
        let now = Instant::now();
        let files = (0..4_096_u32)
            .map(|index| FileView {
                file_id: index.to_string(),
                file_index: index,
                path: vec![format!("directory-{index:04}"), "x".repeat(128)],
                length_bytes: "16384".to_owned(),
                torrent_offset_bytes: (u64::from(index) * 16_384).to_string(),
                first_piece: Some(index),
                last_piece: Some(index),
                selection: Some(FileSelectionView::Wanted),
                padding: false,
                done_bytes: "0".to_owned(),
                verified_bytes: "0".to_owned(),
            })
            .collect::<Vec<_>>();
        let snapshot = ViewSnapshot::Files {
            torrent_id: TORRENT_ID.to_owned(),
            state: FileCatalogState::Available,
            filesystem_content_base: Some("/tmp/rstorrent/content".to_owned()),
            files,
        };
        let encoded_snapshot = serde_json::to_vec(&snapshot)
            .expect("encode snapshot")
            .len();
        assert!(encoded_snapshot > DEFAULT_VIEW_SET_QUEUE_BYTES as usize);
        assert!(encoded_snapshot < MAX_VIEW_SET_SNAPSHOT_BYTES as usize);
        let spec = ViewSpec::TorrentFiles {
            view_id: "files".to_owned(),
            torrent_id: TORRENT_ID.to_owned(),
            delivery: ViewDeliveryPolicy::default(),
        };
        let inner = ViewSetInner::new(
            "vs_files".to_owned(),
            ViewSetOwner::trusted("owner"),
            ViewSetInitialState {
                revision: 7,
                views: BTreeMap::from([("files".to_owned(), spec)]),
                queue_bytes_limit: DEFAULT_VIEW_SET_QUEUE_BYTES,
                snapshots: vec![ViewSetUpdate::Snapshot {
                    view_id: "files".to_owned(),
                    snapshot,
                }],
                now,
                lease: Duration::from_millis(VIEW_SET_LEASE_MILLIS),
            },
        )
        .expect("large file view set");
        inner
            .enqueue_patch(
                "files",
                ViewPatch::Files {
                    torrent_id: TORRENT_ID.to_owned(),
                    upsert: vec![FileView {
                        file_id: "0".to_owned(),
                        file_index: 0,
                        path: vec!["updated".to_owned()],
                        length_bytes: "16384".to_owned(),
                        torrent_offset_bytes: "0".to_owned(),
                        first_piece: Some(0),
                        last_piece: Some(0),
                        selection: Some(FileSelectionView::Wanted),
                        padding: false,
                        done_bytes: "16384".to_owned(),
                        verified_bytes: "0".to_owned(),
                    }],
                    removed: Vec::new(),
                },
                8,
            )
            .expect("patch behind in-flight snapshot");
        assert!(inner.state().expect("state").reset_pending.is_none());
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
        let next = match inner.poll_state(1, Instant::now()).expect("poll") {
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
    fn nonzero_delivery_interval_defers_accumulated_patch_without_a_task() {
        let now = Instant::now();
        let delayed = ViewSpec::TorrentList {
            view_id: "library".to_owned(),
            delivery: ViewDeliveryPolicy {
                min_interval_millis: 1_000,
            },
        };
        let inner = ViewSetInner::new(
            "vs_delayed".to_owned(),
            ViewSetOwner::trusted("owner"),
            ViewSetInitialState {
                revision: 7,
                views: BTreeMap::from([("library".to_owned(), delayed)]),
                queue_bytes_limit: DEFAULT_VIEW_SET_QUEUE_BYTES,
                snapshots: vec![ViewSetUpdate::Snapshot {
                    view_id: "library".to_owned(),
                    snapshot: ViewSnapshot::TorrentList {
                        torrents: vec![torrent_view("aa", 0)],
                    },
                }],
                now,
                lease: Duration::from_millis(VIEW_SET_LEASE_MILLIS),
            },
        )
        .expect("view set");
        assert!(matches!(
            inner.poll_state(1, now).expect("acknowledge initial"),
            PollState::Wait(None)
        ));
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
        let ready_at = match inner.poll_state(1, Instant::now()).expect("poll") {
            PollState::Wait(Some(ready_at)) => ready_at,
            _ => panic!("patch should wait for its delivery interval"),
        };
        assert!(matches!(
            inner.poll_state(1, ready_at).expect("poll at deadline"),
            PollState::Ready(_)
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
                        category: DiagnosticCategory::from_static(category::LIFECYCLE_TORRENT),
                        code: "oversized".to_owned(),
                        torrent_id: None,
                        message: "x".repeat(MIN_VIEW_SET_QUEUE_BYTES as usize),
                        subjects: Vec::new(),
                        fields: Vec::new(),
                    }],
                    retention: DiagnosticRetention {
                        source_evicted_count: "0".to_owned(),
                        retained_from_sequence: "1".to_owned(),
                    },
                },
                1,
            )
            .expect("overflow is converted to reset");

        let reset = view_set
            .next_updates(&opened.initial.cursor, 0)
            .await
            .expect("reset batch");
        assert_ne!(reset.epoch, opened.initial.epoch);
        assert!(
            parse_decimal(&reset.cursor).expect("reset cursor")
                > parse_decimal(&opened.initial.cursor).expect("initial cursor")
        );
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
        let waiting_view_set = view_set.clone();
        let waiter =
            tokio::spawn(async move { waiting_view_set.next_updates(&cursor, 20_000).await });
        while !view_set.inner.polling.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            view_set.next_updates(&opened.initial.cursor, 0).await,
            Err(ViewSetError::ConsumerBusy)
        );
        hub.close_view_set(&owner, &opened.view_set_id)
            .expect("close");
        let result = tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .expect("waiter timed out")
            .expect("waiter task");
        assert_eq!(result, Err(ViewSetError::Closed));
    }

    #[tokio::test]
    async fn lease_reaper_closes_silent_set_and_wakes_long_poll() {
        let lease = Duration::from_millis(40);
        let hub = ViewHub::new_with_view_set_lease(&service_snapshot(0, 0), lease).expect("hub");
        let mut reaper = ViewSetLeaseReaper::start(hub.clone(), Duration::from_millis(5));
        let owner = ViewSetOwner::trusted("owner");
        let opened = hub
            .open_view_set(owner.clone(), open_request(vec![spec()]))
            .expect("view set");
        let view_set = hub
            .view_set(&owner, &opened.view_set_id)
            .expect("view set handle");
        let cursor = opened.initial.cursor.clone();
        let waiter = tokio::spawn(async move { view_set.next_updates(&cursor, 20_000).await });

        tokio::time::sleep(Duration::from_millis(20)).await;
        hub.record_diagnostic(
            DiagnosticSeverity::Info,
            category::LIFECYCLE_TORRENT,
            "producer_activity",
            None,
            "Producer publication must not renew a client lease",
            &[],
        )
        .expect("record producer activity");

        let result = tokio::time::timeout(Duration::from_millis(300), waiter)
            .await
            .expect("reaper did not wake waiter")
            .expect("waiter task");
        assert_eq!(result, Err(ViewSetError::Closed));
        assert!(matches!(
            hub.view_set(&owner, &opened.view_set_id),
            Err(ViewSetError::UnknownViewSet)
        ));
        reaper.shutdown().await.expect("join reaper");
    }

    #[tokio::test]
    async fn peer_view_upserts_generations_and_removes_only_on_cleanup() {
        let hub = ViewHub::new(&service_snapshot(0, 0)).expect("hub");
        let owner = ViewSetOwner::trusted("owner");
        let opened = hub
            .open_view_set(
                owner.clone(),
                open_request(vec![ViewSpec::TorrentPeers {
                    view_id: "peers".to_owned(),
                    torrent_id: TORRENT_ID.to_owned(),
                    delivery: ViewDeliveryPolicy::default(),
                }]),
            )
            .expect("view set");
        let view_set = hub
            .view_set(&owner, &opened.view_set_id)
            .expect("view set handle");
        assert!(matches!(
            opened.initial.updates.as_slice(),
            [ViewSetUpdate::Snapshot {
                snapshot: ViewSnapshot::Peers { peers, .. },
                ..
            }] if peers.is_empty()
        ));

        let mut peer = PeerConnectionObservation {
            connection_id: ConnectionId::new(7).expect("connection"),
            record_id: None,
            endpoint: "127.0.0.1:6881".parse().expect("endpoint"),
            sources: PeerSources::from_source(PeerSource::Manual),
            direction: PeerConnectionDirection::Incoming,
            transport: PeerTransport::Utp,
            lifecycle: PeerConnectionLifecycle::TransportConnecting,
            role: PeerConnectionRole::Metadata,
            started_at: Duration::from_millis(5),
            lifecycle_changed_at: Duration::from_millis(5),
            peer_id: None,
            supports_extensions: Some(true),
            content: None,
            close_reason: None,
        };
        hub.record_peer_connections(TORRENT_ID, Duration::from_millis(10), &[peer.clone()])
            .expect("connecting row");
        let connecting = view_set
            .next_updates(&opened.initial.cursor, 0)
            .await
            .expect("connecting patch");
        assert!(matches!(
            connecting.updates.as_slice(),
            [ViewSetUpdate::Patch {
                patch: ViewPatch::Peers { upsert, removed, .. },
                ..
            }] if upsert.len() == 1
                && upsert[0].lifecycle == crate::PeerLifecycle::TransportConnecting
                && upsert[0].client_name.is_none()
                && upsert[0].capabilities.client_name == crate::CapabilityStatus::Unavailable
                && upsert[0].peer_flags == [
                    crate::PeerFlagView::Incoming,
                    crate::PeerFlagView::ExtensionProtocol,
                    crate::PeerFlagView::Utp,
                ]
                && removed.is_empty()
        ));

        peer.lifecycle = PeerConnectionLifecycle::Connected;
        peer.role = PeerConnectionRole::Content;
        peer.lifecycle_changed_at = Duration::from_millis(12);
        peer.peer_id = Some(*b"-UT3550-abcdefghijkl");
        peer.content = Some(PeerContentActivity {
            choking: true,
            wanted_piece_count: 8,
            pending_requests: 2,
            target_requests: 4,
            queued_payload_bytes: 32 * 1_024,
            useful_payload_bytes: 0,
            observed_payload_rate: 0,
            connected_age: Duration::from_millis(3),
            last_useful_age: None,
            last_payload_age: None,
            request_timeout: Duration::from_secs(8),
            oldest_request_age: Some(Duration::from_millis(2)),
            request_window_phase: PeerRequestWindowPhase::SlowStart,
        });
        hub.record_peer_connections(TORRENT_ID, Duration::from_millis(15), &[peer.clone()])
            .expect("connected row");
        let connected = view_set
            .next_updates(&connecting.cursor, 0)
            .await
            .expect("connected patch");
        assert!(matches!(
            connected.updates.as_slice(),
            [ViewSetUpdate::Patch {
                patch: ViewPatch::Peers { upsert, removed, .. },
                ..
            }] if upsert.len() == 1
                && upsert[0].client_name.as_deref() == Some("µTorrent 3.5.5")
                && upsert[0].capabilities.client_name == crate::CapabilityStatus::Available
                && upsert[0].peer_flags == [
                    crate::PeerFlagView::Incoming,
                    crate::PeerFlagView::DownloadChoked,
                    crate::PeerFlagView::ExtensionProtocol,
                    crate::PeerFlagView::Utp,
                ]
                && removed.is_empty()
        ));

        peer.lifecycle = PeerConnectionLifecycle::Disconnecting;
        peer.lifecycle_changed_at = Duration::from_millis(20);
        peer.content.as_mut().expect("content activity").choking = false;
        hub.record_peer_connections(TORRENT_ID, Duration::from_millis(25), &[peer])
            .expect("disconnecting row");
        let disconnecting = view_set
            .next_updates(&connected.cursor, 0)
            .await
            .expect("disconnecting patch");
        assert!(matches!(
            disconnecting.updates.as_slice(),
            [ViewSetUpdate::Patch {
                patch: ViewPatch::Peers { upsert, removed, .. },
                ..
            }] if upsert.len() == 1
                && upsert[0].lifecycle == crate::PeerLifecycle::Disconnecting
                && upsert[0].peer_flags.contains(&crate::PeerFlagView::DownloadAllowed)
                && removed.is_empty()
        ));

        hub.record_peer_connections(TORRENT_ID, Duration::from_millis(30), &[])
            .expect("remove row");
        let removed = view_set
            .next_updates(&disconnecting.cursor, 0)
            .await
            .expect("removal patch");
        assert!(matches!(
            removed.updates.as_slice(),
            [ViewSetUpdate::Patch {
                patch: ViewPatch::Peers { upsert, removed, .. },
                ..
            }] if upsert.is_empty() && removed == &["7"]
        ));
    }

    #[test]
    fn sixty_active_peer_snapshot_stays_inside_default_queue_bound() {
        let hub = ViewHub::new(&service_snapshot(0, 0)).expect("hub");
        let peers = (1..=60)
            .map(|value| {
                let connected = value > 30;
                PeerConnectionObservation {
                    connection_id: ConnectionId::new(value).expect("connection"),
                    record_id: None,
                    endpoint: format!("127.0.0.1:{}", 6_000 + value)
                        .parse()
                        .expect("endpoint"),
                    sources: PeerSources::from_source(PeerSource::Manual),
                    direction: PeerConnectionDirection::Outgoing,
                    transport: PeerTransport::Tcp,
                    lifecycle: if connected {
                        PeerConnectionLifecycle::Connected
                    } else {
                        PeerConnectionLifecycle::TransportConnecting
                    },
                    role: if connected {
                        PeerConnectionRole::Content
                    } else {
                        PeerConnectionRole::Metadata
                    },
                    started_at: Duration::from_secs(1),
                    lifecycle_changed_at: Duration::from_secs(2),
                    peer_id: connected.then_some([value as u8; 20]),
                    supports_extensions: connected.then_some(true),
                    content: connected.then_some(PeerContentActivity {
                        choking: false,
                        wanted_piece_count: 8,
                        pending_requests: 2,
                        target_requests: 4,
                        queued_payload_bytes: 32 * 1_024,
                        useful_payload_bytes: 16 * 1_024,
                        observed_payload_rate: 8 * 1_024,
                        connected_age: Duration::from_secs(3),
                        last_useful_age: Some(Duration::from_millis(20)),
                        last_payload_age: Some(Duration::from_millis(10)),
                        request_timeout: Duration::from_secs(8),
                        oldest_request_age: Some(Duration::from_millis(50)),
                        request_window_phase: PeerRequestWindowPhase::Steady,
                    }),
                    close_reason: None,
                }
            })
            .collect::<Vec<_>>();
        let projection_started = Instant::now();
        hub.record_peer_connections(TORRENT_ID, Duration::from_secs(5), &peers)
            .expect("peer projection");
        let projection_elapsed = projection_started.elapsed();
        let owner = ViewSetOwner::trusted("pressure-owner");
        let snapshot_started = Instant::now();
        let opened = hub
            .open_view_set(
                owner.clone(),
                open_request(vec![ViewSpec::TorrentPeers {
                    view_id: "peers".to_owned(),
                    torrent_id: TORRENT_ID.to_owned(),
                    delivery: ViewDeliveryPolicy::default(),
                }]),
            )
            .expect("peer view set");
        let snapshot_elapsed = snapshot_started.elapsed();
        let encoded_bytes = serde_json::to_vec(&opened.initial)
            .expect("encode snapshot")
            .len();
        let view_set = hub.view_set(&owner, &opened.view_set_id).expect("view set");
        let stats = view_set.stats().expect("view stats");
        assert!(matches!(
            opened.initial.updates.as_slice(),
            [ViewSetUpdate::Snapshot {
                snapshot: ViewSnapshot::Peers { peers, .. },
                ..
            }] if peers.len() == 60
        ));
        assert!(encoded_bytes < DEFAULT_VIEW_SET_QUEUE_BYTES as usize);
        assert!(stats.queue_high_water < DEFAULT_VIEW_SET_QUEUE_BYTES as usize);
        assert_eq!(stats.reset_count, 0);
        eprintln!(
            "peer_view_pressure rows=60 projection_micros={} snapshot_micros={} encoded_bytes={} queue_high_water={} resets={}",
            projection_elapsed.as_micros(),
            snapshot_elapsed.as_micros(),
            encoded_bytes,
            stats.queue_high_water,
            stats.reset_count,
        );
    }
}
