use super::super::*;
use super::*;
use crate::diagnostics::category;
use crate::{
    DiagnosticCategory, DiagnosticEvent, DiagnosticFilter, DiagnosticRetention, DiagnosticSeverity,
    FileCatalogState, FileSelectionView, FileView, ProgressAction, ProgressAssessment,
    ProgressDisposition, ProgressPhase, ProgressReason, ServiceSnapshot, StorageState,
    TorrentSnapshot, TorrentState, TorrentView,
};
use rstorrent_engine::peer::{PeerSource, PeerSources};
use rstorrent_engine::swarm::ConnectionId;
use rstorrent_engine::{
    PeerConnectionDirection, PeerConnectionLifecycle, PeerConnectionObservation,
    PeerConnectionRole, PeerContentActivity, PeerRequestWindowPhase, PeerTransport,
    PeerUploadActivity, PeerUploadGrant,
};

const TORRENT_ID: &str = "000102030405060708090a0b0c0d0e0f10111213";

fn torrent_view(id: &str, verified: u32) -> TorrentView {
    TorrentView {
        torrent_id: id.to_owned(),
        display_name: Some("Fixture torrent".to_owned()),
        state: TorrentState::Downloading,
        storage_state: StorageState::Staging,
        metadata_available: true,
        piece_count: 3,
        verified_piece_count: verified,
        requested_bytes: "0".to_owned(),
        received_bytes: "0".to_owned(),
        stored_bytes: "0".to_owned(),
        active_peer_connections: 0,
        configured_tracker_count: Some(2),
        payload_download_rate_bytes: "0".to_owned(),
        required_payload_bytes: None,
        remaining_payload_bytes: None,
        eta_payload_download_rate_bytes: "0".to_owned(),
        eta: TorrentEtaView::Unavailable,
        progress: ProgressAssessment {
            disposition: ProgressDisposition::Active,
            phase: ProgressPhase::Transfer,
            reason: ProgressReason::TransferringPieces,
            actions: Vec::<ProgressAction>::new(),
        },
        checking: None,
        archived: false,
        removal_state: None,
        delete_managed_data_supported: true,
        force_recheck_available: true,
        error: None,
    }
}

fn spec() -> ViewSpec {
    ViewSpec::TorrentList {
        view_id: "library".to_owned(),
        delivery: ViewDeliveryPolicy::default(),
    }
}

fn service_snapshot(revision: u64, verified: u32) -> ServiceSnapshot {
    ServiceSnapshot {
        profile_id: "test".to_owned(),
        revision: revision.to_string(),
        storage: Default::default(),
        client_settings: Default::default(),
        torrents: vec![TorrentSnapshot {
            torrent_id: TORRENT_ID.to_owned(),
            storage_root: "downloads".to_owned(),
            state: TorrentState::Downloading,
            storage_state: StorageState::Staging,
            metadata_available: true,
            piece_count: 3,
            verified_piece_count: verified,
            download_queue_position: None,
            skip_files: Vec::new(),
            selection_default: Default::default(),
            selection_exceptions: Vec::new(),
            archived: false,
            removal_state: None,
            delete_managed_data_supported: true,
            force_recheck_available: true,
            error: None,
        }],
    }
}

fn open_request(views: Vec<ViewSpec>) -> OpenViewSetRequest {
    OpenViewSetRequest {
        views,
        options: OpenViewSetOptions::default(),
    }
}

fn inner(now: Instant) -> Arc<ViewSetInner> {
    let views = BTreeMap::from([("library".to_owned(), spec())]);
    ViewSetInner::new(
        "vs_test".to_owned(),
        ViewSetOwner::trusted("owner"),
        ViewSetInitialState {
            revision: 7,
            views,
            queue_bytes_limit: DEFAULT_VIEW_SET_QUEUE_BYTES,
            snapshots: vec![ViewSetUpdate::Snapshot {
                view_id: "library".to_owned(),
                snapshot: ViewSnapshot::TorrentList {
                    torrents: vec![torrent_view("aa", 0)],
                    storage: Default::default(),
                    client_settings: Default::default(),
                },
            }],
            now,
            lease: Duration::from_millis(VIEW_SET_LEASE_MILLIS),
        },
    )
    .expect("view set")
}

