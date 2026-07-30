use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use rstorrent_engine::{
    DownloadCheckpointSink, DownloadControl, DownloadError, DownloadReport,
    ResumableMagnetDownloadConfig, ResumedStorage, resume_magnet_with_peer_hint_with_control,
};
use rstorrent_protocol::metainfo::Metainfo;
use tokio::task::JoinHandle;

use crate::control::{
    Command, ErrorCode, RequestEnvelope, ResponseEnvelope, ResponseOutcome, StorageState,
    TorrentState,
};
use crate::have::HaveState;
use crate::store::{ConfiguredStorageRoot, ResumeRecord, SessionStore, StoreError};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_PAYLOAD_ALLOWANCE: usize = 32 * 1024;

#[derive(Clone, Debug)]
pub struct ApplicationConfig {
    pub profile_root: PathBuf,
    pub profile_id: String,
    pub storage_roots: Vec<ConfiguredStorageRoot>,
    pub download_timeout: Duration,
    pub max_buffered_payload_bytes: usize,
}

impl ApplicationConfig {
    pub fn new(
        profile_root: PathBuf,
        profile_id: String,
        storage_roots: Vec<ConfiguredStorageRoot>,
    ) -> Self {
        Self {
            profile_root,
            profile_id,
            storage_roots,
            download_timeout: DEFAULT_TIMEOUT,
            max_buffered_payload_bytes: DEFAULT_PAYLOAD_ALLOWANCE,
        }
    }
}

#[derive(Debug)]
struct ActiveDownload {
    torrent_id: String,
    control: DownloadControl,
    task: JoinHandle<Result<DownloadReport, DownloadError>>,
}

#[derive(Debug)]
pub struct ApplicationService {
    store: Arc<Mutex<SessionStore>>,
    storage_roots: BTreeMap<String, PathBuf>,
    download_timeout: Duration,
    max_buffered_payload_bytes: usize,
    active: Option<ActiveDownload>,
}

impl ApplicationService {
    pub async fn open(config: ApplicationConfig) -> Result<Self, ApplicationError> {
        if config.download_timeout.is_zero() {
            return Err(ApplicationError::Configuration(
                "download timeout must be nonzero".to_owned(),
            ));
        }
        let mut storage_roots = BTreeMap::new();
        for root in &config.storage_roots {
            if storage_roots
                .insert(root.id.clone(), root.path.clone())
                .is_some()
            {
                return Err(ApplicationError::Configuration(format!(
                    "storage root {} is duplicated",
                    root.id
                )));
            }
            std::fs::create_dir_all(&root.path).map_err(|source| ApplicationError::Io {
                operation: "create configured storage root",
                source,
            })?;
        }
        let store = SessionStore::open(
            &config.profile_root,
            &config.profile_id,
            &config.storage_roots,
        )?;
        let mut service = Self {
            store: Arc::new(Mutex::new(store)),
            storage_roots,
            download_timeout: config.download_timeout,
            max_buffered_payload_bytes: config.max_buffered_payload_bytes,
            active: None,
        };
        service.restore_running().await?;
        Ok(service)
    }

    pub async fn dispatch(
        &mut self,
        request: RequestEnvelope,
    ) -> Result<ResponseEnvelope, ApplicationError> {
        self.reap_finished().await?;
        let command = request.command.clone();
        if let Some(active) = &self.active {
            let target = match &command {
                Command::AddMagnet { magnet, .. } => {
                    rstorrent_protocol::magnet::Magnet::parse(magnet)
                        .ok()
                        .map(|magnet| encode_info_hash(magnet.info_hash))
                }
                Command::Resume { torrent_id } => Some(torrent_id.to_ascii_lowercase()),
                _ => None,
            };
            if target.is_some_and(|target| target != active.torrent_id) {
                return Ok(ResponseEnvelope::error(
                    request.request_id,
                    self.store_mut()?.revision()?,
                    ErrorCode::Busy,
                    format!(
                        "torrent {} already owns the download slot",
                        active.torrent_id
                    ),
                ));
            }
        }
        let response = self.store_mut()?.handle_durable(&request)?;
        if !matches!(response.outcome, ResponseOutcome::Success { .. }) {
            return Ok(response);
        }

        match command {
            Command::AddMagnet { magnet, .. } => {
                let torrent_id = rstorrent_protocol::magnet::Magnet::parse(&magnet)
                    .map(|magnet| encode_info_hash(magnet.info_hash))
                    .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
                self.start_if_possible(&torrent_id).await?;
            }
            Command::Resume { torrent_id } => {
                self.start_if_possible(&torrent_id.to_ascii_lowercase())
                    .await?;
            }
            Command::Pause { torrent_id } => {
                self.pause(&torrent_id.to_ascii_lowercase()).await?;
            }
            Command::Shutdown => {
                self.shutdown().await?;
            }
            Command::Snapshot => {}
        }
        Ok(response)
    }

