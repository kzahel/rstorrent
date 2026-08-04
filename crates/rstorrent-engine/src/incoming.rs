//! One loopback listener with generation-fenced torrent routing.

mod peer_io;
mod upload_runtime;

use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use rstorrent_protocol::metadata::{
    MetadataExtensionUpdate, MetadataMessage, MetadataUpload, MetadataUploadAction,
    UT_METADATA_LOCAL_ID, encode_extension_handshake, encode_metadata_data, encode_metadata_reject,
    parse_extension_handshake, parse_metadata_message,
};
use rstorrent_protocol::peer_wire::{
    BlockRequest, EXTENSION_PROTOCOL_RESERVED_BIT, EXTENSION_PROTOCOL_RESERVED_INDEX,
    HANDSHAKE_LENGTH, PeerMessage, decode_handshake, encode_handshake_with_reserved,
};
use sha1::{Digest, Sha1};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tokio::sync::{Mutex as AsyncMutex, Semaphore};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{Instant, timeout_at};
use tokio_util::sync::CancellationToken;

use crate::metrics::{ByteMetric, ByteMetricSink};
use crate::network::DEFAULT_PEER_ID;
use crate::peer_budget::{
    DEFAULT_LISTEN_BACKLOG, PeerBudget, PeerBudgetDirection, PeerBudgetPermit, PeerBudgetSnapshot,
};
use crate::peer_io::{PeerIo, record_bytes};
use crate::seed_content::SeedContent;
use crate::upload::{UploadAction, UploadCloseReason, UploadPeerState, UploadRead};
use crate::upload_scheduler::{UploadGrant, UploadSchedulerConfig, UploadSchedulerSnapshot};

use self::peer_io::{FrameValidity, IncomingPeerIo};
use self::upload_runtime::UploadCoordinator;

pub const MAX_SEED_REGISTRATIONS: usize = 1024;
pub const MAX_INCOMING_PENDING: usize = 8;
pub const DEFAULT_UPLOAD_READ_JOBS: usize = 10;
pub const MAX_CONFIGURED_UPLOAD_READ_JOBS: usize = 1_024;
pub const MAX_DEFERRED_METADATA_REQUESTS: usize = 1_024;
pub const METADATA_SEND_BUFFER_WATERMARK: usize = 160 * 1_024;
pub const DEFAULT_INCOMING_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
pub const DEFAULT_INCOMING_PEER_ACTIVITY_TIMEOUT: Duration = Duration::from_secs(120);
pub const DEFAULT_INCOMING_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(60);
pub const DEFAULT_INCOMING_NO_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
pub const DEFAULT_INCOMING_INACTIVITY_TIMEOUT: Duration = Duration::from_secs(600);
pub const MAX_INCOMING_WRITER_BYTES: usize = peer_io::MAX_INCOMING_WRITER_BYTES;
pub const INCOMING_WRITER_NO_PROGRESS_TIMEOUT: Duration =
    peer_io::INCOMING_WRITER_NO_PROGRESS_TIMEOUT;
const MAX_RECENT_REJECTIONS: usize = 32;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum IncomingTcpBootstrap {
    #[default]
    Disabled,
    AutomaticLoopback,
    FixedLoopback(u16),
}

#[derive(Clone, Debug)]
pub struct IncomingPeerServiceConfig {
    pub bootstrap: IncomingTcpBootstrap,
    pub handshake_timeout: Duration,
    pub peer_activity_timeout: Duration,
    pub keepalive_interval: Duration,
    pub no_request_timeout: Duration,
    pub inactivity_timeout: Duration,
    pub peer_id: [u8; 20],
    pub byte_metric_sink: Option<Arc<dyn ByteMetricSink>>,
    pub peer_budget: PeerBudget,
    pub upload_scheduler: UploadSchedulerConfig,
    pub upload_read_jobs: usize,
}

impl IncomingPeerServiceConfig {
    pub fn new(bootstrap: IncomingTcpBootstrap) -> Self {
        Self {
            bootstrap,
            handshake_timeout: DEFAULT_INCOMING_HANDSHAKE_TIMEOUT,
            peer_activity_timeout: DEFAULT_INCOMING_PEER_ACTIVITY_TIMEOUT,
            keepalive_interval: DEFAULT_INCOMING_KEEPALIVE_INTERVAL,
            no_request_timeout: DEFAULT_INCOMING_NO_REQUEST_TIMEOUT,
            inactivity_timeout: DEFAULT_INCOMING_INACTIVITY_TIMEOUT,
            peer_id: DEFAULT_PEER_ID,
            byte_metric_sink: None,
            peer_budget: PeerBudget::system_default(),
            upload_scheduler: UploadSchedulerConfig::default(),
            upload_read_jobs: DEFAULT_UPLOAD_READ_JOBS,
        }
    }