#[test]
fn validates_ids_counts_and_queue_bounds() {
    let duplicate = OpenViewSetRequest {
        views: vec![spec(), spec()],
        options: OpenViewSetOptions::default(),
    };
    assert!(matches!(
        validated_open(&duplicate),
        Err(ViewSetError::DuplicateViewId(_))
    ));
    let invalid = OpenViewSetRequest {
        views: vec![ViewSpec::TorrentList {
            view_id: "bad id".to_owned(),
            delivery: ViewDeliveryPolicy::default(),
        }],
        options: OpenViewSetOptions::default(),
    };
    assert_eq!(validated_open(&invalid), Err(ViewSetError::InvalidViewId));
    let queue = OpenViewSetRequest {
        views: vec![spec()],
        options: OpenViewSetOptions {
            requested_queue_bytes: Some(1),
        },
    };
    assert!(matches!(
        validated_open(&queue),
        Err(ViewSetError::InvalidQueueBound { .. })
    ));
}

#[test]
fn maximum_file_page_is_separate_from_steady_queue_pressure() {
    let now = Instant::now();
    let files = (0..1_024_u32)
        .map(|index| FileView {
            file_id: index.to_string(),
            file_index: index,
            path: vec![format!("directory-{index:04}"), "x".repeat(128)],
            length_bytes: "16384".to_owned(),
            torrent_offset_bytes: (u64::from(index) * 16_384).to_string(),
            first_piece: Some(index),
            last_piece: Some(index),
            selection: Some(FileSelectionView::Wanted),
            padding: false,
            done_bytes: "0".to_owned(),
            verified_bytes: "0".to_owned(),
        })
        .collect::<Vec<_>>();
    let snapshot = ViewSnapshot::Files {
        torrent_id: TORRENT_ID.to_owned(),
        state: FileCatalogState::Available,
        filesystem_content_base: Some("/tmp/rstorrent/content".to_owned()),
        page: CatalogPageView {
            offset: 0,
            limit: 1_024,
            total: 4_096,
            next_offset: Some(1_024),
        },
        files,
    };
    let encoded_snapshot = serde_json::to_vec(&snapshot)
        .expect("encode snapshot")
        .len();
    assert!(encoded_snapshot > DEFAULT_VIEW_SET_QUEUE_BYTES as usize);
    assert!(encoded_snapshot < MAX_VIEW_SET_SNAPSHOT_BYTES as usize);
    let spec = ViewSpec::TorrentFiles {
        view_id: "files".to_owned(),
        torrent_id: TORRENT_ID.to_owned(),
        page: Some(CatalogPageRequest::default()),
        delivery: ViewDeliveryPolicy::default(),
    };
    let inner = ViewSetInner::new(
        "vs_files".to_owned(),
        ViewSetOwner::trusted("owner"),
        ViewSetInitialState {
            revision: 7,
            views: BTreeMap::from([("files".to_owned(), spec)]),
            queue_bytes_limit: DEFAULT_VIEW_SET_QUEUE_BYTES,
            snapshots: vec![ViewSetUpdate::Snapshot {
                view_id: "files".to_owned(),
                snapshot,
            }],
            now,
            lease: Duration::from_millis(VIEW_SET_LEASE_MILLIS),
        },
    )
    .expect("large file view set");
    inner
        .enqueue_patch(
            "files",
            ViewPatch::Files {
                torrent_id: TORRENT_ID.to_owned(),
                upsert: vec![FileView {
                    file_id: "0".to_owned(),
                    file_index: 0,
                    path: vec!["updated".to_owned()],
                    length_bytes: "16384".to_owned(),
                    torrent_offset_bytes: "0".to_owned(),
                    first_piece: Some(0),
                    last_piece: Some(0),
                    selection: Some(FileSelectionView::Wanted),
                    padding: false,
                    done_bytes: "16384".to_owned(),
                    verified_bytes: "0".to_owned(),
                }],
                removed: Vec::new(),
            },
            8,
        )
        .expect("patch behind in-flight snapshot");
    assert!(inner.state().expect("state").reset_pending.is_none());
}

