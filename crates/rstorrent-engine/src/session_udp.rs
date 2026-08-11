//! Single-receiver, bounded session UDP transport ownership.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use rstorrent_protocol::dht::MAX_DATAGRAM_SIZE;
use rstorrent_protocol::utp::{IPV4_UDP_PAYLOAD_CEILING, UTP_HEADER_SIZE, UTP_VERSION};
use tokio::net::UdpSocket;
use tokio::sync::{Mutex, MutexGuard, mpsc, watch};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::network::AddressFamily;
use crate::udp_fragmentation::{
    Ipv4FragmentationProtectionStatus, verify_ipv4_fragmentation_protection,
};

pub const SESSION_UDP_DHT_QUEUE: usize = 64;
pub const SESSION_UDP_UTP_QUEUE: usize = 256;
pub const SESSION_UDP_UTP_DATAGRAM_BYTES: usize = IPV4_UDP_PAYLOAD_CEILING;
const SESSION_UDP_DHT_RECEIVE_BYTES: usize = MAX_DATAGRAM_SIZE + 1;
const SESSION_UDP_RECEIVE_BYTES: usize = SESSION_UDP_UTP_DATAGRAM_BYTES + 1;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SessionUdpSnapshot {
    pub tasks: usize,
    pub task_high_water: usize,
    pub queued: usize,
    pub queue_high_water: usize,
    pub datagrams_received: u64,
    pub datagram_bytes_received: u64,
    pub datagrams_dropped: u64,
    pub dht_datagrams_dropped: u64,
    pub utp_queued: usize,
    pub utp_queue_high_water: usize,
    pub utp_datagrams_classified: u64,
    pub utp_datagram_bytes_classified: u64,
    pub utp_datagrams_dropped: u64,
    pub egress_waiters: usize,
    pub egress_waiter_high_water: usize,
    pub retired_egress_rejections: u64,
    pub ipv4_fragmentation_protection: Ipv4FragmentationProtectionStatus,
}

#[derive(Debug)]
pub enum SessionUdpError {
    Io(io::Error),
    MissingFamily(AddressFamily),
    StaleGeneration {
        family: AddressFamily,
        requested: u64,
        current: Option<u64>,
    },
    RetiredGeneration {
        family: AddressFamily,
        generation: u64,
    },
    UtpTransportTaken,
    TaskJoin(String),
}

impl fmt::Display for SessionUdpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "session UDP I/O failed: {error}"),
            Self::MissingFamily(family) => {
                write!(formatter, "session UDP {family} family is unavailable")
            }
            Self::StaleGeneration {
                family,
                requested,
                current,
            } => write!(
                formatter,
                "session UDP {family} generation {requested} is stale; current generation is {current:?}"
            ),
            Self::RetiredGeneration { family, generation } => write!(
                formatter,
                "session UDP {family} generation {generation} retired before send"
            ),
            Self::UtpTransportTaken => write!(formatter, "session uTP transport was already taken"),
            Self::TaskJoin(error) => write!(formatter, "session UDP task join failed: {error}"),
        }
    }
}

impl Error for SessionUdpError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::MissingFamily(_)
            | Self::StaleGeneration { .. }
            | Self::RetiredGeneration { .. }
            | Self::UtpTransportTaken
            | Self::TaskJoin(_) => None,
        }
    }
}

#[derive(Debug)]
pub(crate) enum SessionUdpIngress {
    Datagram {
        family: AddressFamily,
        source: SocketAddr,
        bytes: Vec<u8>,
    },
    Failed {
        family: AddressFamily,
        detail: String,
    },
}

#[derive(Debug)]
pub(crate) enum SessionUtpIngress {
    Datagram {
        generation: u64,
        family: AddressFamily,
        source: SocketAddr,
        bytes: Vec<u8>,
    },
    Failed {
        generation: u64,
        family: AddressFamily,
        detail: String,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SessionUdpGenerations {
    families: BTreeMap<AddressFamily, u64>,
}

impl SessionUdpGenerations {
    pub(crate) fn generation_for(&self, family: AddressFamily) -> Option<u64> {
        self.families.get(&family).copied()
    }
}

#[derive(Debug)]
pub struct SessionUdpTransport {
    current: Arc<RwLock<SessionUdpCurrent>>,
    ingress: mpsc::Receiver<SessionUdpIngress>,
    ingress_sender: mpsc::Sender<SessionUdpIngress>,
    utp_ingress_sender: mpsc::Sender<SessionUtpIngress>,
    stats: Arc<SessionUdpStats>,
}

#[derive(Debug)]
pub(crate) struct SessionUtpTransport {
    current: Arc<RwLock<SessionUdpCurrent>>,
    ingress: mpsc::Receiver<SessionUtpIngress>,
    generation_changes: watch::Receiver<SessionUdpGenerations>,
}

#[derive(Clone, Debug)]
pub(crate) struct SessionUtpSendHandle {
    current: Arc<RwLock<SessionUdpCurrent>>,
}

#[derive(Debug)]
struct SessionUdpCurrent {
    families: BTreeMap<AddressFamily, SessionUdpFamilyCurrent>,
}

#[derive(Debug)]
struct SessionUdpFamilyCurrent {
    generation: u64,
    egress: Arc<SessionUdpEgress>,
    local_address: SocketAddr,
}

#[derive(Debug)]
struct SessionUdpEgress {
    generation: u64,
    family: AddressFamily,
    socket: Arc<UdpSocket>,
    exclusion: Mutex<()>,
    active: AtomicBool,
    ipv4_fragmentation_protection: Ipv4FragmentationProtectionStatus,
    stats: Arc<SessionUdpStats>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SessionUdpEgressError {
    Retired {
        family: AddressFamily,
        generation: u64,
    },
}

impl SessionUdpEgress {
    fn new(
        generation: u64,
        family: AddressFamily,
        socket: Arc<UdpSocket>,
        stats: Arc<SessionUdpStats>,
    ) -> Result<Self, SessionUdpError> {
        let ipv4_fragmentation_protection = if family == AddressFamily::Ipv4 {
            verify_ipv4_fragmentation_protection(&socket).map_err(SessionUdpError::Io)?
        } else {
            Ipv4FragmentationProtectionStatus::UnsupportedPlatform
        };
        Ok(Self {
            generation,
            family,
            socket,
            exclusion: Mutex::new(()),
            active: AtomicBool::new(true),
            ipv4_fragmentation_protection,
            stats,
        })
    }

    async fn send_to(
        &self,
        bytes: &[u8],
        target: SocketAddr,
    ) -> Result<Result<usize, io::Error>, SessionUdpEgressError> {
        let _guard = self.lock().await;
        if !self.active.load(Ordering::Acquire) {
            self.stats.record_retired_egress_rejection();
            return Err(SessionUdpEgressError::Retired {
                family: self.family,
                generation: self.generation,
            });
        }
        Ok(self.socket.send_to(bytes, target).await)
    }

    async fn lock(&self) -> MutexGuard<'_, ()> {
        match self.exclusion.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                let waiter = EgressWaiter::new(&self.stats);
                let guard = self.exclusion.lock().await;
                drop(waiter);
                guard
            }
        }
    }
}

