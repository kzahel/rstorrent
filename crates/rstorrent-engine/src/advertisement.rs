//! Session-wide tracker discovery and peer-advertisement ownership.

use std::collections::BTreeMap;
use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use rstorrent_protocol::udp_tracker::{AnnounceEvent, MAX_COMPACT_PEERS};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;

use crate::dht::{DhtAnnouncePorts, DhtAnnounceResult, DhtError, DhtHandle, MAX_ACTIVE_LOOKUPS};
use crate::driver::{
    DownloadActivityEvent, DownloadActivitySink, DownloadControl, DownloadError,
    UdpTrackerAnnounce, UdpTrackerExchange, UdpTrackerTiming, UdpTrackerTokenCache,
    announce_udp_tracker, compact_peer_address, random_nonzero_u32,
};
use crate::http_tracker::{
    HTTP_TRACKER_TIMEOUT, HttpTrackerAnnounce, HttpTrackerClients, HttpTrackerError,
    HttpTrackerResponse, TrackerRetryDirective, announce_http_tracker_with_address_families,
};
use crate::network::{
    AddressFamily, AddressFamilyPolicy, NetworkConfig, NetworkPolicy, PeerEncryptionPolicy,
};
use crate::peer::{PeerEndpoint, PeerObservation, PeerSource};
use crate::torrent_peer::TorrentPeerHandle;
use crate::tracker::{
    TrackerAcceptedOutcome, TrackerAction, TrackerConfig, TrackerEndpoint,
    TrackerHttpsAuthentication, TrackerId, TrackerSchedule, TrackerWaitKind,
};

pub const OUTBOUND_ONLY_TRACKER_PORT: u16 = 1;
pub const UNKNOWN_METADATA_LEFT_BYTES: u64 = 16 * 1024;
pub const MAX_TRACKER_OPERATIONS: usize = 8;
pub const DISCOVERY_ADVERTISEMENT_COMMAND_CAPACITY: usize = 256;
pub const TRACKER_STOP_TIMEOUT: Duration = Duration::from_secs(5);
pub const DHT_LOOKUP_INTERVAL: Duration = Duration::from_secs(60);
pub const DHT_ANNOUNCE_INTERVAL: Duration = Duration::from_secs(15 * 60);

