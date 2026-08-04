#![forbid(unsafe_code)]

//! Bounded HTTP and WebSocket adapter for the application contract.

mod application_websocket;

pub use application_websocket::{
    ApplicationClientFrame, ApplicationConnectionError, ApplicationConnectionErrorCode,
    ApplicationConnectionLimits, ApplicationConnectionMetrics,
    ApplicationConnectionMetricsSnapshot, ApplicationFrameMetrics, ApplicationServerFrame,
};

use std::error::Error;
use std::fmt;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::Bytes;
use axum::extract::Request;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use rstorrent_platform::{DownloadDirectoryPicker, NativeDownloadDirectoryPicker, PickerError};
use rstorrent_session::{
    AddTorrentBytesRequest, ApplicationService, CONTROL_VERSION, FileIndexRange,
    FileSelectionIntent, OpenViewSetRequest, RequestEnvelope, StorageRootSnapshot,
    UpdateViewSetRequest, ViewSetError, ViewSetOwner, application_error_response,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use ts_rs::TS;

pub const MAX_INCOMING_MESSAGE_BYTES: usize = 64 * 1024;
pub const MAX_TORRENT_SOURCE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_HTTP_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_CONNECTIONS: usize = 8;
pub const MAX_TOKEN_BYTES: usize = 128;
pub const MAX_ORIGIN_BYTES: usize = 512;
pub const MAX_BASIC_USERNAME_BYTES: usize = 64;
pub const MAX_BASIC_PASSWORD_BYTES: usize = 128;
pub const MAX_BASIC_AUTHORIZATION_BYTES: usize = 512;
pub const MAX_BUILD_ID_BYTES: usize = 128;
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
    Basic(BasicCredentials),
    UnauthenticatedLoopbackDevelopment,
}

#[derive(Clone)]
pub struct BasicCredentials {
    authorization: String,
}

impl GatewayAuthentication {
    pub fn basic(username: &str, password: &str) -> Result<Self, GatewayError> {
        if username.is_empty()
            || username.len() > MAX_BASIC_USERNAME_BYTES
            || username.contains(':')
        {
            return Err(GatewayError::Configuration(format!(
                "basic username must be 1..={MAX_BASIC_USERNAME_BYTES} bytes and contain no colon"
            )));
        }
        if password.is_empty() || password.len() > MAX_BASIC_PASSWORD_BYTES {
            return Err(GatewayError::Configuration(format!(
                "basic password must be 1..={MAX_BASIC_PASSWORD_BYTES} bytes"
            )));
        }
        let encoded = BASE64_STANDARD.encode(format!("{username}:{password}"));
        let authorization = format!("Basic {encoded}");
        if authorization.len() > MAX_BASIC_AUTHORIZATION_BYTES {
            return Err(GatewayError::Configuration(
                "encoded basic credential exceeds its configured bound".to_owned(),
            ));
        }
        Ok(Self::Basic(BasicCredentials { authorization }))
    }
}

