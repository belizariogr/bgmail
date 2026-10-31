//! Durable queues: server-side actions applied optimistically in the UI
//! (`pending_ops`) and messages waiting to be sent (`outbox`).

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::params;

use crate::types::{OutgoingRow, PendingOp};
use crate::Database;

/// Current time in unix seconds (0 if the clock is before the epoch).
pub(crate) fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

impl Database {
    /// Queues a server-side action; returns its id.
    pub fn enqueue_op(
        &self,
        account_id: i64,
        kind: &str,
        message_id: Option<i64>,
        payload: &str,
    ) -> rusqlite::Result<i64> {
        self.conn().execute(
            "INSERT INTO pending_ops (account_id, kind, message_id, payload, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![account_id, kind, message_id, payload, unix_now()],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    /// Queued actions of the account, oldest first (replay order).
    pub fn pending_ops(&self, account_id: i64) -> rusqlite::Result<Vec<PendingOp>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, account_id, kind, message_id, payload, attempts, last_error, created_at
             FROM pending_ops WHERE account_id = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![account_id], |row| {
            Ok(PendingOp {
                id: row.get(0)?,
                account_id: row.get(1)?,
                kind: row.get(2)?,
                message_id: row.get(3)?,
                payload: row.get(4)?,
                attempts: row.get(5)?,
                last_error: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        rows.collect()
    }

    /// Removes a successfully applied action.
    pub fn complete_op(&self, id: i64) -> rusqlite::Result<()> {
        self.conn()
            .execute("DELETE FROM pending_ops WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Records a failed attempt (`attempts + 1`, `last_error`); the op stays queued.
    pub fn fail_op(&self, id: i64, error: &str) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE pending_ops SET attempts = attempts + 1, last_error = ?1 WHERE id = ?2",
            params![error, id],
        )?;
        Ok(())
    }

    /// Drops every queued action that targets `message_id`.
    pub fn drop_ops_for_message(&self, message_id: i64) -> rusqlite::Result<()> {
        self.conn().execute(
            "DELETE FROM pending_ops WHERE message_id = ?1",
            params![message_id],
        )?;
        Ok(())
    }

    /// Queues a raw RFC 822 message for sending; returns its id.
    pub fn enqueue_outgoing(
        &self,
        account_id: i64,
        sender: &str,
        recipients: &[String],
        raw: &[u8],
        message_id_hdr: Option<&str>,
    ) -> rusqlite::Result<i64> {
        self.conn().execute(
            "INSERT INTO outbox (account_id, sender, recipients, raw, message_id_hdr, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                account_id,
                sender,
                recipients.join("\n"),
                raw,
                message_id_hdr,
                unix_now()
            ],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    /// Outgoing messages of the account, oldest first.
    pub fn outbox(&self, account_id: i64) -> rusqlite::Result<Vec<OutgoingRow>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, account_id, sender, recipients, raw, message_id_hdr, attempts,
                    last_error, created_at
             FROM outbox WHERE account_id = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![account_id], |row| {
            let recipients: String = row.get(3)?;
            Ok(OutgoingRow {
                id: row.get(0)?,
                account_id: row.get(1)?,
                sender: row.get(2)?,
                recipients: recipients
                    .lines()
                    .filter(|line| !line.is_empty())
                    .map(str::to_string)
                    .collect(),
                raw: row.get(4)?,
                message_id_hdr: row.get(5)?,
                attempts: row.get(6)?,
                last_error: row.get(7)?,
                created_at: row.get(8)?,
            })
        })?;
        rows.collect()
    }

    /// Outgoing messages across every account.
    pub fn outbox_count(&self) -> rusqlite::Result<usize> {
        let count: i64 = self
            .conn()
            .query_row("SELECT COUNT(*) FROM outbox", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Removes a sent message from the outbox.
    pub fn complete_outgoing(&self, id: i64) -> rusqlite::Result<()> {
        self.conn()
            .execute("DELETE FROM outbox WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Records a failed send (`attempts + 1`, `last_error`); the row stays queued.
    pub fn fail_outgoing(&self, id: i64, error: &str) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE outbox SET attempts = attempts + 1, last_error = ?1 WHERE id = ?2",
            params![error, id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::remote_db;

    #[test]
    fn unix_now_is_after_2020() {
        assert!(unix_now() > 1_577_836_800);
    }

    #[test]
    fn pending_ops_lifecycle() {
        let (_file, db, fx) = remote_db();
        let first = db
            .enqueue_op(fx.account, "move", Some(7), r#"{"to":"Trash"}"#)
            .unwrap();
        let second = db.enqueue_op(fx.account, "flag", None, "{}").unwrap();
        db.enqueue_op(fx.local_account, "other", None, "{}")
            .unwrap();

        let ops = db.pending_ops(fx.account).unwrap();
        assert_eq!(
            ops.iter().map(|op| op.id).collect::<Vec<_>>(),
            [first, second]
        );
        assert_eq!(ops[0].kind, "move");
        assert_eq!(ops[0].message_id, Some(7));
        assert_eq!(ops[0].payload, r#"{"to":"Trash"}"#);
        assert_eq!(ops[0].attempts, 0);
        assert!(ops[0].created_at > 0);

        db.fail_op(first, "timeout").unwrap();
        db.fail_op(first, "refused").unwrap();
        let op = &db.pending_ops(fx.account).unwrap()[0];
        assert_eq!(op.attempts, 2);
        assert_eq!(op.last_error.as_deref(), Some("refused"));

        db.complete_op(second).unwrap();
        assert_eq!(db.pending_ops(fx.account).unwrap().len(), 1);

        db.drop_ops_for_message(7).unwrap();
        assert!(db.pending_ops(fx.account).unwrap().is_empty());
        assert_eq!(db.pending_ops(fx.local_account).unwrap().len(), 1);
    }

    #[test]
    fn outbox_lifecycle() {
        let (_file, db, fx) = remote_db();
        let recipients = vec!["a@x.com".to_string(), "b@x.com".to_string()];
        let first = db
            .enqueue_outgoing(fx.account, "me@x.com", &recipients, b"RAW1", Some("<id@x>"))
            .unwrap();
        let second = db
            .enqueue_outgoing(fx.account, "me@x.com", &[], b"RAW2", None)
            .unwrap();
        assert_eq!(db.outbox_count().unwrap(), 2);

        let rows = db.outbox(fx.account).unwrap();
        assert_eq!(rows[0].id, first);
        assert_eq!(rows[0].recipients, recipients);
        assert_eq!(rows[0].raw, b"RAW1");
        assert_eq!(rows[0].message_id_hdr.as_deref(), Some("<id@x>"));
        assert!(rows[1].recipients.is_empty());
        assert!(db.outbox(fx.local_account).unwrap().is_empty());

        db.fail_outgoing(first, "550").unwrap();
        let row = &db.outbox(fx.account).unwrap()[0];
        assert_eq!(row.attempts, 1);
        assert_eq!(row.last_error.as_deref(), Some("550"));

        db.complete_outgoing(second).unwrap();
        db.complete_outgoing(first).unwrap();
        assert_eq!(db.outbox_count().unwrap(), 0);
    }
}
