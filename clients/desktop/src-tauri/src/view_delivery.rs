use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use rstorrent_session::{
    AcknowledgedViewStream, AcknowledgedViewStreamError, ApiHello, ApplicationService,
    DeliveryMode, OpenViewSetRequest, OpenViewSetResponse, UpdateBatch, UpdateViewSetRequest,
    ViewSet, ViewSetError, ViewSetOwner,
};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{State, WebviewWindow};
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

use crate::DesktopState;

const VIEW_STREAM_WAIT_MILLIS: u32 = 20_000;

static NEXT_VIEW_STREAM_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) struct DesktopViewResources {
    view_sets: Mutex<BTreeMap<(String, String), DesktopViewSet>>,
    view_streams: Mutex<BTreeMap<(String, String), DesktopViewStream>>,
}

#[derive(Clone)]
struct DesktopViewSet {
    window_generation: u64,
    owner: ViewSetOwner,
}

struct DesktopViewStream {
    window_generation: u64,
    view_set_id: String,
    acknowledgements: mpsc::Sender<String>,
    cancellation: CancellationToken,
    task: tauri::async_runtime::JoinHandle<()>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct DesktopApiError {
    code: &'static str,
    message: String,
}

impl DesktopApiError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum DesktopViewStreamEvent {
    Batch { batch: Box<UpdateBatch> },
    Error { error: DesktopApiError },
}

impl DesktopViewResources {
    pub(crate) fn new() -> Self {
        Self {
            view_sets: Mutex::new(BTreeMap::new()),
            view_streams: Mutex::new(BTreeMap::new()),
        }
    }

    pub(crate) async fn close_window(
        &self,
        service: Arc<Mutex<ApplicationService>>,
        label: String,
        window_generation: u64,
    ) {
        let streams = {
            let mut view_streams = self.view_streams.lock().await;
            let keys = view_streams
                .iter()
                .filter(|(key, stream)| {
                    key.0 == label && stream.window_generation == window_generation
                })
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            keys.into_iter()
                .filter_map(|key| view_streams.remove(&key))
                .collect::<Vec<_>>()
        };
        for stream in streams {
            stop_view_stream(stream).await;
        }
        let sets = {
            let mut view_sets = self.view_sets.lock().await;
            let keys = view_sets
                .iter()
                .filter(|(key, view_set)| {
                    key.0 == label && view_set.window_generation == window_generation
                })
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            keys.into_iter()
                .filter_map(|key| {
                    view_sets
                        .remove(&key)
                        .map(|view_set| (key.1, view_set.owner))
                })
                .collect::<Vec<_>>()
        };
        let service = service.lock().await;
        for (view_set_id, owner) in sets {
            let _ = service.close_view_set(&owner, &view_set_id);
        }
    }

