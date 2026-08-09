use std::error::Error;
use std::fmt;

use rusqlite::{Connection, Transaction, params};

use super::contract::{
    ClientSettings, EncryptionPolicy, HttpsServerAuthenticationPolicy, ListenerPolicy,
    PortMappingPolicy,
};

const CLIENT_SETTINGS_TABLE_SQL: &str = "CREATE TABLE client_settings (
        singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
        listener_mode TEXT NOT NULL CHECK (
            listener_mode IN (
                'disabled', 'automatic_loopback', 'fixed_loopback',
                'automatic_local_network', 'fixed_local_network'
            )
        ),
        listener_port INTEGER,
        preferred_listen_port INTEGER NOT NULL CHECK (
            preferred_listen_port BETWEEN 1024 AND 65535
        ),
        port_mapping_mode TEXT NOT NULL CHECK (
            port_mapping_mode IN ('disabled', 'upnp')
        ),
        peer_connection_limit INTEGER NOT NULL CHECK (
            peer_connection_limit BETWEEN 1 AND 2000
        ),
        upload_slots INTEGER NOT NULL CHECK (upload_slots BETWEEN 0 AND 50),
        encryption TEXT NOT NULL DEFAULT 'allow' CHECK (
            encryption IN ('disabled', 'allow', 'prefer', 'required')
        ),
        ipv6_enabled INTEGER NOT NULL DEFAULT 1 CHECK (ipv6_enabled IN (0, 1)),
        tracker_https_server_authentication TEXT NOT NULL CHECK (
            tracker_https_server_authentication IN ('system_trust', 'disabled')
        ),
        CHECK (
            (listener_mode IN ('fixed_loopback', 'fixed_local_network') AND
             listener_port BETWEEN 1024 AND 65535) OR
            (listener_mode IN (
                'disabled', 'automatic_loopback', 'automatic_local_network'
             ) AND
             listener_port IS NULL)
        )
     );";

const CLIENT_SETTINGS_TABLE_V11_SQL: &str = "CREATE TABLE client_settings (
        singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
        listener_mode TEXT NOT NULL CHECK (
            listener_mode IN (
                'disabled', 'automatic_loopback', 'fixed_loopback',
                'automatic_local_network', 'fixed_local_network'
            )
        ),
        listener_port INTEGER,
        preferred_listen_port INTEGER NOT NULL CHECK (
            preferred_listen_port BETWEEN 1024 AND 65535
        ),
        port_mapping_mode TEXT NOT NULL CHECK (
            port_mapping_mode IN ('disabled', 'upnp')
        ),
        peer_connection_limit INTEGER NOT NULL CHECK (
            peer_connection_limit BETWEEN 1 AND 2000
        ),
        upload_slots INTEGER NOT NULL CHECK (upload_slots BETWEEN 0 AND 50),
        CHECK (
            (listener_mode IN ('fixed_loopback', 'fixed_local_network') AND
             listener_port BETWEEN 1024 AND 65535) OR
            (listener_mode IN (
                'disabled', 'automatic_loopback', 'automatic_local_network'
             ) AND
             listener_port IS NULL)
        )
     );";

const CLIENT_SETTINGS_TABLE_V10_SQL: &str = "CREATE TABLE client_settings (
        singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
        listener_mode TEXT NOT NULL CHECK (
            listener_mode IN (
                'disabled', 'automatic_loopback', 'fixed_loopback',
                'automatic_local_network', 'fixed_local_network'
            )
        ),
        listener_port INTEGER,
        port_mapping_mode TEXT NOT NULL CHECK (
            port_mapping_mode IN ('disabled', 'upnp')
        ),
        peer_connection_limit INTEGER NOT NULL CHECK (
            peer_connection_limit BETWEEN 1 AND 2000
        ),
        upload_slots INTEGER NOT NULL CHECK (upload_slots BETWEEN 0 AND 50),
        CHECK (
            (listener_mode IN ('fixed_loopback', 'fixed_local_network') AND
             listener_port BETWEEN 1024 AND 65535) OR
            (listener_mode IN (
                'disabled', 'automatic_loopback', 'automatic_local_network'
             ) AND
             listener_port IS NULL)
        )
     );";

