use std::path::PathBuf;
use std::sync::Arc;

use base64::Engine as _;
use rstorrent_gateway::{GatewayAuthentication, GatewayConfig, bind};
use rstorrent_session::ApplicationService;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::error::Result;
use crate::owner::random_array;
use crate::{RemoteAccessOwner, RemoteHostConfig, RemoteHostError};

const INTERNAL_GATEWAY_ORIGIN: &str = "http://127.0.0.1:1";
const INTERNAL_GATEWAY_CONNECTIONS: usize = 2;

/// Product-owned composition that gives the remote host one process-private
/// connection to the incumbent application service.
pub struct RemoteApplicationRuntime {
    owner: Arc<RemoteAccessOwner>,
    #[cfg(feature = "direct-file-webrtc")]
    direct_file_factory: rstorrent_direct_file::DirectFileEndpointFactory,
    gateway_shutdown: CancellationToken,
    gateway_task:
        Mutex<Option<JoinHandle<std::result::Result<(), rstorrent_gateway::GatewayError>>>>,
}

impl RemoteApplicationRuntime {
    pub async fn open_validation(
        authority_root: impl Into<PathBuf>,
        relay_base: &str,
        relay_certificate_der: Vec<u8>,
        host_build: impl Into<String>,
        service: Arc<Mutex<ApplicationService>>,
    ) -> Result<Self> {
        Self::open_with_config(
            authority_root,
            host_build,
            service,
            |gateway, token, build| {
                RemoteHostConfig::validation(
                    relay_base,
                    relay_certificate_der,
                    gateway,
                    INTERNAL_GATEWAY_ORIGIN,
                    token,
                    build,
                )
            },
        )
        .await
    }

    pub async fn open_product(
        authority_root: impl Into<PathBuf>,
        host_build: impl Into<String>,
        service: Arc<Mutex<ApplicationService>>,
    ) -> Result<Self> {
        Self::open_with_config(
            authority_root,
            host_build,
            service,
            |gateway, token, build| {
                RemoteHostConfig::product(gateway, INTERNAL_GATEWAY_ORIGIN, token, build)
            },
        )
        .await
    }

    async fn open_with_config(
        authority_root: impl Into<PathBuf>,
        host_build: impl Into<String>,
        service: Arc<Mutex<ApplicationService>>,
        config: impl FnOnce(String, String, String) -> Result<RemoteHostConfig>,
    ) -> Result<Self> {
        let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random_array::<32>()?);
        #[cfg(feature = "direct-file-webrtc")]
        let direct_file_factory =
            rstorrent_direct_file::DirectFileEndpointFactory::new(service.clone());
        let gateway = bind(
            GatewayConfig {
                bind: "127.0.0.1:0"
                    .parse()
                    .map_err(|_| RemoteHostError::Configuration("internal gateway bind"))?,
                authentication: GatewayAuthentication::Bearer {
                    token: token.clone(),
                },
                allowed_origin: INTERNAL_GATEWAY_ORIGIN.to_owned(),
                max_connections: INTERNAL_GATEWAY_CONNECTIONS,
            },
            service,
        )
        .await
        .map_err(|_| RemoteHostError::Gateway)?;
        let gateway_address = gateway.local_addr();
        let gateway_shutdown = CancellationToken::new();
        let task_shutdown = gateway_shutdown.clone();
        let gateway_task = tokio::spawn(async move { gateway.serve(task_shutdown).await });
        let config = match config(
            format!("ws://{gateway_address}/api/v1/connect"),
            token,
            host_build.into(),
        ) {
            Ok(config) => config,
            Err(error) => {
                gateway_shutdown.cancel();
                let _ = gateway_task.await;
                return Err(error);
            }
        };
        let owner = match RemoteAccessOwner::open(authority_root, config).await {
            Ok(owner) => Arc::new(owner),
            Err(error) => {
                gateway_shutdown.cancel();
                let _ = gateway_task.await;
                return Err(error);
            }
        };
        Ok(Self {
            owner,
            #[cfg(feature = "direct-file-webrtc")]
            direct_file_factory,
            gateway_shutdown,
            gateway_task: Mutex::new(Some(gateway_task)),
        })
    }

    pub fn owner(&self) -> Arc<RemoteAccessOwner> {
        self.owner.clone()
    }

    #[cfg(feature = "direct-file-webrtc")]
    pub fn direct_file_endpoint_factory(&self) -> rstorrent_direct_file::DirectFileEndpointFactory {
        self.direct_file_factory.clone()
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.owner.shutdown().await;
        self.gateway_shutdown.cancel();
        if let Some(task) = self.gateway_task.lock().await.take() {
            task.await
                .map_err(|_| RemoteHostError::Gateway)?
                .map_err(|_| RemoteHostError::Gateway)?;
        }
        Ok(())
    }
}
