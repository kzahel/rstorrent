//! Controlled loopback and explicit WAN uTP/libtorrent interoperability roles.

use std::collections::BTreeSet;
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::io::Write as _;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rstorrent_engine::peer::PeerRegistrySnapshot;
use rstorrent_engine::port_mapping::upnp::{
    UpnpDiscoveryConfig, UpnpMapping, UpnpStage, UpnpTransport, discover_igd_v2,
};
use rstorrent_engine::{
    AddressFamilyPolicy, IncomingPeerRuntime, IncomingPeerServiceConfig,
    IncomingPeerServiceSnapshot, IncomingTcpBootstrap, NetworkConfig, NetworkPolicy,
    PeerConnectionObservation, PeerEncryptionPolicy, PeerTransport, SeedContent, SeedRegistration,
    SessionUdpService, SessionUdpSnapshot, TorrentPeerActivitySink, TorrentPeerHandle, UtpService,
    UtpServiceSnapshot, download_controlled_utp,
};
use rstorrent_protocol::metainfo::{BEP9_METAINFO_LIMITS, Metainfo, MetainfoMode};
use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use tokio::io::AsyncWriteExt;
use tokio::net::UdpSocket;
use tokio::sync::oneshot;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

const PAYLOAD_BYTES: u64 = 2 * 1024 * 1024 + 731;
const PIECE_BYTES: u32 = 64 * 1024;
const LOOPBACK_ROLE_TIMEOUT: Duration = Duration::from_secs(30);
const WAN_ROLE_TIMEOUT: Duration = Duration::from_secs(180);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const PEER_IO_TIMEOUT: Duration = Duration::from_secs(5);
const LEECHER_PEER_ID: [u8; 20] = *b"-RSUTPL-000000000000";
const SEED_PEER_ID: [u8; 20] = *b"-RSUTPS-000000000000";
const MAPPING_DESCRIPTION: &str = "RSTorrent";
const USAGE: &str = "\
Usage:
  rstorrent-utp-interop leecher --metainfo PATH --peer 127.0.0.1:PORT --output PATH
  rstorrent-utp-interop wan-leecher --metainfo PATH --peer PUBLIC_IPV4:PORT --output PATH
  rstorrent-utp-interop seed --metainfo PATH --storage-root PATH
  rstorrent-utp-interop wan-seed --metainfo PATH --storage-root PATH
  rstorrent-utp-interop wan-mapping-audit --local-port PORT --external-port PORT";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LeecherScope {
    Loopback,
    Wan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SeedScope {
    Loopback,
    Wan,
}

impl SeedScope {
    const fn role(self) -> &'static str {
        match self {
            Self::Loopback => "seed",
            Self::Wan => "wan-seed",
        }
    }
}

impl LeecherScope {
    const fn role(self) -> &'static str {
        match self {
            Self::Loopback => "leecher",
            Self::Wan => "wan-leecher",
        }
    }

    const fn bind_address(self) -> Ipv4Addr {
        match self {
            Self::Loopback => Ipv4Addr::LOCALHOST,
            Self::Wan => Ipv4Addr::UNSPECIFIED,
        }
    }

    const fn network_policy(self) -> NetworkPolicy {
        match self {
            Self::Loopback => NetworkPolicy::LoopbackOnly,
            Self::Wan => NetworkPolicy::Online,
        }
    }
}

#[derive(Debug)]
enum Arguments {
    Leecher {
        scope: LeecherScope,
        metainfo: PathBuf,
        peer: SocketAddr,
        output: PathBuf,
    },
    Seed {
        scope: SeedScope,
        metainfo: PathBuf,
        storage_root: PathBuf,
    },
    MappingAudit {
        local_port: u16,
        external_port: u16,
    },
}

impl Arguments {
    fn timeout(&self) -> Duration {
        match self {
            Self::Leecher {
                scope: LeecherScope::Wan,
                ..
            } => WAN_ROLE_TIMEOUT,
            Self::Leecher {
                scope: LeecherScope::Loopback,
                ..
            }
            | Self::Seed {
                scope: SeedScope::Loopback,
                ..
            }
            | Self::MappingAudit { .. } => LOOPBACK_ROLE_TIMEOUT,
            Self::Seed {
                scope: SeedScope::Wan,
                ..
            } => WAN_ROLE_TIMEOUT,
        }
    }

