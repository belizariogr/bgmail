# crates/storage/src/ — Source API

## Purpose

Source for the `storage` crate: local SQLite schema, queries, folder conventions, message mutations, search helpers, and seed import.

## Ownership

- Owns: All modules under `crates/storage/src/`.
- Does not own: crate-level dependency/feature policy (parent `.dox/crates/storage/AGENTS.md`).

## Local Contracts

- Every function/method in this tree is listed under **API Catalog** with purpose
  and behavior. Update the matching entry when changing signatures or semantics.
- Prefer rustdoc on public items; DOX purpose/behavior should stay aligned with
  rustdoc.
- Visibility in entries (`pub` / `private`) reflects the source item.
- No networking or remote sync; all persistence is local SQLite only.

## Work Guidance

- After adding/removing/renaming a function, update this catalog in the same
  change.
- Do not weaken parent DOX contracts from root/`crates/`/`storage/`.
- Folder paths use `sys:` / `user:` prefixes; membership is stored in comma-wrapped `folders_csv` columns.

## Verification

- `cargo test -p storage`
- Workspace `fmt` / `clippy -D warnings` as required by root `AGENTS.md`.

## Child DOX Index

_(none — modules are files; their APIs are cataloged below.)_

## API Catalog

_68 functions/methods documented._

### `src/lib.rs`

#### Types / constants

- _(Re-exports only — no local types or functions.)_

#### Functions / methods

_(No functions.)_

### `src/database.rs`

#### Types / constants

- **struct `Database`**: Owns an open `rusqlite::Connection` to the local mail store; all query and mutation methods hang off this type.

#### Functions / methods

##### Context: `module`

- **`database_path`** (pub, L16)
  - Signature: `pub fn database_path() -> PathBuf`
  - Purpose: Returns the default on-disk location for `mail.db`.
  - Behavior: Joins `config_dir()` with `"mail.db"`. Typically `~/.config/BGMail/mail.db` when `HOME` or `USERPROFILE` is set. No I/O.

- **`config_dir`** (private, L20)
  - Signature: `fn config_dir() -> PathBuf`
  - Purpose: Resolves the BGMail config directory across platforms.
  - Behavior: Uses `$HOME/.config/BGMail` or `$USERPROFILE/.config/BGMail`; falls back to `.config/BGMail` relative path when neither env var exists. No directory creation.

- **`global_folder_path`** (pub, L301)
  - Signature: `pub fn global_folder_path(name: &str) -> Option<&'static str>`
  - Purpose: Maps UI global-mailbox keys to canonical system folder paths.
  - Behavior: `"inbox" → sys:inbox`, `"flagged" → sys:flagged`, `"drafts" → sys:drafts`, `"sent" → sys:sent`; any other name returns `None`. No I/O.

##### Context: `Database`

- **`open`** (pub, L38)
  - Signature: `pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self>`
  - Purpose: Opens or creates a SQLite database and brings schema to current version.
  - Behavior: Creates parent directories (errors ignored), opens the file, enables `PRAGMA foreign_keys = ON`, runs `migrate()`, returns `Database`. Propagates SQLite errors; does not panic.

- **`open_default`** (pub, L50)
  - Signature: `pub fn open_default() -> rusqlite::Result<Self>`
  - Purpose: Convenience opener for the standard user config path.
  - Behavior: Delegates to `Self::open(database_path())`.

- **`conn`** (pub, L54)
  - Signature: `pub fn conn(&self) -> &Connection`
  - Purpose: Exposes the underlying SQLite connection for seeding and low-level use.
  - Behavior: Returns `&self.conn`. No side effects.

- **`migrate`** (private, L58)
  - Signature: `fn migrate(&self) -> rusqlite::Result<()>`
  - Purpose: Applies DDL and one-time schema bookkeeping on open.
  - Behavior: Executes `CREATE_META`, `CREATE_ACCOUNTS`, `CREATE_FOLDERS`, `CREATE_MESSAGES`, and index DDL from `schema`. Inserts `schema_version` into `meta` when missing. Calls `repair_starred_folder_membership()`. Propagates SQL errors.

