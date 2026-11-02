# crates/storage/src/ — Source API

## Purpose

Source for the `storage` crate: SQLite schema + migrations, queries, folder conventions, message mutations, server-location/membership bookkeeping, sync-facing writes, queues, search helpers, and seed import.

## Ownership

- Owns: All modules under `crates/storage/src/`.
- Does not own: crate-level dependency/feature policy (parent `.dox/crates/storage/AGENTS.md`).

## Local Contracts

- Every function/method in this tree is listed under **API Catalog** with purpose
  and behavior. Update the matching entry when changing signatures or semantics.
- Prefer rustdoc on public items; DOX purpose/behavior should stay aligned with
  rustdoc.
- Visibility in entries (`pub` / `private`) reflects the source item.
- No networking: sync-facing functions only persist what a caller fetched.
- Multi-statement writes go through `Database::write_transaction` (`BEGIN IMMEDIATE`); helpers that must run inside a caller's transaction take `&Connection` (`*_in` suffix / `membership.rs`).
- Unit tests share fixtures from `test_support.rs` (`remote_db`, `remote_message`).

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

Signatures are authoritative; `Lnn` anchors (where present) are approximate.

### `src/lib.rs`

- Module declarations (`test_support` only under `cfg(test)`) and re-exports: `Database`, `database_path`, `global_folder_path`, folder helpers, `SCHEMA_VERSION`, search helpers, seed API, `BODY_STATE_FULL` / `BODY_STATE_HEADERS`, and every type in `types.rs` (incl. `RemoteInsert`, `OutgoingState`) plus `PROVIDER_LOCAL` / `PROVIDER_IMAP` / `PROVIDER_GMAIL`. No functions.

### `src/types.rs`

#### Types / constants

- **const `PROVIDER_LOCAL` / `PROVIDER_IMAP` / `PROVIDER_GMAIL`**: `"local"`, `"imap"`, `"gmail"` — values of `accounts.provider`.
- **struct `Account`**: `id`, `name`, `email`, `provider`, `config_json` (opaque caller JSON, `{}` default).
- **struct `Folder`**: `id`, `account_id`, storage `path`, `display_name` (empty for system folders), `remote_name: Option<String>` (server mailbox; `None` = local folder), `role: Option<String>` (special-use role or `None`).
- **struct `MessageListItem`**: list row without body — ids, sender, subject, `preview`, `received_at` display string, `date_ts` (unix seconds, 0 for mock), `thread_id`, `unread`/`starred`/`has_attachment`, `folders_csv`.
- **struct `MessageDetail`**: reader row — list fields plus `plain_text`, `raw_content`, `raw_format` (`html`/`text`), `to_addrs`/`cc_addrs`/`reply_to` (opaque display lists), `message_id_hdr`, `in_reply_to`, `references_hdr` (space-separated), `thread_id`, `body_state` (0 headers only, 1 body present), `answered`, live `folders_csv`.
- **enum `MailListQuery`**: `GlobalSystemFolder(path)` (all accounts), `AccountFolder { account_id, folder_path }`, `Search(text)`.
- **struct `FolderSyncState`** (`Default`, `Copy`): `uid_validity: Option<u32>`, `uid_next: Option<u32>`, `highest_modseq: Option<u64>`, `synced_at: Option<i64>`.
- **struct `MessageLocation`**: one server location — `folder_id`, folder `remote_name`, folder `path`, `uid`.
- **struct `NewRemoteMessage`** (`Default`): headers + flags discovered on the server: sender/email/subject/preview/plain_text, `to_addrs`, `cc_addrs`, `reply_to`, `message_id_hdr`, `in_reply_to`, `references_hdr`, `thread_id`, `date_ts`, `received_at`, `unread`, `starred`, `answered`, `has_attachment`, `size`, `gm_msgid`.
- **struct `RemoteInsert`** (`Copy`): `message_id`, `created` (new row vs. known location / merge) — result of `record_remote_message`.
- **struct `NewAttachment`** (`Default`): `part_index`, `filename`, `mime_type`, `size`, `content_id`, `inline`.
- **struct `AttachmentRow`**: stored attachment metadata (`id`, `message_id` + `NewAttachment` fields).
- **struct `PendingOp`**: queued server action — `id`, `account_id`, `kind`, `message_id`, `payload` (opaque JSON), `attempts`, `last_error`, `created_at` (unix s).
- **enum `OutgoingState`** (`Copy`, `Hash`): `Queued`, `Sending` (claimed, leased), `MaybeSent` (connection lost after the data; never auto-resent), `Failed` (too many permanent rejections), `AppendPending` (sent; Sent copy still due).
- **`OutgoingState::is_stopped`** (pub) — `pub fn is_stopped(self) -> bool`: `MaybeSent` / `Failed`: no automatic retry is coming.
  - **`OutgoingState::as_str`** (pub) — `"queued"` / `"sending"` / `"maybe_sent"` / `"failed"` / `"append_pending"` (`outbox.state`).
  - **`OutgoingState::parse`** (pub) — inverse of `as_str`; unknown values → `Queued` (fail open: never silently stuck).
