use std::error::Error;
use std::fmt;

use rusqlite::{Connection, Transaction, params};

use super::contract::{ClientSettings, ListenerPolicy, PortMappingPolicy};

const CLIENT_SETTINGS_TABLE_SQL: &str = "CREATE TABLE client_settings (
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
) -> Result<(), SettingsPersistenceError> {
    transaction.execute_batch(CLIENT_SETTINGS_TABLE_SQL)?;
    let settings = ClientSettings::default();
    let (mode, port) = listener_columns(settings.listener);
    let mapping_mode = mapping_column(settings.port_mapping);
    transaction.execute(
        "INSERT INTO client_settings(
            singleton, listener_mode, listener_port,
            port_mapping_mode, peer_connection_limit, upload_slots
         ) VALUES (1, ?1, ?2, ?3, ?4, ?5)",
        params![
            mode,
            port.map(i64::from),
            mapping_mode,
            i64::from(settings.peer_connection_limit),
            i64::from(settings.upload_slots),
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
    let (mode, port, mapping_mode, peer_connection_limit, upload_slots) = connection.query_row(
        "SELECT listener_mode, listener_port, port_mapping_mode,
                peer_connection_limit, upload_slots
         FROM client_settings WHERE singleton = 1",
        [],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
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
        port_mapping,
        peer_connection_limit: u32::try_from(peer_connection_limit).map_err(|_| {
            SettingsPersistenceError::Corrupt(
                "peer connection limit cannot be represented".to_owned(),
            )
        })?,
        upload_slots: u16::try_from(upload_slots).map_err(|_| {
            SettingsPersistenceError::Corrupt("upload slots cannot be represented".to_owned())
        })?,
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
    let changed = transaction.execute(
        "UPDATE client_settings
         SET listener_mode = ?1, listener_port = ?2,
             port_mapping_mode = ?3,
             peer_connection_limit = ?4, upload_slots = ?5
         WHERE singleton = 1",
        params![
            mode,
            port.map(i64::from),
            mapping_mode,
            i64::from(settings.peer_connection_limit),
            i64::from(settings.upload_slots),
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

pub(crate) fn migrate_client_settings_to_v10(
    transaction: &Transaction<'_>,
) -> Result<(), SettingsPersistenceError> {
    transaction.execute_batch("ALTER TABLE client_settings RENAME TO client_settings_v9;")?;
    transaction.execute_batch(CLIENT_SETTINGS_TABLE_SQL)?;
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
