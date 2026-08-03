use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use rstorrent_session::{
    API_VERSION, AcknowledgedViewStream, AcknowledgedViewStreamError, ApiEncoding, ApiHello,
    ApplicationCall, ApplicationCallError, ApplicationCallResult, ApplicationService, DeliveryMode,
    UpdateBatch, ViewSetError, ViewSetOwner,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore, mpsc};
use tokio::task::{JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use super::{
    GatewayAuthentication, GatewayState, MAX_INCOMING_MESSAGE_BYTES, MAX_TOKEN_BYTES,
    constant_time_equal, valid_view_set_id,
};

pub const MAX_APPLICATION_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_SERVER_ENVELOPE_BYTES: usize = 4 * 1024;
pub const MAX_SERVER_MESSAGE_BYTES: usize =
    MAX_APPLICATION_PAYLOAD_BYTES + MAX_SERVER_ENVELOPE_BYTES;
pub const MAX_PENDING_CALLS: usize = 16;
pub const MAX_ATTACHMENTS_PER_CONNECTION: usize = 8;
pub const MAX_CONTROL_MESSAGES: usize = 32;
pub const MAX_IDENTIFIER_BYTES: usize = 64;
pub const CLIENT_INSTANCE_ID_BYTES: usize = 32;
pub const HANDSHAKE_TIMEOUT_MILLIS: u64 = 5_000;
pub const HEARTBEAT_IDLE_MILLIS: u64 = 15_000;
pub const HEARTBEAT_TIMEOUT_MILLIS: u64 = 10_000;
pub const MAX_INVALID_MESSAGES: u8 = 3;

const VIEW_STREAM_WAIT_MILLIS: u32 = 20_000;
const DATA_RESERVATIONS: usize = 2;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
pub struct ApplicationConnectionLimits {
    pub max_attachments: u16,
    pub max_pending_calls: u16,
    pub max_client_message_bytes: u32,
    pub max_application_payload_bytes: u32,
    pub heartbeat_idle_millis: u32,
    pub heartbeat_timeout_millis: u32,
}

impl Default for ApplicationConnectionLimits {
    fn default() -> Self {
        Self {
            max_attachments: MAX_ATTACHMENTS_PER_CONNECTION as u16,
            max_pending_calls: MAX_PENDING_CALLS as u16,
            max_client_message_bytes: MAX_INCOMING_MESSAGE_BYTES as u32,
            max_application_payload_bytes: MAX_APPLICATION_PAYLOAD_BYTES as u32,
            heartbeat_idle_millis: HEARTBEAT_IDLE_MILLIS as u32,
            heartbeat_timeout_millis: HEARTBEAT_TIMEOUT_MILLIS as u32,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ApplicationClientFrame {
    Connect {
        api_version: u16,
        encoding: ApiEncoding,
        client_instance_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        token: Option<String>,
    },
    Call {
        call_id: String,
        operation: ApplicationCall,
    },
    Attach {
        call_id: String,
        stream_id: String,
        view_set_id: String,
        after: String,
    },
    Ack {
        stream_id: String,
        cursor: String,
    },
    Detach {
        call_id: String,
        stream_id: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ApplicationServerFrame {
    Connected {
        api_version: u16,
        encoding: ApiEncoding,
        hello: ApiHello,
        connection_limits: ApplicationConnectionLimits,
    },
    Result {
        call_id: String,
        result: ApplicationCallResult,
    },
    CallError {
        call_id: String,
        error: ApplicationConnectionError,
    },
    Attached {
        call_id: String,
        stream_id: String,
        view_set_id: String,
    },
    ViewBatch {
        stream_id: String,
        batch: Box<UpdateBatch>,
    },
    StreamError {
        stream_id: String,
        error: ApplicationConnectionError,
    },
    Detached {
        call_id: String,
        stream_id: String,
    },
    ConnectionError {
        error: ApplicationConnectionError,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationConnectionErrorCode {
    AuthenticationFailed,
    InvalidVersion,
    InvalidMessage,
    InvalidCall,
    ResourceLimit,
    UnknownViewSet,
    ConsumerBusy,
    ViewSetClosed,
    UnknownStream,
    InvalidCursor,
    ResponseTooLarge,
    Internal,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
pub struct ApplicationConnectionError {
    pub code: ApplicationConnectionErrorCode,
    pub message: String,
}

impl ApplicationConnectionError {
    fn new(code: ApplicationConnectionErrorCode, message: impl Into<String>) -> Self {
        let mut message = message.into();
        message.truncate(message.floor_char_boundary(1_024));
        Self { code, message }
    }
}

#[derive(Clone, Default)]
pub(crate) struct ApplicationConnectionRegistry {
    state: Arc<Mutex<RegistryState>>,
    next_generation: Arc<AtomicU64>,
}

#[derive(Default)]
struct RegistryState {
    connections: BTreeMap<String, RegistryConnection>,
    attachments: BTreeMap<String, RegistryAttachment>,
}

struct RegistryConnection {
    generation: u64,
    cancellation: CancellationToken,
    done: CancellationToken,
}

struct RegistryAttachment {
    owner: String,
    generation: u64,
    cancellation: CancellationToken,
    done: CancellationToken,
}

impl ApplicationConnectionRegistry {
    pub(crate) fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(RegistryState::default())),
            next_generation: Arc::new(AtomicU64::new(1)),
        }
    }

    fn generation(&self) -> u64 {
        self.next_generation.fetch_add(1, Ordering::Relaxed)
    }

    async fn activate_connection(
        &self,
        owner: String,
        generation: u64,
        cancellation: CancellationToken,
        done: CancellationToken,
    ) {
        let previous = self.state.lock().await.connections.insert(
            owner,
            RegistryConnection {
                generation,
                cancellation,
                done,
            },
        );
        if let Some(previous) = previous {
            previous.cancellation.cancel();
            previous.done.cancelled().await;
        }
    }

    async fn release_connection(&self, owner: &str, generation: u64) {
        let mut state = self.state.lock().await;
        if state
            .connections
            .get(owner)
            .is_some_and(|active| active.generation == generation)
        {
            state.connections.remove(owner);
        }
    }

    async fn claim_attachment(
        &self,
        owner: String,
        view_set_id: String,
        generation: u64,
        cancellation: CancellationToken,
        done: CancellationToken,
    ) -> Result<(), ApplicationConnectionError> {
        let previous = {
            let mut state = self.state.lock().await;
            if state
                .attachments
                .get(&view_set_id)
                .is_some_and(|active| active.owner != owner)
            {
                return Err(ApplicationConnectionError::new(
                    ApplicationConnectionErrorCode::ConsumerBusy,
                    "view set already has an active consumer",
                ));
            }
            state.attachments.insert(
                view_set_id,
                RegistryAttachment {
                    owner,
                    generation,
                    cancellation,
                    done,
                },
            )
        };
        if let Some(previous) = previous {
            previous.cancellation.cancel();
            previous.done.cancelled().await;
        }
        Ok(())
    }

    async fn release_attachment(&self, view_set_id: &str, generation: u64) {
        let mut state = self.state.lock().await;
        if state
            .attachments
            .get(view_set_id)
            .is_some_and(|active| active.generation == generation)
        {
            state.attachments.remove(view_set_id);
        }
    }
}

pub(crate) fn application_hello(service: &ApplicationService) -> ApiHello {
    let mut hello = service.api_hello();
    if !hello.deliveries.contains(&DeliveryMode::Stream) {
        hello.deliveries.push(DeliveryMode::Stream);
    }
    hello
}

pub(crate) async fn upgrade_application_connection(
    State(state): State<GatewayState>,
    ConnectInfo(_peer): ConnectInfo<std::net::SocketAddr>,
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
        .on_upgrade(move |socket| serve_application_connection(socket, state, permit))
        .into_response()
}

struct OutboundFrame {
    frame: ApplicationServerFrame,
    _reservation: Option<OwnedSemaphorePermit>,
}

enum WriterCommand {
    Frame(OutboundFrame),
    Transport(Message),
}

struct ConnectionAttachment {
    generation: u64,
    acknowledgements: mpsc::Sender<String>,
    cancellation: CancellationToken,
    task: JoinHandle<()>,
}

struct PumpStopped {
    stream_id: String,
    generation: u64,
}

async fn serve_application_connection(
    socket: WebSocket,
    state: GatewayState,
    _permit: OwnedSemaphorePermit,
) {
    let (websocket_writer, mut websocket_reader) = socket.split();
    let connection_cancel = CancellationToken::new();
    let connection_done = CancellationToken::new();
    let (control, control_reader) = mpsc::channel(MAX_CONTROL_MESSAGES);
    let (data, data_reader) = mpsc::channel(MAX_ATTACHMENTS_PER_CONNECTION);
    let writer = tokio::spawn(run_writer(
        websocket_writer,
        control_reader,
        data_reader,
        connection_cancel.clone(),
    ));

    let connected = match tokio::time::timeout(
        Duration::from_millis(HANDSHAKE_TIMEOUT_MILLIS),
        websocket_reader.next(),
    )
    .await
    {
        Ok(Some(Ok(Message::Text(text)))) if text.len() <= MAX_INCOMING_MESSAGE_BYTES => {
            serde_json::from_str::<ApplicationClientFrame>(&text).ok()
        }
        _ => None,
    };
    let Some(ApplicationClientFrame::Connect {
        api_version,
        encoding,
        client_instance_id,
        token,
    }) = connected
    else {
        fatal_connection(
            &control,
            ApplicationConnectionErrorCode::InvalidMessage,
            "connect must be the first application message",
            1002,
        )
        .await;
        finish_writer(control, data, writer).await;
        return;
    };
    if api_version != API_VERSION || encoding != ApiEncoding::Json {
        fatal_connection(
            &control,
            ApplicationConnectionErrorCode::InvalidVersion,
            "application connection version or encoding is unsupported",
            1008,
        )
        .await;
        finish_writer(control, data, writer).await;
        return;
    }
    if !valid_client_instance_id(&client_instance_id)
        || !connection_token_matches(&state.authentication, token.as_deref())
    {
        fatal_connection(
            &control,
            ApplicationConnectionErrorCode::AuthenticationFailed,
            "application connection authentication failed",
            1008,
        )
        .await;
        finish_writer(control, data, writer).await;
        return;
    }

    let owner_key = format!(
        "gateway-client-{}-{client_instance_id}",
        state.http_owner_namespace
    );
    let owner = ViewSetOwner::trusted(owner_key.clone());
    let connection_generation = state.connection_registry.generation();
    state
        .connection_registry
        .activate_connection(
            owner_key.clone(),
            connection_generation,
            connection_cancel.clone(),
            connection_done.clone(),
        )
        .await;
    let hello = {
        let service = state.service.lock().await;
        application_hello(&service)
    };
    if send_control(
        &control,
        ApplicationServerFrame::Connected {
            api_version: API_VERSION,
            encoding: ApiEncoding::Json,
            hello,
            connection_limits: ApplicationConnectionLimits::default(),
        },
        None,
    )
    .await
    .is_err()
    {
        connection_cancel.cancel();
    }

    let reservations = Arc::new(Semaphore::new(DATA_RESERVATIONS));
    let pending_ids = Arc::new(StdMutex::new(BTreeSet::<String>::new()));
    let mut calls = JoinSet::new();
    let mut attachments = BTreeMap::<String, ConnectionAttachment>::new();
    let (pump_stopped, mut stopped_pumps) =
        mpsc::channel::<PumpStopped>(MAX_ATTACHMENTS_PER_CONNECTION);
    let mut invalid_messages = 0_u8;
    let mut heartbeat = tokio::time::interval(Duration::from_secs(1));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_client_activity = Instant::now();
    let mut outstanding_ping: Option<(Vec<u8>, Instant)> = None;
    let mut service_shutdown = false;

    loop {
        tokio::select! {
            biased;
            () = state.gateway_shutdown.cancelled() => {
                service_shutdown = true;
                break;
            }
            () = connection_cancel.cancelled() => break,
            _ = heartbeat.tick() => {
                if let Some((_, sent)) = &outstanding_ping {
                    if sent.elapsed() >= Duration::from_millis(HEARTBEAT_TIMEOUT_MILLIS) {
                        break;
                    }
                } else if last_client_activity.elapsed()
                    >= Duration::from_millis(HEARTBEAT_IDLE_MILLIS)
                {
                    let nonce = connection_generation.to_be_bytes().to_vec();
                    if control.send(WriterCommand::Transport(Message::Ping(nonce.clone().into())))
                        .await.is_err()
                    {
                        break;
                    }
                    outstanding_ping = Some((nonce, Instant::now()));
                }
            }
            stopped = stopped_pumps.recv() => {
                if let Some(stopped) = stopped
                    && attachments
                        .get(&stopped.stream_id)
                        .is_some_and(|active| active.generation == stopped.generation)
                    && let Some(active) = attachments.remove(&stopped.stream_id)
                {
                    let _ = active.task.await;
                }
            }
            joined = calls.join_next(), if !calls.is_empty() => {
                if joined.is_some_and(|result| result.is_err()) {
                    break;
                }
            }
            incoming = websocket_reader.next() => {
                let Some(incoming) = incoming else { break; };
                let message = match incoming {
                    Ok(message) => message,
                    Err(_) => break,
                };
                last_client_activity = Instant::now();
                match message {
                    Message::Pong(payload) => {
                        if outstanding_ping
                            .as_ref()
                            .is_some_and(|(expected, _)| expected.as_slice() == payload.as_ref())
                        {
                            outstanding_ping = None;
                        }
                    }
                    Message::Ping(payload) => {
                        if control.send(WriterCommand::Transport(Message::Pong(payload)))
                            .await.is_err()
                        {
                            break;
                        }
                    }
                    Message::Close(frame) => {
                        let _ = control.send(WriterCommand::Transport(Message::Close(frame))).await;
                        break;
                    }
                    Message::Text(text) if text.len() <= MAX_INCOMING_MESSAGE_BYTES => {
                        let parsed = serde_json::from_str::<ApplicationClientFrame>(&text);
                        let Ok(frame) = parsed else {
                            invalid_messages = invalid_messages.saturating_add(1);
                            if invalid_messages >= MAX_INVALID_MESSAGES {
                                fatal_connection(
                                    &control,
                                    ApplicationConnectionErrorCode::InvalidMessage,
                                    "too many invalid application messages",
                                    1002,
                                ).await;
                                break;
                            }
                            let _ = send_control(
                                &control,
                                ApplicationServerFrame::ConnectionError {
                                    error: ApplicationConnectionError::new(
                                        ApplicationConnectionErrorCode::InvalidMessage,
                                        "application message is invalid",
                                    ),
                                },
                                None,
                            ).await;
                            continue;
                        };
                        if matches!(frame, ApplicationClientFrame::Connect { .. }) {
                            invalid_messages = invalid_messages.saturating_add(1);
                            if invalid_messages >= MAX_INVALID_MESSAGES {
                                fatal_connection(
                                    &control,
                                    ApplicationConnectionErrorCode::InvalidMessage,
                                    "connect cannot be repeated",
                                    1002,
                                ).await;
                                break;
                            }
                            continue;
                        }
                        handle_client_frame(
                            frame,
                            &state,
                            &owner,
                            &owner_key,
                            &control,
                            &data,
                            &reservations,
                            &pending_ids,
                            &mut calls,
                            &mut attachments,
                            &pump_stopped,
                            &connection_cancel,
                        ).await;
                    }
                    Message::Text(_) | Message::Binary(_) => {
                        fatal_connection(
                            &control,
                            ApplicationConnectionErrorCode::InvalidMessage,
                            "application message exceeds its bound or is not text",
                            1009,
                        ).await;
                        break;
                    }
                }
            }
        }
    }

    connection_cancel.cancel();
    for (_, active) in attachments {
        stop_attachment(active).await;
    }
    calls.abort_all();
    while calls.join_next().await.is_some() {}
    state
        .connection_registry
        .release_connection(&owner_key, connection_generation)
        .await;
    connection_done.cancel();
    let (code, reason) = if service_shutdown {
        (1001, "service shutdown")
    } else {
        (1000, "application connection closed")
    };
    let _ = control
        .send(WriterCommand::Transport(Message::Close(Some(CloseFrame {
            code,
            reason: reason.into(),
        }))))
        .await;
    finish_writer(control, data, writer).await;
}

#[allow(clippy::too_many_arguments)]
async fn handle_client_frame(
    frame: ApplicationClientFrame,
    state: &GatewayState,
    owner: &ViewSetOwner,
    owner_key: &str,
    control: &mpsc::Sender<WriterCommand>,
    data: &mpsc::Sender<OutboundFrame>,
    reservations: &Arc<Semaphore>,
    pending_ids: &Arc<StdMutex<BTreeSet<String>>>,
    calls: &mut JoinSet<()>,
    attachments: &mut BTreeMap<String, ConnectionAttachment>,
    pump_stopped: &mpsc::Sender<PumpStopped>,
    connection_cancel: &CancellationToken,
) {
    match frame {
        ApplicationClientFrame::Call { call_id, operation } => {
            if !valid_identifier(&call_id) {
                send_call_error(
                    control,
                    call_id,
                    ApplicationConnectionErrorCode::InvalidCall,
                    "call ID is invalid",
                )
                .await;
                return;
            }
            if !valid_application_call(&operation) {
                send_call_error(
                    control,
                    call_id,
                    ApplicationConnectionErrorCode::InvalidCall,
                    "application call contains an invalid view-set ID",
                )
                .await;
                return;
            }
            if !insert_pending(pending_ids, &call_id) {
                send_call_error(
                    control,
                    call_id,
                    ApplicationConnectionErrorCode::ResourceLimit,
                    "call ID is pending or the pending call limit was reached",
                )
                .await;
                return;
            }
            let service = state.service.clone();
            let owner = owner.clone();
            let control = control.clone();
            let reservations = reservations.clone();
            let pending_ids = pending_ids.clone();
            calls.spawn(async move {
                let reservation = reservations.acquire_owned().await.ok();
                let result = service
                    .lock()
                    .await
                    .application_call(&owner, operation)
                    .await;
                let frame = match result {
                    Ok(result) if semantic_size(&result).is_some() => {
                        ApplicationServerFrame::Result {
                            call_id: call_id.clone(),
                            result,
                        }
                    }
                    Ok(_) => ApplicationServerFrame::CallError {
                        call_id: call_id.clone(),
                        error: ApplicationConnectionError::new(
                            ApplicationConnectionErrorCode::ResponseTooLarge,
                            "application call result exceeds its configured bound",
                        ),
                    },
                    Err(error) => ApplicationServerFrame::CallError {
                        call_id: call_id.clone(),
                        error: application_call_error(error),
                    },
                };
                let _ = send_control(&control, frame, reservation).await;
                remove_pending(&pending_ids, &call_id);
            });
        }
        ApplicationClientFrame::Attach {
            call_id,
            stream_id,
            view_set_id,
            after,
        } => {
            if !valid_identifier(&call_id)
                || !valid_identifier(&stream_id)
                || !valid_view_set_id(&view_set_id)
                || !valid_cursor(&after)
            {
                send_call_error(
                    control,
                    call_id,
                    ApplicationConnectionErrorCode::InvalidCall,
                    "view attachment identifiers or cursor are invalid",
                )
                .await;
                return;
            }
            if attachments.contains_key(&stream_id) || !insert_pending(pending_ids, &call_id) {
                send_call_error(
                    control,
                    call_id,
                    ApplicationConnectionErrorCode::ResourceLimit,
                    "stream or call ID is pending",
                )
                .await;
                return;
            }
            if attachments.len() >= MAX_ATTACHMENTS_PER_CONNECTION {
                remove_pending(pending_ids, &call_id);
                send_call_error(
                    control,
                    call_id,
                    ApplicationConnectionErrorCode::ResourceLimit,
                    "connection attachment limit reached",
                )
                .await;
                return;
            }
            let view_set = state.service.lock().await.view_set(owner, &view_set_id);
            let view_set = match view_set {
                Ok(view_set) => view_set,
                Err(error) => {
                    remove_pending(pending_ids, &call_id);
                    let error = view_set_connection_error(error);
                    let _ = send_control(
                        control,
                        ApplicationServerFrame::CallError { call_id, error },
                        None,
                    )
                    .await;
                    return;
                }
            };
            let generation = state.connection_registry.generation();
            let cancellation = connection_cancel.child_token();
            let done = CancellationToken::new();
            if let Err(error) = state
                .connection_registry
                .claim_attachment(
                    owner_key.to_owned(),
                    view_set_id.clone(),
                    generation,
                    cancellation.clone(),
                    done.clone(),
                )
                .await
            {
                remove_pending(pending_ids, &call_id);
                let _ = send_control(
                    control,
                    ApplicationServerFrame::CallError { call_id, error },
                    None,
                )
                .await;
                return;
            }
            if send_control(
                control,
                ApplicationServerFrame::Attached {
                    call_id: call_id.clone(),
                    stream_id: stream_id.clone(),
                    view_set_id: view_set_id.clone(),
                },
                None,
            )
            .await
            .is_err()
            {
                cancellation.cancel();
            }
            remove_pending(pending_ids, &call_id);
            let (acknowledgements, acknowledgement_receiver) = mpsc::channel(1);
            let task = tokio::spawn(run_attachment(
                AcknowledgedViewStream::new(view_set, after),
                stream_id.clone(),
                generation,
                cancellation.clone(),
                done,
                acknowledgement_receiver,
                control.clone(),
                data.clone(),
                reservations.clone(),
                state.connection_registry.clone(),
                pump_stopped.clone(),
            ));
            if let Some(replaced) = attachments.insert(
                stream_id,
                ConnectionAttachment {
                    generation,
                    acknowledgements,
                    cancellation,
                    task,
                },
            ) {
                stop_attachment(replaced).await;
            }
        }
        ApplicationClientFrame::Ack { stream_id, cursor } => {
            if !valid_identifier(&stream_id) || !valid_cursor(&cursor) {
                send_stream_error(
                    control,
                    stream_id,
                    ApplicationConnectionErrorCode::InvalidCursor,
                    "stream ID or cursor is invalid",
                )
                .await;
                return;
            }
            let Some(active) = attachments.get(&stream_id) else {
                send_stream_error(
                    control,
                    stream_id,
                    ApplicationConnectionErrorCode::UnknownStream,
                    "view stream is unavailable",
                )
                .await;
                return;
            };
            if active.acknowledgements.try_send(cursor).is_err() {
                active.cancellation.cancel();
                send_stream_error(
                    control,
                    stream_id,
                    ApplicationConnectionErrorCode::InvalidCursor,
                    "view stream already has a pending acknowledgement",
                )
                .await;
            }
        }
        ApplicationClientFrame::Detach { call_id, stream_id } => {
            if !valid_identifier(&call_id)
                || !valid_identifier(&stream_id)
                || !insert_pending(pending_ids, &call_id)
            {
                send_call_error(
                    control,
                    call_id,
                    ApplicationConnectionErrorCode::InvalidCall,
                    "detach call or stream ID is invalid or pending",
                )
                .await;
                return;
            }
            let Some(active) = attachments.remove(&stream_id) else {
                remove_pending(pending_ids, &call_id);
                send_call_error(
                    control,
                    call_id,
                    ApplicationConnectionErrorCode::UnknownStream,
                    "view stream is unavailable",
                )
                .await;
                return;
            };
            stop_attachment(active).await;
            remove_pending(pending_ids, &call_id);
            let _ = send_control(
                control,
                ApplicationServerFrame::Detached { call_id, stream_id },
                None,
            )
            .await;
        }
        ApplicationClientFrame::Connect { .. } => unreachable!("connect handled before routing"),
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_attachment(
    mut stream: AcknowledgedViewStream,
    stream_id: String,
    generation: u64,
    cancellation: CancellationToken,
    done: CancellationToken,
    mut acknowledgements: mpsc::Receiver<String>,
    control: mpsc::Sender<WriterCommand>,
    data: mpsc::Sender<OutboundFrame>,
    reservations: Arc<Semaphore>,
    registry: ApplicationConnectionRegistry,
    stopped: mpsc::Sender<PumpStopped>,
) {
    let view_set_id = stream.view_set_id().to_owned();
    loop {
        let reservation = tokio::select! {
            biased;
            () = cancellation.cancelled() => break,
            reservation = reservations.clone().acquire_owned() => {
                let Ok(reservation) = reservation else { break; };
                reservation
            }
        };
        let batch = tokio::select! {
            biased;
            () = cancellation.cancelled() => break,
            batch = stream.next_batch(VIEW_STREAM_WAIT_MILLIS) => {
                match batch {
                    Ok(batch) => batch,
                    Err(error) => {
                        let _ = send_control(
                            &control,
                            ApplicationServerFrame::StreamError {
                                stream_id: stream_id.clone(),
                                error: acknowledged_stream_error(error),
                            },
                            None,
                        ).await;
                        break;
                    }
                }
            }
        };
        if semantic_size(&batch).is_none() {
            let _ = send_stream_error(
                &control,
                stream_id.clone(),
                ApplicationConnectionErrorCode::ResponseTooLarge,
                "view batch exceeds its configured bound",
            )
            .await;
            break;
        }
        let outgoing = OutboundFrame {
            frame: ApplicationServerFrame::ViewBatch {
                stream_id: stream_id.clone(),
                batch: Box::new(batch),
            },
            _reservation: Some(reservation),
        };
        let sent = tokio::select! {
            biased;
            () = cancellation.cancelled() => false,
            result = data.send(outgoing) => result.is_ok(),
        };
        if !sent {
            break;
        }
        let acknowledgement = tokio::select! {
            biased;
            () = cancellation.cancelled() => break,
            acknowledgement = acknowledgements.recv() => acknowledgement,
        };
        let Some(acknowledgement) = acknowledgement else {
            break;
        };
        if let Err(error) = stream.acknowledge(&acknowledgement) {
            let _ = send_control(
                &control,
                ApplicationServerFrame::StreamError {
                    stream_id: stream_id.clone(),
                    error: acknowledged_stream_error(error),
                },
                None,
            )
            .await;
            break;
        }
    }
    registry.release_attachment(&view_set_id, generation).await;
    done.cancel();
    let _ = stopped.try_send(PumpStopped {
        stream_id,
        generation,
    });
}

async fn stop_attachment(active: ConnectionAttachment) {
    active.cancellation.cancel();
    drop(active.acknowledgements);
    let _ = active.task.await;
}

async fn run_writer(
    mut websocket: futures_util::stream::SplitSink<WebSocket, Message>,
    mut control: mpsc::Receiver<WriterCommand>,
    mut data: mpsc::Receiver<OutboundFrame>,
    cancellation: CancellationToken,
) {
    loop {
        let command = match control.try_recv() {
            Ok(command) => Some(command),
            Err(mpsc::error::TryRecvError::Empty) => tokio::select! {
                biased;
                command = control.recv() => command,
                frame = data.recv() => frame.map(WriterCommand::Frame),
            },
            Err(mpsc::error::TryRecvError::Disconnected) => {
                data.recv().await.map(WriterCommand::Frame)
            }
        };
        let Some(command) = command else {
            break;
        };
        let message = match command {
            WriterCommand::Frame(outgoing) => {
                let encoded = match serde_json::to_string(&outgoing.frame) {
                    Ok(encoded) if encoded.len() <= MAX_SERVER_MESSAGE_BYTES => encoded,
                    _ => break,
                };
                Message::Text(encoded.into())
            }
            WriterCommand::Transport(message) => message,
        };
        let closing = matches!(message, Message::Close(_));
        if websocket.send(message).await.is_err() || closing {
            break;
        }
    }
    cancellation.cancel();
    let _ = websocket.close().await;
}

async fn finish_writer(
    control: mpsc::Sender<WriterCommand>,
    data: mpsc::Sender<OutboundFrame>,
    writer: JoinHandle<()>,
) {
    drop(control);
    drop(data);
    let _ = writer.await;
}

async fn send_control(
    outgoing: &mpsc::Sender<WriterCommand>,
    frame: ApplicationServerFrame,
    reservation: Option<OwnedSemaphorePermit>,
) -> Result<(), ()> {
    outgoing
        .send(WriterCommand::Frame(OutboundFrame {
            frame,
            _reservation: reservation,
        }))
        .await
        .map_err(|_| ())
}

async fn fatal_connection(
    outgoing: &mpsc::Sender<WriterCommand>,
    code: ApplicationConnectionErrorCode,
    message: &str,
    close_code: u16,
) {
    let _ = send_control(
        outgoing,
        ApplicationServerFrame::ConnectionError {
            error: ApplicationConnectionError::new(code, message),
        },
        None,
    )
    .await;
    let _ = outgoing
        .send(WriterCommand::Transport(Message::Close(Some(CloseFrame {
            code: close_code,
            reason: "application connection rejected".into(),
        }))))
        .await;
}

async fn send_call_error(
    outgoing: &mpsc::Sender<WriterCommand>,
    call_id: String,
    code: ApplicationConnectionErrorCode,
    message: &str,
) {
    let _ = send_control(
        outgoing,
        ApplicationServerFrame::CallError {
            call_id,
            error: ApplicationConnectionError::new(code, message),
        },
        None,
    )
    .await;
}

async fn send_stream_error(
    outgoing: &mpsc::Sender<WriterCommand>,
    stream_id: String,
    code: ApplicationConnectionErrorCode,
    message: &str,
) {
    let _ = send_control(
        outgoing,
        ApplicationServerFrame::StreamError {
            stream_id,
            error: ApplicationConnectionError::new(code, message),
        },
        None,
    )
    .await;
}

fn insert_pending(pending: &StdMutex<BTreeSet<String>>, call_id: &str) -> bool {
    let mut pending = pending.lock().expect("pending call lock poisoned");
    if pending.len() >= MAX_PENDING_CALLS || pending.contains(call_id) {
        return false;
    }
    pending.insert(call_id.to_owned());
    true
}

fn remove_pending(pending: &StdMutex<BTreeSet<String>>, call_id: &str) {
    pending
        .lock()
        .expect("pending call lock poisoned")
        .remove(call_id);
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_IDENTIFIER_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn valid_client_instance_id(value: &str) -> bool {
    value.len() == CLIENT_INSTANCE_ID_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn valid_cursor(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 20
        && (value.len() == 1 || !value.starts_with('0'))
        && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn valid_application_call(call: &ApplicationCall) -> bool {
    match call {
        ApplicationCall::UpdateViewSet { view_set_id, .. }
        | ApplicationCall::CloseViewSet { view_set_id } => valid_view_set_id(view_set_id),
        ApplicationCall::Dispatch { .. } | ApplicationCall::OpenViewSet { .. } => true,
    }
}

fn connection_token_matches(
    authentication: &GatewayAuthentication,
    candidate: Option<&str>,
) -> bool {
    match authentication {
        GatewayAuthentication::Bearer { token } => candidate.is_some_and(|candidate| {
            !candidate.is_empty()
                && candidate.len() <= MAX_TOKEN_BYTES
                && constant_time_equal(candidate.as_bytes(), token.as_bytes())
        }),
        GatewayAuthentication::UnauthenticatedLoopbackDevelopment => candidate.is_none(),
    }
}

fn semantic_size<T: Serialize>(value: &T) -> Option<usize> {
    serde_json::to_vec(value)
        .ok()
        .map(|encoded| encoded.len())
        .filter(|length| *length <= MAX_APPLICATION_PAYLOAD_BYTES)
}

fn application_call_error(error: ApplicationCallError) -> ApplicationConnectionError {
    match error {
        ApplicationCallError::ViewSet(error) => view_set_connection_error(error),
        ApplicationCallError::Application(_) => ApplicationConnectionError::new(
            ApplicationConnectionErrorCode::Internal,
            "application call failed",
        ),
    }
}

fn view_set_connection_error(error: ViewSetError) -> ApplicationConnectionError {
    let code = match error {
        ViewSetError::InvalidViewCount { .. }
        | ViewSetError::InvalidViewId
        | ViewSetError::DuplicateViewId(_)
        | ViewSetError::InvalidDeliveryInterval { .. }
        | ViewSetError::InvalidQueueBound { .. }
        | ViewSetError::InvalidView(_)
        | ViewSetError::SnapshotExceedsQueue { .. } => ApplicationConnectionErrorCode::InvalidCall,
        ViewSetError::ResourceLimit => ApplicationConnectionErrorCode::ResourceLimit,
        ViewSetError::UnknownViewSet => ApplicationConnectionErrorCode::UnknownViewSet,
        ViewSetError::ConsumerBusy => ApplicationConnectionErrorCode::ConsumerBusy,
        ViewSetError::Closed => ApplicationConnectionErrorCode::ViewSetClosed,
        ViewSetError::Internal(_) => ApplicationConnectionErrorCode::Internal,
    };
    ApplicationConnectionError::new(code, error.to_string())
}

fn acknowledged_stream_error(error: AcknowledgedViewStreamError) -> ApplicationConnectionError {
    match error {
        AcknowledgedViewStreamError::ViewSet(error) => view_set_connection_error(error),
        AcknowledgedViewStreamError::AcknowledgementOutstanding => ApplicationConnectionError::new(
            ApplicationConnectionErrorCode::ConsumerBusy,
            error.to_string(),
        ),
        AcknowledgedViewStreamError::InvalidAcknowledgement => ApplicationConnectionError::new(
            ApplicationConnectionErrorCode::InvalidCursor,
            error.to_string(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ApplicationClientFrame, ApplicationConnectionErrorCode, ApplicationConnectionRegistry,
        CLIENT_INSTANCE_ID_BYTES, valid_client_instance_id, valid_cursor, valid_identifier,
    };
    use tokio_util::sync::CancellationToken;

    #[test]
    fn validates_bounded_wire_identifiers() {
        assert!(valid_identifier("call-1"));
        assert!(!valid_identifier(""));
        assert!(!valid_identifier("contains space"));
        assert!(valid_client_instance_id(
            &"a".repeat(CLIENT_INSTANCE_ID_BYTES)
        ));
        assert!(!valid_client_instance_id(
            &"A".repeat(CLIENT_INSTANCE_ID_BYTES)
        ));
        assert!(valid_cursor("0"));
        assert!(valid_cursor("42"));
        assert!(!valid_cursor("01"));
    }

    #[test]
    fn connect_token_is_optional_on_the_wire() {
        let frame: ApplicationClientFrame = serde_json::from_value(serde_json::json!({
            "type": "connect",
            "api_version": 1,
            "encoding": "json",
            "client_instance_id": "00000000000000000000000000000001"
        }))
        .expect("development connect frame");
        assert!(matches!(
            frame,
            ApplicationClientFrame::Connect { token: None, .. }
        ));
        let code = ApplicationConnectionErrorCode::ConsumerBusy;
        assert_eq!(
            serde_json::to_string(&code).expect("error code"),
            "\"consumer_busy\""
        );
    }

    #[tokio::test]
    async fn connection_takeover_waits_and_old_cleanup_cannot_remove_replacement() {
        let registry = ApplicationConnectionRegistry::new();
        let first_cancel = CancellationToken::new();
        let first_done = CancellationToken::new();
        registry
            .activate_connection(
                "owner".to_owned(),
                1,
                first_cancel.clone(),
                first_done.clone(),
            )
            .await;

        let second_cancel = CancellationToken::new();
        let second_done = CancellationToken::new();
        let takeover_registry = registry.clone();
        let takeover_cancel = second_cancel.clone();
        let takeover_done = second_done.clone();
        let takeover = tokio::spawn(async move {
            takeover_registry
                .activate_connection("owner".to_owned(), 2, takeover_cancel, takeover_done)
                .await;
        });
        first_cancel.cancelled().await;
        assert!(!takeover.is_finished());
        first_done.cancel();
        takeover.await.expect("takeover task");
        registry.release_connection("owner", 1).await;

        let third_cancel = CancellationToken::new();
        let third_done = CancellationToken::new();
        let third_registry = registry.clone();
        let third = tokio::spawn(async move {
            third_registry
                .activate_connection("owner".to_owned(), 3, third_cancel, third_done)
                .await;
        });
        second_cancel.cancelled().await;
        assert!(!third.is_finished());
        second_done.cancel();
        third.await.expect("third connection");
    }

    #[tokio::test]
    async fn attachment_takeover_is_generation_safe_and_owner_bounded() {
        let registry = ApplicationConnectionRegistry::new();
        let first_cancel = CancellationToken::new();
        let first_done = CancellationToken::new();
        registry
            .claim_attachment(
                "owner".to_owned(),
                "view-set".to_owned(),
                1,
                first_cancel.clone(),
                first_done.clone(),
            )
            .await
            .expect("first attachment");

        let second_cancel = CancellationToken::new();
        let second_done = CancellationToken::new();
        let takeover_registry = registry.clone();
        let takeover_cancel = second_cancel.clone();
        let takeover_done = second_done.clone();
        let takeover = tokio::spawn(async move {
            takeover_registry
                .claim_attachment(
                    "owner".to_owned(),
                    "view-set".to_owned(),
                    2,
                    takeover_cancel,
                    takeover_done,
                )
                .await
        });
        first_cancel.cancelled().await;
        assert!(!takeover.is_finished());
        first_done.cancel();
        takeover
            .await
            .expect("takeover task")
            .expect("takeover attachment");
        registry.release_attachment("view-set", 1).await;

        let error = registry
            .claim_attachment(
                "other-owner".to_owned(),
                "view-set".to_owned(),
                3,
                CancellationToken::new(),
                CancellationToken::new(),
            )
            .await
            .expect_err("unrelated owner must not take over");
        assert_eq!(error.code, ApplicationConnectionErrorCode::ConsumerBusy);
        second_cancel.cancel();
        second_done.cancel();
    }
}
