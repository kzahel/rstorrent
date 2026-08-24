#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use rstorrent_media::LoopbackMediaServer;
use rstorrent_session::{
    AddTorrentBytesRequest, ApplicationConfig, ApplicationService, CONTROL_VERSION, FileIndexRange,
    FileSelectionIntent, MediaUrlResponse, NetworkConfig, NetworkPolicy, RequestEnvelope,
    ResponseEnvelope, StorageRootSnapshot, SubscriptionSpec, ViewSubscription, ViewUpdate,
    application_error_response,
};
use tauri::WebviewWindowBuilder;
use tauri::ipc::{Channel, InvokeBody, Request as IpcRequest};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WebviewWindow, WindowEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tokio::sync::{Mutex, Semaphore, watch};
use tokio_util::sync::CancellationToken;

mod desktop_lifecycle;
mod updater;
mod view_delivery;

use desktop_lifecycle::{
    CloseAction, DesktopShellSettings, ShutdownGate, ShutdownPhase, close_action,
    load_desktop_shell_settings, persist_run_in_background,
};
use updater::{desktop_release_info, get_or_create_installation_id};
use view_delivery::{
    DesktopViewResources, application_view_close, application_view_hello, application_view_open,
    application_view_stream, application_view_stream_ack, application_view_stream_close,
    application_view_update,
};

const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_ID: &str = "rstorrent-tray";
const TRAY_SHOW_ID: &str = "rstorrent-tray-show";
const TRAY_UPDATE_ID: &str = "rstorrent-tray-update";
const TRAY_BACKGROUND_ID: &str = "rstorrent-tray-background";
const TRAY_QUIT_ID: &str = "rstorrent-tray-quit";
const UPDATE_CHECK_EVENT: &str = "rstorrent://check-for-updates";
const PEER_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const PEER_IO_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_TORRENT_SOURCE_BYTES: usize = 64 * 1024 * 1024;

const HEADER_REQUEST_ID: &str = "x-rstorrent-request-id";
const HEADER_EXPECTED_REVISION: &str = "x-rstorrent-expected-revision";
const HEADER_STORAGE_ROOT: &str = "x-rstorrent-storage-root";
const HEADER_START_CONTENT: &str = "x-rstorrent-start-content";
const HEADER_SELECTION: &str = "x-rstorrent-selection";
const HEADER_WANTED_RANGES: &str = "x-rstorrent-wanted-ranges";

struct DesktopState {
    service: Arc<Mutex<ApplicationService>>,
    subscriptions: Arc<Mutex<BTreeMap<(String, String), DesktopSubscription>>>,
    view_resources: Arc<DesktopViewResources>,
    torrent_uploads: Arc<Semaphore>,
    media_server: Mutex<Option<LoopbackMediaServer>>,
    window_generation: AtomicU64,
    shell_settings_path: PathBuf,
    shell_settings: StdMutex<DesktopShellSettings>,
    background_menu_item: CheckMenuItem<tauri::Wry>,
    shutdown: ShutdownGate,
    shutdown_status: watch::Sender<ShutdownPhase>,
    shutdown_error: StdMutex<Option<String>>,
    restart_after_shutdown: AtomicBool,
    update_check_generation: AtomicU64,
}

struct DesktopSubscription {
    window_generation: u64,
    subscription: ViewSubscription,
    cancellation: CancellationToken,
    task: tauri::async_runtime::JoinHandle<()>,
}

#[tauri::command]
async fn application_dispatch(
    state: State<'_, DesktopState>,
    request: RequestEnvelope,
) -> Result<ResponseEnvelope, String> {
    let request_id = request.request_id.clone();
    let mut service = state.service.lock().await;
    Ok(match service.dispatch(request).await {
        Ok(response) => response,
        Err(error) => {
            application_error_response(request_id, service.revision().unwrap_or(0), &error)
        }
    })
}

#[tauri::command]
async fn application_create_media_url(
    state: State<'_, DesktopState>,
    torrent_id: String,
    file_index: u32,
) -> Result<MediaUrlResponse, String> {
    state
        .service
        .lock()
        .await
        .create_media_url(&torrent_id, file_index)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn application_open_media_url(
    state: State<'_, DesktopState>,
    url: String,
) -> Result<(), String> {
    let media_server = state.media_server.lock().await;
    let server = media_server
        .as_ref()
        .ok_or_else(|| "media server is unavailable".to_owned())?;
    validate_local_media_url(&url, server.local_addr())?;
    open_with_system(&url)
}

fn validate_local_media_url(
    source: &str,
    expected_address: std::net::SocketAddr,
) -> Result<(), String> {
    let url = url::Url::parse(source).map_err(|_| "media URL is invalid".to_owned())?;
    if url.scheme() != "http"
        || url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.host_str() != Some("127.0.0.1")
        || url.port() != Some(expected_address.port())
    {
        return Err("media URL is not owned by this desktop process".to_owned());
    }
    let Some(capability) = url.path().strip_prefix("/media/v1/") else {
        return Err("media URL path is invalid".to_owned());
    };
    if capability.len() != 43
        || !capability
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("media URL capability is invalid".to_owned());
    }
    Ok(())
}

