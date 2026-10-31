//! SQLite mail store: connection setup and list/detail queries.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::types::Value;
use rusqlite::{
    params, params_from_iter, Connection, OptionalExtension, Transaction, TransactionBehavior,
};

use crate::folder::{folder_like_pattern, system};
use crate::search::fold_for_search;
use crate::types::{MailListQuery, MessageDetail, MessageListItem};

/// Default database path: `~/.config/BGMail/mail.db`.
pub fn database_path() -> PathBuf {
    config_dir().join("mail.db")
}

fn config_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
    {
        home.join(".config").join("BGMail")
    } else {
        PathBuf::from(".config/BGMail")
    }
}

/// How long a connection waits for another one (e.g. the sync thread) to
/// release a write lock before failing with `SQLITE_BUSY`.
const BUSY_TIMEOUT: Duration = Duration::from_millis(5000);

/// Columns read into [`MessageListItem`] (see [`list_item_from_row`]).
const LIST_COLUMNS: &str = "id, account_id, sender, sender_email, subject, preview,
    received_at, date_ts, thread_id, unread, starred, has_attachment";

/// Newest first; mock rows share `date_ts = 0`, so `sort_order` orders them.
const LIST_ORDER: &str = "ORDER BY date_ts DESC, sort_order ASC, id DESC";

/// Folded queries shorter than this cannot use the trigram index.
const FTS_MIN_CHARS: usize = 3;

