//! Bounded uTP stream queue, packetization, ACK, and retransmission scheduling.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt;

use super::{
    CongestionController, CongestionError, CongestionSnapshot, ConnectionError, ConnectionPhase,
    ConnectionSnapshot, ConnectionState, DecodedPacket, ExtensionToEncode, IncomingDisposition,
    IncomingOutcome, MAX_SENT_BYTES, MAX_SENT_PACKETS, MtuError, MtuProbeFailure, MtuProbeOutcome,
    OutboundPacketIntent, Pacer, PacerSnapshot, PacketToEncode, PacketType, PathMtuSnapshot,
    PathMtuState, ReceiveDisposition, SACK_EXTENSION, SequenceNumber, TimestampMicros,
    UTP_HEADER_SIZE, UtpCodecError, UtpHeader, encode_packet,
};

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransportEmission {
    pub intent: OutboundPacketIntent,
    pub timestamp: TimestampMicros,
    pub timestamp_difference_micros: u32,
    pub payload: Vec<u8>,
    pub retransmission: bool,
    pub mtu_probe: bool,
    pub fragmentable_mtu_retry: bool,
    pub dont_fragment: bool,
}

impl TransportEmission {
    #[must_use]
    pub fn datagram_bytes(&self) -> usize {
        let selective_ack_bytes = self
            .intent
            .selective_ack
            .map_or(0, |selective_ack| selective_ack.as_bytes().len());
        utp_header_bytes(selective_ack_bytes) + self.payload.len()
    }

    pub fn encode(&self) -> Result<Vec<u8>, UtpCodecError> {
        let header = UtpHeader {
            packet_type: self.intent.packet_type,
            connection_id: self.intent.connection_id,
            timestamp: self.timestamp,
            timestamp_difference_micros: self.timestamp_difference_micros,
            window_size: self.intent.window_size,
            sequence_number: self.intent.sequence_number,
            acknowledgement_number: self.intent.acknowledgement_number,
        };
        if let Some(selective_ack) = self.intent.selective_ack {
            let extensions = [ExtensionToEncode {
                kind: SACK_EXTENSION,
                bytes: selective_ack.as_bytes(),
            }];
            encode_packet(PacketToEncode {
                header,
                extensions: &extensions,
                payload: &self.payload,
            })
        } else {
            encode_packet(PacketToEncode {
                header,
                extensions: &[],
                payload: &self.payload,
            })
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatagramSendResult {
    Sent,
    WouldBlock,
    MessageTooLarge,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransportSnapshot {
    pub connection: ConnectionSnapshot,
    pub transmit: TransmitQueueSnapshot,
    pub acknowledgements: AckSchedulerSnapshot,
    pub retransmissions: RetransmissionSnapshot,
    pub congestion: CongestionSnapshot,
    pub mtu: PathMtuSnapshot,
    pub pacer: PacerSnapshot,
    pub remote_window_bytes: usize,
    pub in_flight_packets: usize,
    pub in_flight_bytes: usize,
    pub in_flight_packet_high_water: usize,
    pub in_flight_byte_high_water: usize,
    pub congestion_control_acknowledgements: u64,
    pub congestion_control_acknowledged_bytes: u64,
    pub congestion_limited_acknowledgements: u64,
    pub sender_underfilled_acknowledgements: u64,
    pub remote_window_limited_acknowledgements: u64,
    pub window_growth_acknowledgements: u64,
    pub timestamp_difference_micros: u32,
    pub pending_emission_bytes: usize,
    pub close_requested: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransportIncomingOutcome {
    pub connection: IncomingOutcome,
    pub remote_window_bytes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransportError {
    Connection(ConnectionError),
    Congestion(CongestionError),
    Mtu(MtuError),
    Queue(TransmitQueueError),
    Retransmission(RetransmissionLimit),
    MissingOutstandingPacket(SequenceNumber),
    RetransmissionDatagramLimit {
        sequence_number: SequenceNumber,
        bytes: usize,
        maximum: usize,
    },
    PendingEmissionMismatch {
        expected_sequence: SequenceNumber,
        actual_sequence: SequenceNumber,
    },
}

impl fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connection(error) => write!(formatter, "uTP transport connection: {error}"),
            Self::Congestion(error) => write!(formatter, "uTP transport congestion: {error}"),
            Self::Mtu(error) => write!(formatter, "uTP transport MTU: {error}"),
            Self::Queue(error) => write!(formatter, "uTP transport queue: {error}"),
            Self::Retransmission(error) => {
                write!(formatter, "uTP transport retransmission: {error}")
            }
            Self::MissingOutstandingPacket(sequence_number) => write!(
                formatter,
                "uTP transport packet {} is no longer outstanding",
                sequence_number.get()
            ),
            Self::RetransmissionDatagramLimit {
                sequence_number,
                bytes,
                maximum,
            } => write!(
                formatter,
                "uTP retransmission packet {} requires {bytes} bytes above datagram limit {maximum}",
                sequence_number.get()
            ),
            Self::PendingEmissionMismatch {
                expected_sequence,
                actual_sequence,
            } => write!(
                formatter,
                "uTP send result for packet {} does not match pending packet {}",
                actual_sequence.get(),
                expected_sequence.get()
            ),
        }
    }
}

impl Error for TransportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Connection(error) => Some(error),
            Self::Congestion(error) => Some(error),
            Self::Mtu(error) => Some(error),
            Self::Queue(error) => Some(error),
            Self::Retransmission(error) => Some(error),
            Self::MissingOutstandingPacket(_)
            | Self::RetransmissionDatagramLimit { .. }
            | Self::PendingEmissionMismatch { .. } => None,
        }
    }
}

impl From<ConnectionError> for TransportError {
    fn from(error: ConnectionError) -> Self {
        Self::Connection(error)
    }
}

impl From<CongestionError> for TransportError {
    fn from(error: CongestionError) -> Self {
        Self::Congestion(error)
    }
}

impl From<MtuError> for TransportError {
    fn from(error: MtuError) -> Self {
        Self::Mtu(error)
    }
}

impl From<TransmitQueueError> for TransportError {
    fn from(error: TransmitQueueError) -> Self {
        Self::Queue(error)
    }
}

impl From<RetransmissionLimit> for TransportError {
    fn from(error: RetransmissionLimit) -> Self {
        Self::Retransmission(error)
    }
}

#[derive(Clone, Debug)]
pub struct TransportState {
    connection: ConnectionState,
    transmit: TransmitQueue,
    acknowledgements: AckScheduler,
    retransmissions: RetransmissionQueue,
    congestion: CongestionController,
    mtu: PathMtuState,
    pacer: Pacer,
    remote_window_bytes: usize,
    in_flight: BTreeMap<SequenceNumber, usize>,
    in_flight_packet_high_water: usize,
    in_flight_byte_high_water: usize,
    congestion_control_acknowledgements: u64,
    congestion_control_acknowledged_bytes: u64,
    congestion_limited_acknowledgements: u64,
    sender_underfilled_acknowledgements: u64,
    remote_window_limited_acknowledgements: u64,
    window_growth_acknowledgements: u64,
    timestamp_difference_micros: u32,
    initial_syn_pending: bool,
    pending_emission: Option<TransportEmission>,
    close_requested: bool,
}

