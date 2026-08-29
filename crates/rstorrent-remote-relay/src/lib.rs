#![forbid(unsafe_code)]
//! Local-only bounded dumb relay for Tactical 190's controlled proof.
//!
//! This crate owns routing metadata and opaque WebSocket messages. It has no
//! dependency on OPAQUE, encrypted records, or application frame types and it
//! exposes no durable/public relay binary.

use std::collections::BTreeMap;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use subtle::ConstantTimeEq;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore, oneshot};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

pub const MAX_REGISTERED_ROUTES: usize = 1_024;
pub const MAX_ACTIVE_CIRCUITS: usize = 256;
pub const MAX_USERNAME_BYTES: usize = 32;
pub const MIN_USERNAME_BYTES: usize = 3;
pub const RELAY_CREDENTIAL_BYTES: usize = 32;
pub const PREAUTH_MESSAGE_BYTES: usize = 4 * 1024;
pub const MAX_RELAY_MESSAGE_BYTES: usize = 16 * 1024 * 1024 + 4 * 1024 + 32 + 64 * 1024;
pub const WAITING_HOST_TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_CIRCUIT_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);
pub const PAIRED_CONTROL: &[u8; 4] = b"RSP1";

const HOST_CLAIM_MAGIC: &[u8; 4] = b"RSH1";
const CLIENT_SELECT_MAGIC: &[u8; 4] = b"RSC1";
const CLOSE_UNAVAILABLE: u16 = 4_004;
const CLOSE_CLAIM_REJECTED: u16 = 4_003;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RelayMetricsSnapshot {
    pub registered_routes: usize,
    pub registered_routes_high_water: usize,
    pub waiting_hosts: usize,
    pub waiting_hosts_high_water: usize,
    pub active_circuits: usize,
    pub active_circuits_high_water: usize,
    pub accepted_host_claims: u64,
    pub rejected_host_claims: u64,
    pub host_replacements: u64,
    pub unavailable_clients: u64,
    pub busy_clients: u64,
    pub paired_circuits: u64,
    pub completed_circuits: u64,
    pub forwarded_client_messages: u64,
    pub forwarded_client_bytes: u64,
    pub forwarded_host_messages: u64,
    pub forwarded_host_bytes: u64,
    pub forwarded_message_bytes_high_water: usize,
    pub active_pumps: usize,
    pub active_pumps_high_water: usize,
}

#[derive(Default)]
struct RelayMetrics {
    registered_routes: AtomicUsize,
    registered_routes_high_water: AtomicUsize,
    waiting_hosts: AtomicUsize,
    waiting_hosts_high_water: AtomicUsize,
    active_circuits: AtomicUsize,
    active_circuits_high_water: AtomicUsize,
    accepted_host_claims: AtomicU64,
    rejected_host_claims: AtomicU64,
    host_replacements: AtomicU64,
    unavailable_clients: AtomicU64,
    busy_clients: AtomicU64,
    paired_circuits: AtomicU64,
    completed_circuits: AtomicU64,
    forwarded_client_messages: AtomicU64,
    forwarded_client_bytes: AtomicU64,
    forwarded_host_messages: AtomicU64,
    forwarded_host_bytes: AtomicU64,
    forwarded_message_bytes_high_water: AtomicUsize,
    active_pumps: AtomicUsize,
    active_pumps_high_water: AtomicUsize,
}

