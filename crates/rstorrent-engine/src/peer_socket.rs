//! One-generation peer socket ownership and bounded task messaging.

use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use rstorrent_protocol::peer_wire::{
    EXTENSION_PROTOCOL_RESERVED_BIT, EXTENSION_PROTOCOL_RESERVED_INDEX, FrameDecoder, FrameError,
    HANDSHAKE_LENGTH, Handshake, HandshakeError, PeerMessage, decode_handshake, encode_handshake,
    encode_handshake_with_reserved, encode_message,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::{JoinError, JoinHandle, JoinSet};
use tokio::time::{Instant, timeout, timeout_at};
use tokio_util::sync::CancellationToken;

use crate::network::{NetworkConfig, NetworkPolicy};
use crate::peer::{DialAttempt, DialAttemptId, PeerFailure};
use crate::peer_runtime::connection_id;
use crate::swarm::ConnectionId;
use crate::{ByteMetric, ByteMetricSink};

const CLIENT_PEER_ID: [u8; 20] = *b"-RS0001-000000000000";
const NETWORK_READ_LENGTH: usize = 16 * 1024;

pub(crate) const PEER_COMMAND_QUEUE: usize = 16;
pub(crate) const PEER_EVENT_QUEUE: usize = 64;

#[derive(Debug)]
pub(crate) struct PeerConnection {
    attempt: DialAttempt,
    stream: TcpStream,
    decoder: FrameDecoder,
    queued_messages: VecDeque<PeerMessage>,
    io_timeout: Duration,
    byte_metric_sink: Option<Arc<dyn ByteMetricSink>>,
}

impl PeerConnection {
    pub(crate) const fn attempt(&self) -> DialAttempt {
        self.attempt
    }

    pub(crate) const fn io_timeout(&self) -> Duration {
        self.io_timeout
    }

    pub(crate) fn prepend_messages(&mut self, mut messages: VecDeque<PeerMessage>) {
        messages.append(&mut self.queued_messages);
        self.queued_messages = messages;
    }

    #[cfg(test)]
    pub(crate) fn for_test(attempt: DialAttempt, stream: TcpStream, io_timeout: Duration) -> Self {
        Self {
            attempt,
            stream,
            decoder: FrameDecoder::new(),
            queued_messages: VecDeque::new(),
            io_timeout,
            byte_metric_sink: None,
        }
    }
}

#[derive(Debug)]
pub(crate) enum PeerSocketError {
    Cancelled,
    NetworkPolicyDenied {
        address: SocketAddr,
        policy: NetworkPolicy,
    },
    Io {
        operation: &'static str,
        source: io::Error,
    },
    TimedOut {
        operation: &'static str,
        timeout: Duration,
    },
    Closed,
    Handshake(HandshakeError),
    Frame(FrameError),
}

impl PeerSocketError {
    pub(crate) fn peer_failure(&self) -> PeerFailure {
        match self {
            Self::Cancelled
            | Self::NetworkPolicyDenied { .. }
            | Self::Io {
                operation: "connect to peer",
                ..
            }
            | Self::TimedOut {
                operation: "connect",
                ..
            } => PeerFailure::Connect,
            Self::TimedOut {
                operation: "handshake read" | "handshake write",
                ..
            }
            | Self::Handshake(_) => PeerFailure::Handshake,
            Self::Closed => PeerFailure::RemoteClosed,
            Self::Io { .. } | Self::TimedOut { .. } | Self::Frame(_) => PeerFailure::Protocol,
        }
    }
}

impl fmt::Display for PeerSocketError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("peer socket operation cancelled"),
            Self::NetworkPolicyDenied { address, policy } => {
                write!(
                    formatter,
                    "outbound address {address} is denied by network policy {policy}"
                )
            }
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::TimedOut { operation, timeout } => {
                write!(
                    formatter,
                    "peer {operation} timed out after {}s",
                    timeout.as_secs()
                )
            }
            Self::Closed => formatter.write_str("peer closed the connection"),
            Self::Handshake(error) => write!(formatter, "peer handshake: {error}"),
            Self::Frame(error) => write!(formatter, "peer frame: {error}"),
        }
    }
}

