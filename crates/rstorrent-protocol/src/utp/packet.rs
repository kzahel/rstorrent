//! Hostile-input uTP v1 packet and extension codec.

use std::error::Error;
use std::fmt;

use super::{SequenceNumber, TimestampMicros};

pub const UTP_VERSION: u8 = 1;
pub const UTP_HEADER_SIZE: usize = 20;
pub const MAX_UTP_PACKET_SIZE: usize = u16::MAX as usize;
pub const MAX_UTP_EXTENSION_COUNT: usize = 8;
pub const SACK_EXTENSION: u8 = 1;
pub const MAX_SACK_BYTES: usize = 252;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum PacketType {
    Data = 0,
    Fin = 1,
    State = 2,
    Reset = 3,
    Syn = 4,
}

impl PacketType {
    const fn from_wire(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Data),
            1 => Some(Self::Fin),
            2 => Some(Self::State),
            3 => Some(Self::Reset),
            4 => Some(Self::Syn),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UtpHeader {
    pub packet_type: PacketType,
    pub connection_id: u16,
    pub timestamp: TimestampMicros,
    pub timestamp_difference_micros: u32,
    pub window_size: u32,
    pub sequence_number: SequenceNumber,
    pub acknowledgement_number: SequenceNumber,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Extension<'a> {
    pub kind: u8,
    pub bytes: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtensionToEncode<'a> {
    pub kind: u8,
    pub bytes: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PacketToEncode<'a> {
    pub header: UtpHeader,
    pub extensions: &'a [ExtensionToEncode<'a>],
    pub payload: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecodedPacket<'a> {
    pub header: UtpHeader,
    first_extension: u8,
    extension_bytes: &'a [u8],
    extension_count: u8,
    selective_ack: Option<&'a [u8]>,
    payload: &'a [u8],
}

impl<'a> DecodedPacket<'a> {
    #[must_use]
    pub const fn payload(&self) -> &'a [u8] {
        self.payload
    }

    #[must_use]
    pub const fn extension_count(&self) -> usize {
        self.extension_count as usize
    }

    #[must_use]
    pub fn extensions(&self) -> Extensions<'a> {
        Extensions {
            next_kind: self.first_extension,
            remaining: self.extension_bytes,
            remaining_count: self.extension_count,
        }
    }

    #[must_use]
    pub const fn selective_ack(&self) -> Option<SelectiveAck<'a>> {
        match self.selective_ack {
            Some(bytes) => Some(SelectiveAck { bytes }),
            None => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Extensions<'a> {
    next_kind: u8,
    remaining: &'a [u8],
    remaining_count: u8,
}

impl<'a> Iterator for Extensions<'a> {
    type Item = Extension<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining_count == 0 {
            return None;
        }
        let kind = self.next_kind;
        self.next_kind = self.remaining[0];
        let length = usize::from(self.remaining[1]);
        let bytes = &self.remaining[2..2 + length];
        self.remaining = &self.remaining[2 + length..];
        self.remaining_count -= 1;
        Some(Extension { kind, bytes })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = usize::from(self.remaining_count);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for Extensions<'_> {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectiveAck<'a> {
    bytes: &'a [u8],
}

impl<'a> SelectiveAck<'a> {
    #[must_use]
    pub const fn as_bytes(self) -> &'a [u8] {
        self.bytes
    }

