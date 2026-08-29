use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rstorrent_remote_host::RemoteAccessOwner;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio_util::sync::CancellationToken;
use zeroize::{Zeroize, Zeroizing};

use crate::runtime::HeadlessError;

const ADMIN_SOCKET_NAME: &str = "remote-admin-v1.sock";
const MAX_ADMIN_MESSAGE_BYTES: usize = 8 * 1024;
const MAX_ADMIN_RESPONSE_BYTES: usize = 1024 * 1024;
const ADMIN_DEADLINE: Duration = Duration::from_secs(10);

#[derive(Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum RemoteAdminRequest {
    Status,
    Enable {
        username: String,
        passphrase: String,
    },
    Rename {
        client_id: String,
        label: String,
    },
    Revoke {
        client_id: String,
    },
    RevokeAllOther {
        retained_client_id: String,
    },
    CloseCircuit {
        circuit_id: String,
    },
    RequirePassword,
    ChangePassphrase {
        passphrase: String,
    },
    Disable,
    Recover {
        username: String,
        passphrase: String,
    },
    ClearHistory,
}

impl Drop for RemoteAdminRequest {
    fn drop(&mut self) {
        match self {
            Self::Enable { passphrase, .. }
            | Self::ChangePassphrase { passphrase }
            | Self::Recover { passphrase, .. } => passphrase.zeroize(),
            _ => {}
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoteAdminResponse {
    ok: bool,
    result: Option<Value>,
    error: Option<String>,
}

pub struct RemoteAdminServer {
    listener: UnixListener,
    path: PathBuf,
    owner: Arc<RemoteAccessOwner>,
}

impl Drop for RemoteAdminServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl RemoteAdminServer {
    pub fn bind(profile_root: &Path, owner: Arc<RemoteAccessOwner>) -> Result<Self, HeadlessError> {
        use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _, PermissionsExt as _};

        let path = admin_socket_path(profile_root);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if !metadata.file_type().is_socket()
                    || metadata.uid() != rustix::process::getuid().as_raw()
                {
                    return Err(HeadlessError::configuration(
                        "remote administration path is not an owner socket",
                    ));
                }
                if std::os::unix::net::UnixStream::connect(&path).is_ok() {
                    return Err(HeadlessError::configuration(
                        "remote administration owner is already running",
                    ));
                }
                std::fs::remove_file(&path).map_err(|error| {
                    HeadlessError::configuration(format!(
                        "remove stale remote administration socket: {error}"
                    ))
                })?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(HeadlessError::configuration(format!(
                    "inspect remote administration socket: {error}"
                )));
            }
        }
        let listener = UnixListener::bind(&path).map_err(|error| {
            HeadlessError::configuration(format!("bind remote administration socket: {error}"))
        })?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(
            |error| {
                HeadlessError::configuration(format!(
                    "protect remote administration socket: {error}"
                ))
            },
        )?;
        Ok(Self {
            listener,
            path,
            owner,
        })
    }

    pub async fn serve(self, shutdown: CancellationToken) -> Result<(), HeadlessError> {
        loop {
            let accepted = tokio::select! {
                () = shutdown.cancelled() => break,
                accepted = self.listener.accept() => accepted,
            };
            let (stream, _) = accepted.map_err(|error| {
                HeadlessError::runtime(format!("accept remote administration request: {error}"))
            })?;
            let peer = stream.peer_cred().map_err(|error| {
                HeadlessError::runtime(format!("inspect remote administration peer: {error}"))
            })?;
            if peer.uid() != rustix::process::getuid().as_raw() {
                continue;
            }
            let _ = tokio::time::timeout(ADMIN_DEADLINE, handle(stream, &self.owner)).await;
        }
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(HeadlessError::runtime(format!(
                "remove remote administration socket: {error}"
            ))),
        }
    }
}

pub fn admin_socket_path(profile_root: &Path) -> PathBuf {
    profile_root.join(ADMIN_SOCKET_NAME)
}