- **struct `OutgoingRow`**: outbox row — `id`, `account_id`, `sender`, `recipients: Vec<String>`, `raw` bytes, `message_id_hdr`, `attempts` (permanent rejections only), `last_error`, `created_at`, `state: OutgoingState`, `next_attempt_at` (unix s; 0 = due), `deferrals` (transient failures, drives back-off).

_(No functions.)_

### `src/schema.rs`

- **const `SCHEMA_VERSION`**: `3`.
- **const `CREATE_ACCOUNTS` / `CREATE_FOLDERS` / `CREATE_MESSAGES` / `CREATE_META`**: v1 base DDL (`IF NOT EXISTS`), the starting point of every database.
- **const `INDEX_MESSAGES_SEARCH` / `INDEX_MESSAGES_ACCOUNT` / `INDEX_MESSAGES_FOLDERS`**: v1 indexes.
- **const `V2_COLUMNS`**: `(table, column, definition)` list added in place by the v1→v2 step (accounts: provider/config_json/sort_order; folders: remote_name/role/uid_validity/uid_next/highest_modseq/synced_at; messages: message_id_hdr/in_reply_to/references_hdr/thread_id/to_addrs/cc_addrs/reply_to/date_ts/answered/body_state/gm_msgid/size).
- **const `V2_OBJECTS`**: idempotent DDL for `message_locations` (+ index on message_id), `message_bodies`, `attachments` (+ index), `pending_ops` (+ index), `outbox`, message indexes `(account_id, message_id_hdr)`, `(account_id, gm_msgid)`, `(thread_id)`, `(date_ts)`, the `messages_fts` FTS5 trigram external-content table and its insert/delete/update-of-search_text triggers.
- **const `V3_COLUMNS`**: outbox `state TEXT NOT NULL DEFAULT 'queued'`, `next_attempt_at INTEGER NOT NULL DEFAULT 0`, `lease_at INTEGER`, `deferrals INTEGER NOT NULL DEFAULT 0`.
- **const `V3_OBJECTS`**: index `idx_outbox_account_state (account_id, state)`.

_(No functions.)_

### `src/migration.rs`

- **`migrate`** (pub(crate)) — `fn migrate(conn: &Connection) -> rusqlite::Result<()>`
  - Purpose: bring any database to `SCHEMA_VERSION`.
  - Behavior: one `IMMEDIATE` transaction: runs v1 base DDL, reads `schema_version` (missing → 1), runs `migrate_v1_to_v2` when < 2 and `migrate_v2_to_v3` when < 3, upserts `meta.schema_version` when it changed or was missing, commits. Newer versions are left untouched.
- **`schema_version`** (pub(crate)) — `fn schema_version(conn: &Connection) -> rusqlite::Result<Option<i64>>`
  - Reads and parses `meta.schema_version`; `None` when absent or not an integer.
- **`migrate_v1_to_v2`** (private) — `fn migrate_v1_to_v2(conn: &Connection) -> rusqlite::Result<()>`
  - Adds every `V2_COLUMNS` entry via `add_column_if_missing`, executes `V2_OBJECTS`, then `INSERT INTO messages_fts(messages_fts) VALUES ('rebuild')` so pre-existing rows are searchable.
- **`migrate_v2_to_v3`** (private) — `fn migrate_v2_to_v3(conn: &Connection) -> rusqlite::Result<()>`
  - Adds `V3_COLUMNS` (existing outbox rows become `queued`, due now, attempts kept) and runs `V3_OBJECTS`.
- **`add_column_if_missing`** (private) — `fn add_column_if_missing(conn, table, column, definition) -> rusqlite::Result<()>`
  - `ALTER TABLE {table} ADD COLUMN {column} {definition}` unless `column_exists`.
- **`column_exists`** (pub(crate)) — `fn column_exists(conn, table, column) -> rusqlite::Result<bool>`
  - Scans `PRAGMA table_info(table)` names.
- Tests: `v1_database` (fixture: v1 DDL + one account/folder/message, version 1), `v1_database_migrates_in_place_preserving_data` (provider `local`, `config_json {}`, folder role/remote `None`, message fields kept, `body_state 1`, `date_ts 0`, FTS finds old row), `v2_outbox_rows_migrate_to_queued_v3_rows` (v2 fixture built by `migrate_v1_to_v2` + version 2 + an outbox row), `migration_is_idempotent_across_reopens`, `fresh_database_starts_at_current_version`, `add_column_if_missing_tolerates_existing_columns`, `schema_version_is_none_without_row`.

### `src/database.rs`

#### Types / constants

- **struct `Database`**: owns one `rusqlite::Connection`; every query/mutation hangs off it.
- **const `BUSY_TIMEOUT`**: 5000 ms.
- **const `LIST_COLUMNS` / `LIST_ORDER`**: list-row column set and `ORDER BY date_ts DESC, sort_order ASC, id DESC`.
- **const `FTS_MIN_CHARS`**: 3 — shorter folded queries use `LIKE`.

#### Functions / methods

