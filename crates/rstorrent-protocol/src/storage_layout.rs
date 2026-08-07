use std::error::Error;
use std::fmt;
use std::ops::RangeInclusive;

use crate::metainfo::{Metainfo, MetainfoFile};
use crate::peer_wire::MAX_REQUEST_BLOCK_LENGTH;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceClass {
    Wanted,
    Boundary,
    Skipped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentTarget {
    WantedFile { file_index: usize, file_offset: u64 },
    SkippedFile { file_index: usize, file_offset: u64 },
    Padding,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutSegment {
    pub piece_offset: u32,
    pub block_offset: usize,
    pub length: usize,
    pub target: SegmentTarget,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileLayoutSegment {
    pub piece_offset: u32,
    pub block_offset: usize,
    pub length: usize,
    pub file_index: usize,
    pub file_offset: u64,
    pub padding: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestRange {
    pub begin: u32,
    pub length: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RequiredPayloadGeometry {
    pub required_payload_bytes: u64,
    pub verified_required_payload_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSelection {
    wanted: Vec<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutError {
    InvalidFileIndex {
        index: usize,
        file_count: usize,
    },
    PaddingSelection {
        index: usize,
    },
    InvalidPieceIndex {
        index: u32,
        piece_count: usize,
    },
    InvalidHaveLength {
        length: usize,
        piece_count: usize,
    },
    EmptyInterval,
    IntervalOutOfRange {
        piece: u32,
        begin: u32,
        length: u32,
        piece_length: u32,
    },
    ArithmeticOverflow,
    LayoutGap {
        torrent_offset: u64,
    },
}

impl fmt::Display for LayoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFileIndex { index, file_count } => {
                write!(formatter, "file index {index} is outside 0..{file_count}")
            }
            Self::PaddingSelection { index } => {
                write!(formatter, "padding file {index} cannot be selected")
            }
            Self::InvalidPieceIndex { index, piece_count } => {
                write!(formatter, "piece index {index} is outside 0..{piece_count}")
            }
            Self::InvalidHaveLength {
                length,
                piece_count,
            } => write!(
                formatter,
                "have state contains {length} pieces but layout contains {piece_count}"
            ),
            Self::EmptyInterval => write!(formatter, "storage interval is empty"),
            Self::IntervalOutOfRange {
                piece,
                begin,
                length,
                piece_length,
            } => write!(
                formatter,
                "piece {piece} interval {begin}+{length} exceeds length {piece_length}"
            ),
            Self::ArithmeticOverflow => write!(formatter, "storage layout arithmetic overflow"),
            Self::LayoutGap { torrent_offset } => {
                write!(
                    formatter,
                    "storage layout does not cover torrent offset {torrent_offset}"
                )
            }
        }
    }
}

impl Error for LayoutError {}

impl FileSelection {
    pub fn new(layout: &TorrentLayout, skipped: &[usize]) -> Result<Self, LayoutError> {
        let mut wanted: Vec<bool> = layout.files.iter().map(|file| !file.padding).collect();
        for &index in skipped {
            let file = layout
                .files
                .get(index)
                .ok_or(LayoutError::InvalidFileIndex {
                    index,
                    file_count: layout.files.len(),
                })?;
            if file.padding {
                return Err(LayoutError::PaddingSelection { index });
            }
            wanted[index] = false;
        }
        Ok(Self { wanted })
    }

    pub fn is_wanted(&self, index: usize) -> bool {
        self.wanted.get(index).copied().unwrap_or(false)
    }

    pub fn set_wanted(
        &mut self,
        layout: &TorrentLayout,
        index: usize,
        wanted: bool,
    ) -> Result<(), LayoutError> {
        let file = layout
            .files
            .get(index)
            .ok_or(LayoutError::InvalidFileIndex {
                index,
                file_count: layout.files.len(),
            })?;
        if file.padding && wanted {
            return Err(LayoutError::PaddingSelection { index });
        }
        self.wanted[index] = wanted;
        Ok(())
    }

    pub fn file_count(&self) -> usize {
        self.wanted.len()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TorrentLayout {
    piece_length: u32,
    total_length: u64,
    piece_count: usize,
    files: Vec<MetainfoFile>,
}

impl TorrentLayout {
    pub fn from_metainfo(metainfo: &Metainfo) -> Self {
        Self {
            piece_length: metainfo.piece_length,
            total_length: metainfo.total_length,
            piece_count: metainfo.piece_count(),
            files: metainfo.files.clone(),
        }
    }

    pub fn files(&self) -> &[MetainfoFile] {
        &self.files
    }

    pub fn piece_count(&self) -> usize {
        self.piece_count
    }

    pub fn piece_length(&self) -> u32 {
        self.piece_length
    }

    pub fn total_length(&self) -> u64 {
        self.total_length
    }

    pub fn piece_length_at(&self, index: u32) -> Result<u32, LayoutError> {
        let piece_index = usize::try_from(index).map_err(|_| LayoutError::InvalidPieceIndex {
            index,
            piece_count: self.piece_count,
        })?;
        if piece_index >= self.piece_count {
            return Err(LayoutError::InvalidPieceIndex {
                index,
                piece_count: self.piece_count,
            });
        }
        let begin = u64::from(index)
            .checked_mul(u64::from(self.piece_length))
            .ok_or(LayoutError::ArithmeticOverflow)?;
        let remaining = self
            .total_length
            .checked_sub(begin)
            .ok_or(LayoutError::ArithmeticOverflow)?;
        u32::try_from(remaining.min(u64::from(self.piece_length)))
            .map_err(|_| LayoutError::ArithmeticOverflow)
    }

    pub fn piece_class(
        &self,
        index: u32,
        selection: &FileSelection,
    ) -> Result<PieceClass, LayoutError> {
        let length = self.piece_length_at(index)?;
        let segments = self.segments(index, 0, length, selection)?;
        let mut wanted = false;
        let mut skipped = false;
        for segment in segments {
            match segment.target {
                SegmentTarget::WantedFile { .. } => wanted = true,
                SegmentTarget::SkippedFile { .. } => skipped = true,
                SegmentTarget::Padding => {}
            }
        }
        Ok(match (wanted, skipped) {
            (true, false) => PieceClass::Wanted,
            (true, true) => PieceClass::Boundary,
            (false, _) => PieceClass::Skipped,
        })
    }

    pub fn request_ranges(
        &self,
        index: u32,
        selection: &FileSelection,
    ) -> Result<Vec<RequestRange>, LayoutError> {
        if self.piece_class(index, selection)? == PieceClass::Skipped {
            return Ok(Vec::new());
        }

        let length = self.piece_length_at(index)?;
        let segments = self.segments(index, 0, length, selection)?;
        let mut data_ranges = Vec::<RequestRange>::new();
        for segment in segments {
            if segment.target == SegmentTarget::Padding {
                continue;
            }
            let begin = segment.piece_offset;
            let segment_length =
                u32::try_from(segment.length).map_err(|_| LayoutError::ArithmeticOverflow)?;
            if let Some(previous) = data_ranges.last_mut()
                && previous.begin + previous.length == begin
            {
                previous.length = previous
                    .length
                    .checked_add(segment_length)
                    .ok_or(LayoutError::ArithmeticOverflow)?;
            } else {
                data_ranges.push(RequestRange {
                    begin,
                    length: segment_length,
                });
            }
        }

        let mut requests = Vec::new();
        for range in data_ranges {
            let mut begin = range.begin;
            let end = range
                .begin
                .checked_add(range.length)
                .ok_or(LayoutError::ArithmeticOverflow)?;
            while begin < end {
                let length = MAX_REQUEST_BLOCK_LENGTH.min(end - begin);
                requests.push(RequestRange { begin, length });
                begin += length;
            }
        }
        Ok(requests)
    }

    pub fn required_payload_geometry(
        &self,
        selection: &FileSelection,
        have: &[bool],
    ) -> Result<RequiredPayloadGeometry, LayoutError> {
        if selection.file_count() != self.files.len() {
            return Err(LayoutError::InvalidFileIndex {
                index: selection.file_count(),
                file_count: self.files.len(),
            });
        }
        if have.len() != self.piece_count {
            return Err(LayoutError::InvalidHaveLength {
                length: have.len(),
                piece_count: self.piece_count,
            });
        }

        let mut required_ranges = Vec::<RangeInclusive<u32>>::new();
        for (file_index, file) in self.files.iter().enumerate() {
            if file.padding || file.length == 0 || !selection.is_wanted(file_index) {
                continue;
            }
            let range = self
                .file_piece_range(file_index)?
                .expect("non-empty file has a piece range");
            let first = *range.start();
            let last = *range.end();
            if let Some(previous) = required_ranges.last_mut()
                && first <= previous.end().saturating_add(1)
            {
                let merged_last = last.max(*previous.end());
                let merged_first = *previous.start();
                *previous = merged_first..=merged_last;
            } else {
                required_ranges.push(range);
            }
        }

        let mut required_payload_bytes = 0_u64;
        let mut verified_required_payload_bytes = 0_u64;
        let mut file_index = 0_usize;
        for piece_index in required_ranges.into_iter().flatten() {
            let piece_start = u64::from(piece_index)
                .checked_mul(u64::from(self.piece_length))
                .ok_or(LayoutError::ArithmeticOverflow)?;
            let piece_end = piece_start
                .checked_add(u64::from(self.piece_length_at(piece_index)?))
                .ok_or(LayoutError::ArithmeticOverflow)?;
            let mut cursor = piece_start;
            let mut piece_payload_bytes = 0_u64;
            while cursor < piece_end {
                let file = self.files.get(file_index).ok_or(LayoutError::LayoutGap {
                    torrent_offset: cursor,
                })?;
                let file_end = file
                    .offset
                    .checked_add(file.length)
                    .ok_or(LayoutError::ArithmeticOverflow)?;
                if file.length == 0 || file_end <= cursor {
                    file_index += 1;
                    continue;
                }
                if cursor < file.offset {
                    return Err(LayoutError::LayoutGap {
                        torrent_offset: cursor,
                    });
                }
                let segment_end = piece_end.min(file_end);
                if !file.padding {
                    piece_payload_bytes = piece_payload_bytes
                        .checked_add(segment_end - cursor)
                        .ok_or(LayoutError::ArithmeticOverflow)?;
                }
                cursor = segment_end;
                if cursor == file_end {
                    file_index += 1;
                }
            }
            required_payload_bytes = required_payload_bytes
                .checked_add(piece_payload_bytes)
                .ok_or(LayoutError::ArithmeticOverflow)?;
            if have
                .get(usize::try_from(piece_index).map_err(|_| LayoutError::ArithmeticOverflow)?)
                .copied()
                .ok_or(LayoutError::InvalidPieceIndex {
                    index: piece_index,
                    piece_count: self.piece_count,
                })?
            {
                verified_required_payload_bytes = verified_required_payload_bytes
                    .checked_add(piece_payload_bytes)
                    .ok_or(LayoutError::ArithmeticOverflow)?;
            }
        }
        Ok(RequiredPayloadGeometry {
            required_payload_bytes,
            verified_required_payload_bytes,
        })
    }

    pub fn segments(
        &self,
        piece: u32,
        begin: u32,
        length: u32,
        selection: &FileSelection,
    ) -> Result<Vec<LayoutSegment>, LayoutError> {
        if length == 0 {
            return Err(LayoutError::EmptyInterval);
        }
        if selection.file_count() != self.files.len() {
            return Err(LayoutError::InvalidFileIndex {
                index: selection.file_count(),
                file_count: self.files.len(),
            });
        }
        let segments = self
            .file_segments(piece, begin, length)?
            .into_iter()
            .map(|segment| {
                let target = if segment.padding {
                    SegmentTarget::Padding
                } else if selection.is_wanted(segment.file_index) {
                    SegmentTarget::WantedFile {
                        file_index: segment.file_index,
                        file_offset: segment.file_offset,
                    }
                } else {
                    SegmentTarget::SkippedFile {
                        file_index: segment.file_index,
                        file_offset: segment.file_offset,
                    }
                };
                LayoutSegment {
                    piece_offset: segment.piece_offset,
                    block_offset: segment.block_offset,
                    length: segment.length,
                    target,
                }
            })
            .collect::<Vec<_>>();
        Ok(segments)
    }

    pub fn file_segments(
        &self,
        piece: u32,
        begin: u32,
        length: u32,
    ) -> Result<Vec<FileLayoutSegment>, LayoutError> {
        if length == 0 {
            return Err(LayoutError::EmptyInterval);
        }
        let actual_piece_length = self.piece_length_at(piece)?;
        let interval_end = begin
            .checked_add(length)
            .ok_or(LayoutError::IntervalOutOfRange {
                piece,
                begin,
                length,
                piece_length: actual_piece_length,
            })?;
        if interval_end > actual_piece_length {
            return Err(LayoutError::IntervalOutOfRange {
                piece,
                begin,
                length,
                piece_length: actual_piece_length,
            });
        }

        let piece_start = u64::from(piece)
            .checked_mul(u64::from(self.piece_length))
            .ok_or(LayoutError::ArithmeticOverflow)?;
        let torrent_start = piece_start
            .checked_add(u64::from(begin))
            .ok_or(LayoutError::ArithmeticOverflow)?;
        let torrent_end = torrent_start
            .checked_add(u64::from(length))
            .ok_or(LayoutError::ArithmeticOverflow)?;

        let mut file_index = self.files.partition_point(|file| {
            file.offset
                .checked_add(file.length)
                .is_some_and(|end| end <= torrent_start)
        });
        let mut cursor = torrent_start;
        let mut segments = Vec::new();
        while cursor < torrent_end {
            let file = self.files.get(file_index).ok_or(LayoutError::LayoutGap {
                torrent_offset: cursor,
            })?;
            let file_end = file
                .offset
                .checked_add(file.length)
                .ok_or(LayoutError::ArithmeticOverflow)?;
            if file.length == 0 || cursor == file_end {
                file_index += 1;
                continue;
            }
            if cursor < file.offset || cursor > file_end {
                return Err(LayoutError::LayoutGap {
                    torrent_offset: cursor,
                });
            }
            let segment_end = torrent_end.min(file_end);
            let segment_length = usize::try_from(segment_end - cursor)
                .map_err(|_| LayoutError::ArithmeticOverflow)?;
            segments.push(FileLayoutSegment {
                piece_offset: u32::try_from(cursor - piece_start)
                    .map_err(|_| LayoutError::ArithmeticOverflow)?,
                block_offset: usize::try_from(cursor - torrent_start)
                    .map_err(|_| LayoutError::ArithmeticOverflow)?,
                length: segment_length,
                file_index,
                file_offset: cursor - file.offset,
                padding: file.padding,
            });
            cursor = segment_end;
            if cursor == file_end {
                file_index += 1;
            }
        }
        Ok(segments)
    }

    pub fn file_pieces(&self, index: usize) -> Result<Vec<u32>, LayoutError> {
        Ok(self
            .file_piece_range(index)?
            .into_iter()
            .flatten()
            .collect())
    }

    pub fn file_piece_range(
        &self,
        index: usize,
    ) -> Result<Option<RangeInclusive<u32>>, LayoutError> {
        let file = self.files.get(index).ok_or(LayoutError::InvalidFileIndex {
            index,
            file_count: self.files.len(),
        })?;
        if file.length == 0 {
            return Ok(None);
        }
        let first = file.offset / u64::from(self.piece_length);
        let final_byte = file
            .offset
            .checked_add(file.length - 1)
            .ok_or(LayoutError::ArithmeticOverflow)?;
        let last = final_byte / u64::from(self.piece_length);
        let first = u32::try_from(first).map_err(|_| LayoutError::ArithmeticOverflow)?;
        let last = u32::try_from(last).map_err(|_| LayoutError::ArithmeticOverflow)?;
        Ok(Some(first..=last))
    }

    pub fn piece_has_skipped_file(
        &self,
        piece: u32,
        selection: &FileSelection,
    ) -> Result<bool, LayoutError> {
        let length = self.piece_length_at(piece)?;
        Ok(self
            .segments(piece, 0, length, selection)?
            .iter()
            .any(|segment| matches!(segment.target, SegmentTarget::SkippedFile { .. })))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FileSelection, LayoutError, PieceClass, RequestRange, RequiredPayloadGeometry,
        SegmentTarget, TorrentLayout,
    };
    use crate::metainfo::{MAX_METAINFO_PIECES, Metainfo, MetainfoFile, MetainfoMode};

    fn fixture() -> (TorrentLayout, FileSelection) {
        let lengths = [20_000, 50_000, 7_000, 18_000, 0, 3_304, 35_000];
        let paths = [
            &["wanted", "start.bin"][..],
            &["skip", "large.bin"][..],
            &["later.bin"][..],
            &["wanted", "end.bin"][..],
            &["wanted", "empty.bin"][..],
            &[".pad", "3304"][..],
            &["tail.bin"][..],
        ];
        let mut offset = 0_u64;
        let files = lengths
            .into_iter()
            .enumerate()
            .map(|(index, length)| {
                let file = MetainfoFile {
                    path: paths[index].iter().map(|part| (*part).to_owned()).collect(),
                    length,
                    offset,
                    padding: index == 5,
                };
                offset += length;
                file
            })
            .collect();
        let metainfo = Metainfo {
            info_hash: [1; 20],
            piece_hashes: vec![[2; 20]; 5],
            piece_length: 32_768,
            total_length: 133_304,
            name: "fixture".to_owned(),
            private: false,
            mode: MetainfoMode::MultiFile,
            files,
        };
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[1, 2]).expect("selection");
        (layout, selection)
    }

    #[test]
    fn classifies_every_nasty_fixture_piece() {
        let (layout, selection) = fixture();
        assert_eq!(
            (0..5)
                .map(|piece| layout.piece_class(piece, &selection))
                .collect::<Result<Vec<_>, _>>()
                .expect("classes"),
            [
                PieceClass::Boundary,
                PieceClass::Skipped,
                PieceClass::Boundary,
                PieceClass::Wanted,
                PieceClass::Wanted,
            ]
        );
        assert_eq!(layout.piece_length_at(4), Ok(2_232));
        assert!(matches!(
            layout.piece_length_at(5),
            Err(LayoutError::InvalidPieceIndex { .. })
        ));
    }

    #[test]
    fn maps_one_request_across_three_real_files() {
        let (layout, selection) = fixture();
        let segments = layout
            .segments(2, 0, 16_384, &selection)
            .expect("three-file request");

        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].piece_offset, 0);
        assert_eq!(segments[0].block_offset, 0);
        assert_eq!(segments[0].length, 4_464);
        assert_eq!(
            segments[0].target,
            SegmentTarget::SkippedFile {
                file_index: 1,
                file_offset: 45_536,
            }
        );
        assert_eq!(segments[1].block_offset, 4_464);
        assert_eq!(segments[1].length, 7_000);
        assert_eq!(
            segments[1].target,
            SegmentTarget::SkippedFile {
                file_index: 2,
                file_offset: 0,
            }
        );
        assert_eq!(segments[2].block_offset, 11_464);
        assert_eq!(segments[2].length, 4_920);
        assert_eq!(
            segments[2].target,
            SegmentTarget::WantedFile {
                file_index: 3,
                file_offset: 0,
            }
        );
    }

    #[test]
    fn plans_cross_file_requests_and_excludes_padding_and_skipped_piece() {
        let (layout, selection) = fixture();
        assert_eq!(
            layout.request_ranges(0, &selection).expect("piece zero"),
            [
                RequestRange {
                    begin: 0,
                    length: 16_384,
                },
                RequestRange {
                    begin: 16_384,
                    length: 16_384,
                },
            ]
        );
        assert!(
            layout
                .request_ranges(1, &selection)
                .expect("skipped piece")
                .is_empty()
        );
        assert_eq!(
            layout.request_ranges(2, &selection).expect("piece two"),
            [
                RequestRange {
                    begin: 0,
                    length: 16_384,
                },
                RequestRange {
                    begin: 16_384,
                    length: 13_080,
                },
            ]
        );
        assert_eq!(
            layout.request_ranges(4, &selection).expect("final piece"),
            [RequestRange {
                begin: 0,
                length: 2_232,
            }]
        );
    }

    #[test]
    fn derives_selection_aware_required_and_verified_payload() {
        let (layout, selection) = fixture();
        assert_eq!(
            layout.required_payload_geometry(&selection, &[true, true, true, false, true],),
            Ok(RequiredPayloadGeometry {
                required_payload_bytes: 97_232,
                verified_required_payload_bytes: 64_464,
            })
        );
    }

    #[test]
    fn required_payload_matches_request_plan_for_every_fixture_selection() {
        let (layout, _) = fixture();
        let selectable = [0_usize, 1, 2, 3, 4, 6];
        let have = [true, false, true, false, true];
        for skipped_mask in 0_u32..(1 << selectable.len()) {
            let skipped = selectable
                .iter()
                .enumerate()
                .filter(|(bit, _)| skipped_mask & (1 << bit) != 0)
                .map(|(_, index)| *index)
                .collect::<Vec<_>>();
            let selection = FileSelection::new(&layout, &skipped).expect("selection");
            let mut required = 0_u64;
            let mut verified = 0_u64;
            for piece_index in 0..layout.piece_count() {
                let piece_index = u32::try_from(piece_index).expect("piece index");
                let piece_bytes = layout
                    .request_ranges(piece_index, &selection)
                    .expect("request ranges")
                    .into_iter()
                    .map(|range| u64::from(range.length))
                    .sum::<u64>();
                required += piece_bytes;
                if have[usize::try_from(piece_index).expect("piece index")] {
                    verified += piece_bytes;
                }
            }
            assert_eq!(
                layout.required_payload_geometry(&selection, &have),
                Ok(RequiredPayloadGeometry {
                    required_payload_bytes: required,
                    verified_required_payload_bytes: verified,
                }),
                "skipped mask {skipped_mask:#08b}"
            );
        }
    }

    #[test]
    fn required_payload_matches_request_plan_across_generated_layouts() {
        let mut random = 0x4d59_5df4_d0f3_3173_u64;
        for case in 0..96_u32 {
            let piece_length = 1 + u32::try_from(next_random(&mut random) % 32).expect("piece");
            let file_count = 1 + usize::try_from(next_random(&mut random) % 7).expect("files");
            let mut lengths = (0..file_count)
                .map(|_| next_random(&mut random) % 49)
                .collect::<Vec<_>>();
            if lengths.iter().all(|length| *length == 0) {
                lengths[file_count - 1] = 1;
            }
            let mut offset = 0_u64;
            let files = lengths
                .into_iter()
                .enumerate()
                .map(|(index, length)| {
                    let padding = length > 0 && next_random(&mut random).is_multiple_of(5);
                    let file = MetainfoFile {
                        path: vec![format!("file-{index}")],
                        length,
                        offset,
                        padding,
                    };
                    offset += length;
                    file
                })
                .collect::<Vec<_>>();
            let piece_count = offset.div_ceil(u64::from(piece_length));
            let metainfo = Metainfo {
                info_hash: [1; 20],
                piece_hashes: vec![[2; 20]; usize::try_from(piece_count).expect("piece count")],
                piece_length,
                total_length: offset,
                name: format!("generated-{case}"),
                private: false,
                mode: MetainfoMode::MultiFile,
                files,
            };
            let layout = TorrentLayout::from_metainfo(&metainfo);
            let have = (0..layout.piece_count())
                .map(|_| next_random(&mut random).is_multiple_of(2))
                .collect::<Vec<_>>();
            let selectable = layout
                .files()
                .iter()
                .enumerate()
                .filter_map(|(index, file)| (!file.padding).then_some(index))
                .collect::<Vec<_>>();
            for skipped_mask in 0_u32..(1 << selectable.len()) {
                let skipped = selectable
                    .iter()
                    .enumerate()
                    .filter(|(bit, _)| skipped_mask & (1 << bit) != 0)
                    .map(|(_, index)| *index)
                    .collect::<Vec<_>>();
                let selection = FileSelection::new(&layout, &skipped).expect("selection");
                let expected = request_plan_geometry(&layout, &selection, &have);
                assert_eq!(
                    layout.required_payload_geometry(&selection, &have),
                    Ok(expected),
                    "generated case {case}, skipped mask {skipped_mask:#09b}"
                );
            }
        }
    }

    #[test]
    #[ignore = "release-mode maximum geometry calibration"]
    fn maximum_required_payload_geometry_stays_structurally_bounded() {
        const MAX_FILES: usize = 4_096;
        const PIECE_LENGTH: u32 = 16 * 1_024;

        let total_length =
            u64::try_from(MAX_METAINFO_PIECES).expect("piece count") * u64::from(PIECE_LENGTH);
        let ordinary_file_length = total_length / u64::try_from(MAX_FILES).expect("file count") + 1;
        let mut offset = 0_u64;
        let files = (0..MAX_FILES)
            .map(|index| {
                let length = if index + 1 == MAX_FILES {
                    total_length - offset
                } else {
                    ordinary_file_length
                };
                let file = MetainfoFile {
                    path: vec![format!("file-{index:04}")],
                    length,
                    offset,
                    padding: index % 8 == 3,
                };
                offset += length;
                file
            })
            .collect::<Vec<_>>();
        let metainfo = Metainfo {
            info_hash: [1; 20],
            piece_hashes: vec![[2; 20]; MAX_METAINFO_PIECES],
            piece_length: PIECE_LENGTH,
            total_length,
            name: "maximum-geometry".to_owned(),
            private: false,
            mode: MetainfoMode::MultiFile,
            files,
        };
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let skipped = layout
            .files()
            .iter()
            .enumerate()
            .filter_map(|(index, file)| (!file.padding && index % 2 == 1).then_some(index))
            .collect::<Vec<_>>();
        let selection = FileSelection::new(&layout, &skipped).expect("fragmented selection");
        let have = (0..MAX_METAINFO_PIECES)
            .map(|piece| piece % 2 == 0)
            .collect::<Vec<_>>();

        let geometry = layout
            .required_payload_geometry(&selection, &have)
            .expect("maximum geometry");

        assert!(geometry.required_payload_bytes > 0);
        assert!(geometry.verified_required_payload_bytes <= geometry.required_payload_bytes);
        assert_eq!(std::mem::size_of::<RequiredPayloadGeometry>(), 16);
        eprintln!(
            "maximum ETA geometry: pieces={MAX_METAINFO_PIECES} files={MAX_FILES} temporary_range_upper_bound_bytes={} retained_geometry_bytes={}",
            MAX_FILES * std::mem::size_of::<std::ops::RangeInclusive<u32>>(),
            std::mem::size_of::<RequiredPayloadGeometry>(),
        );
    }

    fn request_plan_geometry(
        layout: &TorrentLayout,
        selection: &FileSelection,
        have: &[bool],
    ) -> RequiredPayloadGeometry {
        let mut required_payload_bytes = 0_u64;
        let mut verified_required_payload_bytes = 0_u64;
        for (piece_index, verified) in have.iter().copied().enumerate() {
            let piece_payload = layout
                .request_ranges(u32::try_from(piece_index).expect("piece index"), selection)
                .expect("request ranges")
                .into_iter()
                .map(|range| u64::from(range.length))
                .sum::<u64>();
            required_payload_bytes += piece_payload;
            if verified {
                verified_required_payload_bytes += piece_payload;
            }
        }
        RequiredPayloadGeometry {
            required_payload_bytes,
            verified_required_payload_bytes,
        }
    }

    fn next_random(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    #[test]
    fn required_payload_handles_all_skipped_and_rejects_wrong_have_length() {
        let (layout, _) = fixture();
        let selection = FileSelection::new(&layout, &[0, 1, 2, 3, 4, 6]).expect("all skipped");
        assert_eq!(
            layout.required_payload_geometry(&selection, &[false; 5]),
            Ok(RequiredPayloadGeometry::default())
        );
        assert_eq!(
            layout.required_payload_geometry(&selection, &[false; 4]),
            Err(LayoutError::InvalidHaveLength {
                length: 4,
                piece_count: 5,
            })
        );
    }

    #[test]
    fn materialized_file_keeps_boundary_slot_for_other_skipped_file() {
        let (layout, mut selection) = fixture();
        assert_eq!(layout.file_pieces(1), Ok(vec![0, 1, 2]));
        assert_eq!(layout.file_pieces(2), Ok(vec![2]));
        assert!(layout.piece_has_skipped_file(2, &selection).unwrap());

        selection
            .set_wanted(&layout, 2, true)
            .expect("materialize file");
        assert!(layout.piece_has_skipped_file(2, &selection).unwrap());
        assert_eq!(layout.piece_class(2, &selection), Ok(PieceClass::Boundary));
    }

    #[test]
    fn rejects_invalid_selection_and_intervals() {
        let (layout, selection) = fixture();
        assert!(matches!(
            FileSelection::new(&layout, &[5]),
            Err(LayoutError::PaddingSelection { index: 5 })
        ));
        assert!(matches!(
            FileSelection::new(&layout, &[7]),
            Err(LayoutError::InvalidFileIndex { .. })
        ));
        assert_eq!(
            layout.segments(4, 2_000, 233, &selection),
            Err(LayoutError::IntervalOutOfRange {
                piece: 4,
                begin: 2_000,
                length: 233,
                piece_length: 2_232,
            })
        );
        assert_eq!(
            layout.segments(0, 0, 0, &selection),
            Err(LayoutError::EmptyInterval)
        );
    }
}
