#![forbid(unsafe_code)]

//! Bounded loopback HTTP and WebSocket adapter for the application contract.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use rstorrent_session::{
    ApplicationService, OpenViewSetRequest, RequestEnvelope, ResponseEnvelope, SubscriptionSpec,
    UpdateViewSetRequest, ViewSetError, ViewSetOwner, ViewSubscription, ViewUpdate,
    application_error_response,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore, mpsc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tower_http::cors::CorsLayer;
use ts_rs::TS;

pub const GATEWAY_CONTRACT_VERSION: u16 = 1;
pub const MAX_INCOMING_MESSAGE_BYTES: usize = 64 * 1024;
pub const MAX_OUTGOING_MESSAGE_BYTES: usize = 512 * 1024;
pub const MAX_HTTP_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_OUTGOING_MESSAGES: usize = 8;
pub const MAX_CONNECTIONS: usize = 8;
pub const MAX_SUBSCRIPTIONS_PER_CONNECTION: usize = 8;
pub const MAX_TOKEN_BYTES: usize = 128;
pub const MAX_ORIGIN_BYTES: usize = 512;
pub const HTTP_OWNER_HEX_BYTES: usize = 32;

static NEXT_HTTP_OWNER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub struct GatewayConfig {
    pub bind: SocketAddr,
    pub authentication: GatewayAuthentication,
    pub allowed_origin: String,
    pub max_connections: usize,
}

#[derive(Clone)]
pub enum GatewayAuthentication {
    Bearer { token: String },
    UnauthenticatedLoopbackDevelopment,
}

impl fmt::Debug for GatewayAuthentication {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bearer { .. } => formatter.write_str("Bearer { token: [redacted] }"),
            Self::UnauthenticatedLoopbackDevelopment => {
                formatter.write_str("UnauthenticatedLoopbackDevelopment")
            }
        }
    }
}

impl fmt::Debug for GatewayConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GatewayConfig")
            .field("bind", &self.bind)
            .field("authentication", &self.authentication)
            .field("allowed_origin", &self.allowed_origin)
            .field("max_connections", &self.max_connections)
            .finish()
    }
}

impl GatewayConfig {
    pub fn loopback(token: String, allowed_origin: String) -> Self {
        Self {
            bind: SocketAddr::from(([127, 0, 0, 1], 3030)),
            authentication: GatewayAuthentication::Bearer { token },
            allowed_origin,
            max_connections: MAX_CONNECTIONS,
        }
    }