impl TransportState {
    pub fn initiate(
        receive_connection_id: u16,
        initial_sequence_number: SequenceNumber,
        now_micros: u64,
        floor_datagram_bytes: usize,
        ceiling_datagram_bytes: usize,
    ) -> Result<Self, TransportError> {
        let connection =
            ConnectionState::initiate(receive_connection_id, initial_sequence_number, now_micros)?;
        Self::from_connection(
            connection,
            0,
            floor_datagram_bytes,
            ceiling_datagram_bytes,
            true,
            false,
        )
    }

    pub fn accept_syn(
        syn: DecodedPacket<'_>,
        initial_sequence_number: SequenceNumber,
        floor_datagram_bytes: usize,
        ceiling_datagram_bytes: usize,
    ) -> Result<Self, TransportError> {
        let remote_window_bytes = syn.header.window_size as usize;
        let connection = ConnectionState::accept_syn(syn, initial_sequence_number)?;
        Self::from_connection(
            connection,
            remote_window_bytes,
            floor_datagram_bytes,
            ceiling_datagram_bytes,
            false,
            true,
        )
    }

    fn from_connection(
        connection: ConnectionState,
        remote_window_bytes: usize,
        floor_datagram_bytes: usize,
        ceiling_datagram_bytes: usize,
        initial_syn_pending: bool,
        initial_ack_pending: bool,
    ) -> Result<Self, TransportError> {
        let mtu = PathMtuState::new(floor_datagram_bytes, ceiling_datagram_bytes)?;
        let maximum_segment_bytes = floor_datagram_bytes.saturating_sub(UTP_HEADER_SIZE);
        let congestion = CongestionController::new(maximum_segment_bytes)?;
        let mut acknowledgements = AckScheduler::default();
        if initial_ack_pending {
            acknowledgements.on_window_reopened(0);
        }
        Ok(Self {
            connection,
            transmit: TransmitQueue::new(),
            acknowledgements,
            retransmissions: RetransmissionQueue::default(),
            congestion,
            mtu,
            pacer: Pacer::default(),
            remote_window_bytes,
            in_flight: BTreeMap::new(),
            in_flight_packet_high_water: 0,
            in_flight_byte_high_water: 0,
            congestion_control_acknowledgements: 0,
            congestion_control_acknowledged_bytes: 0,
            congestion_limited_acknowledgements: 0,
            sender_underfilled_acknowledgements: 0,
            remote_window_limited_acknowledgements: 0,
            window_growth_acknowledgements: 0,
            timestamp_difference_micros: 0,
            initial_syn_pending,
            pending_emission: None,
            close_requested: false,
        })
    }

    #[must_use]
    pub fn snapshot(&self) -> TransportSnapshot {
        TransportSnapshot {
            connection: self.connection.snapshot(),
            transmit: self.transmit.snapshot(),
            acknowledgements: self.acknowledgements.snapshot(),
            retransmissions: self.retransmissions.snapshot(),
            congestion: self.congestion.snapshot(),
            mtu: self.mtu.snapshot(),
            pacer: self.pacer.snapshot(),
            remote_window_bytes: self.remote_window_bytes,
            in_flight_packets: self.in_flight.len(),
            in_flight_bytes: self.in_flight_bytes(),
            in_flight_packet_high_water: self.in_flight_packet_high_water,
            in_flight_byte_high_water: self.in_flight_byte_high_water,
            congestion_control_acknowledgements: self.congestion_control_acknowledgements,
            congestion_control_acknowledged_bytes: self.congestion_control_acknowledged_bytes,
            congestion_limited_acknowledgements: self.congestion_limited_acknowledgements,
            sender_underfilled_acknowledgements: self.sender_underfilled_acknowledgements,
            remote_window_limited_acknowledgements: self.remote_window_limited_acknowledgements,
            window_growth_acknowledgements: self.window_growth_acknowledgements,
            timestamp_difference_micros: self.timestamp_difference_micros,
            pending_emission_bytes: self
                .pending_emission
                .as_ref()
                .map_or(0, TransportEmission::datagram_bytes),
            close_requested: self.close_requested,
        }
    }

    pub fn queue_data(&mut self, bytes: &[u8]) -> Result<(), TransportError> {
        if self.close_requested {
            return Err(TransportError::Queue(TransmitQueueError::Terminal));
        }
        Ok(self.transmit.append(bytes)?)
    }

    pub fn request_close(&mut self) {
        self.close_requested = true;
    }

    pub fn finish(&mut self) -> Result<(), TransportError> {
        self.connection.finish()?;
        self.release_terminal_ownership();
        Ok(())
    }

    #[must_use]
    pub fn next_wakeup_micros(&self) -> Option<u64> {
        let snapshot = self.snapshot();
        if matches!(
            snapshot.connection.phase,
            ConnectionPhase::Reset | ConnectionPhase::Closed
        ) {
            return None;
        }
        let mut deadline = snapshot.acknowledgements.deadline_micros;
        deadline = minimum_deadline(deadline, snapshot.connection.send.timeout_deadline_micros);
        if snapshot.retransmissions.pending_packets > 0 {
            deadline = minimum_deadline(deadline, Some(snapshot.pacer.next_send_micros));
        }
        if snapshot.transmit.unsent_bytes > 0
            && snapshot.connection.send.outstanding_packets < MAX_SENT_PACKETS
            && snapshot.connection.send.outstanding_bytes < MAX_SENT_BYTES
            && snapshot.in_flight_bytes < snapshot.congestion.congestion_window_bytes
            && snapshot.in_flight_bytes < snapshot.remote_window_bytes
            && matches!(
                snapshot.connection.phase,
                ConnectionPhase::Connected | ConnectionPhase::RemoteFinReceived
            )
            && snapshot.retransmissions.pending_packets == 0
        {
            deadline = minimum_deadline(deadline, Some(0));
        }
        if snapshot.close_requested
            && snapshot.transmit.unsent_bytes == 0
            && !snapshot.connection.send.fin_sent
            && snapshot.connection.send.outstanding_packets < MAX_SENT_PACKETS
            && snapshot.retransmissions.pending_packets == 0
            && matches!(
                snapshot.connection.phase,
                ConnectionPhase::Connected | ConnectionPhase::RemoteFinReceived
            )
        {
            deadline = minimum_deadline(deadline, Some(0));
        }
        deadline
    }

    pub fn consume_received(
        &mut self,
        bytes: usize,
        now_micros: u64,
    ) -> Result<usize, TransportError> {
        let was_zero = self
            .connection
            .snapshot()
            .receive
            .is_some_and(|receive| receive.advertised_window_bytes == 0);
        let available = self.connection.consume_received(bytes)?;
        if was_zero && available > 0 {
            self.acknowledgements.on_window_reopened(now_micros);
        }
        Ok(available)
    }

