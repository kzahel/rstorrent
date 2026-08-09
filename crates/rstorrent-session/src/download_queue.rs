//! Transactional durable ordering for incomplete downloads.

use rusqlite::{OptionalExtension, Transaction, params};

use crate::store::StoreError;

const QUEUE_STRIDE: i64 = 1_024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QueueEdge {
    Top,
    Bottom,
}

pub(crate) fn append(
    transaction: &Transaction<'_>,
    info_hash: &[u8; 20],
) -> Result<bool, StoreError> {
    place_missing(transaction, info_hash, QueueEdge::Bottom)
}

pub(crate) fn place_missing(
    transaction: &Transaction<'_>,
    info_hash: &[u8; 20],
    edge: QueueEdge,
) -> Result<bool, StoreError> {
    let current = queue_position(transaction, info_hash)?;
    if current.is_some() {
        return Ok(false);
    }
    let position = edge_position(transaction, edge)?;
    let updated = transaction.execute(
        "UPDATE torrents SET download_queue_position = ?2 WHERE info_hash = ?1",
        params![info_hash.as_slice(), position],
    )?;
    if updated != 1 {
        return Err(StoreError::UnknownTorrent(hex_info_hash(info_hash)));
    }
    Ok(true)
}

pub(crate) fn move_to_edge(
    transaction: &Transaction<'_>,
    info_hash: &[u8; 20],
    edge: QueueEdge,
) -> Result<bool, StoreError> {
    let current = queue_position(transaction, info_hash)?
        .ok_or_else(|| StoreError::DurableState("torrent has no download queue position".into()))?;
    let boundary: Option<i64> = transaction.query_row(
        match edge {
            QueueEdge::Top => "SELECT MIN(download_queue_position) FROM torrents",
            QueueEdge::Bottom => "SELECT MAX(download_queue_position) FROM torrents",
        },
        [],
        |row| row.get(0),
    )?;
    if boundary == Some(current) {
        return Ok(false);
    }
    let position = edge_position(transaction, edge)?;
    transaction.execute(
        "UPDATE torrents SET download_queue_position = ?2 WHERE info_hash = ?1",
        params![info_hash.as_slice(), position],
    )?;
    Ok(true)
}

pub(crate) fn is_at_edge(
    transaction: &Transaction<'_>,
    info_hash: &[u8; 20],
    edge: QueueEdge,
) -> Result<bool, StoreError> {
    let current = queue_position(transaction, info_hash)?;
    let boundary: Option<i64> = transaction.query_row(
        match edge {
            QueueEdge::Top => "SELECT MIN(download_queue_position) FROM torrents",
            QueueEdge::Bottom => "SELECT MAX(download_queue_position) FROM torrents",
        },
        [],
        |row| row.get(0),
    )?;
    Ok(current.is_some() && current == boundary)
}

pub(crate) fn queue_position(
    transaction: &Transaction<'_>,
    info_hash: &[u8; 20],
) -> Result<Option<i64>, StoreError> {
    transaction
        .query_row(
            "SELECT download_queue_position FROM torrents WHERE info_hash = ?1",
            [info_hash.as_slice()],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| StoreError::UnknownTorrent(hex_info_hash(info_hash)))
}

fn edge_position(transaction: &Transaction<'_>, edge: QueueEdge) -> Result<i64, StoreError> {
    let boundary: Option<i64> = transaction.query_row(
        match edge {
            QueueEdge::Top => "SELECT MIN(download_queue_position) FROM torrents",
            QueueEdge::Bottom => "SELECT MAX(download_queue_position) FROM torrents",
        },
        [],
        |row| row.get(0),
    )?;
    let candidate = match (edge, boundary) {
        (_, None) => Some(0),
        (QueueEdge::Top, Some(value)) => value.checked_sub(QUEUE_STRIDE),
        (QueueEdge::Bottom, Some(value)) => value.checked_add(QUEUE_STRIDE),
    };
    if let Some(candidate) = candidate {
        return Ok(candidate);
    }
    dense_renumber(transaction)?;
    let boundary: Option<i64> = transaction.query_row(
        match edge {
            QueueEdge::Top => "SELECT MIN(download_queue_position) FROM torrents",
            QueueEdge::Bottom => "SELECT MAX(download_queue_position) FROM torrents",
        },
        [],
        |row| row.get(0),
    )?;
    let candidate = match (edge, boundary) {
        (_, None) => Some(0),
        (QueueEdge::Top, Some(value)) => value.checked_sub(QUEUE_STRIDE),
        (QueueEdge::Bottom, Some(value)) => value.checked_add(QUEUE_STRIDE),
    };
    candidate.ok_or_else(|| StoreError::DurableState("download queue position overflow".to_owned()))
}

