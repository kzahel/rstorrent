//! Read-only upload access to verified pieces owned by an active storage pipeline.

use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;

use rstorrent_protocol::content::HybridPaddingMap;
use rstorrent_protocol::peer_wire::BlockRequest;
use rstorrent_protocol::storage_layout::{ContentLayout, FileSelection};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::piece_availability::PieceAvailability;
use crate::selective_storage::{
    MAX_ACTIVE_FILE_READ_BYTES, SelectiveFileReadPlan, SelectiveStorageError,
    SelectiveUploadReadPlan,
};
use crate::streaming::StreamingPieceInterval;

pub(crate) const ACTIVE_UPLOAD_PLAN_CAPACITY: usize = 16;
pub const MAX_STREAMING_AHEAD_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_STREAMING_AHEAD_PIECES: u32 = 16;

pub(crate) struct ActiveUploadPlanRequest {
    pub(crate) request: BlockRequest,
    pub(crate) route_epoch: u64,
    pub(crate) response: oneshot::Sender<Result<SelectiveUploadReadPlan, SelectiveStorageError>>,
}

pub(crate) struct ActiveFilePlanRequest {
    pub(crate) file_index: usize,
    pub(crate) offset: u64,
    pub(crate) length: usize,
    pub(crate) route_epoch: u64,
    pub(crate) response: oneshot::Sender<Result<SelectiveFileReadPlan, SelectiveStorageError>>,
}

#[derive(Clone, Debug)]
pub(crate) struct ActiveUploadFailureSignal {
    inner: Arc<ActiveUploadFailureState>,
}

#[derive(Debug)]
struct ActiveUploadFailureState {
    cancellation: CancellationToken,
    failure: Mutex<Option<ActiveUploadFailure>>,
}

#[derive(Debug)]
struct ActiveUploadFailure {
    piece: u32,
    error: SelectiveStorageError,
}

impl ActiveUploadFailureSignal {
    fn new() -> Self {
        Self {
            inner: Arc::new(ActiveUploadFailureState {
                cancellation: CancellationToken::new(),
                failure: Mutex::new(None),
            }),
        }
    }

    fn report(&self, piece: u32, error: SelectiveStorageError) {
        let mut failure = self
            .inner
            .failure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if failure.is_none() {
            *failure = Some(ActiveUploadFailure { piece, error });
            self.inner.cancellation.cancel();
        }
    }

    pub(crate) async fn cancelled(&self) {
        self.inner.cancellation.cancelled().await;
    }

