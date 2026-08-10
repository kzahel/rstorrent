//! Incoming peer reader plus joined, byte-charged writer ownership.

use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use rstorrent_protocol::mse::{MseCipherPair, Rc4};
use rstorrent_protocol::peer_wire::{FrameDecoder, PeerMessage, encode_message};
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout_at};
use tokio_util::sync::CancellationToken;

use crate::metrics::{ByteMetric, ByteMetricSink};
use crate::peer_io::{
    NETWORK_READ_LENGTH, PeerIoError, message_payload_metric, record_bytes, record_sent_range,
};
use crate::peer_stream::PeerStream;

pub(super) const MAX_INCOMING_WRITER_BYTES: usize = 528_396;
pub(super) const INCOMING_WRITER_NO_PROGRESS_TIMEOUT: Duration = Duration::from_secs(60);
const WRITER_FRAME_QUEUE: usize = 64;

#[derive(Debug)]
pub(super) struct FrameValidity {
    cancellation: CancellationToken,
}

impl FrameValidity {
    pub(super) fn new() -> Self {
        Self {
            cancellation: CancellationToken::new(),
        }
    }

    pub(super) fn cancel(&self) {
        self.cancellation.cancel();
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    async fn cancelled(&self) {
        self.cancellation.cancelled().await;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WriterSnapshot {
    queued_bytes: usize,
    queued_high_water: usize,
    uploaded_payload_bytes: u64,
    running: bool,
}

#[derive(Debug)]
struct WriterState {
    snapshot: WriterSnapshot,
}

impl WriterState {
    fn new() -> Self {
        Self {
            snapshot: WriterSnapshot {
                queued_bytes: 0,
                queued_high_water: 0,
                uploaded_payload_bytes: 0,
                running: true,
            },
        }
    }

    fn reserve(&mut self, bytes: usize) -> Result<(), PeerIoError> {
        let next = self
            .snapshot
            .queued_bytes
            .checked_add(bytes)
            .filter(|next| *next <= MAX_INCOMING_WRITER_BYTES)
            .ok_or_else(|| PeerIoError::Io {
                operation: "queue incoming peer frame",
                source: io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "incoming peer writer byte limit reached",
                ),
            })?;
        if !self.snapshot.running {
            return Err(PeerIoError::Closed);
        }
        self.snapshot.queued_bytes = next;
        self.snapshot.queued_high_water = self.snapshot.queued_high_water.max(next);
        Ok(())
    }

    fn release_written(&mut self, written: usize, uploaded: usize) {
        self.snapshot.queued_bytes = self.snapshot.queued_bytes.saturating_sub(written);
        self.snapshot.uploaded_payload_bytes = self
            .snapshot
            .uploaded_payload_bytes
            .saturating_add(uploaded.try_into().unwrap_or(u64::MAX));
    }

    fn release_frame(&mut self, remaining: usize) {
        self.snapshot.queued_bytes = self.snapshot.queued_bytes.saturating_sub(remaining);
    }

    fn stop(&mut self) {
        self.snapshot.running = false;
        self.snapshot.queued_bytes = 0;
    }
}

#[derive(Debug)]
struct WriterFrame {
    bytes: Vec<u8>,
    payload_length: usize,
    payload_metric: Option<ByteMetric>,
    validity: Option<Arc<FrameValidity>>,
}

#[derive(Debug)]
struct IncomingWriter {
    commands: Option<mpsc::Sender<WriterFrame>>,
    state: Arc<Mutex<WriterState>>,
    changes: watch::Receiver<WriterSnapshot>,
    cancellation: CancellationToken,
    task: Option<JoinHandle<Result<(), PeerIoError>>>,
}

impl IncomingWriter {
    fn spawn(
        write: WriteHalf<PeerStream>,
        send_cipher: Option<Rc4>,
        byte_metric_sink: Option<Arc<dyn ByteMetricSink>>,
    ) -> Self {
        let (commands, receiver) = mpsc::channel(WRITER_FRAME_QUEUE);
        let state = Arc::new(Mutex::new(WriterState::new()));
        let (changes, change_receiver) = watch::channel(state_guard(&state).snapshot);
        let cancellation = CancellationToken::new();
        let task = tokio::spawn(run_writer(
            write,
            receiver,
            state.clone(),
            changes,
            cancellation.clone(),
            send_cipher,
            byte_metric_sink,
        ));
        Self {
            commands: Some(commands),
            state,
            changes: change_receiver,
            cancellation,
            task: Some(task),
        }
    }

    fn queue(
        &mut self,
        message: &PeerMessage,
        validity: Option<Arc<FrameValidity>>,
    ) -> Result<(), PeerIoError> {
        let bytes = encode_message(message).map_err(PeerIoError::Frame)?;
        let (payload_length, payload_metric) = message_payload_metric(message);
        {
            let mut state = state_guard(&self.state);
            state.reserve(bytes.len())?;
        }
        let frame_length = bytes.len();
        let frame = WriterFrame {
            bytes,
            payload_length,
            payload_metric,
            validity,
        };
        let result = self
            .commands
            .as_ref()
            .ok_or(PeerIoError::Closed)?
            .try_send(frame);
        if let Err(error) = result {
            state_guard(&self.state).release_frame(frame_length);
            return Err(match error {
                mpsc::error::TrySendError::Full(_) => PeerIoError::Io {
                    operation: "queue incoming peer frame",
                    source: io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "incoming peer writer descriptor limit reached",
                    ),
                },
                mpsc::error::TrySendError::Closed(_) => PeerIoError::Closed,
            });
        }
        Ok(())
    }

    fn snapshot(&self) -> WriterSnapshot {
        state_guard(&self.state).snapshot
    }

    async fn changed(&mut self) -> Result<WriterSnapshot, PeerIoError> {
        self.changes
            .changed()
            .await
            .map_err(|_| PeerIoError::Closed)?;
        let snapshot = *self.changes.borrow_and_update();
        if snapshot.running {
            Ok(snapshot)
        } else {
            Err(PeerIoError::Closed)
        }
    }

    async fn flush(&mut self, timeout: Duration) -> Result<(), PeerIoError> {
        let deadline = Instant::now() + timeout;
        loop {
            let snapshot = self.snapshot();
            if snapshot.queued_bytes == 0 {
                return Ok(());
            }
            timeout_at(deadline, self.changed())
                .await
                .map_err(|_| PeerIoError::TimedOut {
                    operation: "message write",
                    timeout,
                })??;
        }
    }

    async fn shutdown(&mut self) -> Result<(), PeerIoError> {
        self.cancellation.cancel();
        self.commands.take();
        let Some(task) = self.task.take() else {
            return Ok(());
        };
        task.await.map_err(|error| PeerIoError::Io {
            operation: "join incoming peer writer",
            source: io::Error::other(error.to_string()),
        })?
    }
}

impl Drop for IncomingWriter {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

#[derive(Debug)]
pub(super) struct IncomingPeerIo {
    read: ReadHalf<PeerStream>,
    receive_cipher: Option<Rc4>,
    decoder: FrameDecoder,
    queued_messages: VecDeque<PeerMessage>,
    writer: IncomingWriter,
    io_timeout: Duration,
    byte_metric_sink: Option<Arc<dyn ByteMetricSink>>,
}

impl IncomingPeerIo {
    #[cfg(test)]
    pub fn new(
        stream: impl Into<PeerStream>,
        io_timeout: Duration,
        byte_metric_sink: Option<Arc<dyn ByteMetricSink>>,
    ) -> Self {
        Self::new_with_mse(stream, io_timeout, byte_metric_sink, None, &[])
            .expect("empty carried input is valid")
    }