- **`repair_starred_folder_membership`** (private, L86)
  - Signature: `fn repair_starred_folder_membership(&self) -> rusqlite::Result<()>`
  - Purpose: Backfills flagged-folder membership for starred messages missing `sys:flagged` in `folders_csv`.
  - Behavior: Selects starred rows whose `folders_csv` does not match the flagged LIKE pattern; parses paths, appends `sys:flagged` when absent, re-serializes via `folders_csv`, and updates each row. No-op when nothing to fix.

- **`list_accounts`** (pub, L123)
  - Signature: `pub fn list_accounts(&self) -> rusqlite::Result<Vec<Account>>`
  - Purpose: Loads all accounts ordered by id.
  - Behavior: `SELECT id, name, email FROM accounts ORDER BY id ASC`; maps rows to `Account`. Returns empty vec when none.

- **`list_folders`** (pub, L137)
  - Signature: `pub fn list_folders(&self, account_id: i64) -> rusqlite::Result<Vec<Folder>>`
  - Purpose: Lists folders belonging to one account.
  - Behavior: Filters `folders` by `account_id`, orders by id, maps to `Folder` (path + display_name).

- **`unread_in_folder`** (pub, L153)
  - Signature: `pub fn unread_in_folder(&self, account_id: Option<i64>, folder_path: &str) -> rusqlite::Result<usize>`
  - Purpose: Counts unread messages in a folder, optionally scoped to one account.
  - Behavior: Builds a `folders_csv` LIKE pattern via `folder_like_pattern`. With `Some(account_id)`, counts unread rows for that account and folder; with `None`, counts across all accounts. Returns `usize` count (cast from SQLite integer).

- **`global_unread`** (pub, L176)
  - Signature: `pub fn global_unread(&self, folder_path: &str) -> rusqlite::Result<usize>`
  - Purpose: Unread count for a system folder aggregated over every account.
  - Behavior: Calls `unread_in_folder(None, folder_path)`.

- **`list_messages`** (pub, L180)
  - Signature: `pub fn list_messages(&self, query: &MailListQuery) -> rusqlite::Result<Vec<MessageListItem>>`
  - Purpose: Primary list-column query dispatcher.
  - Behavior: `MailListQuery::Search` → `search_messages`; `GlobalSystemFolder(path)` → `messages_in_folder(None, path)`; `AccountFolder { account_id, folder_path }` → `messages_in_folder(Some(account_id), folder_path)`.

- **`messages_in_folder`** (private, L191)
  - Signature: `fn messages_in_folder(&self, account_id: Option<i64>, folder_path: &str) -> rusqlite::Result<Vec<MessageListItem>>`
  - Purpose: Lists messages whose `folders_csv` contains the given folder path.
  - Behavior: Uses LIKE pattern matching on comma-wrapped CSV. Optional account filter. Orders by `sort_order ASC, id ASC`. Maps SQLite integer flags to `bool` for unread/starred/has_attachment.

- **`search_messages`** (private, L234)
  - Signature: `fn search_messages(&self, query: &str) -> rusqlite::Result<Vec<MessageListItem>>`
  - Purpose: Accent/case-insensitive search over precomputed `search_text`.
  - Behavior: Returns empty vec immediately when `search_like_pattern` yields `None` (blank/whitespace query). Otherwise `WHERE search_text LIKE ?` with folded partial pattern, same ordering and row mapping as folder lists.

- **`get_message`** (pub, L262)
  - Signature: `pub fn get_message(&self, id: i64) -> rusqlite::Result<Option<MessageDetail>>`
  - Purpose: Loads one message for the reader pane, including body and folder membership.
  - Behavior: Selects full row by primary key. Returns `Ok(None)` when id absent; `Ok(Some(detail))` when found. Converts integer flags to bool.

- **`message_count_for_query`** (pub, L289)
  - Signature: `pub fn message_count_for_query(&self, query: &MailListQuery) -> rusqlite::Result<usize>`
  - Purpose: Counts messages matching a list query (via full list fetch).
  - Behavior: Returns `list_messages(query)?.len()`. Note: loads all matching rows — acceptable for mock/seed scale.

