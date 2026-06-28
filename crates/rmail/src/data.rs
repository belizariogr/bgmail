//! Sample (mock) data used by the visual prototype.
//!
//! No real e-mail logic lives here — just static structures to populate the UI
//! while we validate the layout and performance. Once the domain layer is built,
//! these types will be replaced by the real models (likely in a `mail_core`
//! crate).

use gpui::SharedString;

use crate::locale::{Key, Language};

/// Raw bytes of the image embedded in the first sample message, baked into the
/// binary so the message is self-contained: the webview renders it from a
/// `data:` URI (see [`embedded_image_data_uri`]), which avoids file-access
/// quirks on a page loaded from an HTML string. Must be a real raster image a
/// browser engine can decode (PNG/JPEG).
const EMBEDDED_IMAGE_BYTES: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/tweezers.png"));

/// Requested display size (px) for the embedded image. Matches the asset's
/// intrinsic 700×200 so the explicit `width`/`height` keep its aspect ratio.
const EMBEDDED_IMAGE_WIDTH: u32 = 700;
const EMBEDDED_IMAGE_HEIGHT: u32 = 200;

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

/// The content of a message. Real e-mails arrive either as HTML or plain text,
/// so the reader must handle both; the viewer renders each appropriately.
#[derive(Debug, Clone)]
pub enum MessageBody {
    Html(SharedString),
    Text(SharedString),
}

/// An e-mail message.
#[derive(Debug, Clone)]
pub struct Message {
    pub sender: SharedString,
    pub sender_email: SharedString,
    pub subject: SharedString,
    pub preview: SharedString,
    pub body: MessageBody,
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
        .enumerate()
        .map(
            |(idx, (sender, email, subject, preview, unread, starred, attach, time))| {
                // Mix HTML and plain-text bodies so the reader exercises both
                // rendering paths (every fourth message is plain text).
                let body = if idx % 4 == 3 {
                    MessageBody::Text(text_body(preview, sender))
                } else {
                    // The first message embeds a real image (as a self-contained
                    // `data:` URI); the others reference a remote URL.
                    let image_src = if idx == 0 {
                        embedded_image_data_uri()
                    } else {
                        "https://example.com/banner.png".to_string()
                    };
                    MessageBody::Html(html_body(subject, preview, sender, &image_src))
                };
                Message {
                    sender: sender.into(),
                    sender_email: email.into(),
                    subject: subject.into(),
                    preview: preview.into(),
                    body,
                    time: time.into(),
                    unread,
                    starred,
                    has_attachment: attach,
                }
            },
        )
        .collect()
}

/// Builds a rich HTML body that exercises the reader's HTML viewer.
fn html_body(subject: &str, preview: &str, sender: &str, image_src: &str) -> SharedString {
    let width = EMBEDDED_IMAGE_WIDTH;
    let height = EMBEDDED_IMAGE_HEIGHT;
    format!(
        "<h2>{subject}</h2>\
         <p>{preview}</p>\
         <p>Hi there,</p>\
         <p>This message is rendered by rMail's built-in <strong>HTML viewer</strong>. \
         It supports <em>emphasis</em>, <u>underline</u>, <s>strikethrough</s>, \
         <a href=\"https://example.com\">links</a> and <code>inline code</code>.</p>\
         <h3>Highlights</h3>\
         <ul>\
           <li>Rich text with <strong>bold</strong> and <em>italic</em></li>\
           <li>Ordered and unordered lists</li>\
           <li>Block quotes, code blocks and rules</li>\
         </ul>\
         <h3>Steps</h3>\
         <ol>\
           <li>Open the message</li>\
           <li>Read the formatted content</li>\
           <li>Reply when ready</li>\
         </ol>\
         <blockquote>\"Simplicity is the ultimate sophistication.\"</blockquote>\
         <pre>fn main() {{\n    println!(\"Hello, rMail!\");\n}}</pre>\
         <hr>\
         <p>Messages can embed images; local ones render inline, remote ones \
         show as a placeholder:</p>\
         <p><img src=\"{image_src}\" alt=\"Embedded image\" width=\"{width}\" height=\"{height}\"></p>\
         <p>Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor \
         incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud \
         exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure \
         dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur.</p>\
         <p>Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt \
         mollit anim id est laborum. Sed ut perspiciatis unde omnis iste natus error sit \
         voluptatem accusantium doloremque laudantium, totam rem aperiam.</p>\
         <p>Best regards,<br><strong>{sender}</strong></p>"
    )
    .into()
}