    /// Test an offset from the packet's cumulative acknowledgement number.
    #[must_use]
    pub fn acknowledges_offset(self, offset: u16) -> bool {
        let Some(bit_index) = usize::from(offset).checked_sub(2) else {
            return false;
        };
        self.bytes
            .get(bit_index / 8)
            .is_some_and(|byte| byte & (1 << (bit_index % 8)) != 0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UtpCodecError {
    PacketTooShort {
        length: usize,
        minimum: usize,
    },
    PacketTooLarge {
        length: usize,
        maximum: usize,
    },
    UnsupportedVersion(u8),
    UnsupportedPacketType(u8),
    InvalidExtensionKind,
    TooManyExtensions {
        count: usize,
        maximum: usize,
    },
    TruncatedExtensionHeader {
        index: usize,
    },
    TruncatedExtensionPayload {
        kind: u8,
        declared: usize,
        available: usize,
    },
    InvalidSackLength(usize),
    DuplicateSack,
    ExtensionPayloadTooLong {
        kind: u8,
        length: usize,
        maximum: usize,
    },
    InvalidPayloadLength {
        packet_type: PacketType,
        length: usize,
    },
}

impl fmt::Display for UtpCodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PacketTooShort { length, minimum } => {
                write!(
                    formatter,
                    "uTP packet length {length} is shorter than {minimum}"
                )
            }
            Self::PacketTooLarge { length, maximum } => {
                write!(formatter, "uTP packet length {length} exceeds {maximum}")
            }
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported uTP version {version}")
            }
            Self::UnsupportedPacketType(packet_type) => {
                write!(formatter, "unsupported uTP packet type {packet_type}")
            }
            Self::InvalidExtensionKind => {
                formatter.write_str("uTP extension kind 0 is the chain terminator")
            }
            Self::TooManyExtensions { count, maximum } => {
                write!(formatter, "uTP extension count {count} exceeds {maximum}")
            }
            Self::TruncatedExtensionHeader { index } => {
                write!(formatter, "uTP extension {index} header is truncated")
            }
            Self::TruncatedExtensionPayload {
                kind,
                declared,
                available,
            } => write!(
                formatter,
                "uTP extension {kind} declares {declared} bytes with {available} available"
            ),
            Self::InvalidSackLength(length) => {
                write!(
                    formatter,
                    "uTP SACK length {length} is not a 4-byte multiple in 4..=252"
                )
            }
            Self::DuplicateSack => formatter.write_str("uTP packet contains more than one SACK"),
            Self::ExtensionPayloadTooLong {
                kind,
                length,
                maximum,
            } => write!(
                formatter,
                "uTP extension {kind} length {length} exceeds {maximum}"
            ),
            Self::InvalidPayloadLength {
                packet_type,
                length,
            } => write!(
                formatter,
                "uTP {packet_type:?} packet has invalid payload length {length}"
            ),
        }
    }
}

impl Error for UtpCodecError {}

pub fn decode_packet(bytes: &[u8]) -> Result<DecodedPacket<'_>, UtpCodecError> {
    if bytes.len() > MAX_UTP_PACKET_SIZE {
        return Err(UtpCodecError::PacketTooLarge {
            length: bytes.len(),
            maximum: MAX_UTP_PACKET_SIZE,
        });
    }
    if bytes.len() < UTP_HEADER_SIZE {
        return Err(UtpCodecError::PacketTooShort {
            length: bytes.len(),
            minimum: UTP_HEADER_SIZE,
        });
    }

    let type_version = bytes[0];
    let version = type_version & 0x0f;
    if version != UTP_VERSION {
        return Err(UtpCodecError::UnsupportedVersion(version));
    }
    let packet_type_wire = type_version >> 4;
    let packet_type = PacketType::from_wire(packet_type_wire)
        .ok_or(UtpCodecError::UnsupportedPacketType(packet_type_wire))?;
    let first_extension = bytes[1];
    let header = UtpHeader {
        packet_type,
        connection_id: read_u16(bytes, 2),
        timestamp: TimestampMicros::new(read_u32(bytes, 4)),
        timestamp_difference_micros: read_u32(bytes, 8),
        window_size: read_u32(bytes, 12),
        sequence_number: SequenceNumber::new(read_u16(bytes, 16)),
        acknowledgement_number: SequenceNumber::new(read_u16(bytes, 18)),
    };

    let mut next_extension = first_extension;
    let mut offset = UTP_HEADER_SIZE;
    let mut extension_count = 0_usize;
    let mut selective_ack = None;
    while next_extension != 0 {
        extension_count += 1;
        if extension_count > MAX_UTP_EXTENSION_COUNT {
            return Err(UtpCodecError::TooManyExtensions {
                count: extension_count,
                maximum: MAX_UTP_EXTENSION_COUNT,
            });
        }
        if bytes.len().saturating_sub(offset) < 2 {
            return Err(UtpCodecError::TruncatedExtensionHeader {
                index: extension_count - 1,
            });
        }
        let kind = next_extension;
        next_extension = bytes[offset];
        let length = usize::from(bytes[offset + 1]);
        offset += 2;
        let available = bytes.len().saturating_sub(offset);
        if available < length {
            return Err(UtpCodecError::TruncatedExtensionPayload {
                kind,
                declared: length,
                available,
            });
        }
        let extension_payload = &bytes[offset..offset + length];
        if kind == SACK_EXTENSION {
            validate_sack(length)?;
            if selective_ack.replace(extension_payload).is_some() {
                return Err(UtpCodecError::DuplicateSack);
            }
        }
        offset += length;
    }

    let payload = &bytes[offset..];
    validate_payload(packet_type, payload.len())?;
    Ok(DecodedPacket {
        header,
        first_extension,
        extension_bytes: &bytes[UTP_HEADER_SIZE..offset],
        extension_count: extension_count as u8,
        selective_ack,
        payload,
    })
}

