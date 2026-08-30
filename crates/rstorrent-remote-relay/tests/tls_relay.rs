use std::sync::Arc;
use std::time::Duration;

use rcgen::{CertifiedKey, generate_simple_self_signed};
use rstorrent_remote_relay::TlsProductRelayServer;
use tempfile::TempDir;
use tokio::time::timeout;
use tokio_rustls::rustls::pki_types::CertificateDer;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use tokio_tungstenite::{Connector, connect_async_tls_with_config};

const ORIGIN: &str = "https://localhost:7443";

#[cfg(unix)]
#[tokio::test]
async fn tls_service_accepts_valid_clients_while_a_handshake_stalls() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = TempDir::new().unwrap();
    let certificate_path = root.path().join("certificate.der");
    let private_key_path = root.path().join("private-key.der");
    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(["localhost".to_owned()]).unwrap();
    let certificate = cert.der().to_vec();
    std::fs::write(&certificate_path, &certificate).unwrap();
    std::fs::write(&private_key_path, signing_key.serialize_der()).unwrap();
    std::fs::set_permissions(&private_key_path, std::fs::Permissions::from_mode(0o600)).unwrap();

    let server = TlsProductRelayServer::bind(
        "127.0.0.1:0".parse().unwrap(),
        root.path().join("state"),
        ORIGIN,
        &certificate_path,
        &private_key_path,
    )
    .await
    .unwrap();
    let address = server.local_addr();
    let relay = server.relay();
    let task = tokio::spawn(async move { server.serve().await.unwrap() });

    let stalled = tokio::net::TcpStream::connect(address).await.unwrap();
    wait_for(|| relay.metrics().pending_tls_handshakes == 1).await;

    let mut roots = RootCertStore::empty();
    roots
        .add(CertificateDer::from(certificate.clone()))
        .unwrap();
    let client = ClientConfig::builder_with_provider(Arc::new(
        tokio_rustls::rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    let request = format!("wss://localhost:{}/host/nobody", address.port())
        .into_client_request()
        .unwrap();
    let (mut websocket, _) = timeout(
        Duration::from_secs(2),
        connect_async_tls_with_config(
            request,
            None,
            false,
            Some(Connector::Rustls(Arc::new(client))),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(relay.metrics().pending_tls_handshakes_high_water >= 2);
    websocket.close(None).await.unwrap();
    relay.shutdown();
    timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(relay.metrics().pending_tls_handshakes, 0);
    drop(stalled);
}

#[cfg(unix)]
#[tokio::test]
async fn tls_service_rejects_weakened_key_mode_and_relative_paths() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = TempDir::new().unwrap();
    let certificate_path = root.path().join("certificate.der");
    let private_key_path = root.path().join("private-key.der");
    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(["localhost".to_owned()]).unwrap();
    std::fs::write(&certificate_path, cert.der()).unwrap();
    std::fs::write(&private_key_path, signing_key.serialize_der()).unwrap();
    std::fs::set_permissions(&private_key_path, std::fs::Permissions::from_mode(0o644)).unwrap();

    assert!(
        TlsProductRelayServer::bind(
            "127.0.0.1:0".parse().unwrap(),
            root.path().join("state"),
            ORIGIN,
            &certificate_path,
            &private_key_path,
        )
        .await
        .is_err()
    );
    assert!(
        TlsProductRelayServer::bind(
            "127.0.0.1:0".parse().unwrap(),
            root.path().join("state"),
            ORIGIN,
            "certificate.der",
            "private-key.der",
        )
        .await
        .is_err()
    );
}

#[cfg(unix)]
#[test]
fn service_binary_reports_readiness_and_drains_sigterm() {
    use std::io::{BufRead as _, BufReader};
    use std::os::unix::fs::PermissionsExt as _;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    let root = TempDir::new().unwrap();
    let certificate_path = root.path().join("certificate.der");
    let private_key_path = root.path().join("private-key.der");
    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(["localhost".to_owned()]).unwrap();
    std::fs::write(&certificate_path, cert.der()).unwrap();
    std::fs::write(&private_key_path, signing_key.serialize_der()).unwrap();
    std::fs::set_permissions(&private_key_path, std::fs::Permissions::from_mode(0o600)).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_rstorrent-remote-relay"))
        .args([
            "--root",
            root.path().join("state").to_str().unwrap(),
            "--listen",
            "127.0.0.1:0",
            "--client-origin",
            ORIGIN,
            "--certificate-der",
            certificate_path.to_str().unwrap(),
            "--private-key-der",
            private_key_path.to_str().unwrap(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut ready = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    let ready: serde_json::Value = serde_json::from_str(&ready).unwrap();
    assert_eq!(ready["event"], "ready");
    let address: std::net::SocketAddr = ready["address"].as_str().unwrap().parse().unwrap();
    assert!(address.ip().is_loopback());
    assert_ne!(address.port(), 0);
    assert!(
        ready["relay_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );

    let pid = rustix::process::Pid::from_raw(child.id() as i32).unwrap();
    rustix::process::kill_process(pid, rustix::process::Signal::TERM).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("relay service did not drain SIGTERM");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

async fn wait_for<F>(mut condition: F)
where
    F: FnMut() -> bool,
{
    timeout(Duration::from_secs(2), async {
        while !condition() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