    fn parse(mut values: impl Iterator<Item = OsString>) -> Result<Self, String> {
        let role = values
            .next()
            .ok_or_else(|| "a role is required".to_owned())?;
        let role = role
            .to_str()
            .ok_or_else(|| "the role must be valid UTF-8".to_owned())?;
        let values = values.collect::<Vec<_>>();
        let mut metainfo = None;
        let mut peer = None;
        let mut output = None;
        let mut storage_root = None;
        let mut local_port = None;
        let mut external_port = None;
        let mut index = 0;
        while index < values.len() {
            let flag = values[index]
                .to_str()
                .ok_or_else(|| "flags must be valid UTF-8".to_owned())?;
            index += 1;
            let value = values
                .get(index)
                .ok_or_else(|| format!("{flag} requires a value"))?;
            index += 1;
            match flag {
                "--metainfo" => set_once(&mut metainfo, PathBuf::from(value), flag)?,
                "--peer" => {
                    let value = value
                        .to_str()
                        .ok_or_else(|| "--peer must be valid UTF-8".to_owned())?;
                    let address = value
                        .parse::<SocketAddr>()
                        .map_err(|_| "--peer must be an IP address and port".to_owned())?;
                    set_once(&mut peer, address, flag)?;
                }
                "--output" => set_once(&mut output, PathBuf::from(value), flag)?,
                "--storage-root" => {
                    set_once(&mut storage_root, PathBuf::from(value), flag)?;
                }
                "--local-port" => {
                    set_once(&mut local_port, parse_port(value, flag)?, flag)?;
                }
                "--external-port" => {
                    set_once(&mut external_port, parse_port(value, flag)?, flag)?;
                }
                _ => return Err(format!("unknown argument {flag}")),
            }
        }
        match role {
            "leecher" | "wan-leecher" => {
                if storage_root.is_some() || local_port.is_some() || external_port.is_some() {
                    return Err(format!("{role} received a role-specific argument"));
                }
                let metainfo = metainfo.ok_or_else(|| "--metainfo is required".to_owned())?;
                let peer = peer.ok_or_else(|| format!("{role} requires --peer"))?;
                let scope = if role == "leecher" {
                    LeecherScope::Loopback
                } else {
                    LeecherScope::Wan
                };
                let eligible = match (scope, peer) {
                    (LeecherScope::Loopback, SocketAddr::V4(address)) => {
                        address.ip().is_loopback() && address.port() != 0
                    }
                    (LeecherScope::Wan, SocketAddr::V4(address)) => {
                        eligible_public_ipv4(*address.ip()) && address.port() != 0
                    }
                    (_, SocketAddr::V6(_)) => false,
                };
                if !eligible {
                    return Err(match scope {
                        LeecherScope::Loopback => {
                            "--peer must be a nonzero IPv4 loopback endpoint".to_owned()
                        }
                        LeecherScope::Wan => {
                            "--peer must be a nonzero public IPv4 endpoint".to_owned()
                        }
                    });
                }
                Ok(Self::Leecher {
                    scope,
                    metainfo,
                    peer,
                    output: output.ok_or_else(|| format!("{role} requires --output"))?,
                })
            }
            "seed" | "wan-seed" => {
                if peer.is_some()
                    || output.is_some()
                    || local_port.is_some()
                    || external_port.is_some()
                {
                    return Err(format!("{role} received a role-specific argument"));
                }
                Ok(Self::Seed {
                    scope: if role == "seed" {
                        SeedScope::Loopback
                    } else {
                        SeedScope::Wan
                    },
                    metainfo: metainfo.ok_or_else(|| "--metainfo is required".to_owned())?,
                    storage_root: storage_root
                        .ok_or_else(|| format!("{role} requires --storage-root"))?,
                })
            }
            "wan-mapping-audit" => {
                if metainfo.is_some()
                    || peer.is_some()
                    || output.is_some()
                    || storage_root.is_some()
                {
                    return Err("wan-mapping-audit received a role-specific argument".to_owned());
                }
                Ok(Self::MappingAudit {
                    local_port: local_port
                        .ok_or_else(|| "wan-mapping-audit requires --local-port".to_owned())?,
                    external_port: external_port
                        .ok_or_else(|| "wan-mapping-audit requires --external-port".to_owned())?,
                })
            }
            _ => Err(format!("unknown role {role}")),
        }
    }
}

