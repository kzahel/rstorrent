#![forbid(unsafe_code)]

use std::env;
use std::ffi::OsString;
use std::fmt;
use std::io::Write as _;
use std::net::SocketAddr;
use std::path::PathBuf;

use base64::Engine as _;
use rstorrent_remote_relay::TlsProductRelayServer;
use serde::Serialize;

const USAGE: &str = "Usage: rstorrent-remote-relay \\
  --root ABSOLUTE_PATH \\
  --listen LOOPBACK_IP:PORT \\
  --client-origin HTTPS_ORIGIN \\
  --certificate-der ABSOLUTE_PATH \\
  --private-key-der ABSOLUTE_PATH";

#[derive(Debug, Eq, PartialEq)]
struct Arguments {
    root: PathBuf,
    listen: SocketAddr,
    client_origin: String,
    certificate_der: PathBuf,
    private_key_der: PathBuf,
}

impl Arguments {
    fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Self, ArgumentError> {
        let mut root = None;
        let mut listen = None;
        let mut client_origin = None;
        let mut certificate_der = None;
        let mut private_key_der = None;
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            let value = arguments.next().ok_or(ArgumentError::Usage)?;
            match argument.to_str() {
                Some("--root") if root.is_none() => root = Some(PathBuf::from(value)),
                Some("--listen") if listen.is_none() => {
                    listen = Some(
                        value
                            .to_str()
                            .ok_or(ArgumentError::Usage)?
                            .parse()
                            .map_err(|_| ArgumentError::Usage)?,
                    );
                }
                Some("--client-origin") if client_origin.is_none() => {
                    client_origin = Some(value.into_string().map_err(|_| ArgumentError::Usage)?);
                }
                Some("--certificate-der") if certificate_der.is_none() => {
                    certificate_der = Some(PathBuf::from(value));
                }
                Some("--private-key-der") if private_key_der.is_none() => {
                    private_key_der = Some(PathBuf::from(value));
                }
                _ => return Err(ArgumentError::Usage),
            }
        }
        let arguments = Self {
            root: root.ok_or(ArgumentError::Usage)?,
            listen: listen.ok_or(ArgumentError::Usage)?,
            client_origin: client_origin.ok_or(ArgumentError::Usage)?,
            certificate_der: certificate_der.ok_or(ArgumentError::Usage)?,
            private_key_der: private_key_der.ok_or(ArgumentError::Usage)?,
        };
        if !arguments.root.is_absolute()
            || !arguments.certificate_der.is_absolute()
            || !arguments.private_key_der.is_absolute()
            || !arguments.listen.ip().is_loopback()
        {
            return Err(ArgumentError::Usage);
        }
        Ok(arguments)
    }
}

#[derive(Debug)]
enum ArgumentError {
    Usage,
}

impl fmt::Display for ArgumentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(USAGE)
    }
}

impl std::error::Error for ArgumentError {}

#[derive(Serialize)]
struct Ready<'a> {
    event: &'a str,
    address: String,
    relay_id: String,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = Arguments::parse(env::args_os().skip(1))?;
    let server = TlsProductRelayServer::bind(
        arguments.listen,
        arguments.root,
        arguments.client_origin,
        arguments.certificate_der,
        arguments.private_key_der,
    )
    .await?;
    let relay = server.relay();
    let shutdown = shutdown_signal()?;
    tokio::pin!(shutdown);
    println!(
        "{}",
        serde_json::to_string(&Ready {
            event: "ready",
            address: server.local_addr().to_string(),
            relay_id: base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(relay.deployment_id()),
        })?
    );
    std::io::stdout().flush()?;
    let serving = server.serve();
    tokio::pin!(serving);
    tokio::select! {
        result = &mut serving => result?,
        () = &mut shutdown => {
            relay.shutdown();
            serving.await?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn shutdown_signal() -> Result<impl std::future::Future<Output = ()>, std::io::Error> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut terminate = signal(SignalKind::terminate())?;
    Ok(async move {
        tokio::select! {
            result = tokio::signal::ctrl_c() => { let _ = result; }
            _ = terminate.recv() => {}
        }
    })
}

#[cfg(not(unix))]
fn shutdown_signal() -> Result<impl std::future::Future<Output = ()>, std::io::Error> {
    Ok(async {
        let _ = tokio::signal::ctrl_c().await;
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_require_complete_absolute_loopback_configuration() {
        let root = env::temp_dir().join("relay-root");
        let certificate = env::temp_dir().join("relay-cert.der");
        let key = env::temp_dir().join("relay-key.der");
        let parsed = Arguments::parse([
            OsString::from("--root"),
            root.clone().into_os_string(),
            OsString::from("--listen"),
            OsString::from("127.0.0.1:8443"),
            OsString::from("--client-origin"),
            OsString::from("https://127.0.0.1:7443"),
            OsString::from("--certificate-der"),
            certificate.clone().into_os_string(),
            OsString::from("--private-key-der"),
            key.clone().into_os_string(),
        ])
        .unwrap();
        assert_eq!(parsed.root, root);
        assert_eq!(parsed.certificate_der, certificate);
        assert_eq!(parsed.private_key_der, key);
        assert!(Arguments::parse(Vec::<OsString>::new()).is_err());
        assert!(
            Arguments::parse([
                OsString::from("--root"),
                OsString::from("relative"),
                OsString::from("--listen"),
                OsString::from("0.0.0.0:8443"),
                OsString::from("--client-origin"),
                OsString::from("https://127.0.0.1:7443"),
                OsString::from("--certificate-der"),
                certificate.into_os_string(),
                OsString::from("--private-key-der"),
                key.into_os_string(),
            ])
            .is_err()
        );
    }
}
