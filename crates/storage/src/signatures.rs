//! Signatures the user keeps, and which one each account signs its mail with.
//! A signature's document belongs to the editor that wrote it: here it is text.

use rusqlite::params;

use crate::database::Database;

/// A saved signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    pub id: i64,
    pub name: String,
    /// The editor's document (JSON in BGMail), stored as given.
    pub document: String,
}

impl Database {
    /// Every signature, in the order they were added.
    pub fn list_signatures(&self) -> rusqlite::Result<Vec<Signature>> {
        let mut stmt = self
            .conn()
            .prepare("SELECT id, name, document FROM signatures ORDER BY sort_order ASC, id ASC")?;
        let signatures = stmt
            .query_map([], |row| {
                Ok(Signature {
                    id: row.get(0)?,
                    name: crate::database::lossy_text(row, 1)?,
                    document: crate::database::lossy_text(row, 2)?,
                })
            })?
            .collect();
        signatures
    }

    /// Adds a signature after the others and returns its id.
    pub fn create_signature(&self, name: &str, document: &str) -> rusqlite::Result<i64> {
        self.conn().execute(
            "INSERT INTO signatures (name, document, sort_order)
             VALUES (?1, ?2, (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM signatures))",
            params![name, document],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    /// Replaces the name and document of signature `id`.
    pub fn update_signature(&self, id: i64, name: &str, document: &str) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE signatures SET name = ?1, document = ?2 WHERE id = ?3",
            params![name, document, id],
        )?;
        Ok(())
    }

    /// Deletes signature `id`; the accounts that used it go without one.
    pub fn delete_signature(&self, id: i64) -> rusqlite::Result<()> {
        self.conn()
            .execute("DELETE FROM signatures WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Which signature account `account_id` puts in new mail and in replies and
    /// forwards (`None`: none).
    pub fn set_account_signatures(
        &self,
        account_id: i64,
        new_mail: Option<i64>,
        replies: Option<i64>,
    ) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE accounts SET signature_new = ?1, signature_reply = ?2 WHERE id = ?3",
            params![new_mail, replies, account_id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::remote_db;

    #[test]
    fn signatures_are_kept_in_order_and_edited_in_place() {
        let (_file, db, _) = remote_db();
        assert!(db.list_signatures().unwrap().is_empty());
        let work = db.create_signature("Work", "{\"text\":\"Bob\"}").unwrap();
        let home = db.create_signature("Home", "").unwrap();
        db.update_signature(work, "Office", "{}").unwrap();
        let names: Vec<_> = db
            .list_signatures()
            .unwrap()
            .into_iter()
            .map(|signature| (signature.id, signature.name, signature.document))
            .collect();
        assert_eq!(
            names,
            [
                (work, "Office".to_string(), "{}".to_string()),
                (home, "Home".to_string(), String::new()),
            ]
        );
    }

    #[test]
    fn each_account_chooses_its_signatures_and_loses_a_deleted_one() {
        let (_file, db, fx) = remote_db();
        let work = db.create_signature("Work", "").unwrap();
        let short = db.create_signature("Short", "").unwrap();
        db.set_account_signatures(fx.account, Some(work), Some(short))
            .unwrap();
        let account = db.get_account(fx.account).unwrap().unwrap();
        assert_eq!(
            (account.signature_new, account.signature_reply),
            (Some(work), Some(short))
        );
        let other = db.get_account(fx.local_account).unwrap().unwrap();
        assert_eq!((other.signature_new, other.signature_reply), (None, None));

        db.delete_signature(short).unwrap();
        let account = db.get_account(fx.account).unwrap().unwrap();
        assert_eq!(
            (account.signature_new, account.signature_reply),
            (Some(work), None)
        );
        // A signature that doesn't exist can't be chosen.
        assert!(db
            .set_account_signatures(fx.account, Some(9999), None)
            .is_err());
    }
}