fn parse_port(value: &OsString, flag: &str) -> Result<u16, String> {
    let value = value
        .to_str()
        .ok_or_else(|| format!("{flag} must be valid UTF-8"))?;
    let port = value
        .parse::<u16>()
        .map_err(|_| format!("{flag} must be a nonzero port"))?;
    if port == 0 {
        return Err(format!("{flag} must be a nonzero port"));
    }
    Ok(port)
}

fn set_once<T>(target: &mut Option<T>, value: T, flag: &str) -> Result<(), String> {
    if target.replace(value).is_some() {
        return Err(format!("{flag} may only be supplied once"));
    }
    Ok(())
}

fn eligible_public_ipv4(address: Ipv4Addr) -> bool {
    let [first, second, third, _] = address.octets();
    !matches!(first, 0 | 10 | 127)
        && !(first == 100 && (64..=127).contains(&second))
        && !(first == 169 && second == 254)
        && !(first == 172 && (16..=31).contains(&second))
        && !(first == 192 && second == 0 && third == 0)
        && !(first == 192 && second == 0 && third == 2)
        && !(first == 192 && second == 88 && third == 99)
        && !(first == 192 && second == 168)
        && !(first == 198 && (second == 18 || second == 19))
        && !(first == 198 && second == 51 && third == 100)
        && !(first == 203 && second == 0 && third == 113)
        && first < 224
}

#[derive(Clone, Debug, Default)]
struct PeerEvidence {
    connection_high_water: usize,
    utp_high_water: usize,
    tcp_high_water: usize,
    endpoints: BTreeSet<SocketAddr>,
}

#[derive(Debug, Default)]
struct RecordingPeerSink {
    evidence: Mutex<PeerEvidence>,
}