- **`message_count_all`** (pub, L292)
  - Signature: `pub fn message_count_all(&self) -> rusqlite::Result<usize>`
  - Purpose: Total message row count in the database.
  - Behavior: `SELECT COUNT(*) FROM messages`; returns as `usize`.

##### Context: `tests` (`#[cfg(test)]`)

- **`database_path_uses_dot_config_bgmail`** (private, L318)
  - Signature: `fn database_path_uses_dot_config_bgmail()`
  - Purpose: Smoke test for default path naming.
  - Behavior: Asserts path contains `"BGMail"` and file name is `mail.db`. Panics on failure.

- **`account_folder_lists_only_that_account`** (private, L325)
  - Signature: `fn account_folder_lists_only_that_account()`
  - Purpose: Ensures per-account folder queries do not leak cross-account messages.
  - Behavior: Seeds two accounts with one inbox message each; lists inbox for account A and asserts exactly one message with subject `"A only"`. Uses `unwrap` on setup — panics if seed/open fails.

### `src/folder.rs`

#### Types / constants

- **const `SYSTEM_PREFIX`**: `"sys:"` — prefix for built-in folders that cannot be renamed or deleted.
- **const `USER_PREFIX`**: `"user:"` — prefix for user-created custom folders.
- **mod `system` constants**: Canonical storage paths — `INBOX`, `DRAFTS`, `SENT`, `JUNK`, `TRASH`, `ARCHIVE`, `FLAGGED` (each `sys:…`).

#### Functions / methods

##### Context: `module`

- **`folders_csv`** (pub, L24)
  - Signature: `pub fn folders_csv<'a>(paths: impl IntoIterator<Item = &'a str>) -> String`
  - Purpose: Serializes folder paths into the comma-wrapped DB format.
  - Behavior: Joins paths with commas and wraps as `,{path1},{path2},` so SQL `LIKE '%,path,%'` matches unambiguously. Empty iterator yields `,,`.

- **`folder_like_pattern`** (pub, L30)
  - Signature: `pub fn folder_like_pattern(folder_path: &str) -> String`
  - Purpose: Builds a SQL LIKE pattern for membership in `folders_csv`.
  - Behavior: Returns `format!("%,{folder_path},%")`. No escaping beyond the stored path string.

- **`user_folder_path`** (pub, L35)
  - Signature: `pub fn user_folder_path(display_name: &str) -> String`
  - Purpose: Derives the storage path for a custom folder from its display name.
  - Behavior: Returns `user:{display_name}`. Does not validate uniqueness.

- **`is_system_path`** (pub, L40)
  - Signature: `pub fn is_system_path(path: &str) -> bool`
  - Purpose: Detects built-in folder paths.
  - Behavior: True when `path.starts_with(SYSTEM_PREFIX)`.

- **`is_manual_move_destination_forbidden`** (pub, L45)
  - Signature: `pub fn is_manual_move_destination_forbidden(path: &str) -> bool`
  - Purpose: Blocks user-initiated moves into virtual or send/draft mailboxes.
  - Behavior: True for `sys:flagged`, `sys:drafts`, and `sys:sent`; false for inbox, trash, archive, junk, and user folders.

##### Context: `tests` (`#[cfg(test)]`)

- **`folders_csv_wraps_with_commas`** (private, L54)
  - Signature: `fn folders_csv_wraps_with_commas()`
  - Purpose: Verifies CSV wrapping format.
  - Behavior: Asserts inbox+flagged paths serialize to `,sys:inbox,sys:flagged,`.

- **`folder_like_pattern_matches_inside_csv`** (private, L62)
  - Signature: `fn folder_like_pattern_matches_inside_csv()`
  - Purpose: Verifies LIKE pattern shape and substring containment.
  - Behavior: Asserts pattern for inbox is `%,sys:inbox,%` and sample CSV contains `sys:inbox,`.

