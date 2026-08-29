#![forbid(unsafe_code)]
//! Native host adapter for Tactical 190's controlled local proof.
//!
//! This crate is deliberately an opt-in harness rather than product runtime.
//! It composes the pure remote crypto, the dumb relay and the existing
//! loopback application WebSocket without adding a second application API.

use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use rstorrent_gateway::{ApplicationClientFrame, ApplicationServerFrame};
use rstorrent_remote_crypto::{
    Binding, PasswordFile, SecureChannel, ServerAuthority, finish_server_login,
    finish_server_registration, random_operation_seed, start_server_login,
    start_server_registration,
};
use rstorrent_remote_relay::{PAIRED_CONTROL, encode_host_claim};
use rstorrent_session::{ApplicationCall, ApplicationCallResult};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{self, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

const HANDSHAKE_MESSAGE_BYTES: usize = 4 * 1024;
const FULL_LOGIN_DEADLINE: Duration = Duration::from_secs(20);
const CIRCUIT_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);
const RECONNECT_DELAY: Duration = Duration::from_millis(20);
const REGISTRATION_REQUEST: &[u8; 4] = b"RSG1";
const REGISTRATION_RESPONSE: &[u8; 4] = b"RSG2";
const REGISTRATION_UPLOAD: &[u8; 4] = b"RSG3";
const REGISTRATION_COMPLETE: &[u8; 4] = b"RSG4";
const LOGIN_REQUEST: &[u8; 4] = b"RSL1";
const LOGIN_RESPONSE: &[u8; 4] = b"RSL2";
const LOGIN_FINALIZATION: &[u8; 4] = b"RSL3";
const AUTHENTICATED_READY: &[u8; 4] = b"RSA1";

type RelaySocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

pub struct ProofHostConfig {
    pub relay_host_url: String,
    pub relay_credential: Zeroizing<[u8; 32]>,
    pub binding: Binding,
    pub authority: ServerAuthority,
    pub gateway_websocket_url: String,
    pub gateway_origin: String,
}

#[derive(Clone, Default)]
pub struct ProofHostMetrics {
    inner: Arc<ProofHostMetricsInner>,
}

#[derive(Default)]
struct ProofHostMetricsInner {
    accepted_route_claims: AtomicU64,
    completed_registrations: AtomicU64,
    login_attempts: AtomicU64,
    authenticated_logins: AtomicU64,
    failed_circuits: AtomicU64,
    active_circuits: AtomicUsize,
    active_circuits_high_water: AtomicUsize,
    client_application_frames: AtomicU64,
    server_application_frames: AtomicU64,
    client_acknowledgements: AtomicU64,
    server_view_batches: AtomicU64,
    server_call_results: AtomicU64,
    rejected_application_breadth: AtomicU64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProofHostMetricsSnapshot {
    pub accepted_route_claims: u64,
    pub completed_registrations: u64,
    pub login_attempts: u64,
    pub authenticated_logins: u64,
    pub failed_circuits: u64,
    pub active_circuits: usize,
    pub active_circuits_high_water: usize,
    pub client_application_frames: u64,
    pub server_application_frames: u64,
    pub client_acknowledgements: u64,
    pub server_view_batches: u64,
    pub server_call_results: u64,
    pub rejected_application_breadth: u64,
}

impl ProofHostMetrics {
    pub fn snapshot(&self) -> ProofHostMetricsSnapshot {
        ProofHostMetricsSnapshot {
            accepted_route_claims: self.inner.accepted_route_claims.load(Ordering::Relaxed),
            completed_registrations: self.inner.completed_registrations.load(Ordering::Relaxed),
            login_attempts: self.inner.login_attempts.load(Ordering::Relaxed),
            authenticated_logins: self.inner.authenticated_logins.load(Ordering::Relaxed),
            failed_circuits: self.inner.failed_circuits.load(Ordering::Relaxed),
            active_circuits: self.inner.active_circuits.load(Ordering::Relaxed),
            active_circuits_high_water: self
                .inner
                .active_circuits_high_water
                .load(Ordering::Relaxed),
            client_application_frames: self.inner.client_application_frames.load(Ordering::Relaxed),
            server_application_frames: self.inner.server_application_frames.load(Ordering::Relaxed),
            client_acknowledgements: self.inner.client_acknowledgements.load(Ordering::Relaxed),
            server_view_batches: self.inner.server_view_batches.load(Ordering::Relaxed),
            server_call_results: self.inner.server_call_results.load(Ordering::Relaxed),
            rejected_application_breadth: self
                .inner
                .rejected_application_breadth
                .load(Ordering::Relaxed),
        }
    }