fn dense_renumber(transaction: &Transaction<'_>) -> Result<(), StoreError> {
    let ordered = {
        let mut statement = transaction.prepare(
            "SELECT info_hash FROM torrents
             WHERE download_queue_position IS NOT NULL
             ORDER BY download_queue_position, info_hash",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, Vec<u8>>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    transaction.execute(
        "UPDATE torrents SET download_queue_position = NULL
         WHERE download_queue_position IS NOT NULL",
        [],
    )?;
    for (index, info_hash) in ordered.iter().enumerate() {
        let index = i64::try_from(index)
            .map_err(|_| StoreError::DurableState("download queue is too large".to_owned()))?;
        let position = index
            .checked_mul(QUEUE_STRIDE)
            .ok_or_else(|| StoreError::DurableState("download queue is too large".to_owned()))?;
        transaction.execute(
            "UPDATE torrents SET download_queue_position = ?2 WHERE info_hash = ?1",
            params![info_hash, position],
        )?;
    }
    Ok(())
}

fn hex_info_hash(info_hash: &[u8; 20]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(40);
    for byte in info_hash {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

#[cfg(test)]
mod tests {
    use rusqlite::{Connection, params};

    use super::{QueueEdge, append, move_to_edge};

    fn database() -> Connection {
        let connection = Connection::open_in_memory().expect("database");
        connection
            .execute_batch(
                "CREATE TABLE torrents (
                    info_hash BLOB PRIMARY KEY,
                    download_queue_position INTEGER
                 );
                 CREATE UNIQUE INDEX download_queue_order
                 ON torrents(download_queue_position)
                 WHERE download_queue_position IS NOT NULL;",
            )
            .expect("schema");
        connection
    }

    fn hash(value: u8) -> [u8; 20] {
        [value; 20]
    }

    fn positions(connection: &Connection) -> Vec<u8> {
        let mut statement = connection
            .prepare(
                "SELECT info_hash FROM torrents
                 WHERE download_queue_position IS NOT NULL
                 ORDER BY download_queue_position, info_hash",
            )
            .expect("query");
        statement
            .query_map([], |row| row.get::<_, Vec<u8>>(0))
            .expect("rows")
            .map(|row| row.expect("row")[0])
            .collect()
    }

    #[test]
    fn append_and_edge_moves_preserve_a_total_order() {
        let mut connection = database();
        for value in 1..=3 {
            connection
                .execute(
                    "INSERT INTO torrents(info_hash) VALUES (?1)",
                    [hash(value).as_slice()],
                )
                .expect("insert");
            let transaction = connection.transaction().expect("transaction");
            assert!(append(&transaction, &hash(value)).expect("append"));
            transaction.commit().expect("commit");
        }
        let transaction = connection.transaction().expect("transaction");
        assert!(move_to_edge(&transaction, &hash(3), QueueEdge::Top).expect("move top"));
        transaction.commit().expect("commit");
        assert_eq!(positions(&connection), vec![3, 1, 2]);

        let transaction = connection.transaction().expect("transaction");
        assert!(move_to_edge(&transaction, &hash(3), QueueEdge::Bottom).expect("move bottom"));
        transaction.commit().expect("commit");
        assert_eq!(positions(&connection), vec![1, 2, 3]);
    }

    #[test]
    fn near_overflow_is_renumbered_inside_the_transaction() {
        let mut connection = database();
        for (value, position) in [(1, i64::MAX - 1), (2, i64::MAX)] {
            connection
                .execute(
                    "INSERT INTO torrents(info_hash, download_queue_position)
                     VALUES (?1, ?2)",
                    params![hash(value).as_slice(), position],
                )
                .expect("insert");
        }
        connection
            .execute(
                "INSERT INTO torrents(info_hash) VALUES (?1)",
                [hash(3).as_slice()],
            )
            .expect("insert");
        let transaction = connection.transaction().expect("transaction");
        assert!(append(&transaction, &hash(3)).expect("append"));
        transaction.commit().expect("commit");
        assert_eq!(positions(&connection), vec![1, 2, 3]);
    }
}