impl RecordingPeerSink {
    fn snapshot(&self) -> PeerEvidence {
        self.evidence
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl TorrentPeerActivitySink for RecordingPeerSink {
    fn record_peer_connections(
        &self,
        _captured_at: Duration,
        peers: Vec<PeerConnectionObservation>,
    ) {
        let mut evidence = self
            .evidence
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        evidence.connection_high_water = evidence.connection_high_water.max(peers.len());
        let utp = peers
            .iter()
            .filter(|peer| peer.transport == PeerTransport::Utp)
            .count();
        let tcp = peers
            .iter()
            .filter(|peer| peer.transport == PeerTransport::Tcp)
            .count();
        evidence.utp_high_water = evidence.utp_high_water.max(utp);
        evidence.tcp_high_water = evidence.tcp_high_water.max(tcp);
        evidence
            .endpoints
            .extend(peers.iter().map(|peer| peer.endpoint));
    }

    fn record_peer_registry(&self, _active: bool, _snapshot: PeerRegistrySnapshot) {}
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let arguments = match Arguments::parse(env::args_os().skip(1)) {
        Ok(arguments) => arguments,
        Err(error) => {
            eprintln!("argument error: {error}\n{USAGE}");
            std::process::exit(2);
        }
    };
    let role_timeout = arguments.timeout();
    match timeout(role_timeout, run(arguments)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            eprintln!("uTP interoperability role failed: {error}");
            std::process::exit(1);
        }
        Err(_) => {
            eprintln!(
                "uTP interoperability role exceeded {} seconds",
                role_timeout.as_secs()
            );
            std::process::exit(1);
        }
    }
}

async fn run(arguments: Arguments) -> Result<(), Box<dyn Error>> {
    match arguments {
        Arguments::Leecher {
            scope,
            metainfo,
            peer,
            output,
        } => run_leecher(scope, &metainfo, peer, &output).await,
        Arguments::Seed {
            scope,
            metainfo,
            storage_root,
        } => run_seed(scope, &metainfo, &storage_root).await,
        Arguments::MappingAudit {
            local_port,
            external_port,
        } => run_mapping_audit(local_port, external_port).await,
    }
}

async fn run_leecher(
    scope: LeecherScope,
    metainfo_path: &Path,
    peer: SocketAddr,
    output: &Path,
) -> Result<(), Box<dyn Error>> {
    let (metainfo, _) = read_fixture(metainfo_path).await?;
    let socket = UdpSocket::bind((scope.bind_address(), 0)).await?;
    let (mut udp, dht) = SessionUdpService::start(socket)?;
    let utp = UtpService::start(&mut udp)?;
    let local_address = udp.local_address();
    write_json(json!({
        "event": "ready",
        "role": scope.role(),
        "listen": local_address.to_string(),
    }))?;

    let stream = utp.handle().connect(peer).await?;
    let network = NetworkConfig::new(scope.network_policy(), HANDSHAKE_TIMEOUT, PEER_IO_TIMEOUT)
        .with_address_families(AddressFamilyPolicy::ipv4_only())
        .with_peer_id(LEECHER_PEER_ID)
        .with_encryption(PeerEncryptionPolicy::Disabled);
    let (payload, report) = download_controlled_utp(stream, &metainfo, network).await?;
    let digest = hex(&Sha1::digest(&payload));
    write_new_file(output, &payload).await?;

    let live_utp = utp.snapshot();
    let live_udp = udp.snapshot();
    let terminal_utp = utp.shutdown().await?;
    drop(dht);
    let terminal_udp = udp.shutdown().await?;
    validate_terminal(&terminal_utp, &terminal_udp)?;
    write_json(json!({
        "event": "complete",
        "role": scope.role(),
        "listen": local_address.to_string(),
        "peer": peer.to_string(),
        "payload": {
            "bytes": report.bytes,
            "pieces": report.pieces,
            "requests": report.requests,
            "sha1": digest,
        },
        "remote_peer_id": hex(&report.peer_id),
        "resources": {
            "live_udp": udp_json(live_udp),
            "live_utp": utp_json(live_utp),
            "terminal_udp": udp_json(terminal_udp),
            "terminal_utp": utp_json(terminal_utp),
        },
    }))?;
    Ok(())
}

struct SeedEvidence {
    live_incoming: IncomingPeerServiceSnapshot,
    peers: PeerEvidence,
    live_utp: UtpServiceSnapshot,
    live_udp: SessionUdpSnapshot,
}

struct SeedMapping {
    gateway: rstorrent_engine::port_mapping::upnp::UpnpGateway,
    mapping: UpnpMapping,
    cancellation: CancellationToken,
}

async fn run_seed(
    scope: SeedScope,
    metainfo_path: &Path,
    storage_root: &Path,
) -> Result<(), Box<dyn Error>> {
    let (metainfo, raw_info) = read_fixture(metainfo_path).await?;
    let bind_address = match scope {
        SeedScope::Loopback => Ipv4Addr::LOCALHOST,
        SeedScope::Wan => select_local_network_ipv4().await?,
    };
    let socket = UdpSocket::bind((bind_address, 0)).await?;
    let (mut udp, dht) = SessionUdpService::start(socket)?;
    let mut utp = UtpService::start(&mut udp)?;
    let mut incoming_config = IncomingPeerServiceConfig::new(IncomingTcpBootstrap::Disabled)
        .with_encryption(PeerEncryptionPolicy::Disabled);
    incoming_config.peer_id = SEED_PEER_ID;
    let incoming_runtime = IncomingPeerRuntime::start(incoming_config)?;
    let incoming = incoming_runtime.handle();
    let peer_sink = Arc::new(RecordingPeerSink::default());
    let torrent_peers = TorrentPeerHandle::new(peer_sink.clone())?;
    let content = SeedContent::open_published(
        storage_root,
        &metainfo,
        &vec![true; metainfo.piece_count()],
        &[],
    )
    .await?;
    let token = incoming
        .register(SeedRegistration::new(raw_info, content, torrent_peers)?)
        .await?;
    let local_endpoint = match udp.local_address() {
        SocketAddr::V4(endpoint) => endpoint,
        SocketAddr::V6(_) => return Err("uTP WAN seed requires an IPv4 UDP socket".into()),
    };
    let mut mapping_owner = None;
    if scope == SeedScope::Wan {
        write_json(json!({
            "event": "bound",
            "role": scope.role(),
            "listen": local_endpoint.to_string(),
        }))?;
        let cancellation = CancellationToken::new();
        let gateway =
            discover_igd_v2(UpnpDiscoveryConfig::new(bind_address)?, &cancellation).await?;
        let existing = gateway
            .query_mapping(
                local_endpoint.port(),
                UpnpTransport::Udp,
                UpnpStage::Add,
                &cancellation,
            )
            .await?;
        if existing.is_some() {
            return Err("exact diagnostic UDP external port is already occupied".into());
        }
        write_json(json!({
            "event": "mapping-intent",
            "role": scope.role(),
            "local_port": local_endpoint.port(),
            "external_port": local_endpoint.port(),
            "protocol": "UDP",
        }))?;
        let mapping = gateway
            .create_exact_mapping(
                UpnpTransport::Udp,
                local_endpoint.port(),
                local_endpoint.port(),
                &cancellation,
            )
            .await?;
        write_json(json!({
            "event": "ready",
            "role": scope.role(),
            "listen": local_endpoint.to_string(),
            "external_address": mapping.external_address.to_string(),
            "external_port": mapping.external_port,
            "mapping": {
                "protocol": mapping.transport.as_str(),
                "transport": "UPnP",
                "lease_seconds": mapping.lease_seconds,
            },
        }))?;
        mapping_owner = Some(SeedMapping {
            gateway,
            mapping,
            cancellation,
        });
    } else {
        write_json(json!({
            "event": "ready",
            "role": scope.role(),
            "listen": local_endpoint.to_string(),
        }))?;
    }

    let transfer_result: Result<SeedEvidence, Box<dyn Error>> = async {
        let stream = utp
            .accept()
            .await
            .ok_or("uTP service stopped before accepting a stream")?;
        incoming.admit_utp(stream, HANDSHAKE_TIMEOUT).await?;
        wait_for_stop().await?;
        let live_incoming = incoming.snapshot();
        let peers = peer_sink.snapshot();
        validate_seed_evidence(scope, &live_incoming, &peers)?;
        Ok(SeedEvidence {
            live_incoming,
            peers,
            live_utp: utp.snapshot(),
            live_udp: udp.snapshot(),
        })
    }
    .await;

    let mut failures = Vec::new();
    let evidence = match transfer_result {
        Ok(evidence) => Some(evidence),
        Err(error) => {
            failures.push(error.to_string());
            None
        }
    };
    match incoming.unregister(token).await {
        Ok(true) => {}
        Ok(false) => failures.push("seed registration disappeared before shutdown".to_owned()),
        Err(error) => failures.push(format!("unregister seed: {error}")),
    }
    let terminal_incoming = match incoming_runtime.shutdown().await {
        Ok(snapshot) => Some(snapshot),
        Err(error) => {
            failures.push(format!("shutdown incoming owner: {error}"));
            None
        }
    };
    let terminal_utp = match utp.shutdown().await {
        Ok(snapshot) => Some(snapshot),
        Err(error) => {
            failures.push(format!("shutdown uTP owner: {error}"));
            None
        }
    };
    let mapping_deleted = if let Some(owner) = mapping_owner {
        match owner
            .gateway
            .delete_mapping(&owner.mapping, &owner.cancellation)
            .await
        {
            Ok(()) => true,
            Err(error) => {
                failures.push(format!("delete exact UDP mapping: {error}"));
                false
            }
        }
    } else {
        false
    };
    drop(dht);
    let terminal_udp = match udp.shutdown().await {
        Ok(snapshot) => Some(snapshot),
        Err(error) => {
            failures.push(format!("shutdown session UDP owner: {error}"));
            None
        }
    };
    if let (Some(terminal_utp), Some(terminal_udp)) = (&terminal_utp, &terminal_udp)
        && let Err(error) = validate_terminal(terminal_utp, terminal_udp)
    {
        failures.push(error.to_string());
    }
    if let Some(terminal_incoming) = &terminal_incoming
        && (terminal_incoming.pending != 0 || terminal_incoming.established != 0)
    {
        failures.push("incoming peer ownership was nonzero after shutdown".to_owned());
    }
    if !failures.is_empty() {
        return Err(failures.join("; ").into());
    }
    let evidence = evidence.ok_or("seed completed without transfer evidence")?;
    let terminal_incoming = terminal_incoming.ok_or("incoming terminal snapshot is missing")?;
    let terminal_utp = terminal_utp.ok_or("uTP terminal snapshot is missing")?;
    let terminal_udp = terminal_udp.ok_or("UDP terminal snapshot is missing")?;
    write_json(json!({
        "event": "complete",
        "role": scope.role(),
        "mapping_deleted": mapping_deleted,
        "payload": {
            "bytes": metainfo.total_length,
            "pieces": metainfo.piece_count(),
        },
        "peer_evidence": peer_evidence_json(&evidence.peers),
        "resources": {
            "live_incoming": incoming_json(&evidence.live_incoming),
            "live_udp": udp_json(evidence.live_udp),
            "live_utp": utp_json(evidence.live_utp),
            "terminal_incoming": incoming_json(&terminal_incoming),
            "terminal_udp": udp_json(terminal_udp),
            "terminal_utp": utp_json(terminal_utp),
        },
    }))?;
    Ok(())
}

async fn select_local_network_ipv4() -> Result<Ipv4Addr, Box<dyn Error>> {
    let probe = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)).await?;
    probe
        .connect(SocketAddrV4::new(Ipv4Addr::new(239, 255, 255, 250), 1_900))
        .await?;
    let address = match probe.local_addr()? {
        SocketAddr::V4(endpoint) => *endpoint.ip(),
        SocketAddr::V6(_) => return Err("ordinary local route selected IPv6".into()),
    };
    if address.is_unspecified()
        || address.is_loopback()
        || address.is_multicast()
        || address.is_broadcast()
    {
        return Err("ordinary local route selected an ineligible IPv4 address".into());
    }
    Ok(address)
}

