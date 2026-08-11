//! SQLite schema facts and the newest bounded migration.
//!
//! Legacy migrations remain next to their one-off projection helpers in
//! `store` for now. New schema work enters here so queue/state evolution does
//! not further expand the store's command and projection owner.

use rusqlite::Connection;

use crate::settings::{migrate_client_settings_to_v17, migrate_client_settings_to_v18};
use crate::store::StoreError;

pub(crate) const SCHEMA_VERSION: i64 = 18;

pub(crate) const DHT_TABLES_SQL: &str = "CREATE TABLE dht_state (
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
     ) WITHOUT ROWID;
     CREATE TABLE dht_identities (
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
     ) WITHOUT ROWID;";

pub(crate) const REMOVAL_TABLE_SQL: &str = "CREATE TABLE removal_jobs (
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

pub(crate) const SOURCE_TABLES_SQL: &str = "CREATE TABLE torrent_source (
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

pub(crate) const DOWNLOAD_QUEUE_INDEX_SQL: &str =
    "CREATE UNIQUE INDEX IF NOT EXISTS download_queue_order
     ON torrents(download_queue_position)
     WHERE download_queue_position IS NOT NULL;";

pub(crate) fn migrate_to_v17(connection: &mut Connection) -> Result<(), StoreError> {
    let transaction = connection.transaction()?;
    migrate_client_settings_to_v17(&transaction)?;
    let has_queue_position = {
        let mut statement = transaction.prepare("PRAGMA table_info(torrents)")?;
        let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
        columns
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .any(|column| column == "download_queue_position")
    };
    if !has_queue_position {
        transaction.execute_batch(
            "ALTER TABLE torrents ADD COLUMN download_queue_position INTEGER;
             WITH queued AS (
            SELECT info_hash,
                   (ROW_NUMBER() OVER (
                       ORDER BY created_revision, info_hash
                   ) - 1) * 1024 AS position
            FROM torrents
            WHERE payload_state <> 'final_owned'
         )
         UPDATE torrents
         SET download_queue_position = (
             SELECT position FROM queued
             WHERE queued.info_hash = torrents.info_hash
         )
             WHERE info_hash IN (SELECT info_hash FROM queued);",
        )?;
    }
    transaction.execute_batch(DOWNLOAD_QUEUE_INDEX_SQL)?;
    transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    transaction.commit()?;
    Ok(())
}

pub(crate) fn migrate_to_v18(connection: &mut Connection) -> Result<(), StoreError> {
    let transaction = connection.transaction()?;
    migrate_client_settings_to_v18(&transaction)?;
    let columns = {
        let mut statement = transaction.prepare("PRAGMA table_info(torrents)")?;
        let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
        columns.collect::<Result<Vec<_>, _>>()?
    };
    if !columns.iter().any(|column| column == "upload_rate_limit") {
        transaction.execute_batch(
            "ALTER TABLE torrents ADD COLUMN upload_rate_limit INTEGER NOT NULL DEFAULT 0
             CHECK (upload_rate_limit = 0 OR upload_rate_limit BETWEEN 1024 AND 4294967295);",
        )?;
    }
    if !columns.iter().any(|column| column == "download_rate_limit") {
        transaction.execute_batch(
            "ALTER TABLE torrents ADD COLUMN download_rate_limit INTEGER NOT NULL DEFAULT 0
             CHECK (download_rate_limit = 0 OR download_rate_limit BETWEEN 1024 AND 4294967295);",
        )?;
    }
    transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    transaction.commit()?;
    Ok(())
}
