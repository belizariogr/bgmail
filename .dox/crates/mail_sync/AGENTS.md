# crates/mail_sync/ — Synchronization engine

## Purpose

Mirrors mail servers into `storage`: per-account worker threads (blocking,
no async runtime) that connect through `mail_core::MailConnector`, replay the
UI's queued server ops, send the outbox, sync folders/headers/flags/bodies
and watch the Inbox with IMAP IDLE; plus the optimistic UI-side `actions`
that edit the database and queue those server ops.

## Ownership

- Owns: `Engine` / `EngineOptions`, `Command`, `SyncEvent` / `AccountStatus`
  / `NewMailNotice`, the pure planner (`plan`), body → storage conversion
  (`body`), optimistic actions (`actions`), the `pending_ops` payload format
  (`PendingAction` JSON, `kind` = `flags` | `move` | `delete`), worker/IDLE
  thread lifecycle, back-off and retry policy.
- Does not own: SQL / schema / membership rules (`storage`), MIME parsing,
  flags, folder roles, fakes (`mail_core`), IMAP/SMTP/OAuth (`protocols`,
  injected as `Arc<dyn MailConnector>`), UI wiring and notifications
  (`bgmail`).

## Local Contracts

- **No GPUI, zero `unsafe`** (`#![forbid(unsafe_code)]`). Dependencies:
  `mail_core`, `storage`, `rusqlite` (only to name storage's error type; same
  version), `serde` / `serde_json` (op payloads), `async-channel` 2 (events).
  Dev: `mail_core` with `fake`, `tempfile`.
- **Threads:** per account one worker thread (owns its own
  `storage::Database` on the shared WAL file and a lazily connected
  `Box<dyn MailRemote>`) and, once the first full cycle succeeded and the
  server advertises IDLE, one IDLE thread with its own connection that pokes
  the worker with `Command::SyncFolder(inbox)` on `Changed`. IDLE
  `Unsupported` → the thread exits for good (polling only).
- **Commands** travel over `std::sync::mpsc`; the worker coalesces them
  (`SyncNow` absorbs queued `SyncFolder`/`ProcessQueue`, duplicates dropped,
  `FetchBody` jumps the queue and is also served between folders/batches/ops
  of a running cycle). `Shutdown` / cancellation abort the cycle.
- **Events** go out with `try_send` (never blocks): the UI must give
  `Engine::new` an **unbounded** `async_channel` and `await` it.
- **Full cycle order** (`SyncNow`): connect → replay `pending_ops` (oldest
  first) → outbox → `list_folders` + `plan_folders` → every folder in
  `sync_order` (select, `uids`, `plan_folder`, reset/expunge, headers in
  batches of 100 newest first with `Syncing` + `MailboxChanged` after each,
  flags (CONDSTORE `CHANGEDSINCE` when possible), checkpoint, body prefetch)
  → `purge_orphans` → `Status Idle`. `SyncFolder` = queue + one folder;
  `ProcessQueue` = queue + outbox. Full syncs also run every `poll_interval`
  (IDLE only covers the Inbox).
- **In-flight rule:** UIDs referenced by queued ops are never fetched or
  flag-checked (`in_flight_uids`), and flags are re-checked against a fresh
  op list, so optimistic edits are not reverted. Actions queue the op
  *before* the local write.
- **Errors:** Network/TLS/Cancelled → drop connection, `Offline` with
  exponential back-off (`retry_base` doubling to `retry_max`) and automatic
  retry; Auth → `AuthFailed`, no retries (queue/folder commands ignored)
  until `SyncNow` / `Reconnect`; other errors on one folder skip that folder;
  database errors → `Status Error`. Ops failing with non-connection errors are
  dropped after 3 attempts (folder modseq cleared + `SyncFolder` queued);
  outgoing mail: connection errors keep the row untouched, others count
  attempts and stop after 3 (`SendFailed { permanent: true }`, row kept).
  Sent copies are APPENDed (`\Seen`) to the `sys:sent` folder after
  `complete_outgoing`, unless `connector.saves_sent_copy()`.
- **NewMail:** only unread rows *created* (not merged) in the `sys:inbox`
  folder after its first sync, never after a UIDVALIDITY reset.
- **Folder mapping** (`plan::folder_path_for`): first folder per role takes
  the system path; Gmail `\All` becomes `sys:archive` only when no
  `\Archive` folder exists; `\Flagged`/`\Important`/non-selectable folders
  are not synced; other folders → `user:<decoded/path>` (leading `INBOX`
  segment dropped). A system path re-pointed at another mailbox is reset.
- **Stop:** `stop_account` / `shutdown` / `Drop` cancel the account's
  `CancelToken` (interrupting blocking I/O), send `Shutdown`, join for at
  most `STOP_TIMEOUT` (1 s) and detach stragglers.
- `local` accounts are never synced (the worker reports an error and exits);
  actions only change them locally.

## Work Guidance

- Keep decisions in `plan` (pure, table-tested); the worker only executes.
- Every function gets a test; engine behavior gets an end-to-end test in
  `src/tests.rs` (real worker + `FakeConnector` + temp DB, waiting on events
  with timeouts — no fixed sleeps).
- New server ops: add a `PendingAction` variant (serde-tagged `type`), its
  replay in `Worker::process_queue`, and the producing action.

## Verification

- `cargo test -p mail_sync`
- `cargo clippy -p mail_sync --all-targets --all-features -- -D warnings`
- `cargo fmt`

## Child DOX Index

| Scope | Doc | Owns |
|---|---|---|
| `crates/mail_sync/src/` | [`.dox/crates/mail_sync/src/AGENTS.md`](src/AGENTS.md) | Full API catalog (every fn/method) |
