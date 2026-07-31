use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rstorrent_engine::dht::DhtSnapshot;
use rstorrent_engine::{PreparedFileHash, plan_descriptor_storage};
use rstorrent_protocol::bencode::MAX_BENCODE_INPUT_LENGTH;
use rstorrent_protocol::dht::{DhtEndpoint, DhtIp, NodeContact, NodeId};
use rstorrent_protocol::magnet::Magnet;
use rstorrent_protocol::metainfo::{MAX_FILES, MAX_PIECES, Metainfo};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::control::{
    Command, ErrorCode, RequestEnvelope, ResponseEnvelope, ServiceSnapshot, StorageState,
    TorrentSnapshot, TorrentState, decode_info_hash, encode_info_hash, parse_revision,
    validate_identifier, validate_request,
};
use crate::have::{HaveError, HaveState};

const SCHEMA_VERSION: i64 = 3;
const DATABASE_FILENAME: &str = "session.db";
const MAX_RECEIPTS: i64 = 1024;
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfiguredStorageRoot {
    pub id: String,
    pub location: StorageRootLocation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StorageRootLocation {
    Path(PathBuf),
    PlatformCapability,
}

impl ConfiguredStorageRoot {
    pub fn path(id: impl Into<String>, path: PathBuf) -> Self {
        Self {
            id: id.into(),
            location: StorageRootLocation::Path(path),
        }
    }

    pub fn platform(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            location: StorageRootLocation::PlatformCapability,
        }
    }
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
    pub have: Option<HaveState>,
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
                        piece_count, have_state, desired_state
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
                        row.get::<_, Option<i64>>(5)?,
                        row.get::<_, Option<Vec<u8>>>(6)?,
                        row.get::<_, String>(7)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| StoreError::UnknownTorrent(torrent_id.to_owned()))?;
        let state = TorrentState::parse(&row.2)
            .ok_or_else(|| StoreError::DurableState("invalid torrent state".to_owned()))?;
        let storage_state = StorageState::parse(&row.3)
            .ok_or_else(|| StoreError::DurableState("invalid storage state".to_owned()))?;
        let skip_files = read_selection(&self.connection, &info_hash)?;
        let have = match (row.5, row.6) {
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
            desired_running: match row.7.as_str() {
                "running" => true,
                "paused" => false,
                _ => {
                    return Err(StoreError::DurableState(
                        "invalid desired torrent state".to_owned(),
                    ));
                }
            },
            raw_info: row.4,
            have,
        })
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
        let have = HaveState::empty(expected_info_hash, metainfo.piece_count())?.encode();
        let transaction = self.connection.transaction()?;
        let revision = increment_revision(&transaction)?;
        let revision_sql = sql_revision(revision)?;
        let updated = transaction.execute(
            "UPDATE torrents
             SET raw_info = ?2,
                 piece_count = ?3,
                 have_state = ?4,
                 state = CASE
                    WHEN desired_state = 'paused' THEN 'paused'
                    ELSE ?5
                 END,
                 storage_state = ?6,
                 error = NULL,
                 updated_revision = ?7
             WHERE info_hash = ?1",
            params![
                expected_info_hash.as_slice(),
                raw_info,
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
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        let transaction = self.connection.transaction()?;
        let (piece_count, bytes) = read_have_columns(&transaction, &info_hash)?;
        let mut have = HaveState::decode(&bytes, info_hash, piece_count)?;
        have.set(piece_index, true)?;
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
             SET state = ?2, storage_state = ?3, error = ?4,
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
                locator TEXT NOT NULL
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
                piece_count INTEGER CHECK (
                    piece_count IS NULL OR
                    (piece_count > 0 AND piece_count <= 26214)
                ),
                have_state BLOB CHECK (
                    have_state IS NULL OR length(have_state) <= 3311
                ),
                error TEXT CHECK (error IS NULL OR length(error) <= 1024),
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
        transaction.execute_batch(DHT_TABLES_SQL)?;
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
                piece_count INTEGER CHECK (
                    piece_count IS NULL OR
                    (piece_count > 0 AND piece_count <= 26214)
                ),
                have_state BLOB CHECK (
                    have_state IS NULL OR length(have_state) <= 3311
                ),
                error TEXT CHECK (error IS NULL OR length(error) <= 1024),
                created_revision INTEGER NOT NULL,
                updated_revision INTEGER NOT NULL,
                CHECK (
                    (piece_count IS NULL AND have_state IS NULL) OR
                    (piece_count IS NOT NULL AND have_state IS NOT NULL)
                )
             );
             INSERT INTO torrents
                SELECT * FROM torrents_v1;
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
        transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        transaction.commit()?;
    }
    if version == 2 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(DHT_TABLES_SQL)?;
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

fn register_storage_roots(
    connection: &mut Connection,
    storage_roots: &[ConfiguredStorageRoot],
) -> Result<(), StoreError> {
    let transaction = connection.transaction()?;
    for root in storage_roots {
        validate_identifier(&root.id, "storage root", crate::control::MAX_ROOT_ID_LENGTH)
            .map_err(|(_, message)| StoreError::Configuration(message))?;
        let locator = match &root.location {
            StorageRootLocation::Path(path) => path.to_str().ok_or_else(|| {
                StoreError::Configuration(format!(
                    "storage root {} is not representable as UTF-8",
                    root.id
                ))
            })?,
            StorageRootLocation::PlatformCapability => "platform-capability:",
        };
        transaction.execute(
            "INSERT INTO storage_roots(root_id, locator)
             VALUES (?1, ?2)
             ON CONFLICT(root_id) DO UPDATE SET locator = excluded.locator",
            params![root.id, locator],
        )?;
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
            skip_files,
        } => add_magnet(
            transaction,
            magnet,
            storage_root,
            skip_files,
            current_revision,
        ),
        Command::Pause { torrent_id } => {
            set_desired_state(transaction, torrent_id, false, current_revision)
        }
        Command::Resume { torrent_id } => {
            set_desired_state(transaction, torrent_id, true, current_revision)
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

fn add_magnet(
    transaction: &Transaction<'_>,
    source: &str,
    storage_root: &str,
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
                storage_state, created_revision, updated_revision
             ) VALUES (?1, ?2, ?3, 'running', ?4, ?5, ?6, ?6)",
            params![
                magnet.info_hash.as_slice(),
                canonical_magnet(&magnet),
                storage_root,
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
            "SELECT state, raw_info IS NOT NULL, desired_state
             FROM torrents WHERE info_hash = ?1",
            [info_hash.as_slice()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, bool>(1)?,
                    row.get::<_, String>(2)?,
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

fn read_snapshot(connection: &Connection, profile_id: &str) -> Result<ServiceSnapshot, StoreError> {
    let revision = read_revision(connection)?;
    let mut statement = connection.prepare(
        "SELECT info_hash, storage_root, state, storage_state,
                raw_info IS NOT NULL, piece_count, have_state, error
         FROM torrents
         ORDER BY info_hash",
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
            error: row.7,
        });
    }
    Ok(ServiceSnapshot {
        profile_id: profile_id.to_owned(),
        revision: revision.to_string(),
        torrents,
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
        ConfiguredStorageRoot, PreparedFileRecord, SCHEMA_VERSION, SessionStore, StoreError,
    };
    use crate::{
        CONTROL_VERSION, Command, ErrorCode, RequestEnvelope, ResponseOutcome, StorageState,
        TorrentState,
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
                skip_files: vec![1, 3],
            },
        }
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
}