impl fmt::Debug for GatewayAuthentication {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bearer { .. } => formatter.write_str("Bearer { token: [redacted] }"),
            Self::Basic(_) => formatter.write_str("Basic { credential: [redacted] }"),
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
        if !matches!(self.authentication, GatewayAuthentication::Basic(_))
            && !self.bind.ip().is_loopback()
        {
            return Err(GatewayError::Configuration(
                "the proof gateway only binds a loopback address".to_owned(),
            ));
        }
        if matches!(self.authentication, GatewayAuthentication::Basic(_))
            && (self.bind.ip().is_unspecified() || self.bind.ip().is_multicast())
        {
            return Err(GatewayError::Configuration(
                "the authenticated host requires one explicit unicast bind address".to_owned(),
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
        if matches!(self.authentication, GatewayAuthentication::Basic(_))
            && !is_https_origin(&self.allowed_origin)
        {
            return Err(GatewayError::Configuration(
                "basic authentication requires one exact HTTPS public origin".to_owned(),
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

#[derive(Clone, Debug)]
pub struct HostedAssets {
    root: PathBuf,
    build_id: String,
}

impl HostedAssets {
    pub fn new(root: PathBuf, build_id: String) -> Result<Self, GatewayError> {
        if !root.is_absolute() || !root.is_dir() || !root.join("index.html").is_file() {
            return Err(GatewayError::Configuration(
                "hosted web root must be an absolute directory containing index.html".to_owned(),
            ));
        }
        if build_id.is_empty()
            || build_id.len() > MAX_BUILD_ID_BYTES
            || !build_id.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(GatewayError::Configuration(format!(
                "hosted build ID must be 1..={MAX_BUILD_ID_BYTES} printable ASCII bytes"
            )));
        }
        Ok(Self { root, build_id })
    }
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
pub struct ChooseDownloadRootRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repair_root: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
pub struct ChooseDownloadRootResponse {
    pub root: Option<StorageRootSnapshot>,
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
    torrent_uploads: Arc<Semaphore>,
    http_owner_namespace: u64,
    connection_registry: application_websocket::ApplicationConnectionRegistry,
    connection_metrics: ApplicationConnectionMetrics,
    gateway_shutdown: CancellationToken,
    download_directory_picker: Arc<dyn DownloadDirectoryPicker>,
    hosted_assets: Option<Arc<HostedAssets>>,
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
    bind_with_picker_and_assets(
        config,
        service,
        Arc::new(NativeDownloadDirectoryPicker),
        None,
    )
    .await
}

pub async fn bind_hosted(
    config: GatewayConfig,
    service: Arc<Mutex<ApplicationService>>,
    assets: HostedAssets,
) -> Result<GatewayServer, GatewayError> {
    if !matches!(config.authentication, GatewayAuthentication::Basic(_)) {
        return Err(GatewayError::Configuration(
            "hosted web assets require basic authentication".to_owned(),
        ));
    }
    bind_with_picker_and_assets(
        config,
        service,
        Arc::new(UnavailableDownloadDirectoryPicker),
        Some(assets),
    )
    .await
}

struct UnavailableDownloadDirectoryPicker;

impl DownloadDirectoryPicker for UnavailableDownloadDirectoryPicker {
    fn choose<'a>(
        &'a self,
        _starting_directory: &'a std::path::Path,
    ) -> rstorrent_platform::PickerFuture<'a> {
        Box::pin(async { Err(PickerError::Unsupported) })
    }
}

#[cfg(test)]
async fn bind_with_picker(
    config: GatewayConfig,
    service: Arc<Mutex<ApplicationService>>,
    download_directory_picker: Arc<dyn DownloadDirectoryPicker>,
) -> Result<GatewayServer, GatewayError> {
    bind_with_picker_and_assets(config, service, download_directory_picker, None).await
}

async fn bind_with_picker_and_assets(
    config: GatewayConfig,
    service: Arc<Mutex<ApplicationService>>,
    download_directory_picker: Arc<dyn DownloadDirectoryPicker>,
    hosted_assets: Option<HostedAssets>,
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
        torrent_uploads: Arc::new(Semaphore::new(1)),
        http_owner_namespace: NEXT_HTTP_OWNER.fetch_add(1, Ordering::Relaxed),
        connection_registry: application_websocket::ApplicationConnectionRegistry::new(),
        connection_metrics: ApplicationConnectionMetrics::default(),
        gateway_shutdown: CancellationToken::new(),
        download_directory_picker,
        hosted_assets: hosted_assets.map(Arc::new),
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

    pub fn connection_metrics(&self) -> ApplicationConnectionMetrics {
        self.state.connection_metrics.clone()
    }

    pub async fn serve(self, shutdown: CancellationToken) -> Result<(), GatewayError> {
        let service = self.state.service.clone();
        let gateway_shutdown = self.state.gateway_shutdown.clone();
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
        let torrent_upload = post(api_torrent_upload)
            .layer(axum::extract::DefaultBodyLimit::max(
                MAX_TORRENT_SOURCE_BYTES,
            ))
            .layer(middleware::from_fn_with_state(
                self.state.clone(),
                admit_torrent_upload,
            ));
        let mut router = Router::new()
            .route(
                "/api/v1/connect",
                get(application_websocket::upgrade_application_connection),
            )
            .route("/api/v1/hello", get(api_hello))
            .route("/api/v1/commands", post(api_command))
            .route("/api/v1/torrents", torrent_upload)
            .route("/api/v1/platform/download-root", post(choose_download_root))
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
            .route("/healthz", get(healthz))
            .layer(axum::extract::DefaultBodyLimit::max(
                MAX_INCOMING_MESSAGE_BYTES,
            ))
            .layer(cors)
            .with_state(self.state.clone());
        if let Some(assets) = &self.state.hosted_assets {
            router = router.fallback_service(ServeDir::new(&assets.root));
        }
        router = router.layer(middleware::from_fn_with_state(
            self.state.clone(),
            require_basic_auth,
        ));
        axum::serve(
            self.listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            shutdown.cancelled().await;
            gateway_shutdown.cancel();
            service.lock().await.close_view_sets();
        })
        .await
        .map_err(GatewayError::Serve)
    }
}

async fn require_basic_auth(
    State(state): State<GatewayState>,
    request: Request,
    next: Next,
) -> Response {
    let GatewayAuthentication::Basic(credentials) = state.authentication.as_ref() else {
        return next.run(request).await;
    };
    let accepted = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.len() <= MAX_BASIC_AUTHORIZATION_BYTES)
        .is_some_and(|value| {
            constant_time_equal(value.as_bytes(), credentials.authorization.as_bytes())
        });
    if accepted {
        next.run(request).await
    } else {
        basic_auth_rejection()
    }
}

async fn admit_torrent_upload(
    State(state): State<GatewayState>,
    request: Request,
    next: Next,
) -> Response {
    if let Err(error) = authenticate_http(&state, request.headers()) {
        return error.into_response();
    }
    if request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        != Some("application/x-bittorrent")
    {
        return invalid_request("torrent upload content type must be application/x-bittorrent");
    }
    let Ok(permit) = state.torrent_uploads.clone().try_acquire_owned() else {
        return api_error(
            StatusCode::TOO_MANY_REQUESTS,
            ApiErrorCode::ResourceLimit,
            "another torrent upload is already in progress",
        );
    };
    let response = next.run(request).await;
    drop(permit);
    response
}

fn basic_auth_rejection() -> Response {
    let mut response = api_error(
        StatusCode::UNAUTHORIZED,
        ApiErrorCode::AuthenticationFailed,
        "gateway credential was rejected",
    );
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Basic realm=\"RSTorrent\", charset=\"UTF-8\""),
    );
    response
}

#[derive(Serialize)]
struct HealthResponse<'a> {
    status: &'static str,
    build_id: &'a str,
}

async fn healthz(State(state): State<GatewayState>) -> Response {
    let Some(assets) = &state.hosted_assets else {
        return StatusCode::NOT_FOUND.into_response();
    };
    json_response(
        StatusCode::OK,
        &HealthResponse {
            status: "ok",
            build_id: &assets.build_id,
        },
    )
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

fn is_https_origin(origin: &str) -> bool {
    let Ok(uri) = origin.parse::<Uri>() else {
        return false;
    };
    if uri.scheme_str() != Some("https") {
        return false;
    }
    let Some(authority) = uri.authority() else {
        return false;
    };
    if authority.host().is_empty() {
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

#[derive(Debug, Deserialize)]
struct TorrentUploadQuery {
    request_id: String,
    storage_root: String,
    #[serde(default)]
    expected_revision: Option<String>,
    #[serde(default = "default_true")]
    start_content: bool,
    #[serde(default)]
    selection: Option<String>,
    wanted_ranges: Option<String>,
}

fn default_true() -> bool {
    true
}

async fn api_hello(State(state): State<GatewayState>, headers: HeaderMap) -> Response {
    if let Err(error) = authenticate_http(&state, &headers) {
        return error.into_response();
    }
    let hello = {
        let service = state.service.lock().await;
        application_websocket::application_hello(&service)
    };
    json_response(StatusCode::OK, &hello)
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

async fn api_torrent_upload(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    query: Result<Query<TorrentUploadQuery>, QueryRejection>,
    body: Result<Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    if let Err(error) = authenticate_http(&state, &headers) {
        return error.into_response();
    }
    let Query(query) = match query {
        Ok(query) => query,
        Err(error) => return invalid_request(error.body_text()),
    };
    let body = match body {
        Ok(body) => body,
        Err(error) => return invalid_request(error.body_text()),
    };
    let selection =
        match parse_upload_selection(query.selection.as_deref(), query.wanted_ranges.as_deref()) {
            Ok(selection) => selection,
            Err(message) => return invalid_request(message),
        };
    let request = AddTorrentBytesRequest {
        version: CONTROL_VERSION,
        request_id: query.request_id,
        expected_revision: query.expected_revision,
        storage_root: query.storage_root,
        start_content: query.start_content,
        selection,
        source_length: body.len() as u32,
        source_sha256: encode_sha256(&Sha256::digest(&body)),
    };
    let request_id = request.request_id.clone();
    let mut service = state.service.lock().await;
    let response = match service.add_torrent_bytes(request, body.into()).await {
        Ok(response) => response,
        Err(error) => {
            application_error_response(request_id, service.revision().unwrap_or(0), &error)
        }
    };
    json_response(StatusCode::OK, &response)
}

fn parse_upload_selection(
    mode: Option<&str>,
    ranges: Option<&str>,
) -> Result<FileSelectionIntent, String> {
    match mode.unwrap_or("all") {
        "all" if ranges.is_none() => Ok(FileSelectionIntent::All),
        "none" if ranges.is_none() => Ok(FileSelectionIntent::None),
        "ranges" => Ok(FileSelectionIntent::WantedRanges {
            ranges: parse_upload_file_ranges(ranges.unwrap_or(""))?,
        }),
        "all" | "none" => Err("wanted_ranges is only valid with selection=ranges".to_owned()),
        _ => Err("selection must be all, none, or ranges".to_owned()),
    }
}

fn parse_upload_file_ranges(value: &str) -> Result<Vec<FileIndexRange>, String> {
    if value.is_empty() {
        return Ok(Vec::new());
    }
    value
        .split(',')
        .map(|part| {
            let (start, end) = part
                .split_once('-')
                .ok_or_else(|| "wanted_ranges must use canonical start-end pairs".to_owned())?;
            Ok(FileIndexRange {
                start: parse_canonical_u32(start, "wanted range start")?,
                end_exclusive: parse_canonical_u32(end, "wanted range end")?,
            })
        })
        .collect()
}

fn parse_canonical_u32(value: &str, label: &str) -> Result<u32, String> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(format!("{label} must be a canonical integer"));
    }
    value
        .parse()
        .map_err(|_| format!("{label} exceeds unsigned 32-bit range"))
}

fn encode_sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[(byte >> 4) as usize]));
        output.push(char::from(HEX[(byte & 0x0f) as usize]));
    }
    output
}

