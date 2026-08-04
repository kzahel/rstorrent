//! Projection mapping and typed-patch behavior.

use super::support::*;

#[tokio::test]
async fn dht_view_replaces_and_coalesces_one_complete_observation() {
    let hub = ViewHub::new(&snapshot(0, 4)).expect("hub");
    let subscription = hub.subscribe(dht_spec()).expect("DHT subscription");
    let initial = subscription
        .next_update()
        .await
        .expect("initial DHT snapshot");
    let ViewUpdatePayload::Snapshot {
        snapshot: ViewSnapshot::SessionDht { inspection },
    } = initial.payload
    else {
        panic!("expected DHT snapshot");
    };
    assert_eq!(inspection.buckets_v4.len(), 160);

    let mut first = DhtInspectionView::inactive();
    first.captured_millis = "1".to_owned();
    first.queries_sent = "10".to_owned();
    hub.publish_dht(first).expect("first observation");
    let mut latest = DhtInspectionView::inactive();
    latest.captured_millis = "2".to_owned();
    latest.queries_sent = "11".to_owned();
    hub.publish_dht(latest).expect("latest observation");

    let update = subscription
        .next_update()
        .await
        .expect("coalesced DHT patch");
    let ViewUpdatePayload::Patch {
        patch: ViewPatch::SessionDht { inspection },
    } = update.payload
    else {
        panic!("expected DHT replacement patch");
    };
    assert_eq!(inspection.captured_millis, "2");
    assert_eq!(inspection.queries_sent, "11");
    assert_eq!(inspection.buckets_v4.len(), 160);
}

#[test]
fn speed_clock_uses_the_fastest_interested_live_range() {
    let hub = ViewHub::new(&snapshot(0, 4)).expect("hub");
    assert_eq!(hub.speed_tick_interval(), None);
    let historical = hub
        .subscribe(speed_spec(SpeedRange::Hours24))
        .expect("historical subscription");
    assert_eq!(hub.speed_tick_interval(), None);
    let short = hub
        .subscribe(speed_spec(SpeedRange::Minutes2))
        .expect("short subscription");
    assert_eq!(hub.speed_tick_interval(), Some(Duration::from_millis(500)));
    let recent = hub
        .subscribe(speed_spec(SpeedRange::Seconds30))
        .expect("recent subscription");
    assert_eq!(hub.speed_tick_interval(), Some(Duration::from_millis(100)));
    drop(recent);
    assert_eq!(hub.speed_tick_interval(), Some(Duration::from_millis(500)));
    drop(short);
    drop(historical);
    assert_eq!(hub.speed_tick_interval(), None);
}