#[test]
fn replays_until_acknowledged_then_emits_accumulated_patch() {
    let now = Instant::now();
    let inner = inner(now);
    let first = match inner.poll_state(0, now).expect("poll") {
        PollState::Ready(batch) => batch,
        _ => panic!("initial batch missing"),
    };
    assert_eq!(first.cursor, "1");
    let replay = match inner.poll_state(0, now).expect("poll") {
        PollState::Ready(batch) => batch,
        _ => panic!("replay missing"),
    };
    assert_eq!(replay, first);
    inner
        .enqueue_patch(
            "library",
            ViewPatch::TorrentList {
                upsert: vec![torrent_view("aa", 1)],
                removed: Vec::new(),
                storage: None,
                client_settings: None,
            },
            8,
        )
        .expect("patch");
    let next = match inner.poll_state(1, Instant::now()).expect("poll") {
        PollState::Ready(batch) => batch,
        _ => panic!("next batch missing"),
    };
    assert_eq!(next.base_cursor, "1");
    assert_eq!(next.cursor, "2");
    assert_eq!(next.durable_revision, "8");
    assert_eq!(next.updates.len(), 1);
}

#[test]
fn mismatched_cursor_requests_reset() {
    let now = Instant::now();
    let inner = inner(now);
    assert!(matches!(
        inner.poll_state(99, now).expect("poll"),
        PollState::Reset(ResetReason::CursorMismatch)
    ));
}

#[test]
fn nonzero_delivery_interval_defers_accumulated_patch_without_a_task() {
    let now = Instant::now();
    let delayed = ViewSpec::TorrentList {
        view_id: "library".to_owned(),
        delivery: ViewDeliveryPolicy {
            min_interval_millis: 1_000,
        },
    };
    let inner = ViewSetInner::new(
        "vs_delayed".to_owned(),
        ViewSetOwner::trusted("owner"),
        ViewSetInitialState {
            revision: 7,
            views: BTreeMap::from([("library".to_owned(), delayed)]),
            queue_bytes_limit: DEFAULT_VIEW_SET_QUEUE_BYTES,
            snapshots: vec![ViewSetUpdate::Snapshot {
                view_id: "library".to_owned(),
                snapshot: ViewSnapshot::TorrentList {
                    torrents: vec![torrent_view("aa", 0)],
                    storage: Default::default(),
                    client_settings: Default::default(),
                },
            }],
            now,
            lease: Duration::from_millis(VIEW_SET_LEASE_MILLIS),
        },
    )
    .expect("view set");
    assert!(matches!(
        inner.poll_state(1, now).expect("acknowledge initial"),
        PollState::Wait(None)
    ));
    inner
        .enqueue_patch(
            "library",
            ViewPatch::TorrentList {
                upsert: vec![torrent_view("aa", 1)],
                removed: Vec::new(),
                storage: None,
                client_settings: None,
            },
            8,
        )
        .expect("patch");
    let ready_at = match inner.poll_state(1, Instant::now()).expect("poll") {
        PollState::Wait(Some(ready_at)) => ready_at,
        _ => panic!("patch should wait for its delivery interval"),
    };
    assert!(matches!(
        inner.poll_state(1, ready_at).expect("poll at deadline"),
        PollState::Ready(_)
    ));
}

#[test]
fn expiry_and_close_are_observable() {
    let now = Instant::now();
    let inner = inner(now);
    assert!(!inner.is_expired(now));
    assert!(inner.is_expired(now + Duration::from_millis(VIEW_SET_LEASE_MILLIS)));
    inner.close();
    assert!(matches!(
        inner.poll_state(0, now).expect("poll"),
        PollState::Closed
    ));
}

