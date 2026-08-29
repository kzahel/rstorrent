//! Controlled interoperability owner for an application-backed completed seed.

#![recursion_limit = "256"]

use std::env;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use rstorrent_engine::dht::BootstrapNode;
use rstorrent_engine::{
    ContentFingerprint, IncomingPeerServiceSnapshot, SelectiveStorage, TorrentArtifactIdentity,
    TorrentId, torrent_storage_paths_for_metainfo,
};
use rstorrent_protocol::content::{TorrentContent, TorrentContentProjection};
use rstorrent_protocol::metainfo::{BEP9_METAINFO_LIMITS, Metainfo};
use rstorrent_protocol::storage_layout::{FileSelection, TorrentLayout};
use rstorrent_session::{
    AddTorrentBytesRequest, ApplicationConfig, ApplicationService, BandwidthRuntimeView,
    CONTROL_VERSION, ClientSettings, Command, CommandResult, ConfiguredStorageRoot, DeliveryPolicy,
    EncryptionPolicy, FileIndexRange, FileSelectionIntent, Ipv6PinholeDiagnosticResult,
    Ipv6PinholeStatus, ListenerPolicy, NetworkConfig, NetworkPolicy, PeerTransportPolicy,
    PortMappingPolicy, PortMappingStatus, RequestEnvelope, ResponseOutcome, SessionStore,
    SessionUdpStatus, StoreError, SubscriptionSpec, TorrentTransferLimits, TransferRateLimit,
    TransportAddressFamily, ViewProjection, ViewSelector, ViewSnapshot, ViewUpdatePayload,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::time::timeout;

const READY_TIMEOUT: Duration = Duration::from_secs(30);
const PROFILE_ID: &str = "default";

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("incoming seed failed: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), SeedHarnessError> {
    let arguments = Arguments::parse(env::args_os().skip(1))?;
    let outer = std::fs::read(&arguments.metainfo).map_err(|source| SeedHarnessError::Io {
        operation: "read metainfo",
        source,
    })?;
    if outer.len() > BEP9_METAINFO_LIMITS.max_outer_bytes {
        return Err(SeedHarnessError::Arguments(format!(
            "metainfo exceeds {} bytes",
            BEP9_METAINFO_LIMITS.max_outer_bytes
        )));
    }
    let projection = TorrentContentProjection::from_bytes_with_limits(&outer, BEP9_METAINFO_LIMITS)
        .map_err(|error| SeedHarnessError::Metainfo(error.to_string()))?;
    let content = &projection.content;
    let raw_info = outer[projection.info_span.clone()].to_vec();
    std::fs::create_dir_all(&arguments.storage_root).map_err(|source| SeedHarnessError::Io {
        operation: "create storage root",
        source,
    })?;
    let storage_roots = vec![ConfiguredStorageRoot::path(
        "downloads",
        arguments.storage_root.clone(),
    )];
    let torrent_id = initialize_catalog(
        &arguments.profile_root,
        &storage_roots,
        content,
        &outer,
        &raw_info,
        &arguments,
    )?;
    if let Some(payload) = &arguments.fixture_payload {
        let metainfo = content.v1().ok_or_else(|| {
            SeedHarnessError::Arguments(
                "--fixture-payload is not implemented for pure-v2 fixtures".to_owned(),
            )
        })?;
        stage_partial_fixture(
            payload,
            &arguments.storage_root,
            torrent_id,
            &metainfo.metainfo,
            &raw_info,
            &arguments,
        )
        .await?;
    }

    let network_policy = if arguments.local_network_listener() {
        NetworkPolicy::Online
    } else {
        NetworkPolicy::LoopbackOnly
    };
    let mut config = ApplicationConfig::new(
        arguments.profile_root.clone(),
        PROFILE_ID.to_owned(),
        storage_roots,
        NetworkConfig::new(
            network_policy,
            Duration::from_secs(5),
            Duration::from_secs(5),
        ),
    );
    if arguments.controlled_local_network {
        config.dht.bootstrap_nodes.clear();
    }
    if let Some(bootstrap) = arguments.dht_bootstrap {
        config.dht.bootstrap_nodes = vec![BootstrapNode::Address(bootstrap)];
    }
    if arguments.utp {
        config.peer_transport_policy = PeerTransportPolicy::PreferUtp;
    } else if arguments.tcp_only {
        config.peer_transport_policy = PeerTransportPolicy::TcpOnly;
    }
    let mut service = ApplicationService::open(config).await?;
    let expected_registrations = content.info_hashes().identity_count();
    let ready = timeout(READY_TIMEOUT, async {
        loop {
            if let Some(snapshot) = service.incoming_peer_snapshot()
                && snapshot.registrations == expected_registrations
            {
                break snapshot;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| SeedHarnessError::ReadinessTimeout)?;
    let readiness = async {
        let mapping = if arguments.upnp {
            Some(wait_for_mapping(&service, MappingWait::Tcp).await?)
        } else {
            None
        };
        let udp_mapping = if arguments.await_udp_mapping {
            Some(wait_for_mapping(&service, MappingWait::Udp).await?)
        } else {
            None
        };
        let staged_ipv6 = if arguments.staged_ipv6_pinhole {
            Some(wait_for_ipv6_pinhole(&service, PinholeWait::Disabled).await?)
        } else {
            None
        };
        let utp_listen = if service.utp_snapshot().is_some() {
            Some(session_udp_endpoint(&service).await?)
        } else {
            None
        };
        let mapping_statuses = mapping_statuses(&service).await?;
        Ok::<_, SeedHarnessError>((
            mapping,
            udp_mapping,
            staged_ipv6,
            utp_listen,
            mapping_statuses,
        ))
    }
    .await;
    let (mapping, udp_mapping, staged_ipv6, utp_listen, mapping_statuses) = match readiness {
        Ok(readiness) => readiness,
        Err(readiness_error) => {
            service.shutdown().await?;
            return Err(readiness_error);
        }
    };
    let (tcp_mapping_status, udp_mapping_status) = mapping_statuses;
    let ready_json = serde_json::json!({
        "event": if arguments.staged_ipv6_pinhole { "pre_pinhole" } else { "ready" },
        "torrent_id": torrent_id.to_string(),
        "protocol": match content {
            TorrentContent::V1(_) => "v1",
            TorrentContent::V2(_) => "v2",
            TorrentContent::Hybrid(_) => "hybrid",
        },
        "info_hash": hex(content.swarm_key().into_bytes()),
        "full_info_hash": full_info_hash(content),
        "listen": ready.listen_address.to_string(),
        "registrations": ready.registrations,
        "pending_high_water": ready.pending_high_water,
        "established_high_water": ready.established_high_water,
        "connection_high_water": ready.peer_budget.total_high_water,
        "upload_slots_high_water": ready.upload_slots_high_water,
        "queued_requests_high_water": ready.queued_requests_high_water,
        "queued_bytes_high_water": ready.queued_bytes_high_water,
        "read_high_water": ready.read_high_water,
        "read_bytes_high_water": ready.read_bytes_high_water,
        "payload_bytes_sent": ready.payload_bytes_sent,
        "mapping": mapping,
        "udp_mapping": udp_mapping,
        "tcp_mapping_status": tcp_mapping_status,
        "udp_mapping_status": udp_mapping_status,
        "utp_listen": utp_listen,
        "udp": service.session_udp_snapshot().map(session_udp_snapshot_json),
        "ipv6_listener": staged_ipv6.as_ref().map(|(_, endpoint)| endpoint.to_string()),
        "ipv6_pinhole": staged_ipv6.as_ref().map(|(status, _)| status),
    });
    let mut stdout = tokio::io::stdout();
    stdout
        .write_all(format!("{ready_json}\n").as_bytes())
        .await
        .map_err(|source| SeedHarnessError::Io {
            operation: "write ready observation",
            source,
        })?;
    stdout
        .flush()
        .await
        .map_err(|source| SeedHarnessError::Io {
            operation: "flush ready observation",
            source,
        })?;

    let mut command = String::new();
    let mut stdin = BufReader::new(tokio::io::stdin());
    loop {
        command.clear();
        let read = stdin
            .read_line(&mut command)
            .await
            .map_err(|source| SeedHarnessError::Io {
                operation: "read seed harness command",
                source,
            })?;
        if read == 0 || command.trim().is_empty() || command.trim() == "stop" {
            break;
        }
        if command.trim() == "enable-pinhole" && arguments.staged_ipv6_pinhole {
            apply_port_mapping(&mut service, &arguments, PortMappingPolicy::Upnp).await?;
            let (status, endpoint) = wait_for_ipv6_pinhole(&service, PinholeWait::Settled).await?;
            let event = if matches!(status, Ipv6PinholeStatus::Pinholed { .. }) {
                "pinholed"
            } else {
                "pinhole_terminal"
            };
            write_observation(
                &mut stdout,
                serde_json::json!({
                    "event": event,
                    "ipv6_listener": endpoint.to_string(),
                    "ipv6_pinhole": status,
                }),
            )
            .await?;
            continue;
        }
        if command.trim() == "pinhole-packets" && arguments.staged_ipv6_pinhole {
            let result = service
                .ipv6_pinhole_packets_for_diagnostics(false)
                .await
                .ok_or_else(|| {
                    SeedHarnessError::Catalog(
                        "active IPv6 pinhole diagnostic is unavailable".to_owned(),
                    )
                })?;
            write_observation(
                &mut stdout,
                pinhole_diagnostic_json("pinhole_packets", result),
            )
            .await?;
            continue;
        }
        if command.trim() == "disable-pinhole" && arguments.staged_ipv6_pinhole {
            apply_port_mapping(&mut service, &arguments, PortMappingPolicy::Disabled).await?;
            let (status, endpoint) = wait_for_ipv6_pinhole(&service, PinholeWait::Disabled).await?;
            write_observation(
                &mut stdout,
                serde_json::json!({
                    "event": "pinhole_disabled",
                    "ipv6_listener": endpoint.to_string(),
                    "ipv6_pinhole": status,
                }),
            )
            .await?;
            continue;
        }
        if command.trim() == "deleted-pinhole-packets" && arguments.staged_ipv6_pinhole {
            let result = service
                .ipv6_pinhole_packets_for_diagnostics(true)
                .await
                .ok_or_else(|| {
                    SeedHarnessError::Catalog(
                        "deleted IPv6 pinhole diagnostic is unavailable".to_owned(),
                    )
                })?;
            write_observation(
                &mut stdout,
                pinhole_diagnostic_json("deleted_pinhole_packets", result),
            )
            .await?;
            continue;
        }
        if command.trim() != "snapshot" {
            return Err(SeedHarnessError::Arguments(format!(
                "unknown seed harness command {}",
                command.trim()
            )));
        }
        let snapshot = service
            .incoming_peer_snapshot()
            .expect("enabled incoming service remains owned before shutdown");
        let bandwidth: BandwidthRuntimeView = service.bandwidth_snapshot().into();
        let snapshot_json = serde_json::json!({
            "event": "snapshot",
            "pending": snapshot.pending,
            "established": snapshot.established,
            "pending_high_water": snapshot.pending_high_water,
            "established_high_water": snapshot.established_high_water,
            "connection_high_water": snapshot.peer_budget.total_high_water,
            "upload_slots_high_water": snapshot.upload_slots_high_water,
            "queued_requests_high_water": snapshot.queued_requests_high_water,
            "read_high_water": snapshot.read_high_water,
            "payload_bytes_sent": snapshot.payload_bytes_sent,
            "rejection_counts": incoming_rejection_counts_json(&snapshot),
            "bandwidth": bandwidth,
            "utp": service.utp_snapshot().map(utp_snapshot_json),
            "peers": snapshot_view(&service, ViewSelector::Torrent {
                torrent_id: torrent_id.to_string(),
            }, ViewProjection::Peers).await?,
            "swarm": snapshot_view(&service, ViewSelector::Torrent {
                torrent_id: torrent_id.to_string(),
            }, ViewProjection::Swarm).await?,
            "summary": snapshot_view(&service, ViewSelector::Torrent {
                torrent_id: torrent_id.to_string(),
            }, ViewProjection::Summary).await?,
        });
        stdout
            .write_all(format!("{snapshot_json}\n").as_bytes())
            .await
            .map_err(|source| SeedHarnessError::Io {
                operation: "write live seed observation",
                source,
            })?;
        stdout
            .flush()
            .await
            .map_err(|source| SeedHarnessError::Io {
                operation: "flush live seed observation",
                source,
            })?;
    }
    let final_snapshot = service
        .incoming_peer_snapshot()
        .expect("enabled incoming service remains owned before shutdown");
    let final_utp = service.utp_snapshot().map(utp_snapshot_json);
    let final_udp = service
        .session_udp_snapshot()
        .map(session_udp_snapshot_json);
    let final_bandwidth: BandwidthRuntimeView = service.bandwidth_snapshot().into();
    service.shutdown().await?;
    let stopped_json = serde_json::json!({
        "event": "stopped",
        "pending_before_shutdown": final_snapshot.pending,
        "established_before_shutdown": final_snapshot.established,
        "reads_before_shutdown": final_snapshot.reads,
        "payload_bytes_sent": final_snapshot.payload_bytes_sent,
        "bandwidth": final_bandwidth,
        "utp_before_shutdown": final_utp,
        "udp_before_shutdown": final_udp,
        "pending_high_water": final_snapshot.pending_high_water,
        "established_high_water": final_snapshot.established_high_water,
        "connection_high_water": final_snapshot.peer_budget.total_high_water,
        "upload_regular_high_water": final_snapshot.upload_regular_high_water,
        "upload_optimistic_high_water": final_snapshot.upload_optimistic_high_water,
        "upload_slots_high_water": final_snapshot.upload_slots_high_water,
        "queued_requests_high_water": final_snapshot.queued_requests_high_water,
        "queued_bytes_high_water": final_snapshot.queued_bytes_high_water,
        "read_high_water": final_snapshot.read_high_water,
        "read_bytes_high_water": final_snapshot.read_bytes_high_water,
        "writer_send_buffer_high_water": final_snapshot.writer_send_buffer_high_water,
        "rejection_counts": incoming_rejection_counts_json(&final_snapshot),
        "mapping_tasks_after_shutdown": 0,
        "mappings_after_shutdown": 0,
        "pinholes_after_shutdown": 0,
    });
    stdout
        .write_all(format!("{stopped_json}\n").as_bytes())
        .await
        .map_err(|source| SeedHarnessError::Io {
            operation: "write stopped observation",
            source,
        })?;
    stdout
        .flush()
        .await
        .map_err(|source| SeedHarnessError::Io {
            operation: "flush stopped observation",
            source,
        })?;
    Ok(())
}

fn initialize_catalog(
    profile_root: &std::path::Path,
    storage_roots: &[ConfiguredStorageRoot],
    content: &TorrentContent,
    metainfo_source: &[u8],
    raw_info: &[u8],
    arguments: &Arguments,
) -> Result<TorrentId, SeedHarnessError> {
    let mut store = SessionStore::open(profile_root, PROFILE_ID, storage_roots)?;
    let desired_settings = ClientSettings {
        listener: if arguments.local_network_listener() {
            ListenerPolicy::AutomaticLocalNetwork
        } else {
            ListenerPolicy::AutomaticLoopback
        },
        port_mapping: if arguments.upnp {
            PortMappingPolicy::Upnp
        } else {
            PortMappingPolicy::Disabled
        },
        encryption: arguments.encryption,
        upload_rate_limit: rate_limit(arguments.upload_rate_limit),
        download_rate_limit: rate_limit(arguments.download_rate_limit),
        ..ClientSettings::default()
    };
    if store.client_settings()? != desired_settings {
        let settings = store.handle_durable(&RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: format!("configure-incoming-seed-{}", store.revision()?),
            expected_revision: None,
            command: Command::UpdateClientSettings {
                patch: desired_settings.into(),
            },
        })?;
        if !matches!(settings.outcome, ResponseOutcome::Success { .. }) {
            return Err(SeedHarnessError::Catalog(
                "fixture client settings request was rejected".to_owned(),
            ));
        }
    }
    let partial = arguments.fixture_payload.is_some();
    let downloading = arguments.download_fixture;
    let snapshot = store.snapshot()?;
    let mut existing = None;
    for torrent in &snapshot.torrents {
        let resume = store.load_resume(&torrent.torrent_id)?;
        if resume.info_hashes == content.info_hashes() {
            existing = Some(resume);
            break;
        }
    }
    if let Some(resume) = existing {
        let torrent_id = resume.torrent_id.to_string();
        if (downloading && resume.state != rstorrent_session::TorrentState::Complete)
            || (!partial
                && !downloading
                && resume.state == rstorrent_session::TorrentState::Complete)
            || (partial && resume.state != rstorrent_session::TorrentState::Complete)
        {
            ensure_transfer_limits(&mut store, &torrent_id, arguments)?;
            return Ok(resume.torrent_id);
        }
        return Err(SeedHarnessError::Catalog(
            "existing fixture catalog row does not match the requested mode".to_owned(),
        ));
    }
    if !snapshot.torrents.is_empty() {
        return Err(SeedHarnessError::Catalog(
            "fixture profile belongs to a different torrent".to_owned(),
        ));
    }
    let response = if let Some(v1) = content.v1() {
        let info_hash = hex(v1.metainfo.info_hash);
        let mut magnet = arguments.tracker.as_deref().map_or_else(
            || format!("magnet:?xt=urn:btih:{info_hash}"),
            |tracker| format!("magnet:?xt=urn:btih:{info_hash}&tr={tracker}"),
        );
        if let Some(peer) = arguments.peer {
            magnet.push_str("&x.pe=");
            magnet.push_str(&peer.to_string());
        }
        store.handle_durable(&RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: "initialize-incoming-seed".to_owned(),
            expected_revision: None,
            command: Command::AddMagnet {
                magnet,
                storage_root: "downloads".to_owned(),
                start_content: true,
                skip_files: arguments
                    .skip_files
                    .iter()
                    .map(|index| u32::try_from(*index))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| {
                        SeedHarnessError::Arguments("--skip-file exceeds u32".to_owned())
                    })?,
            },
        })?
    } else {
        if arguments.tracker.is_some() || arguments.peer.is_some() {
            return Err(SeedHarnessError::Arguments(
                "pure-v2 seed fixtures carry discovery in their metainfo source".to_owned(),
            ));
        }
        let source_length = u32::try_from(metainfo_source.len()).map_err(|_| {
            SeedHarnessError::Arguments("metainfo source length exceeds u32".to_owned())
        })?;
        store.handle_torrent_bytes(
            &AddTorrentBytesRequest {
                version: CONTROL_VERSION,
                request_id: "initialize-incoming-seed".to_owned(),
                expected_revision: None,
                storage_root: "downloads".to_owned(),
                start_content: true,
                selection: selection_intent(content.files().len(), &arguments.skip_files)?,
                source_length,
            },
            metainfo_source.to_vec(),
        )?
    };
    if !matches!(response.outcome, ResponseOutcome::Success { .. }) {
        return Err(SeedHarnessError::Catalog(
            "fixture add request was rejected".to_owned(),
        ));
    }
    let torrent_id = match response.result {
        Some(CommandResult::AddTorrent { result }) => result.torrent_id,
        _ => {
            return Err(SeedHarnessError::Catalog(
                "fixture add response omitted its torrent owner".to_owned(),
            ));
        }
    };
    if content.v1().is_some() {
        store.record_metadata(&torrent_id, raw_info)?;
    }
    if downloading {
        // Exact byte intake already owns metadata, selection, desired-running
        // state, and the empty have set. The application runtime performs the
        // ordinary discovery, storage, and checkpoint path.
    } else if partial {
        store.record_pieces(&torrent_id, &arguments.initial_pieces)?;
    } else {
        store.record_pieces(&torrent_id, &(0..content.piece_count()).collect::<Vec<_>>())?;
        store.mark_complete(&torrent_id)?;
    }
    ensure_transfer_limits(&mut store, &torrent_id, arguments)?;
    torrent_id.parse().map_err(|_| {
        SeedHarnessError::Catalog("fixture add returned an invalid torrent owner".to_owned())
    })
}

fn full_info_hash(content: &TorrentContent) -> String {
    let info_hashes = content.info_hashes();
    match content {
        TorrentContent::V1(_) => info_hashes
            .v1_hash()
            .expect("v1 content has a v1 identity")
            .to_string(),
        TorrentContent::V2(_) => info_hashes
            .v2_hash()
            .expect("pure-v2 content has a v2 identity")
            .to_string(),
        TorrentContent::Hybrid(_) => format!(
            "{}+{}",
            info_hashes
                .v1_hash()
                .expect("hybrid content has a v1 identity"),
            info_hashes
                .v2_hash()
                .expect("hybrid content has a v2 identity")
        ),
    }
}

fn selection_intent(
    file_count: usize,
    skipped: &[usize],
) -> Result<FileSelectionIntent, SeedHarnessError> {
    if skipped.is_empty() {
        return Ok(FileSelectionIntent::All);
    }
    if skipped.iter().any(|index| *index >= file_count) {
        return Err(SeedHarnessError::Arguments(
            "--skip-file exceeds the metainfo file count".to_owned(),
        ));
    }
    let mut ranges = Vec::new();
    let mut start = None;
    for index in 0..=file_count {
        let wanted = index < file_count && !skipped.contains(&index);
        match (start, wanted) {
            (None, true) => start = Some(index),
            (Some(range_start), false) => {
                ranges.push(FileIndexRange {
                    start: u32::try_from(range_start).map_err(|_| {
                        SeedHarnessError::Arguments("file index exceeds u32".to_owned())
                    })?,
                    end_exclusive: u32::try_from(index).map_err(|_| {
                        SeedHarnessError::Arguments("file index exceeds u32".to_owned())
                    })?,
                });
                start = None;
            }
            _ => {}
        }
    }
    Ok(FileSelectionIntent::WantedRanges { ranges })
}

fn rate_limit(bytes_per_second: Option<u32>) -> TransferRateLimit {
    bytes_per_second.map_or(TransferRateLimit::Unlimited, |bytes_per_second| {
        TransferRateLimit::Limited { bytes_per_second }
    })
}

fn ensure_transfer_limits(
    store: &mut SessionStore,
    torrent_id: &str,
    arguments: &Arguments,
) -> Result<(), SeedHarnessError> {
    let limits = TorrentTransferLimits {
        upload: rate_limit(arguments.torrent_upload_rate_limit),
        download: rate_limit(arguments.torrent_download_rate_limit),
    };
    let response = store.handle_durable(&RequestEnvelope {
        version: CONTROL_VERSION,
        request_id: format!("configure-incoming-seed-rates-{}", store.revision()?),
        expected_revision: None,
        command: Command::UpdateTorrentSettings {
            torrent_id: torrent_id.to_owned(),
            patch: limits.into(),
        },
    })?;
    if matches!(response.outcome, ResponseOutcome::Success { .. }) {
        Ok(())
    } else {
        Err(SeedHarnessError::Catalog(
            "fixture torrent rate-limit request was rejected".to_owned(),
        ))
    }
}

async fn stage_partial_fixture(
    payload_path: &std::path::Path,
    storage_root: &std::path::Path,
    torrent_id: TorrentId,
    metainfo: &Metainfo,
    raw_info: &[u8],
    arguments: &Arguments,
) -> Result<(), SeedHarnessError> {
    if arguments.initial_pieces.is_empty() {
        return Err(SeedHarnessError::Arguments(
            "--fixture-payload requires at least one --initial-piece".to_owned(),
        ));
    }
    let payload = std::fs::read(payload_path).map_err(|source| SeedHarnessError::Io {
        operation: "read partial fixture payload",
        source,
    })?;
    let total_length = usize::try_from(metainfo.total_length).map_err(|_| {
        SeedHarnessError::Catalog("fixture payload length exceeds this platform".to_owned())
    })?;
    if payload.len() != total_length {
        return Err(SeedHarnessError::Catalog(format!(
            "fixture payload has {} bytes, expected {total_length}",
            payload.len()
        )));
    }
    if arguments
        .initial_pieces
        .iter()
        .any(|piece| *piece >= metainfo.piece_count())
    {
        return Err(SeedHarnessError::Arguments(
            "--initial-piece exceeds the metainfo piece count".to_owned(),
        ));
    }
    let layout = TorrentLayout::from_metainfo(metainfo);
    let selection = FileSelection::new(&layout, &arguments.skip_files)
        .map_err(|error| SeedHarnessError::Catalog(error.to_string()))?;
    let paths = torrent_storage_paths_for_metainfo(storage_root, metainfo, torrent_id)
        .map_err(|error| SeedHarnessError::Catalog(error.to_string()))?;
    let artifact_identity = TorrentArtifactIdentity {
        torrent_id,
        content_fingerprint: ContentFingerprint::for_info_bytes(raw_info),
    };
    let mut storage = SelectiveStorage::create(
        paths.content.clone(),
        artifact_identity,
        metainfo,
        layout.clone(),
        selection.clone(),
    )
    .await
    .map_err(|error| SeedHarnessError::Catalog(error.to_string()))?;
    let piece_length = usize::try_from(metainfo.piece_length)
        .map_err(|_| SeedHarnessError::Catalog("piece length exceeds this platform".to_owned()))?;
    for &piece_index in &arguments.initial_pieces {
        let piece = u32::try_from(piece_index)
            .map_err(|_| SeedHarnessError::Catalog("piece index exceeds u32".to_owned()))?;
        let piece_offset = piece_index
            .checked_mul(piece_length)
            .ok_or_else(|| SeedHarnessError::Catalog("fixture piece offset overflow".to_owned()))?;
        for request in layout
            .request_ranges(piece, &selection)
            .map_err(|error| SeedHarnessError::Catalog(error.to_string()))?
        {
            let begin = usize::try_from(request.begin).map_err(|_| {
                SeedHarnessError::Catalog("fixture request offset exceeds this platform".to_owned())
            })?;
            let length = usize::try_from(request.length).map_err(|_| {
                SeedHarnessError::Catalog("fixture request length exceeds this platform".to_owned())
            })?;
            let start = piece_offset.checked_add(begin).ok_or_else(|| {
                SeedHarnessError::Catalog("fixture request offset overflow".to_owned())
            })?;
            let end = start.checked_add(length).ok_or_else(|| {
                SeedHarnessError::Catalog("fixture request length overflow".to_owned())
            })?;
            let bytes = payload.get(start..end).ok_or_else(|| {
                SeedHarnessError::Catalog("fixture request exceeds payload".to_owned())
            })?;
            storage
                .write_block(piece, request.begin, bytes.to_vec())
                .await
                .map_err(|error| SeedHarnessError::Catalog(error.to_string()))?;
        }
        storage
            .sync_piece(piece)
            .await
            .map_err(|error| SeedHarnessError::Catalog(error.to_string()))?;
        let actual = storage
            .hash_piece(piece)
            .await
            .map_err(|error| SeedHarnessError::Catalog(error.to_string()))?;
        if actual != metainfo.piece_hashes[piece_index] {
            return Err(SeedHarnessError::Catalog(format!(
                "fixture piece {piece_index} does not match metainfo"
            )));
        }
    }
    Ok(())
}

#[derive(Debug)]
struct Arguments {
    profile_root: PathBuf,
    storage_root: PathBuf,
    metainfo: PathBuf,
    upnp: bool,
    await_udp_mapping: bool,
    staged_ipv6_pinhole: bool,
    controlled_local_network: bool,
    utp: bool,
    tcp_only: bool,
    encryption: EncryptionPolicy,
    tracker: Option<String>,
    dht_bootstrap: Option<std::net::SocketAddr>,
    fixture_payload: Option<PathBuf>,
    download_fixture: bool,
    initial_pieces: Vec<usize>,
    skip_files: Vec<usize>,
    peer: Option<std::net::SocketAddr>,
    upload_rate_limit: Option<u32>,
    download_rate_limit: Option<u32>,
    torrent_upload_rate_limit: Option<u32>,
    torrent_download_rate_limit: Option<u32>,
}

impl Arguments {
    fn parse(
        arguments: impl Iterator<Item = std::ffi::OsString>,
    ) -> Result<Self, SeedHarnessError> {
        let arguments = arguments.collect::<Vec<_>>();
        let mut profile_root = None;
        let mut storage_root = None;
        let mut metainfo = None;
        let mut upnp = false;
        let mut await_udp_mapping = false;
        let mut staged_ipv6_pinhole = false;
        let mut controlled_local_network = false;
        let mut utp = false;
        let mut tcp_only = false;
        let mut encryption = None;
        let mut tracker = None;
        let mut dht_bootstrap = None;
        let mut fixture_payload = None;
        let mut download_fixture = false;
        let mut initial_pieces = Vec::new();
        let mut skip_files = Vec::new();
        let mut peer = None;
        let mut upload_rate_limit = None;
        let mut download_rate_limit = None;
        let mut torrent_upload_rate_limit = None;
        let mut torrent_download_rate_limit = None;
        let mut index = 0;
        while index < arguments.len() {
            let flag = arguments[index]
                .to_str()
                .ok_or_else(|| SeedHarnessError::Arguments("flag is not UTF-8".to_owned()))?;
            if flag == "--upnp" {
                if std::mem::replace(&mut upnp, true) {
                    return Err(SeedHarnessError::Arguments(
                        "--upnp may appear only once".to_owned(),
                    ));
                }
                index += 1;
                continue;
            }
            if flag == "--await-udp-mapping" {
                if std::mem::replace(&mut await_udp_mapping, true) {
                    return Err(SeedHarnessError::Arguments(
                        "--await-udp-mapping may appear only once".to_owned(),
                    ));
                }
                index += 1;
                continue;
            }
            if flag == "--staged-ipv6-pinhole" {
                if std::mem::replace(&mut staged_ipv6_pinhole, true) {
                    return Err(SeedHarnessError::Arguments(
                        "--staged-ipv6-pinhole may appear only once".to_owned(),
                    ));
                }
                index += 1;
                continue;
            }
            if flag == "--controlled-local-network" {
                if std::mem::replace(&mut controlled_local_network, true) {
                    return Err(SeedHarnessError::Arguments(
                        "--controlled-local-network may appear only once".to_owned(),
                    ));
                }
                index += 1;
                continue;
            }
            if flag == "--utp" {
                if std::mem::replace(&mut utp, true) {
                    return Err(SeedHarnessError::Arguments(
                        "--utp may appear only once".to_owned(),
                    ));
                }
                index += 1;
                continue;
            }
            if flag == "--tcp-only" {
                if std::mem::replace(&mut tcp_only, true) {
                    return Err(SeedHarnessError::Arguments(
                        "--tcp-only may appear only once".to_owned(),
                    ));
                }
                index += 1;
                continue;
            }
            if flag == "--download-fixture" {
                if std::mem::replace(&mut download_fixture, true) {
                    return Err(SeedHarnessError::Arguments(
                        "--download-fixture may appear only once".to_owned(),
                    ));
                }
                index += 1;
                continue;
            }
            let value = arguments
                .get(index + 1)
                .ok_or_else(|| SeedHarnessError::Arguments(format!("{flag} requires a value")))?;
            if flag == "--tracker" {
                let value = value.to_str().ok_or_else(|| {
                    SeedHarnessError::Arguments("--tracker must be UTF-8".to_owned())
                })?;
                if tracker.replace(value.to_owned()).is_some() {
                    return Err(SeedHarnessError::Arguments(
                        "--tracker may appear only once".to_owned(),
                    ));
                }
                index += 2;
                continue;
            }
            if flag == "--dht-bootstrap" {
                let value = value.to_str().ok_or_else(|| {
                    SeedHarnessError::Arguments("--dht-bootstrap must be UTF-8".to_owned())
                })?;
                let value = value.parse().map_err(|_| {
                    SeedHarnessError::Arguments(
                        "--dht-bootstrap must be a socket address".to_owned(),
                    )
                })?;
                if dht_bootstrap.replace(value).is_some() {
                    return Err(SeedHarnessError::Arguments(
                        "--dht-bootstrap may appear only once".to_owned(),
                    ));
                }
                index += 2;
                continue;
            }
            if flag == "--encryption" {
                let value = match value.to_str().ok_or_else(|| {
                    SeedHarnessError::Arguments("--encryption must be UTF-8".to_owned())
                })? {
                    "disabled" => EncryptionPolicy::Disabled,
                    "allow" => EncryptionPolicy::Allow,
                    "prefer" => EncryptionPolicy::Prefer,
                    "required" => EncryptionPolicy::Required,
                    _ => {
                        return Err(SeedHarnessError::Arguments(
                            "--encryption must be disabled, allow, prefer, or required".to_owned(),
                        ));
                    }
                };
                if encryption.replace(value).is_some() {
                    return Err(SeedHarnessError::Arguments(
                        "--encryption may appear only once".to_owned(),
                    ));
                }
                index += 2;
                continue;
            }
            if flag == "--initial-piece" || flag == "--skip-file" {
                let value = value
                    .to_str()
                    .ok_or_else(|| SeedHarnessError::Arguments(format!("{flag} must be UTF-8")))?;
                let value = value.parse::<usize>().map_err(|_| {
                    SeedHarnessError::Arguments(format!("{flag} must be a nonnegative integer"))
                })?;
                let values = if flag == "--initial-piece" {
                    &mut initial_pieces
                } else {
                    &mut skip_files
                };
                if values.contains(&value) {
                    return Err(SeedHarnessError::Arguments(format!(
                        "{flag} {value} may appear only once"
                    )));
                }
                values.push(value);
                index += 2;
                continue;
            }
            if flag == "--peer" {
                let value = value.to_str().ok_or_else(|| {
                    SeedHarnessError::Arguments("--peer must be UTF-8".to_owned())
                })?;
                let value = value.parse().map_err(|_| {
                    SeedHarnessError::Arguments("--peer must be a socket address".to_owned())
                })?;
                if peer.replace(value).is_some() {
                    return Err(SeedHarnessError::Arguments(
                        "--peer may appear only once".to_owned(),
                    ));
                }
                index += 2;
                continue;
            }
            let rate_target = match flag {
                "--upload-rate-limit" => Some(&mut upload_rate_limit),
                "--download-rate-limit" => Some(&mut download_rate_limit),
                "--torrent-upload-rate-limit" => Some(&mut torrent_upload_rate_limit),
                "--torrent-download-rate-limit" => Some(&mut torrent_download_rate_limit),
                _ => None,
            };
            if let Some(target) = rate_target {
                let value = value
                    .to_str()
                    .ok_or_else(|| SeedHarnessError::Arguments(format!("{flag} must be UTF-8")))?;
                let value = value.parse::<u32>().map_err(|_| {
                    SeedHarnessError::Arguments(format!(
                        "{flag} must be an integer number of bytes per second"
                    ))
                })?;
                if value < 1_024 {
                    return Err(SeedHarnessError::Arguments(format!(
                        "{flag} must be at least 1024 bytes per second"
                    )));
                }
                if target.replace(value).is_some() {
                    return Err(SeedHarnessError::Arguments(format!(
                        "{flag} may appear only once"
                    )));
                }
                index += 2;
                continue;
            }
            let target = match flag {
                "--profile-root" => &mut profile_root,
                "--storage-root" => &mut storage_root,
                "--metainfo" => &mut metainfo,
                "--fixture-payload" => &mut fixture_payload,
                _ => {
                    return Err(SeedHarnessError::Arguments(format!(
                        "unknown argument {flag}"
                    )));
                }
            };
            if target.replace(PathBuf::from(value)).is_some() {
                return Err(SeedHarnessError::Arguments(format!(
                    "{flag} may appear only once"
                )));
            }
            index += 2;
        }
        if upnp && staged_ipv6_pinhole {
            return Err(SeedHarnessError::Arguments(
                "--upnp and --staged-ipv6-pinhole are mutually exclusive".to_owned(),
            ));
        }
        if await_udp_mapping && (!upnp || !utp) {
            return Err(SeedHarnessError::Arguments(
                "--await-udp-mapping requires --upnp and --utp".to_owned(),
            ));
        }
        if controlled_local_network && (upnp || staged_ipv6_pinhole) {
            return Err(SeedHarnessError::Arguments(
                "--controlled-local-network cannot request a port mapping".to_owned(),
            ));
        }
        if utp && tcp_only {
            return Err(SeedHarnessError::Arguments(
                "--utp and --tcp-only are mutually exclusive".to_owned(),
            ));
        }
        if download_fixture && (fixture_payload.is_some() || !initial_pieces.is_empty()) {
            return Err(SeedHarnessError::Arguments(
                "--download-fixture cannot be combined with partial-fixture arguments".to_owned(),
            ));
        }
        Ok(Self {
            profile_root: profile_root.ok_or_else(|| {
                SeedHarnessError::Arguments("--profile-root is required".to_owned())
            })?,
            storage_root: storage_root.ok_or_else(|| {
                SeedHarnessError::Arguments("--storage-root is required".to_owned())
            })?,
            metainfo: metainfo
                .ok_or_else(|| SeedHarnessError::Arguments("--metainfo is required".to_owned()))?,
            upnp,
            await_udp_mapping,
            staged_ipv6_pinhole,
            controlled_local_network,
            utp,
            tcp_only,
            encryption: encryption.unwrap_or(EncryptionPolicy::Allow),
            tracker,
            dht_bootstrap,
            fixture_payload,
            download_fixture,
            initial_pieces,
            skip_files,
            peer,
            upload_rate_limit,
            download_rate_limit,
            torrent_upload_rate_limit,
            torrent_download_rate_limit,
        })
    }

    fn local_network_listener(&self) -> bool {
        self.upnp || self.staged_ipv6_pinhole || self.controlled_local_network
    }
}

async fn session_udp_endpoint(service: &ApplicationService) -> Result<String, SeedHarnessError> {
    let snapshot =
        snapshot_view(service, ViewSelector::TorrentList, ViewProjection::Summary).await?;
    let ViewSnapshot::TorrentList {
        client_settings, ..
    } = snapshot
    else {
        return Err(SeedHarnessError::Catalog(
            "torrent-list snapshot returned the wrong projection".to_owned(),
        ));
    };
    match client_settings.session_udp_status {
        SessionUdpStatus::Bound { address, port, .. } => Ok(format!("{address}:{port}")),
        SessionUdpStatus::Unavailable => Err(SeedHarnessError::Catalog(
            "uTP policy has no bound session UDP endpoint".to_owned(),
        )),
    }
}

fn utp_terminal_json(failure: &rstorrent_engine::UtpTerminalEvidence) -> serde_json::Value {
    serde_json::json!({
            "kind": failure.kind.as_str(),
            "detail": failure.detail,
            "new_data_datagrams_sent": failure.new_data_datagrams_sent,
            "retransmission_data_datagrams_sent": failure.retransmission_data_datagrams_sent,
            "data_datagrams_received": failure.data_datagrams_received,
            "sent_sequence_cycles": failure.sent_sequence_cycles,
            "received_sequence_cycles": failure.received_sequence_cycles,
            "last_data_sequence_sent": failure.last_data_sequence_sent,
            "last_retransmission_sequence_sent": failure.last_retransmission_sequence_sent,
            "last_data_sequence_received": failure.last_data_sequence_received,
            "loss_signals_received": failure.loss_signals_received,
            "duplicate_acknowledgements": failure.duplicate_acknowledgements,
            "stale_acknowledgements": failure.stale_acknowledgements,
            "future_acknowledgements": failure.future_acknowledgements,
            "ambiguous_acknowledgements": failure.ambiguous_acknowledgements,
            "duplicate_data_datagrams": failure.duplicate_data_datagrams,
            "too_far_ahead_data_datagrams": failure.too_far_ahead_data_datagrams,
            "ambiguous_data_datagrams": failure.ambiguous_data_datagrams,
            "fin_datagrams_received": failure.fin_datagrams_received,
            "reset_datagrams_received": failure.reset_datagrams_received,
            "outstanding_packets": failure.outstanding_packets,
            "outstanding_bytes": failure.outstanding_bytes,
            "in_flight_packets": failure.in_flight_packets,
            "in_flight_bytes": failure.in_flight_bytes,
            "pending_retransmissions": failure.pending_retransmissions,
            "congestion_window_bytes": failure.congestion_window_bytes,
            "remote_window_bytes": failure.remote_window_bytes,
            "smoothed_rtt_micros": failure.smoothed_rtt_micros,
            "effective_rto_micros": failure.effective_rto_micros,
            "consecutive_timeouts": failure.consecutive_timeouts,
            "loss_reductions": failure.loss_reductions,
            "timeout_collapses": failure.timeout_collapses,
    })
}

fn utp_snapshot_json(snapshot: rstorrent_engine::UtpServiceSnapshot) -> serde_json::Value {
    let first_terminal = snapshot.first_terminal.as_ref().map(utp_terminal_json);
    let last_failure = snapshot.last_failure.as_ref().map(utp_terminal_json);
    serde_json::json!({
        "path_mtu_profile": snapshot.path_mtu_profile.as_str(),
        "active_connections": snapshot.active_connections,
        "connections_started": snapshot.connections_started,
        "connection_high_water": snapshot.connection_high_water,
        "incoming_half_open": snapshot.incoming_half_open,
        "incoming_half_open_high_water": snapshot.incoming_half_open_high_water,
        "incoming_stream_queue_high_water": snapshot.incoming_stream_queue_high_water,
        "connection_datagram_queue_high_water": snapshot.connection_datagram_queue_high_water,
        "malformed_datagrams": snapshot.malformed_datagrams,
        "unknown_connection_datagrams": snapshot.unknown_connection_datagrams,
        "stale_generation_datagrams": snapshot.stale_generation_datagrams,
        "connection_datagrams_dropped": snapshot.connection_datagrams_dropped,
        "datagrams_sent": snapshot.datagrams_sent,
        "datagram_bytes_sent": snapshot.datagram_bytes_sent,
        "data_datagrams_sent": snapshot.data_datagrams_sent,
        "state_datagrams_sent": snapshot.state_datagrams_sent,
        "retransmission_datagrams_sent": snapshot.retransmission_datagrams_sent,
        "retransmission_bytes_sent": snapshot.retransmission_bytes_sent,
        "retransmission_queue_high_water": snapshot.retransmission_queue_high_water,
        "in_flight_packet_high_water": snapshot.in_flight_packet_high_water,
        "in_flight_byte_high_water": snapshot.in_flight_byte_high_water,
        "congestion_control_acknowledgements_high_water": snapshot.congestion_control_acknowledgements_high_water,
        "congestion_control_acknowledged_bytes_high_water": snapshot.congestion_control_acknowledged_bytes_high_water,
        "congestion_limited_acknowledgements_high_water": snapshot.congestion_limited_acknowledgements_high_water,
        "sender_underfilled_acknowledgements_high_water": snapshot.sender_underfilled_acknowledgements_high_water,
        "remote_window_limited_acknowledgements_high_water": snapshot.remote_window_limited_acknowledgements_high_water,
        "window_growth_acknowledgements_high_water": snapshot.window_growth_acknowledgements_high_water,
        "slow_start_active_observed": snapshot.slow_start_active_observed,
        "slow_start_threshold_byte_high_water": snapshot.slow_start_threshold_byte_high_water,
        "slow_start_acknowledgements_high_water": snapshot.slow_start_acknowledgements_high_water,
        "slow_start_exits_high_water": snapshot.slow_start_exits_high_water,
        "pending_ack_packet_high_water": snapshot.pending_ack_packet_high_water,
        "loss_reduction_high_water": snapshot.loss_reduction_high_water,
        "timeout_collapse_high_water": snapshot.timeout_collapse_high_water,
        "delivered_byte_high_water": snapshot.delivered_byte_high_water,
        "receive_reorder_packet_high_water": snapshot.receive_reorder_packet_high_water,
        "receive_buffered_byte_high_water": snapshot.receive_buffered_byte_high_water,
        "receive_window_drop_high_water": snapshot.receive_window_drop_high_water,
        "unsent_byte_high_water": snapshot.unsent_byte_high_water,
        "sent_byte_high_water": snapshot.sent_byte_high_water,
        "application_coalesce_byte_high_water": snapshot.application_coalesce_byte_high_water,
        "smoothed_rtt_min_micros": snapshot.smoothed_rtt_min_micros,
        "smoothed_rtt_max_micros": snapshot.smoothed_rtt_max_micros,
        "effective_rto_min_micros": snapshot.effective_rto_min_micros,
        "effective_rto_max_micros": snapshot.effective_rto_max_micros,
        "base_delay_min_micros": snapshot.base_delay_min_micros,
        "base_delay_max_micros": snapshot.base_delay_max_micros,
        "queue_delay_min_micros": snapshot.queue_delay_min_micros,
        "queue_delay_max_micros": snapshot.queue_delay_max_micros,
        "congestion_window_min_bytes": snapshot.congestion_window_min_bytes,
        "congestion_window_max_bytes": snapshot.congestion_window_max_bytes,
        "advertised_receive_window_min_bytes": snapshot.advertised_receive_window_min_bytes,
        "advertised_receive_window_max_bytes": snapshot.advertised_receive_window_max_bytes,
        "selected_mtu_min_bytes": snapshot.selected_mtu_min_bytes,
        "selected_mtu_max_bytes": snapshot.selected_mtu_max_bytes,
        "mtu_candidate_min_bytes": snapshot.mtu_candidate_min_bytes,
        "mtu_candidate_max_bytes": snapshot.mtu_candidate_max_bytes,
        "mtu_probes_started_high_water": snapshot.mtu_probes_started_high_water,
        "mtu_probes_acknowledged_high_water": snapshot.mtu_probes_acknowledged_high_water,
        "mtu_probes_failed_high_water": snapshot.mtu_probes_failed_high_water,
        "mtu_revalidations_started_high_water": snapshot.mtu_revalidations_started_high_water,
        "mtu_revalidations_acknowledged_high_water": snapshot.mtu_revalidations_acknowledged_high_water,
        "mtu_revalidations_failed_high_water": snapshot.mtu_revalidations_failed_high_water,
        "mtu_downward_recoveries_high_water": snapshot.mtu_downward_recoveries_high_water,
        "mtu_probe_datagrams_sent": snapshot.mtu_probe_datagrams_sent,
        "mtu_fragmentable_retry_datagrams_sent": snapshot.mtu_fragmentable_retry_datagrams_sent,
        "retry_exhausted_connections": snapshot.retry_exhausted_connections,
        "graceful_connections": snapshot.graceful_connections,
        "reset_connections": snapshot.reset_connections,
        "consumer_dropped_connections": snapshot.consumer_dropped_connections,
        "generation_changed_connections": snapshot.generation_changed_connections,
        "service_cancelled_connections": snapshot.service_cancelled_connections,
        "protocol_error_connections": snapshot.protocol_error_connections,
        "io_error_connections": snapshot.io_error_connections,
        "worker_panics": snapshot.worker_panics,
        "first_terminal": first_terminal,
        "last_failure": last_failure,
    })
}

fn incoming_rejection_counts_json(snapshot: &IncomingPeerServiceSnapshot) -> serde_json::Value {
    serde_json::Value::Object(
        snapshot
            .rejection_counts
            .iter()
            .map(|(reason, count)| (format!("{reason:?}"), serde_json::json!(count)))
            .collect(),
    )
}

fn session_udp_snapshot_json(snapshot: rstorrent_engine::SessionUdpSnapshot) -> serde_json::Value {
    serde_json::json!({
        "datagrams_received": snapshot.datagrams_received,
        "datagram_bytes_received": snapshot.datagram_bytes_received,
        "datagrams_dropped": snapshot.datagrams_dropped,
        "utp_datagrams_classified": snapshot.utp_datagrams_classified,
        "utp_datagram_bytes_classified": snapshot.utp_datagram_bytes_classified,
        "utp_datagrams_dropped": snapshot.utp_datagrams_dropped,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PinholeWait {
    Disabled,
    Settled,
}

async fn apply_port_mapping(
    service: &mut ApplicationService,
    arguments: &Arguments,
    port_mapping: PortMappingPolicy,
) -> Result<(), SeedHarnessError> {
    let response = service
        .dispatch(RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: format!("staged-pinhole-{}-{port_mapping:?}", service.revision()?),
            expected_revision: None,
            command: Command::UpdateClientSettings {
                patch: ClientSettings {
                    listener: ListenerPolicy::AutomaticLocalNetwork,
                    port_mapping,
                    encryption: arguments.encryption,
                    ..ClientSettings::default()
                }
                .into(),
            },
        })
        .await?;
    if matches!(response.outcome, ResponseOutcome::Success { .. }) {
        Ok(())
    } else {
        Err(SeedHarnessError::Catalog(
            "staged IPv6 pinhole settings request was rejected".to_owned(),
        ))
    }
}

async fn wait_for_ipv6_pinhole(
    service: &ApplicationService,
    target: PinholeWait,
) -> Result<(Ipv6PinholeStatus, std::net::SocketAddrV6), SeedHarnessError> {
    let subscription = service
        .subscribe(SubscriptionSpec {
            selector: ViewSelector::TorrentList,
            projection: ViewProjection::Summary,
            delivery: DeliveryPolicy {
                min_interval_millis: 0,
                max_queue_bytes: 64 * 1_024,
            },
            diagnostics: None,
            catalog_page: None,
        })
        .map_err(|error| SeedHarnessError::Catalog(error.to_string()))?;
    timeout(READY_TIMEOUT, async {
        loop {
            let update = subscription.next_update().await.ok_or_else(|| {
                SeedHarnessError::Catalog(
                    "IPv6 pinhole view subscription closed before readiness".to_owned(),
                )
            })?;
            let runtime = match update.payload {
                ViewUpdatePayload::Snapshot {
                    snapshot:
                        ViewSnapshot::TorrentList {
                            client_settings, ..
                        },
                } => Some(client_settings),
                ViewUpdatePayload::Patch {
                    patch:
                        rstorrent_session::ViewPatch::TorrentList {
                            client_settings: Some(client_settings),
                            ..
                        },
                } => Some(client_settings),
                _ => None,
            };
            let Some(runtime) = runtime else {
                continue;
            };
            let endpoint = runtime
                .transport_families
                .iter()
                .find(|family| family.family == TransportAddressFamily::Ipv6)
                .and_then(|family| family.tcp_endpoint.as_deref())
                .and_then(|endpoint| endpoint.parse::<std::net::SocketAddr>().ok())
                .and_then(|endpoint| match endpoint {
                    std::net::SocketAddr::V6(endpoint) => Some(endpoint),
                    std::net::SocketAddr::V4(_) => None,
                });
            let Some(endpoint) = endpoint else {
                continue;
            };
            let status = runtime.ipv6_pinhole_status;
            let ready = match (&status, target) {
                (Ipv6PinholeStatus::Disabled, PinholeWait::Disabled) => true,
                (
                    Ipv6PinholeStatus::Pinholed {
                        internal_address,
                        internal_port,
                        ..
                    },
                    PinholeWait::Settled,
                ) => {
                    internal_address == &endpoint.ip().to_string()
                        && *internal_port == endpoint.port()
                }
                (
                    Ipv6PinholeStatus::ServiceUnavailable
                    | Ipv6PinholeStatus::ActionUnavailable { .. }
                    | Ipv6PinholeStatus::InboundPinholeDisallowed
                    | Ipv6PinholeStatus::Unfiltered { .. }
                    | Ipv6PinholeStatus::Failed { .. }
                    | Ipv6PinholeStatus::RenewalFailed { .. }
                    | Ipv6PinholeStatus::CleanupFailed { .. },
                    PinholeWait::Settled,
                ) => true,
                _ => false,
            };
            if ready {
                return Ok((status, endpoint));
            }
        }
    })
    .await
    .map_err(|_| SeedHarnessError::ReadinessTimeout)?
}

async fn write_observation(
    stdout: &mut tokio::io::Stdout,
    observation: serde_json::Value,
) -> Result<(), SeedHarnessError> {
    stdout
        .write_all(format!("{observation}\n").as_bytes())
        .await
        .map_err(|source| SeedHarnessError::Io {
            operation: "write pinhole observation",
            source,
        })?;
    stdout.flush().await.map_err(|source| SeedHarnessError::Io {
        operation: "flush pinhole observation",
        source,
    })
}

fn pinhole_diagnostic_json(event: &str, result: Ipv6PinholeDiagnosticResult) -> serde_json::Value {
    match result {
        Ipv6PinholeDiagnosticResult::Packets(packets) => {
            serde_json::json!({ "event": event, "type": "packets", "packets": packets })
        }
        Ipv6PinholeDiagnosticResult::Fault { code, .. } => {
            serde_json::json!({ "event": event, "type": "fault", "code": code })
        }
    }
}

#[derive(Clone, Copy)]
enum MappingWait {
    Tcp,
    Udp,
}

impl MappingWait {
    const fn label(self) -> &'static str {
        match self {
            Self::Tcp => "TCP",
            Self::Udp => "UDP/uTP",
        }
    }
}

async fn wait_for_mapping(
    service: &ApplicationService,
    target: MappingWait,
) -> Result<PortMappingStatus, SeedHarnessError> {
    let subscription = service
        .subscribe(SubscriptionSpec {
            selector: ViewSelector::TorrentList,
            projection: ViewProjection::Summary,
            delivery: DeliveryPolicy {
                min_interval_millis: 0,
                max_queue_bytes: 64 * 1_024,
            },
            diagnostics: None,
            catalog_page: None,
        })
        .map_err(|error| SeedHarnessError::Catalog(error.to_string()))?;
    let mut last_status = None;
    let result = timeout(READY_TIMEOUT, async {
        loop {
            let update = subscription.next_update().await.ok_or_else(|| {
                SeedHarnessError::Catalog(
                    "mapping view subscription closed before readiness".to_owned(),
                )
            })?;
            let status = match update.payload {
                ViewUpdatePayload::Snapshot {
                    snapshot:
                        ViewSnapshot::TorrentList {
                            client_settings, ..
                        },
                } => Some(match target {
                    MappingWait::Tcp => client_settings.port_mapping_status,
                    MappingWait::Udp => client_settings.udp_port_mapping_status,
                }),
                ViewUpdatePayload::Patch {
                    patch:
                        rstorrent_session::ViewPatch::TorrentList {
                            client_settings: Some(client_settings),
                            ..
                        },
                } => Some(match target {
                    MappingWait::Tcp => client_settings.port_mapping_status,
                    MappingWait::Udp => client_settings.udp_port_mapping_status,
                }),
                _ => None,
            };
            if let Some(status) = &status {
                last_status = Some(status.clone());
            }
            match status {
                Some(status @ PortMappingStatus::Mapped { .. }) => return Ok(status),
                Some(PortMappingStatus::Failed { stage, detail }) => {
                    return Err(SeedHarnessError::Catalog(format!(
                        "{} UPnP mapping failed during {stage:?}: {detail}",
                        target.label()
                    )));
                }
                Some(PortMappingStatus::RenewalFailed { detail, .. }) => {
                    return Err(SeedHarnessError::Catalog(format!(
                        "{} UPnP mapping renewal failed before readiness: {detail}",
                        target.label()
                    )));
                }
                Some(PortMappingStatus::CleanupFailed { detail, .. }) => {
                    return Err(SeedHarnessError::Catalog(format!(
                        "prior {} UPnP mapping cleanup remains uncertain: {detail}",
                        target.label()
                    )));
                }
                _ => {}
            }
        }
    })
    .await;
    match result {
        Ok(result) => result,
        Err(_) => Err(SeedHarnessError::Catalog(format!(
            "{} UPnP mapping readiness timed out; last status: {:?}",
            target.label(),
            last_status
        ))),
    }
}

async fn mapping_statuses(
    service: &ApplicationService,
) -> Result<(PortMappingStatus, PortMappingStatus), SeedHarnessError> {
    let subscription = service
        .subscribe(SubscriptionSpec {
            selector: ViewSelector::TorrentList,
            projection: ViewProjection::Summary,
            delivery: DeliveryPolicy {
                min_interval_millis: 0,
                max_queue_bytes: 64 * 1_024,
            },
            diagnostics: None,
            catalog_page: None,
        })
        .map_err(|error| SeedHarnessError::Catalog(error.to_string()))?;
    timeout(READY_TIMEOUT, async {
        loop {
            let update = subscription.next_update().await.ok_or_else(|| {
                SeedHarnessError::Catalog(
                    "mapping view subscription closed before observation".to_owned(),
                )
            })?;
            let statuses = match update.payload {
                ViewUpdatePayload::Snapshot {
                    snapshot:
                        ViewSnapshot::TorrentList {
                            client_settings, ..
                        },
                } => Some((
                    client_settings.port_mapping_status,
                    client_settings.udp_port_mapping_status,
                )),
                ViewUpdatePayload::Patch {
                    patch:
                        rstorrent_session::ViewPatch::TorrentList {
                            client_settings: Some(client_settings),
                            ..
                        },
                } => Some((
                    client_settings.port_mapping_status,
                    client_settings.udp_port_mapping_status,
                )),
                _ => None,
            };
            if let Some(statuses) = statuses {
                return Ok(statuses);
            }
        }
    })
    .await
    .map_err(|_| SeedHarnessError::ReadinessTimeout)?
}

async fn snapshot_view(
    service: &ApplicationService,
    selector: ViewSelector,
    projection: ViewProjection,
) -> Result<ViewSnapshot, SeedHarnessError> {
    let subscription = service
        .subscribe(SubscriptionSpec {
            selector,
            projection,
            delivery: DeliveryPolicy {
                min_interval_millis: 0,
                max_queue_bytes: 256 * 1_024,
            },
            diagnostics: None,
            catalog_page: None,
        })
        .map_err(|error| SeedHarnessError::Catalog(error.to_string()))?;
    let update = subscription.next_update().await.ok_or_else(|| {
        SeedHarnessError::Catalog("view subscription closed before snapshot".to_owned())
    })?;
    match update.payload {
        ViewUpdatePayload::Snapshot { snapshot } => Ok(snapshot),
        _ => Err(SeedHarnessError::Catalog(
            "view subscription did not begin with a snapshot".to_owned(),
        )),
    }
}

fn hex(bytes: [u8; 20]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Debug)]
enum SeedHarnessError {
    Arguments(String),
    Metainfo(String),
    Catalog(String),
    ReadinessTimeout,
    Io {
        operation: &'static str,
        source: std::io::Error,
    },
    Store(StoreError),
    Application(rstorrent_session::ApplicationError),
}

impl fmt::Display for SeedHarnessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Arguments(error) | Self::Metainfo(error) | Self::Catalog(error) => {
                formatter.write_str(error)
            }
            Self::ReadinessTimeout => formatter.write_str("seed readiness timed out"),
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::Store(error) => write!(formatter, "{error}"),
            Self::Application(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for SeedHarnessError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Store(error) => Some(error),
            Self::Application(error) => Some(error),
            _ => None,
        }
    }
}

