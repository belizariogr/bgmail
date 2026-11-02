//! Mail domain vocabulary shared by the sync engine, storage adapters and
//! protocol clients: addresses, flags, folders, MIME parsing/building,
//! reply/thread helpers and the `MailRemote` / `MailSender` contracts.
//!
//! Pure logic only: no GPUI, no network, no filesystem I/O.

#![forbid(unsafe_code)]

pub mod account;
pub mod address;
pub mod cancel;
pub mod compose;
#[cfg(any(test, feature = "fake"))]
pub mod fake;
pub mod flags;
pub mod folder;
pub mod mailto;
pub mod parse;
pub mod remote;
pub mod reply;
pub mod thread;

pub use account::{AccountConfig, AuthKind, Security, ServerConfig, DEFAULT_SYNC_WINDOW};
pub use address::{
    format_address_list, parse_address_list, same_email, split_tokens, valid_addresses, Address,
};
pub use cancel::{CancelGuard, CancelToken};
pub use compose::{
    build_message, envelope_recipients, guess_mime_type, new_message_id, sent_copy, Draft,
    OutgoingAttachment,
};
pub use flags::MessageFlags;
pub use folder::{
    decode_imap_utf7, encode_imap_utf7, role_from_name, role_from_special_use, FolderRole,
    RemoteFolder,
};
pub use mailto::draft_from_mailto;
pub use parse::{
    attachment_content, has_attachments, html_references_cid, inline_images, parse_message,
    AttachmentInfo, ParsedMessage,
};
pub use remote::{
    Capabilities, FolderStatus, IdleOutcome, MailConnector, MailRemote, MailSender, RemoteError,
    RemoteErrorKind, RemoteFlags, RemoteHeader, RemoteResult, SendError, SendFailure,
};
pub use reply::{
    forward_subject, html_to_text, prepare_reply, quote_text, readable_text, reply_html,
    reply_html_with_original, reply_subject, QuoteLabels, ReplyKind,
};
pub use thread::{normalize_subject, thread_key};
