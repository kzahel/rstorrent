#![forbid(unsafe_code)]

//! Bounded loopback WebSocket adapter for the application contract.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures_util::{SinkExt, StreamExt};
use rstorrent_session::{
    ApplicationService, RequestEnvelope, ResponseEnvelope, SubscriptionSpec, ViewSubscription,
    ViewUpdate, application_error_response,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore, mpsc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

pub const GATEWAY_CONTRACT_VERSION: u16 = 1;
pub const MAX_INCOMING_MESSAGE_BYTES: usize = 64 * 1024;
pub const MAX_OUTGOING_MESSAGE_BYTES: usize = 512 * 1024;
pub const MAX_OUTGOING_MESSAGES: usize = 8;
pub const MAX_CONNECTIONS: usize = 8;
pub const MAX_SUBSCRIPTIONS_PER_CONNECTION: usize = 8;
pub const MAX_TOKEN_BYTES: usize = 128;
pub const MAX_ORIGIN_BYTES: usize = 512;

#[derive(Clone)]
pub struct GatewayConfig {
    pub bind: SocketAddr,
    pub token: String,
    pub allowed_origin: String,
    pub max_connections: usize,
}

impl fmt::Debug for GatewayConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GatewayConfig")
            .field("bind", &self.bind)
            .field("token", &"[redacted]")
            .field("allowed_origin", &self.allowed_origin)
            .field("max_connections", &self.max_connections)
            .finish()
    }
}

impl GatewayConfig {
    pub fn loopback(token: String, allowed_origin: String) -> Self {
        Self {
            bind: SocketAddr::from(([127, 0, 0, 1], 3030)),
            token,
            allowed_origin,
            max_connections: MAX_CONNECTIONS,
        }
    }

    pub fn validate(&self) -> Result<(), GatewayError> {
        if !self.bind.ip().is_loopback() {
            return Err(GatewayError::Configuration(
                "the proof gateway only binds a loopback address".to_owned(),
            ));
        }
        if self.token.is_empty() || self.token.len() > MAX_TOKEN_BYTES {
            return Err(GatewayError::Configuration(format!(
                "gateway token must be 1..={MAX_TOKEN_BYTES} bytes"
            )));
        }
        if self.allowed_origin.is_empty() || self.allowed_origin.len() > MAX_ORIGIN_BYTES {
            return Err(GatewayError::Configuration(format!(
                "allowed origin must be 1..={MAX_ORIGIN_BYTES} bytes"
            )));
        }
        if self.max_connections == 0 || self.max_connections > MAX_CONNECTIONS {
            return Err(GatewayError::Configuration(format!(
                "connection limit must be 1..={MAX_CONNECTIONS}"
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GatewayClientMessage {
    Authenticate {
        contract_version: u16,
        token: String,
    },
    Dispatch {
        request: RequestEnvelope,
    },
    Subscribe {
        request_id: String,
        spec: SubscriptionSpec,
    },
    Resync {
        request_id: String,
        stream_id: String,
    },
    Unsubscribe {
        request_id: String,
        stream_id: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GatewayServerMessage {
    Authenticated {
        contract_version: u16,
    },
    Response {
        response: ResponseEnvelope,
    },
    Subscribed {
        request_id: String,
        stream_id: String,
    },
    Update {
        update: ViewUpdate,
    },
    Unsubscribed {
        request_id: String,
        stream_id: String,
    },
    Error {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        code: GatewayErrorCode,
        message: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum GatewayErrorCode {
    AuthenticationRequired,
    AuthenticationFailed,
    InvalidVersion,
    InvalidMessage,
    ResourceLimit,
    UnknownSubscription,
    Internal,
}

#[derive(Debug)]
pub enum GatewayError {
    Configuration(String),
    Bind(std::io::Error),
    Serve(std::io::Error),
}

impl fmt::Display for GatewayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(message) => write!(formatter, "gateway configuration: {message}"),
            Self::Bind(error) => write!(formatter, "bind gateway: {error}"),
            Self::Serve(error) => write!(formatter, "serve gateway: {error}"),
        }
    }
}

impl Error for GatewayError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Bind(error) | Self::Serve(error) => Some(error),
            Self::Configuration(_) => None,
        }
    }
}

#[derive(Clone)]
struct GatewayState {
    token: Arc<str>,
    allowed_origin: Arc<str>,
    service: Arc<Mutex<ApplicationService>>,
    connections: Arc<Semaphore>,
}

impl fmt::Debug for GatewayState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GatewayState")
            .field("token", &"[redacted]")
            .field("allowed_origin", &self.allowed_origin)
            .field(
                "available_connections",
                &self.connections.available_permits(),
            )
            .finish_non_exhaustive()
    }
}

pub async fn bind(
    config: GatewayConfig,
    service: Arc<Mutex<ApplicationService>>,
) -> Result<GatewayServer, GatewayError> {
    config.validate()?;
    let listener = TcpListener::bind(config.bind)
        .await
        .map_err(GatewayError::Bind)?;
    let local_addr = listener.local_addr().map_err(GatewayError::Bind)?;
    let state = GatewayState {
        token: Arc::from(config.token),
        allowed_origin: Arc::from(config.allowed_origin),
        service,
        connections: Arc::new(Semaphore::new(config.max_connections)),
    };
    Ok(GatewayServer {
        listener,
        local_addr,
        state,
    })
}

pub struct GatewayServer {
    listener: TcpListener,
    local_addr: SocketAddr,
    state: GatewayState,
}

impl fmt::Debug for GatewayServer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GatewayServer")
            .field("local_addr", &self.local_addr)
            .finish_non_exhaustive()
    }
}

