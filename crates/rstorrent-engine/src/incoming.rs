//! One loopback listener with generation-fenced torrent routing.

use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use rstorrent_protocol::metadata::{
    MetadataExtensionUpdate, MetadataMessage, MetadataUpload, MetadataUploadAction,
    UT_METADATA_LOCAL_ID, encode_extension_handshake, encode_metadata_data, encode_metadata_reject,
    parse_extension_handshake, parse_metadata_message,
};
use rstorrent_protocol::peer_wire::{
    EXTENSION_PROTOCOL_RESERVED_BIT, EXTENSION_PROTOCOL_RESERVED_INDEX, HANDSHAKE_LENGTH,
    PeerMessage, decode_handshake, encode_handshake_with_reserved,
};
use sha1::{Digest, Sha1};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex as AsyncMutex, OwnedSemaphorePermit, Semaphore};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{Instant, timeout_at};
use tokio_util::sync::CancellationToken;

use crate::metrics::{ByteMetric, ByteMetricSink};
use crate::network::DEFAULT_PEER_ID;
use crate::peer_io::{PeerIo, record_bytes};
use crate::seed_content::SeedContent;
use crate::upload::{UploadAction, UploadCloseReason, UploadPeerState, UploadRead};

pub const MAX_SEED_REGISTRATIONS: usize = 1024;
pub const MAX_INCOMING_PENDING: usize = 8;
pub const MAX_INCOMING_ESTABLISHED: usize = 1;
pub const DEFAULT_INCOMING_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
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
    pub peer_io_timeout: Duration,
    pub peer_id: [u8; 20],
    pub byte_metric_sink: Option<Arc<dyn ByteMetricSink>>,
}