    pub fn incoming(
        &mut self,
        packet: DecodedPacket<'_>,
        now_micros: u64,
        local_timestamp: TimestampMicros,
    ) -> Result<TransportIncomingOutcome, TransportError> {
        let previous_remote_window = self.remote_window_bytes;
        let flight_size_before_ack = self.in_flight_bytes();
        let remote_timestamp = packet.header.timestamp;
        let advertised_window = packet.header.window_size as usize;
        let one_way_delay_micros = packet.header.timestamp_difference_micros;
        let packet_type = packet.header.packet_type;
        let outcome = self.connection.incoming(packet, now_micros)?;
        if outcome.disposition != IncomingDisposition::Accepted {
            return Ok(TransportIncomingOutcome {
                connection: outcome,
                remote_window_bytes: self.remote_window_bytes,
            });
        }

        self.remote_window_bytes = advertised_window;
        self.timestamp_difference_micros = local_timestamp.elapsed_since(remote_timestamp);
        if let Some(acknowledgement) = &outcome.acknowledgement {
            let smoothed_rtt = self.connection.snapshot().send.rtt.smoothed_rtt_micros;
            for sequence_number in &acknowledgement.acknowledged_sequences {
                self.in_flight.remove(sequence_number);
                self.retransmissions.remove(*sequence_number);
                let mtu_outcome =
                    self.mtu
                        .on_acknowledged(*sequence_number, now_micros, smoothed_rtt);
                if matches!(mtu_outcome, MtuProbeOutcome::RaisedFloor { .. }) {
                    self.update_congestion_mss()?;
                }
            }

            if acknowledgement.acknowledged_bytes > 0 && one_way_delay_micros != 0 {
                let congestion_snapshot = self.congestion.snapshot();
                let mss = congestion_snapshot.maximum_segment_bytes;
                let next_segment_flight = flight_size_before_ack.saturating_add(mss);
                let sender_has_window_headroom =
                    next_segment_flight <= congestion_snapshot.congestion_window_bytes;
                let remote_window_limited = next_segment_flight > previous_remote_window;
                let congestion_limited = !sender_has_window_headroom && !remote_window_limited;
                self.congestion_control_acknowledgements =
                    self.congestion_control_acknowledgements.saturating_add(1);
                self.congestion_control_acknowledged_bytes =
                    self.congestion_control_acknowledged_bytes.saturating_add(
                        u64::try_from(acknowledgement.acknowledged_bytes).unwrap_or(u64::MAX),
                    );
                if congestion_limited {
                    self.congestion_limited_acknowledgements =
                        self.congestion_limited_acknowledgements.saturating_add(1);
                }
                if self.transmit.snapshot().unsent_bytes > 0
                    && sender_has_window_headroom
                    && !remote_window_limited
                {
                    self.sender_underfilled_acknowledgements =
                        self.sender_underfilled_acknowledgements.saturating_add(1);
                }
                if remote_window_limited {
                    self.remote_window_limited_acknowledgements = self
                        .remote_window_limited_acknowledgements
                        .saturating_add(1);
                }
                let congestion_outcome = self.congestion.on_ack(
                    now_micros,
                    acknowledgement.acknowledged_bytes,
                    flight_size_before_ack,
                    one_way_delay_micros,
                    smoothed_rtt,
                    congestion_limited,
                )?;
                if congestion_outcome.window_delta_bytes > 0 {
                    self.window_growth_acknowledgements =
                        self.window_growth_acknowledgements.saturating_add(1);
                }
            }

            let only_loss = acknowledgement.loss_signals.len() == 1;
            for sequence_number in &acknowledgement.loss_signals {
                let mtu_snapshot = self.mtu.snapshot();
                let is_active_probe = mtu_snapshot
                    .active_probe
                    .is_some_and(|probe| probe.sequence_number == *sequence_number);
                let is_fragmentable_probe_retry = mtu_snapshot
                    .fragmentable_retry
                    .is_some_and(|probe| probe.sequence_number == *sequence_number);
                let isolated_probe = only_loss && (is_active_probe || is_fragmentable_probe_retry);
                if only_loss
                    && is_fragmentable_probe_retry
                    && self.in_flight.contains_key(sequence_number)
                {
                    self.congestion.on_loss(now_micros, smoothed_rtt, true)?;
                    continue;
                }
                self.in_flight.remove(sequence_number);
                if is_active_probe {
                    let mtu_outcome = self.mtu.on_probe_loss(
                        *sequence_number,
                        if isolated_probe {
                            MtuProbeFailure::ThreeLaterAcknowledgements
                        } else {
                            MtuProbeFailure::CongestionOrUnknown
                        },
                        now_micros,
                        smoothed_rtt,
                    );
                    if matches!(
                        mtu_outcome,
                        MtuProbeOutcome::LoweredCeiling {
                            previous_floor,
                            floor,
                            ..
                        } if previous_floor != floor
                    ) {
                        self.update_congestion_mss()?;
                    }
                }
                self.retransmissions.schedule([*sequence_number])?;
                self.congestion
                    .on_loss(now_micros, smoothed_rtt, isolated_probe)?;
            }
        }

        if let Some(receive) = &outcome.receive {
            self.acknowledgements.on_receive(
                packet_type,
                receive.disposition,
                now_micros,
                self.connection.snapshot().send.rtt.smoothed_rtt_micros,
            );
        }
        if outcome.phase == ConnectionPhase::Reset {
            self.release_terminal_ownership();
        }
        Ok(TransportIncomingOutcome {
            connection: outcome,
            remote_window_bytes: self.remote_window_bytes,
        })
    }

    pub fn poll_transmit(
        &mut self,
        now_micros: u64,
        local_timestamp: TimestampMicros,
    ) -> Result<Option<TransportEmission>, TransportError> {
        if let Some(pending) = &self.pending_emission {
            return Ok(Some(pending.clone()));
        }
        let smoothed_rtt = self.connection.snapshot().send.rtt.smoothed_rtt_micros;
        self.congestion.advance_time(now_micros, smoothed_rtt)?;
        self.process_timeout(now_micros, smoothed_rtt)?;

        if self.initial_syn_pending {
            let intent =
                self.connection
                    .syn_intent()
                    .ok_or(TransportError::MissingOutstandingPacket(
                        self.connection.snapshot().send.next_sequence_number,
                    ))?;
            self.initial_syn_pending = false;
            return Ok(Some(self.install_pending_emission(TransportEmission {
                intent,
                timestamp: local_timestamp,
                timestamp_difference_micros: 0,
                payload: Vec::new(),
                retransmission: false,
                mtu_probe: false,
                fragmentable_mtu_retry: false,
                dont_fragment: false,
            })));
        }

        if self.pacer.is_ready(now_micros)
            && let Some(emission) = self.poll_retransmission(now_micros, local_timestamp)?
        {
            return Ok(Some(self.install_pending_emission(emission)));
        }
        if self.retransmissions.front().is_none()
            && let Some(emission) = self.poll_new_data(now_micros, local_timestamp)?
        {
            return Ok(Some(self.install_pending_emission(emission)));
        }
        if self.retransmissions.front().is_none()
            && let Some(emission) = self.poll_fin(now_micros, local_timestamp)?
        {
            return Ok(Some(self.install_pending_emission(emission)));
        }

        if self.acknowledgements.is_due(now_micros) {
            let intent = self.connection.state_intent()?;
            self.acknowledgements.acknowledge();
            return Ok(Some(self.install_pending_emission(TransportEmission {
                intent,
                timestamp: local_timestamp,
                timestamp_difference_micros: self.timestamp_difference_micros,
                payload: Vec::new(),
                retransmission: false,
                mtu_probe: false,
                fragmentable_mtu_retry: false,
                dont_fragment: false,
            })));
        }
        Ok(None)
    }