impl RelayMetrics {
    fn snapshot(&self) -> RelayMetricsSnapshot {
        RelayMetricsSnapshot {
            registered_routes: self.registered_routes.load(Ordering::Relaxed),
            registered_routes_high_water: self.registered_routes_high_water.load(Ordering::Relaxed),
            waiting_hosts: self.waiting_hosts.load(Ordering::Relaxed),
            waiting_hosts_high_water: self.waiting_hosts_high_water.load(Ordering::Relaxed),
            active_circuits: self.active_circuits.load(Ordering::Relaxed),
            active_circuits_high_water: self.active_circuits_high_water.load(Ordering::Relaxed),
            accepted_host_claims: self.accepted_host_claims.load(Ordering::Relaxed),
            rejected_host_claims: self.rejected_host_claims.load(Ordering::Relaxed),
            host_replacements: self.host_replacements.load(Ordering::Relaxed),
            unavailable_clients: self.unavailable_clients.load(Ordering::Relaxed),
            busy_clients: self.busy_clients.load(Ordering::Relaxed),
            paired_circuits: self.paired_circuits.load(Ordering::Relaxed),
            completed_circuits: self.completed_circuits.load(Ordering::Relaxed),
            forwarded_client_messages: self.forwarded_client_messages.load(Ordering::Relaxed),
            forwarded_client_bytes: self.forwarded_client_bytes.load(Ordering::Relaxed),
            forwarded_host_messages: self.forwarded_host_messages.load(Ordering::Relaxed),
            forwarded_host_bytes: self.forwarded_host_bytes.load(Ordering::Relaxed),
            forwarded_message_bytes_high_water: self
                .forwarded_message_bytes_high_water
                .load(Ordering::Relaxed),
            active_pumps: self.active_pumps.load(Ordering::Relaxed),
            active_pumps_high_water: self.active_pumps_high_water.load(Ordering::Relaxed),
        }
    }

    fn registered(&self) {
        let value = self.registered_routes.fetch_add(1, Ordering::Relaxed) + 1;
        self.registered_routes_high_water
            .fetch_max(value, Ordering::Relaxed);
    }

    fn waiting_started(&self) {
        let value = self.waiting_hosts.fetch_add(1, Ordering::Relaxed) + 1;
        self.waiting_hosts_high_water
            .fetch_max(value, Ordering::Relaxed);
    }

    fn waiting_stopped(&self) {
        self.waiting_hosts.fetch_sub(1, Ordering::Relaxed);
    }

    fn circuit_started(&self) {
        let value = self.active_circuits.fetch_add(1, Ordering::Relaxed) + 1;
        self.active_circuits_high_water
            .fetch_max(value, Ordering::Relaxed);
        self.paired_circuits.fetch_add(1, Ordering::Relaxed);
    }

    fn circuit_stopped(&self) {
        self.active_circuits.fetch_sub(1, Ordering::Relaxed);
        self.completed_circuits.fetch_add(1, Ordering::Relaxed);
    }

    fn pump_started(&self) {
        let value = self.active_pumps.fetch_add(1, Ordering::Relaxed) + 1;
        self.active_pumps_high_water
            .fetch_max(value, Ordering::Relaxed);
    }

    fn pump_stopped(&self) {
        self.active_pumps.fetch_sub(1, Ordering::Relaxed);
    }

    fn forwarded(&self, direction: Direction, bytes: usize) {
        match direction {
            Direction::ClientToHost => {
                self.forwarded_client_messages
                    .fetch_add(1, Ordering::Relaxed);
                self.forwarded_client_bytes
                    .fetch_add(bytes as u64, Ordering::Relaxed);
            }
            Direction::HostToClient => {
                self.forwarded_host_messages.fetch_add(1, Ordering::Relaxed);
                self.forwarded_host_bytes
                    .fetch_add(bytes as u64, Ordering::Relaxed);
            }
        }
        self.forwarded_message_bytes_high_water
            .fetch_max(bytes, Ordering::Relaxed);
    }
}

#[derive(Clone)]
pub struct ProofRelay {
    inner: Arc<RelayInner>,
}

impl fmt::Debug for ProofRelay {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProofRelay")
            .field("relay_id", &self.inner.relay_id)
            .finish_non_exhaustive()
    }
}

struct RelayInner {
    relay_id: [u8; 32],
    routes: Mutex<BTreeMap<String, Route>>,
    next_generation: AtomicU64,
    circuit_slots: Arc<Semaphore>,
    metrics: RelayMetrics,
    shutdown: CancellationToken,
}

struct Route {
    credential: Zeroizing<[u8; RELAY_CREDENTIAL_BYTES]>,
    waiting: Option<WaitingHost>,
    active_generation: Option<u64>,
}

struct WaitingHost {
    generation: u64,
    client: oneshot::Sender<PairedClient>,
    cancellation: CancellationToken,
}

struct PairedClient {
    socket: WebSocket,
    _slot: OwnedSemaphorePermit,
}

