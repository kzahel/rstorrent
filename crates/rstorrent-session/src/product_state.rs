use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const PRODUCT_DATABASE_FILENAME: &str = "product.db";
const PRODUCT_SCHEMA_VERSION: i64 = 1;
const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const MAX_PRODUCT_VERSION_BYTES: usize = 128;
const MAX_U64_TEXT_BYTES: usize = 20;
pub const CURRENT_PRODUCT_DISCLOSURE_VERSION: u32 = 1;
pub const MAX_PRODUCT_SOURCES: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductMilestoneKind {
    TorrentAdded,
    DownloadCompleted,
}

impl ProductMilestoneKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TorrentAdded => "torrent_added",
            Self::DownloadCompleted => "download_completed",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ProductStateError> {
        match value {
            "torrent_added" => Ok(Self::TorrentAdded),
            "download_completed" => Ok(Self::DownloadCompleted),
            _ => Err(ProductStateError::InvalidState(
                "product milestone has an unknown kind".to_owned(),
            )),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProductMilestone {
    pub source_epoch: [u8; 16],
    pub sequence: u64,
    pub kind: ProductMilestoneKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductSummary {
    pub installation_id: String,
    pub created_at_millis: String,
    pub first_version: String,
    pub current_version: String,
    pub disclosure_version: u32,
    pub statistics_enabled: bool,
    pub torrents_added: String,
    pub downloads_completed: String,
    pub foreground_sessions: String,
    pub reset_generation: String,
    pub last_start_millis: String,
    pub last_clean_shutdown_millis: Option<String>,
    pub days_since_first_use: u64,
    pub transmission_allowed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProductStatePageUsage {
    pub page_size: u64,
    pub page_count: u64,
}

#[derive(Debug)]
pub enum ProductStateError {
    Io {
        operation: &'static str,
        source: std::io::Error,
    },
    Sqlite(rusqlite::Error),
    InvalidConfiguration(String),
    InvalidState(String),
    UnsupportedSchema {
        actual: i64,
        maximum: i64,
    },
    RequiredPragma(&'static str),
    SourceLimit,
    SequenceGap {
        expected: u64,
        actual: u64,
    },
}

impl fmt::Display for ProductStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::Sqlite(error) => write!(formatter, "product state SQLite: {error}"),
            Self::InvalidConfiguration(message) => {
                write!(formatter, "product state configuration: {message}")
            }
            Self::InvalidState(message) => write!(formatter, "invalid product state: {message}"),
            Self::UnsupportedSchema { actual, maximum } => write!(
                formatter,
                "unsupported product-state schema {actual}; maximum is {maximum}"
            ),
            Self::RequiredPragma(pragma) => {
                write!(formatter, "product state could not enable {pragma}")
            }
            Self::SourceLimit => write!(
                formatter,
                "product state exceeds the {MAX_PRODUCT_SOURCES}-source watermark limit"
            ),
            Self::SequenceGap { expected, actual } => write!(
                formatter,
                "product milestone sequence gap: expected {expected}, received {actual}"
            ),
        }
    }
}

impl std::error::Error for ProductStateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Sqlite(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for ProductStateError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}

pub struct ProductStateStore {
    connection: Connection,
    database_path: Option<PathBuf>,
}

impl fmt::Debug for ProductStateStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProductStateStore")
            .field("database_path", &self.database_path)
            .finish_non_exhaustive()
    }
}

impl ProductStateStore {
    pub fn open(
        application_data_root: &Path,
        current_version: &str,
        legacy_installation_id: Option<&str>,
    ) -> Result<Self, ProductStateError> {
        Self::open_at(
            application_data_root,
            current_version,
            legacy_installation_id,
            now_millis()?,
        )
    }

    pub fn open_ephemeral(current_version: &str) -> Result<Self, ProductStateError> {
        Self::open_ephemeral_at(current_version, now_millis()?)
    }

    fn open_at(
        application_data_root: &Path,
        current_version: &str,
        legacy_installation_id: Option<&str>,
        now_millis: u64,
    ) -> Result<Self, ProductStateError> {
        validate_version(current_version)?;
        std::fs::create_dir_all(application_data_root).map_err(|source| ProductStateError::Io {
            operation: "create product-state directory",
            source,
        })?;
        let database_path = application_data_root.join(PRODUCT_DATABASE_FILENAME);
        let connection = Connection::open(&database_path)?;
        configure_durable_connection(&connection)?;
        initialize(
            connection,
            current_version,
            legacy_installation_id,
            now_millis,
            Some(database_path),
        )
    }

