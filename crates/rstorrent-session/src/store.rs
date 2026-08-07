use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rstorrent_engine::dht::DhtSnapshot;
use rstorrent_engine::{PreparedFileHash, plan_descriptor_storage, validate_publication_name};
use rstorrent_protocol::bencode::ParseError;
use rstorrent_protocol::dht::{DhtEndpoint, DhtIp, NodeContact, NodeId};
use rstorrent_protocol::magnet::{
    FileIndexRange as MagnetFileIndexRange, Magnet, TrackerUrlTransport,
};
use rstorrent_protocol::metainfo::{
    DURABLE_METAINFO_LIMITS, EXPLICIT_IMPORT_METAINFO_LIMITS, Metainfo, MetainfoError,
    MetainfoProjection, MetainfoTrackerTransport,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sha2::{Digest as Sha256Digest, Sha256};

use crate::control::{
    AddTorrentBytesRequest, AddTorrentDisposition, AddTorrentResult, Command, CommandResult,
    ErrorCode, FilePriority, FileSelectionIntent, MAX_FILE_SELECTION_ENTRIES, RemovalDataPolicy,
    RemovalState, RequestEnvelope, ResponseEnvelope, ServiceSnapshot, StorageState,
    TorrentSnapshot, TorrentState, decode_info_hash, encode_info_hash, parse_revision,
    validate_add_torrent_bytes_request, validate_identifier, validate_request,
};
use crate::have::{HaveError, HaveState, MAX_DURABLE_HAVE_STATE_BYTES, MAX_DURABLE_PIECES};
use crate::settings::{
    ClientSettings, SettingsPersistenceError, StorageRootAvailability, StorageRootSnapshot,
    StorageSettingsSnapshot, create_client_settings, migrate_client_settings_to_v10,
    migrate_client_settings_to_v11, migrate_client_settings_to_v12, read_client_settings,
    replace_client_settings,
};

const SCHEMA_VERSION: i64 = 13;
const DATABASE_FILENAME: &str = "session.db";
const MAX_RECEIPTS: i64 = 1024;
pub(crate) const EPHEMERAL_SESSION_MAX_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_STORAGE_ROOTS: usize = 32;
pub const MAX_STORAGE_ROOT_LOCATOR_LENGTH: usize = 4096;
const BUSY_TIMEOUT: Duration = Duration::from_secs(2);
const DHT_TABLES_SQL: &str = "CREATE TABLE dht_state (
        singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
        format_version INTEGER NOT NULL CHECK (format_version > 0),
        node_id BLOB NOT NULL CHECK (length(node_id) = 20)
     );
     CREATE TABLE dht_nodes (
        family INTEGER NOT NULL CHECK (family IN (4, 6)),
        sample_order INTEGER NOT NULL CHECK (
            sample_order >= 0 AND sample_order < 64
        ),
        node_id BLOB NOT NULL CHECK (length(node_id) = 20),
        address BLOB NOT NULL CHECK (
            (family = 4 AND length(address) = 4) OR
            (family = 6 AND length(address) = 16)
        ),
        port INTEGER NOT NULL CHECK (port > 0 AND port <= 65535),
        PRIMARY KEY (family, sample_order),
        UNIQUE (family, node_id),
        UNIQUE (family, address, port)
     ) WITHOUT ROWID;";
const REMOVAL_TABLE_SQL: &str = "CREATE TABLE removal_jobs (
        info_hash BLOB PRIMARY KEY
            REFERENCES torrents(info_hash) ON DELETE CASCADE,
        operation_id TEXT NOT NULL UNIQUE
            CHECK (length(operation_id) BETWEEN 1 AND 128),
        data_policy TEXT NOT NULL
            CHECK (data_policy IN ('keep', 'delete_managed')),
        state TEXT NOT NULL
            CHECK (state IN ('pending', 'awaiting_platform', 'failed')),
        error TEXT CHECK (error IS NULL OR length(error) <= 1024),
        created_revision INTEGER NOT NULL CHECK (created_revision >= 0),
        updated_revision INTEGER NOT NULL CHECK (updated_revision >= 0)
     ) WITHOUT ROWID;";
const SOURCE_TABLES_SQL: &str = "CREATE TABLE torrent_source (
        info_hash BLOB PRIMARY KEY
            REFERENCES torrents(info_hash) ON DELETE CASCADE,
        kind TEXT NOT NULL CHECK (kind IN ('magnet', 'metainfo')),
        fidelity TEXT NOT NULL CHECK (fidelity IN ('verbatim', 'canonicalized')),
        magnet TEXT CHECK (magnet IS NULL OR length(magnet) <= 16384),
        metainfo BLOB CHECK (metainfo IS NULL OR length(metainfo) <= 67108864),
        byte_length INTEGER NOT NULL CHECK (byte_length BETWEEN 1 AND 67108864),
        sha256 BLOB NOT NULL CHECK (length(sha256) = 32),
        CHECK (
            (kind = 'magnet' AND magnet IS NOT NULL AND metainfo IS NULL) OR
            (kind = 'metainfo' AND magnet IS NULL AND metainfo IS NOT NULL)
        )
     ) WITHOUT ROWID;
     CREATE TABLE torrent_trackers (
        info_hash BLOB NOT NULL
            REFERENCES torrents(info_hash) ON DELETE CASCADE,
        tier INTEGER NOT NULL CHECK (tier BETWEEN 0 AND 999993),
        position INTEGER NOT NULL CHECK (position BETWEEN 0 AND 999993),
        url TEXT NOT NULL CHECK (length(url) BETWEEN 1 AND 67108864),
        transport TEXT NOT NULL CHECK (transport IN ('udp', 'http', 'https')),
        source TEXT NOT NULL CHECK (source IN ('magnet', 'metainfo')),
        PRIMARY KEY (info_hash, tier, position),
        UNIQUE (info_hash, url)
     ) WITHOUT ROWID;
     CREATE TABLE torrent_peer_hints (
        info_hash BLOB NOT NULL
            REFERENCES torrents(info_hash) ON DELETE CASCADE,
        position INTEGER NOT NULL CHECK (position BETWEEN 0 AND 31),
        host TEXT NOT NULL CHECK (length(host) BETWEEN 1 AND 253),
        port INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
        source TEXT NOT NULL CHECK (source = 'magnet'),
        PRIMARY KEY (info_hash, position),
        UNIQUE (info_hash, host, port)
     ) WITHOUT ROWID;";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfiguredStorageRoot {
    pub id: String,
    pub label: String,
    pub location: StorageRootLocation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StorageRootLocation {
    Path(PathBuf),
    PlatformCapability,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedArtifactState {
    Legacy,
    None,
    Staging,
    Published,
}

impl ManagedArtifactState {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "legacy" => Some(Self::Legacy),
            "none" => Some(Self::None),
            "staging" => Some(Self::Staging),
            "published" => Some(Self::Published),
            _ => None,
        }
    }
}

impl ConfiguredStorageRoot {
    pub fn path(id: impl Into<String>, path: PathBuf) -> Self {
        let id = id.into();
        let label = path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or(&id)
            .to_owned();
        Self {
            id,
            label,
            location: StorageRootLocation::Path(path),
        }
    }