struct EgressWaiter<'a> {
    stats: &'a SessionUdpStats,
}

impl<'a> EgressWaiter<'a> {
    fn new(stats: &'a SessionUdpStats) -> Self {
        stats.record_egress_waiter_started();
        Self { stats }
    }
}

impl Drop for EgressWaiter<'_> {
    fn drop(&mut self) {
        self.stats.record_egress_waiter_finished();
    }
}

#[derive(Clone, Debug)]
pub struct SessionUdpHandle {
    current: Arc<RwLock<SessionUdpCurrent>>,
    ingress_sender: mpsc::Sender<SessionUdpIngress>,
    utp_ingress_sender: mpsc::Sender<SessionUtpIngress>,
    stats: Arc<SessionUdpStats>,
}

impl SessionUdpHandle {
    pub fn generation(&self) -> u64 {
        self.family_current(AddressFamily::Ipv4)
            .or_else(|| self.first_family_current())
            .expect("session UDP has at least one family")
            .generation
    }

    pub fn local_address(&self) -> SocketAddr {
        self.local_address_for(AddressFamily::Ipv4)
            .or_else(|| self.first_family_current().map(|entry| entry.local_address))
            .expect("session UDP has at least one family")
    }

    pub fn generation_for(&self, family: AddressFamily) -> Option<u64> {
        self.family_current(family).map(|entry| entry.generation)
    }

    pub fn local_address_for(&self, family: AddressFamily) -> Option<SocketAddr> {
        self.family_current(family).map(|entry| entry.local_address)
    }

    fn family_current(&self, family: AddressFamily) -> Option<SessionUdpFamilyCurrentSnapshot> {
        self.current_guard()
            .families
            .get(&family)
            .map(|entry| SessionUdpFamilyCurrentSnapshot {
                generation: entry.generation,
                local_address: entry.local_address,
            })
    }

    fn first_family_current(&self) -> Option<SessionUdpFamilyCurrentSnapshot> {
        self.current_guard()
            .families
            .values()
            .next()
            .map(|entry| SessionUdpFamilyCurrentSnapshot {
                generation: entry.generation,
                local_address: entry.local_address,
            })
    }

    pub fn snapshot(&self) -> SessionUdpSnapshot {
        let ipv4_fragmentation_protection = self
            .current_guard()
            .families
            .get(&AddressFamily::Ipv4)
            .map_or(
                Ipv4FragmentationProtectionStatus::UnsupportedPlatform,
                |entry| entry.egress.ipv4_fragmentation_protection,
            );
        self.stats.snapshot(
            &self.ingress_sender,
            &self.utp_ingress_sender,
            ipv4_fragmentation_protection,
        )
    }

    fn current_guard(&self) -> RwLockReadGuard<'_, SessionUdpCurrent> {
        self.current
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[derive(Clone, Copy, Debug)]
struct SessionUdpFamilyCurrentSnapshot {
    generation: u64,
    local_address: SocketAddr,
}

impl SessionUdpTransport {
    pub fn local_address(&self) -> SocketAddr {
        self.local_address_for(AddressFamily::Ipv4)
            .or_else(|| {
                self.current_guard()
                    .families
                    .values()
                    .next()
                    .map(|entry| entry.local_address)
            })
            .expect("session UDP has at least one family")
    }

    pub fn local_address_for(&self, family: AddressFamily) -> Option<SocketAddr> {
        self.current_guard()
            .families
            .get(&family)
            .map(|entry| entry.local_address)
    }

    pub fn handle(&self) -> SessionUdpHandle {
        SessionUdpHandle {
            current: self.current.clone(),
            ingress_sender: self.ingress_sender.clone(),
            utp_ingress_sender: self.utp_ingress_sender.clone(),
            stats: self.stats.clone(),
        }
    }

    pub(crate) async fn receive(
        &mut self,
    ) -> Result<(Vec<u8>, SocketAddr, AddressFamily), SessionUdpError> {
        match self.ingress.recv().await {
            Some(SessionUdpIngress::Datagram {
                family,
                source,
                bytes,
            }) => Ok((bytes, source, family)),
            Some(SessionUdpIngress::Failed { family, detail }) => Err(SessionUdpError::Io(
                io::Error::other(format!("{family} receive failed: {detail}")),
            )),
            None => Err(SessionUdpError::Io(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "session UDP ingress stopped",
            ))),
        }
    }

    pub(crate) async fn send_to(
        &self,
        bytes: &[u8],
        target: SocketAddr,
    ) -> Result<usize, SessionUdpError> {
        let family = AddressFamily::of(target.ip());
        for attempt in 0..2 {
            let egress = self
                .current_guard()
                .families
                .get(&family)
                .map(|entry| entry.egress.clone())
                .ok_or(SessionUdpError::MissingFamily(family))?;
            match egress.send_to(bytes, target).await {
                Ok(result) => return result.map_err(SessionUdpError::Io),
                Err(SessionUdpEgressError::Retired { .. }) if attempt == 0 => {}
                Err(SessionUdpEgressError::Retired { family, generation }) => {
                    return Err(SessionUdpError::RetiredGeneration { family, generation });
                }
            }
        }
        unreachable!("session UDP send attempts are statically bounded")
    }

