//! Stepwise, idempotent schema migrations driven by `meta.schema_version`.
//!
//! Each step alters tables in place (never drop + recreate) so mail that already
//! lives in a user's database survives upgrades.

use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};

use crate::schema::{
    CREATE_ACCOUNTS, CREATE_FOLDERS, CREATE_MESSAGES, CREATE_META, INDEX_MESSAGES_ACCOUNT,
    INDEX_MESSAGES_FOLDERS, INDEX_MESSAGES_SEARCH, SCHEMA_VERSION, V2_COLUMNS, V2_OBJECTS,
    V3_COLUMNS, V3_OBJECTS, V4_OBJECTS, V5_COLUMNS,
};

/// Brings the database to [`SCHEMA_VERSION`] inside one `IMMEDIATE` transaction,
/// so two connections opening the same file at once migrate exactly once.
pub(crate) fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    for ddl in [
        CREATE_META,
        CREATE_ACCOUNTS,
        CREATE_FOLDERS,
        CREATE_MESSAGES,
        INDEX_MESSAGES_SEARCH,
        INDEX_MESSAGES_ACCOUNT,
        INDEX_MESSAGES_FOLDERS,
    ] {
        tx.execute_batch(ddl)?;
    }

    // Missing version = fresh file (just created with the v1 DDL above) or a
    // pre-versioning v1 database; both continue from v1.
    let start = schema_version(&tx)?.unwrap_or(1);
    let mut version = start;
    if version < 2 {
        migrate_v1_to_v2(&tx)?;
        version = 2;
    }
    if version < 3 {
        migrate_v2_to_v3(&tx)?;
        version = 3;
    }
    if version < 4 {
        tx.execute_batch(V4_OBJECTS)?;
        version = 4;
    }
    if version < 5 {
        for (table, column, definition) in V5_COLUMNS {
            add_column_if_missing(&tx, table, column, definition)?;
        }
        version = 5;
    }
    if version != start || schema_version(&tx)?.is_none() {
        tx.execute(
            "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![version.to_string()],
        )?;
    }
    debug_assert!(version >= SCHEMA_VERSION);
    tx.commit()
}

/// Reads `meta.schema_version` (`None` when absent or unparsable).
pub(crate) fn schema_version(conn: &Connection) -> rusqlite::Result<Option<i64>> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    Ok(value.and_then(|value| value.parse().ok()))
}

/// v2: account providers, IMAP folder state, message headers/threading, server
/// locations, raw bodies, attachments, queued ops, outbox and the FTS index.
fn migrate_v1_to_v2(conn: &Connection) -> rusqlite::Result<()> {
    for (table, column, definition) in V2_COLUMNS {
        add_column_if_missing(conn, table, column, definition)?;
    }
    conn.execute_batch(V2_OBJECTS)?;
    conn.execute_batch("INSERT INTO messages_fts(messages_fts) VALUES ('rebuild');")
}

/// v3: outbox delivery state (`queued` / `sending` / `maybe_sent` / `failed` /
/// `append_pending`), retry backoff (`next_attempt_at`, `deferrals`) and the
/// send lease. Existing rows become `queued`, due immediately.
fn migrate_v2_to_v3(conn: &Connection) -> rusqlite::Result<()> {
    for (table, column, definition) in V3_COLUMNS {
        add_column_if_missing(conn, table, column, definition)?;
    }
    conn.execute_batch(V3_OBJECTS)
}

/// `ALTER TABLE ADD COLUMN` that tolerates a column already being there.
fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> rusqlite::Result<()> {
    if column_exists(conn, table, column)? {
        return Ok(());
    }
    conn.execute_batch(&format!(
        "ALTER TABLE {table} ADD COLUMN {column} {definition};"
    ))
}