#[tokio::test]
async fn session_disk_view_publishes_pipeline_rates_and_keyed_piece_changes() {
    let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
    let hub = ViewHub::new(&snapshot(0, 4)).expect("hub");
    let subscription = hub
        .subscribe(SubscriptionSpec {
            selector: ViewSelector::TorrentList,
            projection: ViewProjection::Disk,
            delivery: DeliveryPolicy {
                min_interval_millis: 0,
                max_queue_bytes: 64 * 1024,
            },
            diagnostics: None,
        })
        .expect("disk subscription");
    let initial = subscription.next_update().await.expect("initial disk");
    assert!(matches!(
        initial.payload,
        ViewUpdatePayload::Snapshot {
            snapshot: ViewSnapshot::SessionDisk { ref pieces, ref pipeline }
        } if pieces.is_empty()
            && pipeline.pressure == DiskPressureView::Idle
            && pipeline.checkpoint_stage == DiskCheckpointStageView::Idle
    ));

    hub.record_disk_runtime(torrent_id, &disk_snapshot(1_000, 4_096))
        .expect("first disk sample");
    let first = subscription.next_update().await.expect("first disk patch");
    assert!(matches!(
        first.payload,
        ViewUpdatePayload::Patch {
            patch: ViewPatch::SessionDisk { ref pipeline, ref upsert, ref removed }
        } if pipeline.intake_backpressured
            && pipeline.checkpoint_stage == DiskCheckpointStageView::Syncing
            && pipeline.checkpoint_dirty_pieces == "3"
            && pipeline.checkpoint_dirty_bytes == (3 * 16 * 1024).to_string()
            && pipeline.checkpoint_sync_service_micros == "600"
            && pipeline.checkpoint_active_micros.as_deref() == Some("200")
            && pipeline.receive_rate_bytes == "0"
            && upsert.len() == 1
            && removed.is_empty()
    ));

    let mut next = disk_snapshot(2_000, 8_192);
    next.pressure = DiskPressure::Draining;
    next.intake_backpressured = false;
    next.checkpoint_stage = DiskCheckpointStage::Idle;
    next.checkpoint_dirty_pieces = 0;
    next.checkpoint_dirty_bytes = 0;
    next.checkpoint_batches_completed = 2;
    next.checkpoint_active_micros = None;
    next.pieces.clear();
    hub.record_disk_runtime(torrent_id, &next)
        .expect("second disk sample");
    let second = subscription.next_update().await.expect("second disk patch");
    assert!(matches!(
        second.payload,
        ViewUpdatePayload::Patch {
            patch: ViewPatch::SessionDisk { ref pipeline, ref upsert, ref removed }
        } if pipeline.pressure == DiskPressureView::Draining
            && pipeline.checkpoint_stage == DiskCheckpointStageView::Idle
            && pipeline.checkpoint_dirty_pieces == "0"
            && pipeline.checkpoint_batches_completed == "2"
            && pipeline.checkpoint_active_micros.is_none()
            && pipeline.receive_rate_bytes == "4096"
            && upsert.is_empty()
            && removed == &[format!("{torrent_id}:3:1")]
    ));

    hub.clear_disk_runtime(torrent_id)
        .expect("clear terminal disk runtime");
    let terminal = subscription
        .next_update()
        .await
        .expect("terminal disk patch");
    assert!(matches!(
        terminal.payload,
        ViewUpdatePayload::Patch {
            patch: ViewPatch::SessionDisk { ref pipeline, ref upsert, ref removed }
        } if pipeline.pressure == DiskPressureView::Idle
            && upsert.is_empty()
            && removed.is_empty()
    ));
}

#[tokio::test]
async fn tracker_state_publishes_complete_keyed_rows_and_terminal_inactive_state() {
    let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
    let hub = ViewHub::new(&snapshot(0, 4)).expect("hub");
    let subscription = hub
        .subscribe(SubscriptionSpec {
            selector: ViewSelector::Torrent {
                torrent_id: torrent_id.to_owned(),
            },
            projection: ViewProjection::Trackers,
            delivery: DeliveryPolicy {
                min_interval_millis: 0,
                max_queue_bytes: 16 * 1024,
            },
            diagnostics: None,
        })
        .expect("tracker subscription");
    let summary = hub
        .subscribe(SubscriptionSpec {
            selector: ViewSelector::Torrent {
                torrent_id: torrent_id.to_owned(),
            },
            projection: ViewProjection::Summary,
            delivery: DeliveryPolicy {
                min_interval_millis: 0,
                max_queue_bytes: 16 * 1024,
            },
            diagnostics: None,
        })
        .expect("summary subscription");
    let initial = subscription.next_update().await.expect("initial snapshot");
    summary.next_update().await.expect("initial summary");
    assert!(matches!(
        initial.payload,
        ViewUpdatePayload::Snapshot {
            snapshot: ViewSnapshot::Trackers { trackers, .. }
        } if trackers.is_empty()
    ));

    hub.record_tracker_state(
        torrent_id,
        &tracker_snapshot(TrackerRuntimeStatus::ReannounceWait, 1),
    )
    .expect("tracker success state");
    let update = subscription.next_update().await.expect("success patch");
    assert!(matches!(
        update.payload,
        ViewUpdatePayload::Patch {
            patch: ViewPatch::Trackers { ref upsert, ref removed, .. }
        } if upsert.len() == 1
            && removed.is_empty()
            && upsert[0].last_peer_count == Some(9)
    ));
    let summary_update = summary.next_update().await.expect("summary count patch");
    assert!(matches!(
        summary_update.payload,
        ViewUpdatePayload::Patch {
            patch: ViewPatch::Torrent {
                torrent: Some(ref torrent)
            }
        } if torrent.configured_tracker_count == Some(1)
    ));

    hub.record_tracker_state(
        torrent_id,
        &tracker_snapshot(TrackerRuntimeStatus::Inactive, 2),
    )
    .expect("tracker terminal state");
    let terminal = subscription.next_update().await.expect("terminal patch");
    assert!(matches!(
        terminal.payload,
        ViewUpdatePayload::Patch {
            patch: ViewPatch::Trackers { ref upsert, .. }
        } if upsert.len() == 1
            && matches!(
                upsert[0].status,
                crate::tracker_views::TrackerStatusView::Inactive
            )
    ));
}

