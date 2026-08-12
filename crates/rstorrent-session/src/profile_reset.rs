//! Crash-convergent replacement of recognized pre-release session catalogs.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::store::StoreError;
use crate::store_schema::SCHEMA_VERSION;

pub(crate) const DATABASE_FILENAME: &str = "session.db";
const WAL_FILENAME: &str = "session.db-wal";
const SHM_FILENAME: &str = "session.db-shm";
const RESET_MARKER_MAGIC: &[u8; 8] = b"RSTRST19";
const RESET_MARKER_LENGTH: usize = 8 + 8 + 32;
const INSPECTION_TIMEOUT: Duration = Duration::from_secs(2);

const DISCARDED_CATEGORIES: [&str; 8] = [
    "torrents",
    "verified_piece_state",
    "sources_and_receipts",
    "storage_root_registry",
    "client_and_storage_settings",
    "dht_state",
    "pending_removals",
    "protocol_identity_aliases",
];

const DATABASE_BASENAMES: [&str; 3] = [DATABASE_FILENAME, WAL_FILENAME, SHM_FILENAME];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileResetReport {
    pub previous_schema_version: i64,
    pub discarded_categories: Vec<String>,
    pub database_basenames_considered: Vec<String>,
    pub external_payload_modified: bool,
}

impl ProfileResetReport {
    fn for_version(previous_schema_version: i64) -> Self {
        Self {
            previous_schema_version,
            discarded_categories: DISCARDED_CATEGORIES
                .into_iter()
                .map(str::to_owned)
                .collect(),
            database_basenames_considered: DATABASE_BASENAMES
                .into_iter()
                .map(str::to_owned)
                .collect(),
            external_payload_modified: false,
        }
    }
}

#[derive(Debug)]
pub(crate) enum CatalogPreparation {
    Current,
    Create {
        reset_report: Option<ProfileResetReport>,
    },
}

pub(crate) fn prepare_catalog(profile_root: &Path) -> Result<CatalogPreparation, StoreError> {
    let database_path = profile_root.join(DATABASE_FILENAME);
    let wal_path = profile_root.join(WAL_FILENAME);
    let shm_path = profile_root.join(SHM_FILENAME);
    validate_fixed_file(&database_path, DATABASE_FILENAME)?;
    validate_fixed_file(&wal_path, WAL_FILENAME)?;
    validate_fixed_file(&shm_path, SHM_FILENAME)?;

    let marker_version = read_reset_marker(&shm_path)?;
    if !database_path.exists() {
        return match marker_version {
            Some(version) => {
                ensure_absent(&wal_path, WAL_FILENAME)?;
                Ok(CatalogPreparation::Create {
                    reset_report: Some(ProfileResetReport::for_version(version)),
                })
            }
            None if wal_path.exists() || shm_path.exists() => Err(StoreError::UnsafeProfileFile {
                basename: DATABASE_FILENAME,
                reason: "database is missing while SQLite auxiliary files remain".to_owned(),
            }),
            None => Ok(CatalogPreparation::Create { reset_report: None }),
        };
    }

    let database_length = fs::metadata(&database_path)
        .map_err(|source| io_error("inspect session database", source))?
        .len();
    if database_length == 0 {
        if let Some(version) = marker_version {
            remove_fixed_file(&database_path, DATABASE_FILENAME)?;
            sync_directory(profile_root)?;
            return Ok(CatalogPreparation::Create {
                reset_report: Some(ProfileResetReport::for_version(version)),
            });
        }
        if wal_path.exists() || shm_path.exists() {
            return Err(StoreError::UnsafeProfileFile {
                basename: DATABASE_FILENAME,
                reason: "empty database has SQLite auxiliary files".to_owned(),
            });
        }
        remove_fixed_file(&database_path, DATABASE_FILENAME)?;
        sync_directory(profile_root)?;
        return Ok(CatalogPreparation::Create { reset_report: None });
    }

    let version = match inspect_user_version(&database_path) {
        Ok(version) => version,
        Err(_error) if marker_version.is_some() => {
            remove_fixed_file(&database_path, DATABASE_FILENAME)?;
            sync_directory(profile_root)?;
            return Ok(CatalogPreparation::Create {
                reset_report: Some(ProfileResetReport::for_version(
                    marker_version.expect("checked marker"),
                )),
            });
        }
        Err(error) => return Err(error),
    };
    if version == SCHEMA_VERSION {
        if let Some(previous) = marker_version {
            validate_committed_report(&database_path, previous)?;
            remove_fixed_file(&shm_path, SHM_FILENAME)?;
            sync_directory(profile_root)?;
        }
        return Ok(CatalogPreparation::Current);
    }
    if version == 0 {
        return Err(StoreError::UnsafeProfileFile {
            basename: DATABASE_FILENAME,
            reason: "nonempty database has no recognized schema version".to_owned(),
        });
    }
    if !(0..=SCHEMA_VERSION).contains(&version) {
        return Err(StoreError::UnsupportedSchema {
            actual: version,
            maximum: SCHEMA_VERSION,
        });
    }
    if !(1..SCHEMA_VERSION).contains(&version) {
        return Err(StoreError::UnsafeProfileFile {
            basename: DATABASE_FILENAME,
            reason: format!("schema version {version} is not a recognized reset source"),
        });
    }

    if let Some(previous) = marker_version {
        if previous != version {
            return Err(StoreError::UnsafeProfileFile {
                basename: SHM_FILENAME,
                reason: "reset marker disagrees with the remaining catalog".to_owned(),
            });
        }
        ensure_absent(&wal_path, WAL_FILENAME)?;
        remove_fixed_file(&database_path, DATABASE_FILENAME)?;
        sync_directory(profile_root)?;
        return Ok(CatalogPreparation::Create {
            reset_report: Some(ProfileResetReport::for_version(version)),
        });
    }

    exclusively_validate_legacy(&database_path, version)?;
    remove_fixed_file_if_present(&wal_path, WAL_FILENAME)?;
    remove_fixed_file_if_present(&shm_path, SHM_FILENAME)?;
    sync_directory(profile_root)?;
    write_reset_marker(&shm_path, version)?;
    sync_directory(profile_root)?;
    remove_fixed_file(&database_path, DATABASE_FILENAME)?;
    sync_directory(profile_root)?;
    Ok(CatalogPreparation::Create {
        reset_report: Some(ProfileResetReport::for_version(version)),
    })
}

