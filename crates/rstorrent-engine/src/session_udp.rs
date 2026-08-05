//! Single-receiver, bounded session UDP transport ownership.

use std::error::Error;
use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use rstorrent_protocol::dht::MAX_DATAGRAM_SIZE;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

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
    TaskJoin(String),
}

impl fmt::Display for SessionUdpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "session UDP I/O failed: {error}"),
            Self::TaskJoin(error) => write!(formatter, "session UDP task join failed: {error}"),
        }
    }
}

impl Error for SessionUdpError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::TaskJoin(_) => None,
        }
    }
}

#[derive(Debug)]
pub(crate) enum SessionUdpIngress {
    Datagram { source: SocketAddr, bytes: Vec<u8> },
    Failed(String),
}

#[derive(Debug)]
pub struct SessionUdpTransport {
    socket: Arc<UdpSocket>,
    local_address: SocketAddr,
    ingress: mpsc::Receiver<SessionUdpIngress>,
}

impl SessionUdpTransport {
    pub fn local_address(&self) -> SocketAddr {
        self.local_address
    }

    pub(crate) async fn receive(&mut self) -> Result<(Vec<u8>, SocketAddr), SessionUdpError> {
        match self.ingress.recv().await {
            Some(SessionUdpIngress::Datagram { source, bytes }) => Ok((bytes, source)),
            Some(SessionUdpIngress::Failed(error)) => {
                Err(SessionUdpError::Io(io::Error::other(error)))
            }
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
        self.socket
            .send_to(bytes, target)
            .await
            .map_err(SessionUdpError::Io)
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
    local_address: SocketAddr,
    cancellation: CancellationToken,
    task: Option<JoinHandle<Result<(), SessionUdpError>>>,
    ingress_sender: mpsc::Sender<SessionUdpIngress>,
    stats: Arc<SessionUdpStats>,
}

impl SessionUdpService {
    pub fn start(socket: UdpSocket) -> Result<(Self, SessionUdpTransport), SessionUdpError> {
        let local_address = socket.local_addr().map_err(SessionUdpError::Io)?;
        let socket = Arc::new(socket);
        let (ingress_sender, ingress) = mpsc::channel(SESSION_UDP_DHT_QUEUE);
        let stats = Arc::new(SessionUdpStats::default());
        stats.tasks.store(1, Ordering::Relaxed);
        stats.task_high_water.store(1, Ordering::Relaxed);
        let cancellation = CancellationToken::new();
        let task = tokio::spawn(run_receive_loop(
            socket.clone(),
            ingress_sender.clone(),
            stats.clone(),
            cancellation.clone(),
        ));
        Ok((
            Self {
                local_address,
                cancellation,
                task: Some(task),
                ingress_sender,
                stats,
            },
            SessionUdpTransport {
                socket,
                local_address,
                ingress,
            },
        ))
    }

    pub fn local_address(&self) -> SocketAddr {
        self.local_address
    }

    pub fn snapshot(&self) -> SessionUdpSnapshot {
        self.stats.snapshot(&self.ingress_sender)
    }

    pub async fn shutdown(mut self) -> Result<SessionUdpSnapshot, SessionUdpError> {
        self.cancellation.cancel();
        self.task
            .take()
            .expect("session UDP task exists before shutdown")
            .await
            .map_err(|error| SessionUdpError::TaskJoin(error.to_string()))??;
        Ok(self.snapshot())
    }
}

impl Drop for SessionUdpService {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn run_receive_loop(
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
                        if !dispatch_dht(
                            &ingress,
                            &stats,
                            SessionUdpIngress::Datagram {
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
                            SessionUdpIngress::Failed(detail),
                        );
                        break Err(SessionUdpError::Io(error));
                    }
                }
            }
        }
    };
    stats.tasks.store(0, Ordering::Relaxed);
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
    use std::net::Ipv4Addr;
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
        let (received, source) = timeout(Duration::from_secs(1), transport.receive())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received, b"d1:q4:pinge");
        assert_eq!(source, remote_address);

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
    async fn oversize_input_is_bounded_to_the_dht_malformed_sentinel() {
        let (service, mut transport) = service().await;
        let remote = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        remote
            .send_to(&vec![7; MAX_DATAGRAM_SIZE * 2], transport.local_address())
            .await
            .unwrap();
        let (received, _) = timeout(Duration::from_secs(1), transport.receive())
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
                    source: SocketAddr::from((Ipv4Addr::LOCALHOST, 1)),
                    bytes: vec![u8::try_from(value).unwrap()],
                },
            ));
        }
        assert!(dispatch_dht(
            &sender,
            &stats,
            SessionUdpIngress::Datagram {
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
                source: SocketAddr::from((Ipv4Addr::LOCALHOST, 1)),
                bytes: Vec::new(),
            },
        ));
    }
}
