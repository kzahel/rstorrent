//! Transactional durable ordering for incomplete downloads.

use rstorrent_engine::TorrentId;
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
    torrent_id: &TorrentId,
) -> Result<bool, StoreError> {
    place_missing(transaction, torrent_id, QueueEdge::Bottom)
}

pub(crate) fn place_missing(
    transaction: &Transaction<'_>,
    torrent_id: &TorrentId,
    edge: QueueEdge,
) -> Result<bool, StoreError> {
    let current = queue_position(transaction, torrent_id)?;
    if current.is_some() {
        return Ok(false);
    }
    let position = edge_position(transaction, edge)?;
    let updated = transaction.execute(
        "UPDATE torrents SET download_queue_position = ?2 WHERE torrent_id = ?1",
        params![torrent_id.as_bytes().as_slice(), position],
    )?;
    if updated != 1 {
        return Err(StoreError::UnknownTorrent(torrent_id.to_string()));
    }
    Ok(true)
}

pub(crate) fn move_to_edge(
    transaction: &Transaction<'_>,
    torrent_id: &TorrentId,
    edge: QueueEdge,
) -> Result<bool, StoreError> {
    let current = queue_position(transaction, torrent_id)?
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
        "UPDATE torrents SET download_queue_position = ?2 WHERE torrent_id = ?1",
        params![torrent_id.as_bytes().as_slice(), position],
    )?;
    Ok(true)
}

