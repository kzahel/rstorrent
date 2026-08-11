//! Hierarchical session and per-torrent peer-transfer quota.

use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use tokio::sync::{Notify, oneshot};
use tokio::task::JoinHandle;

pub const MIN_TRANSFER_RATE_BYTES_PER_SECOND: u32 = 1_024;
pub const MAX_BANDWIDTH_GRANT_BYTES: usize = 16 * 1_024;
pub const MAX_BANDWIDTH_BURST_BYTES: u64 = 1_024 * 1_024;
pub const MAX_BANDWIDTH_REGISTRATIONS: usize = 1_024;
pub const MAX_BANDWIDTH_WAITERS: usize = 4_096;

const CREDIT_SCALE: u128 = 1_000_000_000;
const MAX_ACCRUAL_ELAPSED: Duration = Duration::from_secs(1);
const MAX_GRANTS_PER_PASS: usize = MAX_BANDWIDTH_WAITERS;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TransferRateLimit(Option<NonZeroU32>);

impl TransferRateLimit {
    pub const UNLIMITED: Self = Self(None);

    pub fn limited(bytes_per_second: u32) -> Result<Self, TransferRateLimitError> {
        if bytes_per_second < MIN_TRANSFER_RATE_BYTES_PER_SECOND {
            return Err(TransferRateLimitError(bytes_per_second));
        }
        Ok(Self(NonZeroU32::new(bytes_per_second)))
    }

    pub const fn bytes_per_second(self) -> Option<u32> {
        match self.0 {
            Some(value) => Some(value.get()),
            None => None,
        }
    }

    const fn atomic_value(self) -> u32 {
        match self.0 {
            Some(value) => value.get(),
            None => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferRateLimitError(u32);

impl fmt::Display for TransferRateLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "finite transfer rate {} is below the minimum {} bytes per second",
            self.0, MIN_TRANSFER_RATE_BYTES_PER_SECOND
        )
    }
}

impl Error for TransferRateLimitError {}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TorrentTransferRateLimits {
    pub upload: TransferRateLimit,
    pub download: TransferRateLimit,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BandwidthDirectionSnapshot {
    pub registered_torrents: usize,
    pub waiting_requests: usize,
    pub queued_bytes: u64,
    pub granted_bytes: u64,
    pub returned_bytes: u64,
    pub cancelled_requests: u64,
    pub throttle_wait_micros: u64,
    pub throttle_wait_high_water_micros: u64,
    pub current_burst_credit_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SessionBandwidthSnapshot {
    pub upload: BandwidthDirectionSnapshot,
    pub download: BandwidthDirectionSnapshot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BandwidthError {
    Stopped,
    TorrentClosed,
    RegistrationLimit,
    WaiterLimit,
}

impl fmt::Display for BandwidthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Stopped => "session bandwidth owner stopped",
            Self::TorrentClosed => "torrent bandwidth registration closed",
            Self::RegistrationLimit => "session bandwidth registration limit reached",
            Self::WaiterLimit => "session bandwidth waiter limit reached",
        })
    }
}

impl Error for BandwidthError {}

#[derive(Debug)]
pub struct BandwidthPermit {
    grant: GrantedQuota,
}

impl BandwidthPermit {
    pub const fn bytes(&self) -> usize {
        self.grant.remaining
    }

    pub fn commit(mut self, used: usize) {
        assert!(
            used <= self.grant.remaining,
            "used quota exceeds bandwidth grant"
        );
        self.grant.remaining -= used;
    }
}

#[derive(Debug)]
struct GrantedQuota {
    direction: Option<Arc<DirectionInner>>,
    torrent_id: u64,
    remaining: usize,
}

impl GrantedQuota {
    const fn direct(bytes: usize) -> Self {
        Self {
            direction: None,
            torrent_id: 0,
            remaining: bytes,
        }
    }

    fn disarm(mut self) -> usize {
        self.direction = None;
        let remaining = self.remaining;
        self.remaining = 0;
        remaining
    }

    fn complete(&mut self) {
        if let Some(direction) = &self.direction {
            direction.complete_grant(self.torrent_id, self.remaining);
        }
        self.direction = None;
        self.remaining = 0;
    }
}

impl Drop for GrantedQuota {
    fn drop(&mut self) {
        self.complete();
    }
}

#[derive(Debug)]
pub struct SessionBandwidth {
    upload: DirectionService,
    download: DirectionService,
    next_torrent_id: AtomicU64,
    closed: AtomicBool,
}

