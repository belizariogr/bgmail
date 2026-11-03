//! SQLite mail store: connection setup and list/detail queries.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::types::Value;
use rusqlite::{
    params, params_from_iter, Connection, OptionalExtension, Transaction, TransactionBehavior,
};

use crate::folder::{folder_csv_needle, system};
use crate::search::{collapse_spaces, fold_for_search, search_like_pattern};
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
    received_at, date_ts, thread_id, unread, starred, has_attachment, folders_csv";

/// Newest first; mock rows share `date_ts = 0`, so `sort_order` orders them.
const LIST_ORDER: &str = "ORDER BY date_ts DESC, sort_order ASC, id DESC";

/// Folded queries shorter than this cannot use the trigram index.
const FTS_MIN_CHARS: usize = 3;

/// Local SQLite mail database.
pub struct Database {
    conn: Connection,
}

/// All of the user's mail is in there: other users of the machine can't read it. SQLite
/// gives its `-wal` and `-shm` files the mode of the database. Best effort: nothing to do
/// about a file system that doesn't allow it.
#[cfg(unix)]
fn keep_private(path: &Path) {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let private = std::fs::Permissions::from_mode(0o600);
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path);
    let _ = std::fs::set_permissions(path, private.clone());
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = path.as_os_str().to_owned();
        sidecar.push(suffix);
        let _ = std::fs::set_permissions(&sidecar, private.clone());
    }
    if path.parent() == Some(config_dir().as_path()) {
        let _ = std::fs::set_permissions(config_dir(), std::fs::Permissions::from_mode(0o700));
    }
}

#[cfg(not(unix))]
fn keep_private(_: &Path) {}

impl Database {
    /// Opens (or creates) the database at `path` and applies migrations.
    ///
    /// WAL + a busy timeout let the UI and a background sync thread each hold
    /// their own connection to the same file.
    pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        keep_private(path.as_ref());
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