#[derive(Debug)]
pub(crate) enum SettingsPersistenceError {
    Sqlite(rusqlite::Error),
    Corrupt(String),
}

impl fmt::Display for SettingsPersistenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "client settings database: {error}"),
            Self::Corrupt(message) => {
                write!(formatter, "invalid durable client settings: {message}")
            }
        }
    }
}

impl Error for SettingsPersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
            Self::Corrupt(_) => None,
        }
    }
}

impl From<rusqlite::Error> for SettingsPersistenceError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

pub(crate) fn create_client_settings(
    transaction: &Transaction<'_>,
    settings: &ClientSettings,
) -> Result<(), SettingsPersistenceError> {
    transaction.execute_batch(CLIENT_SETTINGS_TABLE_SQL)?;
    let (mode, port) = listener_columns(settings.listener);
    let mapping_mode = mapping_column(settings.port_mapping);
    let tracker_https_authentication =
        tracker_https_authentication_column(settings.tracker_https_server_authentication);
    let encryption = encryption_column(settings.encryption);
    transaction.execute(
        "INSERT INTO client_settings(
            singleton, listener_mode, listener_port, preferred_listen_port,
            port_mapping_mode, peer_connection_limit, upload_slots,
            encryption, ipv6_enabled, tracker_https_server_authentication
         ) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            mode,
            port.map(i64::from),
            i64::from(settings.preferred_listen_port),
            mapping_mode,
            i64::from(settings.peer_connection_limit),
            i64::from(settings.upload_slots),
            encryption,
            settings.ipv6_enabled,
            tracker_https_authentication,
        ],
    )?;
    Ok(())
}

pub(crate) fn read_client_settings(
    connection: &Connection,
) -> Result<ClientSettings, SettingsPersistenceError> {
    let count: i64 =
        connection.query_row("SELECT COUNT(*) FROM client_settings", [], |row| row.get(0))?;
    if count != 1 {
        return Err(SettingsPersistenceError::Corrupt(format!(
            "expected one singleton row, found {count}"
        )));
    }
    let (
        mode,
        port,
        preferred_listen_port,
        mapping_mode,
        peer_connection_limit,
        upload_slots,
        encryption,
        ipv6_enabled,
        tracker_https_authentication,
    ) = connection.query_row(
        "SELECT listener_mode, listener_port, preferred_listen_port, port_mapping_mode,
                peer_connection_limit, upload_slots, encryption, ipv6_enabled,
                tracker_https_server_authentication
         FROM client_settings WHERE singleton = 1",
        [],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, bool>(7)?,
                row.get::<_, String>(8)?,
            ))
        },
    )?;
    let listener = match (mode.as_str(), port) {
        ("disabled", None) => ListenerPolicy::Disabled,
        ("automatic_loopback", None) => ListenerPolicy::AutomaticLoopback,
        ("fixed_loopback", Some(port)) => ListenerPolicy::FixedLoopback {
            port: u16::try_from(port).map_err(|_| {
                SettingsPersistenceError::Corrupt("fixed listener port is out of range".to_owned())
            })?,
        },
        ("automatic_local_network", None) => ListenerPolicy::AutomaticLocalNetwork,
        ("fixed_local_network", Some(port)) => ListenerPolicy::FixedLocalNetwork {
            port: u16::try_from(port).map_err(|_| {
                SettingsPersistenceError::Corrupt("fixed listener port is out of range".to_owned())
            })?,
        },
        _ => {
            return Err(SettingsPersistenceError::Corrupt(
                "listener mode and port are inconsistent".to_owned(),
            ));
        }
    };
    let port_mapping = match mapping_mode.as_str() {
        "disabled" => PortMappingPolicy::Disabled,
        "upnp" => PortMappingPolicy::Upnp,
        _ => {
            return Err(SettingsPersistenceError::Corrupt(
                "port mapping mode is invalid".to_owned(),
            ));
        }
    };
    let settings = ClientSettings {
        listener,
        preferred_listen_port: u16::try_from(preferred_listen_port).map_err(|_| {
            SettingsPersistenceError::Corrupt(
                "preferred listener port cannot be represented".to_owned(),
            )
        })?,
        port_mapping,
        peer_connection_limit: u32::try_from(peer_connection_limit).map_err(|_| {
            SettingsPersistenceError::Corrupt(
                "peer connection limit cannot be represented".to_owned(),
            )
        })?,
        upload_slots: u16::try_from(upload_slots).map_err(|_| {
            SettingsPersistenceError::Corrupt("upload slots cannot be represented".to_owned())
        })?,
        encryption: match encryption.as_str() {
            "disabled" => EncryptionPolicy::Disabled,
            "allow" => EncryptionPolicy::Allow,
            "prefer" => EncryptionPolicy::Prefer,
            "required" => EncryptionPolicy::Required,
            _ => {
                return Err(SettingsPersistenceError::Corrupt(
                    "encryption policy is invalid".to_owned(),
                ));
            }
        },
        ipv6_enabled,
        tracker_https_server_authentication: match tracker_https_authentication.as_str() {
            "system_trust" => HttpsServerAuthenticationPolicy::SystemTrust,
            "disabled" => HttpsServerAuthenticationPolicy::Disabled,
            _ => {
                return Err(SettingsPersistenceError::Corrupt(
                    "tracker HTTPS server authentication policy is invalid".to_owned(),
                ));
            }
        },
    };
    settings
        .validate()
        .map_err(|error| SettingsPersistenceError::Corrupt(error.to_string()))?;
    Ok(settings)
}

