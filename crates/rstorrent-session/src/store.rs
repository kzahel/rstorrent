use std::error::Error;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rstorrent_engine::dht::{DhtIdentity, DhtSnapshot};
use rstorrent_engine::{
    PreparedFileHash, PublicationShape, torrent_storage_paths_for_metainfo,
    validate_publication_name,
};
use rstorrent_protocol::bencode::ParseError;
use rstorrent_protocol::dht::{DhtEndpoint, DhtIp, NodeContact, NodeId};
use rstorrent_protocol::magnet::{
    FileIndexRange as MagnetFileIndexRange, MAX_MAGNET_LENGTH, MAX_TRACKERS, Magnet, TrackerUrl,
    TrackerUrlTransport,
};
use rstorrent_protocol::metainfo::{
    DURABLE_METAINFO_LIMITS, EXPLICIT_IMPORT_METAINFO_LIMITS, Metainfo, MetainfoError,
    MetainfoProjection, MetainfoTrackerTransport,
};
use rstorrent_protocol::storage_layout::{FileSelection, TorrentLayout};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sha2::{Digest as Sha256Digest, Sha256};

use crate::control::{
    AddTorrentBytesRequest, AddTorrentDisposition, AddTorrentResult, Command, CommandResult,
    ErrorCode, FilePriority, FileSelectionIntent, MAX_FILE_SELECTION_ENTRIES, MagnetExportResult,
    MagnetExportSource, RemovalDataPolicy, RemovalState, RequestEnvelope, ResponseEnvelope,
    ServiceSnapshot, StorageState, TorrentSnapshot, TorrentState, decode_info_hash,
    encode_info_hash, parse_revision, validate_add_torrent_bytes_request, validate_identifier,
    validate_request,
};
use crate::download_queue::{self, QueueEdge};
use crate::durable_state::{
    DerivedStateInput, PayloadState, VerificationState, derive_torrent_state,
};
use crate::have::{HaveError, HaveState, MAX_DURABLE_HAVE_STATE_BYTES, MAX_DURABLE_PIECES};
use crate::settings::{
    ClientSettings, SettingsPersistenceError, StorageRootAvailability, StorageRootSnapshot,
    StorageSettingsSnapshot, TorrentTransferLimits, TransferRateLimit, create_client_settings,
    migrate_client_settings_to_v10, migrate_client_settings_to_v11, migrate_client_settings_to_v12,
    migrate_client_settings_to_v15, migrate_client_settings_to_v16, read_client_settings,
    replace_client_settings,
};
use crate::store_schema::{
    DHT_TABLES_SQL, DOWNLOAD_QUEUE_INDEX_SQL, REMOVAL_TABLE_SQL, SCHEMA_VERSION, SOURCE_TABLES_SQL,
    migrate_to_v17, migrate_to_v18,
};

