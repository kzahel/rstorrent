use std::env;
use std::error::Error;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use rstorrent_gateway::{GatewayConfig, bind};
use rstorrent_session::{ApplicationConfig, ApplicationService, ConfiguredStorageRoot};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let profile_root = required_path("RSTORRENT_PROFILE_ROOT")?;
    let storage_root = required_path("RSTORRENT_STORAGE_ROOT")?;
    let token = required_string("RSTORRENT_GATEWAY_TOKEN")?;
    let origin =
        env::var("RSTORRENT_GATEWAY_ORIGIN").unwrap_or_else(|_| "http://127.0.0.1:5173".to_owned());
    let bind_addr = env::var("RSTORRENT_GATEWAY_BIND")
        .unwrap_or_else(|_| "127.0.0.1:3030".to_owned())
        .parse::<SocketAddr>()?;

    let application = ApplicationService::open(ApplicationConfig::new(
        profile_root,
        "default".to_owned(),
        vec![ConfiguredStorageRoot {
            id: "downloads".to_owned(),
            path: storage_root,
        }],
    ))
    .await?;
    let application = Arc::new(Mutex::new(application));
    let server = bind(
        GatewayConfig {
            bind: bind_addr,
            token,
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