    fn current_guard(&self) -> RwLockReadGuard<'_, SessionUdpCurrent> {
        self.current
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl SessionUtpTransport {
    pub(crate) fn local_address_for(&self, family: AddressFamily) -> Option<SocketAddr> {
        self.current_guard()
            .families
            .get(&family)
            .map(|entry| entry.local_address)
    }

    pub(crate) fn generation_for(&self, family: AddressFamily) -> Option<u64> {
        self.current_guard()
            .families
            .get(&family)
            .map(|entry| entry.generation)
    }

    pub(crate) fn generation_changes(&self) -> watch::Receiver<SessionUdpGenerations> {
        self.generation_changes.clone()
    }

    pub(crate) fn send_handle(&self) -> SessionUtpSendHandle {
        SessionUtpSendHandle {
            current: self.current.clone(),
        }
    }

    pub(crate) async fn receive(
        &mut self,
    ) -> Result<(u64, Vec<u8>, SocketAddr, AddressFamily), SessionUdpError> {
        match self.ingress.recv().await {
            Some(SessionUtpIngress::Datagram {
                generation,
                family,
                source,
                bytes,
            }) => Ok((generation, bytes, source, family)),
            Some(SessionUtpIngress::Failed {
                generation,
                family,
                detail,
            }) => Err(SessionUdpError::Io(io::Error::other(format!(
                "{family} generation {generation} receive failed: {detail}"
            )))),
            None => Err(SessionUdpError::Io(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "session uTP ingress stopped",
            ))),
        }
    }

    fn current_guard(&self) -> RwLockReadGuard<'_, SessionUdpCurrent> {
        self.current
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl SessionUtpSendHandle {
    pub(crate) async fn send_to(
        &self,
        generation: u64,
        bytes: &[u8],
        target: SocketAddr,
    ) -> Result<usize, SessionUdpError> {
        let family = AddressFamily::of(target.ip());
        let egress = {
            let current = self.current_guard();
            let Some(entry) = current.families.get(&family) else {
                return Err(SessionUdpError::MissingFamily(family));
            };
            if entry.generation != generation {
                return Err(SessionUdpError::StaleGeneration {
                    family,
                    requested: generation,
                    current: Some(entry.generation),
                });
            }
            entry.egress.clone()
        };
        match egress.send_to(bytes, target).await {
            Ok(result) => result.map_err(SessionUdpError::Io),
            Err(SessionUdpEgressError::Retired { .. }) => {
                let current = self
                    .current_guard()
                    .families
                    .get(&family)
                    .map(|entry| entry.generation);
                Err(SessionUdpError::StaleGeneration {
                    family,
                    requested: generation,
                    current,
                })
            }
        }
    }

    fn current_guard(&self) -> RwLockReadGuard<'_, SessionUdpCurrent> {
        self.current
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[derive(Debug, Default)]
struct SessionUdpStats {
    tasks: AtomicUsize,
    task_high_water: AtomicUsize,
    queue_high_water: AtomicUsize,
    utp_queue_high_water: AtomicUsize,
    datagrams_received: AtomicU64,
    datagram_bytes_received: AtomicU64,
    datagrams_dropped: AtomicU64,
    dht_datagrams_dropped: AtomicU64,
    utp_datagrams_classified: AtomicU64,
    utp_datagram_bytes_classified: AtomicU64,
    utp_datagrams_dropped: AtomicU64,
    egress_waiters: AtomicUsize,
    egress_waiter_high_water: AtomicUsize,
    retired_egress_rejections: AtomicU64,
}

impl SessionUdpStats {
    fn snapshot(
        &self,
        dht_sender: &mpsc::Sender<SessionUdpIngress>,
        utp_sender: &mpsc::Sender<SessionUtpIngress>,
        ipv4_fragmentation_protection: Ipv4FragmentationProtectionStatus,
    ) -> SessionUdpSnapshot {
        SessionUdpSnapshot {
            tasks: self.tasks.load(Ordering::Relaxed),
            task_high_water: self.task_high_water.load(Ordering::Relaxed),
            queued: queued(dht_sender),
            queue_high_water: self.queue_high_water.load(Ordering::Relaxed),
            datagrams_received: self.datagrams_received.load(Ordering::Relaxed),
            datagram_bytes_received: self.datagram_bytes_received.load(Ordering::Relaxed),
            datagrams_dropped: self.datagrams_dropped.load(Ordering::Relaxed),
            dht_datagrams_dropped: self.dht_datagrams_dropped.load(Ordering::Relaxed),
            utp_queued: queued(utp_sender),
            utp_queue_high_water: self.utp_queue_high_water.load(Ordering::Relaxed),
            utp_datagrams_classified: self.utp_datagrams_classified.load(Ordering::Relaxed),
            utp_datagram_bytes_classified: self
                .utp_datagram_bytes_classified
                .load(Ordering::Relaxed),
            utp_datagrams_dropped: self.utp_datagrams_dropped.load(Ordering::Relaxed),
            egress_waiters: self.egress_waiters.load(Ordering::Relaxed),
            egress_waiter_high_water: self.egress_waiter_high_water.load(Ordering::Relaxed),
            retired_egress_rejections: self.retired_egress_rejections.load(Ordering::Relaxed),
            ipv4_fragmentation_protection,
        }
    }

    fn record_egress_waiter_started(&self) {
        let waiters = self.egress_waiters.fetch_add(1, Ordering::Relaxed) + 1;
        self.egress_waiter_high_water
            .fetch_max(waiters, Ordering::Relaxed);
    }

    fn record_egress_waiter_finished(&self) {
        let previous = self.egress_waiters.fetch_sub(1, Ordering::Relaxed);
        debug_assert!(previous > 0, "session UDP egress waiter underflow");
    }

    fn record_retired_egress_rejection(&self) {
        saturating_add(&self.retired_egress_rejections, 1);
    }

    fn record_datagram(&self, bytes: usize) {
        saturating_add(&self.datagrams_received, 1);
        saturating_add(
            &self.datagram_bytes_received,
            u64::try_from(bytes).unwrap_or(u64::MAX),
        );
    }

    fn record_drop(&self) {
        saturating_add(&self.datagrams_dropped, 1);
    }

    fn record_dht_drop(&self) {
        self.record_drop();
        saturating_add(&self.dht_datagrams_dropped, 1);
    }

    fn record_utp_datagram(&self, bytes: usize) {
        saturating_add(&self.utp_datagrams_classified, 1);
        saturating_add(
            &self.utp_datagram_bytes_classified,
            u64::try_from(bytes).unwrap_or(u64::MAX),
        );
    }

    fn record_utp_drop(&self) {
        self.record_drop();
        saturating_add(&self.utp_datagrams_dropped, 1);
    }
}

fn queued<T>(sender: &mpsc::Sender<T>) -> usize {
    if sender.is_closed() {
        0
    } else {
        sender.max_capacity().saturating_sub(sender.capacity())
    }
}

fn saturating_add(value: &AtomicU64, increment: u64) {
    let _ = value.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
        Some(current.saturating_add(increment))
    });
}

