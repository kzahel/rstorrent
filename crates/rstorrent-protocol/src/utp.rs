//! Runtime-independent BEP 29 uTP v1 values and deterministic transport state.

mod packet;
mod receive;
mod sequence;

pub use packet::{
    DecodedPacket, Extension, ExtensionToEncode, Extensions, MAX_SACK_BYTES,
    MAX_UTP_EXTENSION_COUNT, MAX_UTP_PACKET_SIZE, MAX_UTP_PAYLOAD_SIZE, PacketToEncode, PacketType,
    SACK_EXTENSION, SelectiveAck, UTP_HEADER_SIZE, UTP_VERSION, UtpCodecError, UtpHeader,
    decode_packet, encode_packet,
};
pub use receive::{
    MAX_REORDER_BYTES, MAX_REORDER_DISTANCE, MAX_REORDER_PACKETS, ReceiveDisposition, ReceiveError,
    ReceiveOutcome, ReceiveSnapshot, ReceiveState, ReceivedPayload, SelectiveAckBits,
};
pub use sequence::{SequenceNumber, SequenceRelation, TimestampMicros};
