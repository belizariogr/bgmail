//! Imports the visual mock into an empty database.

use rusqlite::{params, Connection};

use crate::folder::{self, system};
use crate::search::build_search_text;

/// Input for a mailbox row when seeding (mirrors the old `data::Mailbox`).
#[derive(Debug, Clone)]
pub struct SeedMailbox {
    pub system_path: Option<&'static str>,
    pub custom_name: Option<String>,
    pub unread: usize,
}

/// Input for an account when seeding.
#[derive(Debug, Clone)]
pub struct SeedAccount {
    pub name: String,
    pub email: String,
    pub mailboxes: Vec<SeedMailbox>,
}

/// Input for one message when seeding.
#[derive(Debug, Clone)]
pub struct SeedMessage {
    pub account_email: String,
    pub sender: String,
    pub sender_email: String,
    pub subject: String,
    pub preview: String,
    pub plain_text: String,
    pub raw_content: String,
    pub raw_format: &'static str,
    pub received_at: String,
    pub sort_order: i64,
    pub unread: bool,
    pub starred: bool,
    pub has_attachment: bool,
    pub extra_folders: Vec<String>,
}

/// Populates `conn` when it has no accounts. Returns whether seeding ran.
pub fn seed_if_empty(
    conn: &Connection,
    accounts: &[SeedAccount],
    messages: &[SeedMessage],
) -> rusqlite::Result<bool> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM accounts", [], |row| row.get(0))?;
    if count > 0 {
        return Ok(false);
    }
    seed(conn, accounts, messages)?;
    Ok(true)
}

/// Inserts accounts, folders and messages (assumes an empty database).
pub fn seed(
    conn: &Connection,
    accounts: &[SeedAccount],
    messages: &[SeedMessage],
) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;

    let mut account_ids = Vec::new();
    for account in accounts {
        tx.execute(
            "INSERT INTO accounts (name, email) VALUES (?1, ?2)",
            params![account.name, account.email],
        )?;
        let account_id = tx.last_insert_rowid();
        account_ids.push((account.email.clone(), account_id));

        for mailbox in &account.mailboxes {
            let (path, display_name) = if let Some(path) = mailbox.system_path {
                (path.to_string(), String::new())
            } else if let Some(name) = &mailbox.custom_name {
                (folder::user_folder_path(name), name.clone())
            } else {
                continue;
            };
            tx.execute(
                "INSERT INTO folders (account_id, path, display_name) VALUES (?1, ?2, ?3)",
                params![account_id, path, display_name],
            )?;
        }
    }

    for message in messages {
        let account_id = account_ids
            .iter()
            .find(|(email, _)| email == &message.account_email)
            .map(|(_, id)| *id)
            .ok_or_else(|| rusqlite::Error::InvalidParameterName(message.account_email.clone()))?;

        let mut folder_paths = vec![system::INBOX.to_string()];
        if message.starred {
            folder_paths.push(system::FLAGGED.to_string());
        }
        folder_paths.extend(message.extra_folders.iter().cloned());
        folder_paths.sort();
        folder_paths.dedup();
        let folders_csv = folder::folders_csv(folder_paths.iter().map(String::as_str));

        let search_text = build_search_text(
            &message.sender,
            &message.sender_email,
            &message.subject,
            &message.plain_text,
        );

        tx.execute(
            "INSERT INTO messages (
                account_id, sender, sender_email, subject,
                plain_text, raw_content, raw_format, preview, search_text,
                received_at, sort_order, unread, starred, has_attachment, folders_csv
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                account_id,
                message.sender,
                message.sender_email,
                message.subject,
                message.plain_text,
                message.raw_content,
                message.raw_format,
                message.preview,
                search_text,
                message.received_at,
                message.sort_order,
                message.unread as i64,
                message.starred as i64,
                message.has_attachment as i64,
                folders_csv,
            ],
        )?;
    }

    tx.commit()?;
    Ok(())
}

/// Derives a plain-text excerpt for the list preview.
pub fn preview_from_plain(plain: &str, max_chars: usize) -> String {
    let line = plain
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(plain);
    let trimmed = line.trim();
    if trimmed.chars().count() <= max_chars {
        trimmed.to_string()
    } else {
        let end = trimmed
            .char_indices()
            .nth(max_chars)
            .map(|(i, _)| i)
            .unwrap_or(trimmed.len());
        format!("{}…", &trimmed[..end])
    }
}

/// Plain text extracted from raw HTML (minimal tag strip) for storage/search.
pub fn plain_text_from_raw(raw: &str, raw_format: &str) -> String {
    if raw_format == "text" {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len());
    let mut in_tag = false;
    for ch in raw.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::folder::system;
    use crate::search::fold_for_search;
    use tempfile::NamedTempFile;

    fn sample_seed() -> (Vec<SeedAccount>, Vec<SeedMessage>) {
        let accounts = vec![SeedAccount {
            name: "Personal".into(),
            email: "you@gmail.com".into(),
            mailboxes: vec![
                SeedMailbox {
                    system_path: Some(system::INBOX),
                    custom_name: None,
                    unread: 2,
                },
                SeedMailbox {
                    system_path: Some(system::DRAFTS),
                    custom_name: None,
                    unread: 0,
                },
            ],
        }];
        let messages = vec![
            SeedMessage {
                account_email: "you@gmail.com".into(),
                sender: "Alice".into(),
                sender_email: "alice@example.com".into(),
                subject: "Olá mundo".into(),
                preview: "Corpo em português".into(),
                plain_text: "Corpo em português com acentuação".into(),
                raw_content: "<p>Corpo em português com acentuação</p>".into(),
                raw_format: "html",
                received_at: "09:00".into(),
                sort_order: 0,
                unread: true,
                starred: false,
                has_attachment: false,
                extra_folders: vec![],
            },
            SeedMessage {
                account_email: "you@gmail.com".into(),
                sender: "Bob".into(),
                sender_email: "bob@example.com".into(),
                subject: "Starred".into(),
                preview: "Important".into(),
                plain_text: "Important message".into(),
                raw_content: "Important message".into(),
                raw_format: "text",
                received_at: "08:00".into(),
                sort_order: 1,
                unread: false,
                starred: true,
                has_attachment: false,
                extra_folders: vec![],
            },
        ];
        (accounts, messages)
    }

    #[test]
    fn seed_if_empty_populates_database() {
        let file = NamedTempFile::new().unwrap();
        let db = Database::open(file.path()).unwrap();
        let (accounts, messages) = sample_seed();
        assert!(seed_if_empty(db.conn(), &accounts, &messages).unwrap());
        assert!(!seed_if_empty(db.conn(), &accounts, &messages).unwrap());
        assert_eq!(db.list_accounts().unwrap().len(), 1);
        assert_eq!(
            db.list_messages(&crate::types::MailListQuery::GlobalSystemFolder(
                system::INBOX.into()
            ))
            .unwrap()
            .len(),
            2
        );
    }

    #[test]
    fn search_finds_accent_insensitive_match() {
        let file = NamedTempFile::new().unwrap();
        let db = Database::open(file.path()).unwrap();
        let (accounts, messages) = sample_seed();
        seed(db.conn(), &accounts, &messages).unwrap();
        let hits = db
            .list_messages(&crate::types::MailListQuery::Search("portugues".into()))
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert!(fold_for_search(&hits[0].subject).contains("ola"));
    }
}
