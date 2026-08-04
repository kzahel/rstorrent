use std::collections::BTreeMap;
use std::time::Duration;

use rstorrent_engine::peer::{
    PeerEndpoint, PeerObservation, PeerRegistry, PeerRegistryConfig, PeerSelectionContext,
    PeerSource,
};
use rstorrent_engine::{
    DiskCheckpointStage, DiskPieceRuntimeSnapshot, DiskPieceStage, DiskPressure,
    DiskRuntimeSnapshot, TrackerNextAction, TrackerRuntimeRecordSnapshot, TrackerRuntimeSnapshot,
    TrackerRuntimeStatus, TrackerSource, TrackerTransport,
};

use super::{
    DeliveryPolicy, DhtInspectionView, DiagnosticFilter, DiagnosticSeverity,
    DurableTorrentViewState, IndexRange, ProgressAction, ProgressDisposition, ProgressInputs,
    ProgressReason, ResetReason, SubscriptionSpec, TorrentActivity, ViewHub, ViewPatch,
    ViewProjection, ViewSelector, ViewSnapshot, ViewUpdatePayload, assess_progress,
    ranges_from_pieces,
};
use crate::diagnostics::{
    DiagnosticCategory, DiagnosticEvent, DiagnosticProfile, DiagnosticRetention, DiagnosticValue,
    MAX_DIAGNOSTIC_EVENTS, MAX_DIAGNOSTIC_PATCH_EVENTS, category,
};
use crate::tracker_views::TrackerViewModel;
use crate::{
    ServiceSnapshot, SpeedMetric, SpeedRange, StorageState, TorrentSnapshot, TorrentState,
};

fn snapshot(revision: u64, piece_count: u32) -> ServiceSnapshot {
    ServiceSnapshot {
        profile_id: "test".to_owned(),
        revision: revision.to_string(),
        storage: Default::default(),
        torrents: vec![TorrentSnapshot {
            torrent_id: "000102030405060708090a0b0c0d0e0f10111213".to_owned(),
            storage_root: "downloads".to_owned(),
            state: TorrentState::Downloading,
            storage_state: StorageState::Staging,
            metadata_available: true,
            piece_count,
            verified_piece_count: 0,
            skip_files: Vec::new(),
            archived: false,
            removal_state: None,
            delete_managed_data_supported: true,
            error: None,
        }],
    }
}

fn piece_spec(queue: u32) -> SubscriptionSpec {
    SubscriptionSpec {
        selector: ViewSelector::Torrent {
            torrent_id: "000102030405060708090a0b0c0d0e0f10111213".to_owned(),
        },
        projection: ViewProjection::PieceActivity,
        delivery: DeliveryPolicy {
            min_interval_millis: 0,
            max_queue_bytes: queue,
        },
        diagnostics: None,
    }
}

fn speed_spec(range: SpeedRange) -> SubscriptionSpec {
    SubscriptionSpec {
        selector: ViewSelector::SessionSpeed {
            range,
            metrics: vec![SpeedMetric::PayloadReceived],
        },
        projection: ViewProjection::Speed,
        delivery: DeliveryPolicy {
            min_interval_millis: 0,
            max_queue_bytes: 64 * 1024,
        },
        diagnostics: None,
    }
}

fn dht_spec() -> SubscriptionSpec {
    SubscriptionSpec {
        selector: ViewSelector::SessionDht,
        projection: ViewProjection::Dht,
        delivery: DeliveryPolicy {
            min_interval_millis: 0,
            max_queue_bytes: 256 * 1024,
        },
        diagnostics: None,
    }
}

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