pub(crate) fn finish_catalog_creation(profile_root: &Path) -> Result<(), StoreError> {
    let marker_path = profile_root.join(SHM_FILENAME);
    if read_reset_marker(&marker_path)?.is_some() {
        remove_fixed_file(&marker_path, SHM_FILENAME)?;
        sync_directory(profile_root)?;
    }
    Ok(())
}

fn inspect_user_version(database_path: &Path) -> Result<i64, StoreError> {
    let connection = Connection::open_with_flags(
        database_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(profile_sqlite_error)?;
    connection
        .busy_timeout(INSPECTION_TIMEOUT)
        .map_err(profile_sqlite_error)?;
    connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(profile_sqlite_error)
}

fn exclusively_validate_legacy(database_path: &Path, expected: i64) -> Result<(), StoreError> {
    let connection = Connection::open_with_flags(
        database_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(profile_sqlite_error)?;
    connection
        .busy_timeout(INSPECTION_TIMEOUT)
        .map_err(profile_sqlite_error)?;
    connection
        .pragma_update(None, "locking_mode", "EXCLUSIVE")
        .map_err(profile_sqlite_error)?;
    connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); BEGIN EXCLUSIVE;")
        .map_err(profile_sqlite_error)?;
    let observed: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(profile_sqlite_error)?;
    if observed != expected {
        let _ = connection.execute_batch("ROLLBACK;");
        return Err(StoreError::UnsafeProfileFile {
            basename: DATABASE_FILENAME,
            reason: "schema version changed while acquiring the reset lock".to_owned(),
        });
    }
    connection
        .execute_batch("COMMIT;")
        .map_err(profile_sqlite_error)?;
    connection
        .close()
        .map_err(|(_, error)| profile_sqlite_error(error))
}

fn validate_committed_report(database_path: &Path, previous: i64) -> Result<(), StoreError> {
    let connection = Connection::open_with_flags(
        database_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(profile_sqlite_error)?;
    let stored = connection
        .query_row(
            "SELECT previous_schema_version FROM profile_reset_report WHERE singleton = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(profile_sqlite_error)?;
    if stored != previous {
        return Err(StoreError::UnsafeProfileFile {
            basename: DATABASE_FILENAME,
            reason: "committed reset report disagrees with recovery marker".to_owned(),
        });
    }
    Ok(())
}

fn validate_fixed_file(path: &Path, basename: &'static str) -> Result<(), StoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(StoreError::UnsafeProfileFile {
            basename,
            reason: "symbolic links are not accepted".to_owned(),
        }),
        Ok(metadata) if !metadata.is_file() => Err(StoreError::UnsafeProfileFile {
            basename,
            reason: "path is not a regular file".to_owned(),
        }),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(io_error("inspect profile file", source)),
    }
}

