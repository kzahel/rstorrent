//! Incoming peer reader plus joined, byte-charged writer ownership.

use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use rstorrent_protocol::peer_wire::{FrameDecoder, PeerMessage, encode_message};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout_at};
use tokio_util::sync::CancellationToken;

use crate::metrics::{ByteMetric, ByteMetricSink};
use crate::peer_io::{
    NETWORK_READ_LENGTH, PeerIoError, message_payload_metric, record_bytes, record_sent_range,
};

pub(super) const MAX_INCOMING_WRITER_BYTES: usize = 528_396;
pub(super) const INCOMING_WRITER_NO_PROGRESS_TIMEOUT: Duration = Duration::from_secs(60);
const WRITER_FRAME_QUEUE: usize = 2_048;

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
    fn spawn(write: OwnedWriteHalf, byte_metric_sink: Option<Arc<dyn ByteMetricSink>>) -> Self {
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

    fn queue(&mut self, message: &PeerMessage) -> Result<(), PeerIoError> {
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
    read: OwnedReadHalf,
    decoder: FrameDecoder,
    queued_messages: VecDeque<PeerMessage>,
    writer: IncomingWriter,
    io_timeout: Duration,
    byte_metric_sink: Option<Arc<dyn ByteMetricSink>>,
}

impl IncomingPeerIo {
    pub fn new(
        stream: TcpStream,
        io_timeout: Duration,
        byte_metric_sink: Option<Arc<dyn ByteMetricSink>>,
    ) -> Self {
        let (read, write) = stream.into_split();
        Self {
            read,
            decoder: FrameDecoder::new(),
            queued_messages: VecDeque::new(),
            writer: IncomingWriter::spawn(write, byte_metric_sink.clone()),
            io_timeout,
            byte_metric_sink,
        }
    }

    pub async fn send_message(&mut self, message: &PeerMessage) -> Result<(), PeerIoError> {
        self.queue_message(message)?;
        self.writer.flush(self.io_timeout).await
    }

    pub fn queue_message(&mut self, message: &PeerMessage) -> Result<(), PeerIoError> {
        self.writer.queue(message)
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
    mut write: OwnedWriteHalf,
    mut commands: mpsc::Receiver<WriterFrame>,
    state: Arc<Mutex<WriterState>>,
    changes: watch::Sender<WriterSnapshot>,
    cancellation: CancellationToken,
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
                &state,
                &changes,
                &cancellation,
                byte_metric_sink.as_ref(),
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

async fn write_frame(
    write: &mut OwnedWriteHalf,
    frame: WriterFrame,
    state: &Arc<Mutex<WriterState>>,
    changes: &watch::Sender<WriterSnapshot>,
    cancellation: &CancellationToken,
    byte_metric_sink: Option<&Arc<dyn ByteMetricSink>>,
) -> Result<(), PeerIoError> {
    let deadline = Instant::now() + INCOMING_WRITER_NO_PROGRESS_TIMEOUT;
    let mut offset = 0;
    while offset < frame.bytes.len() {
        let written = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Ok(()),
            result = timeout_at(deadline, write.write(&frame.bytes[offset..])) => {
                result.map_err(|_| PeerIoError::TimedOut {
                    operation: "message write",
                    timeout: INCOMING_WRITER_NO_PROGRESS_TIMEOUT,
                })?
            }
        }
        .map_err(|source| PeerIoError::Io {
            operation: "send peer message",
            source,
        })?;
        if written == 0 {
            return Err(PeerIoError::Closed);
        }
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

fn state_guard(state: &Arc<Mutex<WriterState>>) -> MutexGuard<'_, WriterState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::{MAX_INCOMING_WRITER_BYTES, WriterState};

    #[test]
    fn writer_charge_is_exact_and_recoverable() {
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
}
