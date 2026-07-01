//! SQLite schema version and DDL.

pub const SCHEMA_VERSION: i64 = 1;

pub const CREATE_ACCOUNTS: &str = "
CREATE TABLE IF NOT EXISTS accounts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    email TEXT NOT NULL UNIQUE
);
";

pub const CREATE_FOLDERS: &str = "
CREATE TABLE IF NOT EXISTS folders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    display_name TEXT NOT NULL DEFAULT '',
    UNIQUE(account_id, path)
);
";

pub const CREATE_MESSAGES: &str = "
CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    sender TEXT NOT NULL,
    sender_email TEXT NOT NULL,
    subject TEXT NOT NULL,
    plain_text TEXT NOT NULL,
    raw_content TEXT NOT NULL,
    raw_format TEXT NOT NULL CHECK (raw_format IN ('html', 'text')),
    preview TEXT NOT NULL,
    search_text TEXT NOT NULL,
    received_at TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    unread INTEGER NOT NULL DEFAULT 0,
    starred INTEGER NOT NULL DEFAULT 0,
    has_attachment INTEGER NOT NULL DEFAULT 0,
    folders_csv TEXT NOT NULL
);
";

pub const CREATE_META: &str = "
CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
";

pub const INDEX_MESSAGES_SEARCH: &str =
    "CREATE INDEX IF NOT EXISTS idx_messages_search ON messages(search_text);";
pub const INDEX_MESSAGES_ACCOUNT: &str =
    "CREATE INDEX IF NOT EXISTS idx_messages_account ON messages(account_id);";
pub const INDEX_MESSAGES_FOLDERS: &str =
    "CREATE INDEX IF NOT EXISTS idx_messages_folders ON messages(folders_csv);";