async fn run_mapping_audit(local_port: u16, external_port: u16) -> Result<(), Box<dyn Error>> {
    let local_address = select_local_network_ipv4().await?;
    let cancellation = CancellationToken::new();
    let gateway = discover_igd_v2(UpnpDiscoveryConfig::new(local_address)?, &cancellation).await?;
    let entry = gateway
        .query_mapping(
            external_port,
            UpnpTransport::Udp,
            UpnpStage::Delete,
            &cancellation,
        )
        .await?;
    let owned = entry.as_ref().is_some_and(|entry| {
        entry.internal_client == local_address
            && entry.internal_port == local_port
            && entry.enabled
            && entry.description == MAPPING_DESCRIPTION
            && entry.lease_seconds > 0
            && entry.lease_seconds <= 3_600
    });
    let foreign = entry.is_some() && !owned;
    let mut deleted = false;
    if let Some(entry) = entry.filter(|_| owned) {
        let mapping = UpnpMapping {
            local_endpoint: SocketAddrV4::new(local_address, local_port),
            external_address: gateway.external_address(&cancellation).await?,
            external_port,
            lease_seconds: entry.lease_seconds,
            transport: UpnpTransport::Udp,
        };
        gateway.delete_mapping(&mapping, &cancellation).await?;
        deleted = true;
    }
    write_json(json!({
        "event": "mapping-audit",
        "role": "wan-mapping-audit",
        "owned_mapping_found": owned,
        "owned_mapping_deleted": deleted,
        "foreign_mapping_preserved": foreign,
        "owned_mapping_absent": !owned || deleted,
    }))?;
    Ok(())
}

