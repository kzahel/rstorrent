use std::env;
use std::error::Error;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rstorrent_gateway::{
    GatewayAuthentication, GatewayConfig, HostedAssets, MAX_BASIC_PASSWORD_BYTES, bind, bind_hosted,
};
use rstorrent_session::{
    ApplicationConfig, ApplicationService, ConfiguredStorageRoot, DownloadResourceLimits,
    NetworkConfig, NetworkPolicy,
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let profile_root = required_path("RSTORRENT_PROFILE_ROOT")?;
    let storage_roots = optional_path("RSTORRENT_STORAGE_ROOT")?
        .map(|path| vec![ConfiguredStorageRoot::path("downloads", path)])
        .unwrap_or_default();
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
        "basic" => (
            required_string("RSTORRENT_GATEWAY_BIND")?.parse::<SocketAddr>()?,
            GatewayAuthentication::basic(
                &required_string("RSTORRENT_GATEWAY_BASIC_USERNAME")?,
                &required_password_file("RSTORRENT_GATEWAY_BASIC_PASSWORD_FILE")?,
            )?,
        ),
        value => {
            return Err(format!(
                "RSTORRENT_GATEWAY_AUTH must be bearer, basic, or unauthenticated_loopback_development; got {value}"
            )
            .into());
        }
    };
    let test_view_set_lease = env::var("RSTORRENT_TEST_VIEW_SET_LEASE_MILLIS")
        .ok()
        .map(|value| value.parse::<u64>())
        .transpose()?;
    let test_storage_write_delay = env::var("RSTORRENT_TEST_STORAGE_WRITE_DELAY_MILLIS")
        .ok()
        .map(|value| value.parse::<u64>())
        .transpose()?;
    let test_buffered_payload_bytes = env::var("RSTORRENT_TEST_BUFFERED_PAYLOAD_BYTES")
        .ok()
        .map(|value| value.parse::<usize>())
        .transpose()?;
    if (test_view_set_lease.is_some()
        || test_storage_write_delay.is_some()
        || test_buffered_payload_bytes.is_some())
        && !matches!(
            authentication,
            GatewayAuthentication::UnauthenticatedLoopbackDevelopment
        )
    {
        return Err(
            "RSTORRENT_TEST_* controls are accepted only in unauthenticated development mode"
                .into(),
        );
    }
    if test_view_set_lease.is_some_and(|millis| !(250..=60_000).contains(&millis)) {
        return Err("test view-set lease must be within 250..=60000 milliseconds".into());
    }
    if test_storage_write_delay.is_some_and(|millis| millis > 10_000) {
        return Err("test storage write delay cannot exceed 10000 milliseconds".into());
    }
    if test_buffered_payload_bytes
        .is_some_and(|bytes| !(16 * 1024..=32 * 1024 * 1024).contains(&bytes))
    {
        return Err("test buffered payload limit must be within 16384..=33554432 bytes".into());
    }
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

    let mut application_config = ApplicationConfig::new(
        profile_root,
        "default".to_owned(),
        storage_roots,
        NetworkConfig::new(
            network_policy,
            Duration::from_secs(15),
            Duration::from_secs(60),
        ),
    );
    if let Some(lease_millis) = test_view_set_lease {
        application_config.view_set_lease = Duration::from_millis(lease_millis);
        application_config.view_set_reaper_interval =
            Duration::from_millis((lease_millis / 5).clamp(10, 100));
    }
    if let Some(delay_millis) = test_storage_write_delay {
        application_config.storage_write_delay_for_testing = Duration::from_millis(delay_millis);
    }
    if let Some(buffered_bytes) = test_buffered_payload_bytes {
        application_config.download_resource_limits = DownloadResourceLimits::new(
            buffered_bytes.saturating_mul(8),
            buffered_bytes,
            buffered_bytes.saturating_mul(8),
        );
    }
    let application = ApplicationService::open(application_config).await?;
    let application = Arc::new(Mutex::new(application));
    let config = GatewayConfig {
        bind: bind_addr,
        authentication,
        allowed_origin: origin,
        max_connections: rstorrent_gateway::MAX_CONNECTIONS,
    };
    let web_root = optional_path("RSTORRENT_WEB_ROOT")?;
    let build_id = env::var("RSTORRENT_BUILD_ID").ok();
    let server = match (web_root, build_id) {
        (Some(web_root), Some(build_id)) => {
            bind_hosted(
                config,
                application.clone(),
                HostedAssets::new(web_root, build_id)?,
            )
            .await?
        }
        (None, None) => bind(config, application.clone()).await?,
        _ => {
            return Err(
                "RSTORRENT_WEB_ROOT and RSTORRENT_BUILD_ID must be configured together".into(),
            );
        }
    };
    eprintln!("gateway listening on {}", server.local_addr());
    let connection_metrics = server.connection_metrics();
    let shutdown = CancellationToken::new();
    let signal_shutdown = shutdown.clone();
    tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        signal_shutdown.cancel();
    });
    server.serve(shutdown).await?;
    eprintln!(
        "gateway_connection_metrics {}",
        serde_json::to_string(&connection_metrics.snapshot())?
    );
    let mut application = application.lock().await;
    let incoming_before_shutdown = application.incoming_peer_snapshot();
    let storage_before_shutdown = application.storage_file_pool_snapshot();
    application.shutdown().await?;
    let storage_after_shutdown = application.storage_file_pool_snapshot();
    let incoming_metrics = incoming_before_shutdown.map_or(serde_json::Value::Null, |snapshot| {
        serde_json::json!({
            "listen": snapshot.listen_address.to_string(),
            "registrations_before_shutdown": snapshot.registrations,
            "pending_before_shutdown": snapshot.pending,
            "established_before_shutdown": snapshot.established,
            "reads_before_shutdown": snapshot.reads,
            "configured_connection_limit": snapshot.peer_budget.configured_limit,
            "effective_connection_limit": snapshot.peer_budget.effective_limit,
            "incoming_connection_slack": snapshot.peer_budget.incoming_slack,
            "pending_high_water": snapshot.pending_high_water,
            "established_high_water": snapshot.established_high_water,
            "connection_high_water": snapshot.peer_budget.total_high_water,
            "upload_regular_high_water": snapshot.upload_regular_high_water,
            "upload_optimistic_high_water": snapshot.upload_optimistic_high_water,
            "upload_slots_high_water": snapshot.upload_slots_high_water,
            "queued_requests_high_water": snapshot.queued_requests_high_water,
            "queued_bytes_high_water": snapshot.queued_bytes_high_water,
            "read_high_water": snapshot.read_high_water,
            "read_bytes_high_water": snapshot.read_bytes_high_water,
            "writer_send_buffer_high_water": snapshot.writer_send_buffer_high_water,
            "payload_bytes_sent": snapshot.payload_bytes_sent,
        })
    });
    eprintln!(
        "gateway_application_metrics {}",
        serde_json::to_string(&serde_json::json!({
            "incoming": incoming_metrics,
            "incoming_owner_after_shutdown": application.incoming_peer_snapshot().is_some(),
            "storage_limit": storage_before_shutdown.limit,
            "storage_owned_before_shutdown": storage_before_shutdown.current_owned,
            "storage_owned_high_water": storage_before_shutdown.owned_high_water,
            "storage_cached_before_shutdown": storage_before_shutdown.cached_entries,
            "platform_pending_high_water": storage_before_shutdown.platform_pending_high_water,
            "storage_owned_after_shutdown": storage_after_shutdown.current_owned,
            "storage_cached_after_shutdown": storage_after_shutdown.cached_entries,
            "platform_pending_after_shutdown": storage_after_shutdown.platform_pending,
        }))?
    );
    Ok(())
}

async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install terminate signal handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
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

fn optional_path(name: &str) -> Result<Option<PathBuf>, Box<dyn Error>> {
    match env::var_os(name) {
        None => Ok(None),
        Some(value) if value.is_empty() => Err(format!("{name} must be nonempty").into()),
        Some(value) => Ok(Some(PathBuf::from(value))),
    }
}

fn required_password_file(name: &str) -> Result<String, Box<dyn Error>> {
    let path = required_path(name)?;
    let bytes = std::fs::read(&path)?;
    if bytes.len() > MAX_BASIC_PASSWORD_BYTES + 2 {
        return Err(format!("{name} exceeds its configured password bound").into());
    }
    let mut password = String::from_utf8(bytes)?;
    if password.ends_with("\r\n") {
        password.truncate(password.len() - 2);
    } else if password.ends_with('\n') {
        password.pop();
    }
    if password.contains(['\r', '\n']) {
        return Err(format!("{name} must contain one password line").into());
    }
    if password.is_empty() || password.len() > MAX_BASIC_PASSWORD_BYTES {
        return Err(
            format!("{name} must contain 1..={MAX_BASIC_PASSWORD_BYTES} password bytes").into(),
        );
    }
    Ok(password)
}
