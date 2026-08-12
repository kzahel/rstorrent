//! Bounded Tokio ownership for the runtime-independent uTP transport state.

use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::future::pending;
use std::io;
use std::net::SocketAddr;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use futures_util::FutureExt;
use rstorrent_protocol::utp::{
    ConnectionError, ConnectionPhase, DatagramSendResult, IPV4_UDP_PAYLOAD_CEILING,
    IPV4_UDP_PAYLOAD_FLOOR, IncomingDisposition, MAX_UNSENT_BYTES, PacketType, PathMtuState,
    SendError, SequenceNumber, TimestampMicros, TransportError, TransportState, UTP_HEADER_SIZE,
    UtpCodecError, decode_packet,
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{Notify, mpsc, oneshot, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{Instant, sleep, sleep_until};
use tokio_util::sync::{CancellationToken, PollSender};

use crate::network::AddressFamily;
use crate::session_udp::{
    SessionUdpError, SessionUdpGenerations, SessionUdpService, SessionUtpSendHandle,
    SessionUtpTransport,
};
use crate::udp_fragmentation::Ipv4FragmentationProtectionStatus;

pub const MAX_UTP_CONNECTIONS: usize = 64;
pub const MAX_INCOMING_UTP_HALF_OPEN: usize = 16;
pub const UTP_INCOMING_STREAM_QUEUE: usize = 16;
pub const UTP_CONNECTION_DATAGRAM_QUEUE: usize = 64;
pub const MAX_UTP_APPLICATION_WRITE_BYTES: usize = 16 * 1024;
pub const UTP_RUNTIME_DATAGRAM_BYTES: usize = 548;
const UTP_SERVICE_COMMAND_QUEUE: usize = 16;
const UTP_APPLICATION_WRITE_QUEUE: usize = 1;
const UTP_APPLICATION_CONTROL_QUEUE: usize = 1;
const UTP_STREAM_EVENT_QUEUE: usize = 64;
const UTP_DELIVERY_CHUNK_BYTES: usize = 16 * 1024;
const UTP_APPLICATION_COALESCE_BYTES: usize = UTP_RUNTIME_DATAGRAM_BYTES - UTP_HEADER_SIZE;
const UTP_APPLICATION_COALESCE_DELAY: Duration = Duration::from_millis(5);
const UTP_ENTROPY_DRAWS: usize = 16;
const MAX_EMISSIONS_PER_TURN: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UtpRuntimeConfig {
    floor_datagram_bytes: usize,
    ceiling_datagram_bytes: usize,
}

impl UtpRuntimeConfig {
    #[must_use]
    pub const fn diagnostic_ipv4_path_mtu() -> Self {
        Self {
            floor_datagram_bytes: IPV4_UDP_PAYLOAD_FLOOR,
            ceiling_datagram_bytes: IPV4_UDP_PAYLOAD_CEILING,
        }
    }

    #[must_use]
    pub const fn fixed_ipv4() -> Self {
        Self {
            floor_datagram_bytes: UTP_RUNTIME_DATAGRAM_BYTES,
            ceiling_datagram_bytes: UTP_RUNTIME_DATAGRAM_BYTES,
        }
    }

    fn validate(self) -> Result<Self, UtpRuntimeError> {
        PathMtuState::new(self.floor_datagram_bytes, self.ceiling_datagram_bytes)
            .map_err(TransportError::from)?;
        Ok(self)
    }

    const fn profile(self) -> UtpPathMtuProfile {
        if self.floor_datagram_bytes == self.ceiling_datagram_bytes {
            UtpPathMtuProfile::Fixed548
        } else {
            UtpPathMtuProfile::DynamicIpv4
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum UtpPathMtuProfile {
    #[default]
    Fixed548,
    DynamicIpv4,
}

impl UtpPathMtuProfile {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fixed548 => "fixed_548",
            Self::DynamicIpv4 => "dynamic_ipv4",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UtpServiceSnapshot {
    pub path_mtu_profile: UtpPathMtuProfile,
    pub active_connections: usize,
    pub connections_started: u64,
    pub connection_high_water: usize,
    pub incoming_half_open: usize,
    pub incoming_half_open_high_water: usize,
    pub incoming_stream_queue_high_water: usize,
    pub connection_datagram_queue_high_water: usize,
    pub malformed_datagrams: u64,
    pub unknown_connection_datagrams: u64,
    pub stale_generation_datagrams: u64,
    pub connection_datagrams_dropped: u64,
    pub datagrams_sent: u64,
    pub datagram_bytes_sent: u64,
    pub data_datagrams_sent: u64,
    pub state_datagrams_sent: u64,
    pub retransmission_datagrams_sent: u64,
    pub retransmission_bytes_sent: u64,
    pub retransmission_queue_high_water: usize,
    pub in_flight_packet_high_water: usize,
    pub in_flight_byte_high_water: usize,
    pub congestion_control_acknowledgements_high_water: u64,
    pub congestion_control_acknowledged_bytes_high_water: u64,
    pub congestion_limited_acknowledgements_high_water: u64,
    pub sender_underfilled_acknowledgements_high_water: u64,
    pub remote_window_limited_acknowledgements_high_water: u64,
    pub window_growth_acknowledgements_high_water: u64,
    pub pending_ack_packet_high_water: usize,
    pub loss_reduction_high_water: u64,
    pub timeout_collapse_high_water: u64,
    pub delivered_byte_high_water: usize,
    pub unsent_byte_high_water: usize,
    pub sent_byte_high_water: usize,
    pub application_coalesce_byte_high_water: usize,
    pub smoothed_rtt_min_micros: Option<u64>,
    pub smoothed_rtt_max_micros: Option<u64>,
    pub effective_rto_min_micros: Option<u64>,
    pub effective_rto_max_micros: Option<u64>,
    pub base_delay_min_micros: Option<u64>,
    pub base_delay_max_micros: Option<u64>,
    pub queue_delay_min_micros: Option<u64>,
    pub queue_delay_max_micros: Option<u64>,
    pub congestion_window_min_bytes: Option<usize>,
    pub congestion_window_max_bytes: Option<usize>,
    pub advertised_receive_window_min_bytes: Option<usize>,
    pub advertised_receive_window_max_bytes: Option<usize>,
    pub selected_mtu_min_bytes: Option<usize>,
    pub selected_mtu_max_bytes: Option<usize>,
    pub mtu_candidate_min_bytes: Option<usize>,
    pub mtu_candidate_max_bytes: Option<usize>,
    pub mtu_probes_started_high_water: u64,
    pub mtu_probes_acknowledged_high_water: u64,
    pub mtu_probes_failed_high_water: u64,
    pub mtu_revalidations_started_high_water: u64,
    pub mtu_revalidations_acknowledged_high_water: u64,
    pub mtu_revalidations_failed_high_water: u64,
    pub mtu_downward_recoveries_high_water: u64,
    pub mtu_probe_datagrams_sent: u64,
    pub mtu_fragmentable_retry_datagrams_sent: u64,
    pub retry_exhausted_connections: u64,
    pub worker_panics: u64,
}

#[derive(Debug)]
pub enum UtpRuntimeError {
    Udp(SessionUdpError),
    Protocol(TransportError),
    Codec(UtpCodecError),
    Entropy(String),
    UnsupportedFamily(AddressFamily),
    DynamicPathMtuUnavailable(Ipv4FragmentationProtectionStatus),
    ConnectionLimit,
    ConnectionIdCollision,
    ConnectTimedOut(Duration),
    ServiceStopped,
    TaskJoin(String),
}

impl fmt::Display for UtpRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Udp(error) => write!(formatter, "uTP UDP transport: {error}"),
            Self::Protocol(error) => write!(formatter, "uTP protocol: {error}"),
            Self::Codec(error) => write!(formatter, "uTP codec: {error}"),
            Self::Entropy(detail) => write!(formatter, "uTP entropy: {detail}"),
            Self::UnsupportedFamily(family) => {
                write!(formatter, "uTP runtime does not yet support {family}")
            }
            Self::DynamicPathMtuUnavailable(status) => write!(
                formatter,
                "dynamic IPv4 uTP path MTU is unavailable: fragmentation protection is {status:?}"
            ),
            Self::ConnectionLimit => formatter.write_str("uTP connection limit reached"),
            Self::ConnectionIdCollision => {
                formatter.write_str("uTP connection ID entropy collision limit reached")
            }
            Self::ConnectTimedOut(timeout) => {
                write!(formatter, "uTP connect timed out after {timeout:?}")
            }
            Self::ServiceStopped => formatter.write_str("uTP service stopped"),
            Self::TaskJoin(detail) => write!(formatter, "uTP task join failed: {detail}"),
        }
    }
}

impl Error for UtpRuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Udp(error) => Some(error),
            Self::Protocol(error) => Some(error),
            Self::Codec(error) => Some(error),
            Self::Entropy(_)
            | Self::UnsupportedFamily(_)
            | Self::DynamicPathMtuUnavailable(_)
            | Self::ConnectionLimit
            | Self::ConnectionIdCollision
            | Self::ConnectTimedOut(_)
            | Self::ServiceStopped
            | Self::TaskJoin(_) => None,
        }
    }
}

impl From<SessionUdpError> for UtpRuntimeError {
    fn from(error: SessionUdpError) -> Self {
        Self::Udp(error)
    }
}

impl From<TransportError> for UtpRuntimeError {
    fn from(error: TransportError) -> Self {
        Self::Protocol(error)
    }
}

