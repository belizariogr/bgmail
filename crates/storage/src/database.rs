//! SQLite mail store.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use crate::folder::{folder_like_pattern, system};
use crate::schema::{
    CREATE_ACCOUNTS, CREATE_FOLDERS, CREATE_MESSAGES, CREATE_META, INDEX_MESSAGES_ACCOUNT,
    INDEX_MESSAGES_FOLDERS, INDEX_MESSAGES_SEARCH, SCHEMA_VERSION,
};
use crate::search::search_like_pattern;
use crate::types::{Account, Folder, MailListQuery, MessageDetail, MessageListItem};

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

/// Local SQLite mail database.
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Opens (or creates) the database at `path` and applies migrations.
    pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
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

    fn migrate(&self) -> rusqlite::Result<()> {
        self.conn.execute_batch(CREATE_META)?;
        self.conn.execute_batch(CREATE_ACCOUNTS)?;
        self.conn.execute_batch(CREATE_FOLDERS)?;
        self.conn.execute_batch(CREATE_MESSAGES)?;
        self.conn.execute_batch(INDEX_MESSAGES_SEARCH)?;
        self.conn.execute_batch(INDEX_MESSAGES_ACCOUNT)?;
        self.conn.execute_batch(INDEX_MESSAGES_FOLDERS)?;

        let version: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .ok();
        if version.is_none() {
            self.conn.execute(
                "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)",
                params![SCHEMA_VERSION.to_string()],
            )?;
        }
        self.repair_starred_folder_membership()?;
        Ok(())
    }

    /// Ensures starred messages appear in the global flagged folder.
    fn repair_starred_folder_membership(&self) -> rusqlite::Result<()> {
        use crate::folder::{folder_like_pattern, folders_csv, system};

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

    pub fn list_accounts(&self) -> rusqlite::Result<Vec<Account>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, email FROM accounts ORDER BY id ASC")?;
        let rows = stmt.query_map([], |row| {
            Ok(Account {
                id: row.get(0)?,
                name: row.get(1)?,
                email: row.get(2)?,
            })
        })?;
        rows.collect()
    }

    pub fn list_folders(&self, account_id: i64) -> rusqlite::Result<Vec<Folder>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, account_id, path, display_name
             FROM folders WHERE account_id = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![account_id], |row| {
            Ok(Folder {
                id: row.get(0)?,
                account_id: row.get(1)?,
                path: row.get(2)?,
                display_name: row.get(3)?,
            })
        })?;
        rows.collect()
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

    pub fn list_messages(&self, query: &MailListQuery) -> rusqlite::Result<Vec<MessageListItem>> {
        match query {
            MailListQuery::Search(q) => self.search_messages(q),
            MailListQuery::GlobalSystemFolder(path) => self.messages_in_folder(None, path),
            MailListQuery::AccountFolder {
                account_id,
                folder_path,
            } => self.messages_in_folder(Some(*account_id), folder_path),
        }
    }

    fn messages_in_folder(
        &self,
        account_id: Option<i64>,
        folder_path: &str,
    ) -> rusqlite::Result<Vec<MessageListItem>> {
        let pattern = folder_like_pattern(folder_path);
        let sql = if account_id.is_some() {
            "SELECT id, account_id, sender, sender_email, subject, preview,
                    received_at, unread, starred, has_attachment
             FROM messages
             WHERE account_id = ?1 AND folders_csv LIKE ?2
             ORDER BY sort_order ASC, id ASC"
        } else {
            "SELECT id, account_id, sender, sender_email, subject, preview,
                    received_at, unread, starred, has_attachment
             FROM messages
             WHERE folders_csv LIKE ?1
             ORDER BY sort_order ASC, id ASC"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let map = |row: &rusqlite::Row<'_>| {
            Ok(MessageListItem {
                id: row.get(0)?,
                account_id: row.get(1)?,
                sender: row.get(2)?,
                sender_email: row.get(3)?,
                subject: row.get(4)?,
                preview: row.get(5)?,
                received_at: row.get(6)?,
                unread: row.get::<_, i64>(7)? != 0,
                starred: row.get::<_, i64>(8)? != 0,
                has_attachment: row.get::<_, i64>(9)? != 0,
            })
        };
        if let Some(account_id) = account_id {
            let rows = stmt.query_map(params![account_id, pattern], map)?;
            rows.collect()
        } else {
            let rows = stmt.query_map(params![pattern], map)?;
            rows.collect()
        }
    }

    fn search_messages(&self, query: &str) -> rusqlite::Result<Vec<MessageListItem>> {
        let Some(pattern) = search_like_pattern(query) else {
            return Ok(Vec::new());
        };
        let mut stmt = self.conn.prepare(
            "SELECT id, account_id, sender, sender_email, subject, preview,
                    received_at, unread, starred, has_attachment
             FROM messages
             WHERE search_text LIKE ?1
             ORDER BY sort_order ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![pattern], |row| {
            Ok(MessageListItem {
                id: row.get(0)?,
                account_id: row.get(1)?,
                sender: row.get(2)?,
                sender_email: row.get(3)?,
                subject: row.get(4)?,
                preview: row.get(5)?,
                received_at: row.get(6)?,
                unread: row.get::<_, i64>(7)? != 0,
                starred: row.get::<_, i64>(8)? != 0,
                has_attachment: row.get::<_, i64>(9)? != 0,
            })
        })?;
        rows.collect()
    }

    pub fn get_message(&self, id: i64) -> rusqlite::Result<Option<MessageDetail>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, account_id, sender, sender_email, subject,
                    plain_text, raw_content, raw_format, received_at,
                    unread, starred, has_attachment, folders_csv
             FROM messages WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
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
            })
        })?;
        rows.next().transpose()
    }

    pub fn message_count_for_query(&self, query: &MailListQuery) -> rusqlite::Result<usize> {
        Ok(self.list_messages(query)?.len())
    }
    pub fn message_count_all(&self) -> rusqlite::Result<usize> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))?;
        Ok(count as usize)
    }
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
}