#[derive(Debug)]
pub struct SessionUdpService {
    handle: SessionUdpHandle,
    active: BTreeMap<AddressFamily, SessionUdpGeneration>,
    next_generation: u64,
    ingress_sender: mpsc::Sender<SessionUdpIngress>,
    utp_ingress: Option<mpsc::Receiver<SessionUtpIngress>>,
    utp_ingress_sender: mpsc::Sender<SessionUtpIngress>,
    generation_sender: watch::Sender<SessionUdpGenerations>,
    stats: Arc<SessionUdpStats>,
}

#[derive(Debug)]
struct SessionUdpGeneration {
    cancellation: CancellationToken,
    task: Option<JoinHandle<Result<(), SessionUdpError>>>,
}

impl SessionUdpGeneration {
    async fn shutdown(mut self) -> Result<(), SessionUdpError> {
        self.cancellation.cancel();
        self.task
            .take()
            .expect("session UDP generation task exists before shutdown")
            .await
            .map_err(|error| SessionUdpError::TaskJoin(error.to_string()))?
    }
}

impl Drop for SessionUdpGeneration {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl SessionUdpService {
    pub fn start(socket: UdpSocket) -> Result<(Self, SessionUdpTransport), SessionUdpError> {
        let local_address = socket.local_addr().map_err(SessionUdpError::Io)?;
        let family = AddressFamily::of(local_address.ip());
        let socket = Arc::new(socket);
        let (ingress_sender, ingress) = mpsc::channel(SESSION_UDP_DHT_QUEUE);
        let (utp_ingress_sender, utp_ingress) = mpsc::channel(SESSION_UDP_UTP_QUEUE);
        let stats = Arc::new(SessionUdpStats::default());
        let generation = 1;
        let egress = Arc::new(SessionUdpEgress::new(
            generation,
            family,
            socket.clone(),
            stats.clone(),
        )?);
        let current = Arc::new(RwLock::new(SessionUdpCurrent {
            families: BTreeMap::from([(
                family,
                SessionUdpFamilyCurrent {
                    generation,
                    egress,
                    local_address,
                },
            )]),
        }));
        let (generation_sender, _) = watch::channel(SessionUdpGenerations {
            families: BTreeMap::from([(family, generation)]),
        });
        let active = BTreeMap::from([(
            family,
            start_generation(
                generation,
                family,
                socket,
                ingress_sender.clone(),
                utp_ingress_sender.clone(),
                stats.clone(),
            ),
        )]);
        let handle = SessionUdpHandle {
            current: current.clone(),
            ingress_sender: ingress_sender.clone(),
            utp_ingress_sender: utp_ingress_sender.clone(),
            stats: stats.clone(),
        };
        Ok((
            Self {
                handle,
                active,
                next_generation: 2,
                ingress_sender: ingress_sender.clone(),
                utp_ingress: Some(utp_ingress),
                utp_ingress_sender: utp_ingress_sender.clone(),
                generation_sender,
                stats: stats.clone(),
            },
            SessionUdpTransport {
                current,
                ingress,
                ingress_sender,
                utp_ingress_sender,
                stats,
            },
        ))
    }

    pub fn local_address(&self) -> SocketAddr {
        self.handle.local_address()
    }

    pub fn local_address_for(&self, family: AddressFamily) -> Option<SocketAddr> {
        self.handle.local_address_for(family)
    }

    pub fn handle(&self) -> SessionUdpHandle {
        self.handle.clone()
    }

    pub fn generation(&self) -> u64 {
        self.handle.generation()
    }

    pub fn generation_for(&self, family: AddressFamily) -> Option<u64> {
        self.handle.generation_for(family)
    }

    pub fn snapshot(&self) -> SessionUdpSnapshot {
        self.handle.snapshot()
    }

    pub(crate) fn take_utp_transport(&mut self) -> Result<SessionUtpTransport, SessionUdpError> {
        let ingress = self
            .utp_ingress
            .take()
            .ok_or(SessionUdpError::UtpTransportTaken)?;
        Ok(SessionUtpTransport {
            current: self.handle.current.clone(),
            ingress,
            generation_changes: self.generation_sender.subscribe(),
        })
    }