/// Local SQLite mail database.
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Opens (or creates) the database at `path` and applies migrations.
    ///
    /// WAL + a busy timeout let the UI and a background sync thread each hold
    /// their own connection to the same file.
    pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.query_row("PRAGMA journal_mode = WAL", [], |row| {
            row.get::<_, String>(0)
        })?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    /// Opens the default database path.
    pub fn open_default() -> rusqlite::Result<Self> {
        Self::open(database_path())
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Starts a write transaction with `BEGIN IMMEDIATE`.
    ///
    /// A deferred transaction that reads first and then writes fails with
    /// `SQLITE_BUSY` (no busy wait) when the other connection committed in
    /// between; taking the write lock up front lets `busy_timeout` serialize
    /// the UI and sync connections instead.
    pub(crate) fn write_transaction(&self) -> rusqlite::Result<Transaction<'_>> {
        Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)
    }

    fn migrate(&self) -> rusqlite::Result<()> {
        crate::migration::migrate(&self.conn)?;
        self.repair_starred_folder_membership()
    }

    /// Ensures starred messages appear in the global flagged folder.
    fn repair_starred_folder_membership(&self) -> rusqlite::Result<()> {
        use crate::folder::folders_csv;

        let pattern = folder_like_pattern(system::FLAGGED);
        let mut stmt = self.conn.prepare(
            "SELECT id, folders_csv FROM messages
             WHERE starred = 1 AND folders_csv NOT LIKE ?1",
        )?;
        let rows = stmt.query_map(params![pattern], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        let updates: Vec<(String, i64)> = rows
            .filter_map(|row| row.ok())
            .map(|(id, csv)| {
                let mut paths: Vec<String> = csv
                    .split(',')
                    .filter(|segment| !segment.is_empty())
                    .map(str::to_string)
                    .collect();
                if !paths.iter().any(|path| path == system::FLAGGED) {
                    paths.push(system::FLAGGED.to_string());
                    paths.sort();
                    paths.dedup();
                }
                (folders_csv(paths.iter().map(String::as_str)), id)
            })
            .collect();

        for (folders_csv, id) in updates {
            self.conn.execute(
                "UPDATE messages SET folders_csv = ?1 WHERE id = ?2",
                params![folders_csv, id],
            )?;
        }
        Ok(())
    }

    pub fn unread_in_folder(
        &self,
        account_id: Option<i64>,
        folder_path: &str,
    ) -> rusqlite::Result<usize> {
        let pattern = folder_like_pattern(folder_path);
        let sql = if account_id.is_some() {
            "SELECT COUNT(*) FROM messages
             WHERE account_id = ?1 AND unread = 1 AND folders_csv LIKE ?2"
        } else {
            "SELECT COUNT(*) FROM messages
             WHERE unread = 1 AND folders_csv LIKE ?1"
        };
        let count: i64 = if let Some(account_id) = account_id {
            self.conn
                .query_row(sql, params![account_id, pattern], |row| row.get(0))?
        } else {
            self.conn
                .query_row(sql, params![pattern], |row| row.get(0))?
        };
        Ok(count as usize)
    }

    pub fn global_unread(&self, folder_path: &str) -> rusqlite::Result<usize> {
        self.unread_in_folder(None, folder_path)
    }

    /// Unread messages across every account and folder.
    pub fn unread_count_total(&self) -> rusqlite::Result<usize> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE unread = 1",
            [],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    pub fn list_messages(&self, query: &MailListQuery) -> rusqlite::Result<Vec<MessageListItem>> {
        let Some((filter, values)) = query_filter(query) else {
            return Ok(Vec::new());
        };
        let sql = format!("SELECT {LIST_COLUMNS} FROM messages WHERE {filter} {LIST_ORDER}");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(values), list_item_from_row)?;
        rows.collect()
    }

    pub fn get_message(&self, id: i64) -> rusqlite::Result<Option<MessageDetail>> {
        self.conn
            .query_row(
                "SELECT id, account_id, sender, sender_email, subject,
                        plain_text, raw_content, raw_format, received_at,
                        unread, starred, has_attachment, folders_csv,
                        date_ts, to_addrs, cc_addrs, reply_to, message_id_hdr,
                        in_reply_to, references_hdr, thread_id, body_state, answered
                 FROM messages WHERE id = ?1",
                params![id],
                |row| {
                    Ok(MessageDetail {
                        id: row.get(0)?,
                        account_id: row.get(1)?,
                        sender: row.get(2)?,
                        sender_email: row.get(3)?,
                        subject: row.get(4)?,
                        plain_text: row.get(5)?,
                        raw_content: row.get(6)?,
                        raw_format: row.get(7)?,
                        received_at: row.get(8)?,
                        unread: row.get::<_, i64>(9)? != 0,
                        starred: row.get::<_, i64>(10)? != 0,
                        has_attachment: row.get::<_, i64>(11)? != 0,
                        folders_csv: row.get(12)?,
                        date_ts: row.get(13)?,
                        to_addrs: row.get(14)?,
                        cc_addrs: row.get(15)?,
                        reply_to: row.get(16)?,
                        message_id_hdr: row.get(17)?,
                        in_reply_to: row.get(18)?,
                        references_hdr: row.get(19)?,
                        thread_id: row.get(20)?,
                        body_state: row.get::<_, i64>(21)?.clamp(0, 255) as u8,
                        answered: row.get::<_, i64>(22)? != 0,
                    })
                },
            )
            .optional()
    }

    /// Counts the rows [`Self::list_messages`] would return, without loading them.
    pub fn message_count_for_query(&self, query: &MailListQuery) -> rusqlite::Result<usize> {
        let Some((filter, values)) = query_filter(query) else {
            return Ok(0);
        };
        let sql = format!("SELECT COUNT(*) FROM messages WHERE {filter}");
        let count: i64 = self
            .conn
            .query_row(&sql, params_from_iter(values), |row| row.get(0))?;
        Ok(count as usize)
    }

    pub fn message_count_all(&self) -> rusqlite::Result<usize> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))?;
        Ok(count as usize)
    }
}

/// SQL `WHERE` clause + parameters for a list query; `None` means "no rows"
/// (blank search).
fn query_filter(query: &MailListQuery) -> Option<(&'static str, Vec<Value>)> {
    match query {
        MailListQuery::Search(text) => search_filter(text),
        MailListQuery::GlobalSystemFolder(path) => Some((
            "folders_csv LIKE ?1",
            vec![Value::Text(folder_like_pattern(path))],
        )),
        MailListQuery::AccountFolder {
            account_id,
            folder_path,
        } => Some((
            "account_id = ?1 AND folders_csv LIKE ?2",
            vec![
                Value::Integer(*account_id),
                Value::Text(folder_like_pattern(folder_path)),
            ],
        )),
    }
}

/// Accent/case-insensitive substring search over `search_text`.
///
/// Queries of [`FTS_MIN_CHARS`]+ characters go through the trigram FTS index
/// (a quoted phrase is a substring match there); shorter ones fall back to
/// `LIKE`, which trigrams cannot answer.
fn search_filter(query: &str) -> Option<(&'static str, Vec<Value>)> {
    let folded = fold_for_search(query.trim());
    if folded.is_empty() {
        return None;
    }
    if folded.chars().count() >= FTS_MIN_CHARS {
        Some((
            "id IN (SELECT rowid FROM messages_fts WHERE messages_fts MATCH ?1)",
            vec![Value::Text(fts_phrase(&folded))],
        ))
    } else {
        Some((
            "search_text LIKE ?1",
            vec![Value::Text(format!("%{folded}%"))],
        ))
    }
}

