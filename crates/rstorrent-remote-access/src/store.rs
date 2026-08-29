use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::{
    ClientState, MAX_FAILED_BUCKETS, MAX_OBSERVATION_BYTES, MAX_SECURITY_EVENTS, MAX_TOMBSTONES,
    SecuritySnapshot, Timestamp, decode_fixed, validate_bounded_text,
};
use crate::{EventId, RemoteAccessError, RemoteAuthority, Result};

const AUTHORITY_FILE: &str = "remote-authority-v1.json";
const HISTORY_FILE: &str = "remote-security-history-v1.json";
const MAX_AUTHORITY_BYTES: usize = 1024 * 1024;
const HISTORY_VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommitCrashPoint {
    AfterTemporarySync,
    AfterReplace,
    AfterHistoryReplace,
    AfterAuthorityRemoval,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisableOutcome {
    pub history: SecuritySnapshot,
    pub authority_file_removed: bool,
}

/// Owner-only atomic persistence for the complete enabled authority.
pub struct AuthorityStore {
    root: PathBuf,
    path: PathBuf,
    history_path: PathBuf,
}

impl AuthorityStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let path = root.join(AUTHORITY_FILE);
        let history_path = root.join(HISTORY_FILE);
        Self {
            root,
            path,
            history_path,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn history_path(&self) -> &Path {
        &self.history_path
    }

    pub fn load(&self) -> Result<Option<RemoteAuthority>> {
        #[cfg(unix)]
        {
            match fs::symlink_metadata(&self.path) {
                Ok(metadata) => validate_authority_metadata(&metadata)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error.into()),
            }
            let file = File::open(&self.path)?;
            let metadata = file.metadata()?;
            validate_authority_metadata(&metadata)?;
            let mut encoded = Vec::new();
            file.take((MAX_AUTHORITY_BYTES + 1) as u64)
                .read_to_end(&mut encoded)?;
            if encoded.len() > MAX_AUTHORITY_BYTES {
                return Err(RemoteAccessError::Corrupt("record exceeds size limit"));
            }
            RemoteAuthority::decode(&encoded).map(Some)
        }

        #[cfg(not(unix))]
        Err(RemoteAccessError::PersistenceUnsupported)
    }

    pub fn create(&self, authority: &RemoteAuthority) -> Result<()> {
        if self.path.exists() {
            return Err(RemoteAccessError::Conflict("authority already exists"));
        }
        self.commit(authority, None)
    }

    /// Apply an operation to an isolated candidate and replace current memory
    /// only after the complete candidate is durable.
    pub fn update<T>(
        &self,
        current: &mut RemoteAuthority,
        operation: impl FnOnce(&mut RemoteAuthority) -> Result<T>,
    ) -> Result<T> {
        self.update_with_crash(current, operation, None)
    }

    pub fn update_with_crash<T>(
        &self,
        current: &mut RemoteAuthority,
        operation: impl FnOnce(&mut RemoteAuthority) -> Result<T>,
        crash: Option<CommitCrashPoint>,
    ) -> Result<T> {
        if matches!(
            crash,
            Some(CommitCrashPoint::AfterHistoryReplace | CommitCrashPoint::AfterAuthorityRemoval)
        ) {
            return Err(RemoteAccessError::InvalidInput(
                "authority update crash point",
            ));
        }
        let snapshot = current.encode()?;
        let mut candidate = RemoteAuthority::decode(&snapshot)?;
        let output = operation(&mut candidate)?;
        candidate.advance_generation()?;
        self.commit(&candidate, crash)?;
        *current = candidate;
        Ok(output)
    }

    /// Remove the authority file after all live owners have been joined.
    ///
    /// Callers must first export the non-authorizing history they intend to
    /// retain. This operation never touches torrent or profile state.
    pub fn remove(&self) -> Result<bool> {
        #[cfg(unix)]
        {
            match fs::symlink_metadata(&self.path) {
                Ok(metadata) => validate_authority_metadata(&metadata)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(error) => return Err(error.into()),
            }
            fs::remove_file(&self.path)?;
            sync_directory(&self.root)?;
            Ok(true)
        }

        #[cfg(not(unix))]
        Err(RemoteAccessError::PersistenceUnsupported)
    }

    pub fn load_history(&self) -> Result<Option<SecuritySnapshot>> {
        #[cfg(unix)]
        {
            let Some(encoded) = read_protected(&self.history_path)? else {
                return Ok(None);
            };
            let persisted: PersistedHistory = serde_json::from_slice(&encoded)
                .map_err(|_| RemoteAccessError::Corrupt("malformed security history"))?;
            if persisted.version != HISTORY_VERSION {
                return Err(RemoteAccessError::Corrupt("security history version"));
            }
            validate_history(&persisted.snapshot)?;
            Ok(Some(persisted.snapshot))
        }

        #[cfg(not(unix))]
        Err(RemoteAccessError::PersistenceUnsupported)
    }

    pub fn disable(
        &self,
        authority: RemoteAuthority,
        now: Timestamp,
        event_id: EventId,
    ) -> Result<DisableOutcome> {
        self.disable_with_crash(authority, now, event_id, None)
    }

    pub fn disable_with_crash(
        &self,
        authority: RemoteAuthority,
        now: Timestamp,
        event_id: EventId,
        crash: Option<CommitCrashPoint>,
    ) -> Result<DisableOutcome> {
        let current = authority.disabled_snapshot(now, event_id)?;
        let history = match self.load_history()? {
            Some(previous) => merge_history(previous, current),
            None => current,
        };
        let persisted = PersistedHistory {
            version: HISTORY_VERSION,
            snapshot: history.clone(),
        };
        let mut encoded = serde_json::to_vec_pretty(&persisted)
            .map_err(|_| RemoteAccessError::Corrupt("security history serialization"))?;
        encoded.push(b'\n');
        let history_crash = crash.filter(|point| {
            matches!(
                point,
                CommitCrashPoint::AfterTemporarySync | CommitCrashPoint::AfterHistoryReplace
            )
        });
        self.commit_history(&encoded, history_crash)?;
        if crash == Some(CommitCrashPoint::AfterHistoryReplace) {
            return Err(RemoteAccessError::SimulatedCrash("after history replace"));
        }
        if !self.remove()? {
            return Err(RemoteAccessError::Conflict(
                "authority disappeared during disable",
            ));
        }
        if crash == Some(CommitCrashPoint::AfterAuthorityRemoval) {
            return Err(RemoteAccessError::SimulatedCrash("after authority removal"));
        }
        Ok(DisableOutcome {
            history,
            authority_file_removed: true,
        })
    }

    pub fn clear_history(&self) -> Result<bool> {
        #[cfg(unix)]
        {
            match fs::symlink_metadata(&self.history_path) {
                Ok(metadata) => validate_authority_metadata(&metadata)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(error) => return Err(error.into()),
            }
            fs::remove_file(&self.history_path)?;
            sync_directory(&self.root)?;
            Ok(true)
        }

        #[cfg(not(unix))]
        Err(RemoteAccessError::PersistenceUnsupported)
    }

    fn commit(&self, authority: &RemoteAuthority, crash: Option<CommitCrashPoint>) -> Result<()> {
        #[cfg(unix)]
        {
            ensure_protected_root(&self.root)?;
            if let Ok(metadata) = fs::symlink_metadata(&self.path) {
                validate_authority_metadata(&metadata)?;
            }
            let encoded = authority.encode()?;
            if encoded.len() > MAX_AUTHORITY_BYTES {
                return Err(RemoteAccessError::Capacity("authority file"));
            }
            let mut temporary = tempfile::NamedTempFile::new_in(&self.root)?;
            set_owner_only_file(temporary.path())?;
            temporary.write_all(&encoded)?;
            temporary.as_file().sync_all()?;
            if crash == Some(CommitCrashPoint::AfterTemporarySync) {
                return Err(RemoteAccessError::SimulatedCrash("after temporary sync"));
            }
            temporary
                .persist(&self.path)
                .map_err(|error| RemoteAccessError::Io(error.error))?;
            sync_directory(&self.root)?;
            if crash == Some(CommitCrashPoint::AfterReplace) {
                return Err(RemoteAccessError::SimulatedCrash("after replace"));
            }
            Ok(())
        }

        #[cfg(not(unix))]
        {
            let _ = authority;
            let _ = crash;
            Err(RemoteAccessError::PersistenceUnsupported)
        }
    }

    #[cfg(unix)]
    fn commit_history(&self, encoded: &[u8], crash: Option<CommitCrashPoint>) -> Result<()> {
        ensure_protected_root(&self.root)?;
        if encoded.len() > MAX_AUTHORITY_BYTES {
            return Err(RemoteAccessError::Capacity("security history file"));
        }
        if let Ok(metadata) = fs::symlink_metadata(&self.history_path) {
            validate_authority_metadata(&metadata)?;
        }
        let mut temporary = tempfile::NamedTempFile::new_in(&self.root)?;
        set_owner_only_file(temporary.path())?;
        temporary.write_all(encoded)?;
        temporary.as_file().sync_all()?;
        if crash == Some(CommitCrashPoint::AfterTemporarySync) {
            return Err(RemoteAccessError::SimulatedCrash("after temporary sync"));
        }
        temporary
            .persist(&self.history_path)
            .map_err(|error| RemoteAccessError::Io(error.error))?;
        sync_directory(&self.root)?;
        Ok(())
    }

    #[cfg(not(unix))]
    fn commit_history(&self, _encoded: &[u8], _crash: Option<CommitCrashPoint>) -> Result<()> {
        Err(RemoteAccessError::PersistenceUnsupported)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedHistory {
    version: u16,
    snapshot: SecuritySnapshot,
}

fn merge_history(mut previous: SecuritySnapshot, current: SecuritySnapshot) -> SecuritySnapshot {
    previous.generation = current.generation;
    previous.authorization_generation = current.authorization_generation;
    previous.clients.clear();
    previous.tombstones.extend(current.tombstones);
    previous.tombstones.sort_by_key(|item| item.ended);
    previous
        .tombstones
        .dedup_by(|right, left| right.client_id == left.client_id);
    if previous.tombstones.len() > MAX_TOMBSTONES {
        previous
            .tombstones
            .drain(..previous.tombstones.len() - MAX_TOMBSTONES);
    }
    previous.events.extend(current.events);
    previous.events.sort_by_key(|item| item.timestamp);
    previous
        .events
        .dedup_by(|right, left| right.event_id == left.event_id);
    if previous.events.len() > MAX_SECURITY_EVENTS {
        previous
            .events
            .drain(..previous.events.len() - MAX_SECURITY_EVENTS);
    }
    for current_bucket in current.failed_attempts {
        if let Some(previous_bucket) = previous.failed_attempts.iter_mut().find(|previous| {
            previous.bucket_start == current_bucket.bucket_start
                && previous.kind == current_bucket.kind
                && previous.route_class == current_bucket.route_class
        }) {
            previous_bucket.attempts = previous_bucket
                .attempts
                .saturating_add(current_bucket.attempts);
        } else {
            previous.failed_attempts.push(current_bucket);
        }
    }
    previous
        .failed_attempts
        .sort_by_key(|item| item.bucket_start);
    if previous.failed_attempts.len() > MAX_FAILED_BUCKETS {
        previous
            .failed_attempts
            .drain(..previous.failed_attempts.len() - MAX_FAILED_BUCKETS);
    }
    previous
}

fn validate_history(history: &SecuritySnapshot) -> Result<()> {
    if history.generation == 0
        || history.authorization_generation == 0
        || !history.clients.is_empty()
        || history.tombstones.len() > MAX_TOMBSTONES
        || history.events.len() > MAX_SECURITY_EVENTS
        || history.failed_attempts.len() > MAX_FAILED_BUCKETS
    {
        return Err(RemoteAccessError::Corrupt("security history bounds"));
    }
    let mut client_ids = HashSet::new();
    for tombstone in &history.tombstones {
        let _: [u8; 16] = decode_fixed(&tombstone.client_id)?;
        if !client_ids.insert(&tombstone.client_id)
            || tombstone.state == ClientState::Current
            || tombstone.created > tombstone.last_seen
            || tombstone.last_seen > tombstone.ended
        {
            return Err(RemoteAccessError::Corrupt("security history tombstone"));
        }
        let _: [u8; 32] = decode_fixed(&tombstone.fingerprint)?;
        validate_bounded_text(&tombstone.label, 1, 96, "history label")
            .map_err(|_| RemoteAccessError::Corrupt("security history label"))?;
    }
    let mut event_ids = HashSet::new();
    for event in &history.events {
        let _: [u8; 16] = decode_fixed(&event.event_id)?;
        if !event_ids.insert(&event.event_id) {
            return Err(RemoteAccessError::Corrupt("security history event"));
        }
        if let Some(client_id) = &event.client_id {
            let _: [u8; 16] = decode_fixed(client_id)?;
        }
        if let Some(circuit_id) = &event.circuit_id {
            let _: [u8; 16] = decode_fixed(circuit_id)?;
        }
        for (value, maximum, name) in [
            (&event.route, 64, "history route"),
            (&event.client_build, MAX_OBSERVATION_BYTES, "history build"),
            (&event.reason_class, 64, "history reason"),
        ] {
            if let Some(value) = value {
                validate_bounded_text(value, 1, maximum, name)
                    .map_err(|_| RemoteAccessError::Corrupt(name))?;
            }
        }
    }
    for bucket in &history.failed_attempts {
        if bucket.attempts == 0 {
            return Err(RemoteAccessError::Corrupt("security history bucket"));
        }
        validate_bounded_text(
            &bucket.route_class,
            1,
            MAX_OBSERVATION_BYTES,
            "history route class",
        )
        .map_err(|_| RemoteAccessError::Corrupt("security history route class"))?;
    }
    Ok(())
}

#[cfg(unix)]
fn read_protected(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => validate_authority_metadata(&metadata)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let file = File::open(path)?;
    validate_authority_metadata(&file.metadata()?)?;
    let mut encoded = Vec::new();
    file.take((MAX_AUTHORITY_BYTES + 1) as u64)
        .read_to_end(&mut encoded)?;
    if encoded.len() > MAX_AUTHORITY_BYTES {
        return Err(RemoteAccessError::Corrupt("record exceeds size limit"));
    }
    Ok(Some(encoded))
}

#[cfg(unix)]
fn ensure_protected_root(root: &Path) -> Result<()> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    match fs::symlink_metadata(root) {
        Ok(metadata)
            if metadata.is_dir()
                && !metadata.file_type().is_symlink()
                && metadata.uid() == rustix::process::getuid().as_raw() =>
        {
            fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
        }
        Ok(_) => {
            return Err(RemoteAccessError::Corrupt(
                "authority root ownership or type",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(root)?;
            let metadata = fs::symlink_metadata(root)?;
            if !metadata.is_dir()
                || metadata.file_type().is_symlink()
                || metadata.uid() != rustix::process::getuid().as_raw()
            {
                return Err(RemoteAccessError::Corrupt(
                    "authority root ownership or type",
                ));
            }
            fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

#[cfg(unix)]
fn validate_authority_metadata(metadata: &fs::Metadata) -> Result<()> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != rustix::process::getuid().as_raw()
        || metadata.permissions().mode() & 0o777 != 0o600
    {
        return Err(RemoteAccessError::Corrupt(
            "authority file ownership, type, or permissions",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use tempfile::tempdir;

    use super::*;
    use crate::authority::test_support::{authorize, provision, signing_key};
    use crate::{EventId, Timestamp};

    #[test]
    fn create_load_and_update_preserve_one_complete_generation() {
        let root = tempdir().unwrap();
        let store = AuthorityStore::new(root.path().join("remote"));
        let mut authority = provision(Timestamp::from_millis(1));
        store.create(&authority).unwrap();
        assert_eq!(store.load().unwrap().unwrap().generation(), 1);

        store
            .update(&mut authority, |candidate| {
                authorize(
                    candidate,
                    &signing_key(10),
                    11,
                    Timestamp::from_millis(2),
                    12,
                );
                Ok(())
            })
            .unwrap();
        assert_eq!(authority.generation(), 2);
        let loaded = store.load().unwrap().unwrap();
        assert_eq!(loaded.generation(), 2);
        assert_eq!(loaded.security_snapshot().clients.len(), 1);
    }

    #[test]
    fn crash_before_replace_keeps_prior_and_after_replace_keeps_new() {
        let root = tempdir().unwrap();
        let store = AuthorityStore::new(root.path().join("remote"));
        let mut authority = provision(Timestamp::from_millis(1));
        store.create(&authority).unwrap();

        let result = store.update_with_crash(
            &mut authority,
            |candidate| {
                candidate.set_last_status(Some("candidate-a".to_owned()))?;
                Ok(())
            },
            Some(CommitCrashPoint::AfterTemporarySync),
        );
        assert!(matches!(result, Err(RemoteAccessError::SimulatedCrash(_))));
        assert_eq!(authority.generation(), 1);
        assert_eq!(store.load().unwrap().unwrap().generation(), 1);

        let result = store.update_with_crash(
            &mut authority,
            |candidate| {
                candidate.set_last_status(Some("candidate-b".to_owned()))?;
                Ok(())
            },
            Some(CommitCrashPoint::AfterReplace),
        );
        assert!(matches!(result, Err(RemoteAccessError::SimulatedCrash(_))));
        assert_eq!(authority.generation(), 1);
        assert_eq!(store.load().unwrap().unwrap().generation(), 2);
    }

    #[test]
    fn failed_operation_never_changes_memory_or_disk() {
        let root = tempdir().unwrap();
        let store = AuthorityStore::new(root.path().join("remote"));
        let mut authority = provision(Timestamp::from_millis(1));
        store.create(&authority).unwrap();
        let result: Result<()> = store.update(&mut authority, |_candidate| {
            Err(RemoteAccessError::InvalidInput("injected"))
        });
        assert!(result.is_err());
        assert_eq!(authority.generation(), 1);
        assert_eq!(store.load().unwrap().unwrap().generation(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn authority_file_is_exact_owner_only_and_rejects_weakened_mode() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = tempdir().unwrap();
        let store = AuthorityStore::new(root.path().join("remote"));
        store.create(&provision(Timestamp::from_millis(1))).unwrap();
        assert_eq!(
            fs::metadata(store.path()).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::set_permissions(store.path(), fs::Permissions::from_mode(0o640)).unwrap();
        assert!(matches!(store.load(), Err(RemoteAccessError::Corrupt(_))));
    }

    #[test]
    fn oversized_and_malformed_records_fail_closed() {
        let root = tempdir().unwrap();
        let store = AuthorityStore::new(root.path().join("remote"));
        store.create(&provision(Timestamp::from_millis(1))).unwrap();
        let mut file = File::create(store.path()).unwrap();
        file.write_all(&vec![b'x'; MAX_AUTHORITY_BYTES + 1])
            .unwrap();
        #[cfg(unix)]
        set_owner_only_file(store.path()).unwrap();
        assert!(matches!(store.load(), Err(RemoteAccessError::Corrupt(_))));
    }

    #[test]
    fn removal_is_exact_and_idempotent() {
        let root = tempdir().unwrap();
        let store = AuthorityStore::new(root.path().join("remote"));
        let mut authority = provision(Timestamp::from_millis(1));
        store.create(&authority).unwrap();
        store
            .update(&mut authority, |candidate| {
                candidate.record_failed_attempt(
                    crate::FailedAttemptKind::Password,
                    "offline",
                    Timestamp::from_millis(2),
                )
            })
            .unwrap();
        assert!(store.remove().unwrap());
        assert!(!store.remove().unwrap());
        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn disable_retains_only_non_authorizing_history_and_can_clear_it() {
        let root = tempdir().unwrap();
        let store = AuthorityStore::new(root.path().join("remote"));
        let mut authority = provision(Timestamp::from_millis(1));
        authorize(
            &mut authority,
            &signing_key(21),
            22,
            Timestamp::from_millis(2),
            23,
        );
        store.create(&authority).unwrap();
        let outcome = store
            .disable(authority, Timestamp::from_millis(3), EventId::new([24; 16]))
            .unwrap();
        assert!(outcome.authority_file_removed);
        assert!(store.load().unwrap().is_none());
        assert!(outcome.history.clients.is_empty());
        assert_eq!(outcome.history.tombstones.len(), 1);
        assert_eq!(store.load_history().unwrap(), Some(outcome.history));
        assert!(store.clear_history().unwrap());
        assert!(store.load_history().unwrap().is_none());
    }

    #[test]
    fn disable_crash_resolves_to_enabled_or_history_only() {
        let root = tempdir().unwrap();
        let first = AuthorityStore::new(root.path().join("before-removal"));
        let authority = provision(Timestamp::from_millis(1));
        first.create(&authority).unwrap();
        let result = first.disable_with_crash(
            authority,
            Timestamp::from_millis(2),
            EventId::new([3; 16]),
            Some(CommitCrashPoint::AfterHistoryReplace),
        );
        assert!(matches!(result, Err(RemoteAccessError::SimulatedCrash(_))));
        assert!(first.load().unwrap().is_some());
        assert!(first.load_history().unwrap().is_some());

        let second = AuthorityStore::new(root.path().join("after-removal"));
        let authority = provision(Timestamp::from_millis(1));
        second.create(&authority).unwrap();
        let result = second.disable_with_crash(
            authority,
            Timestamp::from_millis(2),
            EventId::new([4; 16]),
            Some(CommitCrashPoint::AfterAuthorityRemoval),
        );
        assert!(matches!(result, Err(RemoteAccessError::SimulatedCrash(_))));
        assert!(second.load().unwrap().is_none());
        assert!(second.load_history().unwrap().is_some());
    }

    #[test]
    fn event_ids_remain_stable_after_round_trip() {
        let root = tempdir().unwrap();
        let store = AuthorityStore::new(root.path().join("remote"));
        let mut authority = provision(Timestamp::from_millis(1));
        store.create(&authority).unwrap();
        store
            .update(&mut authority, |candidate| {
                candidate.rotate_relay_credential(
                    [9; 32],
                    Timestamp::from_millis(2),
                    EventId::new([8; 16]),
                )
            })
            .unwrap();
        assert_eq!(
            authority.security_snapshot().events,
            store.load().unwrap().unwrap().security_snapshot().events
        );
    }
}
