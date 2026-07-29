use std::error::Error;
use std::fmt;

use sha1::{Digest, Sha1};

use crate::metainfo::MAX_PIECE_LENGTH;
use crate::peer_wire::{BlockRequest, MAX_REQUEST_BLOCK_LENGTH, PeerMessage};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DownloadAction {
    SendInterested,
    Request(BlockRequest),
    Verified(VerifiedPiece),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedPiece {
    pub index: u32,
    pub hash: [u8; 20],
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PieceError {
    InvalidPieceLength {
        length: u32,
        maximum: u32,
    },
    InvalidBitfieldLength {
        actual: usize,
    },
    InvalidBitfieldPadding,
    UnexpectedPieceIndex {
        expected: u32,
        actual: u32,
    },
    BlockOutOfRange {
        begin: u32,
    },
    OverlappingBlock {
        begin: u32,
    },
    InvalidBlockLength {
        begin: u32,
        expected: u32,
        actual: usize,
    },
    UnexpectedBlock {
        begin: u32,
    },
    DuplicateBlock {
        begin: u32,
    },
    UnexpectedMessage(&'static str),
    HashMismatch {
        expected: [u8; 20],
        actual: [u8; 20],
    },
}

impl fmt::Display for PieceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPieceLength { length, maximum } => {
                write!(formatter, "piece length {length} is outside 1..={maximum}")
            }
            Self::InvalidBitfieldLength { actual } => {
                write!(
                    formatter,
                    "single-piece bitfield has invalid length {actual}"
                )
            }
            Self::InvalidBitfieldPadding => {
                write!(formatter, "single-piece bitfield has nonzero padding bits")
            }
            Self::UnexpectedPieceIndex { expected, actual } => write!(
                formatter,
                "received piece index {actual}, expected {expected}"
            ),
            Self::BlockOutOfRange { begin } => {
                write!(
                    formatter,
                    "received block begins outside the piece at {begin}"
                )
            }
            Self::OverlappingBlock { begin } => {
                write!(
                    formatter,
                    "received block begins inside another block at {begin}"
                )
            }
            Self::InvalidBlockLength {
                begin,
                expected,
                actual,
            } => write!(
                formatter,
                "received block at {begin} has length {actual}, expected {expected}"
            ),
            Self::UnexpectedBlock { begin } => {
                write!(formatter, "received unrequested block at {begin}")
            }
            Self::DuplicateBlock { begin } => {
                write!(formatter, "received duplicate block at {begin}")
            }
            Self::UnexpectedMessage(message) => {
                write!(formatter, "received unexpected {message} message")
            }
            Self::HashMismatch { .. } => write!(formatter, "completed piece failed SHA-1"),
        }
    }
}

impl Error for PieceError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BlockStatus {
    Missing,
    Requested,
    Received,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Block {
    begin: u32,
    length: u32,
    status: BlockStatus,
}

#[derive(Debug)]
pub struct OnePieceDownload {
    piece_index: u32,
    expected_hash: [u8; 20],
    bytes: Vec<u8>,
    blocks: Vec<Block>,
    peer_has_piece: bool,
    peer_choking: bool,
    interested: bool,
    verified: bool,
}

impl OnePieceDownload {
    pub fn new(
        piece_index: u32,
        piece_length: u32,
        expected_hash: [u8; 20],
    ) -> Result<Self, PieceError> {
        if piece_length == 0 || piece_length > MAX_PIECE_LENGTH {
            return Err(PieceError::InvalidPieceLength {
                length: piece_length,
                maximum: MAX_PIECE_LENGTH,
            });
        }

        let mut blocks = Vec::new();
        let mut begin = 0;
        while begin < piece_length {
            let length = MAX_REQUEST_BLOCK_LENGTH.min(piece_length - begin);
            blocks.push(Block {
                begin,
                length,
                status: BlockStatus::Missing,
            });
            begin += length;
        }

        Ok(Self {
            piece_index,
            expected_hash,
            bytes: vec![0; piece_length as usize],
            blocks,
            peer_has_piece: false,
            peer_choking: true,
            interested: false,
            verified: false,
        })
    }

