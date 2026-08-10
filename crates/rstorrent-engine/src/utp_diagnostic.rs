//! Controlled plaintext uTP transfer used only by the interoperability gate.

use std::error::Error;
use std::fmt;

use rstorrent_protocol::metainfo::Metainfo;
use rstorrent_protocol::peer_wire::{BlockRequest, MAX_REQUEST_BLOCK_LENGTH, PeerMessage};
use sha1::{Digest, Sha1};

use crate::network::NetworkConfig;
use crate::peer_socket::handshake_over_utp;
use crate::utp_runtime::UtpStream;

pub const MAX_CONTROLLED_UTP_DOWNLOAD_BYTES: u64 = 2 * 1024 * 1024 + 731;
pub const MAX_CONTROLLED_UTP_CHOKE_RETRIES: u8 = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControlledUtpDownloadReport {
    pub bytes: u64,
    pub pieces: usize,
    pub requests: u64,
    pub choke_retries: u8,
    pub peer_id: [u8; 20],
}

#[derive(Debug)]
pub enum ControlledUtpDownloadError {
    PayloadTooLarge {
        maximum: u64,
        actual: u64,
    },
    Peer(String),
    Choked {
        maximum_retries: u8,
    },
    Rejected(BlockRequest),
    UnexpectedBlock {
        expected: BlockRequest,
        index: u32,
        begin: u32,
        length: usize,
    },
    PieceHash {
        index: u32,
    },
    Length {
        expected: u64,
        actual: usize,
    },
}

impl fmt::Display for ControlledUtpDownloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PayloadTooLarge { maximum, actual } => write!(
                formatter,
                "controlled uTP payload length {actual} exceeds {maximum} bytes"
            ),
            Self::Peer(detail) => write!(formatter, "controlled uTP peer: {detail}"),
            Self::Choked { maximum_retries } => write!(
                formatter,
                "controlled uTP peer exceeded {maximum_retries} transient choke retries"
            ),
            Self::Rejected(request) => write!(
                formatter,
                "controlled uTP peer rejected piece {} block {}+{}",
                request.index, request.begin, request.length
            ),
            Self::UnexpectedBlock {
                expected,
                index,
                begin,
                length,
            } => write!(
                formatter,
                "controlled uTP peer returned piece {index} block {begin}+{length}; expected {} block {}+{}",
                expected.index, expected.begin, expected.length
            ),
            Self::PieceHash { index } => {
                write!(formatter, "controlled uTP piece {index} failed SHA-1")
            }
            Self::Length { expected, actual } => write!(
                formatter,
                "controlled uTP payload length {actual} differs from {expected}"
            ),
        }
    }
}

impl Error for ControlledUtpDownloadError {}

pub async fn download_controlled_utp(
    stream: UtpStream,
    metainfo: &Metainfo,
    network: NetworkConfig,
) -> Result<(Vec<u8>, ControlledUtpDownloadReport), ControlledUtpDownloadError> {
    if metainfo.total_length > MAX_CONTROLLED_UTP_DOWNLOAD_BYTES {
        return Err(ControlledUtpDownloadError::PayloadTooLarge {
            maximum: MAX_CONTROLLED_UTP_DOWNLOAD_BYTES,
            actual: metainfo.total_length,
        });
    }
    let (mut peer, handshake) = handshake_over_utp(stream, metainfo.info_hash, false, network)
        .await
        .map_err(|error| ControlledUtpDownloadError::Peer(error.to_string()))?;
    peer.send_message(&PeerMessage::Interested)
        .await
        .map_err(|error| ControlledUtpDownloadError::Peer(error.to_string()))?;
    wait_for_unchoke(&mut peer).await?;

    let capacity = usize::try_from(metainfo.total_length)
        .expect("controlled uTP payload limit fits every supported target");
    let mut payload = Vec::with_capacity(capacity);
    let mut requests = 0_u64;
    let mut choke_retries = 0_u8;
    for (piece_index, expected_hash) in metainfo.piece_hashes.iter().enumerate() {
        let index = u32::try_from(piece_index).expect("metainfo piece limit fits u32");
        let piece_length = metainfo
            .piece_length_at(index)
            .expect("metainfo piece index is valid");
        let mut piece = Vec::with_capacity(piece_length as usize);
        let mut begin = 0_u32;
        while begin < piece_length {
            let request = BlockRequest {
                index,
                begin,
                length: (piece_length - begin).min(MAX_REQUEST_BLOCK_LENGTH),
            };
            let block = loop {
                peer.send_message(&PeerMessage::Request(request))
                    .await
                    .map_err(|error| ControlledUtpDownloadError::Peer(error.to_string()))?;
                requests = requests.saturating_add(1);
                match wait_for_block(&mut peer, request).await? {
                    BlockWait::Received(block) => break block,
                    BlockWait::RetryAfterChoke => {
                        record_choke_retry(&mut choke_retries)?;
                    }
                }
            };
            piece.extend(block);
            begin = begin.saturating_add(request.length);
        }
        let actual_hash: [u8; 20] = Sha1::digest(&piece).into();
        if actual_hash != *expected_hash {
            return Err(ControlledUtpDownloadError::PieceHash { index });
        }
        payload.extend(piece);
    }
    if u64::try_from(payload.len()).unwrap_or(u64::MAX) != metainfo.total_length {
        return Err(ControlledUtpDownloadError::Length {
            expected: metainfo.total_length,
            actual: payload.len(),
        });
    }
    peer.send_message(&PeerMessage::NotInterested)
        .await
        .map_err(|error| ControlledUtpDownloadError::Peer(error.to_string()))?;
    Ok((
        payload,
        ControlledUtpDownloadReport {
            bytes: metainfo.total_length,
            pieces: metainfo.piece_count(),
            requests,
            choke_retries,
            peer_id: handshake.peer_id,
        },
    ))
}

