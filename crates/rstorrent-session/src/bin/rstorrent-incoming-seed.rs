//! Controlled interoperability owner for an application-backed completed seed.

use std::env;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use rstorrent_engine::dht::BootstrapNode;
use rstorrent_protocol::metainfo::{BEP9_METAINFO_LIMITS, Metainfo};
use rstorrent_session::{
    ApplicationConfig, ApplicationService, CONTROL_VERSION, ClientSettings, Command,
    ConfiguredStorageRoot, DeliveryPolicy, EncryptionPolicy, Ipv6PinholeDiagnosticResult,
    Ipv6PinholeStatus, ListenerPolicy, NetworkConfig, NetworkPolicy, PortMappingPolicy,
    PortMappingStatus, RequestEnvelope, ResponseOutcome, SessionStore, StorageState, StoreError,
    SubscriptionSpec, TransportAddressFamily, ViewProjection, ViewSelector, ViewSnapshot,
    ViewUpdatePayload,
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
    let metainfo = Metainfo::from_bytes_with_limits(&outer, BEP9_METAINFO_LIMITS)
        .map_err(|error| SeedHarnessError::Metainfo(error.to_string()))?;
    let raw_info = Metainfo::info_bytes_with_limits(&outer, BEP9_METAINFO_LIMITS)
        .map_err(|error| SeedHarnessError::Metainfo(error.to_string()))?
        .to_vec();
    std::fs::create_dir_all(&arguments.storage_root).map_err(|source| SeedHarnessError::Io {
        operation: "create storage root",
        source,
    })?;
    let storage_roots = vec![ConfiguredStorageRoot::path(
        "downloads",
        arguments.storage_root.clone(),
    )];
    initialize_catalog(
        &arguments.profile_root,
        &storage_roots,
        &metainfo,
        &raw_info,
        &arguments,
    )?;

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
    if let Some(bootstrap) = arguments.dht_bootstrap {
        config.dht.bootstrap_nodes = vec![BootstrapNode::Address(bootstrap)];
    }
    let mut service = ApplicationService::open(config).await?;
    let ready = timeout(READY_TIMEOUT, async {
        loop {
            if let Some(snapshot) = service.incoming_peer_snapshot()
                && snapshot.registrations == 1
            {
                break snapshot;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| SeedHarnessError::ReadinessTimeout)?;
    let mapping = if arguments.upnp {
        Some(wait_for_mapping(&service).await?)
    } else {
        None
    };
    let staged_ipv6 = if arguments.staged_ipv6_pinhole {
        Some(wait_for_ipv6_pinhole(&service, PinholeWait::Disabled).await?)
    } else {
        None
    };
    let ready_json = serde_json::json!({
        "event": if arguments.staged_ipv6_pinhole { "pre_pinhole" } else { "ready" },
        "info_hash": hex(metainfo.info_hash),
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
            let (status, endpoint) = wait_for_ipv6_pinhole(&service, PinholeWait::Pinholed).await?;
            write_observation(
                &mut stdout,
                serde_json::json!({
                    "event": "pinholed",
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
            "peers": snapshot_view(&service, ViewSelector::Torrent {
                torrent_id: hex(metainfo.info_hash),
            }, ViewProjection::Peers).await?,
            "swarm": snapshot_view(&service, ViewSelector::Torrent {
                torrent_id: hex(metainfo.info_hash),
            }, ViewProjection::Swarm).await?,
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
    service.shutdown().await?;
    let stopped_json = serde_json::json!({
        "event": "stopped",
        "pending_before_shutdown": final_snapshot.pending,
        "established_before_shutdown": final_snapshot.established,
        "reads_before_shutdown": final_snapshot.reads,
        "payload_bytes_sent": final_snapshot.payload_bytes_sent,
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
    metainfo: &Metainfo,
    raw_info: &[u8],
    arguments: &Arguments,
) -> Result<(), SeedHarnessError> {
    let torrent_id = hex(metainfo.info_hash);
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
        ..ClientSettings::default()
    };
    if store.client_settings()? != desired_settings {
        let settings = store.handle_durable(&RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: format!("configure-incoming-seed-{}", store.revision()?),
            expected_revision: None,
            command: Command::SetClientSettings {
                settings: desired_settings,
            },
        })?;
        if !matches!(settings.outcome, ResponseOutcome::Success { .. }) {
            return Err(SeedHarnessError::Catalog(
                "fixture client settings request was rejected".to_owned(),
            ));
        }
    }
    match store.load_resume(&torrent_id) {
        Ok(resume)
            if resume.state == rstorrent_session::TorrentState::Complete
                && resume.storage_state == StorageState::Published =>
        {
            return Ok(());
        }
        Ok(_) => {
            return Err(SeedHarnessError::Catalog(
                "existing fixture catalog row is not complete and published".to_owned(),
            ));
        }
        Err(StoreError::UnknownTorrent(_)) => {}
        Err(error) => return Err(error.into()),
    }
    let response = store.handle_durable(&RequestEnvelope {
        version: CONTROL_VERSION,
        request_id: "initialize-incoming-seed".to_owned(),
        expected_revision: None,
        command: Command::AddMagnet {
            magnet: arguments.tracker.as_deref().map_or_else(
                || format!("magnet:?xt=urn:btih:{torrent_id}"),
                |tracker| format!("magnet:?xt=urn:btih:{torrent_id}&tr={tracker}"),
            ),
            storage_root: "downloads".to_owned(),
            start_content: true,
            skip_files: Vec::new(),
        },
    })?;
    if !matches!(response.outcome, ResponseOutcome::Success { .. }) {
        return Err(SeedHarnessError::Catalog(
            "fixture add request was rejected".to_owned(),
        ));
    }
    store.record_metadata(&torrent_id, raw_info)?;
    store.record_pieces(
        &torrent_id,
        &(0..metainfo.piece_count()).collect::<Vec<_>>(),
    )?;
    store.mark_storage_prepared(&torrent_id, StorageState::Published)?;
    store.mark_complete(&torrent_id)?;
    Ok(())
}

#[derive(Debug)]
struct Arguments {
    profile_root: PathBuf,
    storage_root: PathBuf,
    metainfo: PathBuf,
    upnp: bool,
    staged_ipv6_pinhole: bool,
    encryption: EncryptionPolicy,
    tracker: Option<String>,
    dht_bootstrap: Option<std::net::SocketAddr>,
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
        let mut staged_ipv6_pinhole = false;
        let mut encryption = None;
        let mut tracker = None;
        let mut dht_bootstrap = None;
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
            if flag == "--staged-ipv6-pinhole" {
                if std::mem::replace(&mut staged_ipv6_pinhole, true) {
                    return Err(SeedHarnessError::Arguments(
                        "--staged-ipv6-pinhole may appear only once".to_owned(),
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
            let target = match flag {
                "--profile-root" => &mut profile_root,
                "--storage-root" => &mut storage_root,
                "--metainfo" => &mut metainfo,
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
            staged_ipv6_pinhole,
            encryption: encryption.unwrap_or(EncryptionPolicy::Allow),
            tracker,
            dht_bootstrap,
        })
    }

    fn local_network_listener(&self) -> bool {
        self.upnp || self.staged_ipv6_pinhole
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PinholeWait {
    Disabled,
    Pinholed,
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
            command: Command::SetClientSettings {
                settings: ClientSettings {
                    listener: ListenerPolicy::AutomaticLocalNetwork,
                    port_mapping,
                    encryption: arguments.encryption,
                    ..ClientSettings::default()
                },
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
                    PinholeWait::Pinholed,
                ) => {
                    internal_address == &endpoint.ip().to_string()
                        && *internal_port == endpoint.port()
                }
                (Ipv6PinholeStatus::Failed { stage, detail }, PinholeWait::Pinholed) => {
                    return Err(SeedHarnessError::Catalog(format!(
                        "IPv6 pinhole failed during {stage:?}: {detail}"
                    )));
                }
                (Ipv6PinholeStatus::CleanupFailed { detail, .. }, _) => {
                    return Err(SeedHarnessError::Catalog(format!(
                        "IPv6 pinhole cleanup remains uncertain: {detail}"
                    )));
                }
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

async fn wait_for_mapping(
    service: &ApplicationService,
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
    timeout(READY_TIMEOUT, async {
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
                } => Some(client_settings.port_mapping_status),
                ViewUpdatePayload::Patch {
                    patch:
                        rstorrent_session::ViewPatch::TorrentList {
                            client_settings: Some(client_settings),
                            ..
                        },
                } => Some(client_settings.port_mapping_status),
                _ => None,
            };
            match status {
                Some(status @ PortMappingStatus::Mapped { .. }) => return Ok(status),
                Some(PortMappingStatus::Failed { stage, detail }) => {
                    return Err(SeedHarnessError::Catalog(format!(
                        "UPnP mapping failed during {stage:?}: {detail}"
                    )));
                }
                Some(PortMappingStatus::RenewalFailed { detail, .. }) => {
                    return Err(SeedHarnessError::Catalog(format!(
                        "UPnP mapping renewal failed before readiness: {detail}"
                    )));
                }
                Some(PortMappingStatus::CleanupFailed { detail, .. }) => {
                    return Err(SeedHarnessError::Catalog(format!(
                        "prior UPnP mapping cleanup remains uncertain: {detail}"
                    )));
                }
                _ => {}
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
        assert!(Arguments::parse(std::iter::empty()).is_err());
    }
}