    pub fn new_with_mse(
        stream: impl Into<PeerStream>,
        io_timeout: Duration,
        byte_metric_sink: Option<Arc<dyn ByteMetricSink>>,
        ciphers: Option<MseCipherPair>,
        carried: &[u8],
    ) -> Result<Self, PeerIoError> {
        let mut decoder = FrameDecoder::new();
        let queued_messages = decoder
            .push(carried)
            .map_err(PeerIoError::Frame)?
            .into_iter()
            .collect();
        let (send_cipher, receive_cipher) = match ciphers {
            Some(ciphers) => {
                let (send, receive) = ciphers.into_parts();
                (Some(send), Some(receive))
            }
            None => (None, None),
        };
        let (read, write) = tokio::io::split(stream.into());
        Ok(Self {
            read,
            receive_cipher,
            decoder,
            queued_messages,
            writer: IncomingWriter::spawn(write, send_cipher, byte_metric_sink.clone()),
            io_timeout,
            byte_metric_sink,
        })
    }

    pub async fn send_message(&mut self, message: &PeerMessage) -> Result<(), PeerIoError> {
        self.queue_message(message)?;
        self.writer.flush(self.io_timeout).await
    }

    pub async fn flush(&mut self) -> Result<(), PeerIoError> {
        self.writer.flush(self.io_timeout).await
    }