fn open_with_system(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(url);
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };
    #[cfg(not(any(unix, target_os = "windows")))]
    return Err("opening media URLs is unsupported on this platform".to_owned());

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("open media URL with system handler: {error}"))
}

#[tauri::command]
async fn application_add_torrent_bytes(
    state: State<'_, DesktopState>,
    ipc: IpcRequest<'_>,
) -> Result<ResponseEnvelope, String> {
    let (request, source) = decode_torrent_ipc(ipc.body(), ipc.headers())?;
    let request_id = request.request_id.clone();
    let permit = state
        .torrent_uploads
        .clone()
        .try_acquire_owned()
        .map_err(|_| "another torrent upload is already in progress".to_owned())?;
    let mut service = state.service.lock().await;
    let response = match service.add_torrent_bytes(request, source).await {
        Ok(response) => response,
        Err(error) => {
            application_error_response(request_id, service.revision().unwrap_or_default(), &error)
        }
    };
    drop(permit);
    Ok(response)
}

fn decode_torrent_ipc(
    body: &InvokeBody,
    headers: &tauri::http::HeaderMap,
) -> Result<(AddTorrentBytesRequest, Vec<u8>), String> {
    let source = match body {
        InvokeBody::Raw(source) => source.clone(),
        InvokeBody::Json(_) => {
            return Err("torrent intake requires a raw IPC body".to_owned());
        }
    };
    if source.is_empty() || source.len() > MAX_TORRENT_SOURCE_BYTES {
        return Err(format!(
            "torrent source length must be 1..={MAX_TORRENT_SOURCE_BYTES} bytes"
        ));
    }
    let request_id = required_ipc_header(headers, HEADER_REQUEST_ID)?;
    let storage_root = required_ipc_header(headers, HEADER_STORAGE_ROOT)?;
    let expected_revision = optional_ipc_header(headers, HEADER_EXPECTED_REVISION)?;
    let start_content = match optional_ipc_header(headers, HEADER_START_CONTENT)?.as_deref() {
        None | Some("true") => true,
        Some("false") => false,
        Some(_) => return Err("x-rstorrent-start-content must be true or false".to_owned()),
    };
    let selection = parse_ipc_selection(
        optional_ipc_header(headers, HEADER_SELECTION)?.as_deref(),
        optional_ipc_header(headers, HEADER_WANTED_RANGES)?.as_deref(),
    )?;
    let request = AddTorrentBytesRequest {
        version: CONTROL_VERSION,
        request_id,
        expected_revision,
        storage_root,
        start_content,
        selection,
        source_length: source.len() as u32,
    };
    Ok((request, source))
}

fn required_ipc_header(
    headers: &tauri::http::HeaderMap,
    name: &'static str,
) -> Result<String, String> {
    optional_ipc_header(headers, name)?
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("missing {name} header"))
}

fn optional_ipc_header(
    headers: &tauri::http::HeaderMap,
    name: &'static str,
) -> Result<Option<String>, String> {
    headers
        .get(name)
        .map(|value| {
            value
                .to_str()
                .map(str::to_owned)
                .map_err(|_| format!("{name} header is not valid text"))
        })
        .transpose()
}

fn parse_ipc_selection(
    mode: Option<&str>,
    ranges: Option<&str>,
) -> Result<FileSelectionIntent, String> {
    match mode.unwrap_or("all") {
        "all" if ranges.is_none() => Ok(FileSelectionIntent::All),
        "none" if ranges.is_none() => Ok(FileSelectionIntent::None),
        "ranges" => Ok(FileSelectionIntent::WantedRanges {
            ranges: parse_ipc_file_ranges(ranges.unwrap_or(""))?,
        }),
        "all" | "none" => Err("x-rstorrent-wanted-ranges requires selection=ranges".to_owned()),
        _ => Err("x-rstorrent-selection must be all, none, or ranges".to_owned()),
    }
}

fn parse_ipc_file_ranges(value: &str) -> Result<Vec<FileIndexRange>, String> {
    if value.is_empty() {
        return Ok(Vec::new());
    }
    value
        .split(',')
        .map(|part| {
            let (start, end) = part.split_once('-').ok_or_else(|| {
                "x-rstorrent-wanted-ranges must contain start-end pairs".to_owned()
            })?;
            Ok(FileIndexRange {
                start: parse_ipc_u32(start)?,
                end_exclusive: parse_ipc_u32(end)?,
            })
        })
        .collect()
}

