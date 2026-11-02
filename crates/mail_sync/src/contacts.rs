//! Keeps the contact catalog filled from synced mail.

use mail_core::{parse_address_list, same_email};
use storage::{system, Database, NewRemoteMessage};

/// What a message says about contacts: who wrote it, with their name; or, for
/// the user's own mail, who it went to, without a name, as a name typed by the
/// user or a client isn't what that person calls themselves.
fn contacts_in(
    own_email: &str,
    sender: &str,
    sender_email: &str,
    to: &str,
    cc: &str,
) -> Vec<(String, Option<String>)> {
    if !same_email(own_email, sender_email) {
        let name = sender.trim();
        let named = !name.is_empty() && !name.contains('@');
        return vec![(sender_email.to_string(), named.then(|| name.to_string()))];
    }
    [to, cc]
        .into_iter()
        .flat_map(|list| parse_address_list(list).unwrap_or_default())
        .filter(|address| !same_email(own_email, &address.email))
        .map(|address| (address.email, None))
        .collect()
}

/// Records the contacts of a message just synced into `folder_path`. Junk senders
/// aren't people to suggest.
pub(crate) fn record_message(
    db: &Database,
    own_email: &str,
    folder_path: &str,
    message: &NewRemoteMessage,
) -> rusqlite::Result<()> {
    if folder_path == system::JUNK {
        return Ok(());
    }
    db.record_contacts(&contacts_in(
        own_email,
        &message.sender,
        &message.sender_email,
        &message.to_addrs,
        &message.cc_addrs,
    ))
}

/// Once per account, fills the catalog from the mail synced before it existed.
pub(crate) fn backfill(db: &Database, account_id: i64) -> rusqlite::Result<()> {
    let flag = format!("contacts_backfilled_{account_id}");
    if db.meta_flag(&flag)? {
        return Ok(());
    }
    let Some(account) = db.get_account(account_id)? else {
        return Ok(());
    };
    let contacts: Vec<_> = db
        .contact_sources(account_id)?
        .iter()
        .flat_map(|(sender, email, to, cc)| contacts_in(&account.email, sender, email, to, cc))
        .collect();
    db.record_contacts(&contacts)?;
    db.set_meta_flag(&flag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn received_mail_gives_the_sender_with_their_name() {
        assert_eq!(
            contacts_in("me@x.test", "Ann", "ann@x.test", "me@x.test", ""),
            [("ann@x.test".to_string(), Some("Ann".to_string()))]
        );
    }

    #[test]
    fn a_sender_shown_by_address_has_no_name() {
        assert_eq!(
            contacts_in("me@x.test", "ann@x.test", "ann@x.test", "", ""),
            [("ann@x.test".to_string(), None)]
        );
        assert_eq!(
            contacts_in("me@x.test", "  ", "ann@x.test", "", ""),
            [("ann@x.test".to_string(), None)]
        );
    }

    #[test]
    fn sent_mail_gives_its_recipients_without_names() {
        assert_eq!(
            contacts_in(
                "ME@x.test",
                "Me",
                "me@x.test",
                "Ann <ann@x.test>, me@x.test",
                "bob@x.test"
            ),
            [
                ("ann@x.test".to_string(), None),
                ("bob@x.test".to_string(), None)
            ]
        );
    }
}