#[derive(Clone, Debug)]
pub struct SessionBandwidthHandle {
    upload: Arc<DirectionInner>,
    download: Arc<DirectionInner>,
}

impl SessionBandwidthHandle {
    pub fn set_session_limits(&self, limits: TorrentTransferRateLimits) {
        self.upload.set_session_limit(limits.upload);
        self.download.set_session_limit(limits.download);
    }

    pub fn snapshot(&self) -> SessionBandwidthSnapshot {
        SessionBandwidthSnapshot {
            upload: self.upload.snapshot(),
            download: self.download.snapshot(),
        }
    }
}

impl SessionBandwidth {
    pub fn start(limits: TorrentTransferRateLimits) -> Self {
        Self {
            upload: DirectionService::start(limits.upload),
            download: DirectionService::start(limits.download),
            next_torrent_id: AtomicU64::new(1),
            closed: AtomicBool::new(false),
        }
    }

    pub fn set_session_limits(&self, limits: TorrentTransferRateLimits) {
        self.upload.set_session_limit(limits.upload);
        self.download.set_session_limit(limits.download);
    }

    pub fn handle(&self) -> SessionBandwidthHandle {
        SessionBandwidthHandle {
            upload: self.upload.inner.clone(),
            download: self.download.inner.clone(),
        }
    }

    pub fn register_torrent(
        &self,
        limits: TorrentTransferRateLimits,
    ) -> Result<TorrentBandwidth, BandwidthError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(BandwidthError::Stopped);
        }
        let id = self.next_torrent_id.fetch_add(1, Ordering::Relaxed);
        if id == 0 {
            return Err(BandwidthError::RegistrationLimit);
        }
        self.upload.register(id, limits.upload)?;
        if let Err(error) = self.download.register(id, limits.download) {
            self.upload.unregister(id);
            return Err(error);
        }
        Ok(TorrentBandwidth {
            inner: Arc::new(TorrentBandwidthInner {
                id,
                upload: self.upload.registration(id, limits.upload),
                download: self.download.registration(id, limits.download),
            }),
        })
    }

    pub fn snapshot(&self) -> SessionBandwidthSnapshot {
        SessionBandwidthSnapshot {
            upload: self.upload.snapshot(),
            download: self.download.snapshot(),
        }
    }

    pub async fn shutdown(mut self) -> SessionBandwidthSnapshot {
        self.closed.store(true, Ordering::Release);
        self.upload.stop();
        self.download.stop();
        self.upload.join().await;
        self.download.join().await;
        self.snapshot()
    }
}

impl Drop for SessionBandwidth {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Release);
        self.upload.stop();
        self.download.stop();
    }
}

#[derive(Clone, Debug)]
pub struct TorrentBandwidth {
    inner: Arc<TorrentBandwidthInner>,
}

impl TorrentBandwidth {
    pub fn set_limits(&self, limits: TorrentTransferRateLimits) {
        self.inner.upload.set_limit(limits.upload);
        self.inner.download.set_limit(limits.download);
    }

    pub fn limits(&self) -> TorrentTransferRateLimits {
        TorrentTransferRateLimits {
            upload: self.inner.upload.limit(),
            download: self.inner.download.limit(),
        }
    }

    pub fn upload_limited(&self) -> bool {
        self.inner.upload.is_limited()
    }

    pub fn download_limited(&self) -> bool {
        self.inner.download.is_limited()
    }

    pub async fn acquire_upload(
        &self,
        requested: usize,
    ) -> Result<BandwidthPermit, BandwidthError> {
        self.inner.upload.acquire(requested).await
    }

    pub async fn acquire_download(
        &self,
        requested: usize,
    ) -> Result<BandwidthPermit, BandwidthError> {
        self.inner.download.acquire(requested).await
    }
}

#[derive(Debug)]
struct TorrentBandwidthInner {
    id: u64,
    upload: DirectionRegistration,
    download: DirectionRegistration,
}

impl Drop for TorrentBandwidthInner {
    fn drop(&mut self) {
        self.upload.direction.unregister(self.id);
        self.download.direction.unregister(self.id);
    }
}

#[derive(Debug)]
struct DirectionRegistration {
    direction: Arc<DirectionInner>,
    torrent_id: u64,
    limit: Arc<AtomicU32>,
}

impl DirectionRegistration {
    fn is_limited(&self) -> bool {
        self.direction.session_limit.load(Ordering::Acquire) != 0
            || self.limit.load(Ordering::Acquire) != 0
    }

