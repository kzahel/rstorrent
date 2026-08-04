use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rstorrent_protocol::metadata::{
    MetadataError, MetadataExtensionUpdate, MetadataMessage, MetadataUpload, MetadataUploadAction,
    UT_METADATA_LOCAL_ID, encode_extension_handshake, encode_metadata_data, encode_metadata_reject,
    metadata_block_count, parse_extension_handshake, parse_metadata_message,
};
use rstorrent_protocol::metainfo::{BEP9_METAINFO_LIMITS, Metainfo, MetainfoError};
use rstorrent_protocol::peer_wire::{
    EXTENSION_PROTOCOL_RESERVED_BIT, EXTENSION_PROTOCOL_RESERVED_INDEX, FrameDecoder, FrameError,
    HANDSHAKE_LENGTH, HandshakeError, PeerMessage, decode_handshake,
    encode_handshake_with_reserved, encode_message,
};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

const METADATA_SEED_PEER_ID: [u8; 20] = *b"-RS0001-METADATASEED";
const NETWORK_READ_LENGTH: usize = 16 * 1024;

#[derive(Clone, Debug)]
pub struct MetadataSeedConfig {
    pub metainfo_path: PathBuf,
    pub listen: SocketAddr,
    pub timeout: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataSeedReport {
    pub listen_address: SocketAddr,
    pub info_hash: [u8; 20],
    pub metadata_size: usize,
    pub block_count: usize,
    pub request_count: usize,
}

#[derive(Debug)]
pub struct MetadataSeedServer {
    listener: TcpListener,
    listen_address: SocketAddr,
    info_hash: [u8; 20],
    metadata: Vec<u8>,
    timeout: Duration,
}

#[derive(Debug)]
pub enum MetadataSeedError {
    NonLoopbackListen(SocketAddr),
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
    Metadata(MetadataError),
    PeerClosed,
    ExtensionProtocolUnsupported,
    RequestBeforeNegotiation,
    UnexpectedMessage(&'static str),
    TimedOut {
        timeout: Duration,
    },
}

impl fmt::Display for MetadataSeedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonLoopbackListen(address) => {
                write!(formatter, "metadata seed address {address} is not loopback")
            }
            Self::InvalidTimeout => write!(formatter, "metadata seed timeout must be nonzero"),
            Self::MetainfoTooLarge { maximum } => {
                write!(formatter, "metainfo exceeds input limit {maximum}")
            }
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::Metainfo(error) => write!(formatter, "metainfo: {error}"),
            Self::Handshake(error) => write!(formatter, "peer handshake: {error}"),
            Self::Frame(error) => write!(formatter, "peer frame: {error}"),
            Self::Metadata(error) => write!(formatter, "metadata protocol: {error}"),
            Self::PeerClosed => write!(formatter, "peer closed before metadata transfer completed"),
            Self::ExtensionProtocolUnsupported => {
                write!(formatter, "peer does not advertise the extension protocol")
            }
            Self::RequestBeforeNegotiation => {
                write!(
                    formatter,
                    "peer requested metadata before advertising its extension ID"
                )
            }
            Self::UnexpectedMessage(message) => {
                write!(
                    formatter,
                    "unexpected peer message during metadata upload: {message}"
                )
            }
            Self::TimedOut { timeout } => {
                write!(
                    formatter,
                    "metadata seed timed out after {}s",
                    timeout.as_secs()
                )
            }
        }
    }
}

impl Error for MetadataSeedError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Metainfo(error) => Some(error),
            Self::Handshake(error) => Some(error),
            Self::Frame(error) => Some(error),
            Self::Metadata(error) => Some(error),
            _ => None,
        }
    }
}

pub async fn bind_metadata_seed(
    config: MetadataSeedConfig,
) -> Result<MetadataSeedServer, MetadataSeedError> {
    if !config.listen.ip().is_loopback() {
        return Err(MetadataSeedError::NonLoopbackListen(config.listen));
    }
    if config.timeout.is_zero() {
        return Err(MetadataSeedError::InvalidTimeout);
    }

    let metainfo_bytes = read_bounded_metainfo(&config.metainfo_path).await?;
    let metainfo = Metainfo::from_bytes_with_limits(&metainfo_bytes, BEP9_METAINFO_LIMITS)
        .map_err(MetadataSeedError::Metainfo)?;
    let metadata = Metainfo::info_bytes_with_limits(&metainfo_bytes, BEP9_METAINFO_LIMITS)
        .map_err(MetadataSeedError::Metainfo)?
        .to_vec();
    let raw_metainfo = Metainfo::from_info_bytes_with_limits(&metadata, BEP9_METAINFO_LIMITS)
        .map_err(MetadataSeedError::Metainfo)?;
    if raw_metainfo != metainfo {
        return Err(MetadataSeedError::Metainfo(MetainfoError::InvalidField(
            "raw info dictionary identity",
        )));
    }

    let listener =
        TcpListener::bind(config.listen)
            .await
            .map_err(|source| MetadataSeedError::Io {
                operation: "bind metadata seed listener",
                source,
            })?;
    let listen_address = listener
        .local_addr()
        .map_err(|source| MetadataSeedError::Io {
            operation: "read metadata seed listener address",
            source,
        })?;
    Ok(MetadataSeedServer {
        listener,
        listen_address,
        info_hash: metainfo.info_hash,
        metadata,
        timeout: config.timeout,
    })
}