    pub(crate) fn take_failure(&self) -> Option<(u32, SelectiveStorageError)> {
        self.inner
            .failure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            .map(|failure| (failure.piece, failure.error))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ActiveSeedContent {
    info_hash: [u8; 20],
    private: bool,
    piece_lengths: Arc<[u32]>,
    availability: PieceAvailability,
    planner: Arc<Mutex<mpsc::Sender<ActiveUploadPlanRequest>>>,
    file_planner: Arc<Mutex<Option<mpsc::Sender<ActiveFilePlanRequest>>>>,
    file_access: Arc<Mutex<Option<ActiveContentReader>>>,
    failure: ActiveUploadFailureSignal,
}

#[derive(Clone, Debug)]
struct ActiveFileDescriptor {
    name: String,
    length: u64,
    torrent_offset: u64,
    padding: bool,
}

#[derive(Clone, Debug)]
pub struct ActiveContentReader {
    piece_length: u32,
    files: Arc<[ActiveFileDescriptor]>,
    selected: Arc<Mutex<Vec<bool>>>,
    availability: PieceAvailability,
    planner: Arc<Mutex<Option<mpsc::Sender<ActiveFilePlanRequest>>>>,
    failure: ActiveUploadFailureSignal,
    cancellation: CancellationToken,
}

#[derive(Clone, Debug)]
pub struct ActiveFileReader {
    file_index: usize,
    file: ActiveFileDescriptor,
    piece_length: u32,
    expected_epoch: u64,
    availability: PieceAvailability,
    planner: Arc<Mutex<Option<mpsc::Sender<ActiveFilePlanRequest>>>>,
    failure: ActiveUploadFailureSignal,
    cancellation: CancellationToken,
}

impl ActiveSeedContent {
    pub(crate) fn new(
        info_hash: [u8; 20],
        private: bool,
        piece_lengths: Vec<u32>,
        availability: PieceAvailability,
        planner: mpsc::Sender<ActiveUploadPlanRequest>,
    ) -> Self {
        Self {
            info_hash,
            private,
            piece_lengths: piece_lengths.into(),
            availability,
            planner: Arc::new(Mutex::new(planner)),
            file_planner: Arc::new(Mutex::new(None)),
            file_access: Arc::new(Mutex::new(None)),
            failure: ActiveUploadFailureSignal::new(),
        }
    }

    pub(crate) fn configure_file_access(
        &self,
        name: &str,
        layout: &ContentLayout,
        selection: &FileSelection,
        cancellation: CancellationToken,
        planner: mpsc::Sender<ActiveFilePlanRequest>,
    ) -> ActiveContentReader {
        *self
            .file_planner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(planner);
        let files = layout
            .files()
            .iter()
            .enumerate()
            .map(|(file_index, file)| {
                Ok(ActiveFileDescriptor {
                    name: file.path.last().cloned().unwrap_or_else(|| name.to_owned()),
                    length: file.length,
                    torrent_offset: layout.file_piece_space_offset(file_index)?,
                    padding: file.padding,
                })
            })
            .collect::<Result<Vec<_>, rstorrent_protocol::storage_layout::LayoutError>>()
            .expect("validated content layout has file piece-space offsets");
        let selected = (0..files.len())
            .map(|file_index| selection.is_wanted(file_index))
            .collect();
        let reader = ActiveContentReader {
            piece_length: layout.piece_length(),
            files: files.into(),
            selected: Arc::new(Mutex::new(selected)),
            availability: self.availability.clone(),
            planner: Arc::clone(&self.file_planner),
            failure: self.failure.clone(),
            cancellation,
        };
        *self
            .file_access
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(reader.clone());
        reader
    }

    pub(crate) fn update_file_selection(&self, selection: &FileSelection) {
        if let Some(reader) = self
            .file_access
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
        {
            let mut selected = reader
                .selected
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for (file_index, wanted) in selected.iter_mut().enumerate() {
                *wanted = selection.is_wanted(file_index);
            }
        }
    }

    pub(crate) const fn info_hash(&self) -> [u8; 20] {
        self.info_hash
    }

    pub(crate) const fn is_private(&self) -> bool {
        self.private
    }

    pub(crate) fn piece_lengths(&self) -> Arc<[u32]> {
        self.piece_lengths.clone()
    }

    pub(crate) fn availability(&self) -> PieceAvailability {
        self.availability.clone()
    }

    pub(crate) fn failure_signal(&self) -> ActiveUploadFailureSignal {
        self.failure.clone()
    }

    pub(crate) fn replace_planner(&self, planner: mpsc::Sender<ActiveUploadPlanRequest>) {
        *self
            .planner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = planner;
    }

    pub(crate) fn replace_file_planner(&self, planner: mpsc::Sender<ActiveFilePlanRequest>) {
        *self
            .file_planner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(planner);
    }

    pub(crate) async fn read_block(
        &self,
        request: BlockRequest,
    ) -> Result<Vec<u8>, ActiveSeedContentError> {
        let piece =
            usize::try_from(request.index).map_err(|_| ActiveSeedContentError::Unavailable)?;
        let snapshot = self.availability.snapshot();
        if !snapshot.is_available(piece) {
            return Err(ActiveSeedContentError::Unavailable);
        }
        let (response, completion) = oneshot::channel();
        let planner = self
            .planner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        planner
            .send(ActiveUploadPlanRequest {
                request,
                route_epoch: snapshot.epoch,
                response,
            })
            .await
            .map_err(|_| ActiveSeedContentError::Closed)?;
        let plan = match completion
            .await
            .map_err(|_| ActiveSeedContentError::Closed)?
        {
            Ok(plan) => plan,
            Err(error) => {
                return Err(self.classify_storage_failure(request.index, snapshot.epoch, error));
            }
        };
        let before_read = self.availability.snapshot();
        if before_read.epoch != snapshot.epoch || !before_read.is_available(piece) {
            return Err(ActiveSeedContentError::Unavailable);
        }
        match plan.execute().await {
            Ok(block) => Ok(block),
            Err(error) => Err(self.classify_storage_failure(request.index, snapshot.epoch, error)),
        }
    }

    pub(crate) async fn read_hybrid_v1_block(
        &self,
        request: BlockRequest,
        piece_length: u32,
        padding: &HybridPaddingMap,
    ) -> Result<Vec<u8>, ActiveSeedContentError> {
        let request_end = request
            .begin
            .checked_add(request.length)
            .ok_or(ActiveSeedContentError::Unavailable)?;
        if request.length == 0 || request_end > piece_length {
            return Err(ActiveSeedContentError::Unavailable);
        }
        let padding_begin = padding
            .piece_spans(request.index)
            .map(|span| span.begin)
            .min();
        let Some(padding_begin) = padding_begin else {
            return self.read_block(request).await;
        };
        let mut block = vec![0; request.length as usize];
        let real_end = request_end.min(padding_begin);
        if request.begin < real_end {
            let real_length = real_end - request.begin;
            let real = self
                .read_block(BlockRequest {
                    length: real_length,
                    ..request
                })
                .await?;
            if real.len() != real_length as usize {
                return Err(ActiveSeedContentError::Unavailable);
            }
            block[..real.len()].copy_from_slice(&real);
        }
        Ok(block)
    }

    fn classify_storage_failure(
        &self,
        piece: u32,
        expected_epoch: u64,
        error: SelectiveStorageError,
    ) -> ActiveSeedContentError {
        let piece_index = usize::try_from(piece).ok();
        let current = self.availability.snapshot();
        if current.epoch != expected_epoch
            || !piece_index.is_some_and(|piece| current.is_available(piece))
        {
            return ActiveSeedContentError::Unavailable;
        }
        let detail: Arc<str> = error.to_string().into();
        if self
            .availability
            .invalidate_epoch(expected_epoch)
            .unwrap_or(true)
        {
            self.failure.report(piece, error);
        }
        ActiveSeedContentError::Storage(detail)
    }
}

impl ActiveContentReader {
    pub fn file(&self, file_index: usize) -> Result<ActiveFileReader, ActiveFileError> {
        let file = self
            .files
            .get(file_index)
            .cloned()
            .ok_or(ActiveFileError::InvalidFileIndex(file_index))?;
        if file.padding {
            return Err(ActiveFileError::PaddingFile(file_index));
        }
        if !self
            .selected
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(file_index)
            .copied()
            .unwrap_or(false)
        {
            return Err(ActiveFileError::UnselectedFile(file_index));
        }
        if self.cancellation.is_cancelled() {
            return Err(ActiveFileError::Closed);
        }
        Ok(ActiveFileReader {
            file_index,
            file,
            piece_length: self.piece_length,
            expected_epoch: self.availability.snapshot().epoch,
            availability: self.availability.clone(),
            planner: Arc::clone(&self.planner),
            failure: self.failure.clone(),
            cancellation: self.cancellation.clone(),
        })
    }
}

impl ActiveFileReader {
    #[must_use]
    pub fn file_index(&self) -> usize {
        self.file_index
    }

    #[must_use]
    pub fn file_name(&self) -> &str {
        &self.file.name
    }

    #[must_use]
    pub fn length(&self) -> u64 {
        self.file.length
    }

    #[must_use]
    pub fn expected_epoch(&self) -> u64 {
        self.expected_epoch
    }

    #[must_use]
    pub fn cancellation(&self) -> CancellationToken {
        self.cancellation.clone()
    }

    #[must_use]
    pub fn is_generation_current(&self) -> bool {
        !self.cancellation.is_cancelled()
            && self.availability.snapshot().epoch == self.expected_epoch
    }

    pub fn demand_intervals(
        &self,
        offset: u64,
        length: usize,
    ) -> Result<(StreamingPieceInterval, Option<StreamingPieceInterval>), ActiveFileError> {
        let (torrent_start, torrent_end) = self.checked_nonempty_range(offset, length)?;
        let piece_length = u64::from(self.piece_length);
        let current_first = u32::try_from(torrent_start / piece_length)
            .map_err(|_| ActiveFileError::ArithmeticOverflow)?;
        let current_last = u32::try_from((torrent_end - 1) / piece_length)
            .map_err(|_| ActiveFileError::ArithmeticOverflow)?;
        let current = StreamingPieceInterval::new(current_first, current_last)
            .map_err(|_| ActiveFileError::ArithmeticOverflow)?;
        let file_end = self
            .file
            .torrent_offset
            .checked_add(self.file.length)
            .ok_or(ActiveFileError::ArithmeticOverflow)?;
        let byte_ahead_end = torrent_end
            .saturating_add(MAX_STREAMING_AHEAD_BYTES)
            .min(file_end);
        let byte_ahead_last = u32::try_from(byte_ahead_end.saturating_sub(1) / piece_length)
            .map_err(|_| ActiveFileError::ArithmeticOverflow)?;
        let piece_ahead_last = current_last.saturating_add(MAX_STREAMING_AHEAD_PIECES);
        let ahead_last = byte_ahead_last.min(piece_ahead_last);
        let ahead_first = current_last.saturating_add(1);
        let ahead = (ahead_first <= ahead_last)
            .then(|| StreamingPieceInterval::new(ahead_first, ahead_last))
            .transpose()
            .map_err(|_| ActiveFileError::ArithmeticOverflow)?;
        Ok((current, ahead))
    }

    pub fn is_range_verified(&self, offset: u64, length: usize) -> Result<bool, ActiveFileError> {
        if length == 0 {
            self.checked_range(offset, length)?;
            return Ok(true);
        }
        let (torrent_start, torrent_end) = self.checked_nonempty_range(offset, length)?;
        let snapshot = self.availability.snapshot();
        if self.cancellation.is_cancelled() || snapshot.epoch != self.expected_epoch {
            return Err(ActiveFileError::Unavailable);
        }
        let piece_length = u64::from(self.piece_length);
        let first = torrent_start / piece_length;
        let last = (torrent_end - 1) / piece_length;
        Ok((first..=last).all(|piece| {
            usize::try_from(piece)
                .ok()
                .is_some_and(|piece| snapshot.is_available(piece))
        }))
    }

    pub async fn read_range(&self, offset: u64, length: usize) -> Result<Vec<u8>, ActiveFileError> {
        self.checked_range(offset, length)?;
        if length > MAX_ACTIVE_FILE_READ_BYTES {
            return Err(ActiveFileError::ReadTooLarge {
                actual: length,
                maximum: MAX_ACTIVE_FILE_READ_BYTES,
            });
        }
        if length == 0 {
            return Ok(Vec::new());
        }
        if !self.is_range_verified(offset, length)? {
            return Err(ActiveFileError::Unavailable);
        }
        let planner = self
            .planner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .ok_or(ActiveFileError::Closed)?;
        let (response, completion) = oneshot::channel();
        planner
            .send(ActiveFilePlanRequest {
                file_index: self.file_index,
                offset,
                length,
                route_epoch: self.expected_epoch,
                response,
            })
            .await
            .map_err(|_| ActiveFileError::Closed)?;
        let plan = match completion.await.map_err(|_| ActiveFileError::Closed)? {
            Ok(plan) => plan,
            Err(error) => return Err(self.classify_storage_failure(error)),
        };
        if !self.is_range_verified(offset, length)? {
            return Err(ActiveFileError::Unavailable);
        }
        let bytes = plan
            .execute()
            .await
            .map_err(|error| self.classify_storage_failure(error))?;
        if !self.is_range_verified(offset, length)? {
            return Err(ActiveFileError::Unavailable);
        }
        Ok(bytes)
    }

    fn checked_range(&self, offset: u64, length: usize) -> Result<(u64, u64), ActiveFileError> {
        let length = u64::try_from(length).map_err(|_| ActiveFileError::ArithmeticOverflow)?;
        let end = offset
            .checked_add(length)
            .ok_or(ActiveFileError::ArithmeticOverflow)?;
        if offset > self.file.length || end > self.file.length {
            return Err(ActiveFileError::InvalidRange {
                offset,
                length: usize::try_from(length).unwrap_or(usize::MAX),
                file_length: self.file.length,
            });
        }
        let torrent_start = self
            .file
            .torrent_offset
            .checked_add(offset)
            .ok_or(ActiveFileError::ArithmeticOverflow)?;
        let torrent_end = torrent_start
            .checked_add(length)
            .ok_or(ActiveFileError::ArithmeticOverflow)?;
        Ok((torrent_start, torrent_end))
    }

    fn checked_nonempty_range(
        &self,
        offset: u64,
        length: usize,
    ) -> Result<(u64, u64), ActiveFileError> {
        if length == 0 {
            return Err(ActiveFileError::InvalidRange {
                offset,
                length,
                file_length: self.file.length,
            });
        }
        self.checked_range(offset, length)
    }

    fn classify_storage_failure(&self, error: SelectiveStorageError) -> ActiveFileError {
        let current = self.availability.snapshot();
        if current.epoch != self.expected_epoch || self.cancellation.is_cancelled() {
            return ActiveFileError::Unavailable;
        }
        let piece = self
            .file
            .torrent_offset
            .checked_div(u64::from(self.piece_length))
            .and_then(|piece| u32::try_from(piece).ok())
            .unwrap_or(0);
        let detail: Arc<str> = error.to_string().into();
        if self
            .availability
            .invalidate_epoch(self.expected_epoch)
            .unwrap_or(true)
        {
            self.failure.report(piece, error);
        }
        ActiveFileError::Storage(detail)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ActiveFileError {
    InvalidFileIndex(usize),
    PaddingFile(usize),
    UnselectedFile(usize),
    InvalidRange {
        offset: u64,
        length: usize,
        file_length: u64,
    },
    ReadTooLarge {
        actual: usize,
        maximum: usize,
    },
    ArithmeticOverflow,
    Closed,
    Unavailable,
    Storage(Arc<str>),
}

impl fmt::Display for ActiveFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFileIndex(index) => write!(formatter, "active file {index} is invalid"),
            Self::PaddingFile(index) => write!(formatter, "active file {index} is padding"),
            Self::UnselectedFile(index) => write!(formatter, "active file {index} is not selected"),
            Self::InvalidRange {
                offset,
                length,
                file_length,
            } => write!(
                formatter,
                "active file range {offset}+{length} exceeds length {file_length}"
            ),
            Self::ReadTooLarge { actual, maximum } => {
                write!(formatter, "active file read {actual} exceeds {maximum}")
            }
            Self::ArithmeticOverflow => formatter.write_str("active file arithmetic overflow"),
            Self::Closed => formatter.write_str("active file storage owner is closed"),
            Self::Unavailable => formatter.write_str("active file range is not verified"),
            Self::Storage(error) => write!(formatter, "active file storage failed: {error}"),
        }
    }
}

impl Error for ActiveFileError {}

#[derive(Debug)]
pub(crate) enum ActiveSeedContentError {
    Closed,
    Unavailable,
    Storage(Arc<str>),
}

impl fmt::Display for ActiveSeedContentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => formatter.write_str("active upload storage owner is closed"),
            Self::Unavailable => formatter.write_str("active upload piece is unavailable"),
            Self::Storage(error) => write!(formatter, "active upload storage failed: {error}"),
        }
    }
}