impl GatewayServer {
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub async fn serve(self, shutdown: CancellationToken) -> Result<(), GatewayError> {
        let router = Router::new()
            .route("/control", get(upgrade))
            .with_state(self.state);
        axum::serve(
            self.listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown.cancelled_owned())
        .await
        .map_err(GatewayError::Serve)
    }
}

async fn upgrade(
    State(state): State<GatewayState>,
    ConnectInfo(_peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    websocket: WebSocketUpgrade,
) -> Response {
    if headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        != Some(state.allowed_origin.as_ref())
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(permit) = state.connections.clone().try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    websocket
        .max_message_size(MAX_INCOMING_MESSAGE_BYTES)
        .max_frame_size(MAX_INCOMING_MESSAGE_BYTES)
        .on_upgrade(move |socket| serve_connection(socket, state, permit))
        .into_response()
}

struct ActiveSubscription {
    subscription: ViewSubscription,
    cancellation: CancellationToken,
    task: JoinHandle<()>,
}

async fn serve_connection(socket: WebSocket, state: GatewayState, _permit: OwnedSemaphorePermit) {
    let (mut websocket_writer, mut websocket_reader) = socket.split();
    let (outgoing, mut outgoing_reader) =
        mpsc::channel::<GatewayServerMessage>(MAX_OUTGOING_MESSAGES);
    let connection_cancel = CancellationToken::new();
    let writer = tokio::spawn(async move {
        while let Some(message) = outgoing_reader.recv().await {
            let Ok(encoded) = serde_json::to_string(&message) else {
                break;
            };
            if encoded.len() > MAX_OUTGOING_MESSAGE_BYTES
                || websocket_writer
                    .send(Message::Text(encoded.into()))
                    .await
                    .is_err()
            {
                break;
            }
        }
    });
    let mut authenticated = false;
    let mut subscriptions = BTreeMap::<String, ActiveSubscription>::new();

    while let Some(message) = websocket_reader.next().await {
        let Ok(message) = message else {
            break;
        };
        let Message::Text(text) = message else {
            if matches!(message, Message::Close(_)) {
                break;
            }
            send_error(
                &outgoing,
                None,
                GatewayErrorCode::InvalidMessage,
                "only text control messages are accepted",
            )
            .await;
            continue;
        };
        if text.len() > MAX_INCOMING_MESSAGE_BYTES {
            send_error(
                &outgoing,
                None,
                GatewayErrorCode::InvalidMessage,
                "control message exceeds the configured bound",
            )
            .await;
            break;
        }
        let parsed = match serde_json::from_str::<GatewayClientMessage>(&text) {
            Ok(parsed) => parsed,
            Err(error) => {
                send_error(
                    &outgoing,
                    None,
                    GatewayErrorCode::InvalidMessage,
                    &error.to_string(),
                )
                .await;
                continue;
            }
        };

        if !authenticated {
            match parsed {
                GatewayClientMessage::Authenticate {
                    contract_version,
                    token: _,
                } if contract_version != GATEWAY_CONTRACT_VERSION => {
                    send_error(
                        &outgoing,
                        None,
                        GatewayErrorCode::InvalidVersion,
                        "gateway contract version is unsupported",
                    )
                    .await;
                    break;
                }
                GatewayClientMessage::Authenticate { token, .. }
                    if constant_time_equal(token.as_bytes(), state.token.as_bytes()) =>
                {
                    authenticated = true;
                    if outgoing
                        .send(GatewayServerMessage::Authenticated {
                            contract_version: GATEWAY_CONTRACT_VERSION,
                        })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                GatewayClientMessage::Authenticate { .. } => {
                    send_error(
                        &outgoing,
                        None,
                        GatewayErrorCode::AuthenticationFailed,
                        "gateway credential was rejected",
                    )
                    .await;
                    break;
                }
                _ => {
                    send_error(
                        &outgoing,
                        None,
                        GatewayErrorCode::AuthenticationRequired,
                        "authenticate before sending control messages",
                    )
                    .await;
                    break;
                }
            }
            continue;
        }

        match parsed {
            GatewayClientMessage::Authenticate { .. } => {
                send_error(
                    &outgoing,
                    None,
                    GatewayErrorCode::InvalidMessage,
                    "the connection is already authenticated",
                )
                .await;
            }
            GatewayClientMessage::Dispatch { request } => {
                let request_id = request.request_id.clone();
                let mut service = state.service.lock().await;
                let response = match service.dispatch(request).await {
                    Ok(response) => response,
                    Err(error) => application_error_response(
                        request_id,
                        service.revision().unwrap_or(0),
                        &error,
                    ),
                };
                if outgoing
                    .send(GatewayServerMessage::Response { response })
                    .await
                    .is_err()
                {
                    break;
                }
            }
            GatewayClientMessage::Subscribe { request_id, spec } => {
                if subscriptions.len() >= MAX_SUBSCRIPTIONS_PER_CONNECTION {
                    send_error(
                        &outgoing,
                        Some(request_id),
                        GatewayErrorCode::ResourceLimit,
                        "connection subscription limit reached",
                    )
                    .await;
                    continue;
                }
                let subscription = {
                    let service = state.service.lock().await;
                    service.subscribe(spec)
                };
                let subscription = match subscription {
                    Ok(subscription) => subscription,
                    Err(error) => {
                        send_error(
                            &outgoing,
                            Some(request_id),
                            GatewayErrorCode::InvalidMessage,
                            &error.to_string(),
                        )
                        .await;
                        continue;
                    }
                };
                let stream_id = subscription.stream_id();
                let cancellation = connection_cancel.child_token();
                if outgoing
                    .send(GatewayServerMessage::Subscribed {
                        request_id,
                        stream_id: stream_id.clone(),
                    })
                    .await
                    .is_err()
                {
                    break;
                }
                let task =
                    spawn_forwarder(subscription.clone(), outgoing.clone(), cancellation.clone());
                subscriptions.insert(
                    stream_id,
                    ActiveSubscription {
                        subscription,
                        cancellation,
                        task,
                    },
                );
            }
            GatewayClientMessage::Resync {
                request_id,
                stream_id,
            } => {
                let Some(active) = subscriptions.get(&stream_id) else {
                    send_error(
                        &outgoing,
                        Some(request_id),
                        GatewayErrorCode::UnknownSubscription,
                        "subscription is not owned by this connection",
                    )
                    .await;
                    continue;
                };
                if let Err(error) = active.subscription.resync() {
                    send_error(
                        &outgoing,
                        Some(request_id),
                        GatewayErrorCode::Internal,
                        &error.to_string(),
                    )
                    .await;
                }
            }
            GatewayClientMessage::Unsubscribe {
                request_id,
                stream_id,
            } => {
                let Some(active) = subscriptions.remove(&stream_id) else {
                    send_error(
                        &outgoing,
                        Some(request_id),
                        GatewayErrorCode::UnknownSubscription,
                        "subscription is not owned by this connection",
                    )
                    .await;
                    continue;
                };
                stop_subscription(active).await;
                if outgoing
                    .send(GatewayServerMessage::Unsubscribed {
                        request_id,
                        stream_id,
                    })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }
    }

    connection_cancel.cancel();
    for (_, active) in subscriptions {
        stop_subscription(active).await;
    }
    drop(outgoing);
    let _ = writer.await;
}

fn spawn_forwarder(
    subscription: ViewSubscription,
    outgoing: mpsc::Sender<GatewayServerMessage>,
    cancellation: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let update = tokio::select! {
                () = cancellation.cancelled() => break,
                update = subscription.next_update() => update,
            };
            let Some(update) = update else {
                break;
            };
            let message = GatewayServerMessage::Update { update };
            let encoded_len = match serde_json::to_vec(&message) {
                Ok(bytes) => bytes.len(),
                Err(_) => break,
            };
            if encoded_len > MAX_OUTGOING_MESSAGE_BYTES {
                break;
            }
            tokio::select! {
                () = cancellation.cancelled() => break,
                result = outgoing.send(message) => {
                    if result.is_err() {
                        break;
                    }
                }
            }
        }
        subscription.close();
    })
}

