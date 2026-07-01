//! Builds SQLite seed records from the visual mock in [`crate::data`].

use storage::{
    plain_text_from_raw, preview_from_plain, system, SeedAccount, SeedMailbox, SeedMessage,
};

use crate::data::{
    sample_accounts, sample_messages, GlobalMailbox, Mailbox, MailboxKind, Message, MessageBody,
};

/// Accounts + folders for the initial database population.
pub fn seed_accounts() -> Vec<SeedAccount> {
    sample_accounts()
        .into_iter()
        .map(|account| SeedAccount {
            name: account.name.to_string(),
            email: account.email.to_string(),
            mailboxes: account.mailboxes.iter().map(seed_mailbox).collect(),
        })
        .collect()
}

fn seed_mailbox(mailbox: &Mailbox) -> SeedMailbox {
    match mailbox.kind {
        MailboxKind::Inbox => seed_system(system::INBOX, mailbox.unread),
        MailboxKind::Drafts => seed_system(system::DRAFTS, mailbox.unread),
        MailboxKind::Sent => seed_system(system::SENT, mailbox.unread),
        MailboxKind::Junk => seed_system(system::JUNK, mailbox.unread),
        MailboxKind::Trash => seed_system(system::TRASH, mailbox.unread),
        MailboxKind::Archive => seed_system(system::ARCHIVE, mailbox.unread),
        MailboxKind::Custom => SeedMailbox {
            system_path: None,
            custom_name: mailbox.label.as_ref().map(|s| s.to_string()),
            unread: mailbox.unread,
        },
    }
}

fn seed_system(path: &'static str, unread: usize) -> SeedMailbox {
    SeedMailbox {
        system_path: Some(path),
        custom_name: None,
        unread,
    }
}

/// Messages for the initial database population (all assigned to Personal).
pub fn seed_messages() -> Vec<SeedMessage> {
    let default_account = sample_accounts()
        .first()
        .map(|a| a.email.to_string())
        .unwrap_or_else(|| "you@gmail.com".into());

    sample_messages()
        .into_iter()
        .enumerate()
        .map(|(idx, message)| seed_message(&default_account, idx as i64, &message))
        .collect()
}

fn seed_message(account_email: &str, sort_order: i64, message: &Message) -> SeedMessage {
    let (raw_content, raw_format) = match &message.body {
        MessageBody::Html(html) => (html.to_string(), "html"),
        MessageBody::Text(text) => (text.to_string(), "text"),
    };
    let plain_text = plain_text_from_raw(&raw_content, raw_format);
    let preview = if message.preview.is_empty() {
        preview_from_plain(&plain_text, 120)
    } else {
        message.preview.to_string()
    };

    SeedMessage {
        account_email: account_email.to_string(),
        sender: message.sender.to_string(),
        sender_email: message.sender_email.to_string(),
        subject: message.subject.to_string(),
        preview,
        plain_text,
        raw_content,
        raw_format,
        received_at: message.time.to_string(),
        sort_order,
        unread: message.unread,
        starred: message.starred,
        has_attachment: message.has_attachment,
        extra_folders: Vec::new(),
    }
}

/// System folder path for a unified sidebar mailbox.
pub fn global_folder_path(global: GlobalMailbox) -> &'static str {
    match global {
        GlobalMailbox::Inbox => system::INBOX,
        GlobalMailbox::Flagged => system::FLAGGED,
        GlobalMailbox::Drafts => system::DRAFTS,
        GlobalMailbox::Sent => system::SENT,
    }
}

/// Maps a stored folder path to the UI mailbox kind (icons + localization).
pub fn folder_kind_from_path(path: &str) -> MailboxKind {
    match path {
        system::INBOX => MailboxKind::Inbox,
        system::DRAFTS => MailboxKind::Drafts,
        system::SENT => MailboxKind::Sent,
        system::JUNK => MailboxKind::Junk,
        system::TRASH => MailboxKind::Trash,
        system::ARCHIVE => MailboxKind::Archive,
        _ => MailboxKind::Custom,
    }
}

/// Display name for a folder row in the sidebar.
pub fn folder_display_name(
    path: &str,
    display_name: &str,
    language: crate::locale::Language,
) -> gpui::SharedString {
    if !display_name.is_empty() {
        return display_name.to_string().into();
    }
    folder_kind_from_path(path).display_name(language).into()
}

/// Converts a stored message into the reader body enum.
pub fn message_body_from_detail(detail: &storage::MessageDetail) -> MessageBody {
    if detail.raw_format == "html" {
        MessageBody::Html(detail.raw_content.clone().into())
    } else {
        MessageBody::Text(detail.raw_content.clone().into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_messages_match_sample_count() {
        assert_eq!(seed_messages().len(), sample_messages().len());
    }

    #[test]
    fn seed_accounts_include_custom_folders() {
        let accounts = seed_accounts();
        let work = accounts.iter().find(|a| a.name == "Work").unwrap();
        assert!(work
            .mailboxes
            .iter()
            .any(|m| m.custom_name.as_deref() == Some("Clients")));
    }

    #[test]
    fn plain_text_is_derived_from_html_bodies() {
        let plain = plain_text_from_raw("<p>Hello <strong>world</strong></p>", "html");
        assert!(!plain.contains('<'));
        assert!(plain.contains("Hello"));
    }
}