    pub fn unauthenticated_loopback_development(allowed_origin: String) -> Self {
        Self {
            bind: SocketAddr::from(([127, 0, 0, 1], 0)),
            authentication: GatewayAuthentication::UnauthenticatedLoopbackDevelopment,
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
        match &self.authentication {
            GatewayAuthentication::Bearer { token }
                if token.is_empty() || token.len() > MAX_TOKEN_BYTES =>
            {
                return Err(GatewayError::Configuration(format!(
                    "gateway token must be 1..={MAX_TOKEN_BYTES} bytes"
                )));
            }
            GatewayAuthentication::UnauthenticatedLoopbackDevelopment if self.bind.port() != 0 => {
                return Err(GatewayError::Configuration(
                    "unauthenticated development mode requires an OS-assigned port".to_owned(),
                ));
            }
            _ => {}
        }
        if self.allowed_origin.is_empty() || self.allowed_origin.len() > MAX_ORIGIN_BYTES {
            return Err(GatewayError::Configuration(format!(
                "allowed origin must be 1..={MAX_ORIGIN_BYTES} bytes"
            )));
        }
        if HeaderValue::from_str(&self.allowed_origin).is_err() {
            return Err(GatewayError::Configuration(
                "allowed origin is not a valid HTTP header value".to_owned(),
            ));
        }
        if matches!(
            self.authentication,
            GatewayAuthentication::UnauthenticatedLoopbackDevelopment
        ) && !is_loopback_http_origin(&self.allowed_origin)
        {
            return Err(GatewayError::Configuration(
                "unauthenticated development mode requires an exact HTTP loopback origin with a port"
                    .to_owned(),
            ));
        }
        if self.max_connections == 0 || self.max_connections > MAX_CONNECTIONS {
            return Err(GatewayError::Configuration(format!(
                "connection limit must be 1..={MAX_CONNECTIONS}"
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, TS)]
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

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, TS)]
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
        update: Box<ViewUpdate>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum ApiErrorCode {
    AuthenticationFailed,
    InvalidRequest,
    ResourceLimit,
    UnknownViewSet,
    ConcurrentPull,
    ViewSetClosed,
    ResponseTooLarge,
    Internal,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
pub struct ApiError {
    pub code: ApiErrorCode,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
pub struct ApiErrorEnvelope {
    pub error: ApiError,
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
    authentication: Arc<GatewayAuthentication>,
    allowed_origin: Arc<str>,
    service: Arc<Mutex<ApplicationService>>,
    connections: Arc<Semaphore>,
    http_owner_namespace: u64,
}

impl fmt::Debug for GatewayState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GatewayState")
            .field("authentication", &self.authentication)
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
        authentication: Arc::new(config.authentication),
        allowed_origin: Arc::from(config.allowed_origin),
        service,
        connections: Arc::new(Semaphore::new(config.max_connections)),
        http_owner_namespace: NEXT_HTTP_OWNER.fetch_add(1, Ordering::Relaxed),
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
        let service = self.state.service.clone();
        let allowed_origin =
            HeaderValue::from_str(&self.state.allowed_origin).expect("validated allowed origin");
        let cors = CorsLayer::new()
            .allow_origin(allowed_origin)
            .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
            .allow_headers([
                header::ACCEPT,
                header::AUTHORIZATION,
                header::CONTENT_TYPE,
                HeaderName::from_static("x-rstorrent-owner"),
            ]);
        let router = Router::new()
            .route("/control", get(upgrade))
            .route("/api/v1/hello", get(api_hello))
            .route("/api/v1/commands", post(api_command))
            .route("/api/v1/view-sets", post(open_view_set))
            .route(
                "/api/v1/view-sets/{id}/views",
                axum::routing::put(update_view_set),
            )
            .route("/api/v1/view-sets/{id}/updates", get(view_set_updates))
            .route(
                "/api/v1/view-sets/{id}",
                axum::routing::delete(close_view_set),
            )
            .layer(axum::extract::DefaultBodyLimit::max(
                MAX_INCOMING_MESSAGE_BYTES,
            ))
            .layer(cors)
            .with_state(self.state);
        axum::serve(
            self.listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            shutdown.cancelled().await;
            service.lock().await.close_view_sets();
        })
        .await
        .map_err(GatewayError::Serve)
    }
}

fn is_loopback_http_origin(origin: &str) -> bool {
    let Ok(uri) = origin.parse::<Uri>() else {
        return false;
    };
    if uri.scheme_str() != Some("http") {
        return false;
    }
    let Some(authority) = uri.authority() else {
        return false;
    };
    if authority.port_u16().is_none() || !matches!(authority.host(), "127.0.0.1" | "[::1]" | "::1")
    {
        return false;
    }
    uri.path_and_query()
        .is_none_or(|path| path.as_str().is_empty() || path.as_str() == "/")
}

#[derive(Debug, Deserialize)]
struct UpdatesQuery {
    after: String,
    #[serde(default)]
    wait_ms: u32,
}

async fn api_hello(State(state): State<GatewayState>, headers: HeaderMap) -> Response {
    if let Err(error) = authenticate_http(&state, &headers) {
        return error.into_response();
    }
    json_response(StatusCode::OK, &state.service.lock().await.api_hello())
}

async fn api_command(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    request: Result<Json<RequestEnvelope>, JsonRejection>,
) -> Response {
    if let Err(error) = authenticate_http(&state, &headers) {
        return error.into_response();
    }
    let Json(request) = match request {
        Ok(request) => request,
        Err(error) => return invalid_request(error.body_text()),
    };
    let request_id = request.request_id.clone();
    let mut service = state.service.lock().await;
    let response = match service.dispatch(request).await {
        Ok(response) => response,
        Err(error) => {
            application_error_response(request_id, service.revision().unwrap_or(0), &error)
        }
    };
    json_response(StatusCode::OK, &response)
}

async fn open_view_set(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    request: Result<Json<OpenViewSetRequest>, JsonRejection>,
) -> Response {
    let owner = match authenticate_http(&state, &headers) {
        Ok(owner) => owner,
        Err(error) => return error.into_response(),
    };
    let Json(request) = match request {
        Ok(request) => request,
        Err(error) => return invalid_request(error.body_text()),
    };
    let result = state.service.lock().await.open_view_set(owner, request);
    match result {
        Ok(response) => json_response(StatusCode::CREATED, &response),
        Err(error) => view_set_error(error),
    }
}

async fn update_view_set(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    request: Result<Json<UpdateViewSetRequest>, JsonRejection>,
) -> Response {
    let owner = match authenticate_http(&state, &headers) {
        Ok(owner) => owner,
        Err(error) => return error.into_response(),
    };
    if !valid_view_set_id(&id) {
        return invalid_request("view-set ID is invalid");
    }
    let Json(request) = match request {
        Ok(request) => request,
        Err(error) => return invalid_request(error.body_text()),
    };
    let result = state
        .service
        .lock()
        .await
        .update_view_set(&owner, &id, request);
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => view_set_error(error),
    }
}

async fn view_set_updates(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    query: Result<Query<UpdatesQuery>, QueryRejection>,
) -> Response {
    let owner = match authenticate_http(&state, &headers) {
        Ok(owner) => owner,
        Err(error) => return error.into_response(),
    };
    if !valid_view_set_id(&id) {
        return invalid_request("view-set ID is invalid");
    }
    let Query(query) = match query {
        Ok(query) => query,
        Err(error) => return invalid_request(error.body_text()),
    };
    if query.after.len() > 20 {
        return invalid_request("view-set cursor exceeds its bound");
    }
    let view_set = {
        let service = state.service.lock().await;
        service.view_set(&owner, &id)
    };
    let view_set = match view_set {
        Ok(view_set) => view_set,
        Err(error) => return view_set_error(error),
    };
    match view_set.next_updates(&query.after, query.wait_ms).await {
        Ok(batch) => json_response(StatusCode::OK, &batch),
        Err(error) => view_set_error(error),
    }
}

async fn close_view_set(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let owner = match authenticate_http(&state, &headers) {
        Ok(owner) => owner,
        Err(error) => return error.into_response(),
    };
    if !valid_view_set_id(&id) {
        return invalid_request("view-set ID is invalid");
    }
    match state.service.lock().await.close_view_set(&owner, &id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => view_set_error(error),
    }
}

#[derive(Clone, Copy)]
enum HttpAuthError {
    Origin,
    Credential,
    Owner,
}

impl HttpAuthError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Origin => StatusCode::FORBIDDEN,
            Self::Credential | Self::Owner => StatusCode::UNAUTHORIZED,
        };
        api_error(
            status,
            ApiErrorCode::AuthenticationFailed,
            "gateway credential was rejected",
        )
    }
}