    pub(crate) async fn close_all(&self) {
        let streams = {
            let mut view_streams = self.view_streams.lock().await;
            std::mem::take(&mut *view_streams)
        };
        for (_, stream) in streams {
            stop_view_stream(stream).await;
        }
        self.view_sets.lock().await.clear();
    }
}

#[tauri::command]
pub(crate) async fn application_view_hello(
    state: State<'_, DesktopState>,
) -> Result<ApiHello, String> {
    let mut hello = state.service.lock().await.api_hello();
    hello.deliveries = vec![DeliveryMode::Stream];
    Ok(hello)
}

#[tauri::command]
pub(crate) async fn application_view_open(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    request: OpenViewSetRequest,
) -> Result<OpenViewSetResponse, DesktopApiError> {
    let window_generation = state.window_generation.load(Ordering::Acquire);
    let owner = desktop_view_owner(&window, window_generation);
    let response = state
        .service
        .lock()
        .await
        .open_view_set(owner.clone(), request)
        .map_err(desktop_view_error)?;
    state.view_resources.view_sets.lock().await.insert(
        (window.label().to_owned(), response.view_set_id.clone()),
        DesktopViewSet {
            window_generation,
            owner,
        },
    );
    Ok(response)
}

#[tauri::command]
pub(crate) async fn application_view_update(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    view_set_id: String,
    request: UpdateViewSetRequest,
) -> Result<(), DesktopApiError> {
    let view_set = desktop_view_set(&window, &state, &view_set_id).await?;
    state
        .service
        .lock()
        .await
        .update_view_set(&view_set.owner, &view_set_id, request)
        .map_err(desktop_view_error)
}

#[tauri::command]
pub(crate) async fn application_view_stream(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    view_set_id: String,
    after: String,
    updates: Channel<DesktopViewStreamEvent>,
) -> Result<String, DesktopApiError> {
    validate_cursor(&after)?;
    let tracked = desktop_view_set(&window, &state, &view_set_id).await?;
    let view_set = state
        .service
        .lock()
        .await
        .view_set(&tracked.owner, &view_set_id)
        .map_err(desktop_view_error)?;
    let stream_id = format!(
        "tauri-stream-{}",
        NEXT_VIEW_STREAM_ID.fetch_add(1, Ordering::Relaxed)
    );
    let cancellation = CancellationToken::new();
    let task_cancellation = cancellation.clone();
    let (acknowledgements, acknowledgement_receiver) = mpsc::channel(1);
    let task = tauri::async_runtime::spawn(async move {
        run_view_stream(
            view_set,
            after,
            task_cancellation,
            acknowledgement_receiver,
            move |event| updates.send(event).map_err(|_| ()),
        )
        .await;
    });
    let key = (window.label().to_owned(), stream_id.clone());
    let replaced = state.view_resources.view_streams.lock().await.insert(
        key,
        DesktopViewStream {
            window_generation: tracked.window_generation,
            view_set_id,
            acknowledgements,
            cancellation,
            task,
        },
    );
    if let Some(replaced) = replaced {
        stop_view_stream(replaced).await;
    }
    Ok(stream_id)
}

#[tauri::command]
pub(crate) async fn application_view_stream_ack(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    stream_id: String,
    cursor: String,
) -> Result<(), DesktopApiError> {
    validate_cursor(&cursor)?;
    let streams = state.view_resources.view_streams.lock().await;
    let stream = streams
        .get(&(window.label().to_owned(), stream_id))
        .filter(|stream| {
            stream.window_generation == state.window_generation.load(Ordering::Acquire)
        })
        .ok_or_else(|| DesktopApiError::new("unknown_view_stream", "view stream is unavailable"))?;
    stream
        .acknowledgements
        .try_send(cursor)
        .map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => DesktopApiError::new(
                "concurrent_pull",
                "view stream already has a pending acknowledgement",
            ),
            mpsc::error::TrySendError::Closed(_) => {
                DesktopApiError::new("view_set_closed", "view stream is closed")
            }
        })
}

#[tauri::command]
pub(crate) async fn application_view_stream_close(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    stream_id: String,
) -> Result<(), DesktopApiError> {
    let key = (window.label().to_owned(), stream_id);
    let window_generation = state.window_generation.load(Ordering::Acquire);
    let stream = {
        let mut streams = state.view_resources.view_streams.lock().await;
        if !streams
            .get(&key)
            .is_some_and(|stream| stream.window_generation == window_generation)
        {
            return Err(DesktopApiError::new(
                "unknown_view_stream",
                "view stream is unavailable",
            ));
        }
        streams
            .remove(&key)
            .expect("checked stream must remain under the same lock")
    };
    stop_view_stream(stream).await;
    Ok(())
}

#[tauri::command]
pub(crate) async fn application_view_close(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    view_set_id: String,
) -> Result<(), DesktopApiError> {
    let tracked = desktop_view_set(&window, &state, &view_set_id).await?;
    let streams = take_view_set_streams(
        &state.view_resources.view_streams,
        window.label(),
        tracked.window_generation,
        &view_set_id,
    )
    .await;
    for stream in streams {
        stop_view_stream(stream).await;
    }
    state
        .service
        .lock()
        .await
        .close_view_set(&tracked.owner, &view_set_id)
        .map_err(desktop_view_error)?;
    state
        .view_resources
        .view_sets
        .lock()
        .await
        .remove(&(window.label().to_owned(), view_set_id));
    Ok(())
}

async fn stop_view_stream(stream: DesktopViewStream) {
    stream.cancellation.cancel();
    drop(stream.acknowledgements);
    let _ = stream.task.await;
}

async fn run_view_stream<F>(
    view_set: ViewSet,
    after: String,
    cancellation: CancellationToken,
    mut acknowledgements: mpsc::Receiver<String>,
    mut send: F,
) where
    F: FnMut(DesktopViewStreamEvent) -> Result<(), ()>,
{
    let mut stream = AcknowledgedViewStream::new(view_set, after);
    loop {
        let batch = tokio::select! {
            biased;
            () = cancellation.cancelled() => break,
            result = stream.next_batch(VIEW_STREAM_WAIT_MILLIS) => {
                match result {
                    Ok(batch) => batch,
                    Err(error) => {
                        let _ = send(DesktopViewStreamEvent::Error {
                            error: desktop_stream_error(error),
                        });
                        break;
                    }
                }
            }
        };
        if send(DesktopViewStreamEvent::Batch {
            batch: Box::new(batch),
        })
        .is_err()
        {
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
            let _ = send(DesktopViewStreamEvent::Error {
                error: desktop_stream_error(error),
            });
            break;
        }
    }
}

