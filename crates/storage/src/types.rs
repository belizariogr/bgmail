//! Rows returned from SQLite queries.

/// A connected account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: i64,
    pub name: String,
    pub email: String,
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
    pub unread: bool,
    pub starred: bool,
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
