//! Runtime-independent BEP 15 UDP tracker values and codecs.

use std::error::Error;
use std::fmt;

pub const UDP_TRACKER_PROTOCOL_ID: u64 = 0x0417_2710_1980;
pub const CONNECT_ACTION: u32 = 0;
pub const ANNOUNCE_ACTION: u32 = 1;
pub const ERROR_ACTION: u32 = 3;
pub const CONNECT_REQUEST_LENGTH: usize = 16;
pub const CONNECT_RESPONSE_LENGTH: usize = 16;
pub const ANNOUNCE_REQUEST_LENGTH: usize = 98;
pub const ANNOUNCE_RESPONSE_HEADER_LENGTH: usize = 20;
pub const MAX_COMPACT_PEERS: usize = 200;
pub const MAX_TRACKER_ERROR_LENGTH: usize = 512;
pub const MAX_ANNOUNCE_RESPONSE_LENGTH: usize =
    ANNOUNCE_RESPONSE_HEADER_LENGTH + 18 * MAX_COMPACT_PEERS;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TransactionId(u32);

impl TransactionId {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum AnnounceEvent {
    None = 0,
    Completed = 1,
    Started = 2,
    Stopped = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnnounceRequest {
    pub connection_id: u64,
    pub transaction_id: TransactionId,
    pub info_hash: [u8; 20],
    pub peer_id: [u8; 20],
    pub downloaded: u64,
    pub left: u64,
    pub uploaded: u64,
    pub event: AnnounceEvent,
    pub ip_address: u32,
    pub key: u32,
    pub num_want: i32,
    pub port: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackerAddressFamily {
    Ipv4,
    Ipv6,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnnounceResponse {
    pub interval: u32,
    pub leechers: u32,
    pub seeders: u32,
    pub peers: Vec<CompactPeer>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompactPeer {
    Ipv4 { address: [u8; 4], port: u16 },
    Ipv6 { address: [u8; 16], port: u16 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UdpTrackerError {
    PacketTooShort {
        length: usize,
        minimum: usize,
    },
    UnexpectedTransaction {
        expected: TransactionId,
        actual: TransactionId,
    },
    UnexpectedAction {
        expected: u32,
        actual: u32,
    },
    InvalidPeerStride {
        length: usize,
        stride: usize,
    },
    TooManyPeers {
        count: usize,
        maximum: usize,
    },
    ErrorMessageTooLong {
        length: usize,
        maximum: usize,
    },
    InvalidErrorMessage,
    TrackerFailure(String),
}

impl fmt::Display for UdpTrackerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PacketTooShort { length, minimum } => {
                write!(
                    formatter,
                    "UDP tracker packet length {length} is shorter than {minimum}"
                )
            }
            Self::UnexpectedTransaction { expected, actual } => write!(
                formatter,
                "UDP tracker transaction {} does not match {}",
                actual.get(),
                expected.get()
            ),
            Self::UnexpectedAction { expected, actual } => write!(
                formatter,
                "UDP tracker action {actual} does not match {expected}"
            ),
            Self::InvalidPeerStride { length, stride } => write!(
                formatter,
                "UDP tracker peer payload length {length} is not divisible by {stride}"
            ),
            Self::TooManyPeers { count, maximum } => {
                write!(
                    formatter,
                    "UDP tracker returned {count} peers, exceeding {maximum}"
                )
            }
            Self::ErrorMessageTooLong { length, maximum } => write!(
                formatter,
                "UDP tracker error length {length} exceeds {maximum}"
            ),
            Self::InvalidErrorMessage => {
                write!(formatter, "UDP tracker error message is not UTF-8")
            }
            Self::TrackerFailure(message) => {
                write!(formatter, "UDP tracker rejected the request: {message}")
            }
        }
    }
}

impl Error for UdpTrackerError {}

pub fn encode_connect_request(transaction_id: TransactionId) -> [u8; CONNECT_REQUEST_LENGTH] {
    let mut bytes = [0; CONNECT_REQUEST_LENGTH];
    bytes[0..8].copy_from_slice(&UDP_TRACKER_PROTOCOL_ID.to_be_bytes());
    bytes[8..12].copy_from_slice(&CONNECT_ACTION.to_be_bytes());
    bytes[12..16].copy_from_slice(&transaction_id.get().to_be_bytes());
    bytes
}

pub fn parse_connect_response(
    bytes: &[u8],
    expected_transaction: TransactionId,
) -> Result<u64, UdpTrackerError> {
    let action = parse_response_header(bytes, expected_transaction)?;
    if action != CONNECT_ACTION {
        return Err(UdpTrackerError::UnexpectedAction {
            expected: CONNECT_ACTION,
            actual: action,
        });
    }
    require_length(bytes, CONNECT_RESPONSE_LENGTH)?;
    Ok(u64::from_be_bytes(
        bytes[8..16]
            .try_into()
            .expect("connect response length was validated"),
    ))
}

pub fn encode_announce_request(request: AnnounceRequest) -> [u8; ANNOUNCE_REQUEST_LENGTH] {
    let mut bytes = [0; ANNOUNCE_REQUEST_LENGTH];
    bytes[0..8].copy_from_slice(&request.connection_id.to_be_bytes());
    bytes[8..12].copy_from_slice(&ANNOUNCE_ACTION.to_be_bytes());
    bytes[12..16].copy_from_slice(&request.transaction_id.get().to_be_bytes());
    bytes[16..36].copy_from_slice(&request.info_hash);
    bytes[36..56].copy_from_slice(&request.peer_id);
    bytes[56..64].copy_from_slice(&request.downloaded.to_be_bytes());
    bytes[64..72].copy_from_slice(&request.left.to_be_bytes());
    bytes[72..80].copy_from_slice(&request.uploaded.to_be_bytes());
    bytes[80..84].copy_from_slice(&(request.event as u32).to_be_bytes());
    bytes[84..88].copy_from_slice(&request.ip_address.to_be_bytes());
    bytes[88..92].copy_from_slice(&request.key.to_be_bytes());
    bytes[92..96].copy_from_slice(&request.num_want.to_be_bytes());
    bytes[96..98].copy_from_slice(&request.port.to_be_bytes());
    bytes
}

pub fn parse_announce_response(
    bytes: &[u8],
    expected_transaction: TransactionId,
    family: TrackerAddressFamily,
) -> Result<AnnounceResponse, UdpTrackerError> {
    let action = parse_response_header(bytes, expected_transaction)?;
    if action != ANNOUNCE_ACTION {
        return Err(UdpTrackerError::UnexpectedAction {
            expected: ANNOUNCE_ACTION,
            actual: action,
        });
    }
    require_length(bytes, ANNOUNCE_RESPONSE_HEADER_LENGTH)?;

    let peer_bytes = &bytes[ANNOUNCE_RESPONSE_HEADER_LENGTH..];
    let stride = match family {
        TrackerAddressFamily::Ipv4 => 6,
        TrackerAddressFamily::Ipv6 => 18,
    };
    if !peer_bytes.len().is_multiple_of(stride) {
        return Err(UdpTrackerError::InvalidPeerStride {
            length: peer_bytes.len(),
            stride,
        });
    }
    let peer_count = peer_bytes.len() / stride;
    if peer_count > MAX_COMPACT_PEERS {
        return Err(UdpTrackerError::TooManyPeers {
            count: peer_count,
            maximum: MAX_COMPACT_PEERS,
        });
    }

    let mut peers = Vec::with_capacity(peer_count);
    for peer in peer_bytes.chunks_exact(stride) {
        let endpoint = match family {
            TrackerAddressFamily::Ipv4 => CompactPeer::Ipv4 {
                address: [peer[0], peer[1], peer[2], peer[3]],
                port: u16::from_be_bytes([peer[4], peer[5]]),
            },
            TrackerAddressFamily::Ipv6 => CompactPeer::Ipv6 {
                address: <[u8; 16]>::try_from(&peer[..16])
                    .expect("IPv6 tracker peer stride was validated"),
                port: u16::from_be_bytes([peer[16], peer[17]]),
            },
        };
        peers.push(endpoint);
    }

    Ok(AnnounceResponse {
        interval: read_u32(bytes, 8),
        leechers: read_u32(bytes, 12),
        seeders: read_u32(bytes, 16),
        peers,
    })
}

fn parse_response_header(
    bytes: &[u8],
    expected_transaction: TransactionId,
) -> Result<u32, UdpTrackerError> {
    require_length(bytes, 8)?;
    let action = read_u32(bytes, 0);
    let transaction = TransactionId::new(read_u32(bytes, 4));
    if transaction != expected_transaction {
        return Err(UdpTrackerError::UnexpectedTransaction {
            expected: expected_transaction,
            actual: transaction,
        });
    }
    if action == ERROR_ACTION {
        let message = &bytes[8..];
        if message.len() > MAX_TRACKER_ERROR_LENGTH {
            return Err(UdpTrackerError::ErrorMessageTooLong {
                length: message.len(),
                maximum: MAX_TRACKER_ERROR_LENGTH,
            });
        }
        let message =
            std::str::from_utf8(message).map_err(|_| UdpTrackerError::InvalidErrorMessage)?;
        return Err(UdpTrackerError::TrackerFailure(message.to_owned()));
    }
    Ok(action)
}

fn require_length(bytes: &[u8], minimum: usize) -> Result<(), UdpTrackerError> {
    if bytes.len() < minimum {
        return Err(UdpTrackerError::PacketTooShort {
            length: bytes.len(),
            minimum,
        });
    }
    Ok(())
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("caller validated UDP tracker field length"),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        ANNOUNCE_ACTION, ANNOUNCE_REQUEST_LENGTH, AnnounceEvent, AnnounceRequest, CONNECT_ACTION,
        CONNECT_REQUEST_LENGTH, CompactPeer, ERROR_ACTION, MAX_COMPACT_PEERS,
        MAX_TRACKER_ERROR_LENGTH, TrackerAddressFamily, TransactionId, UDP_TRACKER_PROTOCOL_ID,
        UdpTrackerError, encode_announce_request, encode_connect_request, parse_announce_response,
        parse_connect_response,
    };

    #[test]
    fn encodes_exact_connect_and_announce_requests() {
        let connect_transaction = TransactionId::new(0x1020_3040);
        let connect = encode_connect_request(connect_transaction);
        assert_eq!(connect.len(), CONNECT_REQUEST_LENGTH);
        assert_eq!(&connect[0..8], &UDP_TRACKER_PROTOCOL_ID.to_be_bytes());
        assert_eq!(&connect[8..12], &CONNECT_ACTION.to_be_bytes());
        assert_eq!(&connect[12..16], &0x1020_3040_u32.to_be_bytes());

        let announce = encode_announce_request(AnnounceRequest {
            connection_id: 0x0102_0304_0506_0708,
            transaction_id: TransactionId::new(0x90a0_b0c0),
            info_hash: [0x11; 20],
            peer_id: [0x22; 20],
            downloaded: 0x0102_0304_0506_0708,
            left: 0x1112_1314_1516_1718,
            uploaded: u64::MAX,
            event: AnnounceEvent::Started,
            ip_address: 0,
            key: 0xaabb_ccdd,
            num_want: 200,
            port: 0x1ae1,
        });
        assert_eq!(announce.len(), ANNOUNCE_REQUEST_LENGTH);
        assert_eq!(&announce[0..8], &0x0102_0304_0506_0708_u64.to_be_bytes());
        assert_eq!(&announce[8..12], &ANNOUNCE_ACTION.to_be_bytes());
        assert_eq!(&announce[12..16], &0x90a0_b0c0_u32.to_be_bytes());
        assert_eq!(&announce[16..36], &[0x11; 20]);
        assert_eq!(&announce[36..56], &[0x22; 20]);
        assert_eq!(&announce[56..64], &0x0102_0304_0506_0708_u64.to_be_bytes());
        assert_eq!(&announce[64..72], &0x1112_1314_1516_1718_u64.to_be_bytes());
        assert_eq!(&announce[72..80], &u64::MAX.to_be_bytes());
        assert_eq!(&announce[80..84], &2_u32.to_be_bytes());
        assert_eq!(&announce[88..92], &0xaabb_ccdd_u32.to_be_bytes());
        assert_eq!(&announce[92..96], &200_i32.to_be_bytes());
        assert_eq!(&announce[96..98], &0x1ae1_u16.to_be_bytes());
    }

    #[test]
    fn parses_connect_extensions_and_correlated_tracker_errors() {
        let transaction = TransactionId::new(7);
        let mut response = Vec::new();
        response.extend_from_slice(&CONNECT_ACTION.to_be_bytes());
        response.extend_from_slice(&transaction.get().to_be_bytes());
        response.extend_from_slice(&0x0102_0304_0506_0708_u64.to_be_bytes());
        response.extend_from_slice(b"future");
        assert_eq!(
            parse_connect_response(&response, transaction),
            Ok(0x0102_0304_0506_0708)
        );

        let mut error = Vec::new();
        error.extend_from_slice(&ERROR_ACTION.to_be_bytes());
        error.extend_from_slice(&transaction.get().to_be_bytes());
        error.extend_from_slice(b"denied");
        assert_eq!(
            parse_connect_response(&error, transaction),
            Err(UdpTrackerError::TrackerFailure("denied".to_owned()))
        );

        error.truncate(8);
        error.extend(std::iter::repeat_n(b'x', MAX_TRACKER_ERROR_LENGTH + 1));
        assert!(matches!(
            parse_connect_response(&error, transaction),
            Err(UdpTrackerError::ErrorMessageTooLong { .. })
        ));
        error.truncate(8);
        error.push(0xff);
        assert_eq!(
            parse_connect_response(&error, transaction),
            Err(UdpTrackerError::InvalidErrorMessage)
        );
    }

    #[test]
    fn validates_response_headers_transactions_actions_and_strides() {
        let transaction = TransactionId::new(9);
        for length in 0..8 {
            assert!(matches!(
                parse_connect_response(&vec![0; length], transaction),
                Err(UdpTrackerError::PacketTooShort { .. })
            ));
        }
        for length in 8..16 {
            let mut packet = vec![0; length];
            packet[0..4].copy_from_slice(&CONNECT_ACTION.to_be_bytes());
            packet[4..8].copy_from_slice(&transaction.get().to_be_bytes());
            assert_eq!(
                parse_connect_response(&packet, transaction),
                Err(UdpTrackerError::PacketTooShort {
                    length,
                    minimum: 16
                })
            );
        }

        let mut connect = [0; 16];
        connect[0..4].copy_from_slice(&CONNECT_ACTION.to_be_bytes());
        connect[4..8].copy_from_slice(&10_u32.to_be_bytes());
        assert!(matches!(
            parse_connect_response(&connect, transaction),
            Err(UdpTrackerError::UnexpectedTransaction { .. })
        ));
        connect[4..8].copy_from_slice(&transaction.get().to_be_bytes());
        connect[0..4].copy_from_slice(&ANNOUNCE_ACTION.to_be_bytes());
        assert_eq!(
            parse_connect_response(&connect, transaction),
            Err(UdpTrackerError::UnexpectedAction {
                expected: CONNECT_ACTION,
                actual: ANNOUNCE_ACTION
            })
        );

        for length in 8..20 {
            let mut packet = vec![0; length];
            packet[0..4].copy_from_slice(&ANNOUNCE_ACTION.to_be_bytes());
            packet[4..8].copy_from_slice(&transaction.get().to_be_bytes());
            assert_eq!(
                parse_announce_response(&packet, transaction, TrackerAddressFamily::Ipv4),
                Err(UdpTrackerError::PacketTooShort {
                    length,
                    minimum: 20
                })
            );
        }
        let mut announce = vec![0; 21];
        announce[0..4].copy_from_slice(&ANNOUNCE_ACTION.to_be_bytes());
        announce[4..8].copy_from_slice(&transaction.get().to_be_bytes());
        assert_eq!(
            parse_announce_response(&announce, transaction, TrackerAddressFamily::Ipv4),
            Err(UdpTrackerError::InvalidPeerStride {
                length: 1,
                stride: 6
            })
        );
    }

    #[test]
    fn parses_bounded_ipv4_and_ipv6_compact_peers() {
        let transaction = TransactionId::new(11);
        let mut ipv4 = response_header(transaction);
        ipv4.extend_from_slice(&[127, 0, 0, 1, 0x1a, 0xe1]);
        assert_eq!(
            parse_announce_response(&ipv4, transaction, TrackerAddressFamily::Ipv4)
                .expect("IPv4 response")
                .peers,
            [CompactPeer::Ipv4 {
                address: [127, 0, 0, 1],
                port: 6881
            }]
        );

        let mut ipv6 = response_header(transaction);
        ipv6.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        ipv6.extend_from_slice(&6882_u16.to_be_bytes());
        assert_eq!(
            parse_announce_response(&ipv6, transaction, TrackerAddressFamily::Ipv6)
                .expect("IPv6 response")
                .peers,
            [CompactPeer::Ipv6 {
                address: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
                port: 6882
            }]
        );

        let mut maximum = response_header(transaction);
        maximum.extend(std::iter::repeat_n(0, 6 * MAX_COMPACT_PEERS));
        assert_eq!(
            parse_announce_response(&maximum, transaction, TrackerAddressFamily::Ipv4)
                .expect("maximum response")
                .peers
                .len(),
            MAX_COMPACT_PEERS
        );
        maximum.extend_from_slice(&[0; 6]);
        assert!(matches!(
            parse_announce_response(&maximum, transaction, TrackerAddressFamily::Ipv4),
            Err(UdpTrackerError::TooManyPeers { .. })
        ));
    }

    fn response_header(transaction: TransactionId) -> Vec<u8> {
        let mut response = Vec::new();
        response.extend_from_slice(&ANNOUNCE_ACTION.to_be_bytes());
        response.extend_from_slice(&transaction.get().to_be_bytes());
        response.extend_from_slice(&1800_u32.to_be_bytes());
        response.extend_from_slice(&2_u32.to_be_bytes());
        response.extend_from_slice(&3_u32.to_be_bytes());
        response
    }
}