fn parse_ipc_u32(value: &str) -> Result<u32, String> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err("x-rstorrent-wanted-ranges must contain canonical integers".to_owned());
    }
    value
        .parse()
        .map_err(|_| "x-rstorrent-wanted-ranges index exceeds u32".to_owned())
}

#[tauri::command]
async fn choose_download_root(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    repair_root: Option<String>,
) -> Result<Option<StorageRootSnapshot>, String> {
    let suggested = state
        .service
        .lock()
        .await
        .suggested_storage_root_path(repair_root.as_deref())
        .map_err(|error| error.to_string())?;
    let starting_directory = match suggested {
        Some(path) => path,
        None => window
            .app_handle()
            .path()
            .home_dir()
            .map_err(|error| format!("resolve folder-picker home directory: {error}"))?,
    };
    let selected = pick_download_directory(&window, &starting_directory).await?;
    let mut service = state.service.lock().await;
    register_download_root_selection(&mut service, repair_root.as_deref(), selected)
}

async fn pick_download_directory(
    window: &WebviewWindow,
    starting_directory: &Path,
) -> Result<Option<PathBuf>, String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    window
        .dialog()
        .file()
        .set_parent(window)
        .set_title("Choose a download folder")
        .set_directory(starting_directory)
        .pick_folder(move |selection| {
            let _ = sender.send(selection);
        });
    resolve_download_directory_selection(receiver).await
}

async fn resolve_download_directory_selection(
    receiver: tokio::sync::oneshot::Receiver<Option<tauri_plugin_dialog::FilePath>>,
) -> Result<Option<PathBuf>, String> {
    let selection = receiver
        .await
        .map_err(|_| "download folder picker closed without a result".to_owned())?;
    selection
        .map(|path| {
            path.into_path()
                .map_err(|error| format!("resolve selected download folder: {error}"))
        })
        .transpose()
}

fn register_download_root_selection(
    service: &mut ApplicationService,
    repair_root: Option<&str>,
    selected: Option<PathBuf>,
) -> Result<Option<StorageRootSnapshot>, String> {
    let Some(selected) = selected else {
        return Ok(None);
    };
    if let Some(root_id) = repair_root {
        service
            .repair_path_storage_root(root_id, &selected)
            .map(Some)
            .map_err(|error| error.to_string())
    } else {
        service
            .install_path_storage_root(&selected)
            .map(Some)
            .map_err(|error| error.to_string())
    }
}

#[tauri::command]
async fn application_subscribe(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    spec: SubscriptionSpec,
    updates: Channel<ViewUpdate>,
) -> Result<String, String> {
    let subscription = state
        .service
        .lock()
        .await
        .subscribe(spec)
        .map_err(|error| error.to_string())?;
    let stream_id = subscription.stream_id();
    let cancellation = CancellationToken::new();
    let task_subscription = subscription.clone();
    let task_cancellation = cancellation.clone();
    let task = tauri::async_runtime::spawn(async move {
        loop {
            let update = tokio::select! {
                () = task_cancellation.cancelled() => break,
                update = task_subscription.next_update() => update,
            };
            let Some(update) = update else {
                break;
            };
            if updates.send(update).is_err() {
                break;
            }
        }
        task_subscription.close();
    });
    let key = (window.label().to_owned(), stream_id.clone());
    let window_generation = state.window_generation.load(Ordering::Acquire);
    let replaced = state.subscriptions.lock().await.insert(
        key,
        DesktopSubscription {
            window_generation,
            subscription,
            cancellation,
            task,
        },
    );
    if let Some(replaced) = replaced {
        stop_subscription(replaced).await;
    }
    Ok(stream_id)
}

#[tauri::command]
async fn application_resync(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    stream_id: String,
) -> Result<(), String> {
    let subscriptions = state.subscriptions.lock().await;
    let subscription = subscriptions
        .get(&(window.label().to_owned(), stream_id))
        .ok_or_else(|| "unknown subscription for this window".to_owned())?;
    subscription
        .subscription
        .resync()
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn application_unsubscribe(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    stream_id: String,
) -> Result<(), String> {
    let subscription = state
        .subscriptions
        .lock()
        .await
        .remove(&(window.label().to_owned(), stream_id))
        .ok_or_else(|| "unknown subscription for this window".to_owned())?;
    stop_subscription(subscription).await;
    Ok(())
}

#[tauri::command]
async fn application_shutdown(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<(), String> {
    let started = request_application_shutdown(&app, false);
    if !started && state.shutdown.phase() == ShutdownPhase::Running {
        return Err("desktop shutdown could not be started".to_owned());
    }
    wait_for_application_shutdown(&state).await
}

#[tauri::command]
async fn application_restart(app: AppHandle, state: State<'_, DesktopState>) -> Result<(), String> {
    let started = request_application_shutdown(&app, true);
    if !started && state.shutdown.phase() == ShutdownPhase::Running {
        return Err("desktop restart shutdown could not be started".to_owned());
    }
    wait_for_application_shutdown(&state).await
}

fn request_application_shutdown(app: &AppHandle, restart_after_shutdown: bool) -> bool {
    let state = app.state::<DesktopState>();
    if !state.shutdown.try_start() {
        return false;
    }
    state
        .restart_after_shutdown
        .store(restart_after_shutdown, Ordering::Release);
    state.shutdown_status.send_replace(ShutdownPhase::Stopping);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        perform_application_shutdown(app).await;
    });
    true
}