impl From<UtpCodecError> for UtpRuntimeError {
    fn from(error: UtpCodecError) -> Self {
        Self::Codec(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct UtpConnectionKey {
    family: AddressFamily,
    generation: u64,
    remote: SocketAddr,
    receive_connection_id: u16,
}

#[derive(Debug)]
struct UtpRoute {
    ingress: mpsc::Sender<Vec<u8>>,
    cancellation: CancellationToken,
}

type WorkerPanic = Box<dyn std::any::Any + Send>;
type WorkerJoinOutput = (UtpConnectionKey, Result<WorkerReport, WorkerPanic>);
type WorkerSet = JoinSet<WorkerJoinOutput>;
type WorkerJoin = Option<Result<WorkerJoinOutput, tokio::task::JoinError>>;

#[derive(Debug)]
enum UtpServiceCommand {
    Connect {
        target: SocketAddr,
        response: oneshot::Sender<Result<UtpStream, UtpRuntimeError>>,
        cancellation: CancellationToken,
        termination: oneshot::Sender<()>,
    },
}

#[derive(Clone, Debug)]
pub struct UtpHandle {
    commands: mpsc::Sender<UtpServiceCommand>,
    stats: Arc<UtpStats>,
}

impl UtpHandle {
    pub async fn connect(&self, target: SocketAddr) -> Result<UtpStream, UtpRuntimeError> {
        self.connect_inner(target, None).await
    }

    pub async fn connect_with_timeout(
        &self,
        target: SocketAddr,
        timeout: Duration,
    ) -> Result<UtpStream, UtpRuntimeError> {
        self.connect_inner(target, Some(timeout)).await
    }

    async fn connect_inner(
        &self,
        target: SocketAddr,
        connect_timeout: Option<Duration>,
    ) -> Result<UtpStream, UtpRuntimeError> {
        let (response, result) = oneshot::channel();
        let (termination, terminated) = oneshot::channel();
        let cancellation = CancellationToken::new();
        let mut cancellation_guard = UtpConnectCancellation::new(cancellation.clone());
        self.commands
            .send(UtpServiceCommand::Connect {
                target,
                response,
                cancellation,
                termination,
            })
            .await
            .map_err(|_| UtpRuntimeError::ServiceStopped)?;
        let resolved = match connect_timeout {
            Some(connect_timeout) => {
                tokio::select! {
                    biased;
                    result = result => Some(result),
                    _ = sleep(connect_timeout) => None,
                }
            }
            None => Some(result.await),
        };
        match resolved {
            Some(Ok(Ok(stream))) => {
                cancellation_guard.disarm();
                Ok(stream)
            }
            Some(Ok(Err(error))) => {
                let _ = terminated.await;
                Err(error)
            }
            Some(Err(_)) => {
                let _ = terminated.await;
                Err(UtpRuntimeError::ServiceStopped)
            }
            None => {
                cancellation_guard.cancel();
                let _ = terminated.await;
                Err(UtpRuntimeError::ConnectTimedOut(
                    connect_timeout.expect("timeout branch has a duration"),
                ))
            }
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> UtpServiceSnapshot {
        self.stats.snapshot()
    }
}

#[derive(Debug)]
struct UtpConnectCancellation(Option<CancellationToken>);

impl UtpConnectCancellation {
    fn new(cancellation: CancellationToken) -> Self {
        Self(Some(cancellation))
    }

    fn cancel(&mut self) {
        if let Some(cancellation) = self.0.take() {
            cancellation.cancel();
        }
    }

    fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for UtpConnectCancellation {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[derive(Debug)]
pub struct UtpService {
    handle: UtpHandle,
    incoming: mpsc::Receiver<UtpStream>,
    cancellation: CancellationToken,
    task: Option<JoinHandle<Result<UtpServiceSnapshot, UtpRuntimeError>>>,
}

impl UtpService {
    pub fn start(udp: &mut SessionUdpService) -> Result<Self, UtpRuntimeError> {
        let config = product_runtime_config(udp.ipv4_fragmentation_protection_status());
        Self::start_with_config(udp, config)
    }

    pub fn start_diagnostic(
        udp: &mut SessionUdpService,
        config: UtpRuntimeConfig,
    ) -> Result<Self, UtpRuntimeError> {
        Self::start_with_config(udp, config)
    }

    fn start_with_config(
        udp: &mut SessionUdpService,
        config: UtpRuntimeConfig,
    ) -> Result<Self, UtpRuntimeError> {
        let config = config.validate()?;
        let profile = config.profile();
        let capability = udp.ipv4_fragmentation_protection_status();
        if profile == UtpPathMtuProfile::DynamicIpv4
            && capability != Ipv4FragmentationProtectionStatus::Verified
        {
            return Err(UtpRuntimeError::DynamicPathMtuUnavailable(capability));
        }
        let transport = udp.take_utp_transport()?;
        let (commands, command_ingress) = mpsc::channel(UTP_SERVICE_COMMAND_QUEUE);
        let (incoming_sender, incoming) = mpsc::channel(UTP_INCOMING_STREAM_QUEUE);
        let stats = Arc::new(UtpStats::with_profile(profile));
        let cancellation = CancellationToken::new();
        let task = tokio::spawn(run_service(
            transport,
            command_ingress,
            incoming_sender,
            stats.clone(),
            cancellation.clone(),
            config,
        ));
        Ok(Self {
            handle: UtpHandle { commands, stats },
            incoming,
            cancellation,
            task: Some(task),
        })
    }

    #[must_use]
    pub fn handle(&self) -> UtpHandle {
        self.handle.clone()
    }

    pub async fn accept(&mut self) -> Option<UtpStream> {
        self.incoming.recv().await
    }

    #[must_use]
    pub fn snapshot(&self) -> UtpServiceSnapshot {
        self.handle.snapshot()
    }

    pub async fn shutdown(mut self) -> Result<UtpServiceSnapshot, UtpRuntimeError> {
        self.cancellation.cancel();
        self.task
            .take()
            .expect("uTP service task exists before shutdown")
            .await
            .map_err(|error| UtpRuntimeError::TaskJoin(error.to_string()))?
    }
}

const fn product_runtime_config(capability: Ipv4FragmentationProtectionStatus) -> UtpRuntimeConfig {
    match capability {
        Ipv4FragmentationProtectionStatus::Verified => UtpRuntimeConfig::diagnostic_ipv4_path_mtu(),
        Ipv4FragmentationProtectionStatus::VerificationFailed
        | Ipv4FragmentationProtectionStatus::UnsupportedPlatform => UtpRuntimeConfig::fixed_ipv4(),
    }
}

impl Drop for UtpService {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

#[derive(Debug)]
enum UtpStreamControl {
    Flush(oneshot::Sender<io::Result<()>>),
    Shutdown(oneshot::Sender<io::Result<()>>),
}

#[derive(Debug)]
enum UtpStreamEvent {
    Data(Vec<u8>),
    Eof,
}

#[derive(Clone, Debug)]
struct UtpStreamTerminal {
    kind: io::ErrorKind,
    detail: String,
}

#[derive(Debug, Default)]
struct UtpConsumption {
    bytes: AtomicUsize,
    notification: Notify,
}

impl UtpConsumption {
    fn record(&self, bytes: usize) {
        let _ = self
            .bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                Some(current.saturating_add(bytes))
            });
        self.notification.notify_one();
    }

    fn take(&self) -> usize {
        self.bytes.swap(0, Ordering::AcqRel)
    }
}

#[derive(Debug)]
pub struct UtpStream {
    local_address: SocketAddr,
    remote_address: SocketAddr,
    write_sender: PollSender<Vec<u8>>,
    try_write_sender: mpsc::Sender<Vec<u8>>,
    control_sender: PollSender<UtpStreamControl>,
    events: mpsc::Receiver<UtpStreamEvent>,
    current_read: Option<(Vec<u8>, usize)>,
    consumption: Arc<UtpConsumption>,
    read_notification: Arc<Notify>,
    cancellation: CancellationToken,
    terminal: Arc<Mutex<Option<UtpStreamTerminal>>>,
    flush_response: Option<oneshot::Receiver<io::Result<()>>>,
    shutdown_response: Option<oneshot::Receiver<io::Result<()>>>,
    eof: bool,
    write_closed: bool,
}

impl UtpStream {
    #[must_use]
    pub const fn local_addr(&self) -> SocketAddr {
        self.local_address
    }

    #[must_use]
    pub const fn peer_addr(&self) -> SocketAddr {
        self.remote_address
    }

    fn terminal_error(&self) -> io::Error {
        self.terminal
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .map_or_else(
                || io::Error::new(io::ErrorKind::BrokenPipe, "uTP worker stopped"),
                |terminal| io::Error::new(terminal.kind, terminal.detail.clone()),
            )
    }

    pub(crate) fn try_read(&mut self, destination: &mut [u8]) -> io::Result<usize> {
        if self.eof || destination.is_empty() {
            return Ok(0);
        }
        loop {
            if let Some((bytes, offset)) = &mut self.current_read {
                let copied = destination.len().min(bytes.len().saturating_sub(*offset));
                destination[..copied].copy_from_slice(&bytes[*offset..*offset + copied]);
                *offset += copied;
                self.consumption.record(copied);
                if *offset == bytes.len() {
                    self.current_read = None;
                }
                return Ok(copied);
            }
            match self.events.try_recv() {
                Ok(UtpStreamEvent::Data(bytes)) => self.current_read = Some((bytes, 0)),
                Ok(UtpStreamEvent::Eof) => {
                    self.eof = true;
                    return Ok(0);
                }
                Err(mpsc::error::TryRecvError::Empty) => {
                    return Err(io::Error::from(io::ErrorKind::WouldBlock));
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    let error = self.terminal_error();
                    if error.kind() == io::ErrorKind::UnexpectedEof {
                        self.eof = true;
                        return Ok(0);
                    }
                    return Err(error);
                }
            }
        }
    }

    pub(crate) fn try_write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.write_closed {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "uTP write half is closed",
            ));
        }
        if bytes.is_empty() {
            return Ok(0);
        }
        let length = bytes.len().min(MAX_UTP_APPLICATION_WRITE_BYTES);
        self.try_write_sender
            .try_send(bytes[..length].to_vec())
            .map_err(|error| match error {
                TrySendError::Full(_) => io::Error::from(io::ErrorKind::WouldBlock),
                TrySendError::Closed(_) => self.terminal_error(),
            })?;
        Ok(length)
    }

    pub(crate) async fn readable(&self) -> io::Result<()> {
        loop {
            if self.read_is_ready() {
                return Ok(());
            }
            let notified = self.read_notification.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.read_is_ready() {
                return Ok(());
            }
            notified.await;
        }
    }

    pub(crate) async fn writable(&self) -> io::Result<()> {
        let permit = self
            .try_write_sender
            .reserve()
            .await
            .map_err(|_| self.terminal_error())?;
        drop(permit);
        Ok(())
    }

    fn read_is_ready(&self) -> bool {
        self.current_read.is_some()
            || !self.events.is_empty()
            || self.eof
            || self
                .terminal
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_some()
    }
}

impl AsyncRead for UtpStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        destination: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let stream = self.get_mut();
        if stream.eof || destination.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        loop {
            if let Some((bytes, offset)) = &mut stream.current_read {
                let copied = destination
                    .remaining()
                    .min(bytes.len().saturating_sub(*offset));
                destination.put_slice(&bytes[*offset..*offset + copied]);
                *offset += copied;
                stream.consumption.record(copied);
                if *offset == bytes.len() {
                    stream.current_read = None;
                }
                return Poll::Ready(Ok(()));
            }
            match stream.events.poll_recv(cx) {
                Poll::Ready(Some(UtpStreamEvent::Data(bytes))) => {
                    stream.current_read = Some((bytes, 0));
                }
                Poll::Ready(Some(UtpStreamEvent::Eof)) => {
                    stream.eof = true;
                    return Poll::Ready(Ok(()));
                }
                Poll::Ready(None) => {
                    let error = stream.terminal_error();
                    if error.kind() == io::ErrorKind::UnexpectedEof {
                        stream.eof = true;
                        return Poll::Ready(Ok(()));
                    }
                    return Poll::Ready(Err(error));
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl AsyncWrite for UtpStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let stream = self.get_mut();
        if stream.write_closed {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "uTP write half is closed",
            )));
        }
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        match stream.write_sender.poll_reserve(cx) {
            Poll::Ready(Ok(())) => {
                let length = bytes.len().min(MAX_UTP_APPLICATION_WRITE_BYTES);
                stream
                    .write_sender
                    .send_item(bytes[..length].to_vec())
                    .map_err(|_| stream.terminal_error())?;
                Poll::Ready(Ok(length))
            }
            Poll::Ready(Err(_)) => Poll::Ready(Err(stream.terminal_error())),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let stream = self.get_mut();
        if stream.flush_response.is_none() {
            match stream.control_sender.poll_reserve(cx) {
                Poll::Ready(Ok(())) => {
                    let (response, result) = oneshot::channel();
                    stream
                        .control_sender
                        .send_item(UtpStreamControl::Flush(response))
                        .map_err(|_| stream.terminal_error())?;
                    stream.flush_response = Some(result);
                }
                Poll::Ready(Err(_)) => return Poll::Ready(Err(stream.terminal_error())),
                Poll::Pending => return Poll::Pending,
            }
        }
        match Pin::new(
            stream
                .flush_response
                .as_mut()
                .expect("flush response was installed"),
        )
        .poll(cx)
        {
            Poll::Ready(Ok(result)) => {
                stream.flush_response = None;
                Poll::Ready(result)
            }
            Poll::Ready(Err(_)) => Poll::Ready(Err(stream.terminal_error())),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let stream = self.get_mut();
        stream.write_closed = true;
        if stream.shutdown_response.is_none() {
            match stream.control_sender.poll_reserve(cx) {
                Poll::Ready(Ok(())) => {
                    let (response, result) = oneshot::channel();
                    stream
                        .control_sender
                        .send_item(UtpStreamControl::Shutdown(response))
                        .map_err(|_| stream.terminal_error())?;
                    stream.shutdown_response = Some(result);
                }
                Poll::Ready(Err(_)) => return Poll::Ready(Err(stream.terminal_error())),
                Poll::Pending => return Poll::Pending,
            }
        }
        match Pin::new(
            stream
                .shutdown_response
                .as_mut()
                .expect("shutdown response was installed"),
        )
        .poll(cx)
        {
            Poll::Ready(Ok(result)) => Poll::Ready(result),
            Poll::Ready(Err(_)) => Poll::Ready(Err(stream.terminal_error())),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for UtpStream {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

#[derive(Debug)]
struct AtomicU64Range {
    minimum: AtomicU64,
    maximum: AtomicU64,
    observed: AtomicBool,
}

impl Default for AtomicU64Range {
    fn default() -> Self {
        Self {
            minimum: AtomicU64::new(u64::MAX),
            maximum: AtomicU64::new(0),
            observed: AtomicBool::new(false),
        }
    }
}

impl AtomicU64Range {
    fn record(&self, value: u64) {
        self.minimum.fetch_min(value, Ordering::Relaxed);
        self.maximum.fetch_max(value, Ordering::Relaxed);
        self.observed.store(true, Ordering::Release);
    }

    fn snapshot(&self) -> (Option<u64>, Option<u64>) {
        if self.observed.load(Ordering::Acquire) {
            (
                Some(self.minimum.load(Ordering::Relaxed)),
                Some(self.maximum.load(Ordering::Relaxed)),
            )
        } else {
            (None, None)
        }
    }
}

#[derive(Debug)]
struct AtomicUsizeRange {
    minimum: AtomicUsize,
    maximum: AtomicUsize,
    observed: AtomicBool,
}

impl Default for AtomicUsizeRange {
    fn default() -> Self {
        Self {
            minimum: AtomicUsize::new(usize::MAX),
            maximum: AtomicUsize::new(0),
            observed: AtomicBool::new(false),
        }
    }
}

impl AtomicUsizeRange {
    fn record(&self, value: usize) {
        self.minimum.fetch_min(value, Ordering::Relaxed);
        self.maximum.fetch_max(value, Ordering::Relaxed);
        self.observed.store(true, Ordering::Release);
    }

    fn snapshot(&self) -> (Option<usize>, Option<usize>) {
        if self.observed.load(Ordering::Acquire) {
            (
                Some(self.minimum.load(Ordering::Relaxed)),
                Some(self.maximum.load(Ordering::Relaxed)),
            )
        } else {
            (None, None)
        }
    }
}

#[derive(Debug, Default)]
struct UtpStats {
    path_mtu_profile: UtpPathMtuProfile,
    active_connections: AtomicUsize,
    connections_started: AtomicU64,
    connection_high_water: AtomicUsize,
    incoming_half_open: AtomicUsize,
    incoming_half_open_high_water: AtomicUsize,
    incoming_stream_queue_high_water: AtomicUsize,
    connection_datagram_queue_high_water: AtomicUsize,
    malformed_datagrams: AtomicU64,
    unknown_connection_datagrams: AtomicU64,
    stale_generation_datagrams: AtomicU64,
    connection_datagrams_dropped: AtomicU64,
    datagrams_sent: AtomicU64,
    datagram_bytes_sent: AtomicU64,
    data_datagrams_sent: AtomicU64,
    state_datagrams_sent: AtomicU64,
    retransmission_datagrams_sent: AtomicU64,
    retransmission_bytes_sent: AtomicU64,
    retransmission_queue_high_water: AtomicUsize,
    in_flight_packet_high_water: AtomicUsize,
    in_flight_byte_high_water: AtomicUsize,
    congestion_control_acknowledgements_high_water: AtomicU64,
    congestion_control_acknowledged_bytes_high_water: AtomicU64,
    congestion_limited_acknowledgements_high_water: AtomicU64,
    sender_underfilled_acknowledgements_high_water: AtomicU64,
    remote_window_limited_acknowledgements_high_water: AtomicU64,
    window_growth_acknowledgements_high_water: AtomicU64,
    pending_ack_packet_high_water: AtomicUsize,
    loss_reduction_high_water: AtomicU64,
    timeout_collapse_high_water: AtomicU64,
    delivered_byte_high_water: AtomicUsize,
    unsent_byte_high_water: AtomicUsize,
    sent_byte_high_water: AtomicUsize,
    application_coalesce_byte_high_water: AtomicUsize,
    smoothed_rtt_micros: AtomicU64Range,
    effective_rto_micros: AtomicU64Range,
    base_delay_micros: AtomicU64Range,
    queue_delay_micros: AtomicU64Range,
    congestion_window_bytes: AtomicUsizeRange,
    advertised_receive_window_bytes: AtomicUsizeRange,
    selected_mtu_bytes: AtomicUsizeRange,
    mtu_candidate_bytes: AtomicUsizeRange,
    mtu_probes_started_high_water: AtomicU64,
    mtu_probes_acknowledged_high_water: AtomicU64,
    mtu_probes_failed_high_water: AtomicU64,
    mtu_revalidations_started_high_water: AtomicU64,
    mtu_revalidations_acknowledged_high_water: AtomicU64,
    mtu_revalidations_failed_high_water: AtomicU64,
    mtu_downward_recoveries_high_water: AtomicU64,
    mtu_probe_datagrams_sent: AtomicU64,
    mtu_fragmentable_retry_datagrams_sent: AtomicU64,
    retry_exhausted_connections: AtomicU64,
    worker_panics: AtomicU64,
}

impl UtpStats {
    fn with_profile(path_mtu_profile: UtpPathMtuProfile) -> Self {
        Self {
            path_mtu_profile,
            ..Self::default()
        }
    }

    fn snapshot(&self) -> UtpServiceSnapshot {
        let (smoothed_rtt_min_micros, smoothed_rtt_max_micros) =
            self.smoothed_rtt_micros.snapshot();
        let (effective_rto_min_micros, effective_rto_max_micros) =
            self.effective_rto_micros.snapshot();
        let (base_delay_min_micros, base_delay_max_micros) = self.base_delay_micros.snapshot();
        let (queue_delay_min_micros, queue_delay_max_micros) = self.queue_delay_micros.snapshot();
        let (congestion_window_min_bytes, congestion_window_max_bytes) =
            self.congestion_window_bytes.snapshot();
        let (advertised_receive_window_min_bytes, advertised_receive_window_max_bytes) =
            self.advertised_receive_window_bytes.snapshot();
        let (selected_mtu_min_bytes, selected_mtu_max_bytes) = self.selected_mtu_bytes.snapshot();
        let (mtu_candidate_min_bytes, mtu_candidate_max_bytes) =
            self.mtu_candidate_bytes.snapshot();
        UtpServiceSnapshot {
            path_mtu_profile: self.path_mtu_profile,
            active_connections: self.active_connections.load(Ordering::Relaxed),
            connections_started: self.connections_started.load(Ordering::Relaxed),
            connection_high_water: self.connection_high_water.load(Ordering::Relaxed),
            incoming_half_open: self.incoming_half_open.load(Ordering::Relaxed),
            incoming_half_open_high_water: self
                .incoming_half_open_high_water
                .load(Ordering::Relaxed),
            incoming_stream_queue_high_water: self
                .incoming_stream_queue_high_water
                .load(Ordering::Relaxed),
            connection_datagram_queue_high_water: self
                .connection_datagram_queue_high_water
                .load(Ordering::Relaxed),
            malformed_datagrams: self.malformed_datagrams.load(Ordering::Relaxed),
            unknown_connection_datagrams: self.unknown_connection_datagrams.load(Ordering::Relaxed),
            stale_generation_datagrams: self.stale_generation_datagrams.load(Ordering::Relaxed),
            connection_datagrams_dropped: self.connection_datagrams_dropped.load(Ordering::Relaxed),
            datagrams_sent: self.datagrams_sent.load(Ordering::Relaxed),
            datagram_bytes_sent: self.datagram_bytes_sent.load(Ordering::Relaxed),
            data_datagrams_sent: self.data_datagrams_sent.load(Ordering::Relaxed),
            state_datagrams_sent: self.state_datagrams_sent.load(Ordering::Relaxed),
            retransmission_datagrams_sent: self
                .retransmission_datagrams_sent
                .load(Ordering::Relaxed),
            retransmission_bytes_sent: self.retransmission_bytes_sent.load(Ordering::Relaxed),
            retransmission_queue_high_water: self
                .retransmission_queue_high_water
                .load(Ordering::Relaxed),
            in_flight_packet_high_water: self.in_flight_packet_high_water.load(Ordering::Relaxed),
            in_flight_byte_high_water: self.in_flight_byte_high_water.load(Ordering::Relaxed),
            congestion_control_acknowledgements_high_water: self
                .congestion_control_acknowledgements_high_water
                .load(Ordering::Relaxed),
            congestion_control_acknowledged_bytes_high_water: self
                .congestion_control_acknowledged_bytes_high_water
                .load(Ordering::Relaxed),
            congestion_limited_acknowledgements_high_water: self
                .congestion_limited_acknowledgements_high_water
                .load(Ordering::Relaxed),
            sender_underfilled_acknowledgements_high_water: self
                .sender_underfilled_acknowledgements_high_water
                .load(Ordering::Relaxed),
            remote_window_limited_acknowledgements_high_water: self
                .remote_window_limited_acknowledgements_high_water
                .load(Ordering::Relaxed),
            window_growth_acknowledgements_high_water: self
                .window_growth_acknowledgements_high_water
                .load(Ordering::Relaxed),
            pending_ack_packet_high_water: self
                .pending_ack_packet_high_water
                .load(Ordering::Relaxed),
            loss_reduction_high_water: self.loss_reduction_high_water.load(Ordering::Relaxed),
            timeout_collapse_high_water: self.timeout_collapse_high_water.load(Ordering::Relaxed),
            delivered_byte_high_water: self.delivered_byte_high_water.load(Ordering::Relaxed),
            unsent_byte_high_water: self.unsent_byte_high_water.load(Ordering::Relaxed),
            sent_byte_high_water: self.sent_byte_high_water.load(Ordering::Relaxed),
            application_coalesce_byte_high_water: self
                .application_coalesce_byte_high_water
                .load(Ordering::Relaxed),
            smoothed_rtt_min_micros,
            smoothed_rtt_max_micros,
            effective_rto_min_micros,
            effective_rto_max_micros,
            base_delay_min_micros,
            base_delay_max_micros,
            queue_delay_min_micros,
            queue_delay_max_micros,
            congestion_window_min_bytes,
            congestion_window_max_bytes,
            advertised_receive_window_min_bytes,
            advertised_receive_window_max_bytes,
            selected_mtu_min_bytes,
            selected_mtu_max_bytes,
            mtu_candidate_min_bytes,
            mtu_candidate_max_bytes,
            mtu_probes_started_high_water: self
                .mtu_probes_started_high_water
                .load(Ordering::Relaxed),
            mtu_probes_acknowledged_high_water: self
                .mtu_probes_acknowledged_high_water
                .load(Ordering::Relaxed),
            mtu_probes_failed_high_water: self.mtu_probes_failed_high_water.load(Ordering::Relaxed),
            mtu_revalidations_started_high_water: self
                .mtu_revalidations_started_high_water
                .load(Ordering::Relaxed),
            mtu_revalidations_acknowledged_high_water: self
                .mtu_revalidations_acknowledged_high_water
                .load(Ordering::Relaxed),
            mtu_revalidations_failed_high_water: self
                .mtu_revalidations_failed_high_water
                .load(Ordering::Relaxed),
            mtu_downward_recoveries_high_water: self
                .mtu_downward_recoveries_high_water
                .load(Ordering::Relaxed),
            mtu_probe_datagrams_sent: self.mtu_probe_datagrams_sent.load(Ordering::Relaxed),
            mtu_fragmentable_retry_datagrams_sent: self
                .mtu_fragmentable_retry_datagrams_sent
                .load(Ordering::Relaxed),
            retry_exhausted_connections: self.retry_exhausted_connections.load(Ordering::Relaxed),
            worker_panics: self.worker_panics.load(Ordering::Relaxed),
        }
    }

    fn connection_started(&self, incoming: bool) {
        saturating_increment(&self.connections_started, 1);
        let active = self
            .active_connections
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        self.connection_high_water
            .fetch_max(active, Ordering::Relaxed);
        if incoming {
            let half_open = self
                .incoming_half_open
                .fetch_add(1, Ordering::AcqRel)
                .saturating_add(1);
            self.incoming_half_open_high_water
                .fetch_max(half_open, Ordering::Relaxed);
        }
    }

    fn record_datagram_sent(&self, packet_type: PacketType, length: usize) {
        saturating_increment(&self.datagrams_sent, 1);
        saturating_increment(
            &self.datagram_bytes_sent,
            u64::try_from(length).unwrap_or(u64::MAX),
        );
        match packet_type {
            PacketType::Data => saturating_increment(&self.data_datagrams_sent, 1),
            PacketType::State => saturating_increment(&self.state_datagrams_sent, 1),
            PacketType::Fin | PacketType::Reset | PacketType::Syn => {}
        }
    }

    fn connection_stopped(&self) {
        self.active_connections.fetch_sub(1, Ordering::AcqRel);
    }

    fn half_open_published(&self) {
        self.incoming_half_open.fetch_sub(1, Ordering::AcqRel);
    }

    fn record_worker_snapshot(&self, state: &TransportState) {
        let snapshot = state.snapshot();
        self.unsent_byte_high_water
            .fetch_max(snapshot.transmit.byte_high_water, Ordering::Relaxed);
        self.sent_byte_high_water
            .fetch_max(snapshot.connection.send.byte_high_water, Ordering::Relaxed);
        self.retransmission_queue_high_water.fetch_max(
            snapshot.retransmissions.packet_high_water,
            Ordering::Relaxed,
        );
        self.in_flight_packet_high_water
            .fetch_max(snapshot.in_flight_packet_high_water, Ordering::Relaxed);
        self.in_flight_byte_high_water
            .fetch_max(snapshot.in_flight_byte_high_water, Ordering::Relaxed);
        self.congestion_control_acknowledgements_high_water
            .fetch_max(
                snapshot.congestion_control_acknowledgements,
                Ordering::Relaxed,
            );
        self.congestion_control_acknowledged_bytes_high_water
            .fetch_max(
                snapshot.congestion_control_acknowledged_bytes,
                Ordering::Relaxed,
            );
        self.congestion_limited_acknowledgements_high_water
            .fetch_max(
                snapshot.congestion_limited_acknowledgements,
                Ordering::Relaxed,
            );
        self.sender_underfilled_acknowledgements_high_water
            .fetch_max(
                snapshot.sender_underfilled_acknowledgements,
                Ordering::Relaxed,
            );
        self.remote_window_limited_acknowledgements_high_water
            .fetch_max(
                snapshot.remote_window_limited_acknowledgements,
                Ordering::Relaxed,
            );
        self.window_growth_acknowledgements_high_water
            .fetch_max(snapshot.window_growth_acknowledgements, Ordering::Relaxed);
        self.pending_ack_packet_high_water.fetch_max(
            usize::from(snapshot.acknowledgements.pending_packets),
            Ordering::Relaxed,
        );
        self.loss_reduction_high_water
            .fetch_max(snapshot.congestion.loss_reductions, Ordering::Relaxed);
        self.timeout_collapse_high_water
            .fetch_max(snapshot.congestion.timeout_collapses, Ordering::Relaxed);
        if let Some(smoothed_rtt_micros) = snapshot.connection.send.rtt.smoothed_rtt_micros {
            self.smoothed_rtt_micros.record(smoothed_rtt_micros);
        }
        self.effective_rto_micros
            .record(snapshot.connection.send.rtt.effective_rto_micros);
        if let Some(base_delay_micros) = snapshot.congestion.delay.base_delay_micros {
            self.base_delay_micros.record(u64::from(base_delay_micros));
        }
        if let Some(queue_delay_micros) = snapshot.congestion.delay.queue_delay_micros {
            self.queue_delay_micros
                .record(u64::from(queue_delay_micros));
        }
        self.congestion_window_bytes
            .record(snapshot.congestion.congestion_window_bytes);
        self.selected_mtu_bytes
            .record(snapshot.mtu.floor_datagram_bytes);
        self.mtu_candidate_bytes
            .record(snapshot.mtu.candidate_datagram_bytes);
        self.mtu_probes_started_high_water
            .fetch_max(snapshot.mtu.probes_started, Ordering::Relaxed);
        self.mtu_probes_acknowledged_high_water
            .fetch_max(snapshot.mtu.probes_acknowledged, Ordering::Relaxed);
        self.mtu_probes_failed_high_water
            .fetch_max(snapshot.mtu.probes_failed, Ordering::Relaxed);
        self.mtu_revalidations_started_high_water
            .fetch_max(snapshot.mtu.revalidations_started, Ordering::Relaxed);
        self.mtu_revalidations_acknowledged_high_water
            .fetch_max(snapshot.mtu.revalidations_acknowledged, Ordering::Relaxed);
        self.mtu_revalidations_failed_high_water
            .fetch_max(snapshot.mtu.revalidations_failed, Ordering::Relaxed);
        self.mtu_downward_recoveries_high_water
            .fetch_max(snapshot.mtu.downward_recoveries, Ordering::Relaxed);
        if let Some(receive) = snapshot.connection.receive {
            self.delivered_byte_high_water
                .fetch_max(receive.byte_high_water, Ordering::Relaxed);
            self.advertised_receive_window_bytes
                .record(receive.advertised_window_bytes);
        }
    }
}

fn saturating_increment(value: &AtomicU64, increment: u64) {
    let _ = value.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
        Some(current.saturating_add(increment))
    });
}

async fn run_service(
    mut transport: SessionUtpTransport,
    mut commands: mpsc::Receiver<UtpServiceCommand>,
    incoming: mpsc::Sender<UtpStream>,
    stats: Arc<UtpStats>,
    cancellation: CancellationToken,
    config: UtpRuntimeConfig,
) -> Result<UtpServiceSnapshot, UtpRuntimeError> {
    let send = transport.send_handle();
    let generations = transport.generation_changes();
    let clock = UtpClock::new()?;
    let mut routes = BTreeMap::new();
    let mut workers = JoinSet::new();
    let result: Result<(), UtpRuntimeError> = loop {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => break Ok(()),
            joined = workers.join_next(), if !workers.is_empty() => {
                handle_worker_join(joined, &mut routes, &stats)?;
            }
            command = commands.recv() => match command {
                Some(command) => handle_service_command(
                    command,
                    &transport,
                    &send,
                    &generations,
                    &clock,
                    &stats,
                    &mut routes,
                    &mut workers,
                    config,
                ),
                None => break Ok(()),
            },
            received = transport.receive() => {
                let (generation, bytes, remote, family) = received?;
                handle_datagram(
                    generation,
                    family,
                    remote,
                    bytes,
                    &transport,
                    &send,
                    &generations,
                    &clock,
                    &incoming,
                    &stats,
                    &mut routes,
                    &mut workers,
                    config,
                );
            }
        }
    };

    for route in routes.values() {
        route.cancellation.cancel();
    }
    while let Some(joined) = workers.join_next().await {
        handle_worker_join(Some(joined), &mut routes, &stats)?;
    }
    result?;
    Ok(stats.snapshot())
}

fn handle_worker_join(
    joined: WorkerJoin,
    routes: &mut BTreeMap<UtpConnectionKey, UtpRoute>,
    stats: &UtpStats,
) -> Result<(), UtpRuntimeError> {
    let Some(joined) = joined else {
        return Ok(());
    };
    let (key, report) = joined.map_err(|error| UtpRuntimeError::TaskJoin(error.to_string()))?;
    routes.remove(&key);
    match report {
        Ok(WorkerReport {
            terminal: WorkerTerminal::RetryExhausted,
        }) => saturating_increment(&stats.retry_exhausted_connections, 1),
        Ok(_) => {}
        Err(_) => saturating_increment(&stats.worker_panics, 1),
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn handle_service_command(
    command: UtpServiceCommand,
    transport: &SessionUtpTransport,
    send: &SessionUtpSendHandle,
    generations: &watch::Receiver<SessionUdpGenerations>,
    clock: &UtpClock,
    stats: &Arc<UtpStats>,
    routes: &mut BTreeMap<UtpConnectionKey, UtpRoute>,
    workers: &mut WorkerSet,
    config: UtpRuntimeConfig,
) {
    match command {
        UtpServiceCommand::Connect {
            target,
            response,
            cancellation,
            termination,
        } => {
            let result = prepare_outgoing(
                target,
                transport,
                send,
                generations,
                clock,
                stats,
                routes,
                workers,
                config,
                cancellation,
                termination,
                response,
            );
            if let Err((error, response)) = result {
                let _ = response.send(Err(error));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn prepare_outgoing(
    target: SocketAddr,
    transport: &SessionUtpTransport,
    send: &SessionUtpSendHandle,
    generations: &watch::Receiver<SessionUdpGenerations>,
    clock: &UtpClock,
    stats: &Arc<UtpStats>,
    routes: &mut BTreeMap<UtpConnectionKey, UtpRoute>,
    workers: &mut WorkerSet,
    config: UtpRuntimeConfig,
    cancellation: CancellationToken,
    termination: oneshot::Sender<()>,
    response: oneshot::Sender<Result<UtpStream, UtpRuntimeError>>,
) -> Result<
    (),
    (
        UtpRuntimeError,
        oneshot::Sender<Result<UtpStream, UtpRuntimeError>>,
    ),
> {
    let family = AddressFamily::of(target.ip());
    if family != AddressFamily::Ipv4 {
        return Err((UtpRuntimeError::UnsupportedFamily(family), response));
    }
    if routes.len() >= MAX_UTP_CONNECTIONS {
        return Err((UtpRuntimeError::ConnectionLimit, response));
    }
    let Some(generation) = transport.generation_for(family) else {
        return Err((
            UtpRuntimeError::Udp(SessionUdpError::MissingFamily(family)),
            response,
        ));
    };
    let Some(local_address) = transport.local_address_for(family) else {
        return Err((
            UtpRuntimeError::Udp(SessionUdpError::MissingFamily(family)),
            response,
        ));
    };
    let receive_connection_id = match unique_connection_id(family, generation, target, routes) {
        Ok(connection_id) => connection_id,
        Err(error) => return Err((error, response)),
    };
    let initial_sequence = match random_u16() {
        Ok(sequence) => SequenceNumber::new(sequence),
        Err(error) => return Err((error, response)),
    };
    let state = match TransportState::initiate(
        receive_connection_id,
        initial_sequence,
        clock.now_micros(),
        config.floor_datagram_bytes,
        config.ceiling_datagram_bytes,
    ) {
        Ok(state) => state,
        Err(error) => return Err((error.into(), response)),
    };
    let key = UtpConnectionKey {
        family,
        generation,
        remote: target,
        receive_connection_id,
    };
    let (stream, channels) = stream_pair(local_address, target);
    spawn_worker(
        key,
        state,
        WorkerPublication::Outgoing { stream, response },
        channels,
        send.clone(),
        generations.clone(),
        clock.clone(),
        stats.clone(),
        routes,
        workers,
        false,
        cancellation,
        Some(termination),
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn handle_datagram(
    generation: u64,
    family: AddressFamily,
    remote: SocketAddr,
    bytes: Vec<u8>,
    transport: &SessionUtpTransport,
    send: &SessionUtpSendHandle,
    generations: &watch::Receiver<SessionUdpGenerations>,
    clock: &UtpClock,
    incoming: &mpsc::Sender<UtpStream>,
    stats: &Arc<UtpStats>,
    routes: &mut BTreeMap<UtpConnectionKey, UtpRoute>,
    workers: &mut WorkerSet,
    config: UtpRuntimeConfig,
) {
    if family != AddressFamily::Ipv4 {
        saturating_increment(&stats.unknown_connection_datagrams, 1);
        return;
    }
    if transport.generation_for(family) != Some(generation) {
        saturating_increment(&stats.stale_generation_datagrams, 1);
        return;
    }
    let packet = match decode_packet(&bytes) {
        Ok(packet) => packet,
        Err(_) => {
            saturating_increment(&stats.malformed_datagrams, 1);
            return;
        }
    };
    let receive_connection_id = if packet.header.packet_type == PacketType::Syn {
        packet.header.connection_id.wrapping_add(1)
    } else {
        packet.header.connection_id
    };
    let key = UtpConnectionKey {
        family,
        generation,
        remote,
        receive_connection_id,
    };
    if let Some(route) = routes.get(&key) {
        route_existing_datagram(route, bytes, stats);
        return;
    }
    if packet.header.packet_type != PacketType::Syn {
        if packet.header.packet_type != PacketType::Reset {
            saturating_increment(&stats.unknown_connection_datagrams, 1);
        }
        return;
    }
    if routes.len() >= MAX_UTP_CONNECTIONS
        || stats.incoming_half_open.load(Ordering::Relaxed) >= MAX_INCOMING_UTP_HALF_OPEN
    {
        saturating_increment(&stats.connection_datagrams_dropped, 1);
        return;
    }
    let permit = match incoming.clone().try_reserve_owned() {
        Ok(permit) => permit,
        Err(_) => {
            saturating_increment(&stats.connection_datagrams_dropped, 1);
            return;
        }
    };
    let initial_sequence = match random_u16() {
        Ok(sequence) => SequenceNumber::new(sequence),
        Err(_) => {
            saturating_increment(&stats.connection_datagrams_dropped, 1);
            return;
        }
    };
    let state = match TransportState::accept_syn(
        packet,
        initial_sequence,
        config.floor_datagram_bytes,
        config.ceiling_datagram_bytes,
    ) {
        Ok(state) => state,
        Err(_) => {
            saturating_increment(&stats.malformed_datagrams, 1);
            return;
        }
    };
    let Some(local_address) = transport.local_address_for(family) else {
        saturating_increment(&stats.stale_generation_datagrams, 1);
        return;
    };
    let (stream, channels) = stream_pair(local_address, remote);
    spawn_worker(
        key,
        state,
        WorkerPublication::Incoming { stream, permit },
        channels,
        send.clone(),
        generations.clone(),
        clock.clone(),
        stats.clone(),
        routes,
        workers,
        true,
        CancellationToken::new(),
        None,
    );
}

fn route_existing_datagram(route: &UtpRoute, bytes: Vec<u8>, stats: &UtpStats) {
    match route.ingress.try_send(bytes) {
        Ok(()) => {
            let queued = route
                .ingress
                .max_capacity()
                .saturating_sub(route.ingress.capacity());
            stats
                .connection_datagram_queue_high_water
                .fetch_max(queued, Ordering::Relaxed);
        }
        Err(TrySendError::Full(_)) => {
            saturating_increment(&stats.connection_datagrams_dropped, 1);
        }
        Err(TrySendError::Closed(_)) => {
            saturating_increment(&stats.unknown_connection_datagrams, 1);
        }
    }
}

fn unique_connection_id(
    family: AddressFamily,
    generation: u64,
    remote: SocketAddr,
    routes: &BTreeMap<UtpConnectionKey, UtpRoute>,
) -> Result<u16, UtpRuntimeError> {
    for _ in 0..UTP_ENTROPY_DRAWS {
        let receive_connection_id = random_u16()?;
        let key = UtpConnectionKey {
            family,
            generation,
            remote,
            receive_connection_id,
        };
        if !routes.contains_key(&key) {
            return Ok(receive_connection_id);
        }
    }
    Err(UtpRuntimeError::ConnectionIdCollision)
}

fn random_u16() -> Result<u16, UtpRuntimeError> {
    let mut bytes = [0; 2];
    getrandom::fill(&mut bytes).map_err(|error| UtpRuntimeError::Entropy(error.to_string()))?;
    Ok(u16::from_be_bytes(bytes))
}

#[derive(Debug)]
struct WorkerChannels {
    writes: mpsc::Receiver<Vec<u8>>,
    controls: mpsc::Receiver<UtpStreamControl>,
    events: mpsc::Sender<UtpStreamEvent>,
    consumption: Arc<UtpConsumption>,
    read_notification: Arc<Notify>,
    stream_cancellation: CancellationToken,
    terminal: Arc<Mutex<Option<UtpStreamTerminal>>>,
}

fn stream_pair(
    local_address: SocketAddr,
    remote_address: SocketAddr,
) -> (UtpStream, WorkerChannels) {
    let (write_sender, writes) = mpsc::channel(UTP_APPLICATION_WRITE_QUEUE);
    let (control_sender, controls) = mpsc::channel(UTP_APPLICATION_CONTROL_QUEUE);
    let (event_sender, events) = mpsc::channel(UTP_STREAM_EVENT_QUEUE);
    let consumption = Arc::new(UtpConsumption::default());
    let read_notification = Arc::new(Notify::new());
    let cancellation = CancellationToken::new();
    let terminal = Arc::new(Mutex::new(None));
    (
        UtpStream {
            local_address,
            remote_address,
            write_sender: PollSender::new(write_sender.clone()),
            try_write_sender: write_sender,
            control_sender: PollSender::new(control_sender),
            events,
            current_read: None,
            consumption: consumption.clone(),
            read_notification: read_notification.clone(),
            cancellation: cancellation.clone(),
            terminal: terminal.clone(),
            flush_response: None,
            shutdown_response: None,
            eof: false,
            write_closed: false,
        },
        WorkerChannels {
            writes,
            controls,
            events: event_sender,
            consumption,
            read_notification,
            stream_cancellation: cancellation,
            terminal,
        },
    )
}

#[derive(Debug)]
enum WorkerPublication {
    Outgoing {
        stream: UtpStream,
        response: oneshot::Sender<Result<UtpStream, UtpRuntimeError>>,
    },
    Incoming {
        stream: UtpStream,
        permit: mpsc::OwnedPermit<UtpStream>,
    },
    Published,
}

#[derive(Clone, Debug)]
enum WorkerTerminal {
    Graceful,
    Reset,
    RetryExhausted,
    ConsumerDropped,
    GenerationChanged,
    ServiceCancelled,
    Protocol(String),
    Io(String),
}

impl WorkerTerminal {
    fn stream_terminal(&self) -> UtpStreamTerminal {
        let (kind, detail) = match self {
            Self::Graceful => (
                io::ErrorKind::UnexpectedEof,
                "uTP stream reached EOF".into(),
            ),
            Self::Reset => (
                io::ErrorKind::ConnectionReset,
                "uTP peer reset the connection".into(),
            ),
            Self::RetryExhausted => (
                io::ErrorKind::TimedOut,
                "uTP retransmission limit was exhausted".into(),
            ),
            Self::ConsumerDropped => (
                io::ErrorKind::ConnectionAborted,
                "uTP stream consumer was dropped".into(),
            ),
            Self::GenerationChanged => (
                io::ErrorKind::ConnectionAborted,
                "uTP session socket generation changed".into(),
            ),
            Self::ServiceCancelled => (
                io::ErrorKind::Interrupted,
                "uTP service was cancelled".into(),
            ),
            Self::Protocol(detail) => (io::ErrorKind::InvalidData, detail.clone()),
            Self::Io(detail) => (io::ErrorKind::Other, detail.clone()),
        };
        UtpStreamTerminal { kind, detail }
    }
}

#[derive(Clone, Debug)]
struct WorkerReport {
    terminal: WorkerTerminal,
}

#[allow(clippy::too_many_arguments)]
fn spawn_worker(
    key: UtpConnectionKey,
    state: TransportState,
    publication: WorkerPublication,
    channels: WorkerChannels,
    send: SessionUtpSendHandle,
    generations: watch::Receiver<SessionUdpGenerations>,
    clock: UtpClock,
    stats: Arc<UtpStats>,
    routes: &mut BTreeMap<UtpConnectionKey, UtpRoute>,
    workers: &mut WorkerSet,
    incoming: bool,
    cancellation: CancellationToken,
    termination: Option<oneshot::Sender<()>>,
) {
    let (ingress, datagrams) = mpsc::channel(UTP_CONNECTION_DATAGRAM_QUEUE);
    routes.insert(
        key,
        UtpRoute {
            ingress,
            cancellation: cancellation.clone(),
        },
    );
    stats.connection_started(incoming);
    let worker_stats = stats.clone();
    let worker = UtpWorker {
        key,
        state,
        datagrams,
        publication,
        writes: channels.writes,
        controls: channels.controls,
        events: channels.events,
        pending_application_write: Vec::new(),
        application_write_deadline: None,
        pending_delivery: VecDeque::new(),
        consumption: channels.consumption,
        read_notification: channels.read_notification,
        stream_cancellation: channels.stream_cancellation,
        terminal: channels.terminal,
        cancellation,
        send,
        generations,
        clock,
        stats,
        pending_flush: None,
        pending_shutdown: None,
        remote_eof: false,
        eof_sent: false,
        incoming_half_open: incoming,
    };
    workers.spawn(async move {
        let result = AssertUnwindSafe(worker.run()).catch_unwind().await;
        worker_stats.connection_stopped();
        if let Some(termination) = termination {
            let _ = termination.send(());
        }
        (key, result)
    });
}

#[derive(Debug)]
struct UtpWorker {
    key: UtpConnectionKey,
    state: TransportState,
    datagrams: mpsc::Receiver<Vec<u8>>,
    publication: WorkerPublication,
    writes: mpsc::Receiver<Vec<u8>>,
    controls: mpsc::Receiver<UtpStreamControl>,
    events: mpsc::Sender<UtpStreamEvent>,
    pending_application_write: Vec<u8>,
    application_write_deadline: Option<Instant>,
    pending_delivery: VecDeque<Vec<u8>>,
    consumption: Arc<UtpConsumption>,
    read_notification: Arc<Notify>,
    stream_cancellation: CancellationToken,
    terminal: Arc<Mutex<Option<UtpStreamTerminal>>>,
    cancellation: CancellationToken,
    send: SessionUtpSendHandle,
    generations: watch::Receiver<SessionUdpGenerations>,
    clock: UtpClock,
    stats: Arc<UtpStats>,
    pending_flush: Option<oneshot::Sender<io::Result<()>>>,
    pending_shutdown: Option<oneshot::Sender<io::Result<()>>>,
    remote_eof: bool,
    eof_sent: bool,
    incoming_half_open: bool,
}

impl UtpWorker {
    async fn run(mut self) -> WorkerReport {
        let terminal = match self.run_loop().await {
            Ok(terminal) => terminal,
            Err(error) => classify_worker_error(error),
        };
        self.state.abort();
        self.stats.record_worker_snapshot(&self.state);
        if self.incoming_half_open {
            self.stats.half_open_published();
            self.incoming_half_open = false;
        }
        self.fail_pending_controls(&terminal);
        *self
            .terminal
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(terminal.stream_terminal());
        self.read_notification.notify_waiters();
        WorkerReport { terminal }
    }

    async fn run_loop(&mut self) -> Result<WorkerTerminal, UtpRuntimeError> {
        loop {
            self.consume_application_bytes()?;
            self.flush_application_write_if_ready()?;
            self.flush_delivery();
            let sent_any = self.drain_emissions().await?;
            self.maybe_publish(sent_any)?;
            self.complete_flush_if_ready();
            let snapshot = self.state.snapshot();
            self.stats.record_worker_snapshot(&self.state);
            if snapshot.connection.ready_to_close {
                self.state.finish()?;
                if let Some(response) = self.pending_shutdown.take() {
                    let _ = response.send(Ok(()));
                }
                return Ok(WorkerTerminal::Graceful);
            }
            if snapshot.connection.phase == ConnectionPhase::Reset {
                return Ok(WorkerTerminal::Reset);
            }
            let can_accept_write = !snapshot.close_requested
                && snapshot
                    .transmit
                    .unsent_bytes
                    .saturating_add(self.pending_application_write.len())
                    <= MAX_UNSENT_BYTES.saturating_sub(MAX_UTP_APPLICATION_WRITE_BYTES);
            let now_micros = self.clock.now_micros();
            let transport_deadline = self
                .state
                .next_wakeup_micros()
                .filter(|micros| *micros > now_micros)
                .map(|micros| self.clock.instant_at(micros));
            let deadline = minimum_instant(transport_deadline, self.application_write_deadline);
            tokio::select! {
                biased;
                _ = self.cancellation.cancelled() => return Ok(WorkerTerminal::ServiceCancelled),
                _ = self.stream_cancellation.cancelled() => {
                    self.send_pending_courtesy_ack().await?;
                    return Ok(WorkerTerminal::ConsumerDropped);
                },
                changed = self.generations.changed() => {
                    if changed.is_err()
                        || self.generations.borrow_and_update().generation_for(self.key.family)
                            != Some(self.key.generation)
                    {
                        return Ok(WorkerTerminal::GenerationChanged);
                    }
                }
                bytes = self.datagrams.recv() => match bytes {
                    Some(bytes) => self.handle_incoming(&bytes)?,
                    None => return Ok(WorkerTerminal::ServiceCancelled),
                },
                write = self.writes.recv(), if can_accept_write => match write {
                    Some(bytes) => self.queue_application_write(bytes),
                    None if self.controls.is_closed() => return Ok(WorkerTerminal::ConsumerDropped),
                    None => {}
                },
                control = self.controls.recv(), if self.writes.is_empty() => match control {
                    Some(control) => self.handle_control(control),
                    None if self.writes.is_closed() => return Ok(WorkerTerminal::ConsumerDropped),
                    None => {}
                },
                _ = wait_for_deadline(deadline) => {}
                _ = self.consumption.notification.notified() => {}
            }
        }
    }

    fn consume_application_bytes(&mut self) -> Result<(), UtpRuntimeError> {
        let consumed = self.consumption.take();
        if consumed > 0
            && !matches!(
                self.state.snapshot().connection.phase,
                ConnectionPhase::Closed | ConnectionPhase::Reset
            )
        {
            self.state
                .consume_received(consumed, self.clock.now_micros())?;
        }
        Ok(())
    }

    fn queue_application_write(&mut self, bytes: Vec<u8>) {
        if self.pending_application_write.is_empty() {
            self.application_write_deadline = Some(Instant::now() + UTP_APPLICATION_COALESCE_DELAY);
        }
        self.pending_application_write.extend_from_slice(&bytes);
        self.stats
            .application_coalesce_byte_high_water
            .fetch_max(self.pending_application_write.len(), Ordering::Relaxed);
    }

    fn flush_application_write_if_ready(&mut self) -> Result<(), UtpRuntimeError> {
        if self.pending_application_write.is_empty()
            || !should_flush_application_write(
                self.pending_application_write.len(),
                self.application_write_deadline
                    .is_some_and(|deadline| Instant::now() >= deadline),
                self.pending_flush.is_some() || self.pending_shutdown.is_some(),
            )
        {
            return Ok(());
        }
        let bytes = std::mem::take(&mut self.pending_application_write);
        self.application_write_deadline = None;
        self.state.queue_data(&bytes)?;
        Ok(())
    }

    fn handle_incoming(&mut self, bytes: &[u8]) -> Result<(), UtpRuntimeError> {
        let packet = decode_packet(bytes)?;
        let outcome =
            self.state
                .incoming(packet, self.clock.now_micros(), self.clock.timestamp())?;
        if outcome.connection.disposition != IncomingDisposition::Accepted {
            return Ok(());
        }
        if let Some(receive) = outcome.connection.receive {
            for delivered in receive.delivered {
                self.append_delivery(delivered.bytes);
            }
            if receive.eof_reached {
                self.remote_eof = true;
            }
        }
        Ok(())
    }

    fn append_delivery(&mut self, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        if let Some(last) = self.pending_delivery.back_mut()
            && last.len().saturating_add(bytes.len()) <= UTP_DELIVERY_CHUNK_BYTES
        {
            last.extend(bytes);
        } else {
            self.pending_delivery.push_back(bytes);
        }
    }

    fn flush_delivery(&mut self) {
        while let Some(bytes) = self.pending_delivery.pop_front() {
            match self.events.try_send(UtpStreamEvent::Data(bytes)) {
                Ok(()) => self.read_notification.notify_one(),
                Err(TrySendError::Full(UtpStreamEvent::Data(bytes))) => {
                    self.pending_delivery.push_front(bytes);
                    break;
                }
                Err(TrySendError::Closed(_)) => {
                    self.stream_cancellation.cancel();
                    return;
                }
                Err(TrySendError::Full(UtpStreamEvent::Eof)) => {
                    unreachable!("flush sends only data")
                }
            }
        }
        if self.pending_delivery.is_empty() && self.remote_eof && !self.eof_sent {
            match self.events.try_send(UtpStreamEvent::Eof) {
                Ok(()) => {
                    self.eof_sent = true;
                    self.read_notification.notify_one();
                }
                Err(TrySendError::Full(_)) => {}
                Err(TrySendError::Closed(_)) => self.stream_cancellation.cancel(),
            }
        }
    }

    async fn drain_emissions(&mut self) -> Result<bool, UtpRuntimeError> {
        let mut sent_any = false;
        for _ in 0..MAX_EMISSIONS_PER_TURN {
            let now_micros = self.clock.now_micros();
            let Some(emission) = self
                .state
                .poll_transmit(now_micros, self.clock.timestamp())?
            else {
                return Ok(sent_any);
            };
            let sequence_number = emission.intent.sequence_number;
            let bytes = emission.encode()?;
            let send_result = match self
                .send
                .send_to(
                    self.key.generation,
                    &bytes,
                    self.key.remote,
                    emission.dont_fragment,
                )
                .await
            {
                Ok((length, DatagramSendResult::Sent)) if length == bytes.len() => {
                    sent_any = true;
                    self.stats
                        .record_datagram_sent(emission.intent.packet_type, length);
                    if emission.retransmission {
                        saturating_increment(&self.stats.retransmission_datagrams_sent, 1);
                        saturating_increment(
                            &self.stats.retransmission_bytes_sent,
                            u64::try_from(length).unwrap_or(u64::MAX),
                        );
                    }
                    if emission.mtu_probe {
                        saturating_increment(&self.stats.mtu_probe_datagrams_sent, 1);
                    }
                    if emission.fragmentable_mtu_retry {
                        saturating_increment(&self.stats.mtu_fragmentable_retry_datagrams_sent, 1);
                    }
                    DatagramSendResult::Sent
                }
                Ok((length, DatagramSendResult::Sent)) => {
                    return Err(UtpRuntimeError::Udp(SessionUdpError::Io(io::Error::new(
                        io::ErrorKind::WriteZero,
                        format!("uTP datagram wrote {length} of {} bytes", bytes.len()),
                    ))));
                }
                Ok((_, DatagramSendResult::WouldBlock)) => {
                    self.send
                        .writable(self.key.generation, self.key.remote)
                        .await?;
                    DatagramSendResult::WouldBlock
                }
                Ok((_, DatagramSendResult::MessageTooLarge)) => DatagramSendResult::MessageTooLarge,
                Err(error @ SessionUdpError::StaleGeneration { .. }) => {
                    return Err(UtpRuntimeError::Udp(error));
                }
                Err(error) => return Err(UtpRuntimeError::Udp(error)),
            };
            self.state
                .on_send_result(sequence_number, send_result, self.clock.now_micros())?;
            if send_result == DatagramSendResult::WouldBlock {
                return Ok(sent_any);
            }
        }
        tokio::task::yield_now().await;
        Ok(sent_any)
    }

    async fn send_pending_courtesy_ack(&mut self) -> Result<(), UtpRuntimeError> {
        let Some(emission) = self
            .state
            .poll_pending_acknowledgement(self.clock.timestamp())?
        else {
            return Ok(());
        };
        let sequence_number = emission.intent.sequence_number;
        let bytes = emission.encode()?;
        match self
            .send
            .send_to(self.key.generation, &bytes, self.key.remote, false)
            .await
        {
            Ok((length, DatagramSendResult::Sent)) if length == bytes.len() => {
                self.stats
                    .record_datagram_sent(emission.intent.packet_type, length);
                self.state.on_send_result(
                    sequence_number,
                    DatagramSendResult::Sent,
                    self.clock.now_micros(),
                )?;
            }
            Ok((_, DatagramSendResult::Sent)) => {}
            Ok((_, DatagramSendResult::WouldBlock | DatagramSendResult::MessageTooLarge)) => {}
            Err(_) => {}
        }
        Ok(())
    }

    fn maybe_publish(&mut self, sent_any: bool) -> Result<(), UtpRuntimeError> {
        let ready = match &self.publication {
            WorkerPublication::Outgoing { .. } => {
                self.state.snapshot().connection.phase != ConnectionPhase::SynSent
            }
            WorkerPublication::Incoming { .. } => sent_any,
            WorkerPublication::Published => false,
        };
        if !ready {
            return Ok(());
        }
        let publication = std::mem::replace(&mut self.publication, WorkerPublication::Published);
        match publication {
            WorkerPublication::Outgoing { stream, response } => {
                response
                    .send(Ok(stream))
                    .map_err(|_| UtpRuntimeError::ServiceStopped)?;
            }
            WorkerPublication::Incoming { stream, permit } => {
                let sender = permit.send(stream);
                let queued = sender.max_capacity().saturating_sub(sender.capacity());
                self.stats
                    .incoming_stream_queue_high_water
                    .fetch_max(queued, Ordering::Relaxed);
                self.stats.half_open_published();
                self.incoming_half_open = false;
            }
            WorkerPublication::Published => {}
        }
        Ok(())
    }

    fn handle_control(&mut self, control: UtpStreamControl) {
        match control {
            UtpStreamControl::Flush(response) => {
                if self.pending_flush.replace(response).is_some() {
                    unreachable!("one bounded stream control is outstanding")
                }
            }
            UtpStreamControl::Shutdown(response) => {
                self.state.request_close();
                if self.pending_shutdown.replace(response).is_some() {
                    unreachable!("one shutdown command is outstanding")
                }
            }
        }
    }

    fn complete_flush_if_ready(&mut self) {
        let snapshot = self.state.snapshot();
        if snapshot.transmit.unsent_bytes == 0
            && snapshot.connection.send.outstanding_bytes == 0
            && let Some(response) = self.pending_flush.take()
        {
            let _ = response.send(Ok(()));
        }
    }

    fn fail_pending_controls(&mut self, terminal: &WorkerTerminal) {
        let stream_terminal = terminal.stream_terminal();
        if let Some(response) = self.pending_flush.take() {
            let _ = response.send(Err(io::Error::new(
                stream_terminal.kind,
                stream_terminal.detail.clone(),
            )));
        }
        if let Some(response) = self.pending_shutdown.take() {
            let result = if matches!(terminal, WorkerTerminal::Graceful) {
                Ok(())
            } else {
                Err(io::Error::new(stream_terminal.kind, stream_terminal.detail))
            };
            let _ = response.send(result);
        }
    }
}

fn classify_worker_error(error: UtpRuntimeError) -> WorkerTerminal {
    match error {
        UtpRuntimeError::Protocol(TransportError::Connection(ConnectionError::Send(
            SendError::TransmissionLimit { .. },
        ))) => WorkerTerminal::RetryExhausted,
        UtpRuntimeError::Udp(SessionUdpError::StaleGeneration { .. }) => {
            WorkerTerminal::GenerationChanged
        }
        UtpRuntimeError::Protocol(error) => WorkerTerminal::Protocol(error.to_string()),
        UtpRuntimeError::Codec(error) => WorkerTerminal::Protocol(error.to_string()),
        error => WorkerTerminal::Io(error.to_string()),
    }
}

async fn wait_for_deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => sleep_until(deadline).await,
        None => pending::<()>().await,
    }
}

fn minimum_instant(left: Option<Instant>, right: Option<Instant>) -> Option<Instant> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(deadline), None) | (None, Some(deadline)) => Some(deadline),
        (None, None) => None,
    }
}

const fn should_flush_application_write(
    bytes: usize,
    deadline_due: bool,
    control_pending: bool,
) -> bool {
    bytes >= UTP_APPLICATION_COALESCE_BYTES || deadline_due || control_pending
}

#[derive(Clone, Debug)]
struct UtpClock {
    origin: Instant,
    wire_offset: u32,
}

impl UtpClock {
    fn new() -> Result<Self, UtpRuntimeError> {
        let mut bytes = [0; 4];
        getrandom::fill(&mut bytes).map_err(|error| UtpRuntimeError::Entropy(error.to_string()))?;
        Ok(Self {
            origin: Instant::now(),
            wire_offset: u32::from_be_bytes(bytes),
        })
    }

    fn now_micros(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_micros()).unwrap_or(u64::MAX)
    }

    fn timestamp(&self) -> TimestampMicros {
        TimestampMicros::new(self.wire_offset.wrapping_add(self.now_micros() as u32))
    }

    fn instant_at(&self, micros: u64) -> Instant {
        self.origin
            .checked_add(Duration::from_micros(micros))
            .unwrap_or_else(Instant::now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstorrent_protocol::utp::{
        ExtensionToEncode, MAX_RECEIVE_BYTES, PacketToEncode, UtpHeader, encode_packet,
    };
    use std::collections::BTreeSet;
    use std::net::Ipv4Addr;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UdpSocket;
    use tokio::time::timeout;

    fn packet(
        packet_type: PacketType,
        connection_id: u16,
        sequence_number: u16,
        acknowledgement_number: u16,
        payload: &[u8],
    ) -> Vec<u8> {
        encode_packet(PacketToEncode {
            header: UtpHeader {
                packet_type,
                connection_id,
                timestamp: TimestampMicros::new(10),
                timestamp_difference_micros: 1,
                window_size: MAX_RECEIVE_BYTES as u32,
                sequence_number: SequenceNumber::new(sequence_number),
                acknowledgement_number: SequenceNumber::new(acknowledgement_number),
            },
            extensions: &[] as &[ExtensionToEncode<'_>],
            payload,
        })
        .expect("encode uTP runtime fixture")
    }

    async fn service() -> (SessionUdpService, crate::SessionUdpTransport, UtpService) {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let (mut udp, dht) = SessionUdpService::start(socket).unwrap();
        let utp = UtpService::start_diagnostic(&mut udp, UtpRuntimeConfig::fixed_ipv4()).unwrap();
        (udp, dht, utp)
    }

    #[cfg(any(target_os = "android", target_os = "linux", target_os = "macos"))]
    #[tokio::test]
    async fn product_start_selects_dynamic_mtu_only_for_verified_capability() {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let (mut udp, dht) = SessionUdpService::start(socket).unwrap();
        assert_eq!(
            udp.snapshot().ipv4_fragmentation_protection,
            Ipv4FragmentationProtectionStatus::Verified
        );
        let utp = UtpService::start(&mut udp).unwrap();
        assert_eq!(
            utp.snapshot().path_mtu_profile,
            UtpPathMtuProfile::DynamicIpv4
        );
        utp.shutdown().await.unwrap();
        drop(dht);
        udp.shutdown().await.unwrap();
    }

    #[test]
    fn product_mtu_profile_fails_closed_without_verified_capability() {
        assert_eq!(
            product_runtime_config(Ipv4FragmentationProtectionStatus::Verified).profile(),
            UtpPathMtuProfile::DynamicIpv4
        );
        for capability in [
            Ipv4FragmentationProtectionStatus::VerificationFailed,
            Ipv4FragmentationProtectionStatus::UnsupportedPlatform,
        ] {
            let config = product_runtime_config(capability);
            assert_eq!(config.profile(), UtpPathMtuProfile::Fixed548);
            assert_eq!(config.floor_datagram_bytes, UTP_RUNTIME_DATAGRAM_BYTES);
            assert_eq!(config.ceiling_datagram_bytes, UTP_RUNTIME_DATAGRAM_BYTES);
        }
    }

    #[test]
    fn application_writes_coalesce_to_the_conservative_mss_or_a_bound() {
        assert!(!should_flush_application_write(
            UTP_APPLICATION_COALESCE_BYTES - 1,
            false,
            false
        ));
        assert!(should_flush_application_write(
            UTP_APPLICATION_COALESCE_BYTES,
            false,
            false
        ));
        assert!(should_flush_application_write(1, true, false));
        assert!(should_flush_application_write(1, false, true));
    }

    #[cfg(any(target_os = "android", target_os = "linux", target_os = "macos"))]
    #[tokio::test]
    async fn product_dynamic_mtu_sends_protected_probes_and_delivers_exact_bytes() {
        let left_socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let right_socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let (mut left_udp, left_dht) = SessionUdpService::start(left_socket).unwrap();
        let (mut right_udp, right_dht) = SessionUdpService::start(right_socket).unwrap();
        let left_utp = UtpService::start(&mut left_udp).unwrap();
        let mut right_utp = UtpService::start(&mut right_udp).unwrap();
        let left_handle = left_utp.handle();
        let right_address = right_udp.local_address();
        let (left, right) = timeout(Duration::from_secs(2), async {
            tokio::join!(left_handle.connect(right_address), right_utp.accept())
        })
        .await
        .expect("product dynamic uTP connection timeout");
        let mut left = left.unwrap();
        let mut right = right.expect("incoming product dynamic uTP stream");
        let payload = (0..256 * 1024)
            .map(|index| u8::try_from(index % 251).unwrap())
            .collect::<Vec<_>>();

        timeout(Duration::from_secs(10), async {
            let sending = async {
                left.write_all(&payload).await.unwrap();
                left.shutdown().await.unwrap();
            };
            let receiving = async {
                let mut received = Vec::new();
                right.read_to_end(&mut received).await.unwrap();
                assert_eq!(received.len(), payload.len(), "received byte count");
                assert_eq!(received, payload);
                right.shutdown().await.unwrap();
            };
            tokio::join!(sending, receiving);
        })
        .await
        .expect("product dynamic uTP transfer timeout");
        drop(left);
        drop(right);

        let left_terminal = left_utp.shutdown().await.unwrap();
        let right_terminal = right_utp.shutdown().await.unwrap();
        assert_eq!(
            left_terminal.path_mtu_profile,
            UtpPathMtuProfile::DynamicIpv4
        );
        assert_eq!(
            right_terminal.path_mtu_profile,
            UtpPathMtuProfile::DynamicIpv4
        );
        assert!(left_terminal.selected_mtu_max_bytes.unwrap() >= 1_456);
        assert!(left_terminal.mtu_probes_acknowledged_high_water > 0);
        let left_udp_snapshot = left_udp.snapshot();
        assert!(left_udp_snapshot.protected_sends_sent > 0);
        assert!(left_udp_snapshot.maximum_datagram_bytes_sent >= 1_456);
        assert_eq!(left_udp_snapshot.fragmentation_restore_failures, 0);

        drop(left_dht);
        drop(right_dht);
        let left_udp_terminal = left_udp.shutdown().await.unwrap();
        let right_udp_terminal = right_udp.shutdown().await.unwrap();
        assert_eq!(left_udp_terminal.tasks, 0);
        assert_eq!(right_udp_terminal.tasks, 0);
        assert_eq!(left_udp_terminal.egress_waiters, 0);
        assert_eq!(right_udp_terminal.egress_waiters, 0);
    }

    async fn connected_pair() -> (
        SessionUdpService,
        crate::SessionUdpTransport,
        UtpService,
        UtpStream,
        SessionUdpService,
        crate::SessionUdpTransport,
        UtpService,
        UtpStream,
    ) {
        let (left_udp, left_dht, left_utp) = service().await;
        let (right_udp, right_dht, mut right_utp) = service().await;
        let right_address = right_udp.local_address();
        let left_handle = left_utp.handle();
        let (left_stream, right_stream) = timeout(Duration::from_secs(2), async {
            tokio::join!(left_handle.connect(right_address), right_utp.accept())
        })
        .await
        .expect("uTP connection timeout");
        (
            left_udp,
            left_dht,
            left_utp,
            left_stream.expect("outgoing uTP stream"),
            right_udp,
            right_dht,
            right_utp,
            right_stream.expect("incoming uTP stream"),
        )
    }

    async fn wait_for(mut predicate: impl FnMut() -> bool) {
        timeout(Duration::from_secs(2), async {
            while !predicate() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("condition timeout");
    }

    #[tokio::test]
    async fn loopback_stream_is_ordered_duplex_and_closes_gracefully() {
        let (left_udp, left_dht, left_utp, mut left, right_udp, right_dht, right_utp, mut right) =
            connected_pair().await;
        let left_payload = (0..65_731)
            .map(|index| u8::try_from(index % 251).unwrap())
            .collect::<Vec<_>>();
        let right_payload = (0..31_337)
            .map(|index| u8::try_from(index % 239).unwrap())
            .collect::<Vec<_>>();
        let exchange = timeout(Duration::from_secs(5), async {
            let left_exchange = async {
                left.write_all(&left_payload).await.unwrap();
                let mut received = vec![0; right_payload.len()];
                left.read_exact(&mut received).await.unwrap();
                assert_eq!(received, right_payload);
            };
            let right_exchange = async {
                right.write_all(&right_payload).await.unwrap();
                let mut received = vec![0; left_payload.len()];
                right.read_exact(&mut received).await.unwrap();
                assert_eq!(received, left_payload);
            };
            tokio::join!(left_exchange, right_exchange);
            let (left_close, right_close) = tokio::join!(left.shutdown(), right.shutdown());
            left_close.unwrap();
            right_close.unwrap();
        })
        .await;
        exchange.expect("duplex uTP exchange timeout");
        assert_eq!(left.read(&mut [0; 1]).await.unwrap(), 0);
        assert_eq!(right.read(&mut [0; 1]).await.unwrap(), 0);
        drop(left);
        drop(right);
        let left_terminal = left_utp.shutdown().await.unwrap();
        let right_terminal = right_utp.shutdown().await.unwrap();
        assert_eq!(left_terminal.active_connections, 0);
        assert_eq!(right_terminal.active_connections, 0);
        for terminal in [left_terminal, right_terminal] {
            assert_eq!(terminal.connections_started, 1);
            assert_eq!(terminal.selected_mtu_min_bytes, Some(548));
            assert_eq!(terminal.selected_mtu_max_bytes, Some(548));
            assert_eq!(terminal.mtu_candidate_min_bytes, Some(548));
            assert_eq!(terminal.mtu_candidate_max_bytes, Some(548));
            assert_eq!(terminal.mtu_probes_started_high_water, 0);
            assert_eq!(terminal.mtu_probe_datagrams_sent, 0);
            assert_eq!(terminal.mtu_fragmentable_retry_datagrams_sent, 0);
        }
        drop(left_dht);
        drop(right_dht);
        left_udp.shutdown().await.unwrap();
        right_udp.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn stream_try_api_is_bounded_and_observes_prequeued_readiness() {
        let local_address = SocketAddr::from((Ipv4Addr::LOCALHOST, 41000));
        let remote_address = SocketAddr::from((Ipv4Addr::LOCALHOST, 41001));
        let (mut stream, mut channels) = stream_pair(local_address, remote_address);
        let write = vec![7; MAX_UTP_APPLICATION_WRITE_BYTES + 1];
        assert_eq!(
            stream.try_write(&write).unwrap(),
            MAX_UTP_APPLICATION_WRITE_BYTES
        );
        assert_eq!(
            stream.try_write(&[8]).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(
            channels.writes.recv().await.unwrap(),
            vec![7; MAX_UTP_APPLICATION_WRITE_BYTES]
        );
        timeout(Duration::from_secs(1), stream.writable())
            .await
            .unwrap()
            .unwrap();

        channels
            .events
            .try_send(UtpStreamEvent::Data(vec![1, 2, 3]))
            .unwrap();
        channels
            .events
            .try_send(UtpStreamEvent::Data(vec![4, 5]))
            .unwrap();
        timeout(Duration::from_secs(1), stream.readable())
            .await
            .unwrap()
            .unwrap();
        let mut first = [0; 3];
        assert_eq!(stream.try_read(&mut first).unwrap(), 3);
        assert_eq!(first, [1, 2, 3]);
        timeout(Duration::from_secs(1), stream.readable())
            .await
            .unwrap()
            .unwrap();
        let mut second = [0; 2];
        assert_eq!(stream.try_read(&mut second).unwrap(), 2);
        assert_eq!(second, [4, 5]);
        assert_eq!(channels.consumption.take(), 5);
        channels.events.try_send(UtpStreamEvent::Eof).unwrap();
        timeout(Duration::from_secs(1), stream.readable())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stream.try_read(&mut [0; 1]).unwrap(), 0);
    }

    #[tokio::test]
    async fn connection_id_lookup_is_remote_scoped_and_duplicate_syn_reuses_worker() {
        let (udp, dht, mut utp) = service().await;
        let first_remote = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let second_remote = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let syn = packet(PacketType::Syn, 700, 800, 0, &[]);
        first_remote
            .send_to(&syn, udp.local_address())
            .await
            .unwrap();
        second_remote
            .send_to(&syn, udp.local_address())
            .await
            .unwrap();
        let first = timeout(Duration::from_secs(1), utp.accept())
            .await
            .unwrap()
            .unwrap();
        let second = timeout(Duration::from_secs(1), utp.accept())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            BTreeSet::from([first.peer_addr(), second.peer_addr()]),
            BTreeSet::from([
                first_remote.local_addr().unwrap(),
                second_remote.local_addr().unwrap(),
            ])
        );
        assert_eq!(utp.snapshot().active_connections, 2);

        first_remote
            .send_to(&syn, udp.local_address())
            .await
            .unwrap();
        assert!(
            timeout(Duration::from_millis(100), utp.accept())
                .await
                .is_err()
        );
        assert_eq!(utp.snapshot().active_connections, 2);
        drop(first);
        drop(second);
        wait_for(|| utp.snapshot().active_connections == 0).await;
        let terminal = utp.shutdown().await.unwrap();
        assert_eq!(terminal.connection_high_water, 2);
        assert_eq!(terminal.worker_panics, 0);
        drop(dht);
        udp.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn consumer_drop_and_service_cancellation_are_distinct_and_terminal() {
        let (left_udp, left_dht, left_utp, left, right_udp, right_dht, right_utp, mut right) =
            connected_pair().await;
        drop(left);
        wait_for(|| left_utp.snapshot().active_connections == 0).await;
        assert_eq!(right_utp.snapshot().active_connections, 1);
        let left_terminal = left_utp.shutdown().await.unwrap();
        assert_eq!(left_terminal.active_connections, 0);

        let right_terminal = right_utp.shutdown().await.unwrap();
        assert_eq!(right_terminal.active_connections, 0);
        let error = timeout(Duration::from_secs(1), right.read(&mut [0; 1]))
            .await
            .unwrap()
            .expect_err("service cancellation must fail the retained stream");
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        drop(right);
        drop(left_dht);
        drop(right_dht);
        left_udp.shutdown().await.unwrap();
        right_udp.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn malformed_and_unknown_packets_do_not_create_connections() {
        let (udp, dht, utp) = service().await;
        let remote = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let mut malformed = packet(PacketType::State, 40, 10, 9, &[]);
        malformed[1] = 1;
        remote
            .send_to(&malformed, udp.local_address())
            .await
            .unwrap();
        remote
            .send_to(
                &packet(PacketType::State, 41, 10, 9, &[]),
                udp.local_address(),
            )
            .await
            .unwrap();

        wait_for(|| {
            let snapshot = utp.snapshot();
            snapshot.malformed_datagrams == 1 && snapshot.unknown_connection_datagrams == 1
        })
        .await;
        assert_eq!(utp.snapshot().active_connections, 0);
        utp.shutdown().await.unwrap();
        drop(dht);
        udp.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn spoofed_state_and_reset_are_isolated_before_remote_reset() {
        let (udp, dht, utp) = service().await;
        let remote = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let remote_address = remote.local_addr().unwrap();
        let local_address = udp.local_address();
        let handle = utp.handle();
        let connect = tokio::spawn(async move { handle.connect(remote_address).await });
        let mut bytes = [0; UTP_RUNTIME_DATAGRAM_BYTES];
        let (length, source) = timeout(Duration::from_secs(1), remote.recv_from(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(source, local_address);
        let syn = decode_packet(&bytes[..length]).unwrap();
        assert_eq!(syn.header.packet_type, PacketType::Syn);
        remote
            .send_to(
                &packet(
                    PacketType::State,
                    syn.header.connection_id,
                    900,
                    syn.header.sequence_number.get(),
                    &[],
                ),
                local_address,
            )
            .await
            .unwrap();
        let mut stream = timeout(Duration::from_secs(1), connect)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let attacker = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        for packet_type in [PacketType::Reset, PacketType::State] {
            attacker
                .send_to(
                    &packet(
                        packet_type,
                        syn.header.connection_id,
                        901,
                        syn.header.sequence_number.get(),
                        &[],
                    ),
                    local_address,
                )
                .await
                .unwrap();
        }
        wait_for(|| utp.snapshot().unknown_connection_datagrams == 1).await;
        assert_eq!(utp.snapshot().active_connections, 1);
        assert_eq!(stream.peer_addr(), remote_address);

        remote
            .send_to(
                &packet(
                    PacketType::Reset,
                    syn.header.connection_id,
                    902,
                    syn.header.sequence_number.get(),
                    &[],
                ),
                local_address,
            )
            .await
            .unwrap();
        let error = timeout(Duration::from_secs(1), stream.read(&mut [0; 1]))
            .await
            .unwrap()
            .expect_err("RESET must fail stream read");
        assert_eq!(error.kind(), io::ErrorKind::ConnectionReset);
        drop(stream);
        utp.shutdown().await.unwrap();
        drop(dht);
        udp.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn socket_replacement_cancels_only_the_old_generation() {
        let (mut left_udp, left_dht, left_utp, mut left, right_udp, right_dht, right_utp, right) =
            connected_pair().await;
        let replacement = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        left_udp.replace_socket(replacement).await.unwrap();
        let error = timeout(Duration::from_secs(1), left.read(&mut [0; 1]))
            .await
            .unwrap()
            .expect_err("generation replacement must terminate stream");
        assert_eq!(error.kind(), io::ErrorKind::ConnectionAborted);
        wait_for(|| left_utp.snapshot().active_connections == 0).await;
        assert_eq!(right_utp.snapshot().active_connections, 1);
        drop(left);
        drop(right);
        left_utp.shutdown().await.unwrap();
        right_utp.shutdown().await.unwrap();
        drop(left_dht);
        drop(right_dht);
        left_udp.shutdown().await.unwrap();
        right_udp.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn socket_removal_cancels_the_generation_and_releases_ownership() {
        let (mut left_udp, left_dht, left_utp, mut left, right_udp, right_dht, right_utp, right) =
            connected_pair().await;
        left_udp.remove_family(AddressFamily::Ipv4).await.unwrap();
        let error = timeout(Duration::from_secs(1), left.read(&mut [0; 1]))
            .await
            .unwrap()
            .expect_err("family removal must terminate the stream");
        assert_eq!(error.kind(), io::ErrorKind::ConnectionAborted);
        wait_for(|| left_utp.snapshot().active_connections == 0).await;
        assert_eq!(right_utp.snapshot().active_connections, 1);
        drop(left);
        drop(right);
        left_utp.shutdown().await.unwrap();
        right_utp.shutdown().await.unwrap();
        drop(left_dht);
        drop(right_dht);
        left_udp.shutdown().await.unwrap();
        right_udp.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn incoming_stream_queue_saturation_drops_new_syns() {
        let (udp, dht, utp) = service().await;
        let remote = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        for index in 0..=UTP_INCOMING_STREAM_QUEUE {
            let syn = packet(
                PacketType::Syn,
                u16::try_from(100 + index).unwrap(),
                u16::try_from(200 + index).unwrap(),
                0,
                &[],
            );
            assert_eq!(
                remote.send_to(&syn, udp.local_address()).unwrap(),
                syn.len()
            );
        }
        wait_for(|| {
            let snapshot = utp.snapshot();
            snapshot.active_connections == UTP_INCOMING_STREAM_QUEUE
                && snapshot.connection_datagrams_dropped >= 1
        })
        .await;
        let snapshot = utp.snapshot();
        assert_eq!(
            snapshot.incoming_half_open_high_water,
            MAX_INCOMING_UTP_HALF_OPEN
        );
        assert_eq!(snapshot.incoming_half_open, 0);
        assert_eq!(
            snapshot.incoming_stream_queue_high_water,
            UTP_INCOMING_STREAM_QUEUE
        );
        assert!(snapshot.connection_high_water <= MAX_UTP_CONNECTIONS);
        utp.shutdown().await.unwrap();
        drop(dht);
        udp.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn global_connection_limit_rejects_one_more_and_releases_every_worker() {
        let (udp, dht, mut utp) = service().await;
        let remote = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let mut streams = Vec::with_capacity(MAX_UTP_CONNECTIONS);
        for index in 0..MAX_UTP_CONNECTIONS {
            remote
                .send_to(
                    &packet(
                        PacketType::Syn,
                        u16::try_from(1_000 + index).unwrap(),
                        u16::try_from(2_000 + index).unwrap(),
                        0,
                        &[],
                    ),
                    udp.local_address(),
                )
                .await
                .unwrap();
            streams.push(
                timeout(Duration::from_secs(1), utp.accept())
                    .await
                    .unwrap()
                    .unwrap(),
            );
        }
        assert_eq!(utp.snapshot().active_connections, MAX_UTP_CONNECTIONS);

        remote
            .send_to(
                &packet(PacketType::Syn, 9_000, 10_000, 0, &[]),
                udp.local_address(),
            )
            .await
            .unwrap();
        wait_for(|| utp.snapshot().connection_datagrams_dropped >= 1).await;
        assert!(
            timeout(Duration::from_millis(100), utp.accept())
                .await
                .is_err()
        );
        let saturated = utp.snapshot();
        assert_eq!(saturated.active_connections, MAX_UTP_CONNECTIONS);
        assert_eq!(saturated.connection_high_water, MAX_UTP_CONNECTIONS);
        assert_eq!(saturated.incoming_half_open, 0);

        drop(streams);
        wait_for(|| utp.snapshot().active_connections == 0).await;
        let terminal = utp.shutdown().await.unwrap();
        assert_eq!(terminal.active_connections, 0);
        assert_eq!(terminal.incoming_half_open, 0);
        assert_eq!(terminal.worker_panics, 0);
        drop(dht);
        udp.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn consumer_drop_during_retransmission_joins_with_zero_ownership() {
        let (udp, dht, utp) = service().await;
        let remote = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let remote_address = remote.local_addr().unwrap();
        let local_address = udp.local_address();
        let handle = utp.handle();
        let connect = tokio::spawn(async move { handle.connect(remote_address).await });
        let mut bytes = [0; UTP_RUNTIME_DATAGRAM_BYTES];
        let (length, _) = timeout(Duration::from_secs(1), remote.recv_from(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        let syn = decode_packet(&bytes[..length]).unwrap();
        remote
            .send_to(
                &packet(
                    PacketType::State,
                    syn.header.connection_id,
                    900,
                    syn.header.sequence_number.get(),
                    &[],
                ),
                local_address,
            )
            .await
            .unwrap();
        let mut stream = timeout(Duration::from_secs(1), connect)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        stream.write_all(&vec![7; 528]).await.unwrap();
        let (length, _) = timeout(Duration::from_secs(1), remote.recv_from(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            decode_packet(&bytes[..length]).unwrap().header.packet_type,
            PacketType::Data
        );
        wait_for(|| utp.snapshot().retransmission_datagrams_sent >= 1).await;

        drop(stream);
        wait_for(|| utp.snapshot().active_connections == 0).await;
        let terminal = utp.shutdown().await.unwrap();
        assert!(terminal.retransmission_datagrams_sent >= 1);
        assert_eq!(terminal.active_connections, 0);
        assert_eq!(terminal.incoming_half_open, 0);
        assert_eq!(terminal.worker_panics, 0);
        drop(dht);
        udp.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn connect_timeout_joins_worker_before_returning() {
        let (udp, dht, utp) = service().await;
        let remote = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let error = utp
            .handle()
            .connect_with_timeout(remote.local_addr().unwrap(), Duration::from_millis(25))
            .await
            .expect_err("unanswered connect must time out");
        assert!(matches!(error, UtpRuntimeError::ConnectTimedOut(_)));
        assert_eq!(utp.snapshot().active_connections, 0);
        assert!(utp.snapshot().datagrams_sent > 0);

        let terminal = utp.shutdown().await.unwrap();
        assert_eq!(terminal.active_connections, 0);
        drop(dht);
        udp.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn dropped_connect_future_cancels_worker() {
        let (udp, dht, utp) = service().await;
        let remote = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let handle = utp.handle();
        let connect =
            tokio::spawn(async move { handle.connect(remote.local_addr().unwrap()).await });
        wait_for(|| utp.snapshot().active_connections == 1).await;

        connect.abort();
        assert!(
            connect
                .await
                .expect_err("connect task must cancel")
                .is_cancelled()
        );
        wait_for(|| utp.snapshot().active_connections == 0).await;

        let terminal = utp.shutdown().await.unwrap();
        assert_eq!(terminal.active_connections, 0);
        drop(dht);
        udp.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn repeated_service_start_stop_releases_every_owner() {
        for _ in 0..8 {
            let (udp, dht, utp) = service().await;
            let terminal = utp.shutdown().await.unwrap();
            assert_eq!(terminal.active_connections, 0);
            assert_eq!(terminal.incoming_half_open, 0);
            assert_eq!(terminal.worker_panics, 0);
            drop(dht);
            udp.shutdown().await.unwrap();
        }
    }

    #[test]
    fn per_connection_datagram_queue_saturates_and_drops_only_new_work() {
        let (ingress, mut receiver) = mpsc::channel(UTP_CONNECTION_DATAGRAM_QUEUE);
        let route = UtpRoute {
            ingress,
            cancellation: CancellationToken::new(),
        };
        let stats = UtpStats::default();
        for ordinal in 0..UTP_CONNECTION_DATAGRAM_QUEUE {
            route_existing_datagram(&route, vec![ordinal as u8], &stats);
        }
        route_existing_datagram(&route, vec![255], &stats);

        let snapshot = stats.snapshot();
        assert_eq!(
            snapshot.connection_datagram_queue_high_water,
            UTP_CONNECTION_DATAGRAM_QUEUE
        );
        assert_eq!(snapshot.connection_datagrams_dropped, 1);
        for ordinal in 0..UTP_CONNECTION_DATAGRAM_QUEUE {
            assert_eq!(receiver.try_recv().unwrap(), vec![ordinal as u8]);
        }
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
    }

    #[test]
    fn retry_exhaustion_and_worker_panic_are_classified_and_observed() {
        let terminal = classify_worker_error(UtpRuntimeError::Protocol(
            TransportError::Connection(ConnectionError::Send(SendError::TransmissionLimit {
                sequence_number: SequenceNumber::new(9),
                transmissions: 8,
                maximum: 8,
            })),
        ));
        assert!(matches!(terminal, WorkerTerminal::RetryExhausted));
        assert_eq!(terminal.stream_terminal().kind, io::ErrorKind::TimedOut);

        let key = UtpConnectionKey {
            family: AddressFamily::Ipv4,
            generation: 1,
            remote: SocketAddr::from((Ipv4Addr::LOCALHOST, 42000)),
            receive_connection_id: 12,
        };
        let (ingress, _) = mpsc::channel(1);
        let mut routes = BTreeMap::from([(
            key,
            UtpRoute {
                ingress,
                cancellation: CancellationToken::new(),
            },
        )]);
        let stats = UtpStats::default();
        handle_worker_join(
            Some(Ok((
                key,
                Ok(WorkerReport {
                    terminal: WorkerTerminal::RetryExhausted,
                }),
            ))),
            &mut routes,
            &stats,
        )
        .unwrap();
        assert!(routes.is_empty());
        assert_eq!(stats.snapshot().retry_exhausted_connections, 1);

        let (ingress, _) = mpsc::channel(1);
        routes.insert(
            key,
            UtpRoute {
                ingress,
                cancellation: CancellationToken::new(),
            },
        );
        let panic: WorkerPanic = Box::new("controlled worker panic");
        handle_worker_join(Some(Ok((key, Err(panic)))), &mut routes, &stats).unwrap();
        assert!(routes.is_empty());
        assert_eq!(stats.snapshot().worker_panics, 1);
    }
}
