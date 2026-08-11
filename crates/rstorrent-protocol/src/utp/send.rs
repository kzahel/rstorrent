//! Bounded deterministic uTP sent-packet, ACK, RTT, and timer state.

use std::collections::VecDeque;
use std::error::Error;
use std::fmt;

use super::{MAX_SACK_BYTES, MAX_UTP_PAYLOAD_SIZE, PacketType, SequenceNumber, SequenceRelation};

pub const MAX_SENT_PACKETS: usize = 1024;
pub const MAX_SENT_BYTES: usize = 1024 * 1024;
pub const MAX_TRANSMISSIONS: u8 = 8;
pub const INITIAL_RTO_MICROS: u64 = 1_000_000;
pub const MIN_RTO_MICROS: u64 = 500_000;
pub const MAX_RTO_MICROS: u64 = 60_000_000;
const LOSS_ACK_THRESHOLD: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SentPacketSnapshot {
    pub sequence_number: SequenceNumber,
    pub packet_type: PacketType,
    pub payload_bytes: usize,
    pub transmissions: u8,
    pub first_sent_at_micros: u64,
    pub last_sent_at_micros: u64,
    pub loss_signaled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RttSnapshot {
    pub smoothed_rtt_micros: Option<u64>,
    pub rtt_variance_micros: Option<u64>,
    pub base_rto_micros: u64,
    pub effective_rto_micros: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SendSnapshot {
    pub next_sequence_number: SequenceNumber,
    pub cumulative_acknowledgement: SequenceNumber,
    pub highest_sequence_sent: Option<SequenceNumber>,
    pub outstanding_packets: usize,
    pub outstanding_bytes: usize,
    pub packet_high_water: usize,
    pub byte_high_water: usize,
    pub duplicate_ack_count: u8,
    pub consecutive_timeouts: u8,
    pub timeout_deadline_micros: Option<u64>,
    pub rtt: RttSnapshot,
    pub syn_sent: bool,
    pub fin_sent: bool,
    pub terminal: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AckDisposition {
    Duplicate,
    Cumulative,
    Selective,
    CumulativeAndSelective,
    Stale { distance: u16 },
    Future,
    Ambiguous,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AckOutcome {
    pub disposition: AckDisposition,
    pub acknowledged_packets: usize,
    pub acknowledged_bytes: usize,
    pub acknowledged_sequences: Vec<SequenceNumber>,
    pub loss_signals: Vec<SequenceNumber>,
    pub rtt_sample_micros: Option<u64>,
    pub timeout_deadline_micros: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimeoutOutcome {
    pub loss_signals: Vec<SequenceNumber>,
    pub consecutive_timeouts: u8,
    pub effective_rto_micros: u64,
    pub next_deadline_micros: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SendError {
    Terminal,
    InvalidPacketType(PacketType),
    InvalidPayloadLength {
        packet_type: PacketType,
        length: usize,
    },
    PayloadTooLarge {
        length: usize,
        maximum: usize,
    },
    SentPacketLimit {
        count: usize,
        maximum: usize,
    },
    SentByteLimit {
        bytes: usize,
        maximum: usize,
    },
    SequenceWindowLimit {
        distance: u16,
        maximum: u16,
    },
    SynAlreadySent,
    FinAlreadySent,
    DataAfterFin,
    InvalidSackLength(usize),
    UnknownPacket(SequenceNumber),
    TransmissionLimit {
        sequence_number: SequenceNumber,
        transmissions: u8,
        maximum: u8,
    },
}

impl fmt::Display for SendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Terminal => formatter.write_str("uTP send state is terminal"),
            Self::InvalidPacketType(packet_type) => {
                write!(formatter, "uTP {packet_type:?} is not a sent ledger packet")
            }
            Self::InvalidPayloadLength {
                packet_type,
                length,
            } => write!(
                formatter,
                "uTP sent {packet_type:?} has invalid payload length {length}"
            ),
            Self::PayloadTooLarge { length, maximum } => {
                write!(formatter, "uTP sent payload {length} exceeds {maximum}")
            }
            Self::SentPacketLimit { count, maximum } => {
                write!(formatter, "uTP sent packet count {count} exceeds {maximum}")
            }
            Self::SentByteLimit { bytes, maximum } => {
                write!(
                    formatter,
                    "uTP sent payload bytes {bytes} exceeds {maximum}"
                )
            }
            Self::SequenceWindowLimit { distance, maximum } => write!(
                formatter,
                "uTP sent sequence distance {distance} exceeds {maximum}"
            ),
            Self::SynAlreadySent => formatter.write_str("uTP SYN was already sent"),
            Self::FinAlreadySent => formatter.write_str("uTP FIN was already sent"),
            Self::DataAfterFin => formatter.write_str("uTP DATA cannot be sent after FIN"),
            Self::InvalidSackLength(length) => write!(
                formatter,
                "received uTP SACK length {length} is outside 1..={MAX_SACK_BYTES}"
            ),
            Self::UnknownPacket(sequence_number) => write!(
                formatter,
                "uTP sent packet {} is not outstanding",
                sequence_number.get()
            ),
            Self::TransmissionLimit {
                sequence_number,
                transmissions,
                maximum,
            } => write!(
                formatter,
                "uTP packet {} transmission count {transmissions} reached {maximum}",
                sequence_number.get()
            ),
        }
    }
}

impl Error for SendError {}

#[derive(Clone, Debug)]
struct SentPacket {
    sequence_number: SequenceNumber,
    packet_type: PacketType,
    payload: Vec<u8>,
    transmissions: u8,
    first_sent_at_micros: u64,
    last_sent_at_micros: u64,
    loss_signaled: bool,
}

impl SentPacket {
    fn snapshot(&self) -> SentPacketSnapshot {
        SentPacketSnapshot {
            sequence_number: self.sequence_number,
            packet_type: self.packet_type,
            payload_bytes: self.payload.len(),
            transmissions: self.transmissions,
            first_sent_at_micros: self.first_sent_at_micros,
            last_sent_at_micros: self.last_sent_at_micros,
            loss_signaled: self.loss_signaled,
        }
    }
}

#[derive(Clone, Debug)]
struct RttEstimator {
    smoothed_micros: Option<u64>,
    variance_micros: Option<u64>,
    base_rto_micros: u64,
}

impl RttEstimator {
    fn new() -> Self {
        Self {
            smoothed_micros: None,
            variance_micros: None,
            base_rto_micros: INITIAL_RTO_MICROS,
        }
    }

    fn record(&mut self, sample_micros: u64) {
        match (self.smoothed_micros, self.variance_micros) {
            (Some(smoothed), Some(variance)) => {
                let difference = smoothed.abs_diff(sample_micros);
                let next_variance = variance.saturating_mul(3).saturating_add(difference) / 4;
                let next_smoothed = smoothed.saturating_mul(7).saturating_add(sample_micros) / 8;
                self.smoothed_micros = Some(next_smoothed);
                self.variance_micros = Some(next_variance);
                self.base_rto_micros = next_smoothed
                    .saturating_add(next_variance.saturating_mul(4))
                    .clamp(MIN_RTO_MICROS, MAX_RTO_MICROS);
            }
            _ => {
                let variance = sample_micros / 2;
                self.smoothed_micros = Some(sample_micros);
                self.variance_micros = Some(variance);
                self.base_rto_micros = sample_micros
                    .saturating_add(variance.saturating_mul(4))
                    .clamp(MIN_RTO_MICROS, MAX_RTO_MICROS);
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct SendState {
    next_sequence_number: SequenceNumber,
    cumulative_acknowledgement: SequenceNumber,
    highest_sequence_sent: Option<SequenceNumber>,
    outstanding: VecDeque<SentPacket>,
    outstanding_bytes: usize,
    packet_high_water: usize,
    byte_high_water: usize,
    duplicate_ack_count: u8,
    consecutive_timeouts: u8,
    timeout_deadline_micros: Option<u64>,
    rtt: RttEstimator,
    syn_sent: bool,
    fin_sent: bool,
    terminal: bool,
}

impl SendState {
    #[must_use]
    pub fn new(initial_sequence_number: SequenceNumber) -> Self {
        Self {
            next_sequence_number: initial_sequence_number,
            cumulative_acknowledgement: initial_sequence_number.wrapping_sub(1),
            highest_sequence_sent: None,
            outstanding: VecDeque::new(),
            outstanding_bytes: 0,
            packet_high_water: 0,
            byte_high_water: 0,
            duplicate_ack_count: 0,
            consecutive_timeouts: 0,
            timeout_deadline_micros: None,
            rtt: RttEstimator::new(),
            syn_sent: false,
            fin_sent: false,
            terminal: false,
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> SendSnapshot {
        SendSnapshot {
            next_sequence_number: self.next_sequence_number,
            cumulative_acknowledgement: self.cumulative_acknowledgement,
            highest_sequence_sent: self.highest_sequence_sent,
            outstanding_packets: self.outstanding.len(),
            outstanding_bytes: self.outstanding_bytes,
            packet_high_water: self.packet_high_water,
            byte_high_water: self.byte_high_water,
            duplicate_ack_count: self.duplicate_ack_count,
            consecutive_timeouts: self.consecutive_timeouts,
            timeout_deadline_micros: self.timeout_deadline_micros,
            rtt: self.rtt_snapshot(),
            syn_sent: self.syn_sent,
            fin_sent: self.fin_sent,
            terminal: self.terminal,
        }
    }

    #[must_use]
    pub fn outstanding_packets(&self) -> impl ExactSizeIterator<Item = SentPacketSnapshot> + '_ {
        self.outstanding.iter().map(SentPacket::snapshot)
    }

    #[must_use]
    pub fn payload_for_retransmission(&self, sequence_number: SequenceNumber) -> Option<&[u8]> {
        self.outstanding
            .iter()
            .find(|packet| packet.sequence_number == sequence_number)
            .map(|packet| packet.payload.as_slice())
    }

    pub fn record_sent(
        &mut self,
        packet_type: PacketType,
        payload: &[u8],
        now_micros: u64,
    ) -> Result<SequenceNumber, SendError> {
        if self.terminal {
            return Err(SendError::Terminal);
        }
        match packet_type {
            PacketType::Syn if self.syn_sent => return Err(SendError::SynAlreadySent),
            PacketType::Syn if !payload.is_empty() => {
                return Err(SendError::InvalidPayloadLength {
                    packet_type,
                    length: payload.len(),
                });
            }
            PacketType::Data if self.fin_sent => return Err(SendError::DataAfterFin),
            PacketType::Data if payload.is_empty() => {
                return Err(SendError::InvalidPayloadLength {
                    packet_type,
                    length: 0,
                });
            }
            PacketType::Fin if self.fin_sent => return Err(SendError::FinAlreadySent),
            PacketType::State | PacketType::Reset => {
                return Err(SendError::InvalidPacketType(packet_type));
            }
            PacketType::Syn | PacketType::Data | PacketType::Fin => {}
        }
        if payload.len() > MAX_UTP_PAYLOAD_SIZE {
            return Err(SendError::PayloadTooLarge {
                length: payload.len(),
                maximum: MAX_UTP_PAYLOAD_SIZE,
            });
        }

        let next_count = self.outstanding.len() + 1;
        if next_count > MAX_SENT_PACKETS {
            return Err(SendError::SentPacketLimit {
                count: next_count,
                maximum: MAX_SENT_PACKETS,
            });
        }
        let next_bytes =
            self.outstanding_bytes
                .checked_add(payload.len())
                .ok_or(SendError::SentByteLimit {
                    bytes: usize::MAX,
                    maximum: MAX_SENT_BYTES,
                })?;
        if next_bytes > MAX_SENT_BYTES {
            return Err(SendError::SentByteLimit {
                bytes: next_bytes,
                maximum: MAX_SENT_BYTES,
            });
        }
        let sequence_number = self.next_sequence_number;
        let distance = match sequence_number.relation_to(self.cumulative_acknowledgement) {
            SequenceRelation::After(distance) => distance,
            SequenceRelation::Equal | SequenceRelation::Before(_) | SequenceRelation::Ambiguous => {
                return Err(SendError::SequenceWindowLimit {
                    distance: u16::MAX,
                    maximum: MAX_SENT_PACKETS as u16,
                });
            }
        };
        if distance > MAX_SENT_PACKETS as u16 {
            return Err(SendError::SequenceWindowLimit {
                distance,
                maximum: MAX_SENT_PACKETS as u16,
            });
        }

        self.outstanding.push_back(SentPacket {
            sequence_number,
            packet_type,
            payload: payload.to_vec(),
            transmissions: 1,
            first_sent_at_micros: now_micros,
            last_sent_at_micros: now_micros,
            loss_signaled: false,
        });
        self.outstanding_bytes = next_bytes;
        self.packet_high_water = self.packet_high_water.max(next_count);
        self.byte_high_water = self.byte_high_water.max(next_bytes);
        self.highest_sequence_sent = Some(sequence_number);
        match packet_type {
            PacketType::Syn => {
                self.syn_sent = true;
                self.next_sequence_number = self.next_sequence_number.wrapping_add(1);
            }
            PacketType::Data => {
                self.next_sequence_number = self.next_sequence_number.wrapping_add(1);
            }
            PacketType::Fin => self.fin_sent = true,
            PacketType::State | PacketType::Reset => unreachable!("validated packet type"),
        }
        self.recompute_timeout();
        Ok(sequence_number)
    }

    pub fn acknowledge(
        &mut self,
        acknowledgement_number: SequenceNumber,
        selective_ack: Option<&[u8]>,
        source_packet_type: PacketType,
        now_micros: u64,
    ) -> Result<AckOutcome, SendError> {
        if self.terminal {
            return Err(SendError::Terminal);
        }
        if let Some(bytes) = selective_ack {
            validate_sack(bytes)?;
        }

        let advance = match acknowledgement_number.relation_to(self.cumulative_acknowledgement) {
            SequenceRelation::Equal => 0,
            SequenceRelation::Before(distance) => {
                return Ok(self.empty_ack(AckDisposition::Stale { distance }));
            }
            SequenceRelation::Ambiguous => {
                return Ok(self.empty_ack(AckDisposition::Ambiguous));
            }
            SequenceRelation::After(distance) => {
                let Some(highest) = self.highest_sequence_sent else {
                    return Ok(self.empty_ack(AckDisposition::Future));
                };
                match acknowledgement_number.relation_to(highest) {
                    SequenceRelation::Equal | SequenceRelation::Before(_) => distance,
                    SequenceRelation::After(_) | SequenceRelation::Ambiguous => {
                        return Ok(self.empty_ack(AckDisposition::Future));
                    }
                }
            }
        };

        let maximum_sack_offset = self
            .highest_sequence_sent
            .and_then(|highest| highest.relation_to(acknowledgement_number).after_distance());
        let sack_offsets = selective_ack.map_or_else(Vec::new, |bytes| {
            set_sack_offsets(bytes)
                .into_iter()
                .filter(|offset| maximum_sack_offset.is_some_and(|maximum| *offset <= maximum))
                .collect()
        });
        let mut retained = VecDeque::with_capacity(self.outstanding.len());
        let mut acknowledged = Vec::new();
        while let Some(packet) = self.outstanding.pop_front() {
            let cumulative = advance > 0
                && matches!(
                    packet
                        .sequence_number
                        .relation_to(self.cumulative_acknowledgement),
                    SequenceRelation::After(distance) if distance <= advance
                );
            let selective = packet
                .sequence_number
                .relation_to(acknowledgement_number)
                .after_distance()
                .is_some_and(|offset| sack_offsets.binary_search(&offset).is_ok());
            if cumulative || selective {
                self.outstanding_bytes -= packet.payload.len();
                acknowledged.push(packet);
            } else {
                retained.push_back(packet);
            }
        }
        self.outstanding = retained;

        if advance > 0 {
            self.cumulative_acknowledgement = acknowledgement_number;
        }
        let acknowledged_sequences = acknowledged
            .iter()
            .map(|packet| packet.sequence_number)
            .collect();
        let acknowledged_packets = acknowledged.len();
        let acknowledged_bytes = acknowledged.iter().map(|packet| packet.payload.len()).sum();
        let rtt_sample_micros = acknowledged
            .iter()
            .filter(|packet| packet.transmissions == 1)
            .max_by_key(|packet| packet.first_sent_at_micros)
            .map(|packet| now_micros.saturating_sub(packet.first_sent_at_micros));
        if let Some(sample) = rtt_sample_micros {
            self.rtt.record(sample);
        }

        let made_progress = advance > 0 || acknowledged_packets > 0;
        if made_progress {
            self.duplicate_ack_count = 0;
            self.consecutive_timeouts = 0;
        } else if source_packet_type == PacketType::State && sack_offsets.is_empty() {
            self.duplicate_ack_count = self.duplicate_ack_count.saturating_add(1);
        }

        let retransmission_guard_micros = self.rtt.smoothed_micros.unwrap_or(MIN_RTO_MICROS);
        let mut loss_signals = self.loss_signals_from_sack(
            acknowledgement_number,
            &sack_offsets,
            now_micros,
            retransmission_guard_micros,
        );
        if self.duplicate_ack_count >= LOSS_ACK_THRESHOLD as u8 {
            let missing = acknowledgement_number.wrapping_add(1);
            if let Some(packet) = self.outstanding.iter_mut().find(|packet| {
                packet.sequence_number == missing
                    && !packet.loss_signaled
                    && (packet.transmissions == 1
                        || now_micros
                            >= packet
                                .last_sent_at_micros
                                .saturating_add(retransmission_guard_micros))
            }) {
                packet.loss_signaled = true;
                loss_signals.push(missing);
            }
        }
        self.recompute_timeout();

        let had_cumulative = advance > 0;
        let had_selective = !sack_offsets.is_empty();
        let disposition = match (had_cumulative, had_selective) {
            (true, true) => AckDisposition::CumulativeAndSelective,
            (true, false) => AckDisposition::Cumulative,
            (false, true) => AckDisposition::Selective,
            (false, false) => AckDisposition::Duplicate,
        };
        Ok(AckOutcome {
            disposition,
            acknowledged_packets,
            acknowledged_bytes,
            acknowledged_sequences,
            loss_signals,
            rtt_sample_micros,
            timeout_deadline_micros: self.timeout_deadline_micros,
        })
    }

    pub fn mark_retransmitted(
        &mut self,
        sequence_number: SequenceNumber,
        now_micros: u64,
    ) -> Result<SentPacketSnapshot, SendError> {
        if self.terminal {
            return Err(SendError::Terminal);
        }
        let packet = self
            .outstanding
            .iter_mut()
            .find(|packet| packet.sequence_number == sequence_number)
            .ok_or(SendError::UnknownPacket(sequence_number))?;
        if packet.transmissions >= MAX_TRANSMISSIONS {
            return Err(SendError::TransmissionLimit {
                sequence_number,
                transmissions: packet.transmissions,
                maximum: MAX_TRANSMISSIONS,
            });
        }
        packet.transmissions += 1;
        packet.last_sent_at_micros = now_micros;
        packet.loss_signaled = false;
        let snapshot = packet.snapshot();
        self.duplicate_ack_count = 0;
        self.recompute_timeout();
        Ok(snapshot)
    }

    pub fn on_timeout(&mut self, now_micros: u64) -> Result<Option<TimeoutOutcome>, SendError> {
        self.on_timeout_classified(now_micros, true)
    }

    pub fn on_timeout_classified(
        &mut self,
        now_micros: u64,
        apply_backoff: bool,
    ) -> Result<Option<TimeoutOutcome>, SendError> {
        if self.terminal {
            return Err(SendError::Terminal);
        }
        let Some(deadline) = self.timeout_deadline_micros else {
            return Ok(None);
        };
        if now_micros < deadline {
            return Ok(None);
        }

        if apply_backoff {
            self.consecutive_timeouts = self.consecutive_timeouts.saturating_add(1);
        }
        self.duplicate_ack_count = 0;
        let mut loss_signals = Vec::with_capacity(self.outstanding.len());
        for packet in &mut self.outstanding {
            packet.loss_signaled = true;
            loss_signals.push(packet.sequence_number);
        }
        let effective_rto_micros = self.effective_rto_micros();
        let next_deadline_micros = now_micros.saturating_add(effective_rto_micros);
        self.timeout_deadline_micros = Some(next_deadline_micros);
        Ok(Some(TimeoutOutcome {
            loss_signals,
            consecutive_timeouts: self.consecutive_timeouts,
            effective_rto_micros,
            next_deadline_micros,
        }))
    }

    /// Reset timeout backoff after an established connection accepts a valid
    /// packet, even when its acknowledgement does not advance the send ledger.
    pub fn on_valid_incoming(&mut self, now_micros: u64) {
        self.consecutive_timeouts = 0;
        self.timeout_deadline_micros = (!self.outstanding.is_empty())
            .then(|| now_micros.saturating_add(self.effective_rto_micros()));
    }

    pub fn reset(&mut self) {
        self.outstanding.clear();
        self.outstanding_bytes = 0;
        self.duplicate_ack_count = 0;
        self.consecutive_timeouts = 0;
        self.timeout_deadline_micros = None;
        self.terminal = true;
    }

    fn empty_ack(&self, disposition: AckDisposition) -> AckOutcome {
        AckOutcome {
            disposition,
            acknowledged_packets: 0,
            acknowledged_bytes: 0,
            acknowledged_sequences: Vec::new(),
            loss_signals: Vec::new(),
            rtt_sample_micros: None,
            timeout_deadline_micros: self.timeout_deadline_micros,
        }
    }

    fn loss_signals_from_sack(
        &mut self,
        acknowledgement_number: SequenceNumber,
        sack_offsets: &[u16],
        now_micros: u64,
        retransmission_guard_micros: u64,
    ) -> Vec<SequenceNumber> {
        let mut signals = Vec::new();
        for packet in &mut self.outstanding {
            let SequenceRelation::After(offset) =
                packet.sequence_number.relation_to(acknowledgement_number)
            else {
                continue;
            };
            let later = sack_offsets.partition_point(|reported| *reported <= offset);
            let retransmission_is_old_enough = packet.transmissions == 1
                || now_micros
                    >= packet
                        .last_sent_at_micros
                        .saturating_add(retransmission_guard_micros);
            if sack_offsets.len() - later >= LOSS_ACK_THRESHOLD
                && !packet.loss_signaled
                && retransmission_is_old_enough
            {
                packet.loss_signaled = true;
                signals.push(packet.sequence_number);
            }
        }
        signals
    }

    fn recompute_timeout(&mut self) {
        self.timeout_deadline_micros = self
            .outstanding
            .iter()
            .map(|packet| packet.last_sent_at_micros)
            .min()
            .map(|sent_at| sent_at.saturating_add(self.effective_rto_micros()));
    }

    fn effective_rto_micros(&self) -> u64 {
        self.rtt
            .base_rto_micros
            .checked_shl(u32::from(self.consecutive_timeouts).min(63))
            .unwrap_or(u64::MAX)
            .min(MAX_RTO_MICROS)
    }

    fn rtt_snapshot(&self) -> RttSnapshot {
        RttSnapshot {
            smoothed_rtt_micros: self.rtt.smoothed_micros,
            rtt_variance_micros: self.rtt.variance_micros,
            base_rto_micros: self.rtt.base_rto_micros,
            effective_rto_micros: self.effective_rto_micros(),
        }
    }
}

trait SequenceRelationExt {
    fn after_distance(self) -> Option<u16>;
}

impl SequenceRelationExt for SequenceRelation {
    fn after_distance(self) -> Option<u16> {
        match self {
            Self::After(distance) => Some(distance),
            Self::Before(_) | Self::Equal | Self::Ambiguous => None,
        }
    }
}

fn validate_sack(bytes: &[u8]) -> Result<(), SendError> {
    if !(1..=MAX_SACK_BYTES).contains(&bytes.len()) {
        return Err(SendError::InvalidSackLength(bytes.len()));
    }
    Ok(())
}

fn set_sack_offsets(bytes: &[u8]) -> Vec<u16> {
    bytes
        .iter()
        .enumerate()
        .flat_map(|(byte_index, byte)| {
            (0_u8..8).filter_map(move |bit_index| {
                (byte & (1 << bit_index) != 0).then_some(
                    u16::try_from(byte_index * 8 + usize::from(bit_index) + 2)
                        .expect("SACK offset is bounded by 252 bytes"),
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(state: &mut SendState, byte: u8, now: u64) -> SequenceNumber {
        state
            .record_sent(PacketType::Data, &[byte], now)
            .expect("record data")
    }

    #[test]
    fn records_sequence_consumption_and_exact_resources_across_wrap() {
        let mut state = SendState::new(SequenceNumber::new(u16::MAX));
        let syn = state.record_sent(PacketType::Syn, &[], 10).expect("SYN");
        let payload = data(&mut state, 7, 20);
        let fin = state
            .record_sent(PacketType::Fin, b"tail", 30)
            .expect("FIN");
        assert_eq!(syn.get(), u16::MAX);
        assert_eq!(payload.get(), 0);
        assert_eq!(fin.get(), 1);
        assert_eq!(state.snapshot().next_sequence_number.get(), 1);
        assert_eq!(state.snapshot().outstanding_packets, 3);
        assert_eq!(state.snapshot().outstanding_bytes, 5);
        assert_eq!(state.outstanding_packets().count(), 3);
        assert_eq!(state.snapshot().timeout_deadline_micros, Some(1_000_010));
    }

    #[test]
    fn cumulative_and_selective_ack_release_exact_storage() {
        let mut state = SendState::new(SequenceNumber::new(1));
        for byte in 1..=5 {
            data(&mut state, byte, u64::from(byte) * 10);
        }
        let sack = [0b0000_0101];
        let selective = state
            .acknowledge(SequenceNumber::new(1), Some(&sack), PacketType::State, 100)
            .expect("selective ACK");
        assert_eq!(
            selective.disposition,
            AckDisposition::CumulativeAndSelective
        );
        assert_eq!(selective.acknowledged_packets, 3);
        assert_eq!(state.snapshot().outstanding_packets, 2);
        assert_eq!(
            state
                .outstanding_packets()
                .map(|packet| packet.sequence_number.get())
                .collect::<Vec<_>>(),
            vec![2, 4]
        );

        let cumulative = state
            .acknowledge(SequenceNumber::new(5), None, PacketType::State, 120)
            .expect("cumulative ACK");
        assert_eq!(cumulative.acknowledged_packets, 2);
        assert_eq!(state.snapshot().outstanding_packets, 0);
        assert_eq!(state.snapshot().outstanding_bytes, 0);
        assert_eq!(state.snapshot().timeout_deadline_micros, None);
    }

    #[test]
    fn invalid_and_impossible_acks_do_not_mutate_state() {
        let mut state = SendState::new(SequenceNumber::new(100));
        data(&mut state, 1, 0);
        let before = state.snapshot();
        assert_eq!(
            state
                .acknowledge(SequenceNumber::new(99), Some(&[]), PacketType::State, 10)
                .expect_err("invalid SACK"),
            SendError::InvalidSackLength(0)
        );
        assert_eq!(state.snapshot(), before);

        let future = state
            .acknowledge(SequenceNumber::new(101), None, PacketType::State, 10)
            .expect("ignored future");
        assert_eq!(future.disposition, AckDisposition::Future);
        assert_eq!(state.snapshot(), before);
        let ambiguous = state
            .acknowledge(
                SequenceNumber::new(99_u16.wrapping_add(0x8000)),
                None,
                PacketType::State,
                10,
            )
            .expect("ignored ambiguity");
        assert_eq!(ambiguous.disposition, AckDisposition::Ambiguous);
        assert_eq!(state.snapshot(), before);
        let stale = state
            .acknowledge(SequenceNumber::new(98), None, PacketType::State, 10)
            .expect("ignored stale");
        assert!(matches!(stale.disposition, AckDisposition::Stale { .. }));
        assert_eq!(state.snapshot(), before);
    }

    #[test]
    fn sack_bits_beyond_the_highest_sent_packet_cannot_inject_loss() {
        let mut state = SendState::new(SequenceNumber::new(1));
        let sequence = data(&mut state, 9, 0);
        let future_only = [0xff, 0xff, 0xff, 0xff];
        for _ in 0..2 {
            let outcome = state
                .acknowledge(
                    SequenceNumber::new(0),
                    Some(&future_only),
                    PacketType::State,
                    10,
                )
                .expect("bounded future SACK");
            assert!(outcome.loss_signals.is_empty());
        }
        assert_eq!(state.payload_for_retransmission(sequence), Some(&[9][..]));
    }

    #[test]
    fn three_duplicate_state_acks_wait_one_rtt_before_resignaling_retransmit() {
        let mut state = SendState::new(SequenceNumber::new(1));
        let missing = data(&mut state, 1, 0);
        for expected_count in 1..=2 {
            let outcome = state
                .acknowledge(SequenceNumber::new(0), None, PacketType::State, 10)
                .expect("duplicate ACK");
            assert!(outcome.loss_signals.is_empty());
            assert_eq!(state.snapshot().duplicate_ack_count, expected_count);
        }
        let third = state
            .acknowledge(SequenceNumber::new(0), None, PacketType::State, 10)
            .expect("third duplicate ACK");
        assert_eq!(third.loss_signals, vec![missing]);
        let fourth = state
            .acknowledge(SequenceNumber::new(0), None, PacketType::State, 10)
            .expect("fourth duplicate ACK");
        assert!(fourth.loss_signals.is_empty());

        state.mark_retransmitted(missing, 20).expect("retransmit");
        for _ in 0..2 {
            assert!(
                state
                    .acknowledge(SequenceNumber::new(0), None, PacketType::State, 30)
                    .expect("duplicate ACK")
                    .loss_signals
                    .is_empty()
            );
        }
        assert!(
            state
                .acknowledge(SequenceNumber::new(0), None, PacketType::State, 30)
                .expect("third duplicate ACK before guard")
                .loss_signals
                .is_empty()
        );
        assert_eq!(
            state
                .acknowledge(
                    SequenceNumber::new(0),
                    None,
                    PacketType::State,
                    20 + MIN_RTO_MICROS,
                )
                .expect("duplicate ACK after guard")
                .loss_signals,
            vec![missing]
        );
    }

    #[test]
    fn three_later_sack_bits_signal_each_missing_packet_once() {
        let mut state = SendState::new(SequenceNumber::new(1));
        for byte in 1..=5 {
            data(&mut state, byte, 0);
        }
        let sack = [0b0000_0111, 0, 0, 0];
        let first = state
            .acknowledge(SequenceNumber::new(0), Some(&sack), PacketType::State, 10)
            .expect("SACK");
        assert_eq!(first.loss_signals, vec![SequenceNumber::new(1)]);
        assert_eq!(first.acknowledged_packets, 3);
        let repeated = state
            .acknowledge(SequenceNumber::new(0), Some(&sack), PacketType::State, 20)
            .expect("repeated SACK");
        assert!(repeated.loss_signals.is_empty());

        state
            .mark_retransmitted(SequenceNumber::new(1), 30)
            .expect("retransmit missing packet");
        let early = state
            .acknowledge(SequenceNumber::new(0), Some(&sack), PacketType::State, 31)
            .expect("SACK before retransmission guard");
        assert!(early.loss_signals.is_empty());
        let guarded = state
            .acknowledge(
                SequenceNumber::new(0),
                Some(&sack),
                PacketType::State,
                30 + MIN_RTO_MICROS,
            )
            .expect("SACK after retransmission guard");
        assert_eq!(guarded.loss_signals, vec![SequenceNumber::new(1)]);
    }

    #[test]
    fn retransmission_is_excluded_from_rtt_and_attempts_are_bounded() {
        let mut state = SendState::new(SequenceNumber::new(1));
        let first = data(&mut state, 1, 0);
        state.mark_retransmitted(first, 100).expect("retransmit");
        let ack = state
            .acknowledge(first, None, PacketType::State, 200)
            .expect("ACK retransmission");
        assert_eq!(ack.rtt_sample_micros, None);
        assert_eq!(state.snapshot().rtt.smoothed_rtt_micros, None);

        let second = data(&mut state, 2, 300);
        let ack = state
            .acknowledge(second, None, PacketType::State, 500)
            .expect("ACK original");
        assert_eq!(ack.rtt_sample_micros, Some(200));
        assert_eq!(state.snapshot().rtt.smoothed_rtt_micros, Some(200));
        assert_eq!(state.snapshot().rtt.base_rto_micros, MIN_RTO_MICROS);

        let third = data(&mut state, 3, 600);
        for attempt in 2..=MAX_TRANSMISSIONS {
            assert_eq!(
                state
                    .mark_retransmitted(third, 600 + u64::from(attempt))
                    .expect("bounded retransmit")
                    .transmissions,
                attempt
            );
        }
        let before = state.snapshot();
        assert!(matches!(
            state.mark_retransmitted(third, 1000),
            Err(SendError::TransmissionLimit { .. })
        ));
        assert_eq!(state.snapshot(), before);
    }

    #[test]
    fn timeout_backoff_saturates_and_valid_incoming_resets_it() {
        let mut state = SendState::new(SequenceNumber::new(1));
        let sequence = data(&mut state, 1, 0);
        let mut deadline = INITIAL_RTO_MICROS;
        let mut last_effective = 0;
        for _ in 0..10 {
            let timeout = state
                .on_timeout(deadline)
                .expect("timeout state")
                .expect("due timeout");
            assert_eq!(timeout.loss_signals, vec![sequence]);
            assert!(timeout.effective_rto_micros >= last_effective);
            last_effective = timeout.effective_rto_micros;
            deadline = timeout.next_deadline_micros;
        }
        assert_eq!(last_effective, MAX_RTO_MICROS);
        assert_eq!(state.snapshot().rtt.effective_rto_micros, MAX_RTO_MICROS);

        let duplicate = state
            .acknowledge(SequenceNumber::new(0), None, PacketType::State, deadline)
            .expect("valid duplicate ACK");
        assert_eq!(duplicate.disposition, AckDisposition::Duplicate);
        assert_eq!(state.snapshot().consecutive_timeouts, 10);

        state.on_valid_incoming(deadline);
        assert_eq!(state.snapshot().consecutive_timeouts, 0);
        assert_eq!(
            state.snapshot().timeout_deadline_micros,
            Some(deadline + INITIAL_RTO_MICROS)
        );

        state
            .acknowledge(sequence, None, PacketType::State, deadline)
            .expect("ACK progress");
        assert_eq!(state.snapshot().consecutive_timeouts, 0);
        assert_eq!(state.snapshot().timeout_deadline_micros, None);
    }

    #[test]
    fn packet_and_byte_limits_rollback_and_reset_releases_everything() {
        let mut packets = SendState::new(SequenceNumber::new(1));
        for _ in 0..MAX_SENT_PACKETS {
            data(&mut packets, 1, 0);
        }
        let before = packets.snapshot();
        assert_eq!(
            packets
                .record_sent(PacketType::Data, &[1], 0)
                .expect_err("packet limit"),
            SendError::SentPacketLimit {
                count: MAX_SENT_PACKETS + 1,
                maximum: MAX_SENT_PACKETS,
            }
        );
        assert_eq!(packets.snapshot(), before);

        let mut bytes = SendState::new(SequenceNumber::new(1));
        let chunk = vec![1; MAX_UTP_PAYLOAD_SIZE];
        for _ in 0..16 {
            bytes
                .record_sent(PacketType::Data, &chunk, 0)
                .expect("within byte bound");
        }
        let remainder = vec![1; MAX_SENT_BYTES - 16 * MAX_UTP_PAYLOAD_SIZE];
        bytes
            .record_sent(PacketType::Data, &remainder, 0)
            .expect("fill byte bound");
        let before = bytes.snapshot();
        assert!(matches!(
            bytes.record_sent(PacketType::Data, &[1], 0),
            Err(SendError::SentByteLimit { .. })
        ));
        assert_eq!(bytes.snapshot(), before);
        bytes.reset();
        assert_eq!(bytes.snapshot().outstanding_packets, 0);
        assert_eq!(bytes.snapshot().outstanding_bytes, 0);
        assert!(bytes.snapshot().terminal);
        assert_eq!(
            bytes.record_sent(PacketType::Data, &[1], 0),
            Err(SendError::Terminal)
        );
    }
}
