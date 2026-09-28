//! Desktop-owned semantic control. No assets, HTTP API or application owner.
use super::*;
use axum::extract::ws::WebSocketUpgrade;
use axum::{
    Router,
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use rstorrent_session::{ApiBackendIdentity, ApplicationService};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;

pub struct DesktopControlServer {
    listener: tokio::net::TcpListener,
    state: GatewayState,
}

impl DesktopControlServer {
    pub async fn bind(
        service: Arc<Mutex<ApplicationService>>,
        credential: String,
        instance_id: String,
        version: String,
    ) -> Result<Self, GatewayError> {
        if credential.len() != 64 || !credential.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(GatewayError::Configuration(
                "invalid desktop credential".to_owned(),
            ));
        }
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(GatewayError::Bind)?;
        let address = listener.local_addr().map_err(GatewayError::Bind)?;
        let origin = format!("http://{address}");
        let state = GatewayState {
            authentication: Arc::new(GatewayAuthentication::Bearer { token: credential }),
            allowed_origin: Arc::from(chromeos_companion::BETA_EXTENSION_ORIGIN),
            allowed_host: Arc::from(address.to_string()),
            media_host: Arc::from(address.to_string()),
            media_origin: Arc::from(origin),
            service,
            connections: Arc::new(Semaphore::new(4)),
            torrent_uploads: Arc::new(Semaphore::new(1)),
            http_owner_namespace: NEXT_HTTP_OWNER.fetch_add(1, Ordering::Relaxed),
            connection_registry: application_websocket::ApplicationConnectionRegistry::new(),
            connection_metrics: ApplicationConnectionMetrics::default(),
            gateway_shutdown: CancellationToken::new(),
            hello_backend: Some(ApiBackendIdentity {
                kind: "desktop".to_owned(),
                instance_id,
                profile_id: "default".to_owned(),
                product_version: version,
                capability_profile: vec![
                    "desktop_control_v1".to_owned(),
                    "native_window_root_acquisition".to_owned(),
                ],
            }),
            companion_platform: None,
            download_directory_picker: Arc::new(UnavailableDownloadDirectoryPicker),
            hosted_assets: None,
            web_auth: None,
        };
        Ok(Self { listener, state })
    }

    pub fn local_addr(&self) -> std::net::SocketAddr {
        self.listener.local_addr().expect("bound desktop listener")
    }
    pub fn metrics(&self) -> ApplicationConnectionMetrics {
        self.state.connection_metrics.clone()
    }

    pub async fn serve(self, shutdown: CancellationToken) -> Result<(), GatewayError> {
        let connections = self.state.connections.clone();
        let gateway_shutdown = self.state.gateway_shutdown.clone();
        let router = Router::new()
            .route("/api/v1/connect", get(upgrade))
            .with_state(self.state);
        let result = axum::serve(
            self.listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            shutdown.cancelled().await;
            gateway_shutdown.cancel();
        })
        .await
        .map_err(GatewayError::Serve);
        // Axum upgrades outlive HTTP requests. Wait for their pumps/writers too.
        let _joined = connections.acquire_many(4).await;
        result
    }
}

