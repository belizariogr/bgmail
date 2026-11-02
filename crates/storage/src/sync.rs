//! Writes and reads used by the IMAP sync engine (headers, flags, bodies,
//! server locations, attachments).
//!
//! The engine runs on a background thread with its own [`Database`]; every
//! multi-statement write here is one transaction so the UI connection never
//! observes half-applied changes.

use rusqlite::{params, Connection, OptionalExtension};

use crate::membership::{
    message_ids_in_folder_limit, refresh_membership_in, settle_after_location_loss, LOCATION_BATCH,
};
use crate::search::build_search_text;
use crate::types::{
    AttachmentRow, MessageLocation, NewAttachment, NewRemoteMessage, RemoteInsert, PROVIDER_LOCAL,
};
use crate::Database;

/// `messages.body_state` while only headers are known.
pub const BODY_STATE_HEADERS: u8 = 0;
/// `messages.body_state` once the raw body is stored.
pub const BODY_STATE_FULL: u8 = 1;

/// Finds the message an incoming `(folder, uid)` should attach to, if any.
fn find_merge_target(
    conn: &Connection,
    account_id: i64,
    message: &NewRemoteMessage,
) -> rusqlite::Result<Option<i64>> {
    // Gmail exposes one message under many labels/folders: same X-GM-MSGID is
    // always the same message.
    if let Some(gm_msgid) = message.gm_msgid {
        let found = conn
            .query_row(
                "SELECT id FROM messages WHERE account_id = ?1 AND gm_msgid = ?2
                 ORDER BY id ASC LIMIT 1",
                params![account_id, gm_msgid as i64],
                |row| row.get(0),
            )
            .optional()?;
        if found.is_some() {
            return Ok(found);
        }
    }
    // Plain IMAP: only a message "in flight" (moved by the UI, locations
    // detached) may be re-attached by Message-ID; otherwise two copies with the
    // same id (e.g. Sent + Inbox) stay distinct.
    if let Some(message_id_hdr) = &message.message_id_hdr {
        return conn
            .query_row(
                "SELECT m.id FROM messages m
                 WHERE m.account_id = ?1 AND m.message_id_hdr = ?2
                   AND NOT EXISTS (SELECT 1 FROM message_locations l WHERE l.message_id = m.id)
                 ORDER BY m.id ASC LIMIT 1",
                params![account_id, message_id_hdr],
                |row| row.get(0),
            )
            .optional();
    }
    Ok(None)
}