impl ProofRelay {
    pub fn new(relay_id: [u8; 32]) -> Self {
        Self {
            inner: Arc::new(RelayInner {
                relay_id,
                routes: Mutex::new(BTreeMap::new()),
                next_generation: AtomicU64::new(1),
                circuit_slots: Arc::new(Semaphore::new(MAX_ACTIVE_CIRCUITS)),
                metrics: RelayMetrics::default(),
                shutdown: CancellationToken::new(),
            }),
        }
    }

    pub fn relay_id(&self) -> [u8; 32] {
        self.inner.relay_id
    }

    pub fn metrics(&self) -> RelayMetricsSnapshot {
        self.inner.metrics.snapshot()
    }

    pub fn shutdown(&self) {
        self.inner.shutdown.cancel();
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route("/host", get(host_upgrade))
            .route("/client", get(client_upgrade))
            .with_state(self.inner.clone())
    }
}

pub struct ProofRelayServer {
    listener: TcpListener,
    address: SocketAddr,
    relay: ProofRelay,
}

impl fmt::Debug for ProofRelayServer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProofRelayServer")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

impl ProofRelayServer {
    pub async fn bind_loopback(relay_id: [u8; 32]) -> std::io::Result<Self> {
        let listener =
            TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).await?;
        let address = listener.local_addr()?;
        Ok(Self {
            listener,
            address,
            relay: ProofRelay::new(relay_id),
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.address
    }

    pub fn relay(&self) -> ProofRelay {
        self.relay.clone()
    }

    pub async fn serve(self) -> std::io::Result<()> {
        let shutdown = self.relay.inner.shutdown.clone();
        axum::serve(self.listener, self.relay.router())
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
    }
}

pub fn encode_host_claim(username: &str, credential: &[u8; 32]) -> Result<Vec<u8>, ClaimError> {
    validate_username(username)?;
    let mut encoded = Vec::with_capacity(5 + username.len() + credential.len());
    encoded.extend_from_slice(HOST_CLAIM_MAGIC);
    encoded.push(username.len() as u8);
    encoded.extend_from_slice(username.as_bytes());
    encoded.extend_from_slice(credential);
    Ok(encoded)
}

pub fn encode_client_select(username: &str) -> Result<Vec<u8>, ClaimError> {
    validate_username(username)?;
    let mut encoded = Vec::with_capacity(5 + username.len());
    encoded.extend_from_slice(CLIENT_SELECT_MAGIC);
    encoded.push(username.len() as u8);
    encoded.extend_from_slice(username.as_bytes());
    Ok(encoded)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaimError {
    InvalidUsername,
    InvalidMessage,
}

impl fmt::Display for ClaimError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUsername => formatter.write_str("invalid relay username"),
            Self::InvalidMessage => formatter.write_str("invalid relay claim"),
        }
    }
}

impl std::error::Error for ClaimError {}

async fn host_upgrade(
    State(relay): State<Arc<RelayInner>>,
    websocket: WebSocketUpgrade,
) -> Response {
    websocket
        .max_message_size(MAX_RELAY_MESSAGE_BYTES)
        .max_frame_size(MAX_RELAY_MESSAGE_BYTES)
        .on_upgrade(move |socket| host_connection(relay, socket))
        .into_response()
}

async fn client_upgrade(
    State(relay): State<Arc<RelayInner>>,
    websocket: WebSocketUpgrade,
) -> Response {
    websocket
        .max_message_size(MAX_RELAY_MESSAGE_BYTES)
        .max_frame_size(MAX_RELAY_MESSAGE_BYTES)
        .on_upgrade(move |socket| client_connection(relay, socket))
        .into_response()
}