fn read_reset_marker(path: &Path) -> Result<Option<i64>, StoreError> {
    if !path.exists() {
        return Ok(None);
    }
    let metadata = fs::metadata(path).map_err(|source| io_error("inspect reset marker", source))?;
    if metadata.len() != RESET_MARKER_LENGTH as u64 {
        return Ok(None);
    }
    let mut bytes = [0_u8; RESET_MARKER_LENGTH];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|source| io_error("read reset marker", source))?;
    if &bytes[..8] != RESET_MARKER_MAGIC {
        return Ok(None);
    }
    let expected = Sha256::digest(&bytes[..16]);
    if bytes[16..] != expected[..] {
        return Err(StoreError::UnsafeProfileFile {
            basename: SHM_FILENAME,
            reason: "reset marker checksum is invalid".to_owned(),
        });
    }
    let version = i64::from_be_bytes(bytes[8..16].try_into().expect("fixed version bytes"));
    if !(1..SCHEMA_VERSION).contains(&version) {
        return Err(StoreError::UnsafeProfileFile {
            basename: SHM_FILENAME,
            reason: "reset marker schema version is invalid".to_owned(),
        });
    }
    Ok(Some(version))
}

fn write_reset_marker(path: &Path, version: i64) -> Result<(), StoreError> {
    let mut bytes = [0_u8; RESET_MARKER_LENGTH];
    bytes[..8].copy_from_slice(RESET_MARKER_MAGIC);
    bytes[8..16].copy_from_slice(&version.to_be_bytes());
    let digest = Sha256::digest(&bytes[..16]);
    bytes[16..].copy_from_slice(&digest);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| io_error("create reset marker", source))?;
    file.write_all(&bytes)
        .map_err(|source| io_error("write reset marker", source))?;
    file.sync_all()
        .map_err(|source| io_error("sync reset marker", source))
}

fn ensure_absent(path: &Path, basename: &'static str) -> Result<(), StoreError> {
    if path.exists() {
        return Err(StoreError::UnsafeProfileFile {
            basename,
            reason: "unexpected file remains during reset recovery".to_owned(),
        });
    }
    Ok(())
}

fn remove_fixed_file_if_present(path: &Path, basename: &'static str) -> Result<(), StoreError> {
    if path.exists() {
        remove_fixed_file(path, basename)?;
    }
    Ok(())
}

fn remove_fixed_file(path: &Path, basename: &'static str) -> Result<(), StoreError> {
    validate_fixed_file(path, basename)?;
    fs::remove_file(path).map_err(|source| io_error("remove reset database file", source))
}

fn sync_directory(profile_root: &Path) -> Result<(), StoreError> {
    File::open(profile_root)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| io_error("sync profile directory", source))
}

fn profile_sqlite_error(error: rusqlite::Error) -> StoreError {
    match &error {
        rusqlite::Error::SqliteFailure(failure, _)
            if matches!(
                failure.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            ) =>
        {
            StoreError::ProfileResetBusy
        }
        _ => StoreError::UnsafeProfileFile {
            basename: DATABASE_FILENAME,
            reason: error.to_string(),
        },
    }
}

fn io_error(operation: &'static str, source: std::io::Error) -> StoreError {
    StoreError::Io { operation, source }
}

#[cfg(test)]
mod tests {
    use super::{
        CatalogPreparation, DATABASE_FILENAME, RESET_MARKER_LENGTH, SHM_FILENAME,
        finish_catalog_creation, prepare_catalog, write_reset_marker,
    };
    use crate::SessionStore;
    use crate::store::StoreError;
    use rusqlite::Connection;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

    fn root(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "rstorrent-reset-{name}-{}-{}",
            std::process::id(),
            NEXT_TEST.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("test root");
        path
    }

    fn legacy(root: &PathBuf, version: i64) {
        let connection = Connection::open(root.join(DATABASE_FILENAME)).expect("legacy database");
        connection
            .execute_batch("CREATE TABLE legacy(value INTEGER);")
            .expect("legacy table");
        connection
            .pragma_update(None, "user_version", version)
            .expect("legacy version");
    }

