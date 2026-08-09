//! Single-receiver, bounded session UDP transport ownership.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use rstorrent_protocol::dht::MAX_DATAGRAM_SIZE;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::network::AddressFamily;

pub const SESSION_UDP_DHT_QUEUE: usize = 64;
const SESSION_UDP_RECEIVE_BYTES: usize = MAX_DATAGRAM_SIZE + 1;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SessionUdpSnapshot {
    pub tasks: usize,
    pub task_high_water: usize,
    pub queued: usize,
    pub queue_high_water: usize,
    pub datagrams_received: u64,
    pub datagram_bytes_received: u64,
    pub datagrams_dropped: u64,
}

#[derive(Debug)]
pub enum SessionUdpError {
    Io(io::Error),
    MissingFamily(AddressFamily),
    TaskJoin(String),
}

impl fmt::Display for SessionUdpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "session UDP I/O failed: {error}"),
            Self::MissingFamily(family) => {
                write!(formatter, "session UDP {family} family is unavailable")
            }
            Self::TaskJoin(error) => write!(formatter, "session UDP task join failed: {error}"),
        }
    }
}

impl Error for SessionUdpError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::MissingFamily(_) | Self::TaskJoin(_) => None,
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
pub struct SessionUdpTransport {
    current: Arc<RwLock<SessionUdpCurrent>>,
    ingress: mpsc::Receiver<SessionUdpIngress>,
    ingress_sender: mpsc::Sender<SessionUdpIngress>,
    stats: Arc<SessionUdpStats>,
}

#[derive(Debug)]
struct SessionUdpCurrent {
    families: BTreeMap<AddressFamily, SessionUdpFamilyCurrent>,
}

#[derive(Debug)]
struct SessionUdpFamilyCurrent {
    generation: u64,
    socket: Arc<UdpSocket>,
    local_address: SocketAddr,
}

#[derive(Clone, Debug)]
pub struct SessionUdpHandle {
    current: Arc<RwLock<SessionUdpCurrent>>,
    ingress_sender: mpsc::Sender<SessionUdpIngress>,
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
        self.stats.snapshot(&self.ingress_sender)
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
        let socket = self
            .current_guard()
            .families
            .get(&family)
            .map(|entry| entry.socket.clone())
            .ok_or(SessionUdpError::MissingFamily(family))?;
        socket
            .send_to(bytes, target)
            .await
            .map_err(SessionUdpError::Io)
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
    datagrams_received: AtomicU64,
    datagram_bytes_received: AtomicU64,
    datagrams_dropped: AtomicU64,
}

impl SessionUdpStats {
    fn snapshot(&self, sender: &mpsc::Sender<SessionUdpIngress>) -> SessionUdpSnapshot {
        SessionUdpSnapshot {
            tasks: self.tasks.load(Ordering::Relaxed),
            task_high_water: self.task_high_water.load(Ordering::Relaxed),
            queued: if sender.is_closed() {
                0
            } else {
                sender.max_capacity().saturating_sub(sender.capacity())
            },
            queue_high_water: self.queue_high_water.load(Ordering::Relaxed),
            datagrams_received: self.datagrams_received.load(Ordering::Relaxed),
            datagram_bytes_received: self.datagram_bytes_received.load(Ordering::Relaxed),
            datagrams_dropped: self.datagrams_dropped.load(Ordering::Relaxed),
        }
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
        let stats = Arc::new(SessionUdpStats::default());
        let generation = 1;
        let current = Arc::new(RwLock::new(SessionUdpCurrent {
            families: BTreeMap::from([(
                family,
                SessionUdpFamilyCurrent {
                    generation,
                    socket: socket.clone(),
                    local_address,
                },
            )]),
        }));
        let active = BTreeMap::from([(
            family,
            start_generation(family, socket, ingress_sender.clone(), stats.clone()),
        )]);
        let handle = SessionUdpHandle {
            current: current.clone(),
            ingress_sender: ingress_sender.clone(),
            stats: stats.clone(),
        };
        Ok((
            Self {
                handle,
                active,
                next_generation: 2,
                ingress_sender: ingress_sender.clone(),
                stats: stats.clone(),
            },
            SessionUdpTransport {
                current,
                ingress,
                ingress_sender,
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
        self.stats.snapshot(&self.ingress_sender)
    }

    pub async fn replace_socket(&mut self, socket: UdpSocket) -> Result<(), SessionUdpError> {
        let local_address = socket.local_addr().map_err(SessionUdpError::Io)?;
        let family = AddressFamily::of(local_address.ip());
        let generation = self.next_generation;
        self.next_generation = self.next_generation.checked_add(1).ok_or_else(|| {
            SessionUdpError::Io(io::Error::other("session UDP generation exhausted"))
        })?;
        let socket = Arc::new(socket);
        let candidate = start_generation(
            family,
            socket.clone(),
            self.ingress_sender.clone(),
            self.stats.clone(),
        );
        {
            let mut current = self.current_guard();
            current.families.insert(
                family,
                SessionUdpFamilyCurrent {
                    generation,
                    socket,
                    local_address,
                },
            );
        }
        let previous = self.active.insert(family, candidate);
        match previous {
            Some(previous) => previous.shutdown().await,
            None => Ok(()),
        }
    }

    pub async fn remove_family(&mut self, family: AddressFamily) -> Result<(), SessionUdpError> {
        self.current_guard().families.remove(&family);
        match self.active.remove(&family) {
            Some(previous) => previous.shutdown().await,
            None => Ok(()),
        }
    }

    pub async fn shutdown(mut self) -> Result<SessionUdpSnapshot, SessionUdpError> {
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
}

impl Drop for SessionUdpService {
    fn drop(&mut self) {
        self.active.clear();
    }
}

fn start_generation(
    family: AddressFamily,
    socket: Arc<UdpSocket>,
    ingress_sender: mpsc::Sender<SessionUdpIngress>,
    stats: Arc<SessionUdpStats>,
) -> SessionUdpGeneration {
    let cancellation = CancellationToken::new();
    let tasks = stats.tasks.fetch_add(1, Ordering::AcqRel).saturating_add(1);
    stats.task_high_water.fetch_max(tasks, Ordering::AcqRel);
    let task = tokio::spawn(run_receive_loop(
        family,
        socket,
        ingress_sender,
        stats,
        cancellation.clone(),
    ));
    SessionUdpGeneration {
        cancellation,
        task: Some(task),
    }
}

async fn run_receive_loop(
    family: AddressFamily,
    socket: Arc<UdpSocket>,
    ingress: mpsc::Sender<SessionUdpIngress>,
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
                        if !dispatch_dht(
                            &ingress,
                            &stats,
                            SessionUdpIngress::Datagram {
                                family,
                                source,
                                bytes: bytes[..length].to_vec(),
                            },
                        ) {
                            break Ok(());
                        }
                    }
                    Err(error) => {
                        let detail = error.to_string();
                        let _ = dispatch_dht(
                            &ingress,
                            &stats,
                            SessionUdpIngress::Failed { family, detail },
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
            stats.record_drop();
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
        assert_eq!(received.len(), SESSION_UDP_RECEIVE_BYTES);
        drop(transport);
        service.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn full_ingress_drops_new_work_and_tracks_the_bound() {
        let (sender, receiver) = mpsc::channel(SESSION_UDP_DHT_QUEUE);
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
        let snapshot = stats.snapshot(&sender);
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
}