#[tokio::test]
async fn hub_publishes_independent_replayable_batches() {
    let hub = ViewHub::new(&service_snapshot(0, 0)).expect("hub");
    let owner = ViewSetOwner::trusted("owner");
    let first = hub
        .open_view_set(owner.clone(), open_request(vec![spec()]))
        .expect("first view set");
    let second = hub
        .open_view_set(owner.clone(), open_request(vec![spec()]))
        .expect("second view set");
    let first_set = hub
        .view_set(&owner, &first.view_set_id)
        .expect("first handle");
    let second_set = hub
        .view_set(&owner, &second.view_set_id)
        .expect("second handle");

    hub.replace_durable(&service_snapshot(1, 1), &BTreeMap::new())
        .expect("replace durable state");
    let first_batch = first_set
        .next_updates(&first.initial.cursor, 0)
        .await
        .expect("first patch");
    let replay = first_set
        .next_updates(&first.initial.cursor, 0)
        .await
        .expect("replay");
    let second_batch = second_set
        .next_updates(&second.initial.cursor, 0)
        .await
        .expect("second patch");

    assert_eq!(replay, first_batch);
    assert_eq!(first_batch.durable_revision, "1");
    assert_eq!(second_batch.durable_revision, "1");
    assert_ne!(first_batch.view_set_id, second_batch.view_set_id);
    assert!(matches!(
        first_batch.updates.as_slice(),
        [ViewSetUpdate::Patch { view_id, .. }] if view_id == "library"
    ));
}

#[tokio::test]
async fn view_replacement_is_atomic_and_reports_removal() {
    let hub = ViewHub::new(&service_snapshot(0, 0)).expect("hub");
    let owner = ViewSetOwner::trusted("owner");
    let opened = hub
        .open_view_set(owner.clone(), open_request(vec![spec()]))
        .expect("view set");
    let details = ViewSpec::TorrentSummary {
        view_id: "details".to_owned(),
        torrent_id: TORRENT_ID.to_owned(),
        delivery: ViewDeliveryPolicy::default(),
    };
    hub.update_view_set(
        &owner,
        &opened.view_set_id,
        UpdateViewSetRequest {
            views: vec![details.clone()],
        },
    )
    .expect("replace views");
    let view_set = hub
        .view_set(&owner, &opened.view_set_id)
        .expect("view set handle");
    let batch = view_set
        .next_updates(&opened.initial.cursor, 0)
        .await
        .expect("replacement batch");

    assert_eq!(batch.updates.len(), 2);
    assert!(batch.updates.contains(&ViewSetUpdate::ViewRemoved {
        view_id: "library".to_owned(),
    }));
    assert!(matches!(
        batch.updates.as_slice(),
        [_, ViewSetUpdate::Snapshot { view_id, .. }] if view_id == details.view_id()
    ));
}

#[test]
fn owners_cannot_observe_or_mutate_each_others_sets() {
    let hub = ViewHub::new(&service_snapshot(0, 0)).expect("hub");
    let owner = ViewSetOwner::trusted("owner-a");
    let stranger = ViewSetOwner::trusted("owner-b");
    let opened = hub
        .open_view_set(owner, open_request(vec![spec()]))
        .expect("view set");

    assert!(matches!(
        hub.view_set(&stranger, &opened.view_set_id),
        Err(ViewSetError::UnknownViewSet)
    ));
    assert!(matches!(
        hub.close_view_set(&stranger, &opened.view_set_id),
        Err(ViewSetError::UnknownViewSet)
    ));
}

#[tokio::test]
async fn queue_overflow_rotates_epoch_and_restores_snapshots() {
    let hub = ViewHub::new(&service_snapshot(0, 0)).expect("hub");
    let owner = ViewSetOwner::trusted("owner");
    let opened = hub
        .open_view_set(
            owner.clone(),
            OpenViewSetRequest {
                views: vec![ViewSpec::Diagnostics {
                    view_id: "logs".to_owned(),
                    torrent_id: None,
                    filter: DiagnosticFilter::default(),
                    delivery: ViewDeliveryPolicy::default(),
                }],
                options: OpenViewSetOptions {
                    requested_queue_bytes: Some(MIN_VIEW_SET_QUEUE_BYTES),
                },
            },
        )
        .expect("view set");
    let view_set = hub
        .view_set(&owner, &opened.view_set_id)
        .expect("view set handle");
    view_set
        .inner
        .enqueue_patch(
            "logs",
            ViewPatch::Diagnostics {
                events: vec![DiagnosticEvent {
                    sequence: "1".to_owned(),
                    timestamp_millis: "1".to_owned(),
                    severity: DiagnosticSeverity::Info,
                    category: DiagnosticCategory::from_static(category::LIFECYCLE_TORRENT),
                    code: "oversized".to_owned(),
                    torrent_id: None,
                    message: "x".repeat(MIN_VIEW_SET_QUEUE_BYTES as usize),
                    subjects: Vec::new(),
                    fields: Vec::new(),
                }],
                retention: DiagnosticRetention {
                    source_evicted_count: "0".to_owned(),
                    retained_from_sequence: "1".to_owned(),
                },
            },
            1,
        )
        .expect("overflow is converted to reset");

    let reset = view_set
        .next_updates(&opened.initial.cursor, 0)
        .await
        .expect("reset batch");
    assert_ne!(reset.epoch, opened.initial.epoch);
    assert!(
        parse_decimal(&reset.cursor).expect("reset cursor")
            > parse_decimal(&opened.initial.cursor).expect("initial cursor")
    );
    assert!(matches!(
        reset.updates.first(),
        Some(ViewSetUpdate::ResetRequired {
            reason: ResetReason::QueueOverflow,
            ..
        })
    ));
    assert!(matches!(
        reset.updates.get(1),
        Some(ViewSetUpdate::Snapshot { view_id, .. }) if view_id == "logs"
    ));
    assert_eq!(view_set.stats().expect("stats").reset_count, 1);
}

