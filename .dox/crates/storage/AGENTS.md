# crates/storage/ — SQLite persistence

## Purpose

Local mail store: accounts, folders, messages (plain preview + raw body),
multi-folder membership, accent-insensitive search, and first-run seeding.
No GPUI, no network.

## Ownership

- Owns: SQLite schema/migrations version, `Database` API, folder path helpers
  (`sys:` / `user:`), message list/detail queries, move/star/folder ops, search
  text folding, `seed` / `seed_if_empty`.
- Does not own: UI selection/state (`bgmail`), future sync/protocol crates,
  credentials/keychain (future `accounts`).

## Local Contracts

- DB path: `~/.config/BGMail/mail.db` via `database_path` (portable home
  resolution; no GPUI).
- Schema version in `schema.rs` (`SCHEMA_VERSION`). Bump and migrate
  deliberately; do not silently break existing DBs.
- Types returned to UI: `Account`, `Folder`, `MessageListItem`, `MessageDetail`,
  `MailListQuery` — keep these UI-agnostic plain data.
- Folder paths: system (`sys:inbox`, …) vs user (`user:…`); helpers in
  `folder.rs` own path rules and forbidden move targets.
- Messages may belong to multiple folders via `folders_csv`.
- Search: build/fold `search_text` for accent-insensitive matching; keep logic
  in `search.rs` with tests.
- Seeding: `seed_if_empty` only populates an empty DB; mock content comes from
  callers (`bgmail` seed adapters), not hardcoded UI strings here.
- Dependency rule: no `gpui`, `ui`, or `theme`.

## Work Guidance

- Prefer extending `Database` methods over ad-hoc SQL in `bgmail`.
- Every new query/mutation gets unit tests (tempfile DB).
- Networking/sync state machine stays out of this crate until Stage 2 design
  lands it elsewhere (or a dedicated module with a clear API).

## Verification

- `cargo test -p storage` (schema, seed, search, message ops, round-trips).
- Workspace clippy/fmt as in parent.

## Child DOX Index

| Scope | Doc | Owns |
|---|---|---|
| `crates/storage/src/` | [`.dox/crates/storage/src/AGENTS.md`](src/AGENTS.md) | Full API catalog (every fn/method) |
