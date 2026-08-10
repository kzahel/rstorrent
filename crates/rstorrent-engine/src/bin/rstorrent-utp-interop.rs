//! Loopback-only controlled uTP/libtorrent interoperability roles.

use std::collections::BTreeSet;
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::io::Write as _;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rstorrent_engine::peer::PeerRegistrySnapshot;
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

const PAYLOAD_BYTES: u64 = 2 * 1024 * 1024 + 731;
const PIECE_BYTES: u32 = 64 * 1024;
const ROLE_TIMEOUT: Duration = Duration::from_secs(30);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const PEER_IO_TIMEOUT: Duration = Duration::from_secs(5);
const LEECHER_PEER_ID: [u8; 20] = *b"-RSUTPL-000000000000";
const SEED_PEER_ID: [u8; 20] = *b"-RSUTPS-000000000000";
const USAGE: &str = "\
Usage:
  rstorrent-utp-interop leecher --metainfo PATH --peer 127.0.0.1:PORT --output PATH
  rstorrent-utp-interop seed --metainfo PATH --storage-root PATH";

#[derive(Debug)]
enum Arguments {
    Leecher {
        metainfo: PathBuf,
        peer: SocketAddr,
        output: PathBuf,
    },
    Seed {
        metainfo: PathBuf,
        storage_root: PathBuf,
    },
}

impl Arguments {
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
                _ => return Err(format!("unknown argument {flag}")),
            }
        }
        let metainfo = metainfo.ok_or_else(|| "--metainfo is required".to_owned())?;
        match role {
            "leecher" => {
                if storage_root.is_some() {
                    return Err("leecher does not accept --storage-root".to_owned());
                }
                let peer = peer.ok_or_else(|| "leecher requires --peer".to_owned())?;
                if !peer.ip().is_loopback() || !peer.is_ipv4() || peer.port() == 0 {
                    return Err("--peer must be a nonzero IPv4 loopback endpoint".to_owned());
                }
                Ok(Self::Leecher {
                    metainfo,
                    peer,
                    output: output.ok_or_else(|| "leecher requires --output".to_owned())?,
                })
            }
            "seed" => {
                if peer.is_some() || output.is_some() {
                    return Err("seed does not accept --peer or --output".to_owned());
                }
                Ok(Self::Seed {
                    metainfo,
                    storage_root: storage_root
                        .ok_or_else(|| "seed requires --storage-root".to_owned())?,
                })
            }
            _ => Err(format!("unknown role {role}")),
        }
    }
}

fn set_once<T>(target: &mut Option<T>, value: T, flag: &str) -> Result<(), String> {
    if target.replace(value).is_some() {
        return Err(format!("{flag} may only be supplied once"));
    }
    Ok(())
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
    match timeout(ROLE_TIMEOUT, run(arguments)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            eprintln!("uTP interoperability role failed: {error}");
            std::process::exit(1);
        }
        Err(_) => {
            eprintln!("uTP interoperability role exceeded 30 seconds");
            std::process::exit(1);
        }
    }
}

async fn run(arguments: Arguments) -> Result<(), Box<dyn Error>> {
    match arguments {
        Arguments::Leecher {
            metainfo,
            peer,
            output,
        } => run_leecher(&metainfo, peer, &output).await,
        Arguments::Seed {
            metainfo,
            storage_root,
        } => run_seed(&metainfo, &storage_root).await,
    }
}

async fn run_leecher(
    metainfo_path: &Path,
    peer: SocketAddr,
    output: &Path,
) -> Result<(), Box<dyn Error>> {
    let (metainfo, _) = read_fixture(metainfo_path).await?;
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let (mut udp, dht) = SessionUdpService::start(socket)?;
    let utp = UtpService::start(&mut udp)?;
    let local_address = udp.local_address();
    write_json(json!({
        "event": "ready",
        "role": "leecher",
        "listen": local_address.to_string(),
    }))?;

    let stream = utp.handle().connect(peer).await?;
    let network = NetworkConfig::new(
        NetworkPolicy::LoopbackOnly,
        HANDSHAKE_TIMEOUT,
        PEER_IO_TIMEOUT,
    )
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
        "role": "leecher",
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

async fn run_seed(metainfo_path: &Path, storage_root: &Path) -> Result<(), Box<dyn Error>> {
    let (metainfo, raw_info) = read_fixture(metainfo_path).await?;
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await?;
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
    write_json(json!({
        "event": "ready",
        "role": "seed",
        "listen": udp.local_address().to_string(),
    }))?;

    let stream = utp
        .accept()
        .await
        .ok_or("uTP service stopped before accepting a stream")?;
    incoming.admit_utp(stream, HANDSHAKE_TIMEOUT).await?;
    wait_for_stop().await?;
    let live_incoming = incoming.snapshot();
    let peers = peer_sink.snapshot();
    validate_seed_evidence(&live_incoming, &peers)?;
    let live_utp = utp.snapshot();
    let live_udp = udp.snapshot();

    if !incoming.unregister(token).await? {
        return Err("seed registration disappeared before shutdown".into());
    }
    let terminal_incoming = incoming_runtime.shutdown().await?;
    let terminal_utp = utp.shutdown().await?;
    drop(dht);
    let terminal_udp = udp.shutdown().await?;
    validate_terminal(&terminal_utp, &terminal_udp)?;
    if terminal_incoming.pending != 0 || terminal_incoming.established != 0 {
        return Err("incoming peer ownership was nonzero after shutdown".into());
    }
    write_json(json!({
        "event": "complete",
        "role": "seed",
        "payload": {
            "bytes": metainfo.total_length,
            "pieces": metainfo.piece_count(),
        },
        "peer_evidence": peer_evidence_json(&peers),
        "resources": {
            "live_incoming": incoming_json(&live_incoming),
            "live_udp": udp_json(live_udp),
            "live_utp": utp_json(live_utp),
            "terminal_incoming": incoming_json(&terminal_incoming),
            "terminal_udp": udp_json(terminal_udp),
            "terminal_utp": utp_json(terminal_utp),
        },
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
    if peers.endpoints.len() != 1 || peers.endpoints.iter().any(|peer| !peer.ip().is_loopback()) {
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
        "delivered_byte_high_water": snapshot.delivered_byte_high_water,
        "unsent_byte_high_water": snapshot.unsent_byte_high_water,
        "sent_byte_high_water": snapshot.sent_byte_high_water,
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
    fn arguments_are_role_specific_and_loopback_only() {
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
    }
}