    pub fn on_message(&mut self, message: PeerMessage) -> Result<Vec<DownloadAction>, PieceError> {
        match message {
            PeerMessage::KeepAlive => Ok(Vec::new()),
            PeerMessage::Choke => {
                self.peer_choking = true;
                for block in &mut self.blocks {
                    if block.status == BlockStatus::Requested {
                        block.status = BlockStatus::Missing;
                    }
                }
                Ok(Vec::new())
            }
            PeerMessage::Unchoke => {
                self.peer_choking = false;
                Ok(self.availability_actions())
            }
            PeerMessage::Have(index) => {
                self.validate_piece_index(index)?;
                self.peer_has_piece = true;
                Ok(self.availability_actions())
            }
            PeerMessage::Bitfield(bitfield) => {
                if bitfield.len() != 1 {
                    return Err(PieceError::InvalidBitfieldLength {
                        actual: bitfield.len(),
                    });
                }
                if bitfield[0] & 0x7f != 0 {
                    return Err(PieceError::InvalidBitfieldPadding);
                }
                self.peer_has_piece = bitfield[0] & 0x80 != 0;
                Ok(self.availability_actions())
            }
            PeerMessage::Piece {
                index,
                begin,
                block,
            } => self.receive_block(index, begin, block),
            // Remote interest affects uploads only. This diagnostic never seeds.
            PeerMessage::Interested | PeerMessage::NotInterested => Ok(Vec::new()),
            PeerMessage::Request(_) => Err(PieceError::UnexpectedMessage("request")),
        }
    }

    fn availability_actions(&mut self) -> Vec<DownloadAction> {
        if !self.peer_has_piece || self.verified {
            return Vec::new();
        }

        let mut actions = Vec::new();
        if !self.interested {
            self.interested = true;
            actions.push(DownloadAction::SendInterested);
        }
        if self.peer_choking {
            return actions;
        }

        for block in &mut self.blocks {
            if block.status != BlockStatus::Missing {
                continue;
            }
            block.status = BlockStatus::Requested;
            actions.push(DownloadAction::Request(BlockRequest {
                index: self.piece_index,
                begin: block.begin,
                length: block.length,
            }));
        }
        actions
    }

    fn receive_block(
        &mut self,
        index: u32,
        begin: u32,
        bytes: Vec<u8>,
    ) -> Result<Vec<DownloadAction>, PieceError> {
        self.validate_piece_index(index)?;
        let piece_length = self.bytes.len() as u32;
        if begin >= piece_length {
            return Err(PieceError::BlockOutOfRange { begin });
        }

        let Some(block) = self.blocks.iter_mut().find(|block| block.begin == begin) else {
            return Err(PieceError::OverlappingBlock { begin });
        };
        if bytes.len() != block.length as usize {
            return Err(PieceError::InvalidBlockLength {
                begin,
                expected: block.length,
                actual: bytes.len(),
            });
        }
        match block.status {
            BlockStatus::Missing => return Err(PieceError::UnexpectedBlock { begin }),
            BlockStatus::Received => return Err(PieceError::DuplicateBlock { begin }),
            BlockStatus::Requested => {}
        }

        let start = begin as usize;
        let end = start + bytes.len();
        self.bytes[start..end].copy_from_slice(&bytes);
        block.status = BlockStatus::Received;

        if self
            .blocks
            .iter()
            .any(|block| block.status != BlockStatus::Received)
        {
            return Ok(Vec::new());
        }

        let actual_hash: [u8; 20] = Sha1::digest(&self.bytes).into();
        if actual_hash != self.expected_hash {
            return Err(PieceError::HashMismatch {
                expected: self.expected_hash,
                actual: actual_hash,
            });
        }
        self.verified = true;
        Ok(vec![DownloadAction::Verified(VerifiedPiece {
            index: self.piece_index,
            hash: actual_hash,
            bytes: self.bytes.clone(),
        })])
    }

