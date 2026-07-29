use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rstorrent_protocol::bencode::MAX_BENCODE_INPUT_LENGTH;
use rstorrent_protocol::metainfo::{Metainfo, MetainfoError};
use rstorrent_protocol::peer_wire::{
    FrameDecoder, FrameError, HANDSHAKE_LENGTH, HandshakeError, PeerMessage, decode_handshake,
    encode_handshake, encode_message,
};
use rstorrent_protocol::piece::{DownloadAction, OnePieceDownload, PieceError, VerifiedPiece};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

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
    Storage(StorageError),
    PeerClosed,
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
            Self::Storage(error) => write!(formatter, "storage: {error}"),
            Self::PeerClosed => write!(formatter, "peer closed before piece verification"),
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
            Self::Storage(error) => Some(error),
            Self::CleanupAfterFailure { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub async fn download_verified_piece(
    config: DownloadConfig,
) -> Result<DownloadReport, DownloadError> {
    if !config.peer.ip().is_loopback() {
        return Err(DownloadError::NonLoopbackPeer(config.peer));
    }
    if config.timeout.is_zero() {
        return Err(DownloadError::InvalidTimeout);
    }

    let configured_timeout = config.timeout;
    let staging = staging_path(&config.output_path).map_err(DownloadError::Storage)?;
    let result = timeout(configured_timeout, run_download(config))
        .await
        .map_err(|_| DownloadError::TimedOut {
            timeout: configured_timeout,
        })
        .and_then(|result| result);

    match result {
        Ok(report) => Ok(report),
        Err(error) => match remove_staging_if_present(&staging).await {
            Ok(()) => Err(error),
            Err(source) => Err(DownloadError::CleanupAfterFailure {
                failure: error.to_string(),
                source,
            }),
        },
    }
}

async fn run_download(config: DownloadConfig) -> Result<DownloadReport, DownloadError> {
    let metainfo_bytes = read_bounded_metainfo(&config.metainfo_path).await?;
    let metainfo = Metainfo::from_bytes(&metainfo_bytes).map_err(DownloadError::Metainfo)?;
    let piece_length = u32::try_from(metainfo.file_length)
        .map_err(|_| DownloadError::Metainfo(MetainfoError::InvalidField("info.length")))?;
    let mut download = OnePieceDownload::new(
        0,
        piece_length,
        metainfo.piece_hash,
        config.max_buffered_payload_bytes,
    )
    .map_err(DownloadError::Piece)?;
    let mut storage = StagingFile::create(config.output_path.clone(), metainfo.file_length)
        .await
        .map_err(DownloadError::Storage)?;

    let mut peer = TcpStream::connect(config.peer)
        .await
        .map_err(|source| DownloadError::Io {
            operation: "connect to peer",
            source,
        })?;
    peer.write_all(&encode_handshake(metainfo.info_hash, DIAGNOSTIC_PEER_ID))
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
    decode_handshake(&handshake, metainfo.info_hash).map_err(DownloadError::Handshake)?;

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
            return Err(DownloadError::PeerClosed);
        }

        let messages = decoder
            .push(&network_buffer[..read])
            .map_err(DownloadError::Frame)?;
        for message in messages {
            let actions = download.on_message(message).map_err(DownloadError::Piece)?;
            if let Some(piece) =
                process_actions(&mut peer, &mut storage, &mut download, actions).await?
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
                });
            }
        }
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
                if let Err(error) = storage.write_block(u64::from(begin), block.bytes).await {
                    download
                        .on_block_write_failed(index, begin)
                        .map_err(DownloadError::Piece)?;
                    return Err(DownloadError::Storage(error));
                }
                pending.extend(
                    download
                        .on_block_stored(index, begin)
                        .map_err(DownloadError::Piece)?,
                );
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use rstorrent_protocol::piece::MIN_PAYLOAD_ALLOWANCE;
    use tokio::net::TcpListener;

    use super::{DownloadConfig, DownloadError, download_verified_piece};
    use crate::storage::staging_path;

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn test_path(name: &str) -> PathBuf {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rstorrent-driver-test-{}-{sequence}-{name}",
            std::process::id()
        ))
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
}