fn authenticate_http(
    state: &GatewayState,
    headers: &HeaderMap,
) -> Result<ViewSetOwner, HttpAuthError> {
    if headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        != Some(state.allowed_origin.as_ref())
    {
        return Err(HttpAuthError::Origin);
    }
    if let GatewayAuthentication::Bearer { token } = state.authentication.as_ref() {
        let credential = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "));
        if credential.is_none_or(|credential| {
            credential.len() > MAX_TOKEN_BYTES
                || !constant_time_equal(credential.as_bytes(), token.as_bytes())
        }) {
            return Err(HttpAuthError::Credential);
        }
    }
    let owner = headers
        .get("x-rstorrent-owner")
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            value.len() == HTTP_OWNER_HEX_BYTES
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        })
        .ok_or(HttpAuthError::Owner)?;
    Ok(ViewSetOwner::trusted(format!(
        "gateway-http-{}-{owner}",
        state.http_owner_namespace
    )))
}

fn view_set_error(error: ViewSetError) -> Response {
    let (status, code) = match error {
        ViewSetError::InvalidViewCount { .. }
        | ViewSetError::InvalidViewId
        | ViewSetError::DuplicateViewId(_)
        | ViewSetError::InvalidDeliveryInterval { .. }
        | ViewSetError::InvalidQueueBound { .. }
        | ViewSetError::InvalidView(_)
        | ViewSetError::SnapshotExceedsQueue { .. } => {
            (StatusCode::BAD_REQUEST, ApiErrorCode::InvalidRequest)
        }
        ViewSetError::ResourceLimit => (StatusCode::TOO_MANY_REQUESTS, ApiErrorCode::ResourceLimit),
        ViewSetError::UnknownViewSet => (StatusCode::NOT_FOUND, ApiErrorCode::UnknownViewSet),
        ViewSetError::ConsumerBusy => (StatusCode::CONFLICT, ApiErrorCode::ConcurrentPull),
        ViewSetError::Closed => (StatusCode::GONE, ApiErrorCode::ViewSetClosed),
        ViewSetError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, ApiErrorCode::Internal),
    };
    api_error(status, code, &error.to_string())
}

