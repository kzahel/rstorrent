//! uTP connection IDs, handshake, lifecycle, and reliability composition.

use std::error::Error;
use std::fmt;

use super::{
    AckOutcome, DecodedPacket, MAX_RECEIVE_BYTES, PacketType, ReceiveError, ReceiveOutcome,
    ReceiveSnapshot, ReceiveState, SelectiveAckBits, SendError, SendSnapshot, SendState,
    SentPacketSnapshot, SequenceNumber, TimeoutOutcome,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConnectionIds {
    pub send: u16,
    pub receive: u16,
}

impl ConnectionIds {
    /// IDs for an outgoing connection whose SYN advertises `receive`.
    #[must_use]
    pub const fn for_initiator(receive: u16) -> Self {
        Self {
            send: receive.wrapping_add(1),
            receive,
        }
    }

    /// IDs for an accepted SYN carrying `connection_id`.
    #[must_use]
    pub const fn for_acceptor(connection_id: u16) -> Self {
        Self {
            send: connection_id,
            receive: connection_id.wrapping_add(1),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionPhase {
    SynSent,
    Connected,
    LocalFinSent,
    RemoteFinReceived,
    Closing,
    Reset,
    Closed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConnectionSnapshot {
    pub ids: ConnectionIds,
    pub phase: ConnectionPhase,
    pub send: SendSnapshot,
    pub receive: Option<ReceiveSnapshot>,
    pub ready_to_close: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutboundPacketIntent {
    pub packet_type: PacketType,
    pub connection_id: u16,
    pub sequence_number: SequenceNumber,
    pub acknowledgement_number: SequenceNumber,
    pub selective_ack: Option<SelectiveAckBits>,
    pub window_size: u32,
    pub payload_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IncomingDisposition {
    Accepted,
    WrongConnectionId {
        expected: u16,
        actual: u16,
    },
    UnexpectedPacketType(PacketType),
    HandshakeAckMismatch {
        expected: SequenceNumber,
        actual: SequenceNumber,
    },
    Terminal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncomingOutcome {
    pub disposition: IncomingDisposition,
    pub acknowledgement: Option<AckOutcome>,
    pub receive: Option<ReceiveOutcome>,
    pub reply: Option<OutboundPacketIntent>,
    pub phase: ConnectionPhase,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConnectionError {
    ExpectedSyn(PacketType),
    SynHasSelectiveAck,
    InvalidPhase {
        phase: ConnectionPhase,
        operation: &'static str,
    },
    NotReadyToClose,
    Send(SendError),
    Receive(ReceiveError),
}

impl fmt::Display for ConnectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExpectedSyn(packet_type) => {
                write!(formatter, "expected uTP SYN, received {packet_type:?}")
            }
            Self::SynHasSelectiveAck => formatter.write_str("uTP SYN must not carry a SACK"),
            Self::InvalidPhase { phase, operation } => {
                write!(
                    formatter,
                    "cannot {operation} while uTP connection is {phase:?}"
                )
            }
            Self::NotReadyToClose => {
                formatter.write_str("uTP connection has not completed both FIN directions")
            }
            Self::Send(error) => write!(formatter, "uTP connection send error: {error}"),
            Self::Receive(error) => write!(formatter, "uTP connection receive error: {error}"),
        }
    }
}

impl Error for ConnectionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Send(error) => Some(error),
            Self::Receive(error) => Some(error),
            Self::ExpectedSyn(_)
            | Self::SynHasSelectiveAck
            | Self::InvalidPhase { .. }
            | Self::NotReadyToClose => None,
        }
    }
}

impl From<SendError> for ConnectionError {
    fn from(error: SendError) -> Self {
        Self::Send(error)
    }
}

impl From<ReceiveError> for ConnectionError {
    fn from(error: ReceiveError) -> Self {
        Self::Receive(error)
    }
}

#[derive(Clone, Debug)]
pub struct ConnectionState {
    ids: ConnectionIds,
    phase: ConnectionPhase,
    send: SendState,
    receive: Option<ReceiveState>,
}

impl ConnectionState {
    pub fn initiate(
        receive_connection_id: u16,
        initial_sequence_number: SequenceNumber,
        now_micros: u64,
    ) -> Result<Self, ConnectionError> {
        let mut send = SendState::new(initial_sequence_number);
        send.record_sent(PacketType::Syn, &[], now_micros)?;
        Ok(Self {
            ids: ConnectionIds::for_initiator(receive_connection_id),
            phase: ConnectionPhase::SynSent,
            send,
            receive: None,
        })
    }

    pub fn accept_syn(
        syn: DecodedPacket<'_>,
        initial_sequence_number: SequenceNumber,
    ) -> Result<Self, ConnectionError> {
        if syn.header.packet_type != PacketType::Syn {
            return Err(ConnectionError::ExpectedSyn(syn.header.packet_type));
        }
        if syn.selective_ack().is_some() {
            return Err(ConnectionError::SynHasSelectiveAck);
        }
        Ok(Self {
            ids: ConnectionIds::for_acceptor(syn.header.connection_id),
            phase: ConnectionPhase::Connected,
            send: SendState::new(initial_sequence_number),
            receive: Some(ReceiveState::new(syn.header.sequence_number)),
        })
    }

    #[must_use]
    pub fn snapshot(&self) -> ConnectionSnapshot {
        ConnectionSnapshot {
            ids: self.ids,
            phase: self.phase,
            send: self.send.snapshot(),
            receive: self.receive.as_ref().map(ReceiveState::snapshot),
            ready_to_close: self.ready_to_close(),
        }
    }

    #[must_use]
    pub fn syn_intent(&self) -> Option<OutboundPacketIntent> {
        if self.phase != ConnectionPhase::SynSent {
            return None;
        }
        self.send
            .outstanding_packets()
            .find(|packet| packet.packet_type == PacketType::Syn)
            .map(|packet| OutboundPacketIntent {
                packet_type: PacketType::Syn,
                connection_id: self.ids.receive,
                sequence_number: packet.sequence_number,
                acknowledgement_number: SequenceNumber::new(0),
                selective_ack: None,
                window_size: MAX_RECEIVE_BYTES as u32,
                payload_bytes: 0,
            })
    }

    pub fn state_intent(&self) -> Result<OutboundPacketIntent, ConnectionError> {
        let receive = self.receive.as_ref().ok_or(ConnectionError::InvalidPhase {
            phase: self.phase,
            operation: "construct STATE",
        })?;
        if self.is_terminal() {
            return Err(ConnectionError::InvalidPhase {
                phase: self.phase,
                operation: "construct STATE",
            });
        }
        let receive_snapshot = receive.snapshot();
        Ok(OutboundPacketIntent {
            packet_type: PacketType::State,
            connection_id: self.ids.send,
            sequence_number: self.send.snapshot().next_sequence_number,
            acknowledgement_number: receive_snapshot.acknowledgement_number,
            selective_ack: receive.selective_ack(),
            window_size: receive_snapshot.advertised_window_bytes as u32,
            payload_bytes: 0,
        })
    }

    pub fn record_data(
        &mut self,
        payload: &[u8],
        now_micros: u64,
    ) -> Result<OutboundPacketIntent, ConnectionError> {
        if !matches!(
            self.phase,
            ConnectionPhase::Connected | ConnectionPhase::RemoteFinReceived
        ) {
            return Err(ConnectionError::InvalidPhase {
                phase: self.phase,
                operation: "send DATA",
            });
        }
        let sequence_number = self
            .send
            .record_sent(PacketType::Data, payload, now_micros)?;
        self.outbound_sequence_intent(PacketType::Data, sequence_number, payload.len())
    }

    pub fn record_fin(
        &mut self,
        payload: &[u8],
        now_micros: u64,
    ) -> Result<OutboundPacketIntent, ConnectionError> {
        if !matches!(
            self.phase,
            ConnectionPhase::Connected | ConnectionPhase::RemoteFinReceived
        ) {
            return Err(ConnectionError::InvalidPhase {
                phase: self.phase,
                operation: "send FIN",
            });
        }
        let sequence_number = self
            .send
            .record_sent(PacketType::Fin, payload, now_micros)?;
        self.refresh_phase();
        self.outbound_sequence_intent(PacketType::Fin, sequence_number, payload.len())
    }

    #[must_use]
    pub fn payload_for_retransmission(&self, sequence_number: SequenceNumber) -> Option<&[u8]> {
        self.send.payload_for_retransmission(sequence_number)
    }

    #[must_use]
    pub fn outstanding_packets(&self) -> impl ExactSizeIterator<Item = SentPacketSnapshot> + '_ {
        self.send.outstanding_packets()
    }

    pub fn retransmission_intent(
        &self,
        sequence_number: SequenceNumber,
    ) -> Result<OutboundPacketIntent, ConnectionError> {
        let packet = self
            .send
            .outstanding_packets()
            .find(|packet| packet.sequence_number == sequence_number)
            .ok_or(ConnectionError::Send(SendError::UnknownPacket(
                sequence_number,
            )))?;
        match packet.packet_type {
            PacketType::Syn => self.syn_intent().ok_or(ConnectionError::InvalidPhase {
                phase: self.phase,
                operation: "retransmit SYN",
            }),
            PacketType::Data | PacketType::Fin => self.outbound_sequence_intent(
                packet.packet_type,
                packet.sequence_number,
                packet.payload_bytes,
            ),
            PacketType::State | PacketType::Reset => unreachable!("sent ledger packet type"),
        }
    }

    pub fn mark_retransmitted(
        &mut self,
        sequence_number: SequenceNumber,
        now_micros: u64,
    ) -> Result<SentPacketSnapshot, ConnectionError> {
        Ok(self.send.mark_retransmitted(sequence_number, now_micros)?)
    }

    pub fn on_timeout(
        &mut self,
        now_micros: u64,
        apply_backoff: bool,
    ) -> Result<Option<TimeoutOutcome>, ConnectionError> {
        Ok(self.send.on_timeout_classified(now_micros, apply_backoff)?)
    }

    pub fn consume_received(&mut self, bytes: usize) -> Result<usize, ConnectionError> {
        let receive = self.receive.as_mut().ok_or(ConnectionError::InvalidPhase {
            phase: self.phase,
            operation: "consume received payload",
        })?;
        Ok(receive.consume_delivered(bytes)?)
    }

    pub fn incoming(
        &mut self,
        packet: DecodedPacket<'_>,
        now_micros: u64,
    ) -> Result<IncomingOutcome, ConnectionError> {
        if self.is_terminal() {
            return Ok(self.empty_incoming(IncomingDisposition::Terminal));
        }
        if packet.header.connection_id != self.ids.receive {
            return Ok(self.empty_incoming(IncomingDisposition::WrongConnectionId {
                expected: self.ids.receive,
                actual: packet.header.connection_id,
            }));
        }
        if packet.header.packet_type == PacketType::Reset {
            self.reset(ConnectionPhase::Reset);
            return Ok(self.empty_incoming(IncomingDisposition::Accepted));
        }
        if packet.header.packet_type == PacketType::Syn {
            return Ok(
                self.empty_incoming(IncomingDisposition::UnexpectedPacketType(PacketType::Syn))
            );
        }

        if self.phase == ConnectionPhase::SynSent {
            return self.incoming_handshake_ack(packet, now_micros);
        }
        self.incoming_established(packet, now_micros)
    }

    pub fn finish(&mut self) -> Result<(), ConnectionError> {
        if !self.ready_to_close() {
            return Err(ConnectionError::NotReadyToClose);
        }
        self.reset(ConnectionPhase::Closed);
        Ok(())
    }

    pub fn abort(&mut self) {
        self.reset(ConnectionPhase::Closed);
    }

    fn incoming_handshake_ack(
        &mut self,
        packet: DecodedPacket<'_>,
        now_micros: u64,
    ) -> Result<IncomingOutcome, ConnectionError> {
        let expected_ack = self
            .send
            .outstanding_packets()
            .find(|outstanding| outstanding.packet_type == PacketType::Syn)
            .map(|outstanding| outstanding.sequence_number)
            .expect("SynSent connection owns an outstanding SYN");
        if packet.header.acknowledgement_number != expected_ack {
            return Ok(
                self.empty_incoming(IncomingDisposition::HandshakeAckMismatch {
                    expected: expected_ack,
                    actual: packet.header.acknowledgement_number,
                }),
            );
        }

        let mut receive = ReceiveState::new(packet.header.sequence_number.wrapping_sub(1));
        let receive_outcome = match packet.header.packet_type {
            PacketType::Data | PacketType::Fin => Some(receive.receive(
                packet.header.packet_type,
                packet.header.sequence_number,
                packet.payload(),
            )?),
            PacketType::State => None,
            PacketType::Reset | PacketType::Syn => unreachable!("handled packet type"),
        };
        let acknowledgement = self.send.acknowledge(
            packet.header.acknowledgement_number,
            packet.selective_ack().map(|sack| sack.as_bytes()),
            packet.header.packet_type,
            now_micros,
        )?;
        self.receive = Some(receive);
        self.phase = ConnectionPhase::Connected;
        self.refresh_phase();
        let reply = matches!(
            packet.header.packet_type,
            PacketType::Data | PacketType::Fin
        )
        .then(|| self.state_intent())
        .transpose()?;
        Ok(IncomingOutcome {
            disposition: IncomingDisposition::Accepted,
            acknowledgement: Some(acknowledgement),
            receive: receive_outcome,
            reply,
            phase: self.phase,
        })
    }

    fn incoming_established(
        &mut self,
        packet: DecodedPacket<'_>,
        now_micros: u64,
    ) -> Result<IncomingOutcome, ConnectionError> {
        let receive_outcome = match packet.header.packet_type {
            PacketType::Data | PacketType::Fin => Some(
                self.receive
                    .as_mut()
                    .expect("established connection owns receive state")
                    .receive(
                        packet.header.packet_type,
                        packet.header.sequence_number,
                        packet.payload(),
                    )?,
            ),
            PacketType::State => None,
            PacketType::Reset | PacketType::Syn => unreachable!("handled packet type"),
        };
        let acknowledgement = self
            .send
            .acknowledge(
                packet.header.acknowledgement_number,
                packet.selective_ack().map(|sack| sack.as_bytes()),
                packet.header.packet_type,
                now_micros,
            )
            .expect("decoded SACK and live connection preserve send invariants");
        self.send.on_valid_incoming(now_micros);
        self.refresh_phase();
        let reply = matches!(
            packet.header.packet_type,
            PacketType::Data | PacketType::Fin
        )
        .then(|| self.state_intent())
        .transpose()?;
        Ok(IncomingOutcome {
            disposition: IncomingDisposition::Accepted,
            acknowledgement: Some(acknowledgement),
            receive: receive_outcome,
            reply,
            phase: self.phase,
        })
    }

    fn outbound_sequence_intent(
        &self,
        packet_type: PacketType,
        sequence_number: SequenceNumber,
        payload_bytes: usize,
    ) -> Result<OutboundPacketIntent, ConnectionError> {
        let receive = self.receive.as_ref().ok_or(ConnectionError::InvalidPhase {
            phase: self.phase,
            operation: "construct sequence-bearing packet",
        })?;
        let receive_snapshot = receive.snapshot();
        Ok(OutboundPacketIntent {
            packet_type,
            connection_id: self.ids.send,
            sequence_number,
            acknowledgement_number: receive_snapshot.acknowledgement_number,
            selective_ack: receive.selective_ack(),
            window_size: receive_snapshot.advertised_window_bytes as u32,
            payload_bytes,
        })
    }

    fn ready_to_close(&self) -> bool {
        if self.is_terminal() {
            return false;
        }
        self.send.snapshot().fin_sent
            && self.send.snapshot().outstanding_packets == 0
            && self
                .receive
                .as_ref()
                .is_some_and(|receive| receive.snapshot().eof_reached)
    }

    fn refresh_phase(&mut self) {
        if self.is_terminal() || self.phase == ConnectionPhase::SynSent {
            return;
        }
        let local_fin = self.send.snapshot().fin_sent;
        let remote_fin = self
            .receive
            .as_ref()
            .is_some_and(|receive| receive.snapshot().eof_reached);
        self.phase = match (local_fin, remote_fin) {
            (false, false) => ConnectionPhase::Connected,
            (true, false) => ConnectionPhase::LocalFinSent,
            (false, true) => ConnectionPhase::RemoteFinReceived,
            (true, true) => ConnectionPhase::Closing,
        };
    }

    fn reset(&mut self, phase: ConnectionPhase) {
        self.send.reset();
        if let Some(receive) = &mut self.receive {
            receive.reset();
        }
        self.phase = phase;
    }

    fn is_terminal(&self) -> bool {
        matches!(self.phase, ConnectionPhase::Reset | ConnectionPhase::Closed)
    }

    fn empty_incoming(&self, disposition: IncomingDisposition) -> IncomingOutcome {
        IncomingOutcome {
            disposition,
            acknowledgement: None,
            receive: None,
            reply: None,
            phase: self.phase,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utp::{
        AckDisposition, ExtensionToEncode, PacketToEncode, SACK_EXTENSION, TimestampMicros,
        UtpHeader, decode_packet, encode_packet,
    };

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
                timestamp: TimestampMicros::new(0),
                timestamp_difference_micros: 0,
                window_size: 0,
                sequence_number: SequenceNumber::new(sequence_number),
                acknowledgement_number: SequenceNumber::new(acknowledgement_number),
            },
            extensions: &[],
            payload,
        })
        .expect("encode connection fixture")
    }

    fn decoded(bytes: &[u8]) -> DecodedPacket<'_> {
        decode_packet(bytes).expect("decode connection fixture")
    }

    #[test]
    fn connection_ids_follow_the_syn_reversal_and_wrap() {
        assert_eq!(
            ConnectionIds::for_initiator(u16::MAX),
            ConnectionIds {
                send: 0,
                receive: u16::MAX,
            }
        );
        assert_eq!(
            ConnectionIds::for_acceptor(u16::MAX),
            ConnectionIds {
                send: u16::MAX,
                receive: 0,
            }
        );
    }

    #[test]
    fn initiator_and_acceptor_construct_exact_handshake_intents() {
        let initiator =
            ConnectionState::initiate(u16::MAX, SequenceNumber::new(10), 100).expect("initiate");
        assert_eq!(
            initiator.syn_intent().expect("SYN intent"),
            OutboundPacketIntent {
                packet_type: PacketType::Syn,
                connection_id: u16::MAX,
                sequence_number: SequenceNumber::new(10),
                acknowledgement_number: SequenceNumber::new(0),
                selective_ack: None,
                window_size: MAX_RECEIVE_BYTES as u32,
                payload_bytes: 0,
            }
        );

        let syn = packet(PacketType::Syn, u16::MAX, 10, 0, &[]);
        let acceptor = ConnectionState::accept_syn(decoded(&syn), SequenceNumber::new(77))
            .expect("accept SYN");
        assert_eq!(
            acceptor.snapshot().ids,
            ConnectionIds {
                send: u16::MAX,
                receive: 0,
            }
        );
        let state = acceptor.state_intent().expect("STATE intent");
        assert_eq!(state.connection_id, u16::MAX);
        assert_eq!(state.sequence_number, SequenceNumber::new(77));
        assert_eq!(state.acknowledgement_number, SequenceNumber::new(10));
        assert_eq!(
            acceptor
                .state_intent()
                .expect("second STATE")
                .sequence_number,
            SequenceNumber::new(77),
            "STATE must not consume a sequence number"
        );
    }

    #[test]
    fn state_ack_completes_initiator_handshake_without_consuming_remote_sequence() {
        let mut initiator =
            ConnectionState::initiate(40, SequenceNumber::new(10), 100).expect("initiate");
        let state = packet(PacketType::State, 40, 77, 10, &[]);
        let outcome = initiator.incoming(decoded(&state), 300).expect("STATE ACK");
        assert_eq!(outcome.disposition, IncomingDisposition::Accepted);
        assert_eq!(outcome.phase, ConnectionPhase::Connected);
        assert_eq!(
            initiator
                .snapshot()
                .receive
                .expect("receive state")
                .acknowledgement_number,
            SequenceNumber::new(76)
        );
        assert_eq!(initiator.snapshot().send.outstanding_packets, 0);
        assert_eq!(
            outcome.acknowledgement.expect("send ACK").rtt_sample_micros,
            Some(200)
        );
    }

    #[test]
    fn first_data_can_ack_syn_and_is_delivered() {
        let mut initiator =
            ConnectionState::initiate(40, SequenceNumber::new(10), 0).expect("initiate");
        let data = packet(PacketType::Data, 40, 77, 10, b"hello");
        let outcome = initiator.incoming(decoded(&data), 100).expect("DATA ACK");
        let receive = outcome.receive.expect("receive outcome");
        assert_eq!(receive.delivered[0].bytes, b"hello");
        assert_eq!(receive.acknowledgement_number, SequenceNumber::new(77));
        let reply = outcome.reply.expect("STATE reply");
        assert_eq!(reply.packet_type, PacketType::State);
        assert_eq!(reply.connection_id, 41);
        assert_eq!(reply.acknowledgement_number, SequenceNumber::new(77));
        assert_eq!(reply.window_size, (MAX_RECEIVE_BYTES - 5) as u32);
        assert_eq!(initiator.consume_received(5), Ok(MAX_RECEIVE_BYTES));
        assert_eq!(
            initiator
                .state_intent()
                .expect("reopened window")
                .window_size,
            MAX_RECEIVE_BYTES as u32
        );
    }

    #[test]
    fn wrong_id_and_handshake_ack_mismatch_are_atomic() {
        let mut initiator =
            ConnectionState::initiate(40, SequenceNumber::new(10), 0).expect("initiate");
        let before = initiator.snapshot();
        let wrong_id = packet(PacketType::State, 41, 77, 10, &[]);
        assert!(matches!(
            initiator
                .incoming(decoded(&wrong_id), 100)
                .expect("wrong ID")
                .disposition,
            IncomingDisposition::WrongConnectionId { .. }
        ));
        assert_eq!(initiator.snapshot(), before);
        let wrong_ack = packet(PacketType::State, 40, 77, 9, &[]);
        assert!(matches!(
            initiator
                .incoming(decoded(&wrong_ack), 100)
                .expect("wrong ACK")
                .disposition,
            IncomingDisposition::HandshakeAckMismatch { .. }
        ));
        assert_eq!(initiator.snapshot(), before);
    }

    #[test]
    fn data_intents_include_current_ack_and_sack() {
        let syn = packet(PacketType::Syn, 50, 10, 0, &[]);
        let mut acceptor = ConnectionState::accept_syn(decoded(&syn), SequenceNumber::new(100))
            .expect("accept SYN");
        let reordered = packet(PacketType::Data, 51, 12, 99, b"later");
        acceptor
            .incoming(decoded(&reordered), 0)
            .expect("buffer reordered DATA");
        let outbound = acceptor.record_data(b"local", 10).expect("record DATA");
        assert_eq!(outbound.connection_id, 50);
        assert_eq!(outbound.sequence_number, SequenceNumber::new(100));
        assert_eq!(outbound.acknowledgement_number, SequenceNumber::new(10));
        assert!(outbound.selective_ack.expect("SACK").acknowledges_offset(2));
        assert_eq!(
            acceptor.payload_for_retransmission(outbound.sequence_number),
            Some(&b"local"[..])
        );
    }

    #[test]
    fn valid_established_packet_resets_timeout_backoff_without_ack_progress() {
        let syn = packet(PacketType::Syn, 50, 10, 0, &[]);
        let mut connection = ConnectionState::accept_syn(decoded(&syn), SequenceNumber::new(100))
            .expect("accept SYN");
        connection.record_data(b"owned", 0).expect("local DATA");
        connection
            .on_timeout(crate::utp::INITIAL_RTO_MICROS, true)
            .expect("timeout state")
            .expect("due timeout");
        assert_eq!(connection.snapshot().send.consecutive_timeouts, 1);

        let wrong_id = packet(PacketType::State, 50, 11, 99, &[]);
        let outcome = connection
            .incoming(decoded(&wrong_id), crate::utp::INITIAL_RTO_MICROS + 1)
            .expect("wrong connection ID is ignored");
        assert!(matches!(
            outcome.disposition,
            IncomingDisposition::WrongConnectionId { .. }
        ));
        assert_eq!(connection.snapshot().send.consecutive_timeouts, 1);

        let duplicate_ack = packet(PacketType::State, 51, 11, 99, &[]);
        let received_at = crate::utp::INITIAL_RTO_MICROS + 2;
        let outcome = connection
            .incoming(decoded(&duplicate_ack), received_at)
            .expect("valid duplicate ACK");
        assert_eq!(outcome.disposition, IncomingDisposition::Accepted);
        assert_eq!(
            outcome
                .acknowledgement
                .expect("acknowledgement outcome")
                .disposition,
            AckDisposition::Duplicate
        );
        assert_eq!(connection.snapshot().send.consecutive_timeouts, 0);
        assert_eq!(
            connection.snapshot().send.timeout_deadline_micros,
            Some(received_at + crate::utp::INITIAL_RTO_MICROS)
        );
    }

    #[test]
    fn receive_limit_failure_does_not_apply_the_packets_ack() {
        let syn = packet(PacketType::Syn, 50, 0, 0, &[]);
        let mut connection = ConnectionState::accept_syn(decoded(&syn), SequenceNumber::new(100))
            .expect("accept SYN");
        connection.record_data(b"owned", 0).expect("local DATA");
        let payload = vec![7; 65_000];
        for sequence_number in 2..=17 {
            let bytes = packet(PacketType::Data, 51, sequence_number, 99, &payload);
            connection
                .incoming(decoded(&bytes), 0)
                .expect("fill receive byte budget");
        }
        let before = connection.snapshot();
        let rejected = packet(PacketType::Data, 51, 18, 100, &payload);
        assert!(matches!(
            connection.incoming(decoded(&rejected), 1),
            Err(ConnectionError::Receive(
                ReceiveError::ReceiveWindowLimit { .. }
            ))
        ));
        assert_eq!(connection.snapshot(), before);
    }

    #[test]
    fn bidirectional_fin_requires_explicit_terminal_cleanup() {
        let syn = packet(PacketType::Syn, 50, 10, 0, &[]);
        let mut connection = ConnectionState::accept_syn(decoded(&syn), SequenceNumber::new(100))
            .expect("accept SYN");
        let local_fin = connection.record_fin(&[], 0).expect("local FIN");
        assert_eq!(connection.snapshot().phase, ConnectionPhase::LocalFinSent);
        assert_eq!(connection.finish(), Err(ConnectionError::NotReadyToClose));

        let remote_fin = packet(PacketType::Fin, 51, 11, 100, &[]);
        let outcome = connection
            .incoming(decoded(&remote_fin), 10)
            .expect("remote FIN and local ACK");
        assert_eq!(outcome.phase, ConnectionPhase::Closing);
        assert!(outcome.reply.is_some());
        assert!(connection.snapshot().ready_to_close);
        assert_eq!(local_fin.sequence_number, SequenceNumber::new(100));

        connection.finish().expect("terminal cleanup");
        let snapshot = connection.snapshot();
        assert_eq!(snapshot.phase, ConnectionPhase::Closed);
        assert_eq!(snapshot.send.outstanding_packets, 0);
        assert_eq!(snapshot.send.outstanding_bytes, 0);
        assert_eq!(snapshot.receive.expect("receive").queued_packets, 0);
    }

    #[test]
    fn reset_releases_send_and_receive_ownership_and_is_terminal() {
        let syn = packet(PacketType::Syn, 50, 10, 0, &[]);
        let mut connection = ConnectionState::accept_syn(decoded(&syn), SequenceNumber::new(100))
            .expect("accept SYN");
        connection.record_data(b"owned", 0).expect("local DATA");
        let reordered = packet(PacketType::Data, 51, 12, 99, b"queued");
        connection
            .incoming(decoded(&reordered), 0)
            .expect("queue remote DATA");
        let reset = packet(PacketType::Reset, 51, 200, 100, &[]);
        let outcome = connection.incoming(decoded(&reset), 1).expect("RESET");
        assert_eq!(outcome.phase, ConnectionPhase::Reset);
        let snapshot = connection.snapshot();
        assert_eq!(snapshot.send.outstanding_packets, 0);
        assert_eq!(snapshot.send.outstanding_bytes, 0);
        assert_eq!(snapshot.receive.expect("receive").queued_packets, 0);
        assert_eq!(
            connection
                .incoming(decoded(&reset), 2)
                .expect("terminal input")
                .disposition,
            IncomingDisposition::Terminal
        );
    }

    #[test]
    fn syn_with_sack_is_rejected_before_connection_construction() {
        let sack = [0_u8; 4];
        let bytes = encode_packet(PacketToEncode {
            header: UtpHeader {
                packet_type: PacketType::Syn,
                connection_id: 50,
                timestamp: TimestampMicros::new(0),
                timestamp_difference_micros: 0,
                window_size: 0,
                sequence_number: SequenceNumber::new(10),
                acknowledgement_number: SequenceNumber::new(0),
            },
            extensions: &[ExtensionToEncode {
                kind: SACK_EXTENSION,
                bytes: &sack,
            }],
            payload: &[],
        })
        .expect("encode SYN with SACK");
        assert_eq!(
            ConnectionState::accept_syn(decoded(&bytes), SequenceNumber::new(1))
                .expect_err("reject SYN SACK"),
            ConnectionError::SynHasSelectiveAck
        );
    }
}
