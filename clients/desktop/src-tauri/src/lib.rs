#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use rstorrent_media::LoopbackMediaServer;
use rstorrent_session::{
    AddTorrentBytesRequest, ApplicationConfig, ApplicationService, CONTROL_VERSION, FileIndexRange,
    FileSelectionIntent, MediaUrlResponse, NetworkConfig, NetworkPolicy, RequestEnvelope,
    ResponseEnvelope, StorageRootSnapshot, SubscriptionSpec, ViewSubscription, ViewUpdate,
    application_error_response,
};
#[cfg(target_os = "macos")]
use tauri::WebviewWindowBuilder;
use tauri::ipc::{Channel, InvokeBody, Request as IpcRequest};
use tauri::{AppHandle, Manager, RunEvent, State, WebviewWindow, WindowEvent};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;

mod updater;
mod view_delivery;

use updater::{desktop_release_info, get_or_create_installation_id};
use view_delivery::{
    DesktopViewResources, application_view_close, application_view_hello, application_view_open,
    application_view_stream, application_view_stream_ack, application_view_stream_close,
    application_view_update,
};

const MAIN_WINDOW_LABEL: &str = "main";
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
    allow_exit: AtomicBool,
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
    state.allow_exit.store(true, Ordering::Release);
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
    service_result?;
    media_result?;
    app.exit(0);
    Ok(())
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
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
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
    });
}

#[cfg(target_os = "macos")]
fn restore_main_window(app: &AppHandle) -> Result<(), String> {
    let window = if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        window
    } else {
        let state = app.state::<DesktopState>();
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

pub fn run() {
    let application = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let config_dir = app
                .path()
                .app_config_dir()
                .map_err(|error| format!("resolve desktop config directory: {error}"))?;
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
                allow_exit: AtomicBool::new(false),
            };
            let service = state.service.clone();
            let subscriptions = state.subscriptions.clone();
            let view_resources = state.view_resources.clone();
            let window = app
                .get_webview_window(MAIN_WINDOW_LABEL)
                .ok_or_else(|| "main webview window was not created".to_owned())?;
            observe_window_destruction(&window, service, subscriptions, view_resources, 1);
            app.manage(state);
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
            desktop_release_info,
        ])
        .build(tauri::generate_context!())
        .expect("build RSTorrent desktop application");
    application.run(|handle, event| match event {
        RunEvent::ExitRequested { api, .. }
            if !handle
                .state::<DesktopState>()
                .allow_exit
                .load(Ordering::Acquire) =>
        {
            api.prevent_exit();
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