pub async fn request(
    profile_root: &Path,
    request: RemoteAdminRequest,
) -> Result<Value, HeadlessError> {
    let mut stream = UnixStream::connect(admin_socket_path(profile_root))
        .await
        .map_err(|error| {
            HeadlessError::runtime(format!("connect remote administration owner: {error}"))
        })?;
    let encoded = serde_json::to_vec(&request)
        .map_err(|_| HeadlessError::configuration("serialize remote administration request"))?;
    if encoded.len() > MAX_ADMIN_MESSAGE_BYTES {
        return Err(HeadlessError::configuration(
            "remote administration request exceeds size limit",
        ));
    }
    stream.write_all(&encoded).await.map_err(admin_io)?;
    stream.shutdown().await.map_err(admin_io)?;
    let mut response = Vec::new();
    stream
        .take((MAX_ADMIN_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut response)
        .await
        .map_err(admin_io)?;
    if response.len() > MAX_ADMIN_RESPONSE_BYTES {
        return Err(HeadlessError::runtime(
            "remote administration response exceeds size limit",
        ));
    }
    let response: RemoteAdminResponse = serde_json::from_slice(&response)
        .map_err(|_| HeadlessError::runtime("malformed remote administration response"))?;
    match (response.ok, response.result, response.error) {
        (true, Some(result), None) => Ok(result),
        (false, None, Some(error)) => Err(HeadlessError::runtime(error)),
        _ => Err(HeadlessError::runtime(
            "invalid remote administration response",
        )),
    }
}

async fn handle(mut stream: UnixStream, owner: &RemoteAccessOwner) -> Result<(), HeadlessError> {
    let mut encoded = Vec::new();
    (&mut stream)
        .take((MAX_ADMIN_MESSAGE_BYTES + 1) as u64)
        .read_to_end(&mut encoded)
        .await
        .map_err(admin_io)?;
    let response = if encoded.len() > MAX_ADMIN_MESSAGE_BYTES {
        failure("remote administration request exceeds size limit")
    } else {
        match serde_json::from_slice::<RemoteAdminRequest>(&encoded) {
            Ok(request) => execute(owner, request)
                .await
                .unwrap_or_else(|error| failure(&error.to_string())),
            Err(_) => failure("invalid remote administration request"),
        }
    };
    let mut response = serde_json::to_vec(&response)
        .map_err(|_| HeadlessError::runtime("serialize remote administration response"))?;
    if response.len() > MAX_ADMIN_RESPONSE_BYTES {
        response = serde_json::to_vec(&failure(
            "remote administration response exceeds size limit",
        ))
        .map_err(|_| HeadlessError::runtime("serialize remote administration response"))?;
    }
    stream.write_all(&response).await.map_err(admin_io)
}

async fn execute(
    owner: &RemoteAccessOwner,
    mut request: RemoteAdminRequest,
) -> Result<RemoteAdminResponse, rstorrent_remote_host::RemoteHostError> {
    let result = match &mut request {
        RemoteAdminRequest::Status => serde_json::to_value(owner.security_view().await?),
        RemoteAdminRequest::Enable {
            username,
            passphrase,
        } => {
            let passphrase = Zeroizing::new(std::mem::take(passphrase));
            serde_json::to_value(owner.enable(username, passphrase.as_bytes()).await?)
        }
        RemoteAdminRequest::Rename { client_id, label } => {
            owner.rename(client_id, label).await?;
            Ok(Value::Null)
        }
        RemoteAdminRequest::Revoke { client_id } => {
            owner.revoke(client_id).await?;
            Ok(Value::Null)
        }
        RemoteAdminRequest::RevokeAllOther { retained_client_id } => {
            serde_json::to_value(owner.revoke_all_other(retained_client_id).await?)
        }
        RemoteAdminRequest::CloseCircuit { circuit_id } => {
            owner.close_circuit(circuit_id).await?;
            Ok(Value::Null)
        }
        RemoteAdminRequest::RequirePassword => {
            serde_json::to_value(owner.require_password_everywhere().await?)
        }
        RemoteAdminRequest::ChangePassphrase { passphrase } => {
            let passphrase = Zeroizing::new(std::mem::take(passphrase));
            serde_json::to_value(owner.change_passphrase(passphrase.as_bytes()).await?)
        }
        RemoteAdminRequest::Disable => serde_json::to_value(owner.disable().await?),
        RemoteAdminRequest::Recover {
            username,
            passphrase,
        } => {
            let passphrase = Zeroizing::new(std::mem::take(passphrase));
            serde_json::to_value(owner.recover(username, passphrase.as_bytes()).await?)
        }
        RemoteAdminRequest::ClearHistory => serde_json::to_value(owner.clear_history().await?),
    }
    .map_err(|_| rstorrent_remote_host::RemoteHostError::Protocol)?;
    Ok(RemoteAdminResponse {
        ok: true,
        result: Some(result),
        error: None,
    })
}

fn failure(message: &str) -> RemoteAdminResponse {
    RemoteAdminResponse {
        ok: false,
        result: None,
        error: Some(message.chars().take(512).collect()),
    }
}

fn admin_io(error: std::io::Error) -> HeadlessError {
    HeadlessError::runtime(format!("remote administration IO: {error}"))
}
