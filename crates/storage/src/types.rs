//! Rows returned from SQLite queries and inputs for sync writes.

/// Provider of local/mock accounts (no server behind them).
pub const PROVIDER_LOCAL: &str = "local";
/// Generic IMAP/SMTP account.
pub const PROVIDER_IMAP: &str = "imap";
/// Gmail account (IMAP with Gmail extensions).
pub const PROVIDER_GMAIL: &str = "gmail";

/// A connected account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: i64,
    pub name: String,
    pub email: String,
    /// `local`, `imap` or `gmail` (see the `PROVIDER_*` constants).
    pub provider: String,
    /// Opaque connection settings owned by callers (JSON text, `{}` when unset).
    pub config_json: String,
}

/// A folder within an account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub id: i64,
    pub account_id: i64,
    /// Storage path (`sys:inbox`, `user:Clients`, …).
    pub path: String,
    /// Display name for custom folders; empty for system folders (localized in UI).
    pub display_name: String,
    /// Server mailbox name; `None` for local-only folders.
    pub remote_name: Option<String>,
    /// Special-use role (`inbox`, `sent`, `drafts`, `trash`, `junk`, `archive`,
    /// `all`, `flagged`, `important`, `custom`) or `None`.
    pub role: Option<String>,
}

/// A message row for the list column (no raw body).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageListItem {
    pub id: i64,
    pub account_id: i64,
    pub sender: String,
    pub sender_email: String,
    pub subject: String,
    /// Plain-text preview (first line / excerpt).
    pub preview: String,
    pub received_at: String,
    /// Unix seconds; `0` for local/mock rows (ordered by `sort_order` instead).
    pub date_ts: i64,
    pub thread_id: Option<String>,
    pub unread: bool,
    pub starred: bool,
    pub has_attachment: bool,
}

/// Full message for the reader (raw body for rendering).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageDetail {
    pub id: i64,
    pub account_id: i64,
    pub sender: String,
    pub sender_email: String,
    pub subject: String,
    pub plain_text: String,
    pub raw_content: String,
    /// `html` or `text`.
    pub raw_format: String,
    pub received_at: String,
    pub date_ts: i64,
    /// Display-formatted recipient lists (opaque to storage).
    pub to_addrs: String,
    pub cc_addrs: String,
    pub reply_to: String,
    pub message_id_hdr: Option<String>,
    pub in_reply_to: Option<String>,
    /// Space-separated Message-IDs from the `References` header.
    pub references_hdr: String,
    pub thread_id: Option<String>,
    /// `0` = headers only (body not downloaded yet), `1` = body present.
    pub body_state: u8,
    pub unread: bool,
    pub starred: bool,
    pub answered: bool,
    pub has_attachment: bool,
    pub folders_csv: String,
}

/// Which messages to list in the middle column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailListQuery {
    /// Unified system folder across every account (`sys:inbox`, …).
    GlobalSystemFolder(String),
    /// One account + folder path.
    AccountFolder {
        account_id: i64,
        folder_path: String,
    },
    /// Full-text search across all accounts/folders.
    Search(String),
}

/// IMAP synchronization checkpoint of one folder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FolderSyncState {
    pub uid_validity: Option<u32>,
    pub uid_next: Option<u32>,
    pub highest_modseq: Option<u64>,
    /// Unix seconds of the last successful sync.
    pub synced_at: Option<i64>,
}

/// Where a message lives on the server: one `(folder, uid)` pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageLocation {
    pub folder_id: i64,
    pub remote_name: Option<String>,
    pub path: String,
    pub uid: u32,
}

/// Headers and flags of a message discovered on the server (body comes later).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewRemoteMessage {
    pub sender: String,
    pub sender_email: String,
    pub subject: String,
    pub preview: String,
    pub plain_text: String,
    pub to_addrs: String,
    pub cc_addrs: String,
    pub reply_to: String,
    pub message_id_hdr: Option<String>,
    pub in_reply_to: Option<String>,
    pub references_hdr: String,
    pub thread_id: Option<String>,
    pub date_ts: i64,
    /// Display string shown in the list (kept as-is).
    pub received_at: String,
    pub unread: bool,
    pub starred: bool,
    pub answered: bool,
    pub has_attachment: bool,
    pub size: u64,
    pub gm_msgid: Option<u64>,
}

/// Result of [`crate::Database::record_remote_message`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteInsert {
    pub message_id: i64,
    /// `true` when a new message row was created (not a merge or a known
    /// location).
    pub created: bool,
}

/// Attachment metadata extracted from a downloaded body.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewAttachment {
    /// Index of the MIME part inside the raw message (caller-defined numbering).
    pub part_index: u32,
    pub filename: String,
    pub mime_type: String,
    pub size: u64,
    pub content_id: Option<String>,
    /// Inline parts (e.g. `cid:` images) do not count as attachments in the list.
    pub inline: bool,
}

/// Stored attachment metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentRow {
    pub id: i64,
    pub message_id: i64,
    pub part_index: u32,
    pub filename: String,
    pub mime_type: String,
    pub size: u64,
    pub content_id: Option<String>,
    pub inline: bool,
}

/// A server-side action queued while the UI already shows its result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingOp {
    pub id: i64,
    pub account_id: i64,
    /// Caller-defined action name (`move`, `flag`, `expunge`, …).
    pub kind: String,
    pub message_id: Option<i64>,
    /// Opaque JSON owned by callers.
    pub payload: String,
    pub attempts: u32,
    pub last_error: Option<String>,
    /// Unix seconds.
    pub created_at: i64,
}

/// A message waiting to be sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingRow {
    pub id: i64,
    pub account_id: i64,
    pub sender: String,
    pub recipients: Vec<String>,
    /// Raw RFC 822 bytes.
    pub raw: Vec<u8>,
    pub message_id_hdr: Option<String>,
    pub attempts: u32,
    pub last_error: Option<String>,
    /// Unix seconds.
    pub created_at: i64,
}