impl Error for ActiveSeedContentError {}

#[cfg(test)]
mod tests {
    use rstorrent_protocol::metainfo::{Metainfo, MetainfoFile, MetainfoMode};
    use rstorrent_protocol::storage_layout::TorrentLayout;

    use super::*;

    fn metainfo(piece_length: u32, length: u64) -> Metainfo {
        let piece_count = length.div_ceil(u64::from(piece_length)) as usize;
        Metainfo {
            info_hash: [7; 20],
            piece_hashes: vec![[9; 20]; piece_count],
            piece_length,
            total_length: length,
            name: "active.bin".to_owned(),
            private: false,
            mode: MetainfoMode::SingleFile,
            files: vec![MetainfoFile {
                path: vec!["active.bin".to_owned()],
                length,
                offset: 0,
                padding: false,
            }],
        }
    }

    #[test]
    fn tiny_piece_chunk_and_ahead_geometry_stays_compact() {
        let metainfo = metainfo(1, 70_000);
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[]).expect("selection");
        let availability = PieceAvailability::empty(layout.piece_count(), 4).expect("availability");
        let (uploads, _upload_requests) = mpsc::channel(ACTIVE_UPLOAD_PLAN_CAPACITY);
        let (files, _file_requests) = mpsc::channel(ACTIVE_UPLOAD_PLAN_CAPACITY);
        let content = ActiveSeedContent::new(
            metainfo.info_hash,
            false,
            vec![1; layout.piece_count()],
            availability,
            uploads,
        );
        let layout = ContentLayout::from(TorrentLayout::from_metainfo(&metainfo));
        let reader = content.configure_file_access(
            &metainfo.name,
            &layout,
            &selection,
            CancellationToken::new(),
            files,
        );
        let file = reader.file(0).expect("active file");

