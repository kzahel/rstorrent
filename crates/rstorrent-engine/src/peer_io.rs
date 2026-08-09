//! Direction-neutral framed TCP peer I/O.

use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use rstorrent_protocol::mse::{MseCipherPair, MseHandshakeError, Rc4};
use rstorrent_protocol::peer_wire::{
    FrameDecoder, FrameError, HandshakeError, PeerMessage, encode_message,
};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::time::{Instant, timeout_at};

use crate::metrics::{ByteMetric, ByteMetricSink};
use crate::mse::MseDhWorkError;
use crate::network::NetworkPolicy;
use crate::peer::MseEndpointState;

pub(crate) const NETWORK_READ_LENGTH: usize = 16 * 1024;

#[derive(Debug)]
pub(crate) struct PeerIo {
    pub(crate) stream: TcpStream,
    pub(crate) decoder: FrameDecoder,
    pub(crate) queued_messages: VecDeque<PeerMessage>,
    queued_frames: VecDeque<QueuedFrame>,
    receive_cipher: Option<Rc4>,
    send_cipher: Option<Rc4>,
    pub(crate) io_timeout: Duration,
    pub(crate) byte_metric_sink: Option<Arc<dyn ByteMetricSink>>,
}

#[derive(Debug)]
struct QueuedFrame {
    bytes: Vec<u8>,
    written: usize,
    payload_length: usize,
    payload_metric: Option<ByteMetric>,
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
            queued_frames: VecDeque::new(),
            receive_cipher: None,
            send_cipher: None,
            io_timeout,
            byte_metric_sink,
        }
    }

    pub(crate) fn attach_ciphers(&mut self, ciphers: MseCipherPair) {
        let (send, receive) = ciphers.into_parts();
        self.send_cipher = Some(send);
        self.receive_cipher = Some(receive);
    }

    pub(crate) fn prepend_messages(&mut self, mut messages: VecDeque<PeerMessage>) {
        messages.append(&mut self.queued_messages);
        self.queued_messages = messages;
    }

    pub(crate) async fn next_message(&mut self) -> Result<PeerMessage, PeerIoError> {
        loop {
            if let Some(message) = self.next_message_or_send_ready(usize::MAX).await? {
                return Ok(message);
            }
        }
    }

    pub(crate) async fn next_message_or_send_ready(
        &mut self,
        send_watermark: usize,
    ) -> Result<Option<PeerMessage>, PeerIoError> {
        let deadline = Instant::now() + self.io_timeout;
        loop {
            if let Some(message) = self.queued_messages.pop_front() {
                self.record_incoming_message(&message)?;
                return Ok(Some(message));
            }
            let was_at_watermark = self.send_buffer_size() >= send_watermark;
            self.flush_queued_frames()?;
            if was_at_watermark && self.send_buffer_size() < send_watermark {
                return Ok(None);
            }
            let mut network_buffer = [0_u8; NETWORK_READ_LENGTH];
            let read = if self.queued_frames.is_empty() {
                timeout_at(deadline, self.stream.read(&mut network_buffer))
                    .await
                    .map_err(|_| PeerIoError::TimedOut {
                        operation: "message read",
                        timeout: self.io_timeout,
                    })?
                    .map_err(|source| PeerIoError::Io {
                        operation: "read peer message",
                        source,
                    })?
            } else {
                tokio::select! {
                    biased;
                    ready = self.stream.readable() => {
                        ready.map_err(|source| PeerIoError::Io {
                            operation: "wait for peer message",
                            source,
                        })?;
                        match self.stream.try_read(&mut network_buffer) {
                            Ok(read) => read,
                            Err(source) if source.kind() == io::ErrorKind::WouldBlock => continue,
                            Err(source) => return Err(PeerIoError::Io {
                                operation: "read peer message",
                                source,
                            }),
                        }
                    }
                    ready = self.stream.writable() => {
                        ready.map_err(|source| PeerIoError::Io {
                            operation: "wait to send peer message",
                            source,
                        })?;
                        self.flush_queued_frames()?;
                        if was_at_watermark && self.send_buffer_size() < send_watermark {
                            return Ok(None);
                        }
                        continue;
                    }
                    _ = tokio::time::sleep_until(deadline) => {
                        return Err(PeerIoError::TimedOut {
                            operation: "message read or write",
                            timeout: self.io_timeout,
                        });
                    }
                }
            };
            if read == 0 {
                return Err(PeerIoError::Closed);
            }
            let messages = self.decode_received(&mut network_buffer[..read])?;
            self.queued_messages.extend(messages);
        }
    }

    pub(crate) fn queue_message(&mut self, message: &PeerMessage) -> Result<(), PeerIoError> {
        let mut bytes = encode_message(message).map_err(PeerIoError::Frame)?;
        if let Some(cipher) = self.send_cipher.as_mut() {
            cipher.apply(&mut bytes);
        }
        let (payload_length, payload_metric) = message_payload_metric(message);
        self.queued_frames.push_back(QueuedFrame {
            bytes,
            written: 0,
            payload_length,
            payload_metric,
        });
        Ok(())
    }

    pub(crate) fn send_buffer_size(&self) -> usize {
        self.queued_frames
            .iter()
            .map(|frame| frame.bytes.len().saturating_sub(frame.written))
            .sum()
    }

    fn flush_queued_frames(&mut self) -> Result<(), PeerIoError> {
        while let Some(frame) = self.queued_frames.front_mut() {
            let start = frame.written;
            let (written, frame_length, payload_length, payload_metric) =
                match self.stream.try_write(&frame.bytes[start..]) {
                    Ok(0) => return Err(PeerIoError::Closed),
                    Ok(written) => {
                        frame.written += written;
                        (
                            written,
                            frame.bytes.len(),
                            frame.payload_length,
                            frame.payload_metric,
                        )
                    }
                    Err(source) if source.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                    Err(source) => {
                        return Err(PeerIoError::Io {
                            operation: "send peer message",
                            source,
                        });
                    }
                };
            let _ = record_sent_range(
                self.byte_metric_sink.as_ref(),
                frame_length,
                payload_length,
                payload_metric,
                start,
                written,
            );
            if frame.written != frame.bytes.len() {
                return Ok(());
            }
            self.queued_frames.pop_front();
        }
        Ok(())
    }

    pub(crate) async fn send_message(&mut self, message: &PeerMessage) -> Result<(), PeerIoError> {
        self.queue_message(message)?;
        let deadline = Instant::now() + self.io_timeout;
        while !self.queued_frames.is_empty() {
            self.flush_queued_frames()?;
            if self.queued_frames.is_empty() {
                break;
            }
            timeout_at(deadline, self.stream.writable())
                .await
                .map_err(|_| PeerIoError::TimedOut {
                    operation: "message write",
                    timeout: self.io_timeout,
                })?
                .map_err(|source| PeerIoError::Io {
                    operation: "wait to send peer message",
                    source,
                })?;
        }
        Ok(())
    }

    pub(crate) fn decode_received(
        &mut self,
        bytes: &mut [u8],
    ) -> Result<Vec<PeerMessage>, PeerIoError> {
        record_bytes(
            self.byte_metric_sink.as_ref(),
            ByteMetric::PeerWireReceived,
            bytes.len(),
        );
        if let Some(cipher) = self.receive_cipher.as_mut() {
            cipher.apply(bytes);
        }
        self.decoder.push(bytes).map_err(PeerIoError::Frame)
    }

    pub(crate) fn push_decrypted(&mut self, bytes: &[u8]) -> Result<(), PeerIoError> {
        self.queued_messages
            .extend(self.decoder.push(bytes).map_err(PeerIoError::Frame)?);
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

pub(crate) fn message_payload_metric(message: &PeerMessage) -> (usize, Option<ByteMetric>) {
    match message {
        PeerMessage::Piece { block, .. } => (block.len(), Some(ByteMetric::PayloadUploaded)),
        PeerMessage::Extended { payload, .. } => {
            (payload.len(), Some(ByteMetric::MetadataPayloadSent))
        }
        _ => (0, None),
    }
}

pub(crate) fn record_sent_range(
    sink: Option<&Arc<dyn ByteMetricSink>>,
    frame_length: usize,
    payload_length: usize,
    payload_metric: Option<ByteMetric>,
    start: usize,
    written: usize,
) -> usize {
    let end = start.saturating_add(written).min(frame_length);
    let payload_start = frame_length.saturating_sub(payload_length);
    let payload_written = end.saturating_sub(start.max(payload_start)).min(written);
    record_bytes(sink, ByteMetric::PeerWireSent, written);
    record_bytes(
        sink,
        ByteMetric::PeerProtocolSent,
        written.saturating_sub(payload_written),
    );
    if let Some(metric) = payload_metric {
        record_bytes(sink, metric, payload_written);
    }
    payload_written
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
    MseHandshake(MseHandshakeError),
    MseDh(MseDhWorkError),
    Entropy(getrandom::Error),
    MseEndpointUpdate {
        state: MseEndpointState,
        source: Box<PeerIoError>,
    },
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
            Self::MseHandshake(error) => write!(formatter, "MSE handshake: {error}"),
            Self::MseDh(error) => error.fmt(formatter),
            Self::Entropy(error) => write!(formatter, "MSE entropy: {error}"),
            Self::MseEndpointUpdate { source, .. } => source.fmt(formatter),
            Self::Frame(error) => write!(formatter, "peer frame: {error}"),
        }
    }
}

