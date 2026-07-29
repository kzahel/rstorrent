use std::error::Error;
use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
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

const DIAGNOSTIC_PEER_ID: [u8; 20] = *b"-RS0001-000000000000";
const NETWORK_READ_LENGTH: usize = 16 * 1024;

#[derive(Clone, Debug)]
pub struct DownloadConfig {
    pub metainfo_path: PathBuf,
    pub peer: SocketAddr,
    pub output_path: PathBuf,
    pub timeout: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadReport {
    pub info_hash: [u8; 20],
    pub piece_hash: [u8; 20],
    pub bytes_written: usize,
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
    PeerClosed,
    TimedOut {
        timeout: Duration,
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
            Self::PeerClosed => write!(formatter, "peer closed before piece verification"),
            Self::TimedOut { timeout } => {
                write!(
                    formatter,
                    "diagnostic timed out after {}s",
                    timeout.as_secs()
                )
            }
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
    timeout(configured_timeout, run_download(config))
        .await
        .map_err(|_| DownloadError::TimedOut {
            timeout: configured_timeout,
        })?
}

async fn run_download(config: DownloadConfig) -> Result<DownloadReport, DownloadError> {
    let metainfo_bytes = read_bounded_metainfo(&config.metainfo_path).await?;
    let metainfo = Metainfo::from_bytes(&metainfo_bytes).map_err(DownloadError::Metainfo)?;
    let piece_length = u32::try_from(metainfo.file_length)
        .map_err(|_| DownloadError::Metainfo(MetainfoError::InvalidField("info.length")))?;
    let mut download = OnePieceDownload::new(0, piece_length, metainfo.piece_hash)
        .map_err(DownloadError::Piece)?;

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
            return Err(DownloadError::PeerClosed);
        }

        let messages = decoder
            .push(&network_buffer[..read])
            .map_err(DownloadError::Frame)?;
        for message in messages {
            let actions = download.on_message(message).map_err(DownloadError::Piece)?;
            for action in actions {
                match action {
                    DownloadAction::SendInterested => {
                        send_message(&mut peer, &PeerMessage::Interested).await?;
                    }
                    DownloadAction::Request(request) => {
                        send_message(&mut peer, &PeerMessage::Request(request)).await?;
                    }
                    DownloadAction::Verified(piece) => {
                        return write_verified_piece(&config, &metainfo, piece).await;
                    }
                }
            }
        }
    }
}

async fn read_bounded_metainfo(path: &PathBuf) -> Result<Vec<u8>, DownloadError> {
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

async fn write_verified_piece(
    config: &DownloadConfig,
    metainfo: &Metainfo,
    piece: VerifiedPiece,
) -> Result<DownloadReport, DownloadError> {
    let bytes_written = piece.bytes.len();
    tokio::fs::write(&config.output_path, piece.bytes)
        .await
        .map_err(|source| DownloadError::Io {
            operation: "write verified output",
            source,
        })?;
    Ok(DownloadReport {
        info_hash: metainfo.info_hash,
        piece_hash: piece.hash,
        bytes_written,
    })
}
