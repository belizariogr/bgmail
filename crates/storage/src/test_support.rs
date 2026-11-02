//! Shared fixtures for unit tests (compiled only under `cfg(test)`).

use tempfile::NamedTempFile;

use crate::folder::system;
use crate::types::NewRemoteMessage;
use crate::Database;

/// Ids created by [`remote_db`].
pub(crate) struct Fixture {
    /// `imap` account with Inbox, Archive and Trash folders.
    pub account: i64,
    pub inbox: i64,
    pub archive: i64,
    pub trash: i64,
    /// `local` account with an Inbox folder.
    pub local_account: i64,
    pub local_inbox: i64,
}

/// A temp database with one remote and one local account.
pub(crate) fn remote_db() -> (NamedTempFile, Database, Fixture) {
    let file = NamedTempFile::new().unwrap();
    let db = Database::open(file.path()).unwrap();
    let account = db
        .create_account("Work", "work@test.com", "imap", "{}")
        .unwrap();
    let inbox = db
        .upsert_remote_folder(account, system::INBOX, "", "INBOX", Some("inbox"))
        .unwrap();
    let archive = db
        .upsert_remote_folder(account, system::ARCHIVE, "", "Archive", Some("archive"))
        .unwrap();
    let trash = db
        .upsert_remote_folder(account, system::TRASH, "", "Trash", Some("trash"))
        .unwrap();
    let local_account = db
        .create_account("Local", "local@test.com", "local", "{}")
        .unwrap();
    let local_inbox = db
        .upsert_remote_folder(local_account, system::INBOX, "", "INBOX", Some("inbox"))
        .unwrap();
    let fixture = Fixture {
        account,
        inbox,
        archive,
        trash,
        local_account,
        local_inbox,
    };
    (file, db, fixture)
}

/// Headers of a server message whose Message-ID is `<{subject}@test>`.
pub(crate) fn remote_message(subject: &str, gm_msgid: Option<u64>) -> NewRemoteMessage {
    NewRemoteMessage {
        sender: "Sender".into(),
        sender_email: "sender@test.com".into(),
        subject: subject.into(),
        preview: String::new(),
        plain_text: String::new(),
        message_id_hdr: Some(format!("<{subject}@test>")),
        received_at: "09:00".into(),
        gm_msgid,
        ..Default::default()
    }
}