async fn read_fixture(path: &Path) -> Result<(Metainfo, Vec<u8>), Box<dyn Error>> {
    let outer = tokio::fs::read(path).await?;
    let metainfo = Metainfo::from_bytes_with_limits(&outer, BEP9_METAINFO_LIMITS)?;
    if metainfo.total_length != PAYLOAD_BYTES
        || metainfo.piece_length != PIECE_BYTES
        || metainfo.mode != MetainfoMode::SingleFile
        || metainfo.piece_count() != 33
    {
        return Err(format!(
            "fixture must be one {PAYLOAD_BYTES}-byte file with {PIECE_BYTES}-byte pieces"
        )
        .into());
    }
    let raw_info = Metainfo::info_bytes_with_limits(&outer, BEP9_METAINFO_LIMITS)?.to_vec();
    Ok((metainfo, raw_info))
}

async fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut output = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .await?;
    output.write_all(bytes).await?;
    output.flush().await?;
    Ok(())
}

async fn wait_for_stop() -> Result<(), Box<dyn Error>> {
    let (sender, receiver) = oneshot::channel();
    let input_thread = std::thread::spawn(move || {
        let mut command = String::new();
        let result = std::io::stdin()
            .read_line(&mut command)
            .map(|read| (read, command));
        let _ = sender.send(result);
    });
    let (read, command) = receiver.await.map_err(|_| "seed input thread stopped")??;
    input_thread
        .join()
        .map_err(|_| "seed input thread panicked")?;
    if read == 0 || command.trim() != "stop" {
        return Err("seed requires a final stop command after verified download".into());
    }
    Ok(())
}