async fn choose_download_root(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    request: Result<Json<ChooseDownloadRootRequest>, JsonRejection>,
) -> Response {
    if let Err(error) = authenticate_http(&state, &headers) {
        return error.into_response();
    }
    let Json(request) = match request {
        Ok(request) => request,
        Err(error) => return invalid_request(error.body_text()),
    };
    let suggested = {
        let service = state.service.lock().await;
        match service.suggested_storage_root_path(request.repair_root.as_deref()) {
            Ok(suggested) => suggested,
            Err(error) => return invalid_request(error.to_string()),
        }
    };
    let Some(starting_directory) = suggested.or_else(home_directory) else {
        return api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiErrorCode::Internal,
            "no usable folder-picker starting directory is available",
        );
    };
    let selected = match state
        .download_directory_picker
        .choose(&starting_directory)
        .await
    {
        Ok(selected) => selected,
        Err(PickerError::Unsupported) => {
            return api_error(
                StatusCode::NOT_IMPLEMENTED,
                ApiErrorCode::Internal,
                "download folder picker is not implemented on this platform",
            );
        }
        Err(error) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiErrorCode::Internal,
                &error.to_string(),
            );
        }
    };
    let root = if let Some(selected) = selected {
        let mut service = state.service.lock().await;
        let result = if let Some(root_id) = request.repair_root.as_deref() {
            service.repair_path_storage_root(root_id, &selected)
        } else {
            service.install_path_storage_root(&selected)
        };
        match result {
            Ok(root) => Some(root),
            Err(error) => {
                return api_error(
                    StatusCode::CONFLICT,
                    ApiErrorCode::InvalidRequest,
                    &error.to_string(),
                );
            }
        }
    } else {
        None
    };
    json_response(StatusCode::OK, &ChooseDownloadRootResponse { root })
}