    fn limit(&self) -> TransferRateLimit {
        let value = self.limit.load(Ordering::Acquire);
        if value == 0 {
            TransferRateLimit::UNLIMITED
        } else {
            TransferRateLimit(NonZeroU32::new(value))
        }
    }

    fn set_limit(&self, limit: TransferRateLimit) {
        self.limit.store(limit.atomic_value(), Ordering::Release);
        self.direction.set_torrent_limit(self.torrent_id, limit);
    }

    async fn acquire(&self, requested: usize) -> Result<BandwidthPermit, BandwidthError> {
        let requested = requested.clamp(1, MAX_BANDWIDTH_GRANT_BYTES);
        if self.direction.stopped.load(Ordering::Acquire) {
            return Err(BandwidthError::Stopped);
        }
        if self.direction.session_limit.load(Ordering::Acquire) == 0
            && self.limit.load(Ordering::Acquire) == 0
        {
            return Ok(BandwidthPermit {
                grant: GrantedQuota::direct(requested),
            });
        }
        self.direction.request(self.torrent_id, requested).await
    }
}

#[derive(Debug)]
struct DirectionService {
    inner: Arc<DirectionInner>,
    task: Option<JoinHandle<()>>,
}

impl DirectionService {
    fn start(limit: TransferRateLimit) -> Self {
        let inner = Arc::new(DirectionInner::new(limit));
        let task_inner = inner.clone();
        let task = tokio::spawn(async move { task_inner.run().await });
        Self {
            inner,
            task: Some(task),
        }
    }

    fn registration(&self, torrent_id: u64, limit: TransferRateLimit) -> DirectionRegistration {
        DirectionRegistration {
            direction: self.inner.clone(),
            torrent_id,
            limit: Arc::new(AtomicU32::new(limit.atomic_value())),
        }
    }

    fn register(&self, torrent_id: u64, limit: TransferRateLimit) -> Result<(), BandwidthError> {
        self.inner.register(torrent_id, limit)
    }

    fn unregister(&self, torrent_id: u64) {
        self.inner.unregister(torrent_id);
    }

    fn set_session_limit(&self, limit: TransferRateLimit) {
        self.inner.set_session_limit(limit);
    }

    fn snapshot(&self) -> BandwidthDirectionSnapshot {
        self.inner.snapshot()
    }

    fn stop(&self) {
        self.inner.stop();
    }