impl MetadataSeedServer {
    pub fn listen_address(&self) -> SocketAddr {
        self.listen_address
    }

    pub fn info_hash(&self) -> [u8; 20] {
        self.info_hash
    }

    pub fn metadata_size(&self) -> usize {
        self.metadata.len()
    }

    pub async fn serve(self) -> Result<MetadataSeedReport, MetadataSeedError> {
        let configured_timeout = self.timeout;
        timeout(configured_timeout, self.serve_inner())
            .await
            .map_err(|_| MetadataSeedError::TimedOut {
                timeout: configured_timeout,
            })?
    }

    async fn serve_inner(self) -> Result<MetadataSeedReport, MetadataSeedError> {
        let (stream, address) =
            self.listener
                .accept()
                .await
                .map_err(|source| MetadataSeedError::Io {
                    operation: "accept metadata peer",
                    source,
                })?;
        if !address.ip().is_loopback() {
            return Err(MetadataSeedError::NonLoopbackListen(address));
        }
        let mut peer = SeedPeer::handshake(stream, self.info_hash).await?;
        send_message(
            &mut peer.stream,
            &PeerMessage::Extended {
                id: 0,
                payload: encode_extension_handshake(Some(self.metadata.len())),
            },
        )
        .await?;

        let metadata_size = self.metadata.len();
        let block_count = metadata_block_count(metadata_size);
        let mut upload =
            MetadataUpload::new(&self.metadata).map_err(MetadataSeedError::Metadata)?;
        let mut remote_metadata_id = None;
        loop {
            match peer.next_message().await? {
                PeerMessage::Extended { id: 0, payload } => {
                    let handshake =
                        parse_extension_handshake(&payload).map_err(MetadataSeedError::Metadata)?;
                    match handshake.metadata_extension {
                        MetadataExtensionUpdate::Unchanged => {}
                        MetadataExtensionUpdate::Disabled => remote_metadata_id = None,
                        MetadataExtensionUpdate::Enabled(id) => {
                            remote_metadata_id = Some(id);
                        }
                    }
                }
                PeerMessage::Extended {
                    id: UT_METADATA_LOCAL_ID,
                    payload,
                } => {
                    let message =
                        parse_metadata_message(&payload).map_err(MetadataSeedError::Metadata)?;
                    let piece = match message {
                        MetadataMessage::Request { piece } => piece,
                        MetadataMessage::Unknown { .. } => continue,
                        MetadataMessage::Data { .. } => {
                            return Err(MetadataSeedError::UnexpectedMessage("metadata data"));
                        }
                        MetadataMessage::Reject { .. } => {
                            return Err(MetadataSeedError::UnexpectedMessage("metadata reject"));
                        }
                    };
                    let remote_id =
                        remote_metadata_id.ok_or(MetadataSeedError::RequestBeforeNegotiation)?;
                    let response = upload
                        .on_request(piece)
                        .map_err(MetadataSeedError::Metadata)?;
                    let payload = match response {
                        MetadataUploadAction::Data {
                            piece,
                            total_size,
                            block,
                        } => encode_metadata_data(piece, total_size, &block)
                            .map_err(MetadataSeedError::Metadata)?,
                        MetadataUploadAction::Reject { piece } => encode_metadata_reject(piece),
                    };
                    send_message(
                        &mut peer.stream,
                        &PeerMessage::Extended {
                            id: remote_id,
                            payload,
                        },
                    )
                    .await?;
                    if upload.is_complete() {
                        return Ok(MetadataSeedReport {
                            listen_address: self.listen_address,
                            info_hash: self.info_hash,
                            metadata_size,
                            block_count,
                            request_count: upload.request_count(),
                        });
                    }
                }
                PeerMessage::Extended { .. }
                | PeerMessage::KeepAlive
                | PeerMessage::Choke
                | PeerMessage::Unchoke
                | PeerMessage::Interested
                | PeerMessage::NotInterested
                | PeerMessage::Have(_)
                | PeerMessage::Bitfield(_) => {}
                PeerMessage::Request(_) | PeerMessage::Cancel(_) => {
                    return Err(MetadataSeedError::UnexpectedMessage("payload request"));
                }
                PeerMessage::Piece { .. } => {
                    return Err(MetadataSeedError::UnexpectedMessage("payload data"));
                }
            }
        }
    }
}

