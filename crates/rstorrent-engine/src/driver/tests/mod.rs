
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
    CLIENT_PEER_ID, CONTENT_STORAGE_HASH_CONCURRENCY, CONTENT_STORAGE_WRITE_BATCH_BLOCKS,
    CONTENT_STORAGE_WRITE_BATCH_BYTES, CONTENT_STORAGE_WRITE_CONCURRENCY, CoalescedContentWrite,
    ContentCheckpointPipeline, ContentDownloadConfig, ContentStorage, ContentStorageCommand,
    ContentStorageCompletion, ContentStoragePipeline, ContentSupervisorOwner, ContentWriteStats,
    DEFAULT_ADVERTISED_PEER_PORT, DhtRetryTiming, DiskPressure, DownloadActivityEvent,
    DownloadActivitySink, DownloadConfig, DownloadControl, DownloadError, DownloadResourceLimits,
    MAX_DIAGNOSTIC_ERROR_LENGTH, MAX_METADATA_PEERS, MAX_RECENT_METADATA_ATTEMPTS,
    MagnetDownloadConfig, MetadataAcquisitionPhase, MetadataPeerStage, PeerConnection,
    PreparedContentWrite, QueuedContentStorageCommand, ResumableMagnetDownloadConfig,
    ResumeArtifactState, SwarmConfig, TorrentPeerCoordinator, TrackerManager, UdpTrackerAnnounce,
    UdpTrackerExchange, UdpTrackerTiming, UdpTrackerTokenCache, announce_udp_tracker_address,
    atomic_saturating_add, atomic_saturating_increment, coalesce_content_writes,
    collect_content_write_batch, content_dial_slot_available, content_storage_job_limit,
    download_magnet, download_magnet_metadata_with_control, download_magnet_metadata_with_dht,
    download_magnet_with_control, download_verified_piece, download_verified_piece_with_control,
    execute_content_storage_verification, execute_content_storage_writes, next_peer_message,
    resume_magnet, resume_magnet_with_control, retrying_dht_lookup, run_content_download,
    run_magnet_download_with_peers, send_message,
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

#[test]
fn storage_duration_counter_saturates() {
    let value = AtomicU64::new(u64::MAX - 2);
    atomic_saturating_add(&value, 3);
    assert_eq!(value.load(Ordering::Acquire), u64::MAX);
    atomic_saturating_add(&value, 1);
    assert_eq!(value.load(Ordering::Acquire), u64::MAX);

    let count = AtomicUsize::new(usize::MAX);
    atomic_saturating_increment(&count);
    assert_eq!(count.load(Ordering::Acquire), usize::MAX);
}

#[test]
fn checkpoint_stages_and_fixed_counters_are_exact() {
    let control = DownloadControl::new();
    control.configure_disk_runtime(MIN_PAYLOAD_ALLOWANCE);
    let block = BlockKey {
        piece: 7,
        begin: 0,
        length: 16,
    };
    control.disk_block_requested(block, 16);
    control.disk_block_received(block, 16);
    control.disk_block_stored(block, 16);
    control.disk_piece_hashing(7, 16);
    control.disk_piece_hash_verified(7, 16, true);

    let dirty = control.disk_snapshot();
    assert_eq!(dirty.hashing_bytes, 0);
    assert_eq!(dirty.checkpoint_stage, DiskCheckpointStage::Idle);
    assert_eq!(dirty.checkpoint_dirty_pieces, 1);
    assert_eq!(dirty.checkpoint_dirty_bytes, 16);
    assert_eq!(dirty.checkpoint_dirty_piece_high_water, 1);
    assert_eq!(dirty.checkpoint_dirty_byte_high_water, 16);
    assert_eq!(dirty.pieces[0].stage, DiskPieceStage::CheckpointDirty);

    let intent = CheckpointIntent::new(7, 16, Duration::ZERO, [DurabilityTarget::PartFile])
        .expect("checkpoint intent");
    let batch = CheckpointBatch {
        intents: vec![intent],
        dirty_bytes: 16,
        oldest_verified_at: Duration::ZERO,
        targets: vec![DurabilityTarget::PartFile],
    };
    control.disk_checkpoint_sync_started(&batch);
    let syncing = control.disk_snapshot();
    assert_eq!(syncing.checkpoint_stage, DiskCheckpointStage::Syncing);
    assert_eq!(syncing.checkpoint_batches_started, 1);
    assert_eq!(syncing.pieces[0].stage, DiskPieceStage::CheckpointSyncing);

    control.disk_checkpoint_sync_completed(&batch, Duration::from_micros(11));
    let committing = control.disk_snapshot();
    assert_eq!(committing.checkpoint_stage, DiskCheckpointStage::Committing);
    assert_eq!(committing.checkpoint_sync_operations_completed, 1);
    assert_eq!(committing.checkpoint_sync_service_micros, 11);
    assert_eq!(
        committing.pieces[0].stage,
        DiskPieceStage::CheckpointCommitting
    );

    control.disk_checkpoint_completed(&batch, Duration::from_micros(13));
    let completed = control.disk_snapshot();
    assert_eq!(completed.checkpoint_stage, DiskCheckpointStage::Idle);
    assert_eq!(completed.checkpoint_dirty_pieces, 0);
    assert_eq!(completed.checkpoint_dirty_bytes, 0);
    assert_eq!(completed.checkpoint_batches_completed, 1);
    assert_eq!(completed.checkpoint_pieces_completed, 1);
    assert_eq!(completed.checkpoint_commit_service_micros, 13);
    assert!(completed.pieces.is_empty());
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

#[tokio::test]
async fn checkpoint_delays_preserve_storage_progress_bounds_and_final_flush() {
    let control = DownloadControl::new();
    control.configure_disk_runtime(MIN_PAYLOAD_ALLOWANCE);
    control.set_checkpoint_sync_delay_for_testing(Duration::from_millis(350));
    control.set_checkpoint_commit_delay_for_testing(Duration::from_millis(350));
    let sink = Arc::new(RecordingCheckpointSink::default());
    let (sync_path, handles) = checkpoint_sync_handle("checkpoint-delays.bin");
    let mut pipeline = ContentCheckpointPipeline::start(handles, sink.clone(), control.clone())
        .expect("start checkpoint pipeline");
    let full_epoch_length =
        u32::try_from(super::CHECKPOINT_MAX_DIRTY_BYTES).expect("checkpoint byte bound");
    control.disk_piece_hashing(7, full_epoch_length);
    control.disk_piece_hash_verified(7, full_epoch_length, true);
    pipeline
        .enqueue(7, full_epoch_length, vec![DurabilityTarget::PartFile])
        .await
        .expect("enqueue full checkpoint epoch");
    wait_for_checkpoint_stage(&control, DiskCheckpointStage::Syncing).await;

    let output = test_path("checkpoint-overlap-storage.bin");
    let staged = staging_path(&output).expect("staging path");
    let mut storage = single_file_content_storage(output, 16, 16).await;
    let writes =
        execute_content_storage_writes(&mut storage, vec![queued_write(0, 0, 16)], &control).await;
    assert!(matches!(
        writes.as_slice(),
        [ContentStorageCompletion::Write { result: Ok(_), .. }]
    ));
    let expected: [u8; 20] = Sha1::digest([0_u8; 16]).into();
    control.disk_piece_hashing(0, 16);
    let verification = execute_content_storage_verification(
        &mut storage,
        ContentStorageCommand::Verify {
            piece: 0,
            generation: PieceGeneration::new(1).expect("generation"),
            length: 16,
            expected,
            durable: false,
        },
        &control,
    )
    .await;
    assert!(matches!(
        verification,
        ContentStorageCompletion::Verify { result: Ok(_), .. }
    ));
    assert_eq!(
        control.disk_snapshot().checkpoint_stage,
        DiskCheckpointStage::Syncing
    );
    drop(storage);
    let _ = tokio::fs::remove_file(staged).await;

    wait_for_checkpoint_stage(&control, DiskCheckpointStage::Committing).await;
    assert!(
        timeout(
            Duration::from_millis(30),
            pipeline.enqueue(8, 16, vec![DurabilityTarget::PartFile]),
        )
        .await
        .is_err(),
        "dirty-byte permits remain charged through the database callback"
    );
    timeout(Duration::from_secs(2), async {
        while control.disk_snapshot().checkpoint_batches_completed != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("full epoch completed");

    control.disk_piece_hashing(8, 16);
    control.disk_piece_hash_verified(8, 16, true);
    pipeline
        .enqueue(8, 16, vec![DurabilityTarget::PartFile])
        .await
        .expect("capacity reopened after commit");
    pipeline.intents.take();
    pipeline
        .task
        .await
        .expect("checkpoint task joined")
        .expect("final close flushed checkpoint");
    assert_eq!(sink.batches(), vec![vec![7], vec![8]]);
    let completed = control.disk_snapshot();
    assert_eq!(completed.checkpoint_batches_completed, 2);
    assert_eq!(completed.checkpoint_pieces_completed, 2);
    assert_eq!(completed.checkpoint_dirty_pieces, 0);
    assert_eq!(completed.checkpoint_dirty_bytes, 0);
    assert!(completed.checkpoint_sync_service_micros >= 700_000);
    assert!(completed.checkpoint_commit_service_micros >= 700_000);
    std::fs::remove_file(sync_path).expect("remove checkpoint sync target");
}

#[tokio::test]
async fn checkpoint_sync_and_commit_failures_are_typed_and_joined() {
    let control = DownloadControl::new();
    control.configure_disk_runtime(MIN_PAYLOAD_ALLOWANCE);
    control.fail_next_checkpoint_sync();
    let sync_sink = Arc::new(RecordingCheckpointSink::default());
    let (sync_path, handles) = checkpoint_sync_handle("checkpoint-sync-failure.bin");
    let mut pipeline =
        ContentCheckpointPipeline::start(handles, sync_sink.clone(), control.clone())
            .expect("start sync-failure pipeline");
    let full_epoch_length =
        u32::try_from(super::CHECKPOINT_MAX_DIRTY_BYTES).expect("checkpoint byte bound");
    control.disk_piece_hashing(1, full_epoch_length);
    control.disk_piece_hash_verified(1, full_epoch_length, true);
    pipeline
        .enqueue(1, full_epoch_length, vec![DurabilityTarget::PartFile])
        .await
        .expect("enqueue sync failure");
    pipeline.intents.take();
    let reported = timeout(Duration::from_secs(1), pipeline.failures.recv())
        .await
        .expect("sync failure reached supervisor channel")
        .expect("sync failure detail");
    assert!(reported.contains("injected"));
    let error = pipeline
        .task
        .await
        .expect("sync-failure task joined")
        .expect_err("sync failure is terminal");
    assert!(matches!(error, DownloadError::Checkpoint(ref detail) if detail.contains("injected")));
    assert!(sync_sink.batches().is_empty());
    let failed = control.disk_snapshot();
    assert_eq!(failed.checkpoint_stage, DiskCheckpointStage::Error);
    assert_eq!(failed.pieces[0].stage, DiskPieceStage::Failed);
    std::fs::remove_file(sync_path).expect("remove sync-failure target");

    let control = DownloadControl::new();
    control.configure_disk_runtime(MIN_PAYLOAD_ALLOWANCE);
    let commit_sink = Arc::new(RecordingCheckpointSink::failing("injected commit failure"));
    let (commit_path, handles) = checkpoint_sync_handle("checkpoint-commit-failure.bin");
    let mut pipeline =
        ContentCheckpointPipeline::start(handles, commit_sink.clone(), control.clone())
            .expect("start commit-failure pipeline");
    control.disk_piece_hashing(2, full_epoch_length);
    control.disk_piece_hash_verified(2, full_epoch_length, true);
    pipeline
        .enqueue(2, full_epoch_length, vec![DurabilityTarget::PartFile])
        .await
        .expect("enqueue commit failure");
    pipeline.intents.take();
    let error = pipeline
        .task
        .await
        .expect("commit-failure task joined")
        .expect_err("commit failure is terminal");
    assert!(matches!(error, DownloadError::Checkpoint(ref detail) if detail.contains("commit")));
    assert_eq!(commit_sink.batches(), vec![vec![2]]);
    let failed = control.disk_snapshot();
    assert_eq!(failed.checkpoint_stage, DiskCheckpointStage::Error);
    assert_eq!(failed.checkpoint_sync_operations_completed, 1);
    assert_eq!(failed.checkpoint_batches_completed, 0);
    assert_eq!(failed.pieces[0].stage, DiskPieceStage::Failed);
    std::fs::remove_file(commit_path).expect("remove commit-failure target");
}

#[test]
fn disk_pressure_uses_distinct_high_and_low_watermarks() {
    let control = DownloadControl::new();
    let limit = 4 * MIN_PAYLOAD_ALLOWANCE;
    control.configure_disk_runtime(limit);
    assert!(control.try_buffer_payload(2 * MIN_PAYLOAD_ALLOWANCE, limit));
    assert!(!control.disk_snapshot().intake_backpressured);
    assert!(control.try_buffer_payload(MIN_PAYLOAD_ALLOWANCE, limit));
    let high = control.disk_snapshot();
    assert_eq!(high.pressure, DiskPressure::Backpressured);
    assert!(high.intake_backpressured);
    assert_eq!(
        high.resident_high_watermark_bytes,
        3 * MIN_PAYLOAD_ALLOWANCE
    );
    assert_eq!(high.resident_low_watermark_bytes, 2 * MIN_PAYLOAD_ALLOWANCE);

    control.release_buffered_payload(MIN_PAYLOAD_ALLOWANCE / 2);
    assert!(control.disk_snapshot().intake_backpressured);
    control.release_buffered_payload(MIN_PAYLOAD_ALLOWANCE / 2);
    let recovered = control.disk_snapshot();
    assert_eq!(recovered.pressure, DiskPressure::Draining);
    assert!(!recovered.intake_backpressured);
    assert_eq!(recovered.pressure_transition_count, 2);
}

#[test]
fn disk_piece_snapshot_counts_unique_ranges_and_retries() {
    let control = DownloadControl::new();
    control.configure_disk_runtime(8 * MIN_PAYLOAD_ALLOWANCE);
    let first = BlockKey::new(7, 0, MIN_PAYLOAD_ALLOWANCE as u32).expect("first block");
    control.disk_block_requested(first, 2 * MIN_PAYLOAD_ALLOWANCE as u32);
    control.disk_block_requested(first, 2 * MIN_PAYLOAD_ALLOWANCE as u32);
    control.disk_block_received(first, 2 * MIN_PAYLOAD_ALLOWANCE as u32);
    let active = control.disk_snapshot();
    assert_eq!(active.pieces.len(), 1);
    assert_eq!(
        active.pieces[0].requested_bytes,
        MIN_PAYLOAD_ALLOWANCE as u32
    );
    assert_eq!(
        active.pieces[0].received_bytes,
        MIN_PAYLOAD_ALLOWANCE as u32
    );
    assert_eq!(active.pieces[0].attempt, 1);

    control.disk_piece_failed(7, 2 * MIN_PAYLOAD_ALLOWANCE as u32, "piece hash failed");
    let second = BlockKey::new(
        7,
        MIN_PAYLOAD_ALLOWANCE as u32,
        MIN_PAYLOAD_ALLOWANCE as u32,
    )
    .expect("second block");
    control.disk_block_requested(second, 2 * MIN_PAYLOAD_ALLOWANCE as u32);
    let retry = control.disk_snapshot();
    assert_eq!(retry.pieces[0].attempt, 2);
    assert_eq!(
        retry.pieces[0].requested_bytes,
        MIN_PAYLOAD_ALLOWANCE as u32
    );
    assert_eq!(retry.pieces[0].received_bytes, 0);
    assert_eq!(retry.pieces[0].error, None);
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

#[test]
fn storage_write_batch_coalesces_only_exact_piece_ranges() {
    let writes = vec![
        prepared_write(0, 4, b"efgh"),
        prepared_write(1, 0, b"WXYZ"),
        prepared_write(0, 0, b"abcd"),
        prepared_write(0, 12, b"mnop"),
        prepared_write(0, 8, b"ijkl"),
    ];
    let coalesced = coalesce_content_writes(writes).expect("coalesce exact ranges");
    assert_eq!(coalesced.len(), 2);
    let CoalescedContentWrite {
        piece,
        begin,
        bytes,
        members,
        ..
    } = &coalesced[0];
    assert_eq!((*piece, *begin), (0, 0));
    assert_eq!(bytes, b"abcdefghijklmnop");
    assert_eq!(members.len(), 4);
    assert_eq!(coalesced[1].piece, 1);
    assert_eq!(coalesced[1].members.len(), 1);
}

#[test]
fn storage_write_batch_rejects_overlap_and_keeps_gaps() {
    let gapped = coalesce_content_writes(vec![
        prepared_write(0, 0, b"abcd"),
        prepared_write(0, 8, b"ijkl"),
    ])
    .expect("gapped writes remain separate");
    assert_eq!(gapped.len(), 2);

    let overlap = coalesce_content_writes(vec![
        prepared_write(0, 0, b"abcdefgh"),
        prepared_write(0, 4, b"efgh"),
    ]);
    assert!(matches!(
        overlap,
        Err((_, _, DownloadError::StorageTask(_)))
    ));
}

#[test]
fn storage_write_batch_respects_exact_count_and_byte_caps() {
    let (sender, mut receiver) = mpsc::channel(CONTENT_STORAGE_WRITE_BATCH_BLOCKS);
    for piece in 1..=CONTENT_STORAGE_WRITE_BATCH_BLOCKS {
        sender
            .try_send(queued_write(piece as u32, 0, MIN_PAYLOAD_ALLOWANCE))
            .expect("queue test write");
    }
    let mut deferred = None;
    let batch = collect_content_write_batch(
        queued_write(0, 0, MIN_PAYLOAD_ALLOWANCE),
        &mut receiver,
        &mut deferred,
    );
    assert_eq!(batch.len(), CONTENT_STORAGE_WRITE_BATCH_BLOCKS);
    assert_eq!(
        batch
            .iter()
            .map(|queued| queued.command.write_bytes().expect("write bytes"))
            .sum::<usize>(),
        CONTENT_STORAGE_WRITE_BATCH_BYTES
    );
    assert!(deferred.is_none());
    assert_eq!(receiver.len(), 1);
}

#[tokio::test]
async fn failed_coalesced_write_mutates_no_valid_prefix() {
    let output = test_path("coalesced-write-failure.bin");
    let staged = staging_path(&output).expect("staging path");
    let mut storage = single_file_content_storage(output.clone(), 4, 4).await;
    let commands = vec![queued_write(0, 0, 4), queued_write(0, 4, 4)];

    let completions =
        execute_content_storage_writes(&mut storage, commands, &DownloadControl::new()).await;

    assert_eq!(completions.len(), 2);
    assert!(matches!(
        &completions[0],
        ContentStorageCompletion::Write {
            result: Err(DownloadError::SelectiveStorage(
                SelectiveStorageError::Layout(LayoutError::IntervalOutOfRange { .. })
            )),
            ..
        }
    ));
    assert!(!staged.exists(), "invalid batch must create no artifact");
    drop(storage);
    let _ = tokio::fs::remove_file(staged).await;
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

#[test]
fn half_open_dials_do_not_consume_established_connection_slots() {
    let mut config = SwarmConfig::for_request_limit(MIN_PAYLOAD_ALLOWANCE);
    config.max_established_connections = 2;
    config.max_pending_dials = 2;

    assert!(content_dial_slot_available(1, 1, config, false));
    assert!(!content_dial_slot_available(1, 2, config, false));
    assert!(!content_dial_slot_available(2, 0, config, false));
    assert!(content_dial_slot_available(2, 0, config, true));
    assert!(!content_dial_slot_available(2, 1, config, true));
}

#[test]
fn content_supervisor_owner_rotation_is_complete_and_stable() {
    let mut owner = ContentSupervisorOwner::Storage;
    let mut observed = Vec::new();
    for _ in 0..6 {
        observed.push(owner);
        owner = owner.next();
    }
    assert_eq!(
        observed,
        [
            ContentSupervisorOwner::Storage,
            ContentSupervisorOwner::Peer,
            ContentSupervisorOwner::Discovery,
            ContentSupervisorOwner::Storage,
            ContentSupervisorOwner::Peer,
            ContentSupervisorOwner::Discovery,
        ]
    );
}

#[test]
fn product_profiles_are_generous_and_fill_every_initial_peer_window() {
    assert_eq!(
        DownloadResourceLimits::DESKTOP,
        DownloadResourceLimits::new(256 * 1024 * 1024, 32 * 1024 * 1024, 256 * 1024 * 1024)
    );
    assert_eq!(
        DownloadResourceLimits::ANDROID,
        DownloadResourceLimits::new(128 * 1024 * 1024, 16 * 1024 * 1024, 128 * 1024 * 1024)
    );
    let initial_window_bytes = DEFAULT_MAX_ESTABLISHED_CONNECTIONS
        * DEFAULT_INITIAL_REQUESTS_PER_CONNECTION
        * MIN_PAYLOAD_ALLOWANCE;
    for limits in [
        DownloadResourceLimits::DESKTOP,
        DownloadResourceLimits::ANDROID,
    ] {
        assert!(limits.max_outstanding_request_bytes >= initial_window_bytes);
        assert!(limits.max_buffered_payload_bytes >= MIN_PAYLOAD_ALLOWANCE);
        assert!(limits.max_active_piece_bytes >= initial_window_bytes);
        limits.validate().expect("valid product profile");
    }
}

#[test]
fn metadata_diagnostic_history_and_error_detail_are_bounded() {
    let control = DownloadControl::new();
    control.metadata_started();
    let mut registry = PeerRegistry::new(PeerRegistryConfig::default()).expect("registry");
    for offset in 0..=MAX_RECENT_METADATA_ATTEMPTS {
        let endpoint = PeerEndpoint::new(SocketAddr::from((
            [127, 0, 0, 1],
            10_000 + u16::try_from(offset).expect("bounded port"),
        )))
        .expect("valid endpoint");
        registry
            .observe(
                PeerObservation::dialable(endpoint, PeerSource::Tracker),
                Duration::ZERO,
            )
            .expect("observation");
        let context = PeerSelectionContext {
            now: Duration::ZERO,
        };
        let candidate = PeerSelector
            .select(&registry, context)
            .expect("diagnostic candidate");
        let attempt = registry
            .begin_dial(candidate, context)
            .expect("diagnostic attempt");
        control.metadata_dial_started(attempt);
        control.metadata_peer_finished(
            attempt.id(),
            MetadataPeerStage::Failed,
            Some(&"x".repeat(MAX_DIAGNOSTIC_ERROR_LENGTH + 50)),
        );
        registry
            .dial_failed(attempt, Duration::ZERO, PeerFailure::Protocol)
            .expect("terminal attempt");
    }

    let snapshot = control.diagnostic_snapshot().metadata;
    assert_eq!(snapshot.recent_attempts.len(), MAX_RECENT_METADATA_ATTEMPTS);
    assert_eq!(snapshot.recent_attempts_dropped, 1);
    assert!(snapshot.active_attempts.is_empty());
    assert!(snapshot.recent_attempts.iter().all(|attempt| {
        attempt
            .terminal_detail
            .as_ref()
            .is_some_and(|detail| detail.len() == MAX_DIAGNOSTIC_ERROR_LENGTH)
    }));
}

#[tokio::test]
async fn explicit_policies_gate_non_loopback_peers_and_offline_dns() {
    let public = "192.0.2.1:6881".parse().expect("documentation peer");
    let loopback = TorrentPeerCoordinator::from_endpoint(
        public,
        PeerSource::Manual,
        loopback_network(Duration::from_secs(1)),
    );
    assert!(matches!(
        loopback,
        Err(DownloadError::NetworkPolicyDenied {
            address,
            policy: NetworkPolicy::LoopbackOnly,
        }) if address == public
    ));

    let online = TorrentPeerCoordinator::from_endpoint(
        public,
        PeerSource::Manual,
        NetworkConfig::new(
            NetworkPolicy::Online,
            Duration::from_secs(1),
            Duration::from_secs(1),
        ),
    )
    .expect("online policy accepts valid public peer");
    assert_eq!(online.registry.len(), 1);

    let offline = download_magnet_metadata_with_control(
        "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213\
             &x.pe=must-not-resolve.invalid:6881"
            .to_owned(),
        NetworkConfig::new(
            NetworkPolicy::Offline,
            Duration::from_secs(1),
            Duration::from_secs(1),
        ),
        DownloadControl::new(),
    )
    .await;
    assert!(matches!(offline, Err(DownloadError::NetworkDisabled)));
}

#[tokio::test]
async fn final_dial_rechecks_network_policy() {
    let public = "192.0.2.1:6881".parse().expect("documentation peer");
    let mut peers = TorrentPeerCoordinator::from_endpoint(
        public,
        PeerSource::Manual,
        NetworkConfig::new(
            NetworkPolicy::Online,
            Duration::from_secs(1),
            Duration::from_secs(1),
        ),
    )
    .expect("online peer session");
    peers.network.policy = NetworkPolicy::LoopbackOnly;

    let result = peers.connect_next([0; 20], false).await;
    assert!(matches!(
        result,
        Err(DownloadError::NetworkPolicyDenied {
            address,
            policy: NetworkPolicy::LoopbackOnly,
        }) if address == public
    ));
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

#[tokio::test]
async fn fragmented_bytes_cannot_extend_one_message_deadline() {
    let (mut peer, mut server) = connected_pair(Duration::from_millis(50)).await;
    let frame = encode_message(&PeerMessage::KeepAlive).expect("keepalive frame");
    let writer = tokio::spawn(async move {
        for byte in frame {
            if server.write_all(&[byte]).await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });

    let result = next_peer_message(&mut peer).await;
    assert!(matches!(
        result,
        Err(DownloadError::PeerTimedOut {
            operation: "message read",
            ..
        })
    ));
    writer.await.expect("fragment writer");
}

#[tokio::test]
async fn timely_messages_can_outlive_one_io_deadline() {
    let io_timeout = Duration::from_millis(150);
    let (mut peer, mut server) = connected_pair(io_timeout).await;
    let frame = encode_message(&PeerMessage::KeepAlive).expect("keepalive frame");
    let writer = tokio::spawn(async move {
        for _ in 0..4 {
            server
                .write_all(&frame)
                .await
                .expect("write complete keepalive");
            tokio::time::sleep(Duration::from_millis(80)).await;
        }
    });

    for _ in 0..4 {
        assert_eq!(
            next_peer_message(&mut peer)
                .await
                .expect("timely complete message"),
            PeerMessage::KeepAlive
        );
    }
    writer.await.expect("timely message writer");
}

#[tokio::test]
#[ignore = "uses changing public trackers and swarm state"]
async fn live_big_buck_bunny_metadata_probe() {
    let magnet = "magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c\
&dn=Big+Buck+Bunny\
&tr=udp%3A%2F%2Fexplodie.org%3A6969\
&tr=udp%3A%2F%2Ftracker.coppersurfer.tk%3A6969\
&tr=udp%3A%2F%2Ftracker.empire-js.us%3A1337\
&tr=udp%3A%2F%2Ftracker.leechers-paradise.org%3A6969\
&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337";
    let control = DownloadControl::new();
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());
    let task_control = control.clone();
    let mut task = tokio::spawn(download_magnet_metadata_with_control(
        magnet.to_owned(),
        NetworkConfig::new(
            NetworkPolicy::Online,
            Duration::from_secs(15),
            Duration::from_secs(30),
        ),
        task_control,
    ));
    let monitor_control = control.clone();
    let monitor = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            let snapshot = monitor_control.diagnostic_snapshot().metadata;
            let registry = snapshot.registry.as_ref().map(|registry| registry.counts);
            eprintln!(
                "public metadata probe: elapsed={:?} phase={:?} registry={registry:?} \
                     pending_dials={} active_workers={} attempts={} requests={} blocks={} bytes={} \
                     active={} recent={} dropped={}",
                snapshot.captured_at,
                snapshot.phase,
                snapshot.pending_dials,
                snapshot.active_workers,
                snapshot.total_attempts,
                snapshot.total_requests_sent,
                snapshot.total_blocks_received,
                snapshot.total_bytes_received,
                snapshot.active_attempts.len(),
                snapshot.recent_attempts.len(),
                snapshot.recent_attempts_dropped,
            );
        }
    });

    let raw_info = match timeout(Duration::from_secs(90), &mut task).await {
        Ok(result) => {
            monitor.abort();
            let _ = monitor.await;
            let raw_info = result
                .expect("join public metadata probe")
                .expect("acquire public metadata");
            eprintln!(
                "public metadata probe completed:\n{:#?}",
                control.diagnostic_snapshot()
            );
            raw_info
        }
        Err(_) => {
            monitor.abort();
            let _ = monitor.await;
            let timeout_snapshot = control.diagnostic_snapshot();
            let events = activity
                .events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone();
            eprintln!("public metadata probe timeout snapshot:\n{timeout_snapshot:#?}");
            eprintln!("public metadata probe activity:\n{events:#?}");
            control.cancel();
            if timeout(Duration::from_secs(5), &mut task).await.is_err() {
                task.abort();
                let _ = task.await;
            }
            panic!("public metadata probe exceeded 90 seconds");
        }
    };
    let metainfo = Metainfo::from_info_bytes(&raw_info).expect("verified public metadata");
    assert_eq!(
        hex(&metainfo.info_hash),
        "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c"
    );
}

#[tokio::test]
#[ignore = "uses changing public Mainline DHT and swarm state"]
async fn live_big_buck_bunny_trackerless_dht_metadata_probe() {
    let expected_info_hash = "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c";
    let dht = DhtService::start(DhtConfig::for_network(NetworkPolicy::Online))
        .await
        .expect("start public DHT");
    let control = DownloadControl::new();
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());
    let task_control = control.clone();
    let dht_handle = dht.handle();
    let mut task = tokio::spawn(async move {
        download_magnet_metadata_with_dht(
            format!("magnet:?xt=urn:btih:{expected_info_hash}"),
            NetworkConfig::new(
                NetworkPolicy::Online,
                Duration::from_secs(15),
                Duration::from_secs(30),
            ),
            task_control,
            Some(dht_handle),
        )
        .await
    });
    let monitor_control = control.clone();
    let monitor = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            let snapshot = monitor_control.diagnostic_snapshot().metadata;
            let registry = snapshot.registry.as_ref().map(|registry| registry.counts);
            eprintln!(
                "public DHT metadata probe: elapsed={:?} phase={:?} registry={registry:?} \
                     pending_dials={} active_workers={} attempts={} requests={} blocks={} bytes={} \
                     active={} recent={} dropped={} last_error={:?}",
                snapshot.captured_at,
                snapshot.phase,
                snapshot.pending_dials,
                snapshot.active_workers,
                snapshot.total_attempts,
                snapshot.total_requests_sent,
                snapshot.total_blocks_received,
                snapshot.total_bytes_received,
                snapshot.active_attempts.len(),
                snapshot.recent_attempts.len(),
                snapshot.recent_attempts_dropped,
                snapshot.last_error,
            );
        }
    });

    let raw_info = match timeout(Duration::from_secs(120), &mut task).await {
        Ok(result) => {
            monitor.abort();
            let _ = monitor.await;
            let raw_info = result
                .expect("join public DHT metadata probe")
                .expect("acquire public DHT metadata");
            let stats = dht.handle().stats().await.ok();
            eprintln!(
                "public DHT metadata probe completed; stats={stats:?}:\n{:#?}",
                control.diagnostic_snapshot()
            );
            raw_info
        }
        Err(_) => {
            monitor.abort();
            let _ = monitor.await;
            let stats = dht.handle().stats().await.ok();
            let timeout_snapshot = control.diagnostic_snapshot();
            let events = activity
                .events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone();
            eprintln!("public DHT metadata timeout snapshot:\n{timeout_snapshot:#?}");
            eprintln!("public DHT metadata activity:\n{events:#?}");
            control.cancel();
            if timeout(Duration::from_secs(5), &mut task).await.is_err() {
                task.abort();
                let _ = task.await;
            }
            dht.shutdown().await.expect("DHT shutdown after timeout");
            panic!("public trackerless DHT metadata probe exceeded 120 seconds; stats={stats:?}");
        }
    };
    dht.shutdown().await.expect("public DHT shutdown");
    let metainfo = Metainfo::from_info_bytes(&raw_info).expect("verified public metadata");
    assert_eq!(hex(&metainfo.info_hash), expected_info_hash);
}