fn tracker_snapshot(status: TrackerRuntimeStatus, attempts: u32) -> TrackerRuntimeSnapshot {
    TrackerRuntimeSnapshot {
        captured_at: Duration::from_secs(2),
        active: !matches!(status, TrackerRuntimeStatus::Inactive),
        records: vec![TrackerRuntimeRecordSnapshot {
            tracker_id: "udp://tracker.example:6969".to_owned(),
            url: "udp://tracker.example:6969".to_owned(),
            tier: 0,
            source: TrackerSource::Magnet,
            transport: TrackerTransport::Udp,
            status,
            announce_event: None,
            total_attempts: attempts,
            consecutive_failures: u8::from(matches!(status, TrackerRuntimeStatus::RetryWait)),
            last_peer_count: Some(9),
            seeders: Some(4),
            leechers: Some(5),
            interval: Some(Duration::from_secs(600)),
            next_action: Some(if matches!(status, TrackerRuntimeStatus::RetryWait) {
                TrackerNextAction::Retry
            } else {
                TrackerNextAction::Reannounce
            }),
            next_action_in: Some(Duration::from_secs(15)),
            last_success_age: Some(Duration::from_secs(1)),
            last_failure_age: None,
            last_error: None,
        }],
    }
}

fn disk_snapshot(captured_at_millis: u64, received: usize) -> DiskRuntimeSnapshot {
    DiskRuntimeSnapshot {
        captured_at_millis,
        pressure: DiskPressure::Backpressured,
        intake_backpressured: true,
        resident_limit_bytes: 32 * 1024 * 1024,
        resident_high_watermark_bytes: 24 * 1024 * 1024,
        resident_low_watermark_bytes: 16 * 1024 * 1024,
        requested_bytes: 4 * 1024 * 1024,
        resident_bytes: 25 * 1024 * 1024,
        queued_write_bytes: 24 * 1024 * 1024,
        writing_bytes: 256 * 1024,
        hashing_bytes: 0,
        checkpoint_stage: DiskCheckpointStage::Syncing,
        checkpoint_dirty_pieces: 3,
        checkpoint_dirty_bytes: 3 * 16 * 1024,
        checkpoint_dirty_piece_high_water: 5,
        checkpoint_dirty_byte_high_water: 5 * 16 * 1024,
        checkpoint_oldest_dirty_millis: 750,
        checkpoint_batches_started: 2,
        checkpoint_batches_completed: 1,
        checkpoint_pieces_completed: 4,
        checkpoint_sync_operations_completed: 2,
        checkpoint_sync_service_micros: 600,
        checkpoint_sync_service_max_micros: 400,
        checkpoint_commit_service_micros: 300,
        checkpoint_commit_service_max_micros: 300,
        checkpoint_active_micros: Some(200),
        storage_jobs_pending: 96,
        received_bytes_total: received,
        stored_bytes_total: received.saturating_sub(1024),
        verified_bytes_total: received.saturating_sub(2048),
        write_operations_started: 10,
        write_operations_completed: 9,
        hash_operations_started: 2,
        hash_operations_completed: 2,
        write_queue_wait_micros: 4_000,
        write_queue_wait_max_micros: 2_000,
        write_service_micros: 8_000,
        write_service_max_micros: 3_000,
        hash_queue_wait_micros: 500,
        hash_queue_wait_max_micros: 400,
        hash_service_micros: 1_200,
        hash_service_max_micros: 800,
        pressure_transition_count: 1,
        backpressured_millis_total: 900,
        last_error: None,
        pieces: vec![DiskPieceRuntimeSnapshot {
            piece_index: 3,
            piece_length: 16 * 1024,
            attempt: 1,
            stage: DiskPieceStage::Writing,
            requested_bytes: 16 * 1024,
            received_bytes: 16 * 1024,
            stored_bytes: 0,
            age_millis: 100,
            stage_age_millis: 20,
            error: None,
        }],
    }
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
            && pipeline.pressure == super::DiskPressureView::Idle
            && pipeline.checkpoint_stage == super::DiskCheckpointStageView::Idle
    ));

    hub.record_disk_runtime(torrent_id, &disk_snapshot(1_000, 4_096))
        .expect("first disk sample");
    let first = subscription.next_update().await.expect("first disk patch");
    assert!(matches!(
        first.payload,
        ViewUpdatePayload::Patch {
            patch: ViewPatch::SessionDisk { ref pipeline, ref upsert, ref removed }
        } if pipeline.intake_backpressured
            && pipeline.checkpoint_stage == super::DiskCheckpointStageView::Syncing
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
        } if pipeline.pressure == super::DiskPressureView::Draining
            && pipeline.checkpoint_stage == super::DiskCheckpointStageView::Idle
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
        } if pipeline.pressure == super::DiskPressureView::Idle
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
                state: super::SwarmCatalogState::Inactive,
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
                state: super::SwarmCatalogState::Active,
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
                state: super::SwarmCatalogState::Inactive,
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
async fn starts_with_snapshot_and_keeps_large_indices() {
    let hub = ViewHub::new(&snapshot(7, 1_000_000)).expect("hub");
    let subscription = hub.subscribe(piece_spec(4096)).expect("subscribe");
    let update = subscription.next_update().await.expect("snapshot");
    assert_eq!(update.sequence, "1");
    assert_eq!(update.revision, "7");
    let ViewUpdatePayload::Snapshot {
        snapshot:
            ViewSnapshot::PieceActivity {
                piece_count,
                verified,
                ..
            },
    } = update.payload
    else {
        panic!("expected piece snapshot");
    };
    assert_eq!(piece_count, 1_000_000);
    assert!(verified.is_empty());

    hub.record_activity(
        "000102030405060708090a0b0c0d0e0f10111213",
        TorrentActivity::PieceStarted {
            piece_index: 900_000,
            piece_length: 32 * 1024 * 1024,
            attempt: 1,
        },
    )
    .expect("activity");
    let update = subscription.next_update().await.expect("patch");
    let ViewUpdatePayload::Patch { patch } = update.payload else {
        panic!("expected patch");
    };
    let serialized = serde_json::to_string(&patch).expect("serialize");
    assert!(serialized.contains("900000"));
    assert!(serialized.contains("33554432"));
}