    async fn join(&mut self) {
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

#[derive(Debug)]
struct DirectionInner {
    session_limit: AtomicU32,
    stopped: AtomicBool,
    state: Mutex<DirectionState>,
    notify: Notify,
}

impl DirectionInner {
    fn new(limit: TransferRateLimit) -> Self {
        Self {
            session_limit: AtomicU32::new(limit.atomic_value()),
            stopped: AtomicBool::new(false),
            state: Mutex::new(DirectionState::new(limit)),
            notify: Notify::new(),
        }
    }

    fn state(&self) -> MutexGuard<'_, DirectionState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn register(&self, torrent_id: u64, limit: TransferRateLimit) -> Result<(), BandwidthError> {
        if self.stopped.load(Ordering::Acquire) {
            return Err(BandwidthError::Stopped);
        }
        let mut state = self.state();
        state.advance_to_now();
        if state.torrents.len() >= MAX_BANDWIDTH_REGISTRATIONS {
            return Err(BandwidthError::RegistrationLimit);
        }
        state
            .torrents
            .insert(torrent_id, TorrentDirectionState::new(limit));
        state.snapshot.registered_torrents = state.torrents.len();
        drop(state);
        self.notify.notify_one();
        Ok(())
    }

    fn unregister(&self, torrent_id: u64) {
        let mut state = self.state();
        if let Some(mut torrent) = state.torrents.remove(&torrent_id) {
            torrent.ready = false;
            while let Some(request) = torrent.waiters.pop_front() {
                state.snapshot.waiting_requests = state.snapshot.waiting_requests.saturating_sub(1);
                state.snapshot.queued_bytes = state
                    .snapshot
                    .queued_bytes
                    .saturating_sub(request.requested.try_into().unwrap_or(u64::MAX));
                state.snapshot.cancelled_requests =
                    state.snapshot.cancelled_requests.saturating_add(1);
                let _ = request.response.send(Err(BandwidthError::TorrentClosed));
            }
        }
        state.ready.retain(|ready| *ready != torrent_id);
        state.snapshot.registered_torrents = state.torrents.len();
        drop(state);
        self.notify.notify_one();
    }

    fn set_session_limit(&self, limit: TransferRateLimit) {
        let mut state = self.state();
        state.advance_to_now();
        state.session.set_limit(limit);
        self.session_limit
            .store(limit.atomic_value(), Ordering::Release);
        drop(state);
        self.notify.notify_one();
    }

    fn set_torrent_limit(&self, torrent_id: u64, limit: TransferRateLimit) {
        let mut state = self.state();
        state.advance_to_now();
        if let Some(torrent) = state.torrents.get_mut(&torrent_id) {
            torrent.bucket.set_limit(limit);
        }
        drop(state);
        self.notify.notify_one();
    }

    fn snapshot(&self) -> BandwidthDirectionSnapshot {
        let mut state = self.state();
        state.advance_to_now();
        let mut snapshot = state.snapshot;
        snapshot.current_burst_credit_bytes =
            state.session.limit.bytes_per_second().map_or(0, |_| {
                u64::try_from(state.session.available()).unwrap_or(u64::MAX)
            });
        snapshot
    }

    fn stop(&self) {
        if self.stopped.swap(true, Ordering::AcqRel) {
            return;
        }
        let mut state = self.state();
        state.stop(BandwidthError::Stopped);
        drop(state);
        self.notify.notify_waiters();
    }

    async fn request(
        self: &Arc<Self>,
        torrent_id: u64,
        requested: usize,
    ) -> Result<BandwidthPermit, BandwidthError> {
        if self.stopped.load(Ordering::Acquire) {
            return Err(BandwidthError::Stopped);
        }
        let (sender, receiver) = oneshot::channel();
        let request_id = {
            let mut state = self.state();
            state.advance_to_now();
            state.queue(torrent_id, requested, sender)?
        };
        let mut guard = PendingRequestGuard {
            direction: Arc::downgrade(self),
            torrent_id,
            request_id,
            active: true,
        };
        self.notify.notify_one();
        let grant = receiver.await.map_err(|_| BandwidthError::Stopped)??;
        guard.active = false;
        Ok(BandwidthPermit { grant })
    }

    fn cancel_request(&self, torrent_id: u64, request_id: u64) {
        let mut state = self.state();
        if state.cancel(torrent_id, request_id) {
            drop(state);
            self.notify.notify_one();
        }
    }

    fn complete_grant(&self, torrent_id: u64, bytes: usize) {
        let mut state = self.state();
        state.advance_to_now();
        state.session.refund(bytes);
        if let Some(torrent) = state.torrents.get_mut(&torrent_id) {
            torrent.bucket.refund(bytes);
            torrent.grant_in_flight = false;
            if !torrent.waiters.is_empty() && !torrent.ready {
                torrent.ready = true;
                state.ready.push_back(torrent_id);
            }
        }
        state.snapshot.returned_bytes = state
            .snapshot
            .returned_bytes
            .saturating_add(bytes.try_into().unwrap_or(u64::MAX));
        drop(state);
        self.notify.notify_one();
    }

    async fn run(self: Arc<Self>) {
        loop {
            let notified = self.notify.notified();
            let outcome = {
                let mut state = self.state();
                state.advance_to_now();
                if self.stopped.load(Ordering::Acquire) {
                    return;
                }
                state.allocate(&self)
            };
            match outcome {
                AllocationOutcome::Idle => notified.await,
                AllocationOutcome::Wait(delay) => {
                    tokio::select! {
                        _ = notified => {}
                        _ = tokio::time::sleep(delay) => {}
                    }
                }
                AllocationOutcome::Yield => tokio::task::yield_now().await,
            }
        }
    }
}

#[derive(Debug)]
struct PendingRequestGuard {
    direction: Weak<DirectionInner>,
    torrent_id: u64,
    request_id: u64,
    active: bool,
}

impl Drop for PendingRequestGuard {
    fn drop(&mut self) {
        if self.active
            && let Some(direction) = self.direction.upgrade()
        {
            direction.cancel_request(self.torrent_id, self.request_id);
        }
    }
}

#[derive(Debug)]
struct DirectionState {
    session: TokenBucket,
    torrents: BTreeMap<u64, TorrentDirectionState>,
    ready: VecDeque<u64>,
    next_request_id: u64,
    last_advanced: Instant,
    snapshot: BandwidthDirectionSnapshot,
}

impl DirectionState {
    fn new(limit: TransferRateLimit) -> Self {
        Self {
            session: TokenBucket::new(limit),
            torrents: BTreeMap::new(),
            ready: VecDeque::new(),
            next_request_id: 1,
            last_advanced: Instant::now(),
            snapshot: BandwidthDirectionSnapshot::default(),
        }
    }