async fn wait_for_application_shutdown(state: &DesktopState) -> Result<(), String> {
    let mut status = state.shutdown_status.subscribe();
    loop {
        let phase = *status.borrow_and_update();
        match phase {
            ShutdownPhase::Running | ShutdownPhase::Stopping => {
                status
                    .changed()
                    .await
                    .map_err(|_| "desktop shutdown status closed unexpectedly".to_owned())?;
            }
            ShutdownPhase::FinalExit => return Ok(()),
            ShutdownPhase::Failed => {
                return Err(state
                    .shutdown_error
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone()
                    .unwrap_or_else(|| "desktop shutdown failed".to_owned()));
            }
        }
    }
}

async fn perform_application_shutdown(app: AppHandle) {
    let state = app.state::<DesktopState>();
    let subscriptions = {
        let mut subscriptions = state.subscriptions.lock().await;
        std::mem::take(&mut *subscriptions)
    };
    for (_, subscription) in subscriptions {
        stop_subscription(subscription).await;
    }
    state.view_resources.close_all().await;
    let service_result = state
        .service
        .lock()
        .await
        .shutdown()
        .await
        .map_err(|error| error.to_string());
    let media_result = if let Some(mut media_server) = state.media_server.lock().await.take() {
        media_server
            .shutdown()
            .await
            .map_err(|error| error.to_string())
    } else {
        Ok(())
    };
    let result = match (service_result, media_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(service), Ok(())) => Err(format!("application service shutdown: {service}")),
        (Ok(()), Err(media)) => Err(format!("media server shutdown: {media}")),
        (Err(service), Err(media)) => Err(format!(
            "application service shutdown: {service}; media server shutdown: {media}"
        )),
    };
    match result {
        Ok(()) => {
            let restart = state.restart_after_shutdown.load(Ordering::Acquire);
            state.shutdown.complete();
            state.shutdown_status.send_replace(ShutdownPhase::FinalExit);
            if restart {
                app.request_restart();
            } else {
                app.exit(0);
            }
        }
        Err(error) => {
            let error = bounded_diagnostic(error);
            *state
                .shutdown_error
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error.clone());
            state.shutdown.fail();
            state.shutdown_status.send_replace(ShutdownPhase::Failed);
            eprintln!("desktop shutdown failed: {error}");
            show_shutdown_failure(&app);
        }
    }
}

fn bounded_diagnostic(mut error: String) -> String {
    const MAX_CHARS: usize = 1_024;
    if error.chars().count() > MAX_CHARS {
        error = error.chars().take(MAX_CHARS).collect();
        error.push('…');
    }
    error
}

fn show_shutdown_failure(app: &AppHandle) {
    if let Err(error) = restore_main_window(app) {
        eprintln!("failed to restore desktop window after shutdown failure: {error}");
    }
    let dialog = app
        .dialog()
        .message("RSTorrent could not finish shutting down. Your data was not force-closed. Check the diagnostic log before trying again.")
        .title("RSTorrent could not quit")
        .kind(MessageDialogKind::Error);
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        dialog.parent(&window).show(|_| {});
    } else {
        dialog.show(|_| {});
    }
}

async fn stop_subscription(subscription: DesktopSubscription) {
    subscription.cancellation.cancel();
    subscription.subscription.close();
    let _ = subscription.task.await;
}

async fn close_window_subscriptions(
    subscriptions: Arc<Mutex<BTreeMap<(String, String), DesktopSubscription>>>,
    label: String,
    window_generation: u64,
) {
    let removed = {
        let mut subscriptions = subscriptions.lock().await;
        let keys = subscriptions
            .iter()
            .filter(|(key, subscription)| {
                key.0 == label && subscription.window_generation == window_generation
            })
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        keys.into_iter()
            .filter_map(|key| subscriptions.remove(&key))
            .collect::<Vec<_>>()
    };
    for subscription in removed {
        stop_subscription(subscription).await;
    }
}