    fn active_started(&self) {
        let active = self.inner.active_circuits.fetch_add(1, Ordering::Relaxed) + 1;
        self.inner
            .active_circuits_high_water
            .fetch_max(active, Ordering::Relaxed);
    }

    fn active_stopped(&self) {
        self.inner.active_circuits.fetch_sub(1, Ordering::Relaxed);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProofHostError(&'static str);

impl fmt::Display for ProofHostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for ProofHostError {}

/// Maintain one outbound waiting host and serve one circuit at a time.
pub async fn run_proof_host(
    config: ProofHostConfig,
    metrics: ProofHostMetrics,
    shutdown: CancellationToken,
) {
    let mut password_file: Option<PasswordFile> = None;
    while !shutdown.is_cancelled() {
        let connection = tokio::select! {
            _ = shutdown.cancelled() => break,
            connection = connect_async(&config.relay_host_url) => connection,
        };
        let Ok((mut socket, _)) = connection else {
            wait_to_reconnect(&shutdown).await;
            continue;
        };
        let claim =
            match encode_host_claim(config.binding.username().as_str(), &config.relay_credential) {
                Ok(claim) => claim,
                Err(_) => break,
            };
        if socket.send(Message::Binary(claim.into())).await.is_err() {
            wait_to_reconnect(&shutdown).await;
            continue;
        }
        let paired = tokio::select! {
            _ = shutdown.cancelled() => None,
            message = next_binary(&mut socket) => message.ok(),
        };
        if paired.as_deref() != Some(PAIRED_CONTROL) {
            wait_to_reconnect(&shutdown).await;
            continue;
        }
        metrics
            .inner
            .accepted_route_claims
            .fetch_add(1, Ordering::Relaxed);
        metrics.active_started();
        let result = tokio::select! {
            _ = shutdown.cancelled() => Ok(()),
            result = serve_circuit(
                &mut socket,
                &config,
                &mut password_file,
                &metrics,
                &shutdown,
            ) => result,
        };
        metrics.active_stopped();
        if result.is_err() && !shutdown.is_cancelled() {
            metrics
                .inner
                .failed_circuits
                .fetch_add(1, Ordering::Relaxed);
        }
        let _ = socket.close(None).await;
        wait_to_reconnect(&shutdown).await;
    }
}

async fn serve_circuit(
    socket: &mut RelaySocket,
    config: &ProofHostConfig,
    password_file: &mut Option<PasswordFile>,
    metrics: &ProofHostMetrics,
    shutdown: &CancellationToken,
) -> Result<(), ProofHostError> {
    let authenticated = tokio::time::timeout(
        FULL_LOGIN_DEADLINE,
        authenticate_circuit(socket, config, password_file, metrics),
    )
    .await
    .map_err(|_| ProofHostError("remote handshake timed out"))??;
    let Some(channel) = authenticated else {
        return Ok(());
    };
    bridge_application(socket, channel, config, metrics, shutdown).await
}

async fn authenticate_circuit(
    socket: &mut RelaySocket,
    config: &ProofHostConfig,
    password_file: &mut Option<PasswordFile>,
    metrics: &ProofHostMetrics,
) -> Result<Option<SecureChannel>, ProofHostError> {
    let initial = next_binary(socket).await?;
    if let Ok(request) = protocol_payload(&initial, REGISTRATION_REQUEST) {
        if password_file.is_some() {
            return Err(ProofHostError("remote registration is unavailable"));
        }
        let response = start_server_registration(&config.authority, &config.binding, request)
            .map_err(|_| ProofHostError("remote registration failed"))?;
        send_protocol(socket, REGISTRATION_RESPONSE, &response).await?;
        let upload = next_binary(socket).await?;
        let upload = protocol_payload(&upload, REGISTRATION_UPLOAD)?;
        let completed = finish_server_registration(upload)
            .map_err(|_| ProofHostError("remote registration failed"))?;
        *password_file = Some(completed);
        socket
            .send(Message::Binary(REGISTRATION_COMPLETE.to_vec().into()))
            .await
            .map_err(|_| ProofHostError("remote registration failed"))?;
        metrics
            .inner
            .completed_registrations
            .fetch_add(1, Ordering::Relaxed);
        return Ok(None);
    }

    let request = protocol_payload(&initial, LOGIN_REQUEST)?;
    metrics.inner.login_attempts.fetch_add(1, Ordering::Relaxed);
    let login = start_server_login(
        &config.authority,
        password_file.as_ref(),
        &config.binding,
        request,
        random_operation_seed().map_err(|_| ProofHostError("secure randomness failed"))?,
    )
    .map_err(|_| ProofHostError("remote login failed"))?;
    send_protocol(socket, LOGIN_RESPONSE, login.response()).await?;
    let finalization = next_binary(socket).await?;
    let finalization = protocol_payload(&finalization, LOGIN_FINALIZATION)?;
    let channel = finish_server_login(login, finalization)
        .map_err(|_| ProofHostError("remote login failed"))?;
    metrics
        .inner
        .authenticated_logins
        .fetch_add(1, Ordering::Relaxed);
    Ok(Some(channel))
}

async fn bridge_application(
    relay: &mut RelaySocket,
    mut channel: SecureChannel,
    config: &ProofHostConfig,
    metrics: &ProofHostMetrics,
    shutdown: &CancellationToken,
) -> Result<(), ProofHostError> {
    let mut request = config
        .gateway_websocket_url
        .as_str()
        .into_client_request()
        .map_err(|_| ProofHostError("local application connection failed"))?;
    request.headers_mut().insert(
        "Origin",
        config
            .gateway_origin
            .parse()
            .map_err(|_| ProofHostError("local application origin is invalid"))?,
    );
    let (mut gateway, _) = connect_async(request)
        .await
        .map_err(|_| ProofHostError("local application connection failed"))?;
    let ready = channel
        .seal(AUTHENTICATED_READY)
        .map_err(|_| ProofHostError("remote record failed"))?;
    relay
        .send(Message::Binary(ready.into()))
        .await
        .map_err(|_| ProofHostError("remote transport failed"))?;

    let lifetime = tokio::time::sleep(CIRCUIT_LIFETIME);
    tokio::pin!(lifetime);
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
            _ = &mut lifetime => break,
            message = relay.next() => {
                let Some(message) = message else { break; };
                let message = message.map_err(|_| ProofHostError("remote transport failed"))?;
                match message {
                    Message::Binary(record) => {
                        let opened = channel
                            .open(&record)
                            .map_err(|_| ProofHostError("remote record failed"))?;
                        if opened.is_close {
                            let _ = gateway.close(None).await;
                            return Ok(());
                        }
                        let text = std::str::from_utf8(&opened.plaintext)
                            .map_err(|_| ProofHostError("remote application frame is invalid"))?;
                        let frame: ApplicationClientFrame = serde_json::from_str(text)
                            .map_err(|_| ProofHostError("remote application frame is invalid"))?;
                        validate_client_frame(&frame, metrics)?;
                        metrics
                            .inner
                            .client_application_frames
                            .fetch_add(1, Ordering::Relaxed);
                        if matches!(frame, ApplicationClientFrame::Ack { .. }) {
                            metrics
                                .inner
                                .client_acknowledgements
                                .fetch_add(1, Ordering::Relaxed);
                        }
                        gateway
                            .send(Message::Text(text.to_owned().into()))
                            .await
                            .map_err(|_| ProofHostError("local application connection failed"))?;
                    }
                    Message::Ping(payload) => {
                        relay
                            .send(Message::Pong(payload))
                            .await
                            .map_err(|_| ProofHostError("remote transport failed"))?;
                    }
                    Message::Pong(_) => {}
                    Message::Close(_) => break,
                    Message::Text(_) | Message::Frame(_) => {
                        return Err(ProofHostError("remote transport sent plaintext"));
                    }
                }
            }
            message = gateway.next() => {
                let Some(message) = message else { break; };
                let message = message
                    .map_err(|_| ProofHostError("local application connection failed"))?;
                match message {
                    Message::Text(text) => {
                        let frame: ApplicationServerFrame = serde_json::from_str(&text)
                            .map_err(|_| ProofHostError("local application frame is invalid"))?;
                        validate_server_frame(&frame, metrics)?;
                        metrics
                            .inner
                            .server_application_frames
                            .fetch_add(1, Ordering::Relaxed);
                        let record = channel
                            .seal(text.as_bytes())
                            .map_err(|_| ProofHostError("remote record failed"))?;
                        relay
                            .send(Message::Binary(record.into()))
                            .await
                            .map_err(|_| ProofHostError("remote transport failed"))?;
                    }
                    Message::Ping(payload) => {
                        gateway
                            .send(Message::Pong(payload))
                            .await
                            .map_err(|_| ProofHostError("local application connection failed"))?;
                    }
                    Message::Pong(_) => {}
                    Message::Close(_) => break,
                    Message::Binary(_) | Message::Frame(_) => {
                        return Err(ProofHostError("local application frame is invalid"));
                    }
                }
            }
        }
    }
    if let Ok(record) = channel.seal_close() {
        let _ = relay.send(Message::Binary(record.into())).await;
    }
    let _ = gateway.close(None).await;
    Ok(())
}

fn validate_client_frame(
    frame: &ApplicationClientFrame,
    metrics: &ProofHostMetrics,
) -> Result<(), ProofHostError> {
    let unsupported = matches!(frame, ApplicationClientFrame::BeginTorrentUpload { .. })
        || matches!(
            frame,
            ApplicationClientFrame::Call {
                operation: ApplicationCall::CreateMediaUrl { .. },
                ..
            }
        )
        || matches!(
            frame,
            ApplicationClientFrame::Connect { token: Some(_), .. }
        );
    if unsupported {
        metrics
            .inner
            .rejected_application_breadth
            .fetch_add(1, Ordering::Relaxed);
        return Err(ProofHostError(
            "remote application operation is unsupported",
        ));
    }
    Ok(())
}

fn validate_server_frame(
    frame: &ApplicationServerFrame,
    metrics: &ProofHostMetrics,
) -> Result<(), ProofHostError> {
    if matches!(frame, ApplicationServerFrame::TorrentUploadReady { .. })
        || matches!(
            frame,
            ApplicationServerFrame::Result {
                result: ApplicationCallResult::MediaUrl { .. },
                ..
            }
        )
    {
        metrics
            .inner
            .rejected_application_breadth
            .fetch_add(1, Ordering::Relaxed);
        return Err(ProofHostError(
            "remote application operation is unsupported",
        ));
    }
    if matches!(frame, ApplicationServerFrame::ViewBatch { .. }) {
        metrics
            .inner
            .server_view_batches
            .fetch_add(1, Ordering::Relaxed);
    }
    if matches!(frame, ApplicationServerFrame::Result { .. }) {
        metrics
            .inner
            .server_call_results
            .fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}

async fn next_binary(socket: &mut RelaySocket) -> Result<Vec<u8>, ProofHostError> {
    loop {
        let message = socket
            .next()
            .await
            .ok_or(ProofHostError("remote transport closed"))?
            .map_err(|_| ProofHostError("remote transport failed"))?;
        match message {
            Message::Binary(message) if message.len() <= HANDSHAKE_MESSAGE_BYTES => {
                return Ok(message.to_vec());
            }
            Message::Ping(payload) => socket
                .send(Message::Pong(payload))
                .await
                .map_err(|_| ProofHostError("remote transport failed"))?,
            Message::Pong(_) => {}
            Message::Text(_) | Message::Binary(_) | Message::Close(_) | Message::Frame(_) => {
                return Err(ProofHostError("remote handshake message is invalid"));
            }
        }
    }
}

fn protocol_payload<'a>(message: &'a [u8], magic: &[u8; 4]) -> Result<&'a [u8], ProofHostError> {
    message
        .strip_prefix(magic)
        .filter(|payload| !payload.is_empty())
        .ok_or(ProofHostError("remote handshake message is invalid"))
}

async fn send_protocol(
    socket: &mut RelaySocket,
    magic: &[u8; 4],
    payload: &[u8],
) -> Result<(), ProofHostError> {
    if payload.len() + magic.len() > HANDSHAKE_MESSAGE_BYTES {
        return Err(ProofHostError("remote handshake message is invalid"));
    }
    let mut message = Vec::with_capacity(magic.len() + payload.len());
    message.extend_from_slice(magic);
    message.extend_from_slice(payload);
    socket
        .send(Message::Binary(message.into()))
        .await
        .map_err(|_| ProofHostError("remote transport failed"))
}

async fn wait_to_reconnect(shutdown: &CancellationToken) {
    tokio::select! {
        _ = shutdown.cancelled() => {}
        _ = tokio::time::sleep(RECONNECT_DELAY) => {}
    }
}

impl From<tungstenite::Error> for ProofHostError {
    fn from(_: tungstenite::Error) -> Self {
        Self("remote transport failed")
    }
}
