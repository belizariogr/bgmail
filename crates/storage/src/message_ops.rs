//! Folder membership helpers and message mutations.

use rusqlite::params;

use crate::folder::{folder_like_pattern, folders_csv, system};
use crate::Database;

/// Parses a `folders_csv` column into folder paths (without empty segments).
pub fn parse_folders_csv(csv: &str) -> Vec<String> {
    csv.split(',')
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether `csv` lists `folder_path`.
pub fn message_has_folder(csv: &str, folder_path: &str) -> bool {
    parse_folders_csv(csv)
        .iter()
        .any(|path| path == folder_path)
}

/// The virtual flagged folder is driven by the `starred` column, not a primary mailbox.
pub fn is_virtual_flagged_folder(path: &str) -> bool {
    path == system::FLAGGED
}

/// Replaces every non-flagged folder with `primary`, keeping flagged membership when present.
pub fn replace_primary_folder(paths: &[String], primary: &str) -> Vec<String> {
    let mut kept: Vec<String> = paths
        .iter()
        .filter(|path| is_virtual_flagged_folder(path))
        .cloned()
        .collect();
    if !kept.iter().any(|path| path == primary) {
        kept.push(primary.to_string());
    }
    kept.sort();
    kept.dedup();
    kept
}

/// Builds a `folders_csv` value from discrete paths.
pub fn folders_csv_from_paths(paths: &[String]) -> String {
    folders_csv(paths.iter().map(String::as_str))
}

impl Database {
    fn load_message_folders(&self, id: i64) -> rusqlite::Result<Option<(Vec<String>, bool)>> {
        let mut stmt = self
            .conn()
            .prepare("SELECT folders_csv, starred FROM messages WHERE id = ?1")?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? != 0))
        })?;
        Ok(rows
            .next()
            .transpose()?
            .map(|(csv, starred)| (parse_folders_csv(&csv), starred)))
    }

    fn set_message_folders(&self, id: i64, paths: &[String]) -> rusqlite::Result<()> {
        let csv = folders_csv_from_paths(paths);
        self.conn()
            .execute(
                "UPDATE messages SET folders_csv = ?1 WHERE id = ?2",
                params![csv, id],
            )
            .map(|_| ())
    }

    fn set_primary_folder(&self, id: i64, primary: &str) -> rusqlite::Result<()> {
        let Some((paths, _starred)) = self.load_message_folders(id)? else {
            return Ok(());
        };
        let updated = replace_primary_folder(&paths, primary);
        self.set_message_folders(id, &updated)
    }

    /// Moves the message to the Trash folder (keeps flagged membership when starred).
    pub fn move_message_to_trash(&self, id: i64) -> rusqlite::Result<()> {
        self.set_primary_folder(id, system::TRASH)
    }

    /// Restores a trashed message to the Inbox.
    pub fn restore_message_from_trash(&self, id: i64) -> rusqlite::Result<()> {
        let Some((paths, _)) = self.load_message_folders(id)? else {
            return Ok(());
        };
        if !message_has_folder(&folders_csv_from_paths(&paths), system::TRASH) {
            return Ok(());
        }
        self.set_primary_folder(id, system::INBOX)
    }

    /// Permanently deletes a message. Only succeeds when the message is in Trash.
    pub fn delete_message_permanently(&self, id: i64) -> rusqlite::Result<bool> {
        let Some((paths, _)) = self.load_message_folders(id)? else {
            return Ok(false);
        };
        if !message_has_folder(&folders_csv_from_paths(&paths), system::TRASH) {
            return Ok(false);
        }
        let changed = self
            .conn()
            .execute("DELETE FROM messages WHERE id = ?1", params![id])?;
        Ok(changed > 0)
    }

    /// Archives the message (primary folder becomes Archive).
    pub fn archive_message(&self, id: i64) -> rusqlite::Result<()> {
        self.set_primary_folder(id, system::ARCHIVE)
    }

    /// Marks the message as junk (primary folder becomes Junk).
    pub fn mark_message_junk(&self, id: i64) -> rusqlite::Result<()> {
        self.set_primary_folder(id, system::JUNK)
    }

    /// Moves the message to an arbitrary folder path for its account.
    pub fn move_message_to_folder(&self, id: i64, folder_path: &str) -> rusqlite::Result<()> {
        if is_virtual_flagged_folder(folder_path) {
            return Ok(());
        }
        self.set_primary_folder(id, folder_path)
    }

    /// Toggles the starred flag and flagged-folder membership.
    pub fn toggle_message_starred(&self, id: i64) -> rusqlite::Result<bool> {
        let Some((mut paths, starred)) = self.load_message_folders(id)? else {
            return Ok(false);
        };
        let new_starred = !starred;
        if new_starred {
            if !paths.iter().any(|path| path == system::FLAGGED) {
                paths.push(system::FLAGGED.to_string());
            }
        } else {
            paths.retain(|path| path != system::FLAGGED);
        }
        paths.sort();
        paths.dedup();
        self.conn().execute(
            "UPDATE messages SET starred = ?1, folders_csv = ?2 WHERE id = ?3",
            params![new_starred as i64, folders_csv_from_paths(&paths), id],
        )?;
        Ok(new_starred)
    }

    /// Returns whether the message currently lives in Trash.
    pub fn message_is_in_trash(&self, id: i64) -> rusqlite::Result<bool> {
        let pattern = folder_like_pattern(system::TRASH);
        let count: i64 = self.conn().query_row(
            "SELECT COUNT(*) FROM messages WHERE id = ?1 AND folders_csv LIKE ?2",
            params![id, pattern],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seed::{seed, SeedAccount, SeedMailbox, SeedMessage};
    use tempfile::NamedTempFile;

    fn seeded_db() -> (NamedTempFile, Database) {
        let file = NamedTempFile::new().unwrap();
        let db = Database::open(file.path()).unwrap();
        seed(
            db.conn(),
            &[SeedAccount {
                name: "Me".into(),
                email: "me@test.com".into(),
                mailboxes: vec![
                    SeedMailbox {
                        system_path: Some(system::INBOX),
                        custom_name: None,
                        unread: 1,
                    },
                    SeedMailbox {
                        system_path: Some(system::TRASH),
                        custom_name: None,
                        unread: 0,
                    },
                    SeedMailbox {
                        system_path: Some(system::ARCHIVE),
                        custom_name: None,
                        unread: 0,
                    },
                    SeedMailbox {
                        system_path: Some(system::JUNK),
                        custom_name: None,
                        unread: 0,
                    },
                    SeedMailbox {
                        system_path: None,
                        custom_name: Some("Clients".into()),
                        unread: 0,
                    },
                ],
            }],
            &[
                SeedMessage {
                    account_email: "me@test.com".into(),
                    sender: "A".into(),
                    sender_email: "a@test.com".into(),
                    subject: "Inbox".into(),
                    preview: "p".into(),
                    plain_text: "p".into(),
                    raw_content: "p".into(),
                    raw_format: "text",
                    received_at: "1".into(),
                    sort_order: 0,
                    unread: true,
                    starred: true,
                    has_attachment: false,
                    extra_folders: vec![],
                },
                SeedMessage {
                    account_email: "me@test.com".into(),
                    sender: "B".into(),
                    sender_email: "b@test.com".into(),
                    subject: "Trashed".into(),
                    preview: "p".into(),
                    plain_text: "p".into(),
                    raw_content: "p".into(),
                    raw_format: "text",
                    received_at: "2".into(),
                    sort_order: 1,
                    unread: false,
                    starred: false,
                    has_attachment: false,
                    extra_folders: vec![system::TRASH.to_string()],
                },
            ],
        )
        .unwrap();
        (file, db)
    }

    fn message_csv(db: &Database, subject: &str) -> String {
        db.conn()
            .query_row(
                "SELECT folders_csv FROM messages WHERE subject = ?1",
                params![subject],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn message_id(db: &Database, subject: &str) -> i64 {
        db.conn()
            .query_row(
                "SELECT id FROM messages WHERE subject = ?1",
                params![subject],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn replace_primary_folder_keeps_flagged() {
        let paths = vec![system::INBOX.to_string(), system::FLAGGED.to_string()];
        let updated = replace_primary_folder(&paths, system::TRASH);
        assert_eq!(
            updated,
            vec![system::FLAGGED.to_string(), system::TRASH.to_string()]
        );
    }

    #[test]
    fn move_to_trash_keeps_flagged_membership() {
        let (_file, db) = seeded_db();
        let id = message_id(&db, "Inbox");
        db.move_message_to_trash(id).unwrap();
        let csv = message_csv(&db, "Inbox");
        assert!(message_has_folder(&csv, system::TRASH));
        assert!(message_has_folder(&csv, system::FLAGGED));
        assert!(!message_has_folder(&csv, system::INBOX));
    }

    #[test]
    fn restore_from_trash_moves_to_inbox() {
        let (_file, db) = seeded_db();
        let id = message_id(&db, "Trashed");
        db.restore_message_from_trash(id).unwrap();
        let csv = message_csv(&db, "Trashed");
        assert!(message_has_folder(&csv, system::INBOX));
        assert!(!message_has_folder(&csv, system::TRASH));
    }

    #[test]
    fn permanent_delete_only_from_trash() {
        let (_file, db) = seeded_db();
        let inbox_id = message_id(&db, "Inbox");
        assert!(!db.delete_message_permanently(inbox_id).unwrap());

        let trash_id = message_id(&db, "Trashed");
        assert!(db.delete_message_permanently(trash_id).unwrap());
        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn archive_and_junk_replace_primary() {
        let (_file, db) = seeded_db();
        let id = message_id(&db, "Inbox");
        db.archive_message(id).unwrap();
        assert!(message_has_folder(
            &message_csv(&db, "Inbox"),
            system::ARCHIVE
        ));

        db.mark_message_junk(id).unwrap();
        assert!(message_has_folder(&message_csv(&db, "Inbox"), system::JUNK));
    }

    #[test]
    fn toggle_starred_updates_flagged_folder() {
        let (_file, db) = seeded_db();
        let id = message_id(&db, "Inbox");
        assert!(db.toggle_message_starred(id).unwrap() == false);
        let csv = message_csv(&db, "Inbox");
        assert!(!message_has_folder(&csv, system::FLAGGED));

        assert!(db.toggle_message_starred(id).unwrap());
        assert!(message_has_folder(
            &message_csv(&db, "Inbox"),
            system::FLAGGED
        ));
    }

    #[test]
    fn move_to_custom_folder() {
        let (_file, db) = seeded_db();
        let id = message_id(&db, "Inbox");
        let custom = crate::folder::user_folder_path("Clients");
        db.move_message_to_folder(id, &custom).unwrap();
        let csv = message_csv(&db, "Inbox");
        assert!(message_has_folder(&csv, &custom));
        assert!(!message_has_folder(&csv, system::INBOX));
    }
}
