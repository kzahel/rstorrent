//! Direction-neutral framed TCP peer I/O.

use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use rstorrent_protocol::peer_wire::{
    FrameDecoder, FrameError, HandshakeError, PeerMessage, encode_message,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{Instant, timeout, timeout_at};

use crate::metrics::{ByteMetric, ByteMetricSink};
use crate::network::NetworkPolicy;

pub(crate) const NETWORK_READ_LENGTH: usize = 16 * 1024;
pub(crate) const CLIENT_PEER_ID: [u8; 20] = *b"-RS0001-000000000000";

#[derive(Debug)]
pub(crate) struct PeerIo {
    pub(crate) stream: TcpStream,
    pub(crate) decoder: FrameDecoder,
    pub(crate) queued_messages: VecDeque<PeerMessage>,
    pub(crate) io_timeout: Duration,
    pub(crate) byte_metric_sink: Option<Arc<dyn ByteMetricSink>>,
}

impl PeerIo {
    pub(crate) fn new(
        stream: TcpStream,
        io_timeout: Duration,
        byte_metric_sink: Option<Arc<dyn ByteMetricSink>>,
    ) -> Self {
        Self {
            stream,
            decoder: FrameDecoder::new(),
            queued_messages: VecDeque::new(),
            io_timeout,
            byte_metric_sink,
        }
    }

    pub(crate) fn prepend_messages(&mut self, mut messages: VecDeque<PeerMessage>) {
        messages.append(&mut self.queued_messages);
        self.queued_messages = messages;
    }

    pub(crate) async fn next_message(&mut self) -> Result<PeerMessage, PeerIoError> {
        let deadline = Instant::now() + self.io_timeout;
        while self.queued_messages.is_empty() {
            let mut network_buffer = [0_u8; NETWORK_READ_LENGTH];
            let read = timeout_at(deadline, self.stream.read(&mut network_buffer))
                .await
                .map_err(|_| PeerIoError::TimedOut {
                    operation: "message read",
                    timeout: self.io_timeout,
                })?
                .map_err(|source| PeerIoError::Io {
                    operation: "read peer message",
                    source,
                })?;
            if read == 0 {
                return Err(PeerIoError::Closed);
            }
            record_bytes(
                self.byte_metric_sink.as_ref(),
                ByteMetric::PeerWireReceived,
                read,
            );
            self.queued_messages.extend(
                self.decoder
                    .push(&network_buffer[..read])
                    .map_err(PeerIoError::Frame)?,
            );
        }
        let message = self
            .queued_messages
            .pop_front()
            .expect("peer message queue is nonempty after receive loop");
        self.record_incoming_message(&message)?;
        Ok(message)
    }

    pub(crate) async fn send_message(&mut self, message: &PeerMessage) -> Result<(), PeerIoError> {
        let frame = encode_message(message).map_err(PeerIoError::Frame)?;
        timeout(self.io_timeout, self.stream.write_all(&frame))
            .await
            .map_err(|_| PeerIoError::TimedOut {
                operation: "message write",
                timeout: self.io_timeout,
            })?
            .map_err(|source| PeerIoError::Io {
                operation: "send peer message",
                source,
            })?;
        record_bytes(
            self.byte_metric_sink.as_ref(),
            ByteMetric::PeerWireSent,
            frame.len(),
        );
        let (payload_length, payload_metric) = match message {
            PeerMessage::Piece { block, .. } => (block.len(), Some(ByteMetric::PayloadUploaded)),
            PeerMessage::Extended { payload, .. } => {
                (payload.len(), Some(ByteMetric::MetadataPayloadSent))
            }
            _ => (0, None),
        };
        record_bytes(
            self.byte_metric_sink.as_ref(),
            ByteMetric::PeerProtocolSent,
            frame.len().saturating_sub(payload_length),
        );
        if let Some(metric) = payload_metric {
            record_bytes(self.byte_metric_sink.as_ref(), metric, payload_length);
        }
        Ok(())
    }

    pub(crate) fn record_incoming_message(&self, message: &PeerMessage) -> Result<(), PeerIoError> {
        let frame_length = encode_message(message).map_err(PeerIoError::Frame)?.len();
        let (payload_length, payload_metric) = match message {
            PeerMessage::Piece { block, .. } => (block.len(), None),
            PeerMessage::Extended { payload, .. } => {
                (payload.len(), Some(ByteMetric::MetadataPayloadReceived))
            }
            _ => (0, None),
        };
        record_bytes(
            self.byte_metric_sink.as_ref(),
            ByteMetric::PeerProtocolReceived,
            frame_length.saturating_sub(payload_length),
        );
        if let Some(metric) = payload_metric {
            record_bytes(self.byte_metric_sink.as_ref(), metric, payload_length);
        }
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) enum PeerIoError {
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

impl fmt::Display for PeerIoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("peer socket operation cancelled"),
            Self::NetworkPolicyDenied { address, policy } => write!(
                formatter,
                "outbound address {address} is denied by network policy {policy}"
            ),
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::TimedOut { operation, timeout } => write!(
                formatter,
                "peer {operation} timed out after {}s",
                timeout.as_secs()
            ),
            Self::Closed => formatter.write_str("peer closed the connection"),
            Self::Handshake(error) => write!(formatter, "peer handshake: {error}"),
            Self::Frame(error) => write!(formatter, "peer frame: {error}"),
        }
    }
}

impl Error for PeerIoError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Handshake(error) => Some(error),
            Self::Frame(error) => Some(error),
            _ => None,
        }
    }
}

pub(crate) fn record_bytes(
    sink: Option<&Arc<dyn ByteMetricSink>>,
    metric: ByteMetric,
    bytes: usize,
) {
    if let Some(sink) = sink
        && bytes != 0
    {
        sink.record(metric, bytes.try_into().unwrap_or(u64::MAX));
    }
}