pub(crate) fn is_at_edge(
    transaction: &Transaction<'_>,
    torrent_id: &TorrentId,
    edge: QueueEdge,
) -> Result<bool, StoreError> {
    let current = queue_position(transaction, torrent_id)?;
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
    torrent_id: &TorrentId,
) -> Result<Option<i64>, StoreError> {
    transaction
        .query_row(
            "SELECT download_queue_position FROM torrents WHERE torrent_id = ?1",
            [torrent_id.as_bytes().as_slice()],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| StoreError::UnknownTorrent(torrent_id.to_string()))
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
            "SELECT torrent_id FROM torrents
             WHERE download_queue_position IS NOT NULL
             ORDER BY download_queue_position, torrent_id",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, Vec<u8>>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    transaction.execute(
        "UPDATE torrents SET download_queue_position = NULL
         WHERE download_queue_position IS NOT NULL",
        [],
    )?;
    for (index, torrent_id) in ordered.iter().enumerate() {
        let index = i64::try_from(index)
            .map_err(|_| StoreError::DurableState("download queue is too large".to_owned()))?;
        let position = index
            .checked_mul(QUEUE_STRIDE)
            .ok_or_else(|| StoreError::DurableState("download queue is too large".to_owned()))?;
        transaction.execute(
            "UPDATE torrents SET download_queue_position = ?2 WHERE torrent_id = ?1",
            params![torrent_id, position],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use rstorrent_engine::TorrentId;
    use rusqlite::{Connection, params};

    use super::{QueueEdge, append, move_to_edge};

    fn database() -> Connection {
        let connection = Connection::open_in_memory().expect("database");
        connection
            .execute_batch(
                "CREATE TABLE torrents (
                    torrent_id BLOB PRIMARY KEY,
                    download_queue_position INTEGER
                 );
                 CREATE UNIQUE INDEX download_queue_order
                 ON torrents(download_queue_position)
                 WHERE download_queue_position IS NOT NULL;",
            )
            .expect("schema");
        connection
    }

    fn hash(value: u8) -> TorrentId {
        TorrentId::new([value; 16]).expect("nonzero owner")
    }

    fn positions(connection: &Connection) -> Vec<u8> {
        let mut statement = connection
            .prepare(
                "SELECT torrent_id FROM torrents
                 WHERE download_queue_position IS NOT NULL
                 ORDER BY download_queue_position, torrent_id",
            )
            .expect("query");
        statement
            .query_map([], |row| row.get::<_, Vec<u8>>(0))
            .expect("rows")
            .map(|row| row.expect("row")[0])
            .collect()
    }

    fn numbered_hash(value: u32) -> TorrentId {
        let mut torrent_id = [0; 16];
        torrent_id[..4].copy_from_slice(&value.to_be_bytes());
        torrent_id[15] = 1;
        TorrentId::new(torrent_id).expect("numbered owner")
    }

    fn numbered_positions(connection: &Connection) -> Vec<u32> {
        let mut statement = connection
            .prepare(
                "SELECT torrent_id FROM torrents
                 WHERE download_queue_position IS NOT NULL
                 ORDER BY download_queue_position, torrent_id",
            )
            .expect("query");
        statement
            .query_map([], |row| row.get::<_, Vec<u8>>(0))
            .expect("rows")
            .map(|row| {
                let info_hash = row.expect("row");
                u32::from_be_bytes(info_hash[..4].try_into().expect("four-byte prefix"))
            })
            .collect()
    }

    #[test]
    fn append_and_edge_moves_preserve_a_total_order() {
        let mut connection = database();
        for value in 1..=3 {
            connection
                .execute(
                    "INSERT INTO torrents(torrent_id) VALUES (?1)",
                    [hash(value).as_bytes().as_slice()],
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
                    "INSERT INTO torrents(torrent_id, download_queue_position)
                     VALUES (?1, ?2)",
                    params![hash(value).as_bytes().as_slice(), position],
                )
                .expect("insert");
        }
        connection
            .execute(
                "INSERT INTO torrents(torrent_id) VALUES (?1)",
                [hash(3).as_bytes().as_slice()],
            )
            .expect("insert");
        let transaction = connection.transaction().expect("transaction");
        assert!(append(&transaction, &hash(3)).expect("append"));
        transaction.commit().expect("commit");
        assert_eq!(positions(&connection), vec![1, 2, 3]);
    }

    #[test]
    fn thousand_entry_queue_matches_model_across_moves_and_rollbacks() {
        const ENTRY_COUNT: u32 = 1_000;
        const MOVE_COUNT: u32 = 2_000;

        let mut connection = database();
        let mut model = Vec::with_capacity(ENTRY_COUNT as usize);
        for value in 0..ENTRY_COUNT {
            let torrent_id = numbered_hash(value);
            connection
                .execute(
                    "INSERT INTO torrents(torrent_id) VALUES (?1)",
                    [torrent_id.as_bytes().as_slice()],
                )
                .expect("insert");
            let transaction = connection.transaction().expect("transaction");
            assert!(append(&transaction, &torrent_id).expect("append"));
            transaction.commit().expect("commit");
            model.push(value);
        }
        assert_eq!(numbered_positions(&connection), model);

        for step in 0..MOVE_COUNT {
            let value = (step.wrapping_mul(7_919).wrapping_add(17)) % ENTRY_COUNT;
            let edge = if step % 2 == 0 {
                QueueEdge::Top
            } else {
                QueueEdge::Bottom
            };
            let model_index = model
                .iter()
                .position(|candidate| *candidate == value)
                .expect("model value");
            model.remove(model_index);
            match edge {
                QueueEdge::Top => model.insert(0, value),
                QueueEdge::Bottom => model.push(value),
            }

            let transaction = connection.transaction().expect("transaction");
            move_to_edge(&transaction, &numbered_hash(value), edge).expect("move");
            transaction.commit().expect("commit");

            if step % 100 == 99 {
                assert_eq!(numbered_positions(&connection), model);

                let rolled_back_value = (value + 1) % ENTRY_COUNT;
                let transaction = connection.transaction().expect("transaction");
                move_to_edge(
                    &transaction,
                    &numbered_hash(rolled_back_value),
                    QueueEdge::Top,
                )
                .expect("uncommitted move");
                drop(transaction);
                assert_eq!(numbered_positions(&connection), model);
            }
        }

        assert_eq!(numbered_positions(&connection), model);
    }
}