pub fn encode_packet(packet: PacketToEncode<'_>) -> Result<Vec<u8>, UtpCodecError> {
    validate_extensions_for_encode(packet.extensions)?;
    validate_payload(packet.header.packet_type, packet.payload.len())?;
    let extension_size = packet
        .extensions
        .iter()
        .map(|extension| 2 + extension.bytes.len())
        .sum::<usize>();
    let length = UTP_HEADER_SIZE
        .checked_add(extension_size)
        .and_then(|size| size.checked_add(packet.payload.len()))
        .ok_or(UtpCodecError::PacketTooLarge {
            length: usize::MAX,
            maximum: MAX_UTP_PACKET_SIZE,
        })?;
    if length > MAX_UTP_PACKET_SIZE {
        return Err(UtpCodecError::PacketTooLarge {
            length,
            maximum: MAX_UTP_PACKET_SIZE,
        });
    }

    let mut bytes = Vec::with_capacity(length);
    bytes.push(((packet.header.packet_type as u8) << 4) | UTP_VERSION);
    bytes.push(
        packet
            .extensions
            .first()
            .map_or(0, |extension| extension.kind),
    );
    bytes.extend_from_slice(&packet.header.connection_id.to_be_bytes());
    bytes.extend_from_slice(&packet.header.timestamp.get().to_be_bytes());
    bytes.extend_from_slice(&packet.header.timestamp_difference_micros.to_be_bytes());
    bytes.extend_from_slice(&packet.header.window_size.to_be_bytes());
    bytes.extend_from_slice(&packet.header.sequence_number.get().to_be_bytes());
    bytes.extend_from_slice(&packet.header.acknowledgement_number.get().to_be_bytes());
    for (index, extension) in packet.extensions.iter().enumerate() {
        let next = packet.extensions.get(index + 1).map_or(0, |next| next.kind);
        bytes.push(next);
        bytes.push(extension.bytes.len() as u8);
        bytes.extend_from_slice(extension.bytes);
    }
    bytes.extend_from_slice(packet.payload);
    Ok(bytes)
}

