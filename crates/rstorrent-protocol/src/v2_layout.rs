//! Format-aware, runtime-free BEP 52 file and logical-piece geometry.

use std::error::Error;
use std::fmt;
use std::ops::Range;

use crate::merkle::{MAX_BEP52_PIECE_LENGTH, MIN_BEP52_PIECE_LENGTH};
use crate::metainfo::MAX_METAINFO_PIECES;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V2FileGeometry {
    file_index: usize,
    length: u64,
    logical_offset: u64,
    start_piece: u32,
    piece_count: u32,
    alignment_gap_before: u64,
}

impl V2FileGeometry {
    pub const fn file_index(self) -> usize {
        self.file_index
    }

    pub const fn length(self) -> u64 {
        self.length
    }

    pub const fn logical_offset(self) -> u64 {
        self.logical_offset
    }

    pub const fn start_piece(self) -> u32 {
        self.start_piece
    }

    pub const fn piece_count(self) -> u32 {
        self.piece_count
    }

    pub const fn alignment_gap_before(self) -> u64 {
        self.alignment_gap_before
    }

    pub fn piece_range(self) -> Range<u32> {
        self.start_piece..self.start_piece + self.piece_count
    }

    fn end_piece(self) -> u32 {
        self.start_piece + self.piece_count
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V2PieceGeometry {
    pub piece_index: u32,
    pub file_index: usize,
    pub local_piece: u32,
    pub logical_offset: u64,
    pub file_offset: u64,
    pub payload_length: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V2AlignmentGap {
    pub before_file_index: usize,
    pub logical_offset: u64,
    pub length: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct V2TorrentLayout {
    piece_length: u32,
    payload_length: u64,
    logical_length: u64,
    piece_count: usize,
    files: Vec<V2FileGeometry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum V2LayoutError {
    InvalidPieceLength {
        length: u32,
    },
    EmptyPayload,
    TooManyPieces {
        actual: usize,
        maximum: usize,
    },
    InvalidFileIndex {
        index: usize,
        file_count: usize,
    },
    InvalidPieceIndex {
        index: u32,
        piece_count: usize,
    },
    EmptyFileRange,
    FileRangeOutOfBounds {
        file_index: usize,
        offset: u64,
        length: u64,
        file_length: u64,
    },
    ArithmeticOverflow,
}

impl fmt::Display for V2LayoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPieceLength { length } => {
                write!(formatter, "invalid BEP 52 piece length {length}")
            }
            Self::EmptyPayload => formatter.write_str("BEP 52 layout has no payload bytes"),
            Self::TooManyPieces { actual, maximum } => {
                write!(
                    formatter,
                    "BEP 52 layout has {actual} pieces, limit {maximum}"
                )
            }
            Self::InvalidFileIndex { index, file_count } => {
                write!(formatter, "file index {index} is outside 0..{file_count}")
            }
            Self::InvalidPieceIndex { index, piece_count } => {
                write!(formatter, "piece index {index} is outside 0..{piece_count}")
            }
            Self::EmptyFileRange => formatter.write_str("BEP 52 file range is empty"),
            Self::FileRangeOutOfBounds {
                file_index,
                offset,
                length,
                file_length,
            } => write!(
                formatter,
                "file {file_index} range {offset}+{length} exceeds length {file_length}"
            ),
            Self::ArithmeticOverflow => formatter.write_str("BEP 52 layout arithmetic overflow"),
        }
    }
}

impl Error for V2LayoutError {}

impl V2TorrentLayout {
    pub fn new(piece_length: u32, file_lengths: &[u64]) -> Result<Self, V2LayoutError> {
        Self::new_with_piece_limit(piece_length, file_lengths, MAX_METAINFO_PIECES)
    }

    pub fn new_with_piece_limit(
        piece_length: u32,
        file_lengths: &[u64],
        max_pieces: usize,
    ) -> Result<Self, V2LayoutError> {
        if !(MIN_BEP52_PIECE_LENGTH..=MAX_BEP52_PIECE_LENGTH).contains(&piece_length)
            || !piece_length.is_power_of_two()
        {
            return Err(V2LayoutError::InvalidPieceLength {
                length: piece_length,
            });
        }

        let piece_length_u64 = u64::from(piece_length);
        let mut files = Vec::with_capacity(file_lengths.len());
        let mut payload_length = 0_u64;
        let mut cursor = 0_u64;
        let mut piece_count = 0_usize;
        for (file_index, &length) in file_lengths.iter().enumerate() {
            payload_length = payload_length
                .checked_add(length)
                .ok_or(V2LayoutError::ArithmeticOverflow)?;
            if length == 0 {
                files.push(V2FileGeometry {
                    file_index,
                    length,
                    logical_offset: cursor,
                    start_piece: u32::try_from(piece_count)
                        .map_err(|_| V2LayoutError::ArithmeticOverflow)?,
                    piece_count: 0,
                    alignment_gap_before: 0,
                });
                continue;
            }

            let aligned = align_up(cursor, piece_length_u64)?;
            let gap = aligned
                .checked_sub(cursor)
                .ok_or(V2LayoutError::ArithmeticOverflow)?;
            let local_pieces_u64 = length.div_ceil(piece_length_u64);
            let local_pieces =
                usize::try_from(local_pieces_u64).map_err(|_| V2LayoutError::ArithmeticOverflow)?;
            let next_piece_count = piece_count
                .checked_add(local_pieces)
                .ok_or(V2LayoutError::ArithmeticOverflow)?;
            if next_piece_count > max_pieces {
                return Err(V2LayoutError::TooManyPieces {
                    actual: next_piece_count,
                    maximum: max_pieces,
                });
            }
            let start_piece =
                u32::try_from(piece_count).map_err(|_| V2LayoutError::ArithmeticOverflow)?;
            let local_pieces_u32 =
                u32::try_from(local_pieces).map_err(|_| V2LayoutError::ArithmeticOverflow)?;
            files.push(V2FileGeometry {
                file_index,
                length,
                logical_offset: aligned,
                start_piece,
                piece_count: local_pieces_u32,
                alignment_gap_before: gap,
            });
            cursor = aligned
                .checked_add(length)
                .ok_or(V2LayoutError::ArithmeticOverflow)?;
            piece_count = next_piece_count;
        }
        if payload_length == 0 {
            return Err(V2LayoutError::EmptyPayload);
        }

        Ok(Self {
            piece_length,
            payload_length,
            logical_length: cursor,
            piece_count,
            files,
        })
    }

    pub const fn piece_length(&self) -> u32 {
        self.piece_length
    }

    pub const fn payload_length(&self) -> u64 {
        self.payload_length
    }

    pub const fn logical_length(&self) -> u64 {
        self.logical_length
    }

    pub const fn piece_count(&self) -> usize {
        self.piece_count
    }

    pub fn files(&self) -> &[V2FileGeometry] {
        &self.files
    }

    pub fn alignment_gaps(&self) -> impl Iterator<Item = V2AlignmentGap> + '_ {
        self.files.iter().filter_map(|file| {
            (file.alignment_gap_before != 0).then_some(V2AlignmentGap {
                before_file_index: file.file_index,
                logical_offset: file.logical_offset - file.alignment_gap_before,
                length: file.alignment_gap_before,
            })
        })
    }

    pub fn piece(&self, index: u32) -> Result<V2PieceGeometry, V2LayoutError> {
        let index_usize = usize::try_from(index).map_err(|_| V2LayoutError::InvalidPieceIndex {
            index,
            piece_count: self.piece_count,
        })?;
        if index_usize >= self.piece_count {
            return Err(V2LayoutError::InvalidPieceIndex {
                index,
                piece_count: self.piece_count,
            });
        }

        let file_position = self.files.partition_point(|file| file.end_piece() <= index);
        let file = self
            .files
            .get(file_position)
            .filter(|file| file.piece_count != 0)
            .ok_or(V2LayoutError::ArithmeticOverflow)?;
        let local_piece = index
            .checked_sub(file.start_piece)
            .ok_or(V2LayoutError::ArithmeticOverflow)?;
        let file_offset = u64::from(local_piece)
            .checked_mul(u64::from(self.piece_length))
            .ok_or(V2LayoutError::ArithmeticOverflow)?;
        let remaining = file
            .length
            .checked_sub(file_offset)
            .ok_or(V2LayoutError::ArithmeticOverflow)?;
        let payload_length = u32::try_from(remaining.min(u64::from(self.piece_length)))
            .map_err(|_| V2LayoutError::ArithmeticOverflow)?;
        let logical_offset = file
            .logical_offset
            .checked_add(file_offset)
            .ok_or(V2LayoutError::ArithmeticOverflow)?;
        Ok(V2PieceGeometry {
            piece_index: index,
            file_index: file.file_index,
            local_piece,
            logical_offset,
            file_offset,
            payload_length,
        })
    }

    pub fn file_piece_range(&self, file_index: usize) -> Result<Range<u32>, V2LayoutError> {
        self.files
            .get(file_index)
            .copied()
            .map(V2FileGeometry::piece_range)
            .ok_or(V2LayoutError::InvalidFileIndex {
                index: file_index,
                file_count: self.files.len(),
            })
    }

    pub fn file_range_to_pieces(
        &self,
        file_index: usize,
        offset: u64,
        length: u64,
    ) -> Result<Range<u32>, V2LayoutError> {
        if length == 0 {
            return Err(V2LayoutError::EmptyFileRange);
        }
        let file = self
            .files
            .get(file_index)
            .ok_or(V2LayoutError::InvalidFileIndex {
                index: file_index,
                file_count: self.files.len(),
            })?;
        let end = offset
            .checked_add(length)
            .ok_or(V2LayoutError::ArithmeticOverflow)?;
        if end > file.length {
            return Err(V2LayoutError::FileRangeOutOfBounds {
                file_index,
                offset,
                length,
                file_length: file.length,
            });
        }
        let first = offset / u64::from(self.piece_length);
        let last = (end - 1) / u64::from(self.piece_length);
        let first = file
            .start_piece
            .checked_add(u32::try_from(first).map_err(|_| V2LayoutError::ArithmeticOverflow)?)
            .ok_or(V2LayoutError::ArithmeticOverflow)?;
        let end = file
            .start_piece
            .checked_add(
                u32::try_from(last)
                    .map_err(|_| V2LayoutError::ArithmeticOverflow)?
                    .checked_add(1)
                    .ok_or(V2LayoutError::ArithmeticOverflow)?,
            )
            .ok_or(V2LayoutError::ArithmeticOverflow)?;
        Ok(first..end)
    }
}

fn align_up(value: u64, alignment: u64) -> Result<u64, V2LayoutError> {
    let remainder = value % alignment;
    if remainder == 0 {
        Ok(value)
    } else {
        value
            .checked_add(alignment - remainder)
            .ok_or(V2LayoutError::ArithmeticOverflow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligns_nonempty_files_and_keeps_empty_files_out_of_piece_space() {
        let layout = V2TorrentLayout::new(16 * 1024, &[0, 1, 0, 16_383, 16_384, 16_385, 0])
            .expect("valid layout");
        assert_eq!(layout.piece_count(), 5);
        assert_eq!(layout.payload_length(), 49_153);
        assert_eq!(layout.logical_length(), 65_537);
        assert_eq!(layout.file_piece_range(0), Ok(0..0));
        assert_eq!(layout.file_piece_range(1), Ok(0..1));
        assert_eq!(layout.file_piece_range(2), Ok(1..1));
        assert_eq!(layout.file_piece_range(3), Ok(1..2));
        assert_eq!(layout.file_piece_range(4), Ok(2..3));
        assert_eq!(layout.file_piece_range(5), Ok(3..5));
        assert_eq!(layout.file_piece_range(6), Ok(5..5));

        assert_eq!(
            layout.alignment_gaps().collect::<Vec<_>>(),
            [
                V2AlignmentGap {
                    before_file_index: 3,
                    logical_offset: 1,
                    length: 16_383,
                },
                V2AlignmentGap {
                    before_file_index: 4,
                    logical_offset: 32_767,
                    length: 1,
                },
            ]
        );
    }

    #[test]
    fn maps_global_pieces_and_file_ranges_without_gap_payload() {
        let layout = V2TorrentLayout::new(16 * 1024, &[1, 16_385]).expect("valid layout");
        assert_eq!(
            layout.piece(0),
            Ok(V2PieceGeometry {
                piece_index: 0,
                file_index: 0,
                local_piece: 0,
                logical_offset: 0,
                file_offset: 0,
                payload_length: 1,
            })
        );
        assert_eq!(
            layout.piece(1),
            Ok(V2PieceGeometry {
                piece_index: 1,
                file_index: 1,
                local_piece: 0,
                logical_offset: 16_384,
                file_offset: 0,
                payload_length: 16_384,
            })
        );
        assert_eq!(layout.piece(2).expect("short piece").payload_length, 1);
        assert_eq!(layout.file_range_to_pieces(1, 16_383, 2), Ok(1..3));
        assert!(matches!(
            layout.piece(3),
            Err(V2LayoutError::InvalidPieceIndex { .. })
        ));
    }

    #[test]
    fn exact_boundaries_and_maximum_piece_length_are_valid() {
        for length in [1_u64, 16_383, 16_384, 16_385, 256 * 1024 * 1024] {
            let layout = V2TorrentLayout::new(256 * 1024 * 1024, &[length])
                .expect("valid maximum piece length");
            assert_eq!(layout.piece_count(), 1);
            assert_eq!(
                u64::from(layout.piece(0).expect("piece").payload_length),
                length
            );
        }
    }

    #[test]
    fn limits_and_checked_ranges_fail_without_partial_geometry() {
        assert!(matches!(
            V2TorrentLayout::new(16 * 1024 - 1, &[1]),
            Err(V2LayoutError::InvalidPieceLength { .. })
        ));
        assert!(matches!(
            V2TorrentLayout::new(24 * 1024, &[1]),
            Err(V2LayoutError::InvalidPieceLength { .. })
        ));
        assert_eq!(
            V2TorrentLayout::new(16 * 1024, &[0, 0]),
            Err(V2LayoutError::EmptyPayload)
        );
        assert_eq!(
            V2TorrentLayout::new_with_piece_limit(16 * 1024, &[16_385], 1),
            Err(V2LayoutError::TooManyPieces {
                actual: 2,
                maximum: 1,
            })
        );

        let layout = V2TorrentLayout::new(16 * 1024, &[1]).expect("layout");
        assert_eq!(
            layout.file_range_to_pieces(0, 0, 0),
            Err(V2LayoutError::EmptyFileRange)
        );
        assert!(matches!(
            layout.file_range_to_pieces(0, 1, 1),
            Err(V2LayoutError::FileRangeOutOfBounds { .. })
        ));
        assert_eq!(
            V2TorrentLayout::new_with_piece_limit(16 * 1024, &[u64::MAX, 1], usize::MAX),
            Err(V2LayoutError::ArithmeticOverflow)
        );
    }

    #[test]
    fn geometry_retains_only_one_record_per_file() {
        let lengths = vec![1_u64; 10_000];
        let layout = V2TorrentLayout::new(16 * 1024, &lengths).expect("bounded files");
        assert_eq!(layout.files().len(), lengths.len());
        assert_eq!(layout.piece_count(), lengths.len());
        assert_eq!(
            std::mem::size_of_val(layout.files()),
            lengths.len() * std::mem::size_of::<V2FileGeometry>()
        );
    }
}