impl Error for PeerSocketError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Handshake(error) => Some(error),
            Self::Frame(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
pub(crate) async fn connect(
    attempt: DialAttempt,
    info_hash: [u8; 20],
    advertise_extensions: bool,
    network: NetworkConfig,
) -> Result<(PeerConnection, Handshake), PeerSocketError> {
    connect_with_progress(
        attempt,
        info_hash,
        advertise_extensions,
        network,
        None,
        None,
    )
    .await
}

async fn connect_with_progress(
    attempt: DialAttempt,
    info_hash: [u8; 20],
    advertise_extensions: bool,
    network: NetworkConfig,
    progress: Option<&mpsc::Sender<PeerDialProgress>>,
    byte_metric_sink: Option<Arc<dyn ByteMetricSink>>,
) -> Result<(PeerConnection, Handshake), PeerSocketError> {
    let address = attempt.endpoint().address();
    if !network.policy.allows(address) {
        return Err(PeerSocketError::NetworkPolicyDenied {
            address,
            policy: network.policy,
        });
    }
    let mut stream = timeout(network.peer_connect_timeout, TcpStream::connect(address))
        .await
        .map_err(|_| PeerSocketError::TimedOut {
            operation: "connect",
            timeout: network.peer_connect_timeout,
        })?
        .map_err(|source| PeerSocketError::Io {
            operation: "connect to peer",
            source,
        })?;
    if let Some(progress) = progress {
        let _ = progress.send(PeerDialProgress { attempt }).await;
    }
    let handshake = if advertise_extensions {
        let mut reserved = [0; 8];
        reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
        encode_handshake_with_reserved(info_hash, CLIENT_PEER_ID, reserved)
    } else {
        encode_handshake(info_hash, CLIENT_PEER_ID)
    };
    timeout(network.peer_io_timeout, stream.write_all(&handshake))
        .await
        .map_err(|_| PeerSocketError::TimedOut {
            operation: "handshake write",
            timeout: network.peer_io_timeout,
        })?
        .map_err(|source| PeerSocketError::Io {
            operation: "send peer handshake",
            source,
        })?;
    record_bytes(
        byte_metric_sink.as_ref(),
        ByteMetric::PeerWireSent,
        handshake.len(),
    );
    record_bytes(
        byte_metric_sink.as_ref(),
        ByteMetric::PeerProtocolSent,
        handshake.len(),
    );

    let mut handshake = [0_u8; HANDSHAKE_LENGTH];
    timeout(network.peer_io_timeout, stream.read_exact(&mut handshake))
        .await
        .map_err(|_| PeerSocketError::TimedOut {
            operation: "handshake read",
            timeout: network.peer_io_timeout,
        })?
        .map_err(|source| PeerSocketError::Io {
            operation: "read peer handshake",
            source,
        })?;
    record_bytes(
        byte_metric_sink.as_ref(),
        ByteMetric::PeerWireReceived,
        handshake.len(),
    );
    record_bytes(
        byte_metric_sink.as_ref(),
        ByteMetric::PeerProtocolReceived,
        handshake.len(),
    );
    let handshake = decode_handshake(&handshake, info_hash).map_err(PeerSocketError::Handshake)?;
    Ok((
        PeerConnection {
            attempt,
            stream,
            decoder: FrameDecoder::new(),
            queued_messages: VecDeque::new(),
            io_timeout: network.peer_io_timeout,
            byte_metric_sink,
        },
        handshake,
    ))
}

pub(crate) async fn next_message(
    peer: &mut PeerConnection,
) -> Result<PeerMessage, PeerSocketError> {
    let deadline = Instant::now() + peer.io_timeout;
    while peer.queued_messages.is_empty() {
        let mut network_buffer = [0_u8; NETWORK_READ_LENGTH];
        let read = timeout_at(deadline, peer.stream.read(&mut network_buffer))
            .await
            .map_err(|_| PeerSocketError::TimedOut {
                operation: "message read",
                timeout: peer.io_timeout,
            })?
            .map_err(|source| PeerSocketError::Io {
                operation: "read peer message",
                source,
            })?;
        if read == 0 {
            return Err(PeerSocketError::Closed);
        }
        record_bytes(
            peer.byte_metric_sink.as_ref(),
            ByteMetric::PeerWireReceived,
            read,
        );
        peer.queued_messages.extend(
            peer.decoder
                .push(&network_buffer[..read])
                .map_err(PeerSocketError::Frame)?,
        );
    }
    let message = peer
        .queued_messages
        .pop_front()
        .expect("peer message queue is nonempty after receive loop");
    record_incoming_message(peer, &message)?;
    Ok(message)
}

fn record_incoming_message(
    peer: &PeerConnection,
    message: &PeerMessage,
) -> Result<(), PeerSocketError> {
    let frame_length = encode_message(message)
        .map_err(PeerSocketError::Frame)?
        .len();
    let (payload_length, payload_metric) = match &message {
        PeerMessage::Piece { block, .. } => (block.len(), None),
        PeerMessage::Extended { payload, .. } => {
            (payload.len(), Some(ByteMetric::MetadataPayloadReceived))
        }
        _ => (0, None),
    };
    record_bytes(
        peer.byte_metric_sink.as_ref(),
        ByteMetric::PeerProtocolReceived,
        frame_length.saturating_sub(payload_length),
    );
    if let Some(metric) = payload_metric {
        record_bytes(peer.byte_metric_sink.as_ref(), metric, payload_length);
    }
    Ok(())
}

pub(crate) async fn send_message(
    peer: &mut PeerConnection,
    message: &PeerMessage,
) -> Result<(), PeerSocketError> {
    let frame = encode_message(message).map_err(PeerSocketError::Frame)?;
    timeout(peer.io_timeout, peer.stream.write_all(&frame))
        .await
        .map_err(|_| PeerSocketError::TimedOut {
            operation: "message write",
            timeout: peer.io_timeout,
        })?
        .map_err(|source| PeerSocketError::Io {
            operation: "send peer message",
            source,
        })?;
    record_bytes(
        peer.byte_metric_sink.as_ref(),
        ByteMetric::PeerWireSent,
        frame.len(),
    );
    let (payload_length, payload_metric) = match message {
        PeerMessage::Piece { block, .. } => (block.len(), None),
        PeerMessage::Extended { payload, .. } => {
            (payload.len(), Some(ByteMetric::MetadataPayloadSent))
        }
        _ => (0, None),
    };
    record_bytes(
        peer.byte_metric_sink.as_ref(),
        ByteMetric::PeerProtocolSent,
        frame.len().saturating_sub(payload_length),
    );
    if let Some(metric) = payload_metric {
        record_bytes(peer.byte_metric_sink.as_ref(), metric, payload_length);
    }
    Ok(())
}

fn record_bytes(sink: Option<&Arc<dyn ByteMetricSink>>, metric: ByteMetric, bytes: usize) {
    if let Some(sink) = sink
        && bytes != 0
    {
        sink.record(metric, bytes.try_into().unwrap_or(u64::MAX));
    }
}

#[derive(Debug)]
pub(crate) enum PeerTaskCommand {
    Send(PeerMessage),
}

#[derive(Debug)]
pub(crate) enum PeerTaskEvent {
    Message {
        attempt: DialAttempt,
        message: PeerMessage,
    },
    Stopped {
        attempt: DialAttempt,
        result: Result<(), PeerSocketError>,
    },
}

#[derive(Debug)]
pub(crate) struct PeerSocketTask {
    attempt: DialAttempt,
    commands: mpsc::Sender<PeerTaskCommand>,
    cancellation: CancellationToken,
    join: JoinHandle<()>,
}

impl PeerSocketTask {
    pub(crate) fn spawn(connection: PeerConnection, events: mpsc::Sender<PeerTaskEvent>) -> Self {
        let attempt = connection.attempt;
        let (commands, command_rx) = mpsc::channel(PEER_COMMAND_QUEUE);
        let cancellation = CancellationToken::new();
        let task_cancellation = cancellation.clone();
        let join = tokio::spawn(async move {
            let result =
                run_peer_task(connection, command_rx, events.clone(), &task_cancellation).await;
            let stopped = PeerTaskEvent::Stopped { attempt, result };
            tokio::select! {
                biased;
                _ = task_cancellation.cancelled() => {}
                _ = events.send(stopped) => {}
            }
        });
        Self {
            attempt,
            commands,
            cancellation,
            join,
        }
    }

    pub(crate) const fn attempt(&self) -> DialAttempt {
        self.attempt
    }

    pub(crate) async fn send(&self, message: PeerMessage) -> Result<(), PeerTaskSendError> {
        self.commands
            .send(PeerTaskCommand::Send(message))
            .await
            .map_err(|_| PeerTaskSendError)
    }

    pub(crate) async fn shutdown(self) -> Result<(), JoinError> {
        self.cancellation.cancel();
        self.join.await
    }

    fn cancel(&self) {
        self.cancellation.cancel();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PeerTaskSendError;

impl fmt::Display for PeerTaskSendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("peer socket task stopped before accepting command")
    }
}

impl Error for PeerTaskSendError {}

#[derive(Debug)]
pub(crate) enum PeerSetEvent {
    DialPhase {
        attempt: DialAttempt,
    },
    DialCompleted {
        attempt: DialAttempt,
        result: ConnectedPeerResult,
    },
    Peer(PeerTaskEvent),
}

type ConnectedPeerResult = Result<(PeerConnection, Handshake), PeerSocketError>;
type PendingDialResult = (DialAttempt, ConnectedPeerResult);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PeerDialProgress {
    attempt: DialAttempt,
}

#[derive(Debug)]
pub(crate) enum PeerSetError {
    DuplicateDial(DialAttemptId),
    DuplicateConnection(ConnectionId),
    UnknownConnection(ConnectionId),
    EventQueueClosed,
    TaskJoin(JoinError),
}

impl fmt::Display for PeerSetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateDial(id) => write!(formatter, "duplicate pending dial {id}"),
            Self::DuplicateConnection(id) => {
                write!(formatter, "duplicate peer connection {}", id.get())
            }
            Self::UnknownConnection(id) => {
                write!(formatter, "unknown peer connection {}", id.get())
            }
            Self::EventQueueClosed => formatter.write_str("peer event queue closed"),
            Self::TaskJoin(error) => write!(formatter, "peer task join: {error}"),
        }
    }
}