fn insert_message_row(
    conn: &Connection,
    account_id: i64,
    message: &NewRemoteMessage,
) -> rusqlite::Result<i64> {
    let search_text = build_search_text(
        &message.sender,
        &message.sender_email,
        &message.subject,
        &message.plain_text,
    );
    conn.execute(
        "INSERT INTO messages (
            account_id, sender, sender_email, subject, plain_text, raw_content,
            raw_format, preview, search_text, received_at, sort_order, unread, starred,
            has_attachment, folders_csv, message_id_hdr, in_reply_to, references_hdr,
            thread_id, to_addrs, cc_addrs, reply_to, date_ts, answered, body_state,
            gm_msgid, size
        ) VALUES (?1, ?2, ?3, ?4, ?5, '', 'text', ?6, ?7, ?8, 0, ?9, ?10, ?11, ',,',
                  ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
        params![
            account_id,
            message.sender,
            message.sender_email,
            message.subject,
            message.plain_text,
            message.preview,
            search_text,
            message.received_at,
            message.unread as i64,
            message.starred as i64,
            message.has_attachment as i64,
            message.message_id_hdr,
            message.in_reply_to,
            message.references_hdr,
            message.thread_id,
            message.to_addrs,
            message.cc_addrs,
            message.reply_to,
            message.date_ts,
            message.answered as i64,
            BODY_STATE_HEADERS as i64,
            message.gm_msgid.map(|id| id as i64),
            message.size as i64,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

fn location_message_id(
    conn: &Connection,
    folder_id: i64,
    uid: u32,
) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT message_id FROM message_locations WHERE folder_id = ?1 AND uid = ?2",
        params![folder_id, uid],
        |row| row.get(0),
    )
    .optional()
}

impl Database {
    /// Records that `(folder_id, uid)` holds `message` and returns the local
    /// message id.
    ///
    /// Order: an existing `(folder, uid)` wins; then a message of the account
    /// with the same `gm_msgid`; then an in-flight (location-less) message with
    /// the same Message-ID; otherwise a new headers-only row
    /// (`body_state = 0`). Merges keep the existing row's fields and flags —
    /// call [`Self::apply_remote_flags`] to update those.
    pub fn insert_remote_message(
        &self,
        account_id: i64,
        folder_id: i64,
        uid: u32,
        message: &NewRemoteMessage,
    ) -> rusqlite::Result<i64> {
        self.record_remote_message(account_id, folder_id, uid, message)
            .map(|insert| insert.message_id)
    }

    /// [`Self::insert_remote_message`] that also reports whether a new row
    /// was created (the sync engine only announces genuinely new mail, not
    /// merges of moved or multi-label messages).
    pub fn record_remote_message(
        &self,
        account_id: i64,
        folder_id: i64,
        uid: u32,
        message: &NewRemoteMessage,
    ) -> rusqlite::Result<RemoteInsert> {
        let tx = self.write_transaction()?;
        if let Some(existing) = location_message_id(&tx, folder_id, uid)? {
            return Ok(RemoteInsert {
                message_id: existing,
                created: false,
            });
        }
        let (message_id, created) = match find_merge_target(&tx, account_id, message)? {
            Some(id) => (id, false),
            None => (insert_message_row(&tx, account_id, message)?, true),
        };
        tx.execute(
            "INSERT INTO message_locations (message_id, folder_id, uid) VALUES (?1, ?2, ?3)",
            params![message_id, folder_id, uid],
        )?;
        refresh_membership_in(&tx, message_id)?;
        tx.commit()?;
        Ok(RemoteInsert {
            message_id,
            created,
        })
    }

    /// `(uid, message_id)` pairs of the folder, ascending by uid.
    pub fn folder_uids(&self, folder_id: i64) -> rusqlite::Result<Vec<(u32, i64)>> {
        let mut stmt = self.conn().prepare(
            "SELECT uid, message_id FROM message_locations WHERE folder_id = ?1 ORDER BY uid ASC",
        )?;
        let rows = stmt.query_map(params![folder_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect()
    }

    /// Highest known uid of the folder.
    pub fn max_uid(&self, folder_id: i64) -> rusqlite::Result<Option<u32>> {
        self.conn().query_row(
            "SELECT MAX(uid) FROM message_locations WHERE folder_id = ?1",
            params![folder_id],
            |row| row.get(0),
        )
    }

    /// Forgets `uids` of the folder (expunged on the server). Remote messages
    /// left without any location are deleted; others get membership refreshed.
    pub fn remove_locations(&self, folder_id: i64, uids: &[u32]) -> rusqlite::Result<()> {
        let tx = self.write_transaction()?;
        let mut affected = Vec::new();
        for &uid in uids {
            if let Some(message_id) = location_message_id(&tx, folder_id, uid)? {
                tx.execute(
                    "DELETE FROM message_locations WHERE folder_id = ?1 AND uid = ?2",
                    params![folder_id, uid],
                )?;
                affected.push(message_id);
            }
        }
        affected.sort_unstable();
        affected.dedup();
        settle_after_location_loss(&tx, &affected)?;
        tx.commit()
    }

    /// Forgets every location of the folder (UIDVALIDITY changed), with the
    /// same orphan rules as [`Self::remove_locations`].
    ///
    /// Commits every [`LOCATION_BATCH`] messages: one transaction over a
    /// huge folder would hold the write lock long enough for the UI
    /// connection's writes to hit `busy_timeout`.
    pub fn clear_folder_locations(&self, folder_id: i64) -> rusqlite::Result<()> {
        loop {
            let tx = self.write_transaction()?;
            let batch = message_ids_in_folder_limit(&tx, folder_id, LOCATION_BATCH)?;
            if batch.is_empty() {
                return tx.commit();
            }
            for &message_id in &batch {
                tx.execute(
                    "DELETE FROM message_locations WHERE folder_id = ?1 AND message_id = ?2",
                    params![folder_id, message_id],
                )?;
            }
            settle_after_location_loss(&tx, &batch)?;
            tx.commit()?;
        }
    }

    /// Applies server flags to the message at `(folder_id, uid)`. Returns
    /// whether anything changed (`false` for unknown locations too).
    pub fn apply_remote_flags(
        &self,
        folder_id: i64,
        uid: u32,
        unread: bool,
        starred: bool,
        answered: bool,
    ) -> rusqlite::Result<bool> {
        let tx = self.write_transaction()?;
        let Some(message_id) = location_message_id(&tx, folder_id, uid)? else {
            return Ok(false);
        };
        let changed = tx.execute(
            "UPDATE messages SET unread = ?1, starred = ?2, answered = ?3
             WHERE id = ?4 AND (unread != ?1 OR starred != ?2 OR answered != ?3)",
            params![unread as i64, starred as i64, answered as i64, message_id],
        )? > 0;
        if changed {
            refresh_membership_in(&tx, message_id)?;
        }
        tx.commit()?;
        Ok(changed)
    }

    /// Stores the downloaded body: raw bytes, rendered content (`format` is
    /// `html` or `text`), plain text/preview, search text and attachments
    /// (replacing previous ones). `has_attachment` = any non-inline part.
    /// No-op when the message was deleted meanwhile.
    #[allow(clippy::too_many_arguments)]
    pub fn store_body(
        &self,
        message_id: i64,
        raw: &[u8],
        content: &str,
        format: &str,
        plain_text: &str,
        preview: &str,
        attachments: &[NewAttachment],
    ) -> rusqlite::Result<()> {
        let tx = self.write_transaction()?;
        let Some((sender, sender_email, subject)) = tx
            .query_row(
                "SELECT sender, sender_email, subject FROM messages WHERE id = ?1",
                params![message_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?
        else {
            return Ok(());
        };
        let search_text = build_search_text(&sender, &sender_email, &subject, plain_text);
        let has_attachment = attachments.iter().any(|attachment| !attachment.inline);
        tx.execute(
            "INSERT INTO message_bodies (message_id, raw) VALUES (?1, ?2)
             ON CONFLICT(message_id) DO UPDATE SET raw = excluded.raw",
            params![message_id, raw],
        )?;
        tx.execute(
            "UPDATE messages SET raw_content = ?1, raw_format = ?2, plain_text = ?3,
                    preview = ?4, search_text = ?5, has_attachment = ?6, body_state = ?7
             WHERE id = ?8",
            params![
                content,
                format,
                plain_text,
                preview,
                search_text,
                has_attachment as i64,
                BODY_STATE_FULL as i64,
                message_id
            ],
        )?;
        tx.execute(
            "DELETE FROM attachments WHERE message_id = ?1",
            params![message_id],
        )?;
        for attachment in attachments {
            tx.execute(
                "INSERT INTO attachments
                    (message_id, part_index, filename, mime_type, size, content_id, inline)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    message_id,
                    attachment.part_index,
                    attachment.filename,
                    attachment.mime_type,
                    attachment.size as i64,
                    attachment.content_id,
                    attachment.inline as i64,
                ],
            )?;
        }
        tx.commit()
    }

    /// Raw RFC 822 bytes, when the body was downloaded.
    /// Downloaded messages of the account whose preview still looks like
    /// markup: raw HTML or undecoded entities.
    pub fn messages_with_markup_preview(&self, account_id: i64) -> rusqlite::Result<Vec<i64>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id FROM messages
             WHERE account_id = ?1 AND body_state = ?2
               AND (preview LIKE '<%' OR preview LIKE '%&%;%')",
        )?;
        let ids = stmt
            .query_map(params![account_id, BODY_STATE_FULL as i64], |row| {
                row.get(0)
            })?
            .collect();
        ids
    }

    /// Replaces the text derived from a stored body, keeping search in step.
    pub fn update_body_text(
        &self,
        message_id: i64,
        plain_text: &str,
        preview: &str,
    ) -> rusqlite::Result<()> {
        let tx = self.write_transaction()?;
        let Some((sender, sender_email, subject)) = tx
            .query_row(
                "SELECT sender, sender_email, subject FROM messages WHERE id = ?1",
                params![message_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?
        else {
            return Ok(());
        };
        tx.execute(
            "UPDATE messages SET plain_text = ?1, preview = ?2, search_text = ?3 WHERE id = ?4",
            params![
                plain_text,
                preview,
                build_search_text(&sender, &sender_email, &subject, plain_text),
                message_id
            ],
        )?;
        tx.commit()
    }

    pub fn message_raw(&self, message_id: i64) -> rusqlite::Result<Option<Vec<u8>>> {
        self.conn()
            .query_row(
                "SELECT raw FROM message_bodies WHERE message_id = ?1",
                params![message_id],
                |row| row.get(0),
            )
            .optional()
    }

    /// Attachment metadata: real attachments first, then inline parts, each by
    /// `part_index`.
    pub fn list_attachments(&self, message_id: i64) -> rusqlite::Result<Vec<AttachmentRow>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, message_id, part_index, filename, mime_type, size, content_id, inline
             FROM attachments WHERE message_id = ?1
             ORDER BY inline ASC, part_index ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![message_id], |row| {
            Ok(AttachmentRow {
                id: row.get(0)?,
                message_id: row.get(1)?,
                part_index: row.get(2)?,
                filename: row.get(3)?,
                mime_type: row.get(4)?,
                size: row.get::<_, i64>(5)? as u64,
                content_id: row.get(6)?,
                inline: row.get::<_, i64>(7)? != 0,
            })
        })?;
        rows.collect()
    }

    /// Server locations of the message, in insertion order.
    pub fn message_locations(&self, message_id: i64) -> rusqlite::Result<Vec<MessageLocation>> {
        message_locations_in(self.conn(), message_id)
    }

    /// Ids of the account's headers-only messages that can be fetched (have a
    /// location), newest first.
    pub fn messages_without_body(
        &self,
        account_id: i64,
        limit: usize,
    ) -> rusqlite::Result<Vec<i64>> {
        let mut stmt = self.conn().prepare(
            "SELECT m.id FROM messages m
             WHERE m.account_id = ?1 AND m.body_state = ?2
               AND EXISTS (SELECT 1 FROM message_locations l WHERE l.message_id = m.id)
             ORDER BY m.date_ts DESC, m.id DESC
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(
            params![
                account_id,
                BODY_STATE_HEADERS as i64,
                i64::try_from(limit).unwrap_or(i64::MAX)
            ],
            |row| row.get(0),
        )?;
        rows.collect()
    }

    /// Drops every location of the message (the UI moved it), making it "in
    /// flight": its `folders_csv` is kept and the next sync re-attaches it by
    /// Message-ID in the target folder. Returns the removed locations so the
    /// caller can enqueue the server-side move.
    pub fn detach_locations(&self, message_id: i64) -> rusqlite::Result<Vec<MessageLocation>> {
        let tx = self.write_transaction()?;
        let locations = message_locations_in(&tx, message_id)?;
        tx.execute(
            "DELETE FROM message_locations WHERE message_id = ?1",
            params![message_id],
        )?;
        tx.commit()?;
        Ok(locations)
    }
}

impl Database {
    /// Records that the message now also lives at `(folder_id, uid)` (e.g. the
    /// COPYUID of a server-side move) and refreshes its membership. Returns
    /// `false` when the message is gone or the location already belongs to a
    /// message (nothing written then).
    pub fn attach_location(
        &self,
        message_id: i64,
        folder_id: i64,
        uid: u32,
    ) -> rusqlite::Result<bool> {
        let tx = self.write_transaction()?;
        let exists = tx
            .query_row(
                "SELECT 1 FROM messages WHERE id = ?1",
                params![message_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists || location_message_id(&tx, folder_id, uid)?.is_some() {
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO message_locations (message_id, folder_id, uid) VALUES (?1, ?2, ?3)",
            params![message_id, folder_id, uid],
        )?;
        refresh_membership_in(&tx, message_id)?;
        tx.commit()?;
        Ok(true)
    }

    /// Removes the message's (first) location in `folder_id` and returns it so
    /// the caller can queue the server-side move. Membership is refreshed
    /// when other locations remain; a message left without any is "in
    /// flight" and keeps its `folders_csv` (like [`Self::detach_locations`]).
    pub fn detach_location(
        &self,
        message_id: i64,
        folder_id: i64,
    ) -> rusqlite::Result<Option<MessageLocation>> {
        let tx = self.write_transaction()?;
        let Some(location) = message_locations_in(&tx, message_id)?
            .into_iter()
            .find(|location| location.folder_id == folder_id)
        else {
            return Ok(None);
        };
        tx.execute(
            "DELETE FROM message_locations WHERE folder_id = ?1 AND uid = ?2",
            params![folder_id, location.uid],
        )?;
        refresh_membership_in(&tx, message_id)?;
        tx.commit()?;
        Ok(Some(location))
    }

    /// Deletes the account's messages that no longer exist anywhere: no
    /// server location and no queued op referencing them (an op means a move
    /// is still in flight). `local` accounts are never purged. Returns how
    /// many rows were deleted.
    pub fn purge_orphans(&self, account_id: i64) -> rusqlite::Result<usize> {
        let tx = self.write_transaction()?;
        let deleted = tx.execute(
            "DELETE FROM messages
             WHERE account_id = ?1
               AND account_id IN (SELECT id FROM accounts WHERE provider != ?2)
               AND NOT EXISTS (SELECT 1 FROM message_locations l WHERE l.message_id = messages.id)
               AND NOT EXISTS (SELECT 1 FROM pending_ops p WHERE p.message_id = messages.id)",
            params![account_id, PROVIDER_LOCAL],
        )?;
        tx.commit()?;
        Ok(deleted)
    }

    /// Body-prefetch candidates of a folder: among its `limit` newest
    /// messages, the headers-only ones no bigger than `max_size` bytes, as
    /// `(message_id, uid)` newest first.
    pub fn folder_messages_without_body(
        &self,
        folder_id: i64,
        limit: usize,
        max_size: u64,
    ) -> rusqlite::Result<Vec<(i64, u32)>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, uid FROM (
                 SELECT m.id, l.uid, m.body_state, m.size, m.date_ts
                 FROM message_locations l JOIN messages m ON m.id = l.message_id
                 WHERE l.folder_id = ?1
                 ORDER BY m.date_ts DESC, l.uid DESC
                 LIMIT ?2
             )
             WHERE body_state = ?3 AND size <= ?4
             ORDER BY date_ts DESC, uid DESC",
        )?;
        let rows = stmt.query_map(
            params![
                folder_id,
                i64::try_from(limit).unwrap_or(i64::MAX),
                BODY_STATE_HEADERS as i64,
                i64::try_from(max_size).unwrap_or(i64::MAX)
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        rows.collect()
    }
}

fn message_locations_in(
    conn: &Connection,
    message_id: i64,
) -> rusqlite::Result<Vec<MessageLocation>> {
    let mut stmt = conn.prepare(
        "SELECT l.folder_id, f.remote_name, f.path, l.uid
         FROM message_locations l JOIN folders f ON f.id = l.folder_id
         WHERE l.message_id = ?1 ORDER BY l.id ASC",
    )?;
    let rows = stmt.query_map(params![message_id], |row| {
        Ok(MessageLocation {
            folder_id: row.get(0)?,
            remote_name: row.get(1)?,
            path: row.get(2)?,
            uid: row.get(3)?,
        })
    })?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{remote_db, remote_message};
    use crate::{system, MailListQuery};

    fn count(db: &Database, sql: &str) -> i64 {
        db.conn().query_row(sql, [], |row| row.get(0)).unwrap()
    }

    #[test]
    fn insert_creates_headers_only_message_with_location() {
        let (_file, db, fx) = remote_db();
        let mut message = remote_message("Olá", None);
        message.date_ts = 1_700_000_000;
        message.unread = true;
        let id = db
            .insert_remote_message(fx.account, fx.inbox, 10, &message)
            .unwrap();
        let detail = db.get_message(id).unwrap().unwrap();
        assert_eq!(detail.body_state, BODY_STATE_HEADERS);
        assert_eq!(detail.raw_content, "");
        assert_eq!(detail.raw_format, "text");
        assert_eq!(detail.folders_csv, ",sys:inbox,");
        assert_eq!(detail.date_ts, 1_700_000_000);
        assert_eq!(detail.message_id_hdr.as_deref(), Some("<Olá@test>"));
        assert!(detail.unread);
        assert_eq!(db.folder_uids(fx.inbox).unwrap(), vec![(10, id)]);
        // Search text is computed from headers.
        assert_eq!(
            db.list_messages(&MailListQuery::Search("ola".into()))
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn insert_same_location_twice_returns_existing() {
        let (_file, db, fx) = remote_db();
        let a = db
            .insert_remote_message(fx.account, fx.inbox, 1, &remote_message("a", None))
            .unwrap();
        let b = db
            .insert_remote_message(fx.account, fx.inbox, 1, &remote_message("b", None))
            .unwrap();
        assert_eq!(a, b);
        assert_eq!(db.message_count_all().unwrap(), 1);
    }

    #[test]
    fn gm_msgid_merges_locations() {
        let (_file, db, fx) = remote_db();
        let a = db
            .insert_remote_message(fx.account, fx.inbox, 1, &remote_message("a", Some(77)))
            .unwrap();
        let b = db
            .insert_remote_message(fx.account, fx.archive, 3, &remote_message("a", Some(77)))
            .unwrap();
        assert_eq!(a, b);
        let paths: Vec<String> = db
            .message_locations(a)
            .unwrap()
            .into_iter()
            .map(|l| l.path)
            .collect();
        assert_eq!(
            paths,
            vec![system::INBOX.to_string(), system::ARCHIVE.to_string()]
        );
        assert_eq!(
            db.get_message(a).unwrap().unwrap().folders_csv,
            ",sys:archive,sys:inbox,"
        );
        // Different account never merges.
        let other = db
            .insert_remote_message(
                fx.local_account,
                fx.local_inbox,
                1,
                &remote_message("a", Some(77)),
            )
            .unwrap();
        assert_ne!(other, a);
    }

    #[test]
    fn message_id_merges_only_in_flight_messages() {
        let (_file, db, fx) = remote_db();
        let a = db
            .insert_remote_message(fx.account, fx.inbox, 1, &remote_message("a", None))
            .unwrap();
        // Same Message-ID while `a` still has a location: separate copy.
        let copy = db
            .insert_remote_message(fx.account, fx.archive, 1, &remote_message("a", None))
            .unwrap();
        assert_ne!(copy, a);

        // UI moves `a` → in flight; the server copy in Trash re-attaches it.
        let removed = db.detach_locations(a).unwrap();
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].remote_name.as_deref(), Some("INBOX"));
        assert_eq!(removed[0].uid, 1);
        db.move_message_to_trash(a).unwrap();
        let merged = db
            .insert_remote_message(fx.account, fx.trash, 50, &remote_message("a", None))
            .unwrap();
        assert_eq!(merged, a);
        assert_eq!(
            db.get_message(a).unwrap().unwrap().folders_csv,
            ",sys:trash,"
        );
    }

    #[test]
    fn detach_keeps_csv_and_returns_nothing_twice() {
        let (_file, db, fx) = remote_db();
        let a = db
            .insert_remote_message(fx.account, fx.inbox, 1, &remote_message("a", None))
            .unwrap();
        db.detach_locations(a).unwrap();
        assert!(db.detach_locations(a).unwrap().is_empty());
        assert!(db.message_locations(a).unwrap().is_empty());
        assert_eq!(
            db.get_message(a).unwrap().unwrap().folders_csv,
            ",sys:inbox,"
        );
    }

    #[test]
    fn folder_uids_and_max_uid() {
        let (_file, db, fx) = remote_db();
        assert_eq!(db.max_uid(fx.inbox).unwrap(), None);
        let b = db
            .insert_remote_message(fx.account, fx.inbox, 9, &remote_message("b", None))
            .unwrap();
        let a = db
            .insert_remote_message(fx.account, fx.inbox, 4, &remote_message("a", None))
            .unwrap();
        assert_eq!(db.folder_uids(fx.inbox).unwrap(), vec![(4, a), (9, b)]);
        assert_eq!(db.max_uid(fx.inbox).unwrap(), Some(9));
        assert!(db.folder_uids(fx.archive).unwrap().is_empty());
    }

    #[test]
    fn remove_locations_deletes_remote_orphans_only() {
        let (_file, db, fx) = remote_db();
        let gone = db
            .insert_remote_message(fx.account, fx.inbox, 1, &remote_message("gone", None))
            .unwrap();
        let shared = db
            .insert_remote_message(fx.account, fx.inbox, 2, &remote_message("s", Some(5)))
            .unwrap();
        db.insert_remote_message(fx.account, fx.archive, 2, &remote_message("s", Some(5)))
            .unwrap();
        let local = db
            .insert_remote_message(
                fx.local_account,
                fx.local_inbox,
                1,
                &remote_message("local", None),
            )
            .unwrap();
        db.enqueue_op(fx.account, "noop", Some(gone), "{}").unwrap();

        db.remove_locations(fx.inbox, &[1, 2, 99]).unwrap();
        db.remove_locations(fx.local_inbox, &[1]).unwrap();

        assert!(db.get_message(gone).unwrap().is_none());
        assert_eq!(
            db.get_message(shared).unwrap().unwrap().folders_csv,
            ",sys:archive,"
        );
        let local = db.get_message(local).unwrap().unwrap();
        assert_eq!(local.folders_csv, ",sys:inbox,");
        assert!(db.folder_uids(fx.inbox).unwrap().is_empty());
        // FTS index follows the delete.
        assert!(db
            .list_messages(&MailListQuery::Search("gone".into()))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn clear_folder_locations_uses_orphan_rules() {
        let (_file, db, fx) = remote_db();
        let gone = db
            .insert_remote_message(fx.account, fx.inbox, 1, &remote_message("gone", None))
            .unwrap();
        let shared = db
            .insert_remote_message(fx.account, fx.inbox, 2, &remote_message("s", Some(5)))
            .unwrap();
        db.insert_remote_message(fx.account, fx.trash, 7, &remote_message("s", Some(5)))
            .unwrap();
        db.clear_folder_locations(fx.inbox).unwrap();
        assert!(db.get_message(gone).unwrap().is_none());
        assert_eq!(
            db.get_message(shared).unwrap().unwrap().folders_csv,
            ",sys:trash,"
        );
        assert_eq!(db.max_uid(fx.inbox).unwrap(), None);
    }

    #[test]
    fn clear_folder_locations_spans_several_batches() {
        let (_file, db, fx) = remote_db();
        let total = crate::membership::LOCATION_BATCH * 2 + 3;
        for uid in 1..=total as u32 {
            db.insert_remote_message(
                fx.account,
                fx.inbox,
                uid,
                &remote_message(&format!("m{uid}"), None),
            )
            .unwrap();
        }
        let kept = db
            .insert_remote_message(fx.account, fx.trash, 1, &remote_message("kept", None))
            .unwrap();
        db.clear_folder_locations(fx.inbox).unwrap();
        assert_eq!(db.max_uid(fx.inbox).unwrap(), None);
        assert_eq!(
            count(&db, "SELECT COUNT(*) FROM messages WHERE account_id = 1"),
            1
        );
        assert!(db.get_message(kept).unwrap().is_some());

        // delete_folder goes through the same batches.
        for uid in 1..=total as u32 {
            db.insert_remote_message(
                fx.account,
                fx.archive,
                uid,
                &remote_message(&format!("a{uid}"), None),
            )
            .unwrap();
        }
        db.delete_folder(fx.archive).unwrap();
        assert_eq!(
            count(&db, "SELECT COUNT(*) FROM messages WHERE account_id = 1"),
            1
        );
    }

    #[test]
    fn apply_remote_flags_updates_and_reports_change() {
        let (_file, db, fx) = remote_db();
        let id = db
            .insert_remote_message(fx.account, fx.inbox, 1, &remote_message("a", None))
            .unwrap();
        assert!(db
            .apply_remote_flags(fx.inbox, 1, true, true, true)
            .unwrap());
        let detail = db.get_message(id).unwrap().unwrap();
        assert!(detail.unread && detail.starred && detail.answered);
        assert_eq!(detail.folders_csv, ",sys:flagged,sys:inbox,");
        assert!(!db
            .apply_remote_flags(fx.inbox, 1, true, true, true)
            .unwrap());
        assert!(db
            .apply_remote_flags(fx.inbox, 1, false, false, true)
            .unwrap());
        assert_eq!(
            db.get_message(id).unwrap().unwrap().folders_csv,
            ",sys:inbox,"
        );
        assert!(!db
            .apply_remote_flags(fx.inbox, 404, true, true, true)
            .unwrap());
    }

    #[test]
    fn store_body_fills_message_and_attachments() {
        let (_file, db, fx) = remote_db();
        let id = db
            .insert_remote_message(fx.account, fx.inbox, 1, &remote_message("Hello", None))
            .unwrap();
        let attachments = [
            NewAttachment {
                part_index: 3,
                filename: "logo.png".into(),
                mime_type: "image/png".into(),
                size: 10,
                content_id: Some("logo".into()),
                inline: true,
            },
            NewAttachment {
                part_index: 2,
                filename: "b.pdf".into(),
                mime_type: "application/pdf".into(),
                size: 20,
                content_id: None,
                inline: false,
            },
            NewAttachment {
                part_index: 1,
                filename: "a.pdf".into(),
                mime_type: "application/pdf".into(),
                size: 30,
                content_id: None,
                inline: false,
            },
        ];
        db.store_body(
            id,
            b"RAW",
            "<p>Café</p>",
            "html",
            "Café quentinho",
            "Café",
            &attachments,
        )
        .unwrap();

        let detail = db.get_message(id).unwrap().unwrap();
        assert_eq!(detail.body_state, BODY_STATE_FULL);
        assert_eq!(detail.raw_content, "<p>Café</p>");
        assert_eq!(detail.raw_format, "html");
        assert_eq!(detail.plain_text, "Café quentinho");
        assert!(detail.has_attachment);
        assert_eq!(db.message_raw(id).unwrap().as_deref(), Some(&b"RAW"[..]));
        assert_eq!(
            db.list_messages(&MailListQuery::Search("quentinho".into()))
                .unwrap()
                .len(),
            1
        );
        let names: Vec<String> = db
            .list_attachments(id)
            .unwrap()
            .into_iter()
            .map(|a| a.filename)
            .collect();
        assert_eq!(names, vec!["a.pdf", "b.pdf", "logo.png"]);

        // Replacing with inline-only parts clears has_attachment and old rows.
        db.store_body(id, b"RAW2", "t", "text", "t", "t", &attachments[..1])
            .unwrap();
        assert!(!db.get_message(id).unwrap().unwrap().has_attachment);
        assert_eq!(db.list_attachments(id).unwrap().len(), 1);
        assert_eq!(db.message_raw(id).unwrap().as_deref(), Some(&b"RAW2"[..]));
        assert_eq!(count(&db, "SELECT COUNT(*) FROM message_bodies"), 1);
    }

    #[test]
    fn markup_previews_are_found_and_replaced() {
        let (_file, db, fx) = remote_db();
        let ids: Vec<i64> = [1, 2, 3]
            .iter()
            .map(|uid| {
                db.insert_remote_message(fx.account, fx.inbox, *uid, &remote_message("x", None))
                    .unwrap()
            })
            .collect();
        db.store_body(ids[0], b"x", "h", "html", "<html>", "<html> code", &[])
            .unwrap();
        db.store_body(ids[1], b"x", "t", "text", "fine", "Ol&aacute; there", &[])
            .unwrap();
        db.store_body(ids[2], b"x", "t", "text", "fine", "A & B", &[])
            .unwrap();
        assert_eq!(
            db.messages_with_markup_preview(fx.account).unwrap(),
            ids[..2]
        );

        db.update_body_text(ids[0], "readable", "readable").unwrap();
        db.update_body_text(404, "x", "x").unwrap();
        assert_eq!(
            db.messages_with_markup_preview(fx.account).unwrap(),
            vec![ids[1]]
        );
        let detail = db.get_message(ids[0]).unwrap().unwrap();
        assert_eq!(detail.plain_text, "readable");
        assert_eq!(
            db.list_messages(&MailListQuery::Search("readable".into()))
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn store_body_for_missing_message_is_noop() {
        let (_file, db, _fx) = remote_db();
        db.store_body(404, b"x", "x", "text", "x", "x", &[])
            .unwrap();
        assert_eq!(db.message_raw(404).unwrap(), None);
    }

    #[test]
    fn messages_without_body_newest_first_with_locations() {
        let (_file, db, fx) = remote_db();
        let mut old = remote_message("old", None);
        old.date_ts = 100;
        let mut new = remote_message("new", None);
        new.date_ts = 200;
        let old = db
            .insert_remote_message(fx.account, fx.inbox, 1, &old)
            .unwrap();
        let new = db
            .insert_remote_message(fx.account, fx.inbox, 2, &new)
            .unwrap();
        let fetched = db
            .insert_remote_message(fx.account, fx.inbox, 3, &remote_message("f", None))
            .unwrap();
        db.store_body(fetched, b"x", "x", "text", "x", "x", &[])
            .unwrap();
        let in_flight = db
            .insert_remote_message(fx.account, fx.inbox, 4, &remote_message("i", None))
            .unwrap();
        db.detach_locations(in_flight).unwrap();

        assert_eq!(
            db.messages_without_body(fx.account, 10).unwrap(),
            vec![new, old]
        );
        assert_eq!(db.messages_without_body(fx.account, 1).unwrap(), vec![new]);
    }

    #[test]
    fn record_remote_message_reports_creation() {
        let (_file, db, fx) = remote_db();
        let first = db
            .record_remote_message(fx.account, fx.inbox, 1, &remote_message("a", Some(9)))
            .unwrap();
        assert!(first.created);
        let again = db
            .record_remote_message(fx.account, fx.inbox, 1, &remote_message("a", Some(9)))
            .unwrap();
        assert_eq!(again.message_id, first.message_id);
        assert!(!again.created, "known location");
        let merged = db
            .record_remote_message(fx.account, fx.archive, 5, &remote_message("a", Some(9)))
            .unwrap();
        assert_eq!(merged.message_id, first.message_id);
        assert!(!merged.created, "gm_msgid merge");
    }

    #[test]
    fn attach_location_adds_and_refreshes_membership() {
        let (_file, db, fx) = remote_db();
        let id = db
            .insert_remote_message(fx.account, fx.inbox, 1, &remote_message("a", None))
            .unwrap();
        db.detach_locations(id).unwrap();
        assert!(db.attach_location(id, fx.archive, 7).unwrap());
        assert_eq!(db.folder_uids(fx.archive).unwrap(), vec![(7, id)]);
        assert_eq!(
            db.get_message(id).unwrap().unwrap().folders_csv,
            ",sys:archive,"
        );
        assert!(!db.attach_location(id, fx.archive, 7).unwrap(), "taken");
        assert!(
            !db.attach_location(9999, fx.archive, 8).unwrap(),
            "missing message"
        );
    }

    #[test]
    fn detach_location_removes_one_folder_only() {
        let (_file, db, fx) = remote_db();
        let id = db
            .insert_remote_message(fx.account, fx.inbox, 1, &remote_message("a", Some(3)))
            .unwrap();
        db.insert_remote_message(fx.account, fx.archive, 2, &remote_message("a", Some(3)))
            .unwrap();
        let removed = db.detach_location(id, fx.inbox).unwrap().unwrap();
        assert_eq!((removed.folder_id, removed.uid), (fx.inbox, 1));
        assert_eq!(removed.remote_name.as_deref(), Some("INBOX"));
        assert_eq!(
            db.get_message(id).unwrap().unwrap().folders_csv,
            ",sys:archive,"
        );
        assert_eq!(db.detach_location(id, fx.inbox).unwrap(), None);
        // Last location: message becomes in flight and keeps its csv.
        assert!(db.detach_location(id, fx.archive).unwrap().is_some());
        assert_eq!(
            db.get_message(id).unwrap().unwrap().folders_csv,
            ",sys:archive,"
        );
        assert!(db.message_locations(id).unwrap().is_empty());
    }

    #[test]
    fn purge_orphans_spares_located_queued_and_local_mail() {
        let (_file, db, fx) = remote_db();
        let located = db
            .insert_remote_message(fx.account, fx.inbox, 1, &remote_message("a", None))
            .unwrap();
        let orphan = db
            .insert_remote_message(fx.account, fx.inbox, 2, &remote_message("b", None))
            .unwrap();
        let queued = db
            .insert_remote_message(fx.account, fx.inbox, 3, &remote_message("c", None))
            .unwrap();
        let local = db
            .insert_remote_message(
                fx.local_account,
                fx.local_inbox,
                1,
                &remote_message("d", None),
            )
            .unwrap();
        for id in [orphan, queued, local] {
            db.detach_locations(id).unwrap();
        }
        db.enqueue_op(fx.account, "move", Some(queued), "{}")
            .unwrap();
        assert_eq!(db.purge_orphans(fx.account).unwrap(), 1);
        assert!(db.get_message(orphan).unwrap().is_none());
        for id in [located, queued] {
            assert!(db.get_message(id).unwrap().is_some());
        }
        assert_eq!(db.purge_orphans(fx.local_account).unwrap(), 0);
        assert!(db.get_message(local).unwrap().is_some());
    }

    #[test]
    fn folder_messages_without_body_limits_to_newest_and_size() {
        let (_file, db, fx) = remote_db();
        let mut ids = Vec::new();
        for (uid, (date, size)) in [(100, 10), (200, 10), (300, 5_000), (400, 10)]
            .into_iter()
            .enumerate()
        {
            let mut message = remote_message(&format!("m{uid}"), None);
            message.date_ts = date;
            message.size = size;
            ids.push(
                db.insert_remote_message(fx.account, fx.inbox, uid as u32 + 1, &message)
                    .unwrap(),
            );
        }
        db.store_body(ids[3], b"x", "x", "text", "x", "x", &[])
            .unwrap();
        // Newest 3 = ids[3] (has body), ids[2] (too big), ids[1].
        assert_eq!(
            db.folder_messages_without_body(fx.inbox, 3, 1_000).unwrap(),
            vec![(ids[1], 2)]
        );
        assert_eq!(
            db.folder_messages_without_body(fx.inbox, 10, 10_000)
                .unwrap(),
            vec![(ids[2], 3), (ids[1], 2), (ids[0], 1)]
        );
        assert!(db
            .folder_messages_without_body(fx.archive, 10, 10_000)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn list_order_is_date_then_sort_order() {
        let (_file, db, fx) = remote_db();
        let mut a = remote_message("a", None);
        a.date_ts = 10;
        let mut b = remote_message("b", None);
        b.date_ts = 20;
        let a = db
            .insert_remote_message(fx.account, fx.inbox, 1, &a)
            .unwrap();
        let b = db
            .insert_remote_message(fx.account, fx.inbox, 2, &b)
            .unwrap();
        let ids: Vec<i64> = db
            .list_messages(&MailListQuery::AccountFolder {
                account_id: fx.account,
                folder_path: system::INBOX.into(),
            })
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, vec![b, a]);
    }
}
