use super::*;
use crate::PeerBudget;

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
    assert_eq!(online.registry_len(), 1);

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
            PeerBudget::system_default(),
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
    assert!(peers.registry_is_empty());

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

    assert_eq!(peers.registry_len(), 2);
    let failed = peers
        .peers
        .with_state(|state| {
            state
                .registry
                .find_endpoint(PeerEndpoint::new(unreachable).expect("failed endpoint"))
                .cloned()
        })
        .expect("failed tracker peer retained");
    assert_eq!(failed.history().total_failures, 1);
    assert_eq!(failed.history().last_failure, Some(PeerFailure::Connect));
    assert!(failed.sources().contains(PeerSource::Tracker));
    let succeeded = peers
        .peers
        .with_state(|state| {
            state
                .registry
                .find_endpoint(PeerEndpoint::new(reachable).expect("successful endpoint"))
                .cloned()
        })
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

    assert_eq!(peers.registry_len(), 3);
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
    assert_eq!(peers.registry_len(), 1);

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
        loopback_network(Duration::from_secs(1)),
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
    assert!(peers.registry_is_empty());
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
            peer_id: CLIENT_PEER_ID,
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
            peer_id: CLIENT_PEER_ID,
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
        .peers
        .with_state(|state| {
            state
                .registry
                .find_endpoint(PeerEndpoint::new(stalled_address).expect("stalled endpoint"))
                .cloned()
        })
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
        .peers
        .with_state(|state| {
            state
                .registry
                .find_endpoint(PeerEndpoint::new(useful_address).expect("tracker endpoint"))
                .cloned()
        })
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
    assert_eq!(peers.registry_len(), 2);

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
        .peers
        .with_state(|state| {
            state
                .registry
                .find_endpoint(PeerEndpoint::new(unreachable).expect("failed endpoint"))
                .cloned()
        })
        .expect("failed peer record retained");
    assert_eq!(failed.phase(), PeerPhase::Idle);
    assert_eq!(failed.history().dial_attempts, 1);
    assert_eq!(failed.history().total_failures, 1);
    assert_eq!(failed.history().last_failure, Some(PeerFailure::Connect));
    assert!(failed.history().retry_at.is_some());
    assert!(failed.sources().contains(PeerSource::MagnetHint));

    let connected = peers
        .peers
        .with_state(|state| {
            state
                .registry
                .find_endpoint(PeerEndpoint::new(address).expect("connected endpoint"))
                .cloned()
        })
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