    fn open_ephemeral_at(
        current_version: &str,
        now_millis: u64,
    ) -> Result<Self, ProductStateError> {
        validate_version(current_version)?;
        let connection = Connection::open_in_memory()?;
        connection.pragma_update(None, "foreign_keys", true)?;
        initialize(connection, current_version, None, now_millis, None)
    }

    pub fn summary(&self) -> Result<ProductSummary, ProductStateError> {
        self.summary_at(now_millis()?)
    }

    fn summary_at(&self, now_millis: u64) -> Result<ProductSummary, ProductStateError> {
        read_summary(&self.connection, now_millis)
    }

    pub fn acknowledge_disclosure(
        &mut self,
        statistics_enabled: bool,
    ) -> Result<ProductSummary, ProductStateError> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE product_state
             SET disclosure_version = ?1, statistics_enabled = ?2
             WHERE singleton = 1",
            params![
                i64::from(CURRENT_PRODUCT_DISCLOSURE_VERSION),
                statistics_enabled
            ],
        )?;
        transaction.commit()?;
        self.summary()
    }

    pub fn set_statistics_enabled(
        &mut self,
        statistics_enabled: bool,
    ) -> Result<ProductSummary, ProductStateError> {
        let transaction = self.connection.transaction()?;
        let disclosure_version: i64 = transaction.query_row(
            "SELECT disclosure_version FROM product_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )?;
        if disclosure_version != i64::from(CURRENT_PRODUCT_DISCLOSURE_VERSION) {
            return Err(ProductStateError::InvalidState(
                "statistics preference cannot change before current disclosure acknowledgement"
                    .to_owned(),
            ));
        }
        transaction.execute(
            "UPDATE product_state SET statistics_enabled = ?1 WHERE singleton = 1",
            [statistics_enabled],
        )?;
        transaction.commit()?;
        self.summary()
    }

    pub fn reset_statistics(&mut self) -> Result<ProductSummary, ProductStateError> {
        self.reset_statistics_at(now_millis()?)
    }

    fn reset_statistics_at(
        &mut self,
        now_millis: u64,
    ) -> Result<ProductSummary, ProductStateError> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE product_state
             SET installation_id = ?1,
                 created_at_millis = ?2,
                 torrents_added = '0',
                 downloads_completed = '0',
                 foreground_sessions = '0',
                 reset_generation = ?3
             WHERE singleton = 1",
            params![
                Uuid::new_v4().to_string(),
                encode_u64(now_millis),
                Uuid::new_v4().to_string(),
            ],
        )?;
        transaction.commit()?;
        self.summary_at(now_millis)
    }

    pub fn record_foreground_session(&mut self) -> Result<ProductSummary, ProductStateError> {
        let transaction = self.connection.transaction()?;
        increment_counter(&transaction, "foreground_sessions")?;
        transaction.commit()?;
        self.summary()
    }

    pub fn record_clean_shutdown(&mut self) -> Result<(), ProductStateError> {
        let now = now_millis()?;
        self.connection.execute(
            "UPDATE product_state SET last_clean_shutdown_millis = ?1 WHERE singleton = 1",
            [encode_u64(now)],
        )?;
        Ok(())
    }

    pub fn apply_milestones(
        &mut self,
        milestones: &[ProductMilestone],
    ) -> Result<ProductSummary, ProductStateError> {
        if milestones.is_empty() {
            return self.summary();
        }
        let source_epoch = milestones[0].source_epoch;
        if source_epoch == [0; 16] {
            return Err(ProductStateError::InvalidState(
                "product milestone source epoch is zero".to_owned(),
            ));
        }
        if milestones
            .iter()
            .any(|milestone| milestone.source_epoch != source_epoch)
        {
            return Err(ProductStateError::InvalidState(
                "product milestone batch mixes source epochs".to_owned(),
            ));
        }
        let transaction = self.connection.transaction()?;
        let mut last_sequence = source_watermark(&transaction, &source_epoch)?;
        if last_sequence.is_none() {
            let source_count: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM product_source_watermarks",
                [],
                |row| row.get(0),
            )?;
            if source_count
                >= i64::try_from(MAX_PRODUCT_SOURCES).expect("source limit fits SQLite i64")
            {
                return Err(ProductStateError::SourceLimit);
            }
        }
        for milestone in milestones {
            if milestone.sequence == 0 {
                return Err(ProductStateError::InvalidState(
                    "product milestone sequence is zero".to_owned(),
                ));
            }
            if last_sequence.is_some_and(|last| milestone.sequence <= last) {
                continue;
            }
            let expected = last_sequence.map_or(1, |last| last.saturating_add(1));
            if milestone.sequence != expected {
                return Err(ProductStateError::SequenceGap {
                    expected,
                    actual: milestone.sequence,
                });
            }
            match milestone.kind {
                ProductMilestoneKind::TorrentAdded => {
                    increment_counter(&transaction, "torrents_added")?
                }
                ProductMilestoneKind::DownloadCompleted => {
                    increment_counter(&transaction, "downloads_completed")?
                }
            }
            last_sequence = Some(milestone.sequence);
        }
        if let Some(last_sequence) = last_sequence {
            transaction.execute(
                "INSERT INTO product_source_watermarks(source_epoch, last_sequence)
                 VALUES (?1, ?2)
                 ON CONFLICT(source_epoch) DO UPDATE SET
                    last_sequence = excluded.last_sequence",
                params![source_epoch.as_slice(), encode_u64(last_sequence)],
            )?;
        }
        transaction.commit()?;
        self.summary()
    }

    pub fn page_usage(&self) -> Result<ProductStatePageUsage, ProductStateError> {
        Ok(ProductStatePageUsage {
            page_size: pragma_u64(&self.connection, "page_size")?,
            page_count: pragma_u64(&self.connection, "page_count")?,
        })
    }

    pub fn database_path(&self) -> Option<&Path> {
        self.database_path.as_deref()
    }
}

