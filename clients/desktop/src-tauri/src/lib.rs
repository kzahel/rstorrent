#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use rstorrent_session::{
    ApplicationConfig, ApplicationService, ConfiguredStorageRoot, NetworkConfig, NetworkPolicy,
    RequestEnvelope, ResponseEnvelope, SubscriptionSpec, ViewSubscription, ViewUpdate,
    application_error_response,
};
#[cfg(target_os = "macos")]
use tauri::WebviewWindowBuilder;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, RunEvent, State, WebviewWindow, WindowEvent};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

mod view_delivery;

use view_delivery::{
    DesktopViewResources, application_view_close, application_view_hello, application_view_open,
    application_view_stream, application_view_stream_ack, application_view_stream_close,
    application_view_update,
};

const MAIN_WINDOW_LABEL: &str = "main";
const PEER_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const PEER_IO_TIMEOUT: Duration = Duration::from_secs(60);

struct DesktopState {
    service: Arc<Mutex<ApplicationService>>,
    subscriptions: Arc<Mutex<BTreeMap<(String, String), DesktopSubscription>>>,
    view_resources: Arc<DesktopViewResources>,
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
    state
        .service
        .lock()
        .await
        .shutdown()
        .await
        .map_err(|error| error.to_string())?;
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
        .setup(|app| {
            let app_data = app
                .path()
                .app_data_dir()
                .map_err(|error| format!("resolve application data directory: {error}"))?;
            let service = tauri::async_runtime::block_on(ApplicationService::open(
                desktop_application_config(&app_data),
            ))
            .map_err(|error| error.to_string())?;
            let state = DesktopState {
                service: Arc::new(Mutex::new(service)),
                subscriptions: Arc::new(Mutex::new(BTreeMap::new())),
                view_resources: Arc::new(DesktopViewResources::new()),
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
        ])
        .build(tauri::generate_context!())
        .expect("build RSTorrent desktop application");
    application.run(|handle, event| match event {
        RunEvent::ExitRequested { api, .. } => {
            if !handle
                .state::<DesktopState>()
                .allow_exit
                .load(Ordering::Acquire)
            {
                api.prevent_exit();
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
        vec![ConfiguredStorageRoot::path(
            "downloads",
            app_data.join("downloads"),
        )],
        NetworkConfig::new(NetworkPolicy::Online, PEER_CONNECT_TIMEOUT, PEER_IO_TIMEOUT),
    )
}

#[cfg(test)]
mod tests {
    use rstorrent_session::DownloadResourceLimits;

    use super::{NetworkPolicy, desktop_application_config};

    #[test]
    fn desktop_product_explicitly_uses_online_networking() {
        let config = desktop_application_config(std::path::Path::new("/tmp/rstorrent-desktop"));
        assert_eq!(config.network.policy, NetworkPolicy::Online);
        assert_eq!(
            config.download_resource_limits,
            DownloadResourceLimits::DESKTOP
        );
    }
}
