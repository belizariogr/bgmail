//! Accounts and folders (including IMAP folder sync checkpoints).

use rusqlite::{params, OptionalExtension};

use crate::folder::{folder_csv_needle, user_folder_path};
use crate::membership::{message_ids_in_folder, settle_after_location_loss};
use crate::types::{Account, Folder, FolderSyncState};
use crate::Database;

const ACCOUNT_COLUMNS: &str = "id, name, email, provider, config_json, sender_name";
const FOLDER_COLUMNS: &str = "id, account_id, path, display_name, remote_name, role";

fn account_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Account> {
    Ok(Account {
        id: row.get(0)?,
        name: crate::database::lossy_text(row, 1)?,
        email: crate::database::lossy_text(row, 2)?,
        provider: crate::database::lossy_text(row, 3)?,
        config_json: crate::database::lossy_text(row, 4)?,
        sender_name: crate::database::lossy_text(row, 5)?,
    })
}

fn folder_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Folder> {
    Ok(Folder {
        id: row.get(0)?,
        account_id: row.get(1)?,
        path: crate::database::lossy_text(row, 2)?,
        display_name: crate::database::lossy_text(row, 3)?,
        remote_name: row.get(4)?,
        role: row.get(5)?,
    })
}

impl Database {
    /// Every account, in sidebar order.
    pub fn list_accounts(&self) -> rusqlite::Result<Vec<Account>> {
        let mut stmt = self.conn().prepare(&format!(
            "SELECT {ACCOUNT_COLUMNS} FROM accounts ORDER BY sort_order ASC, id ASC"
        ))?;
        let rows = stmt.query_map([], account_from_row)?;
        rows.collect()
    }

    pub fn get_account(&self, id: i64) -> rusqlite::Result<Option<Account>> {
        self.conn()
            .query_row(
                &format!("SELECT {ACCOUNT_COLUMNS} FROM accounts WHERE id = ?1"),
                params![id],
                account_from_row,
            )
            .optional()
    }

    /// Adds an account after the existing ones. Fails when `email` is taken.
    pub fn create_account(
        &self,
        name: &str,
        email: &str,
        provider: &str,
        config_json: &str,
    ) -> rusqlite::Result<i64> {
        self.conn().execute(
            "INSERT INTO accounts (name, email, provider, config_json, sort_order)
             VALUES (?1, ?2, ?3, ?4,
                     (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM accounts))",
            params![name, email, provider, config_json],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    pub fn update_account(&self, id: i64, name: &str, config_json: &str) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE accounts SET name = ?1, config_json = ?2 WHERE id = ?3",
            params![name, config_json, id],
        )?;
        Ok(())
    }