fn initialize(
    mut connection: Connection,
    current_version: &str,
    legacy_installation_id: Option<&str>,
    now_millis: u64,
    database_path: Option<PathBuf>,
) -> Result<ProductStateStore, ProductStateError> {
    connection.pragma_update(None, "foreign_keys", true)?;
    let foreign_keys: i64 =
        connection.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
    if foreign_keys != 1 {
        return Err(ProductStateError::RequiredPragma("foreign_keys"));
    }
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    match version {
        0 => create_schema(
            &mut connection,
            current_version,
            legacy_installation_id,
            now_millis,
        )?,
        PRODUCT_SCHEMA_VERSION => validate_schema(&connection)?,
        actual => {
            return Err(ProductStateError::UnsupportedSchema {
                actual,
                maximum: PRODUCT_SCHEMA_VERSION,
            });
        }
    }
    connection.execute(
        "UPDATE product_state
         SET current_version = ?1, last_start_millis = ?2
         WHERE singleton = 1",
        params![current_version, encode_u64(now_millis)],
    )?;
    validate_schema(&connection)?;
    #[cfg(unix)]
    if let Some(path) = &database_path {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)
            .map_err(|source| ProductStateError::Io {
                operation: "read product-state permissions",
                source,
            })?
            .permissions();
        permissions.set_mode(0o600);
        std::fs::set_permissions(path, permissions).map_err(|source| ProductStateError::Io {
            operation: "set product-state permissions",
            source,
        })?;
    }
    Ok(ProductStateStore {
        connection,
        database_path,
    })
}