pub(crate) fn replace_client_settings(
    transaction: &Transaction<'_>,
    settings: &ClientSettings,
) -> Result<bool, SettingsPersistenceError> {
    settings
        .validate()
        .map_err(|error| SettingsPersistenceError::Corrupt(error.to_string()))?;
    if read_client_settings(transaction)? == *settings {
        return Ok(false);
    }
    let (mode, port) = listener_columns(settings.listener);
    let mapping_mode = mapping_column(settings.port_mapping);
    let tracker_https_authentication =
        tracker_https_authentication_column(settings.tracker_https_server_authentication);
    let encryption = encryption_column(settings.encryption);
    let changed = transaction.execute(
        "UPDATE client_settings
         SET listener_mode = ?1, listener_port = ?2,
             preferred_listen_port = ?3, port_mapping_mode = ?4,
             peer_connection_limit = ?5, upload_slots = ?6,
             encryption = ?7, ipv6_enabled = ?8,
             tracker_https_server_authentication = ?9
         WHERE singleton = 1",
        params![
            mode,
            port.map(i64::from),
            i64::from(settings.preferred_listen_port),
            mapping_mode,
            i64::from(settings.peer_connection_limit),
            i64::from(settings.upload_slots),
            encryption,
            settings.ipv6_enabled,
            tracker_https_authentication,
        ],
    )?;
    if changed != 1 {
        return Err(SettingsPersistenceError::Corrupt(format!(
            "settings replacement changed {changed} rows"
        )));
    }
    Ok(true)
}

fn listener_columns(listener: ListenerPolicy) -> (&'static str, Option<u16>) {
    match listener {
        ListenerPolicy::Disabled => ("disabled", None),
        ListenerPolicy::AutomaticLoopback => ("automatic_loopback", None),
        ListenerPolicy::FixedLoopback { port } => ("fixed_loopback", Some(port)),
        ListenerPolicy::AutomaticLocalNetwork => ("automatic_local_network", None),
        ListenerPolicy::FixedLocalNetwork { port } => ("fixed_local_network", Some(port)),
    }
}

fn mapping_column(policy: PortMappingPolicy) -> &'static str {
    match policy {
        PortMappingPolicy::Disabled => "disabled",
        PortMappingPolicy::Upnp => "upnp",
    }
}

fn tracker_https_authentication_column(policy: HttpsServerAuthenticationPolicy) -> &'static str {
    match policy {
        HttpsServerAuthenticationPolicy::SystemTrust => "system_trust",
        HttpsServerAuthenticationPolicy::Disabled => "disabled",
    }
}

