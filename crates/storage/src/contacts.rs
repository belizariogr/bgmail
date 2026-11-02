//! Catalog of people the user exchanged mail with, offered when addressing a
//! message. It fills itself: there is no screen to manage it.

use rusqlite::{params, OptionalExtension};

use crate::database::Database;

/// Someone mail can be addressed to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Contact {
    pub email: String,
    /// Empty until mail from them brings one.
    pub name: String,
}

impl Database {
    /// Adds each `(email, name)` that is new, and fills in names still blank; a
    /// name already known is kept, as the sender's own header can be anything.
    pub fn record_contacts(&self, contacts: &[(String, Option<String>)]) -> rusqlite::Result<()> {
        if contacts.is_empty() {
            return Ok(());
        }
        let tx = self.write_transaction()?;
        for (email, name) in contacts {
            let email = email.trim();
            if email.is_empty() {
                continue;
            }
            tx.execute(
                "INSERT INTO contacts (email, name) VALUES (?1, ?2)
                 ON CONFLICT(email) DO UPDATE SET name = excluded.name
                 WHERE contacts.name = '' AND excluded.name <> ''",
                params![email, name.as_deref().map_or("", str::trim)],
            )?;
        }
        tx.commit()
    }

    /// Every contact, by name then address (nameless ones last).
    pub fn list_contacts(&self) -> rusqlite::Result<Vec<Contact>> {
        let mut stmt = self.conn().prepare(
            "SELECT email, name FROM contacts
             ORDER BY name = '', name COLLATE NOCASE, email",
        )?;
        let contacts = stmt
            .query_map([], |row| {
                Ok(Contact {
                    email: row.get(0)?,
                    name: row.get(1)?,
                })
            })?
            .collect();
        contacts
    }

    /// Sender and recipients of the account's mail outside Junk, from what was
    /// synced before the catalog existed: `(sender, sender_email, to, cc)`.
    pub fn contact_sources(
        &self,
        account_id: i64,
    ) -> rusqlite::Result<Vec<(String, String, String, String)>> {
        let mut stmt = self.conn().prepare(
            "SELECT sender, sender_email, to_addrs, cc_addrs FROM messages
             WHERE account_id = ?1 AND folders_csv NOT LIKE '%,sys:junk,%'",
        )?;
        let sources = stmt
            .query_map(params![account_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .collect();
        sources
    }

    /// Whether a one-time job already ran, and marking it.
    pub fn meta_flag(&self, key: &str) -> rusqlite::Result<bool> {
        self.conn()
            .query_row(
                "SELECT 1 FROM meta WHERE key = ?1",
                params![key],
                |_| Ok(()),
            )
            .optional()
            .map(|found| found.is_some())
    }

    pub fn set_meta_flag(&self, key: &str) -> rusqlite::Result<()> {
        self.conn().execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, '1')",
            params![key],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::{remote_db, remote_message};

    fn entry(email: &str, name: Option<&str>) -> (String, Option<String>) {
        (email.to_string(), name.map(str::to_string))
    }

    #[test]
    fn a_contact_starts_nameless_and_keeps_the_first_name_it_gets() {
        let (_file, db, _) = remote_db();
        db.record_contacts(&[entry("Ann@x.test", None)]).unwrap();
        assert_eq!(db.list_contacts().unwrap()[0].name, "");

        // Same address in another case: one contact, name filled in.
        db.record_contacts(&[entry("ann@X.test", Some(" Ann "))])
            .unwrap();
        db.record_contacts(&[entry("ann@x.test", Some("Other"))])
            .unwrap();
        db.record_contacts(&[entry("ann@x.test", None)]).unwrap();
        let contacts = db.list_contacts().unwrap();
        assert_eq!(contacts.len(), 1);
        assert_eq!(contacts[0].name, "Ann");
    }

    #[test]
    fn contacts_sort_by_name_with_the_nameless_last() {
        let (_file, db, _) = remote_db();
        db.record_contacts(&[
            entry("z@x.test", None),
            entry("b@x.test", Some("bea")),
            entry("a@x.test", Some("Zoe")),
            entry(" ", Some("blank")),
        ])
        .unwrap();
        let emails: Vec<_> = db
            .list_contacts()
            .unwrap()
            .into_iter()
            .map(|contact| contact.email)
            .collect();
        assert_eq!(emails, ["b@x.test", "a@x.test", "z@x.test"]);
    }

    #[test]
    fn sources_leave_junk_out() {
        let (_file, db, fx) = remote_db();
        let mut message = remote_message("Hi", None);
        message.sender = "Ann".into();
        message.sender_email = "ann@x.test".into();
        db.insert_remote_message(fx.account, fx.inbox, 1, &message)
            .unwrap();
        let sources = db.contact_sources(fx.account).unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].1, "ann@x.test");
        db.conn()
            .execute("UPDATE messages SET folders_csv = ',sys:junk,'", [])
            .unwrap();
        assert!(db.contact_sources(fx.account).unwrap().is_empty());
    }

    #[test]
    fn meta_flags_stay_set() {
        let (_file, db, _) = remote_db();
        assert!(!db.meta_flag("job").unwrap());
        db.set_meta_flag("job").unwrap();
        db.set_meta_flag("job").unwrap();
        assert!(db.meta_flag("job").unwrap());
    }
}
