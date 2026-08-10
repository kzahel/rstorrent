//! Runtime-independent BEP 29 uTP v1 values and deterministic transport state.

mod congestion;
mod connection;
mod packet;
mod receive;
mod send;
mod sequence;
#[cfg(test)]
mod simulation;
mod transport;

pub use congestion::{
    BASE_DELAY_BUCKETS, CURRENT_DELAY_SAMPLE_LIMIT, CongestionAckOutcome, CongestionController,
    CongestionError, CongestionSnapshot, DelaySnapshot, INITIAL_CONGESTION_PACKETS,
    MAX_CONGESTION_WINDOW_BYTES, MIN_CONGESTION_PACKETS, Pacer, PacerSnapshot, TARGET_DELAY_MICROS,
};
pub use connection::{
    ConnectionError, ConnectionIds, ConnectionPhase, ConnectionSnapshot, ConnectionState,
    IncomingDisposition, IncomingOutcome, OutboundPacketIntent,
};
pub use packet::{
    DecodedPacket, Extension, ExtensionToEncode, Extensions, MAX_SACK_BYTES,
    MAX_UTP_EXTENSION_COUNT, MAX_UTP_PACKET_SIZE, MAX_UTP_PAYLOAD_SIZE, PacketToEncode, PacketType,
    SACK_EXTENSION, SelectiveAck, UTP_HEADER_SIZE, UTP_VERSION, UtpCodecError, UtpHeader,
    decode_packet, encode_packet,
};
pub use receive::{
    MAX_RECEIVE_BYTES, MAX_REORDER_BYTES, MAX_REORDER_DISTANCE, MAX_REORDER_PACKETS,
    ReceiveDisposition, ReceiveError, ReceiveOutcome, ReceiveSnapshot, ReceiveState,
    ReceivedPayload, SelectiveAckBits,
};
pub use send::{
    AckDisposition, AckOutcome, INITIAL_RTO_MICROS, MAX_RTO_MICROS, MAX_SENT_BYTES,
    MAX_SENT_PACKETS, MAX_TRANSMISSIONS, MIN_RTO_MICROS, RttSnapshot, SendError, SendSnapshot,
    SendState, SentPacketSnapshot, TimeoutOutcome,
};
pub use sequence::{SequenceNumber, SequenceRelation, TimestampMicros};
pub use transport::{
    AckScheduler, AckSchedulerSnapshot, MAX_DELAYED_ACK_MICROS, MAX_RETRANSMISSION_WORK,
    MAX_UNSENT_BYTES, RetransmissionQueue, RetransmissionSnapshot, TransmitQueue,
    TransmitQueueError, TransmitQueueSnapshot, new_payload_bytes, retransmission_is_admissible,
    utp_header_bytes,
};