- **`user_folder_path_uses_prefix`** (private, L68)
  - Signature: `fn user_folder_path_uses_prefix()`
  - Purpose: Verifies custom folder path prefix.
  - Behavior: Asserts `"Clients"` → `user:Clients`.

- **`manual_move_forbidden_for_sent_drafts_and_flagged`** (private, L73)
  - Signature: `fn manual_move_forbidden_for_sent_drafts_and_flagged()`
  - Purpose: Verifies forbidden vs allowed move destinations.
  - Behavior: Asserts sent/drafts/flagged forbidden; inbox and user folder allowed.

### `src/message_ops.rs`

#### Types / constants

- _(None at module top-level.)_

#### Functions / methods

##### Context: `module`

- **`parse_folders_csv`** (pub, L11)
  - Signature: `pub fn parse_folders_csv(csv: &str) -> Vec<String>`
  - Purpose: Deserializes a `folders_csv` column into path segments.
  - Behavior: Splits on commas, drops empty segments, collects owned strings. Order preserved from storage.

- **`message_has_folder`** (pub, L19)
  - Signature: `pub fn message_has_folder(csv: &str, folder_path: &str) -> bool`
  - Purpose: Tests folder membership in a CSV value.
  - Behavior: Parses CSV and returns true if any segment equals `folder_path` exactly.

- **`is_virtual_flagged_folder`** (pub, L26)
  - Signature: `pub fn is_virtual_flagged_folder(path: &str) -> bool`
  - Purpose: Identifies the starred/flagged virtual mailbox path.
  - Behavior: True only when `path == sys:flagged`. Flagged membership is also driven by the `starred` column.

- **`replace_primary_folder`** (pub, L31)
  - Signature: `pub fn replace_primary_folder(paths: &[String], primary: &str) -> Vec<String>`
  - Purpose: Swaps the message's primary mailbox while preserving flagged membership.
  - Behavior: Keeps only virtual flagged paths from input, appends `primary` if not already present, sorts and deduplicates. Non-flagged primaries (inbox, trash, etc.) are dropped from the kept set.

- **`folders_csv_from_paths`** (pub, L46)
  - Signature: `pub fn folders_csv_from_paths(paths: &[String]) -> String`
  - Purpose: Convenience wrapper to serialize a path slice for DB storage.
  - Behavior: Delegates to `folders_csv(paths.iter().map(String::as_str))`.

##### Context: `Database` (`message_ops` impl block)

- **`load_message_folders`** (private, L51)
  - Signature: `fn load_message_folders(&self, id: i64) -> rusqlite::Result<Option<(Vec<String>, bool)>>`
  - Purpose: Reads parsed folder paths and starred flag for one message.
  - Behavior: `SELECT folders_csv, starred WHERE id = ?`. Returns `Ok(None)` when id missing; otherwise `Ok(Some((paths, starred)))` with parsed paths.

- **`set_message_folders`** (private, L64)
  - Signature: `fn set_message_folders(&self, id: i64, paths: &[String]) -> rusqlite::Result<()>`
  - Purpose: Persists an updated folder membership list.
  - Behavior: Serializes paths to CSV and `UPDATE messages SET folders_csv = ?`. Returns `Ok(())` on success.

- **`set_primary_folder`** (private, L74)
  - Signature: `fn set_primary_folder(&self, id: i64, primary: &str) -> rusqlite::Result<()>`
  - Purpose: Changes primary mailbox while keeping flagged membership when present.
  - Behavior: No-op (`Ok(())`) when message id not found. Otherwise applies `replace_primary_folder` and calls `set_message_folders`.

- **`move_message_to_trash`** (pub, L83)
  - Signature: `pub fn move_message_to_trash(&self, id: i64) -> rusqlite::Result<()>`
  - Purpose: Moves a message to Trash.
  - Behavior: Sets primary folder to `sys:trash`; retains `sys:flagged` when starred.

- **`restore_message_from_trash`** (pub, L88)
  - Signature: `pub fn restore_message_from_trash(&self, id: i64) -> rusqlite::Result<()>`
  - Purpose: Undeletes a trashed message back to Inbox.
  - Behavior: No-op when message missing or not currently in trash. Otherwise sets primary to `sys:inbox`.