fn observe_window_destruction(
    window: &WebviewWindow,
    service: Arc<Mutex<ApplicationService>>,
    subscriptions: Arc<Mutex<BTreeMap<(String, String), DesktopSubscription>>>,
    view_resources: Arc<DesktopViewResources>,
    window_generation: u64,
) {
    let label = window.label().to_owned();
    let app = window.app_handle().clone();
    window.on_window_event(move |event| match event {
        WindowEvent::CloseRequested { api, .. } => {
            let state = app.state::<DesktopState>();
            let run_in_background = state
                .shell_settings
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .run_in_background;
            match close_action(state.shutdown.phase(), run_in_background) {
                CloseAction::Allow => {}
                CloseAction::Hide => {
                    api.prevent_close();
                    if let Some(window) = app.get_webview_window(&label)
                        && let Err(error) = window.hide()
                    {
                        eprintln!("failed to hide desktop window: {error}");
                    }
                }
                CloseAction::StartShutdown => {
                    api.prevent_close();
                    request_application_shutdown(&app, false);
                }
                CloseAction::Prevent => api.prevent_close(),
            }
        }
        WindowEvent::Destroyed => {
            let service = service.clone();
            let subscriptions = subscriptions.clone();
            let view_resources = view_resources.clone();
            let label = label.clone();
            tauri::async_runtime::spawn(async move {
                close_window_subscriptions(subscriptions, label.clone(), window_generation).await;
                view_resources
                    .close_window(service, label, window_generation)
                    .await;
            });
        }
        _ => {}
    });
}

fn restore_main_window(app: &AppHandle) -> Result<(), String> {
    if let Some(state) = app.try_state::<DesktopState>()
        && matches!(
            state.shutdown.phase(),
            ShutdownPhase::Stopping | ShutdownPhase::FinalExit
        )
    {
        return Ok(());
    }
    let window = if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        window
    } else {
        let state = app
            .try_state::<DesktopState>()
            .ok_or_else(|| "desktop application state is not ready".to_owned())?;
        let config = app
            .config()
            .app
            .windows
            .iter()
            .find(|config| config.label == MAIN_WINDOW_LABEL)
            .cloned()
            .ok_or_else(|| "main webview window configuration is missing".to_owned())?;
        let window_generation = state.window_generation.fetch_add(1, Ordering::AcqRel) + 1;
        let window = WebviewWindowBuilder::from_config(app, &config)
            .map_err(|error| format!("configure main webview window: {error}"))?
            .build()
            .map_err(|error| format!("recreate main webview window: {error}"))?;
        observe_window_destruction(
            &window,
            state.service.clone(),
            state.subscriptions.clone(),
            state.view_resources.clone(),
            window_generation,
        );
        window
    };
    apply_platform_window_icon(&window)?;
    window
        .unminimize()
        .map_err(|error| format!("restore main webview window: {error}"))?;
    window
        .show()
        .map_err(|error| format!("show main webview window: {error}"))?;
    window
        .set_focus()
        .map_err(|error| format!("focus main webview window: {error}"))
}

#[cfg(target_os = "linux")]
fn apply_platform_window_icon(window: &WebviewWindow) -> Result<(), String> {
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/icon.png"))
        .map_err(|error| format!("decode Linux desktop window icon: {error}"))?;
    window
        .set_icon(icon)
        .map_err(|error| format!("set Linux desktop window icon: {error}"))
}

#[cfg(not(target_os = "linux"))]
fn apply_platform_window_icon(_window: &WebviewWindow) -> Result<(), String> {
    Ok(())
}

fn build_desktop_tray_menu(
    app: &tauri::App,
    run_in_background: bool,
) -> Result<(Menu<tauri::Wry>, CheckMenuItem<tauri::Wry>), String> {
    let show = MenuItem::with_id(app, TRAY_SHOW_ID, "Show RSTorrent", true, None::<&str>)
        .map_err(|error| format!("create tray Show item: {error}"))?;
    let update = MenuItem::with_id(app, TRAY_UPDATE_ID, "Check for Updates", true, None::<&str>)
        .map_err(|error| format!("create tray update item: {error}"))?;
    let background = CheckMenuItem::with_id(
        app,
        TRAY_BACKGROUND_ID,
        "Run in Background",
        true,
        run_in_background,
        None::<&str>,
    )
    .map_err(|error| format!("create tray background item: {error}"))?;
    let quit = MenuItem::with_id(app, TRAY_QUIT_ID, "Quit RSTorrent", true, None::<&str>)
        .map_err(|error| format!("create tray Quit item: {error}"))?;
    let separator = PredefinedMenuItem::separator(app)
        .map_err(|error| format!("create tray separator: {error}"))?;
    let menu = Menu::with_items(app, &[&show, &update, &background, &separator, &quit])
        .map_err(|error| format!("create desktop tray menu: {error}"))?;
    Ok((menu, background))
}