    /// Emits only an already-owed receive acknowledgement, regardless of its
    /// normal delayed-ACK deadline. It never admits application payload.
    pub fn poll_pending_acknowledgement(
        &mut self,
        local_timestamp: TimestampMicros,
    ) -> Result<Option<TransportEmission>, TransportError> {
        if self.pending_emission.is_some()
            || self.acknowledgements.snapshot().pending_packets == 0
            || matches!(
                self.connection.snapshot().phase,
                ConnectionPhase::Reset | ConnectionPhase::Closed
            )
        {
            return Ok(None);
        }
        let intent = self.connection.state_intent()?;
        self.acknowledgements.acknowledge();
        Ok(Some(self.install_pending_emission(TransportEmission {
            intent,
            timestamp: local_timestamp,
            timestamp_difference_micros: self.timestamp_difference_micros,
            payload: Vec::new(),
            retransmission: false,
            mtu_probe: false,
            fragmentable_mtu_retry: false,
            dont_fragment: false,
        })))
    }

    pub fn on_send_result(
        &mut self,
        sequence_number: SequenceNumber,
        result: DatagramSendResult,
        now_micros: u64,
    ) -> Result<(), TransportError> {
        let pending = self
            .pending_emission
            .as_ref()
            .ok_or(TransportError::MissingOutstandingPacket(sequence_number))?;
        if pending.intent.sequence_number != sequence_number {
            return Err(TransportError::PendingEmissionMismatch {
                expected_sequence: pending.intent.sequence_number,
                actual_sequence: sequence_number,
            });
        }
        match result {
            DatagramSendResult::Sent => {
                self.pending_emission = None;
            }
            DatagramSendResult::WouldBlock => {}
            DatagramSendResult::MessageTooLarge => {
                let pending = self
                    .pending_emission
                    .take()
                    .expect("pending emission was validated");
                let mtu_outcome = self.mtu.on_message_too_large(
                    sequence_number,
                    pending.datagram_bytes(),
                    now_micros,
                    self.connection.snapshot().send.rtt.smoothed_rtt_micros,
                )?;
                if let MtuProbeOutcome::LoweredCeiling {
                    isolated_from_congestion,
                    previous_floor,
                    floor,
                    ..
                } = mtu_outcome
                {
                    if previous_floor != floor {
                        self.update_congestion_mss()?;
                    }
                    self.in_flight.remove(&sequence_number);
                    self.retransmissions.schedule([sequence_number])?;
                    self.congestion.on_loss(
                        now_micros,
                        self.connection.snapshot().send.rtt.smoothed_rtt_micros,
                        isolated_from_congestion,
                    )?;
                    self.pacer.reset(now_micros);
                }
            }
        }
        Ok(())
    }

    pub fn abort(&mut self) {
        self.connection.abort();
        self.release_terminal_ownership();
    }

    fn poll_fin(
        &mut self,
        now_micros: u64,
        local_timestamp: TimestampMicros,
    ) -> Result<Option<TransportEmission>, TransportError> {
        let snapshot = self.snapshot();
        if !self.close_requested
            || snapshot.transmit.unsent_bytes != 0
            || snapshot.connection.send.fin_sent
            || snapshot.connection.send.outstanding_packets >= MAX_SENT_PACKETS
            || !matches!(
                snapshot.connection.phase,
                ConnectionPhase::Connected | ConnectionPhase::RemoteFinReceived
            )
        {
            return Ok(None);
        }
        let intent = self.connection.record_fin(&[], now_micros)?;
        self.record_in_flight(intent.sequence_number, 0);
        self.acknowledgements.acknowledge();
        Ok(Some(TransportEmission {
            intent,
            timestamp: local_timestamp,
            timestamp_difference_micros: self.timestamp_difference_micros,
            payload: Vec::new(),
            retransmission: false,
            mtu_probe: false,
            fragmentable_mtu_retry: false,
            dont_fragment: false,
        }))
    }

    fn poll_retransmission(
        &mut self,
        now_micros: u64,
        local_timestamp: TimestampMicros,
    ) -> Result<Option<TransportEmission>, TransportError> {
        let Some(sequence_number) = self.retransmissions.front() else {
            return Ok(None);
        };
        let payload = self
            .connection
            .payload_for_retransmission(sequence_number)
            .ok_or(TransportError::MissingOutstandingPacket(sequence_number))?
            .to_vec();
        let congestion_window = self.congestion.snapshot().congestion_window_bytes;
        if !retransmission_is_admissible(
            payload.len(),
            congestion_window,
            self.remote_window_bytes,
            self.in_flight_bytes(),
        ) {
            return Ok(None);
        }
        let mut intent = self.connection.retransmission_intent(sequence_number)?;
        let mtu_snapshot = self.mtu.snapshot();
        let fragmentable_mtu_retry = mtu_snapshot
            .fragmentable_retry
            .filter(|retry| retry.sequence_number == sequence_number)
            .map(|retry| retry.datagram_bytes);
        let datagram_limit = fragmentable_mtu_retry.unwrap_or(mtu_snapshot.floor_datagram_bytes);
        let sack_bytes = intent
            .selective_ack
            .map_or(0, |selective_ack| selective_ack.as_bytes().len());
        if utp_header_bytes(sack_bytes).saturating_add(payload.len()) > datagram_limit {
            intent.selective_ack = None;
        }
        let datagram_bytes = utp_header_bytes(
            intent
                .selective_ack
                .map_or(0, |selective_ack| selective_ack.as_bytes().len()),
        )
        .saturating_add(payload.len());
        if datagram_bytes > datagram_limit {
            return Err(TransportError::RetransmissionDatagramLimit {
                sequence_number,
                bytes: datagram_bytes,
                maximum: datagram_limit,
            });
        }
        self.connection
            .mark_retransmitted(sequence_number, now_micros)?;
        debug_assert!(self.retransmissions.complete_front(sequence_number));
        self.record_in_flight(sequence_number, payload.len());
        self.acknowledgements.acknowledge();
        self.pacer.on_payload_emitted(
            now_micros,
            payload.len(),
            congestion_window,
            self.connection.snapshot().send.rtt.smoothed_rtt_micros,
        );
        Ok(Some(TransportEmission {
            intent,
            timestamp: local_timestamp,
            timestamp_difference_micros: self.timestamp_difference_micros,
            payload,
            retransmission: true,
            mtu_probe: false,
            fragmentable_mtu_retry: fragmentable_mtu_retry.is_some(),
            dont_fragment: false,
        }))
    }