fn validate_seed_evidence(
    scope: SeedScope,
    incoming: &IncomingPeerServiceSnapshot,
    peers: &PeerEvidence,
) -> Result<(), Box<dyn Error>> {
    if incoming.payload_bytes_sent < PAYLOAD_BYTES {
        return Err(format!(
            "seed uploaded {} of {PAYLOAD_BYTES} required bytes",
            incoming.payload_bytes_sent
        )
        .into());
    }
    if peers.connection_high_water != 1 || peers.utp_high_water != 1 || peers.tcp_high_water != 0 {
        return Err(format!("unexpected peer transport evidence: {peers:?}").into());
    }
    let endpoint_is_eligible = |peer: &SocketAddr| match (scope, peer) {
        (SeedScope::Loopback, SocketAddr::V4(endpoint)) => endpoint.ip().is_loopback(),
        (SeedScope::Wan, SocketAddr::V4(endpoint)) => eligible_public_ipv4(*endpoint.ip()),
        (_, SocketAddr::V6(_)) => false,
    };
    if peers.endpoints.len() != 1
        || peers
            .endpoints
            .iter()
            .any(|peer| !endpoint_is_eligible(peer))
    {
        return Err(format!("unexpected peer endpoints: {:?}", peers.endpoints).into());
    }
    Ok(())
}

fn validate_terminal(
    utp: &UtpServiceSnapshot,
    udp: &SessionUdpSnapshot,
) -> Result<(), Box<dyn Error>> {
    if utp.active_connections != 0 || utp.incoming_half_open != 0 || utp.worker_panics != 0 {
        return Err(format!("uTP terminal ownership is not clean: {utp:?}").into());
    }
    if udp.tasks != 0 || udp.queued != 0 || udp.utp_queued != 0 {
        return Err(format!("session UDP terminal ownership is not clean: {udp:?}").into());
    }
    Ok(())
}

fn write_json(value: Value) -> Result<(), Box<dyn Error>> {
    println!("{value}");
    std::io::stdout().flush()?;
    Ok(())
}

fn peer_evidence_json(evidence: &PeerEvidence) -> Value {
    json!({
        "connection_high_water": evidence.connection_high_water,
        "utp_high_water": evidence.utp_high_water,
        "tcp_high_water": evidence.tcp_high_water,
        "endpoints": evidence.endpoints.iter().map(ToString::to_string).collect::<Vec<_>>(),
    })
}

fn incoming_json(snapshot: &IncomingPeerServiceSnapshot) -> Value {
    json!({
        "registrations": snapshot.registrations,
        "pending": snapshot.pending,
        "pending_high_water": snapshot.pending_high_water,
        "established": snapshot.established,
        "established_high_water": snapshot.established_high_water,
        "connections": snapshot.peer_budget.total,
        "connection_high_water": snapshot.peer_budget.total_high_water,
        "reads": snapshot.reads,
        "read_bytes": snapshot.read_bytes,
        "read_high_water": snapshot.read_high_water,
        "read_bytes_high_water": snapshot.read_bytes_high_water,
        "queued_requests_high_water": snapshot.queued_requests_high_water,
        "queued_bytes_high_water": snapshot.queued_bytes_high_water,
        "writer_send_buffer_high_water": snapshot.writer_send_buffer_high_water,
        "upload_slots_high_water": snapshot.upload_slots_high_water,
        "payload_bytes_sent": snapshot.payload_bytes_sent,
    })
}

fn udp_json(snapshot: SessionUdpSnapshot) -> Value {
    json!({
        "tasks": snapshot.tasks,
        "task_high_water": snapshot.task_high_water,
        "dht_queued": snapshot.queued,
        "dht_queue_high_water": snapshot.queue_high_water,
        "utp_queued": snapshot.utp_queued,
        "utp_queue_high_water": snapshot.utp_queue_high_water,
        "datagrams_received": snapshot.datagrams_received,
        "datagram_bytes_received": snapshot.datagram_bytes_received,
        "datagrams_dropped": snapshot.datagrams_dropped,
        "dht_datagrams_dropped": snapshot.dht_datagrams_dropped,
        "utp_datagrams_classified": snapshot.utp_datagrams_classified,
        "utp_datagram_bytes_classified": snapshot.utp_datagram_bytes_classified,
        "utp_datagrams_dropped": snapshot.utp_datagrams_dropped,
    })
}