    fn advance_to_now(&mut self) {
        let now = Instant::now();
        let elapsed = now
            .saturating_duration_since(self.last_advanced)
            .min(MAX_ACCRUAL_ELAPSED);
        self.last_advanced = now;
        self.advance(elapsed);
    }

    fn advance(&mut self, elapsed: Duration) {
        self.session.advance(elapsed);
        for torrent in self.torrents.values_mut() {
            torrent.bucket.advance(elapsed);
        }
    }

    fn queue(
        &mut self,
        torrent_id: u64,
        requested: usize,
        response: oneshot::Sender<Result<GrantedQuota, BandwidthError>>,
    ) -> Result<u64, BandwidthError> {
        if self.snapshot.waiting_requests >= MAX_BANDWIDTH_WAITERS {
            return Err(BandwidthError::WaiterLimit);
        }
        let Some(torrent) = self.torrents.get_mut(&torrent_id) else {
            return Err(BandwidthError::TorrentClosed);
        };
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        torrent.waiters.push_back(PendingRequest {
            id: request_id,
            requested,
            enqueued: Instant::now(),
            response,
        });
        if !torrent.ready {
            torrent.ready = true;
            self.ready.push_back(torrent_id);
        }
        self.snapshot.waiting_requests += 1;
        self.snapshot.queued_bytes = self
            .snapshot
            .queued_bytes
            .saturating_add(requested.try_into().unwrap_or(u64::MAX));
        Ok(request_id)
    }

    fn cancel(&mut self, torrent_id: u64, request_id: u64) -> bool {
        let Some(torrent) = self.torrents.get_mut(&torrent_id) else {
            return false;
        };
        let Some(index) = torrent
            .waiters
            .iter()
            .position(|request| request.id == request_id)
        else {
            return false;
        };
        let request = torrent
            .waiters
            .remove(index)
            .expect("located bandwidth request exists");
        self.snapshot.waiting_requests = self.snapshot.waiting_requests.saturating_sub(1);
        self.snapshot.queued_bytes = self
            .snapshot
            .queued_bytes
            .saturating_sub(request.requested.try_into().unwrap_or(u64::MAX));
        self.snapshot.cancelled_requests = self.snapshot.cancelled_requests.saturating_add(1);
        if torrent.waiters.is_empty() {
            torrent.ready = false;
            self.ready.retain(|ready| *ready != torrent_id);
        }
        true
    }

    fn stop(&mut self, error: BandwidthError) {
        for torrent in self.torrents.values_mut() {
            while let Some(request) = torrent.waiters.pop_front() {
                let _ = request.response.send(Err(error));
            }
            torrent.ready = false;
        }
        self.ready.clear();
        self.snapshot.cancelled_requests = self.snapshot.cancelled_requests.saturating_add(
            self.snapshot
                .waiting_requests
                .try_into()
                .unwrap_or(u64::MAX),
        );
        self.snapshot.waiting_requests = 0;
        self.snapshot.queued_bytes = 0;
    }

