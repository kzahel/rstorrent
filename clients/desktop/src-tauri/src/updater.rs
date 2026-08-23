use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

use serde::Serialize;
use uuid::Uuid;

const INSTALLATION_ID_FILE: &str = "cfu-id";
const MAX_INSTALLATION_ID_FILE_BYTES: u64 = 128;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopReleaseInfo {
    version: &'static str,
    build_id: &'static str,
    target: &'static str,
    arch: &'static str,
}

#[tauri::command]
pub(crate) fn desktop_release_info() -> DesktopReleaseInfo {
    DesktopReleaseInfo {
        version: env!("CARGO_PKG_VERSION"),
        build_id: option_env!("RSTORRENT_BUILD_ID").unwrap_or("development"),
        target: env!("TAURI_ENV_TARGET_TRIPLE"),
        arch: std::env::consts::ARCH,
    }
}

fn read_valid_installation_id(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.len() == 0 || metadata.len() > MAX_INSTALLATION_ID_FILE_BYTES {
        return None;
    }
    let value = std::fs::read_to_string(path).ok()?;
    let value = value.trim();
    Uuid::parse_str(value).ok()?;
    Some(value.to_owned())
}

pub(crate) fn get_or_create_installation_id(config_dir: &Path) -> Result<String, String> {
    let path = config_dir.join(INSTALLATION_ID_FILE);
    if let Some(id) = read_valid_installation_id(&path) {
        return Ok(id);
    }

    std::fs::create_dir_all(config_dir)
        .map_err(|error| format!("create desktop config directory: {error}"))?;
    let id = Uuid::new_v4().to_string();
    let temporary = config_dir.join(format!("{INSTALLATION_ID_FILE}.{id}.tmp"));
    let result = (|| {
        let mut file = create_private_file(&temporary)?;
        file.write_all(format!("{id}\n").as_bytes())
            .map_err(|error| format!("write temporary installation ID: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("sync temporary installation ID: {error}"))?;
        drop(file);

        if let Some(existing) = read_valid_installation_id(&path) {
            return Ok(existing);
        }
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|error| format!("remove malformed installation ID: {error}"))?;
        }
        std::fs::rename(&temporary, &path)
            .map_err(|error| format!("publish installation ID: {error}"))?;
        sync_directory(config_dir)?;
        Ok(id.clone())
    })();
    if temporary.exists() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn create_private_file(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|error| format!("create temporary installation ID: {error}"))
}

fn sync_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        File::open(path)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| format!("sync desktop config directory: {error}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{INSTALLATION_ID_FILE, get_or_create_installation_id};

    #[test]
    fn installation_id_is_stable_and_valid() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let first = get_or_create_installation_id(directory.path()).expect("create ID");
        let second = get_or_create_installation_id(directory.path()).expect("reopen ID");
        assert_eq!(first, second);
        assert!(uuid::Uuid::parse_str(&first).is_ok());
    }

    #[test]
    fn malformed_or_oversized_id_is_repaired() {
        for malformed in ["not-a-uuid\n".to_owned(), "x".repeat(256)] {
            let directory = tempfile::tempdir().expect("temporary directory");
            std::fs::write(directory.path().join(INSTALLATION_ID_FILE), malformed)
                .expect("write malformed ID");
            let repaired =
                get_or_create_installation_id(directory.path()).expect("repair installation ID");
            assert!(uuid::Uuid::parse_str(&repaired).is_ok());
        }
    }

    #[cfg(unix)]
    #[test]
    fn installation_id_is_private() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("temporary directory");
        get_or_create_installation_id(directory.path()).expect("create ID");
        let mode = std::fs::metadata(directory.path().join(INSTALLATION_ID_FILE))
            .expect("installation ID metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0);
    }
}