#[tokio::test]
async fn swarm_projection_keeps_registry_rows_after_connections_and_clears_terminally() {
    let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
    let hub = ViewHub::new(&snapshot(0, 4)).expect("hub");
    let subscription = hub
        .subscribe(SubscriptionSpec {
            selector: ViewSelector::Torrent {
                torrent_id: torrent_id.to_owned(),
            },
            projection: ViewProjection::Swarm,
            delivery: DeliveryPolicy {
                min_interval_millis: 0,
                max_queue_bytes: 64 * 1024,
            },
            diagnostics: None,
        })
        .expect("swarm subscription");
    let initial = subscription.next_update().await.expect("initial snapshot");
    assert!(matches!(
        initial.payload,
        ViewUpdatePayload::Snapshot {
            snapshot: ViewSnapshot::Swarm {
                state: SwarmCatalogState::Inactive,
                ref peers,
                ..
            }
        } if peers.is_empty()
    ));

    let mut registry = PeerRegistry::new(PeerRegistryConfig {
        max_records: 1,
        ..PeerRegistryConfig::default()
    })
    .expect("registry");
    let endpoint = PeerEndpoint::new("127.0.0.1:6881".parse().expect("address")).expect("endpoint");
    registry
        .observe(
            PeerObservation::dialable(endpoint, PeerSource::Tracker),
            Duration::from_secs(1),
        )
        .expect("tracker candidate");
    registry
        .observe(
            PeerObservation::dialable(endpoint, PeerSource::Dht),
            Duration::from_secs(2),
        )
        .expect("merged source");
    let active = registry.snapshot(PeerSelectionContext {
        now: Duration::from_secs(3),
    });
    hub.record_peer_registry_state(torrent_id, true, &active)
        .expect("active registry");
    let update = subscription.next_update().await.expect("active patch");
    assert!(matches!(
        update.payload,
        ViewUpdatePayload::Patch {
            patch: ViewPatch::Swarm {
                state: SwarmCatalogState::Active,
                ref counts,
                ref upsert,
                ref removed,
                ..
            }
        } if counts.total == 1
            && counts.eligible == 1
            && upsert.len() == 1
            && upsert[0].sources.len() == 2
            && removed.is_empty()
    ));

    let replacement_endpoint =
        PeerEndpoint::new("127.0.0.1:6882".parse().expect("address")).expect("endpoint");
    registry
        .observe(
            PeerObservation::dialable(replacement_endpoint, PeerSource::Tracker),
            Duration::from_secs(4),
        )
        .expect("capacity replacement");
    let replacement = registry.snapshot(PeerSelectionContext {
        now: Duration::from_secs(5),
    });
    hub.record_peer_registry_state(torrent_id, true, &replacement)
        .expect("replacement registry");
    let update = subscription.next_update().await.expect("replacement patch");
    assert!(matches!(
        update.payload,
        ViewUpdatePayload::Patch {
            patch: ViewPatch::Swarm {
                ref counts,
                ref upsert,
                ref removed,
                ..
            }
        } if counts.total == 1
            && upsert.len() == 1
            && upsert[0].peer_record_id == "2"
            && removed == &["1".to_owned()]
    ));

    hub.record_peer_connections(torrent_id, Duration::from_secs(6), &[])
        .expect("empty connection projection");
    let current = hub
        .inner
        .lock()
        .expect("hub state")
        .snapshot_for(&SubscriptionSpec {
            selector: ViewSelector::Torrent {
                torrent_id: torrent_id.to_owned(),
            },
            projection: ViewProjection::Swarm,
            delivery: DeliveryPolicy::default(),
            diagnostics: None,
        });
    assert!(matches!(
        current,
        ViewSnapshot::Swarm { ref peers, .. } if peers.len() == 1
    ));

    let terminal = PeerRegistry::new(PeerRegistryConfig::default())
        .expect("terminal registry")
        .snapshot(PeerSelectionContext {
            now: Duration::from_secs(7),
        });
    hub.record_peer_registry_state(torrent_id, false, &terminal)
        .expect("terminal registry state");
    let update = subscription.next_update().await.expect("terminal patch");
    assert!(matches!(
        update.payload,
        ViewUpdatePayload::Patch {
            patch: ViewPatch::Swarm {
                state: SwarmCatalogState::Inactive,
                ref counts,
                ref upsert,
                ref removed,
                ..
            }
        } if counts.total == 0
            && upsert.is_empty()
            && removed == &["2".to_owned()]
    ));
}