- **`database_path`** (pub) — `pub fn database_path() -> PathBuf`: `config_dir()/mail.db` (typically `~/.config/BGMail/mail.db`). No I/O.
- **`config_dir`** (private) — `fn config_dir() -> PathBuf`: `$HOME` or `$USERPROFILE` + `.config/BGMail`; relative `.config/BGMail` fallback. No directory creation.
- **`global_folder_path`** (pub) — `pub fn global_folder_path(name: &str) -> Option<&'static str>`: `inbox`/`flagged`/`drafts`/`sent` → `sys:` path; else `None`.
- **`Database::open`** (pub) — `pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self>`: creates parent dirs (errors ignored), opens, sets busy timeout, `PRAGMA journal_mode = WAL`, `foreign_keys = ON`, runs `migrate()`.
- **`Database::open_default`** (pub) — `pub fn open_default() -> rusqlite::Result<Self>`: `open(database_path())`.
- **`Database::conn`** (pub) — `pub fn conn(&self) -> &Connection`: raw connection (seeding, low-level use).
- **`Database::atomically`** (pub) — `pub fn atomically<T>(&self, body: impl FnOnce() -> rusqlite::Result<T>) -> rusqlite::Result<T>`: runs `body` in one write transaction, committed on `Ok`, rolled back on `Err`; for read-then-write callers outside this crate. `body` must not open another transaction.
- **`Database::write_transaction`** (pub(crate)) — `fn write_transaction(&self) -> rusqlite::Result<Transaction<'_>>`: `BEGIN IMMEDIATE` transaction so concurrent writers wait on the busy timeout instead of failing on lock upgrade.
- **`Database::migrate`** (private) — `fn migrate(&self) -> rusqlite::Result<()>`: `migration::migrate`, then `repair_starred_folder_membership`.
- **`Database::repair_starred_folder_membership`** (private): appends `sys:flagged` to `folders_csv` of starred rows missing it (sorted/deduped). No-op when nothing to fix.
- **`Database::unread_in_folder`** (pub) — `pub fn unread_in_folder(&self, account_id: Option<i64>, folder_path: &str) -> rusqlite::Result<usize>`: unread rows whose `folders_csv` contains the path, optionally per account.
- **`Database::global_unread`** (pub) — `pub fn global_unread(&self, folder_path: &str) -> rusqlite::Result<usize>`: `unread_in_folder(None, path)`.
- **`Database::unread_count_total`** (pub) — `pub fn unread_count_total(&self) -> rusqlite::Result<usize>`: all unread messages, every account and folder.
- **`Database::list_messages`** (pub) — `pub fn list_messages(&self, query: &MailListQuery) -> rusqlite::Result<Vec<MessageListItem>>`: `SELECT LIST_COLUMNS … WHERE query_filter … LIST_ORDER`; blank search → empty vec.
- **`Database::get_message`** (pub) — `pub fn get_message(&self, id: i64) -> rusqlite::Result<Option<MessageDetail>>`: full reader row incl. v2 header fields; `None` when absent.
- **`Database::message_count_for_query`** (pub) — `pub fn message_count_for_query(&self, query: &MailListQuery) -> rusqlite::Result<usize>`: `SELECT COUNT(*)` with the same filter as `list_messages` (no row loading); blank search → 0.
- **`Database::message_count_all`** (pub) — `pub fn message_count_all(&self) -> rusqlite::Result<usize>`: `COUNT(*)` of messages.
- **`query_filter`** (private) — `fn query_filter(query: &MailListQuery) -> Option<(&'static str, Vec<Value>)>`: WHERE clause + params per query kind (`folders_csv LIKE`, optional `account_id`, or `search_filter`).
- **`search_filter`** (private) — `fn search_filter(query: &str) -> Option<(&'static str, Vec<Value>)>`: folds trimmed query; empty → `None`; ≥3 chars → `id IN (SELECT rowid FROM messages_fts WHERE messages_fts MATCH fts_phrase)`; else `search_text LIKE %folded%`.
- **`fts_phrase`** (private) — `fn fts_phrase(text: &str) -> String`: wraps in `"…"` doubling inner quotes so input never parses as FTS syntax.
- **`list_item_from_row`** (private): maps a `LIST_COLUMNS` row to `MessageListItem`.
- Tests: `database_path_uses_dot_config_bgmail`, `account_folder_lists_only_that_account`, helpers `search_ids` (also asserts count == list length) / `like_ids` (v1 LIKE oracle), `fts_search_matches_like_semantics` (accents, case, substrings, <3 chars, quotes, FTS keywords), `message_count_for_query_counts_without_loading`, `unread_count_total_spans_accounts`, `open_enables_wal`, `two_connections_read_and_write_concurrently` (sync thread inserts while the UI connection reads and writes).

### `src/accounts.rs`