async fn stop_subscription(active: ActiveSubscription) {
    active.cancellation.cancel();
    active.subscription.close();
    let _ = active.task.await;
}

async fn send_error(
    outgoing: &mpsc::Sender<GatewayServerMessage>,
    request_id: Option<String>,
    code: GatewayErrorCode,
    message: &str,
) {
    let mut message = message.to_owned();
    message.truncate(message.floor_char_boundary(1024));
    let _ = outgoing
        .send(GatewayServerMessage::Error {
            request_id,
            code,
            message,
        })
        .await;
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= usize::from(
            left.get(index).copied().unwrap_or(0) ^ right.get(index).copied().unwrap_or(0),
        );
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    use futures_util::{SinkExt, StreamExt};
    use rstorrent_session::{
        ApplicationConfig, ApplicationService, Command, ConfiguredStorageRoot, DeliveryPolicy,
        RequestEnvelope, ResponseOutcome, SubscriptionSpec, ViewProjection, ViewSelector,
        ViewUpdatePayload,
    };
    use tokio::sync::Mutex;
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_util::sync::CancellationToken;

    use super::{
        GATEWAY_CONTRACT_VERSION, GatewayClientMessage, GatewayConfig, GatewayErrorCode,
        GatewayServerMessage, bind, constant_time_equal,
    };

    static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

    fn test_root(label: &str) -> PathBuf {
        let sequence = NEXT_TEST.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rstorrent-gateway-{label}-{}-{sequence}",
            std::process::id()
        ))
    }

    async fn test_service(root: &Path) -> Arc<Mutex<ApplicationService>> {
        Arc::new(Mutex::new(
            ApplicationService::open(ApplicationConfig::new(
                root.join("profile"),
                "test".to_owned(),
                vec![ConfiguredStorageRoot {
                    id: "downloads".to_owned(),
                    path: root.join("payload"),
                }],
            ))
            .await
            .expect("open service"),
        ))
    }

    async fn read_message(
        socket: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    ) -> GatewayServerMessage {
        let message = tokio::time::timeout(std::time::Duration::from_secs(1), socket.next())
            .await
            .expect("gateway response timed out")
            .expect("gateway closed")
            .expect("websocket response");
        let Message::Text(text) = message else {
            panic!("expected text response");
        };
        serde_json::from_str(&text).expect("decode response")
    }

    #[test]
    fn credential_comparison_includes_length_and_content() {
        assert!(constant_time_equal(b"token", b"token"));
        assert!(!constant_time_equal(b"token", b"taken"));
        assert!(!constant_time_equal(b"token", b"token-longer"));
    }

    #[tokio::test]
    async fn enforces_origin_and_authentication_before_dispatch() {
        let root = test_root("auth");
        let service = test_service(&root).await;
        let server = bind(
            GatewayConfig {
                bind: "127.0.0.1:0".parse().expect("address"),
                token: "correct-token".to_owned(),
                allowed_origin: "http://127.0.0.1:5173".to_owned(),
                max_connections: 2,
            },
            service.clone(),
        )
        .await
        .expect("bind");
        let address = server.local_addr();
        let shutdown = CancellationToken::new();
        let task_shutdown = shutdown.clone();
        let task = tokio::spawn(server.serve(task_shutdown));
        let url = format!("ws://{address}/control");

        let mut bad_origin = url.clone().into_client_request().expect("request");
        bad_origin.headers_mut().insert(
            "Origin",
            "https://attacker.invalid".parse().expect("origin"),
        );
        assert!(connect_async(bad_origin).await.is_err());

        let mut request = url.clone().into_client_request().expect("request");
        request
            .headers_mut()
            .insert("Origin", "http://127.0.0.1:5173".parse().expect("origin"));
        let (mut socket, _) = connect_async(request).await.expect("connect");
        let mutation = GatewayClientMessage::Dispatch {
            request: RequestEnvelope {
                version: rstorrent_session::CONTROL_VERSION,
                request_id: "unauthenticated-add".to_owned(),
                expected_revision: None,
                command: Command::AddMagnet {
                    magnet: "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213&x.pe=127.0.0.1:1".to_owned(),
                    storage_root: "downloads".to_owned(),
                    skip_files: Vec::new(),
                },
            },
        };
        socket
            .send(Message::Text(
                serde_json::to_string(&mutation)
                    .expect("encode mutation")
                    .into(),
            ))
            .await
            .expect("send");
        assert!(matches!(
            read_message(&mut socket).await,
            GatewayServerMessage::Error {
                code: GatewayErrorCode::AuthenticationRequired,
                ..
            }
        ));

        let mut request = url.into_client_request().expect("request");
        request
            .headers_mut()
            .insert("Origin", "http://127.0.0.1:5173".parse().expect("origin"));
        let (mut socket, _) = connect_async(request).await.expect("reconnect");
        socket
            .send(Message::Text(
                serde_json::to_string(&GatewayClientMessage::Authenticate {
                    contract_version: GATEWAY_CONTRACT_VERSION,
                    token: "correct-token".to_owned(),
                })
                .expect("encode auth")
                .into(),
            ))
            .await
            .expect("authenticate");
        assert!(matches!(
            read_message(&mut socket).await,
            GatewayServerMessage::Authenticated { .. }
        ));
        socket
            .send(Message::Text(
                serde_json::to_string(&GatewayClientMessage::Dispatch {
                    request: RequestEnvelope {
                        version: rstorrent_session::CONTROL_VERSION,
                        request_id: "snapshot".to_owned(),
                        expected_revision: None,
                        command: Command::Snapshot,
                    },
                })
                .expect("encode snapshot")
                .into(),
            ))
            .await
            .expect("snapshot");
        let GatewayServerMessage::Response { response } = read_message(&mut socket).await else {
            panic!("expected command response");
        };
        let ResponseOutcome::Success { snapshot } = response.outcome else {
            panic!("snapshot should succeed");
        };
        assert!(
            snapshot.torrents.is_empty(),
            "unauthenticated mutation reached the dispatcher"
        );

        socket
            .send(Message::Text(
                serde_json::to_string(&GatewayClientMessage::Subscribe {
                    request_id: "subscribe".to_owned(),
                    spec: SubscriptionSpec {
                        selector: ViewSelector::TorrentList,
                        projection: ViewProjection::Summary,
                        delivery: DeliveryPolicy {
                            min_interval_millis: 0,
                            max_queue_bytes: 4096,
                        },
                    },
                })
                .expect("encode subscription")
                .into(),
            ))
            .await
            .expect("subscribe");
        let GatewayServerMessage::Subscribed { stream_id, .. } = read_message(&mut socket).await
        else {
            panic!("subscription acknowledgement must precede updates");
        };
        let GatewayServerMessage::Update { update } = read_message(&mut socket).await else {
            panic!("expected initial update");
        };
        assert_eq!(update.stream_id, stream_id);
        assert!(matches!(update.payload, ViewUpdatePayload::Snapshot { .. }));
        socket
            .send(Message::Text(
                serde_json::to_string(&GatewayClientMessage::Unsubscribe {
                    request_id: "unsubscribe".to_owned(),
                    stream_id: stream_id.clone(),
                })
                .expect("encode unsubscribe")
                .into(),
            ))
            .await
            .expect("unsubscribe");
        assert!(matches!(
            read_message(&mut socket).await,
            GatewayServerMessage::Unsubscribed {
                stream_id: closed,
                ..
            } if closed == stream_id
        ));

        socket.close(None).await.expect("close");
        shutdown.cancel();
        task.await
            .expect("server join")
            .expect("server termination");
        service.lock().await.shutdown().await.expect("shutdown");
        drop(service);
        std::fs::remove_dir_all(root).expect("remove root");
    }
}
