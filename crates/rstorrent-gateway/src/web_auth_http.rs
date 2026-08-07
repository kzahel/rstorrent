use std::sync::MutexGuard;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use super::web_auth::{INITIAL_WINDOW_SECONDS, SESSION_IDLE_SECONDS};
use super::{
    ApiErrorCode, AuthorizedWebSession, GatewayError, GatewayState, IssuedWebSession,
    WebAccessPolicy, WebAuthError, WebAuthStore, WebAuthenticationConfig, api_error, json_response,
};

const SESSION_COOKIE: &str = "rstorrent_web_session";
const MAX_COOKIE_HEADER_BYTES: usize = 4096;
const MAX_AUTH_BODY_LABEL_BYTES: usize = 80;

pub(crate) struct WebAuthRuntime {
    store: WebAuthStore,
    policy_override: Option<WebAccessPolicy>,
    initial_deadline: Option<Instant>,
    recovery_deadline: Option<Instant>,
    recovery_consumed: bool,
}

impl WebAuthRuntime {
    pub(crate) fn open(config: &WebAuthenticationConfig) -> Result<Self, GatewayError> {
        let store = WebAuthStore::open(&config.database).map_err(|error| {
            GatewayError::Configuration(format!("open browser authentication state: {error}"))
        })?;
        let policy = store.policy().map_err(|error| {
            GatewayError::Configuration(format!("read browser authentication state: {error}"))
        })?;
        let effective_policy = config.policy_override.unwrap_or(policy);
        if config.policy_override == Some(WebAccessPolicy::Paired)
            && policy != WebAccessPolicy::Paired
        {
            return Err(GatewayError::Configuration(
                "paired override requires an already paired profile".to_owned(),
            ));
        }
        if config.pairing_window && effective_policy != WebAccessPolicy::Paired {
            return Err(GatewayError::Configuration(
                "--pairing-window requires an already paired profile".to_owned(),
            ));
        }
        let now = Instant::now();
        Ok(Self {
            store,
            policy_override: config.policy_override,
            initial_deadline: (effective_policy == WebAccessPolicy::Unconfigured)
                .then(|| now + Duration::from_secs(INITIAL_WINDOW_SECONDS as u64)),
            recovery_deadline: config
                .pairing_window
                .then(|| now + Duration::from_secs(INITIAL_WINDOW_SECONDS as u64)),
            recovery_consumed: false,
        })
    }

    fn policy(&self) -> Result<WebAccessPolicy, WebAuthError> {
        self.policy_override.map_or_else(|| self.store.policy(), Ok)
    }

    fn initial_remaining(&self) -> Option<u64> {
        remaining_seconds(self.initial_deadline?)
    }

    fn recovery_remaining(&self) -> Option<u64> {
        if self.recovery_consumed {
            return None;
        }
        remaining_seconds(self.recovery_deadline?)
    }
}

#[derive(Serialize)]
struct AuthStatusResponse {
    available: bool,
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_session: Option<SessionResponse>,
}

