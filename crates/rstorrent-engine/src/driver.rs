use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use rstorrent_protocol::bencode::MAX_BENCODE_INPUT_LENGTH;
use rstorrent_protocol::metainfo::{Metainfo, MetainfoError};
use rstorrent_protocol::peer_wire::{
    FrameDecoder, FrameError, HANDSHAKE_LENGTH, HandshakeError, PeerMessage, decode_handshake,
    encode_handshake, encode_message,
};
use rstorrent_protocol::piece::{DownloadAction, OnePieceDownload, PieceError, VerifiedPiece};
use rstorrent_protocol::storage_layout::{FileSelection, LayoutError, TorrentLayout};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::selective_storage::{
    SelectiveStorage, SelectiveStorageError, remove_selective_part_if_present,
    remove_selective_staging_if_present,
};
use crate::storage::{
    StagingFile, StorageError, VERIFICATION_CHUNK_LENGTH, remove_staging_if_present, staging_path,
};

const DIAGNOSTIC_PEER_ID: [u8; 20] = *b"-RS0001-000000000000";
const NETWORK_READ_LENGTH: usize = 16 * 1024;

#[derive(Clone, Debug)]
pub struct DownloadConfig {
    pub metainfo_path: PathBuf,
    pub peer: SocketAddr,
    pub output_path: PathBuf,
    pub timeout: Duration,
    pub max_buffered_payload_bytes: usize,
    pub skip_files: Vec<usize>,
    pub materialize_files: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct DownloadControl {
    inner: Arc<DownloadControlInner>,
}

#[derive(Debug)]
struct DownloadControlInner {
    cancellation: CancellationToken,
    buffered_payload_bytes: AtomicUsize,
    payload_high_water: AtomicUsize,
    stored_bytes: AtomicUsize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DownloadProgress {
    pub buffered_payload_bytes: usize,
    pub payload_high_water: usize,
    pub stored_bytes: usize,
}

impl DownloadControl {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DownloadControlInner {
                cancellation: CancellationToken::new(),
                buffered_payload_bytes: AtomicUsize::new(0),
                payload_high_water: AtomicUsize::new(0),
                stored_bytes: AtomicUsize::new(0),
            }),
        }
    }

    pub fn cancel(&self) {
        self.inner.cancellation.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancellation.is_cancelled()
    }

    pub fn snapshot(&self) -> DownloadProgress {
        DownloadProgress {
            buffered_payload_bytes: self.inner.buffered_payload_bytes.load(Ordering::Acquire),
            payload_high_water: self.inner.payload_high_water.load(Ordering::Acquire),
            stored_bytes: self.inner.stored_bytes.load(Ordering::Acquire),
        }
    }

    fn observe(&self, download: &OnePieceDownload) {
        let budget = download.payload_budget();
        self.inner
            .buffered_payload_bytes
            .store(budget.reserved, Ordering::Release);
        self.inner
            .payload_high_water
            .fetch_max(budget.high_water, Ordering::AcqRel);
    }

    fn record_stored(&self, bytes: usize) {
        self.inner.stored_bytes.fetch_add(bytes, Ordering::AcqRel);
    }

    fn clear_buffered_payload(&self) {
        self.inner
            .buffered_payload_bytes
            .store(0, Ordering::Release);
    }
}