    #[test]
    fn recognized_catalog_is_replaced_without_touching_payload_sentinels() {
        let root = root("recognized");
        legacy(&root, 18);
        let payload = root.join("payload-sentinel");
        fs::write(&payload, b"keep").expect("payload sentinel");

        let CatalogPreparation::Create { reset_report } =
            prepare_catalog(&root).expect("prepare reset")
        else {
            panic!("legacy catalog must reset");
        };
        let report = reset_report.expect("reset report");
        assert_eq!(report.previous_schema_version, 18);
        assert!(!report.external_payload_modified);
        assert_eq!(fs::read(&payload).expect("payload survives"), b"keep");
        assert!(!root.join(DATABASE_FILENAME).exists());
        assert_eq!(
            fs::metadata(root.join(SHM_FILENAME)).expect("marker").len(),
            RESET_MARKER_LENGTH as u64
        );
        finish_catalog_creation(&root).expect("finish reset");
        assert!(!root.join(SHM_FILENAME).exists());
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[test]
    fn recovery_marker_converges_after_database_removal() {
        let root = root("recovery");
        write_reset_marker(&root.join(SHM_FILENAME), 7).expect("marker");
        let CatalogPreparation::Create { reset_report } =
            prepare_catalog(&root).expect("resume reset")
        else {
            panic!("marker must resume reset");
        };
        assert_eq!(reset_report.expect("report").previous_schema_version, 7);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[test]
    fn recovery_converges_with_legacy_or_new_database_beside_marker() {
        let legacy_root = root("legacy-marker");
        legacy(&legacy_root, 12);
        write_reset_marker(&legacy_root.join(SHM_FILENAME), 12).expect("marker");
        let CatalogPreparation::Create { reset_report } =
            prepare_catalog(&legacy_root).expect("resume beside legacy database")
        else {
            panic!("legacy database plus marker must resume reset");
        };
        assert_eq!(reset_report.expect("report").previous_schema_version, 12);
        fs::remove_dir_all(legacy_root).expect("remove legacy root");

        let current_root = root("current-marker");
        legacy(&current_root, 18);
        let store = SessionStore::open(&current_root, "default", &[]).expect("reset to current");
        drop(store);
        write_reset_marker(&current_root.join(SHM_FILENAME), 18).expect("recovery marker");
        assert!(matches!(
            prepare_catalog(&current_root).expect("finish committed reset"),
            CatalogPreparation::Current
        ));
        assert!(!current_root.join(SHM_FILENAME).exists());
        fs::remove_dir_all(current_root).expect("remove current root");
    }

    #[test]
    fn malformed_unversioned_and_unsafe_shapes_fail_closed() {
        let malformed = root("malformed");
        fs::write(malformed.join(DATABASE_FILENAME), b"not sqlite").expect("malformed");
        assert!(matches!(
            prepare_catalog(&malformed),
            Err(StoreError::UnsafeProfileFile { .. })
        ));
        fs::remove_dir_all(malformed).expect("remove malformed root");

        let unversioned = root("unversioned");
        let connection = Connection::open(unversioned.join(DATABASE_FILENAME)).expect("database");
        connection
            .execute_batch("CREATE TABLE unknown(value INTEGER);")
            .expect("unknown table");
        drop(connection);
        assert!(matches!(
            prepare_catalog(&unversioned),
            Err(StoreError::UnsafeProfileFile { .. })
        ));
        fs::remove_dir_all(unversioned).expect("remove unversioned root");

        let symlink = root("symlink");
        let target = symlink.join("target");
        fs::write(&target, b"target").expect("target");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&target, symlink.join(DATABASE_FILENAME))
                .expect("database symlink");
            assert!(matches!(
                prepare_catalog(&symlink),
                Err(StoreError::UnsafeProfileFile { .. })
            ));
        }
        fs::remove_dir_all(symlink).expect("remove symlink root");
    }

    #[test]
    fn busy_legacy_catalog_fails_without_removing_it() {
        let root = root("busy");
        legacy(&root, 18);
        let blocker = Connection::open(root.join(DATABASE_FILENAME)).expect("blocking connection");
        blocker
            .execute_batch("BEGIN EXCLUSIVE;")
            .expect("exclusive blocker");
        assert!(matches!(
            prepare_catalog(&root),
            Err(StoreError::ProfileResetBusy)
        ));
        assert!(root.join(DATABASE_FILENAME).exists());
        blocker.execute_batch("ROLLBACK;").expect("release blocker");
        drop(blocker);
        fs::remove_dir_all(root).expect("remove test root");
    }
}