#[tokio::test]
async fn durable_piece_batch_publishes_one_coherent_patch() {
    let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
    let hub = ViewHub::new(&snapshot(0, 4)).expect("hub");
    let subscription = hub.subscribe(piece_spec(4096)).expect("subscribe");
    subscription.next_update().await.expect("snapshot");

    hub.record_pieces_durable(torrent_id, &[3, 1, 1], 1)
        .expect("record durable batch");
    let update = subscription.next_update().await.expect("batch patch");
    assert_eq!(update.revision, "1");
    let ViewUpdatePayload::Patch {
        patch:
            ViewPatch::PieceActivity {
                verified,
                cleared,
                active_upsert,
                active_removed,
                ..
            },
    } = update.payload
    else {
        panic!("expected piece batch patch");
    };
    assert_eq!(
        verified,
        vec![
            IndexRange {
                start: 1,
                end_exclusive: 2,
            },
            IndexRange {
                start: 3,
                end_exclusive: 4,
            },
        ]
    );
    assert!(cleared.is_empty());
    assert!(active_upsert.is_empty());
    assert!(active_removed.is_empty());
    assert_eq!(
        hub.inner
            .lock()
            .expect("hub lock")
            .torrents
            .get(torrent_id)
            .expect("torrent model")
            .view
            .verified_piece_count,
        2
    );
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
    assert_eq!(active_upsert[0].stage, super::ActivePieceStageView::Failed);
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

#[tokio::test]
async fn subscribers_have_independent_queues() {
    let hub = ViewHub::new(&snapshot(0, 100)).expect("hub");
    let fast = hub.subscribe(piece_spec(4096)).expect("fast");
    let slow = hub.subscribe(piece_spec(4096)).expect("slow");
    fast.next_update().await.expect("fast snapshot");
    slow.next_update().await.expect("slow snapshot");

    for piece_index in 0..20 {
        hub.record_activity(
            "000102030405060708090a0b0c0d0e0f10111213",
            TorrentActivity::PieceVerified { piece_index },
        )
        .expect("activity");
        fast.next_update().await.expect("fast patch");
    }
    assert_eq!(fast.stats().expect("fast stats").reset_count, 0);
    assert_eq!(slow.stats().expect("slow stats").reset_count, 0);
    let slow_update = slow.next_update().await.expect("coalesced slow patch");
    assert_eq!(slow_update.sequence, "2");
}

#[tokio::test]
async fn overflow_requires_explicit_resync() {
    let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
    let reference = ViewHub::new(&snapshot(0, 2_000)).expect("reference hub");
    for piece_index in (0..2_000).step_by(2) {
        reference
            .record_activity(torrent_id, TorrentActivity::PieceVerified { piece_index })
            .expect("reference activity");
    }
    let reference_subscription = reference
        .subscribe(piece_spec(4 * 1024 * 1024))
        .expect("reference subscription");
    let final_snapshot = reference_subscription
        .next_update()
        .await
        .expect("reference snapshot");
    let queue_bound = u32::try_from(
        serde_json::to_vec(&final_snapshot)
            .expect("encode reference snapshot")
            .len(),
    )
    .expect("snapshot length fits u32")
    .checked_add(1)
    .expect("small snapshot headroom fits u32")
    .max(4096);

    let hub = ViewHub::new(&snapshot(0, 2_000)).expect("hub");
    let subscription = hub
        .subscribe(piece_spec(queue_bound))
        .expect("subscription");
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        subscription.next_update(),
    )
    .await
    .expect("snapshot delivery timed out")
    .expect("snapshot");

    for piece_index in (0..2_000).step_by(2) {
        hub.record_activity(torrent_id, TorrentActivity::PieceVerified { piece_index })
            .expect("activity");
    }
    assert!(
        subscription.stats().expect("queued stats").queued_bytes > 0,
        "activity must enqueue either a patch or reset"
    );
    let update = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        subscription.next_update(),
    )
    .await
    .expect("reset delivery timed out")
    .expect("reset");
    assert_eq!(
        update.payload,
        ViewUpdatePayload::ResetRequired {
            reason: ResetReason::QueueOverflow
        }
    );
    assert_eq!(subscription.stats().expect("stats").reset_count, 1);
    subscription.resync().expect("resync");
    let replacement = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        subscription.next_update(),
    )
    .await
    .expect("replacement delivery timed out")
    .expect("replacement");
    assert!(matches!(
        replacement.payload,
        ViewUpdatePayload::Snapshot { .. }
    ));
}