const DATABASE_FILENAME: &str = "session.db";
const MAX_RECEIPTS: i64 = 1024;
pub(crate) const EPHEMERAL_SESSION_MAX_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_STORAGE_ROOTS: usize = 32;
pub const MAX_STORAGE_ROOT_LOCATOR_LENGTH: usize = 4096;
const BUSY_TIMEOUT: Duration = Duration::from_secs(2);

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
    pub download_queue_position: Option<i64>,
    pub raw_info: Option<Vec<u8>>,
    pub publication_name: Option<String>,
    pub managed_artifacts: ManagedArtifactState,
    pub have: Option<HaveState>,
    pub(crate) payload_state: PayloadState,
    pub(crate) verification: VerificationState,
    pub(crate) quarantine_reason: Option<String>,
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
        let mut identity_statement = self.connection.prepare(
            "SELECT family, address, node_id
             FROM dht_identities ORDER BY family, identity_order",
        )?;
        let identity_rows = identity_statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })?;
        let mut identities_v4 = Vec::new();
        let mut identities_v6 = Vec::new();
        for row in identity_rows {
            let (family, address, identity_node_id) = row?;
            let node_id = NodeId(identity_node_id.try_into().map_err(|_| {
                StoreError::DurableState("invalid persisted DHT identity node ID".to_owned())
            })?);
            let identity = match family {
                4 => DhtIdentity {
                    address: IpAddr::V4(Ipv4Addr::from(<[u8; 4]>::try_from(address).map_err(
                        |_| {
                            StoreError::DurableState(
                                "invalid persisted DHT identity IPv4 address".to_owned(),
                            )
                        },
                    )?)),
                    node_id,
                },
                6 => DhtIdentity {
                    address: IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(address).map_err(
                        |_| {
                            StoreError::DurableState(
                                "invalid persisted DHT identity IPv6 address".to_owned(),
                            )
                        },
                    )?)),
                    node_id,
                },
                _ => {
                    return Err(StoreError::DurableState(
                        "invalid persisted DHT identity family".to_owned(),
                    ));
                }
            };
            if family == 4 {
                identities_v4.push(identity);
            } else {
                identities_v6.push(identity);
            }
        }
        DhtSnapshot {
            version,
            legacy_node_id: (version == 1).then_some(node_id),
            identities_v4,
            identities_v6,
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
        transaction.execute("DELETE FROM dht_identities", [])?;
        transaction.execute("DELETE FROM dht_nodes", [])?;
        transaction.execute("DELETE FROM dht_state", [])?;
        transaction.execute(
            "INSERT INTO dht_state(singleton, format_version, node_id)
             VALUES (1, ?1, ?2)",
            params![
                i64::from(snapshot.version),
                snapshot
                    .legacy_node_id
                    .or_else(|| snapshot
                        .identities_v4
                        .first()
                        .map(|identity| identity.node_id))
                    .or_else(|| snapshot
                        .identities_v6
                        .first()
                        .map(|identity| identity.node_id))
                    .unwrap_or(NodeId([1; 20]))
                    .0
                    .as_slice()
            ],
        )?;
        for (family, identities) in [
            (4_i64, snapshot.identities_v4.clone()),
            (6_i64, snapshot.identities_v6.clone()),
        ] {
            for (order, identity) in identities.into_iter().enumerate() {
                let address = match identity.address {
                    IpAddr::V4(address) => address.octets().to_vec(),
                    IpAddr::V6(address) => address.octets().to_vec(),
                };
                transaction.execute(
                    "INSERT INTO dht_identities(
                        family, identity_order, address, node_id
                     ) VALUES (?1, ?2, ?3, ?4)",
                    params![
                        family,
                        i64::try_from(order).expect("DHT identity order is bounded"),
                        address,
                        identity.node_id.0.as_slice(),
                    ],
                )?;
            }
        }
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
            let mut response = ResponseEnvelope::success(
                request.request_id.clone(),
                self.revision()?,
                self.snapshot()?,
            );
            if let Command::ExportMagnet { torrent_id } = &request.command {
                response = match self.export_magnet(torrent_id) {
                    Ok(result) => response.with_result(CommandResult::ExportMagnet { result }),
                    Err(StoreError::UnknownTorrent(_)) => ResponseEnvelope::error(
                        request.request_id.clone(),
                        self.revision()?,
                        ErrorCode::UnknownTorrent,
                        format!(
                            "torrent {} is not in the profile",
                            torrent_id.to_ascii_lowercase()
                        ),
                    ),
                    Err(error) => return Err(error),
                };
            }
            return Ok(response);
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
                "SELECT magnet, storage_root, raw_info, publication_name,
                        piece_count, have_state, desired_state, payload_state,
                        verification_requested, verification_completed,
                        quarantine_reason, download_queue_position
                 FROM torrents
                WHERE info_hash = ?1",
                [info_hash.as_slice()],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<Vec<u8>>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                        row.get::<_, Option<Vec<u8>>>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, i64>(8)?,
                        row.get::<_, i64>(9)?,
                        row.get::<_, Option<String>>(10)?,
                        row.get::<_, Option<i64>>(11)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| StoreError::UnknownTorrent(torrent_id.to_owned()))?;
        let payload_state = PayloadState::parse(&row.7)
            .ok_or_else(|| StoreError::DurableState("invalid payload state".to_owned()))?;
        let verification_requested = u64::try_from(row.8).map_err(|_| {
            StoreError::DurableState("invalid requested verification generation".to_owned())
        })?;
        let verification_completed = u64::try_from(row.9).map_err(|_| {
            StoreError::DurableState("invalid completed verification generation".to_owned())
        })?;
        let verification = VerificationState::new(verification_requested, verification_completed)
            .ok_or_else(|| {
            StoreError::DurableState(
                "completed verification exceeds requested generation".to_owned(),
            )
        })?;
        let skip_files = read_selection(&self.connection, &info_hash)?;
        let trackers = read_trackers(&self.connection, &info_hash)?;
        let have = match (row.4, row.5) {
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
        let desired_running = match row.6.as_str() {
            "running" => true,
            "paused" => false,
            _ => {
                return Err(StoreError::DurableState(
                    "invalid desired torrent state".to_owned(),
                ));
            }
        };
        let (has_wanted_pieces, all_wanted_verified, evidence_error) = match (&row.2, &have) {
            (Some(raw_info), Some(have)) => {
                match wanted_piece_evidence(raw_info, &skip_files, have) {
                    Ok((has_wanted, all_verified)) => (has_wanted, all_verified, None),
                    Err(error) => (true, false, Some(bounded_error(&error.to_string()))),
                }
            }
            _ => (true, false, None),
        };
        let quarantine_reason = row.10.or(evidence_error);
        let state = derive_torrent_state(DerivedStateInput {
            metadata_available: row.2.is_some(),
            root_available: true,
            desired_running,
            has_wanted_pieces,
            payload: payload_state,
            verification,
            all_wanted_verified,
            quarantined: quarantine_reason.is_some(),
        });
        let storage_state = if quarantine_reason.is_some() {
            StorageState::NeedsRepair
        } else {
            payload_state.storage_state()
        };
        let managed_artifacts = match payload_state {
            PayloadState::Absent => ManagedArtifactState::None,
            PayloadState::LegacyOwned => ManagedArtifactState::Legacy,
            PayloadState::WorkOwned | PayloadState::PublicationPending => {
                ManagedArtifactState::Staging
            }
            PayloadState::FinalOwned => ManagedArtifactState::Published,
        };
        Ok(ResumeRecord {
            torrent_id: torrent_id.to_ascii_lowercase(),
            magnet: operational_magnet,
            storage_root: row.1,
            skip_files,
            trackers,
            state,
            storage_state,
            desired_running,
            download_queue_position: row.11,
            raw_info: row.2,
            publication_name: row.3,
            managed_artifacts,
            have,
            payload_state,
            verification,
            quarantine_reason,
        })
    }

    fn export_magnet(&self, torrent_id: &str) -> Result<MagnetExportResult, StoreError> {
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        let publication_name = self
            .connection
            .query_row(
                "SELECT publication_name FROM torrents WHERE info_hash = ?1",
                [info_hash.as_slice()],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::UnknownTorrent(torrent_id.to_ascii_lowercase()))?;

        let source = self
            .connection
            .query_row(
                "SELECT kind, fidelity, magnet, byte_length, sha256
                 FROM torrent_source WHERE info_hash = ?1",
                [info_hash.as_slice()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, Vec<u8>>(4)?,
                    ))
                },
            )
            .optional()?;
        if let Some((kind, fidelity, Some(magnet), byte_length, digest)) = source
            && kind == "magnet"
            && usize::try_from(byte_length).ok() == Some(magnet.len())
            && digest == Sha256::digest(magnet.as_bytes()).as_slice()
            && Magnet::parse(&magnet).is_ok_and(|parsed| parsed.info_hash == info_hash)
            && let Some(source) = match fidelity.as_str() {
                "verbatim" => Some(MagnetExportSource::Verbatim),
                "canonicalized" => Some(MagnetExportSource::Canonicalized),
                _ => None,
            }
        {
            return Ok(MagnetExportResult {
                magnet,
                source,
                omitted_tracker_count: 0,
            });
        }

        Ok(synthesize_magnet_export(
            info_hash,
            publication_name.as_deref(),
            &read_trackers(&self.connection, &info_hash)?,
        ))
    }

    pub fn load_removal(&self, torrent_id: &str) -> Result<RemovalRecord, StoreError> {
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        self.connection
            .query_row(
                "SELECT r.operation_id, t.storage_root, r.data_policy, r.state,
                        t.raw_info, t.publication_name, t.payload_state, r.error
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
                        payload_state: row.get(6)?,
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
                    r.state, t.raw_info, t.publication_name, t.payload_state, r.error
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
                    payload_state: row.get(7)?,
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
                 payload_state = 'absent',
                 error = NULL,
                 updated_revision = ?6
             WHERE info_hash = ?1",
            params![
                expected_info_hash.as_slice(),
                raw_info,
                metainfo.name,
                i64::try_from(metainfo.piece_count())
                    .map_err(|_| StoreError::DurableState("piece count overflow".to_owned()))?,
                have,
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

    pub fn invalidate_pieces(
        &mut self,
        torrent_id: &str,
        piece_indices: &[usize],
    ) -> Result<u64, StoreError> {
        if piece_indices.is_empty() {
            return Err(StoreError::DurableState(
                "invalidated piece batch must be nonempty".to_owned(),
            ));
        }
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        let transaction = self.connection.transaction()?;
        let (piece_count, bytes) = read_have_columns(&transaction, &info_hash)?;
        let mut have = HaveState::decode(&bytes, info_hash, piece_count)?;
        for &piece_index in piece_indices {
            have.set(piece_index, false)?;
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
        self.begin_recheck_with_generation(torrent_id)
            .map(|(revision, _)| revision)
    }

    pub(crate) fn begin_recheck_with_generation(
        &mut self,
        torrent_id: &str,
    ) -> Result<(u64, u64), StoreError> {
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        let transaction = self.connection.transaction()?;
        let (requested, completed, updated_revision) = transaction
            .query_row(
                "SELECT verification_requested, verification_completed, updated_revision
                 FROM torrents WHERE info_hash = ?1 AND raw_info IS NOT NULL
                       AND have_state IS NOT NULL",
                [info_hash.as_slice()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| {
                StoreError::DurableState(
                    "recheck requires verified metadata and have state".to_owned(),
                )
            })?;
        if requested != completed {
            let revision = u64::try_from(updated_revision)
                .map_err(|_| StoreError::DurableState("torrent revision is invalid".to_owned()))?;
            let generation = u64::try_from(requested).map_err(|_| {
                StoreError::DurableState("verification generation is invalid".to_owned())
            })?;
            return Ok((revision, generation));
        }
        let next_requested = requested.checked_add(1).ok_or_else(|| {
            StoreError::DurableState("verification generation overflow".to_owned())
        })?;
        let revision = increment_revision(&transaction)?;
        let revision_sql = sql_revision(revision)?;
        let updated = transaction.execute(
            "UPDATE torrents
             SET error = NULL, verification_requested = ?2,
                 updated_revision = ?3
             WHERE info_hash = ?1 AND raw_info IS NOT NULL
                   AND have_state IS NOT NULL",
            params![info_hash.as_slice(), next_requested, revision_sql,],
        )?;
        if updated != 1 {
            return Err(StoreError::DurableState(
                "recheck requires verified metadata and have state".to_owned(),
            ));
        }
        transaction.commit()?;
        let generation = u64::try_from(next_requested).map_err(|_| {
            StoreError::DurableState("verification generation is invalid".to_owned())
        })?;
        Ok((revision, generation))
    }

    pub fn complete_recheck(
        &mut self,
        torrent_id: &str,
        have: &HaveState,
    ) -> Result<u64, StoreError> {
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        let requested = self
            .connection
            .query_row(
                "SELECT verification_requested FROM torrents WHERE info_hash = ?1",
                [info_hash.as_slice()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::UnknownTorrent(torrent_id.to_owned()))?;
        let generation = u64::try_from(requested).map_err(|_| {
            StoreError::DurableState("verification generation is invalid".to_owned())
        })?;
        self.complete_recheck_generation(torrent_id, generation, have)
    }

    pub(crate) fn complete_recheck_generation(
        &mut self,
        torrent_id: &str,
        generation: u64,
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
        let (piece_count, current_bytes) = read_have_columns(&transaction, &info_hash)?;
        if have.pieces().len() != piece_count {
            return Err(StoreError::DurableState(
                "replacement have state has the wrong piece count".to_owned(),
            ));
        }
        let (requested, completed, updated_revision) = transaction.query_row(
            "SELECT verification_requested, verification_completed, updated_revision
             FROM torrents WHERE info_hash = ?1",
            [info_hash.as_slice()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )?;
        let generation_sql = i64::try_from(generation)
            .map_err(|_| StoreError::DurableState("verification generation overflow".to_owned()))?;
        if requested != generation_sql {
            return Err(StoreError::DurableState(
                "stale recheck completion generation".to_owned(),
            ));
        }
        if completed == requested {
            let current = HaveState::decode(&current_bytes, info_hash, piece_count)?;
            if &current == have {
                return u64::try_from(updated_revision).map_err(|_| {
                    StoreError::DurableState("torrent revision is invalid".to_owned())
                });
            }
            return Err(StoreError::DurableState(
                "completed verification generation has different evidence".to_owned(),
            ));
        }
        let revision = increment_revision(&transaction)?;
        let revision_sql = sql_revision(revision)?;
        transaction.execute(
            "UPDATE torrents
             SET have_state = ?2,
                 error = NULL, quarantine_reason = NULL,
                 verification_completed = verification_requested,
                 updated_revision = ?3
             WHERE info_hash = ?1",
            params![info_hash.as_slice(), have.encode(), revision_sql],
        )?;
        transaction.commit()?;
        Ok(revision)
    }

    pub fn mark_complete(&mut self, torrent_id: &str) -> Result<u64, StoreError> {
        self.update_payload_state(torrent_id, PayloadState::FinalOwned, None)
    }

    pub fn mark_storage_prepared(
        &mut self,
        torrent_id: &str,
        storage_state: StorageState,
    ) -> Result<u64, StoreError> {
        let payload_state = match storage_state {
            StorageState::Staging => PayloadState::WorkOwned,
            StorageState::Published => PayloadState::FinalOwned,
            _ => {
                return Err(StoreError::DurableState(
                    "storage preparation requires staging or published ownership".to_owned(),
                ));
            }
        };
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        let transaction = self.connection.transaction()?;
        let revision = increment_revision(&transaction)?;
        let revision_sql = sql_revision(revision)?;
        let updated = transaction.execute(
            "UPDATE torrents
             SET payload_state = ?2,
                 download_queue_position = CASE
                    WHEN ?2 = 'final_owned' THEN NULL
                    ELSE download_queue_position
                 END,
                 updated_revision = ?3
             WHERE info_hash = ?1",
            params![info_hash.as_slice(), payload_state.as_str(), revision_sql],
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
        let (payload_state, updated_revision) = transaction
            .query_row(
                "SELECT payload_state, updated_revision
                 FROM torrents WHERE info_hash = ?1",
                [info_hash.as_slice()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
            .ok_or_else(|| StoreError::UnknownTorrent(torrent_id.to_owned()))?;
        let payload_state = PayloadState::parse(&payload_state)
            .ok_or_else(|| StoreError::DurableState("invalid payload state".to_owned()))?;
        if payload_state == PayloadState::PublicationPending {
            return u64::try_from(updated_revision)
                .map_err(|_| StoreError::DurableState("torrent revision is invalid".to_owned()));
        }
        if payload_state != PayloadState::WorkOwned {
            return Err(StoreError::DurableState(
                "path publication requires owned durable staging data".to_owned(),
            ));
        }
        let revision = increment_revision(&transaction)?;
        let revision_sql = sql_revision(revision)?;
        transaction.execute(
            "UPDATE torrents
             SET error = NULL, payload_state = 'publication_pending',
                 updated_revision = ?2
             WHERE info_hash = ?1",
            params![info_hash.as_slice(), revision_sql],
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
             SET error = ?2, updated_revision = ?3
             WHERE info_hash = ?1",
            params![info_hash.as_slice(), error, revision_sql],
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
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &skip_files)
            .map_err(|error| StoreError::DurableState(error.to_string()))?;
        let wanted = layout
            .files()
            .iter()
            .enumerate()
            .filter(|(file_index, file)| !file.padding && selection.is_wanted(*file_index))
            .map(|(file_index, file)| (file_index, file.length))
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
             SET error = NULL, payload_state = 'publication_pending',
                 updated_revision = ?2
             WHERE info_hash = ?1",
            params![info_hash.as_slice(), revision_sql],
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
        let (payload_state, updated_revision) = transaction
            .query_row(
                "SELECT payload_state, updated_revision
                 FROM torrents WHERE info_hash = ?1",
                [info_hash.as_slice()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
            .ok_or_else(|| StoreError::UnknownTorrent(torrent_id.to_owned()))?;
        let payload_state = PayloadState::parse(&payload_state)
            .ok_or_else(|| StoreError::DurableState("invalid payload state".to_owned()))?;
        if payload_state == PayloadState::FinalOwned {
            return u64::try_from(updated_revision)
                .map_err(|_| StoreError::DurableState("torrent revision is invalid".to_owned()));
        }
        if payload_state != PayloadState::PublicationPending {
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
             SET error = NULL, payload_state = 'final_owned',
                 download_queue_position = NULL,
                 updated_revision = ?2
             WHERE info_hash = ?1",
            params![info_hash.as_slice(), revision_sql],
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
        let (payload_state, requested, completed, updated_revision) = transaction
            .query_row(
                "SELECT payload_state, verification_requested,
                        verification_completed, updated_revision
                 FROM torrents WHERE info_hash = ?1",
                [info_hash.as_slice()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| StoreError::UnknownTorrent(torrent_id.to_owned()))?;
        let payload_state = PayloadState::parse(&payload_state)
            .ok_or_else(|| StoreError::DurableState("invalid payload state".to_owned()))?;
        if payload_state == PayloadState::FinalOwned {
            return u64::try_from(updated_revision)
                .map_err(|_| StoreError::DurableState("torrent revision is invalid".to_owned()));
        }
        if payload_state != PayloadState::PublicationPending {
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
        let next_requested = if requested == completed {
            requested.checked_add(1).ok_or_else(|| {
                StoreError::DurableState("verification generation overflow".to_owned())
            })?
        } else {
            requested
        };
        transaction.execute(
            "UPDATE torrents
             SET error = NULL, payload_state = 'final_owned',
                 download_queue_position = NULL,
                 verification_requested = ?2, updated_revision = ?3
             WHERE info_hash = ?1",
            params![info_hash.as_slice(), next_requested, revision_sql],
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
        let info_hash = decode_info_hash(torrent_id)
            .ok_or_else(|| StoreError::DurableState("invalid torrent identity".to_owned()))?;
        let transaction = self.connection.transaction()?;
        let revision = increment_revision(&transaction)?;
        let updated = transaction.execute(
            "UPDATE torrents
             SET quarantine_reason = ?2, error = ?2, updated_revision = ?3
             WHERE info_hash = ?1",
            params![
                info_hash.as_slice(),
                bounded_error(message),
                sql_revision(revision)?,
            ],
        )?;
        if updated != 1 {
            return Err(StoreError::UnknownTorrent(torrent_id.to_owned()));
        }
        transaction.commit()?;
        Ok(revision)
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
             SET error = ?2,
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

    fn update_payload_state(
        &mut self,
        torrent_id: &str,
        payload_state: PayloadState,
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
             SET payload_state = ?2, quarantine_reason = NULL,
                 download_queue_position = CASE
                    WHEN ?2 = 'final_owned' THEN NULL
                    ELSE download_queue_position
                 END,
                 error = ?3, updated_revision = ?4
             WHERE info_hash = ?1",
            params![
                info_hash.as_slice(),
                payload_state.as_str(),
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
                raw_info BLOB CHECK (
                    raw_info IS NULL OR length(raw_info) <= 67108864
                ),
                publication_name TEXT CHECK (
                    publication_name IS NULL OR
                    length(publication_name) BETWEEN 1 AND 255
                ),
                payload_state TEXT NOT NULL DEFAULT 'absent' CHECK (
                    payload_state IN (
                        'absent', 'legacy_owned', 'work_owned',
                        'publication_pending', 'final_owned'
                    )
                ),
                verification_requested INTEGER NOT NULL DEFAULT 0 CHECK (
                    verification_requested >= 0
                ),
                verification_completed INTEGER NOT NULL DEFAULT 0 CHECK (
                    verification_completed >= 0 AND
                    verification_completed <= verification_requested
                ),
                quarantine_reason TEXT CHECK (
                    quarantine_reason IS NULL OR length(quarantine_reason) <= 1024
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
                download_queue_position INTEGER,
                upload_rate_limit INTEGER NOT NULL DEFAULT 0 CHECK (
                    upload_rate_limit = 0 OR
                    upload_rate_limit BETWEEN 1024 AND 4294967295
                ),
                download_rate_limit INTEGER NOT NULL DEFAULT 0 CHECK (
                    download_rate_limit = 0 OR
                    download_rate_limit BETWEEN 1024 AND 4294967295
                ),
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
        transaction.execute_batch(DOWNLOAD_QUEUE_INDEX_SQL)?;
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
    if (1..=13).contains(&version) {
        migrate_payload_facts_to_v14(connection)?;
    }
    if (1..=14).contains(&version) {
        migrate_client_settings_to_v15_store(connection)?;
    }
    if (1..=15).contains(&version) {
        migrate_dual_stack_state_to_v16(connection)?;
    }
    if (1..=16).contains(&version) {
        migrate_to_v17(connection)?;
    }
    if (1..=17).contains(&version) {
        migrate_to_v18(connection)?;
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

struct PayloadMigrationResult {
    info_hash: Vec<u8>,
    payload_state: PayloadState,
    quarantine_reason: Option<String>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ArtifactObservation {
    Missing,
    Exact,
    Unsafe,
}

fn observe_artifact(path: &Path, shape: PublicationShape) -> ArtifactObservation {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => ArtifactObservation::Unsafe,
        Ok(metadata)
            if matches!(shape, PublicationShape::File) && metadata.file_type().is_file() =>
        {
            ArtifactObservation::Exact
        }
        Ok(metadata)
            if matches!(shape, PublicationShape::Tree) && metadata.file_type().is_dir() =>
        {
            ArtifactObservation::Exact
        }
        Ok(_) => ArtifactObservation::Unsafe,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ArtifactObservation::Missing,
        Err(_) => ArtifactObservation::Unsafe,
    }
}

fn observe_part(path: &Path) -> ArtifactObservation {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() && !metadata.file_type().is_symlink() => {
            ArtifactObservation::Exact
        }
        Ok(_) => ArtifactObservation::Unsafe,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ArtifactObservation::Missing,
        Err(_) => ArtifactObservation::Unsafe,
    }
}

fn inspect_payload_migrations(
    connection: &Connection,
) -> Result<Vec<PayloadMigrationResult>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT t.info_hash, t.raw_info, t.publication_name, t.storage_state,
                t.managed_artifacts, r.kind, r.locator
         FROM torrents t JOIN storage_roots r ON r.root_id = t.storage_root
         ORDER BY t.info_hash",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, Vec<u8>>(0)?,
            row.get::<_, Option<Vec<u8>>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
        ))
    })?;
    let mut results = Vec::new();
    for row in rows {
        let (info_hash, raw_info, publication_name, storage, managed, kind, locator) = row?;
        let hash: [u8; 20] = info_hash.clone().try_into().map_err(|_| {
            StoreError::DurableState("invalid schema-13 info-hash length".to_owned())
        })?;
        let pair = (storage.as_str(), managed.as_str());
        let semantic = match pair {
            (_, "legacy") => Some(PayloadState::LegacyOwned),
            ("none", "none") => Some(PayloadState::Absent),
            ("staging", "staging") => Some(PayloadState::WorkOwned),
            ("prepared", "staging" | "published") => Some(PayloadState::PublicationPending),
            ("published", "published") => Some(PayloadState::FinalOwned),
            ("staging", "published") => Some(PayloadState::FinalOwned),
            _ => None,
        };
        let mut payload_state = semantic.unwrap_or(PayloadState::Absent);
        let mut quarantine = semantic
            .is_none()
            .then(|| "schema-13 payload state could not be mapped safely".to_owned());
        if kind == "path" {
            let inspected = raw_info.as_deref().map(parse_durable_metainfo).transpose();
            match inspected {
                Err(error) => quarantine = Some(bounded_error(&error.to_string())),
                Ok(None) if payload_state != PayloadState::Absent => {
                    quarantine = Some("owned payload has no verified metadata".to_owned());
                }
                Ok(None) => {}
                Ok(Some(metainfo)) => {
                    if publication_name
                        .as_deref()
                        .is_some_and(|name| name != metainfo.name)
                    {
                        quarantine = Some(
                            "stored publication name does not match verified metadata".to_owned(),
                        );
                    } else if let Ok(paths) =
                        torrent_storage_paths_for_metainfo(Path::new(&locator), &metainfo)
                    {
                        let shape = PublicationShape::from_metainfo(&metainfo);
                        let final_side = observe_artifact(&paths.output, shape);
                        let work_side = observe_artifact(&paths.staging, shape);
                        let part = observe_part(&paths.part);
                        if part == ArtifactObservation::Unsafe {
                            quarantine =
                                Some("selective part artifact has an unsafe type".to_owned());
                        }
                        match pair {
                            (_, "legacy") => {
                                let legacy = observe_artifact(
                                    &Path::new(&locator).join(encode_info_hash(hash)),
                                    shape,
                                );
                                if legacy != ArtifactObservation::Exact {
                                    quarantine = Some(
                                        "legacy owned payload is missing or unsafe".to_owned(),
                                    );
                                }
                            }
                            ("none", "none") => {
                                if final_side != ArtifactObservation::Missing
                                    || work_side != ArtifactObservation::Missing
                                {
                                    quarantine = Some(
                                        "unowned row has an exact managed artifact".to_owned(),
                                    );
                                }
                            }
                            ("staging", "staging") => {
                                if work_side != ArtifactObservation::Exact
                                    || final_side != ArtifactObservation::Missing
                                {
                                    quarantine = Some(
                                        "work-owned payload is missing, unsafe, or ambiguous"
                                            .to_owned(),
                                    );
                                }
                            }
                            ("prepared", "staging" | "published") => {
                                if !matches!(
                                    (work_side, final_side),
                                    (ArtifactObservation::Exact, ArtifactObservation::Missing)
                                        | (
                                            ArtifactObservation::Missing,
                                            ArtifactObservation::Exact
                                        )
                                ) {
                                    quarantine = Some(
                                        "pending publication has no single safe artifact side"
                                            .to_owned(),
                                    );
                                }
                            }
                            ("published", "published") => {
                                if final_side != ArtifactObservation::Exact
                                    || work_side != ArtifactObservation::Missing
                                {
                                    quarantine = Some(
                                        "final-owned payload is missing, unsafe, or ambiguous"
                                            .to_owned(),
                                    );
                                }
                            }
                            ("staging", "published") => match (work_side, final_side) {
                                (ArtifactObservation::Missing, ArtifactObservation::Exact) => {
                                    payload_state = PayloadState::FinalOwned;
                                }
                                (ArtifactObservation::Exact, ArtifactObservation::Missing) => {
                                    payload_state = PayloadState::WorkOwned;
                                }
                                _ => {
                                    quarantine = Some(
                                        "defective schema-13 payload has no single safe artifact side"
                                            .to_owned(),
                                    );
                                }
                            },
                            _ => {}
                        }
                    } else {
                        quarantine = Some("stored publication path is invalid".to_owned());
                    }
                }
            }
        } else if kind == "platform" && pair == ("staging", "published") {
            quarantine =
                Some("platform payload ownership requires provider reconciliation".to_owned());
        }
        results.push(PayloadMigrationResult {
            info_hash,
            payload_state,
            quarantine_reason: quarantine.map(|reason| bounded_error(&reason)),
        });
    }
    Ok(results)
}

fn migrate_payload_facts_to_v14(connection: &mut Connection) -> Result<(), StoreError> {
    let inspected = inspect_payload_migrations(connection)?;
    let transaction = connection.transaction()?;
    let columns = {
        let mut statement = transaction.prepare("PRAGMA table_info(torrents)")?;
        let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
        columns.collect::<Result<Vec<_>, _>>()?
    };
    if !columns.iter().any(|column| column == "payload_state") {
        transaction.execute_batch(
            "ALTER TABLE torrents ADD COLUMN payload_state TEXT NOT NULL DEFAULT 'absent'
            CHECK (payload_state IN (
                'absent', 'legacy_owned', 'work_owned',
                'publication_pending', 'final_owned'
            ));
         ALTER TABLE torrents ADD COLUMN verification_requested INTEGER NOT NULL DEFAULT 0
            CHECK (verification_requested >= 0);
         ALTER TABLE torrents ADD COLUMN verification_completed INTEGER NOT NULL DEFAULT 0
            CHECK (
                verification_completed >= 0 AND
                verification_completed <= verification_requested
            );
         ALTER TABLE torrents ADD COLUMN quarantine_reason TEXT
            CHECK (quarantine_reason IS NULL OR length(quarantine_reason) <= 1024);",
        )?;
    }
    transaction.execute_batch(
        "UPDATE torrents
            SET payload_state = CASE
                WHEN managed_artifacts = 'legacy'
                    THEN 'legacy_owned'
                WHEN storage_state = 'none' AND managed_artifacts = 'none'
                    THEN 'absent'
                WHEN storage_state = 'staging' AND managed_artifacts = 'staging'
                    THEN 'work_owned'
                WHEN storage_state = 'prepared' AND managed_artifacts IN ('staging', 'published')
                    THEN 'publication_pending'
                WHEN storage_state = 'published' AND managed_artifacts = 'published'
                    THEN 'final_owned'
                WHEN storage_state = 'staging' AND managed_artifacts = 'published'
                    THEN 'final_owned'
                ELSE 'absent'
            END,
                verification_requested = CASE
                    WHEN raw_info IS NULL THEN 0 ELSE 1
                END,
                verification_completed = CASE
                    WHEN raw_info IS NULL THEN 0
                    WHEN state IN ('complete', 'downloading', 'paused')
                         AND error IS NULL THEN 0
                    ELSE 0
                END,
                quarantine_reason = CASE
                    WHEN state IN ('needs_repair', 'error')
                        THEN COALESCE(error, 'schema-13 torrent requires repair')
                    WHEN managed_artifacts = 'legacy'
                        THEN NULL
                    WHEN (storage_state = 'none' AND managed_artifacts = 'none')
                      OR (storage_state = 'staging' AND managed_artifacts = 'staging')
                      OR (storage_state = 'prepared' AND managed_artifacts IN ('staging', 'published'))
                      OR (storage_state = 'published' AND managed_artifacts = 'published')
                      OR (storage_state = 'staging' AND managed_artifacts = 'published')
                        THEN NULL
                    ELSE 'schema-13 payload state could not be mapped safely'
                END;",
    )?;
    for result in inspected {
        transaction.execute(
            "UPDATE torrents
             SET payload_state = ?2,
                 quarantine_reason = COALESCE(?3, quarantine_reason)
             WHERE info_hash = ?1",
            params![
                result.info_hash,
                result.payload_state.as_str(),
                result.quarantine_reason,
            ],
        )?;
    }
    transaction.execute_batch(
        "ALTER TABLE torrents DROP COLUMN state;
         ALTER TABLE torrents DROP COLUMN storage_state;
         ALTER TABLE torrents DROP COLUMN managed_artifacts;",
    )?;
    transaction.pragma_update(None, "user_version", 14)?;
    transaction.commit()?;
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

fn migrate_client_settings_to_v15_store(connection: &mut Connection) -> Result<(), StoreError> {
    let transaction = connection.transaction()?;
    migrate_client_settings_to_v15(&transaction)?;
    transaction.pragma_update(None, "user_version", 15)?;
    transaction.commit()?;
    Ok(())
}

fn migrate_dual_stack_state_to_v16(connection: &mut Connection) -> Result<(), StoreError> {
    let transaction = connection.transaction()?;
    migrate_client_settings_to_v16(&transaction)?;
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS dht_identities (
            family INTEGER NOT NULL CHECK (family IN (4, 6)),
            identity_order INTEGER NOT NULL CHECK (
                identity_order >= 0 AND identity_order < 8
            ),
            address BLOB NOT NULL CHECK (
                (family = 4 AND length(address) = 4) OR
                (family = 6 AND length(address) = 16)
            ),
            node_id BLOB NOT NULL CHECK (length(node_id) = 20),
            PRIMARY KEY (family, identity_order),
            UNIQUE (family, address)
         ) WITHOUT ROWID;",
    )?;
    transaction.pragma_update(None, "user_version", 16)?;
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
            false,
            current_revision,
        ),
        Command::DownloadFiles {
            torrent_id,
            file_indices,
        } => download_files(transaction, torrent_id, file_indices, current_revision),
        Command::MoveDownloadToTop { torrent_id } => {
            move_download_to_edge(transaction, torrent_id, QueueEdge::Top, current_revision)
        }
        Command::MoveDownloadToBottom { torrent_id } => {
            move_download_to_edge(transaction, torrent_id, QueueEdge::Bottom, current_revision)
        }
        Command::SetDefaultStorageRoot { storage_root } => {
            set_default_storage_root(transaction, storage_root, current_revision)
        }
        Command::SetShowAddOptions { show } => {
            set_show_add_options(transaction, *show, current_revision)
        }
        Command::SetClientSettings { .. } => unreachable!("settings are handled atomically above"),
        Command::SetTorrentTransferLimits { torrent_id, limits } => {
            set_torrent_transfer_limits(transaction, torrent_id, *limits, current_revision)
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
        Command::ExportMagnet { .. } | Command::Snapshot | Command::Shutdown => {
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
                info_hash, magnet, storage_root, desired_state, payload_state,
                raw_info, publication_name, piece_count, have_state,
                created_revision, updated_revision, selection_default
             ) VALUES (
                ?1, NULL, ?2, ?3, 'absent', ?4, ?5, ?6, ?7, ?8, ?8, ?9
             )",
            params![
                metainfo.info_hash.as_slice(),
                request.storage_root,
                if request.start_content {
                    "running"
                } else {
                    "paused"
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
    if request.start_content {
        download_queue::append(transaction, &metainfo.info_hash)
            .map_err(AddTorrentBytesError::Store)?;
    }
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
            "SELECT raw_info IS NOT NULL, payload_state,
                    verification_requested, verification_completed,
                    EXISTS(SELECT 1 FROM removal_jobs r
                           WHERE r.info_hash = torrents.info_hash)
             FROM torrents WHERE info_hash = ?1",
            [info_hash.as_slice()],
            |row| {
                Ok((
                    row.get::<_, bool>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, bool>(4)?,
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
    if row.4 {
        return Err((
            ErrorCode::InvalidTorrentState,
            "torrent removal is already in progress".to_owned(),
        ));
    }
    let payload_state = PayloadState::parse(&row.1)
        .ok_or_else(|| internal_message("database contains an invalid payload state"))?;
    if !row.0 || !payload_state.can_recheck() {
        return Err((
            ErrorCode::InvalidTorrentState,
            "force recheck requires verified managed staging or published content".to_owned(),
        ));
    }
    if row.2 < 0 || row.3 < 0 || row.3 > row.2 {
        return Err(internal_message(
            "database contains invalid verification generations",
        ));
    }
    let requested = if row.2 == row.3 {
        row.2
            .checked_add(1)
            .ok_or_else(|| internal_message("verification generation overflow"))?
    } else {
        row.2
    };
    if requested == row.2 {
        return Ok(current_revision);
    }
    let revision = next_revision(transaction, current_revision)?;
    let revision_sql =
        i64::try_from(revision).map_err(|_| internal_message("profile revision overflow"))?;
    transaction
        .execute(
            "UPDATE torrents
             SET error = NULL, updated_revision = ?2,
                 verification_requested = ?3
             WHERE info_hash = ?1",
            params![info_hash.as_slice(), revision_sql, requested,],
        )
        .map_err(internal_error)?;
    Ok(revision)
}

fn move_download_to_edge(
    transaction: &Transaction<'_>,
    torrent_id: &str,
    edge: QueueEdge,
    current_revision: u64,
) -> Result<u64, (ErrorCode, String)> {
    let info_hash = decode_info_hash(torrent_id).ok_or_else(|| {
        (
            ErrorCode::InvalidRequest,
            "invalid torrent identity".to_owned(),
        )
    })?;
    let eligible = transaction
        .query_row(
            "SELECT download_queue_position IS NOT NULL,
                    payload_state <> 'final_owned', archived = 0,
                    NOT EXISTS(SELECT 1 FROM removal_jobs r
                               WHERE r.info_hash = torrents.info_hash)
             FROM torrents WHERE info_hash = ?1",
            [info_hash.as_slice()],
            |row| {
                Ok((
                    row.get::<_, bool>(0)?,
                    row.get::<_, bool>(1)?,
                    row.get::<_, bool>(2)?,
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
    if !(eligible.0 && eligible.1 && eligible.2 && eligible.3) {
        return Err((
            ErrorCode::InvalidTorrentState,
            "queue movement requires an incomplete retained download with a queue position"
                .to_owned(),
        ));
    }
    if download_queue::is_at_edge(transaction, &info_hash, edge)
        .map_err(|error| internal_message(&error.to_string()))?
    {
        return Ok(current_revision);
    }
    let revision = next_revision(transaction, current_revision)?;
    download_queue::move_to_edge(transaction, &info_hash, edge)
        .map_err(|error| internal_message(&error.to_string()))?;
    transaction
        .execute(
            "UPDATE torrents SET updated_revision = ?2 WHERE info_hash = ?1",
            params![
                info_hash.as_slice(),
                i64::try_from(revision)
                    .map_err(|_| internal_message("profile revision overflow"))?
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
            "SELECT raw_info, selection_default, payload_state,
                    desired_state, archived, quarantine_reason,
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
                    row.get::<_, bool>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, bool>(6)?,
                ))
            },
        )
        .optional()
        .map_err(internal_error)?;
    if let Some((
        raw_info,
        selection_default,
        payload_state,
        desired_state,
        archived,
        quarantine_reason,
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
            && (quarantine_reason.is_some()
                || payload_state == PayloadState::PublicationPending.as_str())
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
        let request_verification = raw_info.is_some() && desired_state == "running" && !archived;
        transaction
            .execute(
                "UPDATE torrents SET updated_revision = ?2,
                    error = CASE WHEN ?3 THEN NULL ELSE error END
                 WHERE info_hash = ?1",
                params![
                    magnet.info_hash.as_slice(),
                    sql_revision(revision).map_err(|e| internal_message(&e.to_string()))?,
                    request_verification,
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
                info_hash, magnet, storage_root, desired_state, payload_state,
                created_revision, updated_revision, selection_default
             ) VALUES (?1, ?2, ?3, ?4, 'absent', ?5, ?5, ?6)",
            params![
                magnet.info_hash.as_slice(),
                canonical_magnet(&magnet),
                storage_root,
                if start_content { "running" } else { "paused" },
                revision_sql,
                if magnet.select_only.is_some() {
                    "skipped"
                } else {
                    "wanted"
                }
            ],
        )
        .map_err(internal_error)?;
    download_queue::append(transaction, &magnet.info_hash)
        .map_err(|error| internal_message(&error.to_string()))?;
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
        false,
        current_revision,
    )
}

fn download_files(
    transaction: &Transaction<'_>,
    torrent_id: &str,
    file_indices: &[u32],
    current_revision: u64,
) -> Result<u64, (ErrorCode, String)> {
    set_file_priority_indices(
        transaction,
        torrent_id,
        file_indices.iter().copied(),
        FilePriority::Normal,
        true,
        current_revision,
    )
}

fn set_file_priority_indices<I>(
    transaction: &Transaction<'_>,
    torrent_id: &str,
    file_indices: I,
    priority: FilePriority,
    set_running: bool,
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
            "SELECT t.raw_info, t.payload_state, t.quarantine_reason,
                    r.info_hash IS NOT NULL,
                    t.selection_default, t.desired_state, t.archived,
                    t.download_queue_position
             FROM torrents t
             LEFT JOIN removal_jobs r ON r.info_hash = t.info_hash
             WHERE t.info_hash = ?1",
            [info_hash.as_slice()],
            |row| {
                Ok((
                    row.get::<_, Option<Vec<u8>>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, bool>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, bool>(6)?,
                    row.get::<_, Option<i64>>(7)?,
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
    if !matches!(row.4.as_str(), "wanted" | "skipped") {
        return Err(internal_message(
            "database contains an invalid selection default",
        ));
    }
    let payload_state = PayloadState::parse(&row.1)
        .ok_or_else(|| internal_message("database contains an invalid payload state"))?;
    if !matches!(row.5.as_str(), "running" | "paused") {
        return Err(internal_message(
            "database contains an invalid desired torrent state",
        ));
    }
    if set_running && row.6 {
        return Err((
            ErrorCode::InvalidTorrentState,
            "archived torrent must be restored before downloading files".to_owned(),
        ));
    }
    let selection_changed = exceptions != initial_exceptions;
    let running_changed = set_running && row.5 != "running";
    let move_download_to_head = set_running
        && row.7.is_some()
        && !download_queue::is_at_edge(transaction, &info_hash, QueueEdge::Top)
            .map_err(|error| internal_message(&error.to_string()))?;
    let append_reopened_download = selection_changed
        && priority == FilePriority::Normal
        && row.5 == "running"
        && row.7.is_none();
    if (selection_changed || set_running)
        && (row.2.is_some() || payload_state == PayloadState::PublicationPending)
    {
        return Err((
            ErrorCode::InvalidTorrentState,
            "file selection cannot change during repair or publication".to_owned(),
        ));
    }
    if !selection_changed && !running_changed && !move_download_to_head && !append_reopened_download
    {
        return Ok(current_revision);
    }
    let revision = next_revision(transaction, current_revision)?;
    if selection_changed {
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
    }
    let desired_state = if set_running {
        "running"
    } else {
        row.5.as_str()
    };
    transaction
        .execute(
            "UPDATE torrents
             SET desired_state = ?2, error = NULL, updated_revision = ?3
             WHERE info_hash = ?1",
            params![
                info_hash.as_slice(),
                desired_state,
                i64::try_from(revision)
                    .map_err(|_| internal_message("profile revision overflow"))?
            ],
        )
        .map_err(internal_error)?;
    if set_running {
        if row.7.is_some() {
            download_queue::move_to_edge(transaction, &info_hash, QueueEdge::Top)
                .map_err(|error| internal_message(&error.to_string()))?;
        } else if selection_changed {
            download_queue::place_missing(transaction, &info_hash, QueueEdge::Top)
                .map_err(|error| internal_message(&error.to_string()))?;
        }
    } else if append_reopened_download {
        download_queue::append(transaction, &info_hash)
            .map_err(|error| internal_message(&error.to_string()))?;
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
            "SELECT t.desired_state, t.quarantine_reason, t.payload_state,
                    r.info_hash IS NOT NULL
             FROM torrents t
             LEFT JOIN removal_jobs r ON r.info_hash = t.info_hash
             WHERE t.info_hash = ?1",
            [info_hash.as_slice()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
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
    if row.0 == desired {
        return Ok(current_revision);
    }
    if running && row.1.is_some() {
        return Err((
            ErrorCode::InvalidTorrentState,
            "torrent cannot resume while quarantined".to_owned(),
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
            "UPDATE torrents
             SET desired_state = ?2, error = NULL, updated_revision = ?3
             WHERE info_hash = ?1",
            params![info_hash.as_slice(), desired, revision_sql],
        )
        .map_err(internal_error)?;
    if running && row.2 != PayloadState::FinalOwned.as_str() {
        download_queue::append(transaction, &info_hash)
            .map_err(|error| internal_message(&error.to_string()))?;
    }
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
             SET desired_state = 'paused', error = NULL,
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

fn set_torrent_transfer_limits(
    transaction: &Transaction<'_>,
    torrent_id: &str,
    limits: TorrentTransferLimits,
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
            "SELECT upload_rate_limit, download_rate_limit
             FROM torrents WHERE info_hash = ?1",
            [info_hash.as_slice()],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
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
    let desired = (limits.upload.persisted(), limits.download.persisted());
    if current == desired {
        return Ok(current_revision);
    }
    let revision = next_revision(transaction, current_revision)?;
    let changed = transaction
        .execute(
            "UPDATE torrents
             SET upload_rate_limit = ?2, download_rate_limit = ?3,
                 updated_revision = ?4
             WHERE info_hash = ?1",
            params![
                info_hash.as_slice(),
                desired.0,
                desired.1,
                i64::try_from(revision).map_err(|_| internal_message("revision overflow"))?,
            ],
        )
        .map_err(internal_error)?;
    if changed != 1 {
        return Err(internal_message(
            "torrent transfer-limit update changed an unexpected row count",
        ));
    }
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
    let queue_ordinals = read_download_queue_ordinals(connection)?;
    let mut statement = connection.prepare(
        "SELECT t.info_hash, t.storage_root, t.raw_info, t.piece_count,
                t.have_state, t.error, t.archived, r.state, r.error,
                t.desired_state, t.payload_state, t.verification_requested,
                t.verification_completed, t.quarantine_reason,
                t.upload_rate_limit, t.download_rate_limit
         FROM torrents t
         LEFT JOIN removal_jobs r ON r.info_hash = t.info_hash
         ORDER BY t.info_hash",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, Vec<u8>>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<Vec<u8>>>(2)?,
            row.get::<_, Option<i64>>(3)?,
            row.get::<_, Option<Vec<u8>>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, bool>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, String>(9)?,
            row.get::<_, String>(10)?,
            row.get::<_, i64>(11)?,
            row.get::<_, i64>(12)?,
            row.get::<_, Option<String>>(13)?,
            row.get::<_, i64>(14)?,
            row.get::<_, i64>(15)?,
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
        let payload = PayloadState::parse(&row.10)
            .ok_or_else(|| StoreError::DurableState("invalid payload state".to_owned()))?;
        let requested = u64::try_from(row.11).map_err(|_| {
            StoreError::DurableState("invalid requested verification generation".to_owned())
        })?;
        let completed = u64::try_from(row.12).map_err(|_| {
            StoreError::DurableState("invalid completed verification generation".to_owned())
        })?;
        let verification = VerificationState::new(requested, completed).ok_or_else(|| {
            StoreError::DurableState(
                "completed verification exceeds requested generation".to_owned(),
            )
        })?;
        let piece_count = match row.3 {
            Some(piece_count) => bounded_piece_count(piece_count)?,
            None => 0,
        };
        let have = match (&row.4, piece_count) {
            (Some(bytes), count) if count != 0 => HaveState::decode(bytes, info_hash, count).ok(),
            (None, 0) => None,
            _ => None,
        };
        let malformed_have =
            row.4.is_some() != (piece_count != 0) || (row.4.is_some() && have.is_none());
        let (selection_default, selection_exceptions) =
            read_selection_state(connection, &info_hash)?;
        let skip_files = if selection_default == FilePriority::Normal {
            selection_exceptions.clone()
        } else {
            read_selection(connection, &info_hash)?
        };
        let (has_wanted_pieces, all_wanted_verified, evidence_error) = match (&row.2, &have) {
            (Some(raw_info), Some(have)) => {
                match wanted_piece_evidence(raw_info, &skip_files, have) {
                    Ok((has_wanted, all_verified)) => (has_wanted, all_verified, None),
                    Err(error) => (true, false, Some(bounded_error(&error.to_string()))),
                }
            }
            _ => (true, false, None),
        };
        let quarantined = row.13.is_some() || malformed_have || evidence_error.is_some();
        let state = derive_torrent_state(DerivedStateInput {
            metadata_available: row.2.is_some(),
            root_available: true,
            desired_running: row.9 == "running",
            has_wanted_pieces,
            payload,
            verification,
            all_wanted_verified,
            quarantined,
        });
        let storage_state = if quarantined {
            StorageState::NeedsRepair
        } else {
            payload.storage_state()
        };
        let verified_piece_count = if verification.is_pending()
            || matches!(
                state,
                TorrentState::AwaitingStorage | TorrentState::NeedsRepair
            ) {
            0
        } else {
            have.as_ref().map_or(0, HaveState::verified_count)
        };
        torrents.push(TorrentSnapshot {
            torrent_id,
            storage_root: row.1,
            state,
            storage_state,
            metadata_available: row.2.is_some(),
            piece_count: u32::try_from(piece_count)
                .map_err(|_| StoreError::DurableState("piece count overflow".to_owned()))?,
            verified_piece_count: u32::try_from(verified_piece_count)
                .map_err(|_| StoreError::DurableState("verified count overflow".to_owned()))?,
            desired_running: row.9 == "running",
            download_queue_position: queue_ordinals.get(&info_hash).copied(),
            transfer_limits: TorrentTransferLimits {
                upload: TransferRateLimit::from_persisted(row.14).map_err(|error| {
                    StoreError::DurableState(format!("invalid torrent upload rate: {error}"))
                })?,
                download: TransferRateLimit::from_persisted(row.15).map_err(|error| {
                    StoreError::DurableState(format!("invalid torrent download rate: {error}"))
                })?,
            },
            skip_files: if selection_default == FilePriority::Normal {
                selection_exceptions.clone()
            } else {
                Vec::new()
            },
            selection_default,
            selection_exceptions,
            archived: row.6,
            removal_state: match row.7.as_deref() {
                Some(value) => {
                    Some(RemovalState::parse(value).ok_or_else(|| {
                        StoreError::DurableState("invalid removal state".to_owned())
                    })?)
                }
                None => None,
            },
            delete_managed_data_supported: true,
            force_recheck_available: row.2.is_some()
                && row.7.is_none()
                && row.13.is_none()
                && payload.can_recheck(),
            error: row.8.or(row.13).or(evidence_error).or(row.5),
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

fn read_download_queue_ordinals(
    connection: &Connection,
) -> Result<std::collections::BTreeMap<[u8; 20], u32>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT info_hash FROM torrents
         WHERE download_queue_position IS NOT NULL
         ORDER BY download_queue_position, info_hash",
    )?;
    let rows = statement.query_map([], |row| row.get::<_, Vec<u8>>(0))?;
    let mut ordinals = std::collections::BTreeMap::new();
    for (index, row) in rows.enumerate() {
        let info_hash: [u8; 20] = row?
            .try_into()
            .map_err(|_| StoreError::DurableState("invalid info-hash length".to_owned()))?;
        let ordinal = u32::try_from(index + 1)
            .map_err(|_| StoreError::DurableState("download queue is too large".to_owned()))?;
        ordinals.insert(info_hash, ordinal);
    }
    Ok(ordinals)
}

struct RemovalRow {
    operation_id: String,
    storage_root: String,
    data_policy: String,
    state: String,
    raw_info: Option<Vec<u8>>,
    publication_name: Option<String>,
    payload_state: String,
    error: Option<String>,
}

fn removal_record(torrent_id: &str, row: RemovalRow) -> Result<RemovalRecord, StoreError> {
    let policy = RemovalDataPolicy::parse(&row.data_policy)
        .ok_or_else(|| StoreError::DurableState("invalid removal data policy".to_owned()))?;
    let state = RemovalState::parse(&row.state)
        .ok_or_else(|| StoreError::DurableState("invalid removal state".to_owned()))?;
    let payload_state = PayloadState::parse(&row.payload_state)
        .ok_or_else(|| StoreError::DurableState("invalid payload state".to_owned()))?;
    let storage_state = payload_state.storage_state();
    let managed_artifacts = match payload_state {
        PayloadState::Absent => ManagedArtifactState::None,
        PayloadState::LegacyOwned => ManagedArtifactState::Legacy,
        PayloadState::WorkOwned | PayloadState::PublicationPending => ManagedArtifactState::Staging,
        PayloadState::FinalOwned => ManagedArtifactState::Published,
    };
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

fn wanted_piece_evidence(
    raw_info: &[u8],
    skip_files: &[u32],
    have: &HaveState,
) -> Result<(bool, bool), StoreError> {
    let metainfo = parse_durable_metainfo(raw_info)?;
    let layout = TorrentLayout::from_metainfo(&metainfo);
    let skipped = skip_files
        .iter()
        .map(|index| {
            usize::try_from(*index)
                .map_err(|_| StoreError::DurableState("file index overflow".to_owned()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let selection = FileSelection::new(&layout, &skipped)
        .map_err(|error| StoreError::DurableState(error.to_string()))?;
    let mut has_wanted = false;
    for (piece_index, verified) in have.pieces().iter().copied().enumerate() {
        let piece_index = u32::try_from(piece_index)
            .map_err(|_| StoreError::DurableState("piece index overflow".to_owned()))?;
        if !layout
            .request_ranges(piece_index, &selection)
            .map_err(|error| StoreError::DurableState(error.to_string()))?
            .is_empty()
        {
            has_wanted = true;
            if !verified {
                return Ok((true, false));
            }
        }
    }
    Ok((has_wanted, true))
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

fn synthesize_magnet_export(
    info_hash: [u8; 20],
    publication_name: Option<&str>,
    trackers: &[StoredTracker],
) -> MagnetExportResult {
    let mut magnet = format!("magnet:?xt=urn:btih:{}", encode_info_hash(info_hash));
    if let Some(publication_name) = publication_name {
        let mut parameter = String::from("&dn=");
        percent_encode_query_value(&mut parameter, publication_name.as_bytes());
        if magnet.len() + parameter.len() <= MAX_MAGNET_LENGTH {
            magnet.push_str(&parameter);
        }
    }

    let mut included_trackers = 0_usize;
    let mut omitted_trackers = 0_usize;
    for tracker in trackers {
        if included_trackers == MAX_TRACKERS || TrackerUrl::from_magnet_url(&tracker.url).is_none()
        {
            omitted_trackers += 1;
            continue;
        }
        let mut parameter = String::from("&tr=");
        percent_encode_query_value(&mut parameter, tracker.url.as_bytes());
        if magnet.len() + parameter.len() > MAX_MAGNET_LENGTH {
            omitted_trackers += 1;
            continue;
        }
        magnet.push_str(&parameter);
        included_trackers += 1;
    }

    MagnetExportResult {
        magnet,
        source: MagnetExportSource::Synthesized,
        omitted_tracker_count: u32::try_from(omitted_trackers).unwrap_or(u32::MAX),
    }
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
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use rstorrent_engine::PreparedFileHash;
    use rstorrent_engine::dht::{DHT_SNAPSHOT_VERSION, DhtIdentity, DhtSnapshot};
    use rstorrent_protocol::dht::{DhtEndpoint, DhtIp, NodeContact, NodeId};
    use rstorrent_protocol::magnet::{MAX_MAGNET_LENGTH, MAX_TRACKERS, Magnet};
    use rstorrent_protocol::metainfo::{EXPLICIT_IMPORT_METAINFO_LIMITS, Metainfo, MetainfoFile};
    use rusqlite::Connection;
    use sha1::{Digest, Sha1};
    use sha2::Sha256;

    use super::{
        ConfiguredStorageRoot, ManagedArtifactState, PreparedFileRecord, SCHEMA_VERSION,
        SessionStore, StoreError, StoredTracker, StoredTrackerSource, StoredTrackerTransport,
        synthesize_magnet_export,
    };
    use crate::ClientSettings;
    use crate::durable_state::PayloadState;
    use crate::have::{HaveState, MAX_DURABLE_HAVE_STATE_BYTES, MAX_DURABLE_PIECES};
    use crate::{
        AddTorrentBytesRequest, CONTROL_VERSION, Command, ErrorCode, FileIndexRange, FilePriority,
        FileSelectionIntent, ListenerPolicy, MagnetExportSource, RemovalDataPolicy, RemovalState,
        RequestEnvelope, ResponseOutcome, StorageState, TorrentState, TorrentTransferLimits,
        TransferRateLimit,
    };

    static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

    fn install_legacy_torrent_state_columns(connection: &Connection) {
        connection
            .execute_batch(
                "ALTER TABLE torrents ADD COLUMN state TEXT NOT NULL DEFAULT 'paused';
                 ALTER TABLE torrents ADD COLUMN storage_state TEXT NOT NULL DEFAULT 'none';
                 ALTER TABLE torrents ADD COLUMN managed_artifacts TEXT NOT NULL DEFAULT 'none';
                 UPDATE torrents SET
                    state = CASE
                        WHEN quarantine_reason IS NOT NULL THEN 'needs_repair'
                        WHEN raw_info IS NULL THEN 'awaiting_metadata'
                        WHEN desired_state = 'paused' THEN 'paused'
                        WHEN payload_state = 'publication_pending' THEN 'awaiting_publication'
                        ELSE 'downloading'
                    END,
                    storage_state = CASE payload_state
                        WHEN 'work_owned' THEN 'staging'
                        WHEN 'publication_pending' THEN 'prepared'
                        WHEN 'legacy_owned' THEN 'published'
                        WHEN 'final_owned' THEN 'published'
                        ELSE 'none'
                    END,
                    managed_artifacts = CASE payload_state
                        WHEN 'work_owned' THEN 'staging'
                        WHEN 'publication_pending' THEN 'staging'
                        WHEN 'legacy_owned' THEN 'legacy'
                        WHEN 'final_owned' THEN 'published'
                        ELSE 'none'
                    END;",
            )
            .expect("install legacy torrent state columns");
    }

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
    fn magnet_export_preserves_verified_source_and_falls_back_after_corruption() {
        let root = test_root("magnet-export-source");
        let configured = configured_root(&root);
        let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
        let source = "magnet:?dn=Original%20Name&ws=https%3A%2F%2Fseed.example%2Ffile\
             &tr=udp%3A%2F%2Ftracker.example%3A6969%2Fannounce\
             &xt=urn:btih:000102030405060708090A0B0C0D0E0F10111213";
        let mut store = SessionStore::open(&root, "default", &[configured]).expect("open");
        let added = store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "add-export-source".to_owned(),
                expected_revision: None,
                command: Command::AddMagnet {
                    magnet: source.to_owned(),
                    storage_root: "downloads".to_owned(),
                    start_content: false,
                    skip_files: Vec::new(),
                },
            })
            .expect("add exact source");
        assert_eq!(added.revision, "1");

        let export = store
            .handle_durable(&export_request("export-verbatim", torrent_id))
            .expect("export exact source");
        assert_eq!(export.revision, "1");
        let result = match export.result.expect("export result") {
            crate::CommandResult::ExportMagnet { result } => result,
            crate::CommandResult::AddTorrent { .. } => panic!("unexpected add result"),
        };
        assert_eq!(result.magnet, source);
        assert_eq!(result.source, MagnetExportSource::Verbatim);
        assert_eq!(result.omitted_tracker_count, 0);

        store
            .connection
            .execute(
                "UPDATE torrent_source SET fidelity = 'canonicalized' WHERE info_hash = ?1",
                [crate::control::decode_info_hash(torrent_id)
                    .expect("hash")
                    .as_slice()],
            )
            .expect("mark source as migrated canonical text");
        let canonicalized = store
            .handle_durable(&export_request("export-canonicalized", torrent_id))
            .expect("export canonicalized source");
        let result = match canonicalized.result.expect("canonicalized result") {
            crate::CommandResult::ExportMagnet { result } => result,
            crate::CommandResult::AddTorrent { .. } => panic!("unexpected add result"),
        };
        assert_eq!(result.magnet, source);
        assert_eq!(result.source, MagnetExportSource::Canonicalized);

        store
            .connection
            .execute(
                "UPDATE torrent_source SET sha256 = zeroblob(32) WHERE info_hash = ?1",
                [crate::control::decode_info_hash(torrent_id)
                    .expect("hash")
                    .as_slice()],
            )
            .expect("corrupt retained source digest");
        let fallback = store
            .handle_durable(&export_request("export-fallback", torrent_id))
            .expect("fall back from corrupt source");
        assert_eq!(fallback.revision, "1");
        let result = match fallback.result.expect("fallback result") {
            crate::CommandResult::ExportMagnet { result } => result,
            crate::CommandResult::AddTorrent { .. } => panic!("unexpected add result"),
        };
        assert_eq!(result.source, MagnetExportSource::Synthesized);
        assert_eq!(
            result.magnet,
            "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213\
             &tr=udp%3A%2F%2Ftracker.example%3A6969%2Fannounce"
        );
        assert_eq!(result.omitted_tracker_count, 0);

        let missing = store
            .handle_durable(&export_request(
                "export-missing",
                "ffffffffffffffffffffffffffffffffffffffff",
            ))
            .expect("return typed missing result");
        assert!(matches!(
            missing.outcome,
            ResponseOutcome::Error {
                error: crate::ErrorResponse {
                    code: ErrorCode::UnknownTorrent,
                    ..
                }
            }
        ));
        drop(store);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn metainfo_export_synthesizes_verified_name_and_ordered_trackers() {
        let root = test_root("metainfo-magnet-export");
        let configured = configured_root(&root);
        let source = rich_torrent_source();
        let projection =
            Metainfo::project_bytes_with_limits(&source, EXPLICIT_IMPORT_METAINFO_LIMITS)
                .expect("project rich metainfo");
        let torrent_id = crate::control::encode_info_hash(projection.metainfo.info_hash);
        let mut store = SessionStore::open(&root, "default", &[configured]).expect("open");
        let added = store
            .handle_torrent_bytes(&torrent_bytes_request("add-rich-metainfo", &source), source)
            .expect("add rich metainfo");
        let export = store
            .handle_durable(&export_request("export-rich-metainfo", &torrent_id))
            .expect("export synthesized magnet");
        assert_eq!(export.revision, added.revision);
        let result = match export.result.expect("export result") {
            crate::CommandResult::ExportMagnet { result } => result,
            crate::CommandResult::AddTorrent { .. } => panic!("unexpected add result"),
        };
        assert_eq!(result.source, MagnetExportSource::Synthesized);
        assert_eq!(result.omitted_tracker_count, 0);
        assert_eq!(
            result.magnet,
            format!(
                "magnet:?xt=urn:btih:{torrent_id}\
                 &dn=Test%20%26%20Stuff\
                 &tr=udp%3A%2F%2Ftracker.example%3A6969%2Fannounce\
                 &tr=https%3A%2F%2Fbackup.example%2Fannounce%3Fkey%3Dabc"
            )
        );
        assert_eq!(
            Magnet::parse(&result.magnet)
                .expect("parse synthesized magnet")
                .trackers
                .len(),
            2
        );
        drop(store);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn synthesized_magnet_reports_count_and_length_omissions() {
        let tracker = |position: u32, url: String| StoredTracker {
            tier: 0,
            position,
            url,
            transport: StoredTrackerTransport::Udp,
            source: StoredTrackerSource::Metainfo,
        };
        let count_trackers = (0..MAX_TRACKERS + 2)
            .map(|index| {
                tracker(
                    index as u32,
                    format!("udp://tracker-{index}.example:80/announce"),
                )
            })
            .collect::<Vec<_>>();
        let count_bounded = synthesize_magnet_export([7; 20], None, &count_trackers);
        assert_eq!(count_bounded.omitted_tracker_count, 2);
        assert_eq!(
            Magnet::parse(&count_bounded.magnet)
                .expect("parse count-bounded magnet")
                .trackers
                .len(),
            MAX_TRACKERS
        );

        let large_token = "%".repeat(1_900);
        let mut length_trackers = (0..3)
            .map(|index| {
                tracker(
                    index,
                    format!("https://large-{index}.example/announce?token={large_token}"),
                )
            })
            .collect::<Vec<_>>();
        let short = "udp://short.example:80/announce";
        length_trackers.push(tracker(3, short.to_owned()));
        let length_bounded = synthesize_magnet_export([8; 20], Some("Bounded"), &length_trackers);
        assert!(length_bounded.magnet.len() <= MAX_MAGNET_LENGTH);
        assert_eq!(length_bounded.omitted_tracker_count, 1);
        assert!(
            length_bounded
                .magnet
                .ends_with("&tr=udp%3A%2F%2Fshort.example%3A80%2Fannounce")
        );
        assert_eq!(
            Magnet::parse(&length_bounded.magnet)
                .expect("parse length-bounded magnet")
                .trackers
                .len(),
            3
        );
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

    fn add_hash_request(request_id: &str, value: u8) -> RequestEnvelope {
        RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: request_id.to_owned(),
            expected_revision: None,
            command: Command::AddMagnet {
                magnet: format!(
                    "magnet:?xt=urn:btih:{}&x.pe=127.0.0.1:1",
                    format!("{value:02x}").repeat(20)
                ),
                storage_root: "downloads".to_owned(),
                start_content: true,
                skip_files: Vec::new(),
            },
        }
    }

    fn queued_ids(store: &SessionStore) -> Vec<String> {
        let mut torrents = store.snapshot().expect("queue snapshot").torrents;
        torrents.sort_unstable_by_key(|torrent| torrent.download_queue_position);
        torrents
            .into_iter()
            .filter(|torrent| torrent.download_queue_position.is_some())
            .map(|torrent| torrent.torrent_id)
            .collect()
    }

    #[test]
    fn download_queue_is_durable_replayable_and_keeps_pause_position() {
        let root = test_root("download-queue");
        let configured = configured_root(&root);
        let mut store =
            SessionStore::open(&root, "default", std::slice::from_ref(&configured)).expect("open");
        for value in 1..=3 {
            store
                .handle_durable(&add_hash_request(&format!("add-{value}"), value))
                .expect("add queued torrent");
        }
        let ids = (1..=3)
            .map(|value| format!("{value:02x}").repeat(20))
            .collect::<Vec<_>>();
        assert_eq!(queued_ids(&store), ids);

        for (request_id, command) in [
            (
                "pause-middle",
                Command::Pause {
                    torrent_id: ids[1].clone(),
                },
            ),
            (
                "resume-middle",
                Command::Resume {
                    torrent_id: ids[1].clone(),
                },
            ),
        ] {
            store
                .handle_durable(&RequestEnvelope {
                    version: CONTROL_VERSION,
                    request_id: request_id.to_owned(),
                    expected_revision: None,
                    command,
                })
                .expect("change durable intent");
        }
        assert_eq!(queued_ids(&store), ids);

        let move_top = RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: "move-third-top".to_owned(),
            expected_revision: None,
            command: Command::MoveDownloadToTop {
                torrent_id: ids[2].clone(),
            },
        };
        let accepted = store.handle_durable(&move_top).expect("move to top");
        assert_eq!(
            queued_ids(&store),
            vec![ids[2].clone(), ids[0].clone(), ids[1].clone()]
        );
        assert_eq!(
            store.handle_durable(&move_top).expect("exact replay"),
            accepted
        );

        let conflict = store
            .handle_durable(&RequestEnvelope {
                command: Command::MoveDownloadToBottom {
                    torrent_id: ids[2].clone(),
                },
                ..move_top.clone()
            })
            .expect("request conflict response");
        assert!(matches!(
            conflict.outcome,
            ResponseOutcome::Error {
                error: crate::ErrorResponse {
                    code: ErrorCode::RequestConflict,
                    ..
                }
            }
        ));
        let stale = store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "stale-queue-move".to_owned(),
                expected_revision: Some("0".to_owned()),
                command: Command::MoveDownloadToBottom {
                    torrent_id: ids[2].clone(),
                },
            })
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

        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "move-third-bottom".to_owned(),
                expected_revision: None,
                command: Command::MoveDownloadToBottom {
                    torrent_id: ids[2].clone(),
                },
            })
            .expect("move to bottom");
        assert_eq!(queued_ids(&store), ids);
        drop(store);

        let reopened = SessionStore::open(&root, "default", &[configured]).expect("reopen");
        assert_eq!(queued_ids(&reopened), ids);
        drop(reopened);
        fs::remove_dir_all(root).expect("remove test profile");
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

    fn rich_torrent_source() -> Vec<u8> {
        let primary = "udp://tracker.example:6969/announce";
        let backup = "https://backup.example/announce?key=abc";
        let name = "Test & Stuff";
        let info = format!(
            "d6:lengthi4e4:name{}:{name}12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae",
            name.len()
        );
        format!(
            "d13:announce-listll{}:{primary}el{}:{backup}ee4:info{info}e",
            primary.len(),
            backup.len()
        )
        .into_bytes()
    }

    fn export_request(request_id: &str, torrent_id: &str) -> RequestEnvelope {
        RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: request_id.to_owned(),
            expected_revision: None,
            command: Command::ExportMagnet {
                torrent_id: torrent_id.to_owned(),
            },
        }
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
        assert_eq!(resume.state, TorrentState::Paused);
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
        assert_eq!(pending.state, TorrentState::Paused);
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
    fn all_skipped_idles_running_intent_and_normal_resumes_without_recheck() {
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
        assert!(!idle.verification.is_pending());
        let verification_generation = idle.verification.requested();

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
        let resumed = store.load_resume(&torrent_id).expect("load resumed state");
        assert!(resumed.desired_running);
        assert_eq!(resumed.state, TorrentState::Downloading);
        assert_eq!(resumed.skip_files, vec![0]);
        assert_eq!(resumed.verification.requested(), verification_generation);
        assert!(!resumed.verification.is_pending());
        drop(store);
        fs::remove_dir_all(root).expect("remove profile");
    }

    #[test]
    fn download_files_commits_one_replay_safe_wanted_and_running_revision() {
        let root = test_root("download-files-atomic");
        let configured = configured_root(&root);
        let mut store = SessionStore::open(&root, "default", std::slice::from_ref(&configured))
            .expect("open store");
        let raw_info = multi_file_info();
        let torrent_id = crate::control::encode_info_hash(Sha1::digest(&raw_info).into());
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "download-files-add".to_owned(),
                expected_revision: None,
                command: Command::AddMagnet {
                    magnet: format!("magnet:?xt=urn:btih:{torrent_id}"),
                    storage_root: "downloads".to_owned(),
                    start_content: false,
                    skip_files: vec![0, 1],
                },
            })
            .expect("add paused all-skipped torrent");
        store
            .record_metadata(&torrent_id, &raw_info)
            .expect("record metadata");
        let before = store.load_resume(&torrent_id).expect("load paused intent");
        let revision_before = store.revision().expect("load revision");
        let verification_before = before.verification;
        assert!(!before.desired_running);
        assert_eq!(before.skip_files, [0, 1]);

        let request = RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: "download-files".to_owned(),
            expected_revision: Some(revision_before.to_string()),
            command: Command::DownloadFiles {
                torrent_id: torrent_id.clone(),
                file_indices: vec![1],
            },
        };
        let first = store
            .handle_durable(&request)
            .expect("download skipped file");
        assert!(matches!(first.outcome, ResponseOutcome::Success { .. }));
        assert_eq!(
            first.revision.parse::<u64>().expect("response revision"),
            revision_before + 1
        );
        let running = store.load_resume(&torrent_id).expect("load running intent");
        assert!(running.desired_running);
        assert_eq!(running.skip_files, [0]);
        assert_eq!(running.verification, verification_before);

        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "pause-after-download".to_owned(),
                expected_revision: None,
                command: Command::Pause {
                    torrent_id: torrent_id.clone(),
                },
            })
            .expect("pause after download request");
        let revision_after_pause = store.revision().expect("paused revision");
        assert_eq!(store.handle_durable(&request).expect("exact replay"), first);
        let replayed = store.load_resume(&torrent_id).expect("load after replay");
        assert!(!replayed.desired_running);
        assert_eq!(replayed.skip_files, [0]);
        assert_eq!(
            store.revision().expect("replay revision"),
            revision_after_pause
        );

        let stale = store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "download-files-stale".to_owned(),
                expected_revision: Some(revision_before.to_string()),
                command: Command::DownloadFiles {
                    torrent_id: torrent_id.clone(),
                    file_indices: vec![0],
                },
            })
            .expect("reject stale download");
        assert!(matches!(
            stale.outcome,
            ResponseOutcome::Error {
                error: crate::ErrorResponse {
                    code: ErrorCode::StaleRevision,
                    ..
                }
            }
        ));
        let after_stale = store.load_resume(&torrent_id).expect("load after stale");
        assert!(!after_stale.desired_running);
        assert_eq!(after_stale.skip_files, [0]);

        let wanted_paused_revision = store.revision().expect("wanted paused revision");
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "download-wanted-paused".to_owned(),
                expected_revision: None,
                command: Command::DownloadFiles {
                    torrent_id: torrent_id.clone(),
                    file_indices: vec![1],
                },
            })
            .expect("start already wanted file");
        assert_eq!(
            store.revision().expect("wanted running revision"),
            wanted_paused_revision + 1
        );
        let no_op_revision = store.revision().expect("no-op baseline");
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "download-wanted-running".to_owned(),
                expected_revision: None,
                command: Command::DownloadFiles {
                    torrent_id: torrent_id.clone(),
                    file_indices: vec![1],
                },
            })
            .expect("accept wanted running no-op");
        assert_eq!(store.revision().expect("unchanged no-op"), no_op_revision);

        for (request_id, command) in [
            (
                "pause-before-archive",
                Command::Pause {
                    torrent_id: torrent_id.clone(),
                },
            ),
            (
                "archive-before-download",
                Command::Archive {
                    torrent_id: torrent_id.clone(),
                },
            ),
        ] {
            store
                .handle_durable(&RequestEnvelope {
                    version: CONTROL_VERSION,
                    request_id: request_id.to_owned(),
                    expected_revision: None,
                    command,
                })
                .expect("prepare archived torrent");
        }
        let archived_revision = store.revision().expect("archived revision");
        let archived = store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "download-archived".to_owned(),
                expected_revision: None,
                command: Command::DownloadFiles {
                    torrent_id: torrent_id.clone(),
                    file_indices: vec![0],
                },
            })
            .expect("reject archived download");
        assert!(matches!(
            archived.outcome,
            ResponseOutcome::Error {
                error: crate::ErrorResponse {
                    code: ErrorCode::InvalidTorrentState,
                    ..
                }
            }
        ));
        let after_archived = store
            .load_resume(&torrent_id)
            .expect("load rejected archived intent");
        assert!(!after_archived.desired_running);
        assert_eq!(after_archived.skip_files, [0]);
        assert_eq!(after_archived.verification, verification_before);
        assert_eq!(
            store.revision().expect("unchanged archived"),
            archived_revision
        );

        drop(store);
        let reopened = SessionStore::open(&root, "default", &[configured]).expect("reopen store");
        let reopened_resume = reopened
            .load_resume(&torrent_id)
            .expect("load reopened intent");
        assert!(!reopened_resume.desired_running);
        assert_eq!(reopened_resume.skip_files, [0]);
        assert_eq!(
            reopened.revision().expect("reopened revision"),
            archived_revision
        );
        drop(reopened);
        fs::remove_dir_all(root).expect("remove test profile");
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
            legacy_node_id: None,
            identities_v4: vec![DhtIdentity {
                address: IpAddr::V4(Ipv4Addr::LOCALHOST),
                node_id: NodeId([1; 20]),
            }],
            identities_v6: vec![DhtIdentity {
                address: IpAddr::V6(Ipv6Addr::LOCALHOST),
                node_id: NodeId([6; 20]),
            }],
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
        install_legacy_torrent_state_columns(&connection);
        connection
            .execute_batch(
                "DROP TABLE prepared_files;
                 DROP TABLE dht_identities;
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
        install_legacy_torrent_state_columns(&connection);
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
        install_legacy_torrent_state_columns(&connection);
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
        install_legacy_torrent_state_columns(&connection);
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
                    info_hash, magnet, storage_root, desired_state, payload_state,
                    piece_count, have_state, archived,
                    created_revision, updated_revision
                 ) VALUES (
                    ?1, 'magnet:', 'downloads', 'paused', 'absent',
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
        assert_eq!(
            store
                .invalidate_pieces(&torrent_id, &[1, 1])
                .expect("invalidate uncertain piece"),
            5
        );
        assert_eq!(
            store
                .load_resume(&torrent_id)
                .expect("load invalidated batch")
                .have
                .expect("have state")
                .pieces(),
            &[true, false, true]
        );
        assert!(matches!(
            store.invalidate_pieces(&torrent_id, &[3]),
            Err(StoreError::Have(_))
        ));
        assert!(matches!(
            store.invalidate_pieces(&torrent_id, &[]),
            Err(StoreError::DurableState(_))
        ));
        assert_eq!(store.revision().expect("revision after invalidation"), 5);
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
        let first_generation = checking.verification.requested();
        assert!(checking.verification.is_pending());
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
        store
            .handle_durable(&RequestEnvelope {
                request_id: "equivalent-pending-recheck".to_owned(),
                ..request.clone()
            })
            .expect("deduplicate equivalent pending request");
        assert_eq!(store.revision().expect("pending revision"), revision);
        assert_eq!(
            store
                .load_resume(&torrent_id)
                .expect("pending generation")
                .verification
                .requested(),
            first_generation
        );

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
        let (_, second_generation) = store
            .begin_recheck_with_generation(&torrent_id)
            .expect("begin paused recheck");
        assert!(second_generation > first_generation);
        assert!(matches!(
            store.complete_recheck_generation(&torrent_id, first_generation, &replacement),
            Err(StoreError::DurableState(_))
        ));
        assert!(
            store
                .load_resume(&torrent_id)
                .expect("new generation remains pending")
                .verification
                .is_pending()
        );
        store
            .complete_recheck_generation(&torrent_id, second_generation, &replacement)
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
        store
            .record_piece(&torrent_id, 0)
            .expect("record verified payload");

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
            .record_piece(&torrent_id, 0)
            .expect("record verified payload");
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
        install_legacy_torrent_state_columns(&connection);
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
    fn migrates_defective_schema_thirteen_final_side_without_payload_mutation() {
        let root = test_root("schema-v13-defective-final");
        let configured = configured_root(&root);
        let raw_info =
            b"d6:lengthi4e4:name4:test12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = crate::control::encode_info_hash(info_hash);
        let mut store = SessionStore::open(&root, "default", std::slice::from_ref(&configured))
            .expect("open current store");
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "add-defective-v13".to_owned(),
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
        let database = store.database_path().expect("database path").to_owned();
        drop(store);

        let payload = root.join("payload/test");
        fs::create_dir_all(payload.parent().expect("payload parent")).expect("create payload root");
        let original = b"kept";
        fs::write(&payload, original).expect("write final payload");
        let connection = Connection::open(&database).expect("open schema fixture");
        install_legacy_torrent_state_columns(&connection);
        connection
            .execute(
                "UPDATE torrents
                 SET state = 'downloading', storage_state = 'staging',
                     managed_artifacts = 'published'
                 WHERE info_hash = ?1",
                [info_hash.as_slice()],
            )
            .expect("install exact defective pair");
        connection
            .pragma_update(None, "user_version", 13)
            .expect("mark schema thirteen");
        drop(connection);

        let migrated = SessionStore::open(&root, "default", &[configured]).expect("migrate");
        let resume = migrated
            .load_resume(&torrent_id)
            .expect("load migrated row");
        assert_eq!(resume.payload_state, PayloadState::FinalOwned);
        assert!(resume.verification.is_pending());
        assert_eq!(resume.quarantine_reason, None);
        assert_eq!(resume.state, TorrentState::Checking);
        assert_eq!(
            fs::read(&payload).expect("read preserved payload"),
            original
        );
        let columns = {
            let mut statement = migrated
                .connection
                .prepare("PRAGMA table_info(torrents)")
                .expect("prepare column query");
            statement
                .query_map([], |row| row.get::<_, String>(1))
                .expect("query columns")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect columns")
        };
        assert!(!columns.iter().any(|column| {
            matches!(
                column.as_str(),
                "state" | "storage_state" | "managed_artifacts"
            )
        }));
        drop(migrated);
        fs::remove_dir_all(root).expect("remove profile");
    }

    #[test]
    fn migrates_version_sixteen_to_default_setting_and_stable_queue_order() {
        let root = test_root("schema-v16-download-queue");
        let configured = configured_root(&root);
        let mut store =
            SessionStore::open(&root, "default", std::slice::from_ref(&configured)).expect("open");
        store
            .handle_durable(&add_hash_request("add-second-hash-first", 2))
            .expect("add first queue row");
        store
            .handle_durable(&add_hash_request("add-first-hash-second", 1))
            .expect("add second queue row");
        let database_path = store.database_path().expect("database path").to_owned();
        drop(store);

        let connection = Connection::open(&database_path).expect("open raw database");
        connection
            .execute_batch(
                "DROP INDEX download_queue_order;
                 ALTER TABLE torrents DROP COLUMN download_queue_position;
                 ALTER TABLE client_settings DROP COLUMN active_downloads;
                 PRAGMA user_version = 16;",
            )
            .expect("downgrade fixture to version sixteen");
        drop(connection);

        let migrated = SessionStore::open(&root, "default", &[configured]).expect("migrate");
        assert_eq!(
            migrated
                .snapshot()
                .expect("snapshot")
                .client_settings
                .active_downloads,
            3
        );
        assert_eq!(
            queued_ids(&migrated),
            vec![
                format!("{:02x}", 2).repeat(20),
                format!("{:02x}", 1).repeat(20)
            ]
        );
        let connection = Connection::open(database_path).expect("inspect migrated database");
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("schema version");
        assert_eq!(version, SCHEMA_VERSION);
        let index_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'index' AND name = 'download_queue_order'",
                [],
                |row| row.get(0),
            )
            .expect("queue index");
        assert_eq!(index_count, 1);
        drop(connection);
        drop(migrated);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn migrates_version_seventeen_to_unlimited_session_and_torrent_rates() {
        let root = test_root("schema-v17-transfer-rates");
        let configured = configured_root(&root);
        let mut store =
            SessionStore::open(&root, "default", std::slice::from_ref(&configured)).expect("open");
        store
            .handle_durable(&add_hash_request("add-rate-migration", 7))
            .expect("add torrent");
        let database_path = store.database_path().expect("database path").to_owned();
        drop(store);

        let connection = Connection::open(&database_path).expect("open raw database");
        connection
            .execute_batch(
                "ALTER TABLE torrents DROP COLUMN upload_rate_limit;
                 ALTER TABLE torrents DROP COLUMN download_rate_limit;
                 ALTER TABLE client_settings DROP COLUMN upload_rate_limit;
                 ALTER TABLE client_settings DROP COLUMN download_rate_limit;
                 PRAGMA user_version = 17;",
            )
            .expect("downgrade fixture to version seventeen");
        drop(connection);

        let migrated = SessionStore::open(&root, "default", &[configured]).expect("migrate");
        let snapshot = migrated.snapshot().expect("migrated snapshot");
        assert_eq!(
            snapshot.client_settings.upload_rate_limit,
            TransferRateLimit::Unlimited
        );
        assert_eq!(
            snapshot.client_settings.download_rate_limit,
            TransferRateLimit::Unlimited
        );
        assert_eq!(
            snapshot.torrents[0].transfer_limits,
            TorrentTransferLimits::default()
        );
        let version: i64 = migrated
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("schema version");
        assert_eq!(version, SCHEMA_VERSION);
        drop(migrated);
        fs::remove_dir_all(root).expect("remove test profile");
    }

    #[test]
    fn torrent_transfer_limits_are_atomic_replayable_and_durable() {
        let root = test_root("torrent-transfer-limits");
        let configured = configured_root(&root);
        let mut store =
            SessionStore::open(&root, "default", std::slice::from_ref(&configured)).expect("open");
        store
            .handle_durable(&add_hash_request("add-rate-target", 8))
            .expect("add torrent");
        let torrent_id = format!("{:02x}", 8).repeat(20);
        let limits = TorrentTransferLimits {
            upload: TransferRateLimit::Limited {
                bytes_per_second: 24 * 1_024,
            },
            download: TransferRateLimit::Limited {
                bytes_per_second: u32::MAX,
            },
        };
        let request = RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: "set-torrent-transfer-limits".to_owned(),
            expected_revision: Some("1".to_owned()),
            command: Command::SetTorrentTransferLimits {
                torrent_id: torrent_id.clone(),
                limits,
            },
        };
        let accepted = store.handle_durable(&request).expect("set torrent limits");
        assert_eq!(accepted.revision, "2");
        let ResponseOutcome::Success { snapshot } = &accepted.outcome else {
            panic!("torrent limit mutation must succeed");
        };
        assert_eq!(snapshot.torrents[0].transfer_limits, limits);
        assert_eq!(store.handle_durable(&request).expect("replay"), accepted);

        let no_op = store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "torrent-transfer-limits-no-op".to_owned(),
                expected_revision: Some("2".to_owned()),
                command: Command::SetTorrentTransferLimits {
                    torrent_id: torrent_id.clone(),
                    limits,
                },
            })
            .expect("no-op torrent limits");
        assert_eq!(no_op.revision, "2");
        let stale = store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "torrent-transfer-limits-stale".to_owned(),
                expected_revision: Some("1".to_owned()),
                command: Command::SetTorrentTransferLimits {
                    torrent_id,
                    limits: TorrentTransferLimits::default(),
                },
            })
            .expect("stale torrent limits");
        assert!(matches!(
            stale.outcome,
            ResponseOutcome::Error {
                error: crate::ErrorResponse {
                    code: ErrorCode::StaleRevision,
                    ..
                }
            }
        ));
        let database_path = store.database_path().expect("database path").to_owned();
        drop(store);
        let reopened = SessionStore::open(&root, "default", &[configured]).expect("reopen");
        assert_eq!(
            reopened.snapshot().expect("restart snapshot").torrents[0].transfer_limits,
            limits
        );
        drop(reopened);
        assert!(database_path.exists());
        fs::remove_dir_all(root).expect("remove test profile");
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
            active_downloads: 3,
            upload_rate_limit: crate::TransferRateLimit::Limited {
                bytes_per_second: 64 * 1_024,
            },
            download_rate_limit: crate::TransferRateLimit::Limited {
                bytes_per_second: u32::MAX,
            },
            encryption: Default::default(),
            ipv6_enabled: true,
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