fn create_schema(
    connection: &mut Connection,
    current_version: &str,
    legacy_installation_id: Option<&str>,
    now_millis: u64,
) -> Result<(), ProductStateError> {
    let installation_id = legacy_installation_id
        .and_then(|value| Uuid::parse_str(value.trim()).ok())
        .map_or_else(|| Uuid::new_v4().to_string(), |value| value.to_string());
    let transaction = connection.transaction()?;
    transaction.execute_batch(
        "CREATE TABLE product_state (
            singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
            installation_id TEXT NOT NULL UNIQUE CHECK (
                length(installation_id) = 36
            ),
            created_at_millis TEXT NOT NULL CHECK (
                length(created_at_millis) BETWEEN 1 AND 20
            ),
            first_version TEXT NOT NULL CHECK (
                length(first_version) BETWEEN 1 AND 128
            ),
            current_version TEXT NOT NULL CHECK (
                length(current_version) BETWEEN 1 AND 128
            ),
            disclosure_version INTEGER NOT NULL DEFAULT 0 CHECK (
                disclosure_version BETWEEN 0 AND 1
            ),
            statistics_enabled INTEGER NOT NULL DEFAULT 1 CHECK (
                statistics_enabled IN (0, 1)
            ),
            torrents_added TEXT NOT NULL DEFAULT '0' CHECK (
                length(torrents_added) BETWEEN 1 AND 20
            ),
            downloads_completed TEXT NOT NULL DEFAULT '0' CHECK (
                length(downloads_completed) BETWEEN 1 AND 20
            ),
            foreground_sessions TEXT NOT NULL DEFAULT '0' CHECK (
                length(foreground_sessions) BETWEEN 1 AND 20
            ),
            reset_generation TEXT NOT NULL CHECK (
                length(reset_generation) = 36
            ),
            last_start_millis TEXT NOT NULL CHECK (
                length(last_start_millis) BETWEEN 1 AND 20
            ),
            last_clean_shutdown_millis TEXT CHECK (
                last_clean_shutdown_millis IS NULL OR
                length(last_clean_shutdown_millis) BETWEEN 1 AND 20
            )
         );
         CREATE TABLE product_source_watermarks (
            source_epoch BLOB PRIMARY KEY CHECK (
                length(source_epoch) = 16 AND source_epoch <> zeroblob(16)
            ),
            last_sequence TEXT NOT NULL CHECK (
                length(last_sequence) BETWEEN 1 AND 20
            )
         ) WITHOUT ROWID;",
    )?;
    let now = encode_u64(now_millis);
    transaction.execute(
        "INSERT INTO product_state(
            singleton, installation_id, created_at_millis, first_version,
            current_version, reset_generation, last_start_millis
         ) VALUES (1, ?1, ?2, ?3, ?3, ?4, ?2)",
        params![
            installation_id,
            now,
            current_version,
            Uuid::new_v4().to_string(),
        ],
    )?;
    transaction.pragma_update(None, "user_version", PRODUCT_SCHEMA_VERSION)?;
    transaction.commit()?;
    Ok(())
}

fn validate_schema(connection: &Connection) -> Result<(), ProductStateError> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != PRODUCT_SCHEMA_VERSION {
        return Err(ProductStateError::UnsupportedSchema {
            actual: version,
            maximum: PRODUCT_SCHEMA_VERSION,
        });
    }
    let row = connection
        .query_row(
            "SELECT installation_id, created_at_millis, first_version,
                    current_version, disclosure_version, statistics_enabled,
                    torrents_added, downloads_completed, foreground_sessions,
                    reset_generation, last_start_millis,
                    last_clean_shutdown_millis
             FROM product_state WHERE singleton = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, bool>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, Option<String>>(11)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| {
            ProductStateError::InvalidState("product singleton is missing".to_owned())
        })?;
    Uuid::parse_str(&row.0)
        .map_err(|_| ProductStateError::InvalidState("installation ID is not a UUID".to_owned()))?;
    decode_u64(&row.1, "creation time")?;
    validate_version(&row.2).map_err(invalid_configuration_as_state)?;
    validate_version(&row.3).map_err(invalid_configuration_as_state)?;
    if !(0..=i64::from(CURRENT_PRODUCT_DISCLOSURE_VERSION)).contains(&row.4) {
        return Err(ProductStateError::InvalidState(
            "product disclosure version is unsupported".to_owned(),
        ));
    }
    let _statistics_enabled = row.5;
    decode_u64(&row.6, "torrents-added counter")?;
    decode_u64(&row.7, "downloads-completed counter")?;
    decode_u64(&row.8, "foreground-sessions counter")?;
    Uuid::parse_str(&row.9).map_err(|_| {
        ProductStateError::InvalidState("reset generation is not a UUID".to_owned())
    })?;
    decode_u64(&row.10, "last-start time")?;
    if let Some(value) = &row.11 {
        decode_u64(value, "last-clean-shutdown time")?;
    }
    let source_count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM product_source_watermarks",
        [],
        |row| row.get(0),
    )?;
    if source_count > i64::try_from(MAX_PRODUCT_SOURCES).expect("source limit fits SQLite i64") {
        return Err(ProductStateError::InvalidState(
            "product source watermark count exceeds its limit".to_owned(),
        ));
    }
    let mut statement = connection
        .prepare("SELECT last_sequence FROM product_source_watermarks ORDER BY source_epoch")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    for sequence in rows {
        let sequence = decode_u64(&sequence?, "source watermark")?;
        if sequence == 0 {
            return Err(ProductStateError::InvalidState(
                "product source watermark is zero".to_owned(),
            ));
        }
    }
    Ok(())
}