    pub fn revision(&self) -> Result<u64, ApplicationError> {
        Ok(self.store_mut()?.revision()?)
    }

    pub async fn shutdown(&mut self) -> Result<(), ApplicationError> {
        if let Some(active) = self.active.take() {
            active.control.cancel();
            match active.task.await {
                Ok(Ok(_)) | Ok(Err(DownloadError::Cancelled)) => {}
                Ok(Err(error)) => self.record_task_error(&active.torrent_id, &error)?,
                Err(error) if error.is_cancelled() => {}
                Err(error) => return Err(ApplicationError::Join(error.to_string())),
            }
        }
        Ok(())
    }

    fn store_mut(&self) -> Result<MutexGuard<'_, SessionStore>, ApplicationError> {
        self.store
            .lock()
            .map_err(|_| ApplicationError::StorePoisoned)
    }

    async fn restore_running(&mut self) -> Result<(), ApplicationError> {
        let torrent_ids = self
            .store_mut()?
            .snapshot()?
            .torrents
            .into_iter()
            .filter(|torrent| !matches!(torrent.state, TorrentState::Paused))
            .map(|torrent| torrent.torrent_id)
            .collect::<Vec<_>>();
        for torrent_id in torrent_ids {
            let desired_running = match self.load_resume_conservative(&torrent_id) {
                Ok(resume) => resume.desired_running,
                Err(error) => {
                    self.store_mut()?
                        .mark_needs_repair(&torrent_id, &error.to_string())?;
                    continue;
                }
            };
            if desired_running {
                self.start_if_possible(&torrent_id).await?;
                break;
            }
        }
        Ok(())
    }

    async fn start_if_possible(&mut self, torrent_id: &str) -> Result<(), ApplicationError> {
        self.reap_finished().await?;
        if let Some(active) = &self.active {
            if active.torrent_id == torrent_id {
                return Ok(());
            }
            return Err(ApplicationError::Busy(active.torrent_id.clone()));
        }
        let resume = match self.load_resume_conservative(torrent_id) {
            Ok(resume) => resume,
            Err(error) => {
                self.store_mut()?
                    .mark_needs_repair(torrent_id, &error.to_string())?;
                return Ok(());
            }
        };
        if !resume.desired_running
            || matches!(
                resume.state,
                TorrentState::Paused | TorrentState::NeedsRepair
            )
        {
            return Ok(());
        }
        if let Some(raw_info) = &resume.raw_info {
            let metainfo = match Metainfo::from_info_bytes(raw_info) {
                Ok(metainfo) => metainfo,
                Err(error) => {
                    self.store_mut()?
                        .mark_needs_repair(torrent_id, &error.to_string())?;
                    return Ok(());
                }
            };
            if encode_info_hash(metainfo.info_hash) != torrent_id {
                self.store_mut()?.mark_needs_repair(
                    torrent_id,
                    "stored metadata does not match torrent identity",
                )?;
                return Ok(());
            }
        }
        let root = match self.storage_roots.get(&resume.storage_root) {
            Some(root) => root,
            None => {
                self.store_mut()?
                    .mark_needs_repair(torrent_id, "configured storage root is unavailable")?;
                return Ok(());
            }
        };
        let skip_files = resume
            .skip_files
            .iter()
            .map(|index| {
                usize::try_from(*index).map_err(|_| {
                    ApplicationError::Configuration("file selection index overflow".to_owned())
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let verified_pieces = resume
            .have
            .as_ref()
            .map_or_else(Vec::new, |have| have.pieces().to_vec());
        let config = ResumableMagnetDownloadConfig {
            magnet: resume.magnet,
            output_path: root.join(torrent_id),
            timeout: self.download_timeout,
            max_buffered_payload_bytes: self.max_buffered_payload_bytes,
            skip_files,
            verified_info: resume.raw_info,
            verified_pieces,
        };
        let checkpoints: Arc<dyn DownloadCheckpointSink> = Arc::new(StoreCheckpointSink {
            store: self.store.clone(),
            torrent_id: torrent_id.to_owned(),
        });
        let control = DownloadControl::new();
        let task_control = control.clone();
        let task = tokio::spawn(async move {
            resume_magnet_with_peer_hint_with_control(config, checkpoints, task_control).await
        });
        self.active = Some(ActiveDownload {
            torrent_id: torrent_id.to_owned(),
            control,
            task,
        });
        Ok(())
    }

    fn load_resume_conservative(&self, torrent_id: &str) -> Result<ResumeRecord, ApplicationError> {
        let mut store = self.store_mut()?;
        match store.load_resume(torrent_id) {
            Ok(resume) => Ok(resume),
            Err(StoreError::Have(_)) => {
                store.reset_have_from_metadata(torrent_id)?;
                Ok(store.load_resume(torrent_id)?)
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn pause(&mut self, torrent_id: &str) -> Result<(), ApplicationError> {
        if self
            .active
            .as_ref()
            .is_none_or(|active| active.torrent_id != torrent_id)
        {
            return Ok(());
        }
        let active = self.active.take().expect("matching active task exists");
        active.control.cancel();
        match active.task.await {
            Ok(Ok(_)) | Ok(Err(DownloadError::Cancelled)) => Ok(()),
            Ok(Err(error)) => {
                self.record_task_error(torrent_id, &error)?;
                Ok(())
            }
            Err(error) if error.is_cancelled() => Ok(()),
            Err(error) => Err(ApplicationError::Join(error.to_string())),
        }
    }

    async fn reap_finished(&mut self) -> Result<(), ApplicationError> {
        if self
            .active
            .as_ref()
            .is_none_or(|active| !active.task.is_finished())
        {
            return Ok(());
        }
        let active = self.active.take().expect("finished active task exists");
        match active.task.await {
            Ok(Ok(_)) | Ok(Err(DownloadError::Cancelled)) => Ok(()),
            Ok(Err(error)) => {
                self.record_task_error(&active.torrent_id, &error)?;
                Ok(())
            }
            Err(error) if error.is_cancelled() => Ok(()),
            Err(error) => Err(ApplicationError::Join(error.to_string())),
        }
    }

    fn record_task_error(
        &self,
        torrent_id: &str,
        error: &DownloadError,
    ) -> Result<(), ApplicationError> {
        let mut store = self.store_mut()?;
        if matches!(error, DownloadError::SelectiveStorage(_)) {
            store.mark_needs_repair(torrent_id, &error.to_string())?;
        } else {
            store.mark_error(torrent_id, &error.to_string())?;
        }
        Ok(())
    }
}

impl Drop for ApplicationService {
    fn drop(&mut self) {
        if let Some(active) = &self.active {
            active.control.cancel();
        }
    }
}

#[derive(Debug)]
struct StoreCheckpointSink {
    store: Arc<Mutex<SessionStore>>,
    torrent_id: String,
}

impl StoreCheckpointSink {
    fn store(&self) -> Result<MutexGuard<'_, SessionStore>, String> {
        self.store
            .lock()
            .map_err(|_| "session store lock is poisoned".to_owned())
    }
}

impl DownloadCheckpointSink for StoreCheckpointSink {
    fn metadata_verified(&self, raw_info: &[u8]) -> Result<(), String> {
        self.store().and_then(|mut store| {
            store
                .record_metadata(&self.torrent_id, raw_info)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
    }

    fn storage_prepared(&self, storage: ResumedStorage) -> Result<(), String> {
        let storage_state = match storage {
            ResumedStorage::Created | ResumedStorage::Staging => StorageState::Staging,
            ResumedStorage::Published => StorageState::Published,
        };
        self.store().and_then(|mut store| {
            store
                .mark_storage_prepared(&self.torrent_id, storage_state)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
    }

    fn have_rechecked(&self, verified_pieces: &[bool]) -> Result<(), String> {
        let resume = self.store().and_then(|store| {
            store
                .load_resume(&self.torrent_id)
                .map_err(|error| error.to_string())
        })?;
        let have = resume
            .have
            .ok_or_else(|| "metadata checkpoint did not create have state".to_owned())?;
        let replacement = HaveState::from_pieces(have.info_hash(), verified_pieces.to_vec())
            .map_err(|error| error.to_string())?;
        self.store().and_then(|mut store| {
            store
                .replace_have(&self.torrent_id, &replacement)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
    }

    fn piece_durable(&self, piece_index: usize) -> Result<(), String> {
        self.store().and_then(|mut store| {
            store
                .record_piece(&self.torrent_id, piece_index)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
    }

    fn published(&self) -> Result<(), String> {
        self.store().and_then(|mut store| {
            store
                .mark_complete(&self.torrent_id)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
    }
}

#[derive(Debug)]
pub enum ApplicationError {
    Store(StoreError),
    Io {
        operation: &'static str,
        source: std::io::Error,
    },
    Configuration(String),
    Busy(String),
    Join(String),
    StorePoisoned,
}

impl fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "{error}"),
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::Configuration(message) => {
                write!(formatter, "application configuration: {message}")
            }
            Self::Busy(torrent_id) => {
                write!(
                    formatter,
                    "torrent {torrent_id} already owns the download slot"
                )
            }
            Self::Join(message) => write!(formatter, "engine task join: {message}"),
            Self::StorePoisoned => write!(formatter, "session store lock is poisoned"),
        }
    }
}

impl Error for ApplicationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<StoreError> for ApplicationError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

fn encode_info_hash(info_hash: [u8; 20]) -> String {
    let mut output = String::with_capacity(40);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in info_hash {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

pub fn application_error_response(
    request_id: String,
    revision: u64,
    error: &ApplicationError,
) -> ResponseEnvelope {
    let code = match error {
        ApplicationError::Busy(_) => ErrorCode::Busy,
        ApplicationError::Configuration(_) => ErrorCode::InvalidRequest,
        ApplicationError::Store(StoreError::UnknownTorrent(_)) => ErrorCode::UnknownTorrent,
        ApplicationError::Store(StoreError::DurableState(_) | StoreError::Have(_)) => {
            ErrorCode::InvalidDurableState
        }
        _ => ErrorCode::Internal,
    };
    ResponseEnvelope::error(request_id, revision, code, error.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use rusqlite::Connection;
    use sha1::{Digest, Sha1};

    use super::{ApplicationConfig, ApplicationService};
    use crate::{
        CONTROL_VERSION, Command, ConfiguredStorageRoot, RequestEnvelope, ResponseOutcome,
        SessionStore, TorrentState,
    };

    static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

    fn test_root(label: &str) -> PathBuf {
        let sequence = NEXT_TEST.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rstorrent-application-{label}-{}-{sequence}",
            std::process::id()
        ))
    }

    fn config(root: &Path) -> ApplicationConfig {
        let mut config = ApplicationConfig::new(
            root.join("profile"),
            "test".to_owned(),
            vec![ConfiguredStorageRoot {
                id: "downloads".to_owned(),
                path: root.join("payload"),
            }],
        );
        config.download_timeout = std::time::Duration::from_secs(5);
        config
    }

    fn add_request(request_id: &str, torrent_id: &str) -> RequestEnvelope {
        RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: request_id.to_owned(),
            expected_revision: None,
            command: Command::AddMagnet {
                magnet: format!("magnet:?xt=urn:btih:{torrent_id}&x.pe=127.0.0.1:1"),
                storage_root: "downloads".to_owned(),
                skip_files: Vec::new(),
            },
        }
    }

    #[tokio::test]
    async fn pause_is_durable_and_prevents_restart() {
        let root = test_root("pause");
        let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
        let mut service = ApplicationService::open(config(&root))
            .await
            .expect("open service");
        service
            .dispatch(add_request("add", torrent_id))
            .await
            .expect("add");
        let paused = service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "pause".to_owned(),
                expected_revision: None,
                command: Command::Pause {
                    torrent_id: torrent_id.to_owned(),
                },
            })
            .await
            .expect("pause");
        let ResponseOutcome::Success { snapshot } = paused.outcome else {
            panic!("pause should succeed");
        };
        assert_eq!(snapshot.torrents[0].state, TorrentState::Paused);
        service.shutdown().await.expect("shutdown");
        drop(service);

        let mut reopened = ApplicationService::open(config(&root))
            .await
            .expect("reopen service");
        let snapshot = reopened
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "snapshot".to_owned(),
                expected_revision: None,
                command: Command::Snapshot,
            })
            .await
            .expect("snapshot");
        let ResponseOutcome::Success { snapshot } = snapshot.outcome else {
            panic!("snapshot should succeed");
        };
        assert_eq!(snapshot.torrents[0].state, TorrentState::Paused);
        reopened.shutdown().await.expect("shutdown reopened");
        drop(reopened);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn corrupt_metadata_enters_repair_before_storage() {
        let root = test_root("metadata-corruption");
        let configuration = config(&root);
        let raw_info =
            b"d6:lengthi4e4:name4:test12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = super::encode_info_hash(info_hash);
        let configured_root = configuration.storage_roots[0].clone();
        let mut store = SessionStore::open(
            &configuration.profile_root,
            &configuration.profile_id,
            &[configured_root],
        )
        .expect("open store");
        store
            .handle_durable(&add_request("add", &torrent_id))
            .expect("add");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        let database = store.database_path().to_owned();
        drop(store);

        let connection = Connection::open(database).expect("open raw database");
        connection
            .execute(
                "UPDATE torrents SET raw_info = x'00' WHERE info_hash = ?1",
                [info_hash.as_slice()],
            )
            .expect("corrupt raw metadata");
        drop(connection);

        let mut service = ApplicationService::open(configuration)
            .await
            .expect("open service with corrupt metadata");
        let response = service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "snapshot".to_owned(),
                expected_revision: None,
                command: Command::Snapshot,
            })
            .await
            .expect("snapshot");
        let ResponseOutcome::Success { snapshot } = response.outcome else {
            panic!("snapshot should succeed");
        };
        assert_eq!(snapshot.torrents[0].state, TorrentState::NeedsRepair);
        assert_eq!(
            fs::read_dir(root.join("payload"))
                .expect("read empty payload root")
                .count(),
            0
        );
        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn malformed_have_state_is_cleared_conservatively() {
        let root = test_root("have-corruption");
        let configuration = config(&root);
        let raw_info =
            b"d6:lengthi4e4:name4:test12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = super::encode_info_hash(info_hash);
        let configured_root = configuration.storage_roots[0].clone();
        let mut store = SessionStore::open(
            &configuration.profile_root,
            &configuration.profile_id,
            &[configured_root],
        )
        .expect("open store");
        store
            .handle_durable(&add_request("add", &torrent_id))
            .expect("add");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        let database = store.database_path().to_owned();
        drop(store);

        let connection = Connection::open(database).expect("open raw database");
        let mut have: Vec<u8> = connection
            .query_row(
                "SELECT have_state FROM torrents WHERE info_hash = ?1",
                [info_hash.as_slice()],
                |row| row.get(0),
            )
            .expect("read have state");
        *have.last_mut().expect("bitfield byte") = 1;
        connection
            .execute(
                "UPDATE torrents SET have_state = ?2 WHERE info_hash = ?1",
                rusqlite::params![info_hash.as_slice(), have],
            )
            .expect("corrupt have padding");
        drop(connection);

        let mut service = ApplicationService::open(configuration)
            .await
            .expect("open service with malformed have state");
        let response = service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "snapshot".to_owned(),
                expected_revision: None,
                command: Command::Snapshot,
            })
            .await
            .expect("snapshot");
        let ResponseOutcome::Success { snapshot } = response.outcome else {
            panic!("snapshot should succeed");
        };
        assert_eq!(snapshot.torrents[0].verified_piece_count, 0);
        assert_ne!(snapshot.torrents[0].state, TorrentState::NeedsRepair);
        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }
}
