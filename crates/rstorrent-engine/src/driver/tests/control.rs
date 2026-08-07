use super::*;
use crate::FileSelectionUpdate;

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

#[test]
fn checker_progress_reduces_typed_outcomes_monotonically() {
    let control = DownloadControl::new();
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());

    control.checker_started(7, 3);
    assert_eq!(
        control.checker_snapshot(),
        Some(CheckerProgress {
            generation: 7,
            phase: CheckerPhase::Preparing,
            pieces_total: 3,
            pieces_processed: 0,
            pieces_matched: 0,
            pieces_absent: 0,
            pieces_mismatched: 0,
            bytes_hashed: 0,
            active_hash_jobs: 0,
            queued_hash_jobs: 3,
            elapsed_millis: 0,
            last_advance_age_millis: 0,
            oldest_active_job_age_millis: None,
        })
    );

    control.checker_hash_started(0);
    control.checker_piece_processed(0, 16, CheckerPieceOutcome::Matched);
    control.checker_piece_processed(1, 16, CheckerPieceOutcome::Absent);
    control.checker_hash_started(2);
    control.checker_piece_processed(2, 8, CheckerPieceOutcome::Mismatched);
    control.checker_set_phase(CheckerPhase::Finalizing);

    let progress = control.checker_snapshot().expect("active checker progress");
    assert_eq!(progress.generation, 7);
    assert_eq!(progress.phase, CheckerPhase::Finalizing);
    assert_eq!(progress.pieces_total, 3);
    assert_eq!(progress.pieces_processed, 3);
    assert_eq!(progress.pieces_matched, 1);
    assert_eq!(progress.pieces_absent, 1);
    assert_eq!(progress.pieces_mismatched, 1);
    assert_eq!(progress.bytes_hashed, 24);
    assert_eq!(progress.active_hash_jobs, 0);
    assert_eq!(progress.queued_hash_jobs, 0);

    control.checker_finished(6);
    assert!(control.checker_snapshot().is_some());
    control.checker_finished(7);
    assert!(control.checker_snapshot().is_none());
    assert!(
        activity
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .any(|event| matches!(
                event,
                DownloadActivityEvent::CheckerFinished { generation: 7 }
            ))
    );
}

#[test]
fn rapid_file_selection_updates_retain_only_the_latest_revision() {
    let control = DownloadControl::new();
    let mut updates = control.selection_updates();
    for revision in 1..=1_000 {
        control.update_file_selection(FileSelectionUpdate {
            revision,
            skip_files: vec![revision as usize % 3],
        });
    }

    assert!(updates.has_changed().expect("selection controller open"));
    assert_eq!(
        updates.borrow_and_update().clone(),
        Some(FileSelectionUpdate {
            revision: 1_000,
            skip_files: vec![1],
        })
    );
    assert!(!updates.has_changed().expect("selection controller open"));
}