fn desktop_view_owner(window: &WebviewWindow, window_generation: u64) -> ViewSetOwner {
    ViewSetOwner::trusted(format!(
        "tauri-window-{}-{window_generation}",
        window.label()
    ))
}

async fn desktop_view_set(
    window: &WebviewWindow,
    state: &DesktopState,
    view_set_id: &str,
) -> Result<DesktopViewSet, DesktopApiError> {
    let window_generation = state.window_generation.load(Ordering::Acquire);
    state
        .view_resources
        .view_sets
        .lock()
        .await
        .get(&(window.label().to_owned(), view_set_id.to_owned()))
        .filter(|view_set| view_set.window_generation == window_generation)
        .cloned()
        .ok_or_else(|| DesktopApiError::new("unknown_view_set", "view set is unavailable"))
}

fn validate_cursor(cursor: &str) -> Result<(), DesktopApiError> {
    if cursor.is_empty()
        || cursor.len() > 20
        || (cursor.len() > 1 && cursor.starts_with('0'))
        || !cursor.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(DesktopApiError::new(
            "invalid_cursor",
            "view-set cursor is invalid",
        ));
    }
    Ok(())
}

fn desktop_view_error(error: ViewSetError) -> DesktopApiError {
    let code = match &error {
        ViewSetError::InvalidViewCount { .. }
        | ViewSetError::InvalidViewId
        | ViewSetError::DuplicateViewId(_)
        | ViewSetError::InvalidDeliveryInterval { .. }
        | ViewSetError::InvalidQueueBound { .. }
        | ViewSetError::InvalidView(_)
        | ViewSetError::SnapshotExceedsQueue { .. } => "invalid_request",
        ViewSetError::ResourceLimit => "resource_limit",
        ViewSetError::UnknownViewSet => "unknown_view_set",
        ViewSetError::ConsumerBusy => "concurrent_pull",
        ViewSetError::Closed => "view_set_closed",
        ViewSetError::Internal(_) => "internal",
    };
    DesktopApiError::new(code, error.to_string())
}

fn desktop_stream_error(error: AcknowledgedViewStreamError) -> DesktopApiError {
    match error {
        AcknowledgedViewStreamError::ViewSet(error) => desktop_view_error(error),
        AcknowledgedViewStreamError::AcknowledgementOutstanding => DesktopApiError::new(
            "concurrent_pull",
            "view stream already has an unacknowledged batch",
        ),
        AcknowledgedViewStreamError::InvalidAcknowledgement => DesktopApiError::new(
            "invalid_cursor",
            "view stream acknowledgement does not match the delivered cursor",
        ),
    }
}

