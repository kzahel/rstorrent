use std::error::Error;
use std::fmt;

use rusqlite::{Connection, Transaction, params};

use super::contract::{ClientSettings, ListenerPolicy};

const CLIENT_SETTINGS_TABLE_SQL: &str = "CREATE TABLE client_settings (
        singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
        listener_mode TEXT NOT NULL CHECK (
            listener_mode IN ('disabled', 'automatic_loopback', 'fixed_loopback')
        ),
        listener_port INTEGER,
        peer_connection_limit INTEGER NOT NULL CHECK (
            peer_connection_limit BETWEEN 1 AND 2000
        ),
        upload_slots INTEGER NOT NULL CHECK (upload_slots BETWEEN 0 AND 50),
        CHECK (
            (listener_mode = 'fixed_loopback' AND
             listener_port BETWEEN 1024 AND 65535) OR
            (listener_mode IN ('disabled', 'automatic_loopback') AND
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
    transaction.execute(
        "INSERT INTO client_settings(
            singleton, listener_mode, listener_port,
            peer_connection_limit, upload_slots
         ) VALUES (1, ?1, ?2, ?3, ?4)",
        params![
            mode,
            port.map(i64::from),
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
    let (mode, port, peer_connection_limit, upload_slots) = connection.query_row(
        "SELECT listener_mode, listener_port, peer_connection_limit, upload_slots
         FROM client_settings WHERE singleton = 1",
        [],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
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
        _ => {
            return Err(SettingsPersistenceError::Corrupt(
                "listener mode and port are inconsistent".to_owned(),
            ));
        }
    };
    let settings = ClientSettings {
        listener,
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
    let changed = transaction.execute(
        "UPDATE client_settings
         SET listener_mode = ?1, listener_port = ?2,
             peer_connection_limit = ?3, upload_slots = ?4
         WHERE singleton = 1",
        params![
            mode,
            port.map(i64::from),
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
    }
}