    pub fn platform(id: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            label: id.clone(),
            id,
            location: StorageRootLocation::PlatformCapability,
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredStorageRoot {
    pub id: String,
    pub label: String,
    pub location: StorageRootLocation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedFileRecord {
    pub file_index: usize,
    pub length: u64,
    pub sha1: [u8; 20],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoredTrackerTransport {
    Udp,
    Http,
    Https,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoredTrackerSource {
    Magnet,
    Metainfo,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredTracker {
    pub tier: u32,
    pub position: u32,
    pub url: String,
    pub transport: StoredTrackerTransport,
    pub source: StoredTrackerSource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResumeRecord {
    pub torrent_id: String,
    pub magnet: String,
    pub storage_root: String,
    pub skip_files: Vec<u32>,
    pub trackers: Vec<StoredTracker>,
    pub state: TorrentState,
    pub storage_state: StorageState,
    pub desired_running: bool,
    pub raw_info: Option<Vec<u8>>,
    pub publication_name: Option<String>,
    pub managed_artifacts: ManagedArtifactState,
    pub have: Option<HaveState>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemovalRecord {
    pub torrent_id: String,
    pub operation_id: String,
    pub storage_root: String,
    pub policy: RemovalDataPolicy,
    pub state: RemovalState,
    pub raw_info: Option<Vec<u8>>,
    pub publication_name: Option<String>,
    pub storage_state: StorageState,
    pub managed_artifacts: ManagedArtifactState,
    pub error: Option<String>,
}

pub struct SessionStore {
    connection: Connection,
    profile_id: String,
    database_path: Option<PathBuf>,
}

pub(crate) struct PreparedTorrentBytes {
    source: Vec<u8>,
    source_digest: [u8; 32],
    projection: MetainfoProjection,
    selection_default: FilePriority,
    selection_exceptions: Vec<u32>,
}

impl PreparedTorrentBytes {
    pub(crate) fn torrent_id(&self) -> String {
        encode_info_hash(self.projection.metainfo.info_hash)
    }
}

pub(crate) fn prepare_torrent_bytes(
    request: &AddTorrentBytesRequest,
    source: Vec<u8>,
) -> Result<PreparedTorrentBytes, (ErrorCode, String)> {
    validate_add_torrent_bytes_request(request)?;
    if source.len() != request.source_length as usize {
        return Err((
            ErrorCode::InvalidRequest,
            format!(
                "torrent source length {} does not match declared length {}",
                source.len(),
                request.source_length
            ),
        ));
    }
    let source_digest: [u8; 32] = Sha256::digest(&source).into();
    let projection = Metainfo::project_bytes_with_limits(&source, EXPLICIT_IMPORT_METAINFO_LIMITS)
        .map_err(metainfo_intake_error)?;
    let (selection_default, selection_exceptions) =
        project_file_selection(&request.selection, &projection.metainfo.files)?;
    Ok(PreparedTorrentBytes {
        source,
        source_digest,
        projection,
        selection_default,
        selection_exceptions,
    })
}

fn project_file_selection(
    selection: &FileSelectionIntent,
    files: &[rstorrent_protocol::metainfo::MetainfoFile],
) -> Result<(FilePriority, Vec<u32>), (ErrorCode, String)> {
    if let FileSelectionIntent::WantedRanges { ranges } = selection
        && ranges
            .last()
            .is_some_and(|range| range.end_exclusive as usize > files.len())
    {
        return Err((
            ErrorCode::InvalidRequest,
            "file selection range exceeds the torrent file catalog".to_owned(),
        ));
    }
    let mut exceptions = Vec::new();
    let ranges = match selection {
        FileSelectionIntent::All => return Ok((FilePriority::Normal, exceptions)),
        FileSelectionIntent::None => None,
        FileSelectionIntent::WantedRanges { ranges } => Some(ranges.as_slice()),
    };
    let mut range_index = 0;
    for (index, file) in files.iter().enumerate() {
        if file.padding {
            continue;
        }
        let index_u32 = u32::try_from(index).map_err(|_| {
            (
                ErrorCode::InvalidRequest,
                "torrent file index exceeds the supported range".to_owned(),
            )
        })?;
        let wanted = ranges.is_some_and(|ranges| {
            while ranges
                .get(range_index)
                .is_some_and(|range| range.end_exclusive <= index_u32)
            {
                range_index += 1;
            }
            ranges
                .get(range_index)
                .is_some_and(|range| range.start <= index_u32 && index_u32 < range.end_exclusive)
        });
        if wanted {
            exceptions.push(index_u32);
            if exceptions.len() > MAX_FILE_SELECTION_ENTRIES {
                return Err((
                    ErrorCode::ResourceLimit,
                    format!("file selection exceeds {MAX_FILE_SELECTION_ENTRIES} exceptions"),
                ));
            }
        }
    }
    Ok((FilePriority::Skip, exceptions))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DatabasePageUsage {
    pub(crate) page_size: u64,
    pub(crate) page_count: u64,
    pub(crate) maximum_page_count: u64,
}

impl fmt::Debug for SessionStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionStore")
            .field("profile_id", &self.profile_id)
            .field("database_path", &self.database_path)
            .finish_non_exhaustive()
    }
}

impl SessionStore {
    pub fn open(
        profile_root: &Path,
        profile_id: &str,
        storage_roots: &[ConfiguredStorageRoot],
    ) -> Result<Self, StoreError> {
        Self::open_with_initial_client_settings(
            profile_root,
            profile_id,
            storage_roots,
            &ClientSettings::default(),
        )
    }

    pub(crate) fn open_with_initial_client_settings(
        profile_root: &Path,
        profile_id: &str,
        storage_roots: &[ConfiguredStorageRoot],
        initial_client_settings: &ClientSettings,
    ) -> Result<Self, StoreError> {
        std::fs::create_dir_all(profile_root).map_err(|source| StoreError::Io {
            operation: "create profile directory",
            source,
        })?;
        let database_path = profile_root.join(DATABASE_FILENAME);
        let connection = Connection::open(&database_path)?;
        Self::initialize(
            connection,
            profile_id,
            storage_roots,
            Some(database_path),
            None,
            initial_client_settings,
        )
    }

    pub fn open_ephemeral(
        profile_id: &str,
        storage_roots: &[ConfiguredStorageRoot],
    ) -> Result<Self, StoreError> {
        Self::open_ephemeral_with_maximum_bytes(
            profile_id,
            storage_roots,
            EPHEMERAL_SESSION_MAX_BYTES,
        )
    }

    pub(crate) fn open_ephemeral_with_initial_client_settings(
        profile_id: &str,
        storage_roots: &[ConfiguredStorageRoot],
        initial_client_settings: &ClientSettings,
    ) -> Result<Self, StoreError> {
        Self::initialize(
            Connection::open_in_memory()?,
            profile_id,
            storage_roots,
            None,
            Some(EPHEMERAL_SESSION_MAX_BYTES),
            initial_client_settings,
        )
    }

    fn open_ephemeral_with_maximum_bytes(
        profile_id: &str,
        storage_roots: &[ConfiguredStorageRoot],
        maximum_bytes: u64,
    ) -> Result<Self, StoreError> {
        Self::initialize(
            Connection::open_in_memory()?,
            profile_id,
            storage_roots,
            None,
            Some(maximum_bytes),
            &ClientSettings::default(),
        )
    }

    fn initialize(
        mut connection: Connection,
        profile_id: &str,
        storage_roots: &[ConfiguredStorageRoot],
        database_path: Option<PathBuf>,
        ephemeral_maximum_bytes: Option<u64>,
        initial_client_settings: &ClientSettings,
    ) -> Result<Self, StoreError> {
        validate_identifier(
            profile_id,
            "profile ID",
            crate::control::MAX_PROFILE_ID_LENGTH,
        )
        .map_err(|(_, message)| StoreError::Configuration(message))?;
        connection.pragma_update(None, "foreign_keys", true)?;
        let foreign_keys: i64 =
            connection.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
        if foreign_keys != 1 {
            return Err(StoreError::RequiredPragma("foreign_keys"));
        }

        if let Some(maximum_bytes) = ephemeral_maximum_bytes {
            configure_ephemeral_connection(&connection, maximum_bytes)?;
        } else {
            connection.busy_timeout(BUSY_TIMEOUT)?;
            let journal_mode: String =
                connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
            if !journal_mode.eq_ignore_ascii_case("wal") {
                connection.pragma_update(None, "journal_mode", "WAL")?;
            }
            let journal_mode: String =
                connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
            if !journal_mode.eq_ignore_ascii_case("wal") {
                return Err(StoreError::RequiredPragma("journal_mode=WAL"));
            }
            connection.pragma_update(None, "synchronous", "FULL")?;
            let synchronous: i64 =
                connection.pragma_query_value(None, "synchronous", |row| row.get(0))?;
            if synchronous != 2 {
                return Err(StoreError::RequiredPragma("synchronous=FULL"));
            }
        }
        migrate(&mut connection, profile_id, initial_client_settings)?;
        register_storage_roots(&mut connection, storage_roots)?;

        let store = Self {
            connection,
            profile_id: profile_id.to_owned(),
            database_path,
        };
        if ephemeral_maximum_bytes.is_some() {
            let usage = store.page_usage()?;
            if usage.page_count > usage.maximum_page_count {
                return Err(StoreError::RequiredPragma("max_page_count"));
            }
        }
        Ok(store)
    }

    pub fn database_path(&self) -> Option<&Path> {
        self.database_path.as_deref()
    }

    pub(crate) fn page_usage(&self) -> Result<DatabasePageUsage, StoreError> {
        database_page_usage(&self.connection)
    }

    pub fn revision(&self) -> Result<u64, StoreError> {
        read_revision(&self.connection)
    }

    pub fn snapshot(&self) -> Result<ServiceSnapshot, StoreError> {
        read_snapshot(&self.connection, &self.profile_id)
    }

    pub fn client_settings(&self) -> Result<ClientSettings, StoreError> {
        read_client_settings(&self.connection).map_err(StoreError::from)
    }

    pub fn storage_roots(&self) -> Result<Vec<StoredStorageRoot>, StoreError> {
        read_storage_roots(&self.connection)
    }

    pub fn install_path_storage_root(
        &mut self,
        root_id: &str,
        label: &str,
        path: &Path,
    ) -> Result<(u64, String), StoreError> {
        validate_storage_root(root_id, label, path)?;
        let locator = path
            .to_str()
            .ok_or_else(|| {
                StoreError::Configuration("storage root path is not valid UTF-8".to_owned())
            })?
            .to_owned();
        if let Some(existing) = self
            .connection
            .query_row(
                "SELECT root_id FROM storage_roots
                 WHERE kind = 'path' AND locator = ?1",
                [&locator],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            return Ok((self.revision()?, existing));
        }
        let count: i64 =
            self.connection
                .query_row("SELECT COUNT(*) FROM storage_roots", [], |row| row.get(0))?;
        if count >= i64::try_from(MAX_STORAGE_ROOTS).expect("root bound fits i64") {
            return Err(StoreError::Configuration(format!(
                "storage root count exceeds {MAX_STORAGE_ROOTS}"
            )));
        }
        let transaction = self.connection.transaction()?;
        let revision = increment_revision(&transaction)?;
        transaction.execute(
            "INSERT INTO storage_roots(root_id, label, kind, locator)
             VALUES (?1, ?2, 'path', ?3)",
            params![root_id, label, locator],
        )?;
        transaction.execute(
            "UPDATE storage_settings SET default_root = ?1
             WHERE singleton = 1 AND default_root IS NULL",
            [root_id],
        )?;
        transaction.commit()?;
        Ok((revision, root_id.to_owned()))
    }

    pub fn repair_path_storage_root(
        &mut self,
        root_id: &str,
        label: &str,
        path: &Path,
    ) -> Result<u64, StoreError> {
        validate_storage_root(root_id, label, path)?;
        let locator = path
            .to_str()
            .ok_or_else(|| {
                StoreError::Configuration("storage root path is not valid UTF-8".to_owned())
            })?
            .to_owned();
        let duplicate = self
            .connection
            .query_row(
                "SELECT root_id FROM storage_roots
                 WHERE kind = 'path' AND locator = ?1 AND root_id <> ?2",
                params![locator, root_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if let Some(duplicate) = duplicate {
            return Err(StoreError::Configuration(format!(
                "selected folder is already registered as {duplicate}"
            )));
        }
        let transaction = self.connection.transaction()?;
        let revision = increment_revision(&transaction)?;
        let updated = transaction.execute(
            "UPDATE storage_roots
             SET label = ?2, kind = 'path', locator = ?3
             WHERE root_id = ?1",
            params![root_id, label, locator],
        )?;
        if updated != 1 {
            return Err(StoreError::Configuration(format!(
                "storage root {root_id} is not configured"
            )));
        }
        transaction.commit()?;
        Ok(revision)
    }

    pub fn load_dht_snapshot(&self) -> Result<Option<DhtSnapshot>, StoreError> {
        let state = self
            .connection
            .query_row(
                "SELECT format_version, node_id FROM dht_state WHERE singleton = 1",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )
            .optional()?;
        let Some((version, node_id)) = state else {
            return Ok(None);
        };
        let version = u32::try_from(version)
            .map_err(|_| StoreError::DurableState("invalid DHT format version".to_owned()))?;
        let node_id =
            NodeId(node_id.try_into().map_err(|_| {
                StoreError::DurableState("invalid persisted DHT node ID".to_owned())
            })?);
        let mut statement = self.connection.prepare(
            "SELECT family, node_id, address, port
             FROM dht_nodes ORDER BY family, sample_order",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        let mut nodes_v4 = Vec::new();
        let mut nodes_v6 = Vec::new();
        for row in rows {
            let (family, remote_id, address, port) = row?;
            let id = NodeId(remote_id.try_into().map_err(|_| {
                StoreError::DurableState("invalid saved DHT contact ID".to_owned())
            })?);
            let port = u16::try_from(port)
                .ok()
                .filter(|port| *port != 0)
                .ok_or_else(|| StoreError::DurableState("invalid saved DHT port".to_owned()))?;
            let ip = match family {
                4 => DhtIp::V4(<[u8; 4]>::try_from(address).map_err(|_| {
                    StoreError::DurableState("invalid saved DHT IPv4 address".to_owned())
                })?),
                6 => DhtIp::V6(<[u8; 16]>::try_from(address).map_err(|_| {
                    StoreError::DurableState("invalid saved DHT IPv6 address".to_owned())
                })?),
                _ => {
                    return Err(StoreError::DurableState(
                        "invalid saved DHT address family".to_owned(),
                    ));
                }
            };
            let contact = NodeContact {
                id,
                address: DhtEndpoint::new(ip, port),
            };
            if family == 4 {
                nodes_v4.push(contact);
            } else {
                nodes_v6.push(contact);
            }
        }
        DhtSnapshot {
            version,
            node_id,
            nodes_v4,
            nodes_v6,
        }
        .validate()
        .map(Some)
        .map_err(|error| StoreError::DurableState(error.to_string()))
    }

    pub fn save_dht_snapshot(&mut self, snapshot: DhtSnapshot) -> Result<(), StoreError> {
        let snapshot = snapshot
            .validate()
            .map_err(|error| StoreError::DurableState(error.to_string()))?;
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM dht_nodes", [])?;
        transaction.execute("DELETE FROM dht_state", [])?;
        transaction.execute(
            "INSERT INTO dht_state(singleton, format_version, node_id)
             VALUES (1, ?1, ?2)",
            params![i64::from(snapshot.version), snapshot.node_id.0.as_slice()],
        )?;
        for (family, nodes) in [(4_i64, snapshot.nodes_v4), (6_i64, snapshot.nodes_v6)] {
            for (order, node) in nodes.into_iter().enumerate() {
                let address = match node.address.ip {
                    DhtIp::V4(address) => address.to_vec(),
                    DhtIp::V6(address) => address.to_vec(),
                };
                transaction.execute(
                    "INSERT INTO dht_nodes(
                        family, sample_order, node_id, address, port
                     ) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        family,
                        i64::try_from(order).expect("DHT sample order is bounded"),
                        node.id.0.as_slice(),
                        address,
                        i64::from(node.address.port),
                    ],
                )?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn handle_durable(
        &mut self,
        request: &RequestEnvelope,
    ) -> Result<ResponseEnvelope, StoreError> {
        if let Err((code, message)) = validate_request(request) {
            return Ok(ResponseEnvelope::error(
                request.request_id.clone(),
                self.revision()?,
                code,
                message,
            ));
        }
        if !request.command.is_mutation() {
            return Ok(ResponseEnvelope::success(
                request.request_id.clone(),
                self.revision()?,
                self.snapshot()?,
            ));
        }

        let request_json = serde_json::to_string(request)?;
        let transaction = self.connection.transaction()?;
        if let Some((stored_request, stored_response)) = transaction
            .query_row(
                "SELECT request_json, response_json
                 FROM request_receipts
                 WHERE request_id = ?1",
                [&request.request_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
        {
            if stored_request == request_json {
                let response = serde_json::from_str(&stored_response)?;
                transaction.commit()?;
                return Ok(response);
            }
            let revision = read_revision(&transaction)?;
            transaction.commit()?;
            return Ok(ResponseEnvelope::error(
                request.request_id.clone(),
                revision,
                ErrorCode::RequestConflict,
                "request ID was already used for a different envelope",
            ));
        }

        let current_revision = read_revision(&transaction)?;
        let expected_revision = request
            .expected_revision
            .as_deref()
            .map(parse_revision)
            .transpose()
            .map_err(|(_, message)| StoreError::DurableState(message))?;
        let response = if expected_revision.is_some_and(|expected| expected != current_revision) {
            ResponseEnvelope::error(
                request.request_id.clone(),
                current_revision,
                ErrorCode::StaleRevision,
                format!(
                    "expected revision {}, current revision is {current_revision}",
                    request
                        .expected_revision
                        .as_deref()
                        .expect("checked expected revision is present")
                ),
            )
        } else {
            apply_mutation(&transaction, request, current_revision, &self.profile_id)?
        };

        let response_json = serde_json::to_string(&response)?;
        let response_revision = sql_revision(
            response
                .revision
                .parse()
                .map_err(|_| StoreError::DurableState("invalid response revision".to_owned()))?,
        )?;
        transaction.execute(
            "INSERT INTO request_receipts(
                request_id, request_json, response_json, revision
             ) VALUES (?1, ?2, ?3, ?4)",
            params![
                request.request_id,
                request_json,
                response_json,
                response_revision
            ],
        )?;
        transaction.execute(
            "DELETE FROM request_receipts
             WHERE receipt_order <= (
                SELECT COALESCE(MAX(receipt_order), 0) - ?1
                FROM request_receipts
             )",
            [MAX_RECEIPTS],
        )?;
        transaction.commit()?;
        Ok(response)
    }

    pub fn handle_torrent_bytes(
        &mut self,
        request: &AddTorrentBytesRequest,
        source: Vec<u8>,
    ) -> Result<ResponseEnvelope, StoreError> {
        match prepare_torrent_bytes(request, source) {
            Ok(prepared) => self.handle_prepared_torrent_bytes(request, &prepared),
            Err((code, message)) => Ok(ResponseEnvelope::error(
                request.request_id.clone(),
                self.revision()?,
                code,
                message,
            )),
        }
    }

    pub(crate) fn handle_prepared_torrent_bytes(
        &mut self,
        request: &AddTorrentBytesRequest,
        prepared: &PreparedTorrentBytes,
    ) -> Result<ResponseEnvelope, StoreError> {
        let request_json = torrent_bytes_receipt_json(request, &prepared.source_digest)?;
        if let Some(response) =
            replay_or_conflict(&self.connection, &request.request_id, &request_json)?
        {
            return Ok(response);
        }
        let transaction = self.connection.transaction()?;
        let current_revision = read_revision(&transaction)?;
        let expected_revision = request
            .expected_revision
            .as_deref()
            .map(parse_revision)
            .transpose()
            .map_err(|(_, message)| StoreError::DurableState(message))?;
        let response = if expected_revision.is_some_and(|expected| expected != current_revision) {
            ResponseEnvelope::error(
                request.request_id.clone(),
                current_revision,
                ErrorCode::StaleRevision,
                format!(
                    "expected revision {}, current revision is {current_revision}",
                    request
                        .expected_revision
                        .as_deref()
                        .expect("checked expected revision is present")
                ),
            )
        } else {
            match add_torrent_bytes(&transaction, request, prepared, current_revision) {
                Ok((revision, result)) => ResponseEnvelope::success(
                    request.request_id.clone(),
                    revision,
                    read_snapshot(&transaction, &self.profile_id)?,
                )
                .with_result(CommandResult::AddTorrent { result }),
                Err(AddTorrentBytesError::Response(code, message)) => ResponseEnvelope::error(
                    request.request_id.clone(),
                    current_revision,
                    code,
                    message,
                ),
                Err(AddTorrentBytesError::Store(error)) => return Err(error),
            }
        };
        insert_receipt(&transaction, &request.request_id, &request_json, &response)?;
        transaction.commit()?;
        Ok(response)
    }

    pub fn load_resume(&self, torrent_id: &str) -> Result<ResumeRecord, StoreError> {
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        let row = self
            .connection
            .query_row(
                "SELECT magnet, storage_root, state, storage_state, raw_info,
                        publication_name, piece_count, have_state, desired_state,
                        managed_artifacts
                 FROM torrents
                WHERE info_hash = ?1",
                [info_hash.as_slice()],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<Vec<u8>>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<i64>>(6)?,
                        row.get::<_, Option<Vec<u8>>>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, String>(9)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| StoreError::UnknownTorrent(torrent_id.to_owned()))?;
        let state = TorrentState::parse(&row.2)
            .ok_or_else(|| StoreError::DurableState("invalid torrent state".to_owned()))?;
        let storage_state = StorageState::parse(&row.3)
            .ok_or_else(|| StoreError::DurableState("invalid storage state".to_owned()))?;
        let managed_artifacts = ManagedArtifactState::parse(&row.9)
            .ok_or_else(|| StoreError::DurableState("invalid managed artifact state".to_owned()))?;
        let skip_files = read_selection(&self.connection, &info_hash)?;
        let trackers = read_trackers(&self.connection, &info_hash)?;
        let have = match (row.6, row.7) {
            (None, None) => None,
            (Some(piece_count), Some(bytes)) => {
                let piece_count = bounded_piece_count(piece_count)?;
                Some(HaveState::decode(&bytes, info_hash, piece_count)?)
            }
            _ => {
                return Err(StoreError::DurableState(
                    "piece count and have state must appear together".to_owned(),
                ));
            }
        };
        let operational_magnet = row
            .0
            .unwrap_or_else(|| format!("magnet:?xt=urn:btih:{}", encode_info_hash(info_hash)));
        Ok(ResumeRecord {
            torrent_id: torrent_id.to_ascii_lowercase(),
            magnet: operational_magnet,
            storage_root: row.1,
            skip_files,
            trackers,
            state,
            storage_state,
            desired_running: match row.8.as_str() {
                "running" => true,
                "paused" => false,
                _ => {
                    return Err(StoreError::DurableState(
                        "invalid desired torrent state".to_owned(),
                    ));
                }
            },
            raw_info: row.4,
            publication_name: row.5,
            managed_artifacts,
            have,
        })
    }

    pub fn load_removal(&self, torrent_id: &str) -> Result<RemovalRecord, StoreError> {
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        self.connection
            .query_row(
                "SELECT r.operation_id, t.storage_root, r.data_policy, r.state,
                        t.raw_info, t.publication_name, t.storage_state,
                        t.managed_artifacts, r.error
                 FROM removal_jobs r
                 JOIN torrents t ON t.info_hash = r.info_hash
                 WHERE r.info_hash = ?1",
                [info_hash.as_slice()],
                |row| {
                    Ok(RemovalRow {
                        operation_id: row.get(0)?,
                        storage_root: row.get(1)?,
                        data_policy: row.get(2)?,
                        state: row.get(3)?,
                        raw_info: row.get(4)?,
                        publication_name: row.get(5)?,
                        storage_state: row.get(6)?,
                        managed_artifacts: row.get(7)?,
                        error: row.get(8)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| StoreError::UnknownTorrent(torrent_id.to_owned()))
            .and_then(|row| removal_record(torrent_id, row))
    }

    pub fn load_removals(&self) -> Result<Vec<RemovalRecord>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT t.info_hash, r.operation_id, t.storage_root, r.data_policy,
                    r.state, t.raw_info, t.publication_name, t.storage_state,
                    t.managed_artifacts, r.error
             FROM removal_jobs r
             JOIN torrents t ON t.info_hash = r.info_hash
             ORDER BY t.info_hash",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                RemovalRow {
                    operation_id: row.get(1)?,
                    storage_root: row.get(2)?,
                    data_policy: row.get(3)?,
                    state: row.get(4)?,
                    raw_info: row.get(5)?,
                    publication_name: row.get(6)?,
                    storage_state: row.get(7)?,
                    managed_artifacts: row.get(8)?,
                    error: row.get(9)?,
                },
            ))
        })?;
        let mut removals = Vec::new();
        for row in rows {
            let row = row?;
            let info_hash: [u8; 20] = row
                .0
                .try_into()
                .map_err(|_| StoreError::DurableState("invalid info-hash length".to_owned()))?;
            removals.push(removal_record(&encode_info_hash(info_hash), row.1)?);
        }
        Ok(removals)
    }

    pub fn set_removal_state(
        &mut self,
        torrent_id: &str,
        operation_id: &str,
        state: RemovalState,
        error: Option<&str>,
    ) -> Result<u64, StoreError> {
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        let transaction = self.connection.transaction()?;
        let current = transaction
            .query_row(
                "SELECT state FROM removal_jobs
                 WHERE info_hash = ?1 AND operation_id = ?2",
                params![info_hash.as_slice(), operation_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| {
                StoreError::DurableState("removal operation is stale or unavailable".to_owned())
            })?;
        let current = RemovalState::parse(&current)
            .ok_or_else(|| StoreError::DurableState("invalid removal state".to_owned()))?;
        let transition_allowed = matches!(
            (current, state),
            (
                RemovalState::Pending,
                RemovalState::AwaitingPlatform | RemovalState::Failed
            ) | (RemovalState::AwaitingPlatform, RemovalState::Failed)
        );
        if !transition_allowed {
            return Err(StoreError::DurableState(format!(
                "invalid removal transition from {} to {}",
                current.as_str(),
                state.as_str()
            )));
        }
        let revision = increment_revision(&transaction)?;
        let updated = transaction.execute(
            "UPDATE removal_jobs
             SET state = ?3, error = ?4, updated_revision = ?5
             WHERE info_hash = ?1 AND operation_id = ?2",
            params![
                info_hash.as_slice(),
                operation_id,
                state.as_str(),
                error.map(bounded_error),
                sql_revision(revision)?,
            ],
        )?;
        if updated != 1 {
            return Err(StoreError::DurableState(
                "removal operation is stale or unavailable".to_owned(),
            ));
        }
        transaction.execute(
            "UPDATE torrents SET updated_revision = ?2 WHERE info_hash = ?1",
            params![info_hash.as_slice(), sql_revision(revision)?],
        )?;
        transaction.commit()?;
        Ok(revision)
    }

    pub fn finalize_removal(
        &mut self,
        torrent_id: &str,
        operation_id: &str,
    ) -> Result<u64, StoreError> {
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        let transaction = self.connection.transaction()?;
        let state = transaction
            .query_row(
                "SELECT state FROM removal_jobs
                 WHERE info_hash = ?1 AND operation_id = ?2",
                params![info_hash.as_slice(), operation_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| {
                StoreError::DurableState("removal operation is stale or unavailable".to_owned())
            })?;
        let state = RemovalState::parse(&state)
            .ok_or_else(|| StoreError::DurableState("invalid removal state".to_owned()))?;
        if state == RemovalState::Failed {
            return Err(StoreError::DurableState(
                "failed removal must be explicitly retried".to_owned(),
            ));
        }
        let revision = increment_revision(&transaction)?;
        let removed = transaction.execute(
            "DELETE FROM torrents WHERE info_hash = ?1",
            [info_hash.as_slice()],
        )?;
        if removed != 1 {
            return Err(StoreError::UnknownTorrent(torrent_id.to_owned()));
        }
        transaction.commit()?;
        Ok(revision)
    }

    pub fn record_metadata(
        &mut self,
        torrent_id: &str,
        raw_info: &[u8],
    ) -> Result<u64, StoreError> {
        validate_raw_info_length(raw_info)?;
        let expected_info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        let metainfo = parse_durable_metainfo(raw_info)?;
        if metainfo.info_hash != expected_info_hash {
            return Err(StoreError::DurableState(
                "verified metadata does not match torrent identity".to_owned(),
            ));
        }
        validate_publication_name(&metainfo.name)
            .map_err(|error| StoreError::DurableState(error.to_string()))?;
        let have = HaveState::empty(expected_info_hash, metainfo.piece_count())?.encode();
        validate_have_state_length(&have)?;
        let transaction = self.connection.transaction()?;
        let selection_default: String = transaction
            .query_row(
                "SELECT selection_default FROM torrents WHERE info_hash = ?1",
                [expected_info_hash.as_slice()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::UnknownTorrent(torrent_id.to_owned()))?;
        if selection_default == "skipped" {
            let ranges = read_pending_ranges(&transaction, &expected_info_hash)
                .map_err(|(_, message)| StoreError::DurableState(message))?;
            let mut selected = Vec::new();
            for range in ranges {
                let end = usize::try_from(range.end)
                    .unwrap_or(usize::MAX)
                    .min(metainfo.files.len().saturating_sub(1));
                let start = usize::try_from(range.start).unwrap_or(usize::MAX);
                if start > end {
                    continue;
                }
                for index in start..=end {
                    if metainfo.files[index].padding {
                        continue;
                    }
                    selected.push(index);
                    if selected.len() > MAX_FILE_SELECTION_ENTRIES {
                        return Err(StoreError::ResourceLimit {
                            resource: "file selection exceptions",
                            actual: selected.len(),
                            maximum: MAX_FILE_SELECTION_ENTRIES,
                        });
                    }
                }
            }
            for index in selected {
                transaction.execute(
                    "INSERT INTO file_selection(info_hash, file_index, wanted)
                     VALUES (?1, ?2, 1)",
                    params![
                        expected_info_hash.as_slice(),
                        i64::try_from(index).map_err(|_| StoreError::DurableState(
                            "file index overflow".to_owned()
                        ))?
                    ],
                )?;
            }
            transaction.execute(
                "DELETE FROM pending_selection_ranges WHERE info_hash = ?1",
                [expected_info_hash.as_slice()],
            )?;
        } else if selection_default != "wanted" {
            return Err(StoreError::DurableState(
                "invalid selection default".to_owned(),
            ));
        }
        let revision = increment_revision(&transaction)?;
        let revision_sql = sql_revision(revision)?;
        let updated = transaction.execute(
            "UPDATE torrents
             SET raw_info = ?2,
                 publication_name = ?3,
                 piece_count = ?4,
                 have_state = ?5,
                 state = CASE
                    WHEN desired_state = 'paused' OR (
                        selection_default = 'skipped' AND NOT EXISTS (
                            SELECT 1 FROM file_selection f
                            WHERE f.info_hash = torrents.info_hash
                              AND f.wanted = 1
                        )
                    ) THEN 'paused'
                    ELSE ?6
                 END,
                 storage_state = ?7,
                 error = NULL,
                 updated_revision = ?8
             WHERE info_hash = ?1",
            params![
                expected_info_hash.as_slice(),
                raw_info,
                metainfo.name,
                i64::try_from(metainfo.piece_count())
                    .map_err(|_| StoreError::DurableState("piece count overflow".to_owned()))?,
                have,
                TorrentState::Downloading.as_str(),
                StorageState::None.as_str(),
                revision_sql,
            ],
        )?;
        if updated != 1 {
            return Err(StoreError::UnknownTorrent(torrent_id.to_owned()));
        }
        transaction.commit()?;
        Ok(revision)
    }

    pub fn record_piece(
        &mut self,
        torrent_id: &str,
        piece_index: usize,
    ) -> Result<u64, StoreError> {
        self.record_pieces(torrent_id, &[piece_index])
    }

    pub fn record_pieces(
        &mut self,
        torrent_id: &str,
        piece_indices: &[usize],
    ) -> Result<u64, StoreError> {
        if piece_indices.is_empty() {
            return Err(StoreError::DurableState(
                "durable piece batch must be nonempty".to_owned(),
            ));
        }
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        let transaction = self.connection.transaction()?;
        let (piece_count, bytes) = read_have_columns(&transaction, &info_hash)?;
        let mut have = HaveState::decode(&bytes, info_hash, piece_count)?;
        for &piece_index in piece_indices {
            have.set(piece_index, true)?;
        }
        let revision = increment_revision(&transaction)?;
        let revision_sql = sql_revision(revision)?;
        transaction.execute(
            "UPDATE torrents
             SET have_state = ?2,
                 updated_revision = ?3
             WHERE info_hash = ?1",
            params![info_hash.as_slice(), have.encode(), revision_sql],
        )?;
        transaction.commit()?;
        Ok(revision)
    }

    pub fn replace_have(&mut self, torrent_id: &str, have: &HaveState) -> Result<u64, StoreError> {
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        if have.info_hash() != info_hash {
            return Err(StoreError::DurableState(
                "replacement have state has the wrong identity".to_owned(),
            ));
        }
        let encoded = have.encode();
        validate_have_state_length(&encoded)?;
        let transaction = self.connection.transaction()?;
        let (piece_count, _) = read_have_columns(&transaction, &info_hash)?;
        if have.pieces().len() != piece_count {
            return Err(StoreError::DurableState(
                "replacement have state has the wrong piece count".to_owned(),
            ));
        }
        let revision = increment_revision(&transaction)?;
        let revision_sql = sql_revision(revision)?;
        transaction.execute(
            "UPDATE torrents
             SET have_state = ?2, updated_revision = ?3
             WHERE info_hash = ?1",
            params![info_hash.as_slice(), encoded, revision_sql],
        )?;
        transaction.commit()?;
        Ok(revision)
    }

    pub fn begin_recheck(&mut self, torrent_id: &str) -> Result<u64, StoreError> {
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        let transaction = self.connection.transaction()?;
        let revision = increment_revision(&transaction)?;
        let revision_sql = sql_revision(revision)?;
        let updated = transaction.execute(
            "UPDATE torrents
             SET state = ?2, error = NULL, updated_revision = ?3
             WHERE info_hash = ?1 AND raw_info IS NOT NULL
                   AND have_state IS NOT NULL",
            params![
                info_hash.as_slice(),
                TorrentState::Checking.as_str(),
                revision_sql,
            ],
        )?;
        if updated != 1 {
            return Err(StoreError::DurableState(
                "recheck requires verified metadata and have state".to_owned(),
            ));
        }
        transaction.commit()?;
        Ok(revision)
    }

    pub fn complete_recheck(
        &mut self,
        torrent_id: &str,
        have: &HaveState,
    ) -> Result<u64, StoreError> {
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        if have.info_hash() != info_hash {
            return Err(StoreError::DurableState(
                "replacement have state has the wrong identity".to_owned(),
            ));
        }
        let transaction = self.connection.transaction()?;
        let (piece_count, _) = read_have_columns(&transaction, &info_hash)?;
        if have.pieces().len() != piece_count {
            return Err(StoreError::DurableState(
                "replacement have state has the wrong piece count".to_owned(),
            ));
        }
        let revision = increment_revision(&transaction)?;
        let revision_sql = sql_revision(revision)?;
        transaction.execute(
            "UPDATE torrents
             SET have_state = ?2,
                 state = CASE
                    WHEN desired_state = 'paused' THEN 'paused'
                    ELSE 'downloading'
                 END,
                 error = NULL,
                 updated_revision = ?3
             WHERE info_hash = ?1",
            params![info_hash.as_slice(), have.encode(), revision_sql],
        )?;
        transaction.commit()?;
        Ok(revision)
    }

    pub fn mark_complete(&mut self, torrent_id: &str) -> Result<u64, StoreError> {
        self.update_status(
            torrent_id,
            TorrentState::Complete,
            StorageState::Published,
            None,
        )
    }

    pub fn mark_storage_prepared(
        &mut self,
        torrent_id: &str,
        storage_state: StorageState,
    ) -> Result<u64, StoreError> {
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        let transaction = self.connection.transaction()?;
        let revision = increment_revision(&transaction)?;
        let revision_sql = sql_revision(revision)?;
        let updated = transaction.execute(
            "UPDATE torrents
             SET storage_state = ?2,
                 managed_artifacts = CASE
                    WHEN ?2 = 'published' THEN 'published'
                    ELSE 'staging'
                 END,
                 updated_revision = ?3
             WHERE info_hash = ?1",
            params![info_hash.as_slice(), storage_state.as_str(), revision_sql],
        )?;
        if updated != 1 {
            return Err(StoreError::UnknownTorrent(torrent_id.to_owned()));
        }
        transaction.commit()?;
        Ok(revision)
    }

    pub fn mark_publication_prepared(&mut self, torrent_id: &str) -> Result<u64, StoreError> {
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        let transaction = self.connection.transaction()?;
        let (storage_state, managed_artifacts, updated_revision) = transaction
            .query_row(
                "SELECT storage_state, managed_artifacts, updated_revision
                 FROM torrents WHERE info_hash = ?1",
                [info_hash.as_slice()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| StoreError::UnknownTorrent(torrent_id.to_owned()))?;
        let storage_state = StorageState::parse(&storage_state)
            .ok_or_else(|| StoreError::DurableState("invalid storage state".to_owned()))?;
        let managed_artifacts = ManagedArtifactState::parse(&managed_artifacts)
            .ok_or_else(|| StoreError::DurableState("invalid managed artifact state".to_owned()))?;
        if storage_state == StorageState::Prepared {
            return u64::try_from(updated_revision)
                .map_err(|_| StoreError::DurableState("torrent revision is invalid".to_owned()));
        }
        if storage_state != StorageState::Staging
            || managed_artifacts != ManagedArtifactState::Staging
        {
            return Err(StoreError::DurableState(
                "path publication requires owned durable staging data".to_owned(),
            ));
        }
        let revision = increment_revision(&transaction)?;
        let revision_sql = sql_revision(revision)?;
        transaction.execute(
            "UPDATE torrents
             SET state = ?2, storage_state = ?3, error = NULL,
                 updated_revision = ?4
             WHERE info_hash = ?1",
            params![
                info_hash.as_slice(),
                TorrentState::AwaitingPublication.as_str(),
                StorageState::Prepared.as_str(),
                revision_sql,
            ],
        )?;
        transaction.commit()?;
        Ok(revision)
    }

    pub fn mark_awaiting_storage(
        &mut self,
        torrent_id: &str,
        message: Option<&str>,
    ) -> Result<u64, StoreError> {
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        let error = message.map(bounded_error);
        let transaction = self.connection.transaction()?;
        let revision = increment_revision(&transaction)?;
        let revision_sql = sql_revision(revision)?;
        let updated = transaction.execute(
            "UPDATE torrents
             SET state = CASE
                    WHEN desired_state = 'paused' THEN 'paused'
                    ELSE ?2
                 END,
                 error = ?3,
                 updated_revision = ?4
             WHERE info_hash = ?1",
            params![
                info_hash.as_slice(),
                TorrentState::AwaitingStorage.as_str(),
                error,
                revision_sql,
            ],
        )?;
        if updated != 1 {
            return Err(StoreError::UnknownTorrent(torrent_id.to_owned()));
        }
        transaction.commit()?;
        Ok(revision)
    }

    pub fn record_prepared_files(
        &mut self,
        torrent_id: &str,
        files: &[PreparedFileHash],
    ) -> Result<u64, StoreError> {
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        let raw_info = self
            .connection
            .query_row(
                "SELECT raw_info FROM torrents WHERE info_hash = ?1",
                [info_hash.as_slice()],
                |row| row.get::<_, Option<Vec<u8>>>(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::UnknownTorrent(torrent_id.to_owned()))?
            .ok_or_else(|| {
                StoreError::DurableState("torrent has no verified metadata".to_owned())
            })?;
        let metainfo = parse_durable_metainfo(&raw_info)?;
        let skip_files = read_selection(&self.connection, &info_hash)?
            .into_iter()
            .map(|index| index as usize)
            .collect::<Vec<_>>();
        let plan = plan_descriptor_storage(&metainfo, &skip_files, &[])
            .map_err(|error| StoreError::DurableState(error.to_string()))?;
        let wanted = plan
            .files
            .iter()
            .filter(|file| matches!(file.role, rstorrent_engine::DescriptorFileRole::Wanted))
            .map(|file| (file.file_index, file.length))
            .collect::<Vec<_>>();
        if files.len() != wanted.len() {
            return Err(StoreError::DurableState(
                "prepared file manifest does not cover the selected files".to_owned(),
            ));
        }
        let mut ordered = files.to_vec();
        ordered.sort_by_key(|file| file.file_index);
        for (prepared, (expected_index, expected_length)) in ordered.iter().zip(wanted) {
            if prepared.file_index != expected_index || prepared.length != expected_length {
                return Err(StoreError::DurableState(
                    "prepared file manifest does not match the selected layout".to_owned(),
                ));
            }
        }

        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM prepared_files WHERE info_hash = ?1",
            [info_hash.as_slice()],
        )?;
        for file in &ordered {
            transaction.execute(
                "INSERT INTO prepared_files(info_hash, file_index, length, sha1)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    info_hash.as_slice(),
                    i64::try_from(file.file_index).map_err(|_| {
                        StoreError::DurableState("prepared file index overflow".to_owned())
                    })?,
                    i64::try_from(file.length).map_err(|_| {
                        StoreError::DurableState("prepared file length overflow".to_owned())
                    })?,
                    file.sha1.as_slice(),
                ],
            )?;
        }
        let revision = increment_revision(&transaction)?;
        let revision_sql = sql_revision(revision)?;
        transaction.execute(
            "UPDATE torrents
             SET state = ?2, storage_state = ?3,
                 managed_artifacts = 'published', error = NULL,
                 updated_revision = ?4
             WHERE info_hash = ?1",
            params![
                info_hash.as_slice(),
                TorrentState::AwaitingPublication.as_str(),
                StorageState::Prepared.as_str(),
                revision_sql,
            ],
        )?;
        transaction.commit()?;
        Ok(revision)
    }

    pub fn load_prepared_files(
        &self,
        torrent_id: &str,
    ) -> Result<Vec<PreparedFileRecord>, StoreError> {
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        let mut statement = self.connection.prepare(
            "SELECT file_index, length, sha1
             FROM prepared_files WHERE info_hash = ?1 ORDER BY file_index",
        )?;
        let rows = statement.query_map([info_hash.as_slice()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })?;
        let mut files = Vec::new();
        for row in rows {
            let (file_index, length, sha1) = row?;
            files.push(PreparedFileRecord {
                file_index: usize::try_from(file_index).map_err(|_| {
                    StoreError::DurableState("prepared file index is invalid".to_owned())
                })?,
                length: u64::try_from(length).map_err(|_| {
                    StoreError::DurableState("prepared file length is invalid".to_owned())
                })?,
                sha1: sha1.try_into().map_err(|_| {
                    StoreError::DurableState("prepared file hash is invalid".to_owned())
                })?,
            });
        }
        Ok(files)
    }

    pub fn confirm_prepared_publication(&mut self, torrent_id: &str) -> Result<u64, StoreError> {
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        let transaction = self.connection.transaction()?;
        let (state, storage_state, updated_revision) = transaction
            .query_row(
                "SELECT state, storage_state, updated_revision
                 FROM torrents WHERE info_hash = ?1",
                [info_hash.as_slice()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| StoreError::UnknownTorrent(torrent_id.to_owned()))?;
        if TorrentState::parse(&state) == Some(TorrentState::Complete)
            && StorageState::parse(&storage_state) == Some(StorageState::Published)
        {
            return u64::try_from(updated_revision)
                .map_err(|_| StoreError::DurableState("torrent revision is invalid".to_owned()));
        }
        if TorrentState::parse(&state) != Some(TorrentState::AwaitingPublication) {
            return Err(StoreError::DurableState(
                "torrent is not awaiting publication".to_owned(),
            ));
        }
        let manifest_count: i64 = transaction.query_row(
            "SELECT count(*) FROM prepared_files WHERE info_hash = ?1",
            [info_hash.as_slice()],
            |row| row.get(0),
        )?;
        if manifest_count == 0 {
            return Err(StoreError::DurableState(
                "prepared publication manifest is empty".to_owned(),
            ));
        }
        let revision = increment_revision(&transaction)?;
        let revision_sql = sql_revision(revision)?;
        transaction.execute(
            "UPDATE torrents
             SET state = ?2, storage_state = ?3, error = NULL,
                 updated_revision = ?4
             WHERE info_hash = ?1",
            params![
                info_hash.as_slice(),
                TorrentState::Complete.as_str(),
                StorageState::Published.as_str(),
                revision_sql,
            ],
        )?;
        transaction.execute(
            "DELETE FROM prepared_files WHERE info_hash = ?1",
            [info_hash.as_slice()],
        )?;
        transaction.commit()?;
        Ok(revision)
    }

    pub fn begin_published_recheck(&mut self, torrent_id: &str) -> Result<u64, StoreError> {
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        let transaction = self.connection.transaction()?;
        let (state, storage_state, updated_revision) = transaction
            .query_row(
                "SELECT state, storage_state, updated_revision
                 FROM torrents WHERE info_hash = ?1",
                [info_hash.as_slice()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| StoreError::UnknownTorrent(torrent_id.to_owned()))?;
        let state = TorrentState::parse(&state)
            .ok_or_else(|| StoreError::DurableState("invalid torrent state".to_owned()))?;
        let storage_state = StorageState::parse(&storage_state)
            .ok_or_else(|| StoreError::DurableState("invalid storage state".to_owned()))?;
        if storage_state == StorageState::Published
            && matches!(
                state,
                TorrentState::Checking
                    | TorrentState::Downloading
                    | TorrentState::Paused
                    | TorrentState::Complete
            )
        {
            return u64::try_from(updated_revision)
                .map_err(|_| StoreError::DurableState("torrent revision is invalid".to_owned()));
        }
        if state != TorrentState::AwaitingPublication || storage_state != StorageState::Prepared {
            return Err(StoreError::DurableState(
                "torrent is not awaiting a prepared publication".to_owned(),
            ));
        }
        let manifest_count: i64 = transaction.query_row(
            "SELECT count(*) FROM prepared_files WHERE info_hash = ?1",
            [info_hash.as_slice()],
            |row| row.get(0),
        )?;
        if manifest_count == 0 {
            return Err(StoreError::DurableState(
                "prepared publication manifest is empty".to_owned(),
            ));
        }
        let revision = increment_revision(&transaction)?;
        let revision_sql = sql_revision(revision)?;
        transaction.execute(
            "UPDATE torrents
             SET state = ?2, storage_state = ?3,
                 managed_artifacts = 'published', error = NULL,
                 updated_revision = ?4
             WHERE info_hash = ?1",
            params![
                info_hash.as_slice(),
                TorrentState::Checking.as_str(),
                StorageState::Published.as_str(),
                revision_sql,
            ],
        )?;
        transaction.execute(
            "DELETE FROM prepared_files WHERE info_hash = ?1",
            [info_hash.as_slice()],
        )?;
        transaction.commit()?;
        Ok(revision)
    }

    pub fn reset_have_from_metadata(&mut self, torrent_id: &str) -> Result<HaveState, StoreError> {
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        let raw_info = self
            .connection
            .query_row(
                "SELECT raw_info FROM torrents WHERE info_hash = ?1",
                [info_hash.as_slice()],
                |row| row.get::<_, Option<Vec<u8>>>(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::UnknownTorrent(torrent_id.to_owned()))?
            .ok_or_else(|| {
                StoreError::DurableState("torrent has no verified metadata".to_owned())
            })?;
        let metainfo = parse_durable_metainfo(&raw_info)?;
        if metainfo.info_hash != info_hash {
            return Err(StoreError::DurableState(
                "stored metadata does not match torrent identity".to_owned(),
            ));
        }
        let have = HaveState::empty(info_hash, metainfo.piece_count())?;
        self.replace_have(torrent_id, &have)?;
        Ok(have)
    }

    pub fn mark_needs_repair(
        &mut self,
        torrent_id: &str,
        message: &str,
    ) -> Result<u64, StoreError> {
        self.update_status(
            torrent_id,
            TorrentState::NeedsRepair,
            StorageState::NeedsRepair,
            Some(message),
        )
    }

    pub fn mark_error(&mut self, torrent_id: &str, message: &str) -> Result<u64, StoreError> {
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        let error = bounded_error(message);
        let transaction = self.connection.transaction()?;
        let revision = increment_revision(&transaction)?;
        let revision_sql = sql_revision(revision)?;
        let updated = transaction.execute(
            "UPDATE torrents
             SET state = CASE
                    WHEN desired_state = 'paused' THEN 'paused'
                    ELSE 'error'
                 END,
                 error = ?2,
                 updated_revision = ?3
             WHERE info_hash = ?1",
            params![info_hash.as_slice(), error, revision_sql],
        )?;
        if updated != 1 {
            return Err(StoreError::UnknownTorrent(torrent_id.to_owned()));
        }
        transaction.commit()?;
        Ok(revision)
    }

    fn update_status(
        &mut self,
        torrent_id: &str,
        state: TorrentState,
        storage_state: StorageState,
        error: Option<&str>,
    ) -> Result<u64, StoreError> {
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        let error = error.map(bounded_error);
        let transaction = self.connection.transaction()?;
        let revision = increment_revision(&transaction)?;
        let revision_sql = sql_revision(revision)?;
        let updated = transaction.execute(
            "UPDATE torrents
             SET state = ?2, storage_state = ?3,
                 managed_artifacts = CASE
                    WHEN ?3 = 'published' THEN 'published'
                    ELSE managed_artifacts
                 END,
                 error = ?4,
                 updated_revision = ?5
             WHERE info_hash = ?1",
            params![
                info_hash.as_slice(),
                state.as_str(),
                storage_state.as_str(),
                error,
                revision_sql
            ],
        )?;
        if updated != 1 {
            return Err(StoreError::UnknownTorrent(torrent_id.to_owned()));
        }
        transaction.commit()?;
        Ok(revision)
    }
}

fn torrent_bytes_receipt_json(
    request: &AddTorrentBytesRequest,
    source_digest: &[u8; 32],
) -> Result<String, serde_json::Error> {
    let mut request_value = serde_json::to_value(request)?;
    request_value
        .as_object_mut()
        .expect("torrent byte request serializes as an object")
        .insert(
            "source_sha256".to_owned(),
            serde_json::Value::String(encode_digest(source_digest)),
        );
    serde_json::to_string(&serde_json::json!({
        "operation": "add_torrent_bytes",
        "request": request_value,
    }))
}

#[derive(Debug)]
pub enum StoreError {
    Io {
        operation: &'static str,
        source: std::io::Error,
    },
    Sqlite(rusqlite::Error),
    Json(serde_json::Error),
    Have(HaveError),
    UnsupportedSchema {
        actual: i64,
        maximum: i64,
    },
    RequiredPragma(&'static str),
    Configuration(String),
    UnknownTorrent(String),
    DurableState(String),
    ResourceLimit {
        resource: &'static str,
        actual: usize,
        maximum: usize,
    },
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::Sqlite(error) => write!(formatter, "session database: {error}"),
            Self::Json(error) => write!(formatter, "session JSON: {error}"),
            Self::Have(error) => write!(formatter, "verified-piece state: {error}"),
            Self::UnsupportedSchema { actual, maximum } => write!(
                formatter,
                "session schema {actual} is newer than supported version {maximum}"
            ),
            Self::RequiredPragma(pragma) => {
                write!(formatter, "session database could not enable {pragma}")
            }
            Self::Configuration(message) => write!(formatter, "session configuration: {message}"),
            Self::UnknownTorrent(torrent_id) => {
                write!(formatter, "torrent {torrent_id} is not in the profile")
            }
            Self::DurableState(message) => write!(formatter, "invalid durable state: {message}"),
            Self::ResourceLimit {
                resource,
                actual,
                maximum,
            } => write!(
                formatter,
                "durable {resource} {actual} exceeds limit {maximum}"
            ),
        }
    }
}

impl StoreError {
    pub fn is_resource_limit(&self) -> bool {
        matches!(self, Self::ResourceLimit { .. })
            || matches!(
                self,
                Self::Sqlite(rusqlite::Error::SqliteFailure(failure, _))
                    if failure.code == rusqlite::ErrorCode::DiskFull
            )
    }
}

impl Error for StoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Sqlite(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Have(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<HaveError> for StoreError {
    fn from(error: HaveError) -> Self {
        match error {
            HaveError::InvalidPieceCount { actual, maximum } => Self::ResourceLimit {
                resource: "piece count",
                actual,
                maximum,
            },
            error => Self::Have(error),
        }
    }
}

impl From<SettingsPersistenceError> for StoreError {
    fn from(error: SettingsPersistenceError) -> Self {
        match error {
            SettingsPersistenceError::Sqlite(error) => Self::Sqlite(error),
            SettingsPersistenceError::Corrupt(message) => Self::DurableState(message),
        }
    }
}

fn configure_ephemeral_connection(
    connection: &Connection,
    maximum_bytes: u64,
) -> Result<(), StoreError> {
    connection.pragma_update(None, "journal_mode", "MEMORY")?;
    let journal_mode: String =
        connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    if !journal_mode.eq_ignore_ascii_case("memory") {
        return Err(StoreError::RequiredPragma("journal_mode=MEMORY"));
    }

    connection.pragma_update(None, "synchronous", "OFF")?;
    let synchronous: i64 = connection.pragma_query_value(None, "synchronous", |row| row.get(0))?;
    if synchronous != 0 {
        return Err(StoreError::RequiredPragma("synchronous=OFF"));
    }

    connection.pragma_update(None, "temp_store", "MEMORY")?;
    let temp_store: i64 = connection.pragma_query_value(None, "temp_store", |row| row.get(0))?;
    if temp_store != 2 {
        return Err(StoreError::RequiredPragma("temp_store=MEMORY"));
    }

    let page_size = pragma_u64(connection, "page_size")?;
    let maximum_page_count = maximum_bytes / page_size;
    if maximum_page_count == 0 {
        return Err(StoreError::Configuration(
            "ephemeral session database maximum is smaller than one SQLite page".to_owned(),
        ));
    }
    let maximum_page_count = i64::try_from(maximum_page_count).map_err(|_| {
        StoreError::Configuration(
            "ephemeral session database page maximum exceeds SQLite i64".to_owned(),
        )
    })?;
    connection.pragma_update(None, "max_page_count", maximum_page_count)?;
    let configured: i64 =
        connection.pragma_query_value(None, "max_page_count", |row| row.get(0))?;
    if configured != maximum_page_count {
        return Err(StoreError::RequiredPragma("max_page_count"));
    }
    Ok(())
}

fn database_page_usage(connection: &Connection) -> Result<DatabasePageUsage, StoreError> {
    Ok(DatabasePageUsage {
        page_size: pragma_u64(connection, "page_size")?,
        page_count: pragma_u64(connection, "page_count")?,
        maximum_page_count: pragma_u64(connection, "max_page_count")?,
    })
}

fn pragma_u64(connection: &Connection, pragma: &str) -> Result<u64, StoreError> {
    let value: i64 = connection.pragma_query_value(None, pragma, |row| row.get(0))?;
    u64::try_from(value).map_err(|_| StoreError::DurableState(format!("negative SQLite {pragma}")))
}

fn migrate(
    connection: &mut Connection,
    profile_id: &str,
    initial_client_settings: &ClientSettings,
) -> Result<(), StoreError> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(StoreError::UnsupportedSchema {
            actual: version,
            maximum: SCHEMA_VERSION,
        });
    }
    if version == 0 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(
            "CREATE TABLE profile_state (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                profile_id TEXT NOT NULL UNIQUE,
                revision INTEGER NOT NULL CHECK (revision >= 0)
             );
             CREATE TABLE storage_roots (
                root_id TEXT PRIMARY KEY,
                label TEXT NOT NULL CHECK (length(label) BETWEEN 1 AND 256),
                kind TEXT NOT NULL CHECK (kind IN ('path', 'platform')),
                locator TEXT NOT NULL CHECK (length(locator) BETWEEN 1 AND 4096)
             );
             CREATE TABLE storage_settings (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                default_root TEXT
                    REFERENCES storage_roots(root_id)
                    ON UPDATE CASCADE ON DELETE SET NULL,
                show_add_options INTEGER NOT NULL DEFAULT 1
                    CHECK (show_add_options IN (0, 1))
             );
             CREATE TABLE torrents (
                info_hash BLOB PRIMARY KEY CHECK (length(info_hash) = 20),
                magnet TEXT CHECK (magnet IS NULL OR length(magnet) <= 16384),
                storage_root TEXT NOT NULL
                    REFERENCES storage_roots(root_id) ON UPDATE CASCADE,
                desired_state TEXT NOT NULL
                    CHECK (desired_state IN ('running', 'paused')),
                state TEXT NOT NULL CHECK (
                    state IN (
                        'awaiting_metadata', 'awaiting_storage', 'checking',
                        'downloading', 'awaiting_publication', 'paused',
                        'complete', 'needs_repair', 'error'
                    )
                ),
                storage_state TEXT NOT NULL CHECK (
                    storage_state IN (
                        'none', 'staging', 'prepared', 'published',
                        'needs_repair'
                    )
                ),
                raw_info BLOB CHECK (
                    raw_info IS NULL OR length(raw_info) <= 67108864
                ),
                publication_name TEXT CHECK (
                    publication_name IS NULL OR
                    length(publication_name) BETWEEN 1 AND 255
                ),
                managed_artifacts TEXT NOT NULL DEFAULT 'none' CHECK (
                    managed_artifacts IN (
                        'legacy', 'none', 'staging', 'published'
                    )
                ),
                piece_count INTEGER CHECK (
                    piece_count IS NULL OR
                    (piece_count > 0 AND piece_count <= 2097152)
                ),
                have_state BLOB CHECK (
                    have_state IS NULL OR length(have_state) <= 262178
                ),
                error TEXT CHECK (error IS NULL OR length(error) <= 1024),
                archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
                selection_default TEXT NOT NULL DEFAULT 'wanted'
                    CHECK (selection_default IN ('wanted', 'skipped')),
                created_revision INTEGER NOT NULL,
                updated_revision INTEGER NOT NULL,
                CHECK (
                    (piece_count IS NULL AND have_state IS NULL) OR
                    (piece_count IS NOT NULL AND have_state IS NOT NULL)
                )
             );
             CREATE TABLE file_selection (
                info_hash BLOB NOT NULL
                    REFERENCES torrents(info_hash) ON DELETE CASCADE,
                file_index INTEGER NOT NULL
                    CHECK (file_index >= 0 AND file_index < 374998),
                wanted INTEGER NOT NULL CHECK (wanted IN (0, 1)),
                 PRIMARY KEY (info_hash, file_index)
             ) WITHOUT ROWID;
             CREATE TABLE pending_selection_ranges (
                info_hash BLOB NOT NULL
                    REFERENCES torrents(info_hash) ON DELETE CASCADE,
                range_start INTEGER NOT NULL CHECK (
                    range_start >= 0 AND range_start < 374998
                ),
                range_end INTEGER NOT NULL CHECK (
                    range_end >= range_start AND range_end < 374998
                ),
                PRIMARY KEY (info_hash, range_start)
             ) WITHOUT ROWID;
             CREATE TABLE prepared_files (
                info_hash BLOB NOT NULL
                    REFERENCES torrents(info_hash) ON DELETE CASCADE,
                file_index INTEGER NOT NULL
                    CHECK (file_index >= 0 AND file_index < 374998),
                length INTEGER NOT NULL CHECK (length >= 0),
                sha1 BLOB NOT NULL CHECK (length(sha1) = 20),
                PRIMARY KEY (info_hash, file_index)
             ) WITHOUT ROWID;
             CREATE TABLE request_receipts (
                receipt_order INTEGER PRIMARY KEY AUTOINCREMENT,
                request_id TEXT NOT NULL UNIQUE,
                request_json TEXT NOT NULL CHECK (length(request_json) <= 32768),
                response_json TEXT NOT NULL CHECK (length(response_json) <= 1048576),
                revision INTEGER NOT NULL CHECK (revision >= 0)
             );",
        )?;
        transaction.execute(
            "INSERT INTO profile_state(singleton, profile_id, revision)
             VALUES (1, ?1, 0)",
            [profile_id],
        )?;
        transaction.execute(
            "INSERT INTO storage_settings(singleton, default_root, show_add_options)
             VALUES (1, NULL, 1)",
            [],
        )?;
        create_client_settings(&transaction, initial_client_settings)?;
        transaction.execute_batch(DHT_TABLES_SQL)?;
        transaction.execute_batch(REMOVAL_TABLE_SQL)?;
        transaction.execute_batch(SOURCE_TABLES_SQL)?;
        transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        transaction.commit()?;
    }
    if version == 1 {
        let transaction = connection.transaction()?;
        transaction.pragma_update(None, "defer_foreign_keys", true)?;
        transaction.execute_batch(
            "ALTER TABLE file_selection RENAME TO file_selection_v1;
             ALTER TABLE torrents RENAME TO torrents_v1;
             CREATE TABLE torrents (
                info_hash BLOB PRIMARY KEY CHECK (length(info_hash) = 20),
                magnet TEXT NOT NULL CHECK (length(magnet) <= 16384),
                storage_root TEXT NOT NULL
                    REFERENCES storage_roots(root_id) ON UPDATE CASCADE,
                desired_state TEXT NOT NULL
                    CHECK (desired_state IN ('running', 'paused')),
                state TEXT NOT NULL CHECK (
                    state IN (
                        'awaiting_metadata', 'awaiting_storage', 'checking',
                        'downloading', 'awaiting_publication', 'paused',
                        'complete', 'needs_repair', 'error'
                    )
                ),
                storage_state TEXT NOT NULL CHECK (
                    storage_state IN (
                        'none', 'staging', 'prepared', 'published',
                        'needs_repair'
                    )
                ),
                raw_info BLOB CHECK (
                    raw_info IS NULL OR length(raw_info) <= 1048576
                ),
                publication_name TEXT CHECK (
                    publication_name IS NULL OR
                    length(publication_name) BETWEEN 1 AND 255
                ),
                piece_count INTEGER CHECK (
                    piece_count IS NULL OR
                    (piece_count > 0 AND piece_count <= 26214)
                ),
                have_state BLOB CHECK (
                    have_state IS NULL OR length(have_state) <= 3311
                ),
                error TEXT CHECK (error IS NULL OR length(error) <= 1024),
                archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
                created_revision INTEGER NOT NULL,
                updated_revision INTEGER NOT NULL,
                CHECK (
                    (piece_count IS NULL AND have_state IS NULL) OR
                    (piece_count IS NOT NULL AND have_state IS NOT NULL)
                )
             );
             INSERT INTO torrents(
                info_hash, magnet, storage_root, desired_state, state,
                storage_state, raw_info, piece_count, have_state, error,
                archived, created_revision, updated_revision
             ) SELECT
                info_hash, magnet, storage_root, desired_state, state,
                storage_state, raw_info, piece_count, have_state, error,
                0, created_revision, updated_revision
             FROM torrents_v1;
             CREATE TABLE file_selection (
                info_hash BLOB NOT NULL
                    REFERENCES torrents(info_hash) ON DELETE CASCADE,
                file_index INTEGER NOT NULL
                    CHECK (file_index >= 0 AND file_index < 4096),
                wanted INTEGER NOT NULL CHECK (wanted = 0),
                PRIMARY KEY (info_hash, file_index)
             ) WITHOUT ROWID;
             INSERT INTO file_selection
                SELECT * FROM file_selection_v1;
             CREATE TABLE prepared_files (
                info_hash BLOB NOT NULL
                    REFERENCES torrents(info_hash) ON DELETE CASCADE,
                file_index INTEGER NOT NULL
                    CHECK (file_index >= 0 AND file_index < 4096),
                length INTEGER NOT NULL CHECK (length >= 0),
                sha1 BLOB NOT NULL CHECK (length(sha1) = 20),
                PRIMARY KEY (info_hash, file_index)
             ) WITHOUT ROWID;
             DROP TABLE file_selection_v1;
             DROP TABLE torrents_v1;",
        )?;
        transaction.execute_batch(DHT_TABLES_SQL)?;
        transaction.execute_batch(REMOVAL_TABLE_SQL)?;
        transaction.pragma_update(None, "user_version", 4)?;
        transaction.commit()?;
    }
    if version == 2 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(DHT_TABLES_SQL)?;
        transaction.execute(
            "ALTER TABLE torrents ADD COLUMN archived INTEGER NOT NULL DEFAULT 0
             CHECK (archived IN (0, 1))",
            [],
        )?;
        transaction.execute_batch(REMOVAL_TABLE_SQL)?;
        transaction.pragma_update(None, "user_version", 4)?;
        transaction.commit()?;
    }
    if version == 3 {
        let transaction = connection.transaction()?;
        transaction.execute(
            "ALTER TABLE torrents ADD COLUMN archived INTEGER NOT NULL DEFAULT 0
             CHECK (archived IN (0, 1))",
            [],
        )?;
        transaction.execute_batch(REMOVAL_TABLE_SQL)?;
        transaction.pragma_update(None, "user_version", 4)?;
        transaction.commit()?;
    }
    if (1..=4).contains(&version) {
        let transaction = connection.transaction()?;
        transaction.execute(
            "ALTER TABLE storage_roots ADD COLUMN label TEXT NOT NULL DEFAULT 'Downloads'
             CHECK (length(label) BETWEEN 1 AND 256)",
            [],
        )?;
        transaction.execute(
            "ALTER TABLE storage_roots ADD COLUMN kind TEXT NOT NULL DEFAULT 'path'
             CHECK (kind IN ('path', 'platform'))",
            [],
        )?;
        transaction.execute(
            "UPDATE storage_roots
             SET label = root_id,
                 kind = CASE
                    WHEN locator = 'platform-capability:' THEN 'platform'
                    ELSE 'path'
                 END",
            [],
        )?;
        transaction.execute_batch(
            "CREATE TABLE storage_settings (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                default_root TEXT
                    REFERENCES storage_roots(root_id)
                    ON UPDATE CASCADE ON DELETE SET NULL,
                show_add_options INTEGER NOT NULL DEFAULT 1
                    CHECK (show_add_options IN (0, 1))
             );
             INSERT INTO storage_settings(singleton, default_root, show_add_options)
             VALUES (1, NULL, 1);",
        )?;
        transaction.pragma_update(None, "user_version", 5)?;
        transaction.commit()?;
    }
    if (1..=5).contains(&version) {
        let transaction = connection.transaction()?;
        if version != 1 {
            transaction.execute(
                "ALTER TABLE torrents ADD COLUMN publication_name TEXT
                 CHECK (
                    publication_name IS NULL OR
                    length(publication_name) BETWEEN 1 AND 255
                 )",
                [],
            )?;
        }
        transaction.execute(
            "ALTER TABLE torrents ADD COLUMN managed_artifacts TEXT NOT NULL
             DEFAULT 'legacy' CHECK (
                managed_artifacts IN (
                    'legacy', 'none', 'staging', 'published'
                )
             )",
            [],
        )?;
        transaction.pragma_update(None, "user_version", 6)?;
        transaction.commit()?;
    }
    if (1..=6).contains(&version) {
        migrate_piece_bounds_to_v7(connection)?;
    }
    if (1..=7).contains(&version) {
        migrate_sources_and_intake_bounds_to_v8(connection)?;
    }
    if (1..=8).contains(&version) {
        migrate_client_settings_to_v9(connection)?;
    }
    if (1..=9).contains(&version) {
        migrate_client_settings_to_v10_store(connection)?;
    }
    if (1..=10).contains(&version) {
        migrate_client_settings_to_v11_store(connection)?;
    }
    if (1..=11).contains(&version) {
        migrate_client_settings_to_v12_store(connection)?;
    }
    if (1..=12).contains(&version) {
        migrate_selection_to_v13(connection)?;
    }
    let stored_profile: String = connection.query_row(
        "SELECT profile_id FROM profile_state WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?;
    if stored_profile != profile_id {
        return Err(StoreError::Configuration(format!(
            "profile directory belongs to {stored_profile}, not {profile_id}"
        )));
    }
    read_client_settings(connection)?;
    Ok(())
}

fn migrate_piece_bounds_to_v7(connection: &mut Connection) -> Result<(), StoreError> {
    let transaction = connection.transaction()?;
    transaction.pragma_update(None, "defer_foreign_keys", true)?;
    transaction.execute_batch(
        "ALTER TABLE file_selection RENAME TO file_selection_v6;
         ALTER TABLE prepared_files RENAME TO prepared_files_v6;
         ALTER TABLE removal_jobs RENAME TO removal_jobs_v6;
         ALTER TABLE torrents RENAME TO torrents_v6;
         CREATE TABLE torrents (
            info_hash BLOB PRIMARY KEY CHECK (length(info_hash) = 20),
            magnet TEXT NOT NULL CHECK (length(magnet) <= 16384),
            storage_root TEXT NOT NULL
                REFERENCES storage_roots(root_id) ON UPDATE CASCADE,
            desired_state TEXT NOT NULL
                CHECK (desired_state IN ('running', 'paused')),
            state TEXT NOT NULL CHECK (
                state IN (
                    'awaiting_metadata', 'awaiting_storage', 'checking',
                    'downloading', 'awaiting_publication', 'paused',
                    'complete', 'needs_repair', 'error'
                )
            ),
            storage_state TEXT NOT NULL CHECK (
                storage_state IN (
                    'none', 'staging', 'prepared', 'published',
                    'needs_repair'
                )
            ),
            raw_info BLOB CHECK (
                raw_info IS NULL OR length(raw_info) <= 1048576
            ),
            publication_name TEXT CHECK (
                publication_name IS NULL OR
                length(publication_name) BETWEEN 1 AND 255
            ),
            managed_artifacts TEXT NOT NULL DEFAULT 'none' CHECK (
                managed_artifacts IN (
                    'legacy', 'none', 'staging', 'published'
                )
            ),
            piece_count INTEGER CHECK (
                piece_count IS NULL OR
                (piece_count > 0 AND piece_count <= 52428)
            ),
            have_state BLOB CHECK (
                have_state IS NULL OR length(have_state) <= 6588
            ),
            error TEXT CHECK (error IS NULL OR length(error) <= 1024),
            archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
            created_revision INTEGER NOT NULL,
            updated_revision INTEGER NOT NULL,
            CHECK (
                (piece_count IS NULL AND have_state IS NULL) OR
                (piece_count IS NOT NULL AND have_state IS NOT NULL)
            )
         );
         INSERT INTO torrents(
            info_hash, magnet, storage_root, desired_state, state,
            storage_state, raw_info, publication_name, managed_artifacts,
            piece_count, have_state, error, archived, created_revision,
            updated_revision
         ) SELECT
            info_hash, magnet, storage_root, desired_state, state,
            storage_state, raw_info, publication_name, managed_artifacts,
            piece_count, have_state, error, archived, created_revision,
            updated_revision
         FROM torrents_v6;
         CREATE TABLE file_selection (
            info_hash BLOB NOT NULL
                REFERENCES torrents(info_hash) ON DELETE CASCADE,
            file_index INTEGER NOT NULL
                CHECK (file_index >= 0 AND file_index < 4096),
            wanted INTEGER NOT NULL CHECK (wanted = 0),
            PRIMARY KEY (info_hash, file_index)
         ) WITHOUT ROWID;
         INSERT INTO file_selection SELECT * FROM file_selection_v6;
         CREATE TABLE prepared_files (
            info_hash BLOB NOT NULL
                REFERENCES torrents(info_hash) ON DELETE CASCADE,
            file_index INTEGER NOT NULL
                CHECK (file_index >= 0 AND file_index < 4096),
            length INTEGER NOT NULL CHECK (length >= 0),
            sha1 BLOB NOT NULL CHECK (length(sha1) = 20),
            PRIMARY KEY (info_hash, file_index)
         ) WITHOUT ROWID;
         INSERT INTO prepared_files SELECT * FROM prepared_files_v6;
         CREATE TABLE removal_jobs (
            info_hash BLOB PRIMARY KEY
                REFERENCES torrents(info_hash) ON DELETE CASCADE,
            operation_id TEXT NOT NULL UNIQUE
                CHECK (length(operation_id) BETWEEN 1 AND 128),
            data_policy TEXT NOT NULL
                CHECK (data_policy IN ('keep', 'delete_managed')),
            state TEXT NOT NULL
                CHECK (state IN ('pending', 'awaiting_platform', 'failed')),
            error TEXT CHECK (error IS NULL OR length(error) <= 1024),
            created_revision INTEGER NOT NULL CHECK (created_revision >= 0),
            updated_revision INTEGER NOT NULL CHECK (updated_revision >= 0)
         ) WITHOUT ROWID;
         INSERT INTO removal_jobs SELECT * FROM removal_jobs_v6;
         DROP TABLE file_selection_v6;
         DROP TABLE prepared_files_v6;
         DROP TABLE removal_jobs_v6;
         DROP TABLE torrents_v6;",
    )?;
    transaction.pragma_update(None, "user_version", 7)?;
    transaction.commit()?;
    Ok(())
}

fn migrate_sources_and_intake_bounds_to_v8(connection: &mut Connection) -> Result<(), StoreError> {
    let transaction = connection.transaction()?;
    transaction.pragma_update(None, "defer_foreign_keys", true)?;
    transaction.execute_batch(
        "DROP TABLE IF EXISTS torrent_peer_hints;
         DROP TABLE IF EXISTS torrent_trackers;
         DROP TABLE IF EXISTS torrent_source;
         ALTER TABLE file_selection RENAME TO file_selection_v7;
         ALTER TABLE prepared_files RENAME TO prepared_files_v7;
         ALTER TABLE removal_jobs RENAME TO removal_jobs_v7;
         ALTER TABLE torrents RENAME TO torrents_v7;
         CREATE TABLE torrents (
            info_hash BLOB PRIMARY KEY CHECK (length(info_hash) = 20),
            magnet TEXT CHECK (magnet IS NULL OR length(magnet) <= 16384),
            storage_root TEXT NOT NULL
                REFERENCES storage_roots(root_id) ON UPDATE CASCADE,
            desired_state TEXT NOT NULL
                CHECK (desired_state IN ('running', 'paused')),
            state TEXT NOT NULL CHECK (
                state IN (
                    'awaiting_metadata', 'awaiting_storage', 'checking',
                    'downloading', 'awaiting_publication', 'paused',
                    'complete', 'needs_repair', 'error'
                )
            ),
            storage_state TEXT NOT NULL CHECK (
                storage_state IN (
                    'none', 'staging', 'prepared', 'published',
                    'needs_repair'
                )
            ),
            raw_info BLOB CHECK (
                raw_info IS NULL OR length(raw_info) <= 67108864
            ),
            publication_name TEXT CHECK (
                publication_name IS NULL OR
                length(publication_name) BETWEEN 1 AND 255
            ),
            managed_artifacts TEXT NOT NULL DEFAULT 'none' CHECK (
                managed_artifacts IN ('legacy', 'none', 'staging', 'published')
            ),
            piece_count INTEGER CHECK (
                piece_count IS NULL OR
                (piece_count > 0 AND piece_count <= 2097152)
            ),
            have_state BLOB CHECK (
                have_state IS NULL OR length(have_state) <= 262178
            ),
            error TEXT CHECK (error IS NULL OR length(error) <= 1024),
            archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
            created_revision INTEGER NOT NULL,
            updated_revision INTEGER NOT NULL,
            CHECK (
                (piece_count IS NULL AND have_state IS NULL) OR
                (piece_count IS NOT NULL AND have_state IS NOT NULL)
            )
         );
         INSERT INTO torrents(
            info_hash, magnet, storage_root, desired_state, state,
            storage_state, raw_info, publication_name, managed_artifacts,
            piece_count, have_state, error, archived, created_revision,
            updated_revision
         ) SELECT
            info_hash, magnet, storage_root, desired_state, state,
            storage_state, raw_info, publication_name, managed_artifacts,
            piece_count, have_state, error, archived, created_revision,
            updated_revision
         FROM torrents_v7;
         CREATE TABLE file_selection (
            info_hash BLOB NOT NULL
                REFERENCES torrents(info_hash) ON DELETE CASCADE,
            file_index INTEGER NOT NULL
                CHECK (file_index >= 0 AND file_index < 374998),
            wanted INTEGER NOT NULL CHECK (wanted = 0),
            PRIMARY KEY (info_hash, file_index)
         ) WITHOUT ROWID;
         INSERT INTO file_selection SELECT * FROM file_selection_v7;
         CREATE TABLE prepared_files (
            info_hash BLOB NOT NULL
                REFERENCES torrents(info_hash) ON DELETE CASCADE,
            file_index INTEGER NOT NULL
                CHECK (file_index >= 0 AND file_index < 374998),
            length INTEGER NOT NULL CHECK (length >= 0),
            sha1 BLOB NOT NULL CHECK (length(sha1) = 20),
            PRIMARY KEY (info_hash, file_index)
         ) WITHOUT ROWID;
         INSERT INTO prepared_files SELECT * FROM prepared_files_v7;
         CREATE TABLE removal_jobs (
            info_hash BLOB PRIMARY KEY
                REFERENCES torrents(info_hash) ON DELETE CASCADE,
            operation_id TEXT NOT NULL UNIQUE
                CHECK (length(operation_id) BETWEEN 1 AND 128),
            data_policy TEXT NOT NULL
                CHECK (data_policy IN ('keep', 'delete_managed')),
            state TEXT NOT NULL
                CHECK (state IN ('pending', 'awaiting_platform', 'failed')),
            error TEXT CHECK (error IS NULL OR length(error) <= 1024),
            created_revision INTEGER NOT NULL CHECK (created_revision >= 0),
            updated_revision INTEGER NOT NULL CHECK (updated_revision >= 0)
         ) WITHOUT ROWID;
         INSERT INTO removal_jobs SELECT * FROM removal_jobs_v7;
         DROP TABLE file_selection_v7;
         DROP TABLE prepared_files_v7;
         DROP TABLE removal_jobs_v7;
         DROP TABLE torrents_v7;",
    )?;
    transaction.execute_batch(SOURCE_TABLES_SQL)?;

    let magnets = {
        let mut statement = transaction.prepare(
            "SELECT info_hash, magnet FROM torrents WHERE magnet IS NOT NULL ORDER BY info_hash",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for (info_hash, source) in magnets {
        let digest = Sha256::digest(source.as_bytes());
        transaction.execute(
            "INSERT INTO torrent_source(
                info_hash, kind, fidelity, magnet, metainfo, byte_length, sha256
             ) VALUES (?1, 'magnet', 'canonicalized', ?2, NULL, ?3, ?4)",
            params![
                info_hash.as_slice(),
                source,
                i64::try_from(source.len()).expect("magnet bound fits i64"),
                digest.as_slice(),
            ],
        )?;
        let magnet =
            Magnet::parse(&source).map_err(|error| StoreError::DurableState(error.to_string()))?;
        for (position, tracker) in magnet.trackers.iter().enumerate() {
            transaction.execute(
                "INSERT INTO torrent_trackers(
                    info_hash, tier, position, url, transport, source
                 ) VALUES (?1, 0, ?2, ?3, ?4, 'magnet')",
                params![
                    info_hash.as_slice(),
                    i64::try_from(position).expect("magnet tracker count is bounded"),
                    tracker.url(),
                    tracker_transport_name(tracker.transport()),
                ],
            )?;
        }
        for (position, hint) in magnet.peer_hints.iter().enumerate() {
            transaction.execute(
                "INSERT INTO torrent_peer_hints(
                    info_hash, position, host, port, source
                 ) VALUES (?1, ?2, ?3, ?4, 'magnet')",
                params![
                    info_hash.as_slice(),
                    i64::try_from(position).expect("magnet peer hint count is bounded"),
                    hint.host,
                    i64::from(hint.port),
                ],
            )?;
        }
    }
    transaction.pragma_update(None, "user_version", 8)?;
    transaction.commit()?;
    Ok(())
}

fn migrate_client_settings_to_v9(connection: &mut Connection) -> Result<(), StoreError> {
    let transaction = connection.transaction()?;
    create_client_settings(&transaction, &ClientSettings::default())?;
    transaction.pragma_update(None, "user_version", 9)?;
    transaction.commit()?;
    Ok(())
}

fn migrate_client_settings_to_v10_store(connection: &mut Connection) -> Result<(), StoreError> {
    let transaction = connection.transaction()?;
    migrate_client_settings_to_v10(&transaction)?;
    transaction.pragma_update(None, "user_version", 10)?;
    transaction.commit()?;
    Ok(())
}

fn migrate_client_settings_to_v11_store(connection: &mut Connection) -> Result<(), StoreError> {
    let transaction = connection.transaction()?;
    migrate_client_settings_to_v11(&transaction)?;
    transaction.pragma_update(None, "user_version", 11)?;
    transaction.commit()?;
    Ok(())
}

fn migrate_client_settings_to_v12_store(connection: &mut Connection) -> Result<(), StoreError> {
    let transaction = connection.transaction()?;
    migrate_client_settings_to_v12(&transaction)?;
    transaction.pragma_update(None, "user_version", 12)?;
    transaction.commit()?;
    Ok(())
}

fn migrate_selection_to_v13(connection: &mut Connection) -> Result<(), StoreError> {
    let transaction = connection.transaction()?;
    transaction.pragma_update(None, "defer_foreign_keys", true)?;
    let has_selection_default = {
        let mut statement = transaction.prepare("PRAGMA table_info(torrents)")?;
        let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
        columns
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .any(|name| name == "selection_default")
    };
    if !has_selection_default {
        transaction.execute_batch(
            "ALTER TABLE torrents ADD COLUMN selection_default TEXT NOT NULL
                DEFAULT 'wanted' CHECK (selection_default IN ('wanted', 'skipped'));",
        )?;
    }
    transaction.execute_batch(
        "DROP TABLE IF EXISTS pending_selection_ranges;
         ALTER TABLE file_selection RENAME TO file_selection_v12;
         CREATE TABLE file_selection (
            info_hash BLOB NOT NULL
                REFERENCES torrents(info_hash) ON DELETE CASCADE,
            file_index INTEGER NOT NULL
                CHECK (file_index >= 0 AND file_index < 374998),
            wanted INTEGER NOT NULL CHECK (wanted IN (0, 1)),
            PRIMARY KEY (info_hash, file_index)
         ) WITHOUT ROWID;
         INSERT INTO file_selection SELECT * FROM file_selection_v12;
         DROP TABLE file_selection_v12;
         CREATE TABLE pending_selection_ranges (
            info_hash BLOB NOT NULL
                REFERENCES torrents(info_hash) ON DELETE CASCADE,
            range_start INTEGER NOT NULL CHECK (
                range_start >= 0 AND range_start < 374998
            ),
            range_end INTEGER NOT NULL CHECK (
                range_end >= range_start AND range_end < 374998
            ),
            PRIMARY KEY (info_hash, range_start)
         ) WITHOUT ROWID;",
    )?;
    transaction.pragma_update(None, "user_version", 13)?;
    transaction.commit()?;
    Ok(())
}

fn validate_storage_root(root_id: &str, label: &str, path: &Path) -> Result<(), StoreError> {
    validate_identifier(root_id, "storage root", crate::control::MAX_ROOT_ID_LENGTH)
        .map_err(|(_, message)| StoreError::Configuration(message))?;
    validate_root_label(label)?;
    if !path.is_absolute() {
        return Err(StoreError::Configuration(
            "storage root path must be absolute".to_owned(),
        ));
    }
    let locator = path.to_str().ok_or_else(|| {
        StoreError::Configuration("storage root path is not valid UTF-8".to_owned())
    })?;
    validate_root_locator(locator)
}

fn validate_root_label(label: &str) -> Result<(), StoreError> {
    if label.is_empty() || label.len() > crate::control::MAX_ROOT_LABEL_LENGTH {
        return Err(StoreError::Configuration(format!(
            "storage root label must be 1..={} bytes",
            crate::control::MAX_ROOT_LABEL_LENGTH
        )));
    }
    Ok(())
}

fn validate_root_locator(locator: &str) -> Result<(), StoreError> {
    if locator.is_empty() || locator.len() > MAX_STORAGE_ROOT_LOCATOR_LENGTH {
        return Err(StoreError::Configuration(format!(
            "storage root locator must be 1..={MAX_STORAGE_ROOT_LOCATOR_LENGTH} bytes"
        )));
    }
    Ok(())
}

fn read_storage_roots(connection: &Connection) -> Result<Vec<StoredStorageRoot>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT root_id, label, kind, locator
         FROM storage_roots ORDER BY root_id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    let mut roots = Vec::new();
    for row in rows {
        let (id, label, kind, locator) = row?;
        validate_identifier(&id, "storage root", crate::control::MAX_ROOT_ID_LENGTH)
            .map_err(|(_, message)| StoreError::DurableState(message))?;
        validate_root_label(&label).map_err(|error| StoreError::DurableState(error.to_string()))?;
        validate_root_locator(&locator)
            .map_err(|error| StoreError::DurableState(error.to_string()))?;
        let location = match kind.as_str() {
            "path" => StorageRootLocation::Path(PathBuf::from(locator)),
            "platform" if locator == "platform-capability:" => {
                StorageRootLocation::PlatformCapability
            }
            _ => {
                return Err(StoreError::DurableState(
                    "invalid storage root kind or locator".to_owned(),
                ));
            }
        };
        roots.push(StoredStorageRoot {
            id,
            label,
            location,
        });
    }
    if roots.len() > MAX_STORAGE_ROOTS {
        return Err(StoreError::DurableState(format!(
            "storage root count exceeds {MAX_STORAGE_ROOTS}"
        )));
    }
    Ok(roots)
}

fn read_storage_settings(connection: &Connection) -> Result<StorageSettingsSnapshot, StoreError> {
    let (default_root, show_add_options) = connection.query_row(
        "SELECT default_root, show_add_options
         FROM storage_settings WHERE singleton = 1",
        [],
        |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, bool>(1)?)),
    )?;
    let roots = read_storage_roots(connection)?
        .into_iter()
        .map(|root| {
            let (display_path, availability) = match &root.location {
                StorageRootLocation::Path(path) => (
                    path.to_str().map(str::to_owned),
                    if path.is_dir() && std::fs::read_dir(path).is_ok() {
                        StorageRootAvailability::Available
                    } else {
                        StorageRootAvailability::Unavailable
                    },
                ),
                StorageRootLocation::PlatformCapability => {
                    (None, StorageRootAvailability::Available)
                }
            };
            StorageRootSnapshot {
                root_id: root.id,
                label: root.label,
                display_path,
                availability,
            }
        })
        .collect();
    Ok(StorageSettingsSnapshot {
        roots,
        default_root,
        show_add_options,
    })
}

fn register_storage_roots(
    connection: &mut Connection,
    storage_roots: &[ConfiguredStorageRoot],
) -> Result<(), StoreError> {
    if storage_roots.len() > MAX_STORAGE_ROOTS {
        return Err(StoreError::Configuration(format!(
            "configured storage root count exceeds {MAX_STORAGE_ROOTS}"
        )));
    }
    let transaction = connection.transaction()?;
    for root in storage_roots {
        validate_identifier(&root.id, "storage root", crate::control::MAX_ROOT_ID_LENGTH)
            .map_err(|(_, message)| StoreError::Configuration(message))?;
        validate_root_label(&root.label)?;
        let (kind, locator) = match &root.location {
            StorageRootLocation::Path(path) => (
                "path",
                path.to_str().ok_or_else(|| {
                    StoreError::Configuration(format!(
                        "storage root {} is not representable as UTF-8",
                        root.id
                    ))
                })?,
            ),
            StorageRootLocation::PlatformCapability => ("platform", "platform-capability:"),
        };
        validate_root_locator(locator)?;
        transaction.execute(
            "INSERT INTO storage_roots(root_id, label, kind, locator)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(root_id) DO UPDATE SET
                label = excluded.label,
                kind = excluded.kind,
                locator = excluded.locator",
            params![root.id, root.label, kind, locator],
        )?;
    }
    if let Some(first) = storage_roots.first() {
        transaction.execute(
            "UPDATE storage_settings SET default_root = ?1
             WHERE singleton = 1 AND default_root IS NULL",
            [&first.id],
        )?;
    }
    let count: i64 =
        transaction.query_row("SELECT COUNT(*) FROM storage_roots", [], |row| row.get(0))?;
    if count > i64::try_from(MAX_STORAGE_ROOTS).expect("root bound fits i64") {
        return Err(StoreError::Configuration(format!(
            "storage root count exceeds {MAX_STORAGE_ROOTS}"
        )));
    }
    transaction.commit()?;
    Ok(())
}

fn apply_mutation(
    transaction: &Transaction<'_>,
    request: &RequestEnvelope,
    current_revision: u64,
    profile_id: &str,
) -> Result<ResponseEnvelope, StoreError> {
    if let Command::SetClientSettings { settings } = &request.command {
        let changed = replace_client_settings(transaction, settings)?;
        let revision = if changed {
            next_revision_strict(transaction, current_revision)?
        } else {
            current_revision
        };
        return Ok(ResponseEnvelope::success(
            request.request_id.clone(),
            revision,
            read_snapshot(transaction, profile_id)?,
        ));
    }
    if let Command::AddMagnet {
        magnet,
        storage_root,
        start_content,
        skip_files,
    } = &request.command
    {
        return match add_magnet(
            transaction,
            magnet,
            storage_root,
            *start_content,
            skip_files,
            current_revision,
        ) {
            Ok((revision, result)) => Ok(ResponseEnvelope::success(
                request.request_id.clone(),
                revision,
                read_snapshot(transaction, profile_id)?,
            )
            .with_result(CommandResult::AddTorrent { result })),
            Err((code, message)) => Ok(ResponseEnvelope::error(
                request.request_id.clone(),
                current_revision,
                code,
                message,
            )),
        };
    }
    let result = match &request.command {
        Command::AddMagnet { .. } => unreachable!("magnet adds are handled above"),
        Command::SetFilePriority {
            torrent_id,
            file_indices,
            priority,
        } => set_file_priority(
            transaction,
            torrent_id,
            file_indices,
            *priority,
            current_revision,
        ),
        Command::SetFilePriorityRanges {
            torrent_id,
            ranges,
            priority,
        } => set_file_priority_indices(
            transaction,
            torrent_id,
            ranges
                .iter()
                .flat_map(|range| range.start..range.end_exclusive),
            *priority,
            current_revision,
        ),
        Command::SetDefaultStorageRoot { storage_root } => {
            set_default_storage_root(transaction, storage_root, current_revision)
        }
        Command::SetShowAddOptions { show } => {
            set_show_add_options(transaction, *show, current_revision)
        }
        Command::SetClientSettings { .. } => unreachable!("settings are handled atomically above"),
        Command::RemoveStorageRoot { storage_root } => {
            remove_storage_root(transaction, storage_root, current_revision)
        }
        Command::Pause { torrent_id } => {
            set_desired_state(transaction, torrent_id, false, current_revision)
        }
        Command::Resume { torrent_id } => {
            set_desired_state(transaction, torrent_id, true, current_revision)
        }
        Command::ForceRecheck { torrent_id } => {
            force_recheck(transaction, torrent_id, current_revision)
        }
        Command::Archive { torrent_id } => {
            set_archived(transaction, torrent_id, true, current_revision)
        }
        Command::RestoreArchive { torrent_id } => {
            set_archived(transaction, torrent_id, false, current_revision)
        }
        Command::RemoveTorrent { torrent_id, data } => {
            begin_removal(transaction, torrent_id, *data, current_revision)
        }
        Command::Snapshot | Command::Shutdown => {
            unreachable!("non-mutations are handled before transaction")
        }
    };
    match result {
        Ok(revision) => Ok(ResponseEnvelope::success(
            request.request_id.clone(),
            revision,
            read_snapshot(transaction, profile_id)?,
        )),
        Err((code, message)) => Ok(ResponseEnvelope::error(
            request.request_id.clone(),
            current_revision,
            code,
            message,
        )),
    }
}

enum AddTorrentBytesError {
    Response(ErrorCode, String),
    Store(StoreError),
}

fn add_torrent_bytes(
    transaction: &Transaction<'_>,
    request: &AddTorrentBytesRequest,
    prepared: &PreparedTorrentBytes,
    current_revision: u64,
) -> Result<(u64, AddTorrentResult), AddTorrentBytesError> {
    let source = &prepared.source;
    let source_digest = &prepared.source_digest;
    let projection = &prepared.projection;
    let selection_default = prepared.selection_default;
    let selection_exceptions = &prepared.selection_exceptions;
    let metainfo = &projection.metainfo;
    let torrent_id = encode_info_hash(metainfo.info_hash);
    let exists = transaction
        .query_row(
            "SELECT 1 FROM torrents WHERE info_hash = ?1",
            [metainfo.info_hash.as_slice()],
            |_| Ok(()),
        )
        .optional()
        .map_err(|error| AddTorrentBytesError::Store(StoreError::Sqlite(error)))?
        .is_some();
    if exists {
        return Ok((
            current_revision,
            add_result(
                torrent_id,
                AddTorrentDisposition::AlreadyPresent,
                current_revision,
            ),
        ));
    }
    let root_exists = transaction
        .query_row(
            "SELECT 1 FROM storage_roots WHERE root_id = ?1",
            [&request.storage_root],
            |_| Ok(()),
        )
        .optional()
        .map_err(|error| AddTorrentBytesError::Store(StoreError::Sqlite(error)))?
        .is_some();
    if !root_exists {
        return Err(AddTorrentBytesError::Response(
            ErrorCode::UnknownStorageRoot,
            format!("storage root {} is not configured", request.storage_root),
        ));
    }
    let have = HaveState::empty(metainfo.info_hash, metainfo.piece_count())
        .map_err(|error| {
            AddTorrentBytesError::Response(ErrorCode::ResourceLimit, error.to_string())
        })?
        .encode();
    let raw_info = &source[projection.info_span.clone()];
    let revision = current_revision.checked_add(1).ok_or_else(|| {
        AddTorrentBytesError::Response(ErrorCode::Internal, "profile revision overflow".to_owned())
    })?;
    let revision_sql = i64::try_from(revision).map_err(|_| {
        AddTorrentBytesError::Response(ErrorCode::Internal, "profile revision overflow".to_owned())
    })?;
    transaction
        .execute(
            "UPDATE profile_state SET revision = ?1 WHERE singleton = 1",
            [revision_sql],
        )
        .map_err(|error| AddTorrentBytesError::Store(StoreError::Sqlite(error)))?;
    transaction
        .execute(
            "INSERT INTO torrents(
                info_hash, magnet, storage_root, desired_state, state,
                storage_state, raw_info, publication_name, managed_artifacts,
                piece_count, have_state, created_revision, updated_revision,
                selection_default
             ) VALUES (
                ?1, NULL, ?2, ?3, ?4, 'none', ?5, ?6, 'none', ?7, ?8, ?9, ?9,
                ?10
             )",
            params![
                metainfo.info_hash.as_slice(),
                request.storage_root,
                if request.start_content {
                    "running"
                } else {
                    "paused"
                },
                if request.start_content {
                    TorrentState::AwaitingStorage.as_str()
                } else {
                    TorrentState::Paused.as_str()
                },
                raw_info,
                metainfo.name,
                i64::try_from(metainfo.piece_count()).map_err(|_| {
                    AddTorrentBytesError::Response(
                        ErrorCode::Internal,
                        "piece count overflows i64".to_owned(),
                    )
                })?,
                have,
                revision_sql,
                if selection_default == FilePriority::Normal {
                    "wanted"
                } else {
                    "skipped"
                },
            ],
        )
        .map_err(|error| AddTorrentBytesError::Store(StoreError::Sqlite(error)))?;
    transaction
        .execute(
            "INSERT INTO torrent_source(
                info_hash, kind, fidelity, magnet, metainfo, byte_length, sha256
             ) VALUES (?1, 'metainfo', 'verbatim', NULL, ?2, ?3, ?4)",
            params![
                metainfo.info_hash.as_slice(),
                source,
                i64::try_from(source.len()).map_err(|_| {
                    AddTorrentBytesError::Response(
                        ErrorCode::Internal,
                        "torrent source length overflows i64".to_owned(),
                    )
                })?,
                source_digest,
            ],
        )
        .map_err(|error| AddTorrentBytesError::Store(StoreError::Sqlite(error)))?;
    for tracker in &projection.trackers {
        let transport = match tracker.transport {
            MetainfoTrackerTransport::Udp => "udp",
            MetainfoTrackerTransport::Http => "http",
            MetainfoTrackerTransport::Https => "https",
        };
        transaction
            .execute(
                "INSERT INTO torrent_trackers(
                    info_hash, tier, position, url, transport, source
                 ) VALUES (?1, ?2, ?3, ?4, ?5, 'metainfo')",
                params![
                    metainfo.info_hash.as_slice(),
                    i64::from(tracker.tier),
                    i64::from(tracker.position),
                    tracker.url.as_str(),
                    transport,
                ],
            )
            .map_err(|error| AddTorrentBytesError::Store(StoreError::Sqlite(error)))?;
    }
    let exception_wanted = selection_default == FilePriority::Skip;
    for file_index in selection_exceptions {
        transaction
            .execute(
                "INSERT INTO file_selection(info_hash, file_index, wanted)
                 VALUES (?1, ?2, ?3)",
                params![
                    metainfo.info_hash.as_slice(),
                    i64::from(*file_index),
                    exception_wanted
                ],
            )
            .map_err(|error| AddTorrentBytesError::Store(StoreError::Sqlite(error)))?;
    }
    Ok((
        revision,
        add_result(torrent_id, AddTorrentDisposition::Added, revision),
    ))
}

fn force_recheck(
    transaction: &Transaction<'_>,
    torrent_id: &str,
    current_revision: u64,
) -> Result<u64, (ErrorCode, String)> {
    let info_hash = decode_info_hash(torrent_id).ok_or_else(|| {
        (
            ErrorCode::InvalidRequest,
            "invalid torrent identity".to_owned(),
        )
    })?;
    let row = transaction
        .query_row(
            "SELECT raw_info IS NOT NULL, storage_state, managed_artifacts,
                    EXISTS(SELECT 1 FROM removal_jobs r
                           WHERE r.info_hash = torrents.info_hash)
             FROM torrents WHERE info_hash = ?1",
            [info_hash.as_slice()],
            |row| {
                Ok((
                    row.get::<_, bool>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, bool>(3)?,
                ))
            },
        )
        .optional()
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                ErrorCode::UnknownTorrent,
                format!(
                    "torrent {} is not in the profile",
                    torrent_id.to_ascii_lowercase()
                ),
            )
        })?;
    if row.3 {
        return Err((
            ErrorCode::InvalidTorrentState,
            "torrent removal is already in progress".to_owned(),
        ));
    }
    let storage_state = StorageState::parse(&row.1)
        .ok_or_else(|| internal_message("database contains an invalid storage state"))?;
    let managed_artifacts = ManagedArtifactState::parse(&row.2)
        .ok_or_else(|| internal_message("database contains invalid artifact ownership"))?;
    if !row.0
        || !matches!(
            storage_state,
            StorageState::Staging | StorageState::Published
        )
        || !matches!(
            managed_artifacts,
            ManagedArtifactState::Staging | ManagedArtifactState::Published
        )
    {
        return Err((
            ErrorCode::InvalidTorrentState,
            "force recheck requires verified managed staging or published content".to_owned(),
        ));
    }
    let revision = next_revision(transaction, current_revision)?;
    let revision_sql =
        i64::try_from(revision).map_err(|_| internal_message("profile revision overflow"))?;
    transaction
        .execute(
            "UPDATE torrents
             SET state = ?2, error = NULL, updated_revision = ?3
             WHERE info_hash = ?1",
            params![
                info_hash.as_slice(),
                TorrentState::Checking.as_str(),
                revision_sql,
            ],
        )
        .map_err(internal_error)?;
    Ok(revision)
}

fn set_default_storage_root(
    transaction: &Transaction<'_>,
    storage_root: &str,
    current_revision: u64,
) -> Result<u64, (ErrorCode, String)> {
    let exists = transaction
        .query_row(
            "SELECT 1 FROM storage_roots WHERE root_id = ?1",
            [storage_root],
            |_| Ok(()),
        )
        .optional()
        .map_err(internal_error)?
        .is_some();
    if !exists {
        return Err((
            ErrorCode::UnknownStorageRoot,
            format!("storage root {storage_root} is not configured"),
        ));
    }
    let revision = next_revision(transaction, current_revision)?;
    transaction
        .execute(
            "UPDATE storage_settings SET default_root = ?1 WHERE singleton = 1",
            [storage_root],
        )
        .map_err(internal_error)?;
    Ok(revision)
}

fn set_show_add_options(
    transaction: &Transaction<'_>,
    show: bool,
    current_revision: u64,
) -> Result<u64, (ErrorCode, String)> {
    let revision = next_revision(transaction, current_revision)?;
    transaction
        .execute(
            "UPDATE storage_settings SET show_add_options = ?1 WHERE singleton = 1",
            [show],
        )
        .map_err(internal_error)?;
    Ok(revision)
}

fn remove_storage_root(
    transaction: &Transaction<'_>,
    storage_root: &str,
    current_revision: u64,
) -> Result<u64, (ErrorCode, String)> {
    let exists = transaction
        .query_row(
            "SELECT 1 FROM storage_roots WHERE root_id = ?1",
            [storage_root],
            |_| Ok(()),
        )
        .optional()
        .map_err(internal_error)?
        .is_some();
    if !exists {
        return Err((
            ErrorCode::UnknownStorageRoot,
            format!("storage root {storage_root} is not configured"),
        ));
    }
    let references: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM torrents WHERE storage_root = ?1",
            [storage_root],
            |row| row.get(0),
        )
        .map_err(internal_error)?;
    if references != 0 {
        return Err((
            ErrorCode::StorageRootInUse,
            format!("storage root {storage_root} is used by {references} retained torrent(s)"),
        ));
    }
    let revision = next_revision(transaction, current_revision)?;
    transaction
        .execute(
            "DELETE FROM storage_roots WHERE root_id = ?1",
            [storage_root],
        )
        .map_err(internal_error)?;
    Ok(revision)
}

