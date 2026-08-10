//! Bounded uTP stream queue, packetization, ACK, and retransmission scheduling.

use std::collections::{BTreeSet, VecDeque};
use std::error::Error;
use std::fmt;

use super::{PacketType, ReceiveDisposition, SequenceNumber, UTP_HEADER_SIZE};

pub const MAX_UNSENT_BYTES: usize = 1024 * 1024;
pub const MAX_RETRANSMISSION_WORK: usize = 1024;
pub const MAX_DELAYED_ACK_MICROS: u64 = 25_000;
const MIN_DELAYED_ACK_MICROS: u64 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransmitQueueSnapshot {
    pub unsent_bytes: usize,
    pub byte_high_water: usize,
    pub terminal: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransmitQueueError {
    Terminal,
    ByteLimit {
        current: usize,
        requested: usize,
        maximum: usize,
    },
    ConsumeBeyondQueued {
        bytes: usize,
        available: usize,
    },
}

impl fmt::Display for TransmitQueueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Terminal => formatter.write_str("uTP transmit queue is terminal"),
            Self::ByteLimit {
                current,
                requested,
                maximum,
            } => write!(
                formatter,
                "uTP unsent bytes {current} plus {requested} exceed {maximum}"
            ),
            Self::ConsumeBeyondQueued { bytes, available } => write!(
                formatter,
                "cannot consume {bytes} uTP unsent bytes when only {available} are queued"
            ),
        }
    }
}

impl Error for TransmitQueueError {}

#[derive(Clone, Debug)]
pub struct TransmitQueue {
    bytes: VecDeque<u8>,
    byte_high_water: usize,
    terminal: bool,
}