#[derive(Debug)]
struct SeedPeer {
    stream: TcpStream,
    decoder: FrameDecoder,
    queued_messages: VecDeque<PeerMessage>,
}

impl SeedPeer {
    async fn handshake(
        mut stream: TcpStream,
        info_hash: [u8; 20],
    ) -> Result<Self, MetadataSeedError> {
        let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake_bytes)
            .await
            .map_err(|source| MetadataSeedError::Io {
                operation: "read metadata peer handshake",
                source,
            })?;
        let handshake =
            decode_handshake(&handshake_bytes, info_hash).map_err(MetadataSeedError::Handshake)?;
        if !handshake.supports_extensions() {
            return Err(MetadataSeedError::ExtensionProtocolUnsupported);
        }
        let mut reserved = [0; 8];
        reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
        stream
            .write_all(&encode_handshake_with_reserved(
                info_hash,
                METADATA_SEED_PEER_ID,
                reserved,
            ))
            .await
            .map_err(|source| MetadataSeedError::Io {
                operation: "send metadata seed handshake",
                source,
            })?;
        Ok(Self {
            stream,
            decoder: FrameDecoder::new(),
            queued_messages: VecDeque::new(),
        })
    }

    async fn next_message(&mut self) -> Result<PeerMessage, MetadataSeedError> {
        while self.queued_messages.is_empty() {
            let mut buffer = [0; NETWORK_READ_LENGTH];
            let read =
                self.stream
                    .read(&mut buffer)
                    .await
                    .map_err(|source| MetadataSeedError::Io {
                        operation: "read metadata peer message",
                        source,
                    })?;
            if read == 0 {
                return Err(MetadataSeedError::PeerClosed);
            }
            self.queued_messages.extend(
                self.decoder
                    .push(&buffer[..read])
                    .map_err(MetadataSeedError::Frame)?,
            );
        }
        Ok(self
            .queued_messages
            .pop_front()
            .expect("metadata peer queue is nonempty after receive loop"))
    }
}

async fn send_message(
    stream: &mut TcpStream,
    message: &PeerMessage,
) -> Result<(), MetadataSeedError> {
    let frame = encode_message(message).map_err(MetadataSeedError::Frame)?;
    stream
        .write_all(&frame)
        .await
        .map_err(|source| MetadataSeedError::Io {
            operation: "send metadata peer message",
            source,
        })
}

