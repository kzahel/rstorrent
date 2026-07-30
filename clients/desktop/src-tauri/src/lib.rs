#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rstorrent_session::{
    ApplicationConfig, ApplicationService, ConfiguredStorageRoot, RequestEnvelope,
    ResponseEnvelope, SubscriptionSpec, ViewSubscription, ViewUpdate,
};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, RunEvent, State, WebviewWindow, WindowEvent};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

struct DesktopState {
    service: Arc<Mutex<ApplicationService>>,
    subscriptions: Arc<Mutex<BTreeMap<(String, String), DesktopSubscription>>>,
    allow_exit: AtomicBool,
}

struct DesktopSubscription {
    subscription: ViewSubscription,
    cancellation: CancellationToken,
    task: tauri::async_runtime::JoinHandle<()>,
}

#[tauri::command]
async fn application_dispatch(
    state: State<'_, DesktopState>,
    request: RequestEnvelope,
) -> Result<ResponseEnvelope, String> {
    state
        .service
        .lock()
        .await
        .dispatch(request)
        .await
        .map_err(|error| error.to_string())
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
    let replaced = state.subscriptions.lock().await.insert(
        key,
        DesktopSubscription {
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
) {
    let removed = {
        let mut subscriptions = subscriptions.lock().await;
        let keys = subscriptions
            .keys()
            .filter(|(window, _)| window == &label)
            .cloned()
            .collect::<Vec<_>>();
        keys.into_iter()
            .filter_map(|key| subscriptions.remove(&key))
            .collect::<Vec<_>>()
    };
    for subscription in removed {
        stop_subscription(subscription).await;
    }
}

pub fn run() {
    let application = tauri::Builder::default()
        .setup(|app| {
            let app_data = app
                .path()
                .app_data_dir()
                .map_err(|error| format!("resolve application data directory: {error}"))?;
            let service =
                tauri::async_runtime::block_on(ApplicationService::open(ApplicationConfig::new(
                    app_data.join("profile"),
                    "default".to_owned(),
                    vec![ConfiguredStorageRoot::path(
                        "downloads",
                        app_data.join("downloads"),
                    )],
                )))
                .map_err(|error| error.to_string())?;
            let state = DesktopState {
                service: Arc::new(Mutex::new(service)),
                subscriptions: Arc::new(Mutex::new(BTreeMap::new())),
                allow_exit: AtomicBool::new(false),
            };
            let subscriptions = state.subscriptions.clone();
            let window = app
                .get_webview_window("main")
                .ok_or_else(|| "main webview window was not created".to_owned())?;
            let label = window.label().to_owned();
            window.on_window_event(move |event| {
                if matches!(event, WindowEvent::Destroyed) {
                    let subscriptions = subscriptions.clone();
                    let label = label.clone();
                    tauri::async_runtime::spawn(async move {
                        close_window_subscriptions(subscriptions, label).await;
                    });
                }
            });
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            application_dispatch,
            application_subscribe,
            application_resync,
            application_unsubscribe,
            application_shutdown,
        ])
        .build(tauri::generate_context!())
        .expect("build RSTorrent desktop application");
    application.run(|handle, event| {
        if let RunEvent::ExitRequested { api, .. } = event
            && !handle
                .state::<DesktopState>()
                .allow_exit
                .load(Ordering::Acquire)
        {
            api.prevent_exit();
        }
    });
}