impl TransmitQueue {
    #[must_use]
    pub fn new() -> Self {
        Self {
            bytes: VecDeque::new(),
            byte_high_water: 0,
            terminal: false,
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> TransmitQueueSnapshot {
        TransmitQueueSnapshot {
            unsent_bytes: self.bytes.len(),
            byte_high_water: self.byte_high_water,
            terminal: self.terminal,
        }
    }

    pub fn append(&mut self, bytes: &[u8]) -> Result<(), TransmitQueueError> {
        if self.terminal {
            return Err(TransmitQueueError::Terminal);
        }
        let next =
            self.bytes
                .len()
                .checked_add(bytes.len())
                .ok_or(TransmitQueueError::ByteLimit {
                    current: self.bytes.len(),
                    requested: bytes.len(),
                    maximum: MAX_UNSENT_BYTES,
                })?;
        if next > MAX_UNSENT_BYTES {
            return Err(TransmitQueueError::ByteLimit {
                current: self.bytes.len(),
                requested: bytes.len(),
                maximum: MAX_UNSENT_BYTES,
            });
        }
        self.bytes.extend(bytes);
        self.byte_high_water = self.byte_high_water.max(next);
        Ok(())
    }

    #[must_use]
    pub fn prefix(&self, bytes: usize) -> Option<Vec<u8>> {
        (bytes <= self.bytes.len()).then(|| self.bytes.iter().take(bytes).copied().collect())
    }

    pub fn consume(&mut self, bytes: usize) -> Result<(), TransmitQueueError> {
        if self.terminal {
            return Err(TransmitQueueError::Terminal);
        }
        if bytes > self.bytes.len() {
            return Err(TransmitQueueError::ConsumeBeyondQueued {
                bytes,
                available: self.bytes.len(),
            });
        }
        self.bytes.drain(..bytes);
        Ok(())
    }

    pub fn reset(&mut self) {
        self.bytes.clear();
        self.terminal = true;
    }
}

impl Default for TransmitQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[must_use]
pub const fn utp_header_bytes(selective_ack_bytes: usize) -> usize {
    UTP_HEADER_SIZE
        + if selective_ack_bytes == 0 {
            0
        } else {
            2 + selective_ack_bytes
        }
}

#[must_use]
pub fn new_payload_bytes(
    queued_bytes: usize,
    datagram_limit: usize,
    selective_ack_bytes: usize,
    congestion_window_bytes: usize,
    remote_window_bytes: usize,
    in_flight_bytes: usize,
) -> usize {
    let header_bytes = utp_header_bytes(selective_ack_bytes);
    let datagram_payload = datagram_limit.saturating_sub(header_bytes);
    let payload_window = congestion_window_bytes
        .min(remote_window_bytes)
        .saturating_sub(in_flight_bytes);
    queued_bytes.min(datagram_payload).min(payload_window)
}

#[must_use]
pub fn retransmission_is_admissible(
    payload_bytes: usize,
    congestion_window_bytes: usize,
    remote_window_bytes: usize,
    in_flight_bytes: usize,
) -> bool {
    if in_flight_bytes == 0 {
        return true;
    }
    payload_bytes
        <= congestion_window_bytes
            .min(remote_window_bytes)
            .saturating_sub(in_flight_bytes)
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AckSchedulerSnapshot {
    pub pending_packets: u8,
    pub deadline_micros: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct AckScheduler {
    pending_packets: u8,
    deadline_micros: Option<u64>,
}

impl AckScheduler {
    #[must_use]
    pub fn snapshot(&self) -> AckSchedulerSnapshot {
        AckSchedulerSnapshot {
            pending_packets: self.pending_packets,
            deadline_micros: self.deadline_micros,
        }
    }

    pub fn on_receive(
        &mut self,
        packet_type: PacketType,
        disposition: ReceiveDisposition,
        now_micros: u64,
        smoothed_rtt_micros: Option<u64>,
    ) {
        if !matches!(packet_type, PacketType::Data | PacketType::Fin) {
            return;
        }
        let immediate = packet_type == PacketType::Fin
            || !matches!(disposition, ReceiveDisposition::Delivered)
            || self.pending_packets >= 1;
        self.pending_packets = self.pending_packets.saturating_add(1).min(2);
        if immediate {
            self.deadline_micros = Some(now_micros);
            return;
        }
        let delay = smoothed_rtt_micros
            .map(|rtt| (rtt / 4).clamp(MIN_DELAYED_ACK_MICROS, MAX_DELAYED_ACK_MICROS))
            .unwrap_or(MAX_DELAYED_ACK_MICROS);
        self.deadline_micros = Some(now_micros.saturating_add(delay));
    }

    pub fn on_window_reopened(&mut self, now_micros: u64) {
        self.pending_packets = self.pending_packets.max(1);
        self.deadline_micros = Some(now_micros);
    }

    #[must_use]
    pub fn is_due(&self, now_micros: u64) -> bool {
        self.deadline_micros
            .is_some_and(|deadline| now_micros >= deadline)
    }

    pub fn acknowledge(&mut self) -> bool {
        let had_pending = self.deadline_micros.take().is_some();
        self.pending_packets = 0;
        had_pending
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RetransmissionSnapshot {
    pub pending_packets: usize,
    pub packet_high_water: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetransmissionLimit {
    pub count: usize,
    pub maximum: usize,
}

impl fmt::Display for RetransmissionLimit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "uTP retransmission work count {} exceeds {}",
            self.count, self.maximum
        )
    }
}

impl Error for RetransmissionLimit {}

#[derive(Clone, Debug, Default)]
pub struct RetransmissionQueue {
    ordered: VecDeque<SequenceNumber>,
    membership: BTreeSet<SequenceNumber>,
    packet_high_water: usize,
}

impl RetransmissionQueue {
    #[must_use]
    pub fn snapshot(&self) -> RetransmissionSnapshot {
        RetransmissionSnapshot {
            pending_packets: self.ordered.len(),
            packet_high_water: self.packet_high_water,
        }
    }

    pub fn schedule(
        &mut self,
        sequence_numbers: impl IntoIterator<Item = SequenceNumber>,
    ) -> Result<(), RetransmissionLimit> {
        let additions: Vec<_> = sequence_numbers
            .into_iter()
            .filter(|sequence_number| !self.membership.contains(sequence_number))
            .collect();
        let mut unique = BTreeSet::new();
        let additions: Vec<_> = additions
            .into_iter()
            .filter(|sequence_number| unique.insert(*sequence_number))
            .collect();
        let next = self.ordered.len().saturating_add(additions.len());
        if next > MAX_RETRANSMISSION_WORK {
            return Err(RetransmissionLimit {
                count: next,
                maximum: MAX_RETRANSMISSION_WORK,
            });
        }
        for sequence_number in additions {
            self.ordered.push_back(sequence_number);
            self.membership.insert(sequence_number);
        }
        self.packet_high_water = self.packet_high_water.max(next);
        Ok(())
    }

    #[must_use]
    pub fn front(&self) -> Option<SequenceNumber> {
        self.ordered.front().copied()
    }

    pub fn complete_front(&mut self, sequence_number: SequenceNumber) -> bool {
        if self.front() != Some(sequence_number) {
            return false;
        }
        self.ordered.pop_front();
        self.membership.remove(&sequence_number);
        true
    }

    pub fn remove(&mut self, sequence_number: SequenceNumber) -> bool {
        if !self.membership.remove(&sequence_number) {
            return false;
        }
        self.ordered.retain(|queued| *queued != sequence_number);
        true
    }

    pub fn reset(&mut self) {
        self.ordered.clear();
        self.membership.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sequence(value: u16) -> SequenceNumber {
        SequenceNumber::new(value)
    }

    #[test]
    fn unsent_queue_has_atomic_exact_byte_ownership() {
        let mut queue = TransmitQueue::new();
        queue
            .append(&vec![1; MAX_UNSENT_BYTES])
            .expect("fill queue");
        let full = queue.snapshot();
        assert_eq!(full.unsent_bytes, MAX_UNSENT_BYTES);
        assert_eq!(full.byte_high_water, MAX_UNSENT_BYTES);
        assert!(matches!(
            queue.append(b"x"),
            Err(TransmitQueueError::ByteLimit {
                current: MAX_UNSENT_BYTES,
                requested: 1,
                maximum: MAX_UNSENT_BYTES
            })
        ));
        assert_eq!(queue.snapshot(), full);
        assert_eq!(queue.prefix(4), Some(vec![1; 4]));
        queue.consume(4).expect("consume prefix");
        assert_eq!(queue.snapshot().unsent_bytes, MAX_UNSENT_BYTES - 4);
        assert!(matches!(
            queue.consume(MAX_UNSENT_BYTES),
            Err(TransmitQueueError::ConsumeBeyondQueued { .. })
        ));
        queue.reset();
        assert_eq!(queue.snapshot().unsent_bytes, 0);
        assert_eq!(queue.append(b"late"), Err(TransmitQueueError::Terminal));
    }

    #[test]
    fn packetization_obeys_header_mtu_and_both_windows_without_stranding() {
        assert_eq!(utp_header_bytes(0), 20);
        assert_eq!(utp_header_bytes(4), 26);
        assert_eq!(new_payload_bytes(1_000, 548, 0, 2_000, 2_000, 0), 528);
        assert_eq!(new_payload_bytes(1_000, 548, 4, 2_000, 2_000, 0), 522);
        assert_eq!(new_payload_bytes(1_000, 548, 0, 1_000, 507, 500), 7);
        assert_eq!(new_payload_bytes(1_000, 548, 0, 1_000, 500, 500), 0);
        assert_eq!(new_payload_bytes(1_000, 19, 0, 1_000, 1_000, 0), 0);
    }

    #[test]
    fn retransmission_escape_requires_no_other_payload_in_flight() {
        assert!(retransmission_is_admissible(528, 200, 0, 0));
        assert!(retransmission_is_admissible(500, 1_000, 1_000, 500));
        assert!(!retransmission_is_admissible(501, 1_000, 1_000, 500));
    }

    #[test]
    fn delayed_ack_is_bounded_immediate_on_second_or_recovery_input() {
        let mut scheduler = AckScheduler::default();
        scheduler.on_receive(
            PacketType::Data,
            ReceiveDisposition::Delivered,
            10_000,
            Some(40_000),
        );
        assert_eq!(scheduler.snapshot().deadline_micros, Some(20_000));
        assert!(!scheduler.is_due(19_999));
        scheduler.on_receive(
            PacketType::Data,
            ReceiveDisposition::Delivered,
            12_000,
            Some(40_000),
        );
        assert!(scheduler.is_due(12_000));
        assert!(scheduler.acknowledge());
        assert!(!scheduler.acknowledge());

        scheduler.on_receive(PacketType::Data, ReceiveDisposition::Buffered, 30_000, None);
        assert!(scheduler.is_due(30_000));
        scheduler.acknowledge();
        scheduler.on_receive(PacketType::Fin, ReceiveDisposition::Delivered, 40_000, None);
        assert!(scheduler.is_due(40_000));
    }

    #[test]
    fn delayed_ack_deadline_clamps_and_window_reopen_is_immediate() {
        let mut scheduler = AckScheduler::default();
        scheduler.on_receive(
            PacketType::Data,
            ReceiveDisposition::Delivered,
            0,
            Some(400),
        );
        assert_eq!(scheduler.snapshot().deadline_micros, Some(100));
        scheduler.acknowledge();
        scheduler.on_receive(
            PacketType::Data,
            ReceiveDisposition::Delivered,
            10_000,
            Some(1_000_000),
        );
        assert_eq!(scheduler.snapshot().deadline_micros, Some(35_000));
        scheduler.on_window_reopened(20_000);
        assert!(scheduler.is_due(20_000));
    }

    #[test]
    fn retransmission_work_coalesces_and_preserves_signal_order() {
        let mut queue = RetransmissionQueue::default();
        queue
            .schedule([sequence(u16::MAX), sequence(0), sequence(u16::MAX)])
            .expect("schedule across wrap");
        assert_eq!(queue.snapshot().pending_packets, 2);
        assert_eq!(queue.front(), Some(sequence(u16::MAX)));
        assert!(!queue.complete_front(sequence(0)));
        assert!(queue.complete_front(sequence(u16::MAX)));
        assert_eq!(queue.front(), Some(sequence(0)));
        assert!(queue.remove(sequence(0)));
        assert!(!queue.remove(sequence(0)));
        assert_eq!(queue.snapshot().pending_packets, 0);
        assert_eq!(queue.snapshot().packet_high_water, 2);
    }
}