fn invalid_request(message: impl AsRef<str>) -> Response {
    api_error(
        StatusCode::BAD_REQUEST,
        ApiErrorCode::InvalidRequest,
        message.as_ref(),
    )
}

fn api_error(status: StatusCode, code: ApiErrorCode, message: &str) -> Response {
    let mut message = message.to_owned();
    message.truncate(message.floor_char_boundary(1024));
    json_response(
        status,
        &ApiErrorEnvelope {
            error: ApiError { code, message },
        },
    )
}

fn json_response<T: Serialize>(status: StatusCode, value: &T) -> Response {
    let body = match serde_json::to_vec(value) {
        Ok(body) if body.len() <= MAX_HTTP_RESPONSE_BYTES => body,
        Ok(_) => {
            return api_error_unchecked(
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiErrorCode::ResponseTooLarge,
                "gateway response exceeds its configured bound",
            );
        }
        Err(_) => {
            return api_error_unchecked(
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiErrorCode::Internal,
                "gateway response serialization failed",
            );
        }
    };
    (status, [(header::CONTENT_TYPE, "application/json")], body).into_response()
}

fn api_error_unchecked(status: StatusCode, code: ApiErrorCode, message: &str) -> Response {
    let body = serde_json::to_vec(&ApiErrorEnvelope {
        error: ApiError {
            code,
            message: message.to_owned(),
        },
    })
    .expect("fixed API error must serialize");
    (status, [(header::CONTENT_TYPE, "application/json")], body).into_response()
}