async fn upgrade(
    State(state): State<GatewayState>,
    ConnectInfo(peer): ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
    socket: WebSocketUpgrade,
) -> Response {
    if !peer.ip().is_loopback()
        || headers.get(header::HOST).and_then(|v| v.to_str().ok())
            != Some(state.allowed_host.as_ref())
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    application_websocket::upgrade_application_connection(
        State(state),
        ConnectInfo(peer),
        headers,
        socket,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use serde_json::{Value, json};
    use std::time::Duration;
    use tokio_tungstenite::{
        connect_async,
        tungstenite::{Message, client::IntoClientRequest},
    };

    #[tokio::test]
    async fn exact_admission_identity_shared_library_limits_and_joined_shutdown() {
        let root =
            std::env::temp_dir().join(format!("rstorrent-desktop-control-{}", std::process::id()));
        let service = Arc::new(Mutex::new(
            ApplicationService::open(rstorrent_session::ApplicationConfig::new(
                root.join("profile"),
                "default".to_owned(),
                Vec::new(),
                rstorrent_session::NetworkConfig::new(
                    rstorrent_session::NetworkPolicy::LoopbackOnly,
                    Duration::from_secs(1),
                    Duration::from_secs(1),
                ),
            ))
            .await
            .unwrap(),
        ));
        let token = "ab".repeat(32);
        let server = DesktopControlServer::bind(
            service.clone(),
            token.clone(),
            "cd".repeat(16),
            "test".to_owned(),
        )
        .await
        .unwrap();
        let address = server.local_addr();
        assert_eq!(address.ip(), std::net::Ipv4Addr::LOCALHOST);
        let metrics = server.metrics();
        let cancel = CancellationToken::new();
        let task = tokio::spawn(server.serve(cancel.clone()));
        let request = || {
            let mut request = format!("ws://{address}/api/v1/connect")
                .into_client_request()
                .unwrap();
            request.headers_mut().insert(
                "Origin",
                chromeos_companion::BETA_EXTENSION_ORIGIN.parse().unwrap(),
            );
            request
        };
        for (header, value) in [
            ("Origin", "https://example.com"),
            ("Host", "localhost:1234"),
        ] {
            let mut bad = request();
            bad.headers_mut().insert(header, value.parse().unwrap());
            assert!(connect_async(bad).await.is_err());
        }
        let hello = |credential: &str, id: usize| json!({"type":"connect", "api_version":1, "encoding":"json", "client_instance_id":format!("{id:032x}"), "token":credential});
        let (mut bad, _) = connect_async(request()).await.unwrap();
        bad.send(Message::Text(hello("wrong", 1).to_string().into()))
            .await
            .unwrap();
        let frame: Value =
            serde_json::from_str(&bad.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
        assert_eq!(frame["error"]["code"], "authentication_failed");
        drop(bad);
        let mut clients = Vec::new();
        for id in 1..=4 {
            let (mut client, _) = connect_async(request()).await.unwrap();
            client
                .send(Message::Text(hello(&token, id).to_string().into()))
                .await
                .unwrap();
            let frame: Value =
                serde_json::from_str(&client.next().await.unwrap().unwrap().into_text().unwrap())
                    .unwrap();
            assert_eq!(frame["hello"]["backend"]["kind"], "desktop");
            assert_eq!(frame["hello"]["backend"]["profile_id"], "default");
            assert_eq!(frame["hello"]["backend"]["instance_id"], "cd".repeat(16));
            assert!(
                !frame["hello"]["capabilities"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("torrent_media"))
            );
            client.send(Message::Text(json!({"type":"call", "call_id":"library", "operation":{"type":"open_view_set", "request":{"views":[{"type":"torrent_list", "view_id":"library", "delivery":{"kind":"immediate"}}],"options":{}}}}).to_string().into())).await.unwrap();
            let frame: Value =
                serde_json::from_str(&client.next().await.unwrap().unwrap().into_text().unwrap())
                    .unwrap();
            assert_eq!(frame["type"], "result", "{frame}");
            clients.push(client);
        }
        assert!(connect_async(request()).await.is_err());
        cancel.cancel();
        // Drain close frames so cooperative writer shutdown is exercised.
        for mut client in clients {
            while let Some(Ok(frame)) = client.next().await {
                if frame.is_close() {
                    break;
                }
            }
        }
        tokio::time::timeout(Duration::from_secs(6), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(metrics.snapshot().active_connections, 0);
        assert_eq!(metrics.snapshot().active_connections_high_water, 4);
        assert_eq!(metrics.snapshot().rejected_authentication, 1);
        service.lock().await.shutdown().await.unwrap();
        drop(service);
        std::fs::remove_dir_all(root).unwrap();
    }
}