    pub fn queue_message(&mut self, message: &PeerMessage) -> Result<(), PeerIoError> {
        self.writer.queue(message, None)
    }

    pub fn queue_generation_fenced_message(
        &mut self,
        message: &PeerMessage,
        validity: Arc<FrameValidity>,
    ) -> Result<(), PeerIoError> {
        self.writer.queue(message, Some(validity))
    }

    pub fn send_buffer_size(&self) -> usize {
        self.writer.snapshot().queued_bytes
    }

    pub fn send_buffer_high_water(&self) -> usize {
        self.writer.snapshot().queued_high_water
    }

    pub fn uploaded_payload_bytes(&self) -> u64 {
        self.writer.snapshot().uploaded_payload_bytes
    }

    pub async fn next_message_or_send_ready(
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
            let mut network_buffer = [0_u8; NETWORK_READ_LENGTH];
            let read = tokio::select! {
                read = self.read.read(&mut network_buffer) => Some(read),
                changed = self.writer.changed() => {
                    let snapshot = changed?;
                    if was_at_watermark && snapshot.queued_bytes < send_watermark {
                        return Ok(None);
                    }
                    None
                }
                _ = tokio::time::sleep_until(deadline) => {
                    return Err(PeerIoError::TimedOut {
                        operation: "message read",
                        timeout: self.io_timeout,
                    });
                }
            };
            let Some(read) = read else {
                continue;
            };
            let read = read.map_err(|source| PeerIoError::Io {
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
            if let Some(cipher) = self.receive_cipher.as_mut() {
                cipher.apply(&mut network_buffer[..read]);
            }
            self.queued_messages.extend(
                self.decoder
                    .push(&network_buffer[..read])
                    .map_err(PeerIoError::Frame)?,
            );
        }
    }

    pub async fn shutdown(&mut self) -> Result<(), PeerIoError> {
        self.writer.shutdown().await
    }

    fn record_incoming_message(&self, message: &PeerMessage) -> Result<(), PeerIoError> {
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

async fn run_writer(
    mut write: WriteHalf<PeerStream>,
    mut commands: mpsc::Receiver<WriterFrame>,
    state: Arc<Mutex<WriterState>>,
    changes: watch::Sender<WriterSnapshot>,
    cancellation: CancellationToken,
    mut send_cipher: Option<Rc4>,
    byte_metric_sink: Option<Arc<dyn ByteMetricSink>>,
) -> Result<(), PeerIoError> {
    let result = async {
        loop {
            let frame = tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Ok(()),
                frame = commands.recv() => match frame {
                    Some(frame) => frame,
                    None => return Ok(()),
                },
            };
            write_frame(
                &mut write,
                frame,
                WriterCommitContext {
                    state: &state,
                    changes: &changes,
                    cancellation: &cancellation,
                    send_cipher: send_cipher.as_mut(),
                    byte_metric_sink: byte_metric_sink.as_ref(),
                    no_progress_timeout: INCOMING_WRITER_NO_PROGRESS_TIMEOUT,
                },
            )
            .await?;
        }
    }
    .await;
    let snapshot = {
        let mut state = state_guard(&state);
        state.stop();
        state.snapshot
    };
    changes.send_replace(snapshot);
    result
}

struct WriterCommitContext<'a> {
    state: &'a Arc<Mutex<WriterState>>,
    changes: &'a watch::Sender<WriterSnapshot>,
    cancellation: &'a CancellationToken,
    send_cipher: Option<&'a mut Rc4>,
    byte_metric_sink: Option<&'a Arc<dyn ByteMetricSink>>,
    no_progress_timeout: Duration,
}

async fn write_frame<W: AsyncWrite + Unpin>(
    write: &mut W,
    mut frame: WriterFrame,
    context: WriterCommitContext<'_>,
) -> Result<(), PeerIoError> {
    let WriterCommitContext {
        state,
        changes,
        cancellation,
        send_cipher,
        byte_metric_sink,
        no_progress_timeout,
    } = context;
    if frame
        .validity
        .as_ref()
        .is_some_and(|validity| validity.is_cancelled())
    {
        discard_frame(state, changes, frame.bytes.len());
        return Ok(());
    }
    if let Some(cipher) = send_cipher {
        cipher.apply(&mut frame.bytes);
    }
    let mut deadline = Instant::now() + no_progress_timeout;
    let mut offset = 0;
    while offset < frame.bytes.len() {
        let result = if offset == 0 {
            if let Some(validity) = frame.validity.as_ref() {
                tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => return Ok(()),
                    _ = validity.cancelled() => {
                        discard_frame(state, changes, frame.bytes.len());
                        return Ok(());
                    }
                    result = timeout_at(deadline, write.write(&frame.bytes)) => result,
                }
            } else {
                tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => return Ok(()),
                    result = timeout_at(deadline, write.write(&frame.bytes)) => result,
                }
            }
        } else {
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Ok(()),
                result = timeout_at(deadline, write.write(&frame.bytes[offset..])) => result,
            }
        };
        let written = result
            .map_err(|_| PeerIoError::TimedOut {
                operation: "message write",
                timeout: no_progress_timeout,
            })?
            .map_err(|source| PeerIoError::Io {
                operation: "send peer message",
                source,
            })?;
        if written == 0 {
            return Err(PeerIoError::Closed);
        }
        deadline = Instant::now() + no_progress_timeout;
        let uploaded = record_sent_range(
            byte_metric_sink,
            frame.bytes.len(),
            frame.payload_length,
            frame.payload_metric,
            offset,
            written,
        );
        offset += written;
        let snapshot = {
            let mut state = state_guard(state);
            state.release_written(
                written,
                if frame.payload_metric == Some(ByteMetric::PayloadUploaded) {
                    uploaded
                } else {
                    0
                },
            );
            state.snapshot
        };
        changes.send_replace(snapshot);
    }
    Ok(())
}