async fn wait_for_unchoke(
    peer: &mut crate::peer_io::PeerIo,
) -> Result<(), ControlledUtpDownloadError> {
    loop {
        match peer
            .next_message()
            .await
            .map_err(|error| ControlledUtpDownloadError::Peer(error.to_string()))?
        {
            PeerMessage::Unchoke => return Ok(()),
            PeerMessage::Choke
            | PeerMessage::KeepAlive
            | PeerMessage::Have(_)
            | PeerMessage::Bitfield(_)
            | PeerMessage::HaveAll
            | PeerMessage::HaveNone
            | PeerMessage::SuggestPiece(_)
            | PeerMessage::AllowedFast(_)
            | PeerMessage::Extended { .. } => {}
            message => {
                return Err(ControlledUtpDownloadError::Peer(format!(
                    "unexpected pre-transfer message {message:?}"
                )));
            }
        }
    }
}

enum BlockWait {
    Received(Vec<u8>),
    RetryAfterChoke,
}

async fn wait_for_block(
    peer: &mut crate::peer_io::PeerIo,
    expected: BlockRequest,
) -> Result<BlockWait, ControlledUtpDownloadError> {
    let mut choked = false;
    loop {
        match peer
            .next_message()
            .await
            .map_err(|error| ControlledUtpDownloadError::Peer(error.to_string()))?
        {
            PeerMessage::Piece {
                index,
                begin,
                block,
            } if index == expected.index
                && begin == expected.begin
                && block.len() == expected.length as usize =>
            {
                return Ok(BlockWait::Received(block));
            }
            PeerMessage::Piece {
                index,
                begin,
                block,
            } => {
                return Err(ControlledUtpDownloadError::UnexpectedBlock {
                    expected,
                    index,
                    begin,
                    length: block.len(),
                });
            }
            PeerMessage::RejectRequest(request) if request == expected && !choked => {
                return Err(ControlledUtpDownloadError::Rejected(request));
            }
            PeerMessage::RejectRequest(request) if request == expected => {}
            PeerMessage::Choke => choked = true,
            PeerMessage::Unchoke if choked => return Ok(BlockWait::RetryAfterChoke),
            PeerMessage::KeepAlive
            | PeerMessage::Have(_)
            | PeerMessage::Bitfield(_)
            | PeerMessage::HaveAll
            | PeerMessage::HaveNone
            | PeerMessage::SuggestPiece(_)
            | PeerMessage::AllowedFast(_)
            | PeerMessage::Extended { .. }
            | PeerMessage::Unchoke => {}
            message => {
                return Err(ControlledUtpDownloadError::Peer(format!(
                    "unexpected transfer message {message:?}"
                )));
            }
        }
    }
}

fn record_choke_retry(retries: &mut u8) -> Result<(), ControlledUtpDownloadError> {
    if *retries >= MAX_CONTROLLED_UTP_CHOKE_RETRIES {
        return Err(ControlledUtpDownloadError::Choked {
            maximum_retries: MAX_CONTROLLED_UTP_CHOKE_RETRIES,
        });
    }
    *retries += 1;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_choke_retry_budget_is_exact() {
        let mut retries = 0;
        for expected in 1..=MAX_CONTROLLED_UTP_CHOKE_RETRIES {
            record_choke_retry(&mut retries).expect("bounded transient choke");
            assert_eq!(retries, expected);
        }
        assert!(matches!(
            record_choke_retry(&mut retries),
            Err(ControlledUtpDownloadError::Choked {
                maximum_retries: MAX_CONTROLLED_UTP_CHOKE_RETRIES,
            })
        ));
    }
}