#[test]
fn expired_sets_are_pruned_from_owner_capacity() {
    let hub = ViewHub::new(&service_snapshot(0, 0)).expect("hub");
    let owner = ViewSetOwner::trusted("owner");
    let opened = hub
        .open_view_set(owner.clone(), open_request(vec![spec()]))
        .expect("view set");
    hub.expire_view_sets_at(Instant::now() + Duration::from_millis(VIEW_SET_LEASE_MILLIS));

    assert!(matches!(
        hub.view_set(&owner, &opened.view_set_id),
        Err(ViewSetError::UnknownViewSet)
    ));
    hub.open_view_set(owner, open_request(vec![spec()]))
        .expect("expired capacity is reclaimed");
}

#[tokio::test]
async fn close_wakes_a_waiting_long_poll() {
    let hub = ViewHub::new(&service_snapshot(0, 0)).expect("hub");
    let owner = ViewSetOwner::trusted("owner");
    let opened = hub
        .open_view_set(owner.clone(), open_request(vec![spec()]))
        .expect("view set");
    let view_set = hub
        .view_set(&owner, &opened.view_set_id)
        .expect("view set handle");
    let cursor = opened.initial.cursor.clone();
    let waiting_view_set = view_set.clone();
    let waiter = tokio::spawn(async move { waiting_view_set.next_updates(&cursor, 20_000).await });
    while !view_set.inner.polling.load(Ordering::Acquire) {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        view_set.next_updates(&opened.initial.cursor, 0).await,
        Err(ViewSetError::ConsumerBusy)
    );
    hub.close_view_set(&owner, &opened.view_set_id)
        .expect("close");
    let result = tokio::time::timeout(Duration::from_secs(1), waiter)
        .await
        .expect("waiter timed out")
        .expect("waiter task");
    assert_eq!(result, Err(ViewSetError::Closed));
}

#[tokio::test]
async fn lease_reaper_closes_silent_set_and_wakes_long_poll() {
    let lease = Duration::from_millis(40);
    let hub = ViewHub::new_with_view_set_lease(&service_snapshot(0, 0), lease).expect("hub");
    let mut reaper = ViewSetLeaseReaper::start(hub.clone(), Duration::from_millis(5));
    let owner = ViewSetOwner::trusted("owner");
    let opened = hub
        .open_view_set(owner.clone(), open_request(vec![spec()]))
        .expect("view set");
    let view_set = hub
        .view_set(&owner, &opened.view_set_id)
        .expect("view set handle");
    let cursor = opened.initial.cursor.clone();
    let waiter = tokio::spawn(async move { view_set.next_updates(&cursor, 20_000).await });

    tokio::time::sleep(Duration::from_millis(20)).await;
    hub.record_diagnostic(
        DiagnosticSeverity::Info,
        category::LIFECYCLE_TORRENT,
        "producer_activity",
        None,
        "Producer publication must not renew a client lease",
        &[],
    )
    .expect("record producer activity");

    let result = tokio::time::timeout(Duration::from_millis(300), waiter)
        .await
        .expect("reaper did not wake waiter")
        .expect("waiter task");
    assert_eq!(result, Err(ViewSetError::Closed));
    assert!(matches!(
        hub.view_set(&owner, &opened.view_set_id),
        Err(ViewSetError::UnknownViewSet)
    ));
    reaper.shutdown().await.expect("join reaper");
}