fn add_magnet(
    transaction: &Transaction<'_>,
    source: &str,
    storage_root: &str,
    start_content: bool,
    skip_files: &[u32],
    current_revision: u64,
) -> Result<(u64, AddTorrentResult), (ErrorCode, String)> {
    let magnet =
        Magnet::parse(source).map_err(|error| (ErrorCode::InvalidRequest, error.to_string()))?;
    let torrent_id = encode_info_hash(magnet.info_hash);
    if !skip_files.is_empty() && magnet.select_only.is_some() {
        return Err((
            ErrorCode::InvalidRequest,
            "skip_files and select-only magnet intent cannot be combined".to_owned(),
        ));
    }
    let existing = transaction
        .query_row(
            "SELECT raw_info, selection_default, state, storage_state,
                    desired_state, archived,
                    EXISTS(SELECT 1 FROM removal_jobs r
                           WHERE r.info_hash = torrents.info_hash)
             FROM torrents WHERE info_hash = ?1",
            [magnet.info_hash.as_slice()],
            |row| {
                Ok((
                    row.get::<_, Option<Vec<u8>>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, bool>(5)?,
                    row.get::<_, bool>(6)?,
                ))
            },
        )
        .optional()
        .map_err(internal_error)?;
    if let Some((
        raw_info,
        selection_default,
        state,
        storage_state,
        desired_state,
        archived,
        removing,
    )) = existing
    {
        if removing {
            return Err((
                ErrorCode::InvalidTorrentState,
                "torrent removal is already in progress".to_owned(),
            ));
        }
        let Some(select_only) = magnet.select_only.as_ref() else {
            return Ok((
                current_revision,
                add_result(
                    torrent_id,
                    AddTorrentDisposition::AlreadyPresent,
                    current_revision,
                ),
            ));
        };
        if raw_info.is_some()
            && (matches!(state.as_str(), "needs_repair" | "awaiting_publication")
                || matches!(storage_state.as_str(), "needs_repair" | "prepared"))
        {
            return Err((
                ErrorCode::InvalidTorrentState,
                "file selection cannot change during repair or publication".to_owned(),
            ));
        }
        let file_plan = raw_info
            .as_deref()
            .map(|raw_info| {
                plan_duplicate_selection(
                    transaction,
                    &magnet.info_hash,
                    raw_info,
                    &selection_default,
                    select_only.ranges(),
                )
            })
            .transpose()?;
        let pending_plan = if raw_info.is_none() {
            union_pending_selection(transaction, &magnet.info_hash, select_only.ranges())?
        } else {
            None
        };
        if file_plan
            .as_ref()
            .is_some_and(|(_, changes)| changes.is_empty())
            || raw_info.is_none() && pending_plan.is_none()
        {
            return Ok((
                current_revision,
                add_result(
                    torrent_id,
                    AddTorrentDisposition::AlreadyPresent,
                    current_revision,
                ),
            ));
        }
        let revision = next_revision(transaction, current_revision)?;
        if let Some((wanted_value, changes)) = &file_plan {
            for index in changes {
                if *wanted_value {
                    transaction
                        .execute(
                            "INSERT INTO file_selection(info_hash, file_index, wanted)
                         VALUES (?1, ?2, 1)",
                            params![magnet.info_hash.as_slice(), i64::from(*index)],
                        )
                        .map_err(internal_error)?;
                } else {
                    transaction
                        .execute(
                            "DELETE FROM file_selection
                         WHERE info_hash = ?1 AND file_index = ?2 AND wanted = 0",
                            params![magnet.info_hash.as_slice(), i64::from(*index)],
                        )
                        .map_err(internal_error)?;
                }
            }
        }
        if let Some(ranges) = &pending_plan {
            write_pending_ranges(transaction, &magnet.info_hash, ranges)?;
        }
        let next_state = raw_info.as_ref().map(|_| {
            if desired_state == "paused" || archived {
                "paused"
            } else {
                "checking"
            }
        });
        transaction
            .execute(
                "UPDATE torrents SET updated_revision = ?2,
                    state = COALESCE(?3, state),
                    error = CASE WHEN ?3 IS NULL THEN error ELSE NULL END
                 WHERE info_hash = ?1",
                params![
                    magnet.info_hash.as_slice(),
                    sql_revision(revision).map_err(|e| internal_message(&e.to_string()))?,
                    next_state,
                ],
            )
            .map_err(internal_error)?;
        return Ok((
            revision,
            add_result(
                torrent_id,
                AddTorrentDisposition::SelectionExpanded {
                    newly_wanted_count: file_plan.as_ref().map(|(_, changes)| changes.len() as u32),
                },
                revision,
            ),
        ));
    }
    let root_exists = transaction
        .query_row(
            "SELECT 1 FROM storage_roots WHERE root_id = ?1",
            [storage_root],
            |_| Ok(()),
        )
        .optional()
        .map_err(internal_error)?
        .is_some();
    if !root_exists {
        return Err((
            ErrorCode::UnknownStorageRoot,
            format!("storage root {storage_root} is not configured"),
        ));
    }
    let revision = current_revision
        .checked_add(1)
        .ok_or_else(|| internal_message("profile revision overflow"))?;
    let revision_sql =
        i64::try_from(revision).map_err(|_| internal_message("profile revision overflow"))?;
    transaction
        .execute(
            "UPDATE profile_state SET revision = ?1 WHERE singleton = 1",
            [revision_sql],
        )
        .map_err(internal_error)?;
    transaction
        .execute(
            "INSERT INTO torrents(
                info_hash, magnet, storage_root, desired_state, state,
                storage_state, managed_artifacts, created_revision,
                updated_revision, selection_default
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'none', ?7, ?7, ?8)",
            params![
                magnet.info_hash.as_slice(),
                canonical_magnet(&magnet),
                storage_root,
                if start_content { "running" } else { "paused" },
                TorrentState::AwaitingMetadata.as_str(),
                StorageState::None.as_str(),
                revision_sql,
                if magnet.select_only.is_some() {
                    "skipped"
                } else {
                    "wanted"
                }
            ],
        )
        .map_err(internal_error)?;
    let source_digest = Sha256::digest(source.as_bytes());
    transaction
        .execute(
            "INSERT INTO torrent_source(
                info_hash, kind, fidelity, magnet, metainfo, byte_length, sha256
             ) VALUES (?1, 'magnet', 'verbatim', ?2, NULL, ?3, ?4)",
            params![
                magnet.info_hash.as_slice(),
                source,
                i64::try_from(source.len())
                    .map_err(|_| internal_message("magnet length overflows i64"))?,
                source_digest.as_slice(),
            ],
        )
        .map_err(internal_error)?;
    for (position, tracker) in magnet.trackers.iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO torrent_trackers(
                    info_hash, tier, position, url, transport, source
                 ) VALUES (?1, 0, ?2, ?3, ?4, 'magnet')",
                params![
                    magnet.info_hash.as_slice(),
                    i64::try_from(position)
                        .map_err(|_| internal_message("tracker position overflows i64"))?,
                    tracker.url(),
                    tracker_transport_name(tracker.transport()),
                ],
            )
            .map_err(internal_error)?;
    }
    for (position, hint) in magnet.peer_hints.iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO torrent_peer_hints(
                    info_hash, position, host, port, source
                 ) VALUES (?1, ?2, ?3, ?4, 'magnet')",
                params![
                    magnet.info_hash.as_slice(),
                    i64::try_from(position)
                        .map_err(|_| internal_message("peer hint position overflows i64"))?,
                    hint.host,
                    i64::from(hint.port),
                ],
            )
            .map_err(internal_error)?;
    }
    for file_index in skip_files {
        transaction
            .execute(
                "INSERT INTO file_selection(info_hash, file_index, wanted)
                 VALUES (?1, ?2, 0)",
                params![magnet.info_hash.as_slice(), i64::from(*file_index)],
            )
            .map_err(internal_error)?;
    }
    if let Some(selection) = &magnet.select_only {
        write_pending_ranges(transaction, &magnet.info_hash, selection.ranges())?;
    }
    Ok((
        revision,
        add_result(torrent_id, AddTorrentDisposition::Added, revision),
    ))
}