type HttpTrackerClientFactory =
    fn(NetworkPolicy, TrackerHttpsAuthentication) -> Result<HttpTrackerClients, HttpTrackerError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerAdvertisementEndpointScope {
    Loopback,
    LocalNetwork,
    GlobalUnicast,
    Unfiltered,
    Pinholed,
    Mapped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerAdvertisementFamilyEndpoint {
    pub endpoint: Option<SocketAddr>,
    pub source_address: Option<IpAddr>,
    pub scope: Option<PeerAdvertisementEndpointScope>,
}

impl PeerAdvertisementFamilyEndpoint {
    pub const fn outbound_only() -> Self {
        Self {
            endpoint: None,
            source_address: None,
            scope: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerAdvertisementEndpoint {
    pub generation: u64,
    pub ipv4: PeerAdvertisementFamilyEndpoint,
    pub ipv6: PeerAdvertisementFamilyEndpoint,
    pub stopping: bool,
}

impl PeerAdvertisementEndpoint {
    pub const fn outbound_only(generation: u64) -> Self {
        Self {
            generation,
            ipv4: PeerAdvertisementFamilyEndpoint::outbound_only(),
            ipv6: PeerAdvertisementFamilyEndpoint::outbound_only(),
            stopping: false,
        }
    }

    pub const fn stopping(generation: u64) -> Self {
        Self {
            generation,
            ipv4: PeerAdvertisementFamilyEndpoint::outbound_only(),
            ipv6: PeerAdvertisementFamilyEndpoint::outbound_only(),
            stopping: true,
        }
    }

    #[must_use]
    pub const fn family(self, family: AddressFamily) -> PeerAdvertisementFamilyEndpoint {
        match family {
            AddressFamily::Ipv4 => self.ipv4,
            AddressFamily::Ipv6 => self.ipv6,
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
    pub trackers: Vec<TrackerConfig>,
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DiscoveryAdvertisementRuntimeSnapshot {
    pub registrations: usize,
    pub active_registrations: usize,
    pub tracker_operations: usize,
    pub dht_operations: usize,
    pub command_queue_high_water: usize,
}

#[derive(Clone, Debug)]
pub struct DiscoveryAdvertisementHandle {
    sender: mpsc::Sender<Command>,
    queued: Arc<AtomicU64>,
    queue_high_water: Arc<AtomicU64>,
}

impl DiscoveryAdvertisementHandle {
    pub async fn replace_address_family_policy(
        &self,
        policy: AddressFamilyPolicy,
    ) -> Result<(), DiscoveryAdvertisementError> {
        let (sender, receiver) = oneshot::channel();
        self.send(Command::ReplaceAddressFamilyPolicy {
            policy,
            response: sender,
        })
        .await?;
        receiver
            .await
            .map_err(|_| DiscoveryAdvertisementError::OwnerStopped)?
    }

    pub async fn replace_encryption_policy(
        &self,
        policy: PeerEncryptionPolicy,
    ) -> Result<(), DiscoveryAdvertisementError> {
        let (sender, receiver) = oneshot::channel();
        self.send(Command::ReplaceEncryptionPolicy {
            policy,
            response: sender,
        })
        .await?;
        receiver
            .await
            .map_err(|_| DiscoveryAdvertisementError::OwnerStopped)?
    }

    pub async fn replace_https_authentication(
        &self,
        authentication: TrackerHttpsAuthentication,
    ) -> Result<(), DiscoveryAdvertisementError> {
        let (sender, receiver) = oneshot::channel();
        self.send(Command::ReplaceHttpsAuthentication {
            authentication,
            response: sender,
        })
        .await?;
        receiver
            .await
            .map_err(|_| DiscoveryAdvertisementError::OwnerStopped)?
    }

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

    pub async fn snapshot(
        &self,
    ) -> Result<DiscoveryAdvertisementRuntimeSnapshot, DiscoveryAdvertisementError> {
        let (sender, receiver) = oneshot::channel();
        self.send(Command::Snapshot(sender)).await?;
        receiver
            .await
            .map_err(|_| DiscoveryAdvertisementError::OwnerStopped)
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
    initial_https_authentication: Option<TrackerHttpsAuthentication>,
    initial_https_error: Option<String>,
    task:
        Option<JoinHandle<Result<DiscoveryAdvertisementOwnerCounts, DiscoveryAdvertisementError>>>,
}

impl DiscoveryAdvertisementService {
    pub fn start(
        network: NetworkConfig,
        endpoint: watch::Receiver<PeerAdvertisementEndpoint>,
        dht: DhtHandle,
    ) -> Self {
        Self::start_with_https_authentication(
            network,
            endpoint,
            dht,
            TrackerHttpsAuthentication::SystemTrust,
        )
        .expect("platform-authenticated HTTP tracker clients construct")
    }

    pub fn start_with_https_authentication(
        network: NetworkConfig,
        endpoint: watch::Receiver<PeerAdvertisementEndpoint>,
        dht: DhtHandle,
        https_authentication: TrackerHttpsAuthentication,
    ) -> Result<Self, DiscoveryAdvertisementError> {
        Self::start_with_http_client_factory(
            network,
            endpoint,
            dht,
            https_authentication,
            HttpTrackerClients::new_with_authentication,
        )
    }

    fn start_with_http_client_factory(
        network: NetworkConfig,
        endpoint: watch::Receiver<PeerAdvertisementEndpoint>,
        dht: DhtHandle,
        https_authentication: TrackerHttpsAuthentication,
        http_client_factory: HttpTrackerClientFactory,
    ) -> Result<Self, DiscoveryAdvertisementError> {
        let (http_clients, initial_https_authentication, initial_https_error) =
            match http_client_factory(network.policy, https_authentication) {
                Ok(clients) => (clients, Some(https_authentication), None),
                Err(error) => (
                    HttpTrackerClients::http_only(network.policy).map_err(|fallback| {
                        DiscoveryAdvertisementError::HttpClient(format!(
                            "platform verifier unavailable ({error}); HTTP fallback construction failed ({fallback})"
                        ))
                    })?,
                    None,
                    Some(error.to_string()),
                ),
            };
        let initial_endpoint = *endpoint.borrow();
        let http_clients = http_clients
            .with_source_addresses(
                network.policy,
                initial_endpoint.ipv4.source_address,
                initial_endpoint.ipv6.source_address,
            )
            .map_err(|error| DiscoveryAdvertisementError::HttpClient(error.to_string()))?;
        let (sender, receiver) = mpsc::channel(DISCOVERY_ADVERTISEMENT_COMMAND_CAPACITY);
        let queued = Arc::new(AtomicU64::new(0));
        let queue_high_water = Arc::new(AtomicU64::new(0));
        let handle = DiscoveryAdvertisementHandle {
            sender,
            queued: queued.clone(),
            queue_high_water: queue_high_water.clone(),
        };
        let task = tokio::spawn(run_service(
            DiscoveryAdvertisementRuntime {
                network,
                endpoint_receiver: endpoint,
                dht,
                http_clients: Arc::new(http_clients),
                desired_https_authentication: https_authentication,
                http_client_factory,
            },
            receiver,
            queued,
            queue_high_water,
        ));
        Ok(Self {
            handle,
            initial_https_authentication,
            initial_https_error,
            task: Some(task),
        })
    }

    pub fn handle(&self) -> DiscoveryAdvertisementHandle {
        self.handle.clone()
    }

    pub const fn initial_https_authentication(&self) -> Option<TrackerHttpsAuthentication> {
        self.initial_https_authentication
    }

    pub fn initial_https_error(&self) -> Option<&str> {
        self.initial_https_error.as_deref()
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
    HttpClient(String),
    Convergence(String),
}

impl fmt::Display for DiscoveryAdvertisementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OwnerStopped => formatter.write_str("discovery advertisement owner stopped"),
            Self::Entropy(detail) => write!(formatter, "tracker key entropy: {detail}"),
            Self::Join(detail) => write!(formatter, "discovery advertisement owner join: {detail}"),
            Self::HttpClient(detail) => {
                write!(formatter, "HTTP tracker client construction: {detail}")
            }
            Self::Convergence(detail) => {
                write!(formatter, "address-family convergence: {detail}")
            }
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
    ReplaceHttpsAuthentication {
        authentication: TrackerHttpsAuthentication,
        response: oneshot::Sender<Result<(), DiscoveryAdvertisementError>>,
    },
    ReplaceEncryptionPolicy {
        policy: PeerEncryptionPolicy,
        response: oneshot::Sender<Result<(), DiscoveryAdvertisementError>>,
    },
    ReplaceAddressFamilyPolicy {
        policy: AddressFamilyPolicy,
        response: oneshot::Sender<Result<(), DiscoveryAdvertisementError>>,
    },
    Snapshot(oneshot::Sender<DiscoveryAdvertisementRuntimeSnapshot>),
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
    http_tracker_ids: BTreeMap<TrackerId, Vec<u8>>,
    tracker_cancellation: CancellationToken,
    schedule_epoch: u64,
    last_endpoint_generation: u64,
    removal: Option<Removal>,
    pending_replacement: Option<DiscoveryAdvertisementRegistration>,
    dht_epoch: u64,
    dht_inflight: bool,
    next_dht_action: Instant,
    last_dht_endpoint_generation: u64,
}

struct DiscoveryAdvertisementRuntime {
    network: NetworkConfig,
    endpoint_receiver: watch::Receiver<PeerAdvertisementEndpoint>,
    dht: DhtHandle,
    http_clients: Arc<HttpTrackerClients>,
    desired_https_authentication: TrackerHttpsAuthentication,
    http_client_factory: HttpTrackerClientFactory,
}

impl TorrentEntry {
    fn new(
        registration: DiscoveryAdvertisementRegistration,
        https_authentication: TrackerHttpsAuthentication,
    ) -> Result<Self, DiscoveryAdvertisementError> {
        let control = DownloadControl::new();
        control.set_activity_sink(registration.activity_sink.clone());
        let mut scheduled_trackers = registration.trackers.clone();
        shuffle_tracker_configs(&mut scheduled_trackers)
            .map_err(|error| DiscoveryAdvertisementError::Entropy(error.to_string()))?;
        let mut schedule = TrackerSchedule::from_configs(scheduled_trackers);
        schedule.set_https_authentication(https_authentication);
        let tracker_key = random_nonzero_u32()
            .map_err(|error| DiscoveryAdvertisementError::Entropy(error.to_string()))?;
        let entry = Self {
            registration,
            schedule,
            tracker_key,
            control,
            token_caches: BTreeMap::new(),
            http_tracker_ids: BTreeMap::new(),
            tracker_cancellation: CancellationToken::new(),
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
        self.tracker_cancellation.cancel();
        self.tracker_cancellation = CancellationToken::new();
        self.schedule_epoch = self.schedule_epoch.saturating_add(1);
        self.schedule.cancel_inflight();
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
    token_cache: Option<UdpTrackerTokenCache>,
    result: Result<TrackerAnnounceOutcome, TrackerOperationFailure>,
}

#[derive(Debug)]
struct TrackerAnnounceOutcome {
    interval: Duration,
    seeders: Option<u32>,
    leechers: Option<u32>,
    connection_family: Option<crate::tracker::TrackerConnectionFamily>,
    peers: Vec<SocketAddr>,
    tracker_id: Option<Vec<u8>>,
    warnings: Vec<String>,
}

#[derive(Debug)]
enum TrackerOperationFailure {
    Cancelled,
    Transport(String),
    Declared {
        reason: String,
        retry: Option<TrackerRetryDirective>,
    },
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
    announce_ports: Option<DhtAnnouncePorts>,
    result: Result<DhtOperationSuccess, DhtError>,
}

async fn run_service(
    runtime: DiscoveryAdvertisementRuntime,
    mut receiver: mpsc::Receiver<Command>,
    queued: Arc<AtomicU64>,
    queue_high_water: Arc<AtomicU64>,
) -> Result<DiscoveryAdvertisementOwnerCounts, DiscoveryAdvertisementError> {
    let DiscoveryAdvertisementRuntime {
        mut network,
        mut endpoint_receiver,
        dht,
        mut http_clients,
        mut desired_https_authentication,
        http_client_factory,
    } = runtime;
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
        finish_stopped_entries(
            &mut entries,
            shutdown_deadline,
            desired_https_authentication,
        )?;
        if shutting_down
            && (entries.is_empty()
                || shutdown_deadline.is_some_and(|deadline| Instant::now() >= deadline))
        {
            break;
        }

        let tracker_wait = fill_tracker_operations(
            &mut entries,
            &mut operations,
            &http_clients,
            network,
            endpoint,
        );
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
                        registration
                            .peers
                            .enforce_address_families(network.address_families)
                            .map_err(|error| {
                                DiscoveryAdvertisementError::Convergence(error.to_string())
                            })?;
                        let effect = apply_registration(
                            &mut entries,
                            registration,
                            desired_https_authentication,
                        )?;
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
                    Command::ReplaceHttpsAuthentication {
                        authentication,
                        response,
                    } => {
                        let previous = http_clients.https_authentication();
                        let fence_unauthenticated = authentication
                            == TrackerHttpsAuthentication::SystemTrust
                            && previous == Some(TrackerHttpsAuthentication::Disabled);
                        if fence_unauthenticated {
                            http_clients = Arc::new(http_clients.without_https());
                        }
                        match http_client_factory(network.policy, authentication).and_then(|clients| {
                            clients.with_source_addresses(
                                network.policy,
                                endpoint.ipv4.source_address,
                                endpoint.ipv6.source_address,
                            )
                        }) {
                            Ok(clients) => {
                                http_clients = Arc::new(clients);
                                desired_https_authentication = authentication;
                                for entry in entries.values_mut() {
                                    entry.schedule.set_https_authentication(authentication);
                                    entry.emit_snapshot(entry.registration.desired_running);
                                }
                                let _ = response.send(Ok(()));
                            }
                            Err(error) => {
                                if fence_unauthenticated || previous.is_none() {
                                    desired_https_authentication = authentication;
                                    for entry in entries.values_mut() {
                                        entry.schedule.set_https_authentication(authentication);
                                        entry.emit_snapshot(entry.registration.desired_running);
                                    }
                                }
                                let _ = response.send(Err(
                                    DiscoveryAdvertisementError::HttpClient(
                                        error.to_string(),
                                    ),
                                ));
                            }
                        }
                    }
                    Command::ReplaceEncryptionPolicy { policy, response } => {
                        if network.encryption != policy {
                            network.encryption = policy;
                            for entry in entries.values_mut() {
                                if entry.registration.desired_running && entry.removal.is_none() {
                                    entry.schedule.request_update();
                                }
                            }
                        }
                        let _ = response.send(Ok(()));
                    }
                    Command::ReplaceAddressFamilyPolicy { policy, response } => {
                        if network.address_families != policy {
                            network.address_families = policy;
                            operations.abort_all();
                            while operations.join_next().await.is_some() {}
                            dht_operations.abort_all();
                            while dht_operations.join_next().await.is_some() {}
                            for info_hash in entries.keys().copied().collect::<Vec<_>>() {
                                let _ = dht.cancel_lookup(info_hash).await;
                            }
                            for entry in entries.values_mut() {
                                entry.schedule.cancel_inflight();
                                entry.schedule.request_update();
                                entry.token_caches.clear();
                                entry.dht_epoch = entry.dht_epoch.saturating_add(1);
                                entry.dht_inflight = false;
                                entry.next_dht_action = Instant::now();
                            }
                        }
                        let deadline = Instant::now() + Duration::from_secs(5);
                        let result = loop {
                            let mut converged = true;
                            for entry in entries.values() {
                                if entry.registration.peers.enforce_address_families(policy).is_err()
                                    || !entry.registration.peers.address_families_converged(policy)
                                {
                                    converged = false;
                                }
                            }
                            if converged {
                                break Ok(());
                            }
                            if Instant::now() >= deadline {
                                break Err(DiscoveryAdvertisementError::Convergence(
                                    "peer owners did not retire disallowed connections within 5 seconds"
                                        .to_owned(),
                                ));
                            }
                            tokio::time::sleep(Duration::from_millis(10)).await;
                        };
                        let _ = response.send(result);
                    }
                    Command::Snapshot(response) => {
                        let _ = response.send(DiscoveryAdvertisementRuntimeSnapshot {
                            registrations: entries.len(),
                            active_registrations: entries
                                .values()
                                .filter(|entry| {
                                    entry.registration.desired_running
                                        && entry.removal.is_none()
                                })
                                .count(),
                            tracker_operations: operations.len(),
                            dht_operations: dht_operations.len(),
                            command_queue_high_water: queue_high_water
                                .load(Ordering::Acquire)
                                .try_into()
                                .unwrap_or(usize::MAX),
                        });
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
                    if let Ok(rebound) = http_clients.with_source_addresses(
                        network.policy,
                        endpoint.ipv4.source_address,
                        endpoint.ipv6.source_address,
                    ) {
                        http_clients = Arc::new(rebound);
                    }
                    for entry in entries.values_mut() {
                        if entry.registration.incoming_registered
                            && entry.last_endpoint_generation != endpoint.generation
                            && entry.removal.is_none()
                        {
                            entry.token_caches.clear();
                            entry.schedule.request_update();
                        }
                    }
                    let corrected = entries
                        .iter_mut()
                        .filter_map(|(info_hash, entry)| {
                            dht_announce_ports(network.policy, endpoint, &entry.registration)?;
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
    https_authentication: TrackerHttpsAuthentication,
) -> Result<RegistrationEffect, DiscoveryAdvertisementError> {
    let info_hash = registration.info_hash;
    let private_peers =
        (registration.privacy == TorrentPrivacy::Private).then(|| registration.peers.clone());
    let Some(entry) = entries.get_mut(&registration.info_hash) else {
        entries.insert(
            registration.info_hash,
            TorrentEntry::new(registration, https_authentication)?,
        );
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
        entry
            .schedule
            .set_https_authentication(https_authentication);
        entry.token_caches.clear();
        entry.http_tracker_ids.clear();
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
    https_authentication: TrackerHttpsAuthentication,
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
                entries.insert(
                    info_hash,
                    TorrentEntry::new(replacement, https_authentication)?,
                );
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
    http_clients: &Arc<HttpTrackerClients>,
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
                    endpoint: tracker_endpoint,
                    tier,
                    event,
                    attempt,
                    fallback,
                    ..
                } => {
                    let tracker = redacted_tracker_label(&url);
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
                    let ports = tracker_ports(
                        network.policy,
                        endpoint,
                        entry.registration.incoming_registered,
                    );
                    let num_want = if event == AnnounceEvent::Stopped {
                        0
                    } else {
                        MAX_COMPACT_PEERS as i32
                    };
                    let control = entry.control.clone();
                    let info_hash = entry.registration.info_hash;
                    let registration_generation = entry.registration.generation;
                    let schedule_epoch = entry.schedule_epoch;
                    let cancellation = entry.tracker_cancellation.clone();
                    let tracker_key = entry.tracker_key;
                    match tracker_endpoint {
                        TrackerEndpoint::Udp(url) => {
                            let mut token_cache =
                                entry.token_caches.remove(&id).unwrap_or_default();
                            operations.spawn(async move {
                                let result = tokio::select! {
                                    biased;
                                    _ = cancellation.cancelled() => {
                                        Err(TrackerOperationFailure::Cancelled)
                                    }
                                    response = announce_udp_tracker(
                                        &url,
                                        network.policy,
                                        network.address_families,
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
                                            port: ports.ipv4,
                                            ipv6_port: ports.ipv6,
                                        },
                                        UdpTrackerExchange {
                                            timing: UdpTrackerTiming::PRODUCTION,
                                            control: &control,
                                            tracker_label: &tracker,
                                            source_ipv4: endpoint.ipv4.source_address,
                                            source_ipv6: endpoint.ipv6.source_address,
                                        },
                                    ) => response
                                        .map(|result| {
                                            let response = result.response;
                                            TrackerAnnounceOutcome {
                                                interval: Duration::from_secs(u64::from(response.interval)),
                                                seeders: Some(response.seeders),
                                                leechers: Some(response.leechers),
                                                connection_family: Some(result.connection_family),
                                                peers: response
                                                    .peers
                                                    .into_iter()
                                                    .map(compact_peer_address)
                                                    .collect(),
                                                tracker_id: None,
                                                warnings: Vec::new(),
                                            }
                                        })
                                        .map_err(|error| {
                                            TrackerOperationFailure::Transport(error.to_string())
                                        }),
                                };
                                TrackerOperationResult {
                                    info_hash,
                                    registration_generation,
                                    schedule_epoch,
                                    endpoint_generation: endpoint.generation,
                                    id,
                                    tracker,
                                    token_cache: Some(token_cache),
                                    result,
                                }
                            });
                        }
                        TrackerEndpoint::Http { .. } => {
                            let clients = http_clients.clone();
                            let tracker_id = entry.http_tracker_ids.get(&id).cloned();
                            let timeout = if event == AnnounceEvent::Stopped {
                                TRACKER_STOP_TIMEOUT
                            } else {
                                HTTP_TRACKER_TIMEOUT
                            };
                            operations.spawn(async move {
                                let announce = HttpTrackerAnnounce {
                                    info_hash,
                                    peer_id: network.peer_id,
                                    port: ports.ipv4,
                                    ipv6_port: ports.ipv6,
                                    uploaded: counters.uploaded,
                                    downloaded: counters.downloaded,
                                    left: counters.left,
                                    event,
                                    key: tracker_key,
                                    num_want: u32::try_from(num_want).unwrap_or(0),
                                    support_crypto: network.encryption.accepts_incoming_mse(),
                                    tracker_id,
                                };
                                let response = tokio::select! {
                                    biased;
                                    _ = cancellation.cancelled() => {
                                        return TrackerOperationResult {
                                            info_hash,
                                            registration_generation,
                                            schedule_epoch,
                                            endpoint_generation: endpoint.generation,
                                            id,
                                            tracker,
                                            token_cache: None,
                                            result: Err(TrackerOperationFailure::Cancelled),
                                        };
                                    }
                                    response = announce_http_tracker_with_address_families(
                                        &clients,
                                        &url,
                                        network.policy,
                                        network.address_families,
                                        false,
                                        &announce,
                                        timeout,
                                    ) => response,
                                };
                                let result = match response {
                                    Ok(HttpTrackerResponse::Success(success)) => {
                                        let mut warnings = success.diagnostics;
                                        if let Some(warning) = success.warning {
                                            warnings.insert(0, warning);
                                        }
                                        Ok(TrackerAnnounceOutcome {
                                            interval: success.interval,
                                            seeders: success.seeders,
                                            leechers: success.leechers,
                                            connection_family: success.connection_family,
                                            peers: success
                                                .peers
                                                .into_iter()
                                                .filter_map(|peer| {
                                                    match peer {
                                                    crate::http_tracker::TrackerPeer::Address(
                                                        address,
                                                    ) => Some(address),
                                                    crate::http_tracker::TrackerPeer::Hostname {
                                                        ..
                                                    } => None,
                                                }
                                                })
                                                .collect(),
                                            tracker_id: success.tracker_id,
                                            warnings,
                                        })
                                    }
                                    Ok(HttpTrackerResponse::Failure { reason, retry }) => {
                                        Err(TrackerOperationFailure::Declared { reason, retry })
                                    }
                                    Err(error) => {
                                        Err(TrackerOperationFailure::Transport(error.to_string()))
                                    }
                                };
                                TrackerOperationResult {
                                    info_hash,
                                    registration_generation,
                                    schedule_epoch,
                                    endpoint_generation: endpoint.generation,
                                    id,
                                    tracker,
                                    token_cache: None,
                                    result,
                                }
                            });
                        }
                    }
                    spawned = true;
                }
                TrackerAction::Wait {
                    delay, url, kind, ..
                } => {
                    let tracker = redacted_tracker_label(&url);
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

        let announce_ports = dht_announce_ports(policy, endpoint, &entry.registration);
        let info_hash = entry.registration.info_hash;
        let registration_generation = entry.registration.generation;
        let dht_epoch = entry.dht_epoch;
        let endpoint_generation = endpoint.generation;
        let operation_dht = dht.clone();
        entry.dht_inflight = true;
        entry.control.emit(DownloadActivityEvent::DhtLookupStarted);
        operations.spawn(async move {
            let result = match announce_ports {
                Some(ports) => operation_dht
                    .lookup_and_announce_ports(info_hash, ports)
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
                announce_ports,
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
    if operation.announce_ports.is_some()
        && (operation.endpoint_generation != endpoint.generation
            || operation.announce_ports
                != dht_announce_ports(policy, endpoint, &entry.registration))
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
                                .announce_ports
                                .expect("announce result retains its explicit ports")
                                .ipv4,
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

fn dht_announce_ports(
    policy: NetworkPolicy,
    endpoint: PeerAdvertisementEndpoint,
    registration: &DiscoveryAdvertisementRegistration,
) -> Option<DhtAnnouncePorts> {
    if !registration.desired_running
        || !registration.complete
        || !registration.incoming_registered
        || registration.privacy != TorrentPrivacy::Public
        || endpoint.stopping
    {
        return None;
    }
    Some(DhtAnnouncePorts {
        ipv4: family_port(policy, endpoint.ipv4),
        ipv6: family_port(policy, endpoint.ipv6),
    })
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
    if let Some(token_cache) = operation.token_cache {
        entry.token_caches.insert(operation.id, token_cache);
    }
    let now = entry.control.diagnostic_elapsed();
    if operation.endpoint_generation != endpoint.generation && entry.removal.is_none() {
        entry.schedule.supersede(operation.id);
        entry.emit_snapshot(true);
        return;
    }
    match operation.result {
        Ok(response) => {
            let peer_count = response.peers.len().try_into().unwrap_or(u32::MAX);
            let success = entry.schedule.succeeded_outcome(
                operation.id,
                now,
                TrackerAcceptedOutcome {
                    requested_interval: response.interval,
                    peer_count,
                    seeders: response.seeders,
                    leechers: response.leechers,
                    connection_family: response.connection_family,
                },
            );
            if let Some(tracker_id) = response.tracker_id {
                entry.http_tracker_ids.insert(operation.id, tracker_id);
            }
            entry.last_endpoint_generation = operation.endpoint_generation;
            entry
                .control
                .emit(DownloadActivityEvent::TrackerAnnounceSucceeded {
                    tracker: operation.tracker.clone(),
                    peer_count,
                    interval_seconds: success.interval.as_secs(),
                });
            for detail in response.warnings {
                entry.control.emit(DownloadActivityEvent::TrackerWarning {
                    tracker: operation.tracker.clone(),
                    detail,
                });
            }
            for peer in response.peers {
                if !policy.allows(peer) {
                    continue;
                }
                let Ok(endpoint) = PeerEndpoint::new(peer) else {
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
        Err(TrackerOperationFailure::Cancelled) => {}
        Err(TrackerOperationFailure::Transport(detail)) => {
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
        Err(TrackerOperationFailure::Declared { reason, retry }) => {
            let (failures, retry_in_seconds) = match retry {
                Some(TrackerRetryDirective::After(delay)) => {
                    let failure =
                        entry
                            .schedule
                            .failed_with_retry(operation.id, now, &reason, delay);
                    (failure.failures, failure.retry_in.as_secs())
                }
                Some(TrackerRetryDirective::Never) => {
                    entry.schedule.disable(operation.id, now, &reason);
                    (0, 0)
                }
                None => {
                    let failure = entry.schedule.failed(operation.id, now, &reason);
                    (failure.failures, failure.retry_in.as_secs())
                }
            };
            entry
                .control
                .emit(DownloadActivityEvent::TrackerAnnounceFailed {
                    tracker: operation.tracker,
                    failures,
                    retry_in_seconds,
                    detail: reason,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TrackerPorts {
    ipv4: u16,
    ipv6: u16,
}

fn tracker_ports(
    policy: NetworkPolicy,
    endpoint: PeerAdvertisementEndpoint,
    incoming_registered: bool,
) -> TrackerPorts {
    if !incoming_registered || endpoint.stopping {
        return TrackerPorts {
            ipv4: OUTBOUND_ONLY_TRACKER_PORT,
            ipv6: OUTBOUND_ONLY_TRACKER_PORT,
        };
    }
    TrackerPorts {
        ipv4: family_port(policy, endpoint.ipv4),
        ipv6: family_port(policy, endpoint.ipv6),
    }
}

fn family_port(policy: NetworkPolicy, endpoint: PeerAdvertisementFamilyEndpoint) -> u16 {
    match (endpoint.endpoint, endpoint.scope) {
        (
            Some(endpoint),
            Some(
                PeerAdvertisementEndpointScope::Mapped
                | PeerAdvertisementEndpointScope::LocalNetwork
                | PeerAdvertisementEndpointScope::GlobalUnicast
                | PeerAdvertisementEndpointScope::Unfiltered
                | PeerAdvertisementEndpointScope::Pinholed,
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

fn shuffle_tracker_configs(trackers: &mut [TrackerConfig]) -> Result<(), DownloadError> {
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

fn redacted_tracker_label(value: &str) -> String {
    let Ok(url) = url::Url::parse(value) else {
        return "tracker".to_owned();
    };
    let Some(host) = url.host_str() else {
        return "tracker".to_owned();
    };
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    match url.port() {
        Some(port) => format!("{}://{host}:{port}", url.scheme()),
        None => format!("{}://{host}", url.scheme()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peer::PeerRegistrySnapshot;
    use crate::{PeerConnectionObservation, TorrentPeerActivitySink};
    use std::net::{Ipv4Addr, SocketAddrV4};
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream, UdpSocket};
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

    fn ipv4_endpoint(
        generation: u64,
        address: &str,
        scope: PeerAdvertisementEndpointScope,
    ) -> PeerAdvertisementEndpoint {
        let endpoint = address.parse::<SocketAddr>().expect("endpoint");
        PeerAdvertisementEndpoint {
            generation,
            ipv4: PeerAdvertisementFamilyEndpoint {
                endpoint: Some(endpoint),
                source_address: Some(endpoint.ip()),
                scope: Some(scope),
            },
            ipv6: PeerAdvertisementFamilyEndpoint::outbound_only(),
            stopping: false,
        }
    }

    #[derive(Debug, Default)]
    struct RecordingActivity {
        successes: Mutex<usize>,
        failures: Mutex<Vec<String>>,
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

        async fn wait_for_failures(&self, expected: usize) -> String {
            loop {
                {
                    let failures = self
                        .failures
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    if failures.len() >= expected {
                        return failures.last().expect("nonempty failures").clone();
                    }
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
            if let DownloadActivityEvent::TrackerAnnounceFailed { detail, .. } = &event {
                self.failures
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(detail.clone());
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
        let endpoint = ipv4_endpoint(
            4,
            "192.168.1.2:42000",
            PeerAdvertisementEndpointScope::LocalNetwork,
        );
        assert_eq!(
            tracker_ports(NetworkPolicy::Online, endpoint, false),
            TrackerPorts { ipv4: 1, ipv6: 1 }
        );
        assert_eq!(
            tracker_ports(NetworkPolicy::Online, endpoint, true),
            TrackerPorts {
                ipv4: 42_000,
                ipv6: 1,
            }
        );
        assert_eq!(
            tracker_ports(
                NetworkPolicy::Online,
                PeerAdvertisementEndpoint::stopping(5),
                true,
            ),
            TrackerPorts { ipv4: 1, ipv6: 1 }
        );
    }

    #[test]
    fn loopback_listener_does_not_leak_into_online_advertisement() {
        let endpoint = ipv4_endpoint(
            2,
            "127.0.0.1:43000",
            PeerAdvertisementEndpointScope::Loopback,
        );
        assert_eq!(
            tracker_ports(NetworkPolicy::Online, endpoint, true),
            TrackerPorts { ipv4: 1, ipv6: 1 }
        );
        assert_eq!(
            tracker_ports(NetworkPolicy::LoopbackOnly, endpoint, true),
            TrackerPorts {
                ipv4: 43_000,
                ipv6: 1,
            }
        );
    }

    #[test]
    fn tracker_ports_select_each_family_independently() {
        let endpoint = PeerAdvertisementEndpoint {
            generation: 3,
            ipv4: PeerAdvertisementFamilyEndpoint {
                endpoint: Some("192.168.1.2:42000".parse().unwrap()),
                source_address: Some("192.168.1.2".parse().unwrap()),
                scope: Some(PeerAdvertisementEndpointScope::LocalNetwork),
            },
            ipv6: PeerAdvertisementFamilyEndpoint {
                endpoint: Some("[2001:4860:4860::8888]:43000".parse().unwrap()),
                source_address: Some("2001:4860:4860::8888".parse().unwrap()),
                scope: Some(PeerAdvertisementEndpointScope::GlobalUnicast),
            },
            stopping: false,
        };
        assert_eq!(
            tracker_ports(NetworkPolicy::Online, endpoint, true),
            TrackerPorts {
                ipv4: 42_000,
                ipv6: 43_000,
            }
        );
        assert_eq!(
            dht_announce_ports(
                NetworkPolicy::Online,
                endpoint,
                &DiscoveryAdvertisementRegistration {
                    complete: true,
                    incoming_registered: true,
                    privacy: TorrentPrivacy::Public,
                    ..test_registration(1, 41_001)
                },
            ),
            Some(DhtAnnouncePorts {
                ipv4: 42_000,
                ipv6: 43_000,
            })
        );
    }

    #[test]
    fn dht_announcement_requires_verified_public_routable_seed() {
        let endpoint = ipv4_endpoint(
            2,
            "127.0.0.1:43000",
            PeerAdvertisementEndpointScope::Loopback,
        );
        let mut registration = test_registration(1, 41001);
        assert_eq!(
            dht_announce_ports(NetworkPolicy::LoopbackOnly, endpoint, &registration),
            None
        );
        registration.complete = true;
        registration.incoming_registered = true;
        registration.privacy = TorrentPrivacy::Public;
        assert_eq!(
            dht_announce_ports(NetworkPolicy::LoopbackOnly, endpoint, &registration),
            Some(DhtAnnouncePorts {
                ipv4: 43_000,
                ipv6: 1,
            })
        );
        assert_eq!(
            dht_announce_ports(NetworkPolicy::Online, endpoint, &registration),
            Some(DhtAnnouncePorts { ipv4: 1, ipv6: 1 })
        );
        registration.privacy = TorrentPrivacy::Private;
        assert_eq!(
            dht_announce_ports(NetworkPolicy::LoopbackOnly, endpoint, &registration),
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
        apply_registration(
            &mut entries,
            registration.clone(),
            TrackerHttpsAuthentication::SystemTrust,
        )
        .expect("unknown registration");

        registration.privacy = TorrentPrivacy::Private;
        let effect = apply_registration(
            &mut entries,
            registration,
            TrackerHttpsAuthentication::SystemTrust,
        )
        .expect("private transition");
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
        apply_registration(
            &mut entries,
            test_registration(3, 41001),
            TrackerHttpsAuthentication::SystemTrust,
        )
        .expect("first registration");
        apply_registration(
            &mut entries,
            test_registration(4, 41002),
            TrackerHttpsAuthentication::SystemTrust,
        )
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

        finish_stopped_entries(&mut entries, None, TrackerHttpsAuthentication::SystemTrust)
            .expect("finish replacement");
        let replacement = entries.get(&[4; 20]).expect("replacement installed");
        assert_eq!(replacement.registration.generation, 4);
        assert!(replacement.removal.is_none());
    }

    fn test_registration(generation: u64, tracker_port: u16) -> DiscoveryAdvertisementRegistration {
        DiscoveryAdvertisementRegistration {
            generation,
            info_hash: [4; 20],
            trackers: vec![TrackerConfig {
                url: format!("udp://127.0.0.1:{tracker_port}"),
                endpoint: TrackerEndpoint::Udp(rstorrent_protocol::magnet::UdpTrackerUrl {
                    host: "127.0.0.1".to_owned(),
                    port: tracker_port,
                }),
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

    fn reject_system_trust_client_factory(
        policy: NetworkPolicy,
        authentication: TrackerHttpsAuthentication,
    ) -> Result<HttpTrackerClients, HttpTrackerError> {
        if authentication == TrackerHttpsAuthentication::SystemTrust {
            return Err(HttpTrackerError::Client(
                "scripted platform verifier construction failure".to_owned(),
            ));
        }
        HttpTrackerClients::new_with_authentication(policy, authentication)
    }

    fn http_registration(
        info_hash: [u8; 20],
        url: String,
        activity_sink: Arc<dyn DownloadActivitySink>,
    ) -> DiscoveryAdvertisementRegistration {
        DiscoveryAdvertisementRegistration {
            generation: 1,
            info_hash,
            trackers: vec![TrackerConfig {
                endpoint: TrackerEndpoint::from_http_url(&url).expect("HTTP tracker endpoint"),
                url,
                tier: 0,
                position: 0,
                source: crate::TrackerSource::Metainfo,
            }],
            desired_running: true,
            complete: false,
            incoming_registered: false,
            privacy: TorrentPrivacy::Private,
            counters: TrackerCounters::unknown_metadata(),
            peers: TorrentPeerHandle::new(Arc::new(NoopSink)).expect("peer registry"),
            activity_sink,
        }
    }

    async fn read_http_tracker_request(stream: &mut TcpStream) -> String {
        let mut request = Vec::new();
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let mut chunk = [0_u8; 1_024];
            let length = stream.read(&mut chunk).await.expect("read request");
            assert_ne!(length, 0, "request ended before HTTP headers");
            request.extend_from_slice(&chunk[..length]);
            assert!(request.len() <= 16 * 1_024, "request headers are bounded");
        }
        String::from_utf8(request).expect("HTTP tracker request is ASCII")
    }

    fn http_tracker_response(peer: SocketAddrV4) -> Vec<u8> {
        let mut body = b"d8:intervali900e8:completei2e10:incompletei1e10:tracker id4:next15:warning message12:fixture note5:peers6:"
            .to_vec();
        body.extend_from_slice(&peer.ip().octets());
        body.extend_from_slice(&peer.port().to_be_bytes());
        body.push(b'e');
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(&body);
        response
    }

    fn empty_http_tracker_response() -> &'static [u8] {
        b"HTTP/1.1 200 OK\r\nContent-Length: 26\r\nConnection: close\r\n\r\nd8:intervali900e5:peers0:e"
    }

    #[tokio::test]
    async fn session_http_tracker_carries_lifecycle_id_and_peers_through_common_owner() {
        let tracker = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind HTTP tracker");
        let tracker_address = tracker.local_addr().expect("HTTP tracker address");
        let returned_peer = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 49_001);
        let (request_sender, mut request_receiver) = mpsc::channel(4);
        let tracker_task = tokio::spawn(async move {
            for _ in 0..4 {
                let (mut stream, _) = tracker.accept().await.expect("accept HTTP announce");
                let request = read_http_tracker_request(&mut stream).await;
                request_sender
                    .send(request)
                    .await
                    .expect("record HTTP announce");
                stream
                    .write_all(&http_tracker_response(returned_peer))
                    .await
                    .expect("write HTTP announce response");
                stream.shutdown().await.expect("close HTTP response");
            }
        });

        let endpoint = ipv4_endpoint(
            1,
            "127.0.0.1:48001",
            PeerAdvertisementEndpointScope::Loopback,
        );
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
        let tracker_url = format!("http://{tracker_address}/announce?passkey=fixture");
        let registration = DiscoveryAdvertisementRegistration {
            generation: 3,
            info_hash: [4; 20],
            trackers: vec![TrackerConfig {
                endpoint: TrackerEndpoint::from_http_url(&tracker_url)
                    .expect("HTTP tracker endpoint"),
                url: tracker_url,
                tier: 0,
                position: 0,
                source: crate::TrackerSource::Metainfo,
            }],
            desired_running: true,
            complete: false,
            incoming_registered: true,
            privacy: TorrentPrivacy::Public,
            counters: counters.clone(),
            peers: peers.clone(),
            activity_sink: activity.clone(),
        };
        handle.upsert(registration.clone()).await.expect("register");
        let started = request_receiver.recv().await.expect("started announce");
        activity.wait_for_successes(1).await;

        handle
            .replace_encryption_policy(PeerEncryptionPolicy::Disabled)
            .await
            .expect("disable MSE announcement");
        let corrected = request_receiver.recv().await.expect("corrective announce");
        activity.wait_for_successes(2).await;

        let snapshot = peers.registry_snapshot(true);
        assert_eq!(snapshot.records.len(), 1);
        assert_eq!(snapshot.records[0].endpoint.address(), returned_peer.into());
        assert!(snapshot.records[0].sources.contains(PeerSource::Tracker));

        counters.set_left(0);
        handle
            .upsert(DiscoveryAdvertisementRegistration {
                complete: true,
                ..registration
            })
            .await
            .expect("promote complete seed");
        let completed = request_receiver.recv().await.expect("completed announce");
        activity.wait_for_successes(3).await;

        let remove = tokio::spawn(async move { handle.remove([4; 20], 3).await });
        let stopped = request_receiver.recv().await.expect("stopped announce");
        remove
            .await
            .expect("remove task")
            .expect("remove registration");
        let terminal = service.shutdown().await.expect("shutdown service");
        dht.shutdown().await.expect("shutdown DHT");
        tracker_task.await.expect("HTTP tracker task");

        assert!(started.starts_with("GET /announce?passkey=fixture&"));
        assert!(started.contains("&supportcrypto=1&"));
        assert!(!corrected.contains("supportcrypto"));
        assert!(!corrected.contains("&event="));
        assert!(started.contains("&port=48001&uploaded=7&downloaded=11&left=99&"));
        assert!(started.contains("&event=started "));
        assert!(!started.contains("trackerid="));
        assert!(completed.contains("&left=0&"));
        assert!(completed.contains("&event=completed&trackerid=%6E%65%78%74 "));
        assert!(stopped.contains("&numwant=0&event=stopped&trackerid=%6E%65%78%74 "));
        assert_eq!(terminal.registrations, 0);
        assert_eq!(terminal.tracker_operations, 0);
        assert_eq!(terminal.tracker_operations_high_water, 1);
        assert_eq!(terminal.tasks, 0);
    }

    #[tokio::test]
    async fn ipv6_http_tracker_observes_selected_source_and_family_port() {
        let tracker = TcpListener::bind("[::1]:0")
            .await
            .expect("bind IPv6 HTTP tracker");
        let tracker_address = tracker.local_addr().expect("IPv6 tracker address");
        let (observed_sender, mut observed_receiver) = mpsc::channel(2);
        let tracker_task = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut stream, remote) = tracker.accept().await.expect("accept IPv6 announce");
                let request = read_http_tracker_request(&mut stream).await;
                observed_sender
                    .send((remote, request))
                    .await
                    .expect("record IPv6 announce");
                stream
                    .write_all(empty_http_tracker_response())
                    .await
                    .expect("write IPv6 tracker response");
                stream.shutdown().await.expect("close IPv6 response");
            }
        });

        let listener_port = 48_006;
        let endpoint = PeerAdvertisementEndpoint {
            generation: 1,
            ipv4: PeerAdvertisementFamilyEndpoint::outbound_only(),
            ipv6: PeerAdvertisementFamilyEndpoint {
                endpoint: Some(format!("[::1]:{listener_port}").parse().unwrap()),
                source_address: Some("::1".parse().unwrap()),
                scope: Some(PeerAdvertisementEndpointScope::Loopback),
            },
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
        let activity = Arc::new(RecordingActivity::default());
        let tracker_url = format!("http://{tracker_address}/announce");
        let mut registration = http_registration([14; 20], tracker_url, activity.clone());
        registration.incoming_registered = true;
        service
            .handle()
            .upsert(registration)
            .await
            .expect("register IPv6 tracker");
        activity.wait_for_successes(1).await;
        let (remote, started) = observed_receiver
            .recv()
            .await
            .expect("started IPv6 announce");
        assert_eq!(remote.ip(), "::1".parse::<IpAddr>().unwrap());
        assert!(started.contains(&format!("&port={listener_port}&")));

        service.shutdown().await.expect("shutdown service");
        let (_remote, stopped) = observed_receiver
            .recv()
            .await
            .expect("stopped IPv6 announce");
        assert!(stopped.contains("event=stopped"));
        dht.shutdown().await.expect("shutdown DHT");
        tracker_task.await.expect("IPv6 tracker task");
    }

    #[tokio::test]
    async fn failed_secure_replacements_fence_https_but_keep_http_and_owner_alive() {
        let blocked_https = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind blocked HTTPS tracker");
        let blocked_https_address = blocked_https
            .local_addr()
            .expect("blocked HTTPS tracker address");
        let http_tracker = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind HTTP tracker");
        let http_address = http_tracker.local_addr().expect("HTTP tracker address");
        let http_task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for _ in 0..2 {
                let (mut stream, _) = http_tracker.accept().await.expect("accept HTTP announce");
                requests.push(read_http_tracker_request(&mut stream).await);
                stream
                    .write_all(empty_http_tracker_response())
                    .await
                    .expect("write HTTP tracker response");
                stream.shutdown().await.expect("close HTTP response");
            }
            requests
        });

        let (_endpoint_sender, endpoint_receiver) =
            watch::channel(PeerAdvertisementEndpoint::outbound_only(1));
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
        let service = DiscoveryAdvertisementService::start_with_http_client_factory(
            network,
            endpoint_receiver,
            dht.handle(),
            TrackerHttpsAuthentication::Disabled,
            reject_system_trust_client_factory,
        )
        .expect("start with disabled authentication");
        assert_eq!(
            service.initial_https_authentication(),
            Some(TrackerHttpsAuthentication::Disabled)
        );
        let handle = service.handle();

        for _ in 0..32 {
            assert!(
                handle
                    .replace_https_authentication(TrackerHttpsAuthentication::SystemTrust)
                    .await
                    .is_err()
            );
            handle
                .replace_https_authentication(TrackerHttpsAuthentication::Disabled)
                .await
                .expect("same-value recovery replacement");
        }
        assert!(
            handle
                .replace_https_authentication(TrackerHttpsAuthentication::SystemTrust)
                .await
                .is_err()
        );

        let https_activity = Arc::new(RecordingActivity::default());
        handle
            .upsert(http_registration(
                [8; 20],
                format!("https://{blocked_https_address}/announce?passkey=secret"),
                https_activity.clone(),
            ))
            .await
            .expect("register fenced HTTPS tracker");
        let failure =
            tokio::time::timeout(Duration::from_secs(2), https_activity.wait_for_failures(1))
                .await
                .expect("fenced HTTPS failure deadline");
        assert_eq!(failure, "HTTPS tracker authentication is unavailable");
        assert!(
            tokio::time::timeout(Duration::from_millis(200), blocked_https.accept())
                .await
                .is_err(),
            "fenced HTTPS work must fail before opening a socket"
        );

        let http_activity = Arc::new(RecordingActivity::default());
        handle
            .upsert(http_registration(
                [9; 20],
                format!("http://{http_address}/announce?passkey=secret"),
                http_activity.clone(),
            ))
            .await
            .expect("register surviving HTTP tracker");
        let http_result = tokio::time::timeout(Duration::from_secs(10), async {
            tokio::select! {
                _ = http_activity.wait_for_successes(1) => Ok(()),
                failure = http_activity.wait_for_failures(1) => Err(failure),
            }
        })
        .await
        .expect("HTTP result deadline");
        assert_eq!(http_result, Ok(()));

        handle
            .replace_https_authentication(TrackerHttpsAuthentication::Disabled)
            .await
            .expect("recover HTTPS eligibility");
        handle.remove([8; 20], 1).await.expect("remove HTTPS row");
        handle.remove([9; 20], 1).await.expect("remove HTTP row");
        let terminal = service.shutdown().await.expect("shutdown service");
        dht.shutdown().await.expect("shutdown DHT");
        let requests = http_task.await.expect("HTTP tracker task");

        assert!(requests[0].contains("event=started"));
        assert!(requests[1].contains("event=stopped"));
        assert_eq!(terminal.tasks, 0);
        assert_eq!(terminal.registrations, 0);
        assert_eq!(terminal.tracker_operations, 0);
        assert_eq!(terminal.tracker_operations_high_water, 1);
        assert_eq!(terminal.command_queue_high_water, 1);
    }

    #[tokio::test]
    async fn removal_cancels_a_stalled_http_announce_before_sending_stopped() {
        let tracker = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind stalled HTTP tracker");
        let tracker_address = tracker.local_addr().expect("HTTP tracker address");
        let (started_sender, started_receiver) = oneshot::channel();
        let (closed_sender, closed_receiver) = oneshot::channel();
        let tracker_task = tokio::spawn(async move {
            let (mut started_stream, _) = tracker.accept().await.expect("accept started announce");
            let started = read_http_tracker_request(&mut started_stream).await;
            started_sender
                .send(started)
                .expect("report started announce");
            let mut byte = [0_u8; 1];
            let closed = tokio::time::timeout(Duration::from_secs(1), async {
                loop {
                    if started_stream
                        .read(&mut byte)
                        .await
                        .expect("read cancelled stream")
                        == 0
                    {
                        break;
                    }
                }
            })
            .await
            .is_ok();
            closed_sender
                .send(closed)
                .expect("report cancelled connection");

            match tokio::time::timeout(Duration::from_millis(200), tracker.accept()).await {
                Ok(Ok((mut stopped_stream, _))) => {
                    let stopped = read_http_tracker_request(&mut stopped_stream).await;
                    stopped_stream
                        .write_all(empty_http_tracker_response())
                        .await
                        .expect("write stopped response");
                    Some(stopped)
                }
                Ok(Err(error)) => panic!("accept stopped announce: {error}"),
                Err(_) => None,
            }
        });

        let endpoint = PeerAdvertisementEndpoint::outbound_only(1);
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
        let tracker_url = format!("http://{tracker_address}/announce?passkey=redacted");
        handle
            .upsert(DiscoveryAdvertisementRegistration {
                generation: 9,
                info_hash: [9; 20],
                trackers: vec![TrackerConfig {
                    endpoint: TrackerEndpoint::from_http_url(&tracker_url)
                        .expect("HTTP tracker endpoint"),
                    url: tracker_url,
                    tier: 0,
                    position: 0,
                    source: crate::TrackerSource::Metainfo,
                }],
                desired_running: true,
                complete: false,
                incoming_registered: false,
                privacy: TorrentPrivacy::Private,
                counters: TrackerCounters::default(),
                peers: TorrentPeerHandle::new(Arc::new(NoopSink)).expect("peer registry"),
                activity_sink: Arc::new(NoopSink),
            })
            .await
            .expect("register stalled tracker");
        let started = tokio::time::timeout(Duration::from_secs(2), started_receiver)
            .await
            .expect("started announce deadline")
            .expect("started announce report");
        assert!(started.contains("&event=started "));

        let remove = tokio::spawn(async move { handle.remove([9; 20], 9).await });
        assert!(
            closed_receiver
                .await
                .expect("cancelled connection observation"),
            "removal must drop the stalled request before its ordinary timeout"
        );
        remove
            .await
            .expect("remove task")
            .expect("remove registration");
        let stopped = tracker_task.await.expect("stalled tracker task");
        if let Some(stopped) = stopped {
            assert!(stopped.contains("&numwant=0&event=stopped "));
        }

        let terminal = service.shutdown().await.expect("shutdown service");
        dht.shutdown().await.expect("shutdown DHT");
        assert_eq!(terminal.registrations, 0);
        assert_eq!(terminal.tracker_operations, 0);
        assert_eq!(terminal.tasks, 0);
    }

    #[tokio::test]
    async fn mixed_http_and_udp_trackers_share_exact_eight_operation_ceiling() {
        let http_tracker = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind HTTP tracker");
        let http_address = http_tracker.local_addr().expect("HTTP tracker address");
        let udp_tracker = Arc::new(
            UdpSocket::bind("127.0.0.1:0")
                .await
                .expect("bind UDP tracker"),
        );
        let udp_address = udp_tracker.local_addr().expect("UDP tracker address");
        let observed = Arc::new(AtomicUsize::new(0));
        let observed_changed = Arc::new(Notify::new());
        let (release_sender, release_receiver) = watch::channel(false);

        let http_observed = observed.clone();
        let http_changed = observed_changed.clone();
        let http_release = release_receiver.clone();
        let http_task = tokio::spawn(async move {
            let mut handlers = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = http_tracker.accept() => {
                        let (mut stream, _) = accepted.expect("accept HTTP operation");
                        let mut release = http_release.clone();
                        let observed = http_observed.clone();
                        let changed = http_changed.clone();
                        handlers.spawn(async move {
                            let _request = read_http_tracker_request(&mut stream).await;
                            if !*release.borrow() {
                                observed.fetch_add(1, Ordering::AcqRel);
                                changed.notify_waiters();
                                release.changed().await.expect("release HTTP operation");
                            }
                            stream
                                .write_all(empty_http_tracker_response())
                                .await
                                .expect("write HTTP response");
                        });
                    }
                    joined = handlers.join_next(), if !handlers.is_empty() => {
                        joined.expect("HTTP handler join").expect("HTTP handler");
                    }
                }
            }
        });

        let udp_socket = udp_tracker.clone();
        let udp_observed = observed.clone();
        let udp_changed = observed_changed.clone();
        let udp_task = tokio::spawn(async move {
            let mut packet = [0_u8; 2_048];
            let mut handlers = JoinSet::new();
            loop {
                tokio::select! {
                    received = udp_socket.recv_from(&mut packet) => {
                        let (length, client) = received.expect("receive UDP operation");
                        let request = packet[..length].to_vec();
                        let socket = udp_socket.clone();
                        let mut release = release_receiver.clone();
                        let observed = udp_observed.clone();
                        let changed = udp_changed.clone();
                        handlers.spawn(async move {
                            if !*release.borrow() {
                                observed.fetch_add(1, Ordering::AcqRel);
                                changed.notify_waiters();
                                release.changed().await.expect("release UDP operation");
                            }
                            let mut response = if request.len() == 16 {
                                let mut response = Vec::from(0_u32.to_be_bytes());
                                response.extend_from_slice(&request[12..16]);
                                response.extend_from_slice(&0x0102_0304_0506_0708_u64.to_be_bytes());
                                response
                            } else {
                                assert_eq!(request.len(), 98);
                                let mut response = Vec::from(1_u32.to_be_bytes());
                                response.extend_from_slice(&request[12..16]);
                                response.extend_from_slice(&900_u32.to_be_bytes());
                                response.extend_from_slice(&0_u32.to_be_bytes());
                                response.extend_from_slice(&0_u32.to_be_bytes());
                                response
                            };
                            socket
                                .send_to(&response, client)
                                .await
                                .expect("send UDP response");
                            response.clear();
                        });
                    }
                    joined = handlers.join_next(), if !handlers.is_empty() => {
                        joined.expect("UDP handler join").expect("UDP handler");
                    }
                }
            }
        });

        let endpoint = PeerAdvertisementEndpoint::outbound_only(1);
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
        for index in 0_u8..12 {
            let (url, endpoint) = if index % 2 == 0 {
                let url = format!("http://{http_address}/announce/{index}");
                let endpoint = TrackerEndpoint::from_http_url(&url).expect("HTTP tracker endpoint");
                (url, endpoint)
            } else {
                (
                    format!("udp://{udp_address}"),
                    TrackerEndpoint::Udp(rstorrent_protocol::magnet::UdpTrackerUrl {
                        host: udp_address.ip().to_string(),
                        port: udp_address.port(),
                    }),
                )
            };
            handle
                .upsert(DiscoveryAdvertisementRegistration {
                    generation: 1,
                    info_hash: [index; 20],
                    trackers: vec![TrackerConfig {
                        endpoint,
                        url,
                        tier: 0,
                        position: 0,
                        source: crate::TrackerSource::Metainfo,
                    }],
                    desired_running: true,
                    complete: false,
                    incoming_registered: false,
                    privacy: TorrentPrivacy::Private,
                    counters: TrackerCounters::default(),
                    peers: TorrentPeerHandle::new(Arc::new(NoopSink)).expect("peer registry"),
                    activity_sink: Arc::new(NoopSink),
                })
                .await
                .expect("register mixed tracker");
        }

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if observed.load(Ordering::Acquire) == MAX_TRACKER_OPERATIONS {
                    break;
                }
                observed_changed.notified().await;
            }
        })
        .await
        .expect("fill shared tracker operation ceiling");
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(observed.load(Ordering::Acquire), MAX_TRACKER_OPERATIONS);
        release_sender
            .send(true)
            .expect("release tracker operations");

        let terminal = tokio::time::timeout(Duration::from_secs(3), service.shutdown())
            .await
            .expect("bounded mixed tracker shutdown")
            .expect("shutdown service");
        dht.shutdown().await.expect("shutdown DHT");
        http_task.abort();
        udp_task.abort();
        let _ = http_task.await;
        let _ = udp_task.await;
        assert_eq!(
            terminal.tracker_operations_high_water,
            MAX_TRACKER_OPERATIONS
        );
        assert_eq!(terminal.tracker_operations, 0);
        assert_eq!(terminal.registrations, 0);
        assert_eq!(terminal.tasks, 0);
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
            ipv4: PeerAdvertisementFamilyEndpoint {
                endpoint: Some("203.0.113.9:48001".parse().expect("mapped endpoint")),
                source_address: Some(Ipv4Addr::LOCALHOST.into()),
                scope: Some(PeerAdvertisementEndpointScope::Mapped),
            },
            ipv6: PeerAdvertisementFamilyEndpoint::outbound_only(),
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
            trackers: vec![TrackerConfig {
                url: format!("udp://{tracker_address}"),
                endpoint: TrackerEndpoint::Udp(rstorrent_protocol::magnet::UdpTrackerUrl {
                    host: tracker_address.ip().to_string(),
                    port: tracker_address.port(),
                }),
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
        let endpoint = ipv4_endpoint(
            7,
            "127.0.0.1:55001",
            PeerAdvertisementEndpointScope::Loopback,
        );
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
            .send(ipv4_endpoint(
                8,
                "127.0.0.1:55002",
                PeerAdvertisementEndpointScope::Loopback,
            ))
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