        let (current, ahead) = file
            .demand_intervals(0, MAX_ACTIVE_FILE_READ_BYTES)
            .expect("demand geometry");

        assert_eq!((current.first(), current.last()), (0, 65_535));
        let ahead = ahead.expect("bounded ahead");
        assert_eq!((ahead.first(), ahead.last()), (65_536, 65_551));
        assert_eq!(std::mem::size_of_val(&current), 8);
    }

    #[test]
    fn active_reader_requires_selected_file_and_current_epoch() {
        let metainfo = metainfo(4, 8);
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[]).expect("selection");
        let availability = PieceAvailability::new(8, &[true, false]).expect("availability");
        let (uploads, _upload_requests) = mpsc::channel(ACTIVE_UPLOAD_PLAN_CAPACITY);
        let (files, _file_requests) = mpsc::channel(ACTIVE_UPLOAD_PLAN_CAPACITY);
        let content = ActiveSeedContent::new(
            metainfo.info_hash,
            false,
            vec![4, 4],
            availability.clone(),
            uploads,
        );
        let layout = ContentLayout::from(TorrentLayout::from_metainfo(&metainfo));
        let reader = content.configure_file_access(
            &metainfo.name,
            &layout,
            &selection,
            CancellationToken::new(),
            files,
        );
        let file = reader.file(0).expect("active file");
        assert_eq!(file.is_range_verified(0, 4), Ok(true));
        assert_eq!(file.is_range_verified(0, 8), Ok(false));
        availability
            .replace_epoch(9, &[true, true])
            .expect("new route");
        assert_eq!(
            file.is_range_verified(0, 4),
            Err(ActiveFileError::Unavailable)
        );

        let skipped = FileSelection::new_content(&layout, &[0]).expect("skipped selection");
        content.update_file_selection(&skipped);
        assert!(matches!(
            reader.file(0),
            Err(ActiveFileError::UnselectedFile(0))
        ));
    }
}
