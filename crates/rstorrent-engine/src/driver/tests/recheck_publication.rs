use super::*;
use crate::driver::AppliedFileSelection;
use crate::{FileSelectionUpdate, PeerBudget, ResumeValidationIntent};

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
            peer_budget: PeerBudget::system_default(),
            mse_dh: crate::MseDhWorkOwner::new(),
            encryption: crate::PeerEncryptionPolicyHandle::default(),
            torrent_peers: None,
            resource_limits: resource_limits(2 * MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            verified_info: Some(raw_info),
            verified_pieces: vec![false; layout.piece_count()],
            artifact_state: ResumeArtifactState::Staging,
            resume_validation: ResumeValidationIntent::Full,
            download_missing: true,
            dht: None,
            udp_trackers: None,
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
async fn fast_resume_accepts_complete_publication_without_checker_or_hashing() {
    let payload = (0..(2 * MIN_PAYLOAD_ALLOWANCE + 731))
        .map(|index| ((index * 29 + index / 11) & 0xff) as u8)
        .collect::<Vec<_>>();
    let raw_info = single_file_info_with_piece_length(&payload, MIN_PAYLOAD_ALLOWANCE);
    let metainfo = Metainfo::from_info_bytes(&raw_info).expect("fast-resume metainfo");
    let root = test_path("fast-resume-complete");
    tokio::fs::create_dir(&root)
        .await
        .expect("create storage root");
    let paths = torrent_storage_paths_for_metainfo(&root, &metainfo).expect("plan storage");
    tokio::fs::write(&paths.output, &payload)
        .await
        .expect("write complete publication");
    let layout = TorrentLayout::from_metainfo(&metainfo);
    let checkpoints = Arc::new(RecordingCheckpointSink::default());
    let activity = Arc::new(RecordingActivitySink::default());
    let control = DownloadControl::new();
    control.set_activity_sink(activity.clone());
    let unused_peer = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind unused fast-resume peer");
    let peer_address = unused_peer.local_addr().expect("unused peer address");

    let report = resume_magnet_with_control(
        ResumableMagnetDownloadConfig {
            magnet: format!(
                "magnet:?xt=urn:btih:{}&x.pe={peer_address}",
                hex(&metainfo.info_hash)
            ),
            storage_root: root.clone(),
            network: loopback_network(Duration::from_secs(1)),
            peer_budget: PeerBudget::system_default(),
            mse_dh: crate::MseDhWorkOwner::new(),
            encryption: crate::PeerEncryptionPolicyHandle::default(),
            torrent_peers: None,
            resource_limits: resource_limits(2 * MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            verified_info: Some(raw_info),
            verified_pieces: vec![true; layout.piece_count()],
            artifact_state: ResumeArtifactState::Published,
            resume_validation: ResumeValidationIntent::FastEligible,
            download_missing: true,
            dht: None,
            udp_trackers: Some(Vec::new()),
        },
        checkpoints.clone(),
        control,
    )
    .await
    .expect("accept complete fast resume");

    assert_eq!(report.bytes_written, 0);
    assert!(checkpoints.rechecks().is_empty());
    let events = activity
        .events
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert!(events.iter().any(|event| matches!(
        event,
        DownloadActivityEvent::FastResumeAccepted {
            committed_pieces,
            payload_bytes_read: 0,
            hash_jobs: 0,
            ..
        } if *committed_pieces == layout.piece_count()
    )));
    assert!(!events.iter().any(|event| matches!(
        event,
        DownloadActivityEvent::CheckerProgress(_)
            | DownloadActivityEvent::CheckerFinished { .. }
            | DownloadActivityEvent::PieceHashing { .. }
    )));
    drop(events);
    tokio::fs::remove_dir_all(root).await.expect("remove root");
}

#[tokio::test]
async fn cancelling_platform_fast_resume_drops_observation_without_admission() {
    let payload = vec![0x51; MIN_PAYLOAD_ALLOWANCE];
    let raw_info = single_file_info_with_piece_length(&payload, MIN_PAYLOAD_ALLOWANCE);
    let metainfo = Metainfo::from_info_bytes(&raw_info).expect("platform resume metainfo");
    let layout = TorrentLayout::from_metainfo(&metainfo);
    let (client, broker) = crate::platform_storage_channel();
    let pool = crate::StorageFilePool::new(4, Some(client)).expect("platform storage pool");
    let checkpoints = Arc::new(RecordingCheckpointSink::default());
    let activity = Arc::new(RecordingActivitySink::default());
    let control = DownloadControl::new();
    control.set_activity_sink(activity.clone());
    control.set_platform_storage(crate::PlatformStorageSpec {
        pool: pool.clone(),
        root_id: "downloads".to_owned(),
        storage_id: hex(&metainfo.info_hash),
        publication_name: metainfo.name.clone(),
        publication_shape: crate::PublicationShape::from_metainfo(&metainfo),
        namespace_generation: 1,
        managed: true,
        published: true,
    });
    let unused_peer = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind unused platform peer");
    let peer_address = unused_peer.local_addr().expect("unused peer address");
    let task = tokio::spawn(resume_magnet_with_control(
        ResumableMagnetDownloadConfig {
            magnet: format!(
                "magnet:?xt=urn:btih:{}&x.pe={peer_address}",
                hex(&metainfo.info_hash)
            ),
            storage_root: test_path("unused-platform-root"),
            network: loopback_network(Duration::from_secs(1)),
            peer_budget: PeerBudget::system_default(),
            mse_dh: crate::MseDhWorkOwner::new(),
            encryption: crate::PeerEncryptionPolicyHandle::default(),
            torrent_peers: None,
            resource_limits: resource_limits(2 * MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            verified_info: Some(raw_info),
            verified_pieces: vec![true; layout.piece_count()],
            artifact_state: ResumeArtifactState::Published,
            resume_validation: ResumeValidationIntent::FastEligible,
            download_missing: true,
            dht: None,
            udp_trackers: Some(Vec::new()),
        },
        checkpoints.clone(),
        control.clone(),
    ));
    let request = broker.next_request().await.expect("validation observation");
    assert_eq!(request.operation, crate::PlatformStorageOperation::Observe);
    assert_eq!(pool.snapshot().platform_pending, 1);
    control.cancel();
    assert!(matches!(
        timeout(Duration::from_secs(1), task)
            .await
            .expect("cancelled validation timed out")
            .expect("join cancelled validation"),
        Err(DownloadError::Cancelled)
    ));
    assert_eq!(pool.snapshot().platform_pending, 0);
    assert!(checkpoints.rechecks().is_empty());
    let events = activity
        .events
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert!(!events.iter().any(|event| matches!(
        event,
        DownloadActivityEvent::FastResumeAccepted { .. }
            | DownloadActivityEvent::FastResumeRejected { .. }
    )));
    drop(events);
    broker.cancel_all();
    pool.shutdown().await.expect("shutdown platform pool");
}

#[tokio::test]
async fn full_recheck_verifies_readable_skipped_pieces() {
    let first = vec![0x31; MIN_PAYLOAD_ALLOWANCE];
    let second = vec![0x72; MIN_PAYLOAD_ALLOWANCE];
    let torrent_bytes = two_piece_metainfo(&first, &second);
    let raw_info = Metainfo::info_bytes_with_limits(&torrent_bytes, BEP9_METAINFO_LIMITS)
        .expect("two-file raw info")
        .to_vec();
    let metainfo = Metainfo::from_info_bytes(&raw_info).expect("two-file recheck metainfo");
    let root = test_path("full-recheck-skipped-readable");
    tokio::fs::create_dir(&root)
        .await
        .expect("create storage root");
    let paths = torrent_storage_paths_for_metainfo(&root, &metainfo).expect("managed paths");
    let layout = TorrentLayout::from_metainfo(&metainfo);
    let all_wanted = FileSelection::new(&layout, &[]).expect("all files wanted");
    let mut storage =
        SelectiveStorage::create_with_paths(paths.clone(), &metainfo, layout.clone(), all_wanted)
            .await
            .expect("create staging storage");
    for (piece_index, payload) in [first.as_slice(), second.as_slice()]
        .into_iter()
        .enumerate()
    {
        let piece_index = u32::try_from(piece_index).expect("piece index");
        storage
            .write_block(piece_index, 0, payload.to_vec())
            .await
            .expect("write staged piece");
        storage.sync_piece(piece_index).await.expect("sync piece");
    }
    drop(storage);

    let skipped = FileSelection::new(&layout, &[1]).expect("second file skipped");
    let (mut storage, resumed) = SelectiveStorage::resume_with_paths(
        paths,
        &metainfo,
        layout.clone(),
        skipped.clone(),
        vec![false; layout.piece_count()],
    )
    .await
    .expect("resume with skipped retained destination");
    assert_eq!(resumed, ResumedStorage::Staging);
    let control = DownloadControl::new();
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());
    control.checker_started(1, layout.piece_count());
    let mut selection = AppliedFileSelection {
        selection: skipped,
        revision: 0,
    };
    let checked = full_recheck_managed_storage(
        &mut storage,
        &metainfo,
        &layout,
        &vec![false; layout.piece_count()],
        &mut selection,
        &control,
    )
    .await
    .expect("recheck skipped readable piece");
    control.checker_set_phase(CheckerPhase::Finalizing);

    assert_eq!(checked.verified, vec![true, true]);
    let finalizing = activity
        .events
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .filter_map(|event| match event {
            DownloadActivityEvent::CheckerProgress(progress)
                if progress.phase == CheckerPhase::Finalizing =>
            {
                Some(progress.as_ref())
            }
            _ => None,
        })
        .next_back()
        .expect("final checker progress")
        .clone();
    assert_eq!(finalizing.pieces_total, 2);
    assert_eq!(finalizing.pieces_processed, 2);
    assert_eq!(finalizing.pieces_matched, 2);
    assert_eq!(finalizing.bytes_hashed, (2 * MIN_PAYLOAD_ALLOWANCE) as u64);
    assert!(
        activity
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .all(|event| !matches!(event, DownloadActivityEvent::PieceHashFailed { .. }))
    );
    control.checker_finished(1);
    assert!(control.checker_snapshot().is_none());

    tokio::fs::remove_dir_all(root)
        .await
        .expect("remove skipped recheck fixture");
}

#[tokio::test]
async fn selection_fence_and_slow_hash_heartbeat_share_one_check_generation() {
    let first = vec![0x19; MIN_PAYLOAD_ALLOWANCE];
    let second = vec![0x83; MIN_PAYLOAD_ALLOWANCE];
    let torrent_bytes = two_piece_metainfo(&first, &second);
    let raw_info = Metainfo::info_bytes_with_limits(&torrent_bytes, BEP9_METAINFO_LIMITS)
        .expect("two-file raw info")
        .to_vec();
    let metainfo = Metainfo::from_info_bytes(&raw_info).expect("two-file recheck metainfo");
    let root = test_path("selection-during-slow-recheck");
    tokio::fs::create_dir(&root)
        .await
        .expect("create storage root");
    let paths = torrent_storage_paths_for_metainfo(&root, &metainfo).expect("managed paths");
    let layout = TorrentLayout::from_metainfo(&metainfo);
    let selection = FileSelection::new(&layout, &[]).expect("all files wanted");
    let mut storage =
        SelectiveStorage::create_with_paths(paths, &metainfo, layout.clone(), selection.clone())
            .await
            .expect("create staged storage");
    for (piece_index, payload) in [first, second].into_iter().enumerate() {
        let piece_index = u32::try_from(piece_index).expect("piece index");
        storage
            .write_block(piece_index, 0, payload)
            .await
            .expect("write staged piece");
        storage.sync_piece(piece_index).await.expect("sync piece");
    }

    let control = DownloadControl::new();
    control
        .set_storage_execution_limits_for_testing(1, 1)
        .expect("single checker job");
    control.set_storage_hash_delay(Duration::from_millis(1_100));
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());
    control.checker_started(11, layout.piece_count());
    let task_control = control.clone();
    let task_metainfo = metainfo.clone();
    let task_layout = layout.clone();
    let task = tokio::spawn(async move {
        let mut selection = AppliedFileSelection {
            selection,
            revision: 0,
        };
        let result = full_recheck_managed_storage(
            &mut storage,
            &task_metainfo,
            &task_layout,
            &vec![false; task_layout.piece_count()],
            &mut selection,
            &task_control,
        )
        .await;
        (result, selection)
    });
    timeout(Duration::from_secs(1), async {
        while control
            .checker_snapshot()
            .is_none_or(|progress| progress.active_hash_jobs != 1)
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("first checker job did not start");
    control.update_file_selection(FileSelectionUpdate {
        revision: 7,
        skip_files: vec![1],
    });
    timeout(Duration::from_millis(1_050), async {
        loop {
            let heartbeat = activity
                .events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .iter()
                .filter_map(|event| match event {
                    DownloadActivityEvent::CheckerProgress(progress)
                        if progress.phase == CheckerPhase::ReconcilingStorage
                            || progress
                                .oldest_active_job_age_millis
                                .is_some_and(|age| age >= 900) =>
                    {
                        Some(progress.as_ref().clone())
                    }
                    _ => None,
                })
                .next();
            if heartbeat.is_some_and(|progress| progress.active_hash_jobs == 1) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("slow hash did not publish a live heartbeat");

    assert!(control.pause_checking());
    timeout(Duration::from_secs(2), async {
        while control
            .checker_snapshot()
            .is_none_or(|progress| progress.phase != CheckerPhase::Paused)
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("checker did not drain into its paused phase");
    let paused = control.checker_snapshot().expect("paused checker snapshot");
    assert_eq!(paused.generation, 11);
    assert_eq!(paused.pieces_processed, 1);
    assert_eq!(paused.active_hash_jobs, 0);
    assert!(!task.is_finished());
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        control
            .checker_snapshot()
            .expect("retained paused checker")
            .pieces_processed,
        paused.pieces_processed
    );
    control.resume_checking();

    let (checked, final_selection) = timeout(Duration::from_secs(3), task)
        .await
        .expect("selection-aware checker did not finish")
        .expect("checker task joined");
    assert_eq!(checked.expect("checker result").verified, vec![true, true]);
    assert_eq!(final_selection.revision, 7);
    assert!(!final_selection.selection.is_wanted(1));
    assert_eq!(control.applied_file_selection_revision(), 7);
    let progress = activity
        .events
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .filter_map(|event| match event {
            DownloadActivityEvent::CheckerProgress(progress) => Some(progress.as_ref()),
            _ => None,
        })
        .cloned()
        .collect::<Vec<_>>();
    assert!(progress.iter().all(|progress| progress.generation == 11));
    assert!(
        progress
            .iter()
            .any(|progress| progress.phase == CheckerPhase::ReconcilingStorage)
    );
    assert!(
        progress
            .iter()
            .any(|progress| progress.phase == CheckerPhase::Paused)
    );
    assert!(
        progress
            .iter()
            .all(|progress| progress.active_hash_jobs <= 1)
    );

    tokio::fs::remove_dir_all(root)
        .await
        .expect("remove selection recheck fixture");
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
            peer_budget: PeerBudget::system_default(),
            mse_dh: crate::MseDhWorkOwner::new(),
            encryption: crate::PeerEncryptionPolicyHandle::default(),
            torrent_peers: None,
            resource_limits: resource_limits(2 * MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            verified_info: Some(raw_info),
            verified_pieces: vec![true; layout.piece_count()],
            artifact_state: ResumeArtifactState::Staging,
            resume_validation: ResumeValidationIntent::Full,
            download_missing: true,
            dht: None,
            udp_trackers: None,
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
            peer_budget: PeerBudget::system_default(),
            mse_dh: crate::MseDhWorkOwner::new(),
            encryption: crate::PeerEncryptionPolicyHandle::default(),
            torrent_peers: None,
            resource_limits: resource_limits(2 * MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            verified_info: Some(raw_info),
            verified_pieces: vec![false; layout.piece_count()],
            artifact_state: ResumeArtifactState::Staging,
            resume_validation: ResumeValidationIntent::Full,
            download_missing: true,
            dht: None,
            udp_trackers: None,
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
                peer_budget: PeerBudget::system_default(),
                mse_dh: crate::MseDhWorkOwner::new(),
                encryption: crate::PeerEncryptionPolicyHandle::default(),
                torrent_peers: None,
                resource_limits: resource_limits(2 * MIN_PAYLOAD_ALLOWANCE),
                skip_files: Vec::new(),
                verified_info: Some(raw_info.clone()),
                verified_pieces: vec![false; layout.piece_count()],
                artifact_state: ResumeArtifactState::Staging,
                resume_validation: ResumeValidationIntent::Full,
                download_missing: true,
                dht: None,
                udp_trackers: None,
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
                peer_budget: PeerBudget::system_default(),
                mse_dh: crate::MseDhWorkOwner::new(),
                encryption: crate::PeerEncryptionPolicyHandle::default(),
                torrent_peers: None,
                resource_limits: resource_limits(2 * MIN_PAYLOAD_ALLOWANCE),
                skip_files: Vec::new(),
                verified_info: Some(raw_info.clone()),
                verified_pieces: vec![false; layout.piece_count()],
                artifact_state: ResumeArtifactState::Publishing,
                resume_validation: ResumeValidationIntent::Full,
                download_missing: true,
                dht: None,
                udp_trackers: None,
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