/// Whether `table` has `column` (via `PRAGMA table_info`).
pub(crate) fn column_exists(
    conn: &Connection,
    table: &str,
    column: &str,
) -> rusqlite::Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for name in names {
        if name? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::folder::system;
    use crate::search::build_search_text;
    use crate::types::MailListQuery;
    use tempfile::NamedTempFile;

    /// Creates a database exactly as the v1 app left it, with one message.
    fn v1_database(path: &std::path::Path) {
        let conn = Connection::open(path).unwrap();
        for ddl in [
            CREATE_META,
            CREATE_ACCOUNTS,
            CREATE_FOLDERS,
            CREATE_MESSAGES,
            INDEX_MESSAGES_SEARCH,
            INDEX_MESSAGES_ACCOUNT,
            INDEX_MESSAGES_FOLDERS,
        ] {
            conn.execute_batch(ddl).unwrap();
        }
        conn.execute(
            "INSERT INTO meta (key, value) VALUES ('schema_version', '1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO accounts (name, email) VALUES ('Personal', 'me@test.com')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO folders (account_id, path, display_name) VALUES (1, 'sys:inbox', '')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (account_id, sender, sender_email, subject, plain_text,
                raw_content, raw_format, preview, search_text, received_at, sort_order,
                unread, starred, has_attachment, folders_csv)
             VALUES (1, 'Ana', 'ana@test.com', 'Relatório', 'Corpo', '<p>Corpo</p>', 'html',
                'Corpo', ?1, '09:00', 3, 1, 0, 1, ',sys:inbox,')",
            params![build_search_text(
                "Ana",
                "ana@test.com",
                "Relatório",
                "Corpo"
            )],
        )
        .unwrap();
    }

    #[test]
    fn v1_database_migrates_in_place_preserving_data() {
        let file = NamedTempFile::new().unwrap();
        v1_database(file.path());

        let db = Database::open(file.path()).unwrap();
        assert_eq!(schema_version(db.conn()).unwrap(), Some(SCHEMA_VERSION));

        let accounts = db.list_accounts().unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].provider, "local");
        assert_eq!(accounts[0].config_json, "{}");

        let folders = db.list_folders(accounts[0].id).unwrap();
        assert_eq!(folders[0].path, system::INBOX);
        assert_eq!(folders[0].remote_name, None);
        assert_eq!(folders[0].role, None);

        let inbox = db
            .list_messages(&MailListQuery::GlobalSystemFolder(system::INBOX.into()))
            .unwrap();
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].date_ts, 0);
        let detail = db.get_message(inbox[0].id).unwrap().unwrap();
        assert_eq!(detail.subject, "Relatório");
        assert_eq!(detail.raw_content, "<p>Corpo</p>");
        assert_eq!(detail.body_state, 1);
        assert!(detail.unread && detail.has_attachment && !detail.answered);
        assert_eq!(detail.references_hdr, "");

        // The FTS index is rebuilt from rows that predate it.
        let hits = db
            .list_messages(&MailListQuery::Search("relatorio".into()))
            .unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn v2_outbox_rows_migrate_to_queued_v3_rows() {
        let file = NamedTempFile::new().unwrap();
        v1_database(file.path());
        {
            let conn = Connection::open(file.path()).unwrap();
            migrate_v1_to_v2(&conn).unwrap();
            conn.execute(
                "UPDATE meta SET value = '2' WHERE key = 'schema_version'",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO outbox (account_id, sender, recipients, raw, attempts, last_error)
                 VALUES (1, 'me@test.com', 'a@x', X'52', 2, '550')",
                [],
            )
            .unwrap();
            assert!(!column_exists(&conn, "outbox", "state").unwrap());
        }
        let db = Database::open(file.path()).unwrap();
        assert_eq!(schema_version(db.conn()).unwrap(), Some(SCHEMA_VERSION));
        let rows = db.outbox(1).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].state, crate::types::OutgoingState::Queued);
        assert_eq!((rows[0].attempts, rows[0].deferrals), (2, 0));
        assert_eq!(rows[0].next_attempt_at, 0);
        assert_eq!(rows[0].raw, b"R");
    }

    #[test]
    fn migration_is_idempotent_across_reopens() {
        let file = NamedTempFile::new().unwrap();
        v1_database(file.path());
        drop(Database::open(file.path()).unwrap());
        let db = Database::open(file.path()).unwrap();
        assert_eq!(schema_version(db.conn()).unwrap(), Some(SCHEMA_VERSION));
        assert_eq!(db.message_count_all().unwrap(), 1);
        let hits = db
            .list_messages(&MailListQuery::Search("ana".into()))
            .unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn fresh_database_starts_at_current_version() {
        let file = NamedTempFile::new().unwrap();
        let db = Database::open(file.path()).unwrap();
        assert_eq!(schema_version(db.conn()).unwrap(), Some(SCHEMA_VERSION));
        assert!(column_exists(db.conn(), "messages", "gm_msgid").unwrap());
        assert!(column_exists(db.conn(), "accounts", "provider").unwrap());
        assert!(column_exists(db.conn(), "outbox", "next_attempt_at").unwrap());
    }

    #[test]
    fn add_column_if_missing_tolerates_existing_columns() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE t (a INTEGER);").unwrap();
        add_column_if_missing(&conn, "t", "b", "TEXT").unwrap();
        add_column_if_missing(&conn, "t", "b", "TEXT").unwrap();
        assert!(column_exists(&conn, "t", "b").unwrap());
        assert!(!column_exists(&conn, "t", "c").unwrap());
    }

    #[test]
    fn schema_version_is_none_without_row() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(CREATE_META).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), None);
    }
}