    /// [`Self::open`] that survives a damaged file: it is kept beside as `<name>.corrupt`, with
    /// its journals, so it can still be recovered, and a new one starts in its place; the second
    /// value says so. Aborting would leave the app unable to start, with no message, for good.
    /// Thunderbird rebuilds its damaged index files the same way. Other errors (a full disk, no
    /// permission) aren't about the file, which stays untouched.
    pub fn open_or_replace_corrupt(path: impl AsRef<Path>) -> rusqlite::Result<(Self, bool)> {
        use rusqlite::ErrorCode::{DatabaseCorrupt, NotADatabase};
        let path = path.as_ref();
        match Self::open(path) {
            Err(rusqlite::Error::SqliteFailure(error, _))
                if matches!(error.code, NotADatabase | DatabaseCorrupt) =>
            {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let free = (0..)
                    .map(|n| match n {
                        0 => format!("{name}.corrupt"),
                        n => format!("{name}.corrupt-{n}"),
                    })
                    .find(|kept| !path.with_file_name(kept).exists())
                    .unwrap_or_default();
                for suffix in ["", "-wal", "-shm"] {
                    let _ = std::fs::rename(
                        path.with_file_name(format!("{name}{suffix}")),
                        path.with_file_name(format!("{free}{suffix}")),
                    );
                }
                Ok((Self::open(path)?, true))
            }
            opened => opened.map(|db| (db, false)),
        }
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

    /// Runs `body` in one write transaction, committed when it returns `Ok`.
    /// For callers outside this crate whose read-then-write must not
    /// interleave with the other connection; `body` must not open another
    /// transaction.
    pub fn atomically<T>(&self, body: impl FnOnce() -> rusqlite::Result<T>) -> rusqlite::Result<T> {
        let tx = self.write_transaction()?;
        let value = body()?;
        tx.commit()?;
        Ok(value)
    }

    fn migrate(&self) -> rusqlite::Result<()> {
        crate::migration::migrate(&self.conn)?;
        self.repair_starred_folder_membership()
    }

    /// Ensures starred messages appear in the global flagged folder.
    fn repair_starred_folder_membership(&self) -> rusqlite::Result<()> {
        use crate::folder::folders_csv;

        let needle = folder_csv_needle(system::FLAGGED);
        let mut stmt = self.conn.prepare(
            "SELECT id, folders_csv FROM messages
             WHERE starred = 1 AND instr(folders_csv, ?1) = 0",
        )?;
        let rows = stmt.query_map(params![needle], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        let updates: Vec<(String, i64)> = rows
            .filter_map(|row| row.ok())
            .map(|(id, csv)| {
                let mut paths = crate::message_ops::parse_folders_csv(&csv);
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
        let needle = folder_csv_needle(folder_path);
        let hidden = hidden_from_folder(folder_path);
        let sql = if account_id.is_some() {
            format!(
                "SELECT COUNT(*) FROM messages
                 WHERE account_id = ?1 AND unread = 1 AND instr(folders_csv, ?2) > 0{hidden}"
            )
        } else {
            format!(
                "SELECT COUNT(*) FROM messages WHERE unread = 1 AND instr(folders_csv, ?1) > 0{hidden}"
            )
        };
        let count: i64 = if let Some(account_id) = account_id {
            self.conn
                .query_row(&sql, params![account_id, needle], |row| row.get(0))?
        } else {
            self.conn
                .query_row(&sql, params![needle], |row| row.get(0))?
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

/// Extra condition of a folder's listing: deleted mail keeps its star, for when
/// it is restored, but no longer belongs to Starred.
fn hidden_from_folder(folder_path: &str) -> &'static str {
    if folder_path == system::FLAGGED {
        " AND folders_csv NOT LIKE '%,sys:trash,%'"
    } else {
        ""
    }
}

/// SQL `WHERE` clause + parameters for a list query; `None` means "no rows"
/// (blank search).
fn query_filter(query: &MailListQuery) -> Option<(&'static str, Vec<Value>)> {
    match query {
        MailListQuery::Search(text) => search_filter(text),
        MailListQuery::GlobalSystemFolder(path) if path == system::FLAGGED => Some((
            "instr(folders_csv, ?1) > 0 AND folders_csv NOT LIKE '%,sys:trash,%'",
            vec![Value::Text(folder_csv_needle(path))],
        )),
        MailListQuery::GlobalSystemFolder(path) => Some((
            "instr(folders_csv, ?1) > 0",
            vec![Value::Text(folder_csv_needle(path))],
        )),
        MailListQuery::AccountFolder {
            account_id,
            folder_path,
        } => Some((
            "account_id = ?1 AND instr(folders_csv, ?2) > 0",
            vec![
                Value::Integer(*account_id),
                Value::Text(folder_csv_needle(folder_path)),
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
    let folded = collapse_spaces(&fold_for_search(query));
    if folded.is_empty() {
        return None;
    }
    // A phrase costs its trigrams: 64 KB pasted here took 12 s and 2 GB. What starts with this
    // is still found, among the few things that also do.
    let folded: String = folded.chars().take(MAX_QUERY_CHARS).collect();
    if folded.chars().count() >= FTS_MIN_CHARS {
        Some((
            "id IN (SELECT rowid FROM messages_fts WHERE messages_fts MATCH ?1)",
            vec![Value::Text(fts_phrase(&folded))],
        ))
    } else {
        Some((
            "search_text LIKE ?1 ESCAPE '\\'",
            vec![Value::Text(search_like_pattern(&folded)?)],
        ))
    }
}

/// Longest search looked for.
const MAX_QUERY_CHARS: usize = 1024;

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
        folders_csv: row.get(12)?,
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
            .prepare("SELECT id FROM messages WHERE search_text LIKE ?1 ESCAPE '\\' ORDER BY id")
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

    /// One message in each of `paths`, of the fixture's remote account.
    fn messages_in_folders(paths: &[&str]) -> (tempfile::NamedTempFile, Database, i64) {
        let (file, db, fx) = crate::test_support::remote_db();
        for (ix, path) in paths.iter().enumerate() {
            let folder = db
                .upsert_remote_folder(fx.account, path, "", path, None)
                .unwrap();
            let message = crate::types::NewRemoteMessage {
                unread: true,
                ..crate::test_support::remote_message(&format!("m{ix}"), None)
            };
            db.insert_remote_message(fx.account, folder, 1, &message)
                .unwrap();
        }
        (file, db, fx.account)
    }

    fn folder_subjects(db: &Database, account_id: i64, folder_path: &str) -> Vec<String> {
        let query = MailListQuery::AccountFolder {
            account_id,
            folder_path: folder_path.into(),
        };
        db.list_messages(&query)
            .unwrap()
            .into_iter()
            .map(|message| message.subject)
            .collect()
    }

    #[test]
    fn a_folder_lists_only_its_own_messages_whatever_its_name() {
        // Wildcards of `LIKE`, its case folding, and the separator of `folders_csv`.
        let (_file, db, account) = messages_in_folders(&[
            "user:My_Work",
            "user:MyXWork",
            "user:100%",
            "user:1000",
            "user:work",
            "user:WORK",
            "user:Clients",
            "user:Clients, VIP",
        ]);
        for (ix, path) in [
            "user:My_Work",
            "user:MyXWork",
            "user:100%",
            "user:1000",
            "user:work",
            "user:WORK",
            "user:Clients",
            "user:Clients, VIP",
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(folder_subjects(&db, account, path), [format!("m{ix}")]);
            assert_eq!(db.unread_in_folder(Some(account), path).unwrap(), 1);
        }
    }

    #[test]
    fn a_folder_named_after_the_trash_does_not_make_a_message_trash() {
        let (_file, db, account) = messages_in_folders(&["user:x,sys:trash"]);
        let query = MailListQuery::AccountFolder {
            account_id: account,
            folder_path: "user:x,sys:trash".into(),
        };
        let id = db.list_messages(&query).unwrap()[0].id;
        assert!(!db.message_is_in_trash(id).unwrap());
        let message = db.get_message(id).unwrap().unwrap();
        assert_eq!(
            crate::parse_folders_csv(&message.folders_csv),
            ["user:x,sys:trash"]
        );
        assert!(crate::message_has_folder(
            &message.folders_csv,
            "user:x,sys:trash"
        ));
        assert!(!crate::message_has_folder(&message.folders_csv, "user:x"));
    }

    #[cfg(unix)]
    #[test]
    fn only_its_owner_can_read_the_database() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mail.db");
        let mode = |file: &Path| std::fs::metadata(file).unwrap().permissions().mode() & 0o777;
        drop(Database::open(&path).unwrap());
        assert_eq!(mode(&path), 0o600);

        // One created before this keeps nothing open to others either.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let db = Database::open(&path).unwrap();
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&dir.path().join("mail.db-wal")), 0o600);
        drop(db);
    }

    #[test]
    fn a_phrase_is_found_across_a_line_break() {
        let (_file, db, fx) = crate::test_support::remote_db();
        let id = db
            .insert_remote_message(
                fx.account,
                fx.inbox,
                1,
                &crate::test_support::remote_message("s", None),
            )
            .unwrap();
        db.conn()
            .execute(
                "UPDATE messages SET search_text = ?1 WHERE id = ?2",
                params![
                    crate::search::build_search_text("", "", "", "encontro na reuniao\nde amanha"),
                    id
                ],
            )
            .unwrap();
        for query in ["reuniao de amanha", "reuniao   de\tamanha"] {
            let found = db
                .list_messages(&MailListQuery::Search(query.into()))
                .unwrap();
            assert_eq!(found.len(), 1, "{query}");
        }
    }

    #[test]
    fn a_short_search_takes_its_wildcards_literally() {
        let (_file, db, fx) = crate::test_support::remote_db();
        for (uid, subject) in [(1, "a_b"), (2, "axb"), (3, "100%")] {
            db.insert_remote_message(
                fx.account,
                fx.inbox,
                uid,
                &crate::test_support::remote_message(subject, None),
            )
            .unwrap();
        }
        let subjects = |query: &str| -> Vec<String> {
            db.list_messages(&MailListQuery::Search(query.into()))
                .unwrap()
                .into_iter()
                .map(|message| message.subject)
                .collect()
        };
        assert_eq!(subjects("a_"), ["a_b"]);
        assert_eq!(subjects("%"), ["100%"]);
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

    #[test]
    fn a_damaged_database_is_kept_aside_and_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mail.db");
        std::fs::write(&path, vec![b'x'; 4096]).unwrap();
        let (_, replaced) = Database::open_or_replace_corrupt(&path).unwrap();
        assert!(replaced);
        assert_eq!(
            std::fs::read(dir.path().join("mail.db.corrupt")).unwrap(),
            vec![b'x'; 4096]
        );
        // A good one stays, and a second damaged one keeps the first copy.
        let (_, replaced) = Database::open_or_replace_corrupt(&path).unwrap();
        assert!(!replaced);
        std::fs::remove_file(dir.path().join("mail.db-wal")).ok();
        std::fs::remove_file(dir.path().join("mail.db-shm")).ok();
        std::fs::write(&path, vec![b'y'; 4096]).unwrap();
        assert!(Database::open_or_replace_corrupt(&path).unwrap().1);
        assert!(dir.path().join("mail.db.corrupt-1").exists());
    }
}
