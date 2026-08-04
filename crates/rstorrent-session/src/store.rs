use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rstorrent_engine::dht::DhtSnapshot;
use rstorrent_engine::{PreparedFileHash, plan_descriptor_storage, validate_publication_name};
use rstorrent_protocol::bencode::MAX_BENCODE_INPUT_LENGTH;
use rstorrent_protocol::dht::{DhtEndpoint, DhtIp, NodeContact, NodeId};
use rstorrent_protocol::magnet::Magnet;
use rstorrent_protocol::metainfo::{MAX_FILES, MAX_PIECES, Metainfo};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::control::{
    Command, ErrorCode, FilePriority, RemovalDataPolicy, RemovalState, RequestEnvelope,
    ResponseEnvelope, ServiceSnapshot, StorageRootAvailability, StorageRootSnapshot,
    StorageSettingsSnapshot, StorageState, TorrentSnapshot, TorrentState, decode_info_hash,
    encode_info_hash, parse_revision, validate_identifier, validate_request,
};
use crate::have::{HaveError, HaveState};

const SCHEMA_VERSION: i64 = 6;
const DATABASE_FILENAME: &str = "session.db";
const MAX_RECEIPTS: i64 = 1024;
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResumeRecord {
    pub torrent_id: String,
    pub magnet: String,
    pub storage_root: String,
    pub skip_files: Vec<u32>,
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
    pub managed_artifacts: ManagedArtifactState,
    pub error: Option<String>,
}

pub struct SessionStore {
    connection: Connection,
    profile_id: String,
    database_path: PathBuf,
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
        validate_identifier(
            profile_id,
            "profile ID",
            crate::control::MAX_PROFILE_ID_LENGTH,
        )
        .map_err(|(_, message)| StoreError::Configuration(message))?;
        std::fs::create_dir_all(profile_root).map_err(|source| StoreError::Io {
            operation: "create profile directory",
            source,
        })?;
        let database_path = profile_root.join(DATABASE_FILENAME);
        let mut connection = Connection::open(&database_path)?;
        connection.busy_timeout(BUSY_TIMEOUT)?;
        connection.pragma_update(None, "foreign_keys", true)?;
        let foreign_keys: i64 =
            connection.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
        if foreign_keys != 1 {
            return Err(StoreError::RequiredPragma("foreign_keys"));
        }
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
        migrate(&mut connection, profile_id)?;
        register_storage_roots(&mut connection, storage_roots)?;