async fn host_connection(relay: Arc<RelayInner>, mut socket: WebSocket) {
    let Some(claim) = receive_initial_binary(&mut socket, &relay.shutdown).await else {
        close(&mut socket, CLOSE_CLAIM_REJECTED, "claim rejected").await;
        return;
    };
    let Ok((username, credential)) = parse_host_claim(&claim) else {
        relay
            .metrics
            .rejected_host_claims
            .fetch_add(1, Ordering::Relaxed);
        close(&mut socket, CLOSE_CLAIM_REJECTED, "claim rejected").await;
        return;
    };
    let generation = relay.next_generation.fetch_add(1, Ordering::Relaxed);
    let (client_sender, mut client_receiver) = oneshot::channel();
    let cancellation = CancellationToken::new();

    let accepted = {
        let mut routes = relay.routes.lock().await;
        let is_new = !routes.contains_key(&username);
        if !route_registration_available(routes.len(), !is_new) {
            false
        } else {
            let route = routes.entry(username.clone()).or_insert_with(|| {
                relay.metrics.registered();
                Route {
                    credential: Zeroizing::new(credential),
                    waiting: None,
                    active_generation: None,
                }
            });
            if !bool::from(route.credential.ct_eq(&credential)) || route.active_generation.is_some()
            {
                false
            } else {
                if let Some(previous) = route.waiting.take() {
                    previous.cancellation.cancel();
                    relay.metrics.waiting_stopped();
                    relay
                        .metrics
                        .host_replacements
                        .fetch_add(1, Ordering::Relaxed);
                }
                route.waiting = Some(WaitingHost {
                    generation,
                    client: client_sender,
                    cancellation: cancellation.clone(),
                });
                relay.metrics.waiting_started();
                true
            }
        }
    };
    if !accepted {
        relay
            .metrics
            .rejected_host_claims
            .fetch_add(1, Ordering::Relaxed);
        close(&mut socket, CLOSE_CLAIM_REJECTED, "claim rejected").await;
        return;
    }
    relay
        .metrics
        .accepted_host_claims
        .fetch_add(1, Ordering::Relaxed);

    let client = tokio::select! {
        client = &mut client_receiver => client.ok(),
        _ = cancellation.cancelled() => None,
        _ = relay.shutdown.cancelled() => None,
        _ = tokio::time::sleep(WAITING_HOST_TIMEOUT) => None,
        incoming = socket.recv() => {
            let _ = incoming;
            None
        }
    };
    let Some(mut client) = client else {
        release_generation(&relay, &username, generation).await;
        close(&mut socket, 1_000, "waiting ended").await;
        return;
    };

    if socket
        .send(Message::Binary(PAIRED_CONTROL.to_vec().into()))
        .await
        .is_err()
        || client
            .socket
            .send(Message::Binary(PAIRED_CONTROL.to_vec().into()))
            .await
            .is_err()
    {
        release_active(&relay, &username, generation).await;
        return;
    }
    let PairedClient {
        socket: client,
        _slot: circuit_slot,
    } = client;
    relay.metrics.circuit_started();
    run_pair(socket, client, relay.clone()).await;
    relay.metrics.circuit_stopped();
    release_active(&relay, &username, generation).await;
    drop(circuit_slot);
}

async fn client_connection(relay: Arc<RelayInner>, mut socket: WebSocket) {
    let Some(select) = receive_initial_binary(&mut socket, &relay.shutdown).await else {
        close(&mut socket, CLOSE_UNAVAILABLE, "unavailable").await;
        return;
    };
    let Ok(username) = parse_client_select(&select) else {
        close(&mut socket, CLOSE_UNAVAILABLE, "unavailable").await;
        return;
    };

    let waiting = {
        let mut routes = relay.routes.lock().await;
        let Some(route) = routes.get_mut(&username) else {
            relay
                .metrics
                .unavailable_clients
                .fetch_add(1, Ordering::Relaxed);
            drop(routes);
            close(&mut socket, CLOSE_UNAVAILABLE, "unavailable").await;
            return;
        };
        if route.active_generation.is_some() {
            relay.metrics.busy_clients.fetch_add(1, Ordering::Relaxed);
            None
        } else if route.waiting.is_some() {
            if let Ok(slot) = relay.circuit_slots.clone().try_acquire_owned() {
                let waiting = route.waiting.take().expect("waiting host checked above");
                relay.metrics.waiting_stopped();
                route.active_generation = Some(waiting.generation);
                Some((waiting, slot))
            } else {
                relay.metrics.busy_clients.fetch_add(1, Ordering::Relaxed);
                None
            }
        } else {
            relay
                .metrics
                .unavailable_clients
                .fetch_add(1, Ordering::Relaxed);
            None
        }
    };
    let Some((waiting, slot)) = waiting else {
        close(&mut socket, CLOSE_UNAVAILABLE, "unavailable").await;
        return;
    };
    let generation = waiting.generation;
    if let Err(mut returned) = waiting.client.send(PairedClient {
        socket,
        _slot: slot,
    }) {
        release_active(&relay, &username, generation).await;
        close(&mut returned.socket, CLOSE_UNAVAILABLE, "unavailable").await;
    }
}