    fn allocate(&mut self, direction: &Arc<DirectionInner>) -> AllocationOutcome {
        let mut grants = 0;
        let mut shortest_wait: Option<Duration> = None;
        loop {
            let round = self.ready.len();
            if round == 0 {
                return if grants == 0 {
                    AllocationOutcome::Idle
                } else {
                    AllocationOutcome::Yield
                };
            }
            let mut progress = false;
            for _ in 0..round {
                let torrent_id = self.ready.pop_front().expect("ready round has a torrent");
                let Some(mut torrent) = self.torrents.remove(&torrent_id) else {
                    continue;
                };
                while torrent
                    .waiters
                    .front()
                    .is_some_and(|request| request.response.is_closed())
                {
                    let request = torrent.waiters.pop_front().expect("closed waiter exists");
                    self.snapshot.waiting_requests =
                        self.snapshot.waiting_requests.saturating_sub(1);
                    self.snapshot.queued_bytes = self
                        .snapshot
                        .queued_bytes
                        .saturating_sub(request.requested.try_into().unwrap_or(u64::MAX));
                    self.snapshot.cancelled_requests =
                        self.snapshot.cancelled_requests.saturating_add(1);
                }
                let Some(request) = torrent.waiters.pop_front() else {
                    torrent.ready = false;
                    self.torrents.insert(torrent_id, torrent);
                    continue;
                };
                debug_assert!(!torrent.grant_in_flight);
                let available = request
                    .requested
                    .min(MAX_BANDWIDTH_GRANT_BYTES)
                    .min(self.session.available())
                    .min(torrent.bucket.available());
                if available == 0 {
                    let session_wait = self.session.time_until_byte();
                    let torrent_wait = torrent.bucket.time_until_byte();
                    let wait = session_wait.max(torrent_wait);
                    shortest_wait = Some(shortest_wait.map_or(wait, |current| current.min(wait)));
                    torrent.waiters.push_front(request);
                    torrent.ready = true;
                    self.ready.push_back(torrent_id);
                    self.torrents.insert(torrent_id, torrent);
                    continue;
                }
                self.session.consume(available);
                torrent.bucket.consume(available);
                self.snapshot.waiting_requests = self.snapshot.waiting_requests.saturating_sub(1);
                self.snapshot.queued_bytes = self
                    .snapshot
                    .queued_bytes
                    .saturating_sub(request.requested.try_into().unwrap_or(u64::MAX));
                self.snapshot.granted_bytes = self
                    .snapshot
                    .granted_bytes
                    .saturating_add(available.try_into().unwrap_or(u64::MAX));
                let waited = request.enqueued.elapsed().as_micros();
                let waited = u64::try_from(waited).unwrap_or(u64::MAX);
                self.snapshot.throttle_wait_micros =
                    self.snapshot.throttle_wait_micros.saturating_add(waited);
                self.snapshot.throttle_wait_high_water_micros =
                    self.snapshot.throttle_wait_high_water_micros.max(waited);
                let quota = GrantedQuota {
                    direction: Some(direction.clone()),
                    torrent_id,
                    remaining: available,
                };
                torrent.grant_in_flight = true;
                torrent.ready = false;
                match request.response.send(Ok(quota)) {
                    Ok(()) => {
                        progress = true;
                        grants += 1;
                    }
                    Err(Ok(quota)) => {
                        let returned = quota.disarm();
                        self.session.refund(returned);
                        torrent.bucket.refund(returned);
                        torrent.grant_in_flight = false;
                        self.snapshot.returned_bytes = self
                            .snapshot
                            .returned_bytes
                            .saturating_add(returned.try_into().unwrap_or(u64::MAX));
                        self.snapshot.cancelled_requests =
                            self.snapshot.cancelled_requests.saturating_add(1);
                        if !torrent.waiters.is_empty() {
                            torrent.ready = true;
                            self.ready.push_back(torrent_id);
                        }
                    }
                    Err(Err(_)) => unreachable!("allocator only sends successful grants"),
                }
                self.torrents.insert(torrent_id, torrent);
                if grants >= MAX_GRANTS_PER_PASS {
                    return AllocationOutcome::Yield;
                }
            }
            if !progress {
                return AllocationOutcome::Wait(
                    shortest_wait.unwrap_or_else(|| Duration::from_millis(1)),
                );
            }
        }
    }
}

#[derive(Debug)]
struct TorrentDirectionState {
    bucket: TokenBucket,
    waiters: VecDeque<PendingRequest>,
    ready: bool,
    grant_in_flight: bool,
}

impl TorrentDirectionState {
    fn new(limit: TransferRateLimit) -> Self {
        Self {
            bucket: TokenBucket::new(limit),
            waiters: VecDeque::new(),
            ready: false,
            grant_in_flight: false,
        }
    }
}

#[derive(Debug)]
struct PendingRequest {
    id: u64,
    requested: usize,
    enqueued: Instant,
    response: oneshot::Sender<Result<GrantedQuota, BandwidthError>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AllocationOutcome {
    Idle,
    Wait(Duration),
    Yield,
}

#[derive(Clone, Copy, Debug)]
struct TokenBucket {
    limit: TransferRateLimit,
    credit: u128,
}

impl TokenBucket {
    const fn new(limit: TransferRateLimit) -> Self {
        Self { limit, credit: 0 }
    }

    fn set_limit(&mut self, limit: TransferRateLimit) {
        self.limit = limit;
        self.credit = self.credit.min(self.capacity_credit());
    }