/// Wraps `text` as one FTS5 string (phrase), escaping embedded quotes, so user
/// input is never parsed as FTS query syntax.
fn fts_phrase(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\"\""))
}

/// Maps a row selected with [`LIST_COLUMNS`].
fn list_item_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageListItem> {
    Ok(MessageListItem {
        id: row.get(0)?,
        account_id: row.get(1)?,
        sender: row.get(2)?,
        sender_email: row.get(3)?,
        subject: row.get(4)?,
        preview: row.get(5)?,
        received_at: row.get(6)?,
        date_ts: row.get(7)?,
        thread_id: row.get(8)?,
        unread: row.get::<_, i64>(9)? != 0,
        starred: row.get::<_, i64>(10)? != 0,
        has_attachment: row.get::<_, i64>(11)? != 0,
    })
}

/// Maps a [`GlobalMailbox`]-style name to a system folder path.
pub fn global_folder_path(name: &str) -> Option<&'static str> {
    match name {
        "inbox" => Some(system::INBOX),
        "flagged" => Some(system::FLAGGED),
        "drafts" => Some(system::DRAFTS),
        "sent" => Some(system::SENT),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seed::{seed, SeedAccount, SeedMailbox, SeedMessage};
    use tempfile::NamedTempFile;

    #[test]
    fn database_path_uses_dot_config_bgmail() {
        let path = database_path();
        assert!(path.to_string_lossy().contains("BGMail"));
        assert_eq!(path.file_name().unwrap(), "mail.db");
    }

    #[test]
    fn account_folder_lists_only_that_account() {
        let file = NamedTempFile::new().unwrap();
        let db = Database::open(file.path()).unwrap();
        seed(
            db.conn(),
            &[
                SeedAccount {
                    name: "A".into(),
                    email: "a@test.com".into(),
                    mailboxes: vec![SeedMailbox {
                        system_path: Some(system::INBOX),
                        custom_name: None,
                        unread: 0,
                    }],
                },
                SeedAccount {
                    name: "B".into(),
                    email: "b@test.com".into(),
                    mailboxes: vec![SeedMailbox {
                        system_path: Some(system::INBOX),
                        custom_name: None,
                        unread: 0,
                    }],
                },
            ],
            &[
                SeedMessage {
                    account_email: "a@test.com".into(),
                    sender: "X".into(),
                    sender_email: "x@a.com".into(),
                    subject: "A only".into(),
                    preview: "p".into(),
                    plain_text: "p".into(),
                    raw_content: "p".into(),
                    raw_format: "text",
                    received_at: "1".into(),
                    sort_order: 0,
                    unread: false,
                    starred: false,
                    has_attachment: false,
                    extra_folders: vec![],
                },
                SeedMessage {
                    account_email: "b@test.com".into(),
                    sender: "Y".into(),
                    sender_email: "y@b.com".into(),
                    subject: "B only".into(),
                    preview: "p".into(),
                    plain_text: "p".into(),
                    raw_content: "p".into(),
                    raw_format: "text",
                    received_at: "2".into(),
                    sort_order: 0,
                    unread: false,
                    starred: false,
                    has_attachment: false,
                    extra_folders: vec![],
                },
            ],
        )
        .unwrap();
        let accounts = db.list_accounts().unwrap();
        let a_id = accounts[0].id;
        let list = db
            .list_messages(&MailListQuery::AccountFolder {
                account_id: a_id,
                folder_path: system::INBOX.into(),
            })
            .unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].subject, "A only");
    }

    fn search_ids(db: &Database, query: &str) -> Vec<i64> {
        let mut ids: Vec<i64> = db
            .list_messages(&MailListQuery::Search(query.into()))
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect();
        ids.sort_unstable();
        assert_eq!(
            db.message_count_for_query(&MailListQuery::Search(query.into()))
                .unwrap(),
            ids.len(),
            "count/list mismatch for {query:?}"
        );
        ids
    }

    /// Same query answered with the v1 `LIKE` scan, as the parity oracle.
    fn like_ids(db: &Database, query: &str) -> Vec<i64> {
        let Some(pattern) = crate::search::search_like_pattern(query) else {
            return Vec::new();
        };
        let mut stmt = db
            .conn()
            .prepare("SELECT id FROM messages WHERE search_text LIKE ?1 ORDER BY id")
            .unwrap();
        stmt.query_map(params![pattern], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<i64>>>()
            .unwrap()
    }

    #[test]
    fn fts_search_matches_like_semantics() {
        let (_file, db, fx) = crate::test_support::remote_db();
        let mut ids = Vec::new();
        for (uid, (subject, body)) in [
            ("Relatório anual", "São Paulo café"),
            ("GitHub notification", "Pull request merged"),
            ("Ação necessária", "Por favor confirme"),
            ("Quote \"test\"", "x"),
        ]
        .into_iter()
        .enumerate()
        {
            let mut message = crate::test_support::remote_message(subject, None);
            message.plain_text = body.into();
            ids.push(
                db.insert_remote_message(fx.account, fx.inbox, uid as u32 + 1, &message)
                    .unwrap(),
            );
        }
        for query in [
            "relatorio",
            "RELATÓRIO",
            "latór",
            "sao paulo",
            "o pa",
            "CAFÉ",
            "github",
            "ithu",
            "merged",
            "acao",
            "ão",
            "a",
            "gi",
            "   ",
            "",
            "nothing here",
            "\"test\"",
            "quote \"",
            "sender@test",
            "AND",
            "OR x",
        ] {
            assert_eq!(
                search_ids(&db, query),
                like_ids(&db, query),
                "query {query:?}"
            );
        }
        assert_eq!(search_ids(&db, "relatorio"), vec![ids[0]]);
        assert_eq!(search_ids(&db, "ão"), vec![ids[0], ids[2]]);
        assert!(search_ids(&db, "  ").is_empty());
    }

    #[test]
    fn message_count_for_query_counts_without_loading() {
        let (_file, db, fx) = crate::test_support::remote_db();
        for uid in 1..=3 {
            db.insert_remote_message(
                fx.account,
                fx.inbox,
                uid,
                &crate::test_support::remote_message(&format!("m{uid}"), None),
            )
            .unwrap();
        }
        let query = MailListQuery::AccountFolder {
            account_id: fx.account,
            folder_path: system::INBOX.into(),
        };
        assert_eq!(db.message_count_for_query(&query).unwrap(), 3);
        assert_eq!(
            db.message_count_for_query(&MailListQuery::GlobalSystemFolder(system::TRASH.into()))
                .unwrap(),
            0
        );
    }

    #[test]
    fn unread_count_total_spans_accounts() {
        let (_file, db, fx) = crate::test_support::remote_db();
        let mut message = crate::test_support::remote_message("u", None);
        message.unread = true;
        db.insert_remote_message(fx.account, fx.inbox, 1, &message)
            .unwrap();
        db.insert_remote_message(fx.local_account, fx.local_inbox, 1, &message)
            .unwrap();
        assert_eq!(db.unread_count_total().unwrap(), 2);
        assert_eq!(db.global_unread(system::INBOX).unwrap(), 2);
        assert_eq!(
            db.unread_in_folder(Some(fx.account), system::INBOX)
                .unwrap(),
            1
        );
    }

    #[test]
    fn open_enables_wal() {
        let file = NamedTempFile::new().unwrap();
        let db = Database::open(file.path()).unwrap();
        let mode: String = db
            .conn()
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn two_connections_read_and_write_concurrently() {
        let (file, ui, fx) = crate::test_support::remote_db();
        let path = file.path().to_path_buf();
        let worker = std::thread::spawn(move || {
            let sync = Database::open(&path).unwrap();
            for uid in 1..=50 {
                sync.insert_remote_message(
                    fx.account,
                    fx.inbox,
                    uid,
                    &crate::test_support::remote_message(&format!("s{uid}"), None),
                )
                .unwrap();
            }
        });
        // The UI keeps reading and writing while the sync thread inserts.
        for _ in 0..50 {
            ui.list_messages(&MailListQuery::GlobalSystemFolder(system::INBOX.into()))
                .unwrap();
            ui.enqueue_op(fx.account, "noop", None, "{}").unwrap();
        }
        worker.join().unwrap();
        assert_eq!(ui.message_count_all().unwrap(), 50);
        assert_eq!(ui.pending_ops(fx.account).unwrap().len(), 50);
    }
}