async fn receive_initial_binary(
    socket: &mut WebSocket,
    shutdown: &CancellationToken,
) -> Option<Vec<u8>> {
    let received = tokio::select! {
        _ = shutdown.cancelled() => return None,
        received = tokio::time::timeout(Duration::from_secs(20), socket.recv()) => received,
    };
    match received {
        Ok(Some(Ok(Message::Binary(bytes)))) if bytes.len() <= PREAUTH_MESSAGE_BYTES => {
            Some(bytes.to_vec())
        }
        _ => None,
    }
}

async fn release_generation(relay: &RelayInner, username: &str, generation: u64) {
    let mut routes = relay.routes.lock().await;
    let Some(route) = routes.get_mut(username) else {
        return;
    };
    if route
        .waiting
        .as_ref()
        .is_some_and(|waiting| waiting.generation == generation)
    {
        route.waiting = None;
        relay.metrics.waiting_stopped();
    }
    if route.active_generation == Some(generation) {
        route.active_generation = None;
    }
}

async fn release_active(relay: &RelayInner, username: &str, generation: u64) {
    let mut routes = relay.routes.lock().await;
    let Some(route) = routes.get_mut(username) else {
        return;
    };
    if route.active_generation == Some(generation) {
        route.active_generation = None;
    }
}

async fn run_pair(host: WebSocket, client: WebSocket, relay: Arc<RelayInner>) {
    let (host_writer, host_reader) = host.split();
    let (client_writer, client_reader) = client.split();
    let cancellation = CancellationToken::new();
    let mut pumps = JoinSet::new();
    pumps.spawn(run_pump(
        client_reader,
        host_writer,
        Direction::ClientToHost,
        cancellation.clone(),
        relay.clone(),
    ));
    pumps.spawn(run_pump(
        host_reader,
        client_writer,
        Direction::HostToClient,
        cancellation.clone(),
        relay.clone(),
    ));
    let _ = tokio::select! {
        joined = pumps.join_next() => joined,
        _ = relay.shutdown.cancelled() => None,
        _ = tokio::time::sleep(MAX_CIRCUIT_LIFETIME) => None,
    };
    cancellation.cancel();
    while pumps.join_next().await.is_some() {}
}

#[derive(Clone, Copy)]
enum Direction {
    ClientToHost,
    HostToClient,
}

async fn run_pump(
    mut reader: SplitStream<WebSocket>,
    mut writer: SplitSink<WebSocket, Message>,
    direction: Direction,
    cancellation: CancellationToken,
    relay: Arc<RelayInner>,
) {
    relay.metrics.pump_started();
    loop {
        let incoming = tokio::select! {
            _ = cancellation.cancelled() => break,
            _ = relay.shutdown.cancelled() => break,
            incoming = reader.next() => incoming,
        };
        let Some(Ok(message)) = incoming else {
            break;
        };
        let bytes = match &message {
            Message::Binary(bytes) if bytes.len() <= MAX_RELAY_MESSAGE_BYTES => bytes.len(),
            Message::Ping(bytes) | Message::Pong(bytes) => bytes.len(),
            Message::Close(_) => 0,
            Message::Text(_) | Message::Binary(_) => break,
        };
        relay.metrics.forwarded(direction, bytes);
        let is_close = matches!(message, Message::Close(_));
        let sent = tokio::select! {
            _ = cancellation.cancelled() => false,
            _ = relay.shutdown.cancelled() => false,
            result = writer.send(message) => result.is_ok(),
        };
        if !sent || is_close {
            break;
        }
    }
    relay.metrics.pump_stopped();
}