    /// Sets the names of an account: what it is listed under and what its mail goes
    /// out with.
    pub fn set_account_names(
        &self,
        id: i64,
        name: &str,
        sender_name: &str,
    ) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE accounts SET name = ?1, sender_name = ?2 WHERE id = ?3",
            params![name, sender_name, id],
        )?;
        Ok(())
    }

    /// Deletes the account and everything it owns (folders, messages with their
    /// locations/bodies/attachments, queued ops, outbox).
    pub fn delete_account(&self, id: i64) -> rusqlite::Result<()> {
        let tx = self.write_transaction()?;
        // Messages first, explicitly, so their FTS delete trigger runs per row.
        tx.execute("DELETE FROM messages WHERE account_id = ?1", params![id])?;
        tx.execute("DELETE FROM accounts WHERE id = ?1", params![id])?;
        tx.commit()
    }

    pub fn list_folders(&self, account_id: i64) -> rusqlite::Result<Vec<Folder>> {
        let mut stmt = self.conn().prepare(&format!(
            "SELECT {FOLDER_COLUMNS} FROM folders WHERE account_id = ?1 ORDER BY id ASC"
        ))?;
        let rows = stmt.query_map(params![account_id], folder_from_row)?;
        rows.collect()
    }

    pub fn folder_by_id(&self, folder_id: i64) -> rusqlite::Result<Option<Folder>> {
        self.conn()
            .query_row(
                &format!("SELECT {FOLDER_COLUMNS} FROM folders WHERE id = ?1"),
                params![folder_id],
                folder_from_row,
            )
            .optional()
    }

    pub fn folder_by_remote_name(
        &self,
        account_id: i64,
        remote_name: &str,
    ) -> rusqlite::Result<Option<Folder>> {
        self.conn()
            .query_row(
                &format!(
                    "SELECT {FOLDER_COLUMNS} FROM folders
                     WHERE account_id = ?1 AND remote_name = ?2
                     ORDER BY id ASC LIMIT 1"
                ),
                params![account_id, remote_name],
                folder_from_row,
            )
            .optional()
    }

    /// First folder (lowest id) of the account with the given special-use role.
    pub fn folder_for_role(&self, account_id: i64, role: &str) -> rusqlite::Result<Option<Folder>> {
        self.conn()
            .query_row(
                &format!(
                    "SELECT {FOLDER_COLUMNS} FROM folders
                     WHERE account_id = ?1 AND role = ?2
                     ORDER BY id ASC LIMIT 1"
                ),
                params![account_id, role],
                folder_from_row,
            )
            .optional()
    }

    /// Creates or updates the folder keyed by `(account_id, path)` and returns
    /// its id. Sync checkpoints of an existing folder are kept.
    pub fn upsert_remote_folder(
        &self,
        account_id: i64,
        path: &str,
        display_name: &str,
        remote_name: &str,
        role: Option<&str>,
    ) -> rusqlite::Result<i64> {
        self.conn().query_row(
            "INSERT INTO folders (account_id, path, display_name, remote_name, role)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(account_id, path) DO UPDATE SET
                display_name = excluded.display_name,
                remote_name = excluded.remote_name,
                role = excluded.role
             RETURNING id",
            params![account_id, path, display_name, remote_name, role],
            |row| row.get(0),
        )
    }

    /// Deletes the folder and its locations. Messages of remote accounts left
    /// without any location are deleted; the rest get their membership
    /// refreshed. Location-less (local) messages keep their `folders_csv`.
    pub fn delete_folder(&self, folder_id: i64) -> rusqlite::Result<()> {
        // Locations go first in batches (see `clear_folder_locations`) so the
        // final transaction stays short.
        self.clear_folder_locations(folder_id)?;
        let tx = self.write_transaction()?;
        let message_ids = message_ids_in_folder(&tx, folder_id)?;
        tx.execute("DELETE FROM folders WHERE id = ?1", params![folder_id])?;
        settle_after_location_loss(&tx, &message_ids)?;
        tx.commit()
    }

    /// The delimiter the server last said the folder's name uses.
    pub fn folder_delimiter(&self, folder_id: i64) -> rusqlite::Result<Option<String>> {
        self.conn()
            .query_row(
                "SELECT delimiter FROM folders WHERE id = ?1",
                params![folder_id],
                |row| row.get(0),
            )
            .optional()
            .map(Option::flatten)
    }

    /// The delimiters the server uses in the account's folder names.
    pub fn account_delimiters(&self, account_id: i64) -> rusqlite::Result<Vec<String>> {
        self.conn()
            .prepare(
                "SELECT DISTINCT delimiter FROM folders
                 WHERE account_id = ?1 AND delimiter IS NOT NULL",
            )?
            .query_map(params![account_id], |row| row.get(0))?
            .collect()
    }

    /// Records the delimiter of each `(server name, delimiter)` of the account.
    pub fn set_folder_delimiters(
        &self,
        account_id: i64,
        delimiters: &[(&str, Option<&str>)],
    ) -> rusqlite::Result<()> {
        let tx = self.write_transaction()?;
        for (name, delimiter) in delimiters {
            tx.execute(
                "UPDATE folders SET delimiter = ?1
                 WHERE account_id = ?2 AND remote_name = ?3 AND delimiter IS NOT ?1",
                params![delimiter, account_id, name],
            )?;
        }
        tx.commit()
    }

    /// What the server puts before the name of a new top-level folder (`INBOX.`).
    pub fn folder_prefix(&self, account_id: i64) -> rusqlite::Result<String> {
        Ok(self
            .conn()
            .query_row(
                "SELECT folder_prefix FROM accounts WHERE id = ?1",
                params![account_id],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or_default())
    }

    pub fn set_folder_prefix(&self, account_id: i64, prefix: &str) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE accounts SET folder_prefix = ?1 WHERE id = ?2 AND folder_prefix <> ?1",
            params![prefix, account_id],
        )?;
        Ok(())
    }

    /// Gives folders of the user's new display and server names, keeping their messages:
    /// their locations go by folder id, and their membership by path. All at once, so the
    /// sync never sees it half done.
    pub fn rename_folders(
        &self,
        account_id: i64,
        renames: &[(i64, String, Option<String>)],
    ) -> rusqlite::Result<()> {
        let tx = self.write_transaction()?;
        // Membership in one pass over the account's messages, however many folders move.
        let mut needles = Vec::new();
        for (folder_id, display_name, remote_name) in renames {
            let old: Option<String> = tx
                .query_row(
                    "SELECT path FROM folders WHERE id = ?1",
                    params![folder_id],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(old) = old else {
                continue;
            };
            let new = user_folder_path(display_name);
            tx.execute(
                "UPDATE folders SET path = ?1, display_name = ?2, remote_name = ?3 WHERE id = ?4",
                params![new, display_name, remote_name, folder_id],
            )?;
            needles.push((folder_csv_needle(&old), folder_csv_needle(&new)));
        }
        if !needles.is_empty() {
            let any = vec!["instr(folders_csv, ?) > 0"; needles.len()].join(" OR ");
            let select =
                format!("SELECT id, folders_csv FROM messages WHERE account_id = ? AND ({any})");
            let mut values = vec![rusqlite::types::Value::Integer(account_id)];
            values.extend(
                needles
                    .iter()
                    .map(|(old, _)| rusqlite::types::Value::Text(old.clone())),
            );
            let rows: Vec<(i64, String)> = tx
                .prepare(&select)?
                .query_map(rusqlite::params_from_iter(values), |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })?
                .collect::<rusqlite::Result<_>>()?;
            let mut update = tx.prepare("UPDATE messages SET folders_csv = ?1 WHERE id = ?2")?;
            for (id, csv) in rows {
                // `,a,b,`: a needle's closing comma is the next one's opening, so each is
                // replaced on what the one before left, and overlaps are handled.
                let mut new_csv = csv;
                for (old, new) in &needles {
                    new_csv = new_csv.replace(old.as_str(), new.as_str());
                }
                update.execute(params![new_csv, id])?;
            }
        }
        tx.commit()
    }

    /// Sends what is in a folder to the Trash, in one go.
    pub fn trash_folder_messages(
        &self,
        account_id: i64,
        folder_path: &str,
    ) -> rusqlite::Result<()> {
        let tx = self.write_transaction()?;
        let ids: Vec<i64> = tx
            .prepare(
                "SELECT id FROM messages WHERE account_id = ?1 AND instr(folders_csv, ?2) > 0",
            )?
            .query_map(params![account_id, folder_csv_needle(folder_path)], |row| {
                row.get(0)
            })?
            .collect::<rusqlite::Result<_>>()?;
        for id in ids {
            self.move_message_to_trash(id)?;
        }
        tx.commit()
    }

    /// Stored sync checkpoint (all `None` for unknown folders or never synced).
    pub fn folder_sync_state(&self, folder_id: i64) -> rusqlite::Result<FolderSyncState> {
        let state = self
            .conn()
            .query_row(
                "SELECT uid_validity, uid_next, highest_modseq, synced_at
                 FROM folders WHERE id = ?1",
                params![folder_id],
                |row| {
                    Ok(FolderSyncState {
                        uid_validity: row.get::<_, Option<i64>>(0)?.map(|v| v as u32),
                        uid_next: row.get::<_, Option<i64>>(1)?.map(|v| v as u32),
                        // Bit-cast: mod-sequences are unsigned 63-bit, stored as i64.
                        highest_modseq: row.get::<_, Option<i64>>(2)?.map(|v| v as u64),
                        synced_at: row.get(3)?,
                    })
                },
            )
            .optional()?;
        Ok(state.unwrap_or_default())
    }

    pub fn set_folder_sync_state(
        &self,
        folder_id: i64,
        state: &FolderSyncState,
    ) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE folders SET uid_validity = ?1, uid_next = ?2, highest_modseq = ?3,
                                synced_at = ?4
             WHERE id = ?5",
            params![
                state.uid_validity.map(i64::from),
                state.uid_next.map(i64::from),
                state.highest_modseq.map(|v| v as i64),
                state.synced_at,
                folder_id
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::{remote_db, remote_message};
    use crate::types::{FolderSyncState, NewAttachment};
    use crate::{system, user_folder_path, Database, MailListQuery};
    use tempfile::NamedTempFile;

    #[test]
    fn create_get_update_account() {
        let file = NamedTempFile::new().unwrap();
        let db = Database::open(file.path()).unwrap();
        let a = db.create_account("A", "a@x.com", "imap", "{}").unwrap();
        let b = db
            .create_account("B", "b@x.com", "gmail", r#"{"k":1}"#)
            .unwrap();
        let account = db.get_account(b).unwrap().unwrap();
        assert_eq!(account.provider, "gmail");
        assert_eq!(account.config_json, r#"{"k":1}"#);

        db.update_account(a, "Renamed", r#"{"host":"h"}"#).unwrap();
        let account = db.get_account(a).unwrap().unwrap();
        assert_eq!(account.name, "Renamed");
        assert_eq!(account.config_json, r#"{"host":"h"}"#);
        assert_eq!(account.provider, "imap");

        let ids: Vec<i64> = db.list_accounts().unwrap().iter().map(|a| a.id).collect();
        assert_eq!(ids, vec![a, b]);
        assert!(db.get_account(9999).unwrap().is_none());
        assert!(db.create_account("Dup", "a@x.com", "imap", "{}").is_err());
    }

    #[test]
    fn delete_account_cascades_everything() {
        let (_file, db, fx) = remote_db();
        let id = db
            .insert_remote_message(fx.account, fx.inbox, 1, &remote_message("Cascade me", None))
            .unwrap();
        db.store_body(
            id,
            b"raw",
            "body",
            "text",
            "body",
            "body",
            &[NewAttachment {
                filename: "a.pdf".into(),
                ..Default::default()
            }],
        )
        .unwrap();
        db.enqueue_op(fx.account, "move", Some(id), "{}").unwrap();
        db.enqueue_outgoing(fx.account, "me", &["you".into()], b"raw", None)
            .unwrap();

        db.delete_account(fx.account).unwrap();

        for table in [
            "folders",
            "messages",
            "message_locations",
            "message_bodies",
            "attachments",
            "pending_ops",
            "outbox",
        ] {
            let count: i64 = db
                .conn()
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM {table} WHERE {}",
                        if table == "folders" || table == "messages" {
                            format!("account_id = {}", fx.account)
                        } else {
                            "1".to_string()
                        }
                    ),
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 0, "{table} not emptied");
        }
        assert!(db
            .list_messages(&MailListQuery::Search("cascade".into()))
            .unwrap()
            .is_empty());
        db.conn()
            .execute(
                "INSERT INTO messages_fts(messages_fts) VALUES ('integrity-check')",
                [],
            )
            .unwrap();
        assert!(db.get_account(fx.local_account).unwrap().is_some());
    }

    #[test]
    fn mail_goes_out_with_the_account_name_until_a_sender_name_is_set() {
        let (_file, db, fx) = remote_db();
        let account = db.get_account(fx.account).unwrap().unwrap();
        assert_eq!(account.sender_name, "");
        assert_eq!(account.from_name(), account.name);

        db.set_account_names(fx.account, "Work", "Ann Lee").unwrap();
        let account = db.get_account(fx.account).unwrap().unwrap();
        assert_eq!(account.name, "Work");
        assert_eq!(account.from_name(), "Ann Lee");

        db.set_account_names(fx.account, "Work", "  ").unwrap();
        let account = db.get_account(fx.account).unwrap().unwrap();
        assert_eq!(account.from_name(), "Work");
    }

    #[test]
    fn upsert_remote_folder_is_keyed_by_path() {
        let (_file, db, fx) = remote_db();
        let again = db
            .upsert_remote_folder(fx.account, system::INBOX, "", "INBOX", Some("inbox"))
            .unwrap();
        assert_eq!(again, fx.inbox);

        let custom = db
            .upsert_remote_folder(fx.account, "user:Work", "Work", "Work", Some("custom"))
            .unwrap();
        let renamed = db
            .upsert_remote_folder(fx.account, "user:Work", "Job", "Job", None)
            .unwrap();
        assert_eq!(custom, renamed);
        let folder = db.folder_by_id(custom).unwrap().unwrap();
        assert_eq!(folder.display_name, "Job");
        assert_eq!(folder.remote_name.as_deref(), Some("Job"));
        assert_eq!(folder.role, None);
    }

    #[test]
    fn how_the_server_names_folders_is_remembered() {
        let (_file, db, fx) = remote_db();
        let folder = db
            .upsert_remote_folder(fx.account, "user:Work", "Work", "INBOX.Work", None)
            .unwrap();
        assert_eq!(db.folder_delimiter(folder).unwrap(), None);
        db.set_folder_delimiters(
            fx.account,
            &[("INBOX.Work", Some(".")), ("Gone", Some("/"))],
        )
        .unwrap();
        assert_eq!(db.folder_delimiter(folder).unwrap().as_deref(), Some("."));
        assert_eq!(db.account_delimiters(fx.account).unwrap(), ["."]);

        assert_eq!(db.folder_prefix(fx.account).unwrap(), "");
        db.set_folder_prefix(fx.account, "INBOX.").unwrap();
        assert_eq!(db.folder_prefix(fx.account).unwrap(), "INBOX.");
        assert_eq!(db.folder_prefix(fx.account + 1000).unwrap(), "");
    }

    #[test]
    fn renaming_a_folder_keeps_its_messages_where_they_are() {
        let (_file, db, fx) = crate::test_support::remote_db();
        let old = user_folder_path("Old, name");
        let folder = db
            .upsert_remote_folder(fx.account, &old, "Old, name", "Old, name", None)
            .unwrap();
        let id = db
            .insert_remote_message(
                fx.account,
                folder,
                1,
                &crate::test_support::remote_message("Kept", None),
            )
            .unwrap();
        let renames = [(folder, "New name".to_string(), Some("New name".to_string()))];
        db.rename_folders(fx.account, &renames).unwrap();

        let renamed = db.folder_by_id(folder).unwrap().unwrap();
        assert_eq!(renamed.path, user_folder_path("New name"));
        assert_eq!(renamed.display_name, "New name");
        assert_eq!(renamed.remote_name.as_deref(), Some("New name"));
        let listed = db
            .list_messages(&MailListQuery::AccountFolder {
                account_id: fx.account,
                folder_path: renamed.path.clone(),
            })
            .unwrap();
        assert_eq!(listed.iter().map(|m| m.id).collect::<Vec<_>>(), [id]);
        assert!(db
            .list_messages(&MailListQuery::AccountFolder {
                account_id: fx.account,
                folder_path: old,
            })
            .unwrap()
            .is_empty());
        assert_eq!(db.folder_uids(folder).unwrap().len(), 1);
    }

    #[test]
    fn folder_lookups() {
        let (_file, db, fx) = remote_db();
        let inbox = db
            .folder_by_remote_name(fx.account, "INBOX")
            .unwrap()
            .unwrap();
        assert_eq!(inbox.id, fx.inbox);
        assert_eq!(inbox.role.as_deref(), Some("inbox"));
        assert!(db
            .folder_by_remote_name(fx.account, "Nope")
            .unwrap()
            .is_none());
        assert_eq!(
            db.folder_by_remote_name(fx.local_account, "INBOX")
                .unwrap()
                .unwrap()
                .id,
            fx.local_inbox
        );

        assert_eq!(
            db.folder_for_role(fx.account, "trash").unwrap().unwrap().id,
            fx.trash
        );
        assert!(db.folder_for_role(fx.account, "junk").unwrap().is_none());
        assert!(db.folder_by_id(9999).unwrap().is_none());
    }

    #[test]
    fn folder_sync_state_round_trips() {
        let (_file, db, fx) = remote_db();
        assert_eq!(
            db.folder_sync_state(fx.inbox).unwrap(),
            FolderSyncState::default()
        );
        let state = FolderSyncState {
            uid_validity: Some(u32::MAX),
            uid_next: Some(42),
            highest_modseq: Some(u64::MAX >> 1),
            synced_at: Some(1_700_000_000),
        };
        db.set_folder_sync_state(fx.inbox, &state).unwrap();
        assert_eq!(db.folder_sync_state(fx.inbox).unwrap(), state);
        assert_eq!(
            db.folder_sync_state(9999).unwrap(),
            FolderSyncState::default()
        );
    }

    #[test]
    fn delete_folder_drops_remote_orphans_and_refreshes_others() {
        let (_file, db, fx) = remote_db();
        let only_here = db
            .insert_remote_message(fx.account, fx.archive, 1, &remote_message("only", None))
            .unwrap();
        let shared = db
            .insert_remote_message(
                fx.account,
                fx.archive,
                2,
                &remote_message("shared", Some(7)),
            )
            .unwrap();
        db.insert_remote_message(fx.account, fx.inbox, 5, &remote_message("shared", Some(7)))
            .unwrap();

        db.delete_folder(fx.archive).unwrap();

        assert!(db.folder_by_id(fx.archive).unwrap().is_none());
        assert!(db.get_message(only_here).unwrap().is_none());
        assert_eq!(
            db.get_message(shared).unwrap().unwrap().folders_csv,
            ",sys:inbox,"
        );
    }
}