- **`account_from_row`** / **`folder_from_row`** (private): map `ACCOUNT_COLUMNS` / `FOLDER_COLUMNS` rows.
- **`Database::list_accounts`** (pub) — `pub fn list_accounts(&self) -> rusqlite::Result<Vec<Account>>`: all accounts by `sort_order, id`.
- **`Database::get_account`** (pub) — `pub fn get_account(&self, id: i64) -> rusqlite::Result<Option<Account>>`.
- **`Database::create_account`** (pub) — `pub fn create_account(&self, name: &str, email: &str, provider: &str, config_json: &str) -> rusqlite::Result<i64>`: inserts with `sort_order = max + 1`; errors on duplicate email. Creates no folders.
- **`Database::update_account`** (pub) — `pub fn update_account(&self, id: i64, name: &str, config_json: &str) -> rusqlite::Result<()>`: updates name + config; no-op for unknown id.
- **`Database::delete_account`** (pub) — `pub fn delete_account(&self, id: i64) -> rusqlite::Result<()>`: one write tx: deletes the account's messages (FTS triggers, cascades to locations/bodies/attachments), then the account (cascades folders, pending_ops, outbox).
- **`Database::list_folders`** (pub) — `pub fn list_folders(&self, account_id: i64) -> rusqlite::Result<Vec<Folder>>`: by id.
- **`Database::folder_by_id`** (pub) — `pub fn folder_by_id(&self, folder_id: i64) -> rusqlite::Result<Option<Folder>>`.
- **`Database::folder_by_remote_name`** (pub) — `pub fn folder_by_remote_name(&self, account_id: i64, remote_name: &str) -> rusqlite::Result<Option<Folder>>`: lowest id match.
- **`Database::folder_for_role`** (pub) — `pub fn folder_for_role(&self, account_id: i64, role: &str) -> rusqlite::Result<Option<Folder>>`: lowest id with that role.
- **`Database::upsert_remote_folder`** (pub) — `pub fn upsert_remote_folder(&self, account_id: i64, path: &str, display_name: &str, remote_name: &str, role: Option<&str>) -> rusqlite::Result<i64>`: insert or update display/remote/role keyed by `(account_id, path)`; keeps sync checkpoints; returns id (`RETURNING`).
- **`Database::delete_folder`** (pub) — `pub fn delete_folder(&self, folder_id: i64) -> rusqlite::Result<()>`: first `clear_folder_locations` (batched commits), then a short write tx: collects any messages still located there, deletes the folder (cascade locations), `settle_after_location_loss`. Location-less messages keep their csv.
- **`Database::folder_sync_state`** (pub) — `pub fn folder_sync_state(&self, folder_id: i64) -> rusqlite::Result<FolderSyncState>`: stored checkpoint; all `None` for unknown/never-synced folders. Modseq is bit-cast from i64.
- **`Database::set_folder_sync_state`** (pub) — `pub fn set_folder_sync_state(&self, folder_id: i64, state: &FolderSyncState) -> rusqlite::Result<()>`: overwrites all four columns.
- Tests: `create_get_update_account`, `delete_account_cascades_everything` (all child tables empty, FTS integrity-check passes, other account kept), `upsert_remote_folder_is_keyed_by_path`, `folder_lookups`, `folder_sync_state_round_trips` (u32::MAX / 63-bit modseq), `delete_folder_drops_remote_orphans_and_refreshes_others`.

### `src/membership.rs`

- **`refresh_membership_in`** (pub(crate)) — `fn refresh_membership_in(conn: &Connection, message_id: i64) -> rusqlite::Result<()>`: when the message has locations, sets `folders_csv` = sorted/deduped location folder paths + `sys:flagged` if starred; no-op without locations or for unknown ids.
- **`settle_after_location_loss`** (pub(crate)) — `fn settle_after_location_loss(conn: &Connection, message_ids: &[i64]) -> rusqlite::Result<()>`: per id, deletes the message if it has no location and its account's provider is not `local`; otherwise `refresh_membership_in`.
- **`message_ids_in_folder`** (pub(crate)) — `fn message_ids_in_folder(conn: &Connection, folder_id: i64) -> rusqlite::Result<Vec<i64>>`: distinct message ids located in the folder.
- **const `LOCATION_BATCH`** (pub(crate)): 500 messages per write transaction when a whole folder loses its locations.
- **`message_ids_in_folder_limit`** (pub(crate)) — `fn message_ids_in_folder_limit(conn: &Connection, folder_id: i64, limit: usize) -> rusqlite::Result<Vec<i64>>`: up to `limit` distinct message ids located in the folder.
- **`Database::refresh_membership`** (pub) — `pub fn refresh_membership(&self, message_id: i64) -> rusqlite::Result<()>`: public wrapper of `refresh_membership_in`.
- Tests: `membership_follows_locations_and_starred`, `membership_without_locations_is_left_alone`, `refresh_membership_of_missing_message_is_noop`.

### `src/sync.rs`

#### Types / constants

- **const `BODY_STATE_HEADERS`** (`0`) / **`BODY_STATE_FULL`** (`1`): `messages.body_state` values.

#### Functions / methods

