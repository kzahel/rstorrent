//! Session-owned, bounded Mainline DHT runtime.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::error::Error;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use rstorrent_protocol::dht::{
    ALPHA, DhtEndpoint, DhtIp, K, MAX_DATAGRAM_SIZE, Message, NodeContact, NodeId, Query,
    ResponseMessage, RoutingBucketInspection, RoutingTable, Want, decode_message, encode_error,
    encode_query, encode_response, generate_bep42_id, verify_bep42_id,
};
use sha1::{Digest, Sha1};
use tokio::net::{UdpSocket, lookup_host};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior, interval};
use tokio_util::sync::CancellationToken;

use crate::network::{NetworkPolicy, is_valid_outbound_address};
use crate::{ByteMetric, ByteMetricSink};

pub const DHT_SNAPSHOT_VERSION: u32 = 1;
pub const MAX_PERSISTED_NODES_PER_FAMILY: usize = 64;
pub const MAX_ACTIVE_TRANSACTIONS: usize = 256;
pub const MAX_ACTIVE_LOOKUPS: usize = 16;
pub const MAX_LOOKUP_CANDIDATES: usize = 256;
pub const MAX_LOOKUP_PEERS: usize = 200;
pub const MAX_PEER_STORE_HASHES: usize = 256;
pub const MAX_PEERS_PER_HASH: usize = 100;
pub const MAX_RATE_SOURCES: usize = 1024;
pub const MAX_QUERIES_PER_SOURCE_MINUTE: u16 = 30;
pub const MAX_GLOBAL_QUERIES_PER_SECOND: u16 = 250;
pub const DHT_COMMAND_QUEUE: usize = 64;
pub const DEFAULT_QUERY_TIMEOUT: Duration = Duration::from_secs(5);
pub const DEFAULT_LOOKUP_TIMEOUT: Duration = Duration::from_secs(30);
pub const DEFAULT_BOOTSTRAP_RETRY_INTERVAL: Duration = Duration::from_secs(60);
pub const DEFAULT_ROUTING_REFRESH_INTERVAL: Duration = Duration::from_secs(15 * 60);
pub const DHT_OBSERVATION_INTERVAL: Duration = Duration::from_millis(500);
const PEER_TTL: Duration = Duration::from_secs(30 * 60);
const TOKEN_ROTATION: Duration = Duration::from_secs(5 * 60);
const EXTERNAL_ADDRESS_VOTES: usize = 3;
const MAX_EXTERNAL_ADDRESS_CANDIDATES: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BootstrapNode {
    Address(SocketAddr),
    Host { host: String, port: u16 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DhtSnapshot {
    pub version: u32,
    pub node_id: NodeId,
    pub nodes_v4: Vec<NodeContact>,
    pub nodes_v6: Vec<NodeContact>,
}

impl DhtSnapshot {
    pub fn validate(mut self) -> Result<Self, DhtError> {
        if self.version != DHT_SNAPSHOT_VERSION {
            return Err(DhtError::UnsupportedSnapshotVersion(self.version));
        }
        if self.node_id == NodeId::ZERO {
            return Err(DhtError::InvalidSnapshot("zero node ID"));
        }
        if self.nodes_v4.len() > MAX_PERSISTED_NODES_PER_FAMILY
            || self.nodes_v6.len() > MAX_PERSISTED_NODES_PER_FAMILY
        {
            return Err(DhtError::InvalidSnapshot("too many saved nodes"));
        }
        self.nodes_v4
            .retain(|node| node.address.is_ipv4() && valid_node_contact(*node));
        self.nodes_v6
            .retain(|node| node.address.is_ipv6() && valid_node_contact(*node));
        deduplicate_contacts(&mut self.nodes_v4);
        deduplicate_contacts(&mut self.nodes_v6);
        Ok(self)
    }
}

#[derive(Clone, Debug)]
pub struct DhtConfig {
    pub network_policy: NetworkPolicy,
    pub bind_address: SocketAddr,
    pub bootstrap_nodes: Vec<BootstrapNode>,
    pub initial_snapshot: Option<DhtSnapshot>,
    pub query_timeout: Duration,
    pub lookup_timeout: Duration,
    pub bootstrap_retry_interval: Duration,
    pub routing_refresh_interval: Duration,
    pub read_only: bool,
    pub byte_metric_sink: Option<Arc<dyn ByteMetricSink>>,
}

impl DhtConfig {
    pub fn for_network(network_policy: NetworkPolicy) -> Self {
        let bind_address = if matches!(network_policy, NetworkPolicy::Online) {
            SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))
        } else {
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0))
        };
        Self {
            network_policy,
            bind_address,
            bootstrap_nodes: vec![
                BootstrapNode::Host {
                    host: "dht.libtorrent.org".to_owned(),
                    port: 25401,
                },
                BootstrapNode::Host {
                    host: "router.bittorrent.com".to_owned(),
                    port: 6881,
                },
                BootstrapNode::Host {
                    host: "dht.transmissionbt.com".to_owned(),
                    port: 6881,
                },
            ],
            initial_snapshot: None,
            query_timeout: DEFAULT_QUERY_TIMEOUT,
            lookup_timeout: DEFAULT_LOOKUP_TIMEOUT,
            bootstrap_retry_interval: DEFAULT_BOOTSTRAP_RETRY_INTERVAL,
            routing_refresh_interval: DEFAULT_ROUTING_REFRESH_INTERVAL,
            read_only: false,
            byte_metric_sink: None,
        }
    }

    fn validate(&self) -> Result<(), DhtError> {
        if !self.bind_address.is_ipv4() {
            return Err(DhtError::Configuration(
                "initial DHT runtime requires an IPv4 bind address",
            ));
        }
        if self.bind_address.port() != 0 || self.bind_address.ip().is_multicast() {
            return Err(DhtError::Configuration(
                "DHT bind address must use an ephemeral non-multicast port",
            ));
        }
        if self.query_timeout.is_zero() || self.lookup_timeout < self.query_timeout {
            return Err(DhtError::Configuration(
                "DHT timeouts must be nonzero and lookup timeout must cover a query",
            ));
        }
        if self.bootstrap_retry_interval.is_zero() || self.routing_refresh_interval.is_zero() {
            return Err(DhtError::Configuration(
                "DHT bootstrap and refresh intervals must be nonzero",
            ));
        }
        if self.bootstrap_nodes.len() > 64 {
            return Err(DhtError::Configuration("too many DHT bootstrap nodes"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DhtStats {
    pub routing_nodes_v4: u32,
    pub active_transactions: u32,
    pub active_lookups: u32,
    pub queries_sent: u64,
    pub responses_received: u64,
    pub queries_received: u64,
    pub malformed_received: u64,
    pub rate_limited: u64,
    pub discovered_peers: u64,
    pub bootstrap_attempts: u64,
    pub routing_refreshes: u64,
    pub datagram_bytes_sent: u64,
    pub datagram_bytes_received: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DhtLifecycle {
    Offline,
    BootstrapEmpty,
    Participating,
    Inactive,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DhtLookupObservation {
    pub lookup_id: u64,
    pub target: NodeId,
    pub age_millis: u64,
    pub deadline_in_millis: u64,
    pub unqueried_candidates: u16,
    pub in_flight_candidates: u16,
    pub responded_candidates: u16,
    pub failed_candidates: u16,
    pub discovered_peers: u16,
    pub closest_responded_prefix_bits: Option<u16>,
    pub last_convergence_improvement_age_millis: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DhtObservation {
    pub lifecycle: DhtLifecycle,
    pub network_policy: NetworkPolicy,
    pub local_node_id: NodeId,
    pub captured_millis: u64,
    pub routing_nodes_v4: u16,
    pub occupied_buckets_v4: u16,
    pub deepest_shared_prefix_bits_v4: Option<u16>,
    pub stats: DhtStats,
    pub buckets_v4: Vec<RoutingBucketInspection>,
    pub lookups: Vec<DhtLookupObservation>,
}

impl DhtObservation {
    fn initial(network_policy: NetworkPolicy, local_node_id: NodeId) -> Self {
        let routing = RoutingTable::new(local_node_id).inspection(0);
        Self {
            lifecycle: if matches!(network_policy, NetworkPolicy::Offline) {
                DhtLifecycle::Offline
            } else {
                DhtLifecycle::BootstrapEmpty
            },
            network_policy,
            local_node_id,
            captured_millis: 0,
            routing_nodes_v4: routing.routing_nodes,
            occupied_buckets_v4: routing.occupied_buckets,
            deepest_shared_prefix_bits_v4: routing.deepest_shared_prefix_bits,
            stats: DhtStats::default(),
            buckets_v4: routing.buckets,
            lookups: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DhtError {
    Configuration(&'static str),
    Io(String),
    ActorStopped,
    NetworkDisabled,
    LookupCapacity,
    LookupTimedOut,
    NoReachableNodes,
    Cancelled,
    UnsupportedSnapshotVersion(u32),
    InvalidSnapshot(&'static str),
}

impl fmt::Display for DhtError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(message) => {
                write!(formatter, "invalid DHT configuration: {message}")
            }
            Self::Io(message) => write!(formatter, "DHT I/O failed: {message}"),
            Self::ActorStopped => write!(formatter, "DHT owner stopped unexpectedly"),
            Self::NetworkDisabled => write!(formatter, "DHT networking is disabled"),
            Self::LookupCapacity => write!(formatter, "DHT lookup capacity is exhausted"),
            Self::LookupTimedOut => write!(formatter, "DHT peer lookup timed out"),
            Self::NoReachableNodes => write!(formatter, "DHT has no reachable nodes"),
            Self::Cancelled => write!(formatter, "DHT lookup was cancelled"),
            Self::UnsupportedSnapshotVersion(version) => {
                write!(formatter, "unsupported DHT snapshot version {version}")
            }
            Self::InvalidSnapshot(message) => write!(formatter, "invalid DHT snapshot: {message}"),
        }
    }
}

impl Error for DhtError {}

#[derive(Clone, Debug)]
pub struct DhtHandle {
    sender: mpsc::Sender<Command>,
}

impl DhtHandle {
    pub async fn lookup(&self, info_hash: [u8; 20]) -> Result<Vec<SocketAddr>, DhtError> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::Lookup {
                info_hash: NodeId(info_hash),
                result: sender,
            })
            .await
            .map_err(|_| DhtError::ActorStopped)?;
        receiver.await.map_err(|_| DhtError::ActorStopped)?
    }

    pub async fn cancel_lookup(&self, info_hash: [u8; 20]) -> Result<(), DhtError> {
        self.sender
            .send(Command::CancelLookup(NodeId(info_hash)))
            .await
            .map_err(|_| DhtError::ActorStopped)
    }

    pub async fn stats(&self) -> Result<DhtStats, DhtError> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::Stats(sender))
            .await
            .map_err(|_| DhtError::ActorStopped)?;
        receiver.await.map_err(|_| DhtError::ActorStopped)
    }
}

#[derive(Debug)]
pub struct DhtService {
    handle: DhtHandle,
    cancellation: CancellationToken,
    task: Option<JoinHandle<Result<DhtSnapshot, DhtError>>>,
    local_address: SocketAddr,
    observations: watch::Receiver<DhtObservation>,
}

impl DhtService {
    pub async fn start(mut config: DhtConfig) -> Result<Self, DhtError> {
        config.validate()?;
        let snapshot = config
            .initial_snapshot
            .take()
            .map(DhtSnapshot::validate)
            .transpose()?;
        let node_id = snapshot
            .as_ref()
            .map(|snapshot| snapshot.node_id)
            .unwrap_or(random_node_id()?);
        let socket = UdpSocket::bind(config.bind_address)
            .await
            .map_err(|error| DhtError::Io(error.to_string()))?;
        let local_address = socket
            .local_addr()
            .map_err(|error| DhtError::Io(error.to_string()))?;
        let bootstrap = resolve_bootstrap(&config, snapshot.as_ref()).await;
        let (sender, receiver) = mpsc::channel(DHT_COMMAND_QUEUE);
        let (observation_sender, observations) =
            watch::channel(DhtObservation::initial(config.network_policy, node_id));
        let cancellation = CancellationToken::new();
        let task_cancellation = cancellation.clone();
        let actor = Actor::new(
            config,
            socket,
            node_id,
            bootstrap,
            receiver,
            task_cancellation,
            observation_sender,
        )?;
        let task = tokio::spawn(async move { actor.run().await });
        Ok(Self {
            handle: DhtHandle { sender },
            cancellation,
            task: Some(task),
            local_address,
            observations,
        })
    }

    pub fn handle(&self) -> DhtHandle {
        self.handle.clone()
    }

    pub fn local_address(&self) -> SocketAddr {
        self.local_address
    }

    pub fn subscribe_observations(&self) -> watch::Receiver<DhtObservation> {
        self.observations.clone()
    }

    pub async fn shutdown(mut self) -> Result<DhtSnapshot, DhtError> {
        let (sender, receiver) = oneshot::channel();
        if self
            .handle
            .sender
            .send(Command::Shutdown(sender))
            .await
            .is_ok()
        {
            let _ = receiver.await;
        } else {
            self.cancellation.cancel();
        }
        let Some(task) = self.task.take() else {
            return Err(DhtError::ActorStopped);
        };
        task.await
            .map_err(|error| DhtError::Io(error.to_string()))?
    }
}

impl Drop for DhtService {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

#[derive(Debug)]
enum Command {
    Lookup {
        info_hash: NodeId,
        result: oneshot::Sender<Result<Vec<SocketAddr>, DhtError>>,
    },
    CancelLookup(NodeId),
    Stats(oneshot::Sender<DhtStats>),
    Shutdown(oneshot::Sender<()>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CandidateState {
    Unqueried,
    InFlight,
    Responded,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Candidate {
    contact: Option<NodeContact>,
    address: SocketAddr,
    state: CandidateState,
}

#[derive(Debug)]
struct Lookup {
    id: u64,
    target: NodeId,
    candidates: Vec<Candidate>,
    peers: BTreeSet<SocketAddr>,
    waiters: Vec<oneshot::Sender<Result<Vec<SocketAddr>, DhtError>>>,
    deadline: Instant,
    started_at: Instant,
    closest_responded_prefix_bits: Option<u16>,
    last_convergence_improvement_at: Option<Instant>,
}

impl Lookup {
    fn new(
        id: u64,
        target: NodeId,
        seeds: impl IntoIterator<Item = Candidate>,
        waiter: oneshot::Sender<Result<Vec<SocketAddr>, DhtError>>,
        deadline: Instant,
        now: Instant,
    ) -> Self {
        let mut lookup = Self {
            id,
            target,
            candidates: Vec::new(),
            peers: BTreeSet::new(),
            waiters: vec![waiter],
            deadline,
            started_at: now,
            closest_responded_prefix_bits: None,
            last_convergence_improvement_at: None,
        };
        for seed in seeds {
            lookup.add_candidate(seed.contact, seed.address);
        }
        lookup
    }

    fn add_candidate(&mut self, contact: Option<NodeContact>, address: SocketAddr) {
        if self.candidates.len() >= MAX_LOOKUP_CANDIDATES
            || self.candidates.iter().any(|candidate| {
                candidate.address == address
                    || contact.is_some_and(|contact| candidate.contact == Some(contact))
            })
        {
            return;
        }
        self.candidates.push(Candidate {
            contact,
            address,
            state: CandidateState::Unqueried,
        });
        self.sort_candidates();
    }

    fn sort_candidates(&mut self) {
        self.candidates
            .sort_by(|left, right| match (left.contact, right.contact) {
                (Some(left), Some(right)) => {
                    NodeId::compare_distance(left.id, right.id, self.target)
                }
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => left.address.cmp(&right.address),
            });
    }

    fn next_queries(&mut self) -> Vec<SocketAddr> {
        let in_flight = self
            .candidates
            .iter()
            .filter(|candidate| candidate.state == CandidateState::InFlight)
            .count();
        let slots = ALPHA.saturating_sub(in_flight);
        let mut addresses = Vec::with_capacity(slots);
        for candidate in self
            .candidates
            .iter_mut()
            .filter(|candidate| candidate.state == CandidateState::Unqueried)
            .take(slots)
        {
            candidate.state = CandidateState::InFlight;
            addresses.push(candidate.address);
        }
        addresses
    }

    fn mark(&mut self, address: SocketAddr, state: CandidateState, contact: Option<NodeContact>) {
        if let Some(candidate) = self
            .candidates
            .iter_mut()
            .find(|candidate| candidate.address == address)
        {
            candidate.state = state;
            if contact.is_some() {
                candidate.contact = contact;
            }
            if state == CandidateState::Responded
                && let Some(contact) = candidate.contact
            {
                let prefix = self.target.shared_prefix_bits(contact.id);
                if self
                    .closest_responded_prefix_bits
                    .is_none_or(|closest| prefix > closest)
                {
                    self.closest_responded_prefix_bits = Some(prefix);
                    self.last_convergence_improvement_at = Some(Instant::now());
                }
            }
        }
        self.sort_candidates();
    }

    fn observation(&self, now: Instant) -> DhtLookupObservation {
        let count = |state| {
            self.candidates
                .iter()
                .filter(|candidate| candidate.state == state)
                .count() as u16
        };
        DhtLookupObservation {
            lookup_id: self.id,
            target: self.target,
            age_millis: duration_millis(now.saturating_duration_since(self.started_at)),
            deadline_in_millis: duration_millis(self.deadline.saturating_duration_since(now)),
            unqueried_candidates: count(CandidateState::Unqueried),
            in_flight_candidates: count(CandidateState::InFlight),
            responded_candidates: count(CandidateState::Responded),
            failed_candidates: count(CandidateState::Failed),
            discovered_peers: self.peers.len() as u16,
            closest_responded_prefix_bits: self.closest_responded_prefix_bits,
            last_convergence_improvement_age_millis: self
                .last_convergence_improvement_at
                .map(|at| duration_millis(now.saturating_duration_since(at))),
        }
    }

    fn has_work(&self) -> bool {
        self.candidates.iter().any(|candidate| {
            matches!(
                candidate.state,
                CandidateState::Unqueried | CandidateState::InFlight
            )
        })
    }

    fn completed_result(&self) -> Option<Result<Vec<SocketAddr>, DhtError>> {
        if self.has_work() {
            return None;
        }
        if self.peers.is_empty() {
            Some(Err(DhtError::NoReachableNodes))
        } else {
            Some(Ok(self.peers.iter().copied().collect()))
        }
    }

    fn timeout_result(&self) -> Result<Vec<SocketAddr>, DhtError> {
        if self.peers.is_empty() {
            Err(DhtError::LookupTimedOut)
        } else {
            Ok(self.peers.iter().copied().collect())
        }
    }

    fn finish(self, result: Result<Vec<SocketAddr>, DhtError>) {
        for waiter in self.waiters {
            let _ = waiter.send(result.clone());
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TransactionOwner {
    Bootstrap(NodeId),
    Lookup(NodeId),
}

#[derive(Clone, Copy, Debug)]
struct Transaction {
    endpoint: SocketAddr,
    contact: Option<NodeContact>,
    owner: TransactionOwner,
    deadline: Instant,
}

#[derive(Clone, Copy, Debug)]
struct StoredPeer {
    address: SocketAddr,
    expires_at: Instant,
}

#[derive(Clone, Copy, Debug)]
struct RateWindow {
    started: Instant,
    count: u16,
}

#[derive(Debug, Default)]
struct ExternalAddressVotes {
    candidates: HashMap<IpAddr, HashSet<IpAddr>>,
}

impl ExternalAddressVotes {
    fn observe(&mut self, address: IpAddr, voter: IpAddr) -> bool {
        if !self.candidates.contains_key(&address)
            && self.candidates.len() == MAX_EXTERNAL_ADDRESS_CANDIDATES
        {
            self.candidates.clear();
        }
        let voters = self.candidates.entry(address).or_default();
        voters.insert(voter);
        if voters.len() < EXTERNAL_ADDRESS_VOTES {
            return false;
        }
        self.candidates.clear();
        true
    }

    fn clear(&mut self) {
        self.candidates.clear();
    }
}

#[derive(Debug)]
struct Tokens {
    current: [u8; 20],
    previous: [u8; 20],
    rotated_at: Instant,
}

impl Tokens {
    fn new(now: Instant) -> Result<Self, DhtError> {
        Ok(Self {
            current: random_bytes()?,
            previous: random_bytes()?,
            rotated_at: now,
        })
    }

    fn rotate_if_due(&mut self, now: Instant) -> Result<(), DhtError> {
        if now.duration_since(self.rotated_at) >= TOKEN_ROTATION {
            self.previous = self.current;
            self.current = random_bytes()?;
            self.rotated_at = now;
        }
        Ok(())
    }

    fn generate(&self, source: IpAddr, info_hash: NodeId) -> Vec<u8> {
        token_digest(source, &self.current, info_hash)[..8].to_vec()
    }

    fn verify(&self, token: &[u8], source: IpAddr, info_hash: NodeId) -> bool {
        if token.len() != 8 {
            return false;
        }
        [self.current, self.previous]
            .iter()
            .any(|secret| token_digest(source, secret, info_hash)[..8] == *token)
    }
}

#[derive(Debug)]
struct Actor {
    config: DhtConfig,
    socket: UdpSocket,
    node_id: NodeId,
    started: Instant,
    routing_v4: RoutingTable,
    routing_v6: RoutingTable,
    warm_bootstrap: Vec<SocketAddr>,
    fallback_bootstrap: Vec<SocketAddr>,
    fallback_pending: bool,
    bootstrap_queried: HashSet<SocketAddr>,
    last_bootstrap: Instant,
    last_refresh: Instant,
    transactions: HashMap<u16, Transaction>,
    lookups: HashMap<NodeId, Lookup>,
    peer_store: HashMap<NodeId, Vec<StoredPeer>>,
    source_rates: HashMap<IpAddr, RateWindow>,
    global_rate: RateWindow,
    tokens: Tokens,
    external_votes: ExternalAddressVotes,
    commands: mpsc::Receiver<Command>,
    cancellation: CancellationToken,
    stats: DhtStats,
    observations: watch::Sender<DhtObservation>,
    last_observation: Instant,
    next_lookup_id: u64,
}

impl Actor {
    fn new(
        config: DhtConfig,
        socket: UdpSocket,
        node_id: NodeId,
        bootstrap: ResolvedBootstrap,
        commands: mpsc::Receiver<Command>,
        cancellation: CancellationToken,
        observations: watch::Sender<DhtObservation>,
    ) -> Result<Self, DhtError> {
        let now = Instant::now();
        let tokens = Tokens::new(now)?;
        Ok(Self {
            config,
            socket,
            node_id,
            started: now,
            routing_v4: RoutingTable::new(node_id),
            routing_v6: RoutingTable::new(node_id),
            warm_bootstrap: bootstrap.warm,
            fallback_bootstrap: bootstrap.fallback,
            fallback_pending: false,
            bootstrap_queried: HashSet::new(),
            last_bootstrap: now,
            last_refresh: now,
            transactions: HashMap::new(),
            lookups: HashMap::new(),
            peer_store: HashMap::new(),
            source_rates: HashMap::new(),
            global_rate: RateWindow {
                started: now,
                count: 0,
            },
            tokens,
            external_votes: ExternalAddressVotes::default(),
            commands,
            cancellation,
            stats: DhtStats::default(),
            observations,
            last_observation: now,
            next_lookup_id: 1,
        })
    }

    async fn run(mut self) -> Result<DhtSnapshot, DhtError> {
        let result = self.run_active().await;
        if result.is_err() {
            self.cancel_all(DhtError::ActorStopped);
        }
        self.publish_observation(Some(DhtLifecycle::Inactive));
        result.map(|()| self.snapshot())
    }

    async fn run_active(&mut self) -> Result<(), DhtError> {
        let _ = self.bootstrap().await;
        self.publish_observation(None);
        let mut receive_buffer = [0_u8; MAX_DATAGRAM_SIZE + 1];
        let mut maintenance = interval(Duration::from_millis(200));
        maintenance.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                biased;
                _ = self.cancellation.cancelled() => {
                    self.cancel_all(DhtError::Cancelled);
                    return Ok(());
                }
                command = self.commands.recv() => {
                    match command {
                        Some(Command::Shutdown(acknowledge)) => {
                            self.cancel_all(DhtError::Cancelled);
                            let _ = acknowledge.send(());
                            return Ok(());
                        }
                        Some(command) => self.handle_command(command).await?,
                        None => {
                            self.cancel_all(DhtError::ActorStopped);
                            return Ok(());
                        }
                    }
                }
                received = self.socket.recv_from(&mut receive_buffer) => {
                    match received {
                        Ok((length, source)) => {
                            self.stats.datagram_bytes_received = self
                                .stats
                                .datagram_bytes_received
                                .saturating_add(length as u64);
                            if let Some(sink) = &self.config.byte_metric_sink {
                                sink.record(ByteMetric::DhtReceived, length as u64);
                            }
                            if length <= MAX_DATAGRAM_SIZE {
                                self.handle_datagram(&receive_buffer[..length], source).await?;
                            } else {
                                self.stats.malformed_received =
                                    self.stats.malformed_received.saturating_add(1);
                            }
                        }
                        Err(error) => return Err(DhtError::Io(error.to_string())),
                    }
                }
                _ = maintenance.tick() => {
                    self.maintain().await?;
                    if Instant::now().saturating_duration_since(self.last_observation)
                        >= DHT_OBSERVATION_INTERVAL
                    {
                        self.publish_observation(None);
                    }
                },
            }
        }
    }

    async fn bootstrap(&mut self) -> Result<(), DhtError> {
        if matches!(self.config.network_policy, NetworkPolicy::Offline) {
            return Ok(());
        }
        let nodes = if self.warm_bootstrap.is_empty() {
            self.fallback_pending = false;
            self.fallback_bootstrap.clone()
        } else {
            self.fallback_pending = !self.fallback_bootstrap.is_empty();
            self.warm_bootstrap.clone()
        };
        self.start_bootstrap(nodes, self.node_id).await
    }

    async fn bootstrap_fallback(&mut self) -> Result<(), DhtError> {
        self.fallback_pending = false;
        self.start_bootstrap(self.fallback_bootstrap.clone(), self.node_id)
            .await
    }

    async fn start_bootstrap(
        &mut self,
        nodes: Vec<SocketAddr>,
        target: NodeId,
    ) -> Result<(), DhtError> {
        self.bootstrap_queried.clear();
        self.last_bootstrap = Instant::now();
        self.stats.bootstrap_attempts = self.stats.bootstrap_attempts.saturating_add(1);
        for endpoint in nodes {
            if self.transactions.len() == MAX_ACTIVE_TRANSACTIONS {
                break;
            }
            if !self.bootstrap_queried.insert(endpoint) {
                continue;
            }
            let query = Query::FindNode {
                target,
                want: vec![Want::Ipv4],
            };
            let _ = self
                .send_query(endpoint, None, query, TransactionOwner::Bootstrap(target))
                .await;
        }
        Ok(())
    }

    async fn refresh(&mut self) -> Result<(), DhtError> {
        let target = random_node_id()?;
        let contacts = self
            .routing_v4
            .closest(target, K, self.started.elapsed().as_secs());
        if contacts.is_empty() {
            return self.bootstrap().await;
        }
        self.bootstrap_queried.clear();
        self.last_refresh = Instant::now();
        self.stats.routing_refreshes = self.stats.routing_refreshes.saturating_add(1);
        for contact in contacts.into_iter().take(ALPHA) {
            let endpoint = socket_endpoint(contact.address);
            self.bootstrap_queried.insert(endpoint);
            let query = Query::FindNode {
                target,
                want: vec![Want::Ipv4],
            };
            let _ = self
                .send_query(
                    endpoint,
                    Some(contact),
                    query,
                    TransactionOwner::Bootstrap(target),
                )
                .await;
        }
        Ok(())
    }

    async fn handle_command(&mut self, command: Command) -> Result<(), DhtError> {
        match command {
            Command::Lookup { info_hash, result } => {
                if matches!(self.config.network_policy, NetworkPolicy::Offline) {
                    let _ = result.send(Err(DhtError::NetworkDisabled));
                    return Ok(());
                }
                if let Some(lookup) = self.lookups.get_mut(&info_hash) {
                    lookup.waiters.push(result);
                    return Ok(());
                }
                if self.lookups.len() == MAX_ACTIVE_LOOKUPS {
                    let _ = result.send(Err(DhtError::LookupCapacity));
                    return Ok(());
                }
                let elapsed = self.started.elapsed().as_secs();
                let mut seeds = self
                    .routing_v4
                    .closest(info_hash, K, elapsed)
                    .into_iter()
                    .map(|contact| Candidate {
                        contact: Some(contact),
                        address: socket_endpoint(contact.address),
                        state: CandidateState::Unqueried,
                    })
                    .collect::<Vec<_>>();
                let bootstrap = if self.fallback_pending
                    || (self.fallback_bootstrap.is_empty() && !self.warm_bootstrap.is_empty())
                {
                    &self.warm_bootstrap
                } else {
                    &self.fallback_bootstrap
                };
                for address in bootstrap {
                    seeds.push(Candidate {
                        contact: None,
                        address: *address,
                        state: CandidateState::Unqueried,
                    });
                }
                let now = Instant::now();
                self.lookups.insert(
                    info_hash,
                    Lookup::new(
                        self.next_lookup_id,
                        info_hash,
                        seeds,
                        result,
                        now + self.config.lookup_timeout,
                        now,
                    ),
                );
                self.next_lookup_id = self.next_lookup_id.checked_add(1).unwrap_or(1);
                self.fill_lookup(info_hash).await?;
                if let Some(result) = self
                    .lookups
                    .get(&info_hash)
                    .and_then(Lookup::completed_result)
                {
                    self.finish_lookup(info_hash, result);
                }
            }
            Command::CancelLookup(info_hash) => {
                self.finish_lookup(info_hash, Err(DhtError::Cancelled));
                self.transactions.retain(|_, transaction| {
                    transaction.owner != TransactionOwner::Lookup(info_hash)
                });
            }
            Command::Stats(sender) => {
                let mut stats = self.stats;
                stats.routing_nodes_v4 = self.routing_v4.len().try_into().unwrap_or(u32::MAX);
                stats.active_transactions = self.transactions.len().try_into().unwrap_or(u32::MAX);
                stats.active_lookups = self.lookups.len().try_into().unwrap_or(u32::MAX);
                let _ = sender.send(stats);
            }
            Command::Shutdown(_) => unreachable!("shutdown handled by run loop"),
        }
        Ok(())
    }

    async fn fill_lookup(&mut self, info_hash: NodeId) -> Result<(), DhtError> {
        let addresses = self
            .lookups
            .get_mut(&info_hash)
            .map(Lookup::next_queries)
            .unwrap_or_default();
        for address in addresses {
            if self.transactions.len() == MAX_ACTIVE_TRANSACTIONS {
                if let Some(lookup) = self.lookups.get_mut(&info_hash) {
                    lookup.mark(address, CandidateState::Unqueried, None);
                }
                break;
            }
            let contact = self
                .lookups
                .get(&info_hash)
                .and_then(|lookup| {
                    lookup
                        .candidates
                        .iter()
                        .find(|candidate| candidate.address == address)
                })
                .and_then(|candidate| candidate.contact);
            let query = Query::GetPeers {
                info_hash,
                want: vec![Want::Ipv4],
            };
            if self
                .send_query(address, contact, query, TransactionOwner::Lookup(info_hash))
                .await
                .is_err()
                && let Some(lookup) = self.lookups.get_mut(&info_hash)
            {
                lookup.mark(address, CandidateState::Failed, contact);
            }
        }
        Ok(())
    }

    async fn send_query(
        &mut self,
        endpoint: SocketAddr,
        contact: Option<NodeContact>,
        query: Query,
        owner: TransactionOwner,
    ) -> Result<(), DhtError> {
        if !self.config.network_policy.allows(endpoint) || !endpoint.is_ipv4() {
            return Err(DhtError::NoReachableNodes);
        }
        if self.transactions.len() >= MAX_ACTIVE_TRANSACTIONS {
            return Err(DhtError::LookupCapacity);
        }
        let transaction_id = self.allocate_transaction_id()?;
        let transaction_bytes = transaction_id.to_be_bytes();
        let bytes = encode_query(
            &transaction_bytes,
            self.node_id,
            &query,
            self.config.read_only,
        )
        .map_err(|error| DhtError::Io(error.to_string()))?;
        let sent = self
            .socket
            .send_to(&bytes, endpoint)
            .await
            .map_err(|error| DhtError::Io(error.to_string()))?;
        if let Some(sink) = &self.config.byte_metric_sink {
            sink.record(ByteMetric::DhtSent, sent as u64);
        }
        self.stats.datagram_bytes_sent = self.stats.datagram_bytes_sent.saturating_add(sent as u64);
        self.transactions.insert(
            transaction_id,
            Transaction {
                endpoint,
                contact,
                owner,
                deadline: Instant::now() + self.config.query_timeout,
            },
        );
        self.stats.queries_sent = self.stats.queries_sent.saturating_add(1);
        Ok(())
    }

    fn allocate_transaction_id(&mut self) -> Result<u16, DhtError> {
        for _ in 0..=u16::MAX {
            let mut bytes = [0_u8; 2];
            getrandom::fill(&mut bytes).map_err(|error| DhtError::Io(error.to_string()))?;
            let transaction = u16::from_be_bytes(bytes);
            if !self.transactions.contains_key(&transaction) {
                return Ok(transaction);
            }
        }
        Err(DhtError::LookupCapacity)
    }

    async fn handle_datagram(&mut self, bytes: &[u8], source: SocketAddr) -> Result<(), DhtError> {
        if !self.config.network_policy.allows(source) || !source.is_ipv4() {
            return Ok(());
        }
        let message = match decode_message(bytes) {
            Ok(message) => message,
            Err(_) => {
                self.stats.malformed_received = self.stats.malformed_received.saturating_add(1);
                return Ok(());
            }
        };
        match message {
            Message::Response(response) => self.handle_response(response, source).await,
            Message::Error(error) => {
                self.handle_error(&error.transaction, error.observed_address, source)
                    .await
            }
            Message::Query(query) => self.handle_incoming_query(query, source).await,
        }
    }

    async fn handle_response(
        &mut self,
        response: ResponseMessage,
        source: SocketAddr,
    ) -> Result<(), DhtError> {
        let ResponseMessage {
            transaction,
            id,
            mut nodes,
            mut nodes6,
            peers,
            observed_address,
            ..
        } = response;
        let Some(transaction_id) = decode_transaction_id(&transaction) else {
            return Ok(());
        };
        let Some(transaction) = self.transactions.get(&transaction_id).copied() else {
            return Ok(());
        };
        if transaction.endpoint != source || !verify_bep42_id(id, dht_ip(source.ip())) {
            return Ok(());
        }
        self.transactions.remove(&transaction_id);
        self.observe_external(observed_address, source)?;
        nodes.retain(|node| {
            valid_node_contact(*node)
                && self
                    .config
                    .network_policy
                    .allows(socket_endpoint(node.address))
        });
        nodes6.retain(|node| {
            valid_node_contact(*node)
                && self
                    .config
                    .network_policy
                    .allows(socket_endpoint(node.address))
        });
        let contact = NodeContact {
            id,
            address: dht_endpoint(source),
        };
        self.routing_v4
            .record_response(contact, self.started.elapsed().as_secs());
        self.fallback_pending = false;
        for node in &nodes {
            self.routing_v4
                .heard_about(*node, self.started.elapsed().as_secs());
        }
        for node in &nodes6 {
            self.routing_v6
                .heard_about(*node, self.started.elapsed().as_secs());
        }
        self.stats.responses_received = self.stats.responses_received.saturating_add(1);
        match transaction.owner {
            TransactionOwner::Bootstrap(target) => {
                for node in nodes.into_iter().take(ALPHA) {
                    let endpoint = socket_endpoint(node.address);
                    if self.transactions.len() >= MAX_ACTIVE_TRANSACTIONS
                        || self.bootstrap_queried.len() >= MAX_LOOKUP_CANDIDATES
                    {
                        break;
                    }
                    if !self.bootstrap_queried.insert(endpoint) {
                        continue;
                    }
                    let query = Query::FindNode {
                        target,
                        want: vec![Want::Ipv4],
                    };
                    let _ = self
                        .send_query(
                            endpoint,
                            Some(node),
                            query,
                            TransactionOwner::Bootstrap(target),
                        )
                        .await;
                }
            }
            TransactionOwner::Lookup(info_hash) => {
                if let Some(lookup) = self.lookups.get_mut(&info_hash) {
                    lookup.mark(source, CandidateState::Responded, Some(contact));
                    for node in nodes {
                        lookup.add_candidate(Some(node), socket_endpoint(node.address));
                    }
                    for peer in peers.into_iter().map(socket_endpoint) {
                        if lookup.peers.len() == MAX_LOOKUP_PEERS {
                            break;
                        }
                        if self.config.network_policy.allows(peer) {
                            lookup.peers.insert(peer);
                        }
                    }
                }
                self.fill_lookup(info_hash).await?;
                if let Some(result) = self
                    .lookups
                    .get(&info_hash)
                    .and_then(Lookup::completed_result)
                {
                    self.finish_lookup(info_hash, result);
                }
            }
        }
        Ok(())
    }

    async fn handle_error(
        &mut self,
        transaction: &[u8],
        observed_address: Option<DhtEndpoint>,
        source: SocketAddr,
    ) -> Result<(), DhtError> {
        let Some(transaction) =
            take_correlated_transaction(&mut self.transactions, transaction, source)
        else {
            return Ok(());
        };
        self.observe_external(observed_address, source)?;
        self.fail_transaction(transaction).await
    }

    async fn handle_incoming_query(
        &mut self,
        query: rstorrent_protocol::dht::QueryMessage,
        source: SocketAddr,
    ) -> Result<(), DhtError> {
        self.stats.queries_received = self.stats.queries_received.saturating_add(1);
        if self.config.read_only || !self.allow_query(source.ip()) {
            return Ok(());
        }
        if !query.read_only && verify_bep42_id(query.id, dht_ip(source.ip())) {
            self.routing_v4.heard_about(
                NodeContact {
                    id: query.id,
                    address: dht_endpoint(source),
                },
                self.started.elapsed().as_secs(),
            );
        }
        let observed_address = dht_endpoint(source);
        let result = match query.query {
            Query::Ping => encode_response(
                &query.transaction,
                self.node_id,
                &[],
                &[],
                None,
                observed_address,
            ),
            Query::FindNode { target, want } => {
                let nodes = self.response_nodes(target, &want);
                encode_response(
                    &query.transaction,
                    self.node_id,
                    &nodes,
                    &[],
                    None,
                    observed_address,
                )
            }
            Query::GetPeers { info_hash, want } => {
                let nodes = self.response_nodes(info_hash, &want);
                let peers = self
                    .stored_peers(info_hash, source.ip())
                    .into_iter()
                    .map(dht_endpoint)
                    .collect::<Vec<_>>();
                let token = self.tokens.generate(source.ip(), info_hash);
                encode_response(
                    &query.transaction,
                    self.node_id,
                    &nodes,
                    &peers,
                    Some(&token),
                    observed_address,
                )
            }
            Query::AnnouncePeer {
                info_hash,
                port,
                implied_port,
                token,
            } => {
                if !self.tokens.verify(&token, source.ip(), info_hash) {
                    encode_error(&query.transaction, 203, b"invalid token", observed_address)
                } else {
                    let address = SocketAddr::new(
                        source.ip(),
                        if implied_port { source.port() } else { port },
                    );
                    if !is_valid_outbound_address(address) {
                        encode_error(&query.transaction, 203, b"invalid port", observed_address)
                    } else {
                        self.store_peer(info_hash, address);
                        encode_response(
                            &query.transaction,
                            self.node_id,
                            &[],
                            &[],
                            None,
                            observed_address,
                        )
                    }
                }
            }
            Query::Unknown(_) => {
                encode_error(&query.transaction, 204, b"method unknown", observed_address)
            }
        };
        if let Ok(bytes) = result
            && let Ok(sent) = self.socket.send_to(&bytes, source).await
        {
            self.stats.datagram_bytes_sent =
                self.stats.datagram_bytes_sent.saturating_add(sent as u64);
            if let Some(sink) = &self.config.byte_metric_sink {
                sink.record(ByteMetric::DhtSent, sent as u64);
            }
        }
        Ok(())
    }

    fn response_nodes(&self, target: NodeId, want: &[Want]) -> Vec<NodeContact> {
        if want.is_empty() || want.contains(&Want::Ipv4) {
            self.routing_v4
                .closest(target, K, self.started.elapsed().as_secs())
        } else if want.contains(&Want::Ipv6) {
            self.routing_v6
                .closest(target, K, self.started.elapsed().as_secs())
        } else {
            Vec::new()
        }
    }

    fn allow_query(&mut self, source: IpAddr) -> bool {
        let now = Instant::now();
        if now.duration_since(self.global_rate.started) >= Duration::from_secs(1) {
            self.global_rate = RateWindow {
                started: now,
                count: 0,
            };
        }
        if self.global_rate.count >= MAX_GLOBAL_QUERIES_PER_SECOND {
            self.stats.rate_limited = self.stats.rate_limited.saturating_add(1);
            return false;
        }
        self.global_rate.count += 1;

        if !self.source_rates.contains_key(&source)
            && self.source_rates.len() == MAX_RATE_SOURCES
            && let Some(oldest) = self
                .source_rates
                .iter()
                .min_by_key(|(_, rate)| rate.started)
                .map(|(source, _)| *source)
        {
            self.source_rates.remove(&oldest);
        }
        let rate = self.source_rates.entry(source).or_insert(RateWindow {
            started: now,
            count: 0,
        });
        if now.duration_since(rate.started) >= Duration::from_secs(60) {
            *rate = RateWindow {
                started: now,
                count: 0,
            };
        }
        if rate.count >= MAX_QUERIES_PER_SOURCE_MINUTE {
            self.stats.rate_limited = self.stats.rate_limited.saturating_add(1);
            return false;
        }
        rate.count += 1;
        true
    }

    fn store_peer(&mut self, info_hash: NodeId, address: SocketAddr) {
        if !self.peer_store.contains_key(&info_hash)
            && self.peer_store.len() == MAX_PEER_STORE_HASHES
            && let Some(oldest_hash) = self
                .peer_store
                .iter()
                .min_by_key(|(_, peers)| {
                    peers
                        .iter()
                        .map(|peer| peer.expires_at)
                        .min()
                        .unwrap_or_else(Instant::now)
                })
                .map(|(hash, _)| *hash)
        {
            self.peer_store.remove(&oldest_hash);
        }
        let peers = self.peer_store.entry(info_hash).or_default();
        let expires_at = Instant::now() + PEER_TTL;
        if let Some(peer) = peers.iter_mut().find(|peer| peer.address == address) {
            peer.expires_at = expires_at;
            return;
        }
        if peers.len() == MAX_PEERS_PER_HASH {
            let oldest = peers
                .iter()
                .enumerate()
                .min_by_key(|(_, peer)| peer.expires_at)
                .map(|(index, _)| index)
                .expect("full peer store has oldest peer");
            peers.swap_remove(oldest);
        }
        peers.push(StoredPeer {
            address,
            expires_at,
        });
    }

    fn stored_peers(&mut self, info_hash: NodeId, requester: IpAddr) -> Vec<SocketAddr> {
        let now = Instant::now();
        let Some(peers) = self.peer_store.get_mut(&info_hash) else {
            return Vec::new();
        };
        peers.retain(|peer| peer.expires_at > now);
        peers
            .iter()
            .map(|peer| peer.address)
            .filter(|address| {
                address.ip().is_ipv4() == requester.is_ipv4()
                    && self.config.network_policy.allows(*address)
            })
            .take(50)
            .collect()
    }

    async fn maintain(&mut self) -> Result<(), DhtError> {
        let now = Instant::now();
        self.tokens.rotate_if_due(now)?;
        self.source_rates
            .retain(|_, rate| now.duration_since(rate.started) < Duration::from_secs(2 * 60));
        self.peer_store.retain(|_, peers| {
            peers.retain(|peer| peer.expires_at > now);
            !peers.is_empty()
        });

        let expired = self
            .transactions
            .iter()
            .filter(|(_, transaction)| transaction.deadline <= now)
            .map(|(id, transaction)| (*id, *transaction))
            .collect::<Vec<_>>();
        for (id, transaction) in expired {
            self.transactions.remove(&id);
            self.fail_transaction(transaction).await?;
        }

        for lookup in self.lookups.values_mut() {
            lookup.waiters.retain(|waiter| !waiter.is_closed());
        }
        let abandoned = self
            .lookups
            .iter()
            .filter(|(_, lookup)| lookup.waiters.is_empty())
            .map(|(hash, _)| *hash)
            .collect::<Vec<_>>();
        for hash in abandoned {
            self.lookups.remove(&hash);
            self.transactions
                .retain(|_, transaction| transaction.owner != TransactionOwner::Lookup(hash));
        }

        let timed_out = self
            .lookups
            .iter()
            .filter(|(_, lookup)| lookup.deadline <= now)
            .map(|(hash, _)| *hash)
            .collect::<Vec<_>>();
        for hash in timed_out {
            let result = self
                .lookups
                .get(&hash)
                .map(Lookup::timeout_result)
                .unwrap_or(Err(DhtError::LookupTimedOut));
            self.finish_lookup(hash, result);
            self.transactions
                .retain(|_, transaction| transaction.owner != TransactionOwner::Lookup(hash));
        }

        let maintenance_in_flight = self
            .transactions
            .values()
            .any(|transaction| matches!(transaction.owner, TransactionOwner::Bootstrap(_)));
        if !maintenance_in_flight && self.routing_v4.is_empty() && self.fallback_pending {
            self.bootstrap_fallback().await?;
        } else if !maintenance_in_flight
            && self.routing_v4.is_empty()
            && now.duration_since(self.last_bootstrap) >= self.config.bootstrap_retry_interval
        {
            self.bootstrap().await?;
        } else if !maintenance_in_flight
            && !self.routing_v4.is_empty()
            && now.duration_since(self.last_refresh) >= self.config.routing_refresh_interval
        {
            self.refresh().await?;
        }
        Ok(())
    }

    async fn fail_transaction(&mut self, transaction: Transaction) -> Result<(), DhtError> {
        if let Some(contact) = transaction.contact {
            self.routing_v4.record_failure(contact);
        }
        if let TransactionOwner::Lookup(info_hash) = transaction.owner {
            if let Some(lookup) = self.lookups.get_mut(&info_hash) {
                lookup.mark(
                    transaction.endpoint,
                    CandidateState::Failed,
                    transaction.contact,
                );
            }
            self.fill_lookup(info_hash).await?;
            if let Some(result) = self
                .lookups
                .get(&info_hash)
                .and_then(Lookup::completed_result)
            {
                self.finish_lookup(info_hash, result);
            }
        }
        Ok(())
    }

    fn finish_lookup(&mut self, info_hash: NodeId, result: Result<Vec<SocketAddr>, DhtError>) {
        if let Ok(peers) = &result {
            self.stats.discovered_peers = self
                .stats
                .discovered_peers
                .saturating_add(peers.len() as u64);
        }
        if let Some(lookup) = self.lookups.remove(&info_hash) {
            lookup.finish(result);
        }
    }

    fn cancel_all(&mut self, error: DhtError) {
        self.transactions.clear();
        for (_, lookup) in self.lookups.drain() {
            lookup.finish(Err(error.clone()));
        }
    }

    fn publish_observation(&mut self, lifecycle: Option<DhtLifecycle>) {
        let now = Instant::now();
        let elapsed = self.started.elapsed();
        let routing = self.routing_v4.inspection(elapsed.as_secs());
        let mut stats = self.stats;
        stats.routing_nodes_v4 = u32::from(routing.routing_nodes);
        stats.active_transactions = self.transactions.len().try_into().unwrap_or(u32::MAX);
        stats.active_lookups = self.lookups.len().try_into().unwrap_or(u32::MAX);
        let mut lookups = self
            .lookups
            .values()
            .map(|lookup| lookup.observation(now))
            .collect::<Vec<_>>();
        lookups.sort_by_key(|lookup| lookup.lookup_id);
        self.observations.send_replace(DhtObservation {
            lifecycle: lifecycle.unwrap_or({
                if matches!(self.config.network_policy, NetworkPolicy::Offline) {
                    DhtLifecycle::Offline
                } else if routing.routing_nodes == 0 {
                    DhtLifecycle::BootstrapEmpty
                } else {
                    DhtLifecycle::Participating
                }
            }),
            network_policy: self.config.network_policy,
            local_node_id: self.node_id,
            captured_millis: duration_millis(elapsed),
            routing_nodes_v4: routing.routing_nodes,
            occupied_buckets_v4: routing.occupied_buckets,
            deepest_shared_prefix_bits_v4: routing.deepest_shared_prefix_bits,
            stats,
            buckets_v4: routing.buckets,
            lookups,
        });
        self.last_observation = now;
    }

    fn observe_external(
        &mut self,
        observed: Option<DhtEndpoint>,
        source: SocketAddr,
    ) -> Result<(), DhtError> {
        let Some(observed) = observed.filter(|address| address.is_ipv4()) else {
            return Ok(());
        };
        let observed_address = socket_endpoint(observed);
        if observed_address.ip().is_loopback() || observed_address.ip().is_unspecified() {
            return Ok(());
        }
        if verify_bep42_id(self.node_id, observed.ip) {
            self.external_votes.clear();
            return Ok(());
        }
        if !self
            .external_votes
            .observe(observed_address.ip(), source.ip())
        {
            return Ok(());
        }
        self.node_id = generate_bep42_id(observed.ip, random_bytes()?);
        self.routing_v4 = RoutingTable::new(self.node_id);
        self.routing_v6 = RoutingTable::new(self.node_id);
        self.external_votes.clear();
        Ok(())
    }

    fn snapshot(&self) -> DhtSnapshot {
        let elapsed = self.started.elapsed().as_secs();
        DhtSnapshot {
            version: DHT_SNAPSHOT_VERSION,
            node_id: self.node_id,
            nodes_v4: self.routing_v4.responsive_sample(
                false,
                MAX_PERSISTED_NODES_PER_FAMILY,
                elapsed,
            ),
            nodes_v6: self.routing_v6.responsive_sample(
                true,
                MAX_PERSISTED_NODES_PER_FAMILY,
                elapsed,
            ),
        }
    }
}

#[derive(Debug)]
struct ResolvedBootstrap {
    warm: Vec<SocketAddr>,
    fallback: Vec<SocketAddr>,
}

async fn resolve_bootstrap(
    config: &DhtConfig,
    snapshot: Option<&DhtSnapshot>,
) -> ResolvedBootstrap {
    if matches!(config.network_policy, NetworkPolicy::Offline) {
        return ResolvedBootstrap {
            warm: Vec::new(),
            fallback: Vec::new(),
        };
    }
    let mut warm = snapshot
        .into_iter()
        .flat_map(|snapshot| &snapshot.nodes_v4)
        .map(|node| socket_endpoint(node.address))
        .filter(|address| config.network_policy.allows(*address) && address.is_ipv4())
        .collect::<Vec<_>>();
    warm.sort_unstable();
    warm.dedup();
    warm.truncate(MAX_PERSISTED_NODES_PER_FAMILY);

    let mut fallback = config
        .bootstrap_nodes
        .iter()
        .filter_map(|node| match node {
            BootstrapNode::Address(address) => Some(*address),
            BootstrapNode::Host { .. } => None,
        })
        .filter(|address| config.network_policy.allows(*address) && address.is_ipv4())
        .collect::<Vec<_>>();

    if matches!(config.network_policy, NetworkPolicy::Online) {
        let mut tasks = tokio::task::JoinSet::new();
        for node in &config.bootstrap_nodes {
            let BootstrapNode::Host { host, port } = node else {
                continue;
            };
            let host = host.clone();
            let port = *port;
            tasks.spawn(async move {
                tokio::time::timeout(Duration::from_secs(5), lookup_host((host, port)))
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .map(|resolved| resolved.take(16).collect::<Vec<_>>())
                    .unwrap_or_default()
            });
        }
        while let Some(Ok(resolved)) = tasks.join_next().await {
            fallback.extend(
                resolved
                    .into_iter()
                    .filter(|address| config.network_policy.allows(*address) && address.is_ipv4()),
            );
        }
    }
    fallback.sort_unstable();
    fallback.dedup();
    fallback.retain(|address| !warm.contains(address));
    fallback.truncate(64);
    ResolvedBootstrap { warm, fallback }
}

fn random_node_id() -> Result<NodeId, DhtError> {
    let mut bytes = random_bytes()?;
    if bytes == [0; 20] {
        bytes[19] = 1;
    }
    Ok(NodeId(bytes))
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

fn random_bytes() -> Result<[u8; 20], DhtError> {
    let mut bytes = [0; 20];
    getrandom::fill(&mut bytes).map_err(|error| DhtError::Io(error.to_string()))?;
    Ok(bytes)
}

fn token_digest(source: IpAddr, secret: &[u8; 20], info_hash: NodeId) -> [u8; 20] {
    let mut hasher = Sha1::new();
    match source {
        IpAddr::V4(address) => hasher.update(address.octets()),
        IpAddr::V6(address) => hasher.update(address.octets()),
    }
    hasher.update(secret);
    hasher.update(info_hash.0);
    hasher.finalize().into()
}

fn decode_transaction_id(transaction: &[u8]) -> Option<u16> {
    Some(u16::from_be_bytes(transaction.try_into().ok()?))
}

fn take_correlated_transaction(
    transactions: &mut HashMap<u16, Transaction>,
    transaction: &[u8],
    source: SocketAddr,
) -> Option<Transaction> {
    let transaction_id = decode_transaction_id(transaction)?;
    let pending = transactions.get(&transaction_id).copied()?;
    if pending.endpoint != source {
        return None;
    }
    transactions.remove(&transaction_id)
}

fn dht_ip(address: IpAddr) -> DhtIp {
    match address {
        IpAddr::V4(address) => DhtIp::V4(address.octets()),
        IpAddr::V6(address) => DhtIp::V6(address.octets()),
    }
}

fn dht_endpoint(address: SocketAddr) -> DhtEndpoint {
    DhtEndpoint::new(dht_ip(address.ip()), address.port())
}

fn socket_endpoint(address: DhtEndpoint) -> SocketAddr {
    match address.ip {
        DhtIp::V4(octets) => SocketAddr::from((Ipv4Addr::from(octets), address.port)),
        DhtIp::V6(octets) => SocketAddr::from((Ipv6Addr::from(octets), address.port)),
    }
}

fn valid_node_contact(contact: NodeContact) -> bool {
    contact.id != NodeId::ZERO
        && is_valid_outbound_address(socket_endpoint(contact.address))
        && verify_bep42_id(contact.id, contact.address.ip)
}

fn deduplicate_contacts(contacts: &mut Vec<NodeContact>) {
    let mut ids = HashSet::new();
    let mut endpoints = HashSet::new();
    contacts.retain(|contact| ids.insert(contact.id) && endpoints.insert(contact.address));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_address_votes_are_distinct_bounded_and_consumed() {
        let mut votes = ExternalAddressVotes::default();
        let voter = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1));
        let candidate = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 1));
        assert!(!votes.observe(candidate, voter));
        assert!(!votes.observe(candidate, voter));
        assert_eq!(votes.candidates[&candidate].len(), 1);

        for suffix in 2..=MAX_EXTERNAL_ADDRESS_CANDIDATES as u8 {
            let address = IpAddr::V4(Ipv4Addr::new(198, 51, 100, suffix));
            assert!(!votes.observe(address, voter));
        }
        assert_eq!(votes.candidates.len(), MAX_EXTERNAL_ADDRESS_CANDIDATES);

        let replacement = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1));
        assert!(!votes.observe(replacement, voter));
        assert_eq!(votes.candidates.len(), 1);
        assert!(votes.candidates.contains_key(&replacement));

        assert!(!votes.observe(replacement, IpAddr::V4(Ipv4Addr::new(192, 0, 2, 2)),));
        assert!(votes.observe(replacement, IpAddr::V4(Ipv4Addr::new(192, 0, 2, 3)),));
        assert!(votes.candidates.is_empty());
    }

    #[test]
    fn error_transactions_require_an_exact_source_match() {
        let endpoint = SocketAddr::from((Ipv4Addr::LOCALHOST, 6881));
        let transaction = Transaction {
            endpoint,
            contact: None,
            owner: TransactionOwner::Bootstrap(NodeId([1; 20])),
            deadline: Instant::now(),
        };
        let mut transactions = HashMap::from([(0x7a7a, transaction)]);
        assert!(take_correlated_transaction(&mut transactions, b"bad", endpoint).is_none());
        assert!(
            take_correlated_transaction(
                &mut transactions,
                b"zz",
                SocketAddr::from((Ipv4Addr::LOCALHOST, 6882)),
            )
            .is_none()
        );
        assert_eq!(transactions.len(), 1);
        let matched = take_correlated_transaction(&mut transactions, b"zz", endpoint)
            .expect("matching transaction");
        assert_eq!(matched.endpoint, endpoint);
        assert!(transactions.is_empty());
    }

    #[test]
    fn lookup_observation_advances_only_for_closer_responded_ids() {
        let target = NodeId::ZERO;
        let candidate = |prefix: u8, port: u16| {
            let mut id = [0_u8; 20];
            id[0] = prefix;
            Candidate {
                contact: Some(NodeContact {
                    id: NodeId(id),
                    address: dht_endpoint(SocketAddr::from((Ipv4Addr::LOCALHOST, port))),
                }),
                address: SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
                state: CandidateState::Unqueried,
            }
        };
        let now = Instant::now();
        let (sender, _receiver) = oneshot::channel();
        let mut lookup = Lookup::new(
            7,
            target,
            [candidate(0x80, 6201), candidate(0x20, 6202)],
            sender,
            now + Duration::from_secs(30),
            now,
        );
        let first = lookup.candidates[0];
        lookup.mark(first.address, CandidateState::Responded, first.contact);
        let first_prefix = lookup
            .closest_responded_prefix_bits
            .expect("first convergence prefix");
        let first_improvement = lookup
            .last_convergence_improvement_at
            .expect("first convergence instant");

        let farther = lookup
            .candidates
            .iter()
            .copied()
            .find(|candidate| {
                candidate
                    .contact
                    .is_some_and(|contact| target.shared_prefix_bits(contact.id) < first_prefix)
            })
            .expect("farther candidate");
        lookup.mark(farther.address, CandidateState::Responded, farther.contact);
        assert_eq!(lookup.closest_responded_prefix_bits, Some(first_prefix));
        assert_eq!(
            lookup.last_convergence_improvement_at,
            Some(first_improvement)
        );
        let observation = lookup.observation(Instant::now());
        assert_eq!(observation.lookup_id, 7);
        assert_eq!(observation.responded_candidates, 2);
        assert_eq!(
            observation.closest_responded_prefix_bits,
            Some(first_prefix)
        );
    }
    use rstorrent_protocol::dht::{ResponseMessage, decode_message};

    fn loopback_config(bootstrap: Vec<BootstrapNode>) -> DhtConfig {
        DhtConfig {
            network_policy: NetworkPolicy::LoopbackOnly,
            bind_address: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            bootstrap_nodes: bootstrap,
            initial_snapshot: None,
            query_timeout: Duration::from_millis(500),
            lookup_timeout: Duration::from_secs(3),
            bootstrap_retry_interval: Duration::from_secs(1),
            routing_refresh_interval: Duration::from_secs(60),
            read_only: false,
            byte_metric_sink: None,
        }
    }

    async fn exchange(socket: &UdpSocket, server: SocketAddr, bytes: &[u8]) -> Message {
        socket.send_to(bytes, server).await.expect("send query");
        let mut response = [0_u8; MAX_DATAGRAM_SIZE];
        let (length, source) =
            tokio::time::timeout(Duration::from_secs(1), socket.recv_from(&mut response))
                .await
                .expect("response timeout")
                .expect("receive response");
        assert_eq!(source, server);
        decode_message(&response[..length]).expect("decode response")
    }

    #[tokio::test]
    async fn lookup_uses_incoming_announce_and_warm_restart_hint() {
        let server = DhtService::start(loopback_config(Vec::new()))
            .await
            .expect("start server");
        let server_address = server.local_address();
        let announcer = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind announcer");
        let announcer_id = NodeId([7; 20]);
        let info_hash = NodeId([9; 20]);
        let get_peers = encode_query(
            b"aa",
            announcer_id,
            &Query::GetPeers {
                info_hash,
                want: vec![Want::Ipv4],
            },
            false,
        )
        .expect("encode get_peers");
        let Message::Response(ResponseMessage {
            token: Some(token), ..
        }) = exchange(&announcer, server_address, &get_peers).await
        else {
            panic!("get_peers response with token expected");
        };
        let announced_peer = SocketAddr::from((Ipv4Addr::LOCALHOST, 55_555));
        let announce = encode_query(
            b"ab",
            announcer_id,
            &Query::AnnouncePeer {
                info_hash,
                port: announced_peer.port(),
                implied_port: false,
                token,
            },
            false,
        )
        .expect("encode announce");
        assert!(matches!(
            exchange(&announcer, server_address, &announce).await,
            Message::Response(_)
        ));

        let client = DhtService::start(loopback_config(vec![BootstrapNode::Address(
            server_address,
        )]))
        .await
        .expect("start client");
        let peers = client
            .handle()
            .lookup(info_hash.0)
            .await
            .expect("lookup peer");
        assert_eq!(peers, vec![announced_peer]);
        let snapshot = client.shutdown().await.expect("client snapshot");
        assert_eq!(snapshot.nodes_v4.len(), 1);
        assert_eq!(snapshot.nodes_v4[0].address, dht_endpoint(server_address));

        let mut warm_config = loopback_config(Vec::new());
        warm_config.initial_snapshot = Some(snapshot);
        let warm = DhtService::start(warm_config).await.expect("warm client");
        let warm_peers = warm
            .handle()
            .lookup(info_hash.0)
            .await
            .expect("warm lookup peer");
        assert_eq!(warm_peers, vec![announced_peer]);
        warm.shutdown().await.expect("warm shutdown");
        server.shutdown().await.expect("server shutdown");
    }

    #[tokio::test]
    async fn offline_lookup_fails_without_sending() {
        let mut config = loopback_config(Vec::new());
        config.network_policy = NetworkPolicy::Offline;
        let service = DhtService::start(config).await.expect("offline service");
        assert_eq!(
            service.handle().lookup([1; 20]).await,
            Err(DhtError::NetworkDisabled)
        );
        service.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn observations_are_latest_value_bounded_and_terminal() {
        let router = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind silent router");
        let router_address = router.local_addr().expect("router address");
        let mut config = loopback_config(vec![BootstrapNode::Address(router_address)]);
        config.query_timeout = Duration::from_secs(2);
        let service = DhtService::start(config).await.expect("start DHT");
        let service_address = service.local_address();
        let mut observations = service.subscribe_observations();
        assert_eq!(observations.borrow().buckets_v4.len(), 160);
        observations.borrow_and_update();

        let handle = service.handle();
        let lookup_handle = handle.clone();
        let lookup = tokio::spawn(async move { lookup_handle.lookup([5; 20]).await });
        let malformed = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind malformed sender");
        malformed
            .send_to(b"x", service_address)
            .await
            .expect("send malformed datagram");
        let mut received_bytes = 1_u64;
        for transaction in 0..=MAX_QUERIES_PER_SOURCE_MINUTE {
            let ping = encode_query(
                &[u8::try_from(transaction).expect("bounded transaction")],
                NodeId([4; 20]),
                &Query::Ping,
                true,
            )
            .expect("encode ping");
            received_bytes = received_bytes.saturating_add(ping.len() as u64);
            malformed
                .send_to(&ping, service_address)
                .await
                .expect("send rate-limit probe");
        }

        let active = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                observations.changed().await.expect("active observation");
                let observation = observations.borrow_and_update().clone();
                if !observation.lookups.is_empty()
                    && observation.stats.malformed_received == 1
                    && observation.stats.rate_limited == 1
                    && observation.stats.datagram_bytes_sent > 0
                {
                    break observation;
                }
            }
        })
        .await
        .expect("active observation timeout");
        assert_eq!(active.lifecycle, DhtLifecycle::BootstrapEmpty);
        assert_eq!(active.stats.datagram_bytes_received, received_bytes);
        assert_eq!(
            active.stats.queries_received,
            u64::from(MAX_QUERIES_PER_SOURCE_MINUTE) + 1
        );
        assert_eq!(active.lookups.len(), 1);
        assert!(active.stats.active_transactions <= MAX_ACTIVE_TRANSACTIONS as u32);

        service.shutdown().await.expect("shutdown");
        let terminal = loop {
            let observation = observations.borrow_and_update().clone();
            if observation.lifecycle == DhtLifecycle::Inactive {
                break observation;
            }
            if observations.changed().await.is_err() {
                break observations.borrow_and_update().clone();
            }
        };
        assert_eq!(terminal.lifecycle, DhtLifecycle::Inactive);
        assert_eq!(terminal.stats.active_transactions, 0);
        assert!(terminal.lookups.is_empty());
        assert_eq!(lookup.await.expect("lookup task"), Err(DhtError::Cancelled));
    }

    #[tokio::test]
    async fn unreachable_bootstrap_is_retried() {
        let router = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind silent router");
        let router_address = router.local_addr().expect("router address");
        let mut config = loopback_config(vec![BootstrapNode::Address(router_address)]);
        config.query_timeout = Duration::from_millis(100);
        config.lookup_timeout = Duration::from_secs(1);
        config.bootstrap_retry_interval = Duration::from_millis(300);
        let service = DhtService::start(config).await.expect("start DHT");
        let mut packet = [0_u8; MAX_DATAGRAM_SIZE];
        tokio::time::timeout(Duration::from_secs(1), router.recv_from(&mut packet))
            .await
            .expect("initial bootstrap timed out")
            .expect("initial bootstrap");
        tokio::time::timeout(Duration::from_secs(2), router.recv_from(&mut packet))
            .await
            .expect("bootstrap retry timed out")
            .expect("bootstrap retry");
        assert!(
            service
                .handle()
                .stats()
                .await
                .expect("DHT stats")
                .bootstrap_attempts
                >= 2
        );
        service.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn warm_nodes_are_attempted_before_cold_fallback() {
        let warm = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind warm node");
        let warm_address = warm.local_addr().expect("warm address");
        let fallback = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind fallback node");
        let fallback_address = fallback.local_addr().expect("fallback address");
        let mut config = loopback_config(vec![BootstrapNode::Address(fallback_address)]);
        config.query_timeout = Duration::from_millis(100);
        config.lookup_timeout = Duration::from_secs(1);
        config.initial_snapshot = Some(DhtSnapshot {
            version: DHT_SNAPSHOT_VERSION,
            node_id: NodeId([1; 20]),
            nodes_v4: vec![NodeContact {
                id: NodeId([2; 20]),
                address: dht_endpoint(warm_address),
            }],
            nodes_v6: Vec::new(),
        });
        let service = DhtService::start(config).await.expect("start DHT");
        let mut packet = [0_u8; MAX_DATAGRAM_SIZE];
        tokio::time::timeout(Duration::from_secs(1), warm.recv_from(&mut packet))
            .await
            .expect("warm bootstrap timed out")
            .expect("warm bootstrap");
        assert!(
            tokio::time::timeout(Duration::from_millis(50), fallback.recv_from(&mut packet),)
                .await
                .is_err(),
            "cold fallback must wait for the warm attempt to expire"
        );
        tokio::time::timeout(Duration::from_secs(1), fallback.recv_from(&mut packet))
            .await
            .expect("cold fallback timed out")
            .expect("cold fallback");
        service.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn dropped_lookup_receiver_releases_actor_work() {
        let router = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind silent router");
        let router_address = router.local_addr().expect("router address");
        let mut config = loopback_config(vec![BootstrapNode::Address(router_address)]);
        config.query_timeout = Duration::from_secs(1);
        config.lookup_timeout = Duration::from_secs(2);
        let service = DhtService::start(config).await.expect("start DHT");
        let handle = service.handle();
        assert!(
            tokio::time::timeout(Duration::from_millis(20), handle.lookup([5; 20]))
                .await
                .is_err()
        );
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(handle.stats().await.expect("DHT stats").active_lookups, 0);
        service.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn responsive_table_is_refreshed_periodically() {
        let router = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind router");
        let router_address = router.local_addr().expect("router address");
        let mut config = loopback_config(vec![BootstrapNode::Address(router_address)]);
        config.routing_refresh_interval = Duration::from_millis(300);
        let service = DhtService::start(config).await.expect("start DHT");
        let mut packet = [0_u8; MAX_DATAGRAM_SIZE];
        let (length, client) =
            tokio::time::timeout(Duration::from_secs(1), router.recv_from(&mut packet))
                .await
                .expect("initial bootstrap timed out")
                .expect("initial bootstrap");
        let Message::Query(query) = decode_message(&packet[..length]).expect("bootstrap query")
        else {
            panic!("bootstrap must be a query");
        };
        let response = encode_response(
            &query.transaction,
            NodeId([4; 20]),
            &[],
            &[],
            None,
            dht_endpoint(client),
        )
        .expect("bootstrap response");
        router
            .send_to(&response, client)
            .await
            .expect("send bootstrap response");
        tokio::time::timeout(Duration::from_secs(2), router.recv_from(&mut packet))
            .await
            .expect("routing refresh timed out")
            .expect("routing refresh");
        assert!(
            service
                .handle()
                .stats()
                .await
                .expect("DHT stats")
                .routing_refreshes
                >= 1
        );
        service.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    #[ignore = "uses changing public Mainline DHT bootstrap routers"]
    async fn live_public_bootstrap_reaches_bep42_node() {
        let service = DhtService::start(DhtConfig::for_network(NetworkPolicy::Online))
            .await
            .expect("start public DHT");
        let handle = service.handle();
        let reached = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let stats = handle.stats().await.expect("public DHT stats");
                if stats.responses_received > 0 && stats.routing_nodes_v4 > 0 {
                    return stats;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await;
        if reached.is_err() {
            let stats = handle.stats().await.expect("timed-out public DHT stats");
            service.shutdown().await.expect("public DHT shutdown");
            panic!("public bootstrap did not reach a BEP 42-valid node; stats={stats:?}");
        }
        service.shutdown().await.expect("public DHT shutdown");
    }

    #[test]
    fn snapshot_validation_bounds_and_filters_contacts() {
        let snapshot = DhtSnapshot {
            version: DHT_SNAPSHOT_VERSION,
            node_id: NodeId([1; 20]),
            nodes_v4: vec![NodeContact {
                id: NodeId([2; 20]),
                address: dht_endpoint(SocketAddr::from((Ipv4Addr::LOCALHOST, 6881))),
            }],
            nodes_v6: Vec::new(),
        }
        .validate()
        .expect("valid snapshot");
        assert_eq!(snapshot.nodes_v4.len(), 1);
        assert!(matches!(
            DhtSnapshot {
                version: 99,
                ..snapshot
            }
            .validate(),
            Err(DhtError::UnsupportedSnapshotVersion(99))
        ));
    }
}
