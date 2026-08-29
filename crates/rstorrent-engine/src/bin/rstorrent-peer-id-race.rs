//! Controlled crossed-connection diagnostic sharing one torrent peer owner.

use std::collections::BTreeSet;
use std::env;
use std::error::Error;
use std::io::Write;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use rstorrent_engine::peer::PeerRegistrySnapshot;
use rstorrent_engine::{
    DownloadConfig, DownloadControl, DownloadResourceLimits, IncomingPeerService,
    IncomingPeerServiceConfig, IncomingTcpBootstrap, NetworkConfig, NetworkPolicy, PeerBudget,
    PeerConnectionObservation, SeedContent, SeedRegistration, TorrentId, TorrentIdentityContext,
    TorrentPeerActivitySink, TorrentPeerHandle, download_verified_piece_with_peer_state,
};
use rstorrent_protocol::identity::V1InfoHash;
use rstorrent_protocol::metainfo::{BEP9_METAINFO_LIMITS, Metainfo};

#[derive(Debug, Default)]
struct RecordingPeerSink {
    connections: Mutex<Vec<Vec<PeerConnectionObservation>>>,
}

impl TorrentPeerActivitySink for RecordingPeerSink {
    fn record_peer_connections(
        &self,
        _captured_at: std::time::Duration,
        peers: Vec<PeerConnectionObservation>,
    ) {
        self.connections
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(peers);
    }

    fn record_peer_registry(&self, _active: bool, _snapshot: PeerRegistrySnapshot) {}
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("peer-ID race diagnostic failed: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn Error>> {
    let arguments = Arguments::parse(env::args().skip(1))?;
    let outer = std::fs::read(&arguments.metainfo)?;
    let metainfo = Metainfo::from_bytes_with_limits(&outer, BEP9_METAINFO_LIMITS)?;
    let peer_budget = PeerBudget::system_default();
    let sink = Arc::new(RecordingPeerSink::default());
    let torrent_peers = TorrentPeerHandle::new(sink.clone())?;
    let torrent_id =
        TorrentId::generate().map_err(|error| std::io::Error::other(error.to_string()))?;
    let content = SeedContent::open_verified(
        &arguments.seed_root,
        torrent_id,
        &metainfo,
        &vec![true; metainfo.piece_count()],
        &[],
    )
    .await?;
    let raw_info = Metainfo::info_bytes_with_limits(&outer, BEP9_METAINFO_LIMITS)?.to_vec();
    let mut incoming_config =
        IncomingPeerServiceConfig::new(IncomingTcpBootstrap::FixedLoopback(arguments.listen_port))
            .with_peer_budget(peer_budget.clone());
    incoming_config.peer_id = arguments.peer_id;
    let service = IncomingPeerService::bind(incoming_config)
        .await?
        .ok_or("fixed incoming service was disabled")?;
    let incoming = service.handle();
    let token = incoming
        .register(SeedRegistration::new(
            raw_info,
            content,
            torrent_peers.clone(),
        )?)
        .await?;
    println!(
        "{}",
        serde_json::json!({
            "event": "ready",
            "listen": service.listen_address(),
            "peer_id": String::from_utf8_lossy(&arguments.peer_id),
        })
    );
    std::io::stdout().flush()?;

    let control = DownloadControl::new();
    let task_control = control.clone();
    let download = tokio::spawn(download_verified_piece_with_peer_state(
        DownloadConfig {
            identity: TorrentIdentityContext::v1(torrent_id, V1InfoHash::new(metainfo.info_hash)),
            metainfo_path: arguments.metainfo,
            peer: arguments.peer,
            output_path: arguments.output,
            network: NetworkConfig::new(
                NetworkPolicy::LoopbackOnly,
                std::time::Duration::from_secs(10),
                std::time::Duration::from_secs(10),
            )
            .with_peer_id(arguments.peer_id),
            resource_limits: DownloadResourceLimits::DESKTOP,
            skip_files: Vec::new(),
            high_priority_files: Vec::new(),
        },
        task_control,
        peer_budget,
        torrent_peers.clone(),
    ));
    tokio::task::spawn_blocking(|| {
        let mut command = String::new();
        std::io::stdin().read_line(&mut command)
    })
    .await??;
    control.cancel();
    let download = download.await?;
    let live = service.snapshot();
    incoming.unregister(token).await?;
    let terminal = service.shutdown().await?;
    let history = sink
        .connections
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let duplicate_connections = history
        .iter()
        .flatten()
        .filter(|peer| {
            peer.close_reason == Some(rstorrent_engine::peer::PeerFailure::DuplicatePeerId)
        })
        .map(|peer| peer.connection_id)
        .collect::<BTreeSet<_>>()
        .len();
    let outgoing_winners = history
        .iter()
        .filter(|peers| {
            peers.len() == 1
                && peers[0].direction == rstorrent_engine::PeerConnectionDirection::Outgoing
                && peers[0].lifecycle == rstorrent_engine::PeerConnectionLifecycle::Connected
        })
        .count();
    let incoming_winners = history
        .iter()
        .filter(|peers| {
            peers.len() == 1
                && peers[0].direction == rstorrent_engine::PeerConnectionDirection::Incoming
                && peers[0].lifecycle == rstorrent_engine::PeerConnectionLifecycle::Connected
        })
        .count();
    println!(
        "{}",
        serde_json::json!({
            "event": "complete",
            "download_completed": download.is_ok(),
            "live_established": live.established,
            "connection_high_water": live.peer_budget.total_high_water,
            "duplicate_connections": duplicate_connections,
            "outgoing_winner_observed": outgoing_winners > 0,
            "incoming_winner_observed": incoming_winners > 0,
            "terminal_pending": terminal.pending,
            "terminal_established": terminal.established,
            "terminal_connections": torrent_peers.connection_snapshot().len(),
        })
    );
    Ok(())
}

#[derive(Debug)]
struct Arguments {
    metainfo: PathBuf,
    seed_root: PathBuf,
    output: PathBuf,
    peer: SocketAddr,
    listen_port: u16,
    peer_id: [u8; 20],
}

impl Arguments {
    fn parse(arguments: impl Iterator<Item = String>) -> Result<Self, Box<dyn Error>> {
        let mut metainfo = None;
        let mut seed_root = None;
        let mut output = None;
        let mut peer = None;
        let mut listen_port = None;
        let mut peer_id = None;
        let arguments = arguments.collect::<Vec<_>>();
        let mut index = 0;
        while index < arguments.len() {
            let flag = &arguments[index];
            let value = arguments
                .get(index + 1)
                .ok_or_else(|| format!("{flag} requires a value"))?;
            match flag.as_str() {
                "--metainfo" => metainfo = Some(PathBuf::from(value)),
                "--seed-root" => seed_root = Some(PathBuf::from(value)),
                "--output" => output = Some(PathBuf::from(value)),
                "--peer" => peer = Some(value.parse()?),
                "--listen-port" => listen_port = Some(value.parse()?),
                "--peer-id" => {
                    peer_id = Some(
                        value
                            .as_bytes()
                            .try_into()
                            .map_err(|_| "--peer-id must be exactly 20 bytes")?,
                    )
                }
                _ => return Err(format!("unknown flag {flag}").into()),
            }
            index += 2;
        }
        Ok(Self {
            metainfo: metainfo.ok_or("missing --metainfo")?,
            seed_root: seed_root.ok_or("missing --seed-root")?,
            output: output.ok_or("missing --output")?,
            peer: peer.ok_or("missing --peer")?,
            listen_port: listen_port.ok_or("missing --listen-port")?,
            peer_id: peer_id.ok_or("missing --peer-id")?,
        })
    }
}