impl From<StoreError> for SeedHarnessError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<rstorrent_session::ApplicationError> for SeedHarnessError {
    fn from(error: rstorrent_session::ApplicationError) -> Self {
        Self::Application(error)
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use rstorrent_session::EncryptionPolicy;

    use super::Arguments;

    #[test]
    fn requires_each_bounded_path_argument_once() {
        let parsed = Arguments::parse(
            [
                "--profile-root",
                "profile",
                "--storage-root",
                "storage",
                "--metainfo",
                "fixture.torrent",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .expect("parse harness arguments");
        assert_eq!(parsed.profile_root.to_string_lossy(), "profile");
        assert!(!parsed.upnp);
        assert_eq!(parsed.encryption, EncryptionPolicy::Allow);
        let upnp = Arguments::parse(
            [
                "--upnp",
                "--profile-root",
                "profile",
                "--storage-root",
                "storage",
                "--metainfo",
                "fixture.torrent",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .expect("parse UPnP harness arguments");
        assert!(upnp.upnp);
        assert!(!upnp.await_udp_mapping);
        let udp_mapping = Arguments::parse(
            [
                "--upnp",
                "--utp",
                "--await-udp-mapping",
                "--profile-root",
                "profile",
                "--storage-root",
                "storage",
                "--metainfo",
                "fixture.torrent",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .expect("parse UDP mapping readiness arguments");
        assert!(udp_mapping.await_udp_mapping);
        assert!(
            Arguments::parse(
                [
                    "--await-udp-mapping",
                    "--profile-root",
                    "profile",
                    "--storage-root",
                    "storage",
                    "--metainfo",
                    "fixture.torrent",
                ]
                .into_iter()
                .map(OsString::from),
            )
            .is_err()
        );
        let staged = Arguments::parse(
            [
                "--staged-ipv6-pinhole",
                "--profile-root",
                "profile",
                "--storage-root",
                "storage",
                "--metainfo",
                "fixture.torrent",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .expect("parse staged IPv6 pinhole harness");
        assert!(staged.staged_ipv6_pinhole);
        assert!(staged.local_network_listener());
        let controlled = Arguments::parse(
            [
                "--controlled-local-network",
                "--profile-root",
                "profile",
                "--storage-root",
                "storage",
                "--metainfo",
                "fixture.torrent",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .expect("parse controlled local-network harness");
        assert!(controlled.controlled_local_network);
        assert!(controlled.local_network_listener());
        assert!(
            Arguments::parse(
                [
                    "--upnp",
                    "--staged-ipv6-pinhole",
                    "--profile-root",
                    "profile",
                    "--storage-root",
                    "storage",
                    "--metainfo",
                    "fixture.torrent",
                ]
                .into_iter()
                .map(OsString::from),
            )
            .is_err()
        );
        let required = Arguments::parse(
            [
                "--profile-root",
                "profile",
                "--storage-root",
                "storage",
                "--metainfo",
                "fixture.torrent",
                "--encryption",
                "required",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .expect("parse required encryption policy");
        assert_eq!(required.encryption, EncryptionPolicy::Required);
        let partial = Arguments::parse(
            [
                "--profile-root",
                "profile",
                "--storage-root",
                "storage",
                "--metainfo",
                "fixture.torrent",
                "--utp",
                "--fixture-payload",
                "payload.bin",
                "--initial-piece",
                "0",
                "--initial-piece",
                "2",
                "--skip-file",
                "1",
                "--peer",
                "127.0.0.1:6881",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .expect("parse partial fixture arguments");
        assert!(partial.utp);
        assert!(!partial.tcp_only);
        assert_eq!(partial.initial_pieces, [0, 2]);
        assert_eq!(partial.skip_files, [1]);
        assert_eq!(
            partial.peer.expect("partial peer").to_string(),
            "127.0.0.1:6881"
        );
        assert_eq!(
            partial
                .fixture_payload
                .expect("partial fixture payload")
                .to_string_lossy(),
            "payload.bin"
        );
        assert!(
            Arguments::parse(
                [
                    "--profile-root",
                    "profile",
                    "--storage-root",
                    "storage",
                    "--metainfo",
                    "fixture.torrent",
                    "--utp",
                    "--tcp-only",
                ]
                .into_iter()
                .map(OsString::from),
            )
            .is_err()
        );
        let tcp_only = Arguments::parse(
            [
                "--profile-root",
                "profile",
                "--storage-root",
                "storage",
                "--metainfo",
                "fixture.torrent",
                "--tcp-only",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .expect("parse explicit TCP-only policy");
        assert!(tcp_only.tcp_only);
        assert!(!tcp_only.utp);
        assert!(
            Arguments::parse(
                [
                    "--profile-root",
                    "profile",
                    "--storage-root",
                    "storage",
                    "--metainfo",
                    "fixture.torrent",
                    "--initial-piece",
                    "0",
                    "--initial-piece",
                    "0",
                ]
                .into_iter()
                .map(OsString::from),
            )
            .is_err()
        );
        assert!(Arguments::parse(std::iter::empty()).is_err());
    }
}