#[test]
fn safe_cancel_waits_for_storage_creation_boundary() {
    let control = DownloadControl::new();
    let storage_creation = control
        .enter_safe_cancel_critical()
        .expect("enter storage creation");

    control.cancel_when_safe();
    assert!(!control.is_cancelled());
    assert!(matches!(
        control.enter_safe_cancel_critical(),
        Err(DownloadError::Cancelled)
    ));

    drop(storage_creation);
    assert!(control.is_cancelled());

    let immediate = DownloadControl::new();
    immediate.cancel_when_safe();
    assert!(immediate.is_cancelled());
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

#[tokio::test]
async fn multi_piece_single_file_uses_torrent_offsets_and_publishes() {
    let payload = (0..(3 * 16 * 1024 + 731))
        .map(|index| ((index * 47 + index / 19) & 0xff) as u8)
        .collect::<Vec<_>>();
    let info = single_file_info_with_piece_length(&payload, 32 * 1024);
    let metainfo = Metainfo::from_info_bytes(&info).expect("multi-piece single-file metainfo");
    let pieces = payload
        .chunks(32 * 1024)
        .map(<[u8]>::to_vec)
        .collect::<Vec<_>>();
    assert_eq!(pieces.len(), 2);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind multi-piece peer");
    let address = listener.local_addr().expect("multi-piece peer address");
    let peer_task = tokio::spawn(serve_content_peer(
        listener,
        metainfo.info_hash,
        Arc::new(pieces),
        vec![true; metainfo.piece_count()],
    ));
    let output = test_path("multi-piece-single-file.bin");
    let mut peers = TorrentPeerCoordinator::from_endpoint(
        address,
        PeerSource::Manual,
        loopback_network(Duration::from_secs(2)),
    )
    .expect("peer session");

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
    .expect("bounded multi-piece single-file download")
    .expect("multi-piece single-file completion");

    assert_eq!(report.piece_count, 2);
    assert_eq!(report.verified_piece_count, 2);
    assert_eq!(report.bytes_written, payload.len());
    assert_eq!(report.selected_written_bytes, payload.len());
    assert_eq!(
        tokio::fs::read(&output).await.expect("published file"),
        payload
    );
    timeout(Duration::from_secs(1), peer_task)
        .await
        .expect("multi-piece peer joined")
        .expect("multi-piece peer task");
    let _ = tokio::fs::remove_file(output).await;
}

#[tokio::test]
async fn one_entry_multi_file_uses_same_pipeline_and_publishes_a_tree() {
    let payload = (0..(2 * MIN_PAYLOAD_ALLOWANCE + 509))
        .map(|index| ((index * 41 + index / 29) & 0xff) as u8)
        .collect::<Vec<_>>();
    let info = one_entry_multi_file_info(&payload, MIN_PAYLOAD_ALLOWANCE);
    let metainfo = Metainfo::from_info_bytes(&info).expect("one-entry multi-file metainfo");
    let pieces = payload
        .chunks(MIN_PAYLOAD_ALLOWANCE)
        .map(<[u8]>::to_vec)
        .collect::<Vec<_>>();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind one-entry peer");
    let address = listener.local_addr().expect("one-entry peer address");
    let peer_task = tokio::spawn(serve_content_peer(
        listener,
        metainfo.info_hash,
        Arc::new(pieces),
        vec![true; metainfo.piece_count()],
    ));
    let output = test_path("one-entry-multi");
    let mut peers = TorrentPeerCoordinator::from_endpoint(
        address,
        PeerSource::Manual,
        loopback_network(Duration::from_secs(2)),
    )
    .expect("peer session");

    let report = run_content_download(
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
    )
    .await
    .expect("one-entry multi-file completion");

    assert_eq!(report.bytes_written, payload.len());
    assert!(output.is_dir());
    assert_eq!(
        tokio::fs::read(output.join("payload.bin"))
            .await
            .expect("read one-entry publication"),
        payload
    );
    timeout(Duration::from_secs(1), peer_task)
        .await
        .expect("one-entry peer joined")
        .expect("one-entry peer task");
    tokio::fs::remove_dir_all(output)
        .await
        .expect("remove one-entry publication");
}

#[tokio::test]
async fn full_recheck_recovers_synced_single_file_with_empty_have() {
    let payload = (0..(3 * MIN_PAYLOAD_ALLOWANCE + 731))
        .map(|index| ((index * 43 + index / 17) & 0xff) as u8)
        .collect::<Vec<_>>();
    let raw_info = single_file_info_with_piece_length(&payload, 2 * MIN_PAYLOAD_ALLOWANCE);
    let metainfo = Metainfo::from_info_bytes(&raw_info).expect("recheck single-file metainfo");
    let root = test_path("full-recheck-empty-have");
    tokio::fs::create_dir(&root)
        .await
        .expect("create storage root");
    let paths = torrent_storage_paths_for_metainfo(&root, &metainfo).expect("plan managed storage");
    let layout = TorrentLayout::from_metainfo(&metainfo);
    let selection = FileSelection::new(&layout, &[]).expect("all files wanted");
    let mut storage = SelectiveStorage::create_with_paths(
        paths.clone(),
        &metainfo,
        layout.clone(),
        selection.clone(),
    )
    .await
    .expect("create managed staging storage");
    for piece_index in 0..layout.piece_count() {
        let piece_index_u32 = u32::try_from(piece_index).expect("bounded piece index");
        let piece_offset = piece_index * metainfo.piece_length as usize;
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
                .expect("write uncheckpointed piece bytes");
        }
        storage
            .sync_piece(piece_index_u32)
            .await
            .expect("sync before simulated process death");
        assert_eq!(
            storage
                .hash_piece(piece_index_u32)
                .await
                .expect("hash staged fixture"),
            metainfo.piece_hashes[piece_index]
        );
    }
    drop(storage);
    assert!(paths.staging.exists());
    assert!(!paths.output.exists());

    let unused_peer = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind unused recheck peer");
    let peer_address = unused_peer.local_addr().expect("unused peer address");
    let checkpoints = Arc::new(RecordingCheckpointSink::default());
    let result = resume_magnet(
        ResumableMagnetDownloadConfig {
            magnet: format!(
                "magnet:?xt=urn:btih:{}&x.pe={peer_address}",
                hex(&metainfo.info_hash)
            ),
            storage_root: root.clone(),
            network: loopback_network(Duration::from_secs(1)),
            resource_limits: resource_limits(2 * MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            verified_info: Some(raw_info),
            verified_pieces: vec![false; layout.piece_count()],
            artifact_state: ResumeArtifactState::Staging,
            download_missing: true,
            dht: None,
        },
        checkpoints.clone(),
    )
    .await;
    assert!(
        result.is_ok(),
        "recover synced pieces without redownload: {result:?}; rechecks: {:?}",
        checkpoints.rechecks()
    );
    let report = result.expect("successful result checked above");

    assert_eq!(report.verified_piece_count, layout.piece_count());
    assert_eq!(report.bytes_written, 0);
    assert_eq!(
        checkpoints.rechecks(),
        vec![vec![true; layout.piece_count()]]
    );
    assert_eq!(
        tokio::fs::read(&paths.output)
            .await
            .expect("published recovered payload"),
        payload
    );
    assert!(!paths.staging.exists());
    tokio::fs::remove_dir_all(root)
        .await
        .expect("remove recheck fixture");
}

#[tokio::test]
async fn full_recheck_clears_stale_have_and_redownloads_only_corrupt_piece() {
    let payload = (0..(2 * MIN_PAYLOAD_ALLOWANCE))
        .map(|index| ((index * 29 + index / 11) & 0xff) as u8)
        .collect::<Vec<_>>();
    let raw_info = single_file_info_with_piece_length(&payload, MIN_PAYLOAD_ALLOWANCE);
    let metainfo = Metainfo::from_info_bytes(&raw_info).expect("stale-have metainfo");
    let pieces = Arc::new(
        payload
            .chunks(MIN_PAYLOAD_ALLOWANCE)
            .map(<[u8]>::to_vec)
            .collect::<Vec<_>>(),
    );
    let root = test_path("full-recheck-stale-have");
    tokio::fs::create_dir(&root)
        .await
        .expect("create storage root");
    let paths = torrent_storage_paths_for_metainfo(&root, &metainfo).expect("plan managed storage");
    let layout = TorrentLayout::from_metainfo(&metainfo);
    let selection = FileSelection::new(&layout, &[]).expect("all files wanted");
    let mut storage = SelectiveStorage::create_with_paths(
        paths.clone(),
        &metainfo,
        layout.clone(),
        selection.clone(),
    )
    .await
    .expect("create staged payload");
    for piece_index in 0..layout.piece_count() {
        let piece_index_u32 = u32::try_from(piece_index).expect("bounded piece index");
        storage
            .write_block(piece_index_u32, 0, pieces[piece_index].clone())
            .await
            .expect("write staged piece");
        storage
            .sync_piece(piece_index_u32)
            .await
            .expect("sync staged piece");
    }
    drop(storage);

    let mut staged = tokio::fs::OpenOptions::new()
        .write(true)
        .open(&paths.staging)
        .await
        .expect("open staged file for corruption");
    staged
        .seek(std::io::SeekFrom::Start(MIN_PAYLOAD_ALLOWANCE as u64))
        .await
        .expect("seek corrupt piece");
    staged
        .write_all(&[pieces[1][0] ^ 0xff])
        .await
        .expect("corrupt staged piece");
    staged.sync_all().await.expect("sync corruption");
    drop(staged);

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind repair peer");
    let peer_address = listener.local_addr().expect("repair peer address");
    let (requested_sender, mut requested_receiver) = mpsc::unbounded_channel();
    let peer_task = tokio::spawn(serve_content_peer_recording(
        listener,
        metainfo.info_hash,
        pieces,
        vec![true; layout.piece_count()],
        Duration::from_secs(2),
        Some(requested_sender),
    ));
    let checkpoints = Arc::new(RecordingCheckpointSink::default());
    let report = resume_magnet(
        ResumableMagnetDownloadConfig {
            magnet: format!(
                "magnet:?xt=urn:btih:{}&x.pe={peer_address}",
                hex(&metainfo.info_hash)
            ),
            storage_root: root.clone(),
            network: loopback_network(Duration::from_secs(2)),
            resource_limits: resource_limits(2 * MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            verified_info: Some(raw_info),
            verified_pieces: vec![true; layout.piece_count()],
            artifact_state: ResumeArtifactState::Staging,
            download_missing: true,
            dht: None,
        },
        checkpoints.clone(),
    )
    .await
    .expect("repair corrupt staged piece");
    timeout(Duration::from_secs(1), peer_task)
        .await
        .expect("repair peer joined")
        .expect("repair peer task");
    let requested = std::iter::from_fn(|| requested_receiver.try_recv().ok()).collect::<Vec<_>>();

    assert_eq!(checkpoints.rechecks(), vec![vec![true, false]]);
    assert!(!requested.is_empty());
    assert!(requested.iter().all(|piece_index| *piece_index == 1));
    assert_eq!(report.bytes_written, MIN_PAYLOAD_ALLOWANCE);
    assert_eq!(report.verified_piece_count, 2);
    assert_eq!(
        tokio::fs::read(&paths.output)
            .await
            .expect("read repaired publication"),
        payload
    );
    tokio::fs::remove_dir_all(root)
        .await
        .expect("remove stale-have fixture");
}

#[tokio::test]
async fn cancelling_full_recheck_stops_admission_and_joins_bounded_hashes() {
    let payload = (0..(8 * MIN_PAYLOAD_ALLOWANCE))
        .map(|index| ((index * 37 + index / 19) & 0xff) as u8)
        .collect::<Vec<_>>();
    let raw_info = single_file_info_with_piece_length(&payload, MIN_PAYLOAD_ALLOWANCE);
    let metainfo = Metainfo::from_info_bytes(&raw_info).expect("cancel recheck metainfo");
    let root = test_path("full-recheck-cancel");
    tokio::fs::create_dir(&root)
        .await
        .expect("create storage root");
    let paths = torrent_storage_paths_for_metainfo(&root, &metainfo).expect("plan managed storage");
    let layout = TorrentLayout::from_metainfo(&metainfo);
    let selection = FileSelection::new(&layout, &[]).expect("all files wanted");
    let mut storage =
        SelectiveStorage::create_with_paths(paths.clone(), &metainfo, layout.clone(), selection)
            .await
            .expect("create staged payload");
    for piece_index in 0..layout.piece_count() {
        let piece_index_u32 = u32::try_from(piece_index).expect("bounded piece index");
        let offset = piece_index * MIN_PAYLOAD_ALLOWANCE;
        storage
            .write_block(
                piece_index_u32,
                0,
                payload[offset..offset + MIN_PAYLOAD_ALLOWANCE].to_vec(),
            )
            .await
            .expect("write staged piece");
        storage
            .sync_piece(piece_index_u32)
            .await
            .expect("sync staged piece");
    }
    drop(storage);

    let control = DownloadControl::new();
    control
        .set_storage_execution_limits_for_testing(1, 2)
        .expect("set bounded recheck concurrency");
    control.set_storage_hash_delay(Duration::from_millis(200));
    let task_control = control.clone();
    let checkpoints = Arc::new(RecordingCheckpointSink::default());
    let unused_peer = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind unused cancellation peer");
    let peer_address = unused_peer
        .local_addr()
        .expect("unused cancellation peer address");
    let task = tokio::spawn(resume_magnet_with_control(
        ResumableMagnetDownloadConfig {
            magnet: format!(
                "magnet:?xt=urn:btih:{}&x.pe={peer_address}",
                hex(&metainfo.info_hash)
            ),
            storage_root: root.clone(),
            network: loopback_network(Duration::from_secs(1)),
            resource_limits: resource_limits(2 * MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            verified_info: Some(raw_info),
            verified_pieces: vec![false; layout.piece_count()],
            artifact_state: ResumeArtifactState::Staging,
            download_missing: true,
            dht: None,
        },
        checkpoints,
        task_control,
    ));
    timeout(Duration::from_secs(1), async {
        loop {
            if control.snapshot().storage_hash_operations_active == 2 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("two recheck hashes did not become active");
    control.cancel();
    assert!(matches!(
        timeout(Duration::from_secs(2), task)
            .await
            .expect("cancelled recheck timed out")
            .expect("join cancelled recheck"),
        Err(DownloadError::Cancelled)
    ));
    let progress = control.snapshot();
    assert_eq!(progress.storage_hash_operations_started, 2);
    assert_eq!(progress.storage_hash_operations_completed, 2);
    assert_eq!(progress.storage_hash_operations_active, 0);
    assert_eq!(progress.storage_hash_operations_active_high_water, 2);
    assert!(paths.staging.exists());
    assert!(!paths.output.exists());
    tokio::fs::remove_dir_all(root)
        .await
        .expect("remove cancellation fixture");
}

#[tokio::test]
async fn publishing_intent_recovers_both_sides_of_atomic_rename() {
    let payload = (0..(2 * MIN_PAYLOAD_ALLOWANCE + 317))
        .map(|index| ((index * 23 + index / 5) & 0xff) as u8)
        .collect::<Vec<_>>();
    let raw_info = single_file_info_with_piece_length(&payload, MIN_PAYLOAD_ALLOWANCE);
    let metainfo = Metainfo::from_info_bytes(&raw_info).expect("publication fault metainfo");
    let layout = TorrentLayout::from_metainfo(&metainfo);

    for (label, point) in [
        ("after-intent", PublicationFailurePoint::AfterIntent),
        ("after-rename", PublicationFailurePoint::AfterRename),
    ] {
        let root = test_path(label);
        tokio::fs::create_dir(&root)
            .await
            .expect("create publication fault root");
        let paths = torrent_storage_paths_for_metainfo(&root, &metainfo)
            .expect("plan publication fault storage");
        stage_single_file_payload(&paths, &metainfo, &payload).await;
        let unused_peer = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind unused publication peer");
        let peer_address = unused_peer
            .local_addr()
            .expect("unused publication peer address");
        let failpoints = Arc::new(PublicationFailureSink {
            point,
            rechecked: Mutex::new(Vec::new()),
        });
        let failed = resume_magnet(
            ResumableMagnetDownloadConfig {
                magnet: format!(
                    "magnet:?xt=urn:btih:{}&x.pe={peer_address}",
                    hex(&metainfo.info_hash)
                ),
                storage_root: root.clone(),
                network: loopback_network(Duration::from_secs(1)),
                resource_limits: resource_limits(2 * MIN_PAYLOAD_ALLOWANCE),
                skip_files: Vec::new(),
                verified_info: Some(raw_info.clone()),
                verified_pieces: vec![false; layout.piece_count()],
                artifact_state: ResumeArtifactState::Staging,
                download_missing: true,
                dht: None,
            },
            failpoints.clone(),
        )
        .await;
        let rechecked = failpoints
            .rechecked
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        assert!(
            matches!(failed, Err(DownloadError::Checkpoint(_))),
            "unexpected publication failpoint result: {failed:?}; rechecked={rechecked:?}"
        );
        match point {
            PublicationFailurePoint::AfterIntent => {
                assert!(paths.staging.exists());
                assert!(!paths.output.exists());
            }
            PublicationFailurePoint::AfterRename => {
                assert!(!paths.staging.exists());
                assert!(paths.output.exists());
            }
        }

        let recovered = resume_magnet(
            ResumableMagnetDownloadConfig {
                magnet: format!(
                    "magnet:?xt=urn:btih:{}&x.pe={peer_address}",
                    hex(&metainfo.info_hash)
                ),
                storage_root: root.clone(),
                network: loopback_network(Duration::from_secs(1)),
                resource_limits: resource_limits(2 * MIN_PAYLOAD_ALLOWANCE),
                skip_files: Vec::new(),
                verified_info: Some(raw_info.clone()),
                verified_pieces: vec![false; layout.piece_count()],
                artifact_state: ResumeArtifactState::Publishing,
                download_missing: true,
                dht: None,
            },
            Arc::new(RecordingCheckpointSink::default()),
        )
        .await
        .expect("reconcile publication side after injected death");
        assert_eq!(recovered.bytes_written, 0);
        assert_eq!(recovered.verified_piece_count, layout.piece_count());
        assert_eq!(
            tokio::fs::read(&paths.output)
                .await
                .expect("read recovered publication"),
            payload
        );
        assert!(!paths.staging.exists());
        tokio::fs::remove_dir_all(root)
            .await
            .expect("remove publication fault root");
    }
}

#[tokio::test]
async fn capable_peer_grows_pipeline_beyond_initial_request_window() {
    let payload = Arc::new(
        (0..(32 * MIN_PAYLOAD_ALLOWANCE))
            .map(|index| ((index * 31 + index / 23) & 0xff) as u8)
            .collect::<Vec<_>>(),
    );
    let info = single_file_info_with_piece_length(&payload, payload.len());
    let metainfo = Metainfo::from_info_bytes(&info).expect("window probe metainfo");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind window probe peer");
    let address = listener.local_addr().expect("window probe address");
    let max_pending = Arc::new(AtomicUsize::new(0));
    let peer_task = tokio::spawn(serve_window_probe_peer(
        listener,
        metainfo.info_hash,
        payload.clone(),
        max_pending.clone(),
    ));
    let output = test_path("adaptive-window.bin");
    let mut peers = TorrentPeerCoordinator::from_endpoint(
        address,
        PeerSource::Manual,
        loopback_network(Duration::from_secs(2)),
    )
    .expect("peer session");
    let control = DownloadControl::new();
    let payload_limit = payload.len();

    let report = timeout(
        Duration::from_secs(5),
        run_content_download(
            ContentDownloadConfig {
                output_path: output.clone(),
                max_buffered_payload_bytes: payload_limit,
                swarm_config: SwarmConfig::for_request_limit(payload_limit),
                skip_files: Vec::new(),
                materialize_files: Vec::new(),
            },
            metainfo,
            control.clone(),
            None,
            &mut peers,
            None,
        ),
    )
    .await
    .expect("bounded window probe")
    .expect("window probe completion");

    assert_eq!(report.verified_piece_count, 1);
    assert_eq!(report.bytes_written, payload.len());
    assert!(
        max_pending.load(Ordering::Acquire) > DEFAULT_INITIAL_REQUESTS_PER_CONNECTION,
        "peer never observed the request window grow"
    );
    assert!(
        report.outstanding_request_high_water
            > DEFAULT_INITIAL_REQUESTS_PER_CONNECTION * MIN_PAYLOAD_ALLOWANCE
    );
    assert!(report.outstanding_request_high_water <= payload_limit);
    assert!(report.payload_high_water <= payload_limit);
    let swarm = control
        .diagnostic_snapshot()
        .swarm
        .expect("window diagnostics");
    assert!(swarm.request_target_max > DEFAULT_INITIAL_REQUESTS_PER_CONNECTION);
    assert_eq!(swarm.useful_payload_bytes, payload.len());
    assert_eq!(tokio::fs::read(&output).await.expect("output"), *payload);
    timeout(Duration::from_secs(1), peer_task)
        .await
        .expect("window peer joined")
        .expect("window peer task");
    let _ = tokio::fs::remove_file(output).await;
}

#[tokio::test]
async fn request_pipeline_exceeds_independently_bounded_resident_payload() {
    let payload = Arc::new(
        (0..(8 * MIN_PAYLOAD_ALLOWANCE))
            .map(|index| ((index * 17 + index / 11) & 0xff) as u8)
            .collect::<Vec<_>>(),
    );
    let info = single_file_info_with_piece_length(&payload, payload.len());
    let metainfo = Metainfo::from_info_bytes(&info).expect("resource split metainfo");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind resource split peer");
    let address = listener.local_addr().expect("resource split address");
    let max_pending = Arc::new(AtomicUsize::new(0));
    let peer_task = tokio::spawn(serve_window_probe_peer(
        listener,
        metainfo.info_hash,
        payload.clone(),
        max_pending,
    ));
    let output = test_path("independent-resource-budgets.bin");
    let mut peers = TorrentPeerCoordinator::from_endpoint(
        address,
        PeerSource::Manual,
        loopback_network(Duration::from_secs(2)),
    )
    .expect("peer session");
    let control = DownloadControl::new();
    control.set_storage_write_delay(Duration::from_millis(25));

    let report = timeout(
        Duration::from_secs(5),
        run_content_download(
            ContentDownloadConfig {
                output_path: output.clone(),
                max_buffered_payload_bytes: 2 * MIN_PAYLOAD_ALLOWANCE,
                swarm_config: SwarmConfig::for_request_limit(8 * MIN_PAYLOAD_ALLOWANCE),
                skip_files: Vec::new(),
                materialize_files: Vec::new(),
            },
            metainfo,
            control.clone(),
            None,
            &mut peers,
            None,
        ),
    )
    .await
    .expect("resource split deadline")
    .expect("resource split completion");

    assert_eq!(report.verified_piece_count, 1);
    assert!(report.payload_high_water <= 2 * MIN_PAYLOAD_ALLOWANCE);
    assert!(report.outstanding_request_high_water >= 4 * MIN_PAYLOAD_ALLOWANCE);
    assert!(report.outstanding_request_high_water > report.payload_high_water);
    assert_eq!(control.snapshot().buffered_payload_bytes, 0);
    timeout(Duration::from_secs(1), peer_task)
        .await
        .expect("resource split peer joined")
        .expect("resource split peer task");
    let _ = tokio::fs::remove_file(output).await;
}

#[tokio::test]
async fn multi_peer_split_availability_completes_and_joins_every_socket() {
    let first = vec![0x31; 16 * 1024];
    let second = vec![0x72; 16 * 1024];
    let metainfo =
        Metainfo::from_bytes(&two_piece_metainfo(&first, &second)).expect("two-piece metainfo");
    let peers_payload = Arc::new(vec![first.clone(), second.clone()]);
    let first_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind A");
    let first_address = first_listener.local_addr().expect("address A");
    let second_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind B");
    let second_address = second_listener.local_addr().expect("address B");
    let peer_a = tokio::spawn(serve_content_peer(
        first_listener,
        metainfo.info_hash,
        peers_payload.clone(),
        vec![true, false],
    ));
    let peer_b = tokio::spawn(serve_content_peer(
        second_listener,
        metainfo.info_hash,
        peers_payload,
        vec![false, true],
    ));

    let control = DownloadControl::new();
    let output = test_path("split-availability");
    let mut peers = TorrentPeerCoordinator::from_endpoint(
        first_address,
        PeerSource::Manual,
        loopback_network(Duration::from_secs(2)),
    )
    .expect("peer session");
    peers
        .observe_address(second_address, PeerSource::Manual)
        .expect("second peer");
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
            control,
            None,
            &mut peers,
            None,
        ),
    )
    .await
    .expect("bounded multi-peer download")
    .expect("multi-peer download");

    assert_eq!(report.block_count, 2);
    assert_eq!(report.verified_piece_count, 2);
    assert!(report.payload_high_water <= 2 * MIN_PAYLOAD_ALLOWANCE);
    assert_eq!(
        tokio::fs::read(output.join("a")).await.expect("file A"),
        first
    );
    assert_eq!(
        tokio::fs::read(output.join("b")).await.expect("file B"),
        second
    );
    timeout(Duration::from_secs(1), peer_a)
        .await
        .expect("peer A joined")
        .expect("peer A task");
    timeout(Duration::from_secs(1), peer_b)
        .await
        .expect("peer B joined")
        .expect("peer B task");
    let _ = tokio::fs::remove_dir_all(output).await;
}

#[tokio::test]
async fn slow_storage_preserves_multi_peer_resident_payload_bound() {
    let first = vec![0x29; 16 * 1024];
    let second = vec![0xe3; 16 * 1024];
    let metainfo =
        Metainfo::from_bytes(&two_piece_metainfo(&first, &second)).expect("two-piece metainfo");
    let payload = Arc::new(vec![first, second]);
    let mut addresses = Vec::new();
    let mut tasks = Vec::new();
    for _ in 0..2 {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind storage-pressure peer");
        addresses.push(listener.local_addr().expect("storage-pressure address"));
        tasks.push(tokio::spawn(serve_content_peer(
            listener,
            metainfo.info_hash,
            payload.clone(),
            vec![true, true],
        )));
    }
    let mut peers = TorrentPeerCoordinator::from_endpoint(
        addresses[0],
        PeerSource::Manual,
        loopback_network(Duration::from_secs(2)),
    )
    .expect("peer session");
    peers
        .observe_address(addresses[1], PeerSource::Manual)
        .expect("second peer");
    let control = DownloadControl::new();
    control.set_storage_write_delay(Duration::from_millis(250));
    let task_control = control.clone();
    let output = test_path("slow-storage-multi-peer");
    let task_output = output.clone();
    let mut download = tokio::spawn(async move {
        run_content_download(
            ContentDownloadConfig {
                output_path: task_output,
                max_buffered_payload_bytes: 2 * MIN_PAYLOAD_ALLOWANCE,
                swarm_config: SwarmConfig::for_request_limit(2 * MIN_PAYLOAD_ALLOWANCE),
                skip_files: Vec::new(),
                materialize_files: Vec::new(),
            },
            metainfo,
            task_control,
            None,
            &mut peers,
            None,
        )
        .await
    });

    timeout(Duration::from_secs(2), async {
        loop {
            if control.snapshot().received_bytes >= MIN_PAYLOAD_ALLOWANCE {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("first payload reached the supervisor");
    timeout(Duration::from_millis(100), async {
        loop {
            if control.snapshot().received_bytes >= 2 * MIN_PAYLOAD_ALLOWANCE {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("second peer progressed while the first storage write was delayed");
    let active = control.snapshot();
    assert!(active.storage_active_write_micros.is_some());
    assert_eq!(active.storage_active_hash_micros, None);
    assert!((1..=2).contains(&active.storage_write_operations_started));
    assert_eq!(active.storage_write_operations_completed, 0);
    assert!((1..=2).contains(&active.storage_write_operations_active));
    assert!((1..=2).contains(&active.storage_write_operations_active_high_water));

    let report = timeout(Duration::from_secs(3), &mut download)
        .await
        .expect("bounded slow-storage download")
        .expect("download task")
        .expect("slow-storage completion");

    assert_eq!(report.verified_piece_count, 2);
    assert!(report.payload_high_water <= 2 * MIN_PAYLOAD_ALLOWANCE);
    let progress = control.snapshot();
    assert_eq!(progress.storage_jobs_pending, 0);
    assert!(progress.storage_jobs_high_water >= 2);
    let job_limit = content_storage_job_limit(2 * MIN_PAYLOAD_ALLOWANCE);
    assert!(progress.storage_command_queue_high_water <= job_limit);
    assert!(progress.storage_completion_queue_high_water <= job_limit);
    assert!((1..=2).contains(&progress.storage_write_operations_started));
    assert_eq!(
        progress.storage_write_operations_started,
        progress.storage_write_operations_completed
    );
    assert_eq!(progress.storage_write_blocks_started, 2);
    assert_eq!(progress.storage_write_blocks_completed, 2);
    assert!((1..=2).contains(&progress.storage_write_batch_blocks_high_water));
    assert!(
        (MIN_PAYLOAD_ALLOWANCE..=2 * MIN_PAYLOAD_ALLOWANCE)
            .contains(&progress.storage_write_batch_bytes_high_water)
    );
    assert!(
        progress.storage_write_service_micros
            >= progress.storage_write_operations_started as u64 * 200_000
    );
    assert!(progress.storage_write_service_max_micros >= 200_000);
    assert_eq!(progress.storage_hash_operations_started, 2);
    assert_eq!(progress.storage_hash_operations_completed, 2);
    assert_eq!(progress.storage_active_write_micros, None);
    assert_eq!(progress.storage_active_hash_micros, None);
    for task in tasks {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("storage-pressure peer joined")
            .expect("storage-pressure peer task");
    }
    let _ = tokio::fs::remove_dir_all(output).await;
}

#[tokio::test]
async fn cancellation_joins_storage_with_queued_writes() {
    let payload = (0..(2 * MIN_PAYLOAD_ALLOWANCE))
        .map(|index| ((index * 17 + index / 13) & 0xff) as u8)
        .collect::<Vec<_>>();
    let payload_len = payload.len();
    let metainfo =
        Metainfo::from_info_bytes(&single_file_info_with_piece_length(&payload, payload.len()))
            .expect("queued-write metainfo");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind queued-write peer");
    let address = listener.local_addr().expect("queued-write address");
    let peer_task = tokio::spawn(serve_content_peer(
        listener,
        metainfo.info_hash,
        Arc::new(vec![payload.clone()]),
        vec![true],
    ));
    let mut peers = TorrentPeerCoordinator::from_endpoint(
        address,
        PeerSource::Manual,
        loopback_network(Duration::from_secs(2)),
    )
    .expect("peer session");
    let control = DownloadControl::new();
    control.set_storage_write_delay(Duration::from_millis(250));
    let task_control = control.clone();
    let output = test_path("cancel-queued-storage.bin");
    let task_output = output.clone();
    let mut download = tokio::spawn(async move {
        run_content_download(
            ContentDownloadConfig {
                output_path: task_output,
                max_buffered_payload_bytes: payload_len,
                swarm_config: SwarmConfig::for_request_limit(payload_len),
                skip_files: Vec::new(),
                materialize_files: Vec::new(),
            },
            metainfo,
            task_control,
            None,
            &mut peers,
            None,
        )
        .await
    });

    timeout(Duration::from_secs(2), async {
        loop {
            let progress = control.snapshot();
            if progress.received_bytes == payload_len
                && progress.storage_jobs_pending >= 2
                && progress.storage_write_operations_active >= 1
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("two writes entered bounded storage ownership");
    let active = control.snapshot();
    assert!(active.storage_active_write_micros.is_some());
    assert_eq!(active.storage_active_hash_micros, None);
    assert!((1..=2).contains(&active.storage_write_operations_started));
    assert_eq!(active.storage_write_operations_completed, 0);
    assert!((1..=2).contains(&active.storage_write_operations_active));
    assert!((1..=2).contains(&active.storage_write_operations_active_high_water));
    control.cancel();
    let result = timeout(Duration::from_secs(1), &mut download)
        .await
        .expect("storage owner joined after queued-write cancellation")
        .expect("download task");
    assert!(matches!(result, Err(DownloadError::Cancelled)));
    let progress = control.snapshot();
    assert_eq!(progress.buffered_payload_bytes, 0);
    assert_eq!(progress.storage_jobs_pending, 0);
    assert!((1..=2).contains(&progress.storage_write_operations_started));
    assert_eq!(
        progress.storage_write_operations_started,
        progress.storage_write_operations_completed
    );
    assert_eq!(progress.storage_write_operations_active, 0);
    assert!((1..=2).contains(&progress.storage_write_blocks_started));
    assert_eq!(
        progress.storage_write_blocks_started,
        progress.storage_write_blocks_completed
    );
    assert!(progress.storage_write_batch_blocks_high_water > 0);
    assert!(
        progress.storage_write_batch_blocks_high_water <= progress.storage_write_blocks_started
    );
    assert!(progress.storage_write_service_micros >= 200_000);
    assert_eq!(progress.storage_hash_operations_started, 0);
    assert_eq!(progress.storage_active_write_micros, None);
    assert_eq!(progress.storage_active_hash_micros, None);
    assert!(!output.exists());
    timeout(Duration::from_secs(1), peer_task)
        .await
        .expect("queued-write peer joined")
        .expect("queued-write peer task");
    let _ = tokio::fs::remove_file(staging_path(&output).expect("staging path")).await;
}

#[tokio::test]
async fn cancellation_joins_storage_during_piece_hash() {
    let payload = vec![0x4d; MIN_PAYLOAD_ALLOWANCE];
    let metainfo =
        Metainfo::from_info_bytes(&single_file_info(&payload)).expect("hash-cancel metainfo");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind hash-cancel peer");
    let address = listener.local_addr().expect("hash-cancel address");
    let peer_task = tokio::spawn(serve_content_peer(
        listener,
        metainfo.info_hash,
        Arc::new(vec![payload]),
        vec![true],
    ));
    let mut peers = TorrentPeerCoordinator::from_endpoint(
        address,
        PeerSource::Manual,
        loopback_network(Duration::from_secs(2)),
    )
    .expect("peer session");
    let control = DownloadControl::new();
    control.set_storage_hash_delay(Duration::from_millis(250));
    let task_control = control.clone();
    let output = test_path("cancel-storage-hash.bin");
    let task_output = output.clone();
    let mut download = tokio::spawn(async move {
        run_content_download(
            ContentDownloadConfig {
                output_path: task_output,
                max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
                swarm_config: SwarmConfig::for_request_limit(MIN_PAYLOAD_ALLOWANCE),
                skip_files: Vec::new(),
                materialize_files: Vec::new(),
            },
            metainfo,
            task_control,
            None,
            &mut peers,
            None,
        )
        .await
    });

    timeout(Duration::from_secs(2), async {
        loop {
            if control.snapshot().storage_hashes_started == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("piece hash entered storage owner");
    let active = control.snapshot();
    assert_eq!(active.storage_active_write_micros, None);
    assert!(active.storage_active_hash_micros.is_some());
    assert_eq!(active.storage_hash_operations_started, 1);
    assert_eq!(active.storage_hash_operations_completed, 0);
    assert_eq!(active.storage_hash_operations_active, 1);
    assert_eq!(active.storage_hash_operations_active_high_water, 1);
    control.cancel();
    let result = timeout(Duration::from_secs(1), &mut download)
        .await
        .expect("storage owner joined after hash cancellation")
        .expect("download task");
    assert!(matches!(result, Err(DownloadError::Cancelled)));
    let progress = control.snapshot();
    assert_eq!(progress.buffered_payload_bytes, 0);
    assert_eq!(progress.storage_jobs_pending, 0);
    assert_eq!(progress.storage_hash_operations_started, 1);
    assert_eq!(progress.storage_hash_operations_completed, 1);
    assert_eq!(progress.storage_hash_operations_active, 0);
    assert!(progress.storage_hash_service_micros >= 200_000);
    assert_eq!(progress.storage_active_write_micros, None);
    assert_eq!(progress.storage_active_hash_micros, None);
    assert!(!output.exists());
    timeout(Duration::from_secs(1), peer_task)
        .await
        .expect("hash-cancel peer joined")
        .expect("hash-cancel peer task");
    let _ = tokio::fs::remove_file(staging_path(&output).expect("staging path")).await;
}

#[tokio::test]
async fn storage_executor_enforces_independent_write_and_hash_limits() {
    let block_length = MIN_PAYLOAD_ALLOWANCE;
    let output = test_path("storage-executor-limits.bin");
    let staging = staging_path(&output).expect("staging path");
    let control = DownloadControl::new();
    control.set_storage_write_delay(Duration::from_millis(300));
    let storage = single_file_content_storage(output.clone(), 5 * block_length, block_length).await;
    let mut pipeline = ContentStoragePipeline::start(storage, &control, 5 * block_length, None)
        .await
        .expect("start write-limit pipeline");
    for piece in 0..CONTENT_STORAGE_WRITE_CONCURRENCY {
        pipeline
            .enqueue(ContentStorageCommand::Write {
                block: BlockKey::new(piece as u32, 0, block_length as u32)
                    .expect("write-limit block"),
                generation: PieceGeneration::new(1).expect("generation"),
                offset: (piece * block_length) as u64,
                bytes: vec![piece as u8; block_length],
            })
            .expect("enqueue write-limit command");
        timeout(Duration::from_secs(1), async {
            loop {
                if control.snapshot().storage_write_operations_active == piece + 1 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("write reached its execution slot");
    }
    pipeline
        .enqueue(ContentStorageCommand::Write {
            block: BlockKey::new(
                CONTENT_STORAGE_WRITE_CONCURRENCY as u32,
                0,
                block_length as u32,
            )
            .expect("queued over-limit block"),
            generation: PieceGeneration::new(1).expect("generation"),
            offset: (CONTENT_STORAGE_WRITE_CONCURRENCY * block_length) as u64,
            bytes: vec![0xee; block_length],
        })
        .expect("enqueue over-limit write");
    tokio::time::sleep(Duration::from_millis(25)).await;
    let active = control.snapshot();
    assert_eq!(active.storage_write_operations_active, 4);
    assert_eq!(active.storage_write_operations_started, 4);
    assert_eq!(active.storage_write_operations_active_high_water, 4);
    let storage = timeout(Duration::from_secs(1), pipeline.shutdown(true))
        .await
        .expect("write-limit cancellation joined")
        .expect("write-limit storage returned");
    drop(storage);
    let finished = control.snapshot();
    assert_eq!(finished.storage_write_operations_active, 0);
    assert_eq!(finished.storage_write_operations_started, 4);
    assert_eq!(finished.storage_write_operations_completed, 4);
    let _ = tokio::fs::remove_file(&staging).await;

    let output = test_path("storage-hash-limits.bin");
    let staging = staging_path(&output).expect("hash staging path");
    let mut storage = single_file_content_storage(
        output.clone(),
        (CONTENT_STORAGE_HASH_CONCURRENCY + 1) * block_length,
        block_length,
    )
    .await;
    for piece in 0..=CONTENT_STORAGE_HASH_CONCURRENCY {
        storage
            .0
            .prepare_write(piece as u32, 0, vec![piece as u8; block_length])
            .await
            .expect("plan hash fixture write")
            .execute()
            .await
            .expect("write hash fixture");
    }
    let control = DownloadControl::new();
    control.set_storage_hash_delay(Duration::from_millis(300));
    let mut pipeline = ContentStoragePipeline::start(
        storage,
        &control,
        (CONTENT_STORAGE_HASH_CONCURRENCY + 1) * block_length,
        None,
    )
    .await
    .expect("start hash-limit pipeline");
    for piece in 0..CONTENT_STORAGE_HASH_CONCURRENCY {
        let expected: [u8; 20] = Sha1::digest(vec![piece as u8; block_length]).into();
        pipeline
            .enqueue(ContentStorageCommand::Verify {
                piece: piece as u32,
                generation: PieceGeneration::new(1).expect("generation"),
                length: block_length as u32,
                expected,
                durable: false,
            })
            .expect("enqueue hash-limit command");
        timeout(Duration::from_secs(1), async {
            loop {
                if control.snapshot().storage_hash_operations_active == piece + 1 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("hash reached its execution slot");
    }
    let piece = CONTENT_STORAGE_HASH_CONCURRENCY;
    let expected: [u8; 20] = Sha1::digest(vec![piece as u8; block_length]).into();
    pipeline
        .enqueue(ContentStorageCommand::Verify {
            piece: piece as u32,
            generation: PieceGeneration::new(1).expect("generation"),
            length: block_length as u32,
            expected,
            durable: false,
        })
        .expect("enqueue over-limit hash");
    tokio::time::sleep(Duration::from_millis(25)).await;
    let active = control.snapshot();
    assert_eq!(active.storage_hash_operations_active, 4);
    assert_eq!(active.storage_hash_operations_started, 4);
    assert_eq!(active.storage_hash_operations_active_high_water, 4);
    let storage = timeout(Duration::from_secs(1), pipeline.shutdown(true))
        .await
        .expect("hash-limit cancellation joined")
        .expect("hash-limit storage returned");
    drop(storage);
    let finished = control.snapshot();
    assert_eq!(finished.storage_hash_operations_active, 0);
    assert_eq!(finished.storage_hash_operations_started, 4);
    assert_eq!(finished.storage_hash_operations_completed, 4);
    let _ = tokio::fs::remove_file(&staging).await;
}

#[tokio::test]
async fn storage_executor_overlaps_classes_and_survives_full_completion_queue() {
    let block_length = MIN_PAYLOAD_ALLOWANCE;
    let output = test_path("storage-cross-class.bin");
    let staging = staging_path(&output).expect("cross-class staging path");
    let mut storage =
        single_file_content_storage(output.clone(), 2 * block_length, block_length).await;
    storage
        .0
        .prepare_write(0, 0, vec![0x51; block_length])
        .await
        .expect("plan first fixture write")
        .execute()
        .await
        .expect("write first fixture");
    let control = DownloadControl::new();
    control.set_storage_hash_delay(Duration::from_millis(250));
    control.set_storage_write_delay(Duration::from_millis(250));
    let mut pipeline = ContentStoragePipeline::start(storage, &control, 2 * block_length, None)
        .await
        .expect("start cross-class pipeline");
    let expected: [u8; 20] = Sha1::digest(vec![0x51; block_length]).into();
    pipeline
        .enqueue(ContentStorageCommand::Verify {
            piece: 0,
            generation: PieceGeneration::new(1).expect("generation"),
            length: block_length as u32,
            expected,
            durable: false,
        })
        .expect("enqueue delayed hash");
    timeout(Duration::from_secs(1), async {
        loop {
            if control.snapshot().storage_hash_operations_active == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("hash became active");
    pipeline
        .enqueue(ContentStorageCommand::Write {
            block: BlockKey::new(1, 0, block_length as u32).expect("cross-class block"),
            generation: PieceGeneration::new(1).expect("generation"),
            offset: block_length as u64,
            bytes: vec![0xa7; block_length],
        })
        .expect("enqueue overlapping write");
    timeout(Duration::from_secs(1), async {
        loop {
            let progress = control.snapshot();
            if progress.storage_hash_operations_active == 1
                && progress.storage_write_operations_active == 1
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("write and hash overlapped");
    let storage = timeout(Duration::from_secs(1), pipeline.shutdown(true))
        .await
        .expect("cross-class cancellation joined")
        .expect("cross-class storage returned");
    drop(storage);
    let _ = tokio::fs::remove_file(&staging).await;

    let output = test_path("storage-full-completion.bin");
    let staging = staging_path(&output).expect("completion staging path");
    let mut storage =
        single_file_content_storage(output.clone(), 2 * block_length, block_length).await;
    for piece in 0..2 {
        storage
            .0
            .prepare_write(piece as u32, 0, vec![piece as u8; block_length])
            .await
            .expect("plan completion fixture")
            .execute()
            .await
            .expect("write completion fixture");
    }
    let control = DownloadControl::new();
    let mut pipeline = ContentStoragePipeline::start(storage, &control, block_length, None)
        .await
        .expect("start one-slot completion pipeline");
    for piece in 0..2 {
        let expected: [u8; 20] = Sha1::digest(vec![piece as u8; block_length]).into();
        pipeline
            .enqueue(ContentStorageCommand::Verify {
                piece: piece as u32,
                generation: PieceGeneration::new(1).expect("generation"),
                length: block_length as u32,
                expected,
                durable: false,
            })
            .expect("enqueue completion saturation hash");
        timeout(Duration::from_secs(1), async {
            loop {
                if control.snapshot().storage_hash_operations_completed == piece + 1 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("hash completed despite undrained completion channel");
    }
    assert_eq!(control.snapshot().storage_completion_queue_high_water, 1);
    let storage = timeout(Duration::from_secs(1), pipeline.shutdown(true))
        .await
        .expect("saturated completion cancellation joined")
        .expect("saturated completion storage returned");
    drop(storage);
    let finished = control.snapshot();
    assert_eq!(finished.storage_hash_operations_started, 2);
    assert_eq!(finished.storage_hash_operations_completed, 2);
    assert_eq!(finished.storage_hash_operations_active, 0);
    let _ = tokio::fs::remove_file(&staging).await;
}

#[tokio::test]
async fn storage_command_backpressure_is_bounded_and_completes() {
    let payload = (0..(80 * MIN_PAYLOAD_ALLOWANCE))
        .map(|index| ((index * 23 + index / 29) & 0xff) as u8)
        .collect::<Vec<_>>();
    let metainfo =
        Metainfo::from_info_bytes(&single_file_info_with_piece_length(&payload, payload.len()))
            .expect("storage-pressure metainfo");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind storage-pressure peer");
    let address = listener.local_addr().expect("storage-pressure address");
    let peer_task = tokio::spawn(serve_content_peer(
        listener,
        metainfo.info_hash,
        Arc::new(vec![payload.clone()]),
        vec![true],
    ));
    let mut peers = TorrentPeerCoordinator::from_endpoint(
        address,
        PeerSource::Manual,
        loopback_network(Duration::from_secs(2)),
    )
    .expect("peer session");
    let control = DownloadControl::new();
    control.set_storage_write_delay(Duration::from_millis(3));
    let output = test_path("storage-command-pressure.bin");
    let job_limit = content_storage_job_limit(payload.len());

    let report = timeout(
        Duration::from_secs(5),
        run_content_download(
            ContentDownloadConfig {
                output_path: output.clone(),
                max_buffered_payload_bytes: payload.len(),
                swarm_config: SwarmConfig::for_request_limit(payload.len()),
                skip_files: Vec::new(),
                materialize_files: Vec::new(),
            },
            metainfo,
            control.clone(),
            None,
            &mut peers,
            None,
        ),
    )
    .await
    .expect("bounded storage-pressure deadline")
    .expect("storage-pressure completion");

    assert_eq!(report.verified_piece_count, 1);
    assert_eq!(tokio::fs::read(&output).await.expect("output"), payload);
    let progress = control.snapshot();
    assert_eq!(progress.storage_jobs_pending, 0);
    assert!(progress.storage_jobs_high_water <= job_limit);
    assert!(progress.storage_jobs_high_water > CONTENT_STORAGE_WRITE_BATCH_BLOCKS);
    assert!(progress.storage_command_queue_high_water <= job_limit);
    assert!(progress.storage_command_queue_high_water > 0);
    assert!(progress.storage_completion_queue_high_water <= job_limit);
    assert_eq!(progress.storage_write_blocks_started, 80);
    assert_eq!(progress.storage_write_blocks_completed, 80);
    assert!(progress.storage_write_operations_started < 80);
    assert_eq!(
        progress.storage_write_operations_started,
        progress.storage_write_operations_completed
    );
    assert!(progress.storage_write_batch_blocks_high_water > 1);
    assert!(progress.storage_write_batch_blocks_high_water <= CONTENT_STORAGE_WRITE_BATCH_BLOCKS);
    assert!(progress.storage_write_batch_bytes_high_water <= CONTENT_STORAGE_WRITE_BATCH_BYTES);
    timeout(Duration::from_secs(1), peer_task)
        .await
        .expect("storage-pressure peer joined")
        .expect("storage-pressure peer task");
    let _ = tokio::fs::remove_file(output).await;
}

#[tokio::test]
async fn storage_pressure_cannot_starve_dht_intake_or_dial_refill() {
    let payload = (0..(80 * MIN_PAYLOAD_ALLOWANCE))
        .map(|index| ((index * 31 + index / 37) & 0xff) as u8)
        .collect::<Vec<_>>();
    let metainfo =
        Metainfo::from_info_bytes(&single_file_info_with_piece_length(&payload, payload.len()))
            .expect("storage-pressure metainfo");
    let info_hash = metainfo.info_hash;
    let initial_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind initial peer");
    let initial_address = initial_listener.local_addr().expect("initial address");
    let initial_task = tokio::spawn(serve_content_peer(
        initial_listener,
        info_hash,
        Arc::new(vec![payload.clone()]),
        vec![true],
    ));
    let discovered_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind discovered peer");
    let discovered_address = discovered_listener
        .local_addr()
        .expect("discovered address");
    let discovered_task = tokio::spawn(serve_permanently_choked_peer(
        discovered_listener,
        info_hash,
        vec![0x80],
    ));
    let dht_socket = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind scripted DHT");
    let dht_address = dht_socket.local_addr().expect("DHT address");
    let release_dht = Arc::new(Notify::new());
    let dht_task = tokio::spawn(serve_dht_peer_after_signal(
        dht_socket,
        info_hash,
        discovered_address,
        release_dht.clone(),
    ));
    let dht = DhtService::start(dht_config(dht_address))
        .await
        .expect("start DHT client");
    let mut peers = TorrentPeerCoordinator::from_endpoint(
        initial_address,
        PeerSource::Manual,
        loopback_network(Duration::from_secs(2)),
    )
    .expect("peer session");
    peers.dht = Some(dht.handle());
    let control = DownloadControl::new();
    control.set_storage_write_delay(Duration::from_millis(5));
    let task_control = control.clone();
    let payload_limit = payload.len();
    let job_limit = content_storage_job_limit(payload_limit);
    let output = test_path("storage-pressure-dht-intake.bin");
    let task_output = output.clone();
    let download = tokio::spawn(async move {
        let result = run_content_download(
            ContentDownloadConfig {
                output_path: task_output,
                max_buffered_payload_bytes: payload_limit,
                swarm_config: SwarmConfig::for_request_limit(payload_limit),
                skip_files: Vec::new(),
                materialize_files: Vec::new(),
            },
            metainfo,
            task_control,
            None,
            &mut peers,
            None,
        )
        .await;
        (result, peers)
    });

    timeout(Duration::from_secs(2), async {
        loop {
            let disk = control.disk_snapshot();
            if disk.intake_backpressured {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("storage queue saturated");
    release_dht.notify_one();
    let intake_progress = timeout(Duration::from_millis(300), async {
        loop {
            let diagnostics = control.diagnostic_snapshot();
            if diagnostics.content_registry.is_some_and(|registry| {
                registry.total >= 2 && registry.dialing + registry.connected >= 2
            }) {
                break diagnostics.progress;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("DHT peer entered registry and dial cohort during storage pressure");
    assert!(intake_progress.storage_jobs_pending > 0);

    let (result, peers) = timeout(Duration::from_secs(5), download)
        .await
        .expect("storage-pressure download joined")
        .expect("download task");
    let report = result.expect("storage-pressure download");
    assert_eq!(report.verified_piece_count, 1);
    assert_eq!(
        tokio::fs::read(&output).await.expect("published output"),
        payload
    );
    let progress = control.snapshot();
    assert_eq!(progress.storage_jobs_pending, 0);
    assert!(progress.storage_jobs_high_water <= job_limit);
    assert!(progress.storage_jobs_high_water > 0);
    assert!(progress.storage_command_queue_high_water <= job_limit);
    assert!(progress.storage_command_queue_high_water > 0);
    let disk = control.disk_snapshot();
    assert_eq!(disk.pressure, DiskPressure::Idle);
    assert!(!disk.intake_backpressured);
    assert!(disk.pressure_transition_count >= 2);
    let discovered = peers
        .registry
        .find_endpoint(PeerEndpoint::new(discovered_address).expect("DHT endpoint"))
        .expect("DHT peer retained");
    assert!(discovered.sources().contains(PeerSource::Dht));
    assert!(discovered.history().dial_attempts >= 1);
    for task in [initial_task, discovered_task, dht_task] {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("scripted owner joined")
            .expect("scripted task");
    }
    dht.shutdown().await.expect("DHT shutdown");
    let _ = tokio::fs::remove_file(output).await;
}

#[tokio::test]
async fn endgame_cancel_reaches_loser_before_slow_storage_completes() {
    let payload = vec![0x7d; MIN_PAYLOAD_ALLOWANCE];
    let metainfo =
        Metainfo::from_info_bytes(&single_file_info(&payload)).expect("endgame metainfo");
    let loser_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind endgame loser");
    let loser_address = loser_listener.local_addr().expect("loser address");
    let winner_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind endgame winner");
    let winner_address = winner_listener.local_addr().expect("winner address");
    let requests_ready = Arc::new(Barrier::new(2));
    let loser = tokio::spawn(serve_endgame_loser(
        loser_listener,
        metainfo.info_hash,
        requests_ready.clone(),
    ));
    let winner = tokio::spawn(serve_endgame_winner(
        winner_listener,
        metainfo.info_hash,
        payload.clone(),
        requests_ready,
    ));
    let mut peers = TorrentPeerCoordinator::from_endpoint(
        loser_address,
        PeerSource::Manual,
        loopback_network(Duration::from_secs(2)),
    )
    .expect("peer session");
    peers
        .observe_address(winner_address, PeerSource::Manual)
        .expect("winner peer");
    let control = DownloadControl::new();
    control.set_storage_write_delay(Duration::from_millis(250));
    let task_control = control.clone();
    let output = test_path("endgame-cancel.bin");
    let task_output = output.clone();
    let mut download = tokio::spawn(async move {
        run_content_download(
            ContentDownloadConfig {
                output_path: task_output,
                max_buffered_payload_bytes: 2 * MIN_PAYLOAD_ALLOWANCE,
                swarm_config: SwarmConfig::for_request_limit(2 * MIN_PAYLOAD_ALLOWANCE),
                skip_files: Vec::new(),
                materialize_files: Vec::new(),
            },
            metainfo,
            task_control,
            None,
            &mut peers,
            None,
        )
        .await
    });

    let (request, cancel) = timeout(Duration::from_secs(2), loser)
        .await
        .expect("loser observed cancellation")
        .expect("loser task");
    assert_eq!(cancel, request);
    assert!(
        !download.is_finished(),
        "cancel must be emitted before the storage delay completes"
    );
    let report = timeout(Duration::from_secs(3), &mut download)
        .await
        .expect("endgame download deadline")
        .expect("download task")
        .expect("endgame completion");
    assert_eq!(report.verified_piece_count, 1);
    assert_eq!(report.payload_high_water, MIN_PAYLOAD_ALLOWANCE);
    assert_eq!(
        report.outstanding_request_high_water,
        2 * MIN_PAYLOAD_ALLOWANCE
    );
    assert_eq!(
        tokio::fs::read(&output).await.expect("endgame output"),
        payload
    );
    timeout(Duration::from_secs(1), winner)
        .await
        .expect("winner joined")
        .expect("winner task");
    let swarm = control
        .diagnostic_snapshot()
        .swarm
        .expect("terminal swarm diagnostics");
    assert_eq!(swarm.endgame_assignments, 1);
    assert_eq!(swarm.cancelled_request_attempts, 1);
    assert_eq!(swarm.active_request_attempts, 0);
    let _ = tokio::fs::remove_file(output).await;
}

#[tokio::test]
async fn sole_corrupt_source_is_banned_and_clean_peer_retries_piece() {
    let payload = (0..MIN_PAYLOAD_ALLOWANCE)
        .map(|index| ((index * 29 + index / 11) & 0xff) as u8)
        .collect::<Vec<_>>();
    let mut corrupt = payload.clone();
    corrupt[37] ^= 0x80;
    let metainfo =
        Metainfo::from_info_bytes(&single_file_info(&payload)).expect("hash retry metainfo");
    let corrupt_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind corrupt peer");
    let corrupt_address = corrupt_listener.local_addr().expect("corrupt address");
    let clean_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind clean peer");
    let clean_address = clean_listener.local_addr().expect("clean address");
    let corrupt_task = tokio::spawn(serve_content_peer(
        corrupt_listener,
        metainfo.info_hash,
        Arc::new(vec![corrupt]),
        vec![true],
    ));
    let clean_task = tokio::spawn(serve_content_peer(
        clean_listener,
        metainfo.info_hash,
        Arc::new(vec![payload.clone()]),
        vec![true],
    ));
    let mut peers = TorrentPeerCoordinator::from_endpoint(
        corrupt_address,
        PeerSource::Manual,
        loopback_network(Duration::from_secs(2)),
    )
    .expect("peer session");
    peers
        .observe_address(clean_address, PeerSource::Manual)
        .expect("clean peer");
    let mut swarm_config = SwarmConfig::for_request_limit(MIN_PAYLOAD_ALLOWANCE);
    swarm_config.max_established_connections = 1;
    swarm_config.max_pending_dials = 1;
    let control = DownloadControl::new();
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());
    let metrics = Arc::new(RecordingByteMetricSink::default());
    control.set_byte_metric_sink(metrics.clone());
    let output = test_path("piece-hash-retry.bin");

    let report = timeout(
        Duration::from_secs(3),
        run_content_download(
            ContentDownloadConfig {
                output_path: output.clone(),
                max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
                swarm_config,
                skip_files: Vec::new(),
                materialize_files: Vec::new(),
            },
            metainfo,
            control.clone(),
            None,
            &mut peers,
            None,
        ),
    )
    .await
    .expect("bounded hash recovery")
    .expect("clean peer completes failed piece");

    assert_eq!(report.verified_piece_count, 1);
    assert_eq!(report.selected_written_bytes, 2 * payload.len());
    assert_eq!(
        tokio::fs::read(&output).await.expect("published output"),
        payload
    );
    let snapshot = control
        .diagnostic_snapshot()
        .swarm
        .expect("hash failure diagnostics");
    assert_eq!(snapshot.piece_hash_failures, 1);
    assert_eq!(snapshot.failed_piece_bytes, MIN_PAYLOAD_ALLOWANCE);
    assert_eq!(snapshot.last_hash_failure_contributors, 1);
    assert_eq!(snapshot.active_request_attempts, 0);
    assert_eq!(snapshot.outstanding_request_bytes, 0);
    assert_eq!(
        metrics.bytes(ByteMetric::PayloadReceived),
        2 * payload.len() as u64
    );
    assert_eq!(
        metrics.bytes(ByteMetric::StagedWrite),
        2 * payload.len() as u64
    );
    assert_eq!(
        metrics.bytes(ByteMetric::PayloadVerified),
        payload.len() as u64
    );
    assert_eq!(
        metrics.bytes(ByteMetric::PayloadHashFailed),
        payload.len() as u64
    );
    assert_eq!(
        metrics.bytes(ByteMetric::LogicalHashRead),
        2 * payload.len() as u64
    );
    assert!(
        metrics.bytes(ByteMetric::PeerWireReceived) > 2 * payload.len() as u64,
        "peer wire bytes were {}",
        metrics.bytes(ByteMetric::PeerWireReceived),
    );
    assert_eq!(
        metrics.bytes(ByteMetric::PeerWireReceived),
        metrics.bytes(ByteMetric::PayloadReceived)
            + metrics.bytes(ByteMetric::PeerProtocolReceived)
            + metrics.bytes(ByteMetric::MetadataPayloadReceived)
            + metrics.bytes(ByteMetric::PeerUnclassifiedReceived),
    );
    assert_eq!(
        metrics.bytes(ByteMetric::PeerWireSent),
        metrics.bytes(ByteMetric::PeerProtocolSent)
            + metrics.bytes(ByteMetric::MetadataPayloadSent)
            + metrics.bytes(ByteMetric::PeerUnclassifiedSent),
    );
    let corrupt_record = peers
        .registry
        .find_endpoint(PeerEndpoint::new(corrupt_address).expect("corrupt endpoint"))
        .expect("corrupt record");
    assert_eq!(corrupt_record.phase(), crate::peer::PeerPhase::Banned);
    assert_eq!(corrupt_record.integrity().trust_points, -2);
    assert_eq!(corrupt_record.integrity().hash_failures, 1);
    let clean_record = peers
        .registry
        .find_endpoint(PeerEndpoint::new(clean_address).expect("clean endpoint"))
        .expect("clean record");
    assert_eq!(clean_record.integrity().trust_points, 1);
    assert_eq!(clean_record.integrity().valid_pieces, 1);
    {
        let events = activity
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadActivityEvent::PieceHashFailed {
                piece_index: 0,
                contributor_count: 1,
                failed_bytes: MIN_PAYLOAD_ALLOWANCE,
            }
        )));
        assert_eq!(
            events
                .iter()
                .filter_map(|event| match event {
                    DownloadActivityEvent::PieceStarted {
                        piece_index: 0,
                        attempt,
                        ..
                    } => Some(*attempt),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event,
                    DownloadActivityEvent::PieceHashing { piece_index: 0 }
                ))
                .count(),
            2
        );
    }
    for task in [corrupt_task, clean_task] {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("hash recovery peer joined")
            .expect("hash recovery peer task");
    }
    let _ = tokio::fs::remove_file(output).await;
}

#[tokio::test]
async fn ambiguous_corrupt_generation_records_suspects_without_false_bans() {
    let payload = (0..(2 * MIN_PAYLOAD_ALLOWANCE))
        .map(|index| ((index * 17 + index / 13) & 0xff) as u8)
        .collect::<Vec<_>>();
    let mut corrupt = payload.clone();
    corrupt[17] ^= 0x40;
    corrupt[MIN_PAYLOAD_ALLOWANCE + 17] ^= 0x40;
    let info = single_file_info_with_piece_length(&payload, payload.len());
    let metainfo = Metainfo::from_info_bytes(&info).expect("ambiguous hash metainfo");
    let first_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind first suspect");
    let first_address = first_listener.local_addr().expect("first suspect address");
    let second_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind second suspect");
    let second_address = second_listener
        .local_addr()
        .expect("second suspect address");
    let clean_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind clean generation");
    let clean_address = clean_listener.local_addr().expect("clean address");
    let first_task = tokio::spawn(serve_one_block_then_choke_peer(
        first_listener,
        metainfo.info_hash,
        Arc::new(corrupt),
    ));
    let second_task = tokio::spawn(serve_one_block_then_choke_peer(
        second_listener,
        metainfo.info_hash,
        Arc::new(payload.clone()),
    ));
    let clean_task = tokio::spawn(serve_content_peer(
        clean_listener,
        metainfo.info_hash,
        Arc::new(vec![payload.clone()]),
        vec![true],
    ));
    let mut peers = TorrentPeerCoordinator::from_endpoint(
        first_address,
        PeerSource::Manual,
        loopback_network(Duration::from_secs(2)),
    )
    .expect("peer session");
    peers
        .observe_address(second_address, PeerSource::Manual)
        .expect("second suspect");
    peers
        .observe_address(clean_address, PeerSource::Manual)
        .expect("clean peer");
    let mut swarm_config = SwarmConfig::for_request_limit(2 * MIN_PAYLOAD_ALLOWANCE);
    swarm_config.max_established_connections = 2;
    swarm_config.max_pending_dials = 1;
    swarm_config.unproductive_grace = Duration::from_millis(50);
    let control = DownloadControl::new();
    let output = test_path("ambiguous-piece-hash-retry.bin");

    let report = timeout(
        Duration::from_secs(3),
        run_content_download(
            ContentDownloadConfig {
                output_path: output.clone(),
                max_buffered_payload_bytes: 2 * MIN_PAYLOAD_ALLOWANCE,
                swarm_config,
                skip_files: Vec::new(),
                materialize_files: Vec::new(),
            },
            metainfo,
            control.clone(),
            None,
            &mut peers,
            None,
        ),
    )
    .await
    .expect("bounded ambiguous recovery")
    .expect("clean generation completes");

    assert_eq!(report.verified_piece_count, 1);
    assert_eq!(report.selected_written_bytes, 2 * payload.len());
    assert_eq!(
        tokio::fs::read(&output).await.expect("published output"),
        payload
    );
    let snapshot = control
        .diagnostic_snapshot()
        .swarm
        .expect("ambiguous hash diagnostics");
    assert_eq!(snapshot.piece_hash_failures, 1);
    assert_eq!(snapshot.failed_piece_bytes, payload.len());
    assert_eq!(snapshot.last_hash_failure_contributors, 2);
    for address in [first_address, second_address] {
        let record = peers
            .registry
            .find_endpoint(PeerEndpoint::new(address).expect("suspect endpoint"))
            .expect("suspect record");
        assert_ne!(record.phase(), crate::peer::PeerPhase::Banned);
        assert_eq!(record.integrity().trust_points, -2);
        assert_eq!(record.integrity().hash_failures, 1);
        assert!(record.integrity().on_parole);
    }
    let clean_record = peers
        .registry
        .find_endpoint(PeerEndpoint::new(clean_address).expect("clean endpoint"))
        .expect("clean record");
    assert_eq!(clean_record.integrity().trust_points, 1);
    for task in [first_task, second_task, clean_task] {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("ambiguous recovery peer joined")
            .expect("ambiguous recovery peer task");
    }
    let _ = tokio::fs::remove_file(output).await;
}

#[tokio::test]
async fn disconnect_and_choke_reassign_only_their_outstanding_blocks() {
    run_adverse_reassignment_case(AdverseRequestAction::Disconnect).await;
    run_adverse_reassignment_case(AdverseRequestAction::Choke).await;
}

#[tokio::test]
async fn useful_peer_at_end_of_full_pending_cohort_completes_promptly() {
    assert_eq!(DEFAULT_MAX_PENDING_DIALS, 30);
    let first = vec![0x13; 16 * 1024];
    let second = vec![0x57; 16 * 1024];
    let metainfo =
        Metainfo::from_bytes(&two_piece_metainfo(&first, &second)).expect("two-piece metainfo");
    let mut silent_addresses = Vec::new();
    let mut silent_tasks = Vec::new();
    for _ in 1..DEFAULT_MAX_PENDING_DIALS {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind silent peer");
        silent_addresses.push(listener.local_addr().expect("silent address"));
        silent_tasks.push(tokio::spawn(accept_handshake_without_reply(listener)));
    }
    let useful = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind useful peer");
    let useful_address = useful.local_addr().expect("useful address");
    let useful_task = tokio::spawn(serve_content_peer(
        useful,
        metainfo.info_hash,
        Arc::new(vec![first, second]),
        vec![true, true],
    ));
    let mut peers = TorrentPeerCoordinator::from_endpoint(
        silent_addresses[0],
        PeerSource::Manual,
        loopback_network(Duration::from_secs(5)),
    )
    .expect("peer session");
    for address in &silent_addresses[1..] {
        peers
            .observe_address(*address, PeerSource::Manual)
            .expect("silent peer");
    }
    peers
        .observe_address(useful_address, PeerSource::Manual)
        .expect("30th useful peer");
    let output = test_path("silent-handshake-parallel");
    let report = timeout(
        Duration::from_secs(2),
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
    .expect("silent handshakes did not serialize progress")
    .expect("useful peer completed");
    assert_eq!(report.verified_piece_count, 2);
    silent_tasks.push(useful_task);
    for task in silent_tasks {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("peer joined")
            .expect("peer task");
    }
    let _ = tokio::fs::remove_dir_all(output).await;
}

#[tokio::test]
async fn cancellation_joins_a_full_silent_pending_cohort() {
    let payload = vec![0x31; 16 * 1024];
    let metainfo =
        Metainfo::from_info_bytes(&single_file_info(&payload)).expect("single-piece metainfo");
    let accepted = Arc::new(AtomicUsize::new(0));
    let mut addresses = Vec::new();
    let mut tasks = Vec::new();
    for _ in 0..DEFAULT_MAX_PENDING_DIALS {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind silent peer");
        addresses.push(listener.local_addr().expect("silent address"));
        tasks.push(tokio::spawn(accept_handshake_without_reply_and_count(
            listener,
            Some(accepted.clone()),
        )));
    }
    let mut peers = TorrentPeerCoordinator::from_endpoint(
        addresses[0],
        PeerSource::Manual,
        loopback_network(Duration::from_secs(5)),
    )
    .expect("peer session");
    for address in &addresses[1..] {
        peers
            .observe_address(*address, PeerSource::Manual)
            .expect("silent peer");
    }
    let output = test_path("full-silent-pending-cancel.bin");
    let control = DownloadControl::new();
    let task_output = output.clone();
    let task_control = control.clone();
    let download = tokio::spawn(async move {
        let result = run_content_download(
            ContentDownloadConfig {
                output_path: task_output,
                max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
                swarm_config: SwarmConfig::for_request_limit(MIN_PAYLOAD_ALLOWANCE),
                skip_files: Vec::new(),
                materialize_files: Vec::new(),
            },
            metainfo,
            task_control,
            None,
            &mut peers,
            None,
        )
        .await;
        (result, peers)
    });
    let all_started = timeout(Duration::from_secs(2), async {
        while accepted.load(Ordering::Acquire) < DEFAULT_MAX_PENDING_DIALS {
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert!(
        all_started.is_ok(),
        "only {} of {DEFAULT_MAX_PENDING_DIALS} pending handshakes started",
        accepted.load(Ordering::Acquire)
    );
    control.cancel();
    let (result, peers) = timeout(Duration::from_secs(1), download)
        .await
        .expect("download joined")
        .expect("download task");
    assert!(matches!(result, Err(DownloadError::Cancelled)));
    assert_eq!(accepted.load(Ordering::Acquire), DEFAULT_MAX_PENDING_DIALS);
    assert!(
        peers
            .registry
            .records()
            .all(|record| record.phase() == PeerPhase::Idle)
    );
    for task in tasks {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("silent peer joined")
            .expect("silent peer task");
    }
    assert!(!output.exists());
    let staging = staging_path(&output).expect("staging path");
    assert!(
        !staging.exists(),
        "cancellation before the first write leaves no empty artifact"
    );
}

#[tokio::test]
async fn full_choked_set_is_replaced_by_an_eligible_useful_peer() {
    let first = vec![0x21; 16 * 1024];
    let second = vec![0x84; 16 * 1024];
    let metainfo =
        Metainfo::from_bytes(&two_piece_metainfo(&first, &second)).expect("two-piece metainfo");
    let mut addresses = Vec::new();
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind choked peer");
        addresses.push(listener.local_addr().expect("choked address"));
        tasks.push(tokio::spawn(serve_permanently_choked_peer(
            listener,
            metainfo.info_hash,
            vec![0xc0],
        )));
    }
    let useful_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind replacement peer");
    let useful_address = useful_listener.local_addr().expect("replacement address");
    tasks.push(tokio::spawn(serve_content_peer(
        useful_listener,
        metainfo.info_hash,
        Arc::new(vec![first, second]),
        vec![true, true],
    )));
    let mut peers = TorrentPeerCoordinator::from_endpoint(
        addresses[0],
        PeerSource::Manual,
        loopback_network(Duration::from_secs(2)),
    )
    .expect("peer session");
    for address in addresses.into_iter().skip(1) {
        peers
            .observe_address(address, PeerSource::Manual)
            .expect("choked peer");
    }
    peers
        .observe_address(useful_address, PeerSource::Manual)
        .expect("replacement peer");
    let mut swarm_config = SwarmConfig::for_request_limit(2 * MIN_PAYLOAD_ALLOWANCE);
    swarm_config.unproductive_grace = Duration::from_millis(100);
    let output = test_path("choked-capacity-replacement");
    let report = timeout(
        Duration::from_secs(3),
        run_content_download(
            ContentDownloadConfig {
                output_path: output.clone(),
                max_buffered_payload_bytes: 2 * MIN_PAYLOAD_ALLOWANCE,
                swarm_config,
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
    .expect("bounded replacement")
    .expect("replacement peer completed");
    assert_eq!(report.verified_piece_count, 2);
    for task in tasks {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("peer joined")
            .expect("peer task");
    }
    let _ = tokio::fs::remove_dir_all(output).await;
}

#[tokio::test]
async fn full_irrelevant_set_is_replaced_by_a_wanted_piece_peer() {
    let first = vec![0x18; 16 * 1024];
    let second = vec![0xa6; 16 * 1024];
    let metainfo =
        Metainfo::from_bytes(&two_piece_metainfo(&first, &second)).expect("two-piece metainfo");
    let payload = Arc::new(vec![first, second]);
    let mut addresses = Vec::new();
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind irrelevant peer");
        addresses.push(listener.local_addr().expect("irrelevant address"));
        tasks.push(tokio::spawn(serve_content_peer(
            listener,
            metainfo.info_hash,
            payload.clone(),
            vec![false, false],
        )));
    }
    let useful_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind wanted-piece peer");
    let useful_address = useful_listener.local_addr().expect("wanted-piece address");
    tasks.push(tokio::spawn(serve_content_peer(
        useful_listener,
        metainfo.info_hash,
        payload,
        vec![true, true],
    )));
    let mut peers = TorrentPeerCoordinator::from_endpoint(
        addresses[0],
        PeerSource::Manual,
        loopback_network(Duration::from_secs(2)),
    )
    .expect("peer session");
    for address in addresses.into_iter().skip(1) {
        peers
            .observe_address(address, PeerSource::Manual)
            .expect("irrelevant peer");
    }
    peers
        .observe_address(useful_address, PeerSource::Manual)
        .expect("wanted-piece peer");
    let mut swarm_config = SwarmConfig::for_request_limit(2 * MIN_PAYLOAD_ALLOWANCE);
    swarm_config.unproductive_grace = Duration::from_millis(50);
    let output = test_path("irrelevant-capacity-replacement");

    let report = timeout(
        Duration::from_secs(3),
        run_content_download(
            ContentDownloadConfig {
                output_path: output.clone(),
                max_buffered_payload_bytes: 2 * MIN_PAYLOAD_ALLOWANCE,
                swarm_config,
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
    .expect("bounded wanted-piece replacement")
    .expect("wanted-piece peer completed");

    assert_eq!(report.verified_piece_count, 2);
    for task in tasks {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("irrelevant peer joined")
            .expect("irrelevant peer task");
    }
    let _ = tokio::fs::remove_dir_all(output).await;
}

#[tokio::test]
async fn full_choked_set_without_an_alternative_waits_without_churn() {
    let payload = vec![0x5b; 16 * 1024];
    let metainfo =
        Metainfo::from_info_bytes(&single_file_info(&payload)).expect("single-piece metainfo");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind choked peer");
    let address = listener.local_addr().expect("choked address");
    let peer_task = tokio::spawn(serve_permanently_choked_peer(
        listener,
        metainfo.info_hash,
        vec![0x80],
    ));
    let mut peers = TorrentPeerCoordinator::from_endpoint(
        address,
        PeerSource::Manual,
        loopback_network(Duration::from_secs(2)),
    )
    .expect("peer session");
    let mut swarm_config = SwarmConfig::for_request_limit(MIN_PAYLOAD_ALLOWANCE);
    swarm_config.max_established_connections = 1;
    swarm_config.unproductive_grace = Duration::from_millis(50);
    let output = test_path("choked-no-alternative.bin");
    let control = DownloadControl::new();
    let result = {
        let mut download = Box::pin(run_content_download(
            ContentDownloadConfig {
                output_path: output.clone(),
                max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
                swarm_config,
                skip_files: Vec::new(),
                materialize_files: Vec::new(),
            },
            metainfo,
            control.clone(),
            None,
            &mut peers,
            None,
        ));
        assert!(
            timeout(Duration::from_millis(200), &mut download)
                .await
                .is_err(),
            "no-alternative state must wait"
        );
        control.cancel();
        download.await
    };
    assert!(matches!(result, Err(DownloadError::Cancelled)));
    let record = peers.registry.records().next().expect("retained peer");
    assert_eq!(record.history().dial_attempts, 1);
    assert_eq!(record.history().total_failures, 0);
    timeout(Duration::from_secs(1), peer_task)
        .await
        .expect("choked peer joined")
        .expect("choked peer task");
    let _ = tokio::fs::remove_file(staging_path(&output).expect("staging path")).await;
}

#[tokio::test]
async fn unrelated_messages_do_not_prevent_expiry_and_late_payload_is_safe() {
    let payload = vec![0x6a; 16 * 1024];
    let metainfo =
        Metainfo::from_info_bytes(&single_file_info(&payload)).expect("single-piece metainfo");
    let old_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind old peer");
    let old_address = old_listener.local_addr().expect("old address");
    let replacement_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind replacement peer");
    let replacement_address = replacement_listener
        .local_addr()
        .expect("replacement address");
    let old_task = tokio::spawn(serve_delayed_block_peer(
        old_listener,
        metainfo.info_hash,
        payload.clone(),
        Duration::from_millis(130),
        Some(Duration::from_millis(25)),
    ));
    let replacement_task = tokio::spawn(serve_delayed_block_peer(
        replacement_listener,
        metainfo.info_hash,
        payload.clone(),
        Duration::from_millis(100),
        None,
    ));
    let mut peers = TorrentPeerCoordinator::from_endpoint(
        old_address,
        PeerSource::Manual,
        loopback_network(Duration::from_secs(2)),
    )
    .expect("peer session");
    peers
        .observe_address(replacement_address, PeerSource::Manual)
        .expect("replacement peer");
    let mut swarm_config = SwarmConfig::for_request_limit(MIN_PAYLOAD_ALLOWANCE);
    swarm_config.request_timeout = Duration::from_millis(75);
    let output = test_path("late-request-payload.bin");
    let control = DownloadControl::new();
    let report = timeout(
        Duration::from_secs(3),
        run_content_download(
            ContentDownloadConfig {
                output_path: output.clone(),
                max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
                swarm_config,
                skip_files: Vec::new(),
                materialize_files: Vec::new(),
            },
            metainfo,
            control.clone(),
            None,
            &mut peers,
            None,
        ),
    )
    .await
    .expect("bounded expiry and late response")
    .expect("late response download");
    assert_eq!(report.verified_piece_count, 1);
    assert_eq!(report.payload_high_water, MIN_PAYLOAD_ALLOWANCE);
    assert!(control.snapshot().requested_bytes >= 2 * MIN_PAYLOAD_ALLOWANCE);
    assert_eq!(tokio::fs::read(&output).await.expect("output"), payload);
    for task in [old_task, replacement_task] {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("delayed peer joined")
            .expect("delayed peer task");
    }
    let _ = tokio::fs::remove_file(output).await;
}

#[tokio::test]
async fn sampled_stall_moves_a_burst_peers_window_to_a_healthy_peer() {
    let payload = (0..(8 * MIN_PAYLOAD_ALLOWANCE))
        .map(|index| ((index * 43 + index / 17) & 0xff) as u8)
        .collect::<Vec<_>>();
    let info = single_file_info_with_piece_length(&payload, payload.len());
    let metainfo = Metainfo::from_info_bytes(&info).expect("stall metainfo");
    let stalled_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stalled peer");
    let stalled_address = stalled_listener.local_addr().expect("stalled address");
    let useful_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind useful peer");
    let useful_address = useful_listener.local_addr().expect("useful address");
    let stalled_task = tokio::spawn(serve_delayed_block_peer_with_timeout(
        stalled_listener,
        metainfo.info_hash,
        payload.clone(),
        Duration::ZERO,
        None,
        Duration::from_secs(10),
    ));
    let useful_task = tokio::spawn(serve_content_peer_with_timeout(
        useful_listener,
        metainfo.info_hash,
        Arc::new(vec![payload.clone()]),
        vec![true],
        Duration::from_secs(10),
    ));
    let mut peers = TorrentPeerCoordinator::from_endpoint(
        stalled_address,
        PeerSource::Manual,
        loopback_network(Duration::from_secs(10)),
    )
    .expect("peer session");
    peers
        .observe_address(useful_address, PeerSource::Manual)
        .expect("useful peer");
    let payload_limit = payload.len();
    let mut swarm_config = SwarmConfig::for_request_limit(payload_limit);
    swarm_config.request_timeout = Duration::from_secs(10);
    let control = DownloadControl::new();
    let output = test_path("sampled-stall.bin");

    let report = timeout(
        Duration::from_secs(7),
        run_content_download(
            ContentDownloadConfig {
                output_path: output.clone(),
                max_buffered_payload_bytes: payload_limit,
                swarm_config,
                skip_files: Vec::new(),
                materialize_files: Vec::new(),
            },
            metainfo,
            control.clone(),
            None,
            &mut peers,
            None,
        ),
    )
    .await
    .expect("adaptive stall deadline")
    .expect("healthy peer completed stalled work");

    assert_eq!(report.verified_piece_count, 1);
    assert_eq!(tokio::fs::read(&output).await.expect("output"), payload);
    assert!(control.snapshot().requested_bytes > report.bytes_written);
    for task in [stalled_task, useful_task] {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("peer joined")
            .expect("peer task");
    }
    let _ = tokio::fs::remove_file(output).await;
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
        DEFAULT_ADVERTISED_PEER_PORT
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

#[tokio::test]
async fn tracker_only_magnet_discovers_registry_peers_and_downloads() {
    let payload = b"tracker-discovered payload".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let output_path = test_path("tracker-magnet-output.bin");

    let unreachable_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind unreachable peer placeholder");
    let unreachable = unreachable_listener
        .local_addr()
        .expect("unreachable peer address");
    drop(unreachable_listener);

    let peer_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind tracker-discovered peer");
    let reachable = peer_listener.local_addr().expect("peer address");
    let peer_task = tokio::spawn(serve_metadata_then_piece(
        peer_listener,
        info,
        payload.clone(),
        vec![0x80],
    ));

    let tracker_socket = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind scripted UDP tracker");
    let tracker_address = tracker_socket.local_addr().expect("tracker address");
    let tracker_task = tokio::spawn(serve_one_shot_udp_tracker(
        tracker_socket,
        info_hash,
        unreachable,
        reachable,
        Duration::ZERO,
    ));
    let rejecting_tracker = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind rejecting UDP tracker");
    let rejecting_tracker_address = rejecting_tracker
        .local_addr()
        .expect("rejecting tracker address");
    let rejecting_tracker_task = tokio::spawn(serve_rejecting_udp_tracker(rejecting_tracker));

    let magnet = format!(
        "magnet:?xt=urn:btih:{}&tr=udp%3A%2F%2F{rejecting_tracker_address}&\
             tr=udp%3A%2F%2F{tracker_address}%2Fannounce",
        hex(&info_hash)
    );
    let parsed = Magnet::parse(&magnet).expect("parse tracker magnet");
    assert!(parsed.peer_hints.is_empty());
    assert_eq!(parsed.udp_trackers.len(), 2);
    let network = loopback_network(Duration::from_secs(2));
    let control = DownloadControl::new();
    let mut peers = TorrentPeerCoordinator::from_magnet(&parsed, network, control.clone(), None)
        .await
        .expect("prepare tracker discovery");
    assert!(peers.registry.is_empty());

    let report = run_magnet_download_with_peers(
        MagnetDownloadConfig {
            magnet,
            output_path: output_path.clone(),
            network,
            resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            materialize_files: Vec::new(),
            dht: None,
        },
        control,
        parsed,
        &mut peers,
    )
    .await
    .expect("tracker-discovered magnet download");

    assert_eq!(peers.registry.len(), 2);
    let failed = peers
        .registry
        .find_endpoint(PeerEndpoint::new(unreachable).expect("failed endpoint"))
        .expect("failed tracker peer retained");
    assert_eq!(failed.history().total_failures, 1);
    assert_eq!(failed.history().last_failure, Some(PeerFailure::Connect));
    assert!(failed.sources().contains(PeerSource::Tracker));
    let succeeded = peers
        .registry
        .find_endpoint(PeerEndpoint::new(reachable).expect("successful endpoint"))
        .expect("successful tracker peer retained");
    assert_eq!(succeeded.history().total_failures, 0);
    assert!(succeeded.history().last_connected_at.is_some());
    assert!(succeeded.history().last_disconnected_at.is_some());
    assert!(succeeded.sources().contains(PeerSource::Tracker));

    assert_eq!(report.info_hash, info_hash);
    assert_eq!(
        tokio::fs::read(&output_path)
            .await
            .expect("published tracker output"),
        payload
    );
    peers
        .shutdown_tracker()
        .await
        .expect("stop tracker manager");
    if rejecting_tracker_task.is_finished() {
        rejecting_tracker_task
            .await
            .expect("rejecting tracker task");
    } else {
        rejecting_tracker_task.abort();
        let _ = rejecting_tracker_task.await;
    }
    tracker_task.await.expect("scripted tracker task");
    peer_task.await.expect("scripted peer task");
    let _ = tokio::fs::remove_file(output_path).await;
}

#[tokio::test]
async fn tracker_peer_discovered_during_content_becomes_useful() {
    let payload = b"late tracker peer payload".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let metadata_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind metadata-only peer");
    let metadata_address = metadata_listener.local_addr().expect("metadata address");
    let metadata_task = tokio::spawn(serve_metadata_then_piece(
        metadata_listener,
        info,
        payload.clone(),
        vec![0x00],
    ));
    let useful_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind late useful peer");
    let useful_address = useful_listener.local_addr().expect("useful address");
    let useful_task = tokio::spawn(serve_content_peer(
        useful_listener,
        info_hash,
        Arc::new(vec![payload.clone()]),
        vec![true],
    ));
    let unavailable_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind unavailable placeholder");
    let unavailable = unavailable_listener
        .local_addr()
        .expect("unavailable address");
    drop(unavailable_listener);
    let tracker = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind delayed tracker");
    let tracker_address = tracker.local_addr().expect("tracker address");
    let tracker_task = tokio::spawn(serve_one_shot_udp_tracker(
        tracker,
        info_hash,
        unavailable,
        useful_address,
        Duration::from_millis(150),
    ));
    let output = test_path("late-tracker-content.bin");
    let result = timeout(
        Duration::from_secs(3),
        download_magnet(MagnetDownloadConfig {
            magnet: format!(
                "magnet:?xt=urn:btih:{}&x.pe={metadata_address}&\
                     tr=udp%3A%2F%2F{tracker_address}%2Fannounce",
                hex(&info_hash)
            ),
            output_path: output.clone(),
            network: loopback_network(Duration::from_secs(2)),
            resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            materialize_files: Vec::new(),
            dht: None,
        }),
    )
    .await
    .expect("bounded late discovery")
    .expect("late discovered peer completed content");
    assert_eq!(result.verified_piece_count, 1);
    assert_eq!(
        tokio::fs::read(&output).await.expect("downloaded output"),
        payload
    );
    for task in [metadata_task, useful_task, tracker_task] {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("scripted owner joined")
            .expect("scripted task");
    }
    let _ = tokio::fs::remove_file(output).await;
}

#[tokio::test]
async fn dht_peer_discovered_during_content_becomes_useful() {
    let payload = b"late DHT peer payload".to_vec();
    let metainfo =
        Metainfo::from_info_bytes(&single_file_info(&payload)).expect("single-piece metainfo");
    let unavailable_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind unavailable content peer");
    let unavailable_address = unavailable_listener
        .local_addr()
        .expect("unavailable content address");
    let unavailable_task = tokio::spawn(serve_content_peer(
        unavailable_listener,
        metainfo.info_hash,
        Arc::new(vec![payload.clone()]),
        vec![false],
    ));
    let useful_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind DHT content peer");
    let useful_address = useful_listener.local_addr().expect("DHT content address");
    let useful_task = tokio::spawn(serve_content_peer(
        useful_listener,
        metainfo.info_hash,
        Arc::new(vec![payload.clone()]),
        vec![true],
    ));
    let dht_socket = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind scripted DHT");
    let dht_address = dht_socket.local_addr().expect("DHT address");
    let dht_task = tokio::spawn(serve_dht_peer(
        dht_socket,
        metainfo.info_hash,
        useful_address,
    ));
    let dht = DhtService::start(dht_config(dht_address))
        .await
        .expect("start DHT client");
    let mut peers = TorrentPeerCoordinator::from_endpoint(
        unavailable_address,
        PeerSource::Manual,
        loopback_network(Duration::from_secs(2)),
    )
    .expect("peer session");
    peers.dht = Some(dht.handle());
    let output = test_path("late-dht-content.bin");

    let report = timeout(
        Duration::from_secs(3),
        run_content_download(
            ContentDownloadConfig {
                output_path: output.clone(),
                max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
                swarm_config: SwarmConfig::for_request_limit(MIN_PAYLOAD_ALLOWANCE),
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
    .expect("bounded late DHT discovery")
    .expect("late DHT peer completed content");

    assert_eq!(report.verified_piece_count, 1);
    assert_eq!(tokio::fs::read(&output).await.expect("output"), payload);
    let discovered = peers
        .registry
        .find_endpoint(PeerEndpoint::new(useful_address).expect("DHT endpoint"))
        .expect("DHT peer retained");
    assert!(discovered.sources().contains(PeerSource::Dht));
    for task in [unavailable_task, useful_task, dht_task] {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("scripted DHT owner joined")
            .expect("scripted DHT task");
    }
    dht.shutdown().await.expect("DHT shutdown");
    let _ = tokio::fs::remove_file(output).await;
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

#[test]
fn peer_registry_activity_tracks_semantic_transitions_and_terminal_cleanup() {
    let control = DownloadControl::new();
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());
    let mut registry = PeerRegistry::new(PeerRegistryConfig {
        max_records: 1_000,
        max_consecutive_failures: 3,
        reconnect_backoff: Duration::from_secs(10),
    })
    .expect("registry");

    control.observe_peer_registry(&registry, Duration::ZERO, true, true);
    let endpoint = PeerEndpoint::new("127.0.0.1:6881".parse().expect("address")).expect("endpoint");
    registry
        .observe(
            PeerObservation::dialable(endpoint, PeerSource::Tracker),
            Duration::from_secs(1),
        )
        .expect("tracker observation");
    registry
        .observe(
            PeerObservation::dialable(endpoint, PeerSource::Dht),
            Duration::from_secs(2),
        )
        .expect("merged DHT observation");
    control.observe_peer_registry(&registry, Duration::from_secs(2), true, true);

    let attempt = registry
        .begin_dial(
            PeerSelector
                .select(
                    &registry,
                    PeerSelectionContext {
                        now: Duration::from_secs(3),
                    },
                )
                .expect("eligible peer"),
            PeerSelectionContext {
                now: Duration::from_secs(3),
            },
        )
        .expect("dial");
    registry
        .dial_failed(attempt, Duration::from_secs(4), PeerFailure::Connect)
        .expect("failure");
    control.observe_peer_registry(&registry, Duration::from_secs(4), true, true);
    control.observe_peer_registry(&registry, Duration::from_secs(13), true, false);
    control.observe_peer_registry(&registry, Duration::from_secs(14), true, false);
    control.observe_peer_registry(&registry, Duration::from_secs(15), false, true);

    let states = activity
        .events
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .filter_map(|event| match event {
            DownloadActivityEvent::PeerRegistryState { active, snapshot } => {
                Some((*active, snapshot.as_ref().clone()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(states.len(), 5);
    assert!(states[0].0);
    assert_eq!(states[0].1.counts.total, 0);
    assert_eq!(states[1].1.counts.eligible, 1);
    assert_eq!(states[1].1.records[0].sources.len(), 2);
    assert_eq!(states[2].1.counts.backed_off, 1);
    assert_eq!(states[3].1.counts.eligible, 1);
    assert!(!states[4].0);
    assert_eq!(states[4].1.counts.total, 0);
    assert!(states[4].1.records.is_empty());
}

#[test]
fn storage_state_emission_coalesces_hot_updates_and_force_flushes_latest() {
    let control = DownloadControl::new();
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());
    control.configure_disk_runtime(MIN_PAYLOAD_ALLOWANCE);

    let block = BlockKey {
        piece: 7,
        begin: 0,
        length: 16,
    };
    control.disk_block_requested(block, 16);
    control.disk_block_received(block, 16);
    control.disk_block_stored(block, 16);

    {
        let events = activity
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, DownloadActivityEvent::StorageState(_)))
                .count(),
            1,
            "hot mutations inside the observation interval stay coalesced"
        );
    }

    control.emit_storage_state_force();

    let events = activity
        .events
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut snapshots = events.iter().filter_map(|event| match event {
        DownloadActivityEvent::StorageState(snapshot) => Some(snapshot.as_ref()),
        _ => None,
    });
    assert_eq!(snapshots.clone().count(), 2);
    let latest = snapshots
        .next_back()
        .expect("forced latest storage snapshot");
    assert_eq!(latest.received_bytes_total, 16);
    assert_eq!(latest.stored_bytes_total, 16);
    assert_eq!(latest.pieces.len(), 1);
    assert_eq!(latest.pieces[0].requested_bytes, 16);
    assert_eq!(latest.pieces[0].stage, DiskPieceStage::Stored);
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

#[tokio::test]
async fn initial_tracker_operations_start_concurrently_and_merge_results() {
    let barrier = Arc::new(Barrier::new(3));
    let mut tracker_addresses = Vec::new();
    let mut servers = Vec::new();
    for offset in 0..3_u16 {
        let tracker = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind concurrent tracker");
        tracker_addresses.push(tracker.local_addr().expect("concurrent tracker address"));
        servers.push(tokio::spawn(serve_barrier_udp_tracker(
            tracker,
            barrier.clone(),
            41_000 + offset,
        )));
    }
    let trackers = tracker_addresses
        .iter()
        .map(|address| format!("&tr=udp%3A%2F%2F{address}"))
        .collect::<String>();
    let magnet = Magnet::parse(&format!(
        "magnet:?xt=urn:btih:{}{trackers}",
        "00".repeat(20)
    ))
    .expect("parse concurrent tracker magnet");
    let control = DownloadControl::new();
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());
    let mut peers = TorrentPeerCoordinator::from_magnet(
        &magnet,
        loopback_network(Duration::from_secs(1)),
        control,
        None,
    )
    .await
    .expect("start concurrent trackers");

    timeout(Duration::from_secs(2), async {
        for _ in 0..3 {
            peers
                .receive_tracker_peers()
                .await
                .expect("receive concurrent tracker peers");
        }
    })
    .await
    .expect("concurrent tracker result deadline");

    assert_eq!(peers.registry.len(), 3);
    let succeeded = {
        let events = activity
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    DownloadActivityEvent::TrackerAnnounceSucceeded { peer_count: 1, .. }
                )
            })
            .count()
    };
    assert_eq!(succeeded, 3);

    peers
        .shutdown_tracker()
        .await
        .expect("stop concurrent trackers");
    for server in servers {
        server.await.expect("concurrent tracker server");
    }
}

#[tokio::test]
async fn initial_tracker_operations_hold_the_ceiling_and_advance_on_failure() {
    let tracker_count = super::MAX_CONCURRENT_TRACKER_OPERATIONS + 1;
    let started = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(Semaphore::new(0));
    let mut tracker_addresses = Vec::new();
    let mut servers = Vec::new();
    for offset in 0..tracker_count {
        let tracker = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind bounded startup tracker");
        tracker_addresses.push(tracker.local_addr().expect("bounded tracker address"));
        servers.push(tokio::spawn(serve_bounded_startup_tracker(
            tracker,
            started.clone(),
            release.clone(),
            42_000 + u16::try_from(offset).expect("bounded peer port"),
        )));
    }
    let trackers = tracker_addresses
        .iter()
        .map(|address| format!("&tr=udp%3A%2F%2F{address}"))
        .collect::<String>();
    let magnet = Magnet::parse(&format!(
        "magnet:?xt=urn:btih:{}{trackers}",
        "00".repeat(20)
    ))
    .expect("parse bounded tracker magnet");
    let mut peers = TorrentPeerCoordinator::from_magnet(
        &magnet,
        loopback_network(Duration::from_secs(1)),
        DownloadControl::new(),
        None,
    )
    .await
    .expect("start bounded trackers");

    timeout(Duration::from_secs(1), async {
        while started.load(Ordering::Acquire) < super::MAX_CONCURRENT_TRACKER_OPERATIONS {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("fill tracker operation ceiling");
    sleep(Duration::from_millis(25)).await;
    assert_eq!(
        started.load(Ordering::Acquire),
        super::MAX_CONCURRENT_TRACKER_OPERATIONS
    );
    release.add_permits(tracker_count);

    timeout(Duration::from_secs(2), peers.receive_tracker_peers())
        .await
        .expect("bounded tracker result deadline")
        .expect("last startup tracker succeeds");
    assert_eq!(started.load(Ordering::Acquire), tracker_count);
    assert_eq!(peers.registry.len(), 1);

    peers
        .shutdown_tracker()
        .await
        .expect("stop bounded trackers");
    let mut successes = 0;
    for server in servers {
        successes += usize::from(server.await.expect("bounded tracker server"));
    }
    assert_eq!(successes, 1);
}

#[tokio::test]
async fn concurrent_tracker_cancellation_joins_and_releases_every_socket() {
    let mut trackers = Vec::new();
    let mut tracker_addresses = Vec::new();
    for _ in 0..3 {
        let tracker = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind silent concurrent tracker");
        tracker_addresses.push(
            tracker
                .local_addr()
                .expect("silent concurrent tracker address"),
        );
        trackers.push(tracker);
    }
    let tracker_parameters = tracker_addresses
        .iter()
        .map(|address| format!("&tr=udp%3A%2F%2F{address}"))
        .collect::<String>();
    let magnet = Magnet::parse(&format!(
        "magnet:?xt=urn:btih:{}{tracker_parameters}",
        "00".repeat(20)
    ))
    .expect("parse silent concurrent trackers");
    let control = DownloadControl::new();
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());
    let manager = TrackerManager::start(
        magnet.udp_trackers,
        magnet.info_hash,
        NetworkPolicy::LoopbackOnly,
        control,
    )
    .expect("start silent concurrent trackers");
    let mut client_addresses = Vec::new();
    for tracker in &trackers {
        let mut packet = [0; 32];
        let (length, client) = timeout(Duration::from_secs(1), tracker.recv_from(&mut packet))
            .await
            .expect("concurrent connect deadline")
            .expect("receive concurrent connect");
        assert_eq!(length, 16);
        client_addresses.push(client);
    }

    manager
        .shutdown()
        .await
        .expect("shutdown concurrent tracker manager");
    {
        let events = activity
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadActivityEvent::TrackerState(snapshot)
                if snapshot.active
                    && snapshot.records.iter().any(|record| matches!(
                        record.status,
                        crate::TrackerRuntimeStatus::Announcing
                    ))
        )));
        let terminal = events.iter().rev().find_map(|event| match event {
            DownloadActivityEvent::TrackerState(snapshot) => Some(snapshot),
            _ => None,
        });
        assert!(terminal.is_some_and(|snapshot| {
            !snapshot.active
                && snapshot
                    .records
                    .iter()
                    .all(|record| matches!(record.status, crate::TrackerRuntimeStatus::Inactive))
        }));
    }
    for client in client_addresses {
        UdpSocket::bind(client)
            .await
            .expect("concurrent tracker client socket released");
    }
}

#[tokio::test]
async fn zero_peer_success_waits_for_reannounce_without_tracker_failure() {
    let tracker = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind empty tracker");
    let tracker_address = tracker.local_addr().expect("empty tracker address");
    let server = tokio::spawn(serve_empty_udp_tracker(tracker));
    let magnet = Magnet::parse(&format!(
        "magnet:?xt=urn:btih:{}&tr=udp%3A%2F%2F{tracker_address}",
        "00".repeat(20)
    ))
    .expect("parse empty tracker magnet");
    let control = DownloadControl::new();
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());
    let mut peers = TorrentPeerCoordinator::from_magnet(
        &magnet,
        loopback_network(Duration::from_secs(1)),
        control,
        None,
    )
    .await
    .expect("start empty tracker");

    timeout(Duration::from_secs(1), peers.receive_tracker_peers())
        .await
        .expect("empty tracker result deadline")
        .expect("valid empty tracker result");
    assert!(peers.registry.is_empty());
    timeout(Duration::from_secs(1), async {
        loop {
            let has_reannounce = activity
                .events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .iter()
                .any(|event| {
                    matches!(
                        event,
                        DownloadActivityEvent::TrackerReannounceScheduled { .. }
                    )
                });
            if has_reannounce {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("reannounce diagnostic deadline");
    {
        let events = activity
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadActivityEvent::TrackerAnnounceSucceeded { peer_count: 0, .. }
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadActivityEvent::TrackerPeersUnavailable { peer_count: 0, .. }
        )));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, DownloadActivityEvent::TrackerAnnounceFailed { .. }))
        );
    }
    peers.shutdown_tracker().await.expect("stop empty tracker");
    server.await.expect("empty tracker server");
}

#[test]
fn udp_tracker_tokens_expire_after_the_protocol_lifetime() {
    let address = "127.0.0.1:6969".parse().expect("tracker address");
    let inserted_at = Instant::now();
    let mut tokens = UdpTrackerTokenCache::default();
    tokens.insert(address, 42, inserted_at);

    assert_eq!(
        tokens.get(address, inserted_at + Duration::from_secs(59)),
        Some(42)
    );
    assert_eq!(
        tokens.get(address, inserted_at + Duration::from_secs(60)),
        None
    );
}

#[tokio::test]
async fn udp_tracker_retransmits_reuses_token_and_cancels_cleanly() {
    let tracker = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind scripted tracker");
    let tracker_address = tracker.local_addr().expect("tracker address");
    let announced_port = 41_234;
    let server = tokio::spawn(async move {
        let mut packet = [0; 256];

        let (first_connect, first_client) =
            timeout(Duration::from_secs(1), tracker.recv_from(&mut packet))
                .await
                .expect("first connect deadline")
                .expect("first connect");
        assert_eq!(first_connect, 16);
        let connect_transaction =
            u32::from_be_bytes(packet[12..16].try_into().expect("connect transaction"));

        let (second_connect, second_client) =
            timeout(Duration::from_secs(1), tracker.recv_from(&mut packet))
                .await
                .expect("retransmitted connect deadline")
                .expect("retransmitted connect");
        assert_eq!(second_connect, 16);
        assert_eq!(second_client, first_client);
        assert_eq!(
            u32::from_be_bytes(packet[12..16].try_into().expect("connect transaction")),
            connect_transaction
        );
        let connection_id = 0x0102_0304_0506_0708_u64;
        let mut connect_response = Vec::from(0_u32.to_be_bytes());
        connect_response.extend_from_slice(&connect_transaction.to_be_bytes());
        connect_response.extend_from_slice(&connection_id.to_be_bytes());
        tracker
            .send_to(&connect_response, first_client)
            .await
            .expect("connect response");

        let (first_announce, announce_client) =
            timeout(Duration::from_secs(1), tracker.recv_from(&mut packet))
                .await
                .expect("first announce deadline")
                .expect("first announce");
        assert_eq!(first_announce, 98);
        assert_eq!(
            u32::from_be_bytes(packet[80..84].try_into().expect("started event")),
            AnnounceEvent::Started as u32
        );
        assert_eq!(
            u16::from_be_bytes(packet[96..98].try_into().expect("announced port")),
            announced_port
        );
        let announce_transaction =
            u32::from_be_bytes(packet[12..16].try_into().expect("announce transaction"));

        let (second_announce, second_announce_client) =
            timeout(Duration::from_secs(1), tracker.recv_from(&mut packet))
                .await
                .expect("retransmitted announce deadline")
                .expect("retransmitted announce");
        assert_eq!(second_announce, 98);
        assert_eq!(second_announce_client, announce_client);
        assert_eq!(
            u32::from_be_bytes(packet[12..16].try_into().expect("announce transaction")),
            announce_transaction
        );
        assert_eq!(
            u16::from_be_bytes(packet[96..98].try_into().expect("announced port")),
            announced_port
        );
        let mut announce_response = Vec::from(1_u32.to_be_bytes());
        announce_response.extend_from_slice(&announce_transaction.to_be_bytes());
        announce_response.extend_from_slice(&600_u32.to_be_bytes());
        announce_response.extend_from_slice(&0_u32.to_be_bytes());
        announce_response.extend_from_slice(&0_u32.to_be_bytes());
        tracker
            .send_to(&announce_response, announce_client)
            .await
            .expect("first announce response");

        let (cached_announce, cached_client) =
            timeout(Duration::from_secs(1), tracker.recv_from(&mut packet))
                .await
                .expect("cached announce deadline")
                .expect("cached announce");
        assert_eq!(cached_announce, 98, "cached token should skip connect");
        assert_eq!(
            u64::from_be_bytes(packet[0..8].try_into().expect("connection ID")),
            connection_id
        );
        assert_eq!(
            u32::from_be_bytes(packet[80..84].try_into().expect("ordinary event")),
            AnnounceEvent::None as u32
        );
        assert_eq!(
            u16::from_be_bytes(packet[96..98].try_into().expect("announced port")),
            announced_port
        );
        let cached_transaction =
            u32::from_be_bytes(packet[12..16].try_into().expect("announce transaction"));
        announce_response[4..8].copy_from_slice(&cached_transaction.to_be_bytes());
        tracker
            .send_to(&announce_response, cached_client)
            .await
            .expect("cached announce response");
    });

    let timing = UdpTrackerTiming {
        retransmit_after: Duration::from_millis(20),
        completion_timeout: Duration::from_millis(100),
    };
    let control = DownloadControl::new();
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());
    let mut tokens = UdpTrackerTokenCache::default();
    let first = announce_udp_tracker_address(
        tracker_address,
        &mut tokens,
        UdpTrackerAnnounce {
            info_hash: [7; 20],
            key: 1,
            event: AnnounceEvent::Started,
            port: announced_port,
        },
        UdpTrackerExchange {
            timing,
            control: &control,
            tracker_label: "udp://127.0.0.1",
        },
    )
    .await
    .expect("loss-recovered announce");
    assert!(first.peers.is_empty());
    let second = announce_udp_tracker_address(
        tracker_address,
        &mut tokens,
        UdpTrackerAnnounce {
            info_hash: [7; 20],
            key: 1,
            event: AnnounceEvent::None,
            port: announced_port,
        },
        UdpTrackerExchange {
            timing: UdpTrackerTiming {
                retransmit_after: Duration::from_millis(200),
                completion_timeout: Duration::from_secs(1),
            },
            control: &control,
            tracker_label: "udp://127.0.0.1",
        },
    )
    .await
    .expect("cached-token announce");
    assert!(second.peers.is_empty());
    server.await.expect("scripted tracker");

    let retransmissions = {
        let events = activity
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        events
            .iter()
            .filter(|event| matches!(event, DownloadActivityEvent::TrackerUdpRetransmitted { .. }))
            .count()
    };
    assert_eq!(retransmissions, 2);

    assert_tracker_wait_cancels_without_socket_leaks().await;
}

#[tokio::test]
async fn stalled_metadata_peer_does_not_delay_useful_peer() {
    let payload = b"parallel verified metadata".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let stalled_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stalled metadata peer");
    let stalled_address = stalled_listener
        .local_addr()
        .expect("stalled metadata address");
    let stalled_task = tokio::spawn(serve_stalled_metadata_peer(
        stalled_listener,
        info_hash,
        info.len(),
    ));
    let useful_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind useful metadata peer");
    let useful_address = useful_listener
        .local_addr()
        .expect("useful metadata address");
    let useful_task = tokio::spawn(serve_metadata_then_piece(
        useful_listener,
        info,
        payload.clone(),
        vec![0x80],
    ));
    let magnet = format!(
        "magnet:?xt=urn:btih:{}&x.pe={stalled_address}&x.pe={useful_address}",
        hex(&info_hash)
    );
    let parsed = Magnet::parse(&magnet).expect("parse parallel metadata magnet");
    let network = loopback_network(Duration::from_secs(5));
    let mut peers =
        TorrentPeerCoordinator::from_magnet(&parsed, network, DownloadControl::new(), None)
            .await
            .expect("resolve metadata peers");

    let (raw_info, metainfo) = timeout(Duration::from_secs(4), peers.acquire_metadata(info_hash))
        .await
        .expect("stalled metadata peer must not set the completion deadline")
        .expect("useful metadata peer supplies verified metadata");

    assert_eq!(raw_info, single_file_info(&payload));
    assert_eq!(metainfo.info_hash, info_hash);
    let stalled = peers
        .registry
        .find_endpoint(PeerEndpoint::new(stalled_address).expect("stalled endpoint"))
        .expect("stalled peer retained");
    assert_eq!(stalled.phase(), PeerPhase::Idle);
    assert_eq!(stalled.history().dial_attempts, 1);
    assert_eq!(stalled.history().total_failures, 0);
    peers.close_current(None).expect("close metadata winner");
    for task in [stalled_task, useful_task] {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("metadata peer joined")
            .expect("metadata peer task");
    }
}

#[tokio::test]
async fn metadata_cancellation_publishes_empty_peers_after_joined_cleanup() {
    let payload = b"cancelled metadata owner".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind cancelled metadata peer");
    let address = listener.local_addr().expect("metadata peer address");
    let peer_task = tokio::spawn(serve_stalled_metadata_peer(listener, info_hash, info.len()));
    let control = DownloadControl::new();
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());
    let task_control = control.clone();
    let task = tokio::spawn(download_magnet_metadata_with_control(
        format!("magnet:?xt=urn:btih:{}&x.pe={address}", hex(&info_hash)),
        loopback_network(Duration::from_secs(5)),
        task_control,
    ));

    timeout(Duration::from_secs(1), async {
        loop {
            let diagnostics = control.diagnostic_snapshot();
            if diagnostics
                .peer_connections
                .iter()
                .any(|peer| peer.lifecycle == PeerConnectionLifecycle::Connected)
                && diagnostics.metadata.total_requests_sent > 0
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("metadata peer reached connected state");

    control.cancel();
    let result = timeout(Duration::from_secs(1), task)
        .await
        .expect("metadata cancellation joined")
        .expect("metadata task");
    assert!(matches!(result, Err(DownloadError::Cancelled)));
    timeout(Duration::from_secs(1), peer_task)
        .await
        .expect("metadata peer closed before terminal result")
        .expect("metadata peer task");

    let diagnostics = control.diagnostic_snapshot();
    assert!(diagnostics.peer_connections.is_empty());
    assert_eq!(diagnostics.metadata.pending_dials, 0);
    assert_eq!(diagnostics.metadata.active_workers, 0);
    let events = activity
        .events
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let peer_snapshots = events
        .iter()
        .filter_map(|event| match event {
            DownloadActivityEvent::PeerConnections { peers, .. } => Some(peers.as_slice()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(peer_snapshots.iter().any(|peers| {
        peers
            .iter()
            .any(|peer| peer.lifecycle == PeerConnectionLifecycle::Connected)
    }));
    assert!(peer_snapshots.iter().any(|peers| {
        peers
            .iter()
            .any(|peer| peer.lifecycle == PeerConnectionLifecycle::Disconnecting)
    }));
    assert!(peer_snapshots.last().is_some_and(|peers| peers.is_empty()));
}

#[tokio::test]
async fn metadata_blocks_from_multiple_peers_complete_one_dictionary() {
    let payload = vec![0x5a; 1_700];
    let info = single_file_info_with_piece_length(&payload, 1);
    assert!(
        info.len() > 2 * rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH,
        "fixture must span three metadata blocks"
    );
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let partial_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind partial metadata peer");
    let partial_address = partial_listener.local_addr().expect("partial address");
    let complete_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind complementary metadata peer");
    let complete_address = complete_listener.local_addr().expect("complete address");
    let partial_task = tokio::spawn(serve_partial_metadata_peer(
        partial_listener,
        info.clone(),
        true,
    ));
    let complete_task = tokio::spawn(serve_partial_metadata_peer(
        complete_listener,
        info.clone(),
        false,
    ));
    let magnet = format!(
        "magnet:?xt=urn:btih:{}&x.pe={partial_address}&x.pe={complete_address}",
        hex(&info_hash)
    );
    let parsed = Magnet::parse(&magnet).expect("parse multi-source metadata magnet");
    let control = DownloadControl::new();
    let mut peers = TorrentPeerCoordinator::from_magnet(
        &parsed,
        loopback_network(Duration::from_secs(2)),
        control.clone(),
        None,
    )
    .await
    .expect("resolve multi-source metadata peers");

    let (raw_info, metainfo) = timeout(Duration::from_secs(3), peers.acquire_metadata(info_hash))
        .await
        .expect("multi-source metadata completion bound")
        .expect("combine metadata blocks across peers");
    assert_eq!(raw_info, info);
    assert_eq!(metainfo.info_hash, info_hash);
    let snapshot = control.diagnostic_snapshot().metadata;
    assert_eq!(snapshot.total_blocks_received, 3);
    assert!(
        snapshot
            .recent_attempts
            .iter()
            .filter(|peer| peer.blocks_received > 0)
            .count()
            >= 2
    );

    peers.close_current(None).expect("close metadata winner");
    for task in [partial_task, complete_task] {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("multi-source peer joined")
            .expect("multi-source peer task");
    }
}

#[tokio::test]
async fn corrupt_metadata_generation_resets_before_clean_peer_completes() {
    let payload = vec![0x39; 1_700];
    let info = single_file_info_with_piece_length(&payload, 1);
    assert!(
        info.len() > 2 * rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH,
        "fixture must span three metadata blocks"
    );
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let mut corrupt = info.clone();
    corrupt[rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH + 7] ^= 0x01;

    let corrupt_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind corrupt metadata peer");
    let corrupt_address = corrupt_listener.local_addr().expect("corrupt address");
    let clean_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind clean metadata peer");
    let clean_address = clean_listener.local_addr().expect("clean address");
    let corrupt_task = tokio::spawn(serve_metadata_bytes_after_delay(
        corrupt_listener,
        info_hash,
        corrupt,
        Duration::ZERO,
    ));
    let clean_task = tokio::spawn(serve_metadata_bytes_after_delay(
        clean_listener,
        info_hash,
        info.clone(),
        Duration::from_millis(200),
    ));
    let magnet = format!(
        "magnet:?xt=urn:btih:{}&x.pe={corrupt_address}&x.pe={clean_address}",
        hex(&info_hash)
    );
    let parsed = Magnet::parse(&magnet).expect("parse corrupt recovery magnet");
    let control = DownloadControl::new();
    let mut peers = TorrentPeerCoordinator::from_magnet(
        &parsed,
        loopback_network(Duration::from_secs(2)),
        control.clone(),
        None,
    )
    .await
    .expect("resolve corrupt recovery peers");

    let (raw_info, metainfo) = timeout(Duration::from_secs(3), peers.acquire_metadata(info_hash))
        .await
        .expect("corrupt metadata recovery bound")
        .expect("clean source completes after corrupt generation");
    assert_eq!(raw_info, info);
    assert_eq!(metainfo.info_hash, info_hash);
    let snapshot = control.diagnostic_snapshot().metadata;
    assert_eq!(snapshot.total_hash_failures, 1);
    assert_eq!(snapshot.last_hash_failure_contributors, 1);
    assert_eq!(snapshot.total_blocks_received, 6);

    peers
        .close_current(None)
        .expect("close clean metadata winner");
    for task in [corrupt_task, clean_task] {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("corrupt recovery peer joined")
            .expect("corrupt recovery peer task");
    }
}

#[tokio::test]
async fn metadata_requests_ramp_for_one_at_a_time_peer() {
    let payload = vec![0x71; 1_000];
    let info = single_file_info_with_piece_length(&payload, 1);
    assert!(
        info.len() > rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH
            && info.len() <= 2 * rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH,
        "fixture must span exactly two metadata blocks"
    );
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind one-at-a-time metadata peer");
    let address = listener.local_addr().expect("one-at-a-time address");
    let server = tokio::spawn(serve_one_at_a_time_metadata_peer(listener, info.clone()));
    let magnet = format!("magnet:?xt=urn:btih:{}&x.pe={address}", hex(&info_hash));
    let parsed = Magnet::parse(&magnet).expect("parse one-at-a-time magnet");
    let control = DownloadControl::new();
    let mut peers = TorrentPeerCoordinator::from_magnet(
        &parsed,
        loopback_network(Duration::from_secs(2)),
        control.clone(),
        None,
    )
    .await
    .expect("resolve one-at-a-time peer");

    let (raw_info, metainfo) = timeout(Duration::from_secs(2), peers.acquire_metadata(info_hash))
        .await
        .expect("one-at-a-time metadata completion bound")
        .expect("pace requests until first response");
    assert_eq!(raw_info, info);
    assert_eq!(metainfo.info_hash, info_hash);
    let snapshot = control.diagnostic_snapshot().metadata;
    assert_eq!(snapshot.total_requests_sent, 2);
    assert_eq!(snapshot.total_blocks_received, 2);

    peers.close_current(None).expect("close metadata winner");
    timeout(Duration::from_secs(1), server)
        .await
        .expect("one-at-a-time peer joined")
        .expect("one-at-a-time peer task");
}

#[tokio::test]
async fn peers_without_ut_metadata_release_slots_and_remain_diagnosable() {
    let payload = b"diagnosable metadata failover".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let mut missing_addresses = Vec::new();
    let mut missing_tasks = Vec::new();
    for _ in 0..MAX_METADATA_PEERS {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind metadata-incapable peer");
        missing_addresses.push(listener.local_addr().expect("missing metadata address"));
        missing_tasks.push(tokio::spawn(serve_metadata_peer_without_ut_metadata(
            listener, info_hash,
        )));
    }
    let useful_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind useful metadata peer");
    let useful_address = useful_listener
        .local_addr()
        .expect("useful metadata address");
    let useful_task = tokio::spawn(serve_metadata_then_piece(
        useful_listener,
        info.clone(),
        payload,
        vec![0x80],
    ));
    let mut magnet = format!("magnet:?xt=urn:btih:{}", hex(&info_hash));
    for address in &missing_addresses {
        magnet.push_str(&format!("&x.pe={address}"));
    }
    magnet.push_str(&format!("&x.pe={useful_address}"));
    let parsed = Magnet::parse(&magnet).expect("parse diagnostic metadata magnet");
    let control = DownloadControl::new();
    let mut peers = TorrentPeerCoordinator::from_magnet(
        &parsed,
        loopback_network(Duration::from_secs(1)),
        control.clone(),
        None,
    )
    .await
    .expect("resolve diagnostic metadata peers");

    let (raw_info, _) = timeout(Duration::from_secs(1), peers.acquire_metadata(info_hash))
        .await
        .expect("metadata-incapable peers must release all slots")
        .expect("later useful peer supplies metadata");
    assert_eq!(raw_info, info);

    let snapshot = control.diagnostic_snapshot().metadata;
    assert_eq!(snapshot.phase, MetadataAcquisitionPhase::Complete);
    assert_eq!(snapshot.total_attempts, MAX_METADATA_PEERS + 1);
    assert_eq!(snapshot.total_requests_sent, 1);
    assert_eq!(snapshot.total_blocks_received, 1);
    assert_eq!(snapshot.active_attempts, Vec::new());
    assert_eq!(
        snapshot
            .recent_attempts
            .iter()
            .filter(|peer| peer.stage == MetadataPeerStage::Failed)
            .count(),
        MAX_METADATA_PEERS
    );
    assert!(snapshot.recent_attempts.iter().any(|peer| {
        peer.stage == MetadataPeerStage::Complete
            && peer.remote_metadata_id == Some(UT_METADATA_LOCAL_ID)
            && peer.blocks_received == 1
    }));
    assert!(
        snapshot
            .recent_attempts
            .iter()
            .filter(|peer| {
                peer.terminal_detail
                    .as_deref()
                    .is_some_and(|detail| detail.contains("does not advertise"))
            })
            .count()
            >= MAX_METADATA_PEERS
    );
    let registry = snapshot.registry.expect("peer registry snapshot");
    assert_eq!(registry.counts.total, MAX_METADATA_PEERS + 1);

    peers.close_current(None).expect("close metadata winner");
    for task in missing_tasks.into_iter().chain([useful_task]) {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("metadata fixture joined")
            .expect("metadata fixture task");
    }
}

#[tokio::test]
async fn unrelated_messages_cannot_hold_every_metadata_slot() {
    let payload = b"metadata after bounded chatter".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let mut chatter_addresses = Vec::new();
    let mut chatter_tasks = Vec::new();
    for _ in 0..MAX_METADATA_PEERS {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind chattering peer");
        chatter_addresses.push(listener.local_addr().expect("chattering peer address"));
        chatter_tasks.push(tokio::spawn(
            serve_chattering_peer_without_extension_handshake(listener, info_hash),
        ));
    }
    let useful_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind useful metadata peer");
    let useful_address = useful_listener
        .local_addr()
        .expect("useful metadata address");
    let useful_task = tokio::spawn(serve_metadata_then_piece(
        useful_listener,
        info.clone(),
        payload,
        vec![0x80],
    ));
    let mut magnet = format!("magnet:?xt=urn:btih:{}", hex(&info_hash));
    for address in &chatter_addresses {
        magnet.push_str(&format!("&x.pe={address}"));
    }
    magnet.push_str(&format!("&x.pe={useful_address}"));
    let parsed = Magnet::parse(&magnet).expect("parse chattering metadata magnet");
    let control = DownloadControl::new();
    let mut peers = TorrentPeerCoordinator::from_magnet(
        &parsed,
        loopback_network(Duration::from_millis(150)),
        control.clone(),
        None,
    )
    .await
    .expect("resolve chattering metadata peers");

    let (raw_info, _) = timeout(Duration::from_secs(2), peers.acquire_metadata(info_hash))
        .await
        .expect("metadata progress deadline releases chattering peers")
        .expect("later useful peer supplies metadata");
    assert_eq!(raw_info, info);
    let snapshot = control.diagnostic_snapshot().metadata;
    assert_eq!(snapshot.phase, MetadataAcquisitionPhase::Complete);
    assert_eq!(snapshot.total_attempts, MAX_METADATA_PEERS + 1);
    assert!(
        snapshot
            .recent_attempts
            .iter()
            .filter_map(|peer| peer.terminal_detail.as_deref())
            .filter(|detail| detail.contains("metadata progress timed out"))
            .count()
            >= MAX_METADATA_PEERS
    );
    assert!(
        snapshot
            .recent_attempts
            .iter()
            .any(|peer| { peer.stage == MetadataPeerStage::Complete && peer.blocks_received == 1 })
    );

    peers.close_current(None).expect("close metadata winner");
    for task in chatter_tasks.into_iter().chain([useful_task]) {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("metadata fixture joined")
            .expect("metadata fixture task");
    }
}

#[tokio::test]
async fn metadata_rejections_release_slots_and_are_counted() {
    let payload = b"metadata after explicit rejects".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let mut rejecting_addresses = Vec::new();
    let mut rejecting_tasks = Vec::new();
    for _ in 0..MAX_METADATA_PEERS {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind rejecting peer");
        rejecting_addresses.push(listener.local_addr().expect("rejecting peer address"));
        rejecting_tasks.push(tokio::spawn(serve_metadata_rejecting_peer(
            listener,
            info_hash,
            info.len(),
        )));
    }
    let useful_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind useful metadata peer");
    let useful_address = useful_listener
        .local_addr()
        .expect("useful metadata address");
    let useful_task = tokio::spawn(serve_metadata_then_piece(
        useful_listener,
        info.clone(),
        payload,
        vec![0x80],
    ));
    let mut magnet = format!("magnet:?xt=urn:btih:{}", hex(&info_hash));
    for address in &rejecting_addresses {
        magnet.push_str(&format!("&x.pe={address}"));
    }
    magnet.push_str(&format!("&x.pe={useful_address}"));
    let parsed = Magnet::parse(&magnet).expect("parse rejecting metadata magnet");
    let control = DownloadControl::new();
    let mut peers = TorrentPeerCoordinator::from_magnet(
        &parsed,
        loopback_network(Duration::from_secs(1)),
        control.clone(),
        None,
    )
    .await
    .expect("resolve rejecting metadata peers");

    let (raw_info, _) = timeout(Duration::from_secs(1), peers.acquire_metadata(info_hash))
        .await
        .expect("rejecting peers must release all slots")
        .expect("later useful peer supplies metadata");
    assert_eq!(raw_info, info);
    let snapshot = control.diagnostic_snapshot().metadata;
    assert_eq!(snapshot.phase, MetadataAcquisitionPhase::Complete);
    assert_eq!(snapshot.total_attempts, MAX_METADATA_PEERS + 1);
    let rejected_requests = snapshot
        .recent_attempts
        .iter()
        .map(|peer| peer.rejects_received)
        .sum::<usize>();
    assert!((1..=MAX_METADATA_PEERS).contains(&rejected_requests));
    assert!(
        snapshot
            .recent_attempts
            .iter()
            .any(|peer| { peer.stage == MetadataPeerStage::Complete && peer.blocks_received == 1 })
    );

    peers.close_current(None).expect("close metadata winner");
    for task in rejecting_tasks.into_iter().chain([useful_task]) {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("metadata fixture joined")
            .expect("metadata fixture task");
    }
}

#[tokio::test]
async fn tracker_discovery_continues_while_metadata_peer_stalls() {
    let payload = b"late tracker metadata".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let stalled_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stalled metadata peer");
    let stalled_address = stalled_listener
        .local_addr()
        .expect("stalled metadata address");
    let stalled_task = tokio::spawn(serve_stalled_metadata_peer(
        stalled_listener,
        info_hash,
        info.len(),
    ));
    let useful_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind tracker metadata peer");
    let useful_address = useful_listener
        .local_addr()
        .expect("tracker metadata address");
    let useful_task = tokio::spawn(serve_metadata_then_piece(
        useful_listener,
        info,
        payload,
        vec![0x80],
    ));
    let unavailable_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind unavailable placeholder");
    let unavailable = unavailable_listener
        .local_addr()
        .expect("unavailable address");
    drop(unavailable_listener);
    let tracker = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind delayed tracker");
    let tracker_address = tracker.local_addr().expect("tracker address");
    let tracker_task = tokio::spawn(serve_one_shot_udp_tracker(
        tracker,
        info_hash,
        unavailable,
        useful_address,
        Duration::from_millis(100),
    ));
    let magnet = Magnet::parse(&format!(
        "magnet:?xt=urn:btih:{}&x.pe={stalled_address}&\
             tr=udp%3A%2F%2F{tracker_address}%2Fannounce",
        hex(&info_hash)
    ))
    .expect("parse late metadata discovery magnet");
    let mut peers = TorrentPeerCoordinator::from_magnet(
        &magnet,
        loopback_network(Duration::from_secs(2)),
        DownloadControl::new(),
        None,
    )
    .await
    .expect("start metadata discovery");

    let (_, metainfo) = timeout(Duration::from_secs(4), peers.acquire_metadata(info_hash))
        .await
        .expect("late tracker peer must be consumed during metadata work")
        .expect("tracker peer supplies metadata");

    assert_eq!(metainfo.info_hash, info_hash);
    let discovered = peers
        .registry
        .find_endpoint(PeerEndpoint::new(useful_address).expect("tracker endpoint"))
        .expect("tracker peer retained");
    assert!(discovered.sources().contains(PeerSource::Tracker));
    peers.close_current(None).expect("close metadata winner");
    peers
        .shutdown_tracker()
        .await
        .expect("shutdown metadata tracker");
    for task in [stalled_task, useful_task, tracker_task] {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("metadata fixture joined")
            .expect("metadata fixture task");
    }
}

#[tokio::test]
async fn magnet_registry_fails_over_and_hands_same_peer_to_content_download() {
    let payload = b"verified magnet payload".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let output_path = test_path("magnet-output.bin");
    let unreachable_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind unreachable peer placeholder");
    let unreachable = unreachable_listener
        .local_addr()
        .expect("unreachable peer address");
    drop(unreachable_listener);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind scripted metadata peer");
    let address = listener.local_addr().expect("listener address");
    let peer_task = tokio::spawn(serve_metadata_then_piece(
        listener,
        info,
        payload.clone(),
        vec![0x80],
    ));

    let magnet = format!(
        "magnet:?xt=urn:btih:{}&x.pe={unreachable}&x.pe={address}",
        hex(&info_hash)
    );
    let parsed = Magnet::parse(&magnet).expect("parse failover magnet");
    let network = loopback_network(Duration::from_secs(2));
    let mut peers =
        TorrentPeerCoordinator::from_magnet(&parsed, network, DownloadControl::new(), None)
            .await
            .expect("resolve failover peers");
    assert_eq!(peers.registry.len(), 2);

    let report = run_magnet_download_with_peers(
        MagnetDownloadConfig {
            magnet,
            output_path: output_path.clone(),
            network,
            resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            materialize_files: Vec::new(),
            dht: None,
        },
        DownloadControl::new(),
        parsed,
        &mut peers,
    )
    .await
    .expect("magnet metadata and content after failover");

    let failed = peers
        .registry
        .find_endpoint(PeerEndpoint::new(unreachable).expect("failed endpoint"))
        .expect("failed peer record retained");
    assert_eq!(failed.phase(), PeerPhase::Idle);
    assert_eq!(failed.history().dial_attempts, 1);
    assert_eq!(failed.history().total_failures, 1);
    assert_eq!(failed.history().last_failure, Some(PeerFailure::Connect));
    assert!(failed.history().retry_at.is_some());
    assert!(failed.sources().contains(PeerSource::MagnetHint));

    let connected = peers
        .registry
        .find_endpoint(PeerEndpoint::new(address).expect("connected endpoint"))
        .expect("connected peer record retained");
    assert_eq!(connected.phase(), PeerPhase::Idle);
    assert_eq!(connected.history().dial_attempts, 1);
    assert_eq!(connected.history().total_failures, 0);
    assert!(connected.history().last_connected_at.is_some());
    assert!(connected.history().last_disconnected_at.is_some());
    assert!(connected.sources().contains(PeerSource::MagnetHint));

    assert_eq!(report.info_hash, info_hash);
    assert_eq!(
        tokio::fs::read(&output_path)
            .await
            .expect("published output"),
        payload
    );
    peer_task.await.expect("scripted peer task");
    let _ = tokio::fs::remove_file(output_path).await;
}

#[tokio::test]
async fn public_magnet_entry_starts_tracker_and_uses_peer_registry_path() {
    let payload = b"public entry payload".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let output_path = test_path("public-magnet-output.bin");
    let unsupported_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind non-extension peer");
    let unsupported_address = unsupported_listener
        .local_addr()
        .expect("non-extension peer address");
    let unsupported_task = tokio::spawn(async move {
        let (mut stream, _) = unsupported_listener
            .accept()
            .await
            .expect("accept magnet client");
        let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake_bytes)
            .await
            .expect("read magnet handshake");
        assert!(
            decode_handshake(&handshake_bytes, info_hash)
                .expect("valid client handshake")
                .supports_extensions()
        );
        stream
            .write_all(&encode_handshake_with_reserved(
                info_hash,
                *b"-RS-NOEXT-0000000000",
                [0; 8],
            ))
            .await
            .expect("send non-extension handshake");
    });
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind scripted metadata peer");
    let address = listener.local_addr().expect("listener address");
    let peer_task = tokio::spawn(serve_metadata_then_piece(
        listener,
        info,
        payload.clone(),
        vec![0x80],
    ));
    let unused_tracker = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind unused tracker");
    let unused_tracker_address = unused_tracker.local_addr().expect("unused tracker address");

    let report = download_magnet(MagnetDownloadConfig {
        magnet: format!(
            "magnet:?xt=urn:btih:{}&x.pe={unsupported_address}&x.pe={address}&\
                 tr=udp%3A%2F%2F{unused_tracker_address}",
            hex(&info_hash)
        ),
        output_path: output_path.clone(),
        network: loopback_network(Duration::from_secs(2)),
        resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
        skip_files: Vec::new(),
        materialize_files: Vec::new(),
        dht: None,
    })
    .await
    .expect("public magnet entry");

    assert_eq!(report.info_hash, info_hash);
    assert_eq!(
        tokio::fs::read(&output_path)
            .await
            .expect("published output"),
        payload
    );
    unsupported_task.await.expect("non-extension peer task");
    peer_task.await.expect("scripted peer task");
    let mut tracker_packet = [0; 16];
    let (tracker_length, _) = timeout(
        Duration::from_secs(1),
        unused_tracker.recv_from(&mut tracker_packet),
    )
    .await
    .expect("tracker lifecycle should start alongside explicit hints")
    .expect("receive initial tracker connect");
    assert_eq!(tracker_length, 16);
    let _ = tokio::fs::remove_file(output_path).await;
}

#[tokio::test]
async fn transient_dht_miss_retries_without_becoming_terminal() {
    let info_hash = [8; 20];
    let peer = SocketAddr::from(([127, 0, 0, 1], 49_999));
    let dht_socket = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind scripted DHT");
    let dht_address = dht_socket.local_addr().expect("DHT address");
    let dht_task = tokio::spawn(serve_dht_peer_after_retry(dht_socket, info_hash, peer));
    let dht = DhtService::start(dht_config(dht_address))
        .await
        .expect("start DHT client");
    let control = DownloadControl::new();
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());

    let peers = retrying_dht_lookup(
        dht.handle(),
        info_hash,
        control,
        DhtRetryTiming {
            initial_delay: Duration::from_millis(10),
            maximum_delay: Duration::from_millis(20),
        },
        Duration::ZERO,
    )
    .await
    .expect("retry DHT lookup");

    assert_eq!(peers, vec![peer]);
    {
        let events = activity
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(
            events
                .iter()
                .any(|event| matches!(event, DownloadActivityEvent::DhtRetryScheduled { .. }))
        );
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadActivityEvent::DhtLookupSucceeded { peer_count: 1 }
        )));
    }
    dht_task.await.expect("scripted DHT task");
    dht.shutdown().await.expect("DHT shutdown");
}

#[tokio::test]
async fn trackerless_dht_peer_completes_metadata_and_content_path() {
    let payload = b"peer discovered through DHT".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let output_path = test_path("dht-magnet-output.bin");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind DHT-discovered peer");
    let peer_address = listener.local_addr().expect("peer address");
    let peer_task = tokio::spawn(serve_metadata_then_piece(
        listener,
        info,
        payload.clone(),
        vec![0x80],
    ));
    let dht_socket = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind scripted DHT");
    let dht_address = dht_socket.local_addr().expect("DHT address");
    let dht_task = tokio::spawn(serve_dht_peer(dht_socket, info_hash, peer_address));
    let dht = DhtService::start(dht_config(dht_address))
        .await
        .expect("start DHT client");

    let report = download_magnet(MagnetDownloadConfig {
        magnet: format!("magnet:?xt=urn:btih:{}", hex(&info_hash)),
        output_path: output_path.clone(),
        network: loopback_network(Duration::from_secs(2)),
        resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
        skip_files: Vec::new(),
        materialize_files: Vec::new(),
        dht: Some(dht.handle()),
    })
    .await
    .expect("DHT-discovered download");

    assert_eq!(report.info_hash, info_hash);
    assert_eq!(
        tokio::fs::read(&output_path)
            .await
            .expect("published output"),
        payload
    );
    dht_task.await.expect("scripted DHT task");
    peer_task.await.expect("scripted peer task");
    dht.shutdown().await.expect("DHT shutdown");
    let _ = tokio::fs::remove_file(output_path).await;
}

#[tokio::test]
async fn verified_private_metadata_purges_dht_only_peer_before_content() {
    let payload = b"must not be fetched from decentralized peer".to_vec();
    let info = private_single_file_info(&payload);
    let metainfo = Metainfo::from_info_bytes(&info).expect("private metadata");
    assert!(metainfo.private);
    let info_hash = metainfo.info_hash;
    let output_path = test_path("private-dht-output.bin");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind DHT-only peer");
    let peer_address = listener.local_addr().expect("peer address");
    let peer_task = tokio::spawn(serve_metadata_then_piece(
        listener,
        info,
        payload,
        vec![0x80],
    ));
    let dht_socket = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind scripted DHT");
    let dht_address = dht_socket.local_addr().expect("DHT address");
    let dht_task = tokio::spawn(serve_dht_peer(dht_socket, info_hash, peer_address));
    let dht = DhtService::start(dht_config(dht_address))
        .await
        .expect("start DHT client");

    let result = download_magnet(MagnetDownloadConfig {
        magnet: format!("magnet:?xt=urn:btih:{}", hex(&info_hash)),
        output_path: output_path.clone(),
        network: loopback_network(Duration::from_secs(2)),
        resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
        skip_files: Vec::new(),
        materialize_files: Vec::new(),
        dht: Some(dht.handle()),
    })
    .await;

    assert!(matches!(result, Err(DownloadError::NoUsablePeer)));
    assert!(!tokio::fs::try_exists(&output_path).await.expect("output"));
    dht_task.await.expect("scripted DHT task");
    peer_task.await.expect("scripted peer task");
    dht.shutdown().await.expect("DHT shutdown");
}

#[tokio::test]
async fn invalid_premetadata_bitfield_fails_before_storage_creation() {
    let payload = b"not written".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let output_path = test_path("bad-premetadata-output.bin");
    let staging = staging_path(&output_path).expect("staging path");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind scripted metadata peer");
    let address = listener.local_addr().expect("listener address");
    let peer_task = tokio::spawn(serve_metadata_then_piece(
        listener,
        info,
        payload,
        vec![0x80, 0],
    ));

    let result = download_magnet(MagnetDownloadConfig {
        magnet: format!("magnet:?xt=urn:btih:{}&x.pe={address}", hex(&info_hash)),
        output_path: output_path.clone(),
        network: loopback_network(Duration::from_secs(2)),
        resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
        skip_files: Vec::new(),
        materialize_files: Vec::new(),
        dht: None,
    })
    .await;

    assert!(matches!(
        result,
        Err(DownloadError::InvalidPremetadataState(_))
    ));
    assert!(!tokio::fs::try_exists(&output_path).await.expect("output"));
    assert!(!tokio::fs::try_exists(&staging).await.expect("staging"));
    peer_task.abort();
    let _ = peer_task.await;
}

#[tokio::test]
async fn magnet_peer_without_extension_support_fails_before_storage() {
    let info = single_file_info(b"not written");
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let output_path = test_path("no-extension-output.bin");
    let staging = staging_path(&output_path).expect("staging path");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind non-extension peer");
    let address = listener.local_addr().expect("listener address");
    let peer_task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept magnet client");
        let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake_bytes)
            .await
            .expect("read magnet handshake");
        assert!(
            decode_handshake(&handshake_bytes, info_hash)
                .expect("valid client handshake")
                .supports_extensions()
        );
        stream
            .write_all(&encode_handshake_with_reserved(
                info_hash,
                *b"-RS-NOEXT-0000000000",
                [0; 8],
            ))
            .await
            .expect("send non-extension handshake");
    });

    let result = download_magnet(MagnetDownloadConfig {
        magnet: format!("magnet:?xt=urn:btih:{}&x.pe={address}", hex(&info_hash)),
        output_path: output_path.clone(),
        network: loopback_network(Duration::from_secs(2)),
        resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
        skip_files: Vec::new(),
        materialize_files: Vec::new(),
        dht: None,
    })
    .await;

    assert!(matches!(
        result,
        Err(DownloadError::ExtensionProtocolUnsupported)
    ));
    assert!(!tokio::fs::try_exists(&output_path).await.expect("output"));
    assert!(!tokio::fs::try_exists(&staging).await.expect("staging"));
    peer_task.await.expect("non-extension peer task");
}

#[tokio::test]
async fn magnet_peer_disconnect_during_metadata_fails_before_storage() {
    let info = single_file_info(b"not written");
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let output_path = test_path("metadata-disconnect-output.bin");
    let staging = staging_path(&output_path).expect("staging path");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind disconnecting peer");
    let address = listener.local_addr().expect("listener address");
    let peer_task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept magnet client");
        let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake_bytes)
            .await
            .expect("read magnet handshake");
        decode_handshake(&handshake_bytes, info_hash).expect("valid client handshake");
        let mut reserved = [0; 8];
        reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
        stream
            .write_all(&encode_handshake_with_reserved(
                info_hash,
                *b"-RS-DROP--0000000000",
                reserved,
            ))
            .await
            .expect("send extension handshake");
    });

    let result = download_magnet(MagnetDownloadConfig {
        magnet: format!("magnet:?xt=urn:btih:{}&x.pe={address}", hex(&info_hash)),
        output_path: output_path.clone(),
        network: loopback_network(Duration::from_secs(2)),
        resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
        skip_files: Vec::new(),
        materialize_files: Vec::new(),
        dht: None,
    })
    .await;

    assert!(
        matches!(
            &result,
            Err(DownloadError::PeerClosed)
                | Err(DownloadError::Io {
                    operation: "read peer message",
                    ..
                })
        ),
        "unexpected disconnect result: {result:?}"
    );
    assert!(!tokio::fs::try_exists(&output_path).await.expect("output"));
    assert!(!tokio::fs::try_exists(&staging).await.expect("staging"));
    peer_task.await.expect("disconnecting peer task");
}

#[tokio::test]
async fn timeout_removes_unverified_staging_output() {
    let metainfo_path = test_path("fixture.torrent");
    let output_path = test_path("output.bin");
    let staging = staging_path(&output_path).expect("staging path");
    let mut metainfo = b"d4:infod6:lengthi1e4:name1:x12:piece lengthi16384e6:pieces20:".to_vec();
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
        network: loopback_network(Duration::from_millis(50)),
        resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
        skip_files: Vec::new(),
        materialize_files: Vec::new(),
    })
    .await;

    assert!(matches!(result, Err(DownloadError::PeerTimedOut { .. })));
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
        network: loopback_network(Duration::from_millis(50)),
        resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
        skip_files: vec![1],
        materialize_files: Vec::new(),
    })
    .await;

    assert!(matches!(result, Err(DownloadError::PeerTimedOut { .. })));
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
        let (mut stream, _) = listener.accept().await.expect("accept diagnostic");
        let mut handshake = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake)
            .await
            .expect("read diagnostic handshake");
        let mut end = [0; 1];
        assert_eq!(
            stream.read(&mut end).await.expect("wait for peer cleanup"),
            0
        );
    });

    let control = DownloadControl::new();
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());
    let download_control = control.clone();
    let download_task = tokio::spawn(download_verified_piece_with_control(
        DownloadConfig {
            metainfo_path: metainfo_path.clone(),
            peer: address,
            output_path: output_path.clone(),
            network: loopback_network(Duration::from_secs(5)),
            resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
            skip_files: vec![1],
            materialize_files: Vec::new(),
        },
        download_control,
    ));

    assert!(
        !tokio::fs::try_exists(&staging)
            .await
            .expect("staging remains lazy")
    );
    assert!(
        !tokio::fs::try_exists(&part)
            .await
            .expect("part remains lazy")
    );
    timeout(Duration::from_secs(1), async {
        loop {
            if control
                .diagnostic_snapshot()
                .peer_connections
                .iter()
                .any(|peer| peer.lifecycle == PeerConnectionLifecycle::ProtocolHandshaking)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("engine published active diagnostic peer");

    control.cancel();
    control.cancel();
    let result = download_task.await.expect("download task");
    assert!(matches!(result, Err(DownloadError::Cancelled)));
    assert!(control.is_cancelled());
    let progress = control.snapshot();
    assert_eq!(progress.buffered_payload_bytes, 0);
    assert_eq!(progress.requested_bytes, 0);
    assert_eq!(progress.received_bytes, 0);
    assert_eq!(progress.stored_bytes, 0);
    assert_eq!(progress.storage_jobs_pending, 0);
    assert_eq!(progress.outstanding_request_bytes, 0);
    assert!(control.diagnostic_snapshot().peer_connections.is_empty());
    assert!(!tokio::fs::try_exists(&output_path).await.expect("output"));
    assert!(!tokio::fs::try_exists(&staging).await.expect("staging"));
    assert!(!tokio::fs::try_exists(&part).await.expect("part"));

    timeout(Duration::from_secs(1), peer_task)
        .await
        .expect("diagnostic peer joined before terminal result")
        .expect("diagnostic peer task");
    {
        let events = activity
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let peer_snapshots = events
            .iter()
            .filter_map(|event| match event {
                DownloadActivityEvent::PeerConnections { peers, .. } => Some(peers.as_slice()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(peer_snapshots.iter().any(|peers| {
            peers
                .iter()
                .any(|peer| peer.lifecycle == PeerConnectionLifecycle::Disconnecting)
        }));
        assert!(peer_snapshots.last().is_some_and(|peers| peers.is_empty()));
    }
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
        network: loopback_network(Duration::from_secs(1)),
        resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
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

    let _ = tokio::fs::remove_dir_all(selective_staging_path(&output_path).expect("staging")).await;
    let _ = tokio::fs::remove_file(part).await;
    let _ = tokio::fs::remove_file(metainfo_path).await;
}