    pub fn with_peer_budget(mut self, peer_budget: PeerBudget) -> Self {
        self.peer_budget = peer_budget;
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SeedRegistrationToken {
    pub info_hash: [u8; 20],
    pub generation: u64,
}

#[derive(Clone, Debug)]
pub struct SeedRegistration {
    info_hash: [u8; 20],
    raw_info: Arc<[u8]>,
    content: SeedContent,
    piece_lengths: Arc<[u32]>,
    availability: Arc<[bool]>,
}

impl SeedRegistration {
    pub fn new(raw_info: Vec<u8>, content: SeedContent) -> Result<Self, IncomingPeerError> {
        let info_hash: [u8; 20] = Sha1::digest(&raw_info).into();
        if info_hash != content.info_hash() {
            return Err(IncomingPeerError::InvalidRegistration(
                "metadata and seed content identities differ",
            ));
        }
        let raw_info: Arc<[u8]> = raw_info.into();
        MetadataUpload::new(&raw_info).map_err(|_| {
            IncomingPeerError::InvalidRegistration("metadata exceeds upload limits")
        })?;
        let piece_lengths = content
            .piece_lengths()
            .map_err(|_| IncomingPeerError::InvalidRegistration("invalid seed piece geometry"))?;
        let availability = Arc::from(content.availability());
        Ok(Self {
            info_hash,
            raw_info,
            content,
            piece_lengths: piece_lengths.into(),
            availability,
        })
    }

    pub fn info_hash(&self) -> [u8; 20] {
        self.info_hash
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum IncomingRejectionReason {
    PendingLimit,
    HandshakeTimeout,
    HandshakeInvalid,
    UnknownTorrent,
    StaleRegistration,
    SelfConnection,
    ConnectionLimit,
    ActivityTimeout,
    NoRequestTimeout,
    InactivityTimeout,
    Protocol,
    Storage,
    Accept,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IncomingRejection {
    pub reason: IncomingRejectionReason,
    pub remote: Option<SocketAddr>,
    pub info_hash: Option<[u8; 20]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncomingPeerServiceSnapshot {
    pub bootstrap: IncomingTcpBootstrap,
    pub listen_address: SocketAddr,
    pub registrations: usize,
    pub pending: usize,
    pub pending_high_water: usize,
    pub established: usize,
    pub established_high_water: usize,
    pub peer_budget: PeerBudgetSnapshot,
    pub upload_scheduler: UploadSchedulerSnapshot,
    pub upload_read_limit: usize,
    pub reads: usize,
    pub read_bytes: usize,
    pub queued_requests_high_water: usize,
    pub queued_bytes_high_water: usize,
    pub metadata_requests_high_water: usize,
    pub metadata_send_buffer_high_water: usize,
    pub writer_send_buffer_high_water: usize,
    pub upload_regular_high_water: usize,
    pub upload_optimistic_high_water: usize,
    pub upload_slots_high_water: usize,
    pub read_high_water: usize,
    pub read_bytes_high_water: usize,
    pub payload_bytes_sent: u64,
    pub rejection_counts: BTreeMap<IncomingRejectionReason, u64>,
    pub recent_rejections: Vec<IncomingRejection>,
    pub accepting_registrations: bool,
}

#[derive(Debug, Default)]
struct ObservationState {
    pending: usize,
    pending_high_water: usize,
    established: usize,
    established_high_water: usize,
    reads: usize,
    read_bytes: usize,
    queued_requests_high_water: usize,
    queued_bytes_high_water: usize,
    metadata_requests_high_water: usize,
    metadata_send_buffer_high_water: usize,
    writer_send_buffer_high_water: usize,
    upload_regular_high_water: usize,
    upload_optimistic_high_water: usize,
    upload_slots_high_water: usize,
    read_high_water: usize,
    read_bytes_high_water: usize,
    payload_bytes_sent: u64,
    rejection_counts: BTreeMap<IncomingRejectionReason, u64>,
    recent_rejections: VecDeque<IncomingRejection>,
}

#[derive(Debug)]
struct Shared {
    bootstrap: IncomingTcpBootstrap,
    listen_address: SocketAddr,
    registry: Mutex<BTreeMap<[u8; 20], Arc<RegistrationRuntime>>>,
    mutations: AsyncMutex<()>,
    accepting_registrations: AtomicBool,
    next_generation: AtomicU64,
    peer_budget: PeerBudget,
    upload_coordinator: UploadCoordinator,
    upload_reads: Arc<Semaphore>,
    upload_read_limit: usize,
    observations: Mutex<ObservationState>,
    peer_activity_timeout: Duration,
    keepalive_interval: Duration,
    no_request_timeout: Duration,
    inactivity_timeout: Duration,
    peer_id: [u8; 20],
    byte_metric_sink: Option<Arc<dyn ByteMetricSink>>,
}

#[derive(Debug)]
struct RegistrationRuntime {
    generation: u64,
    data: Arc<SeedRegistration>,
    accepting: AtomicBool,
    healthy: AtomicBool,
    cancellation: CancellationToken,
    peers: AsyncMutex<JoinSet<()>>,
}

impl RegistrationRuntime {
    fn new(generation: u64, registration: SeedRegistration) -> Self {
        Self {
            generation,
            data: Arc::new(registration),
            accepting: AtomicBool::new(true),
            healthy: AtomicBool::new(true),
            cancellation: CancellationToken::new(),
            peers: AsyncMutex::new(JoinSet::new()),
        }
    }

    async fn admit(
        self: &Arc<Self>,
        stream: TcpStream,
        remote: SocketAddr,
        supports_extensions: bool,
        permit: PeerBudgetPermit,
        shared: Arc<Shared>,
    ) -> bool {
        let mut peers = self.peers.lock().await;
        while let Some(joined) = peers.try_join_next() {
            if joined.is_err() {
                shared.reject(
                    IncomingRejectionReason::Protocol,
                    None,
                    Some(self.data.info_hash),
                );
            }
        }
        if !self.accepting.load(Ordering::Acquire)
            || !self.healthy.load(Ordering::Acquire)
            || self.cancellation.is_cancelled()
        {
            return false;
        }
        let data = self.data.clone();
        let cancellation = self.cancellation.clone();
        let registration = self.clone();
        let piece_length = data.piece_lengths.first().copied().unwrap_or(1);
        let membership = shared
            .upload_coordinator
            .register(data.info_hash, piece_length);
        peers.spawn(async move {
            let _membership = UploadMembershipGuard {
                shared: shared.clone(),
                id: membership.id,
            };
            let _established = ObservationGuard::established(&shared);
            let termination = run_incoming_peer(
                stream,
                supports_extensions,
                data,
                cancellation,
                shared.clone(),
                membership.id,
                membership.grants,
            )
            .await;
            drop(permit);
            match termination {
                PeerTermination::Storage => {
                    registration.healthy.store(false, Ordering::Release);
                    registration.cancellation.cancel();
                    shared.reject(
                        IncomingRejectionReason::Storage,
                        Some(remote),
                        Some(registration.data.info_hash),
                    );
                }
                PeerTermination::Protocol => shared.reject(
                    IncomingRejectionReason::Protocol,
                    Some(remote),
                    Some(registration.data.info_hash),
                ),
                PeerTermination::ActivityTimeout => shared.reject(
                    IncomingRejectionReason::ActivityTimeout,
                    Some(remote),
                    Some(registration.data.info_hash),
                ),
                PeerTermination::NoRequestTimeout => shared.reject(
                    IncomingRejectionReason::NoRequestTimeout,
                    Some(remote),
                    Some(registration.data.info_hash),
                ),
                PeerTermination::InactivityTimeout => shared.reject(
                    IncomingRejectionReason::InactivityTimeout,
                    Some(remote),
                    Some(registration.data.info_hash),
                ),
                PeerTermination::Closed | PeerTermination::Cancelled => {}
            }
        });
        true
    }

    async fn shutdown(&self) -> Result<(), IncomingPeerError> {
        self.accepting.store(false, Ordering::Release);
        self.cancellation.cancel();
        let mut peers = self.peers.lock().await;
        while let Some(joined) = peers.join_next().await {
            joined.map_err(|error| IncomingPeerError::TaskJoin(error.to_string()))?;
        }
        Ok(())
    }
}

struct UploadMembershipGuard {
    shared: Arc<Shared>,
    id: crate::upload_scheduler::UploadPeerId,
}

impl Drop for UploadMembershipGuard {
    fn drop(&mut self) {
        self.shared.upload_coordinator.remove(self.id);
    }
}

#[derive(Clone, Debug)]
pub struct IncomingPeerHandle {
    shared: Arc<Shared>,
}

impl IncomingPeerHandle {
    pub async fn register(
        &self,
        registration: SeedRegistration,
    ) -> Result<SeedRegistrationToken, IncomingPeerError> {
        if !self.shared.accepting_registrations.load(Ordering::Acquire) {
            return Err(IncomingPeerError::Closed);
        }
        let _mutation = self.shared.mutations.lock().await;
        if !self.shared.accepting_registrations.load(Ordering::Acquire) {
            return Err(IncomingPeerError::Closed);
        }
        let info_hash = registration.info_hash;
        let old = {
            let mut registry = self.shared.registry_guard();
            if !registry.contains_key(&info_hash) && registry.len() == MAX_SEED_REGISTRATIONS {
                return Err(IncomingPeerError::RegistrationLimit {
                    maximum: MAX_SEED_REGISTRATIONS,
                });
            }
            registry.remove(&info_hash)
        };
        if let Some(old) = old {
            old.shutdown().await?;
        }
        let generation = self
            .shared
            .next_generation
            .fetch_add(1, Ordering::AcqRel)
            .max(1);
        self.shared.registry_guard().insert(
            info_hash,
            Arc::new(RegistrationRuntime::new(generation, registration)),
        );
        Ok(SeedRegistrationToken {
            info_hash,
            generation,
        })
    }

    pub async fn unregister(
        &self,
        token: SeedRegistrationToken,
    ) -> Result<bool, IncomingPeerError> {
        let _mutation = self.shared.mutations.lock().await;
        let registration = {
            let mut registry = self.shared.registry_guard();
            match registry.get(&token.info_hash) {
                Some(registration) if registration.generation == token.generation => {
                    registry.remove(&token.info_hash)
                }
                _ => None,
            }
        };
        let Some(registration) = registration else {
            return Ok(false);
        };
        registration.shutdown().await?;
        Ok(true)
    }

    pub fn snapshot(&self) -> IncomingPeerServiceSnapshot {
        self.shared.snapshot()
    }
}

#[derive(Debug)]
pub struct IncomingPeerService {
    handle: IncomingPeerHandle,
    cancellation: CancellationToken,
    accept_task: Option<JoinHandle<()>>,
    upload_task: Option<JoinHandle<()>>,
}

impl IncomingPeerService {
    pub async fn bind(
        config: IncomingPeerServiceConfig,
    ) -> Result<Option<Self>, IncomingPeerError> {
        if config.handshake_timeout.is_zero()
            || config.peer_activity_timeout.is_zero()
            || config.keepalive_interval.is_zero()
            || config.no_request_timeout.is_zero()
            || config.inactivity_timeout.is_zero()
        {
            return Err(IncomingPeerError::InvalidTimeout);
        }
        if !(1..=MAX_CONFIGURED_UPLOAD_READ_JOBS).contains(&config.upload_read_jobs) {
            return Err(IncomingPeerError::InvalidUploadReadJobs {
                maximum: MAX_CONFIGURED_UPLOAD_READ_JOBS,
            });
        }
        let upload_coordinator = UploadCoordinator::new(config.upload_scheduler)
            .map_err(IncomingPeerError::InvalidScheduler)?;
        let upload_interval = config
            .upload_scheduler
            .unchoke_interval
            .min(config.upload_scheduler.optimistic_interval);
        let port = match config.bootstrap {
            IncomingTcpBootstrap::Disabled => return Ok(None),
            IncomingTcpBootstrap::AutomaticLoopback => 0,
            IncomingTcpBootstrap::FixedLoopback(0) => {
                return Err(IncomingPeerError::InvalidFixedPort);
            }
            IncomingTcpBootstrap::FixedLoopback(port) => port,
        };
        let socket =
            TcpSocket::new_v4().map_err(|source| IncomingPeerError::Bind { port, source })?;
        socket
            .bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port).into())
            .map_err(|source| IncomingPeerError::Bind { port, source })?;
        let listener = socket
            .listen(DEFAULT_LISTEN_BACKLOG)
            .map_err(|source| IncomingPeerError::Bind { port, source })?;
        let listen_address = listener
            .local_addr()
            .map_err(|source| IncomingPeerError::Io {
                operation: "read incoming listener address",
                source,
            })?;
        let shared = Arc::new(Shared {
            bootstrap: config.bootstrap,
            listen_address,
            registry: Mutex::new(BTreeMap::new()),
            mutations: AsyncMutex::new(()),
            accepting_registrations: AtomicBool::new(true),
            next_generation: AtomicU64::new(1),
            peer_budget: config.peer_budget,
            upload_coordinator,
            upload_reads: Arc::new(Semaphore::new(config.upload_read_jobs)),
            upload_read_limit: config.upload_read_jobs,
            observations: Mutex::new(ObservationState::default()),
            peer_activity_timeout: config.peer_activity_timeout,
            keepalive_interval: config.keepalive_interval,
            no_request_timeout: config.no_request_timeout,
            inactivity_timeout: config.inactivity_timeout,
            peer_id: config.peer_id,
            byte_metric_sink: config.byte_metric_sink,
        });
        let cancellation = CancellationToken::new();
        let accept_task = tokio::spawn(run_accept_loop(
            listener,
            config.handshake_timeout,
            shared.clone(),
            cancellation.clone(),
        ));
        let upload_task = tokio::spawn(run_upload_scheduler(
            shared.clone(),
            cancellation.clone(),
            upload_interval,
        ));
        Ok(Some(Self {
            handle: IncomingPeerHandle { shared },
            cancellation,
            accept_task: Some(accept_task),
            upload_task: Some(upload_task),
        }))
    }

    pub fn handle(&self) -> IncomingPeerHandle {
        self.handle.clone()
    }

    pub fn listen_address(&self) -> SocketAddr {
        self.handle.shared.listen_address
    }

    pub fn snapshot(&self) -> IncomingPeerServiceSnapshot {
        self.handle.snapshot()
    }

    pub async fn shutdown(mut self) -> Result<IncomingPeerServiceSnapshot, IncomingPeerError> {
        self.handle
            .shared
            .accepting_registrations
            .store(false, Ordering::Release);
        self.cancellation.cancel();
        self.accept_task
            .take()
            .expect("incoming accept task exists before shutdown")
            .await
            .map_err(|error| IncomingPeerError::TaskJoin(error.to_string()))?;
        self.upload_task
            .take()
            .expect("incoming upload task exists before shutdown")
            .await
            .map_err(|error| IncomingPeerError::TaskJoin(error.to_string()))?;
        let _mutation = self.handle.shared.mutations.lock().await;
        let registrations = {
            let mut registry = self.handle.shared.registry_guard();
            std::mem::take(&mut *registry)
                .into_values()
                .collect::<Vec<_>>()
        };
        for registration in registrations {
            registration.shutdown().await?;
        }
        Ok(self.handle.snapshot())
    }
}

impl Drop for IncomingPeerService {
    fn drop(&mut self) {
        self.handle
            .shared
            .accepting_registrations
            .store(false, Ordering::Release);
        self.cancellation.cancel();
    }
}

impl Shared {
    fn registry_guard(&self) -> MutexGuard<'_, BTreeMap<[u8; 20], Arc<RegistrationRuntime>>> {
        self.registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn observations_guard(&self) -> MutexGuard<'_, ObservationState> {
        self.observations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn reject(
        &self,
        reason: IncomingRejectionReason,
        remote: Option<SocketAddr>,
        info_hash: Option<[u8; 20]>,
    ) {
        let mut observations = self.observations_guard();
        *observations.rejection_counts.entry(reason).or_default() += 1;
        if observations.recent_rejections.len() == MAX_RECENT_REJECTIONS {
            observations.recent_rejections.pop_front();
        }
        observations.recent_rejections.push_back(IncomingRejection {
            reason,
            remote,
            info_hash,
        });
    }

    fn snapshot(&self) -> IncomingPeerServiceSnapshot {
        let observations = self.observations_guard();
        IncomingPeerServiceSnapshot {
            bootstrap: self.bootstrap,
            listen_address: self.listen_address,
            registrations: self.registry_guard().len(),
            pending: observations.pending,
            pending_high_water: observations.pending_high_water,
            established: observations.established,
            established_high_water: observations.established_high_water,
            peer_budget: self.peer_budget.snapshot(),
            upload_scheduler: self.upload_coordinator.snapshot(),
            upload_read_limit: self.upload_read_limit,
            reads: observations.reads,
            read_bytes: observations.read_bytes,
            queued_requests_high_water: observations.queued_requests_high_water,
            queued_bytes_high_water: observations.queued_bytes_high_water,
            metadata_requests_high_water: observations.metadata_requests_high_water,
            metadata_send_buffer_high_water: observations.metadata_send_buffer_high_water,
            writer_send_buffer_high_water: observations.writer_send_buffer_high_water,
            upload_regular_high_water: observations.upload_regular_high_water,
            upload_optimistic_high_water: observations.upload_optimistic_high_water,
            upload_slots_high_water: observations.upload_slots_high_water,
            read_high_water: observations.read_high_water,
            read_bytes_high_water: observations.read_bytes_high_water,
            payload_bytes_sent: observations.payload_bytes_sent,
            rejection_counts: observations.rejection_counts.clone(),
            recent_rejections: observations.recent_rejections.iter().copied().collect(),
            accepting_registrations: self.accepting_registrations.load(Ordering::Acquire),
        }
    }
}

struct ObservationGuard {
    shared: Arc<Shared>,
    kind: ObservationKind,
}

enum ObservationKind {
    Pending,
    Established,
    Read(usize),
}

impl ObservationGuard {
    fn pending(shared: &Arc<Shared>) -> Self {
        {
            let mut observations = shared.observations_guard();
            observations.pending += 1;
            observations.pending_high_water =
                observations.pending_high_water.max(observations.pending);
        }
        Self {
            shared: shared.clone(),
            kind: ObservationKind::Pending,
        }
    }

    fn established(shared: &Arc<Shared>) -> Self {
        {
            let mut observations = shared.observations_guard();
            observations.established += 1;
            observations.established_high_water = observations
                .established_high_water
                .max(observations.established);
        }
        Self {
            shared: shared.clone(),
            kind: ObservationKind::Established,
        }
    }

    fn read(shared: &Arc<Shared>, bytes: usize) -> Self {
        {
            let mut observations = shared.observations_guard();
            observations.reads += 1;
            observations.read_bytes += bytes;
            observations.read_high_water = observations.read_high_water.max(observations.reads);
            observations.read_bytes_high_water = observations
                .read_bytes_high_water
                .max(observations.read_bytes);
        }
        Self {
            shared: shared.clone(),
            kind: ObservationKind::Read(bytes),
        }
    }
}

impl Drop for ObservationGuard {
    fn drop(&mut self) {
        let mut observations = self.shared.observations_guard();
        match self.kind {
            ObservationKind::Pending => observations.pending -= 1,
            ObservationKind::Established => observations.established -= 1,
            ObservationKind::Read(bytes) => {
                observations.reads -= 1;
                observations.read_bytes -= bytes;
            }
        }
    }
}

async fn run_accept_loop(
    listener: TcpListener,
    handshake_timeout: Duration,
    shared: Arc<Shared>,
    cancellation: CancellationToken,
) {
    let pending = Arc::new(Semaphore::new(MAX_INCOMING_PENDING));
    let mut handshakes = JoinSet::new();
    loop {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => break,
            joined = handshakes.join_next(), if !handshakes.is_empty() => {
                if joined.is_some_and(|result| result.is_err()) {
                    shared.reject(IncomingRejectionReason::Protocol, None, None);
                }
            }
            accepted = listener.accept() => match accepted {
                Ok((stream, remote)) => {
                    let Ok(permit) = pending.clone().try_acquire_owned() else {
                        shared.reject(IncomingRejectionReason::PendingLimit, Some(remote), None);
                        continue;
                    };
                    let Ok(budget_permit) = shared
                        .peer_budget
                        .try_acquire(PeerBudgetDirection::Incoming)
                    else {
                        shared.reject(
                            IncomingRejectionReason::ConnectionLimit,
                            Some(remote),
                            None,
                        );
                        continue;
                    };
                    let shared = shared.clone();
                    let cancellation = cancellation.clone();
                    handshakes.spawn(async move {
                        let _pending = ObservationGuard::pending(&shared);
                        run_handshake(
                            stream,
                            remote,
                            handshake_timeout,
                            shared,
                            cancellation,
                            budget_permit,
                        )
                        .await;
                        drop(permit);
                    });
                }
                Err(_) => {
                    shared.reject(IncomingRejectionReason::Accept, None, None);
                    tokio::task::yield_now().await;
                }
            },
        }
    }
    handshakes.abort_all();
    while handshakes.join_next().await.is_some() {}
}

async fn run_upload_scheduler(
    shared: Arc<Shared>,
    cancellation: CancellationToken,
    cadence: Duration,
) {
    let mut interval = tokio::time::interval(cadence);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => break,
            _ = interval.tick() => shared.upload_coordinator.evaluate(),
        }
    }
}

async fn run_handshake(
    mut stream: TcpStream,
    remote: SocketAddr,
    timeout: Duration,
    shared: Arc<Shared>,
    cancellation: CancellationToken,
    mut budget_permit: PeerBudgetPermit,
) {
    let deadline = Instant::now() + timeout;
    let mut bytes = [0; HANDSHAKE_LENGTH];
    let read = tokio::select! {
        biased;
        _ = cancellation.cancelled() => return,
        result = timeout_at(deadline, stream.read_exact(&mut bytes)) => result,
    };
    match read {
        Err(_) => {
            shared.reject(
                IncomingRejectionReason::HandshakeTimeout,
                Some(remote),
                None,
            );
            return;
        }
        Ok(Err(_)) => {
            shared.reject(
                IncomingRejectionReason::HandshakeInvalid,
                Some(remote),
                None,
            );
            return;
        }
        Ok(Ok(_)) => {}
    }
    record_bytes(
        shared.byte_metric_sink.as_ref(),
        ByteMetric::PeerWireReceived,
        bytes.len(),
    );
    record_bytes(
        shared.byte_metric_sink.as_ref(),
        ByteMetric::PeerProtocolReceived,
        bytes.len(),
    );
    let info_hash: [u8; 20] = bytes[28..48]
        .try_into()
        .expect("handshake info hash has a fixed length");
    let handshake = match decode_handshake(&bytes, info_hash) {
        Ok(handshake) => handshake,
        Err(_) => {
            shared.reject(
                IncomingRejectionReason::HandshakeInvalid,
                Some(remote),
                Some(info_hash),
            );
            return;
        }
    };
    if handshake.peer_id == shared.peer_id {
        shared.reject(
            IncomingRejectionReason::SelfConnection,
            Some(remote),
            Some(info_hash),
        );
        return;
    }
    let registration = shared.registry_guard().get(&info_hash).cloned();
    let Some(registration) = registration else {
        shared.reject(
            IncomingRejectionReason::UnknownTorrent,
            Some(remote),
            Some(info_hash),
        );
        return;
    };
    if !registration.accepting.load(Ordering::Acquire)
        || !registration.healthy.load(Ordering::Acquire)
    {
        shared.reject(
            IncomingRejectionReason::StaleRegistration,
            Some(remote),
            Some(info_hash),
        );
        return;
    }
    let mut reserved = [0; 8];
    reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
    let response = encode_handshake_with_reserved(info_hash, shared.peer_id, reserved);
    let write = tokio::select! {
        biased;
        _ = cancellation.cancelled() => return,
        result = timeout_at(deadline, stream.write_all(&response)) => result,
    };
    if !matches!(write, Ok(Ok(()))) {
        shared.reject(
            IncomingRejectionReason::HandshakeTimeout,
            Some(remote),
            Some(info_hash),
        );
        return;
    }
    record_bytes(
        shared.byte_metric_sink.as_ref(),
        ByteMetric::PeerWireSent,
        response.len(),
    );
    record_bytes(
        shared.byte_metric_sink.as_ref(),
        ByteMetric::PeerProtocolSent,
        response.len(),
    );
    budget_permit.mark_established();
    if !registration
        .admit(
            stream,
            remote,
            handshake.supports_extensions(),
            budget_permit,
            shared.clone(),
        )
        .await
    {
        shared.reject(
            IncomingRejectionReason::StaleRegistration,
            Some(remote),
            Some(info_hash),
        );
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PeerTermination {
    Closed,
    Cancelled,
    ActivityTimeout,
    NoRequestTimeout,
    InactivityTimeout,
    Protocol,
    Storage,
}

type ActiveRead = (UploadRead, JoinHandle<Result<Vec<u8>, ()>>);

#[derive(Default)]
struct QueuedPieceFrames {
    frames: Vec<(BlockRequest, Weak<FrameValidity>)>,
}

#[derive(Default)]
struct QueuedChokeFrame {
    latest: Option<Weak<FrameValidity>>,
}

impl QueuedChokeFrame {
    fn replace(&mut self) -> Arc<FrameValidity> {
        if let Some(validity) = self.latest.take().and_then(|validity| validity.upgrade()) {
            validity.cancel();
        }
        let validity = Arc::new(FrameValidity::new());
        self.latest = Some(Arc::downgrade(&validity));
        validity
    }
}

impl QueuedPieceFrames {
    fn track(&mut self, request: BlockRequest) -> Arc<FrameValidity> {
        self.frames
            .retain(|(_, validity)| validity.strong_count() != 0);
        let validity = Arc::new(FrameValidity::new());
        self.frames.push((request, Arc::downgrade(&validity)));
        validity
    }

    fn cancel(&mut self, request: BlockRequest) {
        self.frames.retain(|(queued, validity)| {
            let Some(validity) = validity.upgrade() else {
                return false;
            };
            if *queued == request {
                validity.cancel();
            }
            true
        });
    }

    fn cancel_all(&mut self) {
        self.frames.retain(|(_, validity)| {
            let Some(validity) = validity.upgrade() else {
                return false;
            };
            validity.cancel();
            true
        });
    }
}

const MIN_UPLOAD_SEND_TARGET: usize = 10 * 1_024;
const MAX_UPLOAD_SEND_TARGET: usize = 500 * 1_024;
const UPLOAD_SEND_TARGET_FACTOR_PERCENT: u64 = 50;

struct UploadSendTarget {
    window_started: Instant,
    window_payload: u64,
    target: usize,
}

impl UploadSendTarget {
    fn new(payload: u64) -> Self {
        Self {
            window_started: Instant::now(),
            window_payload: payload,
            target: MIN_UPLOAD_SEND_TARGET,
        }
    }

    fn observe(&mut self, payload: u64) {
        let elapsed = self.window_started.elapsed();
        if elapsed < Duration::from_secs(1) {
            return;
        }
        let millis = u64::try_from(elapsed.as_millis())
            .unwrap_or(u64::MAX)
            .max(1);
        let per_second = payload
            .saturating_sub(self.window_payload)
            .saturating_mul(1_000)
            / millis;
        let target = per_second.saturating_mul(UPLOAD_SEND_TARGET_FACTOR_PERCENT) / 100;
        self.target = usize::try_from(target)
            .unwrap_or(usize::MAX)
            .clamp(MIN_UPLOAD_SEND_TARGET, MAX_UPLOAD_SEND_TARGET);
        self.window_started = Instant::now();
        self.window_payload = payload;
    }
}

async fn run_incoming_peer(
    stream: TcpStream,
    supports_extensions: bool,
    registration: Arc<SeedRegistration>,
    cancellation: CancellationToken,
    shared: Arc<Shared>,
    upload_peer: crate::upload_scheduler::UploadPeerId,
    mut grants: tokio::sync::watch::Receiver<UploadGrant>,
) -> PeerTermination {
    let mut io = IncomingPeerIo::new(
        stream,
        shared.peer_activity_timeout,
        shared.byte_metric_sink.clone(),
    );
    let termination = run_incoming_peer_loop(
        &mut io,
        supports_extensions,
        registration,
        cancellation,
        shared.clone(),
        upload_peer,
        &mut grants,
    )
    .await;
    let payload = io.uploaded_payload_bytes();
    if payload != 0 {
        let mut observations = shared.observations_guard();
        observations.payload_bytes_sent = observations.payload_bytes_sent.saturating_add(payload);
        shared
            .upload_coordinator
            .update_payload(upload_peer, payload);
    }
    match (termination, io.shutdown().await) {
        (PeerTermination::Cancelled, _) => PeerTermination::Cancelled,
        (termination, Ok(())) => termination,
        (_, Err(_)) => PeerTermination::Closed,
    }
}

async fn run_incoming_peer_loop(
    io: &mut IncomingPeerIo,
    supports_extensions: bool,
    registration: Arc<SeedRegistration>,
    cancellation: CancellationToken,
    shared: Arc<Shared>,
    upload_peer: crate::upload_scheduler::UploadPeerId,
    grants: &mut tokio::sync::watch::Receiver<UploadGrant>,
) -> PeerTermination {
    let mut upload = match UploadPeerState::from_shared(
        registration.piece_lengths.clone(),
        registration.availability.clone(),
    ) {
        Ok(upload) => upload,
        Err(_) => return PeerTermination::Storage,
    };
    if io
        .send_message(&PeerMessage::Bitfield(upload.bitfield()))
        .await
        .is_err()
    {
        return PeerTermination::Closed;
    }
    if supports_extensions
        && io
            .send_message(&PeerMessage::Extended {
                id: 0,
                payload: encode_extension_handshake(Some(registration.raw_info.len())),
            })
            .await
            .is_err()
    {
        return PeerTermination::Closed;
    }
    let mut metadata = match MetadataUpload::new(&registration.raw_info) {
        Ok(metadata) => metadata,
        Err(_) => return PeerTermination::Storage,
    };
    let mut remote_metadata_id = None;
    let mut deferred_metadata = VecDeque::new();
    let mut read: Option<ActiveRead> = None;
    let mut queued_piece_frames = QueuedPieceFrames::default();
    let mut queued_choke_frame = QueuedChokeFrame::default();
    let mut accounted_payload = io.uploaded_payload_bytes();
    let mut send_target = UploadSendTarget::new(accounted_payload);
    let maintenance_cadence = Duration::from_secs(1)
        .min(shared.peer_activity_timeout)
        .min(shared.keepalive_interval)
        .min(shared.no_request_timeout)
        .min(shared.inactivity_timeout);
    let mut maintenance = tokio::time::interval(maintenance_cadence);
    maintenance.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_peer_activity = Instant::now();
    let mut last_meaningful_activity = last_peer_activity;
    let mut last_request_or_unchoke = last_peer_activity;
    let mut last_keepalive = last_peer_activity;
    loop {
        send_target.observe(io.uploaded_payload_bytes());
        let ready = io.send_buffer_size() < send_target.target;
        if let Some(termination) = apply_upload_actions(
            upload.set_read_enabled(ready),
            io,
            &mut read,
            &mut queued_piece_frames,
            &mut queued_choke_frame,
            &registration,
            &shared,
        )
        .await
        {
            return termination;
        }
        if drain_metadata_requests(
            io,
            &mut metadata,
            remote_metadata_id,
            &mut deferred_metadata,
        )
        .is_err()
        {
            join_read(read.take()).await;
            return PeerTermination::Closed;
        }
        let event = tokio::select! {
            biased;
            _ = cancellation.cancelled() => PeerEvent::Cancelled,
            _ = maintenance.tick() => PeerEvent::Maintenance,
            changed = grants.changed() => PeerEvent::Grant(changed.map(|()| *grants.borrow_and_update())),
            joined = async {
                let (_, task) = read.as_mut().expect("read branch is guarded");
                task.await
            }, if read.is_some() => PeerEvent::Read(joined),
            message = io.next_message_or_send_ready(send_target.target.min(METADATA_SEND_BUFFER_WATERMARK)) => {
                PeerEvent::Message(message)
            },
        };
        let actions = match event {
            PeerEvent::Cancelled => {
                join_read(read.take()).await;
                return PeerTermination::Cancelled;
            }
            PeerEvent::Maintenance => {
                let now = Instant::now();
                if now.saturating_duration_since(last_peer_activity) >= shared.peer_activity_timeout
                {
                    join_read(read.take()).await;
                    return PeerTermination::ActivityTimeout;
                }
                let snapshot = upload.snapshot();
                if snapshot.interested
                    && !snapshot.choking
                    && snapshot.queued_requests == 0
                    && read.is_none()
                    && io.send_buffer_size() == 0
                    && now.saturating_duration_since(last_request_or_unchoke)
                        >= shared.no_request_timeout
                {
                    return PeerTermination::NoRequestTimeout;
                }
                let budget = shared.peer_budget.snapshot();
                if budget.total >= budget.effective_limit
                    && now.saturating_duration_since(last_meaningful_activity)
                        >= shared.inactivity_timeout
                {
                    join_read(read.take()).await;
                    return PeerTermination::InactivityTimeout;
                }
                if now.saturating_duration_since(last_keepalive) >= shared.keepalive_interval {
                    last_keepalive = now;
                    vec![UploadAction::Send(PeerMessage::KeepAlive)]
                } else {
                    Vec::new()
                }
            }
            PeerEvent::Read(joined) => {
                let (pending, _) = read.take().expect("completed read is present");
                let result = match joined {
                    Ok(result) => result,
                    Err(_) => Err(()),
                };
                upload.on_read_complete(pending, result)
            }
            PeerEvent::Grant(Ok(grant)) => {
                if grant == UploadGrant::Choked {
                    queued_piece_frames.cancel_all();
                }
                if grant != UploadGrant::Choked {
                    last_request_or_unchoke = Instant::now();
                    last_meaningful_activity = last_request_or_unchoke;
                }
                upload.set_granted(grant != UploadGrant::Choked)
            }
            PeerEvent::Grant(Err(_)) => {
                join_read(read.take()).await;
                return PeerTermination::Cancelled;
            }
            PeerEvent::Message(Ok(None)) => Vec::new(),
            PeerEvent::Message(Err(crate::peer_io::PeerIoError::Frame(_))) => {
                join_read(read.take()).await;
                return PeerTermination::Protocol;
            }
            PeerEvent::Message(Err(_)) => {
                join_read(read.take()).await;
                return PeerTermination::Closed;
            }
            PeerEvent::Message(Ok(Some(message))) => {
                last_peer_activity = Instant::now();
                if matches!(
                    message,
                    PeerMessage::Interested
                        | PeerMessage::NotInterested
                        | PeerMessage::Request(_)
                        | PeerMessage::Cancel(_)
                ) {
                    last_meaningful_activity = last_peer_activity;
                }
                if matches!(message, PeerMessage::Request(_)) {
                    last_request_or_unchoke = last_peer_activity;
                }
                match message {
                    PeerMessage::NotInterested => queued_piece_frames.cancel_all(),
                    PeerMessage::Cancel(request) => queued_piece_frames.cancel(request),
                    _ => {}
                }
                match handle_metadata_message(
                    io,
                    &mut metadata,
                    &mut remote_metadata_id,
                    &mut deferred_metadata,
                    &message,
                ) {
                    Ok(()) => {
                        let actions = upload.on_message(&message);
                        shared
                            .upload_coordinator
                            .update_interest(upload_peer, upload.snapshot().interested);
                        actions
                    }
                    Err(()) => {
                        join_read(read.take()).await;
                        return PeerTermination::Protocol;
                    }
                }
            }
        };
        if let Some(termination) = apply_upload_actions(
            actions,
            io,
            &mut read,
            &mut queued_piece_frames,
            &mut queued_choke_frame,
            &registration,
            &shared,
        )
        .await
        {
            return termination;
        }
        let payload = io.uploaded_payload_bytes();
        let payload_delta = payload.saturating_sub(accounted_payload);
        if payload_delta != 0 {
            accounted_payload = payload;
            last_meaningful_activity = Instant::now();
            shared
                .upload_coordinator
                .update_payload(upload_peer, payload);
        }
        let snapshot = upload.snapshot();
        let mut observations = shared.observations_guard();
        observations.queued_requests_high_water = observations
            .queued_requests_high_water
            .max(snapshot.queued_requests_high_water);
        observations.queued_bytes_high_water = observations
            .queued_bytes_high_water
            .max(snapshot.queued_bytes_high_water);
        observations.metadata_requests_high_water = observations
            .metadata_requests_high_water
            .max(deferred_metadata.len());
        observations.metadata_send_buffer_high_water = observations
            .metadata_send_buffer_high_water
            .max(io.send_buffer_high_water());
        observations.writer_send_buffer_high_water = observations
            .writer_send_buffer_high_water
            .max(io.send_buffer_high_water());
        let scheduler = shared.upload_coordinator.snapshot();
        observations.upload_regular_high_water = observations
            .upload_regular_high_water
            .max(scheduler.regular);
        observations.upload_optimistic_high_water = observations
            .upload_optimistic_high_water
            .max(scheduler.optimistic);
        observations.upload_slots_high_water = observations
            .upload_slots_high_water
            .max(scheduler.regular.saturating_add(scheduler.optimistic));
    }
}

async fn apply_upload_actions(
    actions: Vec<UploadAction>,
    io: &mut IncomingPeerIo,
    read: &mut Option<ActiveRead>,
    queued_piece_frames: &mut QueuedPieceFrames,
    queued_choke_frame: &mut QueuedChokeFrame,
    registration: &Arc<SeedRegistration>,
    shared: &Arc<Shared>,
) -> Option<PeerTermination> {
    for action in actions {
        match action {
            UploadAction::Send(message) => {
                let result = match &message {
                    PeerMessage::Piece {
                        index,
                        begin,
                        block,
                    } => {
                        let Ok(length) = u32::try_from(block.len()) else {
                            return Some(PeerTermination::Storage);
                        };
                        let validity = queued_piece_frames.track(BlockRequest {
                            index: *index,
                            begin: *begin,
                            length,
                        });
                        io.queue_generation_fenced_message(&message, validity)
                    }
                    PeerMessage::Choke | PeerMessage::Unchoke => {
                        io.queue_generation_fenced_message(&message, queued_choke_frame.replace())
                    }
                    _ => io.queue_message(&message),
                };
                if result.is_err() {
                    join_read(read.take()).await;
                    return Some(PeerTermination::Closed);
                }
            }
            UploadAction::Read(pending) => {
                if read.is_some() {
                    join_read(read.take()).await;
                    return Some(PeerTermination::Protocol);
                }
                let content = registration.content.clone();
                let read_permits = shared.upload_reads.clone();
                let read_shared = shared.clone();
                *read = Some((
                    pending,
                    tokio::spawn(async move {
                        let Ok(_permit) = read_permits.acquire_owned().await else {
                            return Err(());
                        };
                        let _observation =
                            ObservationGuard::read(&read_shared, pending.request.length as usize);
                        content.read_block(pending.request).await.map_err(|_| ())
                    }),
                ));
            }
            UploadAction::Close(reason) => {
                join_read(read.take()).await;
                return Some(match reason {
                    UploadCloseReason::ReadFailed | UploadCloseReason::ShortRead => {
                        PeerTermination::Storage
                    }
                    UploadCloseReason::InvalidRequest | UploadCloseReason::RequestLimit => {
                        PeerTermination::Protocol
                    }
                });
            }
        }
    }
    None
}

enum PeerEvent {
    Cancelled,
    Maintenance,
    Read(Result<Result<Vec<u8>, ()>, tokio::task::JoinError>),
    Grant(Result<UploadGrant, tokio::sync::watch::error::RecvError>),
    Message(Result<Option<PeerMessage>, crate::peer_io::PeerIoError>),
}

trait MetadataSendBuffer {
    fn queue_metadata_message(&mut self, message: &PeerMessage) -> Result<(), ()>;
    fn metadata_send_buffer_size(&self) -> usize;
}

impl MetadataSendBuffer for PeerIo {
    fn queue_metadata_message(&mut self, message: &PeerMessage) -> Result<(), ()> {
        self.queue_message(message).map_err(|_| ())
    }

    fn metadata_send_buffer_size(&self) -> usize {
        self.send_buffer_size()
    }
}

impl MetadataSendBuffer for IncomingPeerIo {
    fn queue_metadata_message(&mut self, message: &PeerMessage) -> Result<(), ()> {
        self.queue_message(message).map_err(|_| ())
    }

    fn metadata_send_buffer_size(&self) -> usize {
        self.send_buffer_size()
    }
}

fn handle_metadata_message<I: MetadataSendBuffer>(
    io: &mut I,
    upload: &mut MetadataUpload,
    remote_metadata_id: &mut Option<u8>,
    deferred: &mut VecDeque<i64>,
    message: &PeerMessage,
) -> Result<(), ()> {
    match message {
        PeerMessage::Extended { id: 0, payload } => {
            let handshake = parse_extension_handshake(payload).map_err(|_| ())?;
            match handshake.metadata_extension {
                MetadataExtensionUpdate::Unchanged => {}
                MetadataExtensionUpdate::Disabled => {
                    *remote_metadata_id = None;
                    deferred.clear();
                }
                MetadataExtensionUpdate::Enabled(id) => *remote_metadata_id = Some(id),
            }
        }
        PeerMessage::Extended {
            id: UT_METADATA_LOCAL_ID,
            payload,
        } => {
            let message = parse_metadata_message(payload).map_err(|_| ())?;
            let piece = match message {
                MetadataMessage::Request { piece } => piece,
                MetadataMessage::Unknown { .. } => return Ok(()),
                MetadataMessage::Data { .. } | MetadataMessage::Reject { .. } => return Err(()),
            };
            let remote_id = remote_metadata_id.ok_or(())?;
            if !upload.can_serve(piece)
                || io.metadata_send_buffer_size() < METADATA_SEND_BUFFER_WATERMARK
            {
                queue_metadata_response(io, upload, remote_id, piece)?;
            } else if deferred.len() < MAX_DEFERRED_METADATA_REQUESTS {
                deferred.push_back(piece);
            } else {
                io.queue_metadata_message(&PeerMessage::Extended {
                    id: remote_id,
                    payload: encode_metadata_reject(piece),
                })?;
            }
        }
        PeerMessage::Extended { .. } => {}
        _ => {}
    }
    Ok(())
}

fn drain_metadata_requests<I: MetadataSendBuffer>(
    io: &mut I,
    upload: &mut MetadataUpload,
    remote_metadata_id: Option<u8>,
    deferred: &mut VecDeque<i64>,
) -> Result<(), ()> {
    let Some(remote_metadata_id) = remote_metadata_id else {
        return Ok(());
    };
    while io.metadata_send_buffer_size() < METADATA_SEND_BUFFER_WATERMARK {
        let Some(piece) = deferred.pop_front() else {
            break;
        };
        queue_metadata_response(io, upload, remote_metadata_id, piece)?;
    }
    Ok(())
}

fn queue_metadata_response<I: MetadataSendBuffer>(
    io: &mut I,
    upload: &mut MetadataUpload,
    remote_metadata_id: u8,
    piece: i64,
) -> Result<(), ()> {
    let payload = match upload.on_request(piece).map_err(|_| ())? {
        MetadataUploadAction::Data {
            piece,
            total_size,
            block,
        } => encode_metadata_data(piece, total_size, &block).map_err(|_| ())?,
        MetadataUploadAction::Reject { piece } => encode_metadata_reject(piece),
    };
    io.queue_metadata_message(&PeerMessage::Extended {
        id: remote_metadata_id,
        payload,
    })
}

async fn join_read(read: Option<ActiveRead>) {
    if let Some((_, read)) = read {
        let _ = read.await;
    }
}

#[derive(Debug)]
pub enum IncomingPeerError {
    InvalidFixedPort,
    InvalidTimeout,
    InvalidScheduler(&'static str),
    InvalidUploadReadJobs {
        maximum: usize,
    },
    Closed,
    RegistrationLimit {
        maximum: usize,
    },
    InvalidRegistration(&'static str),
    Bind {
        port: u16,
        source: io::Error,
    },
    Io {
        operation: &'static str,
        source: io::Error,
    },
    TaskJoin(String),
}

impl fmt::Display for IncomingPeerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFixedPort => formatter.write_str("fixed incoming port must be nonzero"),
            Self::InvalidTimeout => formatter.write_str("incoming peer timeouts must be nonzero"),
            Self::InvalidScheduler(reason) => {
                write!(formatter, "invalid upload scheduler: {reason}")
            }
            Self::InvalidUploadReadJobs { maximum } => {
                write!(
                    formatter,
                    "upload read jobs must be between 1 and {maximum}"
                )
            }
            Self::Closed => formatter.write_str("incoming peer service is closed"),
            Self::RegistrationLimit { maximum } => {
                write!(formatter, "seed registration limit {maximum} reached")
            }
            Self::InvalidRegistration(reason) => {
                write!(formatter, "invalid seed registration: {reason}")
            }
            Self::Bind { port, source } => {
                write!(formatter, "bind loopback incoming port {port}: {source}")
            }
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::TaskJoin(error) => write!(formatter, "incoming task join: {error}"),
        }
    }
}

impl Error for IncomingPeerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Bind { source, .. } | Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use rstorrent_protocol::metadata::{
        MetadataExtensionUpdate, MetadataMessage, MetadataUpload,
        encode_extension_handshake_with_id, encode_metadata_request, parse_extension_handshake,
        parse_metadata_message,
    };
    use rstorrent_protocol::metainfo::{BEP9_METAINFO_LIMITS, Metainfo};
    use rstorrent_protocol::peer_wire::{
        BlockRequest, EXTENSION_PROTOCOL_RESERVED_BIT, EXTENSION_PROTOCOL_RESERVED_INDEX,
        FrameDecoder, HANDSHAKE_LENGTH, PeerMessage, decode_handshake,
        encode_handshake_with_reserved, encode_message,
    };
    use sha1::{Digest, Sha1};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::time::timeout;

    use super::{
        IncomingPeerError, IncomingPeerService, IncomingPeerServiceConfig, IncomingRejectionReason,
        IncomingTcpBootstrap, MAX_DEFERRED_METADATA_REQUESTS, METADATA_SEND_BUFFER_WATERMARK,
        QueuedChokeFrame, QueuedPieceFrames, SeedRegistration, drain_metadata_requests,
        handle_metadata_message,
    };
    use crate::peer_io::PeerIo;
    use crate::{
        DEFAULT_PEER_ID, PeerBudget, PeerBudgetConfig, SeedContent, UploadSchedulerConfig,
    };

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn queued_piece_cancellation_invalidates_only_matching_frames() {
        let first = BlockRequest {
            index: 0,
            begin: 0,
            length: 4,
        };
        let second = BlockRequest {
            index: 0,
            begin: 4,
            length: 4,
        };
        let mut frames = QueuedPieceFrames::default();
        let first_validity = frames.track(first);
        let second_validity = frames.track(second);
        frames.cancel(first);
        assert!(first_validity.is_cancelled());
        assert!(!second_validity.is_cancelled());
        frames.cancel_all();
        assert!(second_validity.is_cancelled());
    }

    #[test]
    fn queued_choke_state_coalesces_to_latest_frame() {
        let mut frame = QueuedChokeFrame::default();
        let first = frame.replace();
        let latest = frame.replace();
        assert!(first.is_cancelled());
        assert!(!latest.is_cancelled());
    }

    fn root(label: &str) -> PathBuf {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rstorrent-incoming-{label}-{}-{sequence}",
            std::process::id()
        ))
    }

    fn raw_info(payload: &[u8]) -> Vec<u8> {
        let mut hashes = Vec::new();
        for piece in payload.chunks(4) {
            hashes.extend_from_slice(&Sha1::digest(piece));
        }
        let mut info = format!(
            "d6:lengthi{}e4:name8:seed.bin12:piece lengthi4e6:pieces{}:",
            payload.len(),
            hashes.len()
        )
        .into_bytes();
        info.extend_from_slice(&hashes);
        info.push(b'e');
        info
    }

    async fn registration(label: &str) -> (PathBuf, Vec<u8>, SeedRegistration) {
        let payload = b"abcdefg";
        let raw_info = raw_info(payload);
        let metainfo = Metainfo::from_info_bytes_with_limits(&raw_info, BEP9_METAINFO_LIMITS)
            .expect("parse fixture info");
        let root = root(label);
        tokio::fs::create_dir_all(&root).await.expect("create root");
        tokio::fs::write(root.join("seed.bin"), payload)
            .await
            .expect("write published payload");
        let content = SeedContent::open_published(&root, &metainfo, &[true, true], &[])
            .await
            .expect("open seed content");
        let registration =
            SeedRegistration::new(raw_info.clone(), content).expect("valid registration");
        (root, raw_info, registration)
    }

    fn config(bootstrap: IncomingTcpBootstrap) -> IncomingPeerServiceConfig {
        IncomingPeerServiceConfig {
            bootstrap,
            handshake_timeout: Duration::from_millis(250),
            peer_activity_timeout: Duration::from_secs(2),
            keepalive_interval: Duration::from_secs(1),
            no_request_timeout: Duration::from_secs(1),
            inactivity_timeout: Duration::from_secs(2),
            peer_id: DEFAULT_PEER_ID,
            byte_metric_sink: None,
            peer_budget: PeerBudget::system_default(),
            upload_scheduler: UploadSchedulerConfig::default(),
            upload_read_jobs: super::DEFAULT_UPLOAD_READ_JOBS,
        }
    }

    #[tokio::test]
    async fn metadata_upload_defers_by_occupancy_not_connection_lifetime() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("listen address");
        let client = TcpStream::connect(address).await.expect("connect");
        let (server, _) = listener.accept().await.expect("accept");
        let drain = tokio::spawn(async move {
            let mut client = client;
            let mut bytes = [0_u8; 64 * 1_024];
            while client.read(&mut bytes).await.unwrap_or(0) != 0 {}
        });
        let mut io = PeerIo::new(server, Duration::from_secs(2), None);
        let metadata = vec![7; 16 * 1_024];
        let mut upload = MetadataUpload::new(&metadata).expect("local metadata");
        let mut remote_id = Some(7);
        let mut deferred = VecDeque::new();
        let request = PeerMessage::Extended {
            id: rstorrent_protocol::metadata::UT_METADATA_LOCAL_ID,
            payload: encode_metadata_request(0),
        };

        while io.send_buffer_size() < METADATA_SEND_BUFFER_WATERMARK {
            handle_metadata_message(
                &mut io,
                &mut upload,
                &mut remote_id,
                &mut deferred,
                &request,
            )
            .expect("queue immediate metadata response");
        }
        for _ in 0..MAX_DEFERRED_METADATA_REQUESTS {
            handle_metadata_message(
                &mut io,
                &mut upload,
                &mut remote_id,
                &mut deferred,
                &request,
            )
            .expect("defer metadata response");
        }
        assert_eq!(deferred.len(), MAX_DEFERRED_METADATA_REQUESTS);
        let before_reject = io.send_buffer_size();
        handle_metadata_message(
            &mut io,
            &mut upload,
            &mut remote_id,
            &mut deferred,
            &request,
        )
        .expect("reject request above deferred occupancy bound");
        assert!(io.send_buffer_size() > before_reject);

        while !deferred.is_empty() {
            assert!(
                timeout(
                    Duration::from_secs(2),
                    io.next_message_or_send_ready(METADATA_SEND_BUFFER_WATERMARK),
                )
                .await
                .expect("queued send drains")
                .expect("queued send remains connected")
                .is_none()
            );
            drain_metadata_requests(&mut io, &mut upload, remote_id, &mut deferred)
                .expect("refill bounded send buffer");
        }
        assert!(upload.request_count() > MAX_DEFERRED_METADATA_REQUESTS);
        drop(io);
        drain.await.expect("reader task");
    }

    async fn send(stream: &mut TcpStream, message: &PeerMessage) {
        stream
            .write_all(&encode_message(message).expect("encode message"))
            .await
            .expect("send message");
    }

    async fn next_message(
        stream: &mut TcpStream,
        decoder: &mut FrameDecoder,
        queued: &mut VecDeque<PeerMessage>,
    ) -> PeerMessage {
        timeout(Duration::from_secs(2), async {
            while queued.is_empty() {
                let mut bytes = [0; 4096];
                let read = stream.read(&mut bytes).await.expect("read peer message");
                assert_ne!(read, 0, "incoming service closed before response");
                queued.extend(decoder.push(&bytes[..read]).expect("decode peer message"));
            }
            queued.pop_front().expect("queued peer message")
        })
        .await
        .expect("peer response timeout")
    }

    async fn connect(
        address: std::net::SocketAddr,
        info_hash: [u8; 20],
        peer_id: [u8; 20],
    ) -> (TcpStream, FrameDecoder, VecDeque<PeerMessage>) {
        let mut stream = TcpStream::connect(address).await.expect("connect listener");
        let mut reserved = [0; 8];
        reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
        let handshake = encode_handshake_with_reserved(info_hash, peer_id, reserved);
        stream
            .write_all(&handshake[..17])
            .await
            .expect("send fragmented handshake prefix");
        stream
            .write_all(&handshake[17..])
            .await
            .expect("send fragmented handshake tail");
        let mut response = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut response)
            .await
            .expect("read server handshake");
        assert!(
            decode_handshake(&response, info_hash)
                .expect("valid server handshake")
                .supports_extensions()
        );
        (stream, FrameDecoder::new(), VecDeque::new())
    }

    async fn observe_close(stream: &mut TcpStream) {
        match timeout(Duration::from_secs(1), stream.read(&mut [0; 1]))
            .await
            .expect("peer close deadline")
        {
            Ok(0) => {}
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
            result => panic!("unexpected peer close result {result:?}"),
        }
    }

    #[tokio::test]
    async fn disabled_and_fixed_bind_contracts_are_exact() {
        assert!(
            IncomingPeerService::bind(config(IncomingTcpBootstrap::Disabled))
                .await
                .expect("disabled service")
                .is_none()
        );
        assert!(matches!(
            IncomingPeerService::bind(config(IncomingTcpBootstrap::FixedLoopback(0))).await,
            Err(IncomingPeerError::InvalidFixedPort)
        ));
        let blocker = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fixed-port blocker");
        let port = blocker.local_addr().expect("blocker address").port();
        assert!(matches!(
            IncomingPeerService::bind(config(IncomingTcpBootstrap::FixedLoopback(port))).await,
            Err(IncomingPeerError::Bind { port: failed, .. }) if failed == port
        ));
    }

    #[tokio::test]
    async fn serves_metadata_then_payload_on_one_incoming_connection() {
        let (root, raw_info, registration) = registration("vertical").await;
        let info_hash = registration.info_hash();
        let service = IncomingPeerService::bind(config(IncomingTcpBootstrap::AutomaticLoopback))
            .await
            .expect("bind service")
            .expect("enabled service");
        assert_eq!(service.listen_address().ip().to_string(), "127.0.0.1");
        assert_ne!(service.listen_address().port(), 0);
        let handle = service.handle();
        let token = handle.register(registration).await.expect("register seed");
        let (mut stream, mut decoder, mut queued) = connect(
            service.listen_address(),
            info_hash,
            *b"-RS-LEECH-0000000000",
        )
        .await;

        assert_eq!(
            next_message(&mut stream, &mut decoder, &mut queued).await,
            PeerMessage::Bitfield(vec![0b1100_0000])
        );
        let PeerMessage::Extended {
            id: 0,
            payload: handshake,
        } = next_message(&mut stream, &mut decoder, &mut queued).await
        else {
            panic!("expected extension handshake");
        };
        let handshake = parse_extension_handshake(&handshake).expect("parse extensions");
        assert_eq!(
            handshake.metadata_extension,
            MetadataExtensionUpdate::Enabled(1)
        );
        assert_eq!(handshake.metadata_size, Some(raw_info.len()));
        send(
            &mut stream,
            &PeerMessage::Extended {
                id: 0,
                payload: encode_extension_handshake_with_id(7, None)
                    .expect("directional extension ID"),
            },
        )
        .await;
        send(
            &mut stream,
            &PeerMessage::Extended {
                id: 1,
                payload: encode_metadata_request(0),
            },
        )
        .await;
        let PeerMessage::Extended { id, payload } =
            next_message(&mut stream, &mut decoder, &mut queued).await
        else {
            panic!("expected metadata response");
        };
        assert_eq!(id, 7);
        let MetadataMessage::Data {
            piece,
            total_size,
            block,
        } = parse_metadata_message(&payload).expect("parse metadata data")
        else {
            panic!("expected metadata data");
        };
        assert_eq!(piece, 0);
        assert_eq!(total_size, raw_info.len());
        assert_eq!(block, raw_info);

        send(&mut stream, &PeerMessage::Interested).await;
        assert_eq!(
            next_message(&mut stream, &mut decoder, &mut queued).await,
            PeerMessage::Unchoke
        );
        send(
            &mut stream,
            &PeerMessage::Request(rstorrent_protocol::peer_wire::BlockRequest {
                index: 0,
                begin: 0,
                length: 4,
            }),
        )
        .await;
        assert_eq!(
            next_message(&mut stream, &mut decoder, &mut queued).await,
            PeerMessage::Piece {
                index: 0,
                begin: 0,
                block: b"abcd".to_vec(),
            }
        );

        assert!(handle.unregister(token).await.expect("unregister seed"));
        assert_eq!(stream.read(&mut [0; 1]).await.expect("observe close"), 0);
        let before_shutdown = handle.snapshot();
        assert_eq!(before_shutdown.registrations, 0);
        assert_eq!(before_shutdown.established, 0);
        assert_eq!(before_shutdown.reads, 0);
        assert_eq!(before_shutdown.read_bytes, 0);
        assert_eq!(before_shutdown.payload_bytes_sent, 4);
        assert_eq!(before_shutdown.established_high_water, 1);
        assert_eq!(before_shutdown.queued_requests_high_water, 1);
        assert_eq!(before_shutdown.read_high_water, 1);
        assert_eq!(before_shutdown.read_bytes_high_water, 4);
        let terminal = service.shutdown().await.expect("shutdown service");
        assert_eq!(terminal.pending, 0);
        assert_eq!(terminal.established, 0);
        assert_eq!(terminal.reads, 0);
        assert_eq!(terminal.registrations, 0);
        assert!(!terminal.accepting_registrations);
        tokio::fs::remove_dir_all(root).await.expect("remove root");
    }

    #[tokio::test]
    async fn ten_peers_share_eight_slots_and_a_departure_fills_immediately() {
        let (root, _, registration) = registration("ten-peers").await;
        let info_hash = registration.info_hash();
        let service = IncomingPeerService::bind(config(IncomingTcpBootstrap::AutomaticLoopback))
            .await
            .expect("bind service")
            .expect("enabled service");
        let handle = service.handle();
        let token = handle.register(registration).await.expect("register seed");
        let mut peers = Vec::new();
        for generation in 1_u8..=10 {
            let mut peer = connect(service.listen_address(), info_hash, [generation; 20]).await;
            assert!(matches!(
                next_message(&mut peer.0, &mut peer.1, &mut peer.2).await,
                PeerMessage::Bitfield(_)
            ));
            assert!(matches!(
                next_message(&mut peer.0, &mut peer.1, &mut peer.2).await,
                PeerMessage::Extended { id: 0, .. }
            ));
            send(&mut peer.0, &PeerMessage::Interested).await;
            peers.push(peer);
        }

        for peer in peers.iter_mut().take(8) {
            assert_eq!(
                next_message(&mut peer.0, &mut peer.1, &mut peer.2).await,
                PeerMessage::Unchoke
            );
        }
        let (first, rest) = peers.split_at_mut(9);
        let ninth = &mut first[8];
        assert!(
            timeout(
                Duration::from_millis(50),
                next_message(&mut ninth.0, &mut ninth.1, &mut ninth.2),
            )
            .await
            .is_err(),
            "ninth interested peer must remain choked"
        );
        assert_eq!(rest.len(), 1);
        let saturated = handle.snapshot();
        assert_eq!(saturated.established, 10);
        assert_eq!(saturated.established_high_water, 10);
        assert_eq!(saturated.upload_scheduler.interested, 10);
        assert_eq!(saturated.upload_scheduler.regular, 7);
        assert_eq!(saturated.upload_scheduler.optimistic, 1);

        for peer in peers.iter_mut().take(8) {
            send(
                &mut peer.0,
                &PeerMessage::Request(rstorrent_protocol::peer_wire::BlockRequest {
                    index: 0,
                    begin: 0,
                    length: 4,
                }),
            )
            .await;
        }
        for peer in peers.iter_mut().take(8) {
            assert!(matches!(
                next_message(&mut peer.0, &mut peer.1, &mut peer.2).await,
                PeerMessage::Piece { block, .. } if block == b"abcd"
            ));
        }

        drop(peers.remove(0));
        let ninth = &mut peers[7];
        assert_eq!(
            next_message(&mut ninth.0, &mut ninth.1, &mut ninth.2).await,
            PeerMessage::Unchoke
        );
        assert!(handle.unregister(token).await.expect("unregister seed"));
        let terminal = service.shutdown().await.expect("shutdown service");
        assert_eq!(terminal.established, 0);
        assert_eq!(terminal.peer_budget.total, 0);
        assert_eq!(terminal.upload_scheduler.peers, 0);
        assert_eq!(terminal.payload_bytes_sent, 8 * 4);
        assert!(terminal.read_high_water <= super::DEFAULT_UPLOAD_READ_JOBS);
        tokio::fs::remove_dir_all(root).await.expect("remove root");
    }

    #[tokio::test]
    async fn keepalive_activity_no_request_and_near_limit_timeouts_are_distinct() {
        let (root, _, seed) = registration("peer-timeouts").await;
        let info_hash = seed.info_hash();
        let mut service_config = config(IncomingTcpBootstrap::AutomaticLoopback);
        service_config.peer_activity_timeout = Duration::from_millis(500);
        service_config.keepalive_interval = Duration::from_millis(20);
        service_config.no_request_timeout = Duration::from_millis(500);
        service_config.inactivity_timeout = Duration::from_millis(500);
        let service = IncomingPeerService::bind(service_config)
            .await
            .expect("bind keepalive service")
            .expect("enabled keepalive service");
        let handle = service.handle();
        let token = handle.register(seed).await.expect("register seed");
        let mut peer = connect(service.listen_address(), info_hash, [31; 20]).await;
        assert!(matches!(
            next_message(&mut peer.0, &mut peer.1, &mut peer.2).await,
            PeerMessage::Bitfield(_)
        ));
        assert!(matches!(
            next_message(&mut peer.0, &mut peer.1, &mut peer.2).await,
            PeerMessage::Extended { id: 0, .. }
        ));
        assert_eq!(
            next_message(&mut peer.0, &mut peer.1, &mut peer.2).await,
            PeerMessage::KeepAlive
        );
        assert!(handle.unregister(token).await.expect("unregister seed"));
        service
            .shutdown()
            .await
            .expect("shutdown keepalive service");
        tokio::fs::remove_dir_all(root).await.expect("remove root");

        let (root, _, seed) = registration("activity-timeout").await;
        let info_hash = seed.info_hash();
        let mut service_config = config(IncomingTcpBootstrap::AutomaticLoopback);
        service_config.peer_activity_timeout = Duration::from_millis(30);
        service_config.keepalive_interval = Duration::from_secs(5);
        service_config.no_request_timeout = Duration::from_secs(5);
        service_config.inactivity_timeout = Duration::from_secs(5);
        let service = IncomingPeerService::bind(service_config)
            .await
            .expect("bind activity service")
            .expect("enabled activity service");
        let handle = service.handle();
        handle.register(seed).await.expect("register seed");
        let mut peer = connect(service.listen_address(), info_hash, [32; 20]).await;
        let _ = next_message(&mut peer.0, &mut peer.1, &mut peer.2).await;
        let _ = next_message(&mut peer.0, &mut peer.1, &mut peer.2).await;
        observe_close(&mut peer.0).await;
        assert_eq!(
            handle
                .snapshot()
                .rejection_counts
                .get(&IncomingRejectionReason::ActivityTimeout),
            Some(&1)
        );
        service.shutdown().await.expect("shutdown activity service");
        tokio::fs::remove_dir_all(root).await.expect("remove root");

        let (root, _, seed) = registration("no-request-timeout").await;
        let info_hash = seed.info_hash();
        let mut service_config = config(IncomingTcpBootstrap::AutomaticLoopback);
        service_config.peer_activity_timeout = Duration::from_secs(5);
        service_config.keepalive_interval = Duration::from_secs(5);
        service_config.no_request_timeout = Duration::from_millis(30);
        service_config.inactivity_timeout = Duration::from_secs(5);
        let service = IncomingPeerService::bind(service_config)
            .await
            .expect("bind no-request service")
            .expect("enabled no-request service");
        let handle = service.handle();
        handle.register(seed).await.expect("register seed");
        let mut peer = connect(service.listen_address(), info_hash, [33; 20]).await;
        let _ = next_message(&mut peer.0, &mut peer.1, &mut peer.2).await;
        let _ = next_message(&mut peer.0, &mut peer.1, &mut peer.2).await;
        send(&mut peer.0, &PeerMessage::Interested).await;
        assert_eq!(
            next_message(&mut peer.0, &mut peer.1, &mut peer.2).await,
            PeerMessage::Unchoke
        );
        observe_close(&mut peer.0).await;
        assert_eq!(
            handle
                .snapshot()
                .rejection_counts
                .get(&IncomingRejectionReason::NoRequestTimeout),
            Some(&1)
        );
        service
            .shutdown()
            .await
            .expect("shutdown no-request service");
        tokio::fs::remove_dir_all(root).await.expect("remove root");

        let (root, _, seed) = registration("inactivity-timeout").await;
        let info_hash = seed.info_hash();
        let mut service_config = config(IncomingTcpBootstrap::AutomaticLoopback);
        service_config.peer_activity_timeout = Duration::from_secs(5);
        service_config.keepalive_interval = Duration::from_secs(5);
        service_config.no_request_timeout = Duration::from_secs(5);
        service_config.inactivity_timeout = Duration::from_millis(30);
        service_config.peer_budget = PeerBudget::new(PeerBudgetConfig {
            configured_limit: 1,
            incoming_slack: 0,
            max_open_files: 10_000,
        });
        let service = IncomingPeerService::bind(service_config)
            .await
            .expect("bind inactivity service")
            .expect("enabled inactivity service");
        let handle = service.handle();
        handle.register(seed).await.expect("register seed");
        let mut peer = connect(service.listen_address(), info_hash, [34; 20]).await;
        let _ = next_message(&mut peer.0, &mut peer.1, &mut peer.2).await;
        let _ = next_message(&mut peer.0, &mut peer.1, &mut peer.2).await;
        observe_close(&mut peer.0).await;
        assert_eq!(
            handle
                .snapshot()
                .rejection_counts
                .get(&IncomingRejectionReason::InactivityTimeout),
            Some(&1)
        );
        service
            .shutdown()
            .await
            .expect("shutdown inactivity service");
        tokio::fs::remove_dir_all(root).await.expect("remove root");
    }

    #[tokio::test]
    async fn unknown_timeout_self_and_connection_saturation_are_bounded() {
        let (root, _, registration) = registration("rejections").await;
        let info_hash = registration.info_hash();
        let mut service_config = config(IncomingTcpBootstrap::AutomaticLoopback);
        service_config.handshake_timeout = Duration::from_millis(50);
        service_config.peer_budget = PeerBudget::new(PeerBudgetConfig {
            configured_limit: 1,
            incoming_slack: 0,
            max_open_files: 10_000,
        });
        let service = IncomingPeerService::bind(service_config)
            .await
            .expect("bind service")
            .expect("enabled service");
        let handle = service.handle();
        handle.register(registration).await.expect("register seed");

        let mut silent = TcpStream::connect(service.listen_address())
            .await
            .expect("connect silent peer");
        timeout(Duration::from_secs(1), silent.read(&mut [0; 1]))
            .await
            .expect("silent peer close deadline")
            .expect("silent peer read");

        let mut unknown = TcpStream::connect(service.listen_address())
            .await
            .expect("connect unknown peer");
        unknown
            .write_all(&encode_handshake_with_reserved(
                [9; 20],
                *b"-RS-UNKNOWN-00000000",
                [0; 8],
            ))
            .await
            .expect("send unknown handshake");
        assert_eq!(unknown.read(&mut [0; 1]).await.expect("unknown close"), 0);

        let mut self_peer = TcpStream::connect(service.listen_address())
            .await
            .expect("connect self peer");
        self_peer
            .write_all(&encode_handshake_with_reserved(
                info_hash,
                super::DEFAULT_PEER_ID,
                [0; 8],
            ))
            .await
            .expect("send self handshake");
        assert_eq!(self_peer.read(&mut [0; 1]).await.expect("self close"), 0);

        let (first, _, _) = connect(
            service.listen_address(),
            info_hash,
            *b"-RS-FIRST--000000000",
        )
        .await;
        let mut second = TcpStream::connect(service.listen_address())
            .await
            .expect("connect saturated peer");
        second
            .write_all(&encode_handshake_with_reserved(
                info_hash,
                *b"-RS-SECOND-000000000",
                [0; 8],
            ))
            .await
            .expect("send saturated handshake");
        match second.read(&mut [0; 1]).await {
            Ok(0) => {}
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
            result => panic!("unexpected saturated close result {result:?}"),
        }
        drop(first);

        timeout(Duration::from_secs(1), async {
            loop {
                let snapshot = handle.snapshot();
                if snapshot.pending == 0
                    && snapshot
                        .rejection_counts
                        .get(&IncomingRejectionReason::HandshakeTimeout)
                        == Some(&1)
                    && snapshot
                        .rejection_counts
                        .get(&IncomingRejectionReason::UnknownTorrent)
                        == Some(&1)
                    && snapshot
                        .rejection_counts
                        .get(&IncomingRejectionReason::SelfConnection)
                        == Some(&1)
                    && snapshot
                        .rejection_counts
                        .get(&IncomingRejectionReason::ConnectionLimit)
                        == Some(&1)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("rejection observations");
        let snapshot = handle.snapshot();
        assert!(snapshot.pending_high_water <= super::MAX_INCOMING_PENDING);
        assert_eq!(snapshot.established_high_water, 1);
        assert_eq!(snapshot.peer_budget.total_high_water, 1);
        assert!(snapshot.recent_rejections.len() <= 32);
        service.shutdown().await.expect("shutdown service");
        tokio::fs::remove_dir_all(root).await.expect("remove root");
    }

    #[tokio::test]
    async fn pending_handshake_and_registration_caps_are_exact() {
        let (root, _, registration) = registration("caps").await;
        let mut service_config = config(IncomingTcpBootstrap::AutomaticLoopback);
        service_config.handshake_timeout = Duration::from_millis(500);
        let service = IncomingPeerService::bind(service_config)
            .await
            .expect("bind service")
            .expect("enabled service");
        let handle = service.handle();
        let template = registration.clone();
        for value in 0..super::MAX_SEED_REGISTRATIONS {
            let mut registration = template.clone();
            registration.info_hash[..8].copy_from_slice(&(value as u64).to_be_bytes());
            handle.register(registration).await.expect("fill registry");
        }
        let mut overflow = template;
        overflow.info_hash = [0xff; 20];
        assert!(matches!(
            handle.register(overflow).await,
            Err(IncomingPeerError::RegistrationLimit { maximum })
                if maximum == super::MAX_SEED_REGISTRATIONS
        ));

        let mut silent = Vec::new();
        for _ in 0..=super::MAX_INCOMING_PENDING {
            silent.push(
                TcpStream::connect(service.listen_address())
                    .await
                    .expect("connect pending peer"),
            );
        }
        timeout(Duration::from_secs(1), async {
            loop {
                let snapshot = handle.snapshot();
                if snapshot
                    .rejection_counts
                    .get(&IncomingRejectionReason::PendingLimit)
                    == Some(&1)
                {
                    assert_eq!(snapshot.pending, super::MAX_INCOMING_PENDING);
                    assert_eq!(snapshot.pending_high_water, super::MAX_INCOMING_PENDING);
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("pending saturation observation");
        drop(silent);
        let terminal = service
            .shutdown()
            .await
            .expect("shutdown saturated service");
        assert_eq!(terminal.pending, 0);
        assert_eq!(terminal.registrations, 0);
        tokio::fs::remove_dir_all(root).await.expect("remove root");
    }
}