#[test]
fn piece_ranges_do_not_expand_indices() {
    let mut pieces = vec![false; 70_005];
    pieces[65_536..70_000].fill(true);
    assert_eq!(
        ranges_from_pieces(&pieces),
        vec![IndexRange {
            start: 65_536,
            end_exclusive: 70_000
        }]
    );
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

#[tokio::test]
async fn diagnostics_filter_before_queue_and_report_ring_drops() {
    let hub = ViewHub::new(&snapshot(0, 1)).expect("hub");
    let filtered = hub
        .subscribe(SubscriptionSpec {
            selector: ViewSelector::TorrentList,
            projection: ViewProjection::Diagnostics,
            delivery: DeliveryPolicy {
                min_interval_millis: 0,
                max_queue_bytes: 4 * 1024 * 1024,
            },
            diagnostics: Some(DiagnosticFilter {
                profile: DiagnosticProfile::Normal,
                minimum_severity: DiagnosticSeverity::Trace,
                categories: vec![DiagnosticCategory::from_static(category::PIECE_BLOCK)],
            }),
        })
        .expect("subscribe");
    filtered.next_update().await.expect("snapshot");
    hub.record_diagnostic(
        DiagnosticSeverity::Trace,
        category::PIECE_BLOCK,
        "block_received",
        None,
        "trace",
        &[],
    )
    .expect("record trace");
    assert_eq!(filtered.stats().expect("stats").queued_bytes, 0);

    hub.record_diagnostic(
        DiagnosticSeverity::Warning,
        category::TRACKER_ANNOUNCE,
        "tracker_unavailable",
        None,
        "warning",
        &[],
    )
    .expect("record warning");
    let warning = filtered.next_update().await.expect("warning patch");
    assert!(
        serde_json::to_string(&warning)
            .expect("encode")
            .contains("tracker_unavailable")
    );

    for index in 0..MAX_DIAGNOSTIC_EVENTS + 20 {
        hub.record_diagnostic(
            DiagnosticSeverity::Warning,
            category::TRACKER_ANNOUNCE,
            "bounded",
            None,
            &format!("event {index}"),
            &[("hostile", "\u{202e}<script>")],
        )
        .expect("record bounded event");
    }
    let snapshot = hub
        .subscribe(SubscriptionSpec {
            selector: ViewSelector::TorrentList,
            projection: ViewProjection::Diagnostics,
            delivery: DeliveryPolicy {
                min_interval_millis: 0,
                max_queue_bytes: 4 * 1024 * 1024,
            },
            diagnostics: Some(DiagnosticFilter {
                profile: DiagnosticProfile::Normal,
                minimum_severity: DiagnosticSeverity::Info,
                categories: Vec::new(),
            }),
        })
        .expect("bounded subscription")
        .next_update()
        .await
        .expect("bounded snapshot");
    let ViewUpdatePayload::Snapshot {
        snapshot: ViewSnapshot::Diagnostics { events, retention },
    } = snapshot.payload
    else {
        panic!("expected diagnostic snapshot");
    };
    assert_eq!(events.len(), MAX_DIAGNOSTIC_EVENTS);
    assert_ne!(retention.source_evicted_count, "0");
    assert!(
        events
            .iter()
            .all(|event| !event.message.contains('\u{202e}'))
    );
    assert!(
        events
            .iter()
            .flat_map(|event| &event.fields)
            .all(|field| match &field.value {
                DiagnosticValue::Text { value }
                | DiagnosticValue::Endpoint { value }
                | DiagnosticValue::ErrorCode { value }
                | DiagnosticValue::Count { value }
                | DiagnosticValue::Bytes { value }
                | DiagnosticValue::DurationMillis { value } => !value.contains('\u{202e}'),
                DiagnosticValue::Boolean { .. } => true,
            })
    );
}

#[test]
fn diagnostic_patch_coalescing_respects_count_and_byte_bounds() {
    let event = DiagnosticEvent {
        sequence: "1".to_owned(),
        timestamp_millis: "1".to_owned(),
        severity: DiagnosticSeverity::Trace,
        category: DiagnosticCategory::from_static(category::PIECE_BLOCK),
        code: "block_received".to_owned(),
        torrent_id: None,
        message: "received".to_owned(),
        subjects: Vec::new(),
        fields: Vec::new(),
    };
    let retention = DiagnosticRetention {
        source_evicted_count: "0".to_owned(),
        retained_from_sequence: "1".to_owned(),
    };
    let mut count_bounded = ViewPatch::Diagnostics {
        events: vec![event.clone(); MAX_DIAGNOSTIC_PATCH_EVENTS],
        retention: retention.clone(),
    };
    let next = ViewPatch::Diagnostics {
        events: vec![event.clone()],
        retention: retention.clone(),
    };
    assert!(!super::coalesce_patch(&mut count_bounded, &next));

    let mut large = event;
    large.message = "x".repeat(3_000);
    let mut byte_bounded = ViewPatch::Diagnostics {
        events: vec![large.clone(); 40],
        retention: retention.clone(),
    };
    let next = ViewPatch::Diagnostics {
        events: vec![large; 10],
        retention,
    };
    assert!(!super::coalesce_patch(&mut byte_bounded, &next));
}
