use std::error::Error;
use std::fmt;

pub const HANDSHAKE_LENGTH: usize = 68;
pub const MAX_REQUEST_BLOCK_LENGTH: u32 = 16 * 1024;
pub const MAX_FRAME_LENGTH: usize = 9 + MAX_REQUEST_BLOCK_LENGTH as usize;
pub const MAX_DECODER_INPUT_LENGTH: usize = 64 * 1024;
const MAX_MESSAGES_PER_PUSH: usize = 1024;
const PROTOCOL_NAME: &[u8; 19] = b"BitTorrent protocol";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handshake {
    pub peer_id: [u8; 20],
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
    let mut bytes = [0_u8; HANDSHAKE_LENGTH];
    bytes[0] = PROTOCOL_NAME.len() as u8;
    bytes[1..20].copy_from_slice(PROTOCOL_NAME);
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
    Ok(Handshake { peer_id })
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
    Piece {
        index: u32,
        begin: u32,
        block: Vec<u8>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameError {
    InputChunkTooLarge { length: usize, maximum: usize },
    FrameLengthTooLarge { length: usize, maximum: usize },
    InvalidMessageLength { id: u8, length: usize },
    RequestBlockTooLarge { length: u32, maximum: u32 },
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
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self::default()
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

            let required = self.required_buffer_length()?;
            if self.buffer.len() < required {
                if consumed == input.len() {
                    break;
                }
                let copy_length = (required - self.buffer.len()).min(input.len() - consumed);
                self.buffer
                    .extend_from_slice(&input[consumed..consumed + copy_length]);
                consumed += copy_length;
            }
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
            } else {
                messages.push(decode_payload(&self.buffer[4..])?);
            }
            self.buffer.clear();
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
        PeerMessage::Request(request) => {
            validate_request_length(request.length)?;
            payload.push(6);
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
    }
    if payload.len() > MAX_FRAME_LENGTH {
        return Err(FrameError::FrameLengthTooLarge {
            length: payload.len(),
            maximum: MAX_FRAME_LENGTH,
        });
    }

    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

fn decode_payload(payload: &[u8]) -> Result<PeerMessage, FrameError> {
    let id = payload[0];
    let length = payload.len();
    match id {
        0 => exact_length(id, length, 1).map(|()| PeerMessage::Choke),
        1 => exact_length(id, length, 1).map(|()| PeerMessage::Unchoke),
        2 => exact_length(id, length, 1).map(|()| PeerMessage::Interested),
        3 => exact_length(id, length, 1).map(|()| PeerMessage::NotInterested),
        4 => {
            exact_length(id, length, 5)?;
            Ok(PeerMessage::Have(u32::from_be_bytes(
                payload[1..5].try_into().expect("validated have payload"),
            )))
        }
        5 => {
            if length < 2 {
                return Err(FrameError::InvalidMessageLength { id, length });
            }
            Ok(PeerMessage::Bitfield(payload[1..].to_vec()))
        }
        6 => {
            exact_length(id, length, 13)?;
            let request = BlockRequest {
                index: read_u32(payload, 1),
                begin: read_u32(payload, 5),
                length: read_u32(payload, 9),
            };
            validate_request_length(request.length)?;
            Ok(PeerMessage::Request(request))
        }
        7 => {
            if !(10..=MAX_FRAME_LENGTH).contains(&length) {
                return Err(FrameError::InvalidMessageLength { id, length });
            }
            Ok(PeerMessage::Piece {
                index: read_u32(payload, 1),
                begin: read_u32(payload, 5),
                block: payload[9..].to_vec(),
            })
        }
        _ => Err(FrameError::UnsupportedMessage { id }),
    }
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
    use super::{
        BlockRequest, FrameDecoder, FrameError, HandshakeError, MAX_FRAME_LENGTH, PeerMessage,
        decode_handshake, encode_handshake, encode_message,
    };

    #[test]
    fn handshake_round_trip_validates_protocol_and_info_hash() {
        let info_hash = [3; 20];
        let peer_id = [4; 20];
        let bytes = encode_handshake(info_hash, peer_id);

        assert_eq!(
            decode_handshake(&bytes, info_hash),
            Ok(super::Handshake { peer_id })
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
    fn request_round_trip_enforces_block_limit() {
        let request = PeerMessage::Request(BlockRequest {
            index: 2,
            begin: 32_768,
            length: 16_384,
        });
        let frame = encode_message(&request).expect("encode request");
        let mut decoder = FrameDecoder::new();
        assert_eq!(decoder.push(&frame).expect("decode request"), [request]);

        assert!(matches!(
            encode_message(&PeerMessage::Request(BlockRequest {
                index: 0,
                begin: 0,
                length: 16_385,
            })),
            Err(FrameError::RequestBlockTooLarge { .. })
        ));
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
}