fn install_desktop_tray(app: &tauri::App, menu: &Menu<tauri::Wry>) -> Result<(), String> {
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/icon.png"))
        .map_err(|error| format!("decode desktop tray icon: {error}"))?;
    TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("RSTorrent")
        .icon(icon)
        .menu(menu)
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(handle_desktop_menu_event)
        .on_tray_icon_event(|tray, event| {
            if !cfg!(target_os = "macos")
                && matches!(
                    event,
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    }
                )
                && let Err(error) = restore_main_window(tray.app_handle())
            {
                eprintln!("failed to restore desktop window from tray: {error}");
            }
        })
        .build(app)
        .map_err(|error| format!("build desktop tray: {error}"))?;
    Ok(())
}

fn handle_desktop_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    match event.id().as_ref() {
        TRAY_SHOW_ID => {
            if let Err(error) = restore_main_window(app) {
                eprintln!("failed to restore desktop window from menu: {error}");
            }
        }
        TRAY_UPDATE_ID => request_manual_update_check(app),
        TRAY_BACKGROUND_ID => toggle_run_in_background(app),
        TRAY_QUIT_ID => {
            request_application_shutdown(app, false);
        }
        _ => {}
    }
}

fn toggle_run_in_background(app: &AppHandle) {
    let state = app.state::<DesktopState>();
    let mut settings = state
        .shell_settings
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let current = *settings;
    let requested = !current.run_in_background;
    match persist_run_in_background(&state.shell_settings_path, current, requested) {
        Ok(next) => {
            *settings = next;
            drop(settings);
            if let Err(error) = state.background_menu_item.set_checked(requested) {
                eprintln!("failed to update background menu checkmark: {error}");
            }
        }
        Err(error) => {
            drop(settings);
            let _ = state
                .background_menu_item
                .set_checked(current.run_in_background);
            eprintln!("failed to persist desktop background setting: {error}");
            show_settings_failure(app);
        }
    }
}

fn show_settings_failure(app: &AppHandle) {
    if let Err(error) = restore_main_window(app) {
        eprintln!("failed to restore desktop window after settings failure: {error}");
    }
    let dialog = app
        .dialog()
        .message("RSTorrent could not save the Run in Background setting. The previous setting is still active.")
        .title("RSTorrent setting was not saved")
        .kind(MessageDialogKind::Error);
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        dialog.parent(&window).show(|_| {});
    } else {
        dialog.show(|_| {});
    }
}

fn request_manual_update_check(app: &AppHandle) {
    if let Err(error) = restore_main_window(app) {
        eprintln!("failed to restore desktop window for update check: {error}");
        return;
    }
    let generation = app
        .state::<DesktopState>()
        .update_check_generation
        .fetch_add(1, Ordering::AcqRel)
        .wrapping_add(1);
    if let Err(error) = app.emit(UPDATE_CHECK_EVENT, generation) {
        eprintln!("failed to deliver desktop update-check request: {error}");
    }
}

#[tauri::command]
fn desktop_update_check_generation(state: State<'_, DesktopState>) -> u64 {
    state.update_check_generation.load(Ordering::Acquire)
}

