//! Keeps the denormalized `folders_csv` in step with server locations.
//!
//! Rule: a message with at least one row in `message_locations` belongs exactly
//! to the paths of those folders (+ `sys:flagged` when starred). A message with
//! no locations (local/mock mail, or a remote message in flight after a move)
//! keeps whatever `folders_csv` the UI last wrote.

use rusqlite::{params, Connection};

use crate::folder::{folders_csv, system};
use crate::types::PROVIDER_LOCAL;
use crate::Database;

/// Recomputes `folders_csv` of `message_id` from its locations (no-op when it
/// has none or does not exist).
pub(crate) fn refresh_membership_in(conn: &Connection, message_id: i64) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare_cached(
        "SELECT f.path FROM message_locations l
         JOIN folders f ON f.id = l.folder_id
         WHERE l.message_id = ?1",
    )?;
    let mut paths = stmt
        .query_map(params![message_id], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if paths.is_empty() {
        return Ok(());
    }
    let starred: i64 = conn.query_row(
        "SELECT starred FROM messages WHERE id = ?1",
        params![message_id],
        |row| row.get(0),
    )?;
    if starred != 0 {
        paths.push(system::FLAGGED.to_string());
    }
    paths.sort();
    paths.dedup();
    conn.execute(
        "UPDATE messages SET folders_csv = ?1 WHERE id = ?2",
        params![folders_csv(paths.iter().map(String::as_str)), message_id],
    )?;
    Ok(())
}

/// After locations were removed: deletes messages of non-local accounts that
/// have no location left (they no longer exist on the server) and refreshes
/// membership of the rest. Local accounts never lose mail this way.
pub(crate) fn settle_after_location_loss(
    conn: &Connection,
    message_ids: &[i64],
) -> rusqlite::Result<()> {
    for &message_id in message_ids {
        let deleted = conn.execute(
            "DELETE FROM messages
             WHERE id = ?1
               AND NOT EXISTS (SELECT 1 FROM message_locations WHERE message_id = ?1)
               AND account_id IN (SELECT id FROM accounts WHERE provider != ?2)",
            params![message_id, PROVIDER_LOCAL],
        )?;
        if deleted == 0 {
            refresh_membership_in(conn, message_id)?;
        }
    }
    Ok(())
}

/// Ids of messages with a location in `folder_id`.
pub(crate) fn message_ids_in_folder(
    conn: &Connection,
    folder_id: i64,
) -> rusqlite::Result<Vec<i64>> {
    let mut stmt =
        conn.prepare("SELECT DISTINCT message_id FROM message_locations WHERE folder_id = ?1")?;
    let ids = stmt.query_map(params![folder_id], |row| row.get(0))?;
    ids.collect()
}

impl Database {
    /// Recomputes `folders_csv` from the message's server locations; messages
    /// without locations keep their current membership.
    pub fn refresh_membership(&self, message_id: i64) -> rusqlite::Result<()> {
        refresh_membership_in(self.conn(), message_id)
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::{remote_db, remote_message};
    use crate::{system, Database};

    fn csv(db: &Database, id: i64) -> String {
        db.get_message(id).unwrap().unwrap().folders_csv
    }

    #[test]
    fn membership_follows_locations_and_starred() {
        let (_file, db, fx) = remote_db();
        let id = db
            .insert_remote_message(fx.account, fx.inbox, 1, &remote_message("a", Some(1)))
            .unwrap();
        assert_eq!(csv(&db, id), ",sys:inbox,");

        db.insert_remote_message(fx.account, fx.archive, 9, &remote_message("a", Some(1)))
            .unwrap();
        db.conn()
            .execute("UPDATE messages SET starred = 1 WHERE id = ?1", [id])
            .unwrap();
        db.refresh_membership(id).unwrap();
        assert_eq!(csv(&db, id), ",sys:archive,sys:flagged,sys:inbox,");
    }

    #[test]
    fn membership_without_locations_is_left_alone() {
        let (_file, db, fx) = remote_db();
        let id = db
            .insert_remote_message(fx.account, fx.inbox, 1, &remote_message("a", None))
            .unwrap();
        db.detach_locations(id).unwrap();
        db.move_message_to_folder(id, system::ARCHIVE).unwrap();
        db.refresh_membership(id).unwrap();
        assert_eq!(csv(&db, id), ",sys:archive,");
    }

    #[test]
    fn refresh_membership_of_missing_message_is_noop() {
        let (_file, db, _fx) = remote_db();
        db.refresh_membership(9999).unwrap();
    }
}