        Ok(Self {
            connection,
            profile_id: profile_id.to_owned(),
            database_path,
        })
    }

    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    pub fn revision(&self) -> Result<u64, StoreError> {
        read_revision(&self.connection)
    }

    pub fn snapshot(&self) -> Result<ServiceSnapshot, StoreError> {
        read_snapshot(&self.connection, &self.profile_id)
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
                        row.get::<_, String>(0)?,
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
        Ok(ResumeRecord {
            torrent_id: torrent_id.to_ascii_lowercase(),
            magnet: row.0,
            storage_root: row.1,
            skip_files,
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
                        t.raw_info, t.publication_name, t.managed_artifacts,
                        r.error
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
                        managed_artifacts: row.get(6)?,
                        error: row.get(7)?,
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
                    r.state, t.raw_info, t.publication_name,
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
                    managed_artifacts: row.get(7)?,
                    error: row.get(8)?,
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
        if raw_info.len() > MAX_BENCODE_INPUT_LENGTH {
            return Err(StoreError::DurableState(format!(
                "verified metadata exceeds {MAX_BENCODE_INPUT_LENGTH} bytes"
            )));
        }
        let expected_info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        let metainfo = Metainfo::from_info_bytes(raw_info)
            .map_err(|error| StoreError::DurableState(error.to_string()))?;
        if metainfo.info_hash != expected_info_hash {
            return Err(StoreError::DurableState(
                "verified metadata does not match torrent identity".to_owned(),
            ));
        }
        validate_publication_name(&metainfo.name)
            .map_err(|error| StoreError::DurableState(error.to_string()))?;
        let have = HaveState::empty(expected_info_hash, metainfo.piece_count())?.encode();
        let transaction = self.connection.transaction()?;
        let revision = increment_revision(&transaction)?;
        let revision_sql = sql_revision(revision)?;
        let updated = transaction.execute(
            "UPDATE torrents
             SET raw_info = ?2,
                 publication_name = ?3,
                 piece_count = ?4,
                 have_state = ?5,
                 state = CASE
                    WHEN desired_state = 'paused' THEN 'paused'
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
                 storage_state = ?3,
                 updated_revision = ?4
             WHERE info_hash = ?1",
            params![
                info_hash.as_slice(),
                have.encode(),
                StorageState::Staging.as_str(),
                revision_sql
            ],
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
                 state = CASE
                    WHEN desired_state = 'paused' THEN 'paused'
                    WHEN ?2 = 'published' THEN 'checking'
                    ELSE 'downloading'
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
        let metainfo = Metainfo::from_info_bytes(&raw_info)
            .map_err(|error| StoreError::DurableState(error.to_string()))?;
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
        let metainfo = Metainfo::from_info_bytes(&raw_info)
            .map_err(|error| StoreError::DurableState(error.to_string()))?;
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
        }
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
        Self::Have(error)
    }
}

fn migrate(connection: &mut Connection, profile_id: &str) -> Result<(), StoreError> {
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
             CREATE TABLE file_selection (
                info_hash BLOB NOT NULL
                    REFERENCES torrents(info_hash) ON DELETE CASCADE,
                file_index INTEGER NOT NULL
                    CHECK (file_index >= 0 AND file_index < 4096),
                wanted INTEGER NOT NULL CHECK (wanted = 0),
                 PRIMARY KEY (info_hash, file_index)
             ) WITHOUT ROWID;
             CREATE TABLE prepared_files (
                info_hash BLOB NOT NULL
                    REFERENCES torrents(info_hash) ON DELETE CASCADE,
                file_index INTEGER NOT NULL
                    CHECK (file_index >= 0 AND file_index < 4096),
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
        transaction.execute_batch(DHT_TABLES_SQL)?;
        transaction.execute_batch(REMOVAL_TABLE_SQL)?;
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
        transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        transaction.commit()?;
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
    let result = match &request.command {
        Command::AddMagnet {
            magnet,
            storage_root,
            start_content,
            skip_files,
        } => add_magnet(
            transaction,
            magnet,
            storage_root,
            *start_content,
            skip_files,
            current_revision,
        ),
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
        Command::SetDefaultStorageRoot { storage_root } => {
            set_default_storage_root(transaction, storage_root, current_revision)
        }
        Command::SetShowAddOptions { show } => {
            set_show_add_options(transaction, *show, current_revision)
        }
        Command::RemoveStorageRoot { storage_root } => {
            remove_storage_root(transaction, storage_root, current_revision)
        }
        Command::Pause { torrent_id } => {
            set_desired_state(transaction, torrent_id, false, current_revision)
        }
        Command::Resume { torrent_id } => {
            set_desired_state(transaction, torrent_id, true, current_revision)
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
) -> Result<u64, (ErrorCode, String)> {
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
    let magnet =
        Magnet::parse(source).map_err(|error| (ErrorCode::InvalidRequest, error.to_string()))?;
    let torrent_id = encode_info_hash(magnet.info_hash);
    let exists = transaction
        .query_row(
            "SELECT 1 FROM torrents WHERE info_hash = ?1",
            [magnet.info_hash.as_slice()],
            |_| Ok(()),
        )
        .optional()
        .map_err(internal_error)?
        .is_some();
    if exists {
        return Err((
            ErrorCode::InvalidTorrentState,
            format!("torrent {torrent_id} already exists"),
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
                updated_revision
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'none', ?7, ?7)",
            params![
                magnet.info_hash.as_slice(),
                canonical_magnet(&magnet),
                storage_root,
                if start_content { "running" } else { "paused" },
                TorrentState::AwaitingMetadata.as_str(),
                StorageState::None.as_str(),
                revision_sql
            ],
        )
        .map_err(internal_error)?;
    for file_index in skip_files {
        transaction
            .execute(
                "INSERT INTO file_selection(info_hash, file_index, wanted)
                 VALUES (?1, ?2, 0)",
                params![magnet.info_hash.as_slice(), i64::from(*file_index)],
            )
            .map_err(internal_error)?;
    }
    Ok(revision)
}

fn set_file_priority(
    transaction: &Transaction<'_>,
    torrent_id: &str,
    file_indices: &[u32],
    priority: FilePriority,
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
            "SELECT t.raw_info, t.desired_state, t.state, t.storage_state,
                    sr.kind, r.info_hash IS NOT NULL
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
    let metainfo = Metainfo::from_info_bytes(&raw_info)
        .map_err(|error| (ErrorCode::InvalidDurableState, error.to_string()))?;
    for &file_index in file_indices {
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
    let mut skipped = read_selection(transaction, &info_hash)
        .map_err(|error| internal_message(&error.to_string()))?;
    let initially_skipped = skipped.clone();
    for &file_index in file_indices {
        match priority {
            FilePriority::Normal => skipped.retain(|index| *index != file_index),
            FilePriority::Skip => {
                if let Err(position) = skipped.binary_search(&file_index) {
                    skipped.insert(position, file_index);
                }
            }
        }
    }
    if skipped == initially_skipped {
        return Ok(current_revision);
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
        .filter(|(index, file)| !file.padding && skipped.binary_search(&(*index as u32)).is_err())
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
    for file_index in skipped {
        transaction
            .execute(
                "INSERT INTO file_selection(info_hash, file_index, wanted)
                 VALUES (?1, ?2, 0)",
                params![info_hash.as_slice(), i64::from(file_index)],
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

fn read_snapshot(connection: &Connection, profile_id: &str) -> Result<ServiceSnapshot, StoreError> {
    let revision = read_revision(connection)?;
    let mut statement = connection.prepare(
        "SELECT t.info_hash, t.storage_root, t.state, t.storage_state,
                t.raw_info IS NOT NULL, t.piece_count, t.have_state,
                t.error, t.archived, r.state, r.error
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
            skip_files: read_selection(connection, &info_hash)?,
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
            error: row.10.or(row.7),
        });
    }
    Ok(ServiceSnapshot {
        profile_id: profile_id.to_owned(),
        revision: revision.to_string(),
        storage: read_storage_settings(connection)?,
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
    Ok(RemovalRecord {
        torrent_id: torrent_id.to_ascii_lowercase(),
        operation_id: row.operation_id,
        storage_root: row.storage_root,
        policy,
        state,
        raw_info: row.raw_info,
        publication_name: row.publication_name,
        managed_artifacts,
        error: row.error,
    })
}

fn read_selection(connection: &Connection, info_hash: &[u8; 20]) -> Result<Vec<u32>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT file_index FROM file_selection
         WHERE info_hash = ?1 ORDER BY file_index",
    )?;
    let rows = statement.query_map([info_hash.as_slice()], |row| row.get::<_, i64>(0))?;
    let mut selection = Vec::new();
    for row in rows {
        let index = row?;
        if !(0..i64::try_from(MAX_FILES).expect("file bound fits i64")).contains(&index) {
            return Err(StoreError::DurableState(
                "invalid file selection index".to_owned(),
            ));
        }
        selection.push(
            u32::try_from(index)
                .map_err(|_| StoreError::DurableState("selection index overflow".to_owned()))?,
        );
    }
    Ok(selection)
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
        (Some(piece_count), Some(bytes)) => Ok((bounded_piece_count(piece_count)?, bytes)),
        _ => Err(StoreError::DurableState(
            "torrent has no verified metadata and have state".to_owned(),
        )),
    }
}

fn bounded_piece_count(piece_count: i64) -> Result<usize, StoreError> {
    let piece_count = usize::try_from(piece_count)
        .map_err(|_| StoreError::DurableState("negative piece count".to_owned()))?;
    if piece_count == 0 || piece_count > MAX_PIECES {
        return Err(StoreError::DurableState(format!(
            "piece count {piece_count} exceeds bound {MAX_PIECES}"
        )));
    }
    Ok(piece_count)
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
    for tracker in &magnet.udp_trackers {
        output.push_str("&tr=udp://");
        if tracker.host.contains(':') {
            output.push('[');
            output.push_str(&tracker.host);
            output.push(']');
        } else {
            output.push_str(&tracker.host);
        }
        output.push(':');
        output.push_str(&tracker.port.to_string());
    }
    output
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
    use rusqlite::Connection;
    use sha1::{Digest, Sha1};

    use super::{
        ConfiguredStorageRoot, ManagedArtifactState, PreparedFileRecord, SCHEMA_VERSION,
        SessionStore, StoreError,
    };
    use crate::{
        CONTROL_VERSION, Command, ErrorCode, FilePriority, RemovalDataPolicy, RemovalState,
        RequestEnvelope, ResponseOutcome, StorageState, TorrentState,
    };

    static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn canonical_magnet_preserves_supported_discovery_sources() {
        let parsed = rstorrent_protocol::magnet::Magnet::parse(
            "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213\
             &x.pe=[::1]:6881\
             &tr=UDP%3A%2F%2FTRACKER.EXAMPLE%3A6969%2Fannounce\
             &tr=udp%3A%2F%2F%5B2001%3Adb8%3A%3A1%5D%3A80",
        )
        .expect("parse magnet");

        assert_eq!(
            super::canonical_magnet(&parsed),
            "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213\
             &x.pe=[::1]:6881\
             &tr=udp://tracker.example:6969\
             &tr=udp://[2001:db8::1]:80"
        );
    }

    #[test]
    fn tracker_only_magnet_survives_catalog_reopen() {
        let root = test_root("tracker-magnet");
        let configured = configured_root(&root);
        let source = "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213\
             &tr=UDP%3A%2F%2FTRACKER.EXAMPLE%3A6969%2Fannounce";
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
        assert_eq!(
            reopened
                .load_resume("000102030405060708090a0b0c0d0e0f10111213")
                .expect("load resumed source")
                .magnet,
            "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213\
             &tr=udp://tracker.example:6969"
        );
        drop(reopened);
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

    fn multi_file_info() -> Vec<u8> {
        let mut info = b"d5:filesld6:lengthi4e4:pathl5:a.bineed6:lengthi4e4:pathl5:b.bineee4:name5:multi12:piece lengthi4e6:pieces40:".to_vec();
        info.extend_from_slice(&[b'a'; 20]);
        info.extend_from_slice(&[b'b'; 20]);
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
        let database_path = store.database_path().to_owned();
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
        let database = store.database_path().to_owned();
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
        let database_path = store.database_path().to_owned();
        drop(store);

        let connection = Connection::open(&database_path).expect("open raw database");
        connection
            .execute_batch(
                "DROP TABLE prepared_files;
                 DROP TABLE dht_nodes;
                 DROP TABLE dht_state;
                 DROP TABLE removal_jobs;
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
        let database_path = store.database_path().to_owned();
        drop(store);

        let connection = Connection::open(&database_path).expect("open raw database");
        connection
            .execute_batch(
                "DROP TABLE removal_jobs;
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
        let database_path = store.database_path().to_owned();
        drop(store);

        let connection = Connection::open(&database_path).expect("open raw database");
        connection
            .execute_batch(
                "ALTER TABLE torrents DROP COLUMN publication_name;
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
        let database_path = store.database_path().to_owned();
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
    fn path_publication_intent_is_durable_before_confirmation() {
        let root = test_root("path-publication-intent");
        let configured = configured_root(&root);
        let mut store =
            SessionStore::open(&root, "default", &[configured.clone()]).expect("open store");
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
}
