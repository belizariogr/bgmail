# crates/storage/ — SQLite persistence

## Purpose

Local mail store: accounts (local/IMAP/Gmail), folders with IMAP sync
checkpoints, messages (headers, flags, plain preview, rendered + raw RFC 822
body, attachments metadata), server locations, multi-folder membership,
accent-insensitive search (FTS5 trigram), queued server ops, outbox, and
first-run seeding. No GPUI, no network — it stores what a sync engine gives it.

## Ownership

- Owns: SQLite schema + stepwise migrations, `Database` API, folder path helpers
  (`sys:` / `user:`), message list/detail queries, move/star/unread ops,
  location/membership bookkeeping, sync-facing writes, `pending_ops` / `outbox`
  queues, search text folding + FTS, `seed` / `seed_if_empty`.
- Does not own: UI selection/state (`bgmail`), protocol code / the sync engine
  (`mail_core`, `protocols`, `mail_sync`), credentials/keychain, the meaning of
  `accounts.config_json`, `pending_ops.kind/payload` (opaque, caller-owned).

## Local Contracts

- DB path: `~/.config/BGMail/mail.db` via `database_path` (portable home
  resolution; no GPUI).
- **Schema v2** (`schema.rs`, `SCHEMA_VERSION = 2`). `migration.rs` applies
  steps by `meta.schema_version` inside one `BEGIN IMMEDIATE` transaction:
  base v1 DDL (`CREATE … IF NOT EXISTS`), then v1→v2 = `ALTER TABLE ADD COLUMN`
  (skipped when the column exists) + new tables/indexes/FTS + FTS `rebuild`.
  Never drop/recreate tables: user mail must survive. v1 accounts become
  `provider = 'local'`; v1 messages get `date_ts = 0`, `body_state = 1`.
  New versions = new step function + bumped constant + migration test built
  from the previous DDL.
- v2 tables: `accounts` (+`provider`, `config_json`, `sort_order`), `folders`
  (+`remote_name`, `role`, `uid_validity`, `uid_next`, `highest_modseq`,
  `synced_at`), `messages` (+headers/threading, `to/cc/reply_to`, `date_ts`,
  `answered`, `body_state`, `gm_msgid`, `size`), `message_locations`
  (`UNIQUE(folder_id, uid)`), `message_bodies` (raw bytes, kept out of
  `messages` so list queries stay small), `attachments`, `pending_ops`
  (`message_id` has no FK: ops must outlive deleted messages), `outbox`. Every
  child table cascades from its account/message.
- **Connections:** `Database::open` sets `busy_timeout` 5000 ms + WAL so the UI
  and a background sync thread each hold their own connection to one file.
  Multi-statement writes use `write_transaction()` (`BEGIN IMMEDIATE`); a
  deferred read-then-write tx gets `SQLITE_BUSY` without waiting.
- **Membership rule:** `folders_csv` is the denormalized membership the UI
  queries. A message with ≥1 location belongs exactly to its location folders'
  paths (+ `sys:flagged` when starred), recomputed by `refresh_membership` from
  every function that changes locations or `starred`. A message without
  locations (local/mock mail, or remote mail in flight after a UI move) keeps
  the `folders_csv` the UI last wrote.
- **Orphans:** when locations are removed (`remove_locations`,
  `clear_folder_locations`, `delete_folder`), messages of non-`local` accounts
  left with no location are deleted; `local` accounts never lose mail this way.
  `purge_orphans(account)` (run by the sync engine after a full cycle) deletes
  remaining location-less remote messages that no `pending_ops` row
  references (a queued op means the message is in flight).
- **Merge on insert** (`insert_remote_message` / `record_remote_message`,
  which also reports `created`): existing `(folder, uid)` → same `gm_msgid` in
  the account → same Message-ID **only if the candidate has no locations** (in
  flight) → new headers-only row (`body_state = 0`). UI moves of remote mail
  detach the moved locations (`detach_location` per folder, or
  `detach_locations`) and queue the server op from the result; the sync engine
  replays `pending_ops` before re-syncing so the source folder does not
  re-attach the message, and `attach_location` records a move's COPYUID.
- List order everywhere: `date_ts DESC, sort_order ASC, id DESC`.
- Types returned to UI/sync: `Account`, `Folder`, `MessageListItem`,
  `MessageDetail`, `MailListQuery`, `FolderSyncState`, `MessageLocation`,
  `NewRemoteMessage`, `RemoteInsert`, `NewAttachment`, `AttachmentRow`, `PendingOp`,
  `OutgoingRow` — UI-agnostic plain data owned by this crate (no `mail_core`
  dependency).
- Folder paths: system (`sys:inbox`, …) vs user (`user:…`); helpers in
  `folder.rs` own path rules and forbidden move targets.
- Search: `search_text` = folded sender/email/subject/plain text
  (`search.rs`). Folded queries of ≥3 chars use `messages_fts` (FTS5,
  `trigram`, external content kept in sync by triggers) with the query as one
  quoted phrase; shorter ones use `LIKE`. Both are accent/case-insensitive
  substring matches (FTS treats `%`/`_` literally).
- Seeding: `seed_if_empty` only populates an empty DB; mock content comes from
  callers (`bgmail` seed adapters), not hardcoded UI strings here.
- Dependency rule: no `gpui`, `ui`, or `theme`.

## Work Guidance

- Prefer extending `Database` methods over ad-hoc SQL in `bgmail`.
- Every new query/mutation gets unit tests (tempfile DB).
- Networking and the sync state machine stay out of this crate; it only offers
  the storage primitives the engine calls.

## Verification

- `cargo test -p storage` (migration v1→v2, seed, FTS/LIKE search parity,
  message ops, membership, sync merges/orphans, queues, WAL two-connection).
- `cargo clippy -p storage --all-targets -- -D warnings`.
- Workspace clippy/fmt as in parent.

## Child DOX Index

| Scope | Doc | Owns |
|---|---|---|
| `crates/storage/src/` | [`.dox/crates/storage/src/AGENTS.md`](src/AGENTS.md) | Full API catalog (every fn/method) |
