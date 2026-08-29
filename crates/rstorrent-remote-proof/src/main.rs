#![forbid(unsafe_code)]

use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use rand::RngCore;
use rstorrent_gateway::{GatewayConfig, bind};
use rstorrent_remote_crypto::{
    Binding, HostId, RelayId, ServerAuthority, Username, random_operation_seed,
};
use rstorrent_remote_proof::{ProofHostConfig, ProofHostMetrics, run_proof_host};
use rstorrent_remote_relay::ProofRelayServer;
use rstorrent_session::{
    ApplicationConfig, ApplicationService, ConfiguredStorageRoot, NetworkConfig, NetworkPolicy,
};
use serde_json::json;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args();
    let _program = arguments.next();
    let origin = arguments.next().ok_or("expected one browser origin")?;
    if arguments.next().is_some()
        || !(origin.starts_with("http://127.0.0.1:") || origin.starts_with("http://localhost:"))
    {
        return Err("browser origin must be one exact loopback HTTP origin".into());
    }

    let temporary = tempfile::tempdir()?;
    let payload_root = temporary.path().join("payload");
    std::fs::create_dir_all(&payload_root)?;
    let service = Arc::new(Mutex::new(
        ApplicationService::open(ApplicationConfig::ephemeral(
            "remote-proof".to_owned(),
            vec![ConfiguredStorageRoot::path("downloads", payload_root)],
            NetworkConfig::new(
                NetworkPolicy::LoopbackOnly,
                Duration::from_secs(1),
                Duration::from_secs(1),
            ),
        ))
        .await?,
    ));

    let gateway = bind(
        GatewayConfig::unauthenticated_loopback_development(origin.clone()),
        service.clone(),
    )
    .await?;
    let gateway_address = gateway.local_addr();
    let gateway_shutdown = CancellationToken::new();
    let gateway_task = tokio::spawn(gateway.serve(gateway_shutdown.clone()));

    let relay_id = random_array()?;
    let relay_server = ProofRelayServer::bind_loopback(relay_id).await?;
    let relay_address = relay_server.local_addr();
    let relay = relay_server.relay();
    let relay_task = tokio::spawn(relay_server.serve());

    let username = Username::parse("alice-proof")?;
    let host_id = random_array()?;
    let binding = Binding::new(
        RelayId::new(relay_id),
        username.clone(),
        HostId::new(host_id),
    );
    let host_metrics = ProofHostMetrics::default();
    let host_shutdown = CancellationToken::new();
    let host_task = tokio::spawn(run_proof_host(
        ProofHostConfig {
            relay_host_url: format!("ws://{relay_address}/host"),
            relay_credential: random_secret_array()?,
            binding,
            authority: ServerAuthority::generate(random_operation_seed()?),
            gateway_websocket_url: format!("ws://{gateway_address}/api/v1/connect"),
            gateway_origin: origin,
        },
        host_metrics.clone(),
        host_shutdown.clone(),
    ));

    tokio::time::timeout(Duration::from_secs(2), async {
        while relay.metrics().waiting_hosts != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| "proof host did not claim its relay route")?;

    println!(
        "READY {}",
        json!({
            "directBaseUrl": format!("http://{gateway_address}"),
            "relayUrl": format!("ws://{relay_address}"),
            "relayId": encode_hex(&relay_id),
            "username": username.as_str(),
            "hostId": encode_hex(&host_id),
        })
    );

    wait_for_quit().await?;
    host_shutdown.cancel();
    relay.shutdown();
    gateway_shutdown.cancel();
    host_task.await?;
    relay_task.await??;
    gateway_task.await??;

    let host = host_metrics.snapshot();
    let relay_metrics = relay.metrics();
    service.lock().await.shutdown().await?;
    drop(service);
    temporary.close()?;
    println!(
        "METRICS {}",
        json!({
            "host": {
                "acceptedRouteClaims": host.accepted_route_claims,
                "completedRegistrations": host.completed_registrations,
                "loginAttempts": host.login_attempts,
                "authenticatedLogins": host.authenticated_logins,
                "failedCircuits": host.failed_circuits,
                "activeCircuits": host.active_circuits,
                "activeCircuitsHighWater": host.active_circuits_high_water,
                "clientApplicationFrames": host.client_application_frames,
                "serverApplicationFrames": host.server_application_frames,
                "clientAcknowledgements": host.client_acknowledgements,
                "serverViewBatches": host.server_view_batches,
                "serverCallResults": host.server_call_results,
                "rejectedApplicationBreadth": host.rejected_application_breadth,
            },
            "relay": {
                "registeredRoutes": relay_metrics.registered_routes,
                "registeredRoutesHighWater": relay_metrics.registered_routes_high_water,
                "waitingHosts": relay_metrics.waiting_hosts,
                "waitingHostsHighWater": relay_metrics.waiting_hosts_high_water,
                "activeCircuits": relay_metrics.active_circuits,
                "activeCircuitsHighWater": relay_metrics.active_circuits_high_water,
                "activePumps": relay_metrics.active_pumps,
                "activePumpsHighWater": relay_metrics.active_pumps_high_water,
                "pairedCircuits": relay_metrics.paired_circuits,
                "completedCircuits": relay_metrics.completed_circuits,
                "forwardedClientMessages": relay_metrics.forwarded_client_messages,
                "forwardedClientBytes": relay_metrics.forwarded_client_bytes,
                "forwardedHostMessages": relay_metrics.forwarded_host_messages,
                "forwardedHostBytes": relay_metrics.forwarded_host_bytes,
                "forwardedMessageBytesHighWater": relay_metrics.forwarded_message_bytes_high_water,
            }
        })
    );
    Ok(())
}

fn random_array() -> Result<[u8; 32], Box<dyn Error>> {
    let mut bytes = [0_u8; 32];
    rand::rngs::OsRng.try_fill_bytes(&mut bytes)?;
    Ok(bytes)
}

fn random_secret_array() -> Result<Zeroizing<[u8; 32]>, Box<dyn Error>> {
    let mut bytes = Zeroizing::new([0_u8; 32]);
    rand::rngs::OsRng.try_fill_bytes(&mut *bytes)?;
    Ok(bytes)
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

async fn wait_for_quit() -> Result<(), std::io::Error> {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    loop {
        tokio::select! {
            signal = tokio::signal::ctrl_c() => return signal,
            line = lines.next_line() => {
                match line? {
                    Some(line) if line.trim() != "QUIT" => continue,
                    Some(_) | None => return Ok(()),
                }
            }
        }
    }
}