- **`delete_message_permanently`** (pub, L99)
  - Signature: `pub fn delete_message_permanently(&self, id: i64) -> rusqlite::Result<bool>`
  - Purpose: Hard-deletes a message from SQLite.
  - Behavior: Returns `Ok(false)` when id missing or message is not in trash. When in trash, `DELETE FROM messages WHERE id = ?` and returns `Ok(true)` if a row was removed.

- **`archive_message`** (pub, L113)
  - Signature: `pub fn archive_message(&self, id: i64) -> rusqlite::Result<()>`
  - Purpose: Archives a message.
  - Behavior: Sets primary folder to `sys:archive` via `set_primary_folder`.

- **`mark_message_junk`** (pub, L118)
  - Signature: `pub fn mark_message_junk(&self, id: i64) -> rusqlite::Result<()>`
  - Purpose: Marks a message as junk/spam.
  - Behavior: Sets primary folder to `sys:junk` via `set_primary_folder`.

- **`move_message_to_folder`** (pub, L123)
  - Signature: `pub fn move_message_to_folder(&self, id: i64, folder_path: &str) -> rusqlite::Result<()>`
  - Purpose: User-initiated move to an allowed folder path.
  - Behavior: No-op (`Ok(())`) when destination is forbidden (flagged, drafts, sent). Otherwise updates primary folder to `folder_path`.

- **`toggle_message_starred`** (pub, L131)
  - Signature: `pub fn toggle_message_starred(&self, id: i64) -> rusqlite::Result<bool>`
  - Purpose: Flips starred state and syncs flagged-folder membership.
  - Behavior: Returns `Ok(false)` when message missing. Toggles `starred`, adds or removes `sys:flagged` in paths, sorts/dedups, updates both `starred` and `folders_csv` in one statement. Returns the new starred value.

- **`message_is_in_trash`** (pub, L153)
  - Signature: `pub fn message_is_in_trash(&self, id: i64) -> rusqlite::Result<bool>`
  - Purpose: Checks whether a message's `folders_csv` includes trash.
  - Behavior: Counts rows matching id and trash LIKE pattern; returns whether count > 0.

##### Context: `tests` (`#[cfg(test)]`)

- **`seeded_db`** (private, L170)
  - Signature: `fn seeded_db() -> (NamedTempFile, Database)`
  - Purpose: Test fixture with one account, custom folder, inbox+trashed messages.
  - Behavior: Creates temp DB, seeds mailboxes and two messages (one starred inbox, one in trash). Panics on setup failure.

- **`message_csv`** (private, L245)
  - Signature: `fn message_csv(db: &Database, subject: &str) -> String`
  - Purpose: Test helper to read `folders_csv` by subject.
  - Behavior: Single-row query; panics if subject not found.

- **`message_id`** (private, L255)
  - Signature: `fn message_id(db: &Database, subject: &str) -> i64`
  - Purpose: Test helper to resolve message id by subject.
  - Behavior: Single-row query; panics if subject not found.

- **`replace_primary_folder_keeps_flagged`** (private, L266)
  - Signature: `fn replace_primary_folder_keeps_flagged()`
  - Purpose: Unit test for primary-folder replacement logic.
  - Behavior: Moving from inbox+flagged to trash yields `[flagged, trash]`.

- **`move_to_trash_keeps_flagged_membership`** (private, L276)
  - Signature: `fn move_to_trash_keeps_flagged_membership()`
  - Purpose: Integration test for trash + starred coexistence.
  - Behavior: After trashing starred inbox message, CSV contains trash and flagged, not inbox.

- **`restore_from_trash_moves_to_inbox`** (private, L287)
  - Signature: `fn restore_from_trash_moves_to_inbox()`
  - Purpose: Integration test for restore from trash.
  - Behavior: Restored message has inbox, not trash.

