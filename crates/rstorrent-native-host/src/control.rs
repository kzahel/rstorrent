//! Bounded same-user bootstrap. This owns no application or torrent state.
use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;

pub const BACKGROUND_ARGUMENT: &str = "--extension-background";
pub const SOCKET_NAME: &str = "desktop-control-v1.sock";
pub const BETA_ORIGIN: &str = "chrome-extension://gcgoepclopkgijmclmlheafaglmbjlcc/";
pub const MAX_READY_BYTES: usize = 4096;

// Deliberately no Debug: credential material must never enter logs.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ControlReady {
    pub endpoint: String,
    pub credential: String,
    pub instance_id: String,
    pub profile_id: String,
}

impl ControlReady {
    pub fn validate(&self) -> io::Result<()> {
        let port = self
            .endpoint
            .strip_prefix("http://127.0.0.1:")
            .and_then(|port| port.parse::<u16>().ok())
            .filter(|port| *port != 0);
        if port.is_none()
            || self.credential.len() != 64
            || !self.credential.bytes().all(|b| b.is_ascii_hexdigit())
            || self.instance_id.len() != 32
            || !self.instance_id.bytes().all(|b| b.is_ascii_hexdigit())
            || self.profile_id != "default"
        {
            return Err(io::Error::other("invalid desktop ready response"));
        }
        Ok(())
    }
}

pub fn is_stopped(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
    )
}

#[cfg(unix)]
fn check_private(path: &Path, directory: bool) -> io::Result<()> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.uid() != rustix::process::getuid().as_raw()
        || metadata.mode() & 0o077 != 0
        || if directory {
            !metadata.is_dir()
        } else {
            !metadata.file_type().is_socket()
        }
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unsafe desktop rendezvous",
        ));
    }
    Ok(())
}

#[cfg(unix)]
pub fn read_ready(path: &Path) -> io::Result<ControlReady> {
    use std::io::Read;
    check_private(
        path.parent()
            .ok_or_else(|| io::Error::other("missing parent"))?,
        true,
    )?;
    check_private(path, false)?;
    let mut stream = std::os::unix::net::UnixStream::connect(path)?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(1)))?;
    #[cfg(target_os = "linux")]
    if rustix::net::sockopt::socket_peercred(&stream)?.uid != rustix::process::getuid() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "foreign desktop owner",
        ));
    }
    let mut length = [0; 4];
    stream.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length > MAX_READY_BYTES {
        return Err(io::Error::other("oversized ready response"));
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes)?;
    let ready: ControlReady = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    ready.validate()?;
    Ok(ready)
}

#[cfg(not(unix))]
pub fn read_ready(_path: &Path) -> io::Result<ControlReady> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "desktop bootstrap is not implemented on this platform",
    ))
}

#[cfg(unix)]
pub struct BootstrapServer {
    listener: tokio::net::UnixListener,
    path: std::path::PathBuf,
    ready: ControlReady,
}

#[cfg(unix)]
impl BootstrapServer {
    pub fn bind(directory: &Path, ready: ControlReady) -> io::Result<Self> {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        ready.validate()?;
        if !directory.exists() {
            std::fs::DirBuilder::new().mode(0o700).create(directory)?;
        }
        check_private(directory, true)?;
        let path = directory.join(SOCKET_NAME);
        match std::fs::symlink_metadata(&path) {
            Ok(_) => {
                check_private(&path, false)?;
                match std::os::unix::net::UnixStream::connect(&path) {
                    Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
                        std::fs::remove_file(&path)?
                    }
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::AddrInUse,
                            "bootstrap owner already exists",
                        ));
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let listener = tokio::net::UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Self {
            listener,
            path,
            ready,
        })
    }

    pub async fn serve(self, cancel: tokio_util::sync::CancellationToken) -> io::Result<()> {
        use tokio::io::AsyncWriteExt;
        let bytes = serde_json::to_vec(&self.ready).map_err(io::Error::other)?;
        if bytes.len() > MAX_READY_BYTES {
            return Err(io::Error::other("oversized ready response"));
        }
        loop {
            let (mut stream, _) = tokio::select! {
                biased;
                () = cancel.cancelled() => return Ok(()),
                accepted = self.listener.accept() => accepted?,
            };
            if stream.peer_cred()?.uid() != rustix::process::getuid().as_raw() {
                continue;
            }
            tokio::select! {
                () = cancel.cancelled() => return Ok(()),
                _ = tokio::time::timeout(std::time::Duration::from_secs(1), async {
                    stream.write_all(&(bytes.len() as u32).to_le_bytes()).await?;
                    stream.write_all(&bytes).await
                }) => {},
            }
        }
    }
}

#[cfg(unix)]
impl Drop for BootstrapServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tokio_util::sync::CancellationToken;

    fn ready() -> ControlReady {
        ControlReady {
            endpoint: "http://127.0.0.1:12345".to_owned(),
            credential: "ab".repeat(32),
            instance_id: "cd".repeat(16),
            profile_id: "default".to_owned(),
        }
    }

    #[tokio::test]
    async fn protected_bootstrap_joins_and_removes_only_owned_socket() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("host");
        let server = BootstrapServer::bind(&directory, ready()).unwrap();
        assert!(BootstrapServer::bind(&directory, ready()).is_err());
        let cancel = CancellationToken::new();
        let task = tokio::spawn(server.serve(cancel.clone()));
        let path = directory.join(SOCKET_NAME);
        let read_path = path.clone();
        let actual = tokio::task::spawn_blocking(move || read_ready(&read_path))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(actual.credential, ready().credential);
        cancel.cancel();
        task.await.unwrap().unwrap();
        assert!(!path.exists());
        assert!(is_stopped(&read_ready(&path).err().unwrap()));
    }

    #[tokio::test]
    async fn rejects_insecure_directory_symlink_and_non_socket() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("host");
        std::fs::create_dir(&directory).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(BootstrapServer::bind(&directory, ready()).is_err());
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.join(SOCKET_NAME);
        std::fs::write(&path, b"preserve").unwrap();
        assert!(BootstrapServer::bind(&directory, ready()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"preserve");
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&directory, &link).unwrap();
        assert!(BootstrapServer::bind(&link, ready()).is_err());
    }

    #[tokio::test]
    async fn reclaims_refused_socket_but_never_a_live_owner() {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = root.path().join(SOCKET_NAME);
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(BootstrapServer::bind(root.path(), ready()).is_err());
        drop(listener);
        let server = BootstrapServer::bind(root.path(), ready()).unwrap();
        drop(server);
        assert!(!path.exists());
    }

    #[test]
    fn refuses_foreign_endpoint_and_invalid_runtime_identity() {
        for endpoint in [
            "http://localhost:1234",
            "http://127.0.0.1:0",
            "http://127.0.0.1:65536",
            "http://127.0.0.1:1234/",
            "http://192.0.2.1:1234",
        ] {
            let mut value = ready();
            value.endpoint = endpoint.to_owned();
            assert!(value.validate().is_err());
        }
    }
}