    fn poll_new_data(
        &mut self,
        now_micros: u64,
        local_timestamp: TimestampMicros,
    ) -> Result<Option<TransportEmission>, TransportError> {
        let queued_bytes = self.transmit.snapshot().unsent_bytes;
        if queued_bytes == 0 {
            return Ok(None);
        }
        let connection_snapshot = self.connection.snapshot();
        if !matches!(
            connection_snapshot.phase,
            ConnectionPhase::Connected | ConnectionPhase::RemoteFinReceived
        ) || connection_snapshot.send.outstanding_packets >= MAX_SENT_PACKETS
        {
            return Ok(None);
        }
        let selective_ack_bytes = connection_snapshot
            .receive
            .and_then(|_| self.connection.state_intent().ok())
            .and_then(|intent| intent.selective_ack)
            .map_or(0, |selective_ack| selective_ack.as_bytes().len());
        let congestion_window = self.congestion.snapshot().congestion_window_bytes;
        let in_flight_bytes = self.in_flight_bytes();
        let ordinary_limit = self.mtu.ordinary_datagram_bytes();
        let mut datagram_limit = ordinary_limit;
        let mut use_probe = false;
        if self.mtu.probe_ready(now_micros, congestion_window) {
            let candidate = self.mtu.candidate_datagram_bytes();
            let candidate_payload = candidate.saturating_sub(utp_header_bytes(selective_ack_bytes));
            let admitted = new_payload_bytes(
                queued_bytes,
                candidate,
                selective_ack_bytes,
                congestion_window,
                self.remote_window_bytes,
                in_flight_bytes,
            );
            if candidate_payload > 0 && admitted == candidate_payload {
                datagram_limit = candidate;
                use_probe = true;
            }
        }
        let payload_bytes = new_payload_bytes(
            queued_bytes,
            datagram_limit,
            selective_ack_bytes,
            congestion_window,
            self.remote_window_bytes,
            in_flight_bytes,
        );
        if payload_bytes == 0
            || connection_snapshot
                .send
                .outstanding_bytes
                .saturating_add(payload_bytes)
                > MAX_SENT_BYTES
        {
            return Ok(None);
        }
        let payload = self
            .transmit
            .prefix(payload_bytes)
            .expect("payload length is bounded by queued bytes");
        let intent = self.connection.record_data(&payload, now_micros)?;
        let datagram_bytes = utp_header_bytes(selective_ack_bytes) + payload_bytes;
        if use_probe {
            self.mtu
                .begin_probe(
                    intent.sequence_number,
                    datagram_bytes,
                    now_micros,
                    congestion_window,
                )
                .expect("probe admission and size were checked before recording DATA");
        } else {
            self.mtu.record_ordinary_sent(datagram_bytes, now_micros);
        }
        self.transmit
            .consume(payload_bytes)
            .expect("recorded payload remains at queue prefix");
        self.record_in_flight(intent.sequence_number, payload_bytes);
        self.acknowledgements.acknowledge();
        Ok(Some(TransportEmission {
            intent,
            timestamp: local_timestamp,
            timestamp_difference_micros: self.timestamp_difference_micros,
            payload,
            retransmission: false,
            mtu_probe: use_probe,
            fragmentable_mtu_retry: false,
            dont_fragment: use_probe,
        }))
    }

    fn process_timeout(
        &mut self,
        now_micros: u64,
        smoothed_rtt_micros: Option<u64>,
    ) -> Result<(), TransportError> {
        let connection_snapshot = self.connection.snapshot();
        if !connection_snapshot
            .send
            .timeout_deadline_micros
            .is_some_and(|deadline| now_micros >= deadline)
        {
            return Ok(());
        }
        let active_probe = self.mtu.snapshot().active_probe;
        let isolated_probe = connection_snapshot.send.outstanding_packets == 1
            && active_probe.is_some_and(|probe| {
                self.connection
                    .outstanding_packets()
                    .next()
                    .is_some_and(|packet| packet.sequence_number == probe.sequence_number)
            });
        let Some(timeout) = self.connection.on_timeout(now_micros, !isolated_probe)? else {
            return Ok(());
        };
        for sequence_number in &timeout.loss_signals {
            self.in_flight.remove(sequence_number);
        }
        if let Some(probe) = active_probe {
            let mtu_outcome = self.mtu.on_probe_loss(
                probe.sequence_number,
                if isolated_probe {
                    MtuProbeFailure::SolePacketTimeout
                } else {
                    MtuProbeFailure::CongestionOrUnknown
                },
                now_micros,
                smoothed_rtt_micros,
            );
            if matches!(
                mtu_outcome,
                MtuProbeOutcome::LoweredCeiling {
                    previous_floor,
                    floor,
                    ..
                } if previous_floor != floor
            ) {
                self.update_congestion_mss()?;
            }
        }
        self.retransmissions.schedule(timeout.loss_signals)?;
        if isolated_probe {
            self.congestion
                .on_loss(now_micros, smoothed_rtt_micros, true)?;
        } else {
            self.congestion
                .on_timeout(now_micros, smoothed_rtt_micros)?;
        }
        self.pacer.reset(now_micros);
        Ok(())
    }

    fn update_congestion_mss(&mut self) -> Result<(), TransportError> {
        let mss = self
            .mtu
            .ordinary_datagram_bytes()
            .saturating_sub(UTP_HEADER_SIZE);
        Ok(self.congestion.update_maximum_segment_size(mss)?)
    }

    fn record_in_flight(&mut self, sequence_number: SequenceNumber, payload_bytes: usize) {
        self.in_flight.insert(sequence_number, payload_bytes);
        self.in_flight_packet_high_water =
            self.in_flight_packet_high_water.max(self.in_flight.len());
        self.in_flight_byte_high_water = self.in_flight_byte_high_water.max(self.in_flight_bytes());
    }

    fn in_flight_bytes(&self) -> usize {
        self.in_flight.values().sum()
    }

    fn install_pending_emission(&mut self, emission: TransportEmission) -> TransportEmission {
        self.pending_emission = Some(emission.clone());
        emission
    }

    fn release_terminal_ownership(&mut self) {
        self.transmit.reset();
        self.retransmissions.reset();
        self.in_flight.clear();
        self.mtu.reset();
        self.pending_emission = None;
        self.initial_syn_pending = false;
        self.acknowledgements.acknowledge();
        self.close_requested = true;
    }
}