pub fn run() {
    let application = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(
            |app, _arguments, _cwd| {
                if let Err(error) = restore_main_window(app) {
                    eprintln!("failed to restore desktop window for second launch: {error}");
                }
            },
        ))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let config_dir = app
                .path()
                .app_config_dir()
                .map_err(|error| format!("resolve desktop config directory: {error}"))?;
            let shell_settings = load_desktop_shell_settings(&config_dir);
            if let Some(diagnostic) = &shell_settings.diagnostic {
                eprintln!("{diagnostic}");
            }
            let (tray_menu, background_menu_item) =
                build_desktop_tray_menu(app, shell_settings.settings.run_in_background)?;
            let installation_id = get_or_create_installation_id(&config_dir)?;
            let updater = tauri_plugin_updater::Builder::new()
                .header("X-CFU-Id", &installation_id)?
                .build();
            app.handle().plugin(updater)?;
            let app_data = app
                .path()
                .app_data_dir()
                .map_err(|error| format!("resolve application data directory: {error}"))?;
            let service = tauri::async_runtime::block_on(ApplicationService::open(
                desktop_application_config(&app_data),
            ))
            .map_err(|error| error.to_string())?;
            let service = Arc::new(Mutex::new(service));
            tauri::async_runtime::block_on(ApplicationService::ensure_maintenance_owner(&service));
            let media_server =
                tauri::async_runtime::block_on(LoopbackMediaServer::bind(service.clone()))
                    .map_err(|error| error.to_string())?;
            let state = DesktopState {
                service,
                subscriptions: Arc::new(Mutex::new(BTreeMap::new())),
                view_resources: Arc::new(DesktopViewResources::new()),
                torrent_uploads: Arc::new(Semaphore::new(1)),
                media_server: Mutex::new(Some(media_server)),
                window_generation: AtomicU64::new(1),
                shell_settings_path: shell_settings.path,
                shell_settings: StdMutex::new(shell_settings.settings),
                background_menu_item,
                shutdown: ShutdownGate::new(),
                shutdown_status: watch::channel(ShutdownPhase::Running).0,
                shutdown_error: StdMutex::new(None),
                restart_after_shutdown: AtomicBool::new(false),
                update_check_generation: AtomicU64::new(0),
            };
            let service = state.service.clone();
            let subscriptions = state.subscriptions.clone();
            let view_resources = state.view_resources.clone();
            let window = app
                .get_webview_window(MAIN_WINDOW_LABEL)
                .ok_or_else(|| "main webview window was not created".to_owned())?;
            apply_platform_window_icon(&window)?;
            observe_window_destruction(&window, service, subscriptions, view_resources, 1);
            app.manage(state);
            install_desktop_tray(app, &tray_menu)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            application_dispatch,
            application_create_media_url,
            application_open_media_url,
            application_add_torrent_bytes,
            choose_download_root,
            application_view_hello,
            application_view_open,
            application_view_update,
            application_view_stream,
            application_view_stream_ack,
            application_view_stream_close,
            application_view_close,
            application_subscribe,
            application_resync,
            application_unsubscribe,
            application_shutdown,
            application_restart,
            desktop_update_check_generation,
            desktop_release_info,
        ])
        .build(tauri::generate_context!())
        .expect("build RSTorrent desktop application");
    application.run(|handle, event| match event {
        RunEvent::ExitRequested { api, .. } => {
            let state = handle.state::<DesktopState>();
            if state.shutdown.phase() != ShutdownPhase::FinalExit {
                api.prevent_exit();
                request_application_shutdown(handle, false);
            }
        }
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => {
            if let Err(error) = restore_main_window(handle) {
                eprintln!("failed to restore desktop window: {error}");
            }
        }
        _ => {}
    });
}

fn desktop_application_config(app_data: &std::path::Path) -> ApplicationConfig {
    ApplicationConfig::new(
        app_data.join("profile"),
        "default".to_owned(),
        Vec::new(),
        NetworkConfig::new(NetworkPolicy::Online, PEER_CONNECT_TIMEOUT, PEER_IO_TIMEOUT),
    )
    .with_fresh_profile_defaults()
}

#[cfg(test)]
mod tests {
    use rstorrent_session::{
        ClientSettings, DownloadResourceLimits, ListenerPolicy, PeerTransportPolicy,
        PortMappingPolicy, StorageRootAvailability,
    };
    use tauri::ipc::InvokeBody;

    use super::{
        ApplicationConfig, HEADER_REQUEST_ID, HEADER_START_CONTENT, HEADER_STORAGE_ROOT,
        NetworkConfig, NetworkPolicy, decode_torrent_ipc, desktop_application_config,
        register_download_root_selection, resolve_download_directory_selection,
        validate_local_media_url,
    };

    #[test]
    fn desktop_product_explicitly_uses_online_networking() {
        let config = desktop_application_config(std::path::Path::new("/tmp/rstorrent-desktop"));
        assert_eq!(config.network.policy, NetworkPolicy::Online);
        assert_eq!(config.peer_transport_policy, PeerTransportPolicy::PreferUtp);
        assert_eq!(
            config.initial_client_settings,
            ClientSettings::fresh_profile_default()
        );
        assert_eq!(
            config.initial_client_settings.listener,
            ListenerPolicy::AutomaticLocalNetwork
        );
        assert_eq!(
            config.initial_client_settings.port_mapping,
            PortMappingPolicy::Upnp
        );
        assert!(config.storage_roots.is_empty());
        assert_eq!(
            config.download_resource_limits,
            DownloadResourceLimits::DESKTOP
        );
    }