async fn read_bounded_metainfo(path: &Path) -> Result<Vec<u8>, MetadataSeedError> {
    let file = File::open(path)
        .await
        .map_err(|source| MetadataSeedError::Io {
            operation: "open metadata seed metainfo",
            source,
        })?;
    let mut bytes = Vec::new();
    file.take((BEP9_METAINFO_LIMITS.max_outer_bytes + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .map_err(|source| MetadataSeedError::Io {
            operation: "read metadata seed metainfo",
            source,
        })?;
    if bytes.len() > BEP9_METAINFO_LIMITS.max_outer_bytes {
        return Err(MetadataSeedError::MetainfoTooLarge {
            maximum: BEP9_METAINFO_LIMITS.max_outer_bytes,
        });
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, VecDeque};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use rstorrent_protocol::metadata::{
        MetadataExtensionUpdate, MetadataMessage, encode_extension_handshake_with_id,
        encode_metadata_request, metadata_block_count, parse_extension_handshake,
        parse_metadata_message,
    };
    use rstorrent_protocol::peer_wire::{
        EXTENSION_PROTOCOL_RESERVED_BIT, EXTENSION_PROTOCOL_RESERVED_INDEX, FrameDecoder,
        HANDSHAKE_LENGTH, PeerMessage, decode_handshake, encode_handshake_with_reserved,
        encode_message,
    };
    use sha1::{Digest, Sha1};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    use super::{MetadataSeedConfig, MetadataSeedError, bind_metadata_seed};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn test_path() -> PathBuf {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rstorrent-metadata-seed-{}-{sequence}.torrent",
            std::process::id()
        ))
    }

    fn multi_block_metainfo() -> (Vec<u8>, Vec<u8>) {
        let mut info = b"d5:filesl".to_vec();
        for index in 0..120 {
            let name = format!("{index:03}-{}", "a".repeat(176));
            info.extend_from_slice(
                format!("d6:lengthi1e4:pathl{}:{name}ee", name.len()).as_bytes(),
            );
        }
        info.extend_from_slice(b"e4:name4:root12:piece lengthi16384e6:pieces20:");
        info.extend_from_slice(&[7; 20]);
        info.push(b'e');
        let mut metainfo = b"d4:info".to_vec();
        metainfo.extend_from_slice(&info);
        metainfo.push(b'e');
        (metainfo, info)
    }

    async fn next_message(
        stream: &mut TcpStream,
        decoder: &mut FrameDecoder,
        queued: &mut VecDeque<PeerMessage>,
    ) -> PeerMessage {
        while queued.is_empty() {
            let mut buffer = [0; 16 * 1024];
            let read = stream.read(&mut buffer).await.expect("read seed response");
            assert_ne!(read, 0, "seed closed before response");
            queued.extend(decoder.push(&buffer[..read]).expect("decode seed response"));
        }
        queued.pop_front().expect("queued seed response")
    }

    async fn send(stream: &mut TcpStream, message: &PeerMessage) {
        stream
            .write_all(&encode_message(message).expect("encode client message"))
            .await
            .expect("send client message");
    }

    #[tokio::test]
    async fn serves_multiblock_metadata_using_the_clients_directional_id() {
        let (metainfo, info) = multi_block_metainfo();
        assert!(info.len() > 16 * 1024);
        let info_hash: [u8; 20] = Sha1::digest(&info).into();
        let metainfo_path = test_path();
        tokio::fs::write(&metainfo_path, metainfo)
            .await
            .expect("write metainfo");
        let server = bind_metadata_seed(MetadataSeedConfig {
            metainfo_path: metainfo_path.clone(),
            listen: "127.0.0.1:0".parse().expect("loopback address"),
            timeout: Duration::from_secs(2),
        })
        .await
        .expect("bind validated seed");
        assert_eq!(server.info_hash(), info_hash);
        assert_eq!(server.metadata_size(), info.len());
        let address = server.listen_address();
        let server_task = tokio::spawn(server.serve());

        let mut stream = TcpStream::connect(address).await.expect("connect seed");
        let mut reserved = [0; 8];
        reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
        stream
            .write_all(&encode_handshake_with_reserved(
                info_hash,
                *b"-RS-CLIENT-000000000",
                reserved,
            ))
            .await
            .expect("send client handshake");
        let mut server_handshake = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut server_handshake)
            .await
            .expect("read server handshake");
        assert!(
            decode_handshake(&server_handshake, info_hash)
                .expect("valid server handshake")
                .supports_extensions()
        );

        let mut decoder = FrameDecoder::new();
        let mut queued = VecDeque::new();
        let PeerMessage::Extended {
            id: 0,
            payload: handshake,
        } = next_message(&mut stream, &mut decoder, &mut queued).await
        else {
            panic!("expected server extension handshake");
        };
        let handshake = parse_extension_handshake(&handshake).expect("parse server extensions");
        assert_eq!(
            handshake.metadata_extension,
            MetadataExtensionUpdate::Enabled(1)
        );
        assert_eq!(handshake.metadata_size, Some(info.len()));

        send(
            &mut stream,
            &PeerMessage::Extended {
                id: 0,
                payload: encode_extension_handshake_with_id(7, None)
                    .expect("client directional ID"),
            },
        )
        .await;
        for (payload, expected_piece) in [
            (b"d8:msg_typei0e5:piecei-1ee".to_vec(), -1),
            (encode_metadata_request(99), 99),
        ] {
            send(&mut stream, &PeerMessage::Extended { id: 1, payload }).await;
            let PeerMessage::Extended { id, payload } =
                next_message(&mut stream, &mut decoder, &mut queued).await
            else {
                panic!("expected metadata reject");
            };
            assert_eq!(id, 7);
            assert_eq!(
                parse_metadata_message(&payload).expect("parse metadata reject"),
                MetadataMessage::Reject {
                    piece: expected_piece
                }
            );
        }
        let mut blocks = BTreeMap::new();
        for piece in 0..metadata_block_count(info.len()) {
            send(
                &mut stream,
                &PeerMessage::Extended {
                    id: 1,
                    payload: encode_metadata_request(piece as u32),
                },
            )
            .await;
            let PeerMessage::Extended { id, payload } =
                next_message(&mut stream, &mut decoder, &mut queued).await
            else {
                panic!("expected metadata data");
            };
            assert_eq!(id, 7);
            let MetadataMessage::Data {
                piece,
                total_size,
                block,
            } = parse_metadata_message(&payload).expect("parse metadata data")
            else {
                panic!("expected metadata data dictionary");
            };
            assert_eq!(total_size, info.len());
            blocks.insert(piece, block.to_vec());
        }
        let assembled = blocks.into_values().flatten().collect::<Vec<_>>();
        assert_eq!(assembled, info);
        assert_eq!(<[u8; 20]>::from(Sha1::digest(&assembled)), info_hash);

        let report = server_task.await.expect("seed task").expect("seed report");
        assert_eq!(report.block_count, metadata_block_count(info.len()));
        assert_eq!(report.request_count, report.block_count + 2);
        let _ = tokio::fs::remove_file(metainfo_path).await;
    }

    #[tokio::test]
    async fn request_before_directional_mapping_is_terminal() {
        let (metainfo, info) = multi_block_metainfo();
        let info_hash: [u8; 20] = Sha1::digest(&info).into();
        let metainfo_path = test_path();
        tokio::fs::write(&metainfo_path, metainfo)
            .await
            .expect("write metainfo");
        let server = bind_metadata_seed(MetadataSeedConfig {
            metainfo_path: metainfo_path.clone(),
            listen: "127.0.0.1:0".parse().expect("loopback address"),
            timeout: Duration::from_secs(2),
        })
        .await
        .expect("bind validated seed");
        let address = server.listen_address();
        let server_task = tokio::spawn(server.serve());

        let mut stream = TcpStream::connect(address).await.expect("connect seed");
        let mut reserved = [0; 8];
        reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
        stream
            .write_all(&encode_handshake_with_reserved(
                info_hash,
                *b"-RS-EARLY-0000000000",
                reserved,
            ))
            .await
            .expect("send client handshake");
        let mut server_handshake = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut server_handshake)
            .await
            .expect("read server handshake");
        let mut decoder = FrameDecoder::new();
        let mut queued = VecDeque::new();
        assert!(matches!(
            next_message(&mut stream, &mut decoder, &mut queued).await,
            PeerMessage::Extended { id: 0, .. }
        ));
        send(
            &mut stream,
            &PeerMessage::Extended {
                id: 1,
                payload: encode_metadata_request(0),
            },
        )
        .await;

        assert!(matches!(
            server_task.await.expect("seed task"),
            Err(MetadataSeedError::RequestBeforeNegotiation)
        ));
        let _ = tokio::fs::remove_file(metainfo_path).await;
    }

    #[tokio::test]
    async fn listener_timeout_is_terminal() {
        let (metainfo, _) = multi_block_metainfo();
        let metainfo_path = test_path();
        tokio::fs::write(&metainfo_path, metainfo)
            .await
            .expect("write metainfo");
        let server = bind_metadata_seed(MetadataSeedConfig {
            metainfo_path: metainfo_path.clone(),
            listen: "127.0.0.1:0".parse().expect("loopback address"),
            timeout: Duration::from_millis(10),
        })
        .await
        .expect("bind validated seed");

        assert!(matches!(
            server.serve().await,
            Err(MetadataSeedError::TimedOut { .. })
        ));
        let _ = tokio::fs::remove_file(metainfo_path).await;
    }

    #[tokio::test]
    async fn peer_disconnect_is_terminal() {
        let (metainfo, info) = multi_block_metainfo();
        let info_hash: [u8; 20] = Sha1::digest(&info).into();
        let metainfo_path = test_path();
        tokio::fs::write(&metainfo_path, metainfo)
            .await
            .expect("write metainfo");
        let server = bind_metadata_seed(MetadataSeedConfig {
            metainfo_path: metainfo_path.clone(),
            listen: "127.0.0.1:0".parse().expect("loopback address"),
            timeout: Duration::from_secs(2),
        })
        .await
        .expect("bind validated seed");
        let address = server.listen_address();
        let server_task = tokio::spawn(server.serve());

        let mut stream = TcpStream::connect(address).await.expect("connect seed");
        let mut reserved = [0; 8];
        reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
        stream
            .write_all(&encode_handshake_with_reserved(
                info_hash,
                *b"-RS-DROP--0000000000",
                reserved,
            ))
            .await
            .expect("send client handshake");
        let mut server_handshake = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut server_handshake)
            .await
            .expect("read server handshake");
        drop(stream);

        let result = server_task.await.expect("seed task");
        assert!(
            matches!(
                result,
                Err(MetadataSeedError::PeerClosed | MetadataSeedError::Io { .. })
            ),
            "{result:?}"
        );
        let _ = tokio::fs::remove_file(metainfo_path).await;
    }
}