- **`permanent_delete_only_from_trash`** (private, L297)
  - Signature: `fn permanent_delete_only_from_trash()`
  - Purpose: Ensures hard delete is trash-gated.
  - Behavior: Inbox message delete returns false; trashed message delete returns true and leaves one row.

- **`archive_and_junk_replace_primary`** (private, L312)
  - Signature: `fn archive_and_junk_replace_primary()`
  - Purpose: Verifies archive then junk primary transitions.
  - Behavior: After archive, CSV has archive; after junk, CSV has junk.

- **`toggle_starred_updates_flagged_folder`** (private, L326)
  - Signature: `fn toggle_starred_updates_flagged_folder()`
  - Purpose: Verifies star toggle syncs flagged folder.
  - Behavior: Toggle off removes flagged; toggle on re-adds flagged.

- **`move_to_custom_folder`** (private, L341)
  - Signature: `fn move_to_custom_folder()`
  - Purpose: Verifies move to user folder replaces inbox primary.
  - Behavior: After move to `user:Clients`, CSV has custom path, not inbox.

- **`move_to_sent_is_noop`** (private, L352)
  - Signature: `fn move_to_sent_is_noop()`
  - Purpose: Verifies manual move to sent is blocked.
  - Behavior: After attempted move to sent, message remains in inbox only.

### `src/schema.rs`

#### Types / constants

- **const `SCHEMA_VERSION`**: Current schema version integer (`1`); written to `meta` on first migrate.
- **const `CREATE_ACCOUNTS`**: DDL for `accounts` table (id, name, unique email).
- **const `CREATE_FOLDERS`**: DDL for `folders` table with FK to accounts and unique `(account_id, path)`.
- **const `CREATE_MESSAGES`**: DDL for `messages` table (headers, bodies, search blob, flags, `folders_csv`; `raw_format` constrained to `html` or `text`).
- **const `CREATE_META`**: DDL for key/value `meta` table (schema version storage).
- **const `INDEX_MESSAGES_SEARCH`**: Index on `messages.search_text` for search queries.
- **const `INDEX_MESSAGES_ACCOUNT`**: Index on `messages.account_id` for per-account lists.
- **const `INDEX_MESSAGES_FOLDERS`**: Index on `messages.folders_csv` for folder LIKE filters.

#### Functions / methods

_(No functions.)_

### `src/search.rs`

#### Types / constants

- _(None at module top-level.)_

#### Functions / methods

##### Context: `module`

- **`fold_for_search`** (pub, L6)
  - Signature: `pub fn fold_for_search(text: &str) -> String`
  - Purpose: Normalizes text for accent-insensitive, case-insensitive matching.
  - Behavior: NFD-decomposes, strips combining marks, lowercases. E.g. `"São Paulo"` → `"sao paulo"`. Allocates new string.

- **`search_like_pattern`** (pub, L14)
  - Signature: `pub fn search_like_pattern(query: &str) -> Option<String>`
  - Purpose: Builds a partial-match SQL LIKE pattern from user search input.
  - Behavior: Trims query, folds via `fold_for_search`. Returns `None` when folded string empty; otherwise `Some("%{folded}%")`.

- **`build_search_text`** (pub, L24)
  - Signature: `pub fn build_search_text(sender: &str, sender_email: &str, subject: &str, plain_text: &str) -> String`
  - Purpose: Composes the persisted search blob for a message row.
  - Behavior: Joins all four fields with spaces, then applies `fold_for_search`. Stored in `messages.search_text` at seed/insert time.

##### Context: `tests` (`#[cfg(test)]`)

- **`fold_strips_accents_and_lowercases`** (private, L38)
  - Signature: `fn fold_strips_accents_and_lowercases()`
  - Purpose: Regression test for accent folding.
  - Behavior: Asserts Portuguese and accented Latin samples fold to ASCII lowercase.

- **`search_pattern_is_partial`** (private, L44)
  - Signature: `fn search_pattern_is_partial()`
  - Purpose: Regression test for pattern trimming and emptiness.
  - Behavior: Whitespace-padded `"Git"` → `Some("%git%")`; whitespace-only → `None`.

### `src/seed.rs`