impl Default for DownloadControl {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadReport {
    pub info_hash: [u8; 20],
    pub piece_hash: [u8; 20],
    pub bytes_written: usize,
    pub block_count: usize,
    pub payload_limit: usize,
    pub payload_high_water: usize,
    pub verification_buffer: usize,
    pub piece_count: usize,
    pub verified_piece_count: usize,
    pub skipped_piece_count: usize,
    pub selected_file_bytes: u64,
    pub skipped_file_bytes: u64,
    pub padding_bytes: u64,
    pub selected_written_bytes: usize,
    pub part_written_bytes: usize,
    pub materialized_bytes: u64,
    pub part_slots_before_materialization: usize,
    pub part_slots_after_materialization: usize,
    pub part_reopened: bool,
    pub part_path: Option<PathBuf>,
}

#[derive(Debug)]
pub enum DownloadError {
    NonLoopbackPeer(SocketAddr),
    InvalidTimeout,
    MetainfoTooLarge {
        maximum: usize,
    },
    Io {
        operation: &'static str,
        source: io::Error,
    },
    Metainfo(MetainfoError),
    Handshake(HandshakeError),
    Frame(FrameError),
    Piece(PieceError),
    Layout(LayoutError),
    Storage(StorageError),
    SelectiveStorage(SelectiveStorageError),
    PeerClosed,
    Cancelled,
    TimedOut {
        timeout: Duration,
    },
    CleanupAfterFailure {
        failure: String,
        source: io::Error,
    },
}

impl fmt::Display for DownloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonLoopbackPeer(peer) => {
                write!(
                    formatter,
                    "diagnostic peer {peer} is not a loopback address"
                )
            }
            Self::InvalidTimeout => write!(formatter, "diagnostic timeout must be nonzero"),
            Self::MetainfoTooLarge { maximum } => {
                write!(formatter, "metainfo exceeds input limit {maximum}")
            }
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::Metainfo(error) => write!(formatter, "metainfo: {error}"),
            Self::Handshake(error) => write!(formatter, "peer handshake: {error}"),
            Self::Frame(error) => write!(formatter, "peer frame: {error}"),
            Self::Piece(error) => write!(formatter, "piece state: {error}"),
            Self::Layout(error) => write!(formatter, "torrent layout: {error}"),
            Self::Storage(error) => write!(formatter, "storage: {error}"),
            Self::SelectiveStorage(error) => write!(formatter, "selective storage: {error}"),
            Self::PeerClosed => write!(formatter, "peer closed before piece verification"),
            Self::Cancelled => write!(formatter, "download cancelled"),
            Self::TimedOut { timeout } => {
                write!(
                    formatter,
                    "diagnostic timed out after {}s",
                    timeout.as_secs()
                )
            }
            Self::CleanupAfterFailure { failure, source } => write!(
                formatter,
                "{failure}; additionally failed to remove staging output: {source}"
            ),
        }
    }
}

