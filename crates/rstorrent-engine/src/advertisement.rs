//! Session-wide tracker discovery and peer-advertisement ownership.

use std::collections::BTreeMap;
use std::fmt;
use std::net::SocketAddrV4;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use rstorrent_protocol::udp_tracker::{AnnounceEvent, AnnounceResponse, MAX_COMPACT_PEERS};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;

use crate::dht::{DhtAnnounceResult, DhtError, DhtHandle, MAX_ACTIVE_LOOKUPS};
use crate::driver::{
    DownloadActivityEvent, DownloadActivitySink, DownloadControl, DownloadError,
    UdpTrackerAnnounce, UdpTrackerExchange, UdpTrackerTiming, UdpTrackerTokenCache,
    announce_udp_tracker, compact_peer_address, random_nonzero_u32,
};
use crate::network::{NetworkConfig, NetworkPolicy};
use crate::peer::{PeerEndpoint, PeerObservation, PeerSource};
use crate::torrent_peer::TorrentPeerHandle;
use crate::tracker::{
    TrackerAction, TrackerId, TrackerSchedule, TrackerWaitKind, UdpTrackerConfig,
};

pub const OUTBOUND_ONLY_TRACKER_PORT: u16 = 1;
pub const UNKNOWN_METADATA_LEFT_BYTES: u64 = 16 * 1024;
pub const MAX_TRACKER_OPERATIONS: usize = 8;
pub const DISCOVERY_ADVERTISEMENT_COMMAND_CAPACITY: usize = 256;
pub const TRACKER_STOP_TIMEOUT: Duration = Duration::from_secs(5);
pub const DHT_LOOKUP_INTERVAL: Duration = Duration::from_secs(60);
pub const DHT_ANNOUNCE_INTERVAL: Duration = Duration::from_secs(15 * 60);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerAdvertisementEndpointScope {
    Loopback,
    LocalNetwork,
    Mapped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerAdvertisementEndpoint {
    pub generation: u64,
    pub endpoint: Option<SocketAddrV4>,
    pub scope: Option<PeerAdvertisementEndpointScope>,
    pub stopping: bool,
}

impl PeerAdvertisementEndpoint {
    pub const fn outbound_only(generation: u64) -> Self {
        Self {
            generation,
            endpoint: None,
            scope: None,
            stopping: false,
        }
    }

    pub const fn stopping(generation: u64) -> Self {
        Self {
            generation,
            endpoint: None,
            scope: None,
            stopping: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TorrentPrivacy {
    Unknown,
    Public,
    Private,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TrackerCounterSnapshot {
    pub downloaded: u64,
    pub uploaded: u64,
    pub left: u64,
}

#[derive(Debug, Default)]
struct TrackerCounterState {
    downloaded: AtomicU64,
    uploaded: AtomicU64,
    left: AtomicU64,
}

#[derive(Clone, Debug, Default)]
pub struct TrackerCounters {
    inner: Arc<TrackerCounterState>,
}

impl TrackerCounters {
    pub fn unknown_metadata() -> Self {
        let counters = Self::default();
        counters.set_left(UNKNOWN_METADATA_LEFT_BYTES);
        counters
    }

    pub fn snapshot(&self) -> TrackerCounterSnapshot {
        TrackerCounterSnapshot {
            downloaded: self.inner.downloaded.load(Ordering::Acquire),
            uploaded: self.inner.uploaded.load(Ordering::Acquire),
            left: self.inner.left.load(Ordering::Acquire),
        }
    }

    pub fn add_downloaded(&self, bytes: u64) {
        atomic_saturating_add(&self.inner.downloaded, bytes);
    }

    pub fn add_uploaded(&self, bytes: u64) {
        atomic_saturating_add(&self.inner.uploaded, bytes);
    }

    pub fn set_left(&self, bytes: u64) {
        self.inner.left.store(bytes, Ordering::Release);
    }
}

fn atomic_saturating_add(value: &AtomicU64, increment: u64) {
    if increment == 0 {
        return;
    }
    let _ = value.fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
        Some(current.saturating_add(increment))
    });
}

#[derive(Clone)]
pub struct DiscoveryAdvertisementRegistration {
    pub generation: u64,
    pub info_hash: [u8; 20],
    pub trackers: Vec<UdpTrackerConfig>,
    pub desired_running: bool,
    pub complete: bool,
    pub incoming_registered: bool,
    pub privacy: TorrentPrivacy,
    pub counters: TrackerCounters,
    pub peers: TorrentPeerHandle,
    pub activity_sink: Arc<dyn DownloadActivitySink>,
}

impl fmt::Debug for DiscoveryAdvertisementRegistration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DiscoveryAdvertisementRegistration")
            .field("generation", &self.generation)
            .field("info_hash", &self.info_hash)
            .field("trackers", &self.trackers)
            .field("desired_running", &self.desired_running)
            .field("complete", &self.complete)
            .field("incoming_registered", &self.incoming_registered)
            .field("privacy", &self.privacy)
            .field("counters", &self.counters)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DiscoveryAdvertisementOwnerCounts {
    pub tasks: usize,
    pub registrations: usize,
    pub tracker_operations: usize,
    pub tracker_operations_high_water: usize,
    pub command_queue_high_water: usize,
    pub dht_operations: usize,
    pub dht_operations_high_water: usize,
}

#[derive(Clone, Debug)]
pub struct DiscoveryAdvertisementHandle {
    sender: mpsc::Sender<Command>,
    queued: Arc<AtomicU64>,
    queue_high_water: Arc<AtomicU64>,
}

impl DiscoveryAdvertisementHandle {
    pub async fn upsert(
        &self,
        registration: DiscoveryAdvertisementRegistration,
    ) -> Result<(), DiscoveryAdvertisementError> {
        self.send(Command::Upsert(registration)).await
    }

    pub async fn remove(
        &self,
        info_hash: [u8; 20],
        generation: u64,
    ) -> Result<(), DiscoveryAdvertisementError> {
        let (sender, receiver) = oneshot::channel();
        self.send(Command::Remove {
            info_hash,
            generation,
            response: sender,
        })
        .await?;
        receiver
            .await
            .map_err(|_| DiscoveryAdvertisementError::OwnerStopped)?
    }

    async fn send(&self, command: Command) -> Result<(), DiscoveryAdvertisementError> {
        let queued = self.queued.fetch_add(1, Ordering::AcqRel).saturating_add(1);
        self.queue_high_water.fetch_max(queued, Ordering::AcqRel);
        let result = self.sender.send(command).await;
        if result.is_err() {
            self.queued.fetch_sub(1, Ordering::AcqRel);
            return Err(DiscoveryAdvertisementError::OwnerStopped);
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct DiscoveryAdvertisementService {
    handle: DiscoveryAdvertisementHandle,
    task:
        Option<JoinHandle<Result<DiscoveryAdvertisementOwnerCounts, DiscoveryAdvertisementError>>>,
}

impl DiscoveryAdvertisementService {
    pub fn start(
        network: NetworkConfig,
        endpoint: watch::Receiver<PeerAdvertisementEndpoint>,
        dht: DhtHandle,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(DISCOVERY_ADVERTISEMENT_COMMAND_CAPACITY);
        let queued = Arc::new(AtomicU64::new(0));
        let queue_high_water = Arc::new(AtomicU64::new(0));
        let handle = DiscoveryAdvertisementHandle {
            sender,
            queued: queued.clone(),
            queue_high_water: queue_high_water.clone(),
        };
        let task = tokio::spawn(run_service(
            network,
            endpoint,
            dht,
            receiver,
            queued,
            queue_high_water,
        ));
        Self {
            handle,
            task: Some(task),
        }
    }

    pub fn handle(&self) -> DiscoveryAdvertisementHandle {
        self.handle.clone()
    }

    pub async fn shutdown(
        mut self,
    ) -> Result<DiscoveryAdvertisementOwnerCounts, DiscoveryAdvertisementError> {
        let (sender, receiver) = oneshot::channel();
        if self.handle.send(Command::Shutdown(sender)).await.is_err() {
            return self
                .task
                .take()
                .expect("advertisement owner exists until shutdown")
                .await
                .map_err(|error| DiscoveryAdvertisementError::Join(error.to_string()))?;
        }
        let response = receiver.await;
        let joined = self
            .task
            .take()
            .expect("advertisement owner exists until shutdown")
            .await
            .map_err(|error| DiscoveryAdvertisementError::Join(error.to_string()))?;
        match response {
            Ok(Ok(())) | Err(_) => joined,
            Ok(Err(error)) => Err(error),
        }
    }
}

impl Drop for DiscoveryAdvertisementService {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

#[derive(Debug)]
pub enum DiscoveryAdvertisementError {
    OwnerStopped,
    Entropy(String),
    Join(String),
}

impl fmt::Display for DiscoveryAdvertisementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OwnerStopped => formatter.write_str("discovery advertisement owner stopped"),
            Self::Entropy(detail) => write!(formatter, "tracker key entropy: {detail}"),
            Self::Join(detail) => write!(formatter, "discovery advertisement owner join: {detail}"),
        }
    }
}

impl std::error::Error for DiscoveryAdvertisementError {}

#[derive(Debug)]
enum Command {
    Upsert(DiscoveryAdvertisementRegistration),
    Remove {
        info_hash: [u8; 20],
        generation: u64,
        response: oneshot::Sender<Result<(), DiscoveryAdvertisementError>>,
    },
    Shutdown(oneshot::Sender<Result<(), DiscoveryAdvertisementError>>),
}

#[derive(Debug)]
struct Removal {
    deadline: Instant,
    response: Option<oneshot::Sender<Result<(), DiscoveryAdvertisementError>>>,
}

#[derive(Debug)]
struct TorrentEntry {
    registration: DiscoveryAdvertisementRegistration,
    schedule: TrackerSchedule,
    tracker_key: u32,
    control: DownloadControl,
    token_caches: BTreeMap<TrackerId, UdpTrackerTokenCache>,
    schedule_epoch: u64,
    last_endpoint_generation: u64,
    removal: Option<Removal>,
    pending_replacement: Option<DiscoveryAdvertisementRegistration>,
    dht_epoch: u64,
    dht_inflight: bool,
    next_dht_action: Instant,
    last_dht_endpoint_generation: u64,
}

impl TorrentEntry {
    fn new(
        registration: DiscoveryAdvertisementRegistration,
    ) -> Result<Self, DiscoveryAdvertisementError> {
        let control = DownloadControl::new();
        control.set_activity_sink(registration.activity_sink.clone());
        let mut scheduled_trackers = registration.trackers.clone();
        shuffle_tracker_configs(&mut scheduled_trackers)
            .map_err(|error| DiscoveryAdvertisementError::Entropy(error.to_string()))?;
        let schedule = TrackerSchedule::from_configs(scheduled_trackers);
        let tracker_key = random_nonzero_u32()
            .map_err(|error| DiscoveryAdvertisementError::Entropy(error.to_string()))?;
        let entry = Self {
            registration,
            schedule,
            tracker_key,
            control,
            token_caches: BTreeMap::new(),
            schedule_epoch: 1,
            last_endpoint_generation: 0,
            removal: None,
            pending_replacement: None,
            dht_epoch: 1,
            dht_inflight: false,
            next_dht_action: Instant::now(),
            last_dht_endpoint_generation: 0,
        };
        entry.emit_snapshot(entry.registration.desired_running);
        Ok(entry)
    }

    fn emit_snapshot(&self, active: bool) {
        self.control
            .emit(DownloadActivityEvent::TrackerState(Box::new(
                self.schedule
                    .snapshot(self.control.diagnostic_elapsed(), active),
            )));
    }

    fn begin_stop(
        &mut self,
        response: Option<oneshot::Sender<Result<(), DiscoveryAdvertisementError>>>,
    ) {
        self.registration.desired_running = false;
        self.dht_epoch = self.dht_epoch.saturating_add(1);
        self.dht_inflight = false;
        self.schedule.request_stop();
        self.removal = Some(Removal {
            deadline: Instant::now() + TRACKER_STOP_TIMEOUT,
            response,
        });
        self.emit_snapshot(true);
    }
}

#[derive(Debug)]
struct RegistrationEffect {
    info_hash: [u8; 20],
    cancel_dht: bool,
    purge_dht_from: Option<TorrentPeerHandle>,
}

#[derive(Debug)]
struct TrackerOperationResult {
    info_hash: [u8; 20],
    registration_generation: u64,
    schedule_epoch: u64,
    endpoint_generation: u64,
    id: TrackerId,
    tracker: String,
    token_cache: UdpTrackerTokenCache,
    result: Result<AnnounceResponse, DownloadError>,
}

#[derive(Debug)]
enum DhtOperationSuccess {
    Lookup(Vec<std::net::SocketAddr>),
    Announce(DhtAnnounceResult),
}

#[derive(Debug)]
struct DhtOperationResult {
    info_hash: [u8; 20],
    registration_generation: u64,
    dht_epoch: u64,
    endpoint_generation: u64,
    announce_port: Option<u16>,
    result: Result<DhtOperationSuccess, DhtError>,
}

async fn run_service(
    network: NetworkConfig,
    mut endpoint_receiver: watch::Receiver<PeerAdvertisementEndpoint>,
    dht: DhtHandle,
    mut receiver: mpsc::Receiver<Command>,
    queued: Arc<AtomicU64>,
    queue_high_water: Arc<AtomicU64>,
) -> Result<DiscoveryAdvertisementOwnerCounts, DiscoveryAdvertisementError> {
    let cancellation = CancellationToken::new();
    let mut endpoint = *endpoint_receiver.borrow_and_update();
    let mut entries = BTreeMap::<[u8; 20], TorrentEntry>::new();
    let mut operations = JoinSet::new();
    let mut dht_operations = JoinSet::new();
    let mut operation_high_water = 0_usize;
    let mut dht_operation_high_water = 0_usize;
    let mut shutting_down = false;
    let mut shutdown_response = None;
    let mut shutdown_deadline = None;

    loop {
        finish_stopped_entries(&mut entries, shutdown_deadline)?;
        if shutting_down
            && (entries.is_empty()
                || shutdown_deadline.is_some_and(|deadline| Instant::now() >= deadline))
        {
            break;
        }

        let tracker_wait =
            fill_tracker_operations(&mut entries, &mut operations, network, endpoint);
        let dht_wait = fill_dht_operations(
            &mut entries,
            &mut dht_operations,
            dht.clone(),
            network.policy,
            endpoint,
        );
        operation_high_water = operation_high_water.max(operations.len());
        dht_operation_high_water = dht_operation_high_water.max(dht_operations.len());
        let now = Instant::now();
        let removal_wait = entries
            .values()
            .filter_map(|entry| entry.removal.as_ref().map(|removal| removal.deadline))
            .chain(shutdown_deadline)
            .map(|deadline| deadline.saturating_duration_since(now))
            .min();
        let next_wait = tracker_wait
            .into_iter()
            .chain(dht_wait)
            .chain(removal_wait)
            .min()
            .unwrap_or(Duration::from_secs(24 * 60 * 60));
        let sleep = tokio::time::sleep(next_wait);
        tokio::pin!(sleep);

        tokio::select! {
            biased;
            command = receiver.recv(), if !shutting_down => {
                let Some(command) = command else {
                    shutting_down = true;
                    begin_session_shutdown(&mut entries, &mut shutdown_deadline);
                    for info_hash in entries.keys().copied().collect::<Vec<_>>() {
                        let _ = dht.cancel_lookup(info_hash).await;
                    }
                    continue;
                };
                queued.fetch_sub(1, Ordering::AcqRel);
                match command {
                    Command::Upsert(registration) => {
                        let effect = apply_registration(&mut entries, registration)?;
                        if effect.cancel_dht {
                            let _ = dht.cancel_lookup(effect.info_hash).await;
                        }
                        if let Some(peers) = effect.purge_dht_from {
                            let _ = peers.remove_discovery_source(PeerSource::Dht);
                            if let Some(entry) = entries.get(&effect.info_hash) {
                                entry.control.emit(DownloadActivityEvent::DhtDisabledForPrivateTorrent);
                            }
                        }
                    }
                    Command::Remove { info_hash, generation, response } => {
                        match entries.get_mut(&info_hash) {
                            Some(entry) if entry.registration.generation == generation => {
                                entry.begin_stop(Some(response));
                                let _ = dht.cancel_lookup(info_hash).await;
                            }
                            _ => {
                                let _ = response.send(Ok(()));
                            }
                        }
                    }
                    Command::Shutdown(response) => {
                        shutting_down = true;
                        shutdown_response = Some(response);
                        begin_session_shutdown(&mut entries, &mut shutdown_deadline);
                        for info_hash in entries.keys().copied().collect::<Vec<_>>() {
                            let _ = dht.cancel_lookup(info_hash).await;
                        }
                    }
                }
            }
            changed = endpoint_receiver.changed() => {
                if changed.is_ok() {
                    endpoint = *endpoint_receiver.borrow_and_update();
                    for entry in entries.values_mut() {
                        if entry.registration.incoming_registered
                            && entry.last_endpoint_generation != endpoint.generation
                            && entry.removal.is_none()
                        {
                            entry.schedule.request_update();
                        }
                    }
                    let corrected = entries
                        .iter_mut()
                        .filter_map(|(info_hash, entry)| {
                            dht_announce_port(network.policy, endpoint, &entry.registration)?;
                            if entry.last_dht_endpoint_generation == endpoint.generation {
                                return None;
                            }
                            entry.dht_epoch = entry.dht_epoch.saturating_add(1);
                            entry.dht_inflight = false;
                            entry.next_dht_action = Instant::now();
                            Some(*info_hash)
                        })
                        .collect::<Vec<_>>();
                    for info_hash in corrected {
                        let _ = dht.cancel_lookup(info_hash).await;
                    }
                }
            }
            joined = operations.join_next(), if !operations.is_empty() => {
                if let Some(Ok(operation)) = joined {
                    apply_tracker_result(&mut entries, operation, network.policy, endpoint);
                }
            }
            joined = dht_operations.join_next(), if !dht_operations.is_empty() => {
                if let Some(Ok(operation)) = joined {
                    apply_dht_result(&mut entries, operation, network.policy, endpoint);
                }
            }
            _ = &mut sleep => {}
        }
    }

    cancellation.cancel();
    operations.abort_all();
    while operations.join_next().await.is_some() {}
    dht_operations.abort_all();
    while dht_operations.join_next().await.is_some() {}
    if let Some(response) = shutdown_response {
        let _ = response.send(Ok(()));
    }
    Ok(DiscoveryAdvertisementOwnerCounts {
        tasks: 0,
        registrations: entries.len(),
        tracker_operations: operations.len(),
        tracker_operations_high_water: operation_high_water,
        command_queue_high_water: queue_high_water
            .load(Ordering::Acquire)
            .try_into()
            .unwrap_or(usize::MAX),
        dht_operations: dht_operations.len(),
        dht_operations_high_water: dht_operation_high_water,
    })
}

fn apply_registration(
    entries: &mut BTreeMap<[u8; 20], TorrentEntry>,
    registration: DiscoveryAdvertisementRegistration,
) -> Result<RegistrationEffect, DiscoveryAdvertisementError> {
    let info_hash = registration.info_hash;
    let private_peers =
        (registration.privacy == TorrentPrivacy::Private).then(|| registration.peers.clone());
    let Some(entry) = entries.get_mut(&registration.info_hash) else {
        entries.insert(registration.info_hash, TorrentEntry::new(registration)?);
        return Ok(RegistrationEffect {
            info_hash,
            cancel_dht: false,
            purge_dht_from: private_peers,
        });
    };
    if registration.generation < entry.registration.generation {
        return Ok(RegistrationEffect {
            info_hash,
            cancel_dht: false,
            purge_dht_from: None,
        });
    }
    if let Some(pending) = entry.pending_replacement.as_mut() {
        if registration.generation >= pending.generation {
            *pending = registration;
        }
        return Ok(RegistrationEffect {
            info_hash,
            cancel_dht: true,
            purge_dht_from: private_peers,
        });
    }
    if registration.generation > entry.registration.generation
        || registration.trackers != entry.registration.trackers
    {
        entry.pending_replacement = Some(registration);
        if entry.removal.is_none() {
            entry.begin_stop(None);
        }
        return Ok(RegistrationEffect {
            info_hash,
            cancel_dht: true,
            purge_dht_from: private_peers,
        });
    }

    let completed = !entry.registration.complete && registration.complete;
    let incoming_changed =
        entry.registration.incoming_registered != registration.incoming_registered;
    let resume = !entry.registration.desired_running && registration.desired_running;
    let dht_changed = entry.registration.privacy != registration.privacy
        || entry.registration.desired_running != registration.desired_running
        || entry.registration.complete != registration.complete
        || incoming_changed;
    entry.registration = registration;
    entry
        .control
        .set_activity_sink(entry.registration.activity_sink.clone());
    if resume {
        entry.schedule = TrackerSchedule::from_configs(entry.registration.trackers.clone());
        entry.token_caches.clear();
        entry.schedule_epoch = entry.schedule_epoch.saturating_add(1);
        entry.removal = None;
    }
    if incoming_changed {
        entry.schedule.request_update();
    }
    if completed && entry.registration.incoming_registered {
        entry.schedule.request_completed();
    }
    if !entry.registration.desired_running && entry.removal.is_none() {
        entry.begin_stop(None);
    }
    if dht_changed {
        entry.dht_epoch = entry.dht_epoch.saturating_add(1);
        entry.dht_inflight = false;
        entry.next_dht_action = Instant::now();
    }
    entry.emit_snapshot(entry.registration.desired_running || entry.removal.is_some());
    Ok(RegistrationEffect {
        info_hash,
        cancel_dht: dht_changed,
        purge_dht_from: private_peers,
    })
}

fn begin_session_shutdown(
    entries: &mut BTreeMap<[u8; 20], TorrentEntry>,
    shutdown_deadline: &mut Option<Instant>,
) {
    *shutdown_deadline = Some(Instant::now() + TRACKER_STOP_TIMEOUT);
    for entry in entries.values_mut() {
        entry.begin_stop(None);
    }
}

fn finish_stopped_entries(
    entries: &mut BTreeMap<[u8; 20], TorrentEntry>,
    shutdown_deadline: Option<Instant>,
) -> Result<(), DiscoveryAdvertisementError> {
    let now = Instant::now();
    let finished = entries
        .iter_mut()
        .filter_map(|(info_hash, entry)| {
            let removal = entry.removal.as_ref()?;
            let exhausted = entry.schedule.stop_complete();
            (exhausted
                || now >= removal.deadline
                || shutdown_deadline.is_some_and(|deadline| now >= deadline))
            .then_some(*info_hash)
        })
        .collect::<Vec<_>>();
    for info_hash in finished {
        if let Some(mut entry) = entries.remove(&info_hash) {
            entry.emit_snapshot(false);
            if let Some(response) = entry.removal.take().and_then(|removal| removal.response) {
                let _ = response.send(Ok(()));
            }
            if shutdown_deadline.is_none()
                && let Some(replacement) = entry.pending_replacement.take()
            {
                entries.insert(info_hash, TorrentEntry::new(replacement)?);
            }
        }
    }
    if shutdown_deadline.is_some_and(|deadline| now >= deadline) {
        entries.clear();
    }
    Ok(())
}

fn fill_tracker_operations(
    entries: &mut BTreeMap<[u8; 20], TorrentEntry>,
    operations: &mut JoinSet<TrackerOperationResult>,
    network: NetworkConfig,
    endpoint: PeerAdvertisementEndpoint,
) -> Option<Duration> {
    let mut minimum_wait = None;
    loop {
        let mut spawned = false;
        for entry in entries.values_mut() {
            if operations.len() >= MAX_TRACKER_OPERATIONS {
                return minimum_wait;
            }
            if !entry.registration.desired_running && entry.removal.is_none() {
                continue;
            }
            match entry
                .schedule
                .next_action(entry.control.diagnostic_elapsed())
            {
                TrackerAction::Announce {
                    id,
                    url,
                    tier,
                    event,
                    attempt,
                    fallback,
                    ..
                } => {
                    let tracker = tracker_label(&url);
                    if fallback {
                        entry
                            .control
                            .emit(DownloadActivityEvent::TrackerFallbackSelected {
                                tracker: tracker.clone(),
                                tier,
                            });
                    }
                    entry
                        .control
                        .emit(DownloadActivityEvent::TrackerAnnounceStarted {
                            tracker: tracker.clone(),
                            tier,
                            attempt,
                            event,
                        });
                    entry.emit_snapshot(true);
                    let counters = entry.registration.counters.snapshot();
                    let port = tracker_port(
                        network.policy,
                        endpoint,
                        entry.registration.incoming_registered,
                    );
                    let num_want = if event == AnnounceEvent::Stopped {
                        0
                    } else {
                        MAX_COMPACT_PEERS as i32
                    };
                    let mut token_cache = entry.token_caches.remove(&id).unwrap_or_default();
                    let control = entry.control.clone();
                    let info_hash = entry.registration.info_hash;
                    let registration_generation = entry.registration.generation;
                    let schedule_epoch = entry.schedule_epoch;
                    let tracker_key = entry.tracker_key;
                    operations.spawn(async move {
                        let result = announce_udp_tracker(
                            &url,
                            network.policy,
                            &mut token_cache,
                            UdpTrackerAnnounce {
                                info_hash,
                                peer_id: network.peer_id,
                                key: tracker_key,
                                downloaded: counters.downloaded,
                                left: counters.left,
                                uploaded: counters.uploaded,
                                event,
                                num_want,
                                port,
                            },
                            UdpTrackerExchange {
                                timing: UdpTrackerTiming::PRODUCTION,
                                control: &control,
                                tracker_label: &tracker,
                            },
                        )
                        .await;
                        TrackerOperationResult {
                            info_hash,
                            registration_generation,
                            schedule_epoch,
                            endpoint_generation: endpoint.generation,
                            id,
                            tracker,
                            token_cache,
                            result,
                        }
                    });
                    spawned = true;
                }
                TrackerAction::Wait { delay, url, kind } => {
                    let tracker = tracker_label(&url);
                    match kind {
                        TrackerWaitKind::FailureRetry => {
                            entry
                                .control
                                .emit(DownloadActivityEvent::TrackerRetryScheduled {
                                    tracker,
                                    retry_in_seconds: delay.as_secs(),
                                })
                        }
                        TrackerWaitKind::Reannounce => {
                            entry
                                .control
                                .emit(DownloadActivityEvent::TrackerReannounceScheduled {
                                    tracker,
                                    announce_in_seconds: delay.as_secs(),
                                })
                        }
                    }
                    minimum_wait =
                        Some(minimum_wait.map_or(delay, |current: Duration| current.min(delay)));
                }
                TrackerAction::Pending | TrackerAction::Exhausted => {}
            }
        }
        if !spawned || operations.len() >= MAX_TRACKER_OPERATIONS {
            return minimum_wait;
        }
    }
}

fn fill_dht_operations(
    entries: &mut BTreeMap<[u8; 20], TorrentEntry>,
    operations: &mut JoinSet<DhtOperationResult>,
    dht: DhtHandle,
    policy: NetworkPolicy,
    endpoint: PeerAdvertisementEndpoint,
) -> Option<Duration> {
    let now = Instant::now();
    let mut minimum_wait = None;
    for entry in entries.values_mut() {
        if operations.len() >= MAX_ACTIVE_LOOKUPS {
            break;
        }
        if entry.removal.is_some()
            || !entry.registration.desired_running
            || entry.registration.privacy == TorrentPrivacy::Private
            || entry.dht_inflight
        {
            continue;
        }
        if entry.next_dht_action > now {
            let wait = entry.next_dht_action.saturating_duration_since(now);
            minimum_wait = Some(minimum_wait.map_or(wait, |current: Duration| current.min(wait)));
            continue;
        }

        let announce_port = dht_announce_port(policy, endpoint, &entry.registration);
        let info_hash = entry.registration.info_hash;
        let registration_generation = entry.registration.generation;
        let dht_epoch = entry.dht_epoch;
        let endpoint_generation = endpoint.generation;
        let operation_dht = dht.clone();
        entry.dht_inflight = true;
        entry.control.emit(DownloadActivityEvent::DhtLookupStarted);
        operations.spawn(async move {
            let result = match announce_port {
                Some(port) => operation_dht
                    .lookup_and_announce(info_hash, port)
                    .await
                    .map(DhtOperationSuccess::Announce),
                None => operation_dht
                    .lookup(info_hash)
                    .await
                    .map(DhtOperationSuccess::Lookup),
            };
            DhtOperationResult {
                info_hash,
                registration_generation,
                dht_epoch,
                endpoint_generation,
                announce_port,
                result,
            }
        });
    }
    minimum_wait
}

fn apply_dht_result(
    entries: &mut BTreeMap<[u8; 20], TorrentEntry>,
    operation: DhtOperationResult,
    policy: NetworkPolicy,
    endpoint: PeerAdvertisementEndpoint,
) {
    let Some(entry) = entries.get_mut(&operation.info_hash) else {
        return;
    };
    if entry.registration.generation != operation.registration_generation
        || entry.dht_epoch != operation.dht_epoch
    {
        return;
    }
    entry.dht_inflight = false;
    if operation.announce_port.is_some()
        && (operation.endpoint_generation != endpoint.generation
            || operation.announce_port != dht_announce_port(policy, endpoint, &entry.registration))
    {
        entry.next_dht_action = Instant::now();
        return;
    }

    let now = Instant::now();
    match operation.result {
        Ok(success) => {
            let (peers, interval) = match success {
                DhtOperationSuccess::Lookup(peers) => (peers, DHT_LOOKUP_INTERVAL),
                DhtOperationSuccess::Announce(report) => {
                    entry.last_dht_endpoint_generation = operation.endpoint_generation;
                    entry
                        .control
                        .emit(DownloadActivityEvent::DhtAnnounceCompleted {
                            port: operation
                                .announce_port
                                .expect("announce result retains its explicit port"),
                            token_nodes: report.token_nodes,
                            announces_sent: report.announces_sent,
                            announces_succeeded: report.announces_succeeded,
                            announces_failed: report.announces_failed,
                        });
                    (report.peers, DHT_ANNOUNCE_INTERVAL)
                }
            };
            let peer_count = peers.len().try_into().unwrap_or(u32::MAX);
            for address in peers {
                if !policy.allows(address) {
                    continue;
                }
                let Ok(endpoint) = PeerEndpoint::new(address) else {
                    continue;
                };
                let _ = entry
                    .registration
                    .peers
                    .observe_discovered_peer(PeerObservation::dialable(endpoint, PeerSource::Dht));
            }
            entry
                .control
                .emit(DownloadActivityEvent::DhtLookupSucceeded { peer_count });
            entry.next_dht_action = now + interval;
        }
        Err(DhtError::Cancelled) => {
            entry.next_dht_action = now;
        }
        Err(error) => {
            entry.control.emit(DownloadActivityEvent::DhtLookupFailed {
                detail: error.to_string(),
            });
            entry
                .control
                .emit(DownloadActivityEvent::DhtRetryScheduled {
                    retry_in_seconds: DHT_LOOKUP_INTERVAL.as_secs(),
                });
            entry.next_dht_action = now + DHT_LOOKUP_INTERVAL;
        }
    }
}

fn dht_announce_port(
    policy: NetworkPolicy,
    endpoint: PeerAdvertisementEndpoint,
    registration: &DiscoveryAdvertisementRegistration,
) -> Option<u16> {
    if !registration.desired_running
        || !registration.complete
        || !registration.incoming_registered
        || registration.privacy != TorrentPrivacy::Public
        || endpoint.stopping
    {
        return None;
    }
    match (endpoint.endpoint, endpoint.scope) {
        (
            Some(endpoint),
            Some(
                PeerAdvertisementEndpointScope::Mapped
                | PeerAdvertisementEndpointScope::LocalNetwork,
            ),
        ) => Some(endpoint.port()),
        (Some(endpoint), Some(PeerAdvertisementEndpointScope::Loopback))
            if policy == NetworkPolicy::LoopbackOnly =>
        {
            Some(endpoint.port())
        }
        _ => None,
    }
}

fn apply_tracker_result(
    entries: &mut BTreeMap<[u8; 20], TorrentEntry>,
    operation: TrackerOperationResult,
    policy: NetworkPolicy,
    endpoint: PeerAdvertisementEndpoint,
) {
    let Some(entry) = entries.get_mut(&operation.info_hash) else {
        return;
    };
    if entry.registration.generation != operation.registration_generation
        || entry.schedule_epoch != operation.schedule_epoch
    {
        return;
    }
    entry
        .token_caches
        .insert(operation.id, operation.token_cache);
    let now = entry.control.diagnostic_elapsed();
    if operation.endpoint_generation != endpoint.generation && entry.removal.is_none() {
        entry.schedule.supersede(operation.id);
        entry.emit_snapshot(true);
        return;
    }
    match operation.result {
        Ok(response) => {
            let peer_count = response.peers.len().try_into().unwrap_or(u32::MAX);
            let success = entry.schedule.succeeded(
                operation.id,
                now,
                response.interval,
                peer_count,
                response.seeders,
                response.leechers,
            );
            entry.last_endpoint_generation = operation.endpoint_generation;
            entry
                .control
                .emit(DownloadActivityEvent::TrackerAnnounceSucceeded {
                    tracker: operation.tracker,
                    peer_count,
                    interval_seconds: success.interval.as_secs(),
                });
            for peer in response.peers {
                let address = compact_peer_address(peer);
                if !policy.allows(address) {
                    continue;
                }
                let Ok(endpoint) = PeerEndpoint::new(address) else {
                    continue;
                };
                let _ = entry
                    .peers()
                    .observe_discovered_peer(PeerObservation::dialable(
                        endpoint,
                        PeerSource::Tracker,
                    ));
            }
        }
        Err(error) => {
            let detail = error.to_string();
            let failure = entry.schedule.failed(operation.id, now, &detail);
            entry
                .control
                .emit(DownloadActivityEvent::TrackerAnnounceFailed {
                    tracker: operation.tracker,
                    failures: failure.failures,
                    retry_in_seconds: failure.retry_in.as_secs(),
                    detail,
                });
        }
    }
    entry.emit_snapshot(true);
}

impl TorrentEntry {
    fn peers(&self) -> &TorrentPeerHandle {
        &self.registration.peers
    }
}

fn tracker_port(
    policy: NetworkPolicy,
    endpoint: PeerAdvertisementEndpoint,
    incoming_registered: bool,
) -> u16 {
    if !incoming_registered || endpoint.stopping {
        return OUTBOUND_ONLY_TRACKER_PORT;
    }
    match (endpoint.endpoint, endpoint.scope) {
        (
            Some(endpoint),
            Some(
                PeerAdvertisementEndpointScope::Mapped
                | PeerAdvertisementEndpointScope::LocalNetwork,
            ),
        ) => endpoint.port(),
        (Some(endpoint), Some(PeerAdvertisementEndpointScope::Loopback))
            if policy == NetworkPolicy::LoopbackOnly =>
        {
            endpoint.port()
        }
        _ => OUTBOUND_ONLY_TRACKER_PORT,
    }
}

fn shuffle_tracker_configs(trackers: &mut [UdpTrackerConfig]) -> Result<(), DownloadError> {
    let mut first = 0;
    while first < trackers.len() {
        let tier = trackers[first].tier;
        let mut end = first + 1;
        while end < trackers.len() && trackers[end].tier == tier {
            end += 1;
        }
        for last in (1..trackers[first..end].len()).rev() {
            let selected =
                usize::try_from(random_nonzero_u32()?).unwrap_or(usize::MAX) % (last + 1);
            trackers[first..end].swap(last, selected);
        }
        first = end;
    }
    Ok(())
}

fn tracker_label(tracker: &rstorrent_protocol::magnet::UdpTrackerUrl) -> String {
    if tracker.host.contains(':') {
        format!("udp://[{}]:{}", tracker.host, tracker.port)
    } else {
        format!("udp://{}:{}", tracker.host, tracker.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peer::PeerRegistrySnapshot;
    use crate::{PeerConnectionObservation, TorrentPeerActivitySink};
    use std::sync::Mutex;
    use tokio::net::UdpSocket;
    use tokio::sync::Notify;

    #[derive(Debug, Default)]
    struct NoopSink;

    impl DownloadActivitySink for NoopSink {
        fn record(&self, _event: DownloadActivityEvent) {}
    }

    impl TorrentPeerActivitySink for NoopSink {
        fn record_peer_connections(
            &self,
            _captured_at: Duration,
            _peers: Vec<PeerConnectionObservation>,
        ) {
        }

        fn record_peer_registry(&self, _active: bool, _snapshot: PeerRegistrySnapshot) {}
    }

    #[derive(Debug, Default)]
    struct RecordingActivity {
        successes: Mutex<usize>,
        dht_announces: Mutex<Vec<DhtAnnounceReport>>,
        changed: Notify,
    }

    type DhtAnnounceReport = (u16, u8, u8, u8, u8);

    impl RecordingActivity {
        async fn wait_for_successes(&self, expected: usize) {
            loop {
                if *self
                    .successes
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    >= expected
                {
                    return;
                }
                self.changed.notified().await;
            }
        }

        async fn wait_for_dht_announces(&self, expected: usize) -> DhtAnnounceReport {
            loop {
                {
                    let reports = self
                        .dht_announces
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    if reports.len() >= expected {
                        return *reports.last().expect("nonempty reports");
                    }
                }
                self.changed.notified().await;
            }
        }
    }

    impl DownloadActivitySink for RecordingActivity {
        fn record(&self, event: DownloadActivityEvent) {
            if matches!(
                event,
                DownloadActivityEvent::TrackerAnnounceSucceeded { .. }
            ) {
                let mut successes = self
                    .successes
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *successes += 1;
                drop(successes);
                self.changed.notify_waiters();
            }
            if let DownloadActivityEvent::DhtAnnounceCompleted {
                port,
                token_nodes,
                announces_sent,
                announces_succeeded,
                announces_failed,
            } = event
            {
                self.dht_announces
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push((
                        port,
                        token_nodes,
                        announces_sent,
                        announces_succeeded,
                        announces_failed,
                    ));
                self.changed.notify_waiters();
            }
        }
    }

    #[test]
    fn tracker_port_requires_matching_incoming_authority() {
        let endpoint = PeerAdvertisementEndpoint {
            generation: 4,
            endpoint: Some("192.168.1.2:42000".parse().expect("endpoint")),
            scope: Some(PeerAdvertisementEndpointScope::LocalNetwork),
            stopping: false,
        };
        assert_eq!(tracker_port(NetworkPolicy::Online, endpoint, false), 1);
        assert_eq!(tracker_port(NetworkPolicy::Online, endpoint, true), 42_000);
        assert_eq!(
            tracker_port(
                NetworkPolicy::Online,
                PeerAdvertisementEndpoint::stopping(5),
                true,
            ),
            1
        );
    }

    #[test]
    fn loopback_listener_does_not_leak_into_online_advertisement() {
        let endpoint = PeerAdvertisementEndpoint {
            generation: 2,
            endpoint: Some("127.0.0.1:43000".parse().expect("endpoint")),
            scope: Some(PeerAdvertisementEndpointScope::Loopback),
            stopping: false,
        };
        assert_eq!(tracker_port(NetworkPolicy::Online, endpoint, true), 1);
        assert_eq!(
            tracker_port(NetworkPolicy::LoopbackOnly, endpoint, true),
            43_000
        );
    }

    #[test]
    fn dht_announcement_requires_verified_public_routable_seed() {
        let endpoint = PeerAdvertisementEndpoint {
            generation: 2,
            endpoint: Some("127.0.0.1:43000".parse().expect("endpoint")),
            scope: Some(PeerAdvertisementEndpointScope::Loopback),
            stopping: false,
        };
        let mut registration = test_registration(1, 41001);
        assert_eq!(
            dht_announce_port(NetworkPolicy::LoopbackOnly, endpoint, &registration),
            None
        );
        registration.complete = true;
        registration.incoming_registered = true;
        registration.privacy = TorrentPrivacy::Public;
        assert_eq!(
            dht_announce_port(NetworkPolicy::LoopbackOnly, endpoint, &registration),
            Some(43_000)
        );
        assert_eq!(
            dht_announce_port(NetworkPolicy::Online, endpoint, &registration),
            None
        );
        registration.privacy = TorrentPrivacy::Private;
        assert_eq!(
            dht_announce_port(NetworkPolicy::LoopbackOnly, endpoint, &registration),
            None
        );
    }

    #[test]
    fn verified_private_transition_cancels_and_purges_dht_only_peers() {
        let peers = TorrentPeerHandle::new(Arc::new(NoopSink)).expect("peer registry");
        peers
            .observe_discovered_peer(PeerObservation::dialable(
                PeerEndpoint::new("127.0.0.1:44001".parse().expect("peer endpoint"))
                    .expect("valid peer"),
                PeerSource::Dht,
            ))
            .expect("observe DHT peer");
        let mut registration = test_registration(1, 41001);
        registration.peers = peers.clone();
        registration.privacy = TorrentPrivacy::Unknown;
        let mut entries = BTreeMap::new();
        apply_registration(&mut entries, registration.clone()).expect("unknown registration");

        registration.privacy = TorrentPrivacy::Private;
        let effect = apply_registration(&mut entries, registration).expect("private transition");
        assert!(effect.cancel_dht);
        effect
            .purge_dht_from
            .expect("private transition supplies purge owner")
            .remove_discovery_source(PeerSource::Dht)
            .expect("purge DHT source");
        assert!(peers.registry_snapshot(true).records.is_empty());
    }

    #[test]
    fn transfer_counters_saturate_and_left_is_current() {
        let counters = TrackerCounters::unknown_metadata();
        counters.add_downloaded(u64::MAX - 1);
        counters.add_downloaded(10);
        counters.add_uploaded(7);
        counters.set_left(0);
        assert_eq!(
            counters.snapshot(),
            TrackerCounterSnapshot {
                downloaded: u64::MAX,
                uploaded: 7,
                left: 0,
            }
        );
    }

    #[test]
    fn registration_replacement_stops_old_tracker_rows_before_installing_new() {
        let mut entries = BTreeMap::new();
        apply_registration(&mut entries, test_registration(3, 41001)).expect("first registration");
        apply_registration(&mut entries, test_registration(4, 41002))
            .expect("replacement registration");

        let stopping = entries.get(&[4; 20]).expect("old entry remains installed");
        assert_eq!(stopping.registration.generation, 3);
        assert!(stopping.removal.is_some());
        assert_eq!(
            stopping
                .pending_replacement
                .as_ref()
                .expect("replacement retained")
                .generation,
            4
        );

        finish_stopped_entries(&mut entries, None).expect("finish replacement");
        let replacement = entries.get(&[4; 20]).expect("replacement installed");
        assert_eq!(replacement.registration.generation, 4);
        assert!(replacement.removal.is_none());
    }

    fn test_registration(generation: u64, tracker_port: u16) -> DiscoveryAdvertisementRegistration {
        DiscoveryAdvertisementRegistration {
            generation,
            info_hash: [4; 20],
            trackers: vec![UdpTrackerConfig {
                url: format!("udp://127.0.0.1:{tracker_port}"),
                endpoint: rstorrent_protocol::magnet::UdpTrackerUrl {
                    host: "127.0.0.1".to_owned(),
                    port: tracker_port,
                },
                tier: 0,
                position: 0,
                source: crate::TrackerSource::Metainfo,
            }],
            desired_running: true,
            complete: false,
            incoming_registered: false,
            privacy: TorrentPrivacy::Unknown,
            counters: TrackerCounters::unknown_metadata(),
            peers: TorrentPeerHandle::new(Arc::new(NoopSink)).expect("peer registry"),
            activity_sink: Arc::new(NoopSink),
        }
    }

    #[tokio::test]
    async fn session_tracker_sends_truthful_lifecycle_ports_and_counters() {
        let tracker = UdpSocket::bind("127.0.0.1:0").await.expect("bind tracker");
        let tracker_address = tracker.local_addr().expect("tracker address");
        let (packet_sender, mut packet_receiver) = mpsc::channel(3);
        let tracker_task = tokio::spawn(async move {
            let connection_id = 0x0102_0304_0506_0708_u64;
            let mut packet = [0_u8; 2048];
            let mut announces = 0;
            while announces < 3 {
                let (length, client) = tracker.recv_from(&mut packet).await.expect("request");
                if length == 16 {
                    let transaction = &packet[12..16];
                    let mut response = Vec::from(0_u32.to_be_bytes());
                    response.extend_from_slice(transaction);
                    response.extend_from_slice(&connection_id.to_be_bytes());
                    tracker
                        .send_to(&response, client)
                        .await
                        .expect("connect response");
                    continue;
                }
                assert_eq!(length, 98);
                packet_sender
                    .send(packet[..length].to_vec())
                    .await
                    .expect("record announce");
                let transaction = &packet[12..16];
                let mut response = Vec::from(1_u32.to_be_bytes());
                response.extend_from_slice(transaction);
                response.extend_from_slice(&900_u32.to_be_bytes());
                response.extend_from_slice(&0_u32.to_be_bytes());
                response.extend_from_slice(&0_u32.to_be_bytes());
                tracker
                    .send_to(&response, client)
                    .await
                    .expect("announce response");
                announces += 1;
            }
        });

        let endpoint = PeerAdvertisementEndpoint {
            generation: 1,
            endpoint: Some("203.0.113.9:48001".parse().expect("mapped endpoint")),
            scope: Some(PeerAdvertisementEndpointScope::Mapped),
            stopping: false,
        };
        let (_endpoint_sender, endpoint_receiver) = watch::channel(endpoint);
        let network = NetworkConfig::new(
            NetworkPolicy::LoopbackOnly,
            Duration::from_secs(1),
            Duration::from_secs(1),
        );
        let mut dht_config = crate::dht::DhtConfig::for_network(NetworkPolicy::Offline);
        dht_config.bootstrap_nodes.clear();
        let dht = crate::dht::DhtService::start(dht_config)
            .await
            .expect("start offline DHT");
        let service =
            DiscoveryAdvertisementService::start(network, endpoint_receiver, dht.handle());
        let handle = service.handle();
        let counters = TrackerCounters::default();
        counters.add_downloaded(11);
        counters.add_uploaded(7);
        counters.set_left(99);
        let peers = TorrentPeerHandle::new(Arc::new(NoopSink)).expect("peer registry");
        let activity = Arc::new(RecordingActivity::default());
        let registration = DiscoveryAdvertisementRegistration {
            generation: 3,
            info_hash: [4; 20],
            trackers: vec![UdpTrackerConfig {
                url: format!("udp://{tracker_address}"),
                endpoint: rstorrent_protocol::magnet::UdpTrackerUrl {
                    host: tracker_address.ip().to_string(),
                    port: tracker_address.port(),
                },
                tier: 0,
                position: 0,
                source: crate::TrackerSource::Metainfo,
            }],
            desired_running: true,
            complete: false,
            incoming_registered: false,
            privacy: TorrentPrivacy::Public,
            counters: counters.clone(),
            peers,
            activity_sink: activity.clone(),
        };
        handle.upsert(registration.clone()).await.expect("register");
        let started = packet_receiver.recv().await.expect("started announce");
        activity.wait_for_successes(1).await;

        counters.set_left(0);
        handle
            .upsert(DiscoveryAdvertisementRegistration {
                complete: true,
                incoming_registered: true,
                ..registration
            })
            .await
            .expect("promote complete seed");
        let completed = packet_receiver.recv().await.expect("completed announce");
        activity.wait_for_successes(2).await;

        let remove = tokio::spawn(async move { handle.remove([4; 20], 3).await });
        let stopped = packet_receiver.recv().await.expect("stopped announce");
        remove
            .await
            .expect("remove task")
            .expect("remove registration");
        let terminal = service.shutdown().await.expect("shutdown service");
        dht.shutdown().await.expect("shutdown DHT");
        tracker_task.await.expect("tracker task");

        assert_eq!(announce_event(&started), AnnounceEvent::Started as u32);
        assert_eq!(announce_port(&started), OUTBOUND_ONLY_TRACKER_PORT);
        assert_eq!(announce_counter(&started, 56), 11);
        assert_eq!(announce_counter(&started, 64), 99);
        assert_eq!(announce_counter(&started, 72), 7);
        assert_eq!(announce_event(&completed), AnnounceEvent::Completed as u32);
        assert_eq!(announce_port(&completed), 48_001);
        assert_eq!(announce_counter(&completed, 64), 0);
        assert_eq!(announce_event(&stopped), AnnounceEvent::Stopped as u32);
        assert_eq!(announce_port(&stopped), 48_001);
        assert_eq!(
            i32::from_be_bytes(stopped[92..96].try_into().expect("num want")),
            0
        );
        assert_eq!(terminal.registrations, 0);
        assert_eq!(terminal.tasks, 0);
        assert_eq!(terminal.tracker_operations, 0);
        assert_eq!(terminal.tracker_operations_high_water, 1);
        assert_eq!(terminal.command_queue_high_water, 1);
        assert_eq!(terminal.dht_operations, 0);
        assert_eq!(terminal.dht_operations_high_water, 1);
    }

    #[tokio::test]
    async fn session_scheduler_announces_public_seed_and_keeps_dht_peers() {
        let server = crate::dht::DhtService::start(crate::dht::DhtConfig {
            network_policy: NetworkPolicy::LoopbackOnly,
            bind_address: "127.0.0.1:0".parse().expect("bind address"),
            bootstrap_nodes: Vec::new(),
            initial_snapshot: None,
            query_timeout: Duration::from_millis(500),
            lookup_timeout: Duration::from_secs(3),
            bootstrap_retry_interval: Duration::from_secs(60),
            routing_refresh_interval: Duration::from_secs(60),
            peer_ttl: Duration::from_millis(100),
            read_only: false,
            byte_metric_sink: None,
        })
        .await
        .expect("start DHT server");
        let client = crate::dht::DhtService::start(crate::dht::DhtConfig {
            bootstrap_nodes: vec![crate::dht::BootstrapNode::Address(server.local_address())],
            ..crate::dht::DhtConfig {
                network_policy: NetworkPolicy::LoopbackOnly,
                bind_address: "127.0.0.1:0".parse().expect("bind address"),
                bootstrap_nodes: Vec::new(),
                initial_snapshot: None,
                query_timeout: Duration::from_millis(500),
                lookup_timeout: Duration::from_secs(3),
                bootstrap_retry_interval: Duration::from_secs(60),
                routing_refresh_interval: Duration::from_secs(60),
                peer_ttl: Duration::from_secs(30 * 60),
                read_only: false,
                byte_metric_sink: None,
            }
        })
        .await
        .expect("start DHT client");
        let endpoint = PeerAdvertisementEndpoint {
            generation: 7,
            endpoint: Some("127.0.0.1:55001".parse().expect("peer endpoint")),
            scope: Some(PeerAdvertisementEndpointScope::Loopback),
            stopping: false,
        };
        let (endpoint_sender, endpoint_receiver) = watch::channel(endpoint);
        let network = NetworkConfig::new(
            NetworkPolicy::LoopbackOnly,
            Duration::from_secs(1),
            Duration::from_secs(1),
        );
        let service =
            DiscoveryAdvertisementService::start(network, endpoint_receiver, client.handle());
        let activity = Arc::new(RecordingActivity::default());
        let peers = TorrentPeerHandle::new(Arc::new(NoopSink)).expect("peer registry");
        service
            .handle()
            .upsert(DiscoveryAdvertisementRegistration {
                generation: 1,
                info_hash: [12; 20],
                trackers: Vec::new(),
                desired_running: true,
                complete: true,
                incoming_registered: true,
                privacy: TorrentPrivacy::Public,
                counters: TrackerCounters::default(),
                peers,
                activity_sink: activity.clone(),
            })
            .await
            .expect("register seed");

        assert_eq!(
            activity.wait_for_dht_announces(1).await,
            (55_001, 1, 1, 1, 0)
        );
        endpoint_sender
            .send(PeerAdvertisementEndpoint {
                generation: 8,
                endpoint: Some("127.0.0.1:55002".parse().expect("corrected endpoint")),
                ..endpoint
            })
            .expect("publish corrected endpoint");
        assert_eq!(
            activity.wait_for_dht_announces(2).await,
            (55_002, 1, 1, 1, 0)
        );
        let discovered = client
            .handle()
            .lookup([12; 20])
            .await
            .expect("lookup announced seed");
        assert!(discovered.iter().any(|peer| peer.port() == 55_002));

        let terminal = service.shutdown().await.expect("shutdown scheduler");
        assert_eq!(terminal.tasks, 0);
        assert_eq!(terminal.registrations, 0);
        assert_eq!(terminal.tracker_operations, 0);
        assert_eq!(terminal.tracker_operations_high_water, 0);
        assert_eq!(terminal.command_queue_high_water, 1);
        assert_eq!(terminal.dht_operations, 0);
        assert_eq!(terminal.dht_operations_high_water, 1);
        let queries_after_shutdown = server
            .handle()
            .stats()
            .await
            .expect("server stats after shutdown")
            .queries_received;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(
            server
                .handle()
                .stats()
                .await
                .expect("server stats after expiry")
                .queries_received,
            queries_after_shutdown,
            "joined scheduler must send no post-stop DHT query"
        );
        assert_eq!(
            client.handle().lookup([12; 20]).await,
            Err(DhtError::NoReachableNodes),
            "controlled short-TTL node must expire stale remote peer state"
        );
        client.shutdown().await.expect("shutdown DHT client");
        server.shutdown().await.expect("shutdown DHT server");
    }

    fn announce_counter(packet: &[u8], offset: usize) -> u64 {
        u64::from_be_bytes(packet[offset..offset + 8].try_into().expect("counter"))
    }

    fn announce_event(packet: &[u8]) -> u32 {
        u32::from_be_bytes(packet[80..84].try_into().expect("event"))
    }

    fn announce_port(packet: &[u8]) -> u16 {
        u16::from_be_bytes(packet[96..98].try_into().expect("port"))
    }
}