#[tokio::test]
async fn peer_view_upserts_generations_and_removes_only_on_cleanup() {
    let hub = ViewHub::new(&service_snapshot(0, 0)).expect("hub");
    let owner = ViewSetOwner::trusted("owner");
    let opened = hub
        .open_view_set(
            owner.clone(),
            open_request(vec![ViewSpec::TorrentPeers {
                view_id: "peers".to_owned(),
                torrent_id: TORRENT_ID.to_owned(),
                delivery: ViewDeliveryPolicy::default(),
            }]),
        )
        .expect("view set");
    let view_set = hub
        .view_set(&owner, &opened.view_set_id)
        .expect("view set handle");
    assert!(matches!(
        opened.initial.updates.as_slice(),
        [ViewSetUpdate::Snapshot {
            snapshot: ViewSnapshot::Peers { peers, .. },
            ..
        }] if peers.is_empty()
    ));

    let mut peer = PeerConnectionObservation {
        connection_id: ConnectionId::new(7).expect("connection"),
        record_id: None,
        endpoint: "127.0.0.1:6881".parse().expect("endpoint"),
        local_endpoint: None,
        sources: PeerSources::from_source(PeerSource::Manual),
        direction: PeerConnectionDirection::Incoming,
        transport: PeerTransport::Utp,
        lifecycle: PeerConnectionLifecycle::TransportConnecting,
        role: PeerConnectionRole::Metadata,
        started_at: Duration::from_millis(5),
        lifecycle_changed_at: Duration::from_millis(5),
        peer_id: None,
        supports_extensions: Some(true),
        supports_ut_metadata: None,
        mse_method: Some(rstorrent_protocol::mse::MseMethod::PlaintextPayload),
        content: None,
        upload: None,
        close_reason: None,
    };
    hub.record_peer_connections(TORRENT_ID, Duration::from_millis(10), &[peer.clone()])
        .expect("connecting row");
    let connecting = view_set
        .next_updates(&opened.initial.cursor, 0)
        .await
        .expect("connecting patch");
    assert!(matches!(
        connecting.updates.as_slice(),
        [ViewSetUpdate::Patch {
            patch: ViewPatch::Peers { upsert, removed, .. },
            ..
        }] if upsert.len() == 1
            && upsert[0].lifecycle == crate::PeerLifecycle::TransportConnecting
            && upsert[0].client_name.is_none()
            && upsert[0].capabilities.client_name == crate::CapabilityStatus::Unavailable
            && upsert[0].mse_method == Some(crate::PeerMseMethodView::PlaintextPayload)
            && upsert[0].peer_flags == [
                crate::PeerFlagView::Incoming,
                crate::PeerFlagView::Encrypted,
                crate::PeerFlagView::ExtensionProtocol,
                crate::PeerFlagView::Utp,
            ]
            && removed.is_empty()
    ));

    peer.lifecycle = PeerConnectionLifecycle::Connected;
    peer.role = PeerConnectionRole::Content;
    peer.mse_method = Some(rstorrent_protocol::mse::MseMethod::Rc4);
    peer.lifecycle_changed_at = Duration::from_millis(12);
    peer.peer_id = Some(*b"-UT3550-abcdefghijkl");
    peer.content = Some(PeerContentActivity {
        choking: true,
        wanted_piece_count: 8,
        pending_requests: 2,
        target_requests: 4,
        queued_payload_bytes: 32 * 1_024,
        useful_payload_bytes: 0,
        observed_payload_rate: 0,
        connected_age: Duration::from_millis(3),
        last_useful_age: None,
        last_payload_age: None,
        request_timeout: Duration::from_secs(8),
        oldest_request_age: Some(Duration::from_millis(2)),
        request_window_phase: PeerRequestWindowPhase::SlowStart,
    });
    hub.record_peer_connections(TORRENT_ID, Duration::from_millis(15), &[peer.clone()])
        .expect("connected row");
    let connected = view_set
        .next_updates(&connecting.cursor, 0)
        .await
        .expect("connected patch");
    assert!(matches!(
        connected.updates.as_slice(),
        [ViewSetUpdate::Patch {
            patch: ViewPatch::Peers { upsert, removed, .. },
            ..
        }] if upsert.len() == 1
            && upsert[0].client_name.as_deref() == Some("µTorrent 3.5.5")
            && upsert[0].capabilities.client_name == crate::CapabilityStatus::Available
            && upsert[0].mse_method == Some(crate::PeerMseMethodView::Rc4)
            && upsert[0].peer_flags == [
                crate::PeerFlagView::Incoming,
                crate::PeerFlagView::Encrypted,
                crate::PeerFlagView::DownloadChoked,
                crate::PeerFlagView::ExtensionProtocol,
                crate::PeerFlagView::Utp,
            ]
            && removed.is_empty()
    ));

    peer.content = None;
    peer.local_endpoint = Some("127.0.0.1:6882".parse().expect("local endpoint"));
    peer.supports_ut_metadata = Some(true);
    peer.upload = Some(PeerUploadActivity {
        interested: true,
        grant: PeerUploadGrant::Optimistic,
        queued_requests: 3,
        queued_bytes: 4_096,
        read_active: true,
        writer_bytes: 512,
        payload_bytes: 16_384,
        payload_rate: 8_192,
    });
    hub.record_peer_connections(TORRENT_ID, Duration::from_millis(18), &[peer.clone()])
        .expect("upload row");
    let uploading = view_set
        .next_updates(&connected.cursor, 0)
        .await
        .expect("upload patch");
    assert!(matches!(
        uploading.updates.as_slice(),
        [ViewSetUpdate::Patch {
            patch: ViewPatch::Peers { upsert, removed, .. },
            ..
        }] if upsert.len() == 1
            && upsert[0].local_endpoint.as_deref() == Some("127.0.0.1:6882")
            && upsert[0].supports_ut_metadata == Some(true)
            && upsert[0].remote_interested == Some(true)
            && upsert[0].local_choking == Some(false)
            && upsert[0].payload_upload_rate_bytes.as_deref() == Some("8192")
            && upsert[0].payload_uploaded_bytes.as_deref() == Some("16384")
            && upsert[0].pending_requests == Some(3)
            && upsert[0].queued_payload_bytes.as_deref() == Some("4608")
            && upsert[0].connected_age_millis.as_deref() == Some("13")
            && upsert[0].capabilities.local_endpoint == crate::CapabilityStatus::Available
            && upsert[0].capabilities.ut_metadata == crate::CapabilityStatus::Available
            && upsert[0].capabilities.interest_directions == crate::CapabilityStatus::Available
            && upsert[0].capabilities.local_choke == crate::CapabilityStatus::Available
            && upsert[0].capabilities.upload == crate::CapabilityStatus::Available
            && upsert[0].peer_flags == [
                crate::PeerFlagView::Incoming,
                crate::PeerFlagView::Encrypted,
                crate::PeerFlagView::UploadAllowed,
                crate::PeerFlagView::ExtensionProtocol,
                crate::PeerFlagView::MetadataExtension,
                crate::PeerFlagView::Utp,
                crate::PeerFlagView::OptimisticUnchoke,
            ]
            && removed.is_empty()
    ));

    peer.lifecycle = PeerConnectionLifecycle::Disconnecting;
    peer.lifecycle_changed_at = Duration::from_millis(20);
    hub.record_peer_connections(TORRENT_ID, Duration::from_millis(25), &[peer])
        .expect("disconnecting row");
    let disconnecting = view_set
        .next_updates(&uploading.cursor, 0)
        .await
        .expect("disconnecting patch");
    assert!(matches!(
        disconnecting.updates.as_slice(),
        [ViewSetUpdate::Patch {
            patch: ViewPatch::Peers { upsert, removed, .. },
            ..
        }] if upsert.len() == 1
            && upsert[0].lifecycle == crate::PeerLifecycle::Disconnecting
            && upsert[0].peer_flags.contains(&crate::PeerFlagView::UploadAllowed)
            && removed.is_empty()
    ));

    hub.record_peer_connections(TORRENT_ID, Duration::from_millis(30), &[])
        .expect("remove row");
    let removed = view_set
        .next_updates(&disconnecting.cursor, 0)
        .await
        .expect("removal patch");
    assert!(matches!(
        removed.updates.as_slice(),
        [ViewSetUpdate::Patch {
            patch: ViewPatch::Peers { upsert, removed, .. },
            ..
        }] if upsert.is_empty() && removed == &["7"]
    ));
}