impl Error for DownloadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Metainfo(error) => Some(error),
            Self::Handshake(error) => Some(error),
            Self::Frame(error) => Some(error),
            Self::Piece(error) => Some(error),
            Self::Layout(error) => Some(error),
            Self::Storage(error) => Some(error),
            Self::SelectiveStorage(error) => Some(error),
            Self::CleanupAfterFailure { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl DownloadError {
    pub fn is_existing_artifact(&self) -> bool {
        preserves_existing_artifact(self)
    }
}

pub async fn download_verified_piece(
    config: DownloadConfig,
) -> Result<DownloadReport, DownloadError> {
    download_verified_piece_with_control(config, DownloadControl::new()).await
}

pub async fn download_verified_piece_with_control(
    config: DownloadConfig,
    control: DownloadControl,
) -> Result<DownloadReport, DownloadError> {
    if !config.peer.ip().is_loopback() {
        return Err(DownloadError::NonLoopbackPeer(config.peer));
    }
    if config.timeout.is_zero() {
        return Err(DownloadError::InvalidTimeout);
    }

    let configured_timeout = config.timeout;
    let staging = staging_path(&config.output_path).map_err(DownloadError::Storage)?;
    let output_path = config.output_path.clone();
    let result = tokio::select! {
        biased;
        _ = control.inner.cancellation.cancelled() => Err(DownloadError::Cancelled),
        result = timeout(
            configured_timeout,
            run_download(config, control.clone()),
        ) => result
            .map_err(|_| DownloadError::TimedOut {
                timeout: configured_timeout,
            })
            .and_then(|result| result),
    };
    control.clear_buffered_payload();

    match result {
        Ok(report) => Ok(report),
        Err(error) if preserves_existing_artifact(&error) => Err(error),
        Err(error) => {
            let cleanup = async {
                remove_staging_if_present(&staging).await?;
                remove_selective_staging_if_present(&output_path).await?;
                remove_selective_part_if_present(&output_path).await
            }
            .await;
            match cleanup {
                Ok(()) => Err(error),
                Err(source) => Err(DownloadError::CleanupAfterFailure {
                    failure: error.to_string(),
                    source,
                }),
            }
        }
    }
}

fn preserves_existing_artifact(error: &DownloadError) -> bool {
    matches!(
        error,
        DownloadError::Storage(StorageError::ExistingOutput(_))
            | DownloadError::Storage(StorageError::ExistingStaging(_))
            | DownloadError::SelectiveStorage(SelectiveStorageError::ExistingOutput(_))
            | DownloadError::SelectiveStorage(SelectiveStorageError::ExistingStaging(_))
            | DownloadError::SelectiveStorage(SelectiveStorageError::ExistingPartFile(_))
            | DownloadError::SelectiveStorage(SelectiveStorageError::PartFile(
                crate::part_file::PartFileError::Existing(_)
            ))
    )
}

async fn run_download(
    config: DownloadConfig,
    control: DownloadControl,
) -> Result<DownloadReport, DownloadError> {
    let metainfo_bytes = read_bounded_metainfo(&config.metainfo_path).await?;
    let metainfo = Metainfo::from_bytes(&metainfo_bytes).map_err(DownloadError::Metainfo)?;
    match metainfo.mode {
        rstorrent_protocol::metainfo::MetainfoMode::SingleFile => {
            if metainfo.piece_count() != 1
                || !config.skip_files.is_empty()
                || !config.materialize_files.is_empty()
            {
                return Err(DownloadError::Metainfo(MetainfoError::Unsupported(
                    "multi-piece single-file or selected single-file diagnostic execution",
                )));
            }
            run_single_download(config, metainfo, control).await
        }
        rstorrent_protocol::metainfo::MetainfoMode::MultiFile => {
            run_selective_download(config, metainfo, control).await
        }
    }
}

async fn run_single_download(
    config: DownloadConfig,
    metainfo: Metainfo,
    control: DownloadControl,
) -> Result<DownloadReport, DownloadError> {
    let piece_length = u32::try_from(metainfo.total_length)
        .map_err(|_| DownloadError::Metainfo(MetainfoError::InvalidField("info.length")))?;
    let mut download = OnePieceDownload::new(
        0,
        piece_length,
        metainfo.piece_hashes[0],
        config.max_buffered_payload_bytes,
    )
    .map_err(DownloadError::Piece)?;
    let mut storage = StagingFile::create(config.output_path.clone(), metainfo.total_length)
        .await
        .map_err(DownloadError::Storage)?;

    let mut peer = connect_peer(config.peer, metainfo.info_hash).await?;

    let mut decoder = FrameDecoder::new();
    let mut network_buffer = [0_u8; NETWORK_READ_LENGTH];
    loop {
        let read = peer
            .read(&mut network_buffer)
            .await
            .map_err(|source| DownloadError::Io {
                operation: "read peer message",
                source,
            })?;
        if read == 0 {
            download.cancel_pending();
            control.observe(&download);
            return Err(DownloadError::PeerClosed);
        }

        let messages = decoder
            .push(&network_buffer[..read])
            .map_err(DownloadError::Frame)?;
        for message in messages {
            let actions = download.on_message(message).map_err(DownloadError::Piece)?;
            control.observe(&download);
            if let Some(piece) =
                process_actions(&mut peer, &mut storage, &mut download, actions, &control).await?
            {
                let budget = download.payload_budget();
                let block_count = download.block_count();
                storage.finalize().await.map_err(DownloadError::Storage)?;
                return Ok(DownloadReport {
                    info_hash: metainfo.info_hash,
                    piece_hash: piece.hash,
                    bytes_written: piece.length as usize,
                    block_count,
                    payload_limit: budget.limit,
                    payload_high_water: budget.high_water,
                    verification_buffer: VERIFICATION_CHUNK_LENGTH,
                    piece_count: 1,
                    verified_piece_count: 1,
                    skipped_piece_count: 0,
                    selected_file_bytes: metainfo.total_length,
                    skipped_file_bytes: 0,
                    padding_bytes: 0,
                    selected_written_bytes: piece.length as usize,
                    part_written_bytes: 0,
                    materialized_bytes: 0,
                    part_slots_before_materialization: 0,
                    part_slots_after_materialization: 0,
                    part_reopened: false,
                    part_path: None,
                });
            }
        }
    }
}

async fn run_selective_download(
    config: DownloadConfig,
    metainfo: Metainfo,
    control: DownloadControl,
) -> Result<DownloadReport, DownloadError> {
    let layout = TorrentLayout::from_metainfo(&metainfo);
    let selection =
        FileSelection::new(&layout, &config.skip_files).map_err(DownloadError::Layout)?;
    for &file_index in &config.materialize_files {
        let file = layout.files().get(file_index).ok_or(DownloadError::Layout(
            LayoutError::InvalidFileIndex {
                index: file_index,
                file_count: layout.files().len(),
            },
        ))?;
        if file.padding || selection.is_wanted(file_index) {
            return Err(DownloadError::Metainfo(MetainfoError::Unsupported(
                "materialized files must be initially skipped non-padding files",
            )));
        }
    }

    let mut plans = Vec::new();
    let mut skipped_piece_count = 0;
    for piece_index in 0..layout.piece_count() {
        let piece_index_u32 = u32::try_from(piece_index)
            .map_err(|_| DownloadError::Layout(LayoutError::ArithmeticOverflow))?;
        let ranges = layout
            .request_ranges(piece_index_u32, &selection)
            .map_err(DownloadError::Layout)?;
        if ranges.is_empty() {
            skipped_piece_count += 1;
        } else {
            plans.push((piece_index_u32, ranges));
        }
    }
    if plans.is_empty() {
        return Err(DownloadError::Metainfo(MetainfoError::Unsupported(
            "selection with no wanted pieces",
        )));
    }

    let mut storage = SelectiveStorage::create(
        config.output_path.clone(),
        &metainfo,
        layout.clone(),
        selection,
    )
    .await
    .map_err(DownloadError::SelectiveStorage)?;
    let selected_file_bytes = storage.selected_bytes();
    let skipped_file_bytes = storage.skipped_bytes();
    let padding_bytes = storage.padding_bytes();
    let part_path = storage.part_path().to_path_buf();
    let mut peer = connect_peer(config.peer, metainfo.info_hash).await?;
    let mut decoder = FrameDecoder::new();
    let mut network_buffer = [0_u8; NETWORK_READ_LENGTH];
    let mut queued_messages = VecDeque::new();
    let mut availability = vec![false; layout.piece_count()];
    let mut availability_known = false;
    let mut peer_choking = true;
    let mut total_blocks = 0;
    let mut total_bytes = 0;
    let mut selected_written_bytes = 0;
    let mut part_written_bytes = 0;
    let mut payload_high_water = 0;
    let mut last_piece = None;

    for (piece_index, ranges) in plans {
        let piece_index_usize = usize::try_from(piece_index)
            .map_err(|_| DownloadError::Layout(LayoutError::ArithmeticOverflow))?;
        let piece_length = layout
            .piece_length_at(piece_index)
            .map_err(DownloadError::Layout)?;
        let mut download = OnePieceDownload::new_for_torrent(
            piece_index,
            piece_length,
            metainfo.piece_hashes[piece_index_usize],
            config.max_buffered_payload_bytes,
            layout.piece_count(),
            &ranges,
        )
        .map_err(DownloadError::Piece)?;
        total_blocks += download.block_count();
        total_bytes += ranges
            .iter()
            .map(|range| range.length as usize)
            .sum::<usize>();

        let mut initial_actions = Vec::new();
        if availability_known {
            initial_actions.extend(
                download
                    .on_message(PeerMessage::Bitfield(encode_availability(&availability)))
                    .map_err(DownloadError::Piece)?,
            );
        }
        if !peer_choking {
            initial_actions.extend(
                download
                    .on_message(PeerMessage::Unchoke)
                    .map_err(DownloadError::Piece)?,
            );
        }
        control.observe(&download);
        if let Some(piece) = process_selective_actions(
            &mut peer,
            &mut storage,
            &mut download,
            initial_actions,
            &mut selected_written_bytes,
            &mut part_written_bytes,
            &control,
        )
        .await?
        {
            storage
                .record_verified(piece.index as usize)
                .map_err(DownloadError::SelectiveStorage)?;
            payload_high_water = payload_high_water.max(download.payload_budget().high_water);
            last_piece = Some(piece);
            continue;
        }

        loop {
            while queued_messages.is_empty() {
                let read =
                    peer.read(&mut network_buffer)
                        .await
                        .map_err(|source| DownloadError::Io {
                            operation: "read peer message",
                            source,
                        })?;
                if read == 0 {
                    download.cancel_pending();
                    control.observe(&download);
                    return Err(DownloadError::PeerClosed);
                }
                queued_messages.extend(
                    decoder
                        .push(&network_buffer[..read])
                        .map_err(DownloadError::Frame)?,
                );
            }

            let message = queued_messages
                .pop_front()
                .expect("message queue is nonempty after receive loop");
            let availability_update = availability_update(&message);
            let actions = download.on_message(message).map_err(DownloadError::Piece)?;
            control.observe(&download);
            match availability_update {
                AvailabilityUpdate::None => {}
                AvailabilityUpdate::Choke(choking) => peer_choking = choking,
                AvailabilityUpdate::Have(index) => {
                    availability[index as usize] = true;
                    availability_known = true;
                }
                AvailabilityUpdate::Bitfield(bitfield) => {
                    decode_availability(&bitfield, &mut availability);
                    availability_known = true;
                }
            }

            if let Some(piece) = process_selective_actions(
                &mut peer,
                &mut storage,
                &mut download,
                actions,
                &mut selected_written_bytes,
                &mut part_written_bytes,
                &control,
            )
            .await?
            {
                storage
                    .record_verified(piece.index as usize)
                    .map_err(DownloadError::SelectiveStorage)?;
                payload_high_water = payload_high_water.max(download.payload_budget().high_water);
                last_piece = Some(piece);
                break;
            }
        }
    }

    storage
        .publish()
        .await
        .map_err(DownloadError::SelectiveStorage)?;
    let part_slots_before_materialization = storage.part_slots();
    storage
        .reopen_part_file()
        .await
        .map_err(DownloadError::SelectiveStorage)?;
    let mut materialized_bytes = 0_u64;
    for file_index in config.materialize_files {
        materialized_bytes += storage
            .materialize_file(file_index)
            .await
            .map_err(DownloadError::SelectiveStorage)?
            .bytes;
    }
    let part_slots_after_materialization = storage.part_slots();
    let last_piece = last_piece.expect("at least one wanted piece was planned");
    Ok(DownloadReport {
        info_hash: metainfo.info_hash,
        piece_hash: last_piece.hash,
        bytes_written: total_bytes,
        block_count: total_blocks,
        payload_limit: config.max_buffered_payload_bytes,
        payload_high_water,
        verification_buffer: VERIFICATION_CHUNK_LENGTH,
        piece_count: layout.piece_count(),
        verified_piece_count: layout.piece_count() - skipped_piece_count,
        skipped_piece_count,
        selected_file_bytes,
        skipped_file_bytes,
        padding_bytes,
        selected_written_bytes,
        part_written_bytes,
        materialized_bytes,
        part_slots_before_materialization,
        part_slots_after_materialization,
        part_reopened: true,
        part_path: Some(part_path),
    })
}

async fn connect_peer(
    address: SocketAddr,
    info_hash: [u8; 20],
) -> Result<TcpStream, DownloadError> {
    let mut peer = TcpStream::connect(address)
        .await
        .map_err(|source| DownloadError::Io {
            operation: "connect to peer",
            source,
        })?;
    peer.write_all(&encode_handshake(info_hash, DIAGNOSTIC_PEER_ID))
        .await
        .map_err(|source| DownloadError::Io {
            operation: "send peer handshake",
            source,
        })?;

    let mut handshake = [0_u8; HANDSHAKE_LENGTH];
    peer.read_exact(&mut handshake)
        .await
        .map_err(|source| DownloadError::Io {
            operation: "read peer handshake",
            source,
        })?;
    decode_handshake(&handshake, info_hash).map_err(DownloadError::Handshake)?;
    Ok(peer)
}

#[derive(Debug)]
enum AvailabilityUpdate {
    None,
    Choke(bool),
    Have(u32),
    Bitfield(Vec<u8>),
}

fn availability_update(message: &PeerMessage) -> AvailabilityUpdate {
    match message {
        PeerMessage::Choke => AvailabilityUpdate::Choke(true),
        PeerMessage::Unchoke => AvailabilityUpdate::Choke(false),
        PeerMessage::Have(index) => AvailabilityUpdate::Have(*index),
        PeerMessage::Bitfield(bitfield) => AvailabilityUpdate::Bitfield(bitfield.clone()),
        _ => AvailabilityUpdate::None,
    }
}

fn encode_availability(availability: &[bool]) -> Vec<u8> {
    let mut bitfield = vec![0_u8; availability.len().div_ceil(8)];
    for (index, available) in availability.iter().enumerate() {
        if *available {
            bitfield[index / 8] |= 1 << (7 - index % 8);
        }
    }
    bitfield
}

fn decode_availability(bitfield: &[u8], availability: &mut [bool]) {
    for (index, available) in availability.iter_mut().enumerate() {
        *available = bitfield[index / 8] & (1 << (7 - index % 8)) != 0;
    }
}

async fn read_bounded_metainfo(path: &Path) -> Result<Vec<u8>, DownloadError> {
    let file = File::open(path).await.map_err(|source| DownloadError::Io {
        operation: "open metainfo",
        source,
    })?;
    let mut bytes = Vec::new();
    file.take((MAX_BENCODE_INPUT_LENGTH + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .map_err(|source| DownloadError::Io {
            operation: "read metainfo",
            source,
        })?;
    if bytes.len() > MAX_BENCODE_INPUT_LENGTH {
        return Err(DownloadError::MetainfoTooLarge {
            maximum: MAX_BENCODE_INPUT_LENGTH,
        });
    }
    Ok(bytes)
}

async fn send_message(peer: &mut TcpStream, message: &PeerMessage) -> Result<(), DownloadError> {
    let frame = encode_message(message).map_err(DownloadError::Frame)?;
    peer.write_all(&frame)
        .await
        .map_err(|source| DownloadError::Io {
            operation: "send peer message",
            source,
        })
}

async fn process_actions(
    peer: &mut TcpStream,
    storage: &mut StagingFile,
    download: &mut OnePieceDownload,
    actions: Vec<DownloadAction>,
    control: &DownloadControl,
) -> Result<Option<VerifiedPiece>, DownloadError> {
    let mut pending = VecDeque::from(actions);
    while let Some(action) = pending.pop_front() {
        match action {
            DownloadAction::SendInterested => {
                send_message(peer, &PeerMessage::Interested).await?;
            }
            DownloadAction::Request(request) => {
                send_message(peer, &PeerMessage::Request(request)).await?;
            }
            DownloadAction::StoreBlock(block) => {
                let index = block.index;
                let begin = block.begin;
                let length = block.bytes.len();
                if let Err(error) = storage.write_block(u64::from(begin), block.bytes).await {
                    download
                        .on_block_write_failed(index, begin)
                        .map_err(DownloadError::Piece)?;
                    control.observe(download);
                    return Err(DownloadError::Storage(error));
                }
                control.record_stored(length);
                pending.extend(
                    download
                        .on_block_stored(index, begin)
                        .map_err(DownloadError::Piece)?,
                );
                control.observe(download);
            }
            DownloadAction::VerifyPiece { index, length } => {
                let actual_hash = storage
                    .hash_piece(0, length)
                    .await
                    .map_err(DownloadError::Storage)?;
                pending.push_back(
                    download
                        .finish_verification(index, actual_hash)
                        .map_err(DownloadError::Piece)?,
                );
            }
            DownloadAction::Verified(piece) => return Ok(Some(piece)),
        }
    }
    Ok(None)
}

async fn process_selective_actions(
    peer: &mut TcpStream,
    storage: &mut SelectiveStorage,
    download: &mut OnePieceDownload,
    actions: Vec<DownloadAction>,
    selected_written_bytes: &mut usize,
    part_written_bytes: &mut usize,
    control: &DownloadControl,
) -> Result<Option<VerifiedPiece>, DownloadError> {
    let mut pending = VecDeque::from(actions);
    while let Some(action) = pending.pop_front() {
        match action {
            DownloadAction::SendInterested => {
                send_message(peer, &PeerMessage::Interested).await?;
            }
            DownloadAction::Request(request) => {
                send_message(peer, &PeerMessage::Request(request)).await?;
            }
            DownloadAction::StoreBlock(block) => {
                let index = block.index;
                let begin = block.begin;
                let length = block.bytes.len();
                let stats = match storage.write_block(index, begin, block.bytes).await {
                    Ok(stats) => stats,
                    Err(error) => {
                        download
                            .on_block_write_failed(index, begin)
                            .map_err(DownloadError::Piece)?;
                        control.observe(download);
                        return Err(DownloadError::SelectiveStorage(error));
                    }
                };
                control.record_stored(length);
                *selected_written_bytes += stats.wanted_bytes;
                *part_written_bytes += stats.skipped_bytes;
                pending.extend(
                    download
                        .on_block_stored(index, begin)
                        .map_err(DownloadError::Piece)?,
                );
                control.observe(download);
            }
            DownloadAction::VerifyPiece { index, .. } => {
                let actual_hash = storage
                    .hash_piece(index)
                    .await
                    .map_err(DownloadError::SelectiveStorage)?;
                pending.push_back(
                    download
                        .finish_verification(index, actual_hash)
                        .map_err(DownloadError::Piece)?,
                );
            }
            DownloadAction::Verified(piece) => return Ok(Some(piece)),
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use rstorrent_protocol::piece::MIN_PAYLOAD_ALLOWANCE;
    use tokio::net::TcpListener;
    use tokio::time::timeout;

    use super::{
        DownloadConfig, DownloadControl, DownloadError, download_verified_piece,
        download_verified_piece_with_control,
    };
    use crate::selective_storage::{
        SelectiveStorageError, selective_part_path, selective_staging_path,
    };
    use crate::storage::staging_path;

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn test_path(name: &str) -> PathBuf {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rstorrent-driver-test-{}-{sequence}-{name}",
            std::process::id()
        ))
    }

    fn two_file_metainfo() -> Vec<u8> {
        let mut metainfo = b"d4:infod5:filesld6:lengthi1e4:pathl1:aee\
d6:lengthi32768e4:pathl1:beee4:name7:fixture12:piece lengthi32768e\
6:pieces40:"
            .to_vec();
        metainfo.extend_from_slice(&[1; 40]);
        metainfo.extend_from_slice(b"ee");
        metainfo
    }

    #[tokio::test]
    async fn timeout_removes_unverified_staging_output() {
        let metainfo_path = test_path("fixture.torrent");
        let output_path = test_path("output.bin");
        let staging = staging_path(&output_path).expect("staging path");
        let mut metainfo =
            b"d4:infod6:lengthi1e4:name1:x12:piece lengthi16384e6:pieces20:".to_vec();
        metainfo.extend_from_slice(&[1; 20]);
        metainfo.extend_from_slice(b"ee");
        tokio::fs::write(&metainfo_path, metainfo)
            .await
            .expect("write metainfo");

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind scripted peer");
        let address = listener.local_addr().expect("listener address");
        let peer_task = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.expect("accept diagnostic");
            tokio::time::sleep(Duration::from_secs(1)).await;
        });

        let result = download_verified_piece(DownloadConfig {
            metainfo_path: metainfo_path.clone(),
            peer: address,
            output_path: output_path.clone(),
            timeout: Duration::from_millis(50),
            max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
            skip_files: Vec::new(),
            materialize_files: Vec::new(),
        })
        .await;

        assert!(matches!(result, Err(DownloadError::TimedOut { .. })));
        assert!(
            !tokio::fs::try_exists(&output_path)
                .await
                .expect("output status")
        );
        assert!(
            !tokio::fs::try_exists(&staging)
                .await
                .expect("staging status")
        );

        peer_task.abort();
        let _ = peer_task.await;
        let _ = tokio::fs::remove_file(metainfo_path).await;
    }

    #[tokio::test]
    async fn selective_timeout_removes_owned_staging_and_part_paths() {
        let metainfo_path = test_path("selective-timeout.torrent");
        let output_path = test_path("selective-timeout");
        let staging = selective_staging_path(&output_path).expect("staging path");
        let part = selective_part_path(&output_path).expect("part path");
        tokio::fs::write(&metainfo_path, two_file_metainfo())
            .await
            .expect("write metainfo");

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind scripted peer");
        let address = listener.local_addr().expect("listener address");
        let peer_task = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.expect("accept diagnostic");
            tokio::time::sleep(Duration::from_secs(1)).await;
        });

        let result = download_verified_piece(DownloadConfig {
            metainfo_path: metainfo_path.clone(),
            peer: address,
            output_path: output_path.clone(),
            timeout: Duration::from_millis(50),
            max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
            skip_files: vec![1],
            materialize_files: Vec::new(),
        })
        .await;

        assert!(matches!(result, Err(DownloadError::TimedOut { .. })));
        assert!(!tokio::fs::try_exists(&output_path).await.expect("output"));
        assert!(!tokio::fs::try_exists(&staging).await.expect("staging"));
        assert!(!tokio::fs::try_exists(&part).await.expect("part"));

        peer_task.abort();
        let _ = peer_task.await;
        let _ = tokio::fs::remove_file(metainfo_path).await;
    }

    #[tokio::test]
    async fn cancellation_is_terminal_and_removes_owned_artifacts() {
        let metainfo_path = test_path("selective-cancel.torrent");
        let output_path = test_path("selective-cancel");
        let staging = selective_staging_path(&output_path).expect("staging path");
        let part = selective_part_path(&output_path).expect("part path");
        tokio::fs::write(&metainfo_path, two_file_metainfo())
            .await
            .expect("write metainfo");

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind scripted peer");
        let address = listener.local_addr().expect("listener address");
        let peer_task = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.expect("accept diagnostic");
            tokio::time::sleep(Duration::from_secs(1)).await;
        });

        let control = DownloadControl::new();
        let download_control = control.clone();
        let download_task = tokio::spawn(download_verified_piece_with_control(
            DownloadConfig {
                metainfo_path: metainfo_path.clone(),
                peer: address,
                output_path: output_path.clone(),
                timeout: Duration::from_secs(5),
                max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
                skip_files: vec![1],
                materialize_files: Vec::new(),
            },
            download_control,
        ));

        timeout(Duration::from_secs(1), async {
            loop {
                if tokio::fs::try_exists(&staging).await.expect("staging")
                    && tokio::fs::try_exists(&part).await.expect("part")
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("engine created owned artifacts");

        control.cancel();
        control.cancel();
        let result = download_task.await.expect("download task");
        assert!(matches!(result, Err(DownloadError::Cancelled)));
        assert!(control.is_cancelled());
        assert_eq!(control.snapshot().buffered_payload_bytes, 0);
        assert!(!tokio::fs::try_exists(&output_path).await.expect("output"));
        assert!(!tokio::fs::try_exists(&staging).await.expect("staging"));
        assert!(!tokio::fs::try_exists(&part).await.expect("part"));

        peer_task.abort();
        let _ = peer_task.await;
        let _ = tokio::fs::remove_file(metainfo_path).await;
    }

    #[tokio::test]
    async fn preexisting_selective_part_file_is_preserved() {
        let metainfo_path = test_path("selective-existing.torrent");
        let output_path = test_path("selective-existing");
        let part = selective_part_path(&output_path).expect("part path");
        tokio::fs::write(&metainfo_path, two_file_metainfo())
            .await
            .expect("write metainfo");
        tokio::fs::write(&part, b"owned elsewhere")
            .await
            .expect("write existing part");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind unused peer");
        let address = listener.local_addr().expect("listener address");

        let result = download_verified_piece(DownloadConfig {
            metainfo_path: metainfo_path.clone(),
            peer: address,
            output_path: output_path.clone(),
            timeout: Duration::from_secs(1),
            max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
            skip_files: vec![1],
            materialize_files: Vec::new(),
        })
        .await;
        assert!(matches!(
            result,
            Err(DownloadError::SelectiveStorage(
                SelectiveStorageError::ExistingPartFile(_)
            ))
        ));
        assert_eq!(
            tokio::fs::read(&part).await.expect("preserved part"),
            b"owned elsewhere"
        );

        let _ =
            tokio::fs::remove_dir_all(selective_staging_path(&output_path).expect("staging")).await;
        let _ = tokio::fs::remove_file(part).await;
        let _ = tokio::fs::remove_file(metainfo_path).await;
    }
}
