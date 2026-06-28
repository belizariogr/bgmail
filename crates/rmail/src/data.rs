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

/// Sample accounts. Several are provided so the sidebar overflows and exercises
/// its scrollbar.
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
        Account {
            name: "University".into(),
            email: "you@university.edu".into(),
            mailboxes: default_mailboxes(5),
        },
        Account {
            name: "Newsletters".into(),
            email: "you@news.example".into(),
            mailboxes: default_mailboxes(12),
        },
        Account {
            name: "Side Project".into(),
            email: "hello@sideproject.dev".into(),
            mailboxes: default_mailboxes(0),
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
        (
            "Figma",
            "updates@figma.com",
            "Sara commented on your design",
            "\"Love the new toolbar layout!\" — see the comment and reply in the file.",
            true,
            false,
            false,
            "Sun",
        ),
        (
            "Linear",
            "notifications@linear.app",
            "3 issues assigned to you",
            "RMAIL-42, RMAIL-47 and RMAIL-51 are now in your current cycle.",
            true,
            false,
            false,
            "Sat",
        ),
        (
            "Dr. Helena Costa",
            "helena.costa@clinic.com",
            "Appointment reminder",
            "Just a reminder about your check-up next Tuesday at 10:00. Reply to reschedule.",
            false,
            false,
            false,
            "Sat",
        ),
        (
            "AWS Billing",
            "no-reply@aws.amazon.com",
            "Your June invoice is ready",
            "Your AWS invoice for June is available. Total: $128.74 across 6 services.",
            false,
            false,
            true,
            "Fri",
        ),
        (
            "Carlos Mendes",
            "carlos@designstudio.com",
            "Logo concepts v2",
            "Attached are three refined directions. My favorite is the second one — thoughts?",
            true,
            true,
            true,
            "Fri",
        ),
        (
            "Spotify",
            "no-reply@spotify.com",
            "Your 2026 Wrapped is almost here",
            "We've been crunching the numbers on your most-played tracks this year.",
            false,
            false,
            false,
            "Thu",
        ),
        (
            "Project Phoenix",
            "ci@phoenix.dev",
            "Build #1042 passed",
            "All 318 tests passed on main. Deployment to staging is ready for approval.",
            false,
            false,
            false,
            "Thu",
        ),
        (
            "Anna Becker",
            "anna.becker@partner.com",
            "Contract signed",
            "Great news — the contract is fully signed. I've attached the countersigned PDF.",
            true,
            false,
            true,
            "Wed",
        ),
        (
            "Booking.com",
            "confirmations@booking.com",
            "Your reservation is confirmed",
            "Your stay in Lisbon (Aug 12–16) is confirmed. View directions and check-in info.",
            false,
            false,
            false,
            "Wed",
        ),
        (
            "Team Standup",
            "calendar@company.com",
            "Daily standup in 15 minutes",
            "Reminder: the daily standup starts at 09:30. Join the call from your calendar.",
            false,
            false,
            false,
            "Tue",
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
                    "{preview}\n\n\
                     This is a sample message body used in the rMail visual mock. The real \
                     content will be rendered from HTML/text once the domain layer is implemented.\n\n\
                     For now we keep the text intentionally long so the reading pane overflows \
                     vertically and we can exercise its scrollbar. Lorem ipsum dolor sit amet, \
                     consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et \
                     dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation \
                     ullamco laboris nisi ut aliquip ex ea commodo consequat.\n\n\
                     Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore \
                     eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, \
                     sunt in culpa qui officia deserunt mollit anim id est laborum.\n\n\
                     Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium \
                     doloremque laudantium, totam rem aperiam, eaque ipsa quae ab illo inventore \
                     veritatis et quasi architecto beatae vitae dicta sunt explicabo.\n\n\
                     Nemo enim ipsam voluptatem quia voluptas sit aspernatur aut odit aut fugit, \
                     sed quia consequuntur magni dolores eos qui ratione voluptatem sequi \
                     nesciunt. Neque porro quisquam est, qui dolorem ipsum quia dolor sit amet.\n\n\
                     Ut enim ad minima veniam, quis nostrum exercitationem ullam corporis suscipit \
                     laboriosam, nisi ut aliquid ex ea commodi consequatur. Quis autem vel eum iure \
                     reprehenderit qui in ea voluptate velit esse quam nihil molestiae consequatur.\n\n\
                     At vero eos et accusamus et iusto odio dignissimos ducimus qui blanditiis \
                     praesentium voluptatum deleniti atque corrupti quos dolores et quas molestias \
                     excepturi sint occaecati cupiditate non provident, similique sunt in culpa.\n\n\
                     Et harum quidem rerum facilis est et expedita distinctio. Nam libero tempore, \
                     cum soluta nobis est eligendi optio cumque nihil impedit quo minus id quod \
                     maxime placeat facere possimus, omnis voluptas assumenda est, omnis dolor.\n\n\
                     Temporibus autem quibusdam et aut officiis debitis aut rerum necessitatibus \
                     saepe eveniet ut et voluptates repudiandae sint et molestiae non recusandae.\n\n\
                     Best regards,\n{sender}"
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
        assert_eq!(accounts.len(), 5);
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
