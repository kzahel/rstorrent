use std::env;
use std::error::Error;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rstorrent_gateway::{GatewayAuthentication, GatewayConfig, bind};
use rstorrent_session::{
    ApplicationConfig, ApplicationService, ConfiguredStorageRoot, NetworkConfig, NetworkPolicy,
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let profile_root = required_path("RSTORRENT_PROFILE_ROOT")?;
    let storage_root = required_path("RSTORRENT_STORAGE_ROOT")?;
    let origin =
        env::var("RSTORRENT_GATEWAY_ORIGIN").unwrap_or_else(|_| "http://127.0.0.1:5173".to_owned());
    let authentication = env::var("RSTORRENT_GATEWAY_AUTH").unwrap_or_else(|_| "bearer".to_owned());
    let (bind_addr, authentication) = match authentication.as_str() {
        "bearer" => (
            env::var("RSTORRENT_GATEWAY_BIND")
                .unwrap_or_else(|_| "127.0.0.1:3030".to_owned())
                .parse::<SocketAddr>()?,
            GatewayAuthentication::Bearer {
                token: required_string("RSTORRENT_GATEWAY_TOKEN")?,
            },
        ),
        "unauthenticated_loopback_development" => {
            if env::var_os("RSTORRENT_GATEWAY_BIND").is_some() {
                return Err(
                    "RSTORRENT_GATEWAY_BIND is not accepted in unauthenticated development mode"
                        .into(),
                );
            }
            (
                SocketAddr::from(([127, 0, 0, 1], 0)),
                GatewayAuthentication::UnauthenticatedLoopbackDevelopment,
            )
        }
        value => {
            return Err(format!(
                "RSTORRENT_GATEWAY_AUTH must be bearer or unauthenticated_loopback_development; got {value}"
            )
            .into());
        }
    };
    let network_policy = match env::var("RSTORRENT_NETWORK_POLICY")
        .unwrap_or_else(|_| "loopback_only".to_owned())
        .as_str()
    {
        "offline" => NetworkPolicy::Offline,
        "loopback_only" => NetworkPolicy::LoopbackOnly,
        "online" => NetworkPolicy::Online,
        value => {
            return Err(format!(
                "RSTORRENT_NETWORK_POLICY must be offline, loopback_only, or online; got {value}"
            )
            .into());
        }
    };

    let application = ApplicationService::open(ApplicationConfig::new(
        profile_root,
        "default".to_owned(),
        vec![ConfiguredStorageRoot::path("downloads", storage_root)],
        NetworkConfig::new(
            network_policy,
            Duration::from_secs(15),
            Duration::from_secs(60),
        ),
    ))
    .await?;
    let application = Arc::new(Mutex::new(application));
    let server = bind(
        GatewayConfig {
            bind: bind_addr,
            authentication,
            allowed_origin: origin,
            max_connections: rstorrent_gateway::MAX_CONNECTIONS,
        },
        application.clone(),
    )
    .await?;
    eprintln!("gateway listening on {}", server.local_addr());
    let shutdown = CancellationToken::new();
    let signal_shutdown = shutdown.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        signal_shutdown.cancel();
    });
    server.serve(shutdown).await?;
    application.lock().await.shutdown().await?;
    Ok(())
}

fn required_string(name: &str) -> Result<String, Box<dyn Error>> {
    let value = env::var(name).map_err(|_| format!("{name} is required"))?;
    if value.is_empty() {
        return Err(format!("{name} must be nonempty").into());
    }
    Ok(value)
}

fn required_path(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    Ok(PathBuf::from(required_string(name)?))
}