    fn advance(&mut self, elapsed: Duration) {
        let Some(rate) = self.limit.bytes_per_second() else {
            self.credit = 0;
            return;
        };
        let elapsed = elapsed.min(MAX_ACCRUAL_ELAPSED);
        let nanos = elapsed.as_nanos();
        self.credit = self
            .credit
            .saturating_add(u128::from(rate).saturating_mul(nanos))
            .min(self.capacity_credit());
    }

    fn available(&self) -> usize {
        if self.limit == TransferRateLimit::UNLIMITED {
            usize::MAX
        } else {
            usize::try_from(self.credit / CREDIT_SCALE).unwrap_or(usize::MAX)
        }
    }

    fn consume(&mut self, bytes: usize) {
        if self.limit == TransferRateLimit::UNLIMITED {
            return;
        }
        let scaled = (bytes as u128).saturating_mul(CREDIT_SCALE);
        debug_assert!(self.credit >= scaled);
        self.credit = self.credit.saturating_sub(scaled);
    }

    fn refund(&mut self, bytes: usize) {
        if self.limit == TransferRateLimit::UNLIMITED {
            return;
        }
        self.credit = self
            .credit
            .saturating_add((bytes as u128).saturating_mul(CREDIT_SCALE))
            .min(self.capacity_credit());
    }

    fn time_until_byte(&self) -> Duration {
        let Some(rate) = self.limit.bytes_per_second() else {
            return Duration::ZERO;
        };
        if self.credit >= CREDIT_SCALE {
            return Duration::ZERO;
        }
        let missing = CREDIT_SCALE - self.credit;
        let nanos = missing.div_ceil(u128::from(rate)).max(1);
        Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
    }

    fn capacity_credit(&self) -> u128 {
        let Some(rate) = self.limit.bytes_per_second() else {
            return 0;
        };
        u128::from(u64::from(rate).min(MAX_BANDWIDTH_BURST_BYTES)) * CREDIT_SCALE
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BandwidthError, DirectionInner, DirectionState, MAX_BANDWIDTH_BURST_BYTES,
        MAX_BANDWIDTH_GRANT_BYTES, SessionBandwidth, TorrentTransferRateLimits, TransferRateLimit,
    };
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::oneshot;

    fn limited(bytes_per_second: u32) -> TransferRateLimit {
        TransferRateLimit::limited(bytes_per_second).expect("valid finite rate")
    }

    #[test]
    fn finite_limit_rejects_values_below_product_minimum() {
        assert!(TransferRateLimit::limited(1_023).is_err());
        assert_eq!(limited(1_024).bytes_per_second(), Some(1_024));
        assert_eq!(TransferRateLimit::UNLIMITED.bytes_per_second(), None);
    }

    #[test]
    fn token_credit_is_fractional_bounded_and_clamped_on_decrease() {
        let mut bucket = super::TokenBucket::new(limited(1_024));
        bucket.advance(Duration::from_micros(500));
        assert_eq!(bucket.available(), 0);
        bucket.advance(Duration::from_micros(500));
        assert_eq!(bucket.available(), 1);
        bucket.advance(Duration::from_secs(30));
        assert_eq!(bucket.available(), 1_024);
        bucket.set_limit(limited(2_048));
        bucket.advance(Duration::from_secs(1));
        assert_eq!(bucket.available(), 2_048);
        bucket.set_limit(limited(1_024));
        assert_eq!(bucket.available(), 1_024);

        let mut fast = super::TokenBucket::new(limited(u32::MAX));
        fast.advance(Duration::from_secs(10));
        assert_eq!(fast.available() as u64, MAX_BANDWIDTH_BURST_BYTES);
    }

    fn queue(
        state: &mut DirectionState,
        torrent_id: u64,
        requested: usize,
    ) -> oneshot::Receiver<Result<super::GrantedQuota, BandwidthError>> {
        let (sender, receiver) = oneshot::channel();
        state
            .queue(torrent_id, requested, sender)
            .expect("queue request");
        receiver
    }

    #[test]
    fn hierarchy_intersects_session_and_torrent_quota() {
        let session_limit = limited(4_096);
        let torrent_limit = limited(2_048);
        let direction = Arc::new(DirectionInner::new(session_limit));
        direction.register(1, torrent_limit).expect("register");
        let mut state = direction.state();
        let mut receiver = queue(&mut state, 1, MAX_BANDWIDTH_GRANT_BYTES);
        state.advance(Duration::from_secs(1));
        assert!(matches!(
            state.allocate(&direction),
            super::AllocationOutcome::Yield
        ));
        drop(state);
        let grant = receiver.try_recv().expect("grant").expect("quota");
        assert_eq!(grant.remaining, 2_048);
        drop(grant);
        assert_eq!(direction.snapshot().returned_bytes, 2_048);
    }