fn home_directory() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
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
        "gateway-client-{}-{owner}",
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
    use rstorrent_platform::DownloadDirectoryPicker;
    use rstorrent_session::{
        AddTorrentBytesRequest, ApplicationCall, ApplicationCallResult, ApplicationConfig,
        ApplicationService, Command, ConfiguredStorageRoot, FileSelectionIntent, NetworkConfig,
        NetworkPolicy, OpenViewSetOptions, OpenViewSetRequest, OpenViewSetResponse,
        RequestEnvelope, ResponseOutcome, SessionStore, UpdateBatch, UpdateViewSetRequest,
        ViewDeliveryPolicy, ViewSetUpdate, ViewSnapshot, ViewSpec,
    };
    use sha1::{Digest, Sha1};
    use sha2::Sha256;
    use tokio::sync::Mutex;
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_util::sync::CancellationToken;

    use super::{
        ApplicationClientFrame, ApplicationConnectionErrorCode, ApplicationServerFrame,
        ChooseDownloadRootResponse, GatewayAuthentication, GatewayConfig, HostedAssets, bind,
        bind_hosted, bind_with_picker, constant_time_equal,
    };

    static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

    struct FixedDirectoryPicker {
        selected: Option<PathBuf>,
        calls: AtomicU64,
    }

    impl DownloadDirectoryPicker for FixedDirectoryPicker {
        fn choose<'a>(
            &'a self,
            starting_directory: &'a Path,
        ) -> rstorrent_platform::PickerFuture<'a> {
            Box::pin(async move {
                assert!(starting_directory.is_dir());
                self.calls.fetch_add(1, Ordering::Relaxed);
                Ok(self.selected.clone())
            })
        }
    }

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

    async fn read_application_message(
        socket: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    ) -> ApplicationServerFrame {
        let message = tokio::time::timeout(Duration::from_secs(1), socket.next())
            .await
            .expect("application response timed out")
            .expect("application connection closed")
            .expect("application websocket response");
        let Message::Text(text) = message else {
            panic!("expected application text response");
        };
        serde_json::from_str(&text).expect("decode application response")
    }

    async fn send_application_message(
        socket: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        frame: &ApplicationClientFrame,
    ) {
        socket
            .send(Message::Text(
                serde_json::to_string(frame)
                    .expect("encode application frame")
                    .into(),
            ))
            .await
            .expect("send application frame");
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
        let authorization = token.map(|token| format!("Bearer {token}"));
        let (status, _, body) = raw_http_request(
            address,
            method,
            path,
            authorization.as_deref(),
            origin,
            owner,
            body,
        )
        .await;
        (status, body)
    }

    async fn raw_http_request(
        address: SocketAddr,
        method: &str,
        path: &str,
        authorization: Option<&str>,
        origin: Option<&str>,
        owner: Option<&str>,
        body: Option<String>,
    ) -> (u16, String, Vec<u8>) {
        let method = method.to_owned();
        let path = path.to_owned();
        let authorization = authorization.map(str::to_owned);
        let origin = origin.map(str::to_owned);
        let owner = owner.map(str::to_owned);
        tokio::task::spawn_blocking(move || {
            let body = body.unwrap_or_default();
            let mut request = format!(
                "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Length: {}\r\n",
                body.len()
            );
            if let Some(authorization) = authorization {
                request.push_str(&format!("Authorization: {authorization}\r\n"));
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
            (status, headers.to_owned(), response[(split + 4)..].to_vec())
        })
        .await
        .expect("HTTP request task")
    }

    async fn raw_torrent_request(
        address: SocketAddr,
        path: &str,
        token: &str,
        origin: &str,
        content_type: &str,
        body: Vec<u8>,
    ) -> (u16, Vec<u8>) {
        let path = path.to_owned();
        let token = token.to_owned();
        let origin = origin.to_owned();
        let content_type = content_type.to_owned();
        tokio::task::spawn_blocking(move || {
            let headers = format!(
                "POST {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Length: {}\r\nContent-Type: {content_type}\r\nAuthorization: Bearer {token}\r\nOrigin: {origin}\r\nX-RSTorrent-Owner: 00000000000000000000000000000001\r\n\r\n",
                body.len()
            );
            let mut stream = TcpStream::connect(address).expect("connect torrent HTTP route");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("set torrent HTTP timeout");
            stream.write_all(headers.as_bytes()).expect("write headers");
            stream.write_all(&body).expect("write torrent body");
            let mut response = Vec::new();
            if let Err(error) = stream.read_to_end(&mut response) {
                assert_eq!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset,
                    "read torrent HTTP response"
                );
                assert!(!response.is_empty(), "server reset without a response");
            }
            let split = response
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .expect("HTTP header terminator");
            let headers = std::str::from_utf8(&response[..split]).expect("response headers");
            let status = headers
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .and_then(|status| status.parse().ok())
                .expect("response status");
            (status, response[(split + 4)..].to_vec())
        })
        .await
        .expect("torrent HTTP task")
    }

    fn torrent_source(ignored_bytes: usize) -> Vec<u8> {
        let mut source = format!("d7:comment{ignored_bytes}:").into_bytes();
        source.resize(source.len() + ignored_bytes, b'x');
        source.extend_from_slice(
            b"4:infod6:lengthi4e4:name4:test12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaaee",
        );
        source
    }

    fn torrent_upload_request(request_id: &str, source: &[u8]) -> AddTorrentBytesRequest {
        AddTorrentBytesRequest {
            version: rstorrent_session::CONTROL_VERSION,
            request_id: request_id.to_owned(),
            expected_revision: None,
            storage_root: "downloads".to_owned(),
            start_content: false,
            selection: FileSelectionIntent::All,
            source_length: source.len() as u32,
            source_sha256: super::encode_sha256(&Sha256::digest(source)),
        }
    }

    #[test]
    fn credential_comparison_includes_length_and_content() {
        assert!(constant_time_equal(b"token", b"token"));
        assert!(!constant_time_equal(b"token", b"taken"));
        assert!(!constant_time_equal(b"token", b"token-longer"));
    }

    #[test]
    fn basic_credentials_are_bounded_and_redacted() {
        let authentication =
            GatewayAuthentication::basic("preview", "doorstop").expect("valid basic credential");
        assert_eq!(
            format!("{authentication:?}"),
            "Basic { credential: [redacted] }"
        );
        assert!(GatewayAuthentication::basic("", "doorstop").is_err());
        assert!(GatewayAuthentication::basic("bad:name", "doorstop").is_err());
        assert!(GatewayAuthentication::basic("preview", "").is_err());
    }

    #[tokio::test]
    async fn raw_http_torrent_upload_uses_route_only_binary_limit() {
        let root = test_root("http-torrent-upload");
        let service = test_service(&root).await;
        let origin = "http://127.0.0.1:5173";
        let server = bind(
            GatewayConfig {
                bind: "127.0.0.1:0".parse().expect("address"),
                authentication: GatewayAuthentication::Bearer {
                    token: "correct-token".to_owned(),
                },
                allowed_origin: origin.to_owned(),
                max_connections: 2,
            },
            service.clone(),
        )
        .await
        .expect("bind");
        let address = server.local_addr();
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(server.serve(shutdown.clone()));
        let source = torrent_source(96 * 1024);
        let path =
            "/api/v1/torrents?request_id=http-upload&storage_root=downloads&start_content=false";

        let (status, body) = raw_torrent_request(
            address,
            path,
            "correct-token",
            origin,
            "application/x-bittorrent",
            source.clone(),
        )
        .await;
        assert_eq!(status, 200);
        let accepted: rstorrent_session::ResponseEnvelope =
            serde_json::from_slice(&body).expect("torrent response");
        assert!(matches!(accepted.outcome, ResponseOutcome::Success { .. }));
        assert_eq!(accepted.revision, "1");

        let (status, _) = raw_torrent_request(
            address,
            path,
            "correct-token",
            origin,
            "application/octet-stream",
            source,
        )
        .await;
        assert_eq!(status, 400);
        let snapshot = service
            .lock()
            .await
            .storage_snapshot()
            .expect("storage snapshot");
        assert_eq!(snapshot.roots.len(), 1);
        assert_eq!(service.lock().await.revision().expect("revision"), 1);

        shutdown.cancel();
        task.await
            .expect("server join")
            .expect("server termination");
        service.lock().await.shutdown().await.expect("shutdown");
        drop(service);
        std::fs::remove_dir_all(root).expect("remove root");
    }

    #[tokio::test]
    async fn websocket_torrent_upload_requires_ready_and_correlates_result() {
        let root = test_root("websocket-torrent-upload");
        let service = test_service(&root).await;
        let origin = "http://127.0.0.1:5173";
        let server = bind(
            GatewayConfig {
                bind: "127.0.0.1:0".parse().expect("address"),
                authentication: GatewayAuthentication::Bearer {
                    token: "correct-token".to_owned(),
                },
                allowed_origin: origin.to_owned(),
                max_connections: 2,
            },
            service.clone(),
        )
        .await
        .expect("bind");
        let address = server.local_addr();
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(server.serve(shutdown.clone()));
        let mut request = format!("ws://{address}/api/v1/connect")
            .into_client_request()
            .expect("request");
        request
            .headers_mut()
            .insert("Origin", origin.parse().expect("origin"));
        let (mut socket, _) = connect_async(request).await.expect("connect");
        send_application_message(
            &mut socket,
            &ApplicationClientFrame::Connect {
                api_version: rstorrent_session::API_VERSION,
                encoding: rstorrent_session::ApiEncoding::Json,
                client_instance_id: "00000000000000000000000000000001".to_owned(),
                token: Some("correct-token".to_owned()),
            },
        )
        .await;
        assert!(matches!(
            read_application_message(&mut socket).await,
            ApplicationServerFrame::Connected { .. }
        ));
        let source = torrent_source(96 * 1024);
        send_application_message(
            &mut socket,
            &ApplicationClientFrame::BeginTorrentUpload {
                call_id: "upload-call".to_owned(),
                upload_id: "upload-body".to_owned(),
                request: torrent_upload_request("ws-upload", &source),
            },
        )
        .await;
        assert_eq!(
            read_application_message(&mut socket).await,
            ApplicationServerFrame::TorrentUploadReady {
                call_id: "upload-call".to_owned(),
                upload_id: "upload-body".to_owned(),
            }
        );
        socket
            .send(Message::Binary(source.into()))
            .await
            .expect("send torrent bytes");
        let ApplicationServerFrame::Result {
            call_id,
            result: ApplicationCallResult::CommandResponse { response },
        } = read_application_message(&mut socket).await
        else {
            panic!("expected correlated torrent result");
        };
        assert_eq!(call_id, "upload-call");
        assert!(matches!(response.outcome, ResponseOutcome::Success { .. }));
        assert_eq!(response.revision, "1");
        socket.close(None).await.expect("close socket");

        shutdown.cancel();
        task.await
            .expect("server join")
            .expect("server termination");
        service.lock().await.shutdown().await.expect("shutdown");
        drop(service);
        std::fs::remove_dir_all(root).expect("remove root");
    }

    #[tokio::test]
    async fn hosted_assets_require_basic_auth_and_exact_origin() {
        let root = test_root("hosted-assets");
        let web_root = root.join("web");
        std::fs::create_dir_all(web_root.join("assets")).expect("create web root");
        std::fs::write(web_root.join("index.html"), b"private-preview-index").expect("write index");
        std::fs::write(web_root.join("assets/app.js"), b"private-preview-asset")
            .expect("write asset");
        let service = test_service(&root).await;
        let origin = "https://preview.example";
        let authentication =
            GatewayAuthentication::basic("preview", "doorstop").expect("basic auth");
        let authorization = match &authentication {
            GatewayAuthentication::Basic(credentials) => credentials.authorization.clone(),
            _ => unreachable!("constructed basic authentication"),
        };
        let server = bind_hosted(
            GatewayConfig {
                bind: "127.0.0.1:0".parse().expect("address"),
                authentication,
                allowed_origin: origin.to_owned(),
                max_connections: 2,
            },
            service.clone(),
            HostedAssets::new(web_root, "test-build".to_owned()).expect("hosted assets"),
        )
        .await
        .expect("bind hosted server");
        let address = server.local_addr();
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(server.serve(shutdown.clone()));

        let (status, headers, _) =
            raw_http_request(address, "GET", "/", None, None, None, None).await;
        assert_eq!(status, 401);
        assert!(headers.contains("www-authenticate: Basic realm=\"RSTorrent\""));
        assert_eq!(
            raw_http_request(address, "GET", "/", Some("Basic wrong"), None, None, None,)
                .await
                .0,
            401
        );
        let (status, _, body) =
            raw_http_request(address, "GET", "/", Some(&authorization), None, None, None).await;
        assert_eq!(status, 200);
        assert_eq!(body, b"private-preview-index");
        let (status, _, body) = raw_http_request(
            address,
            "GET",
            "/healthz",
            Some(&authorization),
            None,
            None,
            None,
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).expect("health JSON"),
            serde_json::json!({"status": "ok", "build_id": "test-build"})
        );
        assert_eq!(
            raw_http_request(
                address,
                "GET",
                "/assets/missing.js",
                Some(&authorization),
                None,
                None,
                None,
            )
            .await
            .0,
            404
        );

        let mut request = format!("ws://{address}/api/v1/connect")
            .into_client_request()
            .expect("request");
        request
            .headers_mut()
            .insert("Origin", origin.parse().expect("origin"));
        request.headers_mut().insert(
            "Authorization",
            authorization.parse().expect("authorization"),
        );
        let (mut socket, _) = connect_async(request).await.expect("connect");
        send_application_message(
            &mut socket,
            &ApplicationClientFrame::Connect {
                api_version: rstorrent_session::API_VERSION,
                encoding: rstorrent_session::ApiEncoding::Json,
                client_instance_id: "00000000000000000000000000000001".to_owned(),
                token: None,
            },
        )
        .await;
        assert!(matches!(
            read_application_message(&mut socket).await,
            ApplicationServerFrame::Connected { .. }
        ));
        socket.close(None).await.expect("close socket");

        let mut wrong_origin = format!("ws://{address}/api/v1/connect")
            .into_client_request()
            .expect("request");
        wrong_origin
            .headers_mut()
            .insert("Origin", "https://wrong.example".parse().expect("origin"));
        wrong_origin.headers_mut().insert(
            "Authorization",
            authorization.parse().expect("authorization"),
        );
        assert!(connect_async(wrong_origin).await.is_err());

        shutdown.cancel();
        task.await
            .expect("server join")
            .expect("server termination");
        service.lock().await.shutdown().await.expect("shutdown");
        drop(service);
        std::fs::remove_dir_all(root).expect("remove root");
    }

    #[tokio::test]
    async fn application_connection_multiplexes_calls_and_acknowledged_views() {
        let root = test_root("application-websocket");
        let service = test_service(&root).await;
        let origin = "http://127.0.0.1:5173";
        let server = bind(
            GatewayConfig {
                bind: "127.0.0.1:0".parse().expect("address"),
                authentication: GatewayAuthentication::Bearer {
                    token: "correct-token".to_owned(),
                },
                allowed_origin: origin.to_owned(),
                max_connections: 2,
            },
            service.clone(),
        )
        .await
        .expect("bind");
        let address = server.local_addr();
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(server.serve(shutdown.clone()));
        let mut request = format!("ws://{address}/api/v1/connect")
            .into_client_request()
            .expect("request");
        request
            .headers_mut()
            .insert("Origin", origin.parse().expect("origin"));
        let (mut socket, _) = connect_async(request).await.expect("connect");
        send_application_message(
            &mut socket,
            &ApplicationClientFrame::Connect {
                api_version: rstorrent_session::API_VERSION,
                encoding: rstorrent_session::ApiEncoding::Json,
                client_instance_id: "00000000000000000000000000000001".to_owned(),
                token: Some("correct-token".to_owned()),
            },
        )
        .await;
        let ApplicationServerFrame::Connected { hello, .. } =
            read_application_message(&mut socket).await
        else {
            panic!("expected connected response");
        };
        assert!(
            hello
                .deliveries
                .contains(&rstorrent_session::DeliveryMode::Stream)
        );

        let library = ViewSpec::TorrentList {
            view_id: "library".to_owned(),
            delivery: ViewDeliveryPolicy::default(),
        };
        send_application_message(
            &mut socket,
            &ApplicationClientFrame::Call {
                call_id: "open".to_owned(),
                operation: ApplicationCall::OpenViewSet {
                    request: OpenViewSetRequest {
                        views: vec![library.clone()],
                        options: OpenViewSetOptions::default(),
                    },
                },
            },
        )
        .await;
        let ApplicationServerFrame::Result {
            call_id,
            result: ApplicationCallResult::ViewSetOpened { response },
        } = read_application_message(&mut socket).await
        else {
            panic!("expected open result");
        };
        assert_eq!(call_id, "open");
        let opened = *response;
        send_application_message(
            &mut socket,
            &ApplicationClientFrame::Attach {
                call_id: "attach".to_owned(),
                stream_id: "general".to_owned(),
                view_set_id: opened.view_set_id.clone(),
                after: opened.initial.cursor.clone(),
            },
        )
        .await;
        assert!(matches!(
            read_application_message(&mut socket).await,
            ApplicationServerFrame::Attached { ref stream_id, .. } if stream_id == "general"
        ));

        send_application_message(
            &mut socket,
            &ApplicationClientFrame::Call {
                call_id: "update-one".to_owned(),
                operation: ApplicationCall::UpdateViewSet {
                    view_set_id: opened.view_set_id.clone(),
                    request: UpdateViewSetRequest {
                        views: vec![
                            library.clone(),
                            ViewSpec::SessionDisk {
                                view_id: "disk".to_owned(),
                                delivery: ViewDeliveryPolicy::default(),
                            },
                        ],
                    },
                },
            },
        )
        .await;
        let mut first_batch = None;
        let mut update_result = false;
        while first_batch.is_none() || !update_result {
            match read_application_message(&mut socket).await {
                ApplicationServerFrame::ViewBatch { stream_id, batch }
                    if stream_id == "general" =>
                {
                    first_batch = Some(*batch);
                }
                ApplicationServerFrame::Result {
                    call_id,
                    result: ApplicationCallResult::ViewSetUpdated,
                } if call_id == "update-one" => update_result = true,
                other => panic!("unexpected application frame: {other:?}"),
            }
        }
        let first_batch = first_batch.expect("first batch");

        send_application_message(
            &mut socket,
            &ApplicationClientFrame::Call {
                call_id: "snapshot".to_owned(),
                operation: ApplicationCall::Dispatch {
                    request: Box::new(RequestEnvelope {
                        version: rstorrent_session::CONTROL_VERSION,
                        request_id: "snapshot-command".to_owned(),
                        expected_revision: None,
                        command: Command::Snapshot,
                    }),
                },
            },
        )
        .await;
        assert!(matches!(
            read_application_message(&mut socket).await,
            ApplicationServerFrame::Result {
                ref call_id,
                result: ApplicationCallResult::CommandResponse { .. },
            } if call_id == "snapshot"
        ));

        send_application_message(
            &mut socket,
            &ApplicationClientFrame::Ack {
                stream_id: "general".to_owned(),
                cursor: first_batch.cursor,
            },
        )
        .await;
        send_application_message(
            &mut socket,
            &ApplicationClientFrame::Call {
                call_id: "update-two".to_owned(),
                operation: ApplicationCall::UpdateViewSet {
                    view_set_id: opened.view_set_id,
                    request: UpdateViewSetRequest {
                        views: vec![library],
                    },
                },
            },
        )
        .await;
        let mut second_cursor = None;
        while second_cursor.is_none() {
            if let ApplicationServerFrame::ViewBatch { stream_id, batch } =
                read_application_message(&mut socket).await
                && stream_id == "general"
            {
                second_cursor = Some(batch.cursor.clone());
            }
        }
        send_application_message(
            &mut socket,
            &ApplicationClientFrame::Ack {
                stream_id: "general".to_owned(),
                cursor: "999".to_owned(),
            },
        )
        .await;
        loop {
            if matches!(
                read_application_message(&mut socket).await,
                ApplicationServerFrame::StreamError {
                    ref stream_id,
                    ref error,
                } if stream_id == "general"
                    && error.code == ApplicationConnectionErrorCode::InvalidCursor
            ) {
                break;
            }
        }

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
    async fn application_connection_rejects_bad_handshakes_and_repeated_invalid_input() {
        let root = test_root("application-websocket-rejections");
        let service = test_service(&root).await;
        let origin = "http://127.0.0.1:5173";
        let server = bind(
            GatewayConfig {
                bind: "127.0.0.1:0".parse().expect("address"),
                authentication: GatewayAuthentication::Bearer {
                    token: "correct-token".to_owned(),
                },
                allowed_origin: origin.to_owned(),
                max_connections: 4,
            },
            service.clone(),
        )
        .await
        .expect("bind");
        let metrics = server.connection_metrics();
        let address = server.local_addr();
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(server.serve(shutdown.clone()));

        let mut wrong_origin = format!("ws://{address}/api/v1/connect")
            .into_client_request()
            .expect("request");
        wrong_origin.headers_mut().insert(
            "Origin",
            "https://attacker.invalid".parse().expect("origin"),
        );
        let error = connect_async(wrong_origin)
            .await
            .expect_err("wrong origin must not upgrade");
        assert!(matches!(
            error,
            tokio_tungstenite::tungstenite::Error::Http(response)
                if response.status() == axum::http::StatusCode::FORBIDDEN
        ));

        for (api_version, token, expected) in [
            (
                rstorrent_session::API_VERSION,
                Some("wrong-token"),
                ApplicationConnectionErrorCode::AuthenticationFailed,
            ),
            (
                rstorrent_session::API_VERSION + 1,
                Some("correct-token"),
                ApplicationConnectionErrorCode::InvalidVersion,
            ),
        ] {
            let mut request = format!("ws://{address}/api/v1/connect")
                .into_client_request()
                .expect("request");
            request
                .headers_mut()
                .insert("Origin", origin.parse().expect("origin"));
            let (mut socket, _) = connect_async(request).await.expect("connect");
            send_application_message(
                &mut socket,
                &ApplicationClientFrame::Connect {
                    api_version,
                    encoding: rstorrent_session::ApiEncoding::Json,
                    client_instance_id: "00000000000000000000000000000001".to_owned(),
                    token: token.map(str::to_owned),
                },
            )
            .await;
            assert!(matches!(
                read_application_message(&mut socket).await,
                ApplicationServerFrame::ConnectionError { error }
                    if error.code == expected
            ));
        }

        let mut request = format!("ws://{address}/api/v1/connect")
            .into_client_request()
            .expect("request");
        request
            .headers_mut()
            .insert("Origin", origin.parse().expect("origin"));
        let (mut silent_socket, _) = connect_async(request).await.expect("connect");
        let timeout_message = tokio::time::timeout(Duration::from_secs(6), silent_socket.next())
            .await
            .expect("handshake rejection timed out")
            .expect("handshake socket closed without an error")
            .expect("read handshake rejection");
        let Message::Text(timeout_text) = timeout_message else {
            panic!("expected handshake rejection text");
        };
        assert!(matches!(
            serde_json::from_str::<ApplicationServerFrame>(&timeout_text)
                .expect("decode handshake rejection"),
            ApplicationServerFrame::ConnectionError { error }
                if error.code == ApplicationConnectionErrorCode::InvalidMessage
        ));

        let mut request = format!("ws://{address}/api/v1/connect")
            .into_client_request()
            .expect("request");
        request
            .headers_mut()
            .insert("Origin", origin.parse().expect("origin"));
        let (mut socket, _) = connect_async(request).await.expect("connect");
        send_application_message(
            &mut socket,
            &ApplicationClientFrame::Connect {
                api_version: rstorrent_session::API_VERSION,
                encoding: rstorrent_session::ApiEncoding::Json,
                client_instance_id: "00000000000000000000000000000001".to_owned(),
                token: Some("correct-token".to_owned()),
            },
        )
        .await;
        assert!(matches!(
            read_application_message(&mut socket).await,
            ApplicationServerFrame::Connected { .. }
        ));
        for _ in 0..3 {
            socket
                .send(Message::Text("{}".into()))
                .await
                .expect("send invalid frame");
            assert!(matches!(
                read_application_message(&mut socket).await,
                ApplicationServerFrame::ConnectionError { error }
                    if error.code == ApplicationConnectionErrorCode::InvalidMessage
            ));
        }
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(1), socket.next())
                .await
                .expect("protocol close timed out")
                .expect("connection ended before close")
                .expect("read close"),
            Message::Close(_)
        ));

        shutdown.cancel();
        task.await
            .expect("server join")
            .expect("server termination");
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.rejected_origins, 1);
        assert_eq!(snapshot.rejected_authentication, 1);
        assert_eq!(snapshot.rejected_handshakes, 2);
        assert_eq!(snapshot.accepted_connections, 1);
        assert_eq!(snapshot.active_connections, 0);
        service.lock().await.shutdown().await.expect("shutdown");
        drop(service);
        std::fs::remove_dir_all(root).expect("remove root");
    }

    #[tokio::test]
    async fn large_file_snapshot_keeps_command_latency_bounded() {
        let root = test_root("large-application-frame");
        let profile = root.join("profile");
        let payload = root.join("payload");
        let configured = ConfiguredStorageRoot::path("downloads", payload.clone());
        let raw_info = large_file_raw_info();
        let torrent_id = hex_digest(Sha1::digest(&raw_info).as_slice());
        let magnet = format!("magnet:?xt=urn:btih:{torrent_id}&x.pe=127.0.0.1:1");
        {
            let mut store = SessionStore::open(&profile, "test", std::slice::from_ref(&configured))
                .expect("open seeded store");
            let response = store
                .handle_durable(&RequestEnvelope {
                    version: rstorrent_session::CONTROL_VERSION,
                    request_id: "seed-large-files".to_owned(),
                    expected_revision: None,
                    command: Command::AddMagnet {
                        magnet,
                        storage_root: "downloads".to_owned(),
                        start_content: true,
                        skip_files: Vec::new(),
                    },
                })
                .expect("seed torrent");
            assert!(matches!(
                response.outcome,
                rstorrent_session::ResponseOutcome::Success { .. }
            ));
            store
                .record_metadata(&torrent_id, &raw_info)
                .expect("record large metadata");
        }
        let service = Arc::new(Mutex::new(
            ApplicationService::open(ApplicationConfig::new(
                profile,
                "test".to_owned(),
                vec![configured],
                NetworkConfig::new(
                    NetworkPolicy::LoopbackOnly,
                    Duration::from_secs(1),
                    Duration::from_secs(1),
                ),
            ))
            .await
            .expect("open application"),
        ));
        let origin = "http://127.0.0.1:5173";
        let server = bind(
            GatewayConfig {
                bind: "127.0.0.1:0".parse().expect("address"),
                authentication: GatewayAuthentication::Bearer {
                    token: "correct-token".to_owned(),
                },
                allowed_origin: origin.to_owned(),
                max_connections: 1,
            },
            service.clone(),
        )
        .await
        .expect("bind");
        let metrics = server.connection_metrics();
        let address = server.local_addr();
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(server.serve(shutdown.clone()));
        let mut request = format!("ws://{address}/api/v1/connect")
            .into_client_request()
            .expect("request");
        request
            .headers_mut()
            .insert("Origin", origin.parse().expect("origin"));
        let (mut socket, _) = connect_async(request).await.expect("connect");
        send_application_message(
            &mut socket,
            &ApplicationClientFrame::Connect {
                api_version: rstorrent_session::API_VERSION,
                encoding: rstorrent_session::ApiEncoding::Json,
                client_instance_id: "00000000000000000000000000000001".to_owned(),
                token: Some("correct-token".to_owned()),
            },
        )
        .await;
        assert!(matches!(
            read_application_message(&mut socket).await,
            ApplicationServerFrame::Connected { .. }
        ));
        send_application_message(
            &mut socket,
            &ApplicationClientFrame::Call {
                call_id: "large-open".to_owned(),
                operation: ApplicationCall::OpenViewSet {
                    request: OpenViewSetRequest {
                        views: vec![ViewSpec::TorrentFiles {
                            view_id: "files".to_owned(),
                            torrent_id: torrent_id.clone(),
                            page: None,
                            delivery: ViewDeliveryPolicy::default(),
                        }],
                        options: OpenViewSetOptions::default(),
                    },
                },
            },
        )
        .await;
        let command_started = std::time::Instant::now();
        send_application_message(
            &mut socket,
            &ApplicationClientFrame::Call {
                call_id: "small-command".to_owned(),
                operation: ApplicationCall::Dispatch {
                    request: Box::new(RequestEnvelope {
                        version: rstorrent_session::CONTROL_VERSION,
                        request_id: "large-frame-snapshot".to_owned(),
                        expected_revision: None,
                        command: Command::Snapshot,
                    }),
                },
            },
        )
        .await;
        let mut large_bytes = None;
        let mut command_latency = None;
        while large_bytes.is_none() || command_latency.is_none() {
            match read_application_message(&mut socket).await {
                ApplicationServerFrame::Result {
                    call_id,
                    result: ApplicationCallResult::ViewSetOpened { response },
                } if call_id == "large-open" => {
                    let catalog = response
                        .initial
                        .updates
                        .iter()
                        .find_map(|update| match update {
                            ViewSetUpdate::Snapshot {
                                snapshot: ViewSnapshot::Files { files, page, .. },
                                ..
                            } => Some((files.len(), page.total)),
                            _ => None,
                        });
                    assert_eq!(catalog, Some((1_024, 4_096)));
                    large_bytes = Some(
                        serde_json::to_vec(&response)
                            .expect("encode large response")
                            .len(),
                    );
                }
                ApplicationServerFrame::Result {
                    call_id,
                    result: ApplicationCallResult::CommandResponse { .. },
                } if call_id == "small-command" => {
                    command_latency = Some(command_started.elapsed());
                }
                other => panic!("unexpected application frame: {other:?}"),
            }
        }
        let large_bytes = large_bytes.expect("large response bytes");
        let command_latency = command_latency.expect("command latency");
        assert!(
            large_bytes > 256 * 1_024,
            "catalog page response was {large_bytes} bytes"
        );
        assert!(
            command_latency < Duration::from_secs(2),
            "small command took {command_latency:?} beside the large frame"
        );

        socket.close(None).await.expect("close");
        shutdown.cancel();
        task.await
            .expect("server join")
            .expect("server termination");
        let snapshot = metrics.snapshot();
        assert!(snapshot.outbound_message_bytes_high_water > 256 * 1_024);
        assert_eq!(snapshot.active_connections, 0);
        service.lock().await.shutdown().await.expect("shutdown");
        drop(service);
        std::fs::remove_dir_all(root).expect("remove root");
        eprintln!(
            "large_application_frame encoded_bytes={large_bytes} command_latency_micros={}",
            command_latency.as_micros()
        );
    }

    fn large_file_raw_info() -> Vec<u8> {
        let mut output = Vec::new();
        output.extend_from_slice(b"d5:filesl");
        for index in 0..4_096_u32 {
            output.extend_from_slice(b"d6:lengthi1e4:pathl");
            push_bencode_bytes(&mut output, format!("directory-{index:04}").as_bytes());
            push_bencode_bytes(&mut output, "x".repeat(128).as_bytes());
            output.extend_from_slice(b"ee");
        }
        output.extend_from_slice(b"e4:name13:large-fixture12:piece lengthi16384e6:pieces20:");
        output.extend_from_slice(&[0_u8; 20]);
        output.push(b'e');
        output
    }

    fn push_bencode_bytes(output: &mut Vec<u8>, value: &[u8]) {
        output.extend_from_slice(value.len().to_string().as_bytes());
        output.push(b':');
        output.extend_from_slice(value);
    }

    fn hex_digest(bytes: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut output = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            output.push(char::from(HEX[usize::from(byte >> 4)]));
            output.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        output
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
        let url = format!("ws://{address}/api/v1/connect");

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
        send_application_message(
            &mut socket,
            &ApplicationClientFrame::Connect {
                api_version: rstorrent_session::API_VERSION,
                encoding: rstorrent_session::ApiEncoding::Json,
                client_instance_id: "00000000000000000000000000000001".to_owned(),
                token: Some("wrong-token".to_owned()),
            },
        )
        .await;
        assert!(matches!(
            read_application_message(&mut socket).await,
            ApplicationServerFrame::ConnectionError { error }
                if error.code == ApplicationConnectionErrorCode::AuthenticationFailed
        ));

        let mut request = url.into_client_request().expect("request");
        request
            .headers_mut()
            .insert("Origin", "http://127.0.0.1:5173".parse().expect("origin"));
        let (mut socket, _) = connect_async(request).await.expect("reconnect");
        send_application_message(
            &mut socket,
            &ApplicationClientFrame::Connect {
                api_version: rstorrent_session::API_VERSION,
                encoding: rstorrent_session::ApiEncoding::Json,
                client_instance_id: "00000000000000000000000000000002".to_owned(),
                token: Some("correct-token".to_owned()),
            },
        )
        .await;
        assert!(matches!(
            read_application_message(&mut socket).await,
            ApplicationServerFrame::Connected { .. }
        ));
        send_application_message(
            &mut socket,
            &ApplicationClientFrame::Call {
                call_id: "snapshot-call".to_owned(),
                operation: ApplicationCall::Dispatch {
                    request: Box::new(RequestEnvelope {
                        version: rstorrent_session::CONTROL_VERSION,
                        request_id: "snapshot".to_owned(),
                        expected_revision: None,
                        command: Command::Snapshot,
                    }),
                },
            },
        )
        .await;
        let ApplicationServerFrame::Result {
            result: ApplicationCallResult::CommandResponse { response },
            ..
        } = read_application_message(&mut socket).await
        else {
            panic!("expected command response");
        };
        assert!(matches!(
            response.outcome,
            rstorrent_session::ResponseOutcome::Success { ref snapshot }
                if snapshot.torrents.is_empty()
        ));
        assert_eq!(
            http_request(
                address,
                "GET",
                "/control",
                Some("correct-token"),
                Some("http://127.0.0.1:5173"),
                None,
            )
            .await
            .0,
            404
        );

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
    async fn folder_picker_requires_http_auth_and_installs_selected_root() {
        let root = test_root("folder-picker");
        let service = test_service(&root).await;
        let selected = root.join("selected downloads");
        std::fs::create_dir_all(&selected).expect("create selection");
        let picker = Arc::new(FixedDirectoryPicker {
            selected: Some(selected.clone()),
            calls: AtomicU64::new(0),
        });
        let server = bind_with_picker(
            GatewayConfig {
                bind: "127.0.0.1:0".parse().expect("address"),
                authentication: GatewayAuthentication::Bearer {
                    token: "correct-token".to_owned(),
                },
                allowed_origin: "http://127.0.0.1:5173".to_owned(),
                max_connections: 2,
            },
            service.clone(),
            picker.clone(),
        )
        .await
        .expect("bind");
        let address = server.local_addr();
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(server.serve(shutdown.clone()));

        assert_eq!(
            http_request(
                address,
                "POST",
                "/api/v1/platform/download-root",
                Some("correct-token"),
                Some("https://attacker.invalid"),
                Some("{}".to_owned()),
            )
            .await
            .0,
            403
        );
        assert_eq!(picker.calls.load(Ordering::Relaxed), 0);

        let (status, body) = http_request(
            address,
            "POST",
            "/api/v1/platform/download-root",
            Some("correct-token"),
            Some("http://127.0.0.1:5173"),
            Some("{}".to_owned()),
        )
        .await;
        assert_eq!(status, 200);
        let response: ChooseDownloadRootResponse =
            serde_json::from_slice(&body).expect("folder picker response");
        let installed = response.root.expect("installed root");
        assert_eq!(installed.label, "selected downloads");
        assert_eq!(
            installed.display_path.as_deref(),
            selected
                .canonicalize()
                .expect("canonical selected path")
                .to_str()
        );
        assert_eq!(picker.calls.load(Ordering::Relaxed), 1);

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