fn read_summary(
    connection: &Connection,
    now_millis: u64,
) -> Result<ProductSummary, ProductStateError> {
    let row = connection.query_row(
        "SELECT installation_id, created_at_millis, first_version,
                current_version, disclosure_version, statistics_enabled,
                torrents_added, downloads_completed, foreground_sessions,
                reset_generation, last_start_millis,
                last_clean_shutdown_millis
         FROM product_state WHERE singleton = 1",
        [],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, u32>(4)?,
                row.get::<_, bool>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, Option<String>>(11)?,
            ))
        },
    )?;
    let created_at_millis = decode_u64(&row.1, "creation time")?;
    let days_since_first_use = now_millis
        .saturating_sub(created_at_millis)
        .checked_div(86_400_000)
        .unwrap_or(0);
    let transmission_allowed = row.4 == CURRENT_PRODUCT_DISCLOSURE_VERSION && row.5;
    Ok(ProductSummary {
        installation_id: row.0,
        created_at_millis: row.1,
        first_version: row.2,
        current_version: row.3,
        disclosure_version: row.4,
        statistics_enabled: row.5,
        torrents_added: row.6,
        downloads_completed: row.7,
        foreground_sessions: row.8,
        reset_generation: row.9,
        last_start_millis: row.10,
        last_clean_shutdown_millis: row.11,
        days_since_first_use,
        transmission_allowed,
    })
}

fn source_watermark(
    transaction: &Transaction<'_>,
    source_epoch: &[u8; 16],
) -> Result<Option<u64>, ProductStateError> {
    transaction
        .query_row(
            "SELECT last_sequence FROM product_source_watermarks
             WHERE source_epoch = ?1",
            [source_epoch.as_slice()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|value| decode_u64(&value, "source watermark"))
        .transpose()
}

fn increment_counter(
    transaction: &Transaction<'_>,
    column: &'static str,
) -> Result<(), ProductStateError> {
    if !matches!(
        column,
        "torrents_added" | "downloads_completed" | "foreground_sessions"
    ) {
        return Err(ProductStateError::InvalidConfiguration(
            "unknown product counter".to_owned(),
        ));
    }
    let query = format!("SELECT {column} FROM product_state WHERE singleton = 1");
    let current: String = transaction.query_row(&query, [], |row| row.get(0))?;
    let next = decode_u64(&current, column)?.saturating_add(1);
    let update = format!("UPDATE product_state SET {column} = ?1 WHERE singleton = 1");
    transaction.execute(&update, [encode_u64(next)])?;
    Ok(())
}

fn validate_version(value: &str) -> Result<(), ProductStateError> {
    if value.is_empty()
        || value.len() > MAX_PRODUCT_VERSION_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(ProductStateError::InvalidConfiguration(format!(
            "product version must be 1..={MAX_PRODUCT_VERSION_BYTES} bytes without control characters"
        )));
    }
    Ok(())
}

fn invalid_configuration_as_state(error: ProductStateError) -> ProductStateError {
    ProductStateError::InvalidState(error.to_string())
}

fn encode_u64(value: u64) -> String {
    value.to_string()
}

fn decode_u64(value: &str, field: &str) -> Result<u64, ProductStateError> {
    if value.is_empty()
        || value.len() > MAX_U64_TEXT_BYTES
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(ProductStateError::InvalidState(format!(
            "{field} is not a canonical unsigned integer"
        )));
    }
    value.parse().map_err(|_| {
        ProductStateError::InvalidState(format!("{field} exceeds the supported range"))
    })
}

fn now_millis() -> Result<u64, ProductStateError> {
    let duration = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| {
        ProductStateError::InvalidState("system clock is before the Unix epoch".to_owned())
    })?;
    u64::try_from(duration.as_millis()).map_err(|_| {
        ProductStateError::InvalidState("system clock milliseconds overflow u64".to_owned())
    })
}