fn encryption_column(policy: EncryptionPolicy) -> &'static str {
    match policy {
        EncryptionPolicy::Disabled => "disabled",
        EncryptionPolicy::Allow => "allow",
        EncryptionPolicy::Prefer => "prefer",
        EncryptionPolicy::Required => "required",
    }
}

pub(crate) fn migrate_client_settings_to_v15(
    transaction: &Transaction<'_>,
) -> Result<(), SettingsPersistenceError> {
    let has_encryption = {
        let mut statement = transaction.prepare("PRAGMA table_info(client_settings)")?;
        let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
        columns
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .any(|column| column == "encryption")
    };
    if has_encryption {
        return Ok(());
    }
    transaction.execute_batch(
        "ALTER TABLE client_settings ADD COLUMN encryption TEXT NOT NULL DEFAULT 'allow'
         CHECK (encryption IN ('disabled', 'allow', 'prefer', 'required'));",
    )?;
    Ok(())
}

pub(crate) fn migrate_client_settings_to_v16(
    transaction: &Transaction<'_>,
) -> Result<(), SettingsPersistenceError> {
    let has_ipv6_enabled = {
        let mut statement = transaction.prepare("PRAGMA table_info(client_settings)")?;
        let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
        columns
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .any(|column| column == "ipv6_enabled")
    };
    if has_ipv6_enabled {
        return Ok(());
    }
    transaction.execute_batch(
        "ALTER TABLE client_settings ADD COLUMN ipv6_enabled INTEGER NOT NULL DEFAULT 1
         CHECK (ipv6_enabled IN (0, 1));",
    )?;
    Ok(())
}

pub(crate) fn migrate_client_settings_to_v10(
    transaction: &Transaction<'_>,
) -> Result<(), SettingsPersistenceError> {
    transaction.execute_batch("ALTER TABLE client_settings RENAME TO client_settings_v9;")?;
    transaction.execute_batch(CLIENT_SETTINGS_TABLE_V10_SQL)?;
    transaction.execute_batch(
        "INSERT INTO client_settings(
            singleton, listener_mode, listener_port, port_mapping_mode,
            peer_connection_limit, upload_slots
         ) SELECT singleton, listener_mode, listener_port, 'disabled',
                  peer_connection_limit, upload_slots
           FROM client_settings_v9;
         DROP TABLE client_settings_v9;",
    )?;
    Ok(())
}

pub(crate) fn migrate_client_settings_to_v11(
    transaction: &Transaction<'_>,
) -> Result<(), SettingsPersistenceError> {
    transaction.execute_batch("ALTER TABLE client_settings RENAME TO client_settings_v10;")?;
    transaction.execute_batch(CLIENT_SETTINGS_TABLE_V11_SQL)?;
    transaction.execute_batch(
        "INSERT INTO client_settings(
            singleton, listener_mode, listener_port, preferred_listen_port,
            port_mapping_mode, peer_connection_limit, upload_slots
         ) SELECT singleton, listener_mode, listener_port, 6881,
                  port_mapping_mode, peer_connection_limit, upload_slots
           FROM client_settings_v10;
         DROP TABLE client_settings_v10;",
    )?;
    Ok(())
}

pub(crate) fn migrate_client_settings_to_v12(
    transaction: &Transaction<'_>,
) -> Result<(), SettingsPersistenceError> {
    transaction.execute_batch("ALTER TABLE client_settings RENAME TO client_settings_v11;")?;
    transaction.execute_batch(CLIENT_SETTINGS_TABLE_SQL)?;
    transaction.execute_batch(
        "INSERT INTO client_settings(
            singleton, listener_mode, listener_port, preferred_listen_port,
            port_mapping_mode, peer_connection_limit, upload_slots,
            tracker_https_server_authentication
         ) SELECT singleton, listener_mode, listener_port, preferred_listen_port,
                  port_mapping_mode, peer_connection_limit, upload_slots, 'system_trust'
           FROM client_settings_v11;
         DROP TABLE client_settings_v11;",
    )?;
    Ok(())
}
