//! Durable queues: server-side actions applied optimistically in the UI
//! (`pending_ops`) and messages waiting to be sent (`outbox`).

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, OptionalExtension};

use crate::types::{OutgoingRow, OutgoingState, PendingOp};
use crate::Database;

/// Current time in unix seconds (0 if the clock is before the epoch).
/// A queued row counts as stuck after this long without leaving.
const STUCK_AFTER_SECS: i64 = 5 * 60;
/// When a row's age starts: its creation, or the user's last retry. 0 for rows from
/// before `created_at` was recorded.
const OUTGOING_AGE_FROM: &str = "MAX(created_at, retry_from)";
/// SQL for a queued row stuck in the queue; `?1` is [`STUCK_AFTER_SECS`] ago.
const STUCK_IN_QUEUE: &str =
    "state = 'queued' AND (deferrals > 0 OR MAX(created_at, retry_from) <= ?1)";
/// [`STUCK_IN_QUEUE`] with its parameter at `?3`.
const STUCK_IN_QUEUE_2: &str =
    "state = 'queued' AND (deferrals > 0 OR MAX(created_at, retry_from) <= ?3)";

pub(crate) fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

/// Columns read by [`op_from_row`].
const OP_COLUMNS: &str =
    "id, account_id, kind, message_id, payload, attempts, last_error, created_at";

