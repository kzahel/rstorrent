use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rstorrent_protocol::dht::{
    DhtEndpoint, DhtIp, Message as DhtMessage, NodeId, Query as DhtQuery, Want,
    decode_message as decode_dht, encode_response as encode_dht_response,
};
use rstorrent_protocol::magnet::Magnet;
use rstorrent_protocol::metadata::{
    MetadataMessage, UT_METADATA_LOCAL_ID, encode_extension_handshake, encode_metadata_data,
    encode_metadata_reject, parse_metadata_message,
};
use rstorrent_protocol::metainfo::{BEP9_METAINFO_LIMITS, Metainfo, MetainfoError};
use rstorrent_protocol::peer_wire::{
    EXTENSION_PROTOCOL_RESERVED_BIT, EXTENSION_PROTOCOL_RESERVED_INDEX, HANDSHAKE_LENGTH,
    PeerMessage, decode_handshake, encode_handshake, encode_handshake_with_reserved,
    encode_message,
};
use rstorrent_protocol::piece::MIN_PAYLOAD_ALLOWANCE;
use rstorrent_protocol::storage_layout::{FileSelection, LayoutError, TorrentLayout};
use rstorrent_protocol::udp_tracker::AnnounceEvent;
use sha1::{Digest, Sha1};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::{Barrier, Notify, Semaphore, mpsc};
use tokio::time::{sleep, timeout};

use super::{
    CHECKPOINT_MAX_DIRTY_BYTES, CLIENT_PEER_ID, CONTENT_STORAGE_HASH_CONCURRENCY,
    CONTENT_STORAGE_WRITE_BATCH_BLOCKS, CONTENT_STORAGE_WRITE_BATCH_BYTES,
    CONTENT_STORAGE_WRITE_CONCURRENCY, CoalescedContentWrite, ContentCheckpointPipeline,
    ContentDownloadConfig, ContentStorage, ContentStorageCommand, ContentStorageCompletion,
    ContentStoragePipeline, ContentSupervisorOwner, ContentWriteStats, DhtRetryTiming,
    DiskPressure, DownloadActivityEvent, DownloadActivitySink, DownloadConfig, DownloadControl,
    DownloadError, DownloadResourceLimits, MAX_CONCURRENT_TRACKER_OPERATIONS,
    MAX_DIAGNOSTIC_ERROR_LENGTH, MAX_METADATA_PEERS, MAX_RECENT_METADATA_ATTEMPTS,
    MagnetDownloadConfig, MetadataAcquisitionPhase, MetadataPeerStage, PeerConnection,
    PreparedContentWrite, QueuedContentStorageCommand, ResumableMagnetDownloadConfig,
    ResumeArtifactState, SwarmConfig, TorrentPeerCoordinator, TrackerManager, UdpTrackerAnnounce,
    UdpTrackerExchange, UdpTrackerTiming, UdpTrackerTokenCache, announce_udp_tracker_address,
    atomic_saturating_add, atomic_saturating_increment, build_content_plan_window,
    coalesce_content_writes, collect_content_write_batch, content_dial_slot_available,
    content_storage_job_limit, download_magnet, download_magnet_metadata_with_control,
    download_magnet_metadata_with_dht, download_magnet_with_control, download_verified_piece,
    download_verified_piece_with_control, execute_content_storage_verification,
    execute_content_storage_writes, next_peer_message, resume_magnet, resume_magnet_with_control,
    retrying_dht_lookup, run_content_download, run_magnet_download_with_peers, send_message,
};

trait TestMetainfoParse: Sized {
    fn from_bytes(bytes: &[u8]) -> Result<Self, MetainfoError>;
    fn from_info_bytes(bytes: &[u8]) -> Result<Self, MetainfoError>;
}

impl TestMetainfoParse for Metainfo {
    fn from_bytes(bytes: &[u8]) -> Result<Self, MetainfoError> {
        Self::from_bytes_with_limits(bytes, BEP9_METAINFO_LIMITS)
    }

    fn from_info_bytes(bytes: &[u8]) -> Result<Self, MetainfoError> {
        Self::from_info_bytes_with_limits(bytes, BEP9_METAINFO_LIMITS)
    }
}
use crate::checkpoint::{CheckpointBatch, CheckpointIntent, DurabilityTarget};
use crate::dht::{BootstrapNode, DhtConfig, DhtService};
use crate::network::{NetworkConfig, NetworkPolicy};
use crate::peer::{
    DialAttempt, PeerEndpoint, PeerFailure, PeerObservation, PeerPhase, PeerRegistry,
    PeerRegistryConfig, PeerSelectionContext, PeerSelector, PeerSource,
};
use crate::peer_runtime::PeerConnectionLifecycle;
use crate::selective_storage::{
    CheckpointFileReference, CheckpointHandles, SelectiveStorage, SelectiveStorageError,
    selective_part_path, selective_staging_path, selective_staging_path as staging_path,
    torrent_storage_paths_for_metainfo,
};
use crate::storage_file_pool::StorageFileLease;
use crate::swarm::{
    BlockKey, DEFAULT_INITIAL_REQUESTS_PER_CONNECTION, DEFAULT_MAX_ESTABLISHED_CONNECTIONS,
    DEFAULT_MAX_PENDING_DIALS, PieceGeneration,
};
use crate::{ByteMetric, ByteMetricSink, DiskCheckpointStage, DiskPieceStage};

static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
struct RecordingCheckpointSink {
    batches: Mutex<Vec<Vec<usize>>>,
    rechecks: Mutex<Vec<Vec<bool>>>,
    failure: Mutex<Option<String>>,
}

impl RecordingCheckpointSink {
    fn failing(detail: &str) -> Self {
        Self {
            batches: Mutex::new(Vec::new()),
            rechecks: Mutex::new(Vec::new()),
            failure: Mutex::new(Some(detail.to_owned())),
        }
    }