fn configure_durable_connection(connection: &Connection) -> Result<(), ProductStateError> {
    connection.busy_timeout(BUSY_TIMEOUT)?;
    let journal_mode: String =
        connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        connection.pragma_update(None, "journal_mode", "WAL")?;
    }
    let journal_mode: String =
        connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        return Err(ProductStateError::RequiredPragma("journal_mode=WAL"));
    }
    connection.pragma_update(None, "synchronous", "FULL")?;
    let synchronous: i64 = connection.pragma_query_value(None, "synchronous", |row| row.get(0))?;
    if synchronous != 2 {
        return Err(ProductStateError::RequiredPragma("synchronous=FULL"));
    }
    Ok(())
}

fn pragma_u64(connection: &Connection, pragma: &str) -> Result<u64, ProductStateError> {
    let value: i64 = connection.pragma_query_value(None, pragma, |row| row.get(0))?;
    u64::try_from(value)
        .map_err(|_| ProductStateError::InvalidState(format!("negative product SQLite {pragma}")))
}

#[cfg(test)]
mod tests {
    use super::{
        CURRENT_PRODUCT_DISCLOSURE_VERSION, MAX_PRODUCT_SOURCES, PRODUCT_DATABASE_FILENAME,
        PRODUCT_SCHEMA_VERSION, ProductMilestone, ProductMilestoneKind, ProductStateError,
        ProductStateStore,
    };

    const START: u64 = 1_800_000_000_000;

    #[test]
    fn fresh_state_is_stable_private_and_not_transmitted_before_disclosure() {
        let root = tempfile::tempdir().expect("temporary product root");
        let first = ProductStateStore::open_at(root.path(), "1.2.3", None, START)
            .expect("open product state");
        let first_summary = first.summary_at(START).expect("fresh summary");
        assert!(uuid::Uuid::parse_str(&first_summary.installation_id).is_ok());
        assert_eq!(first_summary.first_version, "1.2.3");
        assert_eq!(first_summary.current_version, "1.2.3");
        assert!(first_summary.statistics_enabled);
        assert_eq!(first_summary.disclosure_version, 0);
        assert!(!first_summary.transmission_allowed);
        assert_eq!(first_summary.days_since_first_use, 0);
        drop(first);

        let reopened = ProductStateStore::open_at(root.path(), "1.2.4", None, START + 86_400_000)
            .expect("reopen product state");
        let reopened_summary = reopened
            .summary_at(START + 86_400_000)
            .expect("reopened summary");
        assert_eq!(
            reopened_summary.installation_id,
            first_summary.installation_id
        );
        assert_eq!(reopened_summary.first_version, "1.2.3");
        assert_eq!(reopened_summary.current_version, "1.2.4");
        assert_eq!(reopened_summary.days_since_first_use, 1);
        assert!(root.path().join(PRODUCT_DATABASE_FILENAME).is_file());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(root.path().join(PRODUCT_DATABASE_FILENAME))
                .expect("product database metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0);
        }
    }

    #[test]
    fn valid_legacy_id_is_adopted_only_for_fresh_state() {
        let root = tempfile::tempdir().expect("temporary product root");
        let legacy = "87e66203-9849-44c5-a557-8e77c29e7587";
        let first = ProductStateStore::open_at(root.path(), "1", Some(legacy), START)
            .expect("adopt legacy ID");
        assert_eq!(first.summary_at(START).unwrap().installation_id, legacy);
        drop(first);

        let reopened = ProductStateStore::open_at(
            root.path(),
            "2",
            Some("6521c174-0aa9-4fc8-b1fe-702ff3d332d6"),
            START + 1,
        )
        .expect("reopen existing product state");
        assert_eq!(
            reopened.summary_at(START + 1).unwrap().installation_id,
            legacy
        );
    }

    #[test]
    fn disclosure_preference_and_reset_are_atomic() {
        let root = tempfile::tempdir().expect("temporary product root");
        let mut store = ProductStateStore::open_at(root.path(), "1", None, START).unwrap();
        assert!(matches!(
            store.set_statistics_enabled(false),
            Err(ProductStateError::InvalidState(_))
        ));
        let acknowledged = store.acknowledge_disclosure(true).unwrap();
        assert_eq!(
            acknowledged.disclosure_version,
            CURRENT_PRODUCT_DISCLOSURE_VERSION
        );
        assert!(acknowledged.transmission_allowed);
        let disabled = store.set_statistics_enabled(false).unwrap();
        assert!(!disabled.statistics_enabled);
        assert!(!disabled.transmission_allowed);
        let old_id = disabled.installation_id;
        let old_generation = disabled.reset_generation;
        store.record_foreground_session().unwrap();
        let reset = store.reset_statistics_at(START + 5).unwrap();
        assert_ne!(reset.installation_id, old_id);
        assert_ne!(reset.reset_generation, old_generation);
        assert_eq!(reset.created_at_millis, (START + 5).to_string());
        assert_eq!(reset.foreground_sessions, "0");
        assert!(!reset.statistics_enabled);
        assert!(!reset.transmission_allowed);
    }

