//! SQLite schema version and DDL.
//!
//! The v1 DDL below is the base every database starts from; later versions are
//! applied as in-place steps by `migration.rs` so existing user data survives.

pub const SCHEMA_VERSION: i64 = 8;

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

/// v1 → v2 columns added in place: `(table, column, definition)`.
pub const V2_COLUMNS: &[(&str, &str, &str)] = &[
    ("accounts", "provider", "TEXT NOT NULL DEFAULT 'local'"),
    ("accounts", "config_json", "TEXT NOT NULL DEFAULT '{}'"),
    ("accounts", "sort_order", "INTEGER NOT NULL DEFAULT 0"),
    ("folders", "remote_name", "TEXT"),
    ("folders", "role", "TEXT"),
    ("folders", "uid_validity", "INTEGER"),
    ("folders", "uid_next", "INTEGER"),
    ("folders", "highest_modseq", "INTEGER"),
    ("folders", "synced_at", "INTEGER"),
    ("messages", "message_id_hdr", "TEXT"),
    ("messages", "in_reply_to", "TEXT"),
    ("messages", "references_hdr", "TEXT NOT NULL DEFAULT ''"),
    ("messages", "thread_id", "TEXT"),
    ("messages", "to_addrs", "TEXT NOT NULL DEFAULT ''"),
    ("messages", "cc_addrs", "TEXT NOT NULL DEFAULT ''"),
    ("messages", "reply_to", "TEXT NOT NULL DEFAULT ''"),
    ("messages", "date_ts", "INTEGER NOT NULL DEFAULT 0"),
    ("messages", "answered", "INTEGER NOT NULL DEFAULT 0"),
    ("messages", "body_state", "INTEGER NOT NULL DEFAULT 1"),
    ("messages", "gm_msgid", "INTEGER"),
    ("messages", "size", "INTEGER NOT NULL DEFAULT 0"),
];

/// v2 tables, indexes, full-text index and its sync triggers (all idempotent).
pub const V2_OBJECTS: &str = "
CREATE TABLE IF NOT EXISTS message_locations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    folder_id INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    uid INTEGER NOT NULL,
    UNIQUE(folder_id, uid)
);
CREATE INDEX IF NOT EXISTS idx_locations_message ON message_locations(message_id);

CREATE TABLE IF NOT EXISTS message_bodies (
    message_id INTEGER PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    raw BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS attachments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    part_index INTEGER NOT NULL DEFAULT 0,
    filename TEXT NOT NULL DEFAULT '',
    mime_type TEXT NOT NULL DEFAULT '',
    size INTEGER NOT NULL DEFAULT 0,
    content_id TEXT,
    inline INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_attachments_message ON attachments(message_id);

CREATE TABLE IF NOT EXISTS pending_ops (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    message_id INTEGER,
    payload TEXT NOT NULL DEFAULT '{}',
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_pending_ops_account ON pending_ops(account_id);

CREATE TABLE IF NOT EXISTS outbox (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    sender TEXT NOT NULL,
    recipients TEXT NOT NULL,
    raw BLOB NOT NULL,
    message_id_hdr TEXT,
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_messages_msgid ON messages(account_id, message_id_hdr);
CREATE INDEX IF NOT EXISTS idx_messages_gm_msgid ON messages(account_id, gm_msgid);
CREATE INDEX IF NOT EXISTS idx_messages_thread ON messages(thread_id);
CREATE INDEX IF NOT EXISTS idx_messages_date ON messages(date_ts);

CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    search_text,
    content = 'messages',
    content_rowid = 'id',
    tokenize = 'trigram'
);
CREATE TRIGGER IF NOT EXISTS messages_fts_insert AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts(rowid, search_text) VALUES (new.id, new.search_text);
END;
CREATE TRIGGER IF NOT EXISTS messages_fts_delete AFTER DELETE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, search_text)
    VALUES ('delete', old.id, old.search_text);
END;
CREATE TRIGGER IF NOT EXISTS messages_fts_update AFTER UPDATE OF search_text ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, search_text)
    VALUES ('delete', old.id, old.search_text);
    INSERT INTO messages_fts(rowid, search_text) VALUES (new.id, new.search_text);
END;
";

/// v2 → v3 outbox columns (delivery state machine + retry backoff):
/// `(table, column, definition)`.
pub const V3_COLUMNS: &[(&str, &str, &str)] = &[
    ("outbox", "state", "TEXT NOT NULL DEFAULT 'queued'"),
    ("outbox", "next_attempt_at", "INTEGER NOT NULL DEFAULT 0"),
    ("outbox", "lease_at", "INTEGER"),
    ("outbox", "deferrals", "INTEGER NOT NULL DEFAULT 0"),
];

/// v3 indexes (idempotent).
pub const V3_OBJECTS: &str = "
CREATE INDEX IF NOT EXISTS idx_outbox_account_state ON outbox(account_id, state);
";

/// v4: people the user exchanged mail with, offered when addressing a message.
pub const V4_OBJECTS: &str = "
CREATE TABLE IF NOT EXISTS contacts (
    email TEXT PRIMARY KEY COLLATE NOCASE,
    name TEXT NOT NULL DEFAULT ''
);
";

/// v5: the name an account's mail goes out with, apart from the one it is listed
/// under. Blank until set: mail keeps going out with the account's name.
pub const V5_COLUMNS: &[(&str, &str, &str)] =
    &[("accounts", "sender_name", "TEXT NOT NULL DEFAULT ''")];

/// v8: how the server writes folder names, read from its list, for naming new ones.
pub const V8_COLUMNS: &[(&str, &str, &str)] = &[
    ("folders", "delimiter", "TEXT"),
    ("accounts", "folder_prefix", "TEXT NOT NULL DEFAULT ''"),
];

/// v7: when the user last asked for a retry, from which an unsent message's age counts.
pub const V7_COLUMNS: &[(&str, &str, &str)] =
    &[("outbox", "retry_from", "INTEGER NOT NULL DEFAULT 0")];