    fn batches(&self) -> Vec<Vec<usize>> {
        self.batches
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn rechecks(&self) -> Vec<Vec<bool>> {
        self.rechecks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl super::DownloadCheckpointSink for RecordingCheckpointSink {
    fn metadata_verified(&self, _raw_info: &[u8]) -> Result<(), String> {
        Ok(())
    }

    fn storage_prepared(&self, _storage: super::ResumedStorage) -> Result<(), String> {
        Ok(())
    }

    fn recheck_started(&self) -> Result<(), String> {
        Ok(())
    }

    fn have_rechecked(&self, verified_pieces: &[bool]) -> Result<(), String> {
        self.rechecks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(verified_pieces.to_vec());
        Ok(())
    }

    fn pieces_durable(&self, piece_indices: &[usize]) -> Result<(), String> {
        self.batches
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(piece_indices.to_vec());
        self.failure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            .map_or(Ok(()), Err)
    }

    fn descriptor_prepared(&self, _files: &[super::PreparedFileHash]) -> Result<(), String> {
        Ok(())
    }

    fn publication_prepared(&self) -> Result<(), String> {
        Ok(())
    }

    fn published(&self) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum PublicationFailurePoint {
    AfterIntent,
    AfterRename,
}

struct PublicationFailureSink {
    point: PublicationFailurePoint,
    rechecked: Mutex<Vec<bool>>,
}

impl super::DownloadCheckpointSink for PublicationFailureSink {
    fn metadata_verified(&self, _raw_info: &[u8]) -> Result<(), String> {
        Ok(())
    }

    fn storage_prepared(&self, _storage: super::ResumedStorage) -> Result<(), String> {
        Ok(())
    }

    fn recheck_started(&self) -> Result<(), String> {
        Ok(())
    }

    fn have_rechecked(&self, verified_pieces: &[bool]) -> Result<(), String> {
        *self
            .rechecked
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = verified_pieces.to_vec();
        Ok(())
    }

    fn pieces_durable(&self, _piece_indices: &[usize]) -> Result<(), String> {
        Ok(())
    }

    fn descriptor_prepared(&self, _files: &[super::PreparedFileHash]) -> Result<(), String> {
        Ok(())
    }

    fn publication_prepared(&self) -> Result<(), String> {
        match self.point {
            PublicationFailurePoint::AfterIntent => {
                Err("injected death after publication intent".to_owned())
            }
            PublicationFailurePoint::AfterRename => Ok(()),
        }
    }

    fn published(&self) -> Result<(), String> {
        match self.point {
            PublicationFailurePoint::AfterIntent => Ok(()),
            PublicationFailurePoint::AfterRename => {
                Err("injected death after publication rename".to_owned())
            }
        }
    }
}

async fn wait_for_checkpoint_stage(control: &DownloadControl, expected: DiskCheckpointStage) {
    timeout(Duration::from_secs(2), async {
        loop {
            if control.disk_snapshot().checkpoint_stage == expected {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("checkpoint reached expected stage");
}

fn checkpoint_sync_handle(name: &str) -> (PathBuf, CheckpointHandles) {
    let path = test_path(name);
    let file = std::fs::OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&path)
        .expect("create checkpoint sync target");
    let handle = Arc::new(std::sync::OnceLock::new());
    handle
        .set(CheckpointFileReference::Fixed(StorageFileLease::fixed(
            file,
        )))
        .expect("new checkpoint test cell is empty");
    (path, BTreeMap::from([(DurabilityTarget::PartFile, handle)]))
}

fn prepared_write(piece: u32, begin: u32, bytes: &[u8]) -> PreparedContentWrite {
    PreparedContentWrite {
        block: BlockKey::new(piece, begin, bytes.len() as u32).expect("test block"),
        generation: PieceGeneration::new(1).expect("generation"),
        offset: u64::from(piece) * 1024 + u64::from(begin),
        bytes: bytes.to_vec(),
        stats: ContentWriteStats {
            selected_bytes: bytes.len(),
            part_bytes: 0,
        },
    }
}

fn queued_write(piece: u32, begin: u32, length: usize) -> QueuedContentStorageCommand {
    QueuedContentStorageCommand {
        enqueued_at: Instant::now(),
        command: ContentStorageCommand::Write {
            block: BlockKey::new(piece, begin, length as u32).expect("test block"),
            generation: PieceGeneration::new(1).expect("generation"),
            offset: u64::from(piece) * 1024 * 1024 + u64::from(begin),
            bytes: vec![piece as u8; length],
        },
    }
}

fn loopback_network(timeout: Duration) -> NetworkConfig {
    NetworkConfig::new(NetworkPolicy::LoopbackOnly, timeout, timeout)
}

fn resource_limits(bytes: usize) -> DownloadResourceLimits {
    DownloadResourceLimits::new(bytes, bytes, bytes)
}

fn test_path(name: &str) -> PathBuf {
    let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "rstorrent-driver-test-{}-{sequence}-{name}",
        std::process::id()
    ))
}

async fn single_file_content_storage(
    output: PathBuf,
    length: usize,
    piece_length: usize,
) -> ContentStorage {
    let metainfo = Metainfo::from_info_bytes(&single_file_info_with_piece_length(
        &vec![0; length],
        piece_length,
    ))
    .expect("single-file storage metainfo");
    let layout = TorrentLayout::from_metainfo(&metainfo);
    let selection = FileSelection::new(&layout, &[]).expect("all files wanted");
    ContentStorage(Box::new(
        SelectiveStorage::create(output, &metainfo, layout, selection)
            .await
            .expect("create unified torrent storage"),
    ))
}

fn test_dial_attempt() -> DialAttempt {
    let endpoint = PeerEndpoint::new("127.0.0.1:6881".parse().expect("test endpoint"))
        .expect("valid test endpoint");
    let mut registry =
        PeerRegistry::new(PeerRegistryConfig::default()).expect("test peer registry");
    registry
        .observe(
            PeerObservation::dialable(endpoint, PeerSource::Manual),
            Duration::ZERO,
        )
        .expect("test observation");
    let context = PeerSelectionContext {
        now: Duration::ZERO,
    };
    let candidate = PeerSelector
        .select(&registry, context)
        .expect("test candidate");
    registry
        .begin_dial(candidate, context)
        .expect("test dial attempt")
}

async fn connected_pair(io_timeout: Duration) -> (PeerConnection, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind peer message test");
    let address = listener.local_addr().expect("peer message address");
    let client = TcpStream::connect(address)
        .await
        .expect("connect peer message client");
    let (server, _) = listener.accept().await.expect("accept peer message client");
    (
        PeerConnection::for_test(test_dial_attempt(), client, io_timeout),
        server,
    )
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

fn two_piece_metainfo(first: &[u8], second: &[u8]) -> Vec<u8> {
    assert_eq!(first.len(), 16 * 1024);
    assert_eq!(second.len(), 16 * 1024);
    let mut metainfo = format!(
        "d4:infod5:filesld6:lengthi{}e4:pathl1:aeed6:lengthi{}e4:pathl1:beee\
             4:name7:fixture12:piece lengthi16384e6:pieces40:",
        first.len(),
        second.len()
    )
    .into_bytes();
    metainfo.extend_from_slice(&Sha1::digest(first));
    metainfo.extend_from_slice(&Sha1::digest(second));
    metainfo.extend_from_slice(b"ee");
    metainfo
}

async fn serve_content_peer(
    listener: TcpListener,
    info_hash: [u8; 20],
    pieces: Arc<Vec<Vec<u8>>>,
    available: Vec<bool>,
) {
    serve_content_peer_with_timeout(
        listener,
        info_hash,
        pieces,
        available,
        Duration::from_secs(2),
    )
    .await;
}

async fn serve_content_peer_with_timeout(
    listener: TcpListener,
    info_hash: [u8; 20],
    pieces: Arc<Vec<Vec<u8>>>,
    available: Vec<bool>,
    io_timeout: Duration,
) {
    serve_content_peer_recording(listener, info_hash, pieces, available, io_timeout, None).await;
}

async fn serve_content_peer_recording(
    listener: TcpListener,
    info_hash: [u8; 20],
    pieces: Arc<Vec<Vec<u8>>>,
    available: Vec<bool>,
    io_timeout: Duration,
    requested_pieces: Option<mpsc::UnboundedSender<u32>>,
) {
    let (mut stream, _) = listener.accept().await.expect("accept content peer");
    let mut handshake = [0; HANDSHAKE_LENGTH];
    stream
        .read_exact(&mut handshake)
        .await
        .expect("read content handshake");
    decode_handshake(&handshake, info_hash).expect("valid content handshake");
    stream
        .write_all(&encode_handshake(info_hash, *b"-RS-SPLIT-0000000000"))
        .await
        .expect("send content handshake");
    let mut peer = PeerConnection::for_test(test_dial_attempt(), stream, io_timeout);
    let mut bitfield = vec![0_u8; available.len().div_ceil(8)];
    for (piece, present) in available.iter().enumerate() {
        if *present {
            bitfield[piece / 8] |= 1 << (7 - piece % 8);
        }
    }
    send_message(&mut peer, &PeerMessage::Bitfield(bitfield))
        .await
        .expect("send availability");
    send_message(&mut peer, &PeerMessage::Unchoke)
        .await
        .expect("send unchoke");
    loop {
        match next_peer_message(&mut peer).await {
            Ok(PeerMessage::Interested) => {}
            Ok(PeerMessage::Request(request)) => {
                if let Some(requested_pieces) = &requested_pieces {
                    requested_pieces
                        .send(request.index)
                        .expect("record requested piece");
                }
                let piece = request.index as usize;
                assert!(available[piece], "request sent to unavailable peer");
                let begin = request.begin as usize;
                let end = begin + request.length as usize;
                send_message(
                    &mut peer,
                    &PeerMessage::Piece {
                        index: request.index,
                        begin: request.begin,
                        block: pieces[piece][begin..end].to_vec(),
                    },
                )
                .await
                .expect("send content block");
            }
            Ok(PeerMessage::Cancel(_)) => {}
            Err(DownloadError::PeerClosed)
            | Err(DownloadError::Io {
                operation: "read peer message",
                ..
            }) => break,
            Ok(message) => panic!("unexpected content command {message:?}"),
            Err(error) => panic!("content peer failed: {error}"),
        }
    }
}

async fn serve_window_probe_peer(
    listener: TcpListener,
    info_hash: [u8; 20],
    payload: Arc<Vec<u8>>,
    max_pending: Arc<AtomicUsize>,
) {
    let (mut stream, _) = listener.accept().await.expect("accept window peer");
    let mut handshake = [0; HANDSHAKE_LENGTH];
    stream
        .read_exact(&mut handshake)
        .await
        .expect("read window handshake");
    decode_handshake(&handshake, info_hash).expect("valid window handshake");
    stream
        .write_all(&encode_handshake(info_hash, *b"-RS-WINDOW-000000000"))
        .await
        .expect("send window handshake");
    let mut peer = PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(2));
    send_message(&mut peer, &PeerMessage::Bitfield(vec![0x80]))
        .await
        .expect("send window availability");
    send_message(&mut peer, &PeerMessage::Unchoke)
        .await
        .expect("send window unchoke");

    let mut pending = Vec::new();
    while pending.len() < DEFAULT_INITIAL_REQUESTS_PER_CONNECTION {
        match next_peer_message(&mut peer).await {
            Ok(PeerMessage::Interested) => {}
            Ok(PeerMessage::Request(request)) => pending.push(request),
            Ok(PeerMessage::Cancel(_)) => {}
            Ok(message) => panic!("unexpected initial window command {message:?}"),
            Err(error) => panic!("window peer failed before initial requests: {error}"),
        }
    }
    max_pending.fetch_max(pending.len(), Ordering::AcqRel);

    let mut served_bytes = 0;
    while served_bytes < payload.len() {
        while pending.is_empty() {
            match next_peer_message(&mut peer).await {
                Ok(PeerMessage::Request(request)) => pending.push(request),
                Ok(PeerMessage::Interested) => {}
                Ok(PeerMessage::Cancel(_)) => {}
                Ok(message) => panic!("unexpected refill window command {message:?}"),
                Err(error) => panic!("window peer failed while awaiting refill: {error}"),
            }
        }
        let request = pending.remove(0);
        let begin = request.begin as usize;
        let end = begin + request.length as usize;
        send_message(
            &mut peer,
            &PeerMessage::Piece {
                index: request.index,
                begin: request.begin,
                block: payload[begin..end].to_vec(),
            },
        )
        .await
        .expect("send window payload");
        served_bytes += request.length as usize;

        loop {
            match timeout(Duration::from_millis(20), next_peer_message(&mut peer)).await {
                Ok(Ok(PeerMessage::Request(request))) => pending.push(request),
                Ok(Ok(PeerMessage::Interested)) => {}
                Ok(Ok(PeerMessage::Cancel(_))) => {}
                Ok(Err(DownloadError::PeerClosed))
                | Ok(Err(DownloadError::Io {
                    operation: "read peer message",
                    ..
                })) => return,
                Ok(Ok(message)) => panic!("unexpected window command {message:?}"),
                Ok(Err(error)) => panic!("window peer failed: {error}"),
                Err(_) => break,
            }
        }
        max_pending.fetch_max(pending.len(), Ordering::AcqRel);
    }

    loop {
        match next_peer_message(&mut peer).await {
            Ok(PeerMessage::Request(_))
            | Ok(PeerMessage::Cancel(_))
            | Ok(PeerMessage::Interested) => {}
            Err(DownloadError::PeerClosed)
            | Err(DownloadError::Io {
                operation: "read peer message",
                ..
            }) => return,
            Ok(message) => panic!("unexpected final window command {message:?}"),
            Err(error) => panic!("window peer failed after queue drained: {error}"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum AdverseRequestAction {
    Disconnect,
    Choke,
}

async fn serve_adverse_content_peer(
    listener: TcpListener,
    info_hash: [u8; 20],
    action: AdverseRequestAction,
) {
    let (mut stream, _) = listener.accept().await.expect("accept adverse peer");
    let mut handshake = [0; HANDSHAKE_LENGTH];
    stream
        .read_exact(&mut handshake)
        .await
        .expect("read adverse handshake");
    decode_handshake(&handshake, info_hash).expect("valid adverse handshake");
    stream
        .write_all(&encode_handshake(info_hash, *b"-RS-ADVERS-000000000"))
        .await
        .expect("send adverse handshake");
    let mut peer = PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(2));
    send_message(&mut peer, &PeerMessage::Bitfield(vec![0xc0]))
        .await
        .expect("send adverse availability");
    send_message(&mut peer, &PeerMessage::Unchoke)
        .await
        .expect("send adverse unchoke");
    loop {
        match next_peer_message(&mut peer).await {
            Ok(PeerMessage::Interested) => {}
            Ok(PeerMessage::Request(_)) => match action {
                AdverseRequestAction::Disconnect => return,
                AdverseRequestAction::Choke => {
                    send_message(&mut peer, &PeerMessage::Choke)
                        .await
                        .expect("send choke");
                    break;
                }
            },
            Ok(message) => panic!("unexpected adverse command {message:?}"),
            Err(error) => panic!("adverse peer failed before request: {error}"),
        }
    }
    loop {
        match next_peer_message(&mut peer).await {
            Err(DownloadError::PeerClosed)
            | Err(DownloadError::Io {
                operation: "read peer message",
                ..
            }) => return,
            Ok(PeerMessage::Interested) => {}
            Ok(PeerMessage::Request(_)) => {
                // Requests queued before the choke crossed the wire are harmless.
            }
            Ok(PeerMessage::Cancel(_)) => {}
            Ok(message) => panic!("choked peer received command {message:?}"),
            Err(error) => panic!("choked peer failed: {error}"),
        }
    }
}

async fn serve_one_block_then_choke_peer(
    listener: TcpListener,
    info_hash: [u8; 20],
    payload: Arc<Vec<u8>>,
) {
    let (mut stream, _) = listener.accept().await.expect("accept parole peer");
    let mut handshake = [0; HANDSHAKE_LENGTH];
    stream
        .read_exact(&mut handshake)
        .await
        .expect("read parole handshake");
    decode_handshake(&handshake, info_hash).expect("valid parole handshake");
    stream
        .write_all(&encode_handshake(info_hash, *b"-RS-PAROLE-000000000"))
        .await
        .expect("send parole handshake");
    let mut peer = PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(2));
    send_message(&mut peer, &PeerMessage::Bitfield(vec![0x80]))
        .await
        .expect("send parole availability");
    send_message(&mut peer, &PeerMessage::Unchoke)
        .await
        .expect("send parole unchoke");
    let request = loop {
        match next_peer_message(&mut peer).await {
            Ok(PeerMessage::Interested) => {}
            Ok(PeerMessage::Request(request)) => break request,
            Ok(message) => panic!("unexpected parole command {message:?}"),
            Err(error) => panic!("parole peer failed before request: {error}"),
        }
    };
    let begin = request.begin as usize;
    let end = begin + request.length as usize;
    send_message(
        &mut peer,
        &PeerMessage::Piece {
            index: request.index,
            begin: request.begin,
            block: payload[begin..end].to_vec(),
        },
    )
    .await
    .expect("send parole payload");
    send_message(&mut peer, &PeerMessage::Choke)
        .await
        .expect("send parole choke");
    loop {
        match next_peer_message(&mut peer).await {
            Ok(PeerMessage::Interested)
            | Ok(PeerMessage::Request(_))
            | Ok(PeerMessage::Cancel(_)) => {}
            Err(DownloadError::PeerClosed)
            | Err(DownloadError::Io {
                operation: "read peer message",
                ..
            }) => return,
            Ok(message) => panic!("unexpected post-choke command {message:?}"),
            Err(error) => panic!("parole peer failed after choke: {error}"),
        }
    }
}

async fn accept_handshake_without_reply(listener: TcpListener) {
    accept_handshake_without_reply_and_count(listener, None).await;
}

async fn accept_handshake_without_reply_and_count(
    listener: TcpListener,
    accepted: Option<Arc<AtomicUsize>>,
) {
    let (mut stream, _) = listener.accept().await.expect("accept silent peer");
    let mut handshake = [0; HANDSHAKE_LENGTH];
    stream
        .read_exact(&mut handshake)
        .await
        .expect("read silent handshake");
    if let Some(accepted) = accepted {
        accepted.fetch_add(1, Ordering::AcqRel);
    }
    let mut end = [0; 1];
    assert_eq!(stream.read(&mut end).await.expect("wait for close"), 0);
}

async fn serve_permanently_choked_peer(
    listener: TcpListener,
    info_hash: [u8; 20],
    bitfield: Vec<u8>,
) {
    let (mut stream, _) = listener.accept().await.expect("accept choked peer");
    let mut handshake = [0; HANDSHAKE_LENGTH];
    stream
        .read_exact(&mut handshake)
        .await
        .expect("read choked handshake");
    decode_handshake(&handshake, info_hash).expect("valid choked handshake");
    stream
        .write_all(&encode_handshake(info_hash, *b"-RS-CHOKED-000000000"))
        .await
        .expect("send choked handshake");
    let mut peer = PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(2));
    send_message(&mut peer, &PeerMessage::Bitfield(bitfield))
        .await
        .expect("send choked availability");
    loop {
        match next_peer_message(&mut peer).await {
            Ok(PeerMessage::Interested) => {}
            Err(DownloadError::PeerClosed)
            | Err(DownloadError::Io {
                operation: "read peer message",
                ..
            }) => return,
            Ok(message) => panic!("unexpected command for choked peer {message:?}"),
            Err(error) => panic!("choked peer failed: {error}"),
        }
    }
}

async fn prepare_endgame_peer(
    listener: TcpListener,
    info_hash: [u8; 20],
) -> (PeerConnection, rstorrent_protocol::peer_wire::BlockRequest) {
    let (mut stream, _) = listener.accept().await.expect("accept endgame peer");
    let mut handshake = [0; HANDSHAKE_LENGTH];
    stream
        .read_exact(&mut handshake)
        .await
        .expect("read endgame handshake");
    decode_handshake(&handshake, info_hash).expect("valid endgame handshake");
    stream
        .write_all(&encode_handshake(info_hash, *b"-RS-ENDGAME-00000000"))
        .await
        .expect("send endgame handshake");
    let mut peer = PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(2));
    send_message(&mut peer, &PeerMessage::Bitfield(vec![0x80]))
        .await
        .expect("send endgame availability");
    send_message(&mut peer, &PeerMessage::Unchoke)
        .await
        .expect("send endgame unchoke");
    let request = loop {
        match next_peer_message(&mut peer).await {
            Ok(PeerMessage::Interested) => {}
            Ok(PeerMessage::Request(request)) => break request,
            Ok(message) => panic!("unexpected endgame command {message:?}"),
            Err(error) => panic!("endgame peer failed before request: {error}"),
        }
    };
    (peer, request)
}

async fn serve_endgame_loser(
    listener: TcpListener,
    info_hash: [u8; 20],
    requests_ready: Arc<Barrier>,
) -> (
    rstorrent_protocol::peer_wire::BlockRequest,
    rstorrent_protocol::peer_wire::BlockRequest,
) {
    let (mut peer, request) = prepare_endgame_peer(listener, info_hash).await;
    requests_ready.wait().await;
    let cancel = match next_peer_message(&mut peer).await {
        Ok(PeerMessage::Cancel(cancel)) => cancel,
        Ok(message) => panic!("unexpected command before endgame cancel {message:?}"),
        Err(error) => panic!("endgame loser failed before cancel: {error}"),
    };
    (request, cancel)
}

async fn serve_endgame_winner(
    listener: TcpListener,
    info_hash: [u8; 20],
    payload: Vec<u8>,
    requests_ready: Arc<Barrier>,
) {
    let (mut peer, request) = prepare_endgame_peer(listener, info_hash).await;
    requests_ready.wait().await;
    let begin = request.begin as usize;
    let end = begin + request.length as usize;
    send_message(
        &mut peer,
        &PeerMessage::Piece {
            index: request.index,
            begin: request.begin,
            block: payload[begin..end].to_vec(),
        },
    )
    .await
    .expect("send winning endgame block");
    loop {
        match next_peer_message(&mut peer).await {
            Err(DownloadError::PeerClosed)
            | Err(DownloadError::Io {
                operation: "read peer message",
                ..
            }) => return,
            Ok(PeerMessage::Interested) => {}
            Ok(message) => panic!("unexpected post-win command {message:?}"),
            Err(error) => panic!("endgame winner failed after payload: {error}"),
        }
    }
}

async fn serve_delayed_block_peer(
    listener: TcpListener,
    info_hash: [u8; 20],
    payload: Vec<u8>,
    delay: Duration,
    keepalive_interval: Option<Duration>,
) {
    serve_delayed_block_peer_with_timeout(
        listener,
        info_hash,
        payload,
        delay,
        keepalive_interval,
        Duration::from_secs(2),
    )
    .await;
}

async fn serve_delayed_block_peer_with_timeout(
    listener: TcpListener,
    info_hash: [u8; 20],
    payload: Vec<u8>,
    delay: Duration,
    keepalive_interval: Option<Duration>,
    io_timeout: Duration,
) {
    let (mut stream, _) = listener.accept().await.expect("accept delayed peer");
    let mut handshake = [0; HANDSHAKE_LENGTH];
    stream
        .read_exact(&mut handshake)
        .await
        .expect("read delayed handshake");
    decode_handshake(&handshake, info_hash).expect("valid delayed handshake");
    stream
        .write_all(&encode_handshake(info_hash, *b"-RS-DELAY--000000000"))
        .await
        .expect("send delayed handshake");
    let mut peer = PeerConnection::for_test(test_dial_attempt(), stream, io_timeout);
    send_message(&mut peer, &PeerMessage::Bitfield(vec![0x80]))
        .await
        .expect("send delayed availability");
    send_message(&mut peer, &PeerMessage::Unchoke)
        .await
        .expect("send delayed unchoke");
    let request = loop {
        match next_peer_message(&mut peer).await {
            Ok(PeerMessage::Interested) => {}
            Ok(PeerMessage::Request(request)) => break request,
            Ok(message) => panic!("unexpected delayed command {message:?}"),
            Err(error) => panic!("delayed peer failed before request: {error}"),
        }
    };
    let started = tokio::time::Instant::now();
    if let Some(interval) = keepalive_interval {
        while started.elapsed().saturating_add(interval) < delay {
            tokio::time::sleep(interval).await;
            if send_message(&mut peer, &PeerMessage::KeepAlive)
                .await
                .is_err()
            {
                return;
            }
        }
    }
    tokio::time::sleep(delay.saturating_sub(started.elapsed())).await;
    let begin = request.begin as usize;
    let end = begin + request.length as usize;
    if send_message(
        &mut peer,
        &PeerMessage::Piece {
            index: request.index,
            begin: request.begin,
            block: payload[begin..end].to_vec(),
        },
    )
    .await
    .is_err()
    {
        return;
    }
    loop {
        match next_peer_message(&mut peer).await {
            Err(DownloadError::PeerClosed)
            | Err(DownloadError::Io {
                operation: "read peer message",
                ..
            }) => return,
            Ok(PeerMessage::Request(_))
            | Ok(PeerMessage::Cancel(_))
            | Ok(PeerMessage::Interested) => {}
            Ok(message) => panic!("unexpected post-payload command {message:?}"),
            Err(error) => panic!("delayed peer failed after payload: {error}"),
        }
    }
}

async fn run_adverse_reassignment_case(action: AdverseRequestAction) {
    let first = vec![0x44; 16 * 1024];
    let second = vec![0x99; 16 * 1024];
    let metainfo =
        Metainfo::from_bytes(&two_piece_metainfo(&first, &second)).expect("two-piece metainfo");
    let payload = Arc::new(vec![first, second]);
    let adverse_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind adverse");
    let adverse_address = adverse_listener.local_addr().expect("adverse address");
    let useful_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind useful");
    let useful_address = useful_listener.local_addr().expect("useful address");
    let adverse = tokio::spawn(serve_adverse_content_peer(
        adverse_listener,
        metainfo.info_hash,
        action,
    ));
    let useful = tokio::spawn(serve_content_peer(
        useful_listener,
        metainfo.info_hash,
        payload,
        vec![true, true],
    ));
    let output = test_path(match action {
        AdverseRequestAction::Disconnect => "disconnect-reassignment",
        AdverseRequestAction::Choke => "choke-reassignment",
    });
    let mut peers = TorrentPeerCoordinator::from_endpoint(
        adverse_address,
        PeerSource::Manual,
        loopback_network(Duration::from_secs(2)),
    )
    .expect("peer session");
    peers
        .observe_address(useful_address, PeerSource::Manual)
        .expect("useful peer");
    let report = timeout(
        Duration::from_secs(3),
        run_content_download(
            ContentDownloadConfig {
                output_path: output.clone(),
                max_buffered_payload_bytes: 2 * MIN_PAYLOAD_ALLOWANCE,
                swarm_config: SwarmConfig::for_request_limit(2 * MIN_PAYLOAD_ALLOWANCE),
                skip_files: Vec::new(),
                materialize_files: Vec::new(),
            },
            metainfo,
            DownloadControl::new(),
            None,
            &mut peers,
            None,
        ),
    )
    .await
    .expect("bounded reassignment")
    .expect("reassigned download");
    assert_eq!(report.verified_piece_count, 2);
    timeout(Duration::from_secs(1), adverse)
        .await
        .expect("adverse peer joined")
        .expect("adverse peer task");
    timeout(Duration::from_secs(1), useful)
        .await
        .expect("useful peer joined")
        .expect("useful peer task");
    let _ = tokio::fs::remove_dir_all(output).await;
}

fn single_file_info(payload: &[u8]) -> Vec<u8> {
    single_file_info_with_piece_length(payload, 16 * 1024)
}

fn single_file_info_with_piece_length(payload: &[u8], piece_length: usize) -> Vec<u8> {
    assert!(piece_length > 0);
    let piece_hashes = payload
        .chunks(piece_length)
        .flat_map(|piece| Sha1::digest(piece).to_vec())
        .collect::<Vec<_>>();
    let mut info = format!(
        "d6:lengthi{}e4:name1:x12:piece lengthi{}e6:pieces{}:",
        payload.len(),
        piece_length,
        piece_hashes.len()
    )
    .into_bytes();
    info.extend_from_slice(&piece_hashes);
    info.push(b'e');
    info
}

fn one_entry_multi_file_info(payload: &[u8], piece_length: usize) -> Vec<u8> {
    let piece_hashes = payload
        .chunks(piece_length)
        .flat_map(|piece| Sha1::digest(piece).to_vec())
        .collect::<Vec<_>>();
    let mut info = format!(
        "d5:filesld6:lengthi{}e4:pathl11:payload.bineee4:name5:multi12:piece lengthi{}e6:pieces{}:",
        payload.len(),
        piece_length,
        piece_hashes.len()
    )
    .into_bytes();
    info.extend_from_slice(&piece_hashes);
    info.push(b'e');
    info
}

async fn stage_single_file_payload(
    paths: &crate::selective_storage::TorrentStoragePaths,
    metainfo: &Metainfo,
    payload: &[u8],
) {
    let layout = TorrentLayout::from_metainfo(metainfo);
    let selection = FileSelection::new(&layout, &[]).expect("all files wanted");
    let mut storage = SelectiveStorage::create_with_paths(
        paths.clone(),
        metainfo,
        layout.clone(),
        selection.clone(),
    )
    .await
    .expect("create staged single-file payload");
    for piece_index in 0..layout.piece_count() {
        let piece_index_u32 = u32::try_from(piece_index).expect("bounded piece index");
        let piece_offset = piece_index * layout.piece_length() as usize;
        for request in layout
            .request_ranges(piece_index_u32, &selection)
            .expect("piece request ranges")
        {
            let begin = request.begin as usize;
            storage
                .write_block(
                    piece_index_u32,
                    request.begin,
                    payload[piece_offset + begin..piece_offset + begin + request.length as usize]
                        .to_vec(),
                )
                .await
                .expect("write staged single-file range");
        }
        storage
            .sync_piece(piece_index_u32)
            .await
            .expect("sync staged single-file piece");
        assert_eq!(
            storage
                .hash_piece(piece_index_u32)
                .await
                .expect("hash staged single-file piece"),
            metainfo.piece_hashes[piece_index]
        );
    }
}

fn private_single_file_info(payload: &[u8]) -> Vec<u8> {
    let mut info = single_file_info(payload);
    info.splice(
        info.len() - 1..info.len() - 1,
        b"7:privatei1e".iter().copied(),
    );
    info
}

fn dht_config(bootstrap: SocketAddr) -> DhtConfig {
    DhtConfig {
        network_policy: NetworkPolicy::LoopbackOnly,
        bind_address: "127.0.0.1:0".parse().expect("DHT bind"),
        bootstrap_nodes: vec![BootstrapNode::Address(bootstrap)],
        initial_snapshot: None,
        query_timeout: Duration::from_millis(500),
        lookup_timeout: Duration::from_secs(3),
        bootstrap_retry_interval: Duration::from_secs(1),
        routing_refresh_interval: Duration::from_secs(60),
        read_only: false,
        byte_metric_sink: None,
    }
}

fn test_dht_endpoint(address: SocketAddr) -> DhtEndpoint {
    let port = address.port();
    match address.ip() {
        IpAddr::V4(address) => DhtEndpoint::new(DhtIp::V4(address.octets()), port),
        IpAddr::V6(address) => DhtEndpoint::new(DhtIp::V6(address.octets()), port),
    }
}

async fn serve_dht_peer(socket: UdpSocket, info_hash: [u8; 20], peer: SocketAddr) {
    let mut packet = [0_u8; 1024];
    loop {
        let (length, client) = socket.recv_from(&mut packet).await.expect("DHT query");
        let DhtMessage::Query(query) = decode_dht(&packet[..length]).expect("decode DHT query")
        else {
            continue;
        };
        let peers = match query.query {
            DhtQuery::FindNode { .. } => Vec::new(),
            DhtQuery::GetPeers {
                info_hash: target,
                want,
            } => {
                assert_eq!(target, NodeId(info_hash));
                assert!(want.is_empty() || want.contains(&Want::Ipv4));
                vec![test_dht_endpoint(peer)]
            }
            _ => Vec::new(),
        };
        let done = !peers.is_empty();
        let response = encode_dht_response(
            &query.transaction,
            NodeId([6; 20]),
            &[],
            &peers,
            Some(b"fixture"),
            test_dht_endpoint(client),
        )
        .expect("encode DHT response");
        socket
            .send_to(&response, client)
            .await
            .expect("send DHT response");
        if done {
            break;
        }
    }
}

async fn serve_dht_peer_after_signal(
    socket: UdpSocket,
    info_hash: [u8; 20],
    peer: SocketAddr,
    release: Arc<Notify>,
) {
    let mut packet = [0_u8; 1024];
    loop {
        let (length, client) = socket.recv_from(&mut packet).await.expect("DHT query");
        let DhtMessage::Query(query) = decode_dht(&packet[..length]).expect("decode DHT query")
        else {
            continue;
        };
        let peers = match query.query {
            DhtQuery::FindNode { .. } => Vec::new(),
            DhtQuery::GetPeers {
                info_hash: target,
                want,
            } => {
                assert_eq!(target, NodeId(info_hash));
                assert!(want.is_empty() || want.contains(&Want::Ipv4));
                release.notified().await;
                vec![test_dht_endpoint(peer)]
            }
            _ => Vec::new(),
        };
        let done = !peers.is_empty();
        let response = encode_dht_response(
            &query.transaction,
            NodeId([6; 20]),
            &[],
            &peers,
            Some(b"fixture"),
            test_dht_endpoint(client),
        )
        .expect("encode DHT response");
        socket
            .send_to(&response, client)
            .await
            .expect("send DHT response");
        if done {
            break;
        }
    }
}

async fn serve_dht_peer_after_retry(socket: UdpSocket, info_hash: [u8; 20], peer: SocketAddr) {
    let mut packet = [0_u8; 1024];
    let mut peer_queries = 0_u8;
    loop {
        let (length, client) = socket.recv_from(&mut packet).await.expect("DHT query");
        let DhtMessage::Query(query) = decode_dht(&packet[..length]).expect("decode DHT query")
        else {
            continue;
        };
        let peers = match query.query {
            DhtQuery::GetPeers {
                info_hash: target, ..
            } => {
                assert_eq!(target, NodeId(info_hash));
                peer_queries = peer_queries.saturating_add(1);
                if peer_queries >= 2 {
                    vec![test_dht_endpoint(peer)]
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        };
        let done = !peers.is_empty();
        let response = encode_dht_response(
            &query.transaction,
            NodeId([6; 20]),
            &[],
            &peers,
            Some(b"fixture"),
            test_dht_endpoint(client),
        )
        .expect("encode DHT response");
        socket
            .send_to(&response, client)
            .await
            .expect("send DHT response");
        if done {
            break;
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

async fn serve_metadata_then_piece(
    listener: TcpListener,
    info: Vec<u8>,
    payload: Vec<u8>,
    bitfield: Vec<u8>,
) {
    let (mut stream, _) = listener.accept().await.expect("accept magnet client");
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
    stream
        .read_exact(&mut handshake_bytes)
        .await
        .expect("read client handshake");
    let handshake =
        decode_handshake(&handshake_bytes, info_hash).expect("client handshake identity");
    assert!(handshake.supports_extensions());
    assert_eq!(handshake.peer_id, CLIENT_PEER_ID);
    let mut reserved = [0; 8];
    reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
    stream
        .write_all(&encode_handshake_with_reserved(
            info_hash,
            *b"-RS-TEST-00000000000",
            reserved,
        ))
        .await
        .expect("send server handshake");
    let mut peer = PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(5));

    let PeerMessage::Extended { id: 0, .. } = next_peer_message(&mut peer)
        .await
        .expect("client extension handshake")
    else {
        panic!("expected extension handshake");
    };
    send_message(&mut peer, &PeerMessage::Bitfield(bitfield))
        .await
        .expect("send early bitfield");
    send_message(&mut peer, &PeerMessage::Unchoke)
        .await
        .expect("send early unchoke");
    send_message(
        &mut peer,
        &PeerMessage::Extended {
            id: 0,
            payload: encode_extension_handshake(Some(info.len())),
        },
    )
    .await
    .expect("send extension handshake");

    let request = next_peer_message(&mut peer)
        .await
        .expect("metadata request");
    let PeerMessage::Extended {
        id: 1,
        payload: request,
    } = request
    else {
        panic!("expected metadata extension request");
    };
    assert_eq!(
        parse_metadata_message(&request).expect("parse metadata request"),
        MetadataMessage::Request { piece: 0 }
    );
    send_message(
        &mut peer,
        &PeerMessage::Extended {
            id: 1,
            payload: encode_metadata_data(0, info.len(), &info).expect("encode metadata block"),
        },
    )
    .await
    .expect("send metadata data");

    loop {
        match next_peer_message(&mut peer).await {
            Ok(PeerMessage::Interested) => {}
            Ok(PeerMessage::Request(request)) => {
                assert_eq!(request.index, 0);
                let begin = request.begin as usize;
                let end = begin + request.length as usize;
                send_message(
                    &mut peer,
                    &PeerMessage::Piece {
                        index: 0,
                        begin: request.begin,
                        block: payload[begin..end].to_vec(),
                    },
                )
                .await
                .expect("send payload block");
            }
            Err(DownloadError::PeerClosed) => break,
            Ok(message) => panic!("unexpected content message {message:?}"),
            Err(error) => panic!("scripted peer failed: {error}"),
        }
    }
}

async fn serve_stalled_metadata_peer(
    listener: TcpListener,
    info_hash: [u8; 20],
    metadata_size: usize,
) {
    let (mut stream, _) = listener.accept().await.expect("accept magnet client");
    let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
    stream
        .read_exact(&mut handshake_bytes)
        .await
        .expect("read client handshake");
    assert!(
        decode_handshake(&handshake_bytes, info_hash)
            .expect("client handshake identity")
            .supports_extensions()
    );
    let mut reserved = [0; 8];
    reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
    stream
        .write_all(&encode_handshake_with_reserved(
            info_hash,
            *b"-RS-STALL-0000000000",
            reserved,
        ))
        .await
        .expect("send server handshake");
    let mut peer = PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(6));
    assert!(matches!(
        next_peer_message(&mut peer).await,
        Ok(PeerMessage::Extended { id: 0, .. })
    ));
    send_message(
        &mut peer,
        &PeerMessage::Extended {
            id: 0,
            payload: encode_extension_handshake(Some(metadata_size)),
        },
    )
    .await
    .expect("send extension handshake");
    assert!(matches!(
        next_peer_message(&mut peer).await,
        Ok(PeerMessage::Extended { id: 1, .. })
    ));
    loop {
        match next_peer_message(&mut peer).await {
            Ok(PeerMessage::Extended { id: 1, .. }) => {}
            Err(DownloadError::PeerClosed | DownloadError::PeerTimedOut { .. }) => break,
            Ok(message) => panic!("unexpected stalled metadata message {message:?}"),
            Err(error) => panic!("stalled metadata peer failed: {error}"),
        }
    }
}

async fn serve_partial_metadata_peer(
    listener: TcpListener,
    info: Vec<u8>,
    reject_second_request: bool,
) {
    let (mut stream, _) = listener.accept().await.expect("accept metadata client");
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
    stream
        .read_exact(&mut handshake_bytes)
        .await
        .expect("read client handshake");
    assert!(
        decode_handshake(&handshake_bytes, info_hash)
            .expect("client handshake identity")
            .supports_extensions()
    );
    let mut reserved = [0; 8];
    reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
    stream
        .write_all(&encode_handshake_with_reserved(
            info_hash,
            *b"-RS-PARTIAL-00000000",
            reserved,
        ))
        .await
        .expect("send server handshake");
    let mut peer = PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(2));
    assert!(matches!(
        next_peer_message(&mut peer).await,
        Ok(PeerMessage::Extended { id: 0, .. })
    ));
    send_message(
        &mut peer,
        &PeerMessage::Extended {
            id: 0,
            payload: encode_extension_handshake(Some(info.len())),
        },
    )
    .await
    .expect("send metadata extension handshake");

    let mut request_count = 0;
    loop {
        match next_peer_message(&mut peer).await {
            Ok(PeerMessage::Extended { id: 1, payload }) => {
                let MetadataMessage::Request { piece } =
                    parse_metadata_message(&payload).expect("parse metadata request")
                else {
                    panic!("expected metadata request");
                };
                request_count += 1;
                if reject_second_request && request_count == 2 {
                    send_message(
                        &mut peer,
                        &PeerMessage::Extended {
                            id: 1,
                            payload: encode_metadata_reject(piece),
                        },
                    )
                    .await
                    .expect("reject second metadata request");
                    continue;
                }
                let piece = usize::try_from(piece).expect("nonnegative metadata piece");
                let begin = piece * rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH;
                let end =
                    (begin + rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH).min(info.len());
                send_message(
                    &mut peer,
                    &PeerMessage::Extended {
                        id: 1,
                        payload: encode_metadata_data(piece as u32, info.len(), &info[begin..end])
                            .expect("encode metadata block"),
                    },
                )
                .await
                .expect("send metadata block");
            }
            Err(DownloadError::PeerClosed) => break,
            Ok(message) => panic!("unexpected partial metadata message {message:?}"),
            Err(error) => panic!("partial metadata peer failed: {error}"),
        }
    }
}

async fn serve_metadata_bytes_after_delay(
    listener: TcpListener,
    info_hash: [u8; 20],
    bytes: Vec<u8>,
    extension_delay: Duration,
) {
    let (mut stream, _) = listener.accept().await.expect("accept metadata client");
    let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
    stream
        .read_exact(&mut handshake_bytes)
        .await
        .expect("read client handshake");
    assert!(
        decode_handshake(&handshake_bytes, info_hash)
            .expect("client handshake identity")
            .supports_extensions()
    );
    let mut reserved = [0; 8];
    reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
    stream
        .write_all(&encode_handshake_with_reserved(
            info_hash,
            *b"-RS-SCRIPT-000000000",
            reserved,
        ))
        .await
        .expect("send server handshake");
    let mut peer = PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(3));
    assert!(matches!(
        next_peer_message(&mut peer).await,
        Ok(PeerMessage::Extended { id: 0, .. })
    ));
    tokio::time::sleep(extension_delay).await;
    send_message(
        &mut peer,
        &PeerMessage::Extended {
            id: 0,
            payload: encode_extension_handshake(Some(bytes.len())),
        },
    )
    .await
    .expect("send metadata extension handshake");

    loop {
        match next_peer_message(&mut peer).await {
            Ok(PeerMessage::Extended { id: 1, payload }) => {
                let MetadataMessage::Request { piece } =
                    parse_metadata_message(&payload).expect("parse metadata request")
                else {
                    panic!("expected metadata request");
                };
                let piece = usize::try_from(piece).expect("nonnegative metadata piece");
                let begin = piece * rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH;
                let end =
                    (begin + rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH).min(bytes.len());
                send_message(
                    &mut peer,
                    &PeerMessage::Extended {
                        id: 1,
                        payload: encode_metadata_data(
                            piece as u32,
                            bytes.len(),
                            &bytes[begin..end],
                        )
                        .expect("encode metadata block"),
                    },
                )
                .await
                .expect("send metadata block");
            }
            Err(DownloadError::PeerClosed) => break,
            Ok(message) => panic!("unexpected metadata message {message:?}"),
            Err(error) => panic!("scripted metadata peer failed: {error}"),
        }
    }
}

async fn serve_one_at_a_time_metadata_peer(listener: TcpListener, info: Vec<u8>) {
    let (mut stream, _) = listener.accept().await.expect("accept metadata client");
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
    stream
        .read_exact(&mut handshake_bytes)
        .await
        .expect("read client handshake");
    assert!(
        decode_handshake(&handshake_bytes, info_hash)
            .expect("client handshake identity")
            .supports_extensions()
    );
    let mut reserved = [0; 8];
    reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
    stream
        .write_all(&encode_handshake_with_reserved(
            info_hash,
            *b"-RS-ONE-AT-A-TIME000",
            reserved,
        ))
        .await
        .expect("send server handshake");
    let mut peer = PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(2));
    assert!(matches!(
        next_peer_message(&mut peer).await,
        Ok(PeerMessage::Extended { id: 0, .. })
    ));
    send_message(
        &mut peer,
        &PeerMessage::Extended {
            id: 0,
            payload: encode_extension_handshake(Some(info.len())),
        },
    )
    .await
    .expect("send metadata extension handshake");

    let first = next_peer_message(&mut peer)
        .await
        .expect("first metadata request");
    let PeerMessage::Extended {
        id: 1,
        payload: first,
    } = first
    else {
        panic!("expected first metadata request");
    };
    assert_eq!(
        parse_metadata_message(&first).expect("parse first request"),
        MetadataMessage::Request { piece: 0 }
    );
    assert!(
        timeout(Duration::from_millis(200), next_peer_message(&mut peer))
            .await
            .is_err(),
        "client must not pipeline a second metadata request immediately"
    );
    send_message(
        &mut peer,
        &PeerMessage::Extended {
            id: 1,
            payload: encode_metadata_data(
                0,
                info.len(),
                &info[..rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH],
            )
            .expect("encode first metadata block"),
        },
    )
    .await
    .expect("send first metadata block");

    let second = next_peer_message(&mut peer)
        .await
        .expect("second metadata request after response");
    let PeerMessage::Extended {
        id: 1,
        payload: second,
    } = second
    else {
        panic!("expected second metadata request");
    };
    assert_eq!(
        parse_metadata_message(&second).expect("parse second request"),
        MetadataMessage::Request { piece: 1 }
    );
    send_message(
        &mut peer,
        &PeerMessage::Extended {
            id: 1,
            payload: encode_metadata_data(
                1,
                info.len(),
                &info[rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH..],
            )
            .expect("encode second metadata block"),
        },
    )
    .await
    .expect("send second metadata block");
    assert!(matches!(
        next_peer_message(&mut peer).await,
        Err(DownloadError::PeerClosed)
    ));
}

async fn serve_metadata_peer_without_ut_metadata(listener: TcpListener, info_hash: [u8; 20]) {
    let (mut stream, _) = listener.accept().await.expect("accept magnet client");
    let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
    stream
        .read_exact(&mut handshake_bytes)
        .await
        .expect("read client handshake");
    assert!(
        decode_handshake(&handshake_bytes, info_hash)
            .expect("client handshake identity")
            .supports_extensions()
    );
    let mut reserved = [0; 8];
    reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
    stream
        .write_all(&encode_handshake_with_reserved(
            info_hash,
            *b"-RS-STALL-0000000000",
            reserved,
        ))
        .await
        .expect("send server handshake");
    let mut peer = PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(1));
    assert!(matches!(
        next_peer_message(&mut peer).await,
        Ok(PeerMessage::Extended { id: 0, .. })
    ));
    send_message(
        &mut peer,
        &PeerMessage::Extended {
            id: 0,
            payload: b"d1:mdee".to_vec(),
        },
    )
    .await
    .expect("send extension handshake without ut_metadata");
    assert!(matches!(
        next_peer_message(&mut peer).await,
        Err(DownloadError::PeerClosed)
    ));
}

async fn serve_chattering_peer_without_extension_handshake(
    listener: TcpListener,
    info_hash: [u8; 20],
) {
    let (mut stream, _) = listener.accept().await.expect("accept magnet client");
    let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
    stream
        .read_exact(&mut handshake_bytes)
        .await
        .expect("read client handshake");
    assert!(
        decode_handshake(&handshake_bytes, info_hash)
            .expect("client handshake identity")
            .supports_extensions()
    );
    let mut reserved = [0; 8];
    reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
    stream
        .write_all(&encode_handshake_with_reserved(
            info_hash,
            *b"-RS-STALL-0000000000",
            reserved,
        ))
        .await
        .expect("send server handshake");
    let mut peer = PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(1));
    assert!(matches!(
        next_peer_message(&mut peer).await,
        Ok(PeerMessage::Extended { id: 0, .. })
    ));
    loop {
        tokio::time::sleep(Duration::from_millis(10)).await;
        if send_message(&mut peer, &PeerMessage::KeepAlive)
            .await
            .is_err()
        {
            break;
        }
    }
}

async fn serve_metadata_rejecting_peer(
    listener: TcpListener,
    info_hash: [u8; 20],
    metadata_size: usize,
) {
    let (mut stream, _) = listener.accept().await.expect("accept magnet client");
    let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
    stream
        .read_exact(&mut handshake_bytes)
        .await
        .expect("read client handshake");
    decode_handshake(&handshake_bytes, info_hash).expect("client handshake identity");
    let mut reserved = [0; 8];
    reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
    stream
        .write_all(&encode_handshake_with_reserved(
            info_hash,
            *b"-RS-STALL-0000000000",
            reserved,
        ))
        .await
        .expect("send server handshake");
    let mut peer = PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(1));
    assert!(matches!(
        next_peer_message(&mut peer).await,
        Ok(PeerMessage::Extended { id: 0, .. })
    ));
    send_message(
        &mut peer,
        &PeerMessage::Extended {
            id: 0,
            payload: encode_extension_handshake(Some(metadata_size)),
        },
    )
    .await
    .expect("send metadata extension handshake");
    let message = match next_peer_message(&mut peer).await {
        Ok(message) => message,
        Err(DownloadError::PeerClosed | DownloadError::PeerTimedOut { .. }) => return,
        Err(error) => panic!("rejecting metadata peer failed: {error}"),
    };
    let PeerMessage::Extended { id: 1, payload } = message else {
        panic!("expected metadata request");
    };
    let MetadataMessage::Request { piece } =
        parse_metadata_message(&payload).expect("parse metadata request")
    else {
        panic!("expected metadata request payload");
    };
    send_message(
        &mut peer,
        &PeerMessage::Extended {
            id: 1,
            payload: encode_metadata_reject(piece),
        },
    )
    .await
    .expect("reject metadata request");
    assert!(matches!(
        next_peer_message(&mut peer).await,
        Err(DownloadError::PeerClosed)
    ));
}

async fn serve_one_shot_udp_tracker(
    socket: UdpSocket,
    info_hash: [u8; 20],
    unreachable: SocketAddr,
    reachable: SocketAddr,
    announce_delay: Duration,
) {
    let mut request = [0; 2048];
    let (connect_length, client) = socket
        .recv_from(&mut request)
        .await
        .expect("receive tracker connect");
    assert_eq!(connect_length, 16);
    assert_eq!(
        u64::from_be_bytes(request[0..8].try_into().expect("protocol ID")),
        0x0417_2710_1980
    );
    assert_eq!(
        u32::from_be_bytes(request[8..12].try_into().expect("connect action")),
        0
    );
    let connect_transaction =
        u32::from_be_bytes(request[12..16].try_into().expect("connect transaction"));
    assert_ne!(connect_transaction, 0);

    let connection_id = 0x0102_0304_0506_0708_u64;
    socket
        .send_to(&[0, 1, 2, 3], client)
        .await
        .expect("send undersized unrelated response");
    let mut stale_connect = [0; 16];
    stale_connect[0..4].copy_from_slice(&0_u32.to_be_bytes());
    stale_connect[4..8].copy_from_slice(&connect_transaction.wrapping_add(1).to_be_bytes());
    stale_connect[8..16].copy_from_slice(&connection_id.to_be_bytes());
    socket
        .send_to(&stale_connect, client)
        .await
        .expect("send stale connect response");
    let mut connect_response = stale_connect;
    connect_response[4..8].copy_from_slice(&connect_transaction.to_be_bytes());
    socket
        .send_to(&connect_response, client)
        .await
        .expect("send connect response");

    let (announce_length, announce_client) = socket
        .recv_from(&mut request)
        .await
        .expect("receive tracker announce");
    assert_eq!(announce_client, client);
    assert_eq!(announce_length, 98);
    assert_eq!(
        u64::from_be_bytes(request[0..8].try_into().expect("connection ID")),
        connection_id
    );
    assert_eq!(
        u32::from_be_bytes(request[8..12].try_into().expect("announce action")),
        1
    );
    let announce_transaction =
        u32::from_be_bytes(request[12..16].try_into().expect("announce transaction"));
    assert_ne!(announce_transaction, 0);
    assert_ne!(announce_transaction, connect_transaction);
    assert_eq!(&request[16..36], &info_hash);
    tokio::time::sleep(announce_delay).await;
    assert_eq!(&request[36..56], &CLIENT_PEER_ID);
    assert_eq!(
        u64::from_be_bytes(request[56..64].try_into().expect("downloaded")),
        0
    );
    assert_eq!(
        u64::from_be_bytes(request[64..72].try_into().expect("left")),
        16 * 1024
    );
    assert_eq!(
        u64::from_be_bytes(request[72..80].try_into().expect("uploaded")),
        0
    );
    assert_eq!(
        u32::from_be_bytes(request[80..84].try_into().expect("event")),
        2
    );
    assert_eq!(
        u32::from_be_bytes(request[84..88].try_into().expect("IP address")),
        0
    );
    assert_ne!(
        u32::from_be_bytes(request[88..92].try_into().expect("key")),
        0
    );
    assert_eq!(
        i32::from_be_bytes(request[92..96].try_into().expect("num want")),
        200
    );
    assert_eq!(
        u16::from_be_bytes(request[96..98].try_into().expect("listen port")),
        1
    );

    let mut response = Vec::new();
    response.extend_from_slice(&1_u32.to_be_bytes());
    response.extend_from_slice(&announce_transaction.to_be_bytes());
    response.extend_from_slice(&1800_u32.to_be_bytes());
    response.extend_from_slice(&1_u32.to_be_bytes());
    response.extend_from_slice(&1_u32.to_be_bytes());
    response.extend_from_slice(&[127, 0, 0, 1, 0, 0]);
    for address in [unreachable, reachable, reachable] {
        let SocketAddr::V4(address) = address else {
            panic!("scripted tracker uses IPv4 peers");
        };
        response.extend_from_slice(&address.ip().octets());
        response.extend_from_slice(&address.port().to_be_bytes());
    }
    response.extend_from_slice(&[192, 0, 2, 1, 0x1a, 0xe1]);

    let mut stale_response = response.clone();
    stale_response[4..8].copy_from_slice(&announce_transaction.wrapping_add(1).to_be_bytes());
    socket
        .send_to(&stale_response, client)
        .await
        .expect("send stale announce response");
    socket
        .send_to(&response, client)
        .await
        .expect("send announce response");
}

async fn serve_rejecting_udp_tracker(socket: UdpSocket) {
    let mut request = [0; 16];
    let (length, client) = socket
        .recv_from(&mut request)
        .await
        .expect("receive rejected tracker connect");
    assert_eq!(length, request.len());
    let transaction = u32::from_be_bytes(request[12..16].try_into().expect("connect transaction"));
    let mut response = Vec::from(3_u32.to_be_bytes());
    response.extend_from_slice(&transaction.to_be_bytes());
    response.extend_from_slice(b"controlled rejection");
    socket
        .send_to(&response, client)
        .await
        .expect("send tracker rejection");
}

async fn assert_tracker_wait_cancels_without_socket_leaks() {
    let tracker = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind silent tracker");
    let tracker_address = tracker.local_addr().expect("tracker address");
    let output_path = test_path("cancelled-tracker-output.bin");
    let control = DownloadControl::new();
    let task_control = control.clone();
    let task = tokio::spawn(download_magnet_with_control(
        MagnetDownloadConfig {
            magnet: format!(
                "magnet:?xt=urn:btih:{}&tr=udp%3A%2F%2F{tracker_address}",
                "00".repeat(20)
            ),
            output_path: output_path.clone(),
            network: loopback_network(Duration::from_secs(2)),
            resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            materialize_files: Vec::new(),
            dht: None,
        },
        task_control,
    ));

    let mut packet = [0; 32];
    let (length, client) = timeout(Duration::from_secs(1), tracker.recv_from(&mut packet))
        .await
        .expect("tracker connect deadline")
        .expect("receive tracker connect");
    assert_eq!(length, 16);
    control.cancel();
    let result = task.await.expect("join tracker-wait download");
    assert!(matches!(result, Err(DownloadError::Cancelled)));
    assert!(!tokio::fs::try_exists(&output_path).await.expect("output"));
    assert!(
        !tokio::fs::try_exists(staging_path(&output_path).expect("staging path"))
            .await
            .expect("staging")
    );

    UdpSocket::bind(client)
        .await
        .expect("tracker client socket released after terminal result");
}

#[derive(Debug, Default)]
struct RecordingActivitySink {
    events: Mutex<Vec<DownloadActivityEvent>>,
}

impl DownloadActivitySink for RecordingActivitySink {
    fn record(&self, event: DownloadActivityEvent) {
        self.events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(event);
    }
}

#[derive(Debug, Default)]
struct RecordingByteMetricSink {
    bytes: Mutex<BTreeMap<ByteMetric, u64>>,
}

impl RecordingByteMetricSink {
    fn bytes(&self, metric: ByteMetric) -> u64 {
        self.bytes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&metric)
            .copied()
            .unwrap_or(0)
    }
}

impl ByteMetricSink for RecordingByteMetricSink {
    fn record(&self, metric: ByteMetric, bytes: u64) {
        let mut observed = self
            .bytes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let total = observed.entry(metric).or_default();
        *total = total.saturating_add(bytes);
    }
}

async fn serve_empty_udp_tracker(socket: UdpSocket) {
    let mut packet = [0; 256];
    let (connect_length, client) = socket
        .recv_from(&mut packet)
        .await
        .expect("receive empty-tracker connect");
    assert_eq!(connect_length, 16);
    let connect_transaction =
        u32::from_be_bytes(packet[12..16].try_into().expect("connect transaction"));
    let connection_id = 0x0102_0304_0506_0708_u64;
    let mut connect_response = Vec::from(0_u32.to_be_bytes());
    connect_response.extend_from_slice(&connect_transaction.to_be_bytes());
    connect_response.extend_from_slice(&connection_id.to_be_bytes());
    socket
        .send_to(&connect_response, client)
        .await
        .expect("send empty-tracker connect response");

    let (announce_length, announce_client) = socket
        .recv_from(&mut packet)
        .await
        .expect("receive empty-tracker announce");
    assert_eq!(announce_length, 98);
    assert_eq!(announce_client, client);
    let announce_transaction =
        u32::from_be_bytes(packet[12..16].try_into().expect("announce transaction"));
    let mut announce_response = Vec::from(1_u32.to_be_bytes());
    announce_response.extend_from_slice(&announce_transaction.to_be_bytes());
    announce_response.extend_from_slice(&600_u32.to_be_bytes());
    announce_response.extend_from_slice(&0_u32.to_be_bytes());
    announce_response.extend_from_slice(&0_u32.to_be_bytes());
    socket
        .send_to(&announce_response, client)
        .await
        .expect("send valid zero-peer announce response");
}

async fn serve_barrier_udp_tracker(
    socket: UdpSocket,
    connect_barrier: Arc<Barrier>,
    peer_port: u16,
) {
    let mut packet = [0; 256];
    let (connect_length, client) = socket
        .recv_from(&mut packet)
        .await
        .expect("receive concurrent tracker connect");
    assert_eq!(connect_length, 16);
    let connect_transaction =
        u32::from_be_bytes(packet[12..16].try_into().expect("connect transaction"));
    connect_barrier.wait().await;

    let connection_id = 0x0102_0304_0506_0708_u64;
    let mut connect_response = Vec::from(0_u32.to_be_bytes());
    connect_response.extend_from_slice(&connect_transaction.to_be_bytes());
    connect_response.extend_from_slice(&connection_id.to_be_bytes());
    socket
        .send_to(&connect_response, client)
        .await
        .expect("send concurrent tracker connect response");

    let (announce_length, announce_client) = socket
        .recv_from(&mut packet)
        .await
        .expect("receive concurrent tracker announce");
    assert_eq!(announce_length, 98);
    assert_eq!(announce_client, client);
    let announce_transaction =
        u32::from_be_bytes(packet[12..16].try_into().expect("announce transaction"));
    let mut announce_response = Vec::from(1_u32.to_be_bytes());
    announce_response.extend_from_slice(&announce_transaction.to_be_bytes());
    announce_response.extend_from_slice(&600_u32.to_be_bytes());
    announce_response.extend_from_slice(&0_u32.to_be_bytes());
    announce_response.extend_from_slice(&0_u32.to_be_bytes());
    announce_response.extend_from_slice(&[127, 0, 0, 1]);
    announce_response.extend_from_slice(&peer_port.to_be_bytes());
    socket
        .send_to(&announce_response, client)
        .await
        .expect("send concurrent tracker announce response");
}

async fn serve_bounded_startup_tracker(
    socket: UdpSocket,
    started: Arc<AtomicUsize>,
    release: Arc<Semaphore>,
    peer_port: u16,
) -> bool {
    let mut packet = [0; 256];
    let (connect_length, client) = socket
        .recv_from(&mut packet)
        .await
        .expect("receive bounded tracker connect");
    assert_eq!(connect_length, 16);
    let ordinal = started.fetch_add(1, Ordering::AcqRel);
    let _permit = release.acquire().await.expect("startup release permit");
    let connect_transaction =
        u32::from_be_bytes(packet[12..16].try_into().expect("connect transaction"));
    if ordinal < super::MAX_CONCURRENT_TRACKER_OPERATIONS {
        let mut error_response = Vec::from(3_u32.to_be_bytes());
        error_response.extend_from_slice(&connect_transaction.to_be_bytes());
        error_response.extend_from_slice(b"scripted startup failure");
        socket
            .send_to(&error_response, client)
            .await
            .expect("send bounded tracker error");
        return false;
    }

    let connection_id = 0x0102_0304_0506_0708_u64;
    let mut connect_response = Vec::from(0_u32.to_be_bytes());
    connect_response.extend_from_slice(&connect_transaction.to_be_bytes());
    connect_response.extend_from_slice(&connection_id.to_be_bytes());
    socket
        .send_to(&connect_response, client)
        .await
        .expect("send bounded tracker connect response");
    let (announce_length, announce_client) = socket
        .recv_from(&mut packet)
        .await
        .expect("receive bounded tracker announce");
    assert_eq!(announce_length, 98);
    assert_eq!(announce_client, client);
    let announce_transaction =
        u32::from_be_bytes(packet[12..16].try_into().expect("announce transaction"));
    let mut announce_response = Vec::from(1_u32.to_be_bytes());
    announce_response.extend_from_slice(&announce_transaction.to_be_bytes());
    announce_response.extend_from_slice(&600_u32.to_be_bytes());
    announce_response.extend_from_slice(&0_u32.to_be_bytes());
    announce_response.extend_from_slice(&0_u32.to_be_bytes());
    announce_response.extend_from_slice(&[127, 0, 0, 1]);
    announce_response.extend_from_slice(&peer_port.to_be_bytes());
    socket
        .send_to(&announce_response, client)
        .await
        .expect("send bounded tracker announce response");
    true
}

mod content;
mod control;
mod discovery_metadata;
mod recheck_publication;
mod storage_pipeline;