impl IncomingPeerServiceConfig {
    pub fn new(bootstrap: IncomingTcpBootstrap, peer_io_timeout: Duration) -> Self {
        Self {
            bootstrap,
            handshake_timeout: DEFAULT_INCOMING_HANDSHAKE_TIMEOUT,
            peer_io_timeout,
            peer_id: DEFAULT_PEER_ID,
            byte_metric_sink: None,
        }
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
    raw_info: Vec<u8>,
    content: SeedContent,
    piece_lengths: Vec<u32>,
}

impl SeedRegistration {
    pub fn new(raw_info: Vec<u8>, content: SeedContent) -> Result<Self, IncomingPeerError> {
        let info_hash: [u8; 20] = Sha1::digest(&raw_info).into();
        if info_hash != content.info_hash() {
            return Err(IncomingPeerError::InvalidRegistration(
                "metadata and seed content identities differ",
            ));
        }
        MetadataUpload::new(raw_info.clone()).map_err(|_| {
            IncomingPeerError::InvalidRegistration("metadata exceeds upload limits")
        })?;
        let piece_lengths = content
            .piece_lengths()
            .map_err(|_| IncomingPeerError::InvalidRegistration("invalid seed piece geometry"))?;
        Ok(Self {
            info_hash,
            raw_info,
            content,
            piece_lengths,
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
    EstablishedLimit,
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
    pub reads: usize,
    pub read_bytes: usize,
    pub queued_requests_high_water: usize,
    pub queued_bytes_high_water: usize,
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
    established: Arc<Semaphore>,
    observations: Mutex<ObservationState>,
    peer_io_timeout: Duration,
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
    admission: AsyncMutex<Option<JoinHandle<()>>>,
}

impl RegistrationRuntime {
    fn new(generation: u64, registration: SeedRegistration) -> Self {
        Self {
            generation,
            data: Arc::new(registration),
            accepting: AtomicBool::new(true),
            healthy: AtomicBool::new(true),
            cancellation: CancellationToken::new(),
            admission: AsyncMutex::new(None),
        }
    }

    async fn admit(
        self: &Arc<Self>,
        stream: TcpStream,
        remote: SocketAddr,
        supports_extensions: bool,
        permit: OwnedSemaphorePermit,
        shared: Arc<Shared>,
    ) -> bool {
        let finished = {
            let mut admission = self.admission.lock().await;
            if !self.accepting.load(Ordering::Acquire)
                || !self.healthy.load(Ordering::Acquire)
                || self.cancellation.is_cancelled()
            {
                return false;
            }
            match admission.as_ref() {
                Some(task) if !task.is_finished() => return false,
                Some(_) => admission.take(),
                None => None,
            }
        };
        if let Some(finished) = finished {
            let _ = finished.await;
        }
        let mut admission = self.admission.lock().await;
        if !self.accepting.load(Ordering::Acquire)
            || !self.healthy.load(Ordering::Acquire)
            || self.cancellation.is_cancelled()
            || admission.is_some()
        {
            return false;
        }
        let data = self.data.clone();
        let cancellation = self.cancellation.clone();
        let registration = self.clone();
        *admission = Some(tokio::spawn(async move {
            let _established = ObservationGuard::established(&shared);
            let termination = run_incoming_peer(
                stream,
                supports_extensions,
                data,
                cancellation,
                shared.clone(),
            )
            .await;
            drop(permit);
            match termination {
                PeerTermination::Storage => {
                    registration.healthy.store(false, Ordering::Release);
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
                PeerTermination::Closed | PeerTermination::Cancelled => {}
            }
        }));
        true
    }

    async fn shutdown(&self) -> Result<(), IncomingPeerError> {
        self.accepting.store(false, Ordering::Release);
        self.cancellation.cancel();
        if let Some(task) = self.admission.lock().await.take() {
            task.await
                .map_err(|error| IncomingPeerError::TaskJoin(error.to_string()))?;
        }
        Ok(())
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
}

impl IncomingPeerService {
    pub async fn bind(
        config: IncomingPeerServiceConfig,
    ) -> Result<Option<Self>, IncomingPeerError> {
        if config.handshake_timeout.is_zero() || config.peer_io_timeout.is_zero() {
            return Err(IncomingPeerError::InvalidTimeout);
        }
        let port = match config.bootstrap {
            IncomingTcpBootstrap::Disabled => return Ok(None),
            IncomingTcpBootstrap::AutomaticLoopback => 0,
            IncomingTcpBootstrap::FixedLoopback(0) => {
                return Err(IncomingPeerError::InvalidFixedPort);
            }
            IncomingTcpBootstrap::FixedLoopback(port) => port,
        };
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
            .await
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
            established: Arc::new(Semaphore::new(MAX_INCOMING_ESTABLISHED)),
            observations: Mutex::new(ObservationState::default()),
            peer_io_timeout: config.peer_io_timeout,
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
        Ok(Some(Self {
            handle: IncomingPeerHandle { shared },
            cancellation,
            accept_task: Some(accept_task),
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
            reads: observations.reads,
            read_bytes: observations.read_bytes,
            queued_requests_high_water: observations.queued_requests_high_water,
            queued_bytes_high_water: observations.queued_bytes_high_water,
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

async fn run_handshake(
    mut stream: TcpStream,
    remote: SocketAddr,
    timeout: Duration,
    shared: Arc<Shared>,
    cancellation: CancellationToken,
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
    let Ok(established) = shared.established.clone().try_acquire_owned() else {
        shared.reject(
            IncomingRejectionReason::EstablishedLimit,
            Some(remote),
            Some(info_hash),
        );
        return;
    };
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
    if !registration
        .admit(
            stream,
            remote,
            handshake.supports_extensions(),
            established,
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
    Protocol,
    Storage,
}

type ActiveRead = (
    UploadRead,
    JoinHandle<Result<Vec<u8>, ()>>,
    ObservationGuard,
);

async fn run_incoming_peer(
    stream: TcpStream,
    supports_extensions: bool,
    registration: Arc<SeedRegistration>,
    cancellation: CancellationToken,
    shared: Arc<Shared>,
) -> PeerTermination {
    let mut io = PeerIo::new(
        stream,
        shared.peer_io_timeout,
        shared.byte_metric_sink.clone(),
    );
    let mut upload = match UploadPeerState::new(
        registration.piece_lengths.clone(),
        registration.content.availability().to_vec(),
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
    let mut metadata = match MetadataUpload::new(registration.raw_info.clone()) {
        Ok(metadata) => metadata,
        Err(_) => return PeerTermination::Storage,
    };
    let mut remote_metadata_id = None;
    let mut read: Option<ActiveRead> = None;
    loop {
        let event = tokio::select! {
            biased;
            _ = cancellation.cancelled() => PeerEvent::Cancelled,
            joined = async {
                let (_, task, _) = read.as_mut().expect("read branch is guarded");
                task.await
            }, if read.is_some() => PeerEvent::Read(joined),
            message = io.next_message() => PeerEvent::Message(message),
        };
        let actions = match event {
            PeerEvent::Cancelled => {
                join_read(read.take()).await;
                return PeerTermination::Cancelled;
            }
            PeerEvent::Read(joined) => {
                let (pending, _, _observation) = read.take().expect("completed read is present");
                let result = match joined {
                    Ok(result) => result,
                    Err(_) => Err(()),
                };
                upload.on_read_complete(pending, result)
            }
            PeerEvent::Message(Err(crate::peer_io::PeerIoError::Frame(_))) => {
                join_read(read.take()).await;
                return PeerTermination::Protocol;
            }
            PeerEvent::Message(Err(_)) => {
                join_read(read.take()).await;
                return PeerTermination::Closed;
            }
            PeerEvent::Message(Ok(message)) => {
                match handle_metadata_message(
                    &mut io,
                    &mut metadata,
                    &mut remote_metadata_id,
                    &message,
                )
                .await
                {
                    Ok(()) => upload.on_message(&message),
                    Err(()) => {
                        join_read(read.take()).await;
                        return PeerTermination::Protocol;
                    }
                }
            }
        };
        for action in actions {
            match action {
                UploadAction::Send(message) => {
                    if let PeerMessage::Piece { block, .. } = &message {
                        let mut observations = shared.observations_guard();
                        observations.payload_bytes_sent = observations
                            .payload_bytes_sent
                            .saturating_add(block.len() as u64);
                    }
                    if io.send_message(&message).await.is_err() {
                        join_read(read.take()).await;
                        return PeerTermination::Closed;
                    }
                }
                UploadAction::Read(pending) => {
                    if read.is_some() {
                        join_read(read.take()).await;
                        return PeerTermination::Protocol;
                    }
                    let content = registration.content.clone();
                    read = Some((
                        pending,
                        tokio::spawn(async move {
                            content.read_block(pending.request).await.map_err(|_| ())
                        }),
                        ObservationGuard::read(&shared, pending.request.length as usize),
                    ));
                }
                UploadAction::Close(reason) => {
                    join_read(read.take()).await;
                    return match reason {
                        UploadCloseReason::ReadFailed | UploadCloseReason::ShortRead => {
                            PeerTermination::Storage
                        }
                        UploadCloseReason::InvalidRequest | UploadCloseReason::RequestLimit => {
                            PeerTermination::Protocol
                        }
                    };
                }
            }
        }
        let snapshot = upload.snapshot();
        let mut observations = shared.observations_guard();
        observations.queued_requests_high_water = observations
            .queued_requests_high_water
            .max(snapshot.queued_requests_high_water);
        observations.queued_bytes_high_water = observations
            .queued_bytes_high_water
            .max(snapshot.queued_bytes_high_water);
    }
}

enum PeerEvent {
    Cancelled,
    Read(Result<Result<Vec<u8>, ()>, tokio::task::JoinError>),
    Message(Result<PeerMessage, crate::peer_io::PeerIoError>),
}

async fn handle_metadata_message(
    io: &mut PeerIo,
    upload: &mut MetadataUpload,
    remote_metadata_id: &mut Option<u8>,
    message: &PeerMessage,
) -> Result<(), ()> {
    match message {
        PeerMessage::Extended { id: 0, payload } => {
            let handshake = parse_extension_handshake(payload).map_err(|_| ())?;
            match handshake.metadata_extension {
                MetadataExtensionUpdate::Unchanged => {}
                MetadataExtensionUpdate::Disabled => *remote_metadata_id = None,
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
            let response = upload.on_request(piece).map_err(|_| ())?;
            let payload = match response {
                MetadataUploadAction::Data {
                    piece,
                    total_size,
                    block,
                } => encode_metadata_data(piece, total_size, &block).map_err(|_| ())?,
                MetadataUploadAction::Reject { piece } => encode_metadata_reject(piece),
            };
            io.send_message(&PeerMessage::Extended {
                id: remote_id,
                payload,
            })
            .await
            .map_err(|_| ())?;
        }
        PeerMessage::Extended { .. } => {}
        _ => {}
    }
    Ok(())
}

async fn join_read(read: Option<ActiveRead>) {
    if let Some((_, read, _observation)) = read {
        let _ = read.await;
    }
}

#[derive(Debug)]
pub enum IncomingPeerError {
    InvalidFixedPort,
    InvalidTimeout,
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
        MetadataExtensionUpdate, MetadataMessage, encode_extension_handshake_with_id,
        encode_metadata_request, parse_extension_handshake, parse_metadata_message,
    };
    use rstorrent_protocol::metainfo::{BEP9_METAINFO_LIMITS, Metainfo};
    use rstorrent_protocol::peer_wire::{
        EXTENSION_PROTOCOL_RESERVED_BIT, EXTENSION_PROTOCOL_RESERVED_INDEX, FrameDecoder,
        HANDSHAKE_LENGTH, PeerMessage, decode_handshake, encode_handshake_with_reserved,
        encode_message,
    };
    use sha1::{Digest, Sha1};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::time::timeout;

    use super::{
        IncomingPeerError, IncomingPeerService, IncomingPeerServiceConfig, IncomingRejectionReason,
        IncomingTcpBootstrap, SeedRegistration,
    };
    use crate::{DEFAULT_PEER_ID, SeedContent};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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
            peer_io_timeout: Duration::from_secs(2),
            peer_id: DEFAULT_PEER_ID,
            byte_metric_sink: None,
        }
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
    async fn unknown_timeout_self_and_established_saturation_are_bounded() {
        let (root, _, registration) = registration("rejections").await;
        let info_hash = registration.info_hash();
        let mut service_config = config(IncomingTcpBootstrap::AutomaticLoopback);
        service_config.handshake_timeout = Duration::from_millis(50);
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
        assert_eq!(second.read(&mut [0; 1]).await.expect("saturated close"), 0);
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
                        .get(&IncomingRejectionReason::EstablishedLimit)
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