    fn validate_piece_index(&self, index: u32) -> Result<(), PieceError> {
        if index != self.piece_index {
            return Err(PieceError::UnexpectedPieceIndex {
                expected: self.piece_index,
                actual: index,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sha1::{Digest, Sha1};

    use super::{DownloadAction, OnePieceDownload, PieceError};
    use crate::peer_wire::{BlockRequest, PeerMessage};

    fn payload() -> Vec<u8> {
        (0..40_000).map(|offset| (offset % 251) as u8).collect()
    }

    fn hash(bytes: &[u8]) -> [u8; 20] {
        Sha1::digest(bytes).into()
    }

    fn requested_download(expected_hash: [u8; 20]) -> (OnePieceDownload, Vec<BlockRequest>) {
        let mut download =
            OnePieceDownload::new(0, 40_000, expected_hash).expect("valid piece state");
        assert_eq!(
            download
                .on_message(PeerMessage::Bitfield(vec![0x80]))
                .expect("availability"),
            [DownloadAction::SendInterested]
        );
        let requests = download
            .on_message(PeerMessage::Unchoke)
            .expect("unchoke")
            .into_iter()
            .map(|action| match action {
                DownloadAction::Request(request) => request,
                other => panic!("unexpected action {other:?}"),
            })
            .collect();
        (download, requests)
    }

    #[test]
    fn choke_and_availability_gate_requests() {
        let bytes = payload();
        let mut download =
            OnePieceDownload::new(0, bytes.len() as u32, hash(&bytes)).expect("piece state");

        assert!(
            download
                .on_message(PeerMessage::Unchoke)
                .expect("unchoke")
                .is_empty()
        );
        let actions = download
            .on_message(PeerMessage::Bitfield(vec![0x80]))
            .expect("availability");
        assert_eq!(actions.len(), 4);
        assert_eq!(actions[0], DownloadAction::SendInterested);
        assert!(
            actions[1..]
                .iter()
                .all(|action| matches!(action, DownloadAction::Request(_)))
        );

        download
            .on_message(PeerMessage::Choke)
            .expect("choke resets requests");
        let requests = download
            .on_message(PeerMessage::Unchoke)
            .expect("second unchoke");
        assert_eq!(requests.len(), 3);
    }

    #[test]
    fn requests_cover_only_the_selected_piece() {
        let bytes = payload();
        let (_, requests) = requested_download(hash(&bytes));

        assert_eq!(
            requests,
            [
                BlockRequest {
                    index: 0,
                    begin: 0,
                    length: 16_384,
                },
                BlockRequest {
                    index: 0,
                    begin: 16_384,
                    length: 16_384,
                },
                BlockRequest {
                    index: 0,
                    begin: 32_768,
                    length: 7_232,
                },
            ]
        );
    }

    #[test]
    fn assembles_valid_blocks_independent_of_arrival_order() {
        let bytes = payload();
        let expected_hash = hash(&bytes);
        let (mut download, requests) = requested_download(expected_hash);
        let mut final_actions = Vec::new();

        for request in requests.into_iter().rev() {
            let start = request.begin as usize;
            final_actions = download
                .on_message(PeerMessage::Piece {
                    index: request.index,
                    begin: request.begin,
                    block: bytes[start..start + request.length as usize].to_vec(),
                })
                .expect("valid block");
        }

        assert_eq!(final_actions.len(), 1);
        let DownloadAction::Verified(piece) = &final_actions[0] else {
            panic!("expected verified piece");
        };
        assert_eq!(piece.index, 0);
        assert_eq!(piece.hash, expected_hash);
        assert_eq!(piece.bytes, bytes);
    }

    #[test]
    fn rejects_duplicate_overlapping_out_of_range_short_and_unexpected_blocks() {
        let bytes = payload();
        let (mut download, _) = requested_download(hash(&bytes));
        let first = bytes[..16_384].to_vec();
        download
            .on_message(PeerMessage::Piece {
                index: 0,
                begin: 0,
                block: first.clone(),
            })
            .expect("first block");

        assert_eq!(
            download.on_message(PeerMessage::Piece {
                index: 0,
                begin: 0,
                block: first,
            }),
            Err(PieceError::DuplicateBlock { begin: 0 })
        );
        assert_eq!(
            download.on_message(PeerMessage::Piece {
                index: 0,
                begin: 1,
                block: vec![0; 16_384],
            }),
            Err(PieceError::OverlappingBlock { begin: 1 })
        );
        assert_eq!(
            download.on_message(PeerMessage::Piece {
                index: 0,
                begin: 40_000,
                block: vec![0],
            }),
            Err(PieceError::BlockOutOfRange { begin: 40_000 })
        );
        assert_eq!(
            download.on_message(PeerMessage::Piece {
                index: 0,
                begin: 16_384,
                block: vec![0; 16_383],
            }),
            Err(PieceError::InvalidBlockLength {
                begin: 16_384,
                expected: 16_384,
                actual: 16_383,
            })
        );

        let mut unsolicited = OnePieceDownload::new(0, 40_000, hash(&bytes)).expect("piece state");
        assert_eq!(
            unsolicited.on_message(PeerMessage::Piece {
                index: 0,
                begin: 0,
                block: vec![0; 16_384],
            }),
            Err(PieceError::UnexpectedBlock { begin: 0 })
        );
        assert!(matches!(
            download.on_message(PeerMessage::Piece {
                index: 1,
                begin: 16_384,
                block: vec![0; 16_384],
            }),
            Err(PieceError::UnexpectedPieceIndex { .. })
        ));
    }

    #[test]
    fn hash_failure_never_produces_verified_bytes() {
        let bytes = payload();
        let (mut download, requests) = requested_download([0; 20]);
        let mut result = Ok(Vec::new());

        for request in requests {
            let start = request.begin as usize;
            result = download.on_message(PeerMessage::Piece {
                index: request.index,
                begin: request.begin,
                block: bytes[start..start + request.length as usize].to_vec(),
            });
        }

        assert!(matches!(result, Err(PieceError::HashMismatch { .. })));
    }

    #[test]
    fn validates_single_piece_bitfield_shape() {
        let mut download = OnePieceDownload::new(0, 1, hash(&[1])).expect("piece state");
        assert_eq!(
            download.on_message(PeerMessage::Bitfield(vec![0x80, 0])),
            Err(PieceError::InvalidBitfieldLength { actual: 2 })
        );
        assert_eq!(
            download.on_message(PeerMessage::Bitfield(vec![0x81])),
            Err(PieceError::InvalidBitfieldPadding)
        );
    }
}