    #[tokio::test]
    async fn native_picker_selection_installs_repairs_and_restores_root() {
        let temporary = tempfile::tempdir().expect("temporary picker profile");
        let selected = temporary.path().join("selected");
        std::fs::create_dir(&selected).expect("create selected directory");
        let configuration = ApplicationConfig::new(
            temporary.path().join("profile"),
            "picker-test".to_owned(),
            Vec::new(),
            NetworkConfig::new(
                NetworkPolicy::LoopbackOnly,
                std::time::Duration::from_secs(5),
                std::time::Duration::from_secs(5),
            ),
        )
        .with_fresh_profile_defaults();
        let mut service = rstorrent_session::ApplicationService::open(configuration.clone())
            .await
            .expect("open fresh picker profile");

        assert_eq!(
            register_download_root_selection(&mut service, None, None).expect("cancel selection"),
            None
        );
        assert_eq!(
            service
                .storage_snapshot()
                .expect("storage after cancel")
                .roots,
            Vec::new()
        );

        let installed =
            register_download_root_selection(&mut service, None, Some(selected.clone()))
                .expect("install selection")
                .expect("installed root");
        let root_id = installed.root_id.clone();
        let snapshot = service.storage_snapshot().expect("installed storage");
        assert_eq!(snapshot.default_root.as_deref(), Some(root_id.as_str()));
        assert_eq!(snapshot.roots, vec![installed]);

        service.shutdown().await.expect("shutdown selected profile");
        drop(service);
        std::fs::remove_dir(&selected).expect("make selected root unavailable");

        let mut reopened = rstorrent_session::ApplicationService::open(configuration.clone())
            .await
            .expect("reopen unavailable picker profile");
        let unavailable = reopened.storage_snapshot().expect("unavailable storage");
        assert_eq!(unavailable.default_root.as_deref(), Some(root_id.as_str()));
        assert_eq!(unavailable.roots[0].root_id, root_id);
        assert_eq!(
            unavailable.roots[0].availability,
            StorageRootAvailability::Unavailable
        );

        let repaired_directory = temporary.path().join("repaired");
        std::fs::create_dir(&repaired_directory).expect("create repair directory");
        let repaired = register_download_root_selection(
            &mut reopened,
            Some(&root_id),
            Some(repaired_directory),
        )
        .expect("repair selection")
        .expect("repaired root");
        assert_eq!(repaired.root_id, root_id);
        assert_eq!(repaired.availability, StorageRootAvailability::Available);
        reopened
            .shutdown()
            .await
            .expect("shutdown repaired profile");
        drop(reopened);

        let mut restored = rstorrent_session::ApplicationService::open(configuration)
            .await
            .expect("restore repaired picker profile");
        let restored_snapshot = restored.storage_snapshot().expect("restored storage");
        assert_eq!(
            restored_snapshot.default_root.as_deref(),
            Some(root_id.as_str())
        );
        assert_eq!(restored_snapshot.roots.len(), 1);
        assert_eq!(restored_snapshot.roots[0].root_id, root_id);
        assert_eq!(
            restored_snapshot.roots[0].availability,
            StorageRootAvailability::Available
        );
        restored
            .shutdown()
            .await
            .expect("shutdown restored profile");
    }

    #[tokio::test]
    async fn native_picker_callback_channel_fails_closed() {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        drop(sender);
        assert_eq!(
            resolve_download_directory_selection(receiver)
                .await
                .expect_err("closed picker callback must fail"),
            "download folder picker closed without a result"
        );

        let (sender, receiver) = tokio::sync::oneshot::channel();
        sender.send(None).expect("send picker cancellation");
        assert_eq!(
            resolve_download_directory_selection(receiver)
                .await
                .expect("resolve picker cancellation"),
            None
        );
    }

    #[test]
    fn torrent_ipc_requires_raw_bytes_and_decodes_bounded_headers() {
        let source =
            b"d4:infod6:lengthi4e4:name4:test12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaaee"
                .to_vec();
        let mut headers = tauri::http::HeaderMap::new();
        headers.insert(HEADER_REQUEST_ID, "desktop-upload".parse().expect("header"));
        headers.insert(HEADER_STORAGE_ROOT, "downloads".parse().expect("header"));
        headers.insert(HEADER_START_CONTENT, "false".parse().expect("header"));

        let (request, decoded) =
            decode_torrent_ipc(&InvokeBody::Raw(source.clone()), &headers).expect("decode raw IPC");
        assert_eq!(decoded, source);
        assert_eq!(request.request_id, "desktop-upload");
        assert_eq!(request.storage_root, "downloads");
        assert!(!request.start_content);
        assert_eq!(request.source_length as usize, decoded.len());
        assert!(
            decode_torrent_ipc(&InvokeBody::Json(serde_json::json!([1, 2, 3])), &headers)
                .expect_err("reject JSON IPC")
                .contains("raw IPC body")
        );
    }

    #[test]
    fn desktop_opener_only_accepts_its_exact_media_origin_and_capability() {
        let address = "127.0.0.1:43121".parse().expect("address");
        let capability = "A".repeat(43);
        let valid = format!("http://127.0.0.1:43121/media/v1/{capability}");
        assert!(validate_local_media_url(&valid, address).is_ok());
        for invalid in [
            format!("http://localhost:43121/media/v1/{capability}"),
            format!("http://127.0.0.1:43122/media/v1/{capability}"),
            format!("https://127.0.0.1:43121/media/v1/{capability}"),
            format!("http://127.0.0.1:43121/media/v1/{capability}?copy=1"),
            "http://127.0.0.1:43121/media/v1/short".to_owned(),
        ] {
            assert!(
                validate_local_media_url(&invalid, address).is_err(),
                "{invalid}"
            );
        }
    }
}
