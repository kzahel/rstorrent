use super::*;
use crate::PeerBudget;

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
            resource_limits: resource_limits(2 * MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            verified_info: Some(raw_info),
            verified_pieces: vec![false; layout.piece_count()],
            artifact_state: ResumeArtifactState::Staging,
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
            resource_limits: resource_limits(2 * MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            verified_info: Some(raw_info),
            verified_pieces: vec![true; layout.piece_count()],
            artifact_state: ResumeArtifactState::Staging,
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
            resource_limits: resource_limits(2 * MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            verified_info: Some(raw_info),
            verified_pieces: vec![false; layout.piece_count()],
            artifact_state: ResumeArtifactState::Staging,
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
                resource_limits: resource_limits(2 * MIN_PAYLOAD_ALLOWANCE),
                skip_files: Vec::new(),
                verified_info: Some(raw_info.clone()),
                verified_pieces: vec![false; layout.piece_count()],
                artifact_state: ResumeArtifactState::Staging,
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
                resource_limits: resource_limits(2 * MIN_PAYLOAD_ALLOWANCE),
                skip_files: Vec::new(),
                verified_info: Some(raw_info.clone()),
                verified_pieces: vec![false; layout.piece_count()],
                artifact_state: ResumeArtifactState::Publishing,
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