    #[test]
    fn milestone_application_is_ordered_idempotent_and_source_bounded() {
        let mut store = ProductStateStore::open_ephemeral_at("1", START).unwrap();
        let source = [7; 16];
        let milestones = [
            ProductMilestone {
                source_epoch: source,
                sequence: 1,
                kind: ProductMilestoneKind::TorrentAdded,
            },
            ProductMilestone {
                source_epoch: source,
                sequence: 2,
                kind: ProductMilestoneKind::DownloadCompleted,
            },
        ];
        let applied = store.apply_milestones(&milestones).unwrap();
        assert_eq!(applied.torrents_added, "1");
        assert_eq!(applied.downloads_completed, "1");
        let replayed = store.apply_milestones(&milestones).unwrap();
        assert_eq!(replayed.torrents_added, "1");
        assert_eq!(replayed.downloads_completed, "1");
        assert!(matches!(
            store.apply_milestones(&[ProductMilestone {
                source_epoch: source,
                sequence: 4,
                kind: ProductMilestoneKind::TorrentAdded,
            }]),
            Err(ProductStateError::SequenceGap {
                expected: 3,
                actual: 4
            })
        ));

        for index in 1..MAX_PRODUCT_SOURCES {
            let mut epoch = [0; 16];
            epoch[..8].copy_from_slice(&(index as u64).to_be_bytes());
            epoch[15] = 1;
            store
                .apply_milestones(&[ProductMilestone {
                    source_epoch: epoch,
                    sequence: 1,
                    kind: ProductMilestoneKind::TorrentAdded,
                }])
                .unwrap();
        }
        assert!(matches!(
            store.apply_milestones(&[ProductMilestone {
                source_epoch: [9; 16],
                sequence: 1,
                kind: ProductMilestoneKind::TorrentAdded,
            }]),
            Err(ProductStateError::SourceLimit)
        ));
    }

    #[test]
    fn counter_saturates_and_clock_rollback_reports_zero_days() {
        let mut store = ProductStateStore::open_ephemeral_at("1", START).unwrap();
        store
            .connection
            .execute(
                "UPDATE product_state SET foreground_sessions = ?1 WHERE singleton = 1",
                [u64::MAX.to_string()],
            )
            .unwrap();
        let summary = store.record_foreground_session().unwrap();
        assert_eq!(summary.foreground_sessions, u64::MAX.to_string());
        assert_eq!(store.summary_at(START - 1).unwrap().days_since_first_use, 0);
    }

    #[test]
    fn malformed_and_future_state_fail_closed() {
        let root = tempfile::tempdir().expect("temporary product root");
        let store = ProductStateStore::open_at(root.path(), "1", None, START).unwrap();
        store
            .connection
            .execute(
                "UPDATE product_state SET torrents_added = '01' WHERE singleton = 1",
                [],
            )
            .unwrap();
        drop(store);
        assert!(matches!(
            ProductStateStore::open_at(root.path(), "1", None, START + 1),
            Err(ProductStateError::InvalidState(_))
        ));

        let future = tempfile::tempdir().expect("future product root");
        let store = ProductStateStore::open_at(future.path(), "1", None, START).unwrap();
        store
            .connection
            .pragma_update(None, "user_version", PRODUCT_SCHEMA_VERSION + 1)
            .unwrap();
        drop(store);
        assert!(matches!(
            ProductStateStore::open_at(future.path(), "1", None, START + 1),
            Err(ProductStateError::UnsupportedSchema { .. })
        ));
    }

    #[test]
    fn milestone_kind_round_trips_closed_names() {
        for kind in [
            ProductMilestoneKind::TorrentAdded,
            ProductMilestoneKind::DownloadCompleted,
        ] {
            assert_eq!(ProductMilestoneKind::parse(kind.as_str()).unwrap(), kind);
        }
        assert!(ProductMilestoneKind::parse("arbitrary_event").is_err());
    }
}