fn add_result(
    torrent_id: String,
    disposition: AddTorrentDisposition,
    revision: u64,
) -> AddTorrentResult {
    AddTorrentResult {
        torrent_id,
        disposition,
        resulting_revision: revision.to_string(),
    }
}

fn plan_duplicate_selection(
    transaction: &Transaction<'_>,
    info_hash: &[u8; 20],
    raw_info: &[u8],
    selection_default: &str,
    ranges: &[MagnetFileIndexRange],
) -> Result<(bool, Vec<u32>), (ErrorCode, String)> {
    let metainfo = Metainfo::from_info_bytes_with_limits(raw_info, DURABLE_METAINFO_LIMITS)
        .map_err(|error| (ErrorCode::InvalidDurableState, error.to_string()))?;
    let wanted_value = match selection_default {
        "wanted" => false,
        "skipped" => true,
        _ => {
            return Err(internal_message(
                "database contains an invalid selection default",
            ));
        }
    };
    let (_, exceptions) = read_selection_state(transaction, info_hash)
        .map_err(|error| internal_message(&error.to_string()))?;
    let mut changes = Vec::new();
    for range in ranges {
        let end = usize::try_from(range.end)
            .unwrap_or(usize::MAX)
            .min(metainfo.files.len().saturating_sub(1));
        let start = usize::try_from(range.start).unwrap_or(usize::MAX);
        if start > end {
            continue;
        }
        for index in start..=end {
            if metainfo.files[index].padding {
                continue;
            }
            let index =
                u32::try_from(index).map_err(|_| internal_message("file index overflow"))?;
            let is_exception = exceptions.binary_search(&index).is_ok();
            if wanted_value != is_exception {
                changes.push(index);
            }
        }
    }
    if wanted_value && exceptions.len() + changes.len() > MAX_FILE_SELECTION_ENTRIES {
        return Err((
            ErrorCode::ResourceLimit,
            format!("file selection exceeds {MAX_FILE_SELECTION_ENTRIES} exceptions"),
        ));
    }
    Ok((wanted_value, changes))
}