impl Error for PeerIoError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Handshake(error) => Some(error),
            Self::MseDh(error) => Some(error),
            Self::MseEndpointUpdate { source, .. } => Some(source),
            Self::Frame(error) => Some(error),
            Self::Cancelled
            | Self::NetworkPolicyDenied { .. }
            | Self::TimedOut { .. }
            | Self::Closed
            | Self::MseHandshake(_)
            | Self::Entropy(_) => None,
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use rstorrent_protocol::mse::{
        DhPrivateExponent, MseCipherPair, MseRole, compute_public_key, compute_shared_secret,
    };
    use rstorrent_protocol::peer_wire::{PeerMessage, encode_message};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    use super::{PeerIo, record_sent_range};
    use crate::metrics::{ByteMetric, ByteMetricSink};

    #[derive(Debug, Default)]
    struct RecordingSink {
        bytes: Mutex<BTreeMap<ByteMetric, u64>>,
    }

    impl ByteMetricSink for RecordingSink {
        fn record(&self, metric: ByteMetric, bytes: u64) {
            *self
                .bytes
                .lock()
                .expect("recording sink")
                .entry(metric)
                .or_default() += bytes;
        }
    }

    #[test]
    fn partial_writes_account_protocol_and_payload_at_the_exact_boundary() {
        let sink = Arc::new(RecordingSink::default());
        let metric_sink: Arc<dyn ByteMetricSink> = sink.clone();
        assert_eq!(
            record_sent_range(
                Some(&metric_sink),
                20,
                8,
                Some(ByteMetric::PayloadUploaded),
                0,
                10,
            ),
            0
        );
        assert_eq!(
            record_sent_range(
                Some(&metric_sink),
                20,
                8,
                Some(ByteMetric::PayloadUploaded),
                10,
                5,
            ),
            3
        );
        assert_eq!(
            record_sent_range(
                Some(&metric_sink),
                20,
                8,
                Some(ByteMetric::PayloadUploaded),
                15,
                5,
            ),
            5
        );
        let bytes = sink.bytes.lock().expect("recorded bytes");
        assert_eq!(bytes[&ByteMetric::PeerWireSent], 20);
        assert_eq!(bytes[&ByteMetric::PeerProtocolSent], 12);
        assert_eq!(bytes[&ByteMetric::PayloadUploaded], 8);
    }

    #[tokio::test]
    async fn encrypted_direct_send_preserves_queued_order_and_receive_decodes_in_place() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("address");
        let client = TcpStream::connect(address).await.expect("connect");
        let (mut server, _) = listener.accept().await.expect("accept");
        let (initiator_ciphers, mut responder_ciphers) = cipher_pairs();
        let mut io = PeerIo::new(client, Duration::from_secs(1), None);
        io.attach_ciphers(initiator_ciphers);

        let first = PeerMessage::Have(7);
        let second = PeerMessage::Interested;
        io.queue_message(&first).expect("queue first");
        io.send_message(&second).await.expect("send both");

        let first_wire = encode_message(&first).expect("first frame");
        let second_wire = encode_message(&second).expect("second frame");
        let mut encrypted = vec![0; first_wire.len() + second_wire.len()];
        server
            .read_exact(&mut encrypted)
            .await
            .expect("read frames");
        assert_ne!(
            encrypted,
            [first_wire.as_slice(), second_wire.as_slice()].concat()
        );
        responder_ciphers.apply_receive(&mut encrypted);
        assert_eq!(
            encrypted,
            [first_wire.as_slice(), second_wire.as_slice()].concat()
        );

        let response = PeerMessage::Unchoke;
        let mut response_wire = encode_message(&response).expect("response frame");
        responder_ciphers.apply_send(&mut response_wire);
        server
            .write_all(&response_wire)
            .await
            .expect("write response");
        assert_eq!(io.next_message().await.expect("decode response"), response);
    }

    fn cipher_pairs() -> (MseCipherPair, MseCipherPair) {
        let initiator_private = DhPrivateExponent::from_entropy([0x11; 20]);
        let responder_private = DhPrivateExponent::from_entropy([0x91; 20]);
        let initiator_public = compute_public_key(&initiator_private);
        let responder_public = compute_public_key(&responder_private);
        let initiator_shared =
            compute_shared_secret(&initiator_private, responder_public.as_bytes())
                .expect("initiator shared secret");
        let responder_shared =
            compute_shared_secret(&responder_private, initiator_public.as_bytes())
                .expect("responder shared secret");
        let info_hash = [0x44; 20];
        (
            MseCipherPair::new(MseRole::Initiator, &initiator_shared, &info_hash),
            MseCipherPair::new(MseRole::Responder, &responder_shared, &info_hash),
        )
    }
}
