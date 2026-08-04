use std::error::Error;
use std::fmt;

use crate::metainfo::MAX_PIECE_LENGTH;
use crate::peer_wire::{BlockRequest, MAX_REQUEST_BLOCK_LENGTH, PeerMessage};
use crate::storage_layout::RequestRange;

pub const MIN_PAYLOAD_ALLOWANCE: usize = MAX_REQUEST_BLOCK_LENGTH as usize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DownloadAction {
    SendInterested,
    Request(BlockRequest),
    StoreBlock(UnverifiedBlock),
    VerifyPiece { index: u32, length: u32 },
    Verified(VerifiedPiece),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnverifiedBlock {
    pub index: u32,
    pub begin: u32,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerifiedPiece {
    pub index: u32,
    pub hash: [u8; 20],
    pub length: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PayloadBudgetSnapshot {
    pub limit: usize,
    pub reserved: usize,
    pub high_water: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PieceError {
    InvalidPieceLength {
        length: u32,
        maximum: u32,
    },
    InvalidPayloadAllowance {
        configured: usize,
        minimum: usize,
    },
    InvalidPieceCount {
        count: usize,
    },
    PieceIndexOutOfRange {
        index: u32,
        piece_count: usize,
    },
    InvalidRequestRange {
        begin: u32,
        length: u32,
    },
    OverlappingRequestRange {
        previous_end: u32,
        begin: u32,
    },
    EmptyRequestPlan,
    InvalidBitfieldLength {
        actual: usize,
        expected: usize,
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
    InvalidBlockTransition {
        begin: u32,
        expected: &'static str,
        actual: &'static str,
    },
    VerificationNotReady,
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
            Self::InvalidPayloadAllowance {
                configured,
                minimum,
            } => write!(
                formatter,
                "payload allowance {configured} is below minimum {minimum}"
            ),
            Self::InvalidPieceCount { count } => {
                write!(formatter, "torrent piece count {count} is invalid")
            }
            Self::PieceIndexOutOfRange { index, piece_count } => {
                write!(formatter, "piece index {index} is outside 0..{piece_count}")
            }
            Self::InvalidRequestRange { begin, length } => {
                write!(
                    formatter,
                    "request range at {begin} has invalid length {length}"
                )
            }
            Self::OverlappingRequestRange {
                previous_end,
                begin,
            } => write!(
                formatter,
                "request range at {begin} overlaps previous end {previous_end}"
            ),
            Self::EmptyRequestPlan => write!(formatter, "wanted piece has no request ranges"),
            Self::InvalidBitfieldLength { actual, expected } => {
                write!(
                    formatter,
                    "torrent bitfield has length {actual}, expected {expected}"
                )
            }
            Self::InvalidBitfieldPadding => {
                write!(formatter, "torrent bitfield has nonzero padding bits")
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
            Self::InvalidBlockTransition {
                begin,
                expected,
                actual,
            } => write!(
                formatter,
                "block at {begin} is {actual}, expected {expected}"
            ),
            Self::VerificationNotReady => {
                write!(
                    formatter,
                    "piece verification completed before it was ready"
                )
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
    Writing,
    Stored,
}

impl BlockStatus {
    fn name(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Requested => "requested",
            Self::Writing => "writing",
            Self::Stored => "stored",
        }
    }
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
    piece_length: u32,
    piece_count: usize,
    expected_hash: [u8; 20],
    blocks: Vec<Block>,
    payload_budget: PayloadBudgetSnapshot,
    peer_has_piece: bool,
    peer_choking: bool,
    interested: bool,
    verification_started: bool,
    verified: bool,
}

impl OnePieceDownload {
    pub fn new(
        piece_index: u32,
        piece_length: u32,
        expected_hash: [u8; 20],
        payload_allowance: usize,
    ) -> Result<Self, PieceError> {
        if piece_length == 0 || piece_length > MAX_PIECE_LENGTH {
            return Err(PieceError::InvalidPieceLength {
                length: piece_length,
                maximum: MAX_PIECE_LENGTH,
            });
        }
        if payload_allowance < MIN_PAYLOAD_ALLOWANCE {
            return Err(PieceError::InvalidPayloadAllowance {
                configured: payload_allowance,
                minimum: MIN_PAYLOAD_ALLOWANCE,
            });
        }
        if piece_index != 0 {
            return Err(PieceError::PieceIndexOutOfRange {
                index: piece_index,
                piece_count: 1,
            });
        }
        let mut request_ranges = Vec::new();
        let mut begin = 0;
        while begin < piece_length {
            let length = MAX_REQUEST_BLOCK_LENGTH.min(piece_length - begin);
            request_ranges.push(RequestRange { begin, length });
            begin += length;
        }
        Self::new_for_torrent(
            piece_index,
            piece_length,
            expected_hash,
            payload_allowance,
            1,
            &request_ranges,
        )
    }

    pub fn new_for_torrent(
        piece_index: u32,
        piece_length: u32,
        expected_hash: [u8; 20],
        payload_allowance: usize,
        piece_count: usize,
        request_ranges: &[RequestRange],
    ) -> Result<Self, PieceError> {
        if piece_length == 0 || piece_length > MAX_PIECE_LENGTH {
            return Err(PieceError::InvalidPieceLength {
                length: piece_length,
                maximum: MAX_PIECE_LENGTH,
            });
        }
        if payload_allowance < MIN_PAYLOAD_ALLOWANCE {
            return Err(PieceError::InvalidPayloadAllowance {
                configured: payload_allowance,
                minimum: MIN_PAYLOAD_ALLOWANCE,
            });
        }
        if piece_count == 0 {
            return Err(PieceError::InvalidPieceCount { count: piece_count });
        }
        if usize::try_from(piece_index).map_or(true, |index| index >= piece_count) {
            return Err(PieceError::PieceIndexOutOfRange {
                index: piece_index,
                piece_count,
            });
        }
        if request_ranges.is_empty() {
            return Err(PieceError::EmptyRequestPlan);
        }

        let mut blocks = Vec::with_capacity(request_ranges.len());
        let mut previous_end = 0;
        for range in request_ranges {
            let end = range.begin.checked_add(range.length);
            if range.length == 0
                || range.length > MAX_REQUEST_BLOCK_LENGTH
                || end.is_none_or(|end| end > piece_length)
            {
                return Err(PieceError::InvalidRequestRange {
                    begin: range.begin,
                    length: range.length,
                });
            }
            if range.begin < previous_end {
                return Err(PieceError::OverlappingRequestRange {
                    previous_end,
                    begin: range.begin,
                });
            }
            blocks.push(Block {
                begin: range.begin,
                length: range.length,
                status: BlockStatus::Missing,
            });
            previous_end = end.expect("validated request range end");
        }

        Ok(Self {
            piece_index,
            piece_length,
            piece_count,
            expected_hash,
            blocks,
            payload_budget: PayloadBudgetSnapshot {
                limit: payload_allowance,
                reserved: 0,
                high_water: 0,
            },
            peer_has_piece: false,
            peer_choking: true,
            interested: false,
            verification_started: false,
            verified: false,
        })
    }

    pub fn on_message(&mut self, message: PeerMessage) -> Result<Vec<DownloadAction>, PieceError> {
        match message {
            PeerMessage::KeepAlive => Ok(Vec::new()),
            PeerMessage::Choke => {
                self.peer_choking = true;
                self.cancel_requested();
                Ok(Vec::new())
            }
            PeerMessage::Unchoke => {
                self.peer_choking = false;
                Ok(self.availability_actions())
            }
            PeerMessage::Have(index) => {
                if usize::try_from(index).map_or(true, |index| index >= self.piece_count) {
                    return Err(PieceError::PieceIndexOutOfRange {
                        index,
                        piece_count: self.piece_count,
                    });
                }
                if index == self.piece_index {
                    self.peer_has_piece = true;
                }
                Ok(self.availability_actions())
            }
            PeerMessage::Bitfield(bitfield) => {
                let expected_length = self.piece_count.div_ceil(8);
                if bitfield.len() != expected_length {
                    return Err(PieceError::InvalidBitfieldLength {
                        actual: bitfield.len(),
                        expected: expected_length,
                    });
                }
                let padding_bits = expected_length * 8 - self.piece_count;
                if padding_bits > 0
                    && bitfield[expected_length - 1] & ((1_u8 << padding_bits) - 1) != 0
                {
                    return Err(PieceError::InvalidBitfieldPadding);
                }
                let byte = self.piece_index as usize / 8;
                let bit = 7 - (self.piece_index as usize % 8);
                self.peer_has_piece = bitfield[byte] & (1 << bit) != 0;
                Ok(self.availability_actions())
            }
            PeerMessage::Piece {
                index,
                begin,
                block,
            } => self.receive_block(index, begin, block),
            // Remote interest affects uploads only. This diagnostic never seeds.
            PeerMessage::Interested | PeerMessage::NotInterested | PeerMessage::Cancel(_) => {
                Ok(Vec::new())
            }
            PeerMessage::Request(_) => Err(PieceError::UnexpectedMessage("request")),
            PeerMessage::Extended { .. } => Err(PieceError::UnexpectedMessage("extended")),
        }
    }

    pub fn on_block_stored(
        &mut self,
        index: u32,
        begin: u32,
    ) -> Result<Vec<DownloadAction>, PieceError> {
        self.validate_piece_index(index)?;
        let block = self.block_mut(begin)?;
        if block.status != BlockStatus::Writing {
            return Err(PieceError::InvalidBlockTransition {
                begin,
                expected: "writing",
                actual: block.status.name(),
            });
        }
        let length = block.length;
        block.status = BlockStatus::Stored;
        self.release(length);

        if self
            .blocks
            .iter()
            .all(|block| block.status == BlockStatus::Stored)
        {
            self.verification_started = true;
            return Ok(vec![DownloadAction::VerifyPiece {
                index: self.piece_index,
                length: self.piece_length,
            }]);
        }
        Ok(self.fill_request_window())
    }

    pub fn on_block_write_failed(&mut self, index: u32, begin: u32) -> Result<(), PieceError> {
        self.validate_piece_index(index)?;
        let block = self.block_mut(begin)?;
        if block.status != BlockStatus::Writing {
            return Err(PieceError::InvalidBlockTransition {
                begin,
                expected: "writing",
                actual: block.status.name(),
            });
        }
        let length = block.length;
        block.status = BlockStatus::Missing;
        self.release(length);
        Ok(())
    }

    pub fn cancel_pending(&mut self) {
        let mut released = 0_u32;
        for block in &mut self.blocks {
            if matches!(block.status, BlockStatus::Requested | BlockStatus::Writing) {
                released += block.length;
                block.status = BlockStatus::Missing;
            }
        }
        self.release(released);
    }

    pub fn finish_verification(
        &mut self,
        index: u32,
        actual_hash: [u8; 20],
    ) -> Result<DownloadAction, PieceError> {
        self.validate_piece_index(index)?;
        if !self.verification_started
            || self
                .blocks
                .iter()
                .any(|block| block.status != BlockStatus::Stored)
        {
            return Err(PieceError::VerificationNotReady);
        }
        if actual_hash != self.expected_hash {
            return Err(PieceError::HashMismatch {
                expected: self.expected_hash,
                actual: actual_hash,
            });
        }
        self.verified = true;
        Ok(DownloadAction::Verified(VerifiedPiece {
            index: self.piece_index,
            hash: actual_hash,
            length: self.piece_length,
        }))
    }

    pub fn payload_budget(&self) -> PayloadBudgetSnapshot {
        self.payload_budget
    }

    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    fn availability_actions(&mut self) -> Vec<DownloadAction> {
        if !self.peer_has_piece || self.verified || self.verification_started {
            return Vec::new();
        }

        let mut actions = Vec::new();
        if !self.interested {
            self.interested = true;
            actions.push(DownloadAction::SendInterested);
        }
        if !self.peer_choking {
            actions.extend(self.fill_request_window());
        }
        actions
    }

    fn fill_request_window(&mut self) -> Vec<DownloadAction> {
        if self.peer_choking || !self.peer_has_piece || self.verified || self.verification_started {
            return Vec::new();
        }

        let mut actions = Vec::new();
        for block in &mut self.blocks {
            if block.status != BlockStatus::Missing {
                continue;
            }
            let length = block.length as usize;
            if self.payload_budget.reserved + length > self.payload_budget.limit {
                break;
            }
            self.payload_budget.reserved += length;
            self.payload_budget.high_water = self
                .payload_budget
                .high_water
                .max(self.payload_budget.reserved);
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
        if begin >= self.piece_length {
            return Err(PieceError::BlockOutOfRange { begin });
        }

        let block = self.block_mut(begin)?;
        if bytes.len() != block.length as usize {
            return Err(PieceError::InvalidBlockLength {
                begin,
                expected: block.length,
                actual: bytes.len(),
            });
        }
        match block.status {
            BlockStatus::Missing => return Err(PieceError::UnexpectedBlock { begin }),
            BlockStatus::Writing | BlockStatus::Stored => {
                return Err(PieceError::DuplicateBlock { begin });
            }
            BlockStatus::Requested => {}
        }
        block.status = BlockStatus::Writing;

        Ok(vec![DownloadAction::StoreBlock(UnverifiedBlock {
            index,
            begin,
            bytes,
        })])
    }

    fn block_mut(&mut self, begin: u32) -> Result<&mut Block, PieceError> {
        if begin >= self.piece_length {
            return Err(PieceError::BlockOutOfRange { begin });
        }
        self.blocks
            .iter_mut()
            .find(|block| block.begin == begin)
            .ok_or(PieceError::OverlappingBlock { begin })
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

    fn cancel_requested(&mut self) {
        let mut released = 0_u32;
        for block in &mut self.blocks {
            if block.status == BlockStatus::Requested {
                released += block.length;
                block.status = BlockStatus::Missing;
            }
        }
        self.release(released);
    }

    fn release(&mut self, length: u32) {
        self.payload_budget.reserved = self
            .payload_budget
            .reserved
            .checked_sub(length as usize)
            .expect("payload reservation accounting underflow");
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DownloadAction, MIN_PAYLOAD_ALLOWANCE, OnePieceDownload, PieceError, UnverifiedBlock,
    };
    use crate::metainfo::MAX_PIECE_LENGTH;
    use crate::peer_wire::{BlockRequest, MAX_REQUEST_BLOCK_LENGTH, PeerMessage};
    use crate::storage_layout::RequestRange;

    const TWO_BLOCK_ALLOWANCE: usize = 2 * MAX_REQUEST_BLOCK_LENGTH as usize;

    fn expected_hash() -> [u8; 20] {
        [7; 20]
    }

    fn requested_download(
        piece_length: u32,
        allowance: usize,
    ) -> (OnePieceDownload, Vec<BlockRequest>) {
        let mut download = OnePieceDownload::new(0, piece_length, expected_hash(), allowance)
            .expect("valid piece state");
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

    fn receive(download: &mut OnePieceDownload, request: BlockRequest) -> UnverifiedBlock {
        let actions = download
            .on_message(PeerMessage::Piece {
                index: request.index,
                begin: request.begin,
                block: vec![request.begin as u8; request.length as usize],
            })
            .expect("requested block");
        let [DownloadAction::StoreBlock(block)] = actions.as_slice() else {
            panic!("expected one storage action, got {actions:?}");
        };
        block.clone()
    }

    #[test]
    fn accepts_libtorrent_maximum_piece_without_resident_payload() {
        let download =
            OnePieceDownload::new(0, MAX_PIECE_LENGTH, expected_hash(), MIN_PAYLOAD_ALLOWANCE)
                .expect("accepted maximum piece");

        assert_eq!(
            download.block_count(),
            (MAX_PIECE_LENGTH as usize).div_ceil(MAX_REQUEST_BLOCK_LENGTH as usize)
        );
        assert_eq!(download.payload_budget().reserved, 0);
        assert_eq!(download.payload_budget().high_water, 0);
    }

    #[test]
    fn rejects_piece_and_allowance_outside_limits() {
        assert!(matches!(
            OnePieceDownload::new(
                0,
                MAX_PIECE_LENGTH + 1,
                expected_hash(),
                MIN_PAYLOAD_ALLOWANCE
            ),
            Err(PieceError::InvalidPieceLength { .. })
        ));
        assert_eq!(
            OnePieceDownload::new(0, 1, expected_hash(), MIN_PAYLOAD_ALLOWANCE - 1)
                .expect_err("small allowance"),
            PieceError::InvalidPayloadAllowance {
                configured: MIN_PAYLOAD_ALLOWANCE - 1,
                minimum: MIN_PAYLOAD_ALLOWANCE,
            }
        );
    }

    #[test]
    fn slow_storage_holds_request_window_at_allowance() {
        let piece_length = 3 * MAX_REQUEST_BLOCK_LENGTH;
        let (mut download, requests) = requested_download(piece_length, TWO_BLOCK_ALLOWANCE);
        assert_eq!(requests.len(), 2);
        assert_eq!(download.payload_budget().reserved, TWO_BLOCK_ALLOWANCE);
        assert_eq!(download.payload_budget().high_water, TWO_BLOCK_ALLOWANCE);

        let first = receive(&mut download, requests[0]);
        let second = receive(&mut download, requests[1]);
        assert_eq!(download.payload_budget().reserved, TWO_BLOCK_ALLOWANCE);

        let refill = download
            .on_block_stored(first.index, first.begin)
            .expect("first write complete");
        assert_eq!(
            refill,
            [DownloadAction::Request(BlockRequest {
                index: 0,
                begin: 2 * MAX_REQUEST_BLOCK_LENGTH,
                length: MAX_REQUEST_BLOCK_LENGTH,
            })]
        );
        assert_eq!(download.payload_budget().reserved, TWO_BLOCK_ALLOWANCE);

        assert!(
            download
                .on_block_stored(second.index, second.begin)
                .expect("second write complete")
                .is_empty()
        );
        assert_eq!(
            download.payload_budget().reserved,
            MAX_REQUEST_BLOCK_LENGTH as usize
        );
    }

    #[test]
    fn choke_cancellation_and_write_failure_release_reservations() {
        let (mut choked, _) = requested_download(2 * MAX_REQUEST_BLOCK_LENGTH, TWO_BLOCK_ALLOWANCE);
        choked.on_message(PeerMessage::Choke).expect("choke");
        assert_eq!(choked.payload_budget().reserved, 0);
        let rerequests = choked.on_message(PeerMessage::Unchoke).expect("re-unchoke");
        assert_eq!(rerequests.len(), 2);

        let (mut cancelled, requests) =
            requested_download(2 * MAX_REQUEST_BLOCK_LENGTH, TWO_BLOCK_ALLOWANCE);
        receive(&mut cancelled, requests[0]);
        cancelled.cancel_pending();
        assert_eq!(cancelled.payload_budget().reserved, 0);

        let (mut failed, requests) =
            requested_download(2 * MAX_REQUEST_BLOCK_LENGTH, TWO_BLOCK_ALLOWANCE);
        let writing = receive(&mut failed, requests[0]);
        failed
            .on_block_write_failed(writing.index, writing.begin)
            .expect("write failure");
        assert_eq!(
            failed.payload_budget().reserved,
            MAX_REQUEST_BLOCK_LENGTH as usize
        );
    }

    #[test]
    fn stored_blocks_can_complete_out_of_order_and_verify() {
        let (mut download, requests) =
            requested_download(2 * MAX_REQUEST_BLOCK_LENGTH, TWO_BLOCK_ALLOWANCE);
        let first = receive(&mut download, requests[0]);
        let second = receive(&mut download, requests[1]);

        assert!(
            download
                .on_block_stored(second.index, second.begin)
                .expect("second stored")
                .is_empty()
        );
        assert_eq!(
            download
                .on_block_stored(first.index, first.begin)
                .expect("first stored"),
            [DownloadAction::VerifyPiece {
                index: 0,
                length: 2 * MAX_REQUEST_BLOCK_LENGTH,
            }]
        );
        assert_eq!(download.payload_budget().reserved, 0);

        assert_eq!(
            download
                .finish_verification(0, expected_hash())
                .expect("matching hash"),
            DownloadAction::Verified(super::VerifiedPiece {
                index: 0,
                hash: expected_hash(),
                length: 2 * MAX_REQUEST_BLOCK_LENGTH,
            })
        );
    }

    #[test]
    fn hash_failure_and_early_verification_are_typed() {
        let (mut early, _) = requested_download(MAX_REQUEST_BLOCK_LENGTH, MIN_PAYLOAD_ALLOWANCE);
        assert_eq!(
            early.finish_verification(0, expected_hash()),
            Err(PieceError::VerificationNotReady)
        );

        let (mut failed, requests) =
            requested_download(MAX_REQUEST_BLOCK_LENGTH, MIN_PAYLOAD_ALLOWANCE);
        let block = receive(&mut failed, requests[0]);
        failed
            .on_block_stored(block.index, block.begin)
            .expect("stored");
        assert!(matches!(
            failed.finish_verification(0, [9; 20]),
            Err(PieceError::HashMismatch { .. })
        ));
    }

    #[test]
    fn rejects_duplicate_overlapping_out_of_range_short_and_unexpected_blocks() {
        let (mut download, requests) =
            requested_download(2 * MAX_REQUEST_BLOCK_LENGTH, TWO_BLOCK_ALLOWANCE);
        receive(&mut download, requests[0]);

        assert_eq!(
            download.on_message(PeerMessage::Piece {
                index: 0,
                begin: requests[0].begin,
                block: vec![0; requests[0].length as usize],
            }),
            Err(PieceError::DuplicateBlock {
                begin: requests[0].begin
            })
        );
        assert_eq!(
            download.on_message(PeerMessage::Piece {
                index: 0,
                begin: 1,
                block: vec![0; MAX_REQUEST_BLOCK_LENGTH as usize],
            }),
            Err(PieceError::OverlappingBlock { begin: 1 })
        );
        assert_eq!(
            download.on_message(PeerMessage::Piece {
                index: 0,
                begin: 2 * MAX_REQUEST_BLOCK_LENGTH,
                block: vec![0],
            }),
            Err(PieceError::BlockOutOfRange {
                begin: 2 * MAX_REQUEST_BLOCK_LENGTH
            })
        );
        assert_eq!(
            download.on_message(PeerMessage::Piece {
                index: 0,
                begin: requests[1].begin,
                block: vec![0; requests[1].length as usize - 1],
            }),
            Err(PieceError::InvalidBlockLength {
                begin: requests[1].begin,
                expected: requests[1].length,
                actual: requests[1].length as usize - 1,
            })
        );

        let mut unsolicited = OnePieceDownload::new(
            0,
            MAX_REQUEST_BLOCK_LENGTH,
            expected_hash(),
            MIN_PAYLOAD_ALLOWANCE,
        )
        .expect("piece state");
        assert_eq!(
            unsolicited.on_message(PeerMessage::Piece {
                index: 0,
                begin: 0,
                block: vec![0; MAX_REQUEST_BLOCK_LENGTH as usize],
            }),
            Err(PieceError::UnexpectedBlock { begin: 0 })
        );
        assert!(matches!(
            download.on_message(PeerMessage::Piece {
                index: 1,
                begin: requests[1].begin,
                block: vec![0; requests[1].length as usize],
            }),
            Err(PieceError::UnexpectedPieceIndex { .. })
        ));
    }

    #[test]
    fn validates_single_piece_bitfield_shape() {
        let mut download = OnePieceDownload::new(0, 1, expected_hash(), MIN_PAYLOAD_ALLOWANCE)
            .expect("piece state");
        assert_eq!(
            download.on_message(PeerMessage::Bitfield(vec![0x80, 0])),
            Err(PieceError::InvalidBitfieldLength {
                actual: 2,
                expected: 1,
            })
        );
        assert_eq!(
            download.on_message(PeerMessage::Bitfield(vec![0x81])),
            Err(PieceError::InvalidBitfieldPadding)
        );
    }

    #[test]
    fn handles_torrent_bitfield_unrelated_have_and_padding_gap() {
        let ranges = [
            RequestRange {
                begin: 0,
                length: MAX_REQUEST_BLOCK_LENGTH,
            },
            RequestRange {
                begin: MAX_REQUEST_BLOCK_LENGTH,
                length: 13_080,
            },
        ];
        let mut download = OnePieceDownload::new_for_torrent(
            2,
            32_768,
            expected_hash(),
            TWO_BLOCK_ALLOWANCE,
            5,
            &ranges,
        )
        .expect("multi-piece state");

        assert!(
            download
                .on_message(PeerMessage::Have(1))
                .expect("unrelated valid have")
                .is_empty()
        );
        assert_eq!(
            download.on_message(PeerMessage::Have(5)),
            Err(PieceError::PieceIndexOutOfRange {
                index: 5,
                piece_count: 5,
            })
        );
        assert_eq!(
            download
                .on_message(PeerMessage::Bitfield(vec![0x20]))
                .expect("target availability"),
            [DownloadAction::SendInterested]
        );
        assert_eq!(
            download.on_message(PeerMessage::Unchoke).expect("requests"),
            [
                DownloadAction::Request(BlockRequest {
                    index: 2,
                    begin: 0,
                    length: MAX_REQUEST_BLOCK_LENGTH,
                }),
                DownloadAction::Request(BlockRequest {
                    index: 2,
                    begin: MAX_REQUEST_BLOCK_LENGTH,
                    length: 13_080,
                }),
            ]
        );
        assert_eq!(download.block_count(), 2);

        let mut bad_padding = OnePieceDownload::new_for_torrent(
            2,
            32_768,
            expected_hash(),
            TWO_BLOCK_ALLOWANCE,
            5,
            &ranges,
        )
        .expect("state");
        assert_eq!(
            bad_padding.on_message(PeerMessage::Bitfield(vec![0x21])),
            Err(PieceError::InvalidBitfieldPadding)
        );
    }

    #[test]
    fn rejects_invalid_piece_count_and_request_plans() {
        let valid = [RequestRange {
            begin: 0,
            length: 1,
        }];
        assert_eq!(
            OnePieceDownload::new_for_torrent(
                0,
                1,
                expected_hash(),
                MIN_PAYLOAD_ALLOWANCE,
                0,
                &valid,
            )
            .expect_err("zero pieces"),
            PieceError::InvalidPieceCount { count: 0 }
        );
        assert_eq!(
            OnePieceDownload::new_for_torrent(
                1,
                1,
                expected_hash(),
                MIN_PAYLOAD_ALLOWANCE,
                1,
                &valid,
            )
            .expect_err("index outside torrent"),
            PieceError::PieceIndexOutOfRange {
                index: 1,
                piece_count: 1,
            }
        );
        assert_eq!(
            OnePieceDownload::new_for_torrent(
                0,
                1,
                expected_hash(),
                MIN_PAYLOAD_ALLOWANCE,
                1,
                &[],
            )
            .expect_err("empty plan"),
            PieceError::EmptyRequestPlan
        );
        let overlap = [
            RequestRange {
                begin: 0,
                length: 2,
            },
            RequestRange {
                begin: 1,
                length: 1,
            },
        ];
        assert_eq!(
            OnePieceDownload::new_for_torrent(
                0,
                3,
                expected_hash(),
                MIN_PAYLOAD_ALLOWANCE,
                1,
                &overlap,
            )
            .expect_err("overlap"),
            PieceError::OverlappingRequestRange {
                previous_end: 2,
                begin: 1,
            }
        );
    }
}