fn validate_extensions_for_encode(
    extensions: &[ExtensionToEncode<'_>],
) -> Result<(), UtpCodecError> {
    if extensions.len() > MAX_UTP_EXTENSION_COUNT {
        return Err(UtpCodecError::TooManyExtensions {
            count: extensions.len(),
            maximum: MAX_UTP_EXTENSION_COUNT,
        });
    }
    let mut saw_sack = false;
    for extension in extensions {
        if extension.kind == 0 {
            return Err(UtpCodecError::InvalidExtensionKind);
        }
        if extension.bytes.len() > usize::from(u8::MAX) {
            return Err(UtpCodecError::ExtensionPayloadTooLong {
                kind: extension.kind,
                length: extension.bytes.len(),
                maximum: usize::from(u8::MAX),
            });
        }
        if extension.kind == SACK_EXTENSION {
            validate_sack(extension.bytes.len())?;
            if saw_sack {
                return Err(UtpCodecError::DuplicateSack);
            }
            saw_sack = true;
        }
    }
    Ok(())
}

fn validate_sack(length: usize) -> Result<(), UtpCodecError> {
    if !(4..=MAX_SACK_BYTES).contains(&length) || !length.is_multiple_of(4) {
        return Err(UtpCodecError::InvalidSackLength(length));
    }
    Ok(())
}

fn validate_payload(packet_type: PacketType, length: usize) -> Result<(), UtpCodecError> {
    let valid = match packet_type {
        PacketType::Data => length > 0,
        PacketType::Fin => true,
        PacketType::State | PacketType::Reset | PacketType::Syn => length == 0,
    };
    if !valid {
        return Err(UtpCodecError::InvalidPayloadLength {
            packet_type,
            length,
        });
    }
    Ok(())
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes(
        bytes[offset..offset + 2]
            .try_into()
            .expect("uTP header length was validated"),
    )
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("uTP header length was validated"),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        DecodedPacket, ExtensionToEncode, MAX_UTP_EXTENSION_COUNT, MAX_UTP_PACKET_SIZE,
        PacketToEncode, PacketType, SACK_EXTENSION, UtpCodecError, UtpHeader, decode_packet,
        encode_packet,
    };
    use crate::utp::{SequenceNumber, TimestampMicros};

    fn header(packet_type: PacketType) -> UtpHeader {
        UtpHeader {
            packet_type,
            connection_id: 0x1234,
            timestamp: TimestampMicros::new(0x0102_0304),
            timestamp_difference_micros: 0x0506_0708,
            window_size: 0x0d0e_0f10,
            sequence_number: SequenceNumber::new(0x1112),
            acknowledgement_number: SequenceNumber::new(0x1314),
        }
    }

    fn encode(
        packet_type: PacketType,
        extensions: &[ExtensionToEncode<'_>],
        payload: &[u8],
    ) -> Result<Vec<u8>, UtpCodecError> {
        encode_packet(PacketToEncode {
            header: header(packet_type),
            extensions,
            payload,
        })
    }

    #[test]
    fn exact_header_bytes_round_trip() {
        let bytes = encode(PacketType::Data, &[], &[0xaa, 0xbb]).expect("encode");
        assert_eq!(
            bytes,
            [
                0x01, 0x00, 0x12, 0x34, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x0d, 0x0e,
                0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0xaa, 0xbb,
            ]
        );
        let decoded = decode_packet(&bytes).expect("decode");
        assert_eq!(decoded.header, header(PacketType::Data));
        assert_eq!(decoded.payload(), [0xaa, 0xbb]);
        assert_eq!(decoded.extension_count(), 0);
    }

    #[test]
    fn every_packet_type_uses_the_high_nibble() {
        for (packet_type, expected) in [
            (PacketType::Data, 0x01),
            (PacketType::Fin, 0x11),
            (PacketType::State, 0x21),
            (PacketType::Reset, 0x31),
            (PacketType::Syn, 0x41),
        ] {
            let payload: &[u8] = if packet_type == PacketType::Data {
                &[1]
            } else {
                &[]
            };
            let bytes = encode(packet_type, &[], payload).expect("encode type");
            assert_eq!(bytes[0], expected);
            assert_eq!(
                decode_packet(&bytes)
                    .expect("decode type")
                    .header
                    .packet_type,
                packet_type
            );
        }
    }

    #[test]
    fn known_and_unknown_extensions_preserve_chain_order() {
        let sack = [0x05, 0x80, 0, 0];
        let unknown = [9, 8, 7];
        let bytes = encode(
            PacketType::State,
            &[
                ExtensionToEncode {
                    kind: 42,
                    bytes: &unknown,
                },
                ExtensionToEncode {
                    kind: SACK_EXTENSION,
                    bytes: &sack,
                },
            ],
            &[],
        )
        .expect("encode extensions");
        let decoded = decode_packet(&bytes).expect("decode extensions");
        assert_eq!(decoded.extension_count(), 2);
        assert_eq!(
            decoded.extensions().collect::<Vec<_>>(),
            [
                super::Extension {
                    kind: 42,
                    bytes: &unknown,
                },
                super::Extension {
                    kind: SACK_EXTENSION,
                    bytes: &sack,
                },
            ]
        );
        let selective_ack = decoded.selective_ack().expect("SACK");
        assert!(selective_ack.acknowledges_offset(2));
        assert!(!selective_ack.acknowledges_offset(3));
        assert!(selective_ack.acknowledges_offset(4));
        assert!(selective_ack.acknowledges_offset(17));
        assert!(!selective_ack.acknowledges_offset(1));
        assert!(!selective_ack.acknowledges_offset(34));
    }

    #[test]
    fn every_truncated_prefix_is_rejected() {
        let bytes = encode(
            PacketType::State,
            &[ExtensionToEncode {
                kind: SACK_EXTENSION,
                bytes: &[1, 2, 3, 4],
            }],
            &[],
        )
        .expect("encode fixture");
        for length in 0..bytes.len() {
            assert!(
                decode_packet(&bytes[..length]).is_err(),
                "prefix {length} unexpectedly decoded"
            );
        }
    }

    #[test]
    fn malformed_type_version_and_payload_are_rejected() {
        let mut bytes = encode(PacketType::State, &[], &[]).expect("state");
        bytes[0] = 0x22;
        assert_eq!(
            decode_packet(&bytes),
            Err(UtpCodecError::UnsupportedVersion(2))
        );
        bytes[0] = 0xf1;
        assert_eq!(
            decode_packet(&bytes),
            Err(UtpCodecError::UnsupportedPacketType(15))
        );
        assert!(matches!(
            encode(PacketType::Data, &[], &[]),
            Err(UtpCodecError::InvalidPayloadLength {
                packet_type: PacketType::Data,
                length: 0
            })
        ));
        assert!(matches!(
            encode(PacketType::Syn, &[], &[1]),
            Err(UtpCodecError::InvalidPayloadLength {
                packet_type: PacketType::Syn,
                length: 1
            })
        ));
        assert!(encode(PacketType::Fin, &[], &[1]).is_ok());
    }

    #[test]
    fn extension_limits_and_sack_shape_are_enforced_on_both_paths() {
        let unknown = ExtensionToEncode {
            kind: 2,
            bytes: &[],
        };
        let too_many = vec![unknown; MAX_UTP_EXTENSION_COUNT + 1];
        assert!(matches!(
            encode(PacketType::State, &too_many, &[]),
            Err(UtpCodecError::TooManyExtensions { .. })
        ));
        assert_eq!(
            encode(
                PacketType::State,
                &[ExtensionToEncode {
                    kind: 0,
                    bytes: &[]
                }],
                &[]
            ),
            Err(UtpCodecError::InvalidExtensionKind)
        );
        for length in [0, 1, 3, 5, 253, 255] {
            let sack = vec![0; length];
            assert!(matches!(
                encode(
                    PacketType::State,
                    &[ExtensionToEncode {
                        kind: SACK_EXTENSION,
                        bytes: &sack
                    }],
                    &[]
                ),
                Err(UtpCodecError::InvalidSackLength(actual)) if actual == length
            ));
        }
        let sack = [0; 4];
        assert_eq!(
            encode(
                PacketType::State,
                &[
                    ExtensionToEncode {
                        kind: SACK_EXTENSION,
                        bytes: &sack
                    },
                    ExtensionToEncode {
                        kind: SACK_EXTENSION,
                        bytes: &sack
                    }
                ],
                &[]
            ),
            Err(UtpCodecError::DuplicateSack)
        );

        let mut chained = encode(PacketType::State, &[], &[]).expect("base packet");
        chained[1] = 2;
        for index in 0..=MAX_UTP_EXTENSION_COUNT {
            chained.push(if index == MAX_UTP_EXTENSION_COUNT {
                0
            } else {
                2
            });
            chained.push(0);
        }
        assert!(matches!(
            decode_packet(&chained),
            Err(UtpCodecError::TooManyExtensions { .. })
        ));
    }

    #[test]
    fn truncated_extension_header_and_payload_report_bounds() {
        let mut header_only = encode(PacketType::State, &[], &[]).expect("base packet");
        header_only[1] = 7;
        assert_eq!(
            decode_packet(&header_only),
            Err(UtpCodecError::TruncatedExtensionHeader { index: 0 })
        );
        header_only.extend_from_slice(&[0, 4, 1, 2]);
        assert_eq!(
            decode_packet(&header_only),
            Err(UtpCodecError::TruncatedExtensionPayload {
                kind: 7,
                declared: 4,
                available: 2,
            })
        );
    }

    #[test]
    fn packet_size_and_extension_payload_are_bounded() {
        let oversized = vec![0; MAX_UTP_PACKET_SIZE + 1];
        assert!(matches!(
            decode_packet(&oversized),
            Err(UtpCodecError::PacketTooLarge { .. })
        ));
        let extension = vec![0; usize::from(u8::MAX) + 1];
        assert!(matches!(
            encode(
                PacketType::State,
                &[ExtensionToEncode {
                    kind: 2,
                    bytes: &extension
                }],
                &[]
            ),
            Err(UtpCodecError::ExtensionPayloadTooLong { .. })
        ));
        let payload = vec![0; MAX_UTP_PACKET_SIZE];
        assert!(matches!(
            encode(PacketType::Data, &[], &payload),
            Err(UtpCodecError::PacketTooLarge { .. })
        ));
    }

    #[test]
    fn decoded_packet_borrows_payload_and_extensions() {
        fn assert_borrowed<'a>(bytes: &'a [u8]) -> DecodedPacket<'a> {
            decode_packet(bytes).expect("decode borrowed packet")
        }

        let bytes = encode(PacketType::Data, &[], &[1, 2, 3]).expect("packet");
        assert_eq!(assert_borrowed(&bytes).payload(), [1, 2, 3]);
    }
}