    pub async fn replace_socket(&mut self, socket: UdpSocket) -> Result<(), SessionUdpError> {
        let local_address = socket.local_addr().map_err(SessionUdpError::Io)?;
        let family = AddressFamily::of(local_address.ip());
        let generation = self.next_generation;
        self.next_generation = self.next_generation.checked_add(1).ok_or_else(|| {
            SessionUdpError::Io(io::Error::other("session UDP generation exhausted"))
        })?;
        let socket = Arc::new(socket);
        let egress = Arc::new(SessionUdpEgress::new(
            generation,
            family,
            socket.clone(),
            self.stats.clone(),
        )?);
        let candidate = start_generation(
            generation,
            family,
            socket.clone(),
            self.ingress_sender.clone(),
            self.utp_ingress_sender.clone(),
            self.stats.clone(),
        );
        let previous_egress = self
            .handle
            .current_guard()
            .families
            .get(&family)
            .map(|entry| entry.egress.clone());
        let retirement = match &previous_egress {
            Some(previous) => {
                let guard = previous.lock().await;
                previous.active.store(false, Ordering::Release);
                Some(guard)
            }
            None => None,
        };
        {
            let mut current = self.current_guard();
            current.families.insert(
                family,
                SessionUdpFamilyCurrent {
                    generation,
                    egress,
                    local_address,
                },
            );
        }
        drop(retirement);
        self.publish_generations();
        let previous = self.active.insert(family, candidate);
        match previous {
            Some(previous) => previous.shutdown().await,
            None => Ok(()),
        }
    }

    pub async fn remove_family(&mut self, family: AddressFamily) -> Result<(), SessionUdpError> {
        let previous_egress = self
            .handle
            .current_guard()
            .families
            .get(&family)
            .map(|entry| entry.egress.clone());
        let retirement = match &previous_egress {
            Some(previous) => {
                let guard = previous.lock().await;
                previous.active.store(false, Ordering::Release);
                Some(guard)
            }
            None => None,
        };
        self.current_guard().families.remove(&family);
        drop(retirement);
        self.publish_generations();
        match self.active.remove(&family) {
            Some(previous) => previous.shutdown().await,
            None => Ok(()),
        }
    }

    pub async fn shutdown(mut self) -> Result<SessionUdpSnapshot, SessionUdpError> {
        let egress = self
            .handle
            .current_guard()
            .families
            .values()
            .map(|entry| entry.egress.clone())
            .collect::<Vec<_>>();
        for entry in egress {
            let _guard = entry.lock().await;
            entry.active.store(false, Ordering::Release);
        }
        let active = std::mem::take(&mut self.active);
        for (_, generation) in active {
            generation.shutdown().await?;
        }
        Ok(self.snapshot())
    }

    fn current_guard(&self) -> RwLockWriteGuard<'_, SessionUdpCurrent> {
        self.handle
            .current
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn publish_generations(&self) {
        let families = self
            .handle
            .current_guard()
            .families
            .iter()
            .map(|(family, entry)| (*family, entry.generation))
            .collect();
        self.generation_sender
            .send_replace(SessionUdpGenerations { families });
    }
}

impl Drop for SessionUdpService {
    fn drop(&mut self) {
        self.active.clear();
    }
}

fn start_generation(
    generation: u64,
    family: AddressFamily,
    socket: Arc<UdpSocket>,
    ingress_sender: mpsc::Sender<SessionUdpIngress>,
    utp_ingress_sender: mpsc::Sender<SessionUtpIngress>,
    stats: Arc<SessionUdpStats>,
) -> SessionUdpGeneration {
    let cancellation = CancellationToken::new();
    let tasks = stats.tasks.fetch_add(1, Ordering::AcqRel).saturating_add(1);
    stats.task_high_water.fetch_max(tasks, Ordering::AcqRel);
    let task = tokio::spawn(run_receive_loop(
        generation,
        family,
        socket,
        ingress_sender,
        utp_ingress_sender,
        stats,
        cancellation.clone(),
    ));
    SessionUdpGeneration {
        cancellation,
        task: Some(task),
    }
}

async fn run_receive_loop(
    generation: u64,
    family: AddressFamily,
    socket: Arc<UdpSocket>,
    dht_ingress: mpsc::Sender<SessionUdpIngress>,
    utp_ingress: mpsc::Sender<SessionUtpIngress>,
    stats: Arc<SessionUdpStats>,
    cancellation: CancellationToken,
) -> Result<(), SessionUdpError> {
    let mut bytes = [0_u8; SESSION_UDP_RECEIVE_BYTES];
    let result = loop {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => break Ok(()),
            received = socket.recv_from(&mut bytes) => {
                match received {
                    Ok((length, source)) => {
                        stats.record_datagram(length);
                        if AddressFamily::of(source.ip()) != family {
                            stats.record_drop();
                            continue;
                        }
                        if looks_like_utp(&bytes[..length]) {
                            stats.record_utp_datagram(length);
                            dispatch_utp(
                                &utp_ingress,
                                &stats,
                                SessionUtpIngress::Datagram {
                                    generation,
                                    family,
                                    source,
                                    bytes: bytes[..length].to_vec(),
                                },
                            );
                        } else {
                            dispatch_dht(
                                &dht_ingress,
                                &stats,
                                SessionUdpIngress::Datagram {
                                family,
                                source,
                                bytes: bytes[..length.min(SESSION_UDP_DHT_RECEIVE_BYTES)].to_vec(),
                                },
                            );
                        }
                    }
                    Err(error) => {
                        let detail = error.to_string();
                        let _ = dispatch_dht(
                            &dht_ingress,
                            &stats,
                            SessionUdpIngress::Failed {
                                family,
                                detail: detail.clone(),
                            },
                        );
                        let _ = dispatch_utp(
                            &utp_ingress,
                            &stats,
                            SessionUtpIngress::Failed {
                                generation,
                                family,
                                detail,
                            },
                        );
                        break Err(SessionUdpError::Io(error));
                    }
                }
            }
        }
    };
    stats.tasks.fetch_sub(1, Ordering::AcqRel);
    result
}

fn looks_like_utp(bytes: &[u8]) -> bool {
    bytes.len() >= UTP_HEADER_SIZE && bytes[0] & 0x0f == UTP_VERSION && bytes[0] >> 4 <= 4
}