#[test]
fn sixty_active_peer_snapshot_stays_inside_default_queue_bound() {
    let hub = ViewHub::new(&service_snapshot(0, 0)).expect("hub");
    let peers = (1..=60)
        .map(|value| {
            let connected = value > 30;
            PeerConnectionObservation {
                connection_id: ConnectionId::new(value).expect("connection"),
                record_id: None,
                endpoint: format!("127.0.0.1:{}", 6_000 + value)
                    .parse()
                    .expect("endpoint"),
                local_endpoint: None,
                sources: PeerSources::from_source(PeerSource::Manual),
                direction: PeerConnectionDirection::Outgoing,
                transport: PeerTransport::Tcp,
                lifecycle: if connected {
                    PeerConnectionLifecycle::Connected
                } else {
                    PeerConnectionLifecycle::TransportConnecting
                },
                role: if connected {
                    PeerConnectionRole::Content
                } else {
                    PeerConnectionRole::Metadata
                },
                started_at: Duration::from_secs(1),
                lifecycle_changed_at: Duration::from_secs(2),
                peer_id: connected.then_some([value as u8; 20]),
                supports_extensions: connected.then_some(true),
                supports_ut_metadata: None,
                mse_method: None,
                content: connected.then_some(PeerContentActivity {
                    choking: false,
                    wanted_piece_count: 8,
                    pending_requests: 2,
                    target_requests: 4,
                    queued_payload_bytes: 32 * 1_024,
                    useful_payload_bytes: 16 * 1_024,
                    observed_payload_rate: 8 * 1_024,
                    connected_age: Duration::from_secs(3),
                    last_useful_age: Some(Duration::from_millis(20)),
                    last_payload_age: Some(Duration::from_millis(10)),
                    request_timeout: Duration::from_secs(8),
                    oldest_request_age: Some(Duration::from_millis(50)),
                    request_window_phase: PeerRequestWindowPhase::Steady,
                }),
                upload: None,
                close_reason: None,
            }
        })
        .collect::<Vec<_>>();
    let projection_started = Instant::now();
    hub.record_peer_connections(TORRENT_ID, Duration::from_secs(5), &peers)
        .expect("peer projection");
    let projection_elapsed = projection_started.elapsed();
    let owner = ViewSetOwner::trusted("pressure-owner");
    let snapshot_started = Instant::now();
    let opened = hub
        .open_view_set(
            owner.clone(),
            open_request(vec![ViewSpec::TorrentPeers {
                view_id: "peers".to_owned(),
                torrent_id: TORRENT_ID.to_owned(),
                delivery: ViewDeliveryPolicy::default(),
            }]),
        )
        .expect("peer view set");
    let snapshot_elapsed = snapshot_started.elapsed();
    let encoded_bytes = serde_json::to_vec(&opened.initial)
        .expect("encode snapshot")
        .len();
    let view_set = hub.view_set(&owner, &opened.view_set_id).expect("view set");
    let stats = view_set.stats().expect("view stats");
    assert!(matches!(
        opened.initial.updates.as_slice(),
        [ViewSetUpdate::Snapshot {
            snapshot: ViewSnapshot::Peers { peers, .. },
            ..
        }] if peers.len() == 60
    ));
    assert!(encoded_bytes < DEFAULT_VIEW_SET_QUEUE_BYTES as usize);
    assert!(stats.queue_high_water < DEFAULT_VIEW_SET_QUEUE_BYTES as usize);
    assert_eq!(stats.reset_count, 0);
    eprintln!(
        "peer_view_pressure rows=60 projection_micros={} snapshot_micros={} encoded_bytes={} queue_high_water={} resets={}",
        projection_elapsed.as_micros(),
        snapshot_elapsed.as_micros(),
        encoded_bytes,
        stats.queue_high_water,
        stats.reset_count,
    );
}