#[derive(Clone, Serialize)]
struct SessionResponse {
    id: String,
    label: String,
    created_at: i64,
    last_used_at: i64,
    expires_at: i64,
    current: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PolicyRequest {
    policy: RequestedPolicy,
    label: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum RequestedPolicy {
    LocalOpen,
    Paired,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BrowserLabelRequest {
    label: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RedeemRequest {
    code: String,
    label: String,
}

#[derive(Serialize)]
struct PairingTicketResponse {
    code: String,
    expires_at: i64,
}

#[derive(Serialize)]
struct SessionsResponse {
    sessions: Vec<SessionResponse>,
}

#[derive(Serialize)]
struct ChangedResponse {
    changed: usize,
}

#[derive(Clone, Copy)]
enum SessionRequirementError {
    Unavailable,
    Unauthorized,
    Internal,
}

impl SessionRequirementError {
    fn into_response(self) -> Response {
        match self {
            Self::Unavailable => StatusCode::NOT_FOUND.into_response(),
            Self::Unauthorized => unauthorized(),
            Self::Internal => internal_error(),
        }
    }
}

pub(crate) async fn status(State(state): State<GatewayState>, headers: HeaderMap) -> Response {
    let Some(runtime) = &state.web_auth else {
        return json_response(
            StatusCode::OK,
            &AuthStatusResponse {
                available: false,
                state: "unavailable",
                remaining_seconds: None,
                current_session: None,
            },
        );
    };
    let mut runtime = match runtime.lock() {
        Ok(runtime) => runtime,
        Err(_) => return internal_error(),
    };
    let now = unix_seconds();
    let current = optional_session(&mut runtime, &headers, now);
    let policy = match runtime.policy() {
        Ok(policy) => policy,
        Err(_) => return internal_error(),
    };
    let (state_name, remaining) = match policy {
        WebAccessPolicy::Unconfigured => match runtime.initial_remaining() {
            Some(remaining) => ("initial_window_open", Some(remaining)),
            None => ("initial_window_expired", None),
        },
        WebAccessPolicy::LocalOpen => ("local_open", None),
        WebAccessPolicy::Paired if current.is_some() => ("session_valid", None),
        WebAccessPolicy::Paired => match runtime.recovery_remaining() {
            Some(remaining) => ("recovery_window_open", Some(remaining)),
            None => ("session_required", None),
        },
    };
    json_response(
        StatusCode::OK,
        &AuthStatusResponse {
            available: true,
            state: state_name,
            remaining_seconds: remaining,
            current_session: current.map(|session| session_response(session, true)),
        },
    )
}

pub(crate) async fn set_policy(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    request: Result<Json<PolicyRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if !origin_matches(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Json(request) = match request {
        Ok(request) => request,
        Err(error) => return bad_request(error.body_text()),
    };
    let Some(runtime) = &state.web_auth else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut runtime = match runtime.lock() {
        Ok(runtime) => runtime,
        Err(_) => return internal_error(),
    };
    if runtime.policy_override.is_some() {
        return api_error(
            StatusCode::CONFLICT,
            ApiErrorCode::InvalidRequest,
            "web access policy is fixed by the command line",
        );
    }
    let now = unix_seconds();
    let current = optional_session(&mut runtime, &headers, now);
    let policy = match runtime.policy() {
        Ok(policy) => policy,
        Err(_) => return internal_error(),
    };
    let permitted = match policy {
        WebAccessPolicy::Unconfigured => runtime.initial_remaining().is_some() || current.is_some(),
        WebAccessPolicy::LocalOpen => true,
        WebAccessPolicy::Paired => current.is_some(),
    };
    if !permitted {
        return unauthorized();
    }
    match (policy, request.policy) {
        (WebAccessPolicy::Unconfigured, RequestedPolicy::LocalOpen) => {
            match runtime.store.commit_initial_local_open() {
                Ok(()) => StatusCode::NO_CONTENT.into_response(),
                Err(error) => auth_error(error),
            }
        }
        (WebAccessPolicy::Unconfigured, RequestedPolicy::Paired) => {
            let label = match bounded_label(request.label) {
                Ok(label) => label,
                Err(message) => return bad_request(message),
            };
            match runtime.store.commit_initial_paired(&label, now) {
                Ok(session) => session_response_with_cookie(&state, session),
                Err(error) => auth_error(error),
            }
        }
        (WebAccessPolicy::LocalOpen, RequestedPolicy::LocalOpen)
        | (WebAccessPolicy::Paired, RequestedPolicy::Paired) => {
            StatusCode::NO_CONTENT.into_response()
        }
        (WebAccessPolicy::LocalOpen, RequestedPolicy::Paired) => {
            let label = match bounded_label(request.label) {
                Ok(label) => label,
                Err(message) => return bad_request(message),
            };
            match runtime.store.enable_paired(&label, now) {
                Ok(session) => session_response_with_cookie(&state, session),
                Err(error) => auth_error(error),
            }
        }
        (WebAccessPolicy::Paired, RequestedPolicy::LocalOpen) => {
            match runtime.store.enable_local_open() {
                Ok(()) => StatusCode::NO_CONTENT.into_response(),
                Err(error) => auth_error(error),
            }
        }
    }
}

pub(crate) async fn claim_recovery_window(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    request: Result<Json<BrowserLabelRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if !origin_matches(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Json(request) = match request {
        Ok(request) => request,
        Err(error) => return bad_request(error.body_text()),
    };
    let label = match bounded_label(Some(request.label)) {
        Ok(label) => label,
        Err(message) => return bad_request(message),
    };
    let Some(runtime) = &state.web_auth else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut runtime = match runtime.lock() {
        Ok(runtime) => runtime,
        Err(_) => return internal_error(),
    };
    if runtime.recovery_remaining().is_none() {
        return unauthorized();
    }
    match runtime.store.issue_session(&label, unix_seconds()) {
        Ok(session) => {
            runtime.recovery_consumed = true;
            session_response_with_cookie(&state, session)
        }
        Err(error) => auth_error(error),
    }
}

pub(crate) async fn create_pairing_ticket(
    State(state): State<GatewayState>,
    headers: HeaderMap,
) -> Response {
    if !origin_matches(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let (mut runtime, _) = match require_session(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    match runtime.store.create_pairing_ticket(unix_seconds()) {
        Ok(ticket) => json_response(
            StatusCode::CREATED,
            &PairingTicketResponse {
                code: ticket.code,
                expires_at: ticket.expires_at,
            },
        ),
        Err(error) => auth_error(error),
    }
}

pub(crate) async fn redeem_pairing_ticket(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    request: Result<Json<RedeemRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if !origin_matches(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Json(request) = match request {
        Ok(request) => request,
        Err(error) => return bad_request(error.body_text()),
    };
    let label = match bounded_label(Some(request.label)) {
        Ok(label) => label,
        Err(message) => return bad_request(message),
    };
    let Some(runtime) = &state.web_auth else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut runtime = match runtime.lock() {
        Ok(runtime) => runtime,
        Err(_) => return internal_error(),
    };
    match runtime
        .store
        .redeem_pairing_ticket(&request.code, &label, unix_seconds())
    {
        Ok(session) => session_response_with_cookie(&state, session),
        Err(error) => auth_error(error),
    }
}

pub(crate) async fn sessions(State(state): State<GatewayState>, headers: HeaderMap) -> Response {
    let (runtime, current) = match require_session(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    match runtime.store.list_sessions(unix_seconds()) {
        Ok(sessions) => json_response(
            StatusCode::OK,
            &SessionsResponse {
                sessions: sessions
                    .into_iter()
                    .map(|session| {
                        let is_current = session.id == current.id;
                        session_response(session, is_current)
                    })
                    .collect(),
            },
        ),
        Err(error) => auth_error(error),
    }
}

pub(crate) async fn revoke_session(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Response {
    if !origin_matches(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let (mut runtime, current) = match require_session(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    if current.id == session_id {
        return bad_request("sign out the current browser instead");
    }
    match runtime.store.revoke_session(&session_id) {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => auth_error(error),
    }
}

pub(crate) async fn revoke_other_sessions(
    State(state): State<GatewayState>,
    headers: HeaderMap,
) -> Response {
    if !origin_matches(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let (mut runtime, current) = match require_session(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    match runtime.store.revoke_all_other_sessions(&current.id) {
        Ok(changed) => json_response(StatusCode::OK, &ChangedResponse { changed }),
        Err(error) => auth_error(error),
    }
}

pub(crate) async fn logout(State(state): State<GatewayState>, headers: HeaderMap) -> Response {
    if !origin_matches(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let (mut runtime, current) = match require_session(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    match runtime.store.revoke_session(&current.id) {
        Ok(_) => expire_cookie(StatusCode::NO_CONTENT.into_response()),
        Err(error) => auth_error(error),
    }
}

pub(crate) fn authenticate_application_request(
    state: &GatewayState,
    headers: &HeaderMap,
) -> Result<Option<String>, super::HttpAuthError> {
    let Some(runtime) = &state.web_auth else {
        return Err(super::HttpAuthError::Credential);
    };
    let mut runtime = runtime
        .lock()
        .map_err(|_| super::HttpAuthError::Credential)?;
    let policy = runtime
        .policy()
        .map_err(|_| super::HttpAuthError::Credential)?;
    if policy == WebAccessPolicy::LocalOpen
        || (policy == WebAccessPolicy::Unconfigured && runtime.initial_remaining().is_some())
    {
        return Ok(None);
    }
    let token = session_cookie(headers).ok_or(super::HttpAuthError::Credential)?;
    runtime
        .store
        .authenticate(token, unix_seconds())
        .map(|session| Some(session.id))
        .map_err(|_| super::HttpAuthError::Credential)
}

pub(crate) fn session_is_active(state: &GatewayState, session_id: &str) -> bool {
    state
        .web_auth
        .as_ref()
        .and_then(|runtime| runtime.lock().ok())
        .and_then(|runtime| {
            runtime
                .store
                .session_is_active(session_id, unix_seconds())
                .ok()
        })
        .unwrap_or(false)
}

pub(crate) fn host_matches_origin(state: &GatewayState, headers: &HeaderMap) -> bool {
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    host == state.allowed_host.as_ref()
        || state
            .allowed_origin
            .parse::<Uri>()
            .ok()
            .and_then(|origin| {
                origin
                    .authority()
                    .map(|authority| authority.as_str() == host)
            })
            .unwrap_or(false)
}

fn require_session<'a>(
    state: &'a GatewayState,
    headers: &HeaderMap,
) -> Result<(MutexGuard<'a, WebAuthRuntime>, AuthorizedWebSession), SessionRequirementError> {
    let Some(runtime) = &state.web_auth else {
        return Err(SessionRequirementError::Unavailable);
    };
    let mut runtime = runtime
        .lock()
        .map_err(|_| SessionRequirementError::Internal)?;
    let token = session_cookie(headers).ok_or(SessionRequirementError::Unauthorized)?;
    let current = runtime
        .store
        .authenticate(token, unix_seconds())
        .map_err(|_| SessionRequirementError::Unauthorized)?;
    Ok((runtime, current))
}

fn optional_session(
    runtime: &mut WebAuthRuntime,
    headers: &HeaderMap,
    now: i64,
) -> Option<AuthorizedWebSession> {
    session_cookie(headers).and_then(|token| runtime.store.authenticate(token, now).ok())
}

fn origin_matches(state: &GatewayState, headers: &HeaderMap) -> bool {
    headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        == Some(state.allowed_origin.as_ref())
}

fn session_cookie(headers: &HeaderMap) -> Option<&str> {
    let cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    if cookie.len() > MAX_COOKIE_HEADER_BYTES {
        return None;
    }
    cookie.split(';').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        (name == SESSION_COOKIE && !value.is_empty()).then_some(value)
    })
}

fn bounded_label(label: Option<String>) -> Result<String, &'static str> {
    let label = label.unwrap_or_else(|| "Browser".to_owned());
    if label.is_empty()
        || label.len() > MAX_AUTH_BODY_LABEL_BYTES
        || label.chars().any(char::is_control)
    {
        return Err("browser label is invalid");
    }
    Ok(label)
}

fn session_response(session: AuthorizedWebSession, current: bool) -> SessionResponse {
    SessionResponse {
        id: session.id,
        label: session.label,
        created_at: session.created_at,
        last_used_at: session.last_used_at,
        expires_at: session.expires_at,
        current,
    }
}

fn session_response_with_cookie(state: &GatewayState, session: IssuedWebSession) -> Response {
    let body = SessionResponse {
        id: session.id,
        label: session.label,
        created_at: session.created_at,
        last_used_at: session.last_used_at,
        expires_at: session.expires_at,
        current: true,
    };
    let mut response = json_response(StatusCode::CREATED, &body);
    let secure = state.allowed_origin.starts_with("https://");
    let value = format!(
        "{SESSION_COOKIE}={}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}{}",
        session.token,
        SESSION_IDLE_SECONDS,
        if secure { "; Secure" } else { "" }
    );
    if let Ok(value) = HeaderValue::from_str(&value) {
        response.headers_mut().append(header::SET_COOKIE, value);
    } else {
        return internal_error();
    }
    response
}

fn expire_cookie(mut response: Response) -> Response {
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_static(
            "rstorrent_web_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
        ),
    );
    response
}

fn remaining_seconds(deadline: Instant) -> Option<u64> {
    let remaining = deadline.checked_duration_since(Instant::now())?;
    Some(
        remaining
            .as_secs()
            .saturating_add(u64::from(remaining.subsec_nanos() > 0)),
    )
}

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}

fn auth_error(error: WebAuthError) -> Response {
    match error {
        WebAuthError::InvalidLabel | WebAuthError::InvalidCode => bad_request(error.to_string()),
        WebAuthError::NoPairingTicket
        | WebAuthError::TicketExpired
        | WebAuthError::TicketAttemptsExhausted
        | WebAuthError::InvalidSession
        | WebAuthError::SessionExpired => unauthorized(),
        WebAuthError::SessionLimit | WebAuthError::PolicyAlreadyConfigured => api_error(
            StatusCode::CONFLICT,
            ApiErrorCode::ResourceLimit,
            &error.to_string(),
        ),
        WebAuthError::Storage(_) | WebAuthError::Random(_) | WebAuthError::Corrupt(_) => {
            internal_error()
        }
    }
}

fn bad_request(message: impl AsRef<str>) -> Response {
    api_error(
        StatusCode::BAD_REQUEST,
        ApiErrorCode::InvalidRequest,
        message.as_ref(),
    )
}

fn unauthorized() -> Response {
    api_error(
        StatusCode::UNAUTHORIZED,
        ApiErrorCode::AuthenticationFailed,
        "browser authorization is required",
    )
}

fn internal_error() -> Response {
    api_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        ApiErrorCode::Internal,
        "browser authentication state is unavailable",
    )
}