#[tokio::test]
async fn piece_hash_failure_clears_unverified_active_ranges() {
    let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
    let hub = ViewHub::new(&snapshot(0, 1)).expect("hub");
    let subscription = hub.subscribe(piece_spec(4096)).expect("subscribe");
    subscription.next_update().await.expect("snapshot");
    hub.record_activity(
        torrent_id,
        TorrentActivity::PieceStarted {
            piece_index: 0,
            piece_length: 16 * 1024,
            attempt: 1,
        },
    )
    .expect("start piece");
    subscription.next_update().await.expect("start patch");
    hub.record_activity(
        torrent_id,
        TorrentActivity::BlockStored {
            piece_index: 0,
            begin: 0,
            length: 16 * 1024,
        },
    )
    .expect("stored block");
    subscription.next_update().await.expect("stored patch");
    hub.record_activity(
        torrent_id,
        TorrentActivity::PieceHashFailed { piece_index: 0 },
    )
    .expect("failed piece");
    let update = subscription.next_update().await.expect("reset patch");
    let ViewUpdatePayload::Patch {
        patch: ViewPatch::PieceActivity {
            ref active_upsert, ..
        },
    } = update.payload
    else {
        panic!("expected active-piece reset patch");
    };
    assert_eq!(active_upsert.len(), 1);
    assert!(active_upsert[0].requested.is_empty());
    assert!(active_upsert[0].received.is_empty());
    assert!(active_upsert[0].stored.is_empty());
    assert_eq!(active_upsert[0].stage, ActivePieceStageView::Failed);
}

#[tokio::test]
async fn piece_runtime_tracks_simultaneous_attempts_and_keyed_retry_cleanup() {
    let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
    let hub = ViewHub::new(&snapshot(0, 4)).expect("hub");
    let subscription = hub.subscribe(piece_spec(16 * 1024)).expect("subscribe");
    subscription.next_update().await.expect("snapshot");
    let piece = |piece_index, attempt, stage| DiskPieceRuntimeSnapshot {
        piece_index,
        piece_length: 16 * 1024,
        attempt,
        stage,
        requested_bytes: 16 * 1024,
        received_bytes: 0,
        stored_bytes: 0,
        age_millis: 50,
        stage_age_millis: 10,
        error: (stage == DiskPieceStage::Failed).then(|| "piece hash failed; retrying".to_owned()),
    };
    hub.record_piece_runtime(
        torrent_id,
        &[
            piece(0, 1, DiskPieceStage::Receiving),
            piece(2, 1, DiskPieceStage::Hashing),
        ],
    )
    .expect("simultaneous runtime");
    let first = subscription.next_update().await.expect("active patch");
    assert!(matches!(
        first.payload,
        ViewUpdatePayload::Patch {
            patch: ViewPatch::PieceActivity { ref active_upsert, ref active_removed, .. }
        } if active_upsert.len() == 2 && active_removed.is_empty()
    ));

    hub.record_piece_runtime(torrent_id, &[piece(0, 1, DiskPieceStage::Failed)])
        .expect("failed attempt");
    subscription.next_update().await.expect("failed patch");
    hub.record_piece_runtime(torrent_id, &[piece(0, 2, DiskPieceStage::Receiving)])
        .expect("retry attempt");
    let retry = subscription.next_update().await.expect("retry patch");
    assert!(matches!(
        retry.payload,
        ViewUpdatePayload::Patch {
            patch: ViewPatch::PieceActivity { ref active_upsert, ref active_removed, .. }
        } if active_upsert.len() == 1
            && active_upsert[0].piece_id == "0:2"
            && active_removed == &["0:1".to_owned()]
    ));

    hub.clear_piece_runtime(torrent_id)
        .expect("terminal cleanup");
    let terminal = subscription.next_update().await.expect("terminal patch");
    assert!(matches!(
        terminal.payload,
        ViewUpdatePayload::Patch {
            patch: ViewPatch::PieceActivity { ref active_upsert, ref active_removed, .. }
        } if active_upsert.is_empty() && active_removed == &["0:2".to_owned()]
    ));
}