fn op_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PendingOp> {
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
        let mut stmt = self.conn().prepare(&format!(
            "SELECT {OP_COLUMNS} FROM pending_ops WHERE account_id = ?1 ORDER BY id ASC"
        ))?;
        let rows = stmt.query_map(params![account_id], op_from_row)?;
        rows.collect()
    }

    /// The queued action `id` as it is now (`None` once completed/dropped).
    pub fn pending_op(&self, id: i64) -> rusqlite::Result<Option<PendingOp>> {
        self.conn()
            .query_row(
                &format!("SELECT {OP_COLUMNS} FROM pending_ops WHERE id = ?1"),
                params![id],
                op_from_row,
            )
            .optional()
    }

    /// Removes a successfully applied action.
    pub fn complete_op(&self, id: i64) -> rusqlite::Result<()> {
        self.conn()
            .execute("DELETE FROM pending_ops WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Removes an applied action only if it still has the `kind` / `payload`
    /// the worker executed. Returns `false` when the row was rewritten
    /// (`update_op`) or removed meanwhile: the caller must then not apply the
    /// executed action's local result, because the UI changed its intent.
    pub fn complete_op_if_unchanged(
        &self,
        id: i64,
        kind: &str,
        payload: &str,
    ) -> rusqlite::Result<bool> {
        let deleted = self.conn().execute(
            "DELETE FROM pending_ops WHERE id = ?1 AND kind = ?2 AND payload = ?3",
            params![id, kind, payload],
        )?;
        Ok(deleted == 1)
    }

    /// Records a failed attempt (`attempts + 1`, `last_error`); the op stays queued.
    pub fn fail_op(&self, id: i64, error: &str) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE pending_ops SET attempts = attempts + 1, last_error = ?1 WHERE id = ?2",
            params![error, id],
        )?;
        Ok(())
    }

    /// Rewrites a queued action in place (keeps its id, so replay order and
    /// attempts are preserved). Used when a later UI action or a completed
    /// move changes what an older op must do.
    pub fn update_op(&self, id: i64, kind: &str, payload: &str) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE pending_ops SET kind = ?1, payload = ?2 WHERE id = ?3",
            params![kind, payload, id],
        )?;
        Ok(())
    }

    /// Compare-and-swap rewrite: replaces `kind` / `payload` only while the
    /// row still holds `old_kind` / `old_payload` (keeps id and attempts).
    /// Returns `false` when another connection changed or removed it first,
    /// so a stale read never overwrites a newer UI intent.
    pub fn update_op_if_unchanged(
        &self,
        id: i64,
        old_kind: &str,
        old_payload: &str,
        kind: &str,
        payload: &str,
    ) -> rusqlite::Result<bool> {
        let changed = self.conn().execute(
            "UPDATE pending_ops SET kind = ?1, payload = ?2
             WHERE id = ?3 AND kind = ?4 AND payload = ?5",
            params![kind, payload, id, old_kind, old_payload],
        )?;
        Ok(changed == 1)
    }

    /// Drops every queued action that targets `message_id`.
    pub fn drop_ops_for_message(&self, message_id: i64) -> rusqlite::Result<()> {
        self.conn().execute(
            "DELETE FROM pending_ops WHERE message_id = ?1",
            params![message_id],
        )?;
        Ok(())
    }

    /// Queues a raw RFC 822 message for sending (state `queued`, due now);
    /// returns its id.
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

    /// Queues `raw` in place of the stopped row `old_id`, atomically, so an
    /// edited message and its original are never both on their way. Returns
    /// `None` (nothing queued) while the original is back on the send path
    /// after a retry. A row that is gone (discarded, delivered) doesn't block.
    pub fn replace_outgoing(
        &self,
        old_id: i64,
        account_id: i64,
        sender: &str,
        recipients: &[String],
        raw: &[u8],
        message_id_hdr: Option<&str>,
    ) -> rusqlite::Result<Option<i64>> {
        let tx = self.write_transaction()?;
        let state: Option<String> = self
            .conn()
            .query_row(
                "SELECT state FROM outbox WHERE id = ?1",
                params![old_id],
                |row| row.get(0),
            )
            .optional()?;
        if state.is_some_and(|state| !OutgoingState::parse(&state).is_stopped()) {
            return Ok(None);
        }
        self.complete_outgoing(old_id)?;
        let id = self.enqueue_outgoing(account_id, sender, recipients, raw, message_id_hdr)?;
        tx.commit()?;
        Ok(Some(id))
    }

    /// Every outbox row of the account (any state), oldest first.
    pub fn outbox(&self, account_id: i64) -> rusqlite::Result<Vec<OutgoingRow>> {
        self.query_outgoing("raw", "WHERE account_id = ?1", params![account_id])
    }

    /// One outbox row, with its raw bytes.
    pub fn outgoing(&self, id: i64) -> rusqlite::Result<Option<OutgoingRow>> {
        Ok(self
            .query_outgoing("raw", "WHERE id = ?1", params![id])?
            .pop())
    }

    /// Rows the worker should act on now: `queued` or `append_pending` rows
    /// whose `next_attempt_at <= now`, oldest first.
    pub fn due_outgoing(&self, account_id: i64, now: i64) -> rusqlite::Result<Vec<OutgoingRow>> {
        self.query_outgoing(
            "raw",
            "WHERE account_id = ?1 AND state IN ('queued', 'append_pending')
               AND next_attempt_at <= ?2",
            params![account_id, now],
        )
    }

    /// When the earliest backed-off row of the account is due (unix seconds).
    pub fn next_outgoing_due(&self, account_id: i64, now: i64) -> rusqlite::Result<Option<i64>> {
        self.conn().query_row(
            "SELECT MIN(next_attempt_at) FROM outbox
             WHERE account_id = ?1 AND state IN ('queued', 'append_pending')
               AND next_attempt_at > ?2",
            params![account_id, now],
            |row| row.get(0),
        )
    }

    /// Rows the user should know about, across every account, oldest first: those
    /// that stopped retrying (`failed`, `maybe_sent`) and those stuck in the queue,
    /// which could otherwise go out days later with no way to stop them: after a
    /// failure, or left for a few minutes (a worker offline never tries them at all).
    /// `raw` is left empty: the UI lists these on every reload and attachments can
    /// be large, so it gets the bytes from [`Self::outgoing`] only when needed.
    pub fn unsent_outgoing(&self) -> rusqlite::Result<Vec<OutgoingRow>> {
        self.query_outgoing(
            "X''",
            &format!("WHERE state IN ('failed', 'maybe_sent') OR ({STUCK_IN_QUEUE})"),
            params![unix_now() - STUCK_AFTER_SECS],
        )
    }

    /// Stops the queued rows of the account older than `max_age_secs`, counted from
    /// their creation or the user's last retry: what the author wanted sent then may
    /// no longer be what they want now. Returns the ids and errors of the stopped rows.
    pub fn give_up_outgoing(
        &self,
        account_id: i64,
        now: i64,
        max_age_secs: i64,
    ) -> rusqlite::Result<Vec<(i64, Option<String>)>> {
        let mut stmt = self.conn().prepare(&format!(
            "UPDATE outbox SET state = 'failed', lease_at = NULL
             WHERE account_id = ?1 AND state = 'queued'
               AND {OUTGOING_AGE_FROM} > 0 AND {OUTGOING_AGE_FROM} <= ?2
             RETURNING id, last_error"
        ))?;
        let rows = stmt.query_map(params![account_id, now - max_age_secs], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        rows.collect()
    }

    /// Stops the automatic retries of a queued row: it becomes `failed`, waiting for
    /// the user. `false` for any other state, such as one being sent right now.
    pub fn stop_outgoing(&self, id: i64) -> rusqlite::Result<bool> {
        let changed = self.conn().execute(
            "UPDATE outbox SET state = 'failed', lease_at = NULL
             WHERE id = ?1 AND state = 'queued'",
            params![id],
        )?;
        Ok(changed == 1)
    }

    /// `raw_column` is the SQL for the `raw` field: the column, or an empty
    /// blob for listings.
    fn query_outgoing(
        &self,
        raw_column: &str,
        filter: &str,
        args: impl rusqlite::Params,
    ) -> rusqlite::Result<Vec<OutgoingRow>> {
        let mut stmt = self.conn().prepare(&format!(
            "SELECT id, account_id, sender, recipients, {raw_column}, message_id_hdr, attempts,
                    last_error, created_at, state, next_attempt_at, deferrals
             FROM outbox {filter} ORDER BY id ASC"
        ))?;
        let rows = stmt.query_map(args, |row| {
            let recipients: String = row.get(3)?;
            let state: String = row.get(9)?;
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
                state: OutgoingState::parse(&state),
                next_attempt_at: row.get(10)?,
                deferrals: row.get(11)?,
            })
        })?;
        rows.collect()
    }

    /// Messages still waiting to be sent (`queued` or `sending`), across
    /// every account.
    pub fn outbox_count(&self) -> rusqlite::Result<usize> {
        let count: i64 = self.conn().query_row(
            "SELECT COUNT(*) FROM outbox WHERE state IN ('queued', 'sending')",
            [],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Atomically moves a `queued` row to `sending` with a lease stamped
    /// `now`. Returns `false` when the row is gone or not `queued` (another
    /// worker claimed it, or the user cancelled it), so it must not be sent.
    pub fn claim_outgoing(&self, id: i64, now: i64) -> rusqlite::Result<bool> {
        let changed = self.conn().execute(
            "UPDATE outbox SET state = 'sending', lease_at = ?2
             WHERE id = ?1 AND state = 'queued'",
            params![id, now],
        )?;
        Ok(changed == 1)
    }

    /// Turns `sending` rows of the account whose lease is older than
    /// `cutoff` into `maybe_sent`: their worker died mid-transaction, so the
    /// server may have accepted them. Returns the ids of the changed rows.
    pub fn expire_outgoing_leases(
        &self,
        account_id: i64,
        cutoff: i64,
    ) -> rusqlite::Result<Vec<i64>> {
        let mut stmt = self.conn().prepare(
            "UPDATE outbox SET state = 'maybe_sent', lease_at = NULL,
                    last_error = COALESCE(last_error, 'interrupted while sending')
             WHERE account_id = ?1 AND state = 'sending' AND COALESCE(lease_at, 0) < ?2
             RETURNING id",
        )?;
        let rows = stmt.query_map(params![account_id, cutoff], |row| row.get(0))?;
        rows.collect()
    }

    /// Removes a sent (and filed) message from the outbox.
    pub fn complete_outgoing(&self, id: i64) -> rusqlite::Result<()> {
        self.conn()
            .execute("DELETE FROM outbox WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// The SMTP server accepted the row but the Sent copy still has to be
    /// appended: keeps the raw bytes in state `append_pending`, due now.
    pub fn mark_outgoing_append_pending(&self, id: i64) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE outbox SET state = 'append_pending', lease_at = NULL,
                    next_attempt_at = 0, deferrals = 0, last_error = NULL
             WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// Records a transient failure (`deferrals + 1`, `last_error`) and
    /// schedules the next try at `next_attempt_at`. `sending` rows go back to
    /// `queued`; `append_pending` rows keep their state. `attempts` (the
    /// permanent-failure budget) is untouched.
    pub fn defer_outgoing(
        &self,
        id: i64,
        error: &str,
        next_attempt_at: i64,
    ) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE outbox SET deferrals = deferrals + 1, last_error = ?2,
                    next_attempt_at = ?3, lease_at = NULL,
                    state = CASE WHEN state = 'append_pending' THEN state ELSE 'queued' END
             WHERE id = ?1",
            params![id, error, next_attempt_at],
        )?;
        Ok(())
    }

    /// Records a permanent rejection (`attempts + 1`, `last_error`). Once
    /// `attempts` reaches `max_attempts` the row becomes `failed` (no more
    /// automatic retries); otherwise it is `queued` again for
    /// `next_attempt_at`. Returns the resulting state (`Queued` / `Failed`).
    pub fn fail_outgoing(
        &self,
        id: i64,
        error: &str,
        next_attempt_at: i64,
        max_attempts: u32,
    ) -> rusqlite::Result<OutgoingState> {
        self.conn().execute(
            "UPDATE outbox SET attempts = attempts + 1, last_error = ?2,
                    next_attempt_at = ?3, lease_at = NULL,
                    state = CASE WHEN attempts + 1 >= ?4 THEN 'failed' ELSE 'queued' END
             WHERE id = ?1",
            params![id, error, next_attempt_at, max_attempts],
        )?;
        let state: Option<String> = self
            .conn()
            .query_row(
                "SELECT state FROM outbox WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(state.map_or(OutgoingState::Failed, |state| OutgoingState::parse(&state)))
    }

    /// Marks a row `maybe_sent`: the transaction may have been accepted, so
    /// it is never resent automatically (see `unsent_outgoing`).
    pub fn mark_outgoing_maybe_sent(&self, id: i64, error: &str) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE outbox SET state = 'maybe_sent', lease_at = NULL, last_error = ?2
             WHERE id = ?1",
            params![id, error],
        )?;
        Ok(())
    }

    /// User-requested retry of a row in [`Self::unsent_outgoing`]: resets attempts,
    /// deferrals, backoff, lease and error, and puts it back to `queued` so the
    /// engine sends it on its next cycle. Returns `false` for any other state, so a
    /// stale click can't disturb a row already being sent.
    pub fn retry_outgoing(&self, id: i64) -> rusqlite::Result<bool> {
        let changed = self.conn().execute(
            &format!(
                "UPDATE outbox SET state = 'queued', attempts = 0, deferrals = 0,
                        next_attempt_at = 0, lease_at = NULL, last_error = NULL, retry_from = ?2
                 WHERE id = ?1 AND (state IN ('failed', 'maybe_sent') OR ({STUCK_IN_QUEUE_2}))"
            ),
            params![id, unix_now(), unix_now() - STUCK_AFTER_SECS],
        )?;
        Ok(changed == 1)
    }

    /// User-requested discard of an outbox row. `false` when it can't be discarded:
    /// it is being sent, or the server accepted it and only its Sent copy is pending.
    pub fn delete_outgoing(&self, id: i64) -> rusqlite::Result<bool> {
        let changed = self.conn().execute(
            "DELETE FROM outbox WHERE id = ?1 AND state NOT IN ('sending', 'append_pending')",
            params![id],
        )?;
        Ok(changed == 1)
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

        db.update_op(first, "delete", r#"{"x":1}"#).unwrap();
        let op = &db.pending_ops(fx.account).unwrap()[0];
        assert_eq!((op.id, op.kind.as_str()), (first, "delete"));
        assert_eq!(op.payload, r#"{"x":1}"#);
        assert_eq!(op.attempts, 2, "update keeps the attempt count");

        assert_eq!(db.pending_op(second).unwrap().unwrap().kind, "flag");
        db.complete_op(second).unwrap();
        assert_eq!(db.pending_op(second).unwrap(), None);
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

        assert!(rows.iter().all(|row| row.state == OutgoingState::Queued));

        assert_eq!(
            db.fail_outgoing(first, "550", 0, 3).unwrap(),
            OutgoingState::Queued
        );
        let row = &db.outbox(fx.account).unwrap()[0];
        assert_eq!(row.attempts, 1);
        assert_eq!(row.last_error.as_deref(), Some("550"));

        db.complete_outgoing(second).unwrap();
        db.complete_outgoing(first).unwrap();
        assert_eq!(db.outbox_count().unwrap(), 0);
    }

    #[test]
    fn replace_outgoing_only_replaces_stopped_or_missing_rows() {
        let (_file, db, fx) = remote_db();
        let replace = |old| {
            db.replace_outgoing(old, fx.account, "me@x", &[], b"NEW", None)
                .unwrap()
        };
        let id = db
            .enqueue_outgoing(fx.account, "me@x", &[], b"OLD", None)
            .unwrap();
        assert_eq!(replace(id), None, "queued original stays");
        db.claim_outgoing(id, 1).unwrap();
        assert_eq!(replace(id), None, "being sent");
        assert_eq!(db.outbox(fx.account).unwrap().len(), 1, "nothing queued");

        db.mark_outgoing_maybe_sent(id, "reset").unwrap();
        let new = replace(id).unwrap();
        let rows = db.outbox(fx.account).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            (rows[0].id, rows[0].raw.as_slice()),
            (new, b"NEW".as_slice())
        );

        db.delete_outgoing(new).unwrap();
        assert!(replace(new).is_some(), "a discarded original doesn't block");
    }

    #[test]
    fn a_retrying_row_is_listed_and_can_be_stopped_or_discarded() {
        let (_file, db, fx) = remote_db();
        let id = db
            .enqueue_outgoing(fx.account, "a@x", &["b@x".into()], b"r", None)
            .unwrap();
        assert!(db.unsent_outgoing().unwrap().is_empty());

        db.defer_outgoing(id, "connect", 9_999_999_999).unwrap();
        assert_eq!(db.unsent_outgoing().unwrap()[0].id, id);

        assert!(db.retry_outgoing(id).unwrap());
        assert!(db.unsent_outgoing().unwrap().is_empty());
        db.defer_outgoing(id, "connect", 9_999_999_999).unwrap();

        assert!(db.stop_outgoing(id).unwrap());
        assert_eq!(
            db.outgoing(id).unwrap().unwrap().state,
            OutgoingState::Failed
        );
        assert!(!db.stop_outgoing(id).unwrap());
        assert!(db.delete_outgoing(id).unwrap());
        assert!(db.outgoing(id).unwrap().is_none());
    }

    #[test]
    fn a_row_left_in_the_queue_for_minutes_is_listed_and_can_be_retried() {
        let (_file, db, fx) = remote_db();
        let id = db
            .enqueue_outgoing(fx.account, "a@x", &["b@x".into()], b"r", None)
            .unwrap();
        assert!(db.unsent_outgoing().unwrap().is_empty());
        assert!(!db.retry_outgoing(id).unwrap());

        db.conn()
            .execute(
                "UPDATE outbox SET created_at = ?1 WHERE id = ?2",
                params![unix_now() - STUCK_AFTER_SECS, id],
            )
            .unwrap();
        assert_eq!(db.unsent_outgoing().unwrap()[0].id, id);

        // A retry starts the clock again, and the row waits for its result.
        assert!(db.retry_outgoing(id).unwrap());
        assert!(db.unsent_outgoing().unwrap().is_empty());
    }

    #[test]
    fn old_queued_rows_are_given_up_counting_from_the_last_retry() {
        let (_file, db, fx) = remote_db();
        let now = unix_now();
        let queue = || {
            db.enqueue_outgoing(fx.account, "a@x", &["b@x".into()], b"r", None)
                .unwrap()
        };
        let (old, retried, recent, unknown, other) = (queue(), queue(), queue(), queue(), queue());
        for (id, created, retry_from) in [
            (old, now - 100, 0),
            (retried, now - 100, now - 10),
            (recent, now - 10, 0),
            (unknown, 0, 0),
            (other, now - 100, 0),
        ] {
            db.conn()
                .execute(
                    "UPDATE outbox SET created_at = ?1, retry_from = ?2 WHERE id = ?3",
                    params![created, retry_from, id],
                )
                .unwrap();
        }
        db.conn()
            .execute("UPDATE outbox SET state = 'sending' WHERE id = ?1", [other])
            .unwrap();
        let stopped = db.give_up_outgoing(fx.account, now, 50).unwrap();
        assert_eq!(stopped, [(old, None)]);
        assert_eq!(
            db.outgoing(old).unwrap().unwrap().state,
            OutgoingState::Failed
        );
        assert_eq!(
            db.outgoing(retried).unwrap().unwrap().state,
            OutgoingState::Queued
        );
    }

    #[test]
    fn a_row_being_sent_cannot_be_stopped_or_discarded() {
        let (_file, db, fx) = remote_db();
        let id = db
            .enqueue_outgoing(fx.account, "a@x", &["b@x".into()], b"r", None)
            .unwrap();
        db.conn()
            .execute("UPDATE outbox SET state = 'sending' WHERE id = ?1", [id])
            .unwrap();
        assert!(!db.stop_outgoing(id).unwrap());
        assert!(!db.delete_outgoing(id).unwrap());
        assert!(db.outgoing(id).unwrap().is_some());
    }

    #[test]
    fn retry_outgoing_only_touches_stopped_rows() {
        let (_file, db, fx) = remote_db();
        let id = db
            .enqueue_outgoing(fx.account, "me@x", &[], b"A", None)
            .unwrap();
        db.claim_outgoing(id, 1).unwrap();
        assert!(!db.retry_outgoing(id).unwrap());
        assert_eq!(
            db.outgoing(id).unwrap().unwrap().state,
            OutgoingState::Sending
        );
        db.mark_outgoing_maybe_sent(id, "reset").unwrap();
        assert!(db.retry_outgoing(id).unwrap());
        assert_eq!(
            db.outgoing(id).unwrap().unwrap().state,
            OutgoingState::Queued
        );
        assert!(!db.retry_outgoing(i64::MAX).unwrap());
    }

    #[test]
    fn stopped_listing_leaves_raw_out_and_outgoing_brings_it() {
        let (_file, db, fx) = remote_db();
        let id = db
            .enqueue_outgoing(fx.account, "me@x", &["a@x".into()], b"RAW", None)
            .unwrap();
        db.fail_outgoing(id, "550", 0, 1).unwrap();
        let listed = db.unsent_outgoing().unwrap();
        assert_eq!((listed[0].id, listed[0].raw.len()), (id, 0));
        assert_eq!(listed[0].recipients, ["a@x"]);
        assert_eq!(db.outgoing(id).unwrap().unwrap().raw, b"RAW");
        assert_eq!(db.outgoing(i64::MAX).unwrap(), None);
    }

    #[test]
    fn next_outgoing_due_is_the_earliest_future_retry() {
        let (_file, db, fx) = remote_db();
        assert_eq!(db.next_outgoing_due(fx.account, 0).unwrap(), None);
        let a = db
            .enqueue_outgoing(fx.account, "me@x", &[], b"A", None)
            .unwrap();
        let b = db
            .enqueue_outgoing(fx.account, "me@x", &[], b"B", None)
            .unwrap();
        assert_eq!(
            db.next_outgoing_due(fx.account, 0).unwrap(),
            None,
            "due now"
        );
        db.defer_outgoing(a, "421", 500).unwrap();
        db.defer_outgoing(b, "421", 200).unwrap();
        assert_eq!(db.next_outgoing_due(fx.account, 0).unwrap(), Some(200));
        assert_eq!(db.next_outgoing_due(fx.account, 200).unwrap(), Some(500));
        db.fail_outgoing(a, "550", 900, 1).unwrap();
        assert_eq!(
            db.next_outgoing_due(fx.account, 200).unwrap(),
            None,
            "stopped"
        );
    }

    #[test]
    fn atomically_rolls_back_on_error() {
        let (_file, db, fx) = remote_db();
        let result: rusqlite::Result<()> = db.atomically(|| {
            db.enqueue_op(fx.account, "flag", Some(1), "{}")?;
            Err(rusqlite::Error::QueryReturnedNoRows)
        });
        assert!(result.is_err());
        assert!(db.pending_ops(fx.account).unwrap().is_empty());
        let id = db
            .atomically(|| db.enqueue_op(fx.account, "flag", Some(1), "{}"))
            .unwrap();
        assert_eq!(db.pending_ops(fx.account).unwrap()[0].id, id);
    }

    #[test]
    fn complete_op_if_unchanged_detects_rewrites_and_removals() {
        let (_file, db, fx) = remote_db();
        let id = db.enqueue_op(fx.account, "move", Some(1), "A").unwrap();
        db.update_op(id, "move", "B").unwrap();
        assert!(!db.complete_op_if_unchanged(id, "move", "A").unwrap());
        assert_eq!(db.pending_ops(fx.account).unwrap().len(), 1);
        assert!(!db.complete_op_if_unchanged(id, "delete", "B").unwrap());
        assert!(db.complete_op_if_unchanged(id, "move", "B").unwrap());
        assert!(db.pending_ops(fx.account).unwrap().is_empty());
        assert!(!db.complete_op_if_unchanged(id, "move", "B").unwrap());
    }

    #[test]
    fn update_op_if_unchanged_is_a_compare_and_swap() {
        let (_file, db, fx) = remote_db();
        let id = db.enqueue_op(fx.account, "move", Some(1), "A").unwrap();
        assert!(db
            .update_op_if_unchanged(id, "move", "A", "delete", "B")
            .unwrap());
        assert!(
            !db.update_op_if_unchanged(id, "move", "A", "move", "C")
                .unwrap(),
            "stale read loses"
        );
        let op = &db.pending_ops(fx.account).unwrap()[0];
        assert_eq!((op.kind.as_str(), op.payload.as_str()), ("delete", "B"));
        db.complete_op(id).unwrap();
        assert!(!db
            .update_op_if_unchanged(id, "delete", "B", "move", "C")
            .unwrap());
    }

    #[test]
    fn due_outgoing_respects_backoff_and_state() {
        let (_file, db, fx) = remote_db();
        let a = db
            .enqueue_outgoing(fx.account, "me@x", &[], b"A", None)
            .unwrap();
        let b = db
            .enqueue_outgoing(fx.account, "me@x", &[], b"B", None)
            .unwrap();
        let c = db
            .enqueue_outgoing(fx.account, "me@x", &[], b"C", None)
            .unwrap();
        db.defer_outgoing(a, "421 busy", 1_000).unwrap();
        db.mark_outgoing_append_pending(c).unwrap();
        let ids = |now| {
            db.due_outgoing(fx.account, now)
                .unwrap()
                .iter()
                .map(|row| row.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(999), [b, c]);
        assert_eq!(ids(1_000), [a, b, c]);
        let row = &db.outbox(fx.account).unwrap()[0];
        assert_eq!((row.deferrals, row.attempts), (1, 0));
        assert_eq!(row.next_attempt_at, 1_000);
        assert_eq!(row.last_error.as_deref(), Some("421 busy"));

        // Deferring an append keeps it an append.
        db.defer_outgoing(c, "NO", 0).unwrap();
        assert_eq!(
            db.outbox(fx.account).unwrap()[2].state,
            OutgoingState::AppendPending
        );
        // Append-pending rows are already sent: not counted as waiting.
        assert_eq!(db.outbox_count().unwrap(), 2);
        assert!(db
            .due_outgoing(fx.local_account, i64::MAX)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn claim_outgoing_is_exclusive_and_leases_expire_to_maybe_sent() {
        let (_file, db, fx) = remote_db();
        let id = db
            .enqueue_outgoing(fx.account, "me@x", &[], b"A", None)
            .unwrap();
        assert!(db.claim_outgoing(id, 100).unwrap());
        assert!(!db.claim_outgoing(id, 101).unwrap(), "second claim loses");
        assert_eq!(
            db.outbox(fx.account).unwrap()[0].state,
            OutgoingState::Sending
        );
        assert!(db.due_outgoing(fx.account, i64::MAX).unwrap().is_empty());
        assert_eq!(db.outbox_count().unwrap(), 1);

        assert!(db
            .expire_outgoing_leases(fx.account, 100)
            .unwrap()
            .is_empty());
        assert!(db
            .expire_outgoing_leases(fx.local_account, 200)
            .unwrap()
            .is_empty());
        assert_eq!(db.expire_outgoing_leases(fx.account, 101).unwrap(), [id]);
        let row = &db.unsent_outgoing().unwrap()[0];
        assert_eq!(row.state, OutgoingState::MaybeSent);
        assert_eq!(row.last_error.as_deref(), Some("interrupted while sending"));
        assert!(!db.claim_outgoing(id, 102).unwrap());
    }

    #[test]
    fn fail_outgoing_reaches_failed_after_max_attempts() {
        let (_file, db, fx) = remote_db();
        let id = db
            .enqueue_outgoing(fx.account, "me@x", &[], b"A", None)
            .unwrap();
        assert!(db.claim_outgoing(id, 1).unwrap());
        assert_eq!(
            db.fail_outgoing(id, "550", 60, 2).unwrap(),
            OutgoingState::Queued
        );
        let row = &db.outbox(fx.account).unwrap()[0];
        assert_eq!((row.attempts, row.next_attempt_at), (1, 60));
        assert_eq!(
            db.fail_outgoing(id, "551", 0, 2).unwrap(),
            OutgoingState::Failed
        );
        assert!(db.due_outgoing(fx.account, i64::MAX).unwrap().is_empty());
        assert_eq!(db.outbox_count().unwrap(), 0);
        assert_eq!(
            db.fail_outgoing(9_999, "x", 0, 1).unwrap(),
            OutgoingState::Failed
        );
    }

    #[test]
    fn unsent_outgoing_retry_and_delete() {
        let (_file, db, fx) = remote_db();
        let failed = db
            .enqueue_outgoing(fx.account, "me@x", &[], b"A", None)
            .unwrap();
        let maybe = db
            .enqueue_outgoing(fx.local_account, "me@y", &[], b"B", None)
            .unwrap();
        let queued = db
            .enqueue_outgoing(fx.account, "me@x", &[], b"C", None)
            .unwrap();
        db.defer_outgoing(failed, "421", 50).unwrap();
        db.fail_outgoing(failed, "550", 99, 1).unwrap();
        db.mark_outgoing_maybe_sent(maybe, "connection reset")
            .unwrap();

        let stuck = db.unsent_outgoing().unwrap();
        assert_eq!(
            stuck
                .iter()
                .map(|row| (row.id, row.state))
                .collect::<Vec<_>>(),
            [
                (failed, OutgoingState::Failed),
                (maybe, OutgoingState::MaybeSent)
            ]
        );
        assert_eq!(stuck[1].last_error.as_deref(), Some("connection reset"));

        db.retry_outgoing(failed).unwrap();
        let row = db
            .outbox(fx.account)
            .unwrap()
            .into_iter()
            .find(|row| row.id == failed)
            .unwrap();
        assert_eq!(row.state, OutgoingState::Queued);
        assert_eq!(
            (row.attempts, row.deferrals, row.next_attempt_at),
            (0, 0, 0)
        );
        assert_eq!(row.last_error, None);
        assert_eq!(db.due_outgoing(fx.account, 0).unwrap().len(), 2);

        db.delete_outgoing(maybe).unwrap();
        db.delete_outgoing(queued).unwrap();
        assert!(db.unsent_outgoing().unwrap().is_empty());
        assert_eq!(db.outbox(fx.account).unwrap().len(), 1);
    }
}
