//! Sample (mock) data used by the visual prototype.
//!
//! No real e-mail logic lives here — just static structures to populate the UI
//! while we validate the layout and performance. Once the domain layer is built,
//! these types will be replaced by the real models (likely in a `mail_core`
//! crate).

use gpui::SharedString;

use crate::locale::{Key, Language};

/// Semantic kind of a mailbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailboxKind {
    Inbox,
    Drafts,
    Sent,
    Junk,
    Trash,
    Archive,
}

impl MailboxKind {
    /// Localization key for this mailbox's display name.
    fn name_key(self) -> Key {
        match self {
            MailboxKind::Inbox => Key::MailboxInbox,
            MailboxKind::Drafts => Key::MailboxDrafts,
            MailboxKind::Sent => Key::MailboxSent,
            MailboxKind::Junk => Key::MailboxJunk,
            MailboxKind::Trash => Key::MailboxTrash,
            MailboxKind::Archive => Key::MailboxArchive,
        }
    }

    /// Localized display name for the given language.
    pub fn display_name(self, language: Language) -> &'static str {
        self.name_key().tr(language)
    }
}

/// A mailbox within an account. The display name is derived from [`MailboxKind`]
/// so it can be localized at render time.
#[derive(Debug, Clone)]
pub struct Mailbox {
    pub kind: MailboxKind,
    pub unread: usize,
}

impl Mailbox {
    fn new(kind: MailboxKind, unread: usize) -> Self {
        Self { kind, unread }
    }
}

/// A connected e-mail account.
#[derive(Debug, Clone)]
pub struct Account {
    pub name: SharedString,
    pub email: SharedString,
    pub mailboxes: Vec<Mailbox>,
}

/// An e-mail message.
#[derive(Debug, Clone)]
pub struct Message {
    pub sender: SharedString,
    pub sender_email: SharedString,
    pub subject: SharedString,
    pub preview: SharedString,
    pub body: SharedString,
    pub time: SharedString,
    pub unread: bool,
    pub starred: bool,
    pub has_attachment: bool,
}

/// Default mailboxes present in any account.
fn default_mailboxes(inbox_unread: usize) -> Vec<Mailbox> {
    vec![
        Mailbox::new(MailboxKind::Inbox, inbox_unread),
        Mailbox::new(MailboxKind::Drafts, 0),
        Mailbox::new(MailboxKind::Sent, 0),
        Mailbox::new(MailboxKind::Junk, 3),
        Mailbox::new(MailboxKind::Trash, 0),
        Mailbox::new(MailboxKind::Archive, 0),
    ]
}

/// Sample accounts (a Gmail and an IMAP one).
pub fn sample_accounts() -> Vec<Account> {
    vec![
        Account {
            name: "Personal".into(),
            email: "you@gmail.com".into(),
            mailboxes: default_mailboxes(7),
        },
        Account {
            name: "Work".into(),
            email: "you@company.com".into(),
            mailboxes: default_mailboxes(2),
        },
    ]
}

/// Sample inbox messages.
pub fn sample_messages() -> Vec<Message> {
    let raw = [
        (
            "GitHub",
            "noreply@github.com",
            "[zed-industries/zed] New release v0.200.0",
            "The new version brings GPUI performance improvements and fixes...",
            true,
            false,
            true,
            "09:42",
        ),
        (
            "Mary Smith",
            "mary.smith@company.com",
            "Planning meeting — Thursday",
            "Hi! Can we confirm the meeting for Thursday at 2pm? Agenda attached.",
            true,
            true,
            true,
            "09:05",
        ),
        (
            "Rust Newsletter",
            "this-week@rust-lang.org",
            "This Week in Rust #600",
            "This week's news from the Rust ecosystem, including async, GUIs and more.",
            true,
            false,
            false,
            "08:30",
        ),
        (
            "Digital Bank",
            "alerts@bank.com",
            "Your statement is available",
            "This month's statement is closed. Open the app to see the details.",
            false,
            false,
            false,
            "Yesterday",
        ),
        (
            "John Parker",
            "john@startup.io",
            "Re: Partnership proposal",
            "Perfect, that makes sense. Let's move forward with the contract then. Thanks!",
            false,
            true,
            false,
            "Yesterday",
        ),
        (
            "rMail Team",
            "hello@rmail.app",
            "Welcome to rMail",
            "Thanks for trying rMail — a fast and elegant e-mail client.",
            false,
            false,
            false,
            "Mon",
        ),
        (
            "DevConf Conference",
            "info@devconf.com",
            "Your registration is confirmed",
            "See you in October! Keep your ticket and the talks schedule handy.",
            false,
            false,
            true,
            "Mon",
        ),
        (
            "Online Store",
            "orders@store.com",
            "Your order has shipped",
            "Order #48213 is out for delivery and arrives within 3 business days.",
            false,
            false,
            false,
            "Sun",
        ),
    ];

    raw.into_iter()
        .map(
            |(sender, email, subject, preview, unread, starred, attach, time)| Message {
                sender: sender.into(),
                sender_email: email.into(),
                subject: subject.into(),
                preview: preview.into(),
                body: format!(
                    "{preview}\n\nThis is a sample message body used in the rMail visual mock. \
                     The real content will be rendered from HTML/text once the domain layer is \
                     implemented.\n\nBest regards,\n{sender}"
                )
                .into(),
                time: time.into(),
                unread,
                starred,
                has_attachment: attach,
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accounts_have_default_mailboxes() {
        let accounts = sample_accounts();
        assert_eq!(accounts.len(), 2);
        for account in &accounts {
            assert_eq!(account.mailboxes.len(), 6);
            assert_eq!(account.mailboxes[0].kind, MailboxKind::Inbox);
        }
    }

    #[test]
    fn sample_messages_are_populated() {
        let messages = sample_messages();
        assert!(!messages.is_empty());
        assert!(messages.iter().any(|m| m.unread));
        assert!(messages.iter().any(|m| m.has_attachment));
    }

    #[test]
    fn unread_count_matches_first_account() {
        let accounts = sample_accounts();
        assert_eq!(accounts[0].mailboxes[0].unread, 7);
    }

    #[test]
    fn mailbox_names_are_localized() {
        assert_eq!(MailboxKind::Inbox.display_name(Language::English), "Inbox");
        assert_eq!(
            MailboxKind::Inbox.display_name(Language::Portuguese),
            "Caixa de entrada"
        );
    }
}
