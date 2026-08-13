use super::*;
use rstorrent_protocol::content::ExpectedPieceIntegrity;

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
            expected: ExpectedPieceIntegrity::V1Sha1(expected),
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
                artifact_identity: test_artifact_identity(),
                output_path: task_output,
                max_buffered_payload_bytes: 2 * MIN_PAYLOAD_ALLOWANCE,
                storage_intake_high_watermark_bytes: 2 * MIN_PAYLOAD_ALLOWANCE,
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
                artifact_identity: test_artifact_identity(),
                output_path: task_output,
                max_buffered_payload_bytes: payload_len,
                storage_intake_high_watermark_bytes: payload_len,
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
                artifact_identity: test_artifact_identity(),
                output_path: task_output,
                max_buffered_payload_bytes: MIN_PAYLOAD_ALLOWANCE,
                storage_intake_high_watermark_bytes: MIN_PAYLOAD_ALLOWANCE,
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
    let mut pipeline =
        ContentStoragePipeline::start(storage, &control, 5 * block_length, 5 * block_length, None)
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
                expected: ExpectedPieceIntegrity::V1Sha1(expected),
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
            expected: ExpectedPieceIntegrity::V1Sha1(expected),
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
    let mut pipeline =
        ContentStoragePipeline::start(storage, &control, 2 * block_length, 2 * block_length, None)
            .await
            .expect("start cross-class pipeline");
    let expected: [u8; 20] = Sha1::digest(vec![0x51; block_length]).into();
    pipeline
        .enqueue(ContentStorageCommand::Verify {
            piece: 0,
            generation: PieceGeneration::new(1).expect("generation"),
            length: block_length as u32,
            expected: ExpectedPieceIntegrity::V1Sha1(expected),
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
    let mut pipeline =
        ContentStoragePipeline::start(storage, &control, block_length, block_length, None)
            .await
            .expect("start one-slot completion pipeline");
    for piece in 0..2 {
        let expected: [u8; 20] = Sha1::digest(vec![piece as u8; block_length]).into();
        pipeline
            .enqueue(ContentStorageCommand::Verify {
                piece: piece as u32,
                generation: PieceGeneration::new(1).expect("generation"),
                length: block_length as u32,
                expected: ExpectedPieceIntegrity::V1Sha1(expected),
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
                artifact_identity: test_artifact_identity(),
                output_path: output.clone(),
                max_buffered_payload_bytes: payload.len(),
                storage_intake_high_watermark_bytes: payload.len(),
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
                artifact_identity: test_artifact_identity(),
                output_path: task_output,
                max_buffered_payload_bytes: payload_limit,
                storage_intake_high_watermark_bytes: payload_limit,
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
        .peers
        .with_state(|state| {
            state
                .registry
                .find_endpoint(PeerEndpoint::new(discovered_address).expect("DHT endpoint"))
                .cloned()
        })
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