- **`find_merge_target`** (private): same `gm_msgid` in the account (lowest id) → else same `message_id_hdr` in the account among messages with **no** locations → else `None`.
- **`insert_message_row`** (private): inserts a headers-only row (`raw_content ''`, `raw_format 'text'`, `body_state 0`, csv `,,`, `search_text` from sender/email/subject/plain text).
- **`location_message_id`** (private): message id at `(folder_id, uid)`.
- **`message_locations_in`** (private): locations of a message joined with folder path/remote name, by location id.
- **`Database::insert_remote_message`** (pub) — `pub fn insert_remote_message(&self, account_id: i64, folder_id: i64, uid: u32, message: &NewRemoteMessage) -> rusqlite::Result<i64>`: `record_remote_message(..).message_id`.
- **`Database::record_remote_message`** (pub) — `pub fn record_remote_message(&self, account_id: i64, folder_id: i64, uid: u32, message: &NewRemoteMessage) -> rusqlite::Result<RemoteInsert>`: write tx; existing `(folder, uid)` → its id, `created: false` (nothing written); else merge target (`created: false`) or new row (`created: true`), insert location, refresh membership. Merges do not update fields/flags.
- **`Database::folder_uids`** (pub) — `pub fn folder_uids(&self, folder_id: i64) -> rusqlite::Result<Vec<(u32, i64)>>`: `(uid, message_id)` ascending.
- **`Database::max_uid`** (pub) — `pub fn max_uid(&self, folder_id: i64) -> rusqlite::Result<Option<u32>>`.
- **`Database::remove_locations`** (pub) — `pub fn remove_locations(&self, folder_id: i64, uids: &[u32]) -> rusqlite::Result<()>`: write tx; deletes known locations, then `settle_after_location_loss` on affected messages. Unknown uids ignored.
- **`Database::clear_folder_locations`** (pub) — `pub fn clear_folder_locations(&self, folder_id: i64) -> rusqlite::Result<()>`: same orphan rules as above for every location of the folder (UIDVALIDITY reset), committed every `LOCATION_BATCH` messages (loop: write tx, next batch of ids, delete their locations in the folder, settle, commit) so the UI connection never waits on one huge transaction.
- **`Database::apply_remote_flags`** (pub) — `pub fn apply_remote_flags(&self, folder_id: i64, uid: u32, unread: bool, starred: bool, answered: bool) -> rusqlite::Result<bool>`: updates flags only if different, refreshes membership when changed; `false` for unknown location or no change.
- **`Database::store_body`** (pub) — `pub fn store_body(&self, message_id: i64, raw: &[u8], content: &str, format: &str, plain_text: &str, preview: &str, attachments: &[NewAttachment]) -> rusqlite::Result<()>`: write tx; upserts `message_bodies.raw`, sets raw_content/raw_format (`html`|`text`, CHECK-enforced)/plain_text/preview/search_text (recomputed)/`has_attachment` (any non-inline)/`body_state 1`, replaces attachments. No-op for a deleted message.
- Starred (`system::FLAGGED`) leaves out mail in the Trash, in lists and unread counts (`hidden_from_folder`); it keeps its star for when it is restored.
- **`Account::from_name`** (pub) / **`Database::set_account_names`** (pub): the name in the "From" (`sender_name`, schema v5, or the account's name when blank), and setting both.
- **`contacts`**: `Contact`; `Database::record_contacts`, `list_contacts`, `contact_sources`, `meta_flag`, `set_meta_flag` (pub).
- **`Database::messages_with_markup_preview`** (pub) — downloaded messages of an account whose preview starts with `<` or holds `&…;`.
- **`Database::update_body_text`** (pub) — replaces plain text and preview, keeping search text in step; no-op for a missing message.
- **`Database::message_raw`** (pub) — `pub fn message_raw(&self, message_id: i64) -> rusqlite::Result<Option<Vec<u8>>>`.
- **`Database::list_attachments`** (pub) — `pub fn list_attachments(&self, message_id: i64) -> rusqlite::Result<Vec<AttachmentRow>>`: non-inline first, then by `part_index`, id.
- **`Database::message_locations`** (pub) — `pub fn message_locations(&self, message_id: i64) -> rusqlite::Result<Vec<MessageLocation>>`.
- **`Database::messages_without_body`** (pub) — `pub fn messages_without_body(&self, account_id: i64, limit: usize) -> rusqlite::Result<Vec<i64>>`: `body_state 0` messages **with at least one location** (fetchable), newest first (`date_ts DESC, id DESC`).
- **`Database::detach_locations`** (pub) — `pub fn detach_locations(&self, message_id: i64) -> rusqlite::Result<Vec<MessageLocation>>`: write tx; returns then deletes all locations; csv untouched (message is now in flight).
- **`Database::attach_location`** (pub) — `pub fn attach_location(&self, message_id: i64, folder_id: i64, uid: u32) -> rusqlite::Result<bool>`: write tx; inserts the location and refreshes membership; `false` (nothing written) when the message is gone or `(folder, uid)` already belongs to a message.
- **`Database::detach_location`** (pub) — `pub fn detach_location(&self, message_id: i64, folder_id: i64) -> rusqlite::Result<Option<MessageLocation>>`: write tx; removes the message's first location in that folder and returns it; membership refreshed (no-op when no location remains → in flight, csv kept); `None` when it had none there.
- **`Database::purge_orphans`** (pub) — `pub fn purge_orphans(&self, account_id: i64) -> rusqlite::Result<usize>`: write tx; deletes the account's messages with no location and no `pending_ops` row referencing them, only for non-`local` providers; returns the count.
- **`Database::folder_messages_without_body`** (pub) — `pub fn folder_messages_without_body(&self, folder_id: i64, limit: usize, max_size: u64) -> rusqlite::Result<Vec<(i64, u32)>>`: among the folder's `limit` newest located messages (`date_ts DESC, uid DESC`), the `body_state 0` ones with `size <= max_size`, as `(message_id, uid)` newest first (body prefetch).
- Tests: `insert_creates_headers_only_message_with_location`, `insert_same_location_twice_returns_existing`, `gm_msgid_merges_locations` (no cross-account merge), `message_id_merges_only_in_flight_messages`, `detach_keeps_csv_and_returns_nothing_twice`, `folder_uids_and_max_uid`, `remove_locations_deletes_remote_orphans_only`, `clear_folder_locations_uses_orphan_rules`, `clear_folder_locations_spans_several_batches` (also `delete_folder`), `apply_remote_flags_updates_and_reports_change`, `store_body_fills_message_and_attachments`, `store_body_for_missing_message_is_noop`, `messages_without_body_newest_first_with_locations`, `record_remote_message_reports_creation`, `attach_location_adds_and_refreshes_membership`, `detach_location_removes_one_folder_only`, `purge_orphans_spares_located_queued_and_local_mail`, `folder_messages_without_body_limits_to_newest_and_size`, `list_order_is_date_then_sort_order`; helper `count`.

### `src/queue.rs`

- **`unix_now`** (pub(crate)) — `fn unix_now() -> i64`: current unix seconds (0 before epoch).
- **const `OP_COLUMNS`** / **`op_from_row`** (private) — `pending_ops` column list and row → `PendingOp`.
- **`Database::enqueue_op`** (pub) — `pub fn enqueue_op(&self, account_id: i64, kind: &str, message_id: Option<i64>, payload: &str) -> rusqlite::Result<i64>`: inserts with `created_at = now`; returns id.
- **`Database::pending_ops`** (pub) — `pub fn pending_ops(&self, account_id: i64) -> rusqlite::Result<Vec<PendingOp>>`: oldest first (by id).
- **`Database::pending_op`** (pub) — `pub fn pending_op(&self, id: i64) -> rusqlite::Result<Option<PendingOp>>`: the op as it is now; `None` once removed.
- **`Database::complete_op`** (pub) — `pub fn complete_op(&self, id: i64) -> rusqlite::Result<()>`: deletes the op unconditionally.
- **`Database::complete_op_if_unchanged`** (pub) — `pub fn complete_op_if_unchanged(&self, id: i64, kind: &str, payload: &str) -> rusqlite::Result<bool>`: `DELETE … WHERE id AND kind AND payload`; `false` when the row was rewritten or removed meanwhile (the caller must not apply the executed op's local result).
- **`Database::fail_op`** (pub) — `pub fn fail_op(&self, id: i64, error: &str) -> rusqlite::Result<()>`: `attempts + 1`, `last_error = error` (kind/payload untouched, so CAS keys stay valid).
- **`Database::update_op`** (pub) — `pub fn update_op(&self, id: i64, kind: &str, payload: &str) -> rusqlite::Result<()>`: rewrites kind + payload in place (id, attempts, order kept).
- **`Database::update_op_if_unchanged`** (pub) — `pub fn update_op_if_unchanged(&self, id: i64, old_kind: &str, old_payload: &str, kind: &str, payload: &str) -> rusqlite::Result<bool>`: compare-and-swap rewrite; `false` when another connection changed/removed the row first.
- **`Database::drop_ops_for_message`** (pub) — `pub fn drop_ops_for_message(&self, message_id: i64) -> rusqlite::Result<()>`: deletes ops targeting the message.
- **`Database::enqueue_outgoing`** (pub) — `pub fn enqueue_outgoing(&self, account_id: i64, sender: &str, recipients: &[String], raw: &[u8], message_id_hdr: Option<&str>) -> rusqlite::Result<i64>`: recipients stored newline-separated; state `queued`, due now.
- **`Database::outbox`** (pub) — `pub fn outbox(&self, account_id: i64) -> rusqlite::Result<Vec<OutgoingRow>>`: every row of the account (any state), oldest first; recipients split on lines (empty lines dropped).
- **`Database::due_outgoing`** (pub) — `pub fn due_outgoing(&self, account_id: i64, now: i64) -> rusqlite::Result<Vec<OutgoingRow>>`: `queued` / `append_pending` rows with `next_attempt_at <= now`, oldest first.
- **`Database::unsent_outgoing`** (pub) — `pub fn unsent_outgoing(&self) -> rusqlite::Result<Vec<OutgoingRow>>`: `failed`, `maybe_sent` and `queued` after a failure or left for 5 minutes (`STUCK_IN_QUEUE`), oldest first, `raw` empty.
- **`Database::give_up_outgoing`** (pub) — `pub fn give_up_outgoing(&self, account_id: i64, now: i64, max_age_secs: i64) -> rusqlite::Result<Vec<(i64, Option<String>)>>`: stops `queued` rows older than `max_age_secs` from `MAX(created_at, retry_from)`, skipping rows with no known age; returns their ids and errors.
- **`Database::stop_outgoing`** (pub) — `pub fn stop_outgoing(&self, id: i64) -> rusqlite::Result<bool>`: `queued` → `failed`; `false` in any other state.
- **`Database::outgoing`** (pub) — `pub fn outgoing(&self, id: i64) -> rusqlite::Result<Option<OutgoingRow>>`: one row with its raw bytes.
- **`Database::next_outgoing_due`** (pub) — `pub fn next_outgoing_due(&self, account_id: i64, now: i64) -> rusqlite::Result<Option<i64>>`: earliest `next_attempt_at` after `now` among `queued` / `append_pending` rows.
- **`Database::replace_outgoing`** (pub) — `pub fn replace_outgoing(&self, old_id: i64, account_id: i64, sender: &str, recipients: &[String], raw: &[u8], message_id_hdr: Option<&str>) -> rusqlite::Result<Option<i64>>`: one transaction: deletes `old_id` and queues `raw`. `None` (nothing changed) when the old row exists and isn't stopped; a missing old row doesn't block.
- **`Database::query_outgoing`** (private) — shared `SELECT` with a `WHERE` fragment + params.
- **`Database::outbox_count`** (pub) — `pub fn outbox_count(&self) -> rusqlite::Result<usize>`: `queued` + `sending` rows of all accounts (still waiting to be sent).
- **`Database::claim_outgoing`** (pub) — `pub fn claim_outgoing(&self, id: i64, now: i64) -> rusqlite::Result<bool>`: atomic `queued` → `sending` with `lease_at = now`; `false` when not `queued` (claimed elsewhere, cancelled, gone).
- **`Database::expire_outgoing_leases`** (pub) — `pub fn expire_outgoing_leases(&self, account_id: i64, cutoff: i64) -> rusqlite::Result<Vec<i64>>`: the account's `sending` rows leased before `cutoff` → `maybe_sent` (`last_error` defaults to "interrupted while sending"); ids of the changed rows.
- **`Database::complete_outgoing`** (pub) — `pub fn complete_outgoing(&self, id: i64) -> rusqlite::Result<()>`: deletes the row (sent and filed).
- **`Database::mark_outgoing_append_pending`** (pub) — `pub fn mark_outgoing_append_pending(&self, id: i64) -> rusqlite::Result<()>`: accepted by SMTP; keeps the raw for the Sent APPEND (`append_pending`, due now, deferrals/error/lease cleared).
- **`Database::defer_outgoing`** (pub) — `pub fn defer_outgoing(&self, id: i64, error: &str, next_attempt_at: i64) -> rusqlite::Result<()>`: transient failure: `deferrals + 1`, `last_error`, `next_attempt_at`, lease cleared; `sending` → `queued`, `append_pending` kept; attempts untouched.
- **`Database::fail_outgoing`** (pub) — `pub fn fail_outgoing(&self, id: i64, error: &str, next_attempt_at: i64, max_attempts: u32) -> rusqlite::Result<OutgoingState>`: permanent rejection: `attempts + 1`, `last_error`, `next_attempt_at`, lease cleared; `failed` once attempts reach `max_attempts`, else `queued`; returns the new state (`Failed` for a missing row).
- **`Database::mark_outgoing_maybe_sent`** (pub) — `pub fn mark_outgoing_maybe_sent(&self, id: i64, error: &str) -> rusqlite::Result<()>`: `maybe_sent` + `last_error`, lease cleared.
- **`Database::retry_outgoing`** (pub) — `pub fn retry_outgoing(&self, id: i64) -> rusqlite::Result<bool>`: user retry of a stopped row: `queued`, attempts/deferrals/`next_attempt_at` 0, lease and error cleared. `false` (untouched) for any other state. Also accepts a row stuck in the queue; records `retry_from`, from which its age counts.
- **`Database::delete_outgoing`** (pub) — `pub fn delete_outgoing(&self, id: i64) -> rusqlite::Result<bool>`: deletes the row unless it is `sending` or `append_pending` (`false`).
- Tests: `unix_now_is_after_2020`, `pending_ops_lifecycle` (incl. `update_op`, `pending_op`), `outbox_lifecycle`, `complete_op_if_unchanged_detects_rewrites_and_removals`, `update_op_if_unchanged_is_a_compare_and_swap`, `due_outgoing_respects_backoff_and_state`, `claim_outgoing_is_exclusive_and_leases_expire_to_maybe_sent`, `fail_outgoing_reaches_failed_after_max_attempts`, `unsent_outgoing_retry_and_delete`, `a_retrying_row_is_listed_and_can_be_stopped_or_discarded`, `a_row_being_sent_cannot_be_stopped_or_discarded`, `replace_outgoing_only_replaces_stopped_or_missing_rows`, `retry_outgoing_only_touches_stopped_rows`, `stopped_listing_leaves_raw_out_and_outgoing_brings_it`, `next_outgoing_due_is_the_earliest_future_retry`, `atomically_rolls_back_on_error`.

### `src/message_ops.rs`

#### Functions / methods (module)

- **`parse_folders_csv`** (pub) — `pub fn parse_folders_csv(csv: &str) -> Vec<String>`: split on commas, drop empties.
- **`Database::swap_message_folder`** (pub) — `pub fn swap_message_folder(&self, id: i64, left: &str, joined: &str) -> rusqlite::Result<()>`: out of `left`, into `joined`, other folders kept.
- **`message_has_folder`** (pub, re-exported) — `pub fn message_has_folder(csv: &str, folder_path: &str) -> bool`: exact match over decoded segments.
- **`is_virtual_flagged_folder`** (pub) — `pub fn is_virtual_flagged_folder(path: &str) -> bool`: `path == sys:flagged`.
- **`replace_primary_folder`** (pub) — `pub fn replace_primary_folder(paths: &[String], primary: &str) -> Vec<String>`: keeps only flagged, adds `primary`, sorted/deduped.
- **`folders_csv_from_paths`** (pub) — `pub fn folders_csv_from_paths(paths: &[String]) -> String`.

#### `Database` methods

These edit `folders_csv` directly and work for every message. For a remote message with locations, callers must `detach_locations` first or the next membership refresh restores the server folders.

- **`load_message_folders`** (private): `(paths, starred)` or `None`.
- **`set_message_folders`** (private): writes serialized csv.
- **`set_primary_folder`** (private): `replace_primary_folder` + write; no-op for unknown id.
- **`move_message_to_trash`** (pub) — `pub fn move_message_to_trash(&self, id: i64) -> rusqlite::Result<()>`: primary → `sys:trash`.
- **`restore_message_from_trash`** (pub) — `pub fn restore_message_from_trash(&self, id: i64) -> rusqlite::Result<()>`: if in trash, primary → inbox.
- **`delete_message_permanently`** (pub) — `pub fn delete_message_permanently(&self, id: i64) -> rusqlite::Result<bool>`: only when in trash; row delete cascades locations/body/attachments and updates FTS.
- **`delete_message`** (pub) — `pub fn delete_message(&self, id: i64) -> rusqlite::Result<bool>`: unconditional row delete (cascades, FTS follows); whether a row was deleted. Callers queue the server delete first.
- **`archive_message`** / **`mark_message_junk`** (pub) — `(&self, id: i64) -> rusqlite::Result<()>`: primary → archive / junk.
- **`move_message_to_folder`** (pub) — `pub fn move_message_to_folder(&self, id: i64, folder_path: &str) -> rusqlite::Result<()>`: no-op for forbidden targets.
- **`toggle_message_starred`** (pub) — `pub fn toggle_message_starred(&self, id: i64) -> rusqlite::Result<bool>`: flips `starred` + `sys:flagged` in csv, then `refresh_membership_in` (write tx); returns new state (`false` if missing).
- **`set_message_unread`** (pub) — `pub fn set_message_unread(&self, id: i64, unread: bool) -> rusqlite::Result<bool>`: whether the row changed.
- **`set_messages_unread`** (pub) — `pub fn set_messages_unread(&self, ids: &[i64], unread: bool) -> rusqlite::Result<usize>`: write tx; number of rows changed.
- **`message_is_in_trash`** (pub) — `pub fn message_is_in_trash(&self, id: i64) -> rusqlite::Result<bool>`.
- Tests: fixtures `seeded_db`, `message_csv`, `message_id`; `delete_message_works_outside_trash`, `replace_primary_folder_keeps_flagged`, `move_to_trash_keeps_flagged_membership`, `restore_from_trash_moves_to_inbox`, `permanent_delete_only_from_trash`, `archive_and_junk_replace_primary`, `toggle_starred_updates_flagged_folder`, `move_to_custom_folder`, `set_message_unread_reports_change`, `set_messages_unread_counts_changes`, `toggle_starred_on_remote_message_keeps_location_membership`, `move_to_sent_is_noop`.

### `src/test_support.rs` (`cfg(test)`)

- **struct `Fixture`**: ids of an `imap` account (inbox/archive/trash folders) and a `local` account (inbox).
- **`remote_db`** (pub(crate)) — `fn remote_db() -> (NamedTempFile, Database, Fixture)`: temp DB with both accounts and their folders (remote names `INBOX`/`Archive`/`Trash`, roles set).
- **`remote_message`** (pub(crate)) — `fn remote_message(subject: &str, gm_msgid: Option<u64>) -> NewRemoteMessage`: minimal headers with Message-ID `<{subject}@test>`.

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
  - Behavior: Joins paths with commas and wraps as `,{path1},{path2},` with a `,` inside a path written as U+001F (`encode_csv_path`/`decode_csv_path`, crate-private), so a path never spans segments. Empty iterator yields `,,`.

- **`folder_csv_needle`** (pub)
  - Signature: `pub fn folder_csv_needle(folder_path: &str) -> String`
  - Purpose: What `instr(folders_csv, ?)` looks for to test membership.
  - Behavior: Returns `,{encoded path},`. Used with `instr`, not `LIKE`: exact and case-sensitive, so `My_Work`, `MyXWork`, `work` and `WORK` stay distinct.

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

- **`folder_csv_needle_matches_inside_csv`** (private)
  - Signature: `fn folder_csv_needle_matches_inside_csv()`
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
  - Behavior: Runs in one `BEGIN IMMEDIATE` transaction (waits on the busy timeout instead of failing when another connection writes). Inserts each account and its mailboxes (skips mailboxes with neither system path nor custom name). For each message, resolves account id by email (returns `InvalidParameterName` error if email unknown), builds default inbox + optional flagged + extra folders CSV, computes `search_text`, inserts full message row (v2 columns take their defaults: `date_ts 0`, `body_state 1`, no locations). Commits on success; rolls back on error.

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