fn read_pending_ranges(
    transaction: &Transaction<'_>,
    info_hash: &[u8; 20],
) -> Result<Vec<MagnetFileIndexRange>, (ErrorCode, String)> {
    let mut statement = transaction
        .prepare(
            "SELECT range_start, range_end FROM pending_selection_ranges
         WHERE info_hash = ?1 ORDER BY range_start",
        )
        .map_err(internal_error)?;
    let rows = statement
        .query_map([info_hash.as_slice()], |row| {
            Ok(MagnetFileIndexRange {
                start: row.get(0)?,
                end: row.get(1)?,
            })
        })
        .map_err(internal_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(internal_error)
}

fn union_pending_selection(
    transaction: &Transaction<'_>,
    info_hash: &[u8; 20],
    added: &[MagnetFileIndexRange],
) -> Result<Option<Vec<MagnetFileIndexRange>>, (ErrorCode, String)> {
    let initial = read_pending_ranges(transaction, info_hash)?;
    let mut all = initial.clone();
    all.extend_from_slice(added);
    all.sort_unstable_by_key(|range| (range.start, range.end));
    let mut merged: Vec<MagnetFileIndexRange> = Vec::with_capacity(all.len());
    for range in all {
        if let Some(last) = merged.last_mut()
            && range.start <= last.end.saturating_add(1)
        {
            last.end = last.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    if merged == initial {
        return Ok(None);
    }
    Ok(Some(merged))
}

fn write_pending_ranges(
    transaction: &Transaction<'_>,
    info_hash: &[u8; 20],
    ranges: &[MagnetFileIndexRange],
) -> Result<(), (ErrorCode, String)> {
    transaction
        .execute(
            "DELETE FROM pending_selection_ranges WHERE info_hash = ?1",
            [info_hash.as_slice()],
        )
        .map_err(internal_error)?;
    for range in ranges {
        transaction.execute(
            "INSERT INTO pending_selection_ranges(info_hash, range_start, range_end) VALUES (?1, ?2, ?3)",
            params![info_hash.as_slice(), i64::from(range.start), i64::from(range.end)],
        ).map_err(internal_error)?;
    }
    Ok(())
}

fn set_file_priority(
    transaction: &Transaction<'_>,
    torrent_id: &str,
    file_indices: &[u32],
    priority: FilePriority,
    current_revision: u64,
) -> Result<u64, (ErrorCode, String)> {
    set_file_priority_indices(
        transaction,
        torrent_id,
        file_indices.iter().copied(),
        priority,
        current_revision,
    )
}

fn set_file_priority_indices<I>(
    transaction: &Transaction<'_>,
    torrent_id: &str,
    file_indices: I,
    priority: FilePriority,
    current_revision: u64,
) -> Result<u64, (ErrorCode, String)>
where
    I: Iterator<Item = u32> + Clone,
{
    let info_hash = decode_info_hash(torrent_id).ok_or_else(|| {
        (
            ErrorCode::InvalidRequest,
            "invalid torrent identity".to_owned(),
        )
    })?;
    let row = transaction
        .query_row(
            "SELECT t.raw_info, t.desired_state, t.state, t.storage_state,
                    sr.kind, r.info_hash IS NOT NULL, t.selection_default
             FROM torrents t
             JOIN storage_roots sr ON sr.root_id = t.storage_root
             LEFT JOIN removal_jobs r ON r.info_hash = t.info_hash
             WHERE t.info_hash = ?1",
            [info_hash.as_slice()],
            |row| {
                Ok((
                    row.get::<_, Option<Vec<u8>>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, bool>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .optional()
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                ErrorCode::UnknownTorrent,
                format!(
                    "torrent {} is not in the profile",
                    torrent_id.to_ascii_lowercase()
                ),
            )
        })?;
    if row.5 {
        return Err((
            ErrorCode::InvalidTorrentState,
            "torrent removal is already in progress".to_owned(),
        ));
    }
    let raw_info = row.0.ok_or_else(|| {
        (
            ErrorCode::InvalidTorrentState,
            "file selection requires verified metadata".to_owned(),
        )
    })?;
    let metainfo = Metainfo::from_info_bytes_with_limits(&raw_info, DURABLE_METAINFO_LIMITS)
        .map_err(|error| (ErrorCode::InvalidDurableState, error.to_string()))?;
    for file_index in file_indices.clone() {
        let file_index = usize::try_from(file_index).map_err(|_| {
            (
                ErrorCode::InvalidRequest,
                "file selection index exceeds the supported file bound".to_owned(),
            )
        })?;
        let file = metainfo.files.get(file_index).ok_or_else(|| {
            (
                ErrorCode::InvalidRequest,
                format!("file index {file_index} is outside verified metadata"),
            )
        })?;
        if file.padding {
            return Err((
                ErrorCode::InvalidRequest,
                format!("padding file {file_index} cannot be selected"),
            ));
        }
    }
    let (selection_default, mut exceptions) = read_selection_state(transaction, &info_hash)
        .map_err(|error| internal_message(&error.to_string()))?;
    let initial_exceptions = exceptions.clone();
    for file_index in file_indices.clone() {
        if priority == selection_default {
            exceptions.retain(|index| *index != file_index);
        } else if let Err(position) = exceptions.binary_search(&file_index) {
            exceptions.insert(position, file_index);
        }
    }
    if exceptions == initial_exceptions {
        return Ok(current_revision);
    }
    if !matches!(row.6.as_str(), "wanted" | "skipped") {
        return Err(internal_message(
            "database contains an invalid selection default",
        ));
    }
    let current_state = TorrentState::parse(&row.2)
        .ok_or_else(|| internal_message("database contains an invalid torrent state"))?;
    let storage_state = StorageState::parse(&row.3)
        .ok_or_else(|| internal_message("database contains an invalid storage state"))?;
    if current_state == TorrentState::NeedsRepair
        || storage_state == StorageState::NeedsRepair
        || current_state == TorrentState::AwaitingPublication
        || storage_state == StorageState::Prepared
    {
        return Err((
            ErrorCode::InvalidTorrentState,
            "file selection cannot change during repair or publication".to_owned(),
        ));
    }
    let wanted_count = metainfo
        .files
        .iter()
        .enumerate()
        .filter(|(index, file)| {
            !file.padding
                && (selection_default == FilePriority::Normal)
                    != exceptions.binary_search(&(*index as u32)).is_ok()
        })
        .count();
    let next_state = if row.1 == "paused" || wanted_count == 0 {
        TorrentState::Paused
    } else {
        TorrentState::Checking
    };
    let revision = next_revision(transaction, current_revision)?;
    transaction
        .execute(
            "DELETE FROM file_selection WHERE info_hash = ?1",
            [info_hash.as_slice()],
        )
        .map_err(internal_error)?;
    let exception_wanted = selection_default == FilePriority::Skip;
    for file_index in exceptions {
        transaction
            .execute(
                "INSERT INTO file_selection(info_hash, file_index, wanted)
                 VALUES (?1, ?2, ?3)",
                params![
                    info_hash.as_slice(),
                    i64::from(file_index),
                    exception_wanted
                ],
            )
            .map_err(internal_error)?;
    }
    transaction
        .execute(
            "UPDATE torrents
             SET state = ?2, error = NULL, updated_revision = ?3
             WHERE info_hash = ?1",
            params![
                info_hash.as_slice(),
                next_state.as_str(),
                i64::try_from(revision)
                    .map_err(|_| internal_message("profile revision overflow"))?
            ],
        )
        .map_err(internal_error)?;
    Ok(revision)
}

fn set_desired_state(
    transaction: &Transaction<'_>,
    torrent_id: &str,
    running: bool,
    current_revision: u64,
) -> Result<u64, (ErrorCode, String)> {
    let info_hash = decode_info_hash(torrent_id).ok_or_else(|| {
        (
            ErrorCode::InvalidRequest,
            "invalid torrent identity".to_owned(),
        )
    })?;
    let row = transaction
        .query_row(
            "SELECT t.state, t.raw_info IS NOT NULL, t.desired_state,
                    r.info_hash IS NOT NULL
             FROM torrents t
             LEFT JOIN removal_jobs r ON r.info_hash = t.info_hash
             WHERE t.info_hash = ?1",
            [info_hash.as_slice()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, bool>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, bool>(3)?,
                ))
            },
        )
        .optional()
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                ErrorCode::UnknownTorrent,
                format!(
                    "torrent {} is not in the profile",
                    torrent_id.to_ascii_lowercase()
                ),
            )
        })?;
    if row.3 {
        return Err((
            ErrorCode::InvalidTorrentState,
            "torrent removal is already in progress".to_owned(),
        ));
    }
    let desired = if running { "running" } else { "paused" };
    if row.2 == desired {
        return Ok(current_revision);
    }
    let current_state = TorrentState::parse(&row.0)
        .ok_or_else(|| internal_message("database contains an invalid torrent state"))?;
    if running
        && matches!(
            current_state,
            TorrentState::Complete | TorrentState::NeedsRepair
        )
    {
        return Err((
            ErrorCode::InvalidTorrentState,
            format!("torrent cannot resume from {}", current_state.as_str()),
        ));
    }
    let next_state = if current_state == TorrentState::AwaitingPublication {
        TorrentState::AwaitingPublication
    } else if running {
        if row.1 {
            TorrentState::Checking
        } else {
            TorrentState::AwaitingMetadata
        }
    } else {
        TorrentState::Paused
    };
    let revision = current_revision
        .checked_add(1)
        .ok_or_else(|| internal_message("profile revision overflow"))?;
    let revision_sql =
        i64::try_from(revision).map_err(|_| internal_message("profile revision overflow"))?;
    transaction
        .execute(
            "UPDATE profile_state SET revision = ?1 WHERE singleton = 1",
            [revision_sql],
        )
        .map_err(internal_error)?;
    transaction
        .execute(
            "UPDATE torrents
             SET desired_state = ?2, state = ?3, error = NULL,
                 updated_revision = ?4
             WHERE info_hash = ?1",
            params![
                info_hash.as_slice(),
                desired,
                next_state.as_str(),
                revision_sql
            ],
        )
        .map_err(internal_error)?;
    Ok(revision)
}

fn set_archived(
    transaction: &Transaction<'_>,
    torrent_id: &str,
    archived: bool,
    current_revision: u64,
) -> Result<u64, (ErrorCode, String)> {
    let info_hash = decode_info_hash(torrent_id).ok_or_else(|| {
        (
            ErrorCode::InvalidRequest,
            "invalid torrent identity".to_owned(),
        )
    })?;
    let current = transaction
        .query_row(
            "SELECT t.archived, r.info_hash IS NOT NULL
             FROM torrents t
             LEFT JOIN removal_jobs r ON r.info_hash = t.info_hash
             WHERE t.info_hash = ?1",
            [info_hash.as_slice()],
            |row| Ok((row.get::<_, bool>(0)?, row.get::<_, bool>(1)?)),
        )
        .optional()
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                ErrorCode::UnknownTorrent,
                format!(
                    "torrent {} is not in the profile",
                    torrent_id.to_ascii_lowercase()
                ),
            )
        })?;
    if current.1 {
        return Err((
            ErrorCode::InvalidTorrentState,
            "torrent removal is already in progress".to_owned(),
        ));
    }
    if current.0 == archived {
        return Ok(current_revision);
    }
    let revision = next_revision(transaction, current_revision)?;
    transaction
        .execute(
            "UPDATE torrents SET archived = ?2, updated_revision = ?3
             WHERE info_hash = ?1",
            params![
                info_hash.as_slice(),
                archived,
                i64::try_from(revision)
                    .map_err(|_| internal_message("profile revision overflow"))?
            ],
        )
        .map_err(internal_error)?;
    Ok(revision)
}

fn begin_removal(
    transaction: &Transaction<'_>,
    torrent_id: &str,
    policy: RemovalDataPolicy,
    current_revision: u64,
) -> Result<u64, (ErrorCode, String)> {
    let info_hash = decode_info_hash(torrent_id).ok_or_else(|| {
        (
            ErrorCode::InvalidRequest,
            "invalid torrent identity".to_owned(),
        )
    })?;
    let removal_state = transaction
        .query_row(
            "SELECT r.state
             FROM torrents t
             LEFT JOIN removal_jobs r ON r.info_hash = t.info_hash
             WHERE t.info_hash = ?1",
            [info_hash.as_slice()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                ErrorCode::UnknownTorrent,
                format!(
                    "torrent {} is not in the profile",
                    torrent_id.to_ascii_lowercase()
                ),
            )
        })?;
    if removal_state
        .as_deref()
        .is_some_and(|state| state != "failed")
    {
        return Err((
            ErrorCode::InvalidTorrentState,
            "torrent removal is already in progress".to_owned(),
        ));
    }

    let revision = next_revision(transaction, current_revision)?;
    let revision_sql =
        i64::try_from(revision).map_err(|_| internal_message("profile revision overflow"))?;
    let operation_id = format!("remove-{revision}-{}", torrent_id.to_ascii_lowercase());
    transaction
        .execute(
            "UPDATE torrents
             SET desired_state = 'paused', state = 'paused', error = NULL,
                 updated_revision = ?2
             WHERE info_hash = ?1",
            params![info_hash.as_slice(), revision_sql],
        )
        .map_err(internal_error)?;
    transaction
        .execute(
            "INSERT INTO removal_jobs(
                info_hash, operation_id, data_policy, state, error,
                created_revision, updated_revision
             ) VALUES (?1, ?2, ?3, 'pending', NULL, ?4, ?4)
             ON CONFLICT(info_hash) DO UPDATE SET
                operation_id = excluded.operation_id,
                data_policy = excluded.data_policy,
                state = 'pending',
                error = NULL,
                created_revision = excluded.created_revision,
                updated_revision = excluded.updated_revision",
            params![
                info_hash.as_slice(),
                operation_id,
                policy.as_str(),
                revision_sql
            ],
        )
        .map_err(internal_error)?;
    Ok(revision)
}

fn next_revision(
    transaction: &Transaction<'_>,
    current_revision: u64,
) -> Result<u64, (ErrorCode, String)> {
    let revision = current_revision
        .checked_add(1)
        .ok_or_else(|| internal_message("profile revision overflow"))?;
    let revision_sql =
        i64::try_from(revision).map_err(|_| internal_message("profile revision overflow"))?;
    transaction
        .execute(
            "UPDATE profile_state SET revision = ?1 WHERE singleton = 1",
            [revision_sql],
        )
        .map_err(internal_error)?;
    Ok(revision)
}

fn next_revision_strict(
    transaction: &Transaction<'_>,
    current_revision: u64,
) -> Result<u64, StoreError> {
    let revision = current_revision
        .checked_add(1)
        .ok_or_else(|| StoreError::DurableState("profile revision overflow".to_owned()))?;
    let revision_sql = i64::try_from(revision)
        .map_err(|_| StoreError::DurableState("profile revision overflow".to_owned()))?;
    transaction.execute(
        "UPDATE profile_state SET revision = ?1 WHERE singleton = 1",
        [revision_sql],
    )?;
    Ok(revision)
}

fn read_snapshot(connection: &Connection, profile_id: &str) -> Result<ServiceSnapshot, StoreError> {
    let revision = read_revision(connection)?;
    let mut statement = connection.prepare(
        "SELECT t.info_hash, t.storage_root, t.state, t.storage_state,
                t.raw_info IS NOT NULL, t.piece_count, t.have_state,
                t.error, t.archived, r.state, r.error, t.managed_artifacts
         FROM torrents t
         LEFT JOIN removal_jobs r ON r.info_hash = t.info_hash
         ORDER BY t.info_hash",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, Vec<u8>>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, bool>(4)?,
            row.get::<_, Option<i64>>(5)?,
            row.get::<_, Option<Vec<u8>>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, bool>(8)?,
            row.get::<_, Option<String>>(9)?,
            row.get::<_, Option<String>>(10)?,
            row.get::<_, String>(11)?,
        ))
    })?;
    let mut torrents = Vec::new();
    for row in rows {
        let row = row?;
        let info_hash: [u8; 20] = row
            .0
            .try_into()
            .map_err(|_| StoreError::DurableState("invalid info-hash length".to_owned()))?;
        let torrent_id = encode_info_hash(info_hash);
        let mut state = TorrentState::parse(&row.2)
            .ok_or_else(|| StoreError::DurableState("invalid torrent state".to_owned()))?;
        let mut storage_state = StorageState::parse(&row.3)
            .ok_or_else(|| StoreError::DurableState("invalid storage state".to_owned()))?;
        let persisted_storage_state = storage_state;
        let managed_artifacts = ManagedArtifactState::parse(&row.11)
            .ok_or_else(|| StoreError::DurableState("invalid managed artifact state".to_owned()))?;
        let piece_count = match row.5 {
            Some(piece_count) => bounded_piece_count(piece_count)?,
            None => 0,
        };
        let verified_piece_count = match (&row.6, piece_count) {
            (Some(bytes), count) if count != 0 => {
                match HaveState::decode(bytes, info_hash, count) {
                    Ok(have) => have.verified_count(),
                    Err(_) => {
                        state = TorrentState::NeedsRepair;
                        storage_state = StorageState::NeedsRepair;
                        0
                    }
                }
            }
            (None, 0) => 0,
            _ => {
                state = TorrentState::NeedsRepair;
                storage_state = StorageState::NeedsRepair;
                0
            }
        };
        let verified_piece_count = if matches!(
            state,
            TorrentState::Checking | TorrentState::AwaitingStorage
        ) {
            0
        } else {
            verified_piece_count
        };
        let (selection_default, selection_exceptions) =
            read_selection_state(connection, &info_hash)?;
        torrents.push(TorrentSnapshot {
            torrent_id,
            storage_root: row.1,
            state,
            storage_state,
            metadata_available: row.4,
            piece_count: u32::try_from(piece_count)
                .map_err(|_| StoreError::DurableState("piece count overflow".to_owned()))?,
            verified_piece_count: u32::try_from(verified_piece_count)
                .map_err(|_| StoreError::DurableState("verified count overflow".to_owned()))?,
            skip_files: if selection_default == FilePriority::Normal {
                selection_exceptions.clone()
            } else {
                Vec::new()
            },
            selection_default,
            selection_exceptions,
            archived: row.8,
            removal_state: match row.9.as_deref() {
                Some(value) => {
                    Some(RemovalState::parse(value).ok_or_else(|| {
                        StoreError::DurableState("invalid removal state".to_owned())
                    })?)
                }
                None => None,
            },
            delete_managed_data_supported: true,
            force_recheck_available: row.4
                && row.9.is_none()
                && matches!(
                    persisted_storage_state,
                    StorageState::Staging | StorageState::Published
                )
                && matches!(
                    managed_artifacts,
                    ManagedArtifactState::Staging | ManagedArtifactState::Published
                ),
            error: row.10.or(row.7),
        });
    }
    Ok(ServiceSnapshot {
        profile_id: profile_id.to_owned(),
        revision: revision.to_string(),
        storage: read_storage_settings(connection)?,
        client_settings: read_client_settings(connection)?,
        torrents,
    })
}

struct RemovalRow {
    operation_id: String,
    storage_root: String,
    data_policy: String,
    state: String,
    raw_info: Option<Vec<u8>>,
    publication_name: Option<String>,
    storage_state: String,
    managed_artifacts: String,
    error: Option<String>,
}

fn removal_record(torrent_id: &str, row: RemovalRow) -> Result<RemovalRecord, StoreError> {
    let policy = RemovalDataPolicy::parse(&row.data_policy)
        .ok_or_else(|| StoreError::DurableState("invalid removal data policy".to_owned()))?;
    let state = RemovalState::parse(&row.state)
        .ok_or_else(|| StoreError::DurableState("invalid removal state".to_owned()))?;
    let managed_artifacts = ManagedArtifactState::parse(&row.managed_artifacts)
        .ok_or_else(|| StoreError::DurableState("invalid managed artifact state".to_owned()))?;
    let storage_state = StorageState::parse(&row.storage_state)
        .ok_or_else(|| StoreError::DurableState("invalid storage state".to_owned()))?;
    Ok(RemovalRecord {
        torrent_id: torrent_id.to_ascii_lowercase(),
        operation_id: row.operation_id,
        storage_root: row.storage_root,
        policy,
        state,
        raw_info: row.raw_info,
        publication_name: row.publication_name,
        storage_state,
        managed_artifacts,
        error: row.error,
    })
}

fn read_selection(connection: &Connection, info_hash: &[u8; 20]) -> Result<Vec<u32>, StoreError> {
    let (default, exceptions) = read_selection_state(connection, info_hash)?;
    if default == FilePriority::Normal {
        return Ok(exceptions);
    }
    let raw_info: Option<Vec<u8>> = connection.query_row(
        "SELECT raw_info FROM torrents WHERE info_hash = ?1",
        [info_hash.as_slice()],
        |row| row.get(0),
    )?;
    let Some(raw_info) = raw_info else {
        return Ok(Vec::new());
    };
    let metainfo = parse_durable_metainfo(&raw_info)?;
    let mut skipped = Vec::new();
    for (index, file) in metainfo.files.iter().enumerate() {
        let index = u32::try_from(index)
            .map_err(|_| StoreError::DurableState("file index overflow".to_owned()))?;
        if !file.padding && exceptions.binary_search(&index).is_err() {
            skipped.push(index);
        }
    }
    Ok(skipped)
}

fn read_selection_state(
    connection: &Connection,
    info_hash: &[u8; 20],
) -> Result<(FilePriority, Vec<u32>), StoreError> {
    let default: String = connection.query_row(
        "SELECT selection_default FROM torrents WHERE info_hash = ?1",
        [info_hash.as_slice()],
        |row| row.get(0),
    )?;
    let (default, expected_wanted) = match default.as_str() {
        "wanted" => (FilePriority::Normal, false),
        "skipped" => (FilePriority::Skip, true),
        _ => {
            return Err(StoreError::DurableState(
                "invalid selection default".to_owned(),
            ));
        }
    };
    let mut statement = connection.prepare(
        "SELECT file_index, wanted FROM file_selection
         WHERE info_hash = ?1 ORDER BY file_index",
    )?;
    let rows = statement.query_map([info_hash.as_slice()], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, bool>(1)?))
    })?;
    let mut selection = Vec::new();
    for row in rows {
        let (index, wanted) = row?;
        if wanted != expected_wanted {
            return Err(StoreError::DurableState(
                "selection exception matches its default".to_owned(),
            ));
        }
        if !(0..i64::try_from(DURABLE_METAINFO_LIMITS.max_files).expect("file bound fits i64"))
            .contains(&index)
        {
            return Err(StoreError::DurableState(
                "invalid file selection index".to_owned(),
            ));
        }
        selection.push(
            u32::try_from(index)
                .map_err(|_| StoreError::DurableState("selection index overflow".to_owned()))?,
        );
    }
    Ok((default, selection))
}

fn read_trackers(
    connection: &Connection,
    info_hash: &[u8; 20],
) -> Result<Vec<StoredTracker>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT tier, position, url, transport, source
         FROM torrent_trackers
         WHERE info_hash = ?1
         ORDER BY tier, position",
    )?;
    let rows = statement.query_map([info_hash.as_slice()], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    let mut trackers = Vec::new();
    for row in rows {
        let (tier, position, url, transport, source) = row?;
        let tier = u32::try_from(tier)
            .map_err(|_| StoreError::DurableState("tracker tier overflow".to_owned()))?;
        let position = u32::try_from(position)
            .map_err(|_| StoreError::DurableState("tracker position overflow".to_owned()))?;
        let transport = match transport.as_str() {
            "udp" => StoredTrackerTransport::Udp,
            "http" => StoredTrackerTransport::Http,
            "https" => StoredTrackerTransport::Https,
            _ => {
                return Err(StoreError::DurableState(
                    "invalid tracker transport".to_owned(),
                ));
            }
        };
        let source = match source.as_str() {
            "magnet" => StoredTrackerSource::Magnet,
            "metainfo" => StoredTrackerSource::Metainfo,
            _ => {
                return Err(StoreError::DurableState(
                    "invalid tracker source".to_owned(),
                ));
            }
        };
        trackers.push(StoredTracker {
            tier,
            position,
            url,
            transport,
            source,
        });
    }
    Ok(trackers)
}