#### Types / constants

- **struct `SeedMailbox`**: Seed input for one folder row — optional system path or custom name, plus unread count hint (stored only in seed metadata, not enforced as live unread totals).
- **struct `SeedAccount`**: Seed input for one account (name, email, mailbox list).
- **struct `SeedMessage`**: Seed input for one message row — links to account by email, carries list/reader fields, flags, and optional extra folder paths.

#### Functions / methods

##### Context: `module`

- **`seed_if_empty`** (pub, L44)
  - Signature: `pub fn seed_if_empty(conn: &Connection, accounts: &[SeedAccount], messages: &[SeedMessage]) -> rusqlite::Result<bool>`
  - Purpose: Idempotent seed entry point for first-run population.
  - Behavior: Counts accounts; returns `Ok(false)` without writing when count > 0. Otherwise calls `seed` and returns `Ok(true)`.

- **`seed`** (pub, L58)
  - Signature: `pub fn seed(conn: &Connection, accounts: &[SeedAccount], messages: &[SeedMessage]) -> rusqlite::Result<()>`
  - Purpose: Bulk-inserts accounts, folders, and messages into an empty database.
  - Behavior: Runs in one unchecked transaction. Inserts each account and its mailboxes (skips mailboxes with neither system path nor custom name). For each message, resolves account id by email (returns `InvalidParameterName` error if email unknown), builds default inbox + optional flagged + extra folders CSV, computes `search_text`, inserts full message row. Commits on success; rolls back on error.

- **`preview_from_plain`** (pub, L143)
  - Signature: `pub fn preview_from_plain(plain: &str, max_chars: usize) -> String`
  - Purpose: Derives a single-line list preview from plain body text.
  - Behavior: Takes first non-empty line (or whole text if all blank). Trims; returns full line when char count ≤ `max_chars`, otherwise truncates at char boundary and appends `…`.

- **`plain_text_from_raw`** (pub, L162)
  - Signature: `pub fn plain_text_from_raw(raw: &str, raw_format: &str) -> String`
  - Purpose: Extracts searchable plain text from stored raw content.
  - Behavior: When `raw_format == "text"`, returns `raw` unchanged. For HTML, strips tags with a simple state machine (drops chars inside `<…>`), then collapses whitespace to single spaces. Not a full HTML parser.

##### Context: `tests` (`#[cfg(test)]`)

- **`sample_seed`** (private, L187)
  - Signature: `fn sample_seed() -> (Vec<SeedAccount>, Vec<SeedMessage>)`
  - Purpose: Builds minimal bilingual/accented seed data for tests.
  - Behavior: One account with inbox+drafts; two messages (Portuguese HTML body, starred text message). No I/O.

- **`seed_if_empty_populates_database`** (private, L242)
  - Signature: `fn seed_if_empty_populates_database()`
  - Purpose: Integration test for idempotent seeding.
  - Behavior: First `seed_if_empty` returns true and creates 1 account + 2 inbox messages; second call returns false without duplicating.

- **`search_finds_accent_insensitive_match`** (private, L260)
  - Signature: `fn search_finds_accent_insensitive_match()`
  - Purpose: End-to-end test linking seed search blobs to list search.
  - Behavior: Seeds sample data, searches `"portugues"`, expects one hit whose folded subject contains `"ola"`.

### `src/types.rs`

#### Types / constants

- **struct `Account`**: Connected mail account row (`id`, `name`, `email`).
- **struct `Folder`**: Per-account folder row (`id`, `account_id`, storage `path`, UI `display_name` — empty for system folders localized elsewhere).
- **struct `MessageListItem`**: Middle-column list row without body fields (`preview` excerpt, flags, metadata).
- **struct `MessageDetail`**: Reader-pane row with `plain_text`, `raw_content`, `raw_format`, and live `folders_csv`.
- **enum `MailListQuery`**: List dispatcher — `GlobalSystemFolder(String)` (all accounts), `AccountFolder { account_id, folder_path }`, or `Search(String)`.

#### Functions / methods

_(No functions.)_
