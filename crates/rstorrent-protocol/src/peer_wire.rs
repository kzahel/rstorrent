use std::error::Error;
use std::fmt;

use crate::metainfo::MAX_METAINFO_PIECES;
use crate::v2_hashes::{
    HASH_REQUEST_PAYLOAD_LENGTH, HashRequest, HashResponse, MAX_HASH_MESSAGE_LENGTH,
    MAX_HASH_PROOF_LAYERS, MAX_HASH_REQUEST_COUNT, MAX_HASHES_PER_RESPONSE,
};

pub const HANDSHAKE_LENGTH: usize = 68;
pub const MAX_REQUEST_BLOCK_LENGTH: u32 = 16 * 1024;
pub const MAX_CORE_FRAME_LENGTH: usize = 9 + MAX_REQUEST_BLOCK_LENGTH as usize;
pub const MAX_EXTENSION_PAYLOAD_LENGTH: usize = 17 * 1024;
pub const MAX_BITFIELD_PAYLOAD_LENGTH: usize = MAX_METAINFO_PIECES.div_ceil(8);
pub const MAX_FRAME_LENGTH: usize = 1 + MAX_BITFIELD_PAYLOAD_LENGTH;
pub const MAX_DECODER_INPUT_LENGTH: usize = 64 * 1024;
pub const EXTENSION_PROTOCOL_RESERVED_INDEX: usize = 5;
pub const EXTENSION_PROTOCOL_RESERVED_BIT: u8 = 0x10;
pub const FAST_EXTENSION_RESERVED_INDEX: usize = 7;
pub const FAST_EXTENSION_RESERVED_BIT: u8 = 0x04;
const MAX_MESSAGES_PER_PUSH: usize = 1024;
const PROTOCOL_NAME: &[u8; 19] = b"BitTorrent protocol";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PeerProtocol {
    #[default]
    V1,
    V2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handshake {
    pub peer_id: [u8; 20],
    pub reserved: [u8; 8],
}

impl Handshake {
    pub fn supports_extensions(&self) -> bool {
        self.reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] & EXTENSION_PROTOCOL_RESERVED_BIT != 0
    }

    pub fn supports_fast_extension(&self) -> bool {
        self.reserved[FAST_EXTENSION_RESERVED_INDEX] & FAST_EXTENSION_RESERVED_BIT != 0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NegotiatedPeerCapabilities {
    pub fast_extension: bool,
}

impl NegotiatedPeerCapabilities {
    pub fn negotiate(local_reserved: [u8; 8], remote: &Handshake) -> Self {
        Self {
            fast_extension: local_reserved[FAST_EXTENSION_RESERVED_INDEX]
                & FAST_EXTENSION_RESERVED_BIT
                != 0
                && remote.supports_fast_extension(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HandshakeError {
    InvalidLength { actual: usize },
    InvalidProtocol,
    InfoHashMismatch,
}

impl fmt::Display for HandshakeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLength { actual } => {
                write!(
                    formatter,
                    "peer handshake has length {actual}, expected {HANDSHAKE_LENGTH}"
                )
            }
            Self::InvalidProtocol => write!(formatter, "peer handshake protocol string is invalid"),
            Self::InfoHashMismatch => {
                write!(
                    formatter,
                    "peer handshake info hash does not match metainfo"
                )
            }
        }
    }
}

impl Error for HandshakeError {}

pub fn encode_handshake(info_hash: [u8; 20], peer_id: [u8; 20]) -> [u8; HANDSHAKE_LENGTH] {
    encode_handshake_with_reserved(info_hash, peer_id, [0; 8])
}

pub fn encode_handshake_with_reserved(
    info_hash: [u8; 20],
    peer_id: [u8; 20],
    reserved: [u8; 8],
) -> [u8; HANDSHAKE_LENGTH] {
    let mut bytes = [0_u8; HANDSHAKE_LENGTH];
    bytes[0] = PROTOCOL_NAME.len() as u8;
    bytes[1..20].copy_from_slice(PROTOCOL_NAME);
    bytes[20..28].copy_from_slice(&reserved);
    bytes[28..48].copy_from_slice(&info_hash);
    bytes[48..68].copy_from_slice(&peer_id);
    bytes
}

pub fn decode_handshake(
    bytes: &[u8],
    expected_info_hash: [u8; 20],
) -> Result<Handshake, HandshakeError> {
    if bytes.len() != HANDSHAKE_LENGTH {
        return Err(HandshakeError::InvalidLength {
            actual: bytes.len(),
        });
    }
    if bytes[0] != PROTOCOL_NAME.len() as u8 || &bytes[1..20] != PROTOCOL_NAME {
        return Err(HandshakeError::InvalidProtocol);
    }
    if bytes[28..48] != expected_info_hash {
        return Err(HandshakeError::InfoHashMismatch);
    }
    let peer_id = bytes[48..68]
        .try_into()
        .expect("handshake peer ID has a statically checked length");
    let reserved = bytes[20..28]
        .try_into()
        .expect("handshake reserved bytes have a statically checked length");
    Ok(Handshake { peer_id, reserved })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockRequest {
    pub index: u32,
    pub begin: u32,
    pub length: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeerMessage {
    KeepAlive,
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have(u32),
    Bitfield(Vec<u8>),
    Request(BlockRequest),
    Cancel(BlockRequest),
    Piece {
        index: u32,
        begin: u32,
        block: Vec<u8>,
    },
    SuggestPiece(u32),
    HaveAll,
    HaveNone,
    RejectRequest(BlockRequest),
    AllowedFast(u32),
    Extended {
        id: u8,
        payload: Vec<u8>,
    },
    HashRequest(HashRequest),
    Hashes(HashResponse),
    HashReject(HashRequest),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameError {
    InputChunkTooLarge { length: usize, maximum: usize },
    FrameLengthTooLarge { length: usize, maximum: usize },
    InvalidMessageLength { id: u8, length: usize },
    RequestBlockTooLarge { length: u32, maximum: u32 },
    InvalidHashRequest,
    InvalidHashCount { expected: usize, actual: usize },
    UnsupportedMessage { id: u8 },
    TooManyMessages { maximum: usize },
}

impl fmt::Display for FrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputChunkTooLarge { length, maximum } => write!(
                formatter,
                "peer input chunk length {length} exceeds limit {maximum}"
            ),
            Self::FrameLengthTooLarge { length, maximum } => {
                write!(
                    formatter,
                    "peer frame length {length} exceeds limit {maximum}"
                )
            }
            Self::InvalidMessageLength { id, length } => {
                write!(formatter, "peer message {id} has invalid length {length}")
            }
            Self::RequestBlockTooLarge { length, maximum } => write!(
                formatter,
                "peer request block length {length} exceeds limit {maximum}"
            ),
            Self::InvalidHashRequest => formatter.write_str("invalid v2 hash request shape"),
            Self::InvalidHashCount { expected, actual } => write!(
                formatter,
                "v2 hashes message has {actual} hashes, expected {expected}"
            ),
            Self::UnsupportedMessage { id } => {
                write!(
                    formatter,
                    "peer message {id} is unsupported by this diagnostic"
                )
            }
            Self::TooManyMessages { maximum } => {
                write!(
                    formatter,
                    "peer input contains more than {maximum} messages"
                )
            }
        }
    }
}

impl Error for FrameError {}

#[derive(Debug, Default)]
pub struct FrameDecoder {
    buffer: Vec<u8>,
    protocol: PeerProtocol,
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn for_v2() -> Self {
        Self {
            buffer: Vec::new(),
            protocol: PeerProtocol::V2,
        }
    }

    pub fn set_protocol(&mut self, protocol: PeerProtocol) {
        self.protocol = protocol;
    }

    pub fn push(&mut self, input: &[u8]) -> Result<Vec<PeerMessage>, FrameError> {
        if input.len() > MAX_DECODER_INPUT_LENGTH {
            return Err(FrameError::InputChunkTooLarge {
                length: input.len(),
                maximum: MAX_DECODER_INPUT_LENGTH,
            });
        }

        let mut messages = Vec::new();
        let mut consumed = 0;
        while consumed < input.len() || self.buffer.len() >= 4 {
            if self.buffer.len() < 4 {
                let copy_length = (4 - self.buffer.len()).min(input.len() - consumed);
                self.buffer
                    .extend_from_slice(&input[consumed..consumed + copy_length]);
                consumed += copy_length;
                if self.buffer.len() < 4 {
                    continue;
                }
            }

            let mut required = self.required_buffer_length()?;
            if self.buffer.len() < required {
                if consumed == input.len() {
                    break;
                }
                let copy_length = (required - self.buffer.len()).min(input.len() - consumed);
                self.buffer
                    .extend_from_slice(&input[consumed..consumed + copy_length]);
                consumed += copy_length;
            }
            required = self.required_buffer_length()?;
            if self.buffer.len() < required {
                continue;
            }
            if messages.len() == MAX_MESSAGES_PER_PUSH {
                return Err(FrameError::TooManyMessages {
                    maximum: MAX_MESSAGES_PER_PUSH,
                });
            }

            if required == 4 {
                messages.push(PeerMessage::KeepAlive);
                self.buffer.clear();
            } else {
                let frame = std::mem::take(&mut self.buffer);
                messages.push(decode_frame(frame, self.protocol)?);
            }
        }
        Ok(messages)
    }

    fn required_buffer_length(&self) -> Result<usize, FrameError> {
        if self.buffer.len() < 4 {
            return Ok(4);
        }

        let frame_length =
            u32::from_be_bytes(self.buffer[..4].try_into().expect("four-byte prefix")) as usize;
        if frame_length > MAX_FRAME_LENGTH {
            return Err(FrameError::FrameLengthTooLarge {
                length: frame_length,
                maximum: MAX_FRAME_LENGTH,
            });
        }
        if frame_length != 0 && self.buffer.len() == 4 {
            return Ok(5);
        }
        if frame_length != 0 {
            let id = self.buffer[4];
            let maximum = frame_maximum(id, self.protocol)?;
            if frame_length > maximum {
                return Err(FrameError::FrameLengthTooLarge {
                    length: frame_length,
                    maximum,
                });
            }
        }
        Ok(4 + frame_length)
    }
}

pub fn encode_message(message: &PeerMessage) -> Result<Vec<u8>, FrameError> {
    let mut payload = Vec::new();
    match message {
        PeerMessage::KeepAlive => return Ok(vec![0, 0, 0, 0]),
        PeerMessage::Choke => payload.push(0),
        PeerMessage::Unchoke => payload.push(1),
        PeerMessage::Interested => payload.push(2),
        PeerMessage::NotInterested => payload.push(3),
        PeerMessage::Have(index) => {
            payload.push(4);
            payload.extend_from_slice(&index.to_be_bytes());
        }
        PeerMessage::Bitfield(bitfield) => {
            payload.push(5);
            payload.extend_from_slice(bitfield);
        }
        PeerMessage::Request(request)
        | PeerMessage::Cancel(request)
        | PeerMessage::RejectRequest(request) => {
            validate_request_length(request.length)?;
            payload.push(match message {
                PeerMessage::Request(_) => 6,
                PeerMessage::Cancel(_) => 8,
                PeerMessage::RejectRequest(_) => 16,
                _ => unreachable!("request-shaped message match is exhaustive"),
            });
            payload.extend_from_slice(&request.index.to_be_bytes());
            payload.extend_from_slice(&request.begin.to_be_bytes());
            payload.extend_from_slice(&request.length.to_be_bytes());
        }
        PeerMessage::Piece {
            index,
            begin,
            block,
        } => {
            let block_length =
                u32::try_from(block.len()).map_err(|_| FrameError::RequestBlockTooLarge {
                    length: u32::MAX,
                    maximum: MAX_REQUEST_BLOCK_LENGTH,
                })?;
            validate_request_length(block_length)?;
            payload.push(7);
            payload.extend_from_slice(&index.to_be_bytes());
            payload.extend_from_slice(&begin.to_be_bytes());
            payload.extend_from_slice(block);
        }
        PeerMessage::SuggestPiece(index) => {
            payload.push(13);
            payload.extend_from_slice(&index.to_be_bytes());
        }
        PeerMessage::HaveAll => payload.push(14),
        PeerMessage::HaveNone => payload.push(15),
        PeerMessage::AllowedFast(index) => {
            payload.push(17);
            payload.extend_from_slice(&index.to_be_bytes());
        }
        PeerMessage::Extended {
            id,
            payload: extension_payload,
        } => {
            if extension_payload.len() > MAX_EXTENSION_PAYLOAD_LENGTH {
                return Err(FrameError::FrameLengthTooLarge {
                    length: extension_payload.len(),
                    maximum: MAX_EXTENSION_PAYLOAD_LENGTH,
                });
            }
            payload.push(20);
            payload.push(*id);
            payload.extend_from_slice(extension_payload);
        }
        PeerMessage::HashRequest(request) => {
            validate_hash_wire_request(*request, false)?;
            payload.push(21);
            encode_hash_request_payload(&mut payload, *request);
        }
        PeerMessage::Hashes(response) => {
            validate_hash_wire_request(response.request, true)?;
            let expected = response
                .request
                .response_hash_count()
                .map_err(|_| FrameError::InvalidHashRequest)?;
            if response.hashes.len() != expected {
                return Err(FrameError::InvalidHashCount {
                    expected,
                    actual: response.hashes.len(),
                });
            }
            payload.push(22);
            encode_hash_request_payload(&mut payload, response.request);
            for hash in &response.hashes {
                payload.extend_from_slice(hash);
            }
        }
        PeerMessage::HashReject(request) => {
            validate_hash_wire_request(*request, true)?;
            payload.push(23);
            encode_hash_request_payload(&mut payload, *request);
        }
    }
    let maximum = match message {
        PeerMessage::Extended { .. } => 2 + MAX_EXTENSION_PAYLOAD_LENGTH,
        PeerMessage::Bitfield(_) => 1 + MAX_BITFIELD_PAYLOAD_LENGTH,
        PeerMessage::HashRequest(_) | PeerMessage::HashReject(_) => 1 + HASH_REQUEST_PAYLOAD_LENGTH,
        PeerMessage::Hashes(_) => MAX_HASH_MESSAGE_LENGTH,
        _ => MAX_CORE_FRAME_LENGTH,
    };
    if payload.len() > maximum {
        return Err(FrameError::FrameLengthTooLarge {
            length: payload.len(),
            maximum,
        });
    }

    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

fn decode_frame(mut frame: Vec<u8>, protocol: PeerProtocol) -> Result<PeerMessage, FrameError> {
    let id = frame[4];
    let length = frame.len() - 4;
    let maximum = frame_maximum(id, protocol)?;
    if length > maximum {
        return Err(FrameError::FrameLengthTooLarge { length, maximum });
    }
    match id {
        0 => exact_length(id, length, 1).map(|()| PeerMessage::Choke),
        1 => exact_length(id, length, 1).map(|()| PeerMessage::Unchoke),
        2 => exact_length(id, length, 1).map(|()| PeerMessage::Interested),
        3 => exact_length(id, length, 1).map(|()| PeerMessage::NotInterested),
        4 => {
            exact_length(id, length, 5)?;
            Ok(PeerMessage::Have(u32::from_be_bytes(
                frame[5..9].try_into().expect("validated have payload"),
            )))
        }
        5 => {
            if length < 2 {
                return Err(FrameError::InvalidMessageLength { id, length });
            }
            frame.drain(..5);
            Ok(PeerMessage::Bitfield(frame))
        }
        6 | 8 | 16 => {
            exact_length(id, length, 13)?;
            let request = BlockRequest {
                index: read_u32(&frame, 5),
                begin: read_u32(&frame, 9),
                length: read_u32(&frame, 13),
            };
            validate_request_length(request.length)?;
            Ok(match id {
                6 => PeerMessage::Request(request),
                8 => PeerMessage::Cancel(request),
                16 => PeerMessage::RejectRequest(request),
                _ => unreachable!("request-shaped message ID match is exhaustive"),
            })
        }
        7 => {
            if !(10..=MAX_CORE_FRAME_LENGTH).contains(&length) {
                return Err(FrameError::InvalidMessageLength { id, length });
            }
            let index = read_u32(&frame, 5);
            let begin = read_u32(&frame, 9);
            frame.drain(..13);
            Ok(PeerMessage::Piece {
                index,
                begin,
                block: frame,
            })
        }
        13 | 17 => {
            exact_length(id, length, 5)?;
            let index = read_u32(&frame, 5);
            Ok(if id == 13 {
                PeerMessage::SuggestPiece(index)
            } else {
                PeerMessage::AllowedFast(index)
            })
        }
        14 => exact_length(id, length, 1).map(|()| PeerMessage::HaveAll),
        15 => exact_length(id, length, 1).map(|()| PeerMessage::HaveNone),
        20 => {
            if length < 2 {
                return Err(FrameError::InvalidMessageLength { id, length });
            }
            let extension_id = frame[5];
            frame.drain(..6);
            Ok(PeerMessage::Extended {
                id: extension_id,
                payload: frame,
            })
        }
        21 | 23 => {
            exact_length(id, length, 1 + HASH_REQUEST_PAYLOAD_LENGTH)?;
            let request = decode_hash_request_payload(&frame)?;
            validate_hash_wire_request(request, id == 23)?;
            Ok(if id == 21 {
                PeerMessage::HashRequest(request)
            } else {
                PeerMessage::HashReject(request)
            })
        }
        22 => {
            if length < 1 + HASH_REQUEST_PAYLOAD_LENGTH {
                return Err(FrameError::InvalidMessageLength { id, length });
            }
            let request = decode_hash_request_payload(&frame)?;
            validate_hash_wire_request(request, true)?;
            let expected = request
                .response_hash_count()
                .map_err(|_| FrameError::InvalidHashRequest)?;
            let hash_bytes = length - 1 - HASH_REQUEST_PAYLOAD_LENGTH;
            if !hash_bytes.is_multiple_of(32) {
                return Err(FrameError::InvalidMessageLength { id, length });
            }
            let actual = hash_bytes / 32;
            if actual != expected || actual > MAX_HASHES_PER_RESPONSE {
                return Err(FrameError::InvalidHashCount { expected, actual });
            }
            let mut hashes = Vec::with_capacity(actual);
            for chunk in frame[5 + HASH_REQUEST_PAYLOAD_LENGTH..].chunks_exact(32) {
                hashes.push(chunk.try_into().expect("validated SHA-256 hash length"));
            }
            Ok(PeerMessage::Hashes(HashResponse { request, hashes }))
        }
        _ => Err(FrameError::UnsupportedMessage { id }),
    }
}

fn frame_maximum(id: u8, protocol: PeerProtocol) -> Result<usize, FrameError> {
    match id {
        5 => Ok(1 + MAX_BITFIELD_PAYLOAD_LENGTH),
        20 => Ok(2 + MAX_EXTENSION_PAYLOAD_LENGTH),
        21..=23 if protocol == PeerProtocol::V1 => Err(FrameError::UnsupportedMessage { id }),
        21 | 23 => Ok(1 + HASH_REQUEST_PAYLOAD_LENGTH),
        22 => Ok(MAX_HASH_MESSAGE_LENGTH),
        _ => Ok(MAX_CORE_FRAME_LENGTH),
    }
}

fn validate_hash_wire_request(
    request: HashRequest,
    allow_count_one: bool,
) -> Result<(), FrameError> {
    if request.count == 0
        || request.count > MAX_HASH_REQUEST_COUNT
        || !request.count.is_power_of_two()
        || (request.count == 1 && !allow_count_one)
        || !request.index.is_multiple_of(request.count)
        || request.proof_layers > MAX_HASH_PROOF_LAYERS
    {
        return Err(FrameError::InvalidHashRequest);
    }
    Ok(())
}

fn encode_hash_request_payload(output: &mut Vec<u8>, request: HashRequest) {
    output.extend_from_slice(&request.pieces_root);
    output.extend_from_slice(&request.base_layer.to_be_bytes());
    output.extend_from_slice(&request.index.to_be_bytes());
    output.extend_from_slice(&request.count.to_be_bytes());
    output.extend_from_slice(&request.proof_layers.to_be_bytes());
}

fn decode_hash_request_payload(frame: &[u8]) -> Result<HashRequest, FrameError> {
    if frame.len() < 5 + HASH_REQUEST_PAYLOAD_LENGTH {
        return Err(FrameError::InvalidMessageLength {
            id: frame.get(4).copied().unwrap_or_default(),
            length: frame.len().saturating_sub(4),
        });
    }
    Ok(HashRequest {
        pieces_root: frame[5..37]
            .try_into()
            .expect("validated pieces root length"),
        base_layer: read_u32(frame, 37),
        index: read_u32(frame, 41),
        count: read_u32(frame, 45),
        proof_layers: read_u32(frame, 49),
    })
}

fn exact_length(id: u8, actual: usize, expected: usize) -> Result<(), FrameError> {
    if actual != expected {
        return Err(FrameError::InvalidMessageLength { id, length: actual });
    }
    Ok(())
}

fn read_u32(payload: &[u8], start: usize) -> u32 {
    u32::from_be_bytes(
        payload[start..start + 4]
            .try_into()
            .expect("message payload length was validated"),
    )
}

fn validate_request_length(length: u32) -> Result<(), FrameError> {
    if length == 0 || length > MAX_REQUEST_BLOCK_LENGTH {
        return Err(FrameError::RequestBlockTooLarge {
            length,
            maximum: MAX_REQUEST_BLOCK_LENGTH,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::v2_hashes::{HashRequest, HashResponse};

    use super::{
        BlockRequest, EXTENSION_PROTOCOL_RESERVED_BIT, EXTENSION_PROTOCOL_RESERVED_INDEX,
        FAST_EXTENSION_RESERVED_BIT, FAST_EXTENSION_RESERVED_INDEX, FrameDecoder, FrameError,
        HandshakeError, MAX_BITFIELD_PAYLOAD_LENGTH, MAX_EXTENSION_PAYLOAD_LENGTH,
        MAX_FRAME_LENGTH, NegotiatedPeerCapabilities, PeerMessage, decode_handshake,
        encode_handshake, encode_handshake_with_reserved, encode_message,
    };

    #[test]
    fn handshake_round_trip_validates_protocol_and_info_hash() {
        let info_hash = [3; 20];
        let peer_id = [4; 20];
        let bytes = encode_handshake(info_hash, peer_id);

        assert_eq!(
            decode_handshake(&bytes, info_hash),
            Ok(super::Handshake {
                peer_id,
                reserved: [0; 8]
            })
        );

        let mut invalid_protocol = bytes;
        invalid_protocol[1] = b'X';
        assert_eq!(
            decode_handshake(&invalid_protocol, info_hash),
            Err(HandshakeError::InvalidProtocol)
        );
        assert_eq!(
            decode_handshake(&bytes, [5; 20]),
            Err(HandshakeError::InfoHashMismatch)
        );
    }

    #[test]
    fn handshake_round_trip_exposes_extension_support() {
        let mut reserved = [0; 8];
        reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
        let bytes = encode_handshake_with_reserved([3; 20], [4; 20], reserved);

        let handshake = decode_handshake(&bytes, [3; 20]).expect("extension handshake");

        assert_eq!(handshake.reserved, reserved);
        assert!(handshake.supports_extensions());
    }

    #[test]
    fn fast_extension_negotiation_is_bilateral() {
        let mut local = [0; 8];
        local[FAST_EXTENSION_RESERVED_INDEX] = FAST_EXTENSION_RESERVED_BIT;
        let remote_without_fast = super::Handshake {
            peer_id: [1; 20],
            reserved: [0; 8],
        };
        assert!(!NegotiatedPeerCapabilities::negotiate(local, &remote_without_fast).fast_extension);

        let mut remote_with_fast = remote_without_fast;
        remote_with_fast.reserved[FAST_EXTENSION_RESERVED_INDEX] = FAST_EXTENSION_RESERVED_BIT;
        assert!(remote_with_fast.supports_fast_extension());
        assert!(NegotiatedPeerCapabilities::negotiate(local, &remote_with_fast).fast_extension);
        assert!(!NegotiatedPeerCapabilities::negotiate([0; 8], &remote_with_fast).fast_extension);
    }

    #[test]
    fn decoder_handles_fragmented_and_coalesced_frames() {
        let choke = encode_message(&PeerMessage::Choke).expect("encode choke");
        let have = encode_message(&PeerMessage::Have(7)).expect("encode have");
        let mut combined = choke;
        combined.extend_from_slice(&have);
        let mut decoder = FrameDecoder::new();

        assert_eq!(decoder.push(&combined[..2]).expect("prefix fragment"), []);
        assert_eq!(
            decoder.push(&combined[2..]).expect("remaining frames"),
            [PeerMessage::Choke, PeerMessage::Have(7)]
        );
    }

    #[test]
    fn decoder_handles_piece_payload_fragmentation() {
        let message = PeerMessage::Piece {
            index: 1,
            begin: 16_384,
            block: vec![8; 97],
        };
        let frame = encode_message(&message).expect("encode piece");
        let mut decoder = FrameDecoder::new();

        for byte in &frame[..frame.len() - 1] {
            assert!(decoder.push(&[*byte]).expect("fragment").is_empty());
        }
        assert_eq!(
            decoder
                .push(&frame[frame.len() - 1..])
                .expect("last fragment"),
            [message]
        );
    }

    #[test]
    fn rejects_oversized_and_invalid_frame_lengths() {
        let oversized = ((MAX_FRAME_LENGTH + 1) as u32).to_be_bytes();
        let mut decoder = FrameDecoder::new();
        assert!(matches!(
            decoder.push(&oversized),
            Err(FrameError::FrameLengthTooLarge { .. })
        ));

        let mut decoder = FrameDecoder::new();
        assert_eq!(
            decoder.push(&[0, 0, 0, 2, 2, 0]),
            Err(FrameError::InvalidMessageLength { id: 2, length: 2 })
        );

        let mut decoder = FrameDecoder::new();
        assert_eq!(
            decoder.push(&[0, 0, 0, 1, 99]),
            Err(FrameError::UnsupportedMessage { id: 99 })
        );
    }

    #[test]
    fn request_and_cancel_round_trip_enforce_block_limit() {
        let block = BlockRequest {
            index: 2,
            begin: 32_768,
            length: 16_384,
        };
        for message in [PeerMessage::Request(block), PeerMessage::Cancel(block)] {
            let frame = encode_message(&message).expect("encode request-shaped message");
            let mut decoder = FrameDecoder::new();
            assert_eq!(
                decoder.push(&frame).expect("decode request-shaped message"),
                [message]
            );
        }

        for message in [
            PeerMessage::Request(BlockRequest {
                index: 0,
                begin: 0,
                length: 16_385,
            }),
            PeerMessage::Cancel(BlockRequest {
                index: 0,
                begin: 0,
                length: 0,
            }),
        ] {
            assert!(matches!(
                encode_message(&message),
                Err(FrameError::RequestBlockTooLarge { .. })
            ));
        }

        for id in [6, 8] {
            let mut decoder = FrameDecoder::new();
            assert_eq!(
                decoder.push(&[0, 0, 0, 1, id]),
                Err(FrameError::InvalidMessageLength { id, length: 1 })
            );
        }

        let mut zero_length_cancel = vec![0, 0, 0, 13, 8];
        zero_length_cancel.extend_from_slice(&[0; 12]);
        let mut decoder = FrameDecoder::new();
        assert_eq!(
            decoder.push(&zero_length_cancel),
            Err(FrameError::RequestBlockTooLarge {
                length: 0,
                maximum: super::MAX_REQUEST_BLOCK_LENGTH,
            })
        );
    }

    #[test]
    fn fast_messages_round_trip_with_exact_lengths() {
        let request = BlockRequest {
            index: 17,
            begin: 16_384,
            length: 16_384,
        };
        let messages = [
            PeerMessage::SuggestPiece(9),
            PeerMessage::HaveAll,
            PeerMessage::HaveNone,
            PeerMessage::RejectRequest(request),
            PeerMessage::AllowedFast(11),
        ];
        let mut combined = Vec::new();
        for message in &messages {
            combined.extend(encode_message(message).expect("encode Fast message"));
        }
        assert_eq!(
            FrameDecoder::new()
                .push(&combined)
                .expect("decode coalesced Fast messages"),
            messages
        );

        for (id, length) in [(13, 1), (14, 5), (15, 5), (16, 5), (17, 1)] {
            let mut frame = vec![0, 0, 0, length as u8, id];
            frame.resize(4 + length, 0);
            assert_eq!(
                FrameDecoder::new().push(&frame),
                Err(FrameError::InvalidMessageLength { id, length })
            );
        }
    }

    #[test]
    fn v2_hash_messages_fail_closed_for_v1_decoders() {
        for id in [21, 22, 23] {
            let frame = [0, 0, 0, 1, id];
            assert_eq!(
                FrameDecoder::new().push(&frame),
                Err(FrameError::UnsupportedMessage { id })
            );

            let oversized = u32::try_from(super::MAX_CORE_FRAME_LENGTH + 1)
                .expect("core frame limit fits u32")
                .to_be_bytes();
            let mut frame = oversized.to_vec();
            frame.push(id);
            frame.resize(4 + super::MAX_CORE_FRAME_LENGTH + 1, 0);
            assert_eq!(
                FrameDecoder::new().push(&frame),
                Err(FrameError::UnsupportedMessage { id })
            );
        }
    }

    #[test]
    fn v2_hash_messages_round_trip_exact_bep52_fields() {
        let request = HashRequest {
            pieces_root: [0x11; 32],
            base_layer: 3,
            index: 4,
            count: 2,
            proof_layers: 5,
        };
        let response = HashResponse {
            request,
            hashes: vec![
                [0x21; 32], [0x22; 32], [0x31; 32], [0x32; 32], [0x33; 32], [0x34; 32], [0x35; 32],
            ],
        };
        let messages = [
            PeerMessage::HashRequest(request),
            PeerMessage::Hashes(response),
            PeerMessage::HashReject(request),
        ];
        let mut wire = Vec::new();
        for message in &messages {
            wire.extend(encode_message(message).expect("encode v2 hash message"));
        }
        assert_eq!(&wire[..5], &[0, 0, 0, 49, 21]);
        assert_eq!(&wire[5..37], &[0x11; 32]);
        assert_eq!(
            &wire[37..53],
            &[0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0, 2, 0, 0, 0, 5]
        );

        let mut decoder = FrameDecoder::for_v2();
        let mut decoded = Vec::new();
        for chunk in wire.chunks(17) {
            decoded.extend(decoder.push(chunk).expect("decode fragmented v2 hashes"));
        }
        assert_eq!(decoded, messages);
    }

    #[test]
    fn v2_hash_decoder_rejects_hostile_shape_before_hash_allocation() {
        let request = HashRequest {
            pieces_root: [7; 32],
            base_layer: 0,
            index: 0,
            count: 2,
            proof_layers: 0,
        };
        assert!(encode_message(&PeerMessage::HashRequest(request)).is_ok());

        let mut malformed = vec![0, 0, 0, 49, 21];
        malformed.extend_from_slice(&[0; 32]);
        malformed.extend_from_slice(&0_u32.to_be_bytes());
        malformed.extend_from_slice(&0_u32.to_be_bytes());
        malformed.extend_from_slice(&u32::MAX.to_be_bytes());
        malformed.extend_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(
            FrameDecoder::for_v2().push(&malformed),
            Err(FrameError::InvalidHashRequest)
        );

        let valid = HashRequest {
            count: 2,
            proof_layers: 2,
            ..request
        };
        let mut wrong_count = encode_message(&PeerMessage::Hashes(HashResponse {
            request: valid,
            hashes: vec![[1; 32], [2; 32], [3; 32], [4; 32]],
        }))
        .expect("encode valid hashes");
        wrong_count.truncate(wrong_count.len() - 32);
        let wrong_length = u32::try_from(wrong_count.len() - 4).unwrap();
        wrong_count[..4].copy_from_slice(&wrong_length.to_be_bytes());
        assert_eq!(
            FrameDecoder::for_v2().push(&wrong_count),
            Err(FrameError::InvalidHashCount {
                expected: 4,
                actual: 3,
            })
        );

        let mut oversized = (super::MAX_HASH_MESSAGE_LENGTH as u32 + 1)
            .to_be_bytes()
            .to_vec();
        oversized.push(22);
        assert_eq!(
            FrameDecoder::for_v2().push(&oversized),
            Err(FrameError::FrameLengthTooLarge {
                length: super::MAX_HASH_MESSAGE_LENGTH + 1,
                maximum: super::MAX_HASH_MESSAGE_LENGTH,
            })
        );

        let singleton = HashRequest {
            count: 1,
            proof_layers: 2,
            ..valid
        };
        assert_eq!(
            encode_message(&PeerMessage::HashRequest(singleton)),
            Err(FrameError::InvalidHashRequest)
        );
        let reject = PeerMessage::HashReject(singleton);
        assert_eq!(
            FrameDecoder::for_v2()
                .push(&encode_message(&reject).expect("count-one compatibility reject"))
                .expect("decode count-one compatibility reject"),
            [reject]
        );
    }

    #[test]
    fn v2_hash_decoder_accepts_libtorrent_whole_padded_leaf_request() {
        let request = HashRequest {
            pieces_root: [0x42; 32],
            base_layer: 0,
            index: 0,
            count: 4,
            proof_layers: 1,
        };
        let frame = encode_message(&PeerMessage::HashReject(request))
            .expect("encode libtorrent-shaped hash reject");
        assert_eq!(
            FrameDecoder::for_v2()
                .push(&frame)
                .expect("decode libtorrent-shaped hash reject"),
            [PeerMessage::HashReject(request)]
        );
        assert_eq!(request.response_hash_count(), Ok(4));
    }

    #[test]
    fn keepalive_has_no_message_id() {
        let frame = encode_message(&PeerMessage::KeepAlive).expect("encode keepalive");
        let mut decoder = FrameDecoder::new();
        assert_eq!(
            decoder.push(&frame).expect("decode keepalive"),
            [PeerMessage::KeepAlive]
        );
    }

    #[test]
    fn extended_message_round_trip_handles_fragments_and_enforces_its_ceiling() {
        let message = PeerMessage::Extended {
            id: 7,
            payload: vec![9; MAX_EXTENSION_PAYLOAD_LENGTH],
        };
        let frame = encode_message(&message).expect("maximum extension message");
        let mut decoder = FrameDecoder::new();
        assert!(
            decoder
                .push(&frame[..5])
                .expect("first fragment")
                .is_empty()
        );
        assert_eq!(
            decoder.push(&frame[5..]).expect("remaining extension"),
            [message]
        );

        assert!(matches!(
            encode_message(&PeerMessage::Extended {
                id: 1,
                payload: vec![0; MAX_EXTENSION_PAYLOAD_LENGTH + 1],
            }),
            Err(FrameError::FrameLengthTooLarge { .. })
        ));

        let mut decoder = FrameDecoder::new();
        assert_eq!(
            decoder.push(&[0, 0, 0, 1, 20]),
            Err(FrameError::InvalidMessageLength { id: 20, length: 1 })
        );
    }

    #[test]
    fn maximum_geometry_bitfield_is_admitted_without_raising_other_frames() {
        let message = PeerMessage::Bitfield(vec![0xff; MAX_BITFIELD_PAYLOAD_LENGTH]);
        let frame = encode_message(&message).expect("maximum bitfield");
        let mut decoder = FrameDecoder::new();
        let mut decoded = Vec::new();
        for chunk in frame.chunks(super::MAX_DECODER_INPUT_LENGTH) {
            decoded.extend(decoder.push(chunk).expect("bounded bitfield fragment"));
        }
        assert_eq!(decoded, [message]);
        assert!(matches!(
            encode_message(&PeerMessage::Bitfield(vec![
                0;
                MAX_BITFIELD_PAYLOAD_LENGTH + 1
            ])),
            Err(FrameError::FrameLengthTooLarge { .. })
        ));
    }
}