fn read_revision(connection: &Connection) -> Result<u64, StoreError> {
    let revision: i64 = connection.query_row(
        "SELECT revision FROM profile_state WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?;
    u64::try_from(revision)
        .map_err(|_| StoreError::DurableState("negative profile revision".to_owned()))
}

fn replay_or_conflict(
    connection: &Connection,
    request_id: &str,
    request_json: &str,
) -> Result<Option<ResponseEnvelope>, StoreError> {
    let Some((stored_request, stored_response)) = connection
        .query_row(
            "SELECT request_json, response_json
             FROM request_receipts WHERE request_id = ?1",
            [request_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
    else {
        return Ok(None);
    };
    if stored_request == request_json {
        return Ok(Some(serde_json::from_str(&stored_response)?));
    }
    Ok(Some(ResponseEnvelope::error(
        request_id.to_owned(),
        read_revision(connection)?,
        ErrorCode::RequestConflict,
        "request ID was already used for a different envelope",
    )))
}

fn insert_receipt(
    transaction: &Transaction<'_>,
    request_id: &str,
    request_json: &str,
    response: &ResponseEnvelope,
) -> Result<(), StoreError> {
    let response_json = serde_json::to_string(response)?;
    let response_revision = sql_revision(
        response
            .revision
            .parse()
            .map_err(|_| StoreError::DurableState("invalid response revision".to_owned()))?,
    )?;
    transaction.execute(
        "INSERT INTO request_receipts(
            request_id, request_json, response_json, revision
         ) VALUES (?1, ?2, ?3, ?4)",
        params![request_id, request_json, response_json, response_revision],
    )?;
    transaction.execute(
        "DELETE FROM request_receipts
         WHERE receipt_order <= (
            SELECT COALESCE(MAX(receipt_order), 0) - ?1
            FROM request_receipts
         )",
        [MAX_RECEIPTS],
    )?;
    Ok(())
}

fn increment_revision(transaction: &Transaction<'_>) -> Result<u64, StoreError> {
    let current = read_revision(transaction)?;
    let revision = current
        .checked_add(1)
        .ok_or_else(|| StoreError::DurableState("profile revision overflow".to_owned()))?;
    let revision_sql = sql_revision(revision)?;
    transaction.execute(
        "UPDATE profile_state SET revision = ?1 WHERE singleton = 1",
        [revision_sql],
    )?;
    Ok(revision)
}

fn sql_revision(revision: u64) -> Result<i64, StoreError> {
    i64::try_from(revision)
        .map_err(|_| StoreError::DurableState("profile revision exceeds SQLite i64".to_owned()))
}

fn read_have_columns(
    connection: &Connection,
    info_hash: &[u8; 20],
) -> Result<(usize, Vec<u8>), StoreError> {
    let row = connection
        .query_row(
            "SELECT piece_count, have_state FROM torrents WHERE info_hash = ?1",
            [info_hash.as_slice()],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?,
                    row.get::<_, Option<Vec<u8>>>(1)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| StoreError::UnknownTorrent(encode_info_hash(*info_hash)))?;
    match row {
        (Some(piece_count), Some(bytes)) => {
            validate_have_state_length(&bytes)?;
            Ok((bounded_piece_count(piece_count)?, bytes))
        }
        _ => Err(StoreError::DurableState(
            "torrent has no verified metadata and have state".to_owned(),
        )),
    }
}

fn bounded_piece_count(piece_count: i64) -> Result<usize, StoreError> {
    let piece_count = usize::try_from(piece_count)
        .map_err(|_| StoreError::DurableState("negative piece count".to_owned()))?;
    if piece_count == 0 || piece_count > MAX_DURABLE_PIECES {
        return Err(StoreError::ResourceLimit {
            resource: "piece count",
            actual: piece_count,
            maximum: MAX_DURABLE_PIECES,
        });
    }
    Ok(piece_count)
}

fn validate_raw_info_length(raw_info: &[u8]) -> Result<(), StoreError> {
    if raw_info.len() > DURABLE_METAINFO_LIMITS.max_info_bytes {
        return Err(StoreError::ResourceLimit {
            resource: "raw_info bytes",
            actual: raw_info.len(),
            maximum: DURABLE_METAINFO_LIMITS.max_info_bytes,
        });
    }
    Ok(())
}

fn validate_have_state_length(bytes: &[u8]) -> Result<(), StoreError> {
    if bytes.len() > MAX_DURABLE_HAVE_STATE_BYTES {
        return Err(StoreError::ResourceLimit {
            resource: "have-state bytes",
            actual: bytes.len(),
            maximum: MAX_DURABLE_HAVE_STATE_BYTES,
        });
    }
    Ok(())
}

fn encode_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[(byte >> 4) as usize]));
        encoded.push(char::from(HEX[(byte & 0x0f) as usize]));
    }
    encoded
}

fn metainfo_intake_error(error: MetainfoError) -> (ErrorCode, String) {
    let resource_limited = matches!(
        error,
        MetainfoError::InfoTooLarge { .. }
            | MetainfoError::TooManyFiles { .. }
            | MetainfoError::TooManyPieces { .. }
            | MetainfoError::Bencode(
                ParseError::InputTooLarge { .. }
                    | ParseError::StringTooLarge { .. }
                    | ParseError::NestingTooDeep { .. }
                    | ParseError::CollectionTooLarge { .. }
                    | ParseError::TooManyDecodedItems { .. }
            )
    );
    (
        if resource_limited {
            ErrorCode::ResourceLimit
        } else {
            ErrorCode::InvalidRequest
        },
        error.to_string(),
    )
}

fn parse_durable_metainfo(raw_info: &[u8]) -> Result<Metainfo, StoreError> {
    validate_raw_info_length(raw_info)?;
    Metainfo::from_info_bytes_with_limits(raw_info, DURABLE_METAINFO_LIMITS).map_err(|error| {
        match error {
            MetainfoError::InfoTooLarge { length, maximum } => StoreError::ResourceLimit {
                resource: "raw_info bytes",
                actual: length,
                maximum,
            },
            MetainfoError::TooManyPieces { actual, maximum } => StoreError::ResourceLimit {
                resource: "piece count",
                actual,
                maximum,
            },
            error => StoreError::DurableState(error.to_string()),
        }
    })
}

fn canonical_magnet(magnet: &Magnet) -> String {
    let mut output = format!("magnet:?xt=urn:btih:{}", encode_info_hash(magnet.info_hash));
    for hint in &magnet.peer_hints {
        output.push_str("&x.pe=");
        if hint.host.contains(':') {
            output.push('[');
            output.push_str(&hint.host);
            output.push(']');
        } else {
            output.push_str(&hint.host);
        }
        output.push(':');
        output.push_str(&hint.port.to_string());
    }
    for tracker in &magnet.trackers {
        output.push_str("&tr=");
        percent_encode_query_value(&mut output, tracker.url().as_bytes());
    }
    if let Some(selection) = &magnet.select_only {
        output.push_str("&so=");
        output.push_str(&selection.canonical());
    }
    output
}

fn tracker_transport_name(transport: TrackerUrlTransport) -> &'static str {
    match transport {
        TrackerUrlTransport::Udp => "udp",
        TrackerUrlTransport::Http => "http",
        TrackerUrlTransport::Https => "https",
    }
}