fn valid_view_set_id(value: &str) -> bool {
    value.len() == 35
        && value.starts_with("vs_")
        && value[3..].bytes().all(|byte| byte.is_ascii_hexdigit())
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
                    if bearer_token_matches(&state.authentication, &token) =>
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

fn bearer_token_matches(authentication: &GatewayAuthentication, candidate: &str) -> bool {
    match authentication {
        GatewayAuthentication::Bearer { token } => {
            constant_time_equal(candidate.as_bytes(), token.as_bytes())
        }
        GatewayAuthentication::UnauthenticatedLoopbackDevelopment => false,
    }
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
            let message = GatewayServerMessage::Update {
                update: Box::new(update),
            };
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
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use futures_util::{SinkExt, StreamExt};
    use rstorrent_session::{
        ApplicationConfig, ApplicationService, Command, ConfiguredStorageRoot, DeliveryPolicy,
        NetworkConfig, NetworkPolicy, OpenViewSetResponse, RequestEnvelope, ResponseOutcome,
        SubscriptionSpec, UpdateBatch, ViewProjection, ViewSelector, ViewUpdatePayload,
    };
    use tokio::sync::Mutex;
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_util::sync::CancellationToken;

    use super::{
        GATEWAY_CONTRACT_VERSION, GatewayAuthentication, GatewayClientMessage, GatewayConfig,
        GatewayErrorCode, GatewayServerMessage, bind, constant_time_equal,
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
                vec![ConfiguredStorageRoot::path(
                    "downloads",
                    root.join("payload"),
                )],
                NetworkConfig::new(
                    NetworkPolicy::LoopbackOnly,
                    Duration::from_secs(1),
                    Duration::from_secs(1),
                ),
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

    async fn http_request(
        address: SocketAddr,
        method: &str,
        path: &str,
        token: Option<&str>,
        origin: Option<&str>,
        body: Option<String>,
    ) -> (u16, Vec<u8>) {
        http_request_as(
            address,
            method,
            path,
            token,
            origin,
            Some("00000000000000000000000000000001"),
            body,
        )
        .await
    }

    async fn http_request_as(
        address: SocketAddr,
        method: &str,
        path: &str,
        token: Option<&str>,
        origin: Option<&str>,
        owner: Option<&str>,
        body: Option<String>,
    ) -> (u16, Vec<u8>) {
        let method = method.to_owned();
        let path = path.to_owned();
        let token = token.map(str::to_owned);
        let origin = origin.map(str::to_owned);
        let owner = owner.map(str::to_owned);
        tokio::task::spawn_blocking(move || {
            let body = body.unwrap_or_default();
            let mut request = format!(
                "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Length: {}\r\n",
                body.len()
            );
            if let Some(token) = token {
                request.push_str(&format!("Authorization: Bearer {token}\r\n"));
            }
            if let Some(origin) = origin {
                request.push_str(&format!("Origin: {origin}\r\n"));
            }
            if let Some(owner) = owner {
                request.push_str(&format!("X-RSTorrent-Owner: {owner}\r\n"));
            }
            if !body.is_empty() {
                request.push_str("Content-Type: application/json\r\n");
            }
            request.push_str("\r\n");
            request.push_str(&body);
            let mut stream = TcpStream::connect(address).expect("connect HTTP gateway");
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .expect("set HTTP timeout");
            stream
                .write_all(request.as_bytes())
                .expect("write HTTP request");
            let mut response = Vec::new();
            stream
                .read_to_end(&mut response)
                .expect("read HTTP response");
            let split = response
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .expect("HTTP header terminator");
            let headers = std::str::from_utf8(&response[..split]).expect("HTTP response headers");
            let status = headers
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .and_then(|status| status.parse().ok())
                .expect("HTTP response status");
            (status, response[(split + 4)..].to_vec())
        })
        .await
        .expect("HTTP request task")
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
                authentication: GatewayAuthentication::Bearer {
                    token: "correct-token".to_owned(),
                },
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
                        diagnostics: None,
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

    #[tokio::test]
    async fn http_view_sets_enforce_auth_replay_bounds_and_shutdown() {
        let root = test_root("http-view-set");
        let service = test_service(&root).await;
        let server = bind(
            GatewayConfig {
                bind: "127.0.0.1:0".parse().expect("address"),
                authentication: GatewayAuthentication::Bearer {
                    token: "correct-token".to_owned(),
                },
                allowed_origin: "http://127.0.0.1:5173".to_owned(),
                max_connections: 2,
            },
            service.clone(),
        )
        .await
        .expect("bind");
        let address = server.local_addr();
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(server.serve(shutdown.clone()));

        assert_eq!(
            http_request(address, "GET", "/api/v1/hello", None, None, None)
                .await
                .0,
            403
        );
        assert_eq!(
            http_request(
                address,
                "GET",
                "/api/v1/hello",
                None,
                Some("http://127.0.0.1:5173"),
                None,
            )
            .await
            .0,
            401
        );
        assert_eq!(
            http_request(
                address,
                "GET",
                "/api/v1/hello",
                Some("correct-token"),
                Some("https://attacker.invalid"),
                None,
            )
            .await
            .0,
            403
        );
        assert_eq!(
            http_request(
                address,
                "GET",
                "/api/v1/hello",
                Some("correct-token"),
                Some("http://127.0.0.1:5173"),
                None,
            )
            .await
            .0,
            200
        );
        assert_eq!(
            http_request_as(
                address,
                "GET",
                "/api/v1/hello",
                Some("correct-token"),
                Some("http://127.0.0.1:5173"),
                None,
                None,
            )
            .await
            .0,
            401
        );

        let open_body = serde_json::json!({
            "views": [{
                "type": "torrent_list",
                "view_id": "library",
                "delivery": { "min_interval_millis": 0 }
            }],
            "options": {}
        })
        .to_string();
        let (status, body) = http_request(
            address,
            "POST",
            "/api/v1/view-sets",
            Some("correct-token"),
            Some("http://127.0.0.1:5173"),
            Some(open_body),
        )
        .await;
        assert_eq!(status, 201);
        let opened: OpenViewSetResponse = serde_json::from_slice(&body).expect("open response");

        let replay_path = format!(
            "/api/v1/view-sets/{}/updates?after=0&wait_ms=0",
            opened.view_set_id
        );
        assert_eq!(
            http_request_as(
                address,
                "GET",
                &replay_path,
                Some("correct-token"),
                Some("http://127.0.0.1:5173"),
                Some("00000000000000000000000000000002"),
                None,
            )
            .await
            .0,
            404,
            "another browser owner must not acquire a view set by ID"
        );
        let (status, body) = http_request(
            address,
            "GET",
            &replay_path,
            Some("correct-token"),
            Some("http://127.0.0.1:5173"),
            None,
        )
        .await;
        assert_eq!(status, 200);
        let replay: UpdateBatch = serde_json::from_slice(&body).expect("replay response");
        assert_eq!(replay, opened.initial);

        let invalid_wait = format!(
            "/api/v1/view-sets/{}/updates?after={}&wait_ms=20001",
            opened.view_set_id, opened.initial.cursor
        );
        assert_eq!(
            http_request(
                address,
                "GET",
                &invalid_wait,
                Some("correct-token"),
                Some("http://127.0.0.1:5173"),
                None,
            )
            .await
            .0,
            400
        );
        let oversized = format!("{{\"views\":[],\"future\":\"{}\"}}", "x".repeat(70_000));
        assert_eq!(
            http_request(
                address,
                "POST",
                "/api/v1/view-sets",
                Some("correct-token"),
                Some("http://127.0.0.1:5173"),
                Some(oversized),
            )
            .await
            .0,
            400
        );

        let wait_path = format!(
            "/api/v1/view-sets/{}/updates?after={}&wait_ms=20000",
            opened.view_set_id, opened.initial.cursor
        );
        let waiter = tokio::spawn(async move {
            http_request(
                address,
                "GET",
                &wait_path,
                Some("correct-token"),
                Some("http://127.0.0.1:5173"),
                None,
            )
            .await
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        shutdown.cancel();
        let (status, _) = tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .expect("long poll did not wake")
            .expect("long poll task");
        assert_eq!(status, 410);
        task.await
            .expect("server join")
            .expect("server termination");
        service.lock().await.shutdown().await.expect("shutdown");
        drop(service);
        std::fs::remove_dir_all(root).expect("remove root");
    }

    #[tokio::test]
    async fn unauthenticated_development_mode_is_ephemeral_and_origin_bounded() {
        let root = test_root("http-development");
        let service = test_service(&root).await;
        let origin = "http://127.0.0.1:4177";
        let server = bind(
            GatewayConfig::unauthenticated_loopback_development(origin.to_owned()),
            service.clone(),
        )
        .await
        .expect("bind development gateway");
        assert_ne!(server.local_addr().port(), 0);
        let address = server.local_addr();
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(server.serve(shutdown.clone()));

        assert_eq!(
            http_request(address, "GET", "/api/v1/hello", None, Some(origin), None)
                .await
                .0,
            200
        );
        assert_eq!(
            http_request(
                address,
                "GET",
                "/api/v1/hello",
                None,
                Some("http://127.0.0.1:4178"),
                None,
            )
            .await
            .0,
            403
        );

        shutdown.cancel();
        task.await
            .expect("server join")
            .expect("server termination");
        service.lock().await.shutdown().await.expect("shutdown");
        drop(service);
        std::fs::remove_dir_all(root).expect("remove root");
    }

    #[test]
    fn unauthenticated_development_configuration_rejects_fixed_or_remote_scope() {
        let mut fixed =
            GatewayConfig::unauthenticated_loopback_development("http://127.0.0.1:4177".to_owned());
        fixed.bind.set_port(3030);
        assert!(fixed.validate().is_err());
        let remote_origin = GatewayConfig::unauthenticated_loopback_development(
            "https://example.invalid".to_owned(),
        );
        assert!(remote_origin.validate().is_err());
    }

    #[tokio::test]
    async fn http_view_sets_are_isolated_between_gateway_owners() {
        let root = test_root("http-owner");
        let service = test_service(&root).await;
        let first = bind(
            GatewayConfig {
                bind: "127.0.0.1:0".parse().expect("address"),
                authentication: GatewayAuthentication::Bearer {
                    token: "first-token".to_owned(),
                },
                allowed_origin: "http://first.invalid".to_owned(),
                max_connections: 1,
            },
            service.clone(),
        )
        .await
        .expect("bind first");
        let second = bind(
            GatewayConfig {
                bind: "127.0.0.1:0".parse().expect("address"),
                authentication: GatewayAuthentication::Bearer {
                    token: "second-token".to_owned(),
                },
                allowed_origin: "http://second.invalid".to_owned(),
                max_connections: 1,
            },
            service.clone(),
        )
        .await
        .expect("bind second");
        let first_address = first.local_addr();
        let second_address = second.local_addr();
        let first_shutdown = CancellationToken::new();
        let second_shutdown = CancellationToken::new();
        let first_task = tokio::spawn(first.serve(first_shutdown.clone()));
        let second_task = tokio::spawn(second.serve(second_shutdown.clone()));
        let body = serde_json::json!({
            "views": [{
                "type": "torrent_list",
                "view_id": "library",
                "delivery": { "min_interval_millis": 0 }
            }],
            "options": {}
        })
        .to_string();
        let (_, body) = http_request(
            first_address,
            "POST",
            "/api/v1/view-sets",
            Some("first-token"),
            Some("http://first.invalid"),
            Some(body),
        )
        .await;
        let opened: OpenViewSetResponse = serde_json::from_slice(&body).expect("open response");
        let path = format!(
            "/api/v1/view-sets/{}/updates?after=0&wait_ms=0",
            opened.view_set_id
        );
        assert_eq!(
            http_request(
                second_address,
                "GET",
                &path,
                Some("second-token"),
                Some("http://second.invalid"),
                None,
            )
            .await
            .0,
            404
        );

        first_shutdown.cancel();
        second_shutdown.cancel();
        first_task
            .await
            .expect("first server join")
            .expect("first server termination");
        second_task
            .await
            .expect("second server join")
            .expect("second server termination");
        service.lock().await.shutdown().await.expect("shutdown");
        drop(service);
        std::fs::remove_dir_all(root).expect("remove root");
    }
}