/// Builds a plain-text body (the other common e-mail format).
fn text_body(preview: &str, sender: &str) -> SharedString {
    format!(
        "{preview}\n\n\
         This is a plain-text message body. Some senders still deliver mail as plain text, \
         so the reader must handle it as well as HTML.\n\n\
         Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor \
         incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud \
         exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.\n\n\
         Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat \
         nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui \
         officia deserunt mollit anim id est laborum.\n\n\
         Best regards,\n{sender}"
    )
    .into()
}

/// Standard base64 alphabet (RFC 4648).
const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Minimal base64 encoder (with padding). Kept dependency-free since the only
/// use is embedding the sample image as a `data:` URI.
fn base64_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(BASE64_ALPHABET[((triple >> 18) & 0x3f) as usize] as char);
        out.push(BASE64_ALPHABET[((triple >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            BASE64_ALPHABET[((triple >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            BASE64_ALPHABET[(triple & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// The embedded image as a self-contained `data:` URI the webview can render.
fn embedded_image_data_uri() -> String {
    format!(
        "data:image/png;base64,{}",
        base64_encode(EMBEDDED_IMAGE_BYTES)
    )
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
    fn sample_messages_include_html_and_text_bodies() {
        let messages = sample_messages();
        assert!(messages
            .iter()
            .any(|m| matches!(m.body, MessageBody::Html(_))));
        assert!(messages
            .iter()
            .any(|m| matches!(m.body, MessageBody::Text(_))));
    }

    #[test]
    fn first_message_embeds_the_image_as_a_data_uri() {
        let messages = sample_messages();
        let MessageBody::Html(html) = &messages[0].body else {
            panic!("first message should be HTML");
        };
        // The image is baked into the body as a self-contained data URI so the
        // webview renders it without any file access.
        assert!(
            html.contains("data:image/png;base64,"),
            "HTML body must embed the image as a data URI"
        );
        // Explicit dimensions let us verify the webview honors width/height.
        assert!(html.contains(&format!("width=\"{EMBEDDED_IMAGE_WIDTH}\"")));
        assert!(html.contains(&format!("height=\"{EMBEDDED_IMAGE_HEIGHT}\"")));
    }

    #[test]
    fn base64_encodes_known_vectors() {
        // Classic RFC 4648 examples, covering each padding case.
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"Man"), "TWFu");
    }

    #[test]
    fn embedded_image_data_uri_is_well_formed() {
        let uri = embedded_image_data_uri();
        assert!(uri.starts_with("data:image/png;base64,"));
        assert!(uri.len() > "data:image/png;base64,".len());
    }

    #[test]
    fn embedded_image_is_a_decodable_raster() {
        // A WebP-in-.png (or any non-raster) would silently fail to render in the
        // webview. Guard the magic bytes for PNG/JPEG.
        let bytes = EMBEDDED_IMAGE_BYTES;
        let is_png = bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        let is_jpeg = bytes.starts_with(&[0xFF, 0xD8, 0xFF]);
        assert!(
            is_png || is_jpeg,
            "embedded image must be a real PNG/JPEG the engine can decode (got {:?})",
            &bytes[..bytes.len().min(8)]
        );
    }

    #[test]
    fn embedded_image_is_wider_than_a_typical_reading_pane() {
        // A reading pane is roughly 520–620px; the image must be wider so the
        // horizontal scrollbar is actually exercised.
        assert!(EMBEDDED_IMAGE_WIDTH >= 640);
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