#[test]
fn durable_replacement_preserves_exact_have_ranges() {
    let hub = ViewHub::new(&snapshot(0, 4)).expect("hub");
    hub.replace_durable(
        &snapshot(1, 4),
        &BTreeMap::from([(
            "000102030405060708090a0b0c0d0e0f10111213".to_owned(),
            DurableTorrentViewState {
                display_name: Some("Verified fixture".to_owned()),
                verified: vec![IndexRange {
                    start: 1,
                    end_exclusive: 3,
                }],
                files: None,
                trackers: TrackerViewModel::default(),
            },
        )]),
    )
    .expect("replace");
}

#[tokio::test]
async fn verified_metadata_name_patches_list_and_selected_summary() {
    let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
    let hub = ViewHub::new(&snapshot(0, 4)).expect("hub");
    let list = hub
        .subscribe(SubscriptionSpec {
            selector: ViewSelector::TorrentList,
            projection: ViewProjection::Summary,
            delivery: DeliveryPolicy {
                min_interval_millis: 0,
                max_queue_bytes: 4096,
            },
            diagnostics: None,
        })
        .expect("list subscription");
    let summary = hub
        .subscribe(SubscriptionSpec {
            selector: ViewSelector::Torrent {
                torrent_id: torrent_id.to_owned(),
            },
            projection: ViewProjection::Summary,
            delivery: DeliveryPolicy {
                min_interval_millis: 0,
                max_queue_bytes: 4096,
            },
            diagnostics: None,
        })
        .expect("summary subscription");
    list.next_update().await.expect("list snapshot");
    summary.next_update().await.expect("summary snapshot");

    hub.replace_durable(
        &snapshot(1, 4),
        &BTreeMap::from([(
            torrent_id.to_owned(),
            DurableTorrentViewState {
                display_name: Some("Verified fixture".to_owned()),
                verified: Vec::new(),
                files: None,
                trackers: TrackerViewModel::default(),
            },
        )]),
    )
    .expect("replace");

    let list_update = list.next_update().await.expect("list patch");
    let ViewUpdatePayload::Patch {
        patch: ViewPatch::TorrentList { upsert, .. },
    } = list_update.payload
    else {
        panic!("expected torrent-list patch");
    };
    assert_eq!(upsert[0].display_name.as_deref(), Some("Verified fixture"));

    let summary_update = summary.next_update().await.expect("summary patch");
    let ViewUpdatePayload::Patch {
        patch: ViewPatch::Torrent {
            torrent: Some(torrent),
        },
    } = summary_update.payload
    else {
        panic!("expected selected-summary patch");
    };
    assert_eq!(torrent.display_name.as_deref(), Some("Verified fixture"));
}

#[test]
fn discovery_exhaustion_waits_when_another_mechanism_can_act() {
    let mut torrent = snapshot(0, 0).torrents.remove(0);
    torrent.state = TorrentState::AwaitingMetadata;
    torrent.metadata_available = false;
    let blocked = assess_progress(
        &torrent,
        ProgressInputs {
            discovery_exhausted: true,
            ..ProgressInputs::default()
        },
    );
    assert_eq!(blocked.disposition, ProgressDisposition::Blocked);
    assert_eq!(blocked.reason, ProgressReason::NoEnabledDiscoverySource);

    let waiting = assess_progress(
        &torrent,
        ProgressInputs {
            discovery_exhausted: true,
            dht_enabled: true,
            ..ProgressInputs::default()
        },
    );
    assert_eq!(waiting.disposition, ProgressDisposition::Waiting);
    assert_eq!(waiting.reason, ProgressReason::WaitingForDiscovery);
}

#[test]
fn disabled_network_is_blocked_without_changing_torrent_intent() {
    let mut torrent = snapshot(0, 0).torrents.remove(0);
    torrent.state = TorrentState::AwaitingMetadata;
    torrent.metadata_available = false;
    let assessment = assess_progress(
        &torrent,
        ProgressInputs {
            network_disabled: true,
            discovery_exhausted: true,
            ..ProgressInputs::default()
        },
    );

    assert_eq!(assessment.disposition, ProgressDisposition::Blocked);
    assert_eq!(assessment.reason, ProgressReason::NetworkDisabled);
    assert_eq!(assessment.actions, vec![ProgressAction::EnableNetwork]);
}