    #[test]
    fn ready_torrents_rotate_independently_of_peer_count() {
        let direction = Arc::new(DirectionInner::new(limited(4_096)));
        direction
            .register(1, TransferRateLimit::UNLIMITED)
            .expect("first");
        direction
            .register(2, TransferRateLimit::UNLIMITED)
            .expect("second");
        let mut state = direction.state();
        let mut first = Vec::new();
        for _ in 0..4 {
            first.push(queue(&mut state, 1, 1_024));
        }
        let mut second = queue(&mut state, 2, 1_024);
        state.session.credit = 4_096 * super::CREDIT_SCALE;
        let _ = state.allocate(&direction);
        drop(state);
        let first_grant = first[0].try_recv().expect("first torrent grant").unwrap();
        let second_grant = second.try_recv().expect("second torrent grant").unwrap();
        assert!(
            first[1..]
                .iter_mut()
                .all(|receiver| receiver.try_recv().is_err())
        );

        drop(first_grant);
        drop(second_grant);
        let mut state = direction.state();
        let _ = state.allocate(&direction);
        drop(state);
        assert!(first[1].try_recv().is_ok());
    }

    #[tokio::test]
    async fn unlimited_path_is_immediate_and_register_drop_cleans_state() {
        let session = SessionBandwidth::start(TorrentTransferRateLimits::default());
        let torrent = session
            .register_torrent(TorrentTransferRateLimits::default())
            .expect("register");
        let permit = torrent
            .acquire_download(MAX_BANDWIDTH_GRANT_BYTES * 2)
            .await
            .expect("direct permit");
        assert_eq!(permit.bytes(), MAX_BANDWIDTH_GRANT_BYTES);
        permit.commit(MAX_BANDWIDTH_GRANT_BYTES);
        assert_eq!(session.snapshot().download.waiting_requests, 0);
        drop(torrent);
        assert_eq!(session.snapshot().download.registered_torrents, 0);
        let snapshot = session.shutdown().await;
        assert_eq!(snapshot.download.waiting_requests, 0);
    }

    #[tokio::test]
    async fn dropped_wait_removes_request_promptly() {
        let session = SessionBandwidth::start(TorrentTransferRateLimits {
            upload: limited(1_024),
            download: limited(1_024),
        });
        let torrent = session
            .register_torrent(TorrentTransferRateLimits::default())
            .expect("register");
        let mut wait = Box::pin(torrent.acquire_download(1_024));
        std::future::poll_fn(|context| {
            assert!(std::future::Future::poll(wait.as_mut(), context).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        assert_eq!(session.snapshot().download.waiting_requests, 1);
        drop(wait);
        assert_eq!(session.snapshot().download.waiting_requests, 0);
        assert_eq!(session.snapshot().download.cancelled_requests, 1);
        drop(torrent);
        let _ = session.shutdown().await;
    }

    #[tokio::test]
    async fn live_unlimited_change_wakes_waiter() {
        let session = SessionBandwidth::start(TorrentTransferRateLimits {
            upload: limited(1_024),
            download: limited(1_024),
        });
        let torrent = session
            .register_torrent(TorrentTransferRateLimits::default())
            .expect("register");
        let mut wait = Box::pin(torrent.acquire_download(1_024));
        tokio::select! {
            _ = &mut wait => panic!("new finite bucket unexpectedly had quota"),
            _ = tokio::task::yield_now() => {}
        }
        session.set_session_limits(TorrentTransferRateLimits::default());
        let permit = wait.await.expect("unlimited update grants");
        let granted = permit.bytes();
        assert!(granted > 0);
        permit.commit(granted);
        drop(torrent);
        let _ = session.shutdown().await;
    }

    #[tokio::test]
    async fn shutdown_rejects_new_unlimited_acquisitions() {
        let session = SessionBandwidth::start(TorrentTransferRateLimits::default());
        let torrent = session
            .register_torrent(TorrentTransferRateLimits::default())
            .expect("register");
        let _ = session.shutdown().await;
        assert!(matches!(
            torrent.acquire_upload(1_024).await,
            Err(BandwidthError::Stopped)
        ));
    }
}