async fn take_view_set_streams(
    streams: &Mutex<BTreeMap<(String, String), DesktopViewStream>>,
    label: &str,
    window_generation: u64,
    view_set_id: &str,
) -> Vec<DesktopViewStream> {
    let mut streams = streams.lock().await;
    let keys = streams
        .iter()
        .filter(|(key, stream)| {
            key.0 == label
                && stream.window_generation == window_generation
                && stream.view_set_id == view_set_id
        })
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    keys.into_iter()
        .filter_map(|key| streams.remove(&key))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use rstorrent_session::{
        ApplicationService, OpenViewSetOptions, OpenViewSetRequest, UpdateViewSetRequest,
        ViewDeliveryPolicy, ViewSetOwner, ViewSetUpdate, ViewSpec,
    };
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    use super::{DesktopViewStreamEvent, run_view_stream};
    use crate::desktop_application_config;

    static NEXT_TEST_ROOT: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn stream_waits_for_exact_acknowledgement_before_next_batch() {
        tauri::async_runtime::block_on(async {
            let root = test_root("ack");
            let mut service = ApplicationService::open(desktop_application_config(&root))
                .await
                .expect("open application");
            let owner = ViewSetOwner::trusted("desktop-stream-test");
            let list = list_view();
            let opened = service
                .open_view_set(
                    owner.clone(),
                    OpenViewSetRequest {
                        views: vec![list.clone()],
                        options: OpenViewSetOptions::default(),
                    },
                )
                .expect("open view set");
            let view_set = service
                .view_set(&owner, &opened.view_set_id)
                .expect("find view set");
            let view_set_id = opened.view_set_id.clone();
            let initial_cursor = opened.initial.cursor.clone();
            let cancellation = CancellationToken::new();
            let (acknowledgements, acknowledgement_receiver) = mpsc::channel(1);
            let (events, mut received_events) = mpsc::unbounded_channel();
            let task_cancellation = cancellation.clone();
            let task = tauri::async_runtime::spawn(async move {
                run_view_stream(
                    view_set,
                    initial_cursor,
                    task_cancellation,
                    acknowledgement_receiver,
                    move |event| events.send(event).map_err(|_| ()),
                )
                .await;
            });

            service
                .update_view_set(
                    &owner,
                    &view_set_id,
                    UpdateViewSetRequest {
                        views: vec![list.clone(), disk_view()],
                    },
                )
                .expect("add disk view");
            let first = next_batch(&mut received_events).await;
            service
                .update_view_set(
                    &owner,
                    &view_set_id,
                    UpdateViewSetRequest { views: vec![list] },
                )
                .expect("remove disk view");
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(25), received_events.recv(),)
                    .await
                    .is_err(),
                "a second batch arrived before the first was acknowledged"
            );

            acknowledgements
                .send(first.cursor.clone())
                .await
                .expect("acknowledge first batch");
            let second = next_batch(&mut received_events).await;
            assert!(second.updates.iter().any(|update| matches!(
                update,
                ViewSetUpdate::ViewRemoved { view_id } if view_id == "disk"
            )));

            acknowledgements
                .send("999".to_owned())
                .await
                .expect("send wrong acknowledgement");
            let error =
                tokio::time::timeout(std::time::Duration::from_secs(1), received_events.recv())
                    .await
                    .expect("stream error timeout")
                    .expect("stream error event");
            match error {
                DesktopViewStreamEvent::Error { error } => {
                    assert_eq!(error.code, "invalid_cursor");
                }
                DesktopViewStreamEvent::Batch { .. } => {
                    panic!("wrong acknowledgement unexpectedly advanced the stream")
                }
            }
            task.await.expect("join stream task");
            service.shutdown().await.expect("shutdown application");
            std::fs::remove_dir_all(&root).expect("remove test root");
        });
    }

    #[test]
    fn stream_cancellation_joins_a_blocked_view_wait() {
        tauri::async_runtime::block_on(async {
            let root = test_root("cancel");
            let mut service = ApplicationService::open(desktop_application_config(&root))
                .await
                .expect("open application");
            let owner = ViewSetOwner::trusted("desktop-cancel-test");
            let opened = service
                .open_view_set(
                    owner.clone(),
                    OpenViewSetRequest {
                        views: vec![list_view()],
                        options: OpenViewSetOptions::default(),
                    },
                )
                .expect("open view set");
            let view_set = service
                .view_set(&owner, &opened.view_set_id)
                .expect("find view set");
            let initial_cursor = opened.initial.cursor.clone();
            let cancellation = CancellationToken::new();
            let task_cancellation = cancellation.clone();
            let (_acknowledgements, acknowledgement_receiver) = mpsc::channel(1);
            let task = tauri::async_runtime::spawn(async move {
                run_view_stream(
                    view_set,
                    initial_cursor,
                    task_cancellation,
                    acknowledgement_receiver,
                    |_| Ok(()),
                )
                .await;
            });

            tokio::task::yield_now().await;
            cancellation.cancel();
            tokio::time::timeout(std::time::Duration::from_secs(1), task)
                .await
                .expect("stream task did not stop promptly")
                .expect("join stream task");
            service.shutdown().await.expect("shutdown application");
            std::fs::remove_dir_all(&root).expect("remove test root");
        });
    }

    fn list_view() -> ViewSpec {
        ViewSpec::TorrentList {
            view_id: "library".to_owned(),
            delivery: ViewDeliveryPolicy::default(),
        }
    }

    fn disk_view() -> ViewSpec {
        ViewSpec::SessionDisk {
            view_id: "disk".to_owned(),
            delivery: ViewDeliveryPolicy::default(),
        }
    }

    async fn next_batch(
        events: &mut mpsc::UnboundedReceiver<DesktopViewStreamEvent>,
    ) -> rstorrent_session::UpdateBatch {
        let event = tokio::time::timeout(std::time::Duration::from_secs(1), events.recv())
            .await
            .expect("batch timeout")
            .expect("batch event");
        match event {
            DesktopViewStreamEvent::Batch { batch } => *batch,
            DesktopViewStreamEvent::Error { error } => {
                panic!("unexpected stream error {}: {}", error.code, error.message)
            }
        }
    }

    fn test_root(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "rstorrent-desktop-{label}-{}-{}",
            std::process::id(),
            NEXT_TEST_ROOT.fetch_add(1, Ordering::Relaxed)
        ))
    }
}