fn utp_json(snapshot: UtpServiceSnapshot) -> Value {
    json!({
        "active_connections": snapshot.active_connections,
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
        "retransmission_datagrams_sent": snapshot.retransmission_datagrams_sent,
        "retransmission_bytes_sent": snapshot.retransmission_bytes_sent,
        "retransmission_queue_high_water": snapshot.retransmission_queue_high_water,
        "loss_reduction_high_water": snapshot.loss_reduction_high_water,
        "timeout_collapse_high_water": snapshot.timeout_collapse_high_water,
        "delivered_byte_high_water": snapshot.delivered_byte_high_water,
        "unsent_byte_high_water": snapshot.unsent_byte_high_water,
        "sent_byte_high_water": snapshot.sent_byte_high_water,
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
        "worker_panics": snapshot.worker_panics,
    })
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(DIGITS[(byte >> 4) as usize] as char);
        result.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> impl Iterator<Item = OsString> {
        values
            .iter()
            .map(OsString::from)
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn arguments_are_role_specific_and_network_scoped() {
        assert!(matches!(
            Arguments::parse(strings(&[
                "leecher",
                "--metainfo",
                "fixture.torrent",
                "--peer",
                "127.0.0.1:1",
                "--output",
                "payload.bin",
            ])),
            Ok(Arguments::Leecher { .. })
        ));
        assert!(matches!(
            Arguments::parse(strings(&[
                "seed",
                "--metainfo",
                "fixture.torrent",
                "--storage-root",
                "seed",
            ])),
            Ok(Arguments::Seed { .. })
        ));
        let wan_seed = Arguments::parse(strings(&[
            "wan-seed",
            "--metainfo",
            "fixture.torrent",
            "--storage-root",
            "seed",
        ]))
        .unwrap();
        assert_eq!(wan_seed.timeout(), WAN_ROLE_TIMEOUT);
        assert!(matches!(
            &wan_seed,
            Arguments::Seed {
                scope: SeedScope::Wan,
                ..
            }
        ));
        assert!(matches!(
            Arguments::parse(strings(&[
                "wan-mapping-audit",
                "--local-port",
                "42000",
                "--external-port",
                "42000",
            ])),
            Ok(Arguments::MappingAudit {
                local_port: 42_000,
                external_port: 42_000,
            })
        ));
        assert!(
            Arguments::parse(strings(&[
                "wan-mapping-audit",
                "--local-port",
                "0",
                "--external-port",
                "42000",
            ]))
            .is_err()
        );
        assert!(
            Arguments::parse(strings(&[
                "leecher",
                "--metainfo",
                "fixture.torrent",
                "--peer",
                "192.0.2.1:1",
                "--output",
                "payload.bin",
            ]))
            .is_err()
        );
        assert!(matches!(
            Arguments::parse(strings(&[
                "wan-leecher",
                "--metainfo",
                "fixture.torrent",
                "--peer",
                "8.8.8.8:1",
                "--output",
                "payload.bin",
            ])),
            Ok(Arguments::Leecher {
                scope: LeecherScope::Wan,
                ..
            })
        ));
        let wan = Arguments::parse(strings(&[
            "wan-leecher",
            "--metainfo",
            "fixture.torrent",
            "--peer",
            "8.8.8.8:1",
            "--output",
            "payload.bin",
        ]))
        .unwrap();
        assert_eq!(wan.timeout(), WAN_ROLE_TIMEOUT);
        let loopback = Arguments::parse(strings(&[
            "leecher",
            "--metainfo",
            "fixture.torrent",
            "--peer",
            "127.0.0.1:1",
            "--output",
            "payload.bin",
        ]))
        .unwrap();
        assert_eq!(loopback.timeout(), LOOPBACK_ROLE_TIMEOUT);
        for peer in [
            "0.0.0.0:1",
            "10.0.0.1:1",
            "100.64.0.1:1",
            "127.0.0.1:1",
            "169.254.1.1:1",
            "172.16.0.1:1",
            "192.0.2.1:1",
            "192.168.1.1:1",
            "198.18.0.1:1",
            "198.51.100.1:1",
            "203.0.113.1:1",
            "224.0.0.1:1",
        ] {
            assert!(
                Arguments::parse(strings(&[
                    "wan-leecher",
                    "--metainfo",
                    "fixture.torrent",
                    "--peer",
                    peer,
                    "--output",
                    "payload.bin",
                ]))
                .is_err(),
                "accepted {peer}"
            );
        }
    }
}
