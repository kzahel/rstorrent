use std::path::Path;

use rstorrent_session::ProductStateOwner;
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

fn read_valid_legacy_installation_id(path: &Path) -> Option<String> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file() {
        return None;
    }
    if metadata.len() == 0 || metadata.len() > MAX_INSTALLATION_ID_FILE_BYTES {
        return None;
    }
    let value = std::fs::read_to_string(path).ok()?;
    let value = value.trim();
    let parsed = Uuid::parse_str(value).ok()?;
    if parsed.to_string() != value {
        return None;
    }
    Some(value.to_owned())
}

pub(crate) fn open_desktop_product_state(
    config_dir: &Path,
    current_version: &str,
) -> Result<ProductStateOwner, String> {
    let path = config_dir.join(INSTALLATION_ID_FILE);
    let legacy_installation_id = read_valid_legacy_installation_id(&path);
    let product_state = ProductStateOwner::open(
        config_dir,
        current_version,
        legacy_installation_id.as_deref(),
    )
    .map_err(|error| format!("open desktop product state: {error}"))?;
    if std::fs::symlink_metadata(&path).is_ok() {
        std::fs::remove_file(&path)
            .map_err(|error| format!("remove adopted legacy installation ID: {error}"))?;
        sync_directory(config_dir)?;
    }
    Ok(product_state)
}

fn sync_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        std::fs::File::open(path)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| format!("sync desktop config directory: {error}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{INSTALLATION_ID_FILE, open_desktop_product_state};

    #[test]
    fn fresh_product_state_is_stable_but_not_sent_before_disclosure() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let first = open_desktop_product_state(directory.path(), "1").expect("create state");
        let first_id = first.summary().expect("first summary").installation_id;
        assert!(uuid::Uuid::parse_str(&first_id).is_ok());
        assert_eq!(first.updater_installation_id().unwrap(), None);
        drop(first);
        let second = open_desktop_product_state(directory.path(), "2").expect("reopen state");
        assert_eq!(second.summary().unwrap().installation_id, first_id);
        assert_eq!(second.updater_installation_id().unwrap(), None);
    }

    #[test]
    fn malformed_noncanonical_or_oversized_legacy_id_is_not_adopted() {
        for malformed in [
            "not-a-uuid\n".to_owned(),
            "87E66203-9849-44C5-A557-8E77C29E7587".to_owned(),
            "x".repeat(256),
        ] {
            let directory = tempfile::tempdir().expect("temporary directory");
            std::fs::write(directory.path().join(INSTALLATION_ID_FILE), malformed)
                .expect("write malformed ID");
            let state = open_desktop_product_state(directory.path(), "1")
                .expect("replace malformed legacy state");
            assert!(uuid::Uuid::parse_str(&state.summary().unwrap().installation_id).is_ok());
            assert_eq!(state.updater_installation_id().unwrap(), None);
            assert!(!directory.path().join(INSTALLATION_ID_FILE).exists());
        }
    }

    #[test]
    fn valid_legacy_id_is_adopted_before_cleanup_and_remains_the_only_authority() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let legacy = "87e66203-9849-44c5-a557-8e77c29e7587";
        std::fs::write(
            directory.path().join(INSTALLATION_ID_FILE),
            format!("{legacy}\n"),
        )
        .expect("write legacy ID");
        let state =
            open_desktop_product_state(directory.path(), "1").expect("adopt legacy product ID");
        assert_eq!(state.summary().unwrap().installation_id, legacy);
        assert_eq!(
            state.updater_installation_id().unwrap().as_deref(),
            Some(legacy)
        );
        assert!(!directory.path().join(INSTALLATION_ID_FILE).exists());
        drop(state);

        let stale = "6521c174-0aa9-4fc8-b1fe-702ff3d332d6";
        std::fs::write(directory.path().join(INSTALLATION_ID_FILE), stale)
            .expect("restore stale legacy authority after simulated migration crash");
        let reopened = open_desktop_product_state(directory.path(), "2")
            .expect("reopen committed product state");
        assert_eq!(reopened.summary().unwrap().installation_id, legacy);
        assert!(!directory.path().join(INSTALLATION_ID_FILE).exists());
    }

    #[test]
    fn failed_product_open_does_not_remove_legacy_input() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let legacy = "87e66203-9849-44c5-a557-8e77c29e7587";
        let legacy_path = directory.path().join(INSTALLATION_ID_FILE);
        std::fs::write(&legacy_path, legacy).expect("write legacy ID");
        let connection = rusqlite::Connection::open(directory.path().join("product.db"))
            .expect("create future product database");
        connection
            .pragma_update(None, "user_version", 2)
            .expect("set future schema");
        drop(connection);

        assert!(open_desktop_product_state(directory.path(), "1").is_err());
        assert_eq!(std::fs::read_to_string(legacy_path).unwrap(), legacy);
    }
}