fn dispatch_dht(
    ingress: &mpsc::Sender<SessionUdpIngress>,
    stats: &SessionUdpStats,
    datagram: SessionUdpIngress,
) -> bool {
    match ingress.try_send(datagram) {
        Ok(()) => {
            let queued = ingress.max_capacity().saturating_sub(ingress.capacity());
            stats.queue_high_water.fetch_max(queued, Ordering::Relaxed);
            true
        }
        Err(mpsc::error::TrySendError::Full(_)) => {
            stats.record_dht_drop();
            true
        }
        Err(mpsc::error::TrySendError::Closed(_)) => false,
    }
}

fn dispatch_utp(
    ingress: &mpsc::Sender<SessionUtpIngress>,
    stats: &SessionUdpStats,
    datagram: SessionUtpIngress,
) -> bool {
    match ingress.try_send(datagram) {
        Ok(()) => {
            let queued = ingress.max_capacity().saturating_sub(ingress.capacity());
            stats
                .utp_queue_high_water
                .fetch_max(queued, Ordering::Relaxed);
            true
        }
        Err(mpsc::error::TrySendError::Full(_)) => {
            stats.record_utp_drop();
            true
        }
        Err(mpsc::error::TrySendError::Closed(_)) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::net::{Ipv4Addr, Ipv6Addr};
    use tokio::time::{Duration, timeout};

    async fn service() -> (SessionUdpService, SessionUdpTransport) {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        SessionUdpService::start(socket).unwrap()
    }

    async fn wait_for_egress_waiters(handle: &SessionUdpHandle, expected: usize) {
        timeout(Duration::from_secs(1), async {
            loop {
                if handle.snapshot().egress_waiters == expected {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("session UDP egress waiter count reached the expected value");
    }

    #[test]
    fn long_lived_counters_saturate() {
        let stats = SessionUdpStats::default();
        stats
            .datagrams_received
            .store(u64::MAX - 1, Ordering::Relaxed);
        stats
            .datagram_bytes_received
            .store(u64::MAX - 2, Ordering::Relaxed);
        stats.datagrams_dropped.store(u64::MAX, Ordering::Relaxed);
        stats.record_datagram(8);
        stats.record_drop();
        assert_eq!(stats.datagrams_received.load(Ordering::Relaxed), u64::MAX);
        assert_eq!(
            stats.datagram_bytes_received.load(Ordering::Relaxed),
            u64::MAX
        );
        assert_eq!(stats.datagrams_dropped.load(Ordering::Relaxed), u64::MAX);
    }

    #[tokio::test]
    async fn one_transport_receives_and_sends_on_the_owned_socket() {
        let (service, mut transport) = service().await;
        let remote = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let remote_address = remote.local_addr().unwrap();
        remote
            .send_to(b"d1:q4:pinge", transport.local_address())
            .await
            .unwrap();
        let (received, source, family) = timeout(Duration::from_secs(1), transport.receive())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received, b"d1:q4:pinge");
        assert_eq!(source, remote_address);
        assert_eq!(family, AddressFamily::Ipv4);

        transport.send_to(b"response", source).await.unwrap();
        let mut response = [0; 32];
        let (length, source) = timeout(Duration::from_secs(1), remote.recv_from(&mut response))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&response[..length], b"response");
        assert_eq!(source, transport.local_address());
        drop(transport);
        let terminal = service.shutdown().await.unwrap();
        assert_eq!(terminal.tasks, 0);
        assert_eq!(terminal.queued, 0);
        assert_eq!(terminal.task_high_water, 1);
        assert_eq!(terminal.datagrams_received, 1);
        assert_eq!(terminal.datagrams_dropped, 0);
        assert_eq!(terminal.egress_waiters, 0);
    }

    #[tokio::test]
    async fn dht_and_utp_sends_share_one_bounded_egress_exclusion() {
        let (mut service, dht) = service().await;
        let utp = service.take_utp_transport().unwrap();
        let generation = utp.generation_for(AddressFamily::Ipv4).unwrap();
        let send = utp.send_handle();
        let handle = service.handle();
        let egress = handle
            .current_guard()
            .families
            .get(&AddressFamily::Ipv4)
            .unwrap()
            .egress
            .clone();
        let exclusion = egress.exclusion.lock().await;
        let remote = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let target = remote.local_addr().unwrap();

        let dht_send = tokio::spawn(async move { dht.send_to(b"dht", target).await });
        let utp_send = tokio::spawn(async move { send.send_to(generation, b"utp", target).await });
        wait_for_egress_waiters(&handle, 2).await;
        assert_eq!(handle.snapshot().egress_waiter_high_water, 2);

        dht_send.abort();
        utp_send.abort();
        assert!(dht_send.await.unwrap_err().is_cancelled());
        assert!(utp_send.await.unwrap_err().is_cancelled());
        wait_for_egress_waiters(&handle, 0).await;
        drop(exclusion);
        drop(utp);
        let terminal = service.shutdown().await.unwrap();
        assert_eq!(terminal.egress_waiters, 0);
        assert_eq!(terminal.egress_waiter_high_water, 2);
    }

    #[tokio::test]
    async fn replacement_waits_for_egress_and_retires_the_old_generation() {
        let (mut service, dht) = service().await;
        let handle = service.handle();
        let old_egress = handle
            .current_guard()
            .families
            .get(&AddressFamily::Ipv4)
            .unwrap()
            .egress
            .clone();
        let exclusion = old_egress.exclusion.lock().await;
        let replacement = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let replacement_address = replacement.local_addr().unwrap();

        let replacing = tokio::spawn(async move {
            service.replace_socket(replacement).await.unwrap();
            service
        });
        wait_for_egress_waiters(&handle, 1).await;
        assert!(!replacing.is_finished());
        assert!(old_egress.active.load(Ordering::Acquire));

        drop(exclusion);
        let service = replacing.await.unwrap();
        assert_eq!(service.generation(), 2);
        assert_eq!(service.local_address(), replacement_address);
        assert!(!old_egress.active.load(Ordering::Acquire));
        assert_eq!(handle.snapshot().egress_waiters, 0);
        drop(dht);
        service.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn construction_reports_fragmentation_protection_capability() {
        let (service, dht) = service().await;
        let status = service.snapshot().ipv4_fragmentation_protection;
        #[cfg(any(target_os = "android", target_os = "linux"))]
        assert_eq!(status, Ipv4FragmentationProtectionStatus::Verified);
        #[cfg(not(any(target_os = "android", target_os = "linux")))]
        assert_eq!(
            status,
            Ipv4FragmentationProtectionStatus::UnsupportedPlatform
        );
        drop(dht);
        service.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn one_socket_routes_dht_and_utp_to_independent_consumers() {
        let (mut service, mut dht) = service().await;
        let mut utp = service.take_utp_transport().unwrap();
        let remote = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let remote_address = remote.local_addr().unwrap();
        let local_address = dht.local_address();
        let mut utp_packet = vec![0; UTP_HEADER_SIZE];
        utp_packet[0] = UTP_VERSION;

        remote.send_to(b"d1:q4:pinge", local_address).await.unwrap();
        remote.send_to(&utp_packet, local_address).await.unwrap();

        let (dht_bytes, dht_source, dht_family) = timeout(Duration::from_secs(1), dht.receive())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(dht_bytes, b"d1:q4:pinge");
        assert_eq!(dht_source, remote_address);
        assert_eq!(dht_family, AddressFamily::Ipv4);

        let (generation, utp_bytes, utp_source, utp_family) =
            timeout(Duration::from_secs(1), utp.receive())
                .await
                .unwrap()
                .unwrap();
        assert_eq!(generation, 1);
        assert_eq!(utp_bytes, utp_packet);
        assert_eq!(utp_source, remote_address);
        assert_eq!(utp_family, AddressFamily::Ipv4);
        assert_eq!(service.snapshot().utp_datagrams_classified, 1);
    }

    #[tokio::test]
    async fn shallow_classifier_keeps_malformed_utp_off_the_dht_route() {
        let (mut service, mut dht) = service().await;
        let mut utp = service.take_utp_transport().unwrap();
        let remote = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let mut malformed = vec![0; UTP_HEADER_SIZE];
        malformed[0] = (2 << 4) | UTP_VERSION;
        malformed[1] = 1;
        remote
            .send_to(&malformed, dht.local_address())
            .await
            .unwrap();

        let (_, received, _, _) = timeout(Duration::from_secs(1), utp.receive())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received, malformed);
        assert!(
            timeout(Duration::from_millis(25), dht.receive())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn utp_send_rejects_a_replaced_socket_generation() {
        let (mut service, _dht) = service().await;
        let utp = service.take_utp_transport().unwrap();
        let mut changes = utp.generation_changes();
        let first_generation = utp.generation_for(AddressFamily::Ipv4).unwrap();
        let first_address = utp.local_address_for(AddressFamily::Ipv4).unwrap();
        let replacement = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let replacement_address = replacement.local_addr().unwrap();
        service.replace_socket(replacement).await.unwrap();

        timeout(Duration::from_secs(1), changes.changed())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            changes
                .borrow_and_update()
                .generation_for(AddressFamily::Ipv4),
            Some(2)
        );
        assert_eq!(
            utp.local_address_for(AddressFamily::Ipv4),
            Some(replacement_address)
        );
        assert_ne!(first_address, replacement_address);
        let target = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap()
            .local_addr()
            .unwrap();
        assert!(matches!(
            utp.send_handle()
                .send_to(first_generation, b"stale", target)
                .await,
            Err(SessionUdpError::StaleGeneration {
                family: AddressFamily::Ipv4,
                requested: 1,
                current: Some(2),
            })
        ));
    }

    #[tokio::test]
    async fn two_family_receivers_share_one_route_and_join_independently() {
        let (mut service, mut transport) = service().await;
        let ipv6_socket = UdpSocket::bind((Ipv6Addr::LOCALHOST, 0)).await.unwrap();
        let ipv6_address = ipv6_socket.local_addr().unwrap();
        service.replace_socket(ipv6_socket).await.unwrap();
        let ipv4_address = service
            .local_address_for(AddressFamily::Ipv4)
            .expect("IPv4 receiver remains");
        assert_eq!(
            service.local_address_for(AddressFamily::Ipv6),
            Some(ipv6_address)
        );
        assert_eq!(service.snapshot().tasks, 2);
        assert_eq!(service.snapshot().task_high_water, 2);

        let ipv4_remote = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let ipv6_remote = UdpSocket::bind((Ipv6Addr::LOCALHOST, 0)).await.unwrap();
        ipv4_remote.send_to(b"v4", ipv4_address).await.unwrap();
        ipv6_remote.send_to(b"v6", ipv6_address).await.unwrap();
        let mut received = BTreeSet::new();
        for _ in 0..2 {
            let (bytes, source, family) = timeout(Duration::from_secs(1), transport.receive())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(AddressFamily::of(source.ip()), family);
            received.insert((family, bytes));
        }
        assert_eq!(
            received,
            BTreeSet::from([
                (AddressFamily::Ipv4, b"v4".to_vec()),
                (AddressFamily::Ipv6, b"v6".to_vec()),
            ])
        );

        service.remove_family(AddressFamily::Ipv6).await.unwrap();
        assert_eq!(service.snapshot().tasks, 1);
        assert_eq!(service.local_address_for(AddressFamily::Ipv6), None);
        assert!(matches!(
            transport
                .send_to(b"closed", ipv6_remote.local_addr().unwrap())
                .await,
            Err(SessionUdpError::MissingFamily(AddressFamily::Ipv6))
        ));
        drop(transport);
        let terminal = service.shutdown().await.unwrap();
        assert_eq!(terminal.tasks, 0);
        assert_eq!(terminal.queued, 0);
    }

    #[tokio::test]
    async fn stable_transport_moves_ingress_send_and_observation_to_replacement_socket() {
        let (mut service, mut transport) = service().await;
        let handle = transport.handle();
        let first_address = handle.local_address();
        let remote = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();

        remote.send_to(b"first", first_address).await.unwrap();
        assert_eq!(
            timeout(Duration::from_secs(1), transport.receive())
                .await
                .unwrap()
                .unwrap()
                .0,
            b"first"
        );

        let replacement = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let replacement_address = replacement.local_addr().unwrap();
        service.replace_socket(replacement).await.unwrap();
        assert_eq!(service.generation(), 2);
        assert_eq!(handle.generation(), 2);
        assert_eq!(transport.local_address(), replacement_address);
        assert_ne!(replacement_address, first_address);

        remote
            .send_to(b"second", replacement_address)
            .await
            .unwrap();
        assert_eq!(
            timeout(Duration::from_secs(1), transport.receive())
                .await
                .unwrap()
                .unwrap()
                .0,
            b"second"
        );
        transport
            .send_to(b"replacement-source", remote.local_addr().unwrap())
            .await
            .unwrap();
        let mut bytes = [0; 32];
        let (length, source) = timeout(Duration::from_secs(1), remote.recv_from(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&bytes[..length], b"replacement-source");
        assert_eq!(source, replacement_address);
        assert_eq!(service.snapshot().task_high_water, 2);

        drop(transport);
        let terminal = service.shutdown().await.unwrap();
        assert_eq!(terminal.tasks, 0);
        assert_eq!(terminal.queued, 0);
    }

    #[tokio::test]
    async fn oversize_input_is_bounded_to_the_dht_malformed_sentinel() {
        let (service, mut transport) = service().await;
        let remote = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        remote
            .send_to(&vec![7; MAX_DATAGRAM_SIZE * 2], transport.local_address())
            .await
            .unwrap();
        let (received, _, _) = timeout(Duration::from_secs(1), transport.receive())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received.len(), SESSION_UDP_DHT_RECEIVE_BYTES);
        drop(transport);
        service.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn oversize_utp_input_is_bounded_to_the_utp_malformed_sentinel() {
        let (mut service, dht) = service().await;
        let mut utp = service.take_utp_transport().unwrap();
        let remote = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let mut oversized = vec![7; SESSION_UDP_UTP_DATAGRAM_BYTES * 2];
        oversized[0] = UTP_VERSION;
        remote
            .send_to(&oversized, dht.local_address())
            .await
            .unwrap();
        let (_, received, _, _) = timeout(Duration::from_secs(1), utp.receive())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received.len(), SESSION_UDP_UTP_DATAGRAM_BYTES + 1);
        service.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn full_ingress_drops_new_work_and_tracks_the_bound() {
        let (sender, receiver) = mpsc::channel(SESSION_UDP_DHT_QUEUE);
        let (utp_sender, _utp_receiver) = mpsc::channel(SESSION_UDP_UTP_QUEUE);
        let stats = SessionUdpStats::default();
        for value in 0..SESSION_UDP_DHT_QUEUE {
            assert!(dispatch_dht(
                &sender,
                &stats,
                SessionUdpIngress::Datagram {
                    family: AddressFamily::Ipv4,
                    source: SocketAddr::from((Ipv4Addr::LOCALHOST, 1)),
                    bytes: vec![u8::try_from(value).unwrap()],
                },
            ));
        }
        assert!(dispatch_dht(
            &sender,
            &stats,
            SessionUdpIngress::Datagram {
                family: AddressFamily::Ipv4,
                source: SocketAddr::from((Ipv4Addr::LOCALHOST, 1)),
                bytes: vec![255],
            },
        ));
        let snapshot = stats.snapshot(
            &sender,
            &utp_sender,
            Ipv4FragmentationProtectionStatus::UnsupportedPlatform,
        );
        assert_eq!(snapshot.queued, SESSION_UDP_DHT_QUEUE);
        assert_eq!(snapshot.queue_high_water, SESSION_UDP_DHT_QUEUE);
        assert_eq!(snapshot.datagrams_dropped, 1);
        drop(receiver);
        assert!(!dispatch_dht(
            &sender,
            &stats,
            SessionUdpIngress::Datagram {
                family: AddressFamily::Ipv4,
                source: SocketAddr::from((Ipv4Addr::LOCALHOST, 1)),
                bytes: Vec::new(),
            },
        ));
    }

    #[tokio::test]
    async fn full_utp_route_does_not_block_dht_ingress() {
        let (dht_sender, mut dht_receiver) = mpsc::channel(SESSION_UDP_DHT_QUEUE);
        let (utp_sender, _utp_receiver) = mpsc::channel(SESSION_UDP_UTP_QUEUE);
        let stats = SessionUdpStats::default();
        let source = SocketAddr::from((Ipv4Addr::LOCALHOST, 1));
        for _ in 0..SESSION_UDP_UTP_QUEUE {
            assert!(dispatch_utp(
                &utp_sender,
                &stats,
                SessionUtpIngress::Datagram {
                    generation: 1,
                    family: AddressFamily::Ipv4,
                    source,
                    bytes: vec![UTP_VERSION; UTP_HEADER_SIZE],
                },
            ));
        }
        assert!(dispatch_utp(
            &utp_sender,
            &stats,
            SessionUtpIngress::Datagram {
                generation: 1,
                family: AddressFamily::Ipv4,
                source,
                bytes: vec![UTP_VERSION; UTP_HEADER_SIZE],
            },
        ));
        assert!(dispatch_dht(
            &dht_sender,
            &stats,
            SessionUdpIngress::Datagram {
                family: AddressFamily::Ipv4,
                source,
                bytes: b"dht".to_vec(),
            },
        ));

        assert!(matches!(
            dht_receiver.recv().await,
            Some(SessionUdpIngress::Datagram { bytes, .. }) if bytes == b"dht"
        ));
        let snapshot = stats.snapshot(
            &dht_sender,
            &utp_sender,
            Ipv4FragmentationProtectionStatus::UnsupportedPlatform,
        );
        assert_eq!(snapshot.utp_queued, SESSION_UDP_UTP_QUEUE);
        assert_eq!(snapshot.utp_queue_high_water, SESSION_UDP_UTP_QUEUE);
        assert_eq!(snapshot.utp_datagrams_dropped, 1);
        assert_eq!(snapshot.dht_datagrams_dropped, 0);
        assert_eq!(snapshot.datagrams_dropped, 1);
    }
}