fn discard_frame(
    state: &Arc<Mutex<WriterState>>,
    changes: &watch::Sender<WriterSnapshot>,
    remaining: usize,
) {
    let snapshot = {
        let mut state = state_guard(state);
        state.release_frame(remaining);
        state.snapshot
    };
    changes.send_replace(snapshot);
}

fn state_guard(state: &Arc<Mutex<WriterState>>) -> MutexGuard<'_, WriterState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use rstorrent_protocol::mse::{
        DhPrivateExponent, MseCipherPair, MseRole, compute_public_key, compute_shared_secret,
    };
    use rstorrent_protocol::peer_wire::{PeerMessage, encode_message};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    use super::{
        FrameValidity, IncomingPeerIo, MAX_INCOMING_WRITER_BYTES, WRITER_FRAME_QUEUE, WriterState,
    };

    #[test]
    fn writer_charge_is_exact_and_recoverable() {
        assert_eq!(WRITER_FRAME_QUEUE, 64);
        let mut state = WriterState::new();
        state
            .reserve(MAX_INCOMING_WRITER_BYTES)
            .expect("exact hard charge");
        assert!(state.reserve(1).is_err());
        assert_eq!(state.snapshot.queued_high_water, MAX_INCOMING_WRITER_BYTES);
        state.release_written(MAX_INCOMING_WRITER_BYTES, 16_384);
        assert_eq!(state.snapshot.queued_bytes, 0);
        assert_eq!(state.snapshot.uploaded_payload_bytes, 16_384);
    }

    #[tokio::test]
    async fn invalidated_piece_is_discarded_before_its_first_byte() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind pair");
        let mut client = TcpStream::connect(listener.local_addr().expect("pair address"))
            .await
            .expect("connect pair");
        let (server, _) = listener.accept().await.expect("accept pair");
        let mut io = IncomingPeerIo::new(server, Duration::from_secs(1), None);
        io.queue_generation_fenced_message(
            &PeerMessage::Piece {
                index: 0,
                begin: 0,
                block: vec![7; 16],
            },
            {
                let validity = Arc::new(FrameValidity::new());
                validity.cancel();
                validity
            },
        )
        .expect("queue invalid piece");
        io.send_message(&PeerMessage::KeepAlive)
            .await
            .expect("flush through keepalive");

        let mut keepalive = [1_u8; 4];
        client
            .read_exact(&mut keepalive)
            .await
            .expect("read first surviving frame");
        assert_eq!(keepalive, [0; 4]);
        assert_eq!(io.uploaded_payload_bytes(), 0);
        assert_eq!(io.send_buffer_size(), 0);
        io.shutdown().await.expect("join writer");
    }

    #[tokio::test]
    async fn encrypted_writer_discards_before_advancing_the_cipher() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind pair");
        let mut client = TcpStream::connect(listener.local_addr().expect("pair address"))
            .await
            .expect("connect pair");
        let (server, _) = listener.accept().await.expect("accept pair");
        let (mut initiator_ciphers, responder_ciphers) = cipher_pairs();
        let mut io = IncomingPeerIo::new_with_mse(
            server,
            Duration::from_secs(1),
            None,
            Some(responder_ciphers),
            &[],
        )
        .expect("construct encrypted incoming IO");
        io.queue_generation_fenced_message(
            &PeerMessage::Piece {
                index: 0,
                begin: 0,
                block: vec![7; 16],
            },
            {
                let validity = Arc::new(FrameValidity::new());
                validity.cancel();
                validity
            },
        )
        .expect("queue invalid piece");
        io.send_message(&PeerMessage::KeepAlive)
            .await
            .expect("flush through keepalive");

        let mut encrypted_keepalive = [1_u8; 4];
        client
            .read_exact(&mut encrypted_keepalive)
            .await
            .expect("read first surviving encrypted frame");
        assert_ne!(encrypted_keepalive, [0; 4]);
        initiator_ciphers.apply_receive(&mut encrypted_keepalive);
        assert_eq!(encrypted_keepalive, [0; 4]);

        let message = PeerMessage::Interested;
        let mut encrypted_message = encode_message(&message).expect("encode message");
        initiator_ciphers.apply_send(&mut encrypted_message);
        client
            .write_all(&encrypted_message)
            .await
            .expect("write encrypted message");
        assert_eq!(
            io.next_message_or_send_ready(usize::MAX)
                .await
                .expect("read encrypted message"),
            Some(message)
        );
        io.shutdown().await.expect("join writer");
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