fn minimum_deadline(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(deadline), None) | (None, Some(deadline)) => Some(deadline),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utp::{
        IPV4_UDP_PAYLOAD_CEILING, IPV4_UDP_PAYLOAD_FLOOR, MAX_RECEIVE_BYTES, decode_packet,
    };

    fn sequence(value: u16) -> SequenceNumber {
        SequenceNumber::new(value)
    }

    fn connected_pair() -> (TransportState, TransportState) {
        let mut initiator = TransportState::initiate(
            40,
            sequence(10),
            0,
            IPV4_UDP_PAYLOAD_FLOOR,
            IPV4_UDP_PAYLOAD_CEILING,
        )
        .expect("initiate");
        let syn = initiator
            .poll_transmit(0, TimestampMicros::new(0))
            .expect("poll SYN")
            .expect("SYN emission");
        let syn_bytes = syn.encode().expect("encode SYN");
        initiator
            .on_send_result(syn.intent.sequence_number, DatagramSendResult::Sent, 0)
            .expect("send SYN");

        let mut acceptor = TransportState::accept_syn(
            decode_packet(&syn_bytes).expect("decode SYN"),
            sequence(77),
            IPV4_UDP_PAYLOAD_FLOOR,
            IPV4_UDP_PAYLOAD_CEILING,
        )
        .expect("accept SYN");
        let state = acceptor
            .poll_transmit(100, TimestampMicros::new(100))
            .expect("poll STATE")
            .expect("STATE emission");
        let state_bytes = state.encode().expect("encode STATE");
        acceptor
            .on_send_result(state.intent.sequence_number, DatagramSendResult::Sent, 100)
            .expect("send STATE");
        initiator
            .incoming(
                decode_packet(&state_bytes).expect("decode STATE"),
                200,
                TimestampMicros::new(200),
            )
            .expect("complete handshake");
        assert_eq!(
            initiator.snapshot().connection.phase,
            ConnectionPhase::Connected
        );
        (initiator, acceptor)
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

    #[test]
    fn composed_transport_handshakes_packetizes_and_delays_one_ack() {
        let (mut initiator, mut acceptor) = connected_pair();
        initiator
            .queue_data(&vec![7; 700])
            .expect("queue stream bytes");
        let data = initiator
            .poll_transmit(1_000, TimestampMicros::new(1_000))
            .expect("poll DATA")
            .expect("DATA emission");
        assert_eq!(data.intent.packet_type, PacketType::Data);
        assert_eq!(data.payload.len(), 528);
        assert_eq!(data.datagram_bytes(), IPV4_UDP_PAYLOAD_FLOOR);
        assert!(!data.mtu_probe);

        initiator
            .on_send_result(
                data.intent.sequence_number,
                DatagramSendResult::WouldBlock,
                1_000,
            )
            .expect("retain blocked datagram");
        assert_eq!(
            initiator
                .poll_transmit(2_000, TimestampMicros::new(2_000))
                .expect("repoll blocked"),
            Some(data.clone())
        );
        initiator
            .on_send_result(data.intent.sequence_number, DatagramSendResult::Sent, 2_000)
            .expect("send DATA");

        let data_bytes = data.encode().expect("encode DATA");
        let incoming = acceptor
            .incoming(
                decode_packet(&data_bytes).expect("decode DATA"),
                11_000,
                TimestampMicros::new(11_000),
            )
            .expect("receive DATA");
        let receive = incoming.connection.receive.expect("receive outcome");
        assert_eq!(receive.delivered[0].bytes, vec![7; 528]);
        assert_eq!(receive.advertised_window_bytes, MAX_RECEIVE_BYTES - 528);
        assert!(
            acceptor
                .poll_transmit(35_999, TimestampMicros::new(35_999))
                .expect("poll before ACK")
                .is_none()
        );

        let acknowledgement = acceptor
            .poll_transmit(36_000, TimestampMicros::new(36_000))
            .expect("poll ACK")
            .expect("ACK emission");
        assert_eq!(acknowledgement.intent.packet_type, PacketType::State);
        assert_eq!(acknowledgement.timestamp_difference_micros, 10_000);
        let acknowledgement_bytes = acknowledgement.encode().expect("encode ACK");
        acceptor
            .on_send_result(
                acknowledgement.intent.sequence_number,
                DatagramSendResult::Sent,
                36_000,
            )
            .expect("send ACK");
        let acknowledged = initiator
            .incoming(
                decode_packet(&acknowledgement_bytes).expect("decode ACK"),
                46_000,
                TimestampMicros::new(46_000),
            )
            .expect("apply ACK");
        assert_eq!(
            acknowledged
                .connection
                .acknowledgement
                .expect("ACK outcome")
                .acknowledged_sequences,
            vec![data.intent.sequence_number]
        );
        let sender = initiator.snapshot();
        assert_eq!(sender.in_flight_bytes, 0);
        assert_eq!(sender.connection.send.outstanding_bytes, 0);
        assert_eq!(sender.transmit.unsent_bytes, 172);
        assert_eq!(sender.congestion.congestion_window_bytes, 1_056);
        assert_eq!(
            acceptor.consume_received(528, 46_000),
            Ok(MAX_RECEIVE_BYTES)
        );
    }

    #[test]
    fn normal_data_drains_immediately_until_the_congestion_window_is_full() {
        let (mut initiator, _) = connected_pair();
        initiator
            .queue_data(&vec![7; 3 * 528])
            .expect("queue three packets");

        for expected_sequence in [11, 12] {
            let emission = initiator
                .poll_transmit(1_000, TimestampMicros::new(1_000))
                .expect("poll DATA")
                .expect("congestion-window DATA");
            assert_eq!(emission.intent.sequence_number, sequence(expected_sequence));
            assert_eq!(emission.payload.len(), 528);
            initiator
                .on_send_result(
                    emission.intent.sequence_number,
                    DatagramSendResult::Sent,
                    1_000,
                )
                .expect("record DATA send");
        }

        assert!(
            initiator
                .poll_transmit(1_000, TimestampMicros::new(1_000))
                .expect("poll full congestion window")
                .is_none()
        );
        let snapshot = initiator.snapshot();
        assert_eq!(snapshot.in_flight_bytes, 2 * 528);
        assert_eq!(snapshot.transmit.unsent_bytes, 528);
        assert_ne!(initiator.next_wakeup_micros(), Some(0));
    }

    #[test]
    fn remote_window_resizes_new_packet_instead_of_stranding_it() {
        let (mut initiator, _) = connected_pair();
        let tiny_window = encode_packet(PacketToEncode {
            header: UtpHeader {
                packet_type: PacketType::State,
                connection_id: 40,
                timestamp: TimestampMicros::new(300),
                timestamp_difference_micros: 0,
                window_size: 7,
                sequence_number: sequence(77),
                acknowledgement_number: sequence(10),
            },
            extensions: &[],
            payload: &[],
        })
        .expect("encode tiny window");
        initiator
            .incoming(
                decode_packet(&tiny_window).expect("decode tiny window"),
                300,
                TimestampMicros::new(300),
            )
            .expect("apply tiny window");
        initiator.queue_data(&[9; 100]).expect("queue bytes");
        let emission = initiator
            .poll_transmit(400, TimestampMicros::new(400))
            .expect("poll")
            .expect("tiny DATA");
        assert_eq!(emission.payload, vec![9; 7]);
        assert_eq!(initiator.snapshot().transmit.unsent_bytes, 93);
    }

    #[test]
    fn timeout_executes_same_sequence_retransmission_and_collapses_window() {
        let (mut initiator, _) = connected_pair();
        initiator.queue_data(&[3; 528]).expect("queue DATA");
        let first = initiator
            .poll_transmit(1_000, TimestampMicros::new(1_000))
            .expect("poll DATA")
            .expect("DATA");
        initiator
            .on_send_result(
                first.intent.sequence_number,
                DatagramSendResult::Sent,
                1_000,
            )
            .expect("send DATA");
        let deadline = initiator
            .snapshot()
            .connection
            .send
            .timeout_deadline_micros
            .expect("timeout deadline");
        let retransmission = initiator
            .poll_transmit(deadline, TimestampMicros::new(deadline as u32))
            .expect("poll timeout")
            .expect("retransmission");
        assert!(retransmission.retransmission);
        assert_eq!(
            retransmission.intent.sequence_number,
            first.intent.sequence_number
        );
        assert_eq!(retransmission.payload, first.payload);
        let snapshot = initiator.snapshot();
        assert_eq!(snapshot.retransmissions.pending_packets, 0);
        assert_eq!(snapshot.connection.send.consecutive_timeouts, 1);
        assert_eq!(snapshot.congestion.congestion_window_bytes, 528);
        assert_eq!(snapshot.in_flight_bytes, 528);
        assert_eq!(
            initiator
                .connection
                .outstanding_packets()
                .next()
                .expect("outstanding retransmission")
                .transmissions,
            2
        );
    }

    #[test]
    fn revalidation_failure_lowers_mss_and_retries_exact_packet_fragmentably() {
        let (mut initiator, _) = connected_pair();
        let mut now_micros = 1_000_000;
        let mut probe_sequence = 200;
        while !initiator.mtu.search_complete() {
            for _ in 0..3 {
                initiator
                    .mtu
                    .record_ordinary_sent(initiator.mtu.ordinary_datagram_bytes(), now_micros);
            }
            let candidate = initiator.mtu.candidate_datagram_bytes();
            initiator
                .mtu
                .begin_probe(sequence(probe_sequence), candidate, now_micros, 100_000)
                .expect("begin synthetic search probe");
            now_micros += 1_000_000;
            assert!(matches!(
                initiator
                    .mtu
                    .on_acknowledged(sequence(probe_sequence), now_micros, Some(100_000)),
                MtuProbeOutcome::RaisedFloor { .. }
            ));
            now_micros += 1_000_000;
            probe_sequence += 1;
        }
        initiator.update_congestion_mss().expect("raised MSS");
        let confirmed = initiator.mtu.ordinary_datagram_bytes();
        let confirmed_mss = confirmed - UTP_HEADER_SIZE;
        for sample in 0..8 {
            initiator
                .congestion
                .on_ack(
                    now_micros + sample,
                    confirmed_mss,
                    100_000,
                    1,
                    Some(100_000),
                    true,
                )
                .expect("grow synthetic congestion window");
        }
        assert!(
            initiator.snapshot().congestion.congestion_window_bytes > confirmed.saturating_mul(3)
        );
        let deadline = initiator
            .mtu
            .snapshot()
            .revalidation_deadline_micros
            .expect("completed search deadline");
        for _ in 0..3 {
            initiator.mtu.record_ordinary_sent(confirmed, deadline);
        }
        initiator
            .queue_data(&vec![7; confirmed_mss])
            .expect("queue revalidation payload");
        let revalidation = initiator
            .poll_transmit(deadline, TimestampMicros::new(deadline as u32))
            .expect("poll revalidation")
            .expect("revalidation emission");
        assert!(revalidation.mtu_probe);
        assert!(revalidation.dont_fragment);
        assert_eq!(revalidation.datagram_bytes(), confirmed);

        initiator
            .on_send_result(
                revalidation.intent.sequence_number,
                DatagramSendResult::MessageTooLarge,
                deadline,
            )
            .expect("classify revalidation failure");
        let reduced = initiator.snapshot();
        assert_eq!(reduced.mtu.floor_datagram_bytes, IPV4_UDP_PAYLOAD_FLOOR);
        assert_eq!(
            reduced.congestion.maximum_segment_bytes,
            IPV4_UDP_PAYLOAD_FLOOR - UTP_HEADER_SIZE
        );
        assert_eq!(reduced.mtu.downward_recoveries, 1);

        let retry = initiator
            .poll_transmit(deadline, TimestampMicros::new(deadline as u32))
            .expect("poll fragmentable retry")
            .expect("fragmentable retry emission");
        assert!(retry.retransmission);
        assert!(retry.fragmentable_mtu_retry);
        assert!(!retry.dont_fragment);
        assert_eq!(
            retry.intent.sequence_number,
            revalidation.intent.sequence_number
        );
        assert_eq!(retry.payload, revalidation.payload);
        assert_eq!(retry.datagram_bytes(), revalidation.datagram_bytes());
    }

    #[test]
    fn retransmission_omits_new_sack_to_preserve_original_mtu_limit() {
        let (mut initiator, _) = connected_pair();
        initiator.queue_data(&[3; 528]).expect("queue DATA");
        let first = initiator
            .poll_transmit(1_000, TimestampMicros::new(1_000))
            .expect("poll DATA")
            .expect("DATA");
        assert_eq!(first.datagram_bytes(), IPV4_UDP_PAYLOAD_FLOOR);
        initiator
            .on_send_result(
                first.intent.sequence_number,
                DatagramSendResult::Sent,
                1_000,
            )
            .expect("send DATA");

        let reordered = encode_packet(PacketToEncode {
            header: UtpHeader {
                packet_type: PacketType::Data,
                connection_id: 40,
                timestamp: TimestampMicros::new(2_000),
                timestamp_difference_micros: 1,
                window_size: MAX_RECEIVE_BYTES as u32,
                sequence_number: sequence(78),
                acknowledgement_number: sequence(10),
            },
            extensions: &[],
            payload: b"peer",
        })
        .expect("encode reordered peer DATA");
        let incoming = initiator
            .incoming(
                decode_packet(&reordered).expect("decode reordered peer DATA"),
                2_000,
                TimestampMicros::new(2_000),
            )
            .expect("buffer reordered peer DATA");
        assert_eq!(
            incoming
                .connection
                .receive
                .expect("receive outcome")
                .disposition,
            ReceiveDisposition::Buffered
        );
        assert!(
            initiator
                .connection
                .state_intent()
                .expect("SACK intent")
                .selective_ack
                .is_some()
        );

        let deadline = initiator
            .snapshot()
            .connection
            .send
            .timeout_deadline_micros
            .expect("timeout deadline");
        let retransmission = initiator
            .poll_transmit(deadline, TimestampMicros::new(deadline as u32))
            .expect("poll timeout")
            .expect("retransmission");
        assert!(retransmission.retransmission);
        assert_eq!(retransmission.payload, first.payload);
        assert!(retransmission.intent.selective_ack.is_none());
        assert_eq!(retransmission.datagram_bytes(), IPV4_UDP_PAYLOAD_FLOOR);
    }

    #[test]
    fn terminal_courtesy_ack_emits_only_already_pending_receive_state() {
        let (mut initiator, mut acceptor) = connected_pair();
        initiator.queue_data(b"received before drop").unwrap();
        let data = initiator
            .poll_transmit(1_000, TimestampMicros::new(1_000))
            .unwrap()
            .unwrap();
        initiator
            .on_send_result(data.intent.sequence_number, DatagramSendResult::Sent, 1_000)
            .unwrap();
        acceptor
            .incoming(
                decode_packet(&data.encode().unwrap()).unwrap(),
                2_000,
                TimestampMicros::new(2_000),
            )
            .unwrap();
        assert!(
            acceptor
                .snapshot()
                .acknowledgements
                .deadline_micros
                .is_some_and(|deadline| deadline > 2_000)
        );

        let acknowledgement = acceptor
            .poll_pending_acknowledgement(TimestampMicros::new(2_001))
            .unwrap()
            .expect("pending DATA has a courtesy ACK");
        assert_eq!(acknowledgement.intent.packet_type, PacketType::State);
        assert!(acknowledgement.payload.is_empty());
        assert!(!acknowledgement.dont_fragment);
        assert!(
            acceptor
                .poll_pending_acknowledgement(TimestampMicros::new(2_002))
                .unwrap()
                .is_none()
        );
        acceptor
            .on_send_result(
                acknowledgement.intent.sequence_number,
                DatagramSendResult::Sent,
                2_002,
            )
            .unwrap();
        assert!(
            acceptor
                .poll_pending_acknowledgement(TimestampMicros::new(2_003))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn close_drains_data_then_finishes_after_bidirectional_fin_ack() {
        let (mut initiator, mut acceptor) = connected_pair();
        initiator
            .queue_data(b"last bytes")
            .expect("queue final data");
        initiator.request_close();
        assert!(matches!(
            initiator.queue_data(b"too late"),
            Err(TransportError::Queue(TransmitQueueError::Terminal))
        ));

        let data = initiator
            .poll_transmit(1_000, TimestampMicros::new(1_000))
            .expect("poll data")
            .expect("data emission");
        assert_eq!(data.intent.packet_type, PacketType::Data);
        initiator
            .on_send_result(data.intent.sequence_number, DatagramSendResult::Sent, 1_000)
            .expect("record data send");
        acceptor
            .incoming(
                decode_packet(&data.encode().expect("encode data")).expect("decode data"),
                2_000,
                TimestampMicros::new(2_000),
            )
            .expect("receive data");

        let fin = initiator
            .poll_transmit(2_001, TimestampMicros::new(2_001))
            .expect("poll fin")
            .expect("fin emission");
        assert_eq!(fin.intent.packet_type, PacketType::Fin);
        initiator
            .on_send_result(fin.intent.sequence_number, DatagramSendResult::Sent, 2_001)
            .expect("record fin send");
        acceptor
            .incoming(
                decode_packet(&fin.encode().expect("encode fin")).expect("decode fin"),
                3_000,
                TimestampMicros::new(3_000),
            )
            .expect("receive fin");

        acceptor.request_close();
        let response_fin = acceptor
            .poll_transmit(3_001, TimestampMicros::new(3_001))
            .expect("poll response fin")
            .expect("response fin emission");
        assert_eq!(response_fin.intent.packet_type, PacketType::Fin);
        assert_eq!(
            response_fin.intent.acknowledgement_number,
            fin.intent.sequence_number
        );
        acceptor
            .on_send_result(
                response_fin.intent.sequence_number,
                DatagramSendResult::Sent,
                3_001,
            )
            .expect("record response fin send");
        initiator
            .incoming(
                decode_packet(&response_fin.encode().expect("encode response fin"))
                    .expect("decode response fin"),
                4_000,
                TimestampMicros::new(4_000),
            )
            .expect("receive response fin");
        assert!(initiator.snapshot().connection.ready_to_close);

        let final_ack = initiator
            .poll_transmit(4_001, TimestampMicros::new(4_001))
            .expect("poll final ack")
            .expect("final ack emission");
        assert_eq!(final_ack.intent.packet_type, PacketType::State);
        initiator
            .on_send_result(
                final_ack.intent.sequence_number,
                DatagramSendResult::Sent,
                4_001,
            )
            .expect("record final ack send");
        acceptor
            .incoming(
                decode_packet(&final_ack.encode().expect("encode final ack"))
                    .expect("decode final ack"),
                5_000,
                TimestampMicros::new(5_000),
            )
            .expect("receive final ack");
        assert!(acceptor.snapshot().connection.ready_to_close);

        acceptor
            .consume_received(b"last bytes".len(), 5_001)
            .expect("consume final data");
        initiator.finish().expect("finish initiator");
        acceptor.finish().expect("finish acceptor");
        for snapshot in [initiator.snapshot(), acceptor.snapshot()] {
            assert_eq!(snapshot.connection.phase, ConnectionPhase::Closed);
            assert_eq!(snapshot.connection.send.outstanding_packets, 0);
            assert_eq!(snapshot.in_flight_packets, 0);
            assert_eq!(snapshot.pending_emission_bytes, 0);
        }
    }

    #[test]
    fn next_wakeup_tracks_ack_timeout_pacing_and_close_work() {
        let (mut initiator, mut acceptor) = connected_pair();
        assert_eq!(initiator.next_wakeup_micros(), None);
        initiator.queue_data(&[1; 528]).expect("queue data");
        assert_eq!(initiator.next_wakeup_micros(), Some(0));
        let data = initiator
            .poll_transmit(10, TimestampMicros::new(10))
            .expect("poll data")
            .expect("data");
        initiator
            .on_send_result(data.intent.sequence_number, DatagramSendResult::Sent, 10)
            .expect("send data");
        assert_eq!(
            initiator.next_wakeup_micros(),
            initiator.snapshot().connection.send.timeout_deadline_micros
        );

        acceptor
            .incoming(
                decode_packet(&data.encode().expect("encode data")).expect("decode data"),
                20,
                TimestampMicros::new(20),
            )
            .expect("receive data");
        assert_eq!(
            acceptor.next_wakeup_micros(),
            acceptor.snapshot().acknowledgements.deadline_micros
        );

        initiator.abort();
        acceptor.abort();
        assert_eq!(initiator.next_wakeup_micros(), None);
        assert_eq!(acceptor.next_wakeup_micros(), None);
    }

    #[test]
    fn abort_releases_every_current_transport_owner() {
        let (mut initiator, _) = connected_pair();
        initiator.queue_data(&[4; 700]).expect("queue DATA");
        let pending = initiator
            .poll_transmit(1_000, TimestampMicros::new(1_000))
            .expect("poll")
            .expect("pending DATA");
        assert_eq!(pending.payload.len(), 528);
        initiator.abort();
        let snapshot = initiator.snapshot();
        assert_eq!(snapshot.connection.phase, ConnectionPhase::Closed);
        assert_eq!(snapshot.connection.send.outstanding_packets, 0);
        assert_eq!(snapshot.transmit.unsent_bytes, 0);
        assert_eq!(snapshot.retransmissions.pending_packets, 0);
        assert_eq!(snapshot.in_flight_bytes, 0);
        assert_eq!(snapshot.pending_emission_bytes, 0);
    }
}
