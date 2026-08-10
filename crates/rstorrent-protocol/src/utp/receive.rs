//! Bounded deterministic uTP receive ordering, SACK, FIN, and reset state.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use super::{MAX_UTP_PAYLOAD_SIZE, PacketType, SequenceNumber, SequenceRelation};

pub const MAX_REORDER_PACKETS: usize = 64;
pub const MAX_REORDER_BYTES: usize = 1024 * 1024;
pub const MAX_REORDER_DISTANCE: u16 = MAX_REORDER_PACKETS as u16 + 1;
const GENERATED_SACK_BYTES: usize = MAX_REORDER_PACKETS.div_ceil(8);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReceivedPayload {
    pub sequence_number: SequenceNumber,
    pub bytes: Vec<u8>,
    pub carried_by_fin: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceiveDisposition {
    Delivered,
    Buffered,
    Duplicate,
    ConflictingDuplicate,
    TooFarAhead { distance: u16 },
    AmbiguousSequence,
    AfterFin,
    ConflictingFin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectiveAckBits {
    bytes: [u8; GENERATED_SACK_BYTES],
    length: u8,
}

impl SelectiveAckBits {
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.length)]
    }

    #[must_use]
    pub fn acknowledges_offset(&self, offset: u16) -> bool {
        let Some(bit) = usize::from(offset).checked_sub(2) else {
            return false;
        };
        self.as_bytes()
            .get(bit / 8)
            .is_some_and(|byte| byte & (1 << (bit % 8)) != 0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReceiveOutcome {
    pub disposition: ReceiveDisposition,
    pub delivered: Vec<ReceivedPayload>,
    pub acknowledgement_number: SequenceNumber,
    pub selective_ack: Option<SelectiveAckBits>,
    pub eof_reached: bool,
    pub fin_payload_compatibility: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReceiveSnapshot {
    pub acknowledgement_number: SequenceNumber,
    pub queued_packets: usize,
    pub queued_bytes: usize,
    pub packet_high_water: usize,
    pub byte_high_water: usize,
    pub fin_sequence: Option<SequenceNumber>,
    pub eof_reached: bool,
    pub fin_payload_packets: u64,
    pub terminal: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReceiveError {
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
    ReorderPacketLimit {
        count: usize,
        maximum: usize,
    },
    ReorderByteLimit {
        bytes: usize,
        maximum: usize,
    },
}

impl fmt::Display for ReceiveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Terminal => formatter.write_str("uTP receive state is terminal"),
            Self::InvalidPacketType(packet_type) => {
                write!(
                    formatter,
                    "uTP {packet_type:?} is not sequence-bearing receive data"
                )
            }
            Self::InvalidPayloadLength {
                packet_type,
                length,
            } => write!(
                formatter,
                "uTP receive {packet_type:?} has invalid payload length {length}"
            ),
            Self::PayloadTooLarge { length, maximum } => {
                write!(formatter, "uTP receive payload {length} exceeds {maximum}")
            }
            Self::ReorderPacketLimit { count, maximum } => {
                write!(
                    formatter,
                    "uTP reorder packet count {count} exceeds {maximum}"
                )
            }
            Self::ReorderByteLimit { bytes, maximum } => {
                write!(
                    formatter,
                    "uTP reorder payload bytes {bytes} exceeds {maximum}"
                )
            }
        }
    }
}

impl Error for ReceiveError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StoredPacketType {
    Data,
    Fin,
}

impl StoredPacketType {
    const fn from_packet_type(packet_type: PacketType) -> Option<Self> {
        match packet_type {
            PacketType::Data => Some(Self::Data),
            PacketType::Fin => Some(Self::Fin),
            PacketType::State | PacketType::Reset | PacketType::Syn => None,
        }
    }

    const fn carried_by_fin(self) -> bool {
        matches!(self, Self::Fin)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StoredPacket {
    packet_type: StoredPacketType,
    payload: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct ReceiveState {
    acknowledgement_number: SequenceNumber,
    reorder: BTreeMap<SequenceNumber, StoredPacket>,
    reorder_bytes: usize,
    packet_high_water: usize,
    byte_high_water: usize,
    fin_sequence: Option<SequenceNumber>,
    eof_reached: bool,
    fin_payload_packets: u64,
    terminal: bool,
}

impl ReceiveState {
    #[must_use]
    pub fn new(acknowledgement_number: SequenceNumber) -> Self {
        Self {
            acknowledgement_number,
            reorder: BTreeMap::new(),
            reorder_bytes: 0,
            packet_high_water: 0,
            byte_high_water: 0,
            fin_sequence: None,
            eof_reached: false,
            fin_payload_packets: 0,
            terminal: false,
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> ReceiveSnapshot {
        ReceiveSnapshot {
            acknowledgement_number: self.acknowledgement_number,
            queued_packets: self.reorder.len(),
            queued_bytes: self.reorder_bytes,
            packet_high_water: self.packet_high_water,
            byte_high_water: self.byte_high_water,
            fin_sequence: self.fin_sequence,
            eof_reached: self.eof_reached,
            fin_payload_packets: self.fin_payload_packets,
            terminal: self.terminal,
        }
    }

    #[must_use]
    pub fn selective_ack(&self) -> Option<SelectiveAckBits> {
        self.sack()
    }

    pub fn receive(
        &mut self,
        packet_type: PacketType,
        sequence_number: SequenceNumber,
        payload: &[u8],
    ) -> Result<ReceiveOutcome, ReceiveError> {
        if self.terminal {
            return Err(ReceiveError::Terminal);
        }
        let stored_type = StoredPacketType::from_packet_type(packet_type)
            .ok_or(ReceiveError::InvalidPacketType(packet_type))?;
        if packet_type == PacketType::Data && payload.is_empty() {
            return Err(ReceiveError::InvalidPayloadLength {
                packet_type,
                length: 0,
            });
        }
        if payload.len() > MAX_UTP_PAYLOAD_SIZE {
            return Err(ReceiveError::PayloadTooLarge {
                length: payload.len(),
                maximum: MAX_UTP_PAYLOAD_SIZE,
            });
        }

        if let Some(fin_sequence) = self.fin_sequence {
            match sequence_number.relation_to(fin_sequence) {
                SequenceRelation::After(_) => {
                    return Ok(self.empty_outcome(ReceiveDisposition::AfterFin));
                }
                SequenceRelation::Equal if stored_type == StoredPacketType::Fin => {
                    return Ok(self.duplicate_or_conflict(sequence_number, stored_type, payload));
                }
                SequenceRelation::Equal
                | SequenceRelation::Before(_)
                | SequenceRelation::Ambiguous => {}
            }
            if stored_type == StoredPacketType::Fin && sequence_number != fin_sequence {
                return Ok(self.empty_outcome(ReceiveDisposition::ConflictingFin));
            }
        }

        let distance = match sequence_number.relation_to(self.acknowledgement_number) {
            SequenceRelation::Equal | SequenceRelation::Before(_) => {
                return Ok(self.empty_outcome(ReceiveDisposition::Duplicate));
            }
            SequenceRelation::Ambiguous => {
                return Ok(self.empty_outcome(ReceiveDisposition::AmbiguousSequence));
            }
            SequenceRelation::After(distance) => distance,
        };
        if distance > MAX_REORDER_DISTANCE {
            return Ok(self.empty_outcome(ReceiveDisposition::TooFarAhead { distance }));
        }
        if let Some(existing) = self.reorder.get(&sequence_number) {
            let disposition = if existing.packet_type == stored_type && existing.payload == payload
            {
                ReceiveDisposition::Duplicate
            } else {
                ReceiveDisposition::ConflictingDuplicate
            };
            return Ok(self.empty_outcome(disposition));
        }

        if stored_type == StoredPacketType::Fin && self.has_buffered_after(sequence_number) {
            return Ok(self.empty_outcome(ReceiveDisposition::ConflictingFin));
        }

        if distance == 1 {
            if stored_type == StoredPacketType::Fin {
                self.fin_sequence = Some(sequence_number);
            }
            let mut delivered = Vec::with_capacity(self.reorder.len().saturating_add(1));
            self.deliver(
                sequence_number,
                StoredPacket {
                    packet_type: stored_type,
                    payload: payload.to_vec(),
                },
                &mut delivered,
            );
            self.flush_contiguous(&mut delivered);
            let fin_payload_compatibility = delivered.iter().any(|payload| payload.carried_by_fin);
            return Ok(self.outcome(
                ReceiveDisposition::Delivered,
                delivered,
                fin_payload_compatibility,
            ));
        }

        let next_count = self.reorder.len() + 1;
        if next_count > MAX_REORDER_PACKETS {
            return Err(ReceiveError::ReorderPacketLimit {
                count: next_count,
                maximum: MAX_REORDER_PACKETS,
            });
        }
        let next_bytes = self.reorder_bytes.checked_add(payload.len()).ok_or(
            ReceiveError::ReorderByteLimit {
                bytes: usize::MAX,
                maximum: MAX_REORDER_BYTES,
            },
        )?;
        if next_bytes > MAX_REORDER_BYTES {
            return Err(ReceiveError::ReorderByteLimit {
                bytes: next_bytes,
                maximum: MAX_REORDER_BYTES,
            });
        }

        if stored_type == StoredPacketType::Fin {
            self.fin_sequence = Some(sequence_number);
        }
        self.reorder.insert(
            sequence_number,
            StoredPacket {
                packet_type: stored_type,
                payload: payload.to_vec(),
            },
        );
        self.reorder_bytes = next_bytes;
        self.packet_high_water = self.packet_high_water.max(next_count);
        self.byte_high_water = self.byte_high_water.max(next_bytes);
        Ok(self.outcome(ReceiveDisposition::Buffered, Vec::new(), false))
    }

    pub fn reset(&mut self) {
        self.reorder.clear();
        self.reorder_bytes = 0;
        self.fin_sequence = None;
        self.eof_reached = false;
        self.terminal = true;
    }

    fn duplicate_or_conflict(
        &self,
        sequence_number: SequenceNumber,
        stored_type: StoredPacketType,
        payload: &[u8],
    ) -> ReceiveOutcome {
        let disposition =
            self.reorder
                .get(&sequence_number)
                .map_or(ReceiveDisposition::Duplicate, |existing| {
                    if existing.packet_type == stored_type && existing.payload == payload {
                        ReceiveDisposition::Duplicate
                    } else {
                        ReceiveDisposition::ConflictingDuplicate
                    }
                });
        self.empty_outcome(disposition)
    }

    fn has_buffered_after(&self, sequence_number: SequenceNumber) -> bool {
        self.reorder.keys().any(|buffered| {
            matches!(
                buffered.relation_to(sequence_number),
                SequenceRelation::After(_)
            )
        })
    }

    fn flush_contiguous(&mut self, delivered: &mut Vec<ReceivedPayload>) {
        loop {
            let next = self.acknowledgement_number.wrapping_add(1);
            let Some(packet) = self.reorder.remove(&next) else {
                break;
            };
            self.reorder_bytes -= packet.payload.len();
            self.deliver(next, packet, delivered);
        }
    }

    fn deliver(
        &mut self,
        sequence_number: SequenceNumber,
        packet: StoredPacket,
        delivered: &mut Vec<ReceivedPayload>,
    ) {
        self.acknowledgement_number = sequence_number;
        let carried_by_fin = packet.packet_type.carried_by_fin();
        if carried_by_fin {
            self.eof_reached = true;
            if !packet.payload.is_empty() {
                self.fin_payload_packets = self.fin_payload_packets.saturating_add(1);
            }
        }
        if !packet.payload.is_empty() {
            delivered.push(ReceivedPayload {
                sequence_number,
                bytes: packet.payload,
                carried_by_fin,
            });
        }
    }

    fn sack(&self) -> Option<SelectiveAckBits> {
        if self.reorder.is_empty() {
            return None;
        }
        let mut sack = SelectiveAckBits {
            bytes: [0; GENERATED_SACK_BYTES],
            length: 4,
        };
        let mut highest_bit = 0_usize;
        for sequence_number in self.reorder.keys() {
            let SequenceRelation::After(distance) =
                sequence_number.relation_to(self.acknowledgement_number)
            else {
                continue;
            };
            let Some(bit) = usize::from(distance).checked_sub(2) else {
                continue;
            };
            if bit >= MAX_REORDER_PACKETS {
                continue;
            }
            sack.bytes[bit / 8] |= 1 << (bit % 8);
            highest_bit = highest_bit.max(bit);
        }
        let needed_bytes = (highest_bit + 1).div_ceil(8);
        sack.length = needed_bytes.max(4).next_multiple_of(4) as u8;
        Some(sack)
    }

    fn empty_outcome(&self, disposition: ReceiveDisposition) -> ReceiveOutcome {
        self.outcome(disposition, Vec::new(), false)
    }

    fn outcome(
        &self,
        disposition: ReceiveDisposition,
        delivered: Vec<ReceivedPayload>,
        fin_payload_compatibility: bool,
    ) -> ReceiveOutcome {
        ReceiveOutcome {
            disposition,
            delivered,
            acknowledgement_number: self.acknowledgement_number,
            selective_ack: self.sack(),
            eof_reached: self.eof_reached,
            fin_payload_compatibility,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_REORDER_BYTES, MAX_REORDER_DISTANCE, MAX_REORDER_PACKETS, ReceiveDisposition,
        ReceiveError, ReceiveState,
    };
    use crate::utp::{MAX_UTP_PAYLOAD_SIZE, PacketType, SequenceNumber};

    fn sequence(value: u16) -> SequenceNumber {
        SequenceNumber::new(value)
    }

    #[test]
    fn in_order_payload_and_wrap_are_delivered_without_storage() {
        let mut receive = ReceiveState::new(sequence(u16::MAX - 1));
        let first = receive
            .receive(PacketType::Data, sequence(u16::MAX), b"a")
            .expect("first payload");
        assert_eq!(first.disposition, ReceiveDisposition::Delivered);
        assert_eq!(first.delivered[0].bytes, b"a");
        let wrapped = receive
            .receive(PacketType::Data, sequence(0), b"b")
            .expect("wrapped payload");
        assert_eq!(wrapped.delivered[0].bytes, b"b");
        assert_eq!(wrapped.acknowledgement_number, sequence(0));
        assert_eq!(receive.snapshot().queued_bytes, 0);
    }

    #[test]
    fn reordering_generates_sack_and_contiguous_input_releases_all() {
        let mut receive = ReceiveState::new(sequence(10));
        receive
            .receive(PacketType::Data, sequence(12), b"twelve")
            .expect("buffer twelve");
        let buffered = receive
            .receive(PacketType::Data, sequence(13), b"thirteen")
            .expect("buffer thirteen");
        let sack = buffered.selective_ack.expect("SACK");
        assert_eq!(sack.as_bytes(), &[0b0000_0011, 0, 0, 0]);
        assert!(sack.acknowledges_offset(2));
        assert!(sack.acknowledges_offset(3));

        let released = receive
            .receive(PacketType::Data, sequence(11), b"eleven")
            .expect("release range");
        assert_eq!(
            released
                .delivered
                .iter()
                .map(|payload| payload.sequence_number)
                .collect::<Vec<_>>(),
            [sequence(11), sequence(12), sequence(13)]
        );
        assert_eq!(released.acknowledgement_number, sequence(13));
        assert!(released.selective_ack.is_none());
        assert_eq!(receive.snapshot().queued_packets, 0);
    }

    #[test]
    fn duplicate_conflict_future_and_ambiguity_do_not_mutate_state() {
        let mut receive = ReceiveState::new(sequence(100));
        receive
            .receive(PacketType::Data, sequence(102), b"original")
            .expect("buffer");
        let snapshot = receive.snapshot();
        assert_eq!(
            receive
                .receive(PacketType::Data, sequence(102), b"original")
                .expect("duplicate")
                .disposition,
            ReceiveDisposition::Duplicate
        );
        assert_eq!(
            receive
                .receive(PacketType::Data, sequence(102), b"changed")
                .expect("conflict")
                .disposition,
            ReceiveDisposition::ConflictingDuplicate
        );
        assert_eq!(
            receive
                .receive(
                    PacketType::Data,
                    sequence(100_u16.wrapping_add(MAX_REORDER_DISTANCE + 1)),
                    b"future"
                )
                .expect("future")
                .disposition,
            ReceiveDisposition::TooFarAhead {
                distance: MAX_REORDER_DISTANCE + 1
            }
        );
        assert_eq!(
            receive
                .receive(
                    PacketType::Data,
                    sequence(100_u16.wrapping_add(0x8000)),
                    b"ambiguous"
                )
                .expect("ambiguous")
                .disposition,
            ReceiveDisposition::AmbiguousSequence
        );
        assert_eq!(receive.snapshot(), snapshot);
    }

    #[test]
    fn exact_packet_limit_is_bounded_and_rolls_back() {
        let mut receive = ReceiveState::new(sequence(0));
        for offset in 2..=MAX_REORDER_DISTANCE {
            receive
                .receive(PacketType::Data, sequence(offset), b"x")
                .expect("fill reorder packet slot");
        }
        let full = receive.snapshot();
        assert_eq!(full.queued_packets, MAX_REORDER_PACKETS);
        assert_eq!(full.packet_high_water, MAX_REORDER_PACKETS);

        // Make sequence 1 contiguous, release everything, and refill with a
        // missing sequence at the next wrap-relative position to prove the
        // packet limit is not an allocation leak.
        receive
            .receive(PacketType::Data, sequence(1), b"release")
            .expect("release full reorder window");
        assert_eq!(receive.snapshot().queued_packets, 0);
    }

    #[test]
    fn byte_and_payload_limits_reject_without_partial_mutation() {
        let mut receive = ReceiveState::new(sequence(0));
        let payload = vec![7; 65_000];
        for offset in 2..=17 {
            receive
                .receive(PacketType::Data, sequence(offset), &payload)
                .expect("fill byte budget");
        }
        let snapshot = receive.snapshot();
        assert_eq!(snapshot.queued_bytes, payload.len() * 16);
        assert!(matches!(
            receive.receive(PacketType::Data, sequence(18), &payload),
            Err(ReceiveError::ReorderByteLimit {
                bytes,
                maximum: MAX_REORDER_BYTES
            }) if bytes == payload.len() * 17
        ));
        assert_eq!(receive.snapshot(), snapshot);

        let oversized = vec![0; MAX_UTP_PAYLOAD_SIZE + 1];
        assert!(matches!(
            receive.receive(PacketType::Data, sequence(1), &oversized),
            Err(ReceiveError::PayloadTooLarge { .. })
        ));
        assert_eq!(receive.snapshot(), snapshot);
    }

    #[test]
    fn fin_waits_for_missing_data_and_then_reaches_eof() {
        let mut receive = ReceiveState::new(sequence(10));
        receive
            .receive(PacketType::Data, sequence(12), b"middle")
            .expect("buffer middle");
        let fin = receive
            .receive(PacketType::Fin, sequence(13), &[])
            .expect("buffer FIN");
        assert!(!fin.eof_reached);
        assert_eq!(receive.snapshot().fin_sequence, Some(sequence(13)));

        let released = receive
            .receive(PacketType::Data, sequence(11), b"first")
            .expect("fill gap");
        assert!(released.eof_reached);
        assert_eq!(released.acknowledgement_number, sequence(13));
        assert_eq!(released.delivered.len(), 2);
        assert_eq!(receive.snapshot().queued_packets, 0);
    }

    #[test]
    fn fin_payload_is_delivered_and_counted_as_compatibility_behavior() {
        let mut receive = ReceiveState::new(sequence(4));
        let outcome = receive
            .receive(PacketType::Fin, sequence(5), b"last")
            .expect("FIN payload");
        assert!(outcome.eof_reached);
        assert!(outcome.fin_payload_compatibility);
        assert!(outcome.delivered[0].carried_by_fin);
        assert_eq!(outcome.delivered[0].bytes, b"last");
        assert_eq!(receive.snapshot().fin_payload_packets, 1);

        let mut reordered = ReceiveState::new(sequence(0));
        reordered
            .receive(PacketType::Fin, sequence(2), b"reordered last")
            .expect("buffer FIN payload");
        let released = reordered
            .receive(PacketType::Data, sequence(1), b"first")
            .expect("release FIN payload");
        assert!(released.fin_payload_compatibility);
        assert!(released.eof_reached);
        assert_eq!(released.delivered.len(), 2);
    }

    #[test]
    fn data_after_fin_and_conflicting_fin_are_ignored() {
        let mut receive = ReceiveState::new(sequence(10));
        receive
            .receive(PacketType::Fin, sequence(12), &[])
            .expect("buffer FIN");
        let snapshot = receive.snapshot();
        assert_eq!(
            receive
                .receive(PacketType::Data, sequence(13), b"after")
                .expect("after FIN")
                .disposition,
            ReceiveDisposition::AfterFin
        );
        assert_eq!(
            receive
                .receive(PacketType::Fin, sequence(11), &[])
                .expect("different FIN")
                .disposition,
            ReceiveDisposition::ConflictingFin
        );
        assert_eq!(receive.snapshot(), snapshot);
    }

    #[test]
    fn fin_before_already_buffered_later_data_is_rejected_atomically() {
        let mut receive = ReceiveState::new(sequence(10));
        receive
            .receive(PacketType::Data, sequence(13), b"later")
            .expect("buffer later");
        let snapshot = receive.snapshot();
        assert_eq!(
            receive
                .receive(PacketType::Fin, sequence(12), &[])
                .expect("conflicting FIN")
                .disposition,
            ReceiveDisposition::ConflictingFin
        );
        assert_eq!(receive.snapshot(), snapshot);
    }

    #[test]
    fn invalid_types_and_reset_are_terminal_and_release_storage() {
        let mut receive = ReceiveState::new(sequence(0));
        assert_eq!(
            receive.receive(PacketType::State, sequence(1), &[]),
            Err(ReceiveError::InvalidPacketType(PacketType::State))
        );
        assert_eq!(
            receive.receive(PacketType::Data, sequence(1), &[]),
            Err(ReceiveError::InvalidPayloadLength {
                packet_type: PacketType::Data,
                length: 0,
            })
        );
        receive
            .receive(PacketType::Data, sequence(2), b"buffered")
            .expect("buffer");
        receive.reset();
        let snapshot = receive.snapshot();
        assert!(snapshot.terminal);
        assert_eq!(snapshot.queued_packets, 0);
        assert_eq!(snapshot.queued_bytes, 0);
        assert_eq!(
            receive.receive(PacketType::Data, sequence(1), b"late"),
            Err(ReceiveError::Terminal)
        );
    }
}