impl Error for PeerSetError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::TaskJoin(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub(crate) struct PeerSocketSet {
    events_tx: mpsc::Sender<PeerTaskEvent>,
    events_rx: mpsc::Receiver<PeerTaskEvent>,
    tasks: BTreeMap<ConnectionId, PeerSocketTask>,
    pending: JoinSet<PendingDialResult>,
    pending_attempts: BTreeMap<DialAttemptId, (DialAttempt, CancellationToken)>,
    dial_progress_tx: mpsc::Sender<PeerDialProgress>,
    dial_progress_rx: mpsc::Receiver<PeerDialProgress>,
}

impl PeerSocketSet {
    pub(crate) fn new() -> Self {
        let (events_tx, events_rx) = mpsc::channel(PEER_EVENT_QUEUE);
        let (dial_progress_tx, dial_progress_rx) = mpsc::channel(PEER_EVENT_QUEUE);
        Self {
            events_tx,
            events_rx,
            tasks: BTreeMap::new(),
            pending: JoinSet::new(),
            pending_attempts: BTreeMap::new(),
            dial_progress_tx,
            dial_progress_rx,
        }
    }

    pub(crate) fn established_len(&self) -> usize {
        self.tasks.len()
    }

    pub(crate) fn pending_len(&self) -> usize {
        self.pending_attempts.len()
    }

    pub(crate) fn pending_attempts(&self) -> Vec<DialAttempt> {
        self.pending_attempts
            .values()
            .map(|(attempt, _)| *attempt)
            .collect()
    }

    pub(crate) fn connection_attempts(&self) -> Vec<DialAttempt> {
        self.tasks.values().map(PeerSocketTask::attempt).collect()
    }

    pub(crate) fn contains(&self, id: ConnectionId) -> bool {
        self.tasks.contains_key(&id)
    }

    pub(crate) fn attempt(&self, id: ConnectionId) -> Option<DialAttempt> {
        self.tasks.get(&id).map(PeerSocketTask::attempt)
    }

    pub(crate) fn begin_dial(
        &mut self,
        attempt: DialAttempt,
        info_hash: [u8; 20],
        advertise_extensions: bool,
        network: NetworkConfig,
        byte_metric_sink: Option<Arc<dyn ByteMetricSink>>,
    ) -> Result<(), PeerSetError> {
        if self.pending_attempts.contains_key(&attempt.id()) {
            return Err(PeerSetError::DuplicateDial(attempt.id()));
        }
        let cancellation = CancellationToken::new();
        let progress = self.dial_progress_tx.clone();
        self.pending_attempts
            .insert(attempt.id(), (attempt, cancellation.clone()));
        self.pending.spawn(async move {
            let result = tokio::select! {
                biased;
                _ = cancellation.cancelled() => Err(PeerSocketError::Cancelled),
                result = connect_with_progress(
                    attempt,
                    info_hash,
                    advertise_extensions,
                    network,
                    Some(&progress),
                    byte_metric_sink,
                ) => result,
            };
            (attempt, result)
        });
        Ok(())
    }

    pub(crate) fn add_connection(
        &mut self,
        connection: PeerConnection,
    ) -> Result<ConnectionId, PeerSetError> {
        let id = connection_id(connection.attempt);
        if self.tasks.contains_key(&id) {
            return Err(PeerSetError::DuplicateConnection(id));
        }
        self.tasks.insert(
            id,
            PeerSocketTask::spawn(connection, self.events_tx.clone()),
        );
        Ok(id)
    }

    pub(crate) async fn send(
        &self,
        id: ConnectionId,
        message: PeerMessage,
    ) -> Result<(), PeerSetError> {
        self.tasks
            .get(&id)
            .ok_or(PeerSetError::UnknownConnection(id))?
            .send(message)
            .await
            .map_err(|_| PeerSetError::UnknownConnection(id))
    }

    pub(crate) async fn next_event(&mut self) -> Result<PeerSetEvent, PeerSetError> {
        if self.pending_attempts.is_empty() {
            return self
                .events_rx
                .recv()
                .await
                .map(PeerSetEvent::Peer)
                .ok_or(PeerSetError::EventQueueClosed);
        }
        tokio::select! {
            progress = self.dial_progress_rx.recv() => {
                let progress = progress.ok_or(PeerSetError::EventQueueClosed)?;
                Ok(PeerSetEvent::DialPhase {
                    attempt: progress.attempt,
                })
            }
            event = self.events_rx.recv() => event
                .map(PeerSetEvent::Peer)
                .ok_or(PeerSetError::EventQueueClosed),
            joined = self.pending.join_next() => {
                let (attempt, result) = joined
                    .expect("pending dial set is nonempty")
                    .map_err(PeerSetError::TaskJoin)?;
                self.pending_attempts.remove(&attempt.id());
                Ok(PeerSetEvent::DialCompleted { attempt, result })
            }
        }
    }

    pub(crate) async fn remove_connection(
        &mut self,
        id: ConnectionId,
    ) -> Result<DialAttempt, PeerSetError> {
        let task = self
            .tasks
            .remove(&id)
            .ok_or(PeerSetError::UnknownConnection(id))?;
        let attempt = task.attempt();
        task.shutdown().await.map_err(PeerSetError::TaskJoin)?;
        Ok(attempt)
    }

    pub(crate) async fn shutdown(mut self) -> Result<Vec<DialAttempt>, PeerSetError> {
        for task in self.tasks.values() {
            task.cancel();
        }
        for (_, task) in self.tasks {
            task.join.await.map_err(PeerSetError::TaskJoin)?;
        }
        let pending = self
            .pending_attempts
            .values()
            .map(|(attempt, _)| *attempt)
            .collect::<Vec<_>>();
        for (_, cancellation) in self.pending_attempts.into_values() {
            cancellation.cancel();
        }
        while let Some(joined) = self.pending.join_next().await {
            drop(joined.map_err(PeerSetError::TaskJoin)?);
        }
        Ok(pending)
    }
}

impl Default for PeerSocketSet {
    fn default() -> Self {
        Self::new()
    }
}

async fn run_peer_task(
    mut peer: PeerConnection,
    mut commands: mpsc::Receiver<PeerTaskCommand>,
    events: mpsc::Sender<PeerTaskEvent>,
    cancellation: &CancellationToken,
) -> Result<(), PeerSocketError> {
    let mut pending_messages = std::mem::take(&mut peer.queued_messages);
    let mut read_deadline = Instant::now() + peer.io_timeout;
    let mut network_buffer = [0_u8; NETWORK_READ_LENGTH];
    loop {
        if !pending_messages.is_empty() {
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Ok(()),
                permit = events.reserve() => {
                    let permit = permit.map_err(|_| PeerSocketError::Io {
                        operation: "deliver peer event",
                        source: io::Error::new(
                            io::ErrorKind::BrokenPipe,
                            "torrent supervisor stopped",
                        ),
                    })?;
                    let message = pending_messages
                        .pop_front()
                        .expect("pending peer message queue is nonempty");
                    permit.send(PeerTaskEvent::Message {
                        attempt: peer.attempt,
                        message,
                    });
                }
                command = commands.recv() => match command {
                    Some(PeerTaskCommand::Send(message)) => {
                        send_message(&mut peer, &message).await?;
                    }
                    None => return Ok(()),
                },
            }
            continue;
        }
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Ok(()),
            command = commands.recv() => match command {
                Some(PeerTaskCommand::Send(message)) => send_message(&mut peer, &message).await?,
                None => return Ok(()),
            },
            read = timeout_at(read_deadline, peer.stream.read(&mut network_buffer)) => {
                let read = read
                    .map_err(|_| PeerSocketError::TimedOut {
                        operation: "message read",
                        timeout: peer.io_timeout,
                    })?
                    .map_err(|source| PeerSocketError::Io {
                        operation: "read peer message",
                        source,
                    })?;
                if read == 0 {
                    return Err(PeerSocketError::Closed);
                }
                record_bytes(
                    peer.byte_metric_sink.as_ref(),
                    ByteMetric::PeerWireReceived,
                    read,
                );
                let messages = peer.decoder
                    .push(&network_buffer[..read])
                    .map_err(PeerSocketError::Frame)?;
                for message in &messages {
                    record_incoming_message(&peer, message)?;
                }
                if !messages.is_empty() {
                    read_deadline = Instant::now() + peer.io_timeout;
                }
                pending_messages.extend(messages);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rstorrent_protocol::peer_wire::{
        HANDSHAKE_LENGTH, PeerMessage, encode_handshake, encode_message,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::{mpsc, oneshot};
    use tokio::time::timeout;

    use super::{
        PEER_COMMAND_QUEUE, PeerConnection, PeerSetEvent, PeerSocketError, PeerSocketSet,
        PeerSocketTask, PeerTaskEvent,
    };
    use crate::network::{NetworkConfig, NetworkPolicy};
    use crate::peer::{
        DialAttempt, PeerEndpoint, PeerObservation, PeerRegistry, PeerRegistryConfig,
        PeerSelectionContext, PeerSelector, PeerSource,
    };

    fn test_attempt_for(address: std::net::SocketAddr) -> DialAttempt {
        let endpoint = PeerEndpoint::new(address).expect("valid endpoint");
        let mut registry = PeerRegistry::new(PeerRegistryConfig::default()).expect("registry");
        registry
            .observe(
                PeerObservation::dialable(endpoint, PeerSource::Manual),
                Duration::ZERO,
            )
            .expect("observation");
        let context = PeerSelectionContext {
            now: Duration::ZERO,
        };
        let candidate = PeerSelector.select(&registry, context).expect("candidate");
        registry.begin_dial(candidate, context).expect("attempt")
    }

    fn test_attempt() -> DialAttempt {
        test_attempt_for("127.0.0.1:6881".parse().expect("test address"))
    }

    async fn connected_pair(io_timeout: Duration) -> (PeerConnection, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("address");
        let client = TcpStream::connect(address).await.expect("connect");
        let (server, _) = listener.accept().await.expect("accept");
        (
            PeerConnection::for_test(test_attempt(), client, io_timeout),
            server,
        )
    }

    #[tokio::test]
    async fn socket_set_reports_transport_before_handshake_completion() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("address");
        let attempt = test_attempt_for(address);
        let info_hash = [7; 20];
        let (release_tx, release_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let mut request = [0; HANDSHAKE_LENGTH];
            stream
                .read_exact(&mut request)
                .await
                .expect("read handshake");
            release_rx.await.expect("release handshake");
            stream
                .write_all(&encode_handshake(info_hash, [8; 20]))
                .await
                .expect("write handshake");
        });

        let mut sockets = PeerSocketSet::new();
        sockets
            .begin_dial(
                attempt,
                info_hash,
                true,
                NetworkConfig::new(
                    NetworkPolicy::LoopbackOnly,
                    Duration::from_secs(1),
                    Duration::from_secs(1),
                ),
                None,
            )
            .expect("begin dial");
        assert!(matches!(
            timeout(Duration::from_secs(1), sockets.next_event())
                .await
                .expect("transport phase deadline")
                .expect("transport phase"),
            PeerSetEvent::DialPhase { attempt: actual } if actual == attempt
        ));

        release_tx.send(()).expect("release server");
        let connection = match timeout(Duration::from_secs(1), sockets.next_event())
            .await
            .expect("handshake deadline")
            .expect("handshake event")
        {
            PeerSetEvent::DialCompleted {
                attempt: actual,
                result: Ok((connection, _)),
            } => {
                assert_eq!(actual, attempt);
                connection
            }
            event => panic!("unexpected event {event:?}"),
        };
        sockets.add_connection(connection).expect("own connection");
        assert!(sockets.shutdown().await.expect("shutdown").is_empty());
        server.await.expect("server task");
    }

    #[tokio::test]
    async fn task_routes_bounded_commands_and_generation_tagged_events() {
        let (connection, mut server) = connected_pair(Duration::from_secs(1)).await;
        let attempt = connection.attempt();
        let (event_tx, mut events) = mpsc::channel(4);
        let task = PeerSocketTask::spawn(connection, event_tx);
        assert_eq!(task.attempt(), attempt);

        task.send(PeerMessage::Interested)
            .await
            .expect("send command");
        let expected = encode_message(&PeerMessage::Interested).expect("frame");
        let mut received = vec![0; expected.len()];
        server
            .read_exact(&mut received)
            .await
            .expect("read command");
        assert_eq!(received, expected);

        server
            .write_all(&encode_message(&PeerMessage::Unchoke).expect("frame"))
            .await
            .expect("write event");
        match events.recv().await.expect("message event") {
            PeerTaskEvent::Message {
                attempt: actual,
                message: PeerMessage::Unchoke,
            } => assert_eq!(actual, attempt),
            event => panic!("unexpected event {event:?}"),
        }
        task.shutdown().await.expect("join task");
    }

    #[tokio::test]
    async fn cancellation_joins_when_the_event_queue_is_saturated() {
        let (connection, mut server) = connected_pair(Duration::from_secs(1)).await;
        let (event_tx, _events) = mpsc::channel(1);
        let task = PeerSocketTask::spawn(connection, event_tx);
        let mut frames = Vec::new();
        for _ in 0..3 {
            frames.extend(encode_message(&PeerMessage::KeepAlive).expect("frame"));
        }
        server.write_all(&frames).await.expect("fill event queue");
        tokio::task::yield_now().await;
        timeout(Duration::from_millis(200), task.shutdown())
            .await
            .expect("bounded shutdown")
            .expect("join task");
    }

    #[tokio::test]
    async fn outbound_commands_drain_while_inbound_event_delivery_is_backpressured() {
        let (connection, mut server) = connected_pair(Duration::from_secs(1)).await;
        let (event_tx, mut events) = mpsc::channel(1);
        let task = PeerSocketTask::spawn(connection, event_tx);
        let keepalive = encode_message(&PeerMessage::KeepAlive).expect("keepalive");
        let mut inbound = Vec::new();
        for _ in 0..3 {
            inbound.extend_from_slice(&keepalive);
        }
        server
            .write_all(&inbound)
            .await
            .expect("saturate inbound event delivery");
        assert!(matches!(
            timeout(Duration::from_millis(200), events.recv())
                .await
                .expect("first inbound event")
                .expect("event channel"),
            PeerTaskEvent::Message {
                message: PeerMessage::KeepAlive,
                ..
            }
        ));
        tokio::task::yield_now().await;

        timeout(Duration::from_millis(200), async {
            for _ in 0..=PEER_COMMAND_QUEUE {
                task.send(PeerMessage::Interested)
                    .await
                    .expect("bounded outbound command");
            }
        })
        .await
        .expect("event backpressure must not block outbound commands");

        let interested = encode_message(&PeerMessage::Interested).expect("interested");
        let mut outbound = vec![0; interested.len() * (PEER_COMMAND_QUEUE + 1)];
        timeout(Duration::from_millis(200), server.read_exact(&mut outbound))
            .await
            .expect("outbound commands reached socket")
            .expect("read outbound commands");
        assert!(
            outbound
                .chunks_exact(interested.len())
                .all(|frame| frame == interested)
        );
        for _ in 0..2 {
            assert!(matches!(
                timeout(Duration::from_millis(200), events.recv())
                    .await
                    .expect("pending inbound event")
                    .expect("event channel"),
                PeerTaskEvent::Message {
                    message: PeerMessage::KeepAlive,
                    ..
                }
            ));
        }
        task.shutdown().await.expect("join task");
    }

    #[tokio::test]
    async fn fragmented_input_cannot_refresh_the_task_message_deadline() {
        let (connection, mut server) = connected_pair(Duration::from_millis(50)).await;
        let attempt = connection.attempt();
        let (event_tx, mut events) = mpsc::channel(4);
        let task = PeerSocketTask::spawn(connection, event_tx);
        let frame = encode_message(&PeerMessage::KeepAlive).expect("frame");
        let writer = tokio::spawn(async move {
            for byte in frame {
                if server.write_all(&[byte]).await.is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        });
        match timeout(Duration::from_millis(300), events.recv())
            .await
            .expect("task deadline")
            .expect("stopped event")
        {
            PeerTaskEvent::Stopped {
                attempt: actual,
                result:
                    Err(PeerSocketError::TimedOut {
                        operation: "message read",
                        ..
                    }),
            } => assert_eq!(actual, attempt),
            event => panic!("unexpected event {event:?}"),
        }
        writer.await.expect("writer join");
        task.shutdown().await.expect("task join");
    }
}