fn percent_encode_query_value(output: &mut String, value: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in value {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            output.push(char::from(*byte));
        } else {
            output.push('%');
            output.push(char::from(HEX[usize::from(byte >> 4)]));
            output.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
}

fn bounded_error(message: &str) -> String {
    if message.len() <= crate::control::MAX_ERROR_MESSAGE_LENGTH {
        return message.to_owned();
    }
    let mut boundary = crate::control::MAX_ERROR_MESSAGE_LENGTH;
    while !message.is_char_boundary(boundary) {
        boundary -= 1;
    }
    message[..boundary].to_owned()
}

fn internal_error(error: rusqlite::Error) -> (ErrorCode, String) {
    (ErrorCode::Internal, error.to_string())
}

fn internal_message(message: &str) -> (ErrorCode, String) {
    (ErrorCode::Internal, message.to_owned())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use rstorrent_engine::PreparedFileHash;
    use rstorrent_engine::dht::{DHT_SNAPSHOT_VERSION, DhtSnapshot};
    use rstorrent_protocol::dht::{DhtEndpoint, DhtIp, NodeContact, NodeId};
    use rstorrent_protocol::metainfo::{EXPLICIT_IMPORT_METAINFO_LIMITS, Metainfo, MetainfoFile};
    use rusqlite::Connection;
    use sha1::{Digest, Sha1};
    use sha2::Sha256;

    use super::{
        ConfiguredStorageRoot, ManagedArtifactState, PreparedFileRecord, SCHEMA_VERSION,
        SessionStore, StoreError, StoredTrackerSource, StoredTrackerTransport,
    };
    use crate::ClientSettings;
    use crate::have::{HaveState, MAX_DURABLE_HAVE_STATE_BYTES, MAX_DURABLE_PIECES};
    use crate::{
        AddTorrentBytesRequest, CONTROL_VERSION, Command, ErrorCode, FileIndexRange, FilePriority,
        FileSelectionIntent, ListenerPolicy, RemovalDataPolicy, RemovalState, RequestEnvelope,
        ResponseOutcome, StorageState, TorrentState,
    };

    static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn compact_torrent_selection_projects_nonpadding_files() {
        let files = (0..4)
            .map(|index| MetainfoFile {
                path: vec![index.to_string()],
                length: 1,
                offset: index,
                padding: index == 1,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            super::project_file_selection(&FileSelectionIntent::None, &files)
                .expect("select no payload files"),
            (FilePriority::Skip, Vec::new())
        );
        assert_eq!(
            super::project_file_selection(
                &FileSelectionIntent::WantedRanges {
                    ranges: vec![FileIndexRange {
                        start: 2,
                        end_exclusive: 4,
                    }],
                },
                &files,
            )
            .expect("select a compact wanted range"),
            (FilePriority::Skip, vec![2, 3])
        );
        assert!(
            super::project_file_selection(
                &FileSelectionIntent::WantedRanges {
                    ranges: vec![FileIndexRange {
                        start: 2,
                        end_exclusive: 5,
                    }],
                },
                &files,
            )
            .is_err()
        );
    }

    #[test]
    fn canonical_magnet_preserves_supported_discovery_sources() {
        let parsed = rstorrent_protocol::magnet::Magnet::parse(
            "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213\
             &x.pe=[::1]:6881\
             &tr=UDP%3A%2F%2FTRACKER.EXAMPLE%3A6969%2Fannounce\
             &tr=udp%3A%2F%2F%5B2001%3Adb8%3A%3A1%5D%3A80\
             &tr=https%3A%2F%2Ftracker.example%2Fsecret%3Fpasskey%3Dabc%26x%3D1",
        )
        .expect("parse magnet");

        assert_eq!(
            super::canonical_magnet(&parsed),
            "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213\
             &x.pe=[::1]:6881\
             &tr=UDP%3A%2F%2FTRACKER.EXAMPLE%3A6969%2Fannounce\
             &tr=udp%3A%2F%2F%5B2001%3Adb8%3A%3A1%5D%3A80\
             &tr=https%3A%2F%2Ftracker.example%2Fsecret%3Fpasskey%3Dabc%26x%3D1"
        );
    }

    #[test]
    fn tracker_only_magnet_survives_catalog_reopen() {
        let root = test_root("tracker-magnet");
        let configured = configured_root(&root);
        let source = "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213\
             &tr=UDP%3A%2F%2FTRACKER.EXAMPLE%3A6969%2Fannounce\
             &tr=https%3A%2F%2Ftracker.example%2Fannounce%3Fpasskey%3Dabc";
        let mut store =
            SessionStore::open(&root, "default", std::slice::from_ref(&configured)).expect("open");
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "add-tracker-only".to_owned(),
                expected_revision: None,
                command: Command::AddMagnet {
                    magnet: source.to_owned(),
                    storage_root: "downloads".to_owned(),
                    start_content: true,
                    skip_files: Vec::new(),
                },
            })
            .expect("persist tracker-only source");
        drop(store);

        let reopened = SessionStore::open(&root, "default", &[configured]).expect("reopen");
        let resume = reopened
            .load_resume("000102030405060708090a0b0c0d0e0f10111213")
            .expect("load resumed source");
        assert_eq!(
            resume.magnet,
            "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213\
             &tr=UDP%3A%2F%2FTRACKER.EXAMPLE%3A6969%2Fannounce\
             &tr=https%3A%2F%2Ftracker.example%2Fannounce%3Fpasskey%3Dabc"
        );
        assert_eq!(resume.trackers.len(), 2);
        assert_eq!(resume.trackers[0].transport, StoredTrackerTransport::Udp);
        assert_eq!(resume.trackers[1].transport, StoredTrackerTransport::Https);
        assert_eq!(
            resume.trackers[1].url,
            "https://tracker.example/announce?passkey=abc"
        );
        drop(reopened);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn torrent_bytes_are_atomic_exact_replayable_and_restartable() {
        let root = test_root("torrent-bytes");
        let configured = configured_root(&root);
        let source = torrent_source();
        let projection = rstorrent_protocol::metainfo::Metainfo::project_bytes_with_limits(
            &source,
            rstorrent_protocol::metainfo::EXPLICIT_IMPORT_METAINFO_LIMITS,
        )
        .expect("fixture metainfo");
        let torrent_id = crate::control::encode_info_hash(projection.metainfo.info_hash);
        let raw_info = source[projection.info_span.clone()].to_vec();
        let request = torrent_bytes_request("add-torrent-bytes", &source);
        let mut store =
            SessionStore::open(&root, "default", std::slice::from_ref(&configured)).expect("open");

        let accepted = store
            .handle_torrent_bytes(&request, source.clone())
            .expect("accept source bytes");
        assert!(matches!(accepted.outcome, ResponseOutcome::Success { .. }));
        assert_eq!(accepted.revision, "1");
        let stored_receipt: String = store
            .connection
            .query_row(
                "SELECT request_json FROM request_receipts WHERE request_id = ?1",
                [&request.request_id],
                |row| row.get(0),
            )
            .expect("inspect byte-intake receipt");
        let legacy_receipt = serde_json::to_string(&serde_json::json!({
            "operation": "add_torrent_bytes",
            "request": {
                "version": request.version,
                "request_id": request.request_id,
                "storage_root": request.storage_root,
                "start_content": request.start_content,
                "selection": request.selection,
                "source_length": request.source_length,
                "source_sha256": super::encode_digest(&Sha256::digest(&source)),
            },
        }))
        .expect("encode legacy receipt shape");
        assert_eq!(stored_receipt, legacy_receipt);
        assert_eq!(
            store
                .handle_torrent_bytes(&request, source.clone())
                .expect("replay source bytes"),
            accepted
        );
        let resume = store.load_resume(&torrent_id).expect("load imported row");
        assert_eq!(resume.raw_info.as_deref(), Some(raw_info.as_slice()));
        assert_eq!(resume.state, TorrentState::Paused);
        assert!(!resume.desired_running);
        assert_eq!(resume.magnet, format!("magnet:?xt=urn:btih:{torrent_id}"));
        assert_eq!(resume.trackers.len(), 1);
        assert_eq!(resume.trackers[0].tier, 0);
        assert_eq!(resume.trackers[0].position, 0);
        assert_eq!(resume.trackers[0].transport, StoredTrackerTransport::Udp);
        assert_eq!(resume.trackers[0].source, StoredTrackerSource::Metainfo);
        let (kind, fidelity, exact_source, digest): (String, String, Vec<u8>, Vec<u8>) = store
            .connection
            .query_row(
                "SELECT kind, fidelity, metainfo, sha256
                 FROM torrent_source WHERE info_hash = ?1",
                [projection.metainfo.info_hash.as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("inspect exact source");
        assert_eq!((kind.as_str(), fidelity.as_str()), ("metainfo", "verbatim"));
        assert_eq!(exact_source, source);
        assert_eq!(digest, Sha256::digest(&source).as_slice());
        let tracker: (String, String, String) = store
            .connection
            .query_row(
                "SELECT url, transport, source FROM torrent_trackers
                 WHERE info_hash = ?1",
                [projection.metainfo.info_hash.as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("inspect projected tracker");
        assert_eq!(
            tracker,
            (
                "udp://tracker.example:6969/passkey".to_owned(),
                "udp".to_owned(),
                "metainfo".to_owned(),
            )
        );
        drop(store);

        let reopened = SessionStore::open(&root, "default", &[configured]).expect("reopen");
        let reopened_resume = reopened
            .load_resume(&torrent_id)
            .expect("restart imported row");
        assert_eq!(
            reopened_resume.raw_info.as_deref(),
            Some(raw_info.as_slice())
        );
        assert_eq!(reopened_resume.trackers, resume.trackers);
        drop(reopened);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn torrent_bytes_reject_length_conflict_duplicate_and_stale_without_mutation() {
        let root = test_root("torrent-bytes-errors");
        let configured = configured_root(&root);
        let source = torrent_source();
        let mut store =
            SessionStore::open(&root, "default", std::slice::from_ref(&configured)).expect("open");

        let mut mismatch = torrent_bytes_request("mismatch", &source);
        mismatch.source_length += 1;
        let rejected = store
            .handle_torrent_bytes(&mismatch, source.clone())
            .expect("reject length mismatch");
        assert!(matches!(
            rejected.outcome,
            ResponseOutcome::Error {
                error: crate::ErrorResponse {
                    code: ErrorCode::InvalidRequest,
                    ..
                }
            }
        ));
        assert_eq!(store.revision().expect("unchanged revision"), 0);
        assert!(
            store
                .snapshot()
                .expect("empty snapshot")
                .torrents
                .is_empty()
        );

        let request = torrent_bytes_request("shared-id", &source);
        store
            .handle_torrent_bytes(&request, source.clone())
            .expect("accept first source");
        let mut conflict = request.clone();
        conflict.start_content = true;
        let conflicted = store
            .handle_torrent_bytes(&conflict, source.clone())
            .expect("request conflict");
        assert!(matches!(
            conflicted.outcome,
            ResponseOutcome::Error {
                error: crate::ErrorResponse {
                    code: ErrorCode::RequestConflict,
                    ..
                }
            }
        ));
        let mut changed_source = source.clone();
        let comment = changed_source
            .windows(b"preserve me exact".len())
            .position(|window| window == b"preserve me exact")
            .expect("fixture comment");
        changed_source[comment] = b'P';
        let source_conflicted = store
            .handle_torrent_bytes(&request, changed_source)
            .expect("source bytes conflict");
        assert!(matches!(
            source_conflicted.outcome,
            ResponseOutcome::Error {
                error: crate::ErrorResponse {
                    code: ErrorCode::RequestConflict,
                    ..
                }
            }
        ));
        let duplicate = store
            .handle_torrent_bytes(
                &torrent_bytes_request("duplicate-source", &source),
                source.clone(),
            )
            .expect("duplicate response");
        assert!(matches!(duplicate.outcome, ResponseOutcome::Success { .. }));
        assert!(matches!(
            duplicate.result,
            Some(crate::CommandResult::AddTorrent {
                result: crate::AddTorrentResult {
                    disposition: crate::AddTorrentDisposition::AlreadyPresent,
                    ..
                }
            })
        ));
        let mut stale = torrent_bytes_request("stale-source", &source);
        stale.expected_revision = Some("0".to_owned());
        let stale = store
            .handle_torrent_bytes(&stale, source.clone())
            .expect("stale response");
        assert!(matches!(
            stale.outcome,
            ResponseOutcome::Error {
                error: crate::ErrorResponse {
                    code: ErrorCode::StaleRevision,
                    ..
                }
            }
        ));
        assert_eq!(store.revision().expect("one accepted revision"), 1);
        assert_eq!(store.snapshot().expect("one torrent").torrents.len(), 1);
        drop(store);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    fn test_root(label: &str) -> PathBuf {
        let sequence = NEXT_TEST.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rstorrent-session-{label}-{}-{sequence}",
            std::process::id()
        ))
    }

    fn configured_root(root: &std::path::Path) -> ConfiguredStorageRoot {
        ConfiguredStorageRoot::path("downloads", root.join("payload"))
    }

    fn add_request(request_id: &str) -> RequestEnvelope {
        RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: request_id.to_owned(),
            expected_revision: None,
            command: Command::AddMagnet {
                magnet:
                    "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213&x.pe=127.0.0.1:1"
                        .to_owned(),
                storage_root: "downloads".to_owned(),
                start_content: true,
                skip_files: vec![1, 3],
            },
        }
    }

    fn torrent_source() -> Vec<u8> {
        let tracker = "udp://tracker.example:6969/passkey";
        let info = b"d6:lengthi4e4:name4:test12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let mut source = format!(
            "d8:announce{}:{tracker}7:comment17:preserve me exact4:info",
            tracker.len()
        )
        .into_bytes();
        source.extend_from_slice(info);
        source.push(b'e');
        source
    }

    fn torrent_bytes_request(request_id: &str, source: &[u8]) -> AddTorrentBytesRequest {
        AddTorrentBytesRequest {
            version: CONTROL_VERSION,
            request_id: request_id.to_owned(),
            expected_revision: None,
            storage_root: "downloads".to_owned(),
            start_content: false,
            selection: crate::FileSelectionIntent::All,
            source_length: source.len() as u32,
        }
    }

    fn maximum_torrent_source(fill: u8) -> Vec<u8> {
        let outer_bytes =
            rstorrent_protocol::metainfo::EXPLICIT_IMPORT_METAINFO_LIMITS.max_outer_bytes;
        let info_bytes = outer_bytes - b"d4:info".len() - 1;
        let mut prefix = b"d6:lengthi1e4:name1:a12:piece lengthi1e6:pieces20:".to_vec();
        prefix.extend_from_slice(&[0; 20]);
        prefix.extend_from_slice(b"6:source");
        let mut value_bytes = info_bytes - prefix.len() - 1;
        loop {
            let actual = prefix.len() + value_bytes.to_string().len() + 1 + value_bytes + 1;
            if actual == info_bytes {
                break;
            }
            if actual < info_bytes {
                value_bytes += info_bytes - actual;
            } else {
                value_bytes -= actual - info_bytes;
            }
        }
        let mut source = Vec::with_capacity(outer_bytes);
        source.extend_from_slice(b"d4:info");
        source.extend_from_slice(&prefix);
        source.extend_from_slice(value_bytes.to_string().as_bytes());
        source.push(b':');
        source.resize(source.len() + value_bytes, fill);
        source.extend_from_slice(b"ee");
        assert_eq!(source.len(), outer_bytes);
        source
    }

    fn multi_file_torrent_source(file_count: u32) -> Vec<u8> {
        let mut info = b"d5:filesl".to_vec();
        for index in 0..file_count {
            let path = format!("f{index}");
            info.extend_from_slice(
                format!("d6:lengthi1e4:pathl{}:{path}ee", path.len()).as_bytes(),
            );
        }
        info.extend_from_slice(
            format!("e4:name4:root12:piece lengthi{file_count}e6:pieces20:aaaaaaaaaaaaaaaaaaaae")
                .as_bytes(),
        );
        let mut source = b"d4:info".to_vec();
        source.extend_from_slice(&info);
        source.push(b'e');
        source
    }

    #[test]
    fn ranged_file_priority_mutates_without_an_enumerated_request() {
        let root = test_root("ranged-file-priority");
        let mut store = SessionStore::open(
            &root,
            "profile",
            &[ConfiguredStorageRoot::path(
                "downloads",
                root.join("payload"),
            )],
        )
        .expect("open store");
        let source = multi_file_torrent_source(8);
        store
            .handle_torrent_bytes(&torrent_bytes_request("add-ranges", &source), source)
            .expect("add multi-file torrent");
        let torrent_id = store.snapshot().expect("snapshot").torrents[0]
            .torrent_id
            .clone();
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "set-ranges".to_owned(),
                expected_revision: None,
                command: Command::SetFilePriorityRanges {
                    torrent_id: torrent_id.clone(),
                    ranges: vec![
                        FileIndexRange {
                            start: 1,
                            end_exclusive: 3,
                        },
                        FileIndexRange {
                            start: 5,
                            end_exclusive: 8,
                        },
                    ],
                    priority: FilePriority::Skip,
                },
            })
            .expect("apply range mutation");
        assert_eq!(
            store.load_resume(&torrent_id).expect("resume").skip_files,
            [1, 2, 5, 6, 7]
        );
        drop(store);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn ephemeral_store_is_private_bounded_and_preserves_receipts() {
        let configured = ConfiguredStorageRoot::platform("downloads");
        let mut first =
            SessionStore::open_ephemeral("same-profile", std::slice::from_ref(&configured))
                .expect("open first ephemeral store");
        let second =
            SessionStore::open_ephemeral("same-profile", std::slice::from_ref(&configured))
                .expect("open second ephemeral store");

        assert_eq!(first.database_path(), None);
        assert_eq!(first.revision().expect("first revision"), 0);
        assert_eq!(second.revision().expect("second revision"), 0);
        let journal: String = first
            .connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .expect("journal mode");
        let synchronous: i64 = first
            .connection
            .pragma_query_value(None, "synchronous", |row| row.get(0))
            .expect("synchronous");
        let temp_store: i64 = first
            .connection
            .pragma_query_value(None, "temp_store", |row| row.get(0))
            .expect("temp store");
        let foreign_keys: i64 = first
            .connection
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .expect("foreign keys");
        assert_eq!(journal.to_ascii_lowercase(), "memory");
        assert_eq!(synchronous, 0);
        assert_eq!(temp_store, 2);
        assert_eq!(foreign_keys, 1);
        let usage = first.page_usage().expect("page usage");
        assert!(usage.page_count <= usage.maximum_page_count);
        let maximum_bytes = usage.page_size * usage.maximum_page_count;
        assert!(maximum_bytes <= super::EPHEMERAL_SESSION_MAX_BYTES);
        assert!(super::EPHEMERAL_SESSION_MAX_BYTES - maximum_bytes < usage.page_size);

        let request = add_request("ephemeral-replay");
        let accepted = first.handle_durable(&request).expect("accept request");
        assert_eq!(
            first.handle_durable(&request).expect("replay request"),
            accepted
        );
        let mut conflict = request;
        conflict.command = Command::Pause {
            torrent_id: "000102030405060708090a0b0c0d0e0f10111213".to_owned(),
        };
        assert!(matches!(
            first
                .handle_durable(&conflict)
                .expect("return conflict response")
                .outcome,
            ResponseOutcome::Error {
                error: crate::ErrorResponse {
                    code: ErrorCode::RequestConflict,
                    ..
                }
            }
        ));
        assert_eq!(second.revision().expect("isolated revision"), 0);
        assert!(
            second
                .snapshot()
                .expect("isolated snapshot")
                .torrents
                .is_empty()
        );

        drop(first);
        let fresh = SessionStore::open_ephemeral("same-profile", &[configured])
            .expect("open fresh ephemeral store");
        assert_eq!(fresh.revision().expect("fresh revision"), 0);
        assert!(
            fresh
                .snapshot()
                .expect("fresh snapshot")
                .torrents
                .is_empty()
        );
    }

    #[test]
    fn ephemeral_page_cap_rolls_back_current_metadata_transaction() {
        let configured = ConfiguredStorageRoot::platform("downloads");
        let mut store = SessionStore::open_ephemeral_with_maximum_bytes(
            "bounded",
            std::slice::from_ref(&configured),
            512 * 1024,
        )
        .expect("open bounded ephemeral store");
        let raw_info = single_file_info_for_pieces(100_000, 16_384);
        let torrent_id = crate::control::encode_info_hash(Sha1::digest(&raw_info).into());
        let request = RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: "bounded-add".to_owned(),
            expected_revision: None,
            command: Command::AddMagnet {
                magnet: format!("magnet:?xt=urn:btih:{torrent_id}"),
                storage_root: "downloads".to_owned(),
                start_content: false,
                skip_files: Vec::new(),
            },
        };
        store.handle_durable(&request).expect("add pending magnet");
        let revision = store.revision().expect("revision before exhaustion");

        let error = store
            .record_metadata(&torrent_id, &raw_info)
            .expect_err("metadata must exceed the page cap");
        assert!(error.is_resource_limit(), "unexpected error: {error}");
        assert!(matches!(
            crate::application_error_response(
                "bounded-metadata".to_owned(),
                revision,
                &crate::ApplicationError::Store(error),
            )
            .outcome,
            ResponseOutcome::Error {
                error: crate::ErrorResponse {
                    code: ErrorCode::ResourceLimit,
                    ..
                }
            }
        ));
        assert_eq!(store.revision().expect("rolled back revision"), revision);
        let resume = store
            .load_resume(&torrent_id)
            .expect("store remains responsive");
        assert_eq!(resume.state, TorrentState::AwaitingMetadata);
        assert!(resume.raw_info.is_none());
    }

    #[test]
    #[ignore = "allocates two maximum metainfo sources to profile the live page cap"]
    fn maximum_ephemeral_source_fits_and_following_import_rolls_back() {
        let configured = ConfiguredStorageRoot::platform("downloads");
        let mut store =
            SessionStore::open_ephemeral("maximum-source", std::slice::from_ref(&configured))
                .expect("open default ephemeral store");
        let first_source = maximum_torrent_source(b'a');
        let first_request = torrent_bytes_request("maximum-source-first", &first_source);
        let first = store
            .handle_torrent_bytes(&first_request, first_source)
            .expect("first maximum source fits");
        assert!(matches!(first.outcome, ResponseOutcome::Success { .. }));
        let usage = store.page_usage().expect("first import page usage");
        assert!(usage.page_count < usage.maximum_page_count);
        assert!(usage.page_count * usage.page_size > 120 * 1024 * 1024);
        println!(
            "maximum ephemeral source page use: {} of {} bytes",
            usage.page_count * usage.page_size,
            usage.maximum_page_count * usage.page_size
        );
        let first_lengths: (i64, i64) = store
            .connection
            .query_row(
                "SELECT length(s.metainfo), length(t.raw_info)
                   FROM torrent_source s
                   JOIN torrents t USING (info_hash)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("inspect maximum source and exact info");
        assert_eq!(
            first_lengths,
            (
                rstorrent_protocol::metainfo::EXPLICIT_IMPORT_METAINFO_LIMITS.max_outer_bytes
                    as i64,
                (rstorrent_protocol::metainfo::EXPLICIT_IMPORT_METAINFO_LIMITS.max_outer_bytes
                    - b"d4:info".len()
                    - 1) as i64,
            )
        );

        let revision = store.revision().expect("revision before exhaustion");
        let second_source = maximum_torrent_source(b'b');
        let second_request = torrent_bytes_request("maximum-source-second", &second_source);
        let error = store
            .handle_torrent_bytes(&second_request, second_source)
            .expect_err("following maximum source exceeds the live page cap");
        assert!(error.is_resource_limit(), "unexpected error: {error}");
        assert_eq!(store.revision().expect("rolled back revision"), revision);
        assert_eq!(
            store
                .snapshot()
                .expect("store remains responsive")
                .torrents
                .len(),
            1
        );
        assert_eq!(
            store
                .connection
                .query_row("SELECT COUNT(*) FROM torrent_source", [], |row| {
                    row.get::<_, i64>(0)
                })
                .expect("count intact source rows"),
            1
        );
    }

    fn multi_file_info() -> Vec<u8> {
        let mut info = b"d5:filesld6:lengthi4e4:pathl5:a.bineed6:lengthi4e4:pathl5:b.bineee4:name5:multi12:piece lengthi4e6:pieces40:".to_vec();
        info.extend_from_slice(&[b'a'; 20]);
        info.extend_from_slice(&[b'b'; 20]);
        info.push(b'e');
        info
    }

    fn single_file_info_for_pieces(piece_count: usize, piece_length: u32) -> Vec<u8> {
        let total_length = u64::try_from(piece_count)
            .expect("piece count fits u64")
            .checked_mul(u64::from(piece_length))
            .expect("fixture length");
        let hash_bytes = piece_count.checked_mul(20).expect("fixture hash bytes");
        let mut info = format!(
            "d6:lengthi{total_length}e4:name1:x12:piece lengthi{piece_length}e6:pieces{hash_bytes}:"
        )
        .into_bytes();
        info.resize(info.len() + hash_bytes, 0);
        info.push(b'e');
        info
    }

    #[test]
    fn metadata_only_add_and_binary_file_priority_are_durable() {
        let root = test_root("file-priority");
        let configured = configured_root(&root);
        let mut store = SessionStore::open(&root, "default", &[configured]).expect("open store");
        let raw_info = multi_file_info();
        let torrent_id = crate::control::encode_info_hash(Sha1::digest(&raw_info).into());
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "metadata-only-add".to_owned(),
                expected_revision: None,
                command: Command::AddMagnet {
                    magnet: format!("magnet:?xt=urn:btih:{torrent_id}"),
                    storage_root: "downloads".to_owned(),
                    start_content: false,
                    skip_files: Vec::new(),
                },
            })
            .expect("add metadata-only torrent");
        let pending = store.load_resume(&torrent_id).expect("load pending add");
        assert!(!pending.desired_running);
        assert_eq!(pending.state, TorrentState::AwaitingMetadata);
        assert_eq!(pending.storage_state, StorageState::None);

        store
            .record_metadata(&torrent_id, &raw_info)
            .expect("record metadata");
        let ready = store
            .load_resume(&torrent_id)
            .expect("load metadata-only add");
        assert_eq!(ready.state, TorrentState::Paused);
        assert_eq!(ready.storage_state, StorageState::None);

        let skip = RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: "skip-file".to_owned(),
            expected_revision: None,
            command: Command::SetFilePriority {
                torrent_id: torrent_id.clone(),
                file_indices: vec![1],
                priority: FilePriority::Skip,
            },
        };
        let skipped = store.handle_durable(&skip).expect("skip file");
        assert!(matches!(skipped.outcome, ResponseOutcome::Success { .. }));
        let selected = store.load_resume(&torrent_id).expect("load selection");
        assert_eq!(selected.skip_files, vec![1]);
        assert_eq!(selected.state, TorrentState::Paused);
        assert_eq!(
            store.handle_durable(&skip).expect("replay skip receipt"),
            skipped
        );

        drop(store);
        let reopened =
            SessionStore::open(&root, "default", &[configured_root(&root)]).expect("reopen store");
        assert_eq!(
            reopened
                .load_resume(&torrent_id)
                .expect("load reopened selection")
                .skip_files,
            vec![1]
        );
        drop(reopened);
        fs::remove_dir_all(root).expect("remove profile");
    }

    #[test]
    fn select_only_survives_metadata_and_duplicate_expands_once() {
        let root = test_root("select-only");
        let configured = configured_root(&root);
        let mut store = SessionStore::open(&root, "default", &[configured]).expect("open store");
        let raw_info = multi_file_info();
        let torrent_id = crate::control::encode_info_hash(Sha1::digest(&raw_info).into());
        let add = |request_id: &str, so: &str| RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: request_id.to_owned(),
            expected_revision: None,
            command: Command::AddMagnet {
                magnet: format!("magnet:?xt=urn:btih:{torrent_id}&so={so}"),
                storage_root: "ignored-on-duplicate".to_owned(),
                start_content: true,
                skip_files: Vec::new(),
            },
        };
        let mut first = add("select-one", "1");
        if let Command::AddMagnet { storage_root, .. } = &mut first.command {
            *storage_root = "downloads".to_owned();
        }
        let first = store.handle_durable(&first).expect("add pending selection");
        assert!(matches!(
            first.result,
            Some(crate::CommandResult::AddTorrent {
                result: crate::AddTorrentResult {
                    disposition: crate::AddTorrentDisposition::Added,
                    ..
                }
            })
        ));
        store
            .record_metadata(&torrent_id, &raw_info)
            .expect("resolve selection");
        let snapshot = store.snapshot().expect("resolved snapshot");
        assert_eq!(snapshot.torrents[0].selection_default, FilePriority::Skip);
        assert_eq!(snapshot.torrents[0].selection_exceptions, [1]);
        assert!(snapshot.torrents[0].skip_files.is_empty());

        let expanded = store
            .handle_durable(&add("expand-zero", "0"))
            .expect("expand");
        assert!(matches!(
            expanded.result,
            Some(crate::CommandResult::AddTorrent {
                result: crate::AddTorrentResult {
                    disposition: crate::AddTorrentDisposition::SelectionExpanded {
                        newly_wanted_count: Some(1)
                    },
                    ..
                }
            })
        ));
        let revision = store.revision().expect("expanded revision");
        let no_op = store
            .handle_durable(&add("expand-zero-again", "0"))
            .expect("no-op");
        assert!(matches!(
            no_op.result,
            Some(crate::CommandResult::AddTorrent {
                result: crate::AddTorrentResult {
                    disposition: crate::AddTorrentDisposition::AlreadyPresent,
                    ..
                }
            })
        ));
        assert_eq!(store.revision().expect("unchanged revision"), revision);
        assert_eq!(
            store.snapshot().expect("expanded snapshot").torrents[0].selection_exceptions,
            [0, 1]
        );
        drop(store);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn select_only_exception_budget_rejects_metadata_atomically() {
        let root = test_root("select-only-budget");
        let configured = configured_root(&root);
        let mut store = SessionStore::open(&root, "default", &[configured]).expect("open store");
        let source =
            multi_file_torrent_source((crate::control::MAX_FILE_SELECTION_ENTRIES + 1) as u32);
        let projection =
            Metainfo::project_bytes_with_limits(&source, EXPLICIT_IMPORT_METAINFO_LIMITS)
                .expect("bounded metainfo");
        let raw_info = &source[projection.info_span];
        let torrent_id = crate::control::encode_info_hash(projection.metainfo.info_hash);
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "select-budget".to_owned(),
                expected_revision: None,
                command: Command::AddMagnet {
                    magnet: format!(
                        "magnet:?xt=urn:btih:{torrent_id}&so=0-{}",
                        crate::control::MAX_FILE_SELECTION_ENTRIES
                    ),
                    storage_root: "downloads".to_owned(),
                    start_content: true,
                    skip_files: Vec::new(),
                },
            })
            .expect("persist compact pending range");
        let revision = store.revision().expect("add revision");
        assert!(matches!(
            store.record_metadata(&torrent_id, raw_info),
            Err(StoreError::ResourceLimit {
                resource: "file selection exceptions",
                ..
            })
        ));
        assert_eq!(store.revision().expect("unchanged revision"), revision);
        assert!(!store.snapshot().expect("pending snapshot").torrents[0].metadata_available);
        drop(store);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn all_skipped_idles_running_intent_and_normal_restarts_checking() {
        let root = test_root("all-skipped");
        let configured = configured_root(&root);
        let mut store = SessionStore::open(&root, "default", &[configured]).expect("open store");
        let raw_info = multi_file_info();
        let torrent_id = crate::control::encode_info_hash(Sha1::digest(&raw_info).into());
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "running-add".to_owned(),
                expected_revision: None,
                command: Command::AddMagnet {
                    magnet: format!("magnet:?xt=urn:btih:{torrent_id}"),
                    storage_root: "downloads".to_owned(),
                    start_content: true,
                    skip_files: Vec::new(),
                },
            })
            .expect("add running torrent");
        store
            .record_metadata(&torrent_id, &raw_info)
            .expect("record metadata");
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "skip-all".to_owned(),
                expected_revision: None,
                command: Command::SetFilePriority {
                    torrent_id: torrent_id.clone(),
                    file_indices: vec![0, 1],
                    priority: FilePriority::Skip,
                },
            })
            .expect("skip all files");
        let idle = store
            .load_resume(&torrent_id)
            .expect("load all-skipped state");
        assert!(idle.desired_running);
        assert_eq!(idle.state, TorrentState::Paused);
        assert_eq!(idle.skip_files, vec![0, 1]);

        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "normal-one".to_owned(),
                expected_revision: None,
                command: Command::SetFilePriority {
                    torrent_id: torrent_id.clone(),
                    file_indices: vec![1],
                    priority: FilePriority::Normal,
                },
            })
            .expect("restore normal priority");
        let checking = store.load_resume(&torrent_id).expect("load checking state");
        assert!(checking.desired_running);
        assert_eq!(checking.state, TorrentState::Checking);
        assert_eq!(checking.skip_files, vec![0]);
        drop(store);
        fs::remove_dir_all(root).expect("remove profile");
    }

    #[test]
    fn creates_reopens_and_refuses_newer_schema() {
        let root = test_root("schema");
        let configured = configured_root(&root);
        let store =
            SessionStore::open(&root, "default", std::slice::from_ref(&configured)).expect("open");
        assert_eq!(store.revision().expect("revision"), 0);
        let database_path = store
            .database_path()
            .expect("durable database path")
            .to_owned();
        drop(store);

        let store = SessionStore::open(&root, "default", std::slice::from_ref(&configured))
            .expect("reopen");
        assert_eq!(store.revision().expect("revision"), 0);
        drop(store);

        let connection = Connection::open(database_path).expect("open raw database");
        connection
            .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .expect("set newer version");
        drop(connection);
        assert!(matches!(
            SessionStore::open(&root, "default", &[configured]),
            Err(StoreError::UnsupportedSchema { .. })
        ));
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn selected_roots_default_deduplicate_persist_and_block_referenced_removal() {
        let root = test_root("selected-roots");
        let payload = root.join("chosen-payload");
        fs::create_dir_all(&payload).expect("create chosen payload root");
        let payload = fs::canonicalize(&payload).expect("canonical payload root");
        let mut store = SessionStore::open(&root, "default", &[]).expect("open fresh profile");
        let fresh = store.snapshot().expect("fresh snapshot");
        assert!(fresh.storage.roots.is_empty());
        assert_eq!(fresh.storage.default_root, None);
        assert!(fresh.storage.show_add_options);

        let (revision, installed) = store
            .install_path_storage_root("root_00000000000000000000000000000000", "Chosen", &payload)
            .expect("install selected root");
        assert_eq!(revision, 1);
        assert_eq!(installed, "root_00000000000000000000000000000000");
        let selected = store.snapshot().expect("selected snapshot");
        assert_eq!(
            selected.storage.default_root.as_deref(),
            Some(installed.as_str())
        );
        assert_eq!(selected.storage.roots.len(), 1);
        assert_eq!(
            selected.storage.roots[0].availability,
            crate::StorageRootAvailability::Available
        );

        let (deduplicated_revision, deduplicated) = store
            .install_path_storage_root("root_11111111111111111111111111111111", "Alias", &payload)
            .expect("deduplicate selected root");
        assert_eq!(deduplicated_revision, revision);
        assert_eq!(deduplicated, installed);

        let mut add = add_request("add-selected-root");
        let Command::AddMagnet { storage_root, .. } = &mut add.command else {
            unreachable!("test request is add magnet")
        };
        *storage_root = installed.clone();
        store.handle_durable(&add).expect("bind torrent to root");
        let removal = store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "remove-referenced-root".to_owned(),
                expected_revision: None,
                command: Command::RemoveStorageRoot {
                    storage_root: installed.clone(),
                },
            })
            .expect("referenced removal response");
        assert!(matches!(
            removal.outcome,
            ResponseOutcome::Error {
                error: crate::ErrorResponse {
                    code: ErrorCode::StorageRootInUse,
                    ..
                }
            }
        ));
        drop(store);

        let reopened = SessionStore::open(&root, "default", &[]).expect("reopen profile");
        let persisted = reopened.snapshot().expect("persisted root snapshot");
        assert_eq!(
            persisted.storage.default_root.as_deref(),
            Some(installed.as_str())
        );
        assert_eq!(persisted.torrents[0].storage_root, installed);
        drop(reopened);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn missing_selected_root_reopens_unavailable_without_recreation() {
        let root = test_root("unavailable-root");
        let payload = root.join("removable-payload");
        fs::create_dir_all(&payload).expect("create selected root");
        let payload = fs::canonicalize(&payload).expect("canonical selected root");
        let mut store = SessionStore::open(&root, "default", &[]).expect("open fresh profile");
        store
            .install_path_storage_root(
                "root_22222222222222222222222222222222",
                "Removable",
                &payload,
            )
            .expect("install selected root");
        drop(store);
        fs::remove_dir(&payload).expect("make selected root unavailable");

        let reopened = SessionStore::open(&root, "default", &[]).expect("reopen profile");
        let snapshot = reopened.snapshot().expect("unavailable root snapshot");
        assert_eq!(snapshot.storage.roots.len(), 1);
        assert_eq!(
            snapshot.storage.roots[0].availability,
            crate::StorageRootAvailability::Unavailable
        );
        assert!(!payload.exists());
        drop(reopened);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn dht_snapshot_round_trips_and_rejects_corrupt_rows() {
        let root = test_root("dht-state");
        let configured = configured_root(&root);
        let mut store =
            SessionStore::open(&root, "default", std::slice::from_ref(&configured)).expect("open");
        assert_eq!(store.load_dht_snapshot().expect("empty state"), None);
        let snapshot = DhtSnapshot {
            version: DHT_SNAPSHOT_VERSION,
            node_id: NodeId([1; 20]),
            nodes_v4: vec![NodeContact {
                id: NodeId([2; 20]),
                address: DhtEndpoint::new(DhtIp::V4([127, 0, 0, 1]), 6881),
            }],
            nodes_v6: Vec::new(),
        };
        store
            .save_dht_snapshot(snapshot.clone())
            .expect("save DHT state");
        assert_eq!(
            store.load_dht_snapshot().expect("load DHT state"),
            Some(snapshot)
        );
        let database = store
            .database_path()
            .expect("durable database path")
            .to_owned();
        drop(store);
        let connection = Connection::open(&database).expect("inspect database");
        connection
            .execute(
                "UPDATE dht_state SET format_version = 99 WHERE singleton = 1",
                [],
            )
            .expect("corrupt version");
        drop(connection);
        let store = SessionStore::open(&root, "default", &[configured]).expect("reopen");
        assert!(matches!(
            store.load_dht_snapshot(),
            Err(StoreError::DurableState(_))
        ));
        drop(store);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn migrates_version_one_catalog_transactionally() {
        let root = test_root("schema-v1");
        let configured = configured_root(&root);
        let mut store =
            SessionStore::open(&root, "default", std::slice::from_ref(&configured)).expect("open");
        let request = add_request("add-before-migration");
        let expected = store.handle_durable(&request).expect("add durable torrent");
        let database_path = store
            .database_path()
            .expect("durable database path")
            .to_owned();
        drop(store);

        let connection = Connection::open(&database_path).expect("open raw database");
        connection
            .execute_batch(
                "DROP TABLE prepared_files;
                 DROP TABLE dht_nodes;
                 DROP TABLE dht_state;
                 DROP TABLE removal_jobs;
                 DROP TABLE client_settings;
                 DROP TABLE storage_settings;
                 ALTER TABLE storage_roots DROP COLUMN kind;
                 ALTER TABLE storage_roots DROP COLUMN label;
                 PRAGMA user_version = 1;",
            )
            .expect("downgrade fixture to the version-one shape");
        drop(connection);

        let mut migrated =
            SessionStore::open(&root, "default", &[configured]).expect("migrate version one");
        assert_eq!(migrated.snapshot().expect("snapshot").torrents.len(), 1);
        assert_eq!(
            migrated
                .handle_durable(&request)
                .expect("receipt survived migration"),
            expected
        );
        let connection = Connection::open(database_path).expect("inspect migrated database");
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("read schema version");
        assert_eq!(version, SCHEMA_VERSION);
        let prepared_table: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'prepared_files'",
                [],
                |row| row.get(0),
            )
            .expect("inspect prepared table");
        assert_eq!(prepared_table, 1);
        drop(connection);
        drop(migrated);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn migrates_version_three_catalog_with_retention_defaults() {
        let root = test_root("schema-v3");
        let configured = configured_root(&root);
        let mut store =
            SessionStore::open(&root, "default", std::slice::from_ref(&configured)).expect("open");
        store
            .handle_durable(&add_request("add-before-v3-migration"))
            .expect("add durable torrent");
        let database_path = store
            .database_path()
            .expect("durable database path")
            .to_owned();
        drop(store);

        let connection = Connection::open(&database_path).expect("open raw database");
        connection
            .execute_batch(
                "DROP TABLE removal_jobs;
                 DROP TABLE client_settings;
                 DROP TABLE storage_settings;
                 ALTER TABLE storage_roots DROP COLUMN kind;
                 ALTER TABLE storage_roots DROP COLUMN label;
                 ALTER TABLE torrents DROP COLUMN archived;
                 ALTER TABLE torrents DROP COLUMN publication_name;
                 ALTER TABLE torrents DROP COLUMN managed_artifacts;
                 PRAGMA user_version = 3;",
            )
            .expect("downgrade fixture to version-three shape");
        drop(connection);

        let migrated = SessionStore::open(&root, "default", &[configured]).expect("migrate v3");
        let snapshot = migrated.snapshot().expect("migrated snapshot");
        assert_eq!(snapshot.torrents.len(), 1);
        assert!(!snapshot.torrents[0].archived);
        assert_eq!(snapshot.torrents[0].removal_state, None);
        let connection = Connection::open(database_path).expect("inspect migrated database");
        let removal_table: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'removal_jobs'",
                [],
                |row| row.get(0),
            )
            .expect("inspect removal table");
        assert_eq!(removal_table, 1);
        drop(connection);
        drop(migrated);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn migrates_version_five_without_guessing_a_publication_name() {
        let root = test_root("schema-v5-publication");
        let configured = configured_root(&root);
        let raw_info =
            b"d6:lengthi4e4:name4:test12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = crate::control::encode_info_hash(info_hash);
        let mut store =
            SessionStore::open(&root, "default", std::slice::from_ref(&configured)).expect("open");
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "add-before-v5-migration".to_owned(),
                expected_revision: None,
                command: Command::AddMagnet {
                    magnet: format!("magnet:?xt=urn:btih:{torrent_id}"),
                    storage_root: "downloads".to_owned(),
                    start_content: true,
                    skip_files: Vec::new(),
                },
            })
            .expect("add torrent");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        let database_path = store
            .database_path()
            .expect("durable database path")
            .to_owned();
        drop(store);

        let connection = Connection::open(&database_path).expect("open raw database");
        connection
            .execute_batch(
                "DROP TABLE client_settings;
                 ALTER TABLE torrents DROP COLUMN publication_name;
                 ALTER TABLE torrents DROP COLUMN managed_artifacts;
                 PRAGMA user_version = 5;",
            )
            .expect("downgrade fixture to version-five shape");
        drop(connection);

        let mut migrated = SessionStore::open(&root, "default", &[configured]).expect("migrate v5");
        let resume = migrated.load_resume(&torrent_id).expect("load legacy row");
        assert_eq!(resume.raw_info.as_deref(), Some(raw_info.as_slice()));
        assert_eq!(resume.publication_name, None);
        migrated
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "remove-migrated-v5".to_owned(),
                expected_revision: None,
                command: Command::RemoveTorrent {
                    torrent_id: torrent_id.clone(),
                    data: RemovalDataPolicy::DeleteManaged,
                },
            })
            .expect("begin migrated removal");
        assert_eq!(
            migrated
                .load_removal(&torrent_id)
                .expect("load migrated removal")
                .managed_artifacts,
            ManagedArtifactState::Legacy
        );
        let version: i64 = Connection::open(database_path)
            .expect("inspect migrated database")
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("read schema version");
        assert_eq!(version, SCHEMA_VERSION);
        drop(migrated);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn migrates_version_six_bounds_without_losing_related_rows() {
        let root = test_root("schema-v6-bounds");
        let configured = configured_root(&root);
        let mut store =
            SessionStore::open(&root, "default", std::slice::from_ref(&configured)).expect("open");
        let request = add_request("add-before-v6-migration");
        store.handle_durable(&request).expect("add torrent");
        let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "remove-before-v6-migration".to_owned(),
                expected_revision: None,
                command: Command::RemoveTorrent {
                    torrent_id: torrent_id.to_owned(),
                    data: RemovalDataPolicy::Keep,
                },
            })
            .expect("begin removal");
        let database_path = store
            .database_path()
            .expect("durable database path")
            .to_owned();
        drop(store);

        let connection = Connection::open(&database_path).expect("open raw database");
        let info_hash = crate::control::decode_info_hash(torrent_id).expect("fixture identity");
        connection
            .execute(
                "INSERT INTO prepared_files(info_hash, file_index, length, sha1)
                 VALUES (?1, 0, 0, ?2)",
                rusqlite::params![info_hash.as_slice(), [0_u8; 20].as_slice()],
            )
            .expect("insert prepared row");
        connection
            .execute("DROP TABLE client_settings", [])
            .expect("remove version-nine settings table");
        connection
            .pragma_update(None, "user_version", 6)
            .expect("mark schema six");
        drop(connection);

        let migrated = SessionStore::open(&root, "default", &[configured]).expect("migrate v6");
        assert_eq!(migrated.snapshot().expect("snapshot").torrents.len(), 1);
        assert_eq!(
            migrated
                .load_removal(torrent_id)
                .expect("preserved removal")
                .policy,
            RemovalDataPolicy::Keep
        );
        let connection = Connection::open(database_path).expect("inspect migration");
        for (table, expected) in [
            ("file_selection", 2_i64),
            ("prepared_files", 1_i64),
            ("removal_jobs", 1_i64),
        ] {
            let count: i64 = connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .expect("count preserved rows");
            assert_eq!(count, expected, "{table}");
        }
        let schema: String = connection
            .query_row(
                "SELECT sql FROM sqlite_master
                 WHERE type = 'table' AND name = 'torrents'",
                [],
                |row| row.get(0),
            )
            .expect("read torrent schema");
        assert!(schema.contains("piece_count <= 2097152"));
        assert!(schema.contains("length(have_state) <= 262178"));
        drop(connection);
        drop(migrated);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn durable_geometry_and_resource_limits_are_exact() {
        let root = test_root("durable-resource-limits");
        let configured = configured_root(&root);
        let mut store = SessionStore::open(&root, "default", &[configured]).expect("open store");
        let raw_info = single_file_info_for_pieces(40_960, 256 * 1024);
        let torrent_id = crate::control::encode_info_hash(Sha1::digest(&raw_info).into());
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "add-large-geometry".to_owned(),
                expected_revision: None,
                command: Command::AddMagnet {
                    magnet: format!("magnet:?xt=urn:btih:{torrent_id}"),
                    storage_root: "downloads".to_owned(),
                    start_content: false,
                    skip_files: Vec::new(),
                },
            })
            .expect("add geometry");
        store
            .record_metadata(&torrent_id, &raw_info)
            .expect("persist 40,960-piece geometry");
        assert_eq!(
            store
                .load_resume(&torrent_id)
                .expect("load geometry")
                .have
                .expect("have state")
                .pieces()
                .len(),
            40_960
        );

        assert_eq!(
            super::bounded_piece_count(MAX_DURABLE_PIECES as i64).expect("exact piece limit"),
            MAX_DURABLE_PIECES
        );
        assert!(matches!(
            super::bounded_piece_count((MAX_DURABLE_PIECES + 1) as i64),
            Err(StoreError::ResourceLimit {
                resource: "piece count",
                actual,
                maximum,
            }) if actual == MAX_DURABLE_PIECES + 1 && maximum == MAX_DURABLE_PIECES
        ));
        super::validate_have_state_length(&vec![0; MAX_DURABLE_HAVE_STATE_BYTES])
            .expect("exact have-state limit");
        assert!(matches!(
            super::validate_have_state_length(&vec![0; MAX_DURABLE_HAVE_STATE_BYTES + 1]),
            Err(StoreError::ResourceLimit {
                resource: "have-state bytes",
                actual,
                maximum,
            }) if actual == MAX_DURABLE_HAVE_STATE_BYTES + 1
                && maximum == MAX_DURABLE_HAVE_STATE_BYTES
        ));

        let revision = store.revision().expect("revision before rejection");
        let oversized = vec![0; super::DURABLE_METAINFO_LIMITS.max_info_bytes + 1];
        assert!(matches!(
            store.record_metadata(&torrent_id, &oversized),
            Err(StoreError::ResourceLimit {
                resource: "raw_info bytes",
                actual,
                maximum,
            }) if actual == oversized.len()
                && maximum == super::DURABLE_METAINFO_LIMITS.max_info_bytes
        ));
        assert_eq!(store.revision().expect("unchanged revision"), revision);

        let database_path = store
            .database_path()
            .expect("durable database path")
            .to_owned();
        drop(store);
        let connection = Connection::open(database_path).expect("inspect exact schema bounds");
        let exact_hash = [0x55_u8; 20];
        let exact_have = vec![0_u8; MAX_DURABLE_HAVE_STATE_BYTES];
        connection
            .execute(
                "INSERT INTO torrents(
                    info_hash, magnet, storage_root, desired_state, state,
                    storage_state, piece_count, have_state, archived,
                    created_revision, updated_revision
                 ) VALUES (
                    ?1, 'magnet:', 'downloads', 'paused', 'paused', 'none',
                    ?2, ?3, 0, 0, 0
                 )",
                rusqlite::params![exact_hash.as_slice(), MAX_DURABLE_PIECES as i64, exact_have],
            )
            .expect("schema accepts exact durable bounds");
        assert!(
            connection
                .execute(
                    "UPDATE torrents SET piece_count = ?1 WHERE info_hash = ?2",
                    rusqlite::params![(MAX_DURABLE_PIECES + 1) as i64, exact_hash.as_slice()],
                )
                .is_err()
        );
        assert!(
            connection
                .execute(
                    "UPDATE torrents SET have_state = ?1 WHERE info_hash = ?2",
                    rusqlite::params![
                        vec![0_u8; MAX_DURABLE_HAVE_STATE_BYTES + 1],
                        exact_hash.as_slice()
                    ],
                )
                .is_err()
        );
        drop(connection);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn corrupt_database_is_reported_without_recreation() {
        let root = test_root("corrupt-database");
        fs::create_dir_all(&root).expect("create profile root");
        let database = root.join("session.db");
        let corrupt = b"not a SQLite database";
        fs::write(&database, corrupt).expect("write corrupt database");
        let configured = configured_root(&root);
        assert!(matches!(
            SessionStore::open(&root, "default", &[configured]),
            Err(StoreError::Sqlite(_))
        ));
        assert_eq!(
            fs::read(database).expect("read preserved corrupt database"),
            corrupt
        );
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn deduplicates_mutations_and_rejects_conflicts_and_stale_revisions() {
        let root = test_root("receipts");
        let configured = configured_root(&root);
        let mut store =
            SessionStore::open(&root, "default", &[configured]).expect("open session store");
        let request = add_request("add-1");
        let first = store.handle_durable(&request).expect("add");
        assert_eq!(first.revision, "1");
        assert_eq!(store.handle_durable(&request).expect("retry"), first);

        let mut conflict = request.clone();
        conflict.command = Command::Pause {
            torrent_id: "000102030405060708090a0b0c0d0e0f10111213".to_owned(),
        };
        assert!(matches!(
            store.handle_durable(&conflict).expect("conflict").outcome,
            ResponseOutcome::Error {
                error: crate::ErrorResponse {
                    code: ErrorCode::RequestConflict,
                    ..
                }
            }
        ));

        let stale = RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: "pause-stale".to_owned(),
            expected_revision: Some("0".to_owned()),
            command: conflict.command,
        };
        assert!(matches!(
            store.handle_durable(&stale).expect("stale").outcome,
            ResponseOutcome::Error {
                error: crate::ErrorResponse {
                    code: ErrorCode::StaleRevision,
                    ..
                }
            }
        ));
        assert_eq!(store.revision().expect("unchanged revision"), 1);
        drop(store);

        let mut reopened =
            SessionStore::open(&root, "default", &[]).expect("reopen without active root mapping");
        assert_eq!(
            reopened.handle_durable(&request).expect("durable retry"),
            first
        );
        drop(reopened);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn replays_pre_retention_request_receipts_with_safe_defaults() {
        let root = test_root("legacy-receipt");
        let configured = configured_root(&root);
        let request = add_request("legacy-add");
        let mut store = SessionStore::open(&root, "default", std::slice::from_ref(&configured))
            .expect("open session store");
        store.handle_durable(&request).expect("add torrent");
        let database_path = store
            .database_path()
            .expect("durable database path")
            .to_owned();
        drop(store);

        let connection = Connection::open(&database_path).expect("open receipt database");
        let stored_response: String = connection
            .query_row(
                "SELECT response_json FROM request_receipts WHERE request_id = ?1",
                [&request.request_id],
                |row| row.get(0),
            )
            .expect("read stored response");
        let mut legacy_response: serde_json::Value =
            serde_json::from_str(&stored_response).expect("decode stored response fixture");
        let torrent = legacy_response
            .pointer_mut("/snapshot/torrents/0")
            .and_then(serde_json::Value::as_object_mut)
            .expect("stored response torrent");
        assert!(torrent.remove("archived").is_some());
        assert!(torrent.remove("delete_managed_data_supported").is_some());
        connection
            .execute(
                "UPDATE request_receipts SET response_json = ?1 WHERE request_id = ?2",
                rusqlite::params![
                    serde_json::to_string(&legacy_response).expect("encode legacy response"),
                    &request.request_id
                ],
            )
            .expect("install legacy response fixture");
        drop(connection);

        let mut reopened = SessionStore::open(&root, "default", &[configured]).expect("reopen");
        let replay = reopened
            .handle_durable(&request)
            .expect("replay legacy receipt");
        let ResponseOutcome::Success { snapshot } = replay.outcome else {
            panic!("legacy receipt must remain a successful response");
        };
        let replayed_torrent = snapshot.torrents.first().expect("replayed torrent");
        assert!(!replayed_torrent.archived);
        assert_eq!(replayed_torrent.removal_state, None);
        assert!(!replayed_torrent.delete_managed_data_supported);
        drop(reopened);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn archive_and_removal_generations_are_durable_and_idempotent() {
        let root = test_root("retention");
        let configured = configured_root(&root);
        let mut store = SessionStore::open(&root, "default", &[configured]).expect("open");
        let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
        store
            .handle_durable(&add_request("add-retention"))
            .expect("add torrent");

        let archive = RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: "archive".to_owned(),
            expected_revision: None,
            command: Command::Archive {
                torrent_id: torrent_id.to_owned(),
            },
        };
        store.handle_durable(&archive).expect("archive");
        let archived_revision = store.revision().expect("archived revision");
        assert!(store.snapshot().expect("snapshot").torrents[0].archived);
        store
            .handle_durable(&RequestEnvelope {
                request_id: "archive-again".to_owned(),
                ..archive.clone()
            })
            .expect("idempotent archive");
        assert_eq!(store.revision().expect("unchanged"), archived_revision);

        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "restore-archive".to_owned(),
                expected_revision: None,
                command: Command::RestoreArchive {
                    torrent_id: torrent_id.to_owned(),
                },
            })
            .expect("restore archive");
        assert!(!store.snapshot().expect("snapshot").torrents[0].archived);

        let remove = RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: "remove".to_owned(),
            expected_revision: None,
            command: Command::RemoveTorrent {
                torrent_id: torrent_id.to_owned(),
                data: RemovalDataPolicy::DeleteManaged,
            },
        };
        let accepted = store.handle_durable(&remove).expect("request removal");
        let removal = store.load_removal(torrent_id).expect("load removal");
        assert_eq!(removal.policy, RemovalDataPolicy::DeleteManaged);
        assert_eq!(removal.state, RemovalState::Pending);
        assert_eq!(
            store.snapshot().expect("snapshot").torrents[0].removal_state,
            Some(RemovalState::Pending)
        );
        assert!(matches!(
            store
                .handle_durable(&RequestEnvelope {
                    request_id: "resume-removing".to_owned(),
                    command: Command::Resume {
                        torrent_id: torrent_id.to_owned(),
                    },
                    ..remove.clone()
                })
                .expect("reject resume")
                .outcome,
            ResponseOutcome::Error { .. }
        ));

        store
            .set_removal_state(
                torrent_id,
                &removal.operation_id,
                RemovalState::Failed,
                Some("provider unavailable"),
            )
            .expect("record failure");
        drop(store);
        let mut reopened = SessionStore::open(&root, "default", &[]).expect("reopen");
        let failed = reopened.load_removal(torrent_id).expect("durable failure");
        assert_eq!(failed.state, RemovalState::Failed);
        assert_eq!(failed.error.as_deref(), Some("provider unavailable"));
        assert!(
            reopened
                .finalize_removal(torrent_id, &failed.operation_id)
                .is_err()
        );

        reopened
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "remove-keep-after-failure".to_owned(),
                expected_revision: None,
                command: Command::RemoveTorrent {
                    torrent_id: torrent_id.to_owned(),
                    data: RemovalDataPolicy::Keep,
                },
            })
            .expect("rearm removal");
        let rearmed = reopened.load_removal(torrent_id).expect("rearmed job");
        assert_ne!(rearmed.operation_id, removal.operation_id);
        assert_eq!(rearmed.policy, RemovalDataPolicy::Keep);
        assert!(
            reopened
                .finalize_removal(torrent_id, &removal.operation_id)
                .is_err()
        );
        reopened
            .finalize_removal(torrent_id, &rearmed.operation_id)
            .expect("finalize current generation");
        assert!(
            reopened
                .snapshot()
                .expect("empty snapshot")
                .torrents
                .is_empty()
        );
        assert_eq!(
            reopened.handle_durable(&remove).expect("receipt replay"),
            accepted
        );
        drop(reopened);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn records_only_hash_matching_metadata_and_have_state() {
        let root = test_root("metadata");
        let configured = configured_root(&root);
        let mut store =
            SessionStore::open(&root, "default", &[configured]).expect("open session store");
        let raw_info =
            b"d6:lengthi4e4:name4:test12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = crate::control::encode_info_hash(info_hash);
        let request = RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: "add".to_owned(),
            expected_revision: None,
            command: Command::AddMagnet {
                magnet: format!("magnet:?xt=urn:btih:{torrent_id}&x.pe=127.0.0.1:1"),
                storage_root: "downloads".to_owned(),
                start_content: true,
                skip_files: Vec::new(),
            },
        };
        store.handle_durable(&request).expect("add source");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        store.record_piece(&torrent_id, 0).expect("record piece");
        let resume = store.load_resume(&torrent_id).expect("load resume");
        assert_eq!(resume.raw_info.as_deref(), Some(raw_info.as_slice()));
        assert_eq!(resume.publication_name.as_deref(), Some("test"));
        assert_eq!(resume.have.expect("have state").pieces(), &[true]);
        assert_eq!(
            store.snapshot().expect("snapshot").torrents[0].state,
            TorrentState::Downloading
        );

        let wrong_id = "000102030405060708090a0b0c0d0e0f10111213";
        assert!(matches!(
            store.record_metadata(wrong_id, raw_info),
            Err(StoreError::DurableState(_))
        ));

        let reserved_name = format!(
            ".{}.rstorrent-staging",
            "0123456789abcdef0123456789abcdef01234567"
        );
        let mut reserved_info = format!(
            "d6:lengthi4e4:name{}:{}12:piece lengthi4e6:pieces20:",
            reserved_name.len(),
            reserved_name
        )
        .into_bytes();
        reserved_info.extend_from_slice(b"aaaaaaaaaaaaaaaaaaaae");
        let reserved_hash: [u8; 20] = Sha1::digest(&reserved_info).into();
        let reserved_id = crate::control::encode_info_hash(reserved_hash);
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "add-reserved-name".to_owned(),
                expected_revision: None,
                command: Command::AddMagnet {
                    magnet: format!("magnet:?xt=urn:btih:{reserved_id}"),
                    storage_root: "downloads".to_owned(),
                    start_content: true,
                    skip_files: Vec::new(),
                },
            })
            .expect("add reserved-name source");
        assert!(matches!(
            store.record_metadata(&reserved_id, &reserved_info),
            Err(StoreError::DurableState(_))
        ));
        drop(store);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn records_piece_batch_in_one_revision_and_rolls_back_invalid_index() {
        let root = test_root("piece-batch");
        let configured = configured_root(&root);
        let mut store =
            SessionStore::open(&root, "default", &[configured]).expect("open session store");
        let mut raw_info = b"d6:lengthi12e4:name5:batch12:piece lengthi4e6:pieces60:".to_vec();
        raw_info.extend_from_slice(&[b'a'; 20]);
        raw_info.extend_from_slice(&[b'b'; 20]);
        raw_info.extend_from_slice(&[b'c'; 20]);
        raw_info.push(b'e');
        let info_hash: [u8; 20] = Sha1::digest(&raw_info).into();
        let torrent_id = crate::control::encode_info_hash(info_hash);
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "add-batch".to_owned(),
                expected_revision: None,
                command: Command::AddMagnet {
                    magnet: format!("magnet:?xt=urn:btih:{torrent_id}&x.pe=127.0.0.1:1"),
                    storage_root: "downloads".to_owned(),
                    start_content: true,
                    skip_files: Vec::new(),
                },
            })
            .expect("add source");
        assert!(!store.snapshot().expect("metadata snapshot").torrents[0].force_recheck_available);
        store
            .record_metadata(&torrent_id, &raw_info)
            .expect("record metadata");

        assert_eq!(
            store
                .record_pieces(&torrent_id, &[2, 0, 2])
                .expect("record batch"),
            3
        );
        assert_eq!(store.revision().expect("revision"), 3);
        assert_eq!(
            store
                .load_resume(&torrent_id)
                .expect("load batch")
                .have
                .expect("have state")
                .pieces(),
            &[true, false, true]
        );

        assert!(matches!(
            store.record_pieces(&torrent_id, &[1, 3]),
            Err(StoreError::Have(_))
        ));
        assert!(matches!(
            store.record_pieces(&torrent_id, &[]),
            Err(StoreError::DurableState(_))
        ));
        assert_eq!(store.revision().expect("revision after rollback"), 3);
        assert_eq!(
            store
                .load_resume(&torrent_id)
                .expect("load after rollback")
                .have
                .expect("have state")
                .pieces(),
            &[true, false, true]
        );
        assert_eq!(store.record_piece(&torrent_id, 1).expect("single piece"), 4);
        assert_eq!(
            store
                .load_resume(&torrent_id)
                .expect("load complete batch")
                .have
                .expect("have state")
                .pieces(),
            &[true, true, true]
        );
        drop(store);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn piece_checkpoint_does_not_relabel_published_payload() {
        let root = test_root("published-piece-checkpoint");
        let mut store =
            SessionStore::open(&root, "default", &[configured_root(&root)]).expect("open");
        let raw_info =
            b"d6:lengthi4e4:name4:test12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = crate::control::encode_info_hash(info_hash);
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "add-published-checkpoint".to_owned(),
                expected_revision: None,
                command: Command::AddMagnet {
                    magnet: format!("magnet:?xt=urn:btih:{torrent_id}"),
                    storage_root: "downloads".to_owned(),
                    start_content: true,
                    skip_files: Vec::new(),
                },
            })
            .expect("add source");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        store
            .mark_storage_prepared(&torrent_id, StorageState::Published)
            .expect("record final ownership");

        store
            .record_piece(&torrent_id, 0)
            .expect("checkpoint final-owned piece");

        let resume = store.load_resume(&torrent_id).expect("load resume");
        assert_eq!(resume.storage_state, StorageState::Published);
        assert_eq!(resume.managed_artifacts, ManagedArtifactState::Published);
        assert_eq!(resume.have.expect("have state").pieces(), &[true]);
        drop(store);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn force_recheck_is_deduplicated_and_replaces_have_with_state() {
        let root = test_root("force-recheck");
        let mut store = SessionStore::open(&root, "default", &[configured_root(&root)])
            .expect("open session store");
        let mut raw_info = b"d6:lengthi12e4:name7:recheck12:piece lengthi4e6:pieces60:".to_vec();
        raw_info.extend_from_slice(&[b'a'; 20]);
        raw_info.extend_from_slice(&[b'b'; 20]);
        raw_info.extend_from_slice(&[b'c'; 20]);
        raw_info.push(b'e');
        let info_hash: [u8; 20] = Sha1::digest(&raw_info).into();
        let torrent_id = crate::control::encode_info_hash(info_hash);
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "add-force-recheck".to_owned(),
                expected_revision: None,
                command: Command::AddMagnet {
                    magnet: format!("magnet:?xt=urn:btih:{torrent_id}"),
                    storage_root: "downloads".to_owned(),
                    start_content: true,
                    skip_files: Vec::new(),
                },
            })
            .expect("add source");
        store
            .record_metadata(&torrent_id, &raw_info)
            .expect("record metadata");
        store
            .mark_storage_prepared(&torrent_id, StorageState::Staging)
            .expect("record managed staging");
        assert!(store.snapshot().expect("staging snapshot").torrents[0].force_recheck_available);
        store.record_piece(&torrent_id, 0).expect("record old have");

        let request = RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: "force-recheck".to_owned(),
            expected_revision: None,
            command: Command::ForceRecheck {
                torrent_id: torrent_id.clone(),
            },
        };
        let response = store.handle_durable(&request).expect("request recheck");
        let checking = store.load_resume(&torrent_id).expect("load checking state");
        assert_eq!(checking.state, TorrentState::Checking);
        assert_eq!(
            checking.have.expect("old have remains input").pieces(),
            &[true, false, false]
        );
        assert_eq!(
            store.snapshot().expect("checking snapshot").torrents[0].verified_piece_count,
            0,
            "old have is input only while checking"
        );
        let revision = store.revision().expect("checking revision");
        assert_eq!(
            store.handle_durable(&request).expect("replay recheck"),
            response
        );
        assert_eq!(store.revision().expect("revision after replay"), revision);

        let replacement =
            HaveState::from_pieces(info_hash, vec![false, true, true]).expect("replacement have");
        store
            .complete_recheck(&torrent_id, &replacement)
            .expect("complete running recheck");
        let completed = store.load_resume(&torrent_id).expect("load replacement");
        assert_eq!(completed.state, TorrentState::Downloading);
        assert_eq!(
            completed.have.expect("replacement have").pieces(),
            &[false, true, true]
        );

        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "pause-before-recheck".to_owned(),
                expected_revision: None,
                command: Command::Pause {
                    torrent_id: torrent_id.clone(),
                },
            })
            .expect("pause torrent");
        store
            .begin_recheck(&torrent_id)
            .expect("begin paused recheck");
        store
            .complete_recheck(&torrent_id, &replacement)
            .expect("complete paused recheck");
        assert_eq!(
            store.load_resume(&torrent_id).expect("paused result").state,
            TorrentState::Paused
        );
        drop(store);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn prepared_publication_is_durable_and_explicitly_confirmed() {
        let root = test_root("prepared-publication");
        let configured = ConfiguredStorageRoot::platform("downloads");
        let mut store =
            SessionStore::open(&root, "default", &[configured]).expect("open session store");
        let raw_info =
            b"d6:lengthi4e4:name4:test12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = crate::control::encode_info_hash(info_hash);
        let request = RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: "add".to_owned(),
            expected_revision: None,
            command: Command::AddMagnet {
                magnet: format!("magnet:?xt=urn:btih:{torrent_id}&x.pe=127.0.0.1:1"),
                storage_root: "downloads".to_owned(),
                start_content: true,
                skip_files: Vec::new(),
            },
        };
        store.handle_durable(&request).expect("add source");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        assert!(matches!(
            store.record_prepared_files(
                &torrent_id,
                &[PreparedFileHash {
                    file_index: 0,
                    length: 3,
                    sha1: [7; 20],
                }],
            ),
            Err(StoreError::DurableState(_))
        ));

        let expected = PreparedFileHash {
            file_index: 0,
            length: 4,
            sha1: [9; 20],
        };
        store
            .record_prepared_files(&torrent_id, std::slice::from_ref(&expected))
            .expect("record prepared manifest");
        let snapshot = store.snapshot().expect("prepared snapshot");
        assert_eq!(
            snapshot.torrents[0].state,
            TorrentState::AwaitingPublication
        );
        assert_eq!(snapshot.torrents[0].storage_state, StorageState::Prepared);
        assert_eq!(
            store
                .load_prepared_files(&torrent_id)
                .expect("load manifest"),
            vec![PreparedFileRecord {
                file_index: 0,
                length: 4,
                sha1: [9; 20],
            }]
        );
        drop(store);

        let mut reopened =
            SessionStore::open(&root, "default", &[]).expect("reopen prepared store");
        assert_eq!(
            reopened
                .load_prepared_files(&torrent_id)
                .expect("load durable manifest")
                .len(),
            1
        );
        let confirmed_revision = reopened
            .confirm_prepared_publication(&torrent_id)
            .expect("confirm publication");
        let snapshot = reopened.snapshot().expect("complete snapshot");
        assert_eq!(snapshot.torrents[0].state, TorrentState::Complete);
        assert_eq!(snapshot.torrents[0].storage_state, StorageState::Published);
        assert!(
            reopened
                .load_prepared_files(&torrent_id)
                .expect("manifest cleared")
                .is_empty()
        );
        assert_eq!(
            reopened
                .confirm_prepared_publication(&torrent_id)
                .expect("repeat publication confirmation"),
            confirmed_revision
        );
        assert_eq!(
            reopened.revision().expect("revision after repeat"),
            confirmed_revision
        );
        drop(reopened);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn prepared_platform_publication_enters_published_recheck_before_complete() {
        let root = test_root("prepared-publication-recheck");
        let configured = ConfiguredStorageRoot::platform("downloads");
        let mut store =
            SessionStore::open(&root, "default", &[configured]).expect("open session store");
        let raw_info =
            b"d6:lengthi4e4:name4:test12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = crate::control::encode_info_hash(info_hash);
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "add-platform-recheck".to_owned(),
                expected_revision: None,
                command: Command::AddMagnet {
                    magnet: format!("magnet:?xt=urn:btih:{torrent_id}"),
                    storage_root: "downloads".to_owned(),
                    start_content: true,
                    skip_files: Vec::new(),
                },
            })
            .expect("add source");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        store
            .record_prepared_files(
                &torrent_id,
                &[PreparedFileHash {
                    file_index: 0,
                    length: 4,
                    sha1: [9; 20],
                }],
            )
            .expect("record prepared manifest");

        let revision = store
            .begin_published_recheck(&torrent_id)
            .expect("begin published recheck");
        let resume = store.load_resume(&torrent_id).expect("load recheck");
        assert_eq!(resume.state, TorrentState::Checking);
        assert_eq!(resume.storage_state, StorageState::Published);
        assert_eq!(resume.managed_artifacts, ManagedArtifactState::Published);
        assert!(
            store
                .load_prepared_files(&torrent_id)
                .expect("manifest cleared")
                .is_empty()
        );
        assert_eq!(
            store
                .begin_published_recheck(&torrent_id)
                .expect("repeat published recheck"),
            revision
        );
        assert_eq!(store.revision().expect("current revision"), revision);

        drop(store);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn path_publication_intent_is_durable_before_confirmation() {
        let root = test_root("path-publication-intent");
        let configured = configured_root(&root);
        let mut store = SessionStore::open(&root, "default", std::slice::from_ref(&configured))
            .expect("open store");
        let raw_info =
            b"d6:lengthi4e4:name4:test12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = crate::control::encode_info_hash(info_hash);
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "add-path-publication".to_owned(),
                expected_revision: None,
                command: Command::AddMagnet {
                    magnet: format!("magnet:?xt=urn:btih:{torrent_id}"),
                    storage_root: "downloads".to_owned(),
                    start_content: true,
                    skip_files: Vec::new(),
                },
            })
            .expect("add source");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        store
            .mark_storage_prepared(&torrent_id, StorageState::Staging)
            .expect("own staging artifact");

        let prepared_revision = store
            .mark_publication_prepared(&torrent_id)
            .expect("record publication intent");
        assert_eq!(
            store
                .mark_publication_prepared(&torrent_id)
                .expect("repeat publication intent"),
            prepared_revision
        );
        let prepared = store.load_resume(&torrent_id).expect("load intent");
        assert_eq!(prepared.state, TorrentState::AwaitingPublication);
        assert_eq!(prepared.storage_state, StorageState::Prepared);
        assert_eq!(prepared.managed_artifacts, ManagedArtifactState::Staging);

        drop(store);
        let mut reopened =
            SessionStore::open(&root, "default", &[configured]).expect("reopen store");
        let prepared = reopened.load_resume(&torrent_id).expect("reload intent");
        assert_eq!(prepared.storage_state, StorageState::Prepared);
        assert_eq!(prepared.managed_artifacts, ManagedArtifactState::Staging);
        reopened
            .mark_complete(&torrent_id)
            .expect("confirm path publication");
        let complete = reopened.load_resume(&torrent_id).expect("load complete");
        assert_eq!(complete.state, TorrentState::Complete);
        assert_eq!(complete.storage_state, StorageState::Published);
        assert_eq!(complete.managed_artifacts, ManagedArtifactState::Published);
        drop(reopened);
        fs::remove_dir_all(root).expect("remove profile");
    }

    #[test]
    fn fresh_profile_uses_product_defaults_without_rewriting_reopen() {
        let root = test_root("fresh-client-settings");
        let configured = configured_root(&root);
        let store = SessionStore::open_with_initial_client_settings(
            &root,
            "default",
            std::slice::from_ref(&configured),
            &ClientSettings::fresh_profile_default(),
        )
        .expect("open fresh product profile");
        assert_eq!(
            store.client_settings().expect("read fresh settings"),
            ClientSettings::fresh_profile_default()
        );
        drop(store);

        let reopened = SessionStore::open(&root, "default", &[configured]).expect("reopen profile");
        assert_eq!(
            reopened.client_settings().expect("read retained settings"),
            ClientSettings::fresh_profile_default()
        );
        drop(reopened);
        fs::remove_dir_all(root).expect("remove profile");
    }

    #[test]
    fn migrates_version_eight_to_default_client_settings() {
        let root = test_root("schema-v8-client-settings");
        let configured = configured_root(&root);
        let store =
            SessionStore::open(&root, "default", std::slice::from_ref(&configured)).expect("open");
        let database_path = store.database_path().unwrap().to_owned();
        drop(store);

        let connection = Connection::open(&database_path).expect("open schema eight fixture");
        connection
            .execute("DROP TABLE client_settings", [])
            .expect("remove version-nine table");
        connection
            .pragma_update(None, "user_version", 8)
            .expect("mark schema eight");
        drop(connection);

        let reopened = SessionStore::open(&root, "default", &[configured]).expect("migrate");
        assert_eq!(
            reopened.client_settings().expect("read migrated settings"),
            ClientSettings::default()
        );
        assert_eq!(
            reopened
                .connection
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .expect("read schema version"),
            SCHEMA_VERSION
        );
        drop(reopened);
        fs::remove_dir_all(root).expect("remove profile");
    }

    #[test]
    fn client_settings_are_atomic_replayable_and_profile_scoped() {
        let root = test_root("client-settings-command");
        let configured_root = configured_root(&root);
        let mut store =
            SessionStore::open(&root, "default", std::slice::from_ref(&configured_root))
                .expect("open");
        let configured = ClientSettings {
            listener: ListenerPolicy::FixedLoopback { port: 42_001 },
            preferred_listen_port: 6_881,
            port_mapping: crate::PortMappingPolicy::Disabled,
            peer_connection_limit: 321,
            upload_slots: 3,
            tracker_https_server_authentication: Default::default(),
        };
        let request = RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: "set-client-settings".to_owned(),
            expected_revision: Some("0".to_owned()),
            command: Command::SetClientSettings {
                settings: configured.clone(),
            },
        };
        let accepted = store.handle_durable(&request).expect("set settings");
        assert_eq!(accepted.revision, "1");
        let ResponseOutcome::Success { snapshot } = &accepted.outcome else {
            panic!("settings mutation must succeed");
        };
        assert_eq!(snapshot.client_settings, configured);
        assert_eq!(store.handle_durable(&request).expect("replay"), accepted);

        let conflict = store
            .handle_durable(&RequestEnvelope {
                command: Command::SetClientSettings {
                    settings: ClientSettings::default(),
                },
                ..request.clone()
            })
            .expect("settings request conflict");
        assert!(matches!(
            conflict.outcome,
            ResponseOutcome::Error {
                error: crate::ErrorResponse {
                    code: ErrorCode::RequestConflict,
                    ..
                }
            }
        ));

        let no_op = store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "settings-no-op".to_owned(),
                expected_revision: Some("1".to_owned()),
                command: Command::SetClientSettings {
                    settings: configured.clone(),
                },
            })
            .expect("no-op settings");
        assert_eq!(no_op.revision, "1");
        let stale = store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "settings-stale".to_owned(),
                expected_revision: Some("0".to_owned()),
                command: Command::SetClientSettings {
                    settings: ClientSettings::default(),
                },
            })
            .expect("stale settings");
        assert!(matches!(
            stale.outcome,
            ResponseOutcome::Error {
                error: crate::ErrorResponse {
                    code: ErrorCode::StaleRevision,
                    ..
                }
            }
        ));
        let invalid_request_id = "invalid-settings".to_owned();
        let invalid = store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: invalid_request_id.clone(),
                expected_revision: Some("1".to_owned()),
                command: Command::SetClientSettings {
                    settings: ClientSettings {
                        peer_connection_limit: 0,
                        ..ClientSettings::default()
                    },
                },
            })
            .expect("reject invalid settings");
        assert!(matches!(
            invalid.outcome,
            ResponseOutcome::Error {
                error: crate::ErrorResponse {
                    code: ErrorCode::InvalidRequest,
                    ..
                }
            }
        ));
        assert_eq!(store.revision().unwrap(), 1);
        assert_eq!(store.client_settings().unwrap(), configured);
        drop(store);

        let mut reopened =
            SessionStore::open(&root, "default", &[configured_root]).expect("reopen");
        assert_eq!(reopened.client_settings().unwrap(), configured);
        let reverted = reopened
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: invalid_request_id,
                expected_revision: Some("1".to_owned()),
                command: Command::SetClientSettings {
                    settings: ClientSettings::default(),
                },
            })
            .expect("retry corrected settings request");
        assert_eq!(reverted.revision, "2");
        assert_eq!(
            reopened.client_settings().unwrap(),
            ClientSettings::default()
        );
        drop(reopened);

        let mut ephemeral =
            SessionStore::open_ephemeral("ephemeral", &[]).expect("open ephemeral store");
        let accepted = ephemeral
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "ephemeral-settings".to_owned(),
                expected_revision: None,
                command: Command::SetClientSettings {
                    settings: ClientSettings {
                        peer_connection_limit: 199,
                        ..ClientSettings::default()
                    },
                },
            })
            .expect("accept ephemeral settings");
        assert!(matches!(accepted.outcome, ResponseOutcome::Success { .. }));
        assert_eq!(ephemeral.revision().unwrap(), 1);
        assert_eq!(
            ephemeral.client_settings().unwrap(),
            ClientSettings {
                peer_connection_limit: 199,
                ..ClientSettings::default()
            }
        );
        fs::remove_dir_all(root).expect("remove profile");
    }

    #[test]
    fn client_settings_storage_failure_rolls_back_group_revision_and_receipt() {
        let root = test_root("client-settings-rollback");
        let configured_root = configured_root(&root);
        let mut store =
            SessionStore::open(&root, "default", std::slice::from_ref(&configured_root))
                .expect("open");
        store
            .connection
            .execute_batch(
                "CREATE TRIGGER reject_client_settings
                 BEFORE UPDATE ON client_settings
                 BEGIN
                    SELECT RAISE(ABORT, 'injected settings write failure');
                 END;",
            )
            .expect("install failure trigger");
        let request = RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: "settings-write-failure".to_owned(),
            expected_revision: Some("0".to_owned()),
            command: Command::SetClientSettings {
                settings: ClientSettings {
                    peer_connection_limit: 199,
                    ..ClientSettings::default()
                },
            },
        };
        assert!(store.handle_durable(&request).is_err());
        assert_eq!(store.revision().unwrap(), 0);
        assert_eq!(store.client_settings().unwrap(), ClientSettings::default());

        store
            .connection
            .execute_batch("DROP TRIGGER reject_client_settings")
            .expect("remove failure trigger");
        let accepted = store
            .handle_durable(&request)
            .expect("retry unrecorded request");
        assert_eq!(accepted.revision, "1");
        drop(store);
        fs::remove_dir_all(root).expect("remove profile");
    }

    #[test]
    fn corrupt_client_settings_prevent_profile_open() {
        let root = test_root("client-settings-corrupt-open");
        let configured = configured_root(&root);
        let store =
            SessionStore::open(&root, "default", std::slice::from_ref(&configured)).expect("open");
        let database_path = store.database_path().unwrap().to_owned();
        drop(store);

        let connection = Connection::open(database_path).expect("open raw database");
        connection
            .pragma_update(None, "ignore_check_constraints", true)
            .expect("disable constraints for corruption fixture");
        connection
            .execute(
                "UPDATE client_settings SET peer_connection_limit = -1 WHERE singleton = 1",
                [],
            )
            .expect("write corrupt setting");
        drop(connection);

        let error = match SessionStore::open(&root, "default", &[configured]) {
            Ok(_) => panic!("corrupt settings must prevent profile open"),
            Err(error) => error,
        };
        assert!(matches!(error, StoreError::DurableState(_)));
        fs::remove_dir_all(root).expect("remove profile");
    }
}