fn parse_host_claim(encoded: &[u8]) -> Result<(String, [u8; 32]), ClaimError> {
    if encoded.len() < 5 || &encoded[..4] != HOST_CLAIM_MAGIC {
        return Err(ClaimError::InvalidMessage);
    }
    let username_length = usize::from(encoded[4]);
    if encoded.len() != 5 + username_length + RELAY_CREDENTIAL_BYTES {
        return Err(ClaimError::InvalidMessage);
    }
    let username = parse_username(&encoded[5..5 + username_length])?;
    let credential = encoded[5 + username_length..]
        .try_into()
        .map_err(|_| ClaimError::InvalidMessage)?;
    Ok((username, credential))
}

fn parse_client_select(encoded: &[u8]) -> Result<String, ClaimError> {
    if encoded.len() < 5 || &encoded[..4] != CLIENT_SELECT_MAGIC {
        return Err(ClaimError::InvalidMessage);
    }
    let username_length = usize::from(encoded[4]);
    if encoded.len() != 5 + username_length {
        return Err(ClaimError::InvalidMessage);
    }
    parse_username(&encoded[5..])
}

fn parse_username(encoded: &[u8]) -> Result<String, ClaimError> {
    let username = std::str::from_utf8(encoded).map_err(|_| ClaimError::InvalidUsername)?;
    validate_username(username)?;
    Ok(username.to_owned())
}

fn validate_username(username: &str) -> Result<(), ClaimError> {
    let bytes = username.as_bytes();
    if !(MIN_USERNAME_BYTES..=MAX_USERNAME_BYTES).contains(&bytes.len())
        || !bytes
            .first()
            .zip(bytes.last())
            .is_some_and(|(first, last)| {
                first.is_ascii_alphanumeric() && last.is_ascii_alphanumeric()
            })
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
    {
        return Err(ClaimError::InvalidUsername);
    }
    Ok(())
}

fn route_registration_available(route_count: usize, route_exists: bool) -> bool {
    route_exists || route_count < MAX_REGISTERED_ROUTES
}

async fn close(socket: &mut WebSocket, code: u16, reason: &'static str) {
    let _ = socket
        .send(Message::Close(Some(CloseFrame {
            code,
            reason: reason.into(),
        })))
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_codecs_are_exact_and_bounded() {
        let host = encode_host_claim("alice-2", &[7; 32]).unwrap();
        assert_eq!(parse_host_claim(&host), Ok(("alice-2".to_owned(), [7; 32])));
        let client = encode_client_select("alice-2").unwrap();
        assert_eq!(parse_client_select(&client), Ok("alice-2".to_owned()));

        for invalid in ["ab", "-alice", "Alice", "alice_2", "alice-"] {
            assert_eq!(
                encode_client_select(invalid),
                Err(ClaimError::InvalidUsername)
            );
        }
        let mut trailing = client;
        trailing.push(0);
        assert_eq!(
            parse_client_select(&trailing),
            Err(ClaimError::InvalidMessage)
        );
    }

    #[test]
    fn route_registration_limit_is_exact_and_preserves_existing_routes() {
        assert!(route_registration_available(
            MAX_REGISTERED_ROUTES - 1,
            false
        ));
        assert!(!route_registration_available(MAX_REGISTERED_ROUTES, false));
        assert!(route_registration_available(MAX_REGISTERED_ROUTES, true));
    }

    #[tokio::test]
    async fn active_circuit_limit_is_exact_and_releases_capacity() {
        let relay = ProofRelay::new([7; 32]);
        let mut permits = Vec::with_capacity(MAX_ACTIVE_CIRCUITS);
        for _ in 0..MAX_ACTIVE_CIRCUITS {
            permits.push(
                relay
                    .inner
                    .circuit_slots
                    .clone()
                    .try_acquire_owned()
                    .expect("slot below the global limit"),
            );
        }
        assert!(
            relay
                .inner
                .circuit_slots
                .clone()
                .try_acquire_owned()
                .is_err()
        );

        drop(permits);
        assert_eq!(
            relay.inner.circuit_slots.available_permits(),
            MAX_ACTIVE_CIRCUITS
        );
    }
}
