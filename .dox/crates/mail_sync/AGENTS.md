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
  op list, so optimistic edits are not reverted. Messages with a queued
  `Flags` op (wherever it points, `messages_with_pending_flags`) never get
  server flags applied — neither on header merges nor in flag checks.
  Actions queue the op *before* the local write.
- **Op payload** (`PendingAction`, JSON `type` = `flags` | `move` |
  `delete`): source `(folder, uid)` plus `uid_validity` of that folder when
  recorded and the message's `message_id_hdr`; both `#[serde(default)]`
  (older payloads decode with `None`). `uid == 0` = unresolved (a move
  without COPYUID). **Replay check:** the worker SELECTs each source folder
  once per pass; the recorded UID is used only when its UIDVALIDITY matches,
  otherwise it is re-resolved (stored location with a matching checkpoint →
  `MailRemote::find_uid_by_message_id` → legacy: recorded UID while the
  stored checkpoint matches). Not found: `Flags` count a failed attempt (they
  wait for a sync/lookup); `Move`/`Delete` are dropped and both folders
  resynced. Ops are re-read before each run.
- **Op completion is compare-and-swap** (`complete_op_if_unchanged` /
  `update_op_if_unchanged`), on both sides: UI actions (`move_to_folder`,
  `delete_permanently`) restart their decision (bounded `CAS_ROUNDS`) when
  the worker changed an op meanwhile; the worker, when the UI rewrote or
  cancelled an op while its server call ran, reconciles to the UI's latest
  intent (`reconcile_move`: a rewrite now starts where the message landed; a
  cancel queues a compensating Move back). A Move attaches its new location
  *before* completing, and re-points later ops of the message (CAS) to the
  new location (unresolved without COPYUID, so flag edits survive).
- **Errors:** Network/TLS/Cancelled → drop connection, `Offline` with
  exponential back-off (`retry_base` doubling to `retry_max`) and automatic
  retry; Auth → `AuthFailed`, no retries (queue/folder commands ignored)
  until `SyncNow` / `Reconnect`; other errors on one folder skip that folder;
  database errors → `Status Error`. Ops failing with non-connection errors are
  dropped after 3 attempts (folder modseq cleared + `SyncFolder` queued); a
  dropped Move/Delete restores its source first (re-attach the location, or
  re-fetch the header when the row was deleted — regardless of the sync
  window) so `purge_orphans` cannot lose mail that is still on the server.
- **Outbox state machine** (storage `OutgoingState`; `MailSender::send`
  returns `SendError { failure: SendFailure }`): due `queued` rows are
  claimed (`sending` + lease) → accepted: `append_pending` (raw kept) until
  the Sent copy is APPENDed (`\Seen`, `sys:sent`; deleted at once when
  `connector.saves_sent_copy()`; APPEND failures retry with back-off and are
  given up after `MAX_APPEND_DEFERRALS`, never resending); `Transient` →
  `queued` with back-off (`EngineOptions::send_retry_delays`, default
  1 m/5 m/15 m/1 h repeating), attempts untouched; a queued row 3 days old
  becomes `failed` before any further try (`SEND_GIVE_UP_AFTER`,
  `Database::give_up_outgoing`, by age from creation or the last user retry, even with
  the app closed in between; user decision); `Permanent` → attempts+1,
  back-off, `failed` at `MAX_SEND_ATTEMPTS` (3); `Uncertain` → `maybe_sent`,
  never retried. `failed` / `maybe_sent` rows wait for the UI, which also lists rows
  stuck in the queue, after a failure or left for 5 minutes (`Database::unsent_outgoing` / `retry_outgoing` / `stop_outgoing`,
  and `delete_outgoing`, which refuses a row being sent);
  `SendFailed { permanent }` is `true` exactly when a row stopped retrying.
  `sending` leases older than `SEND_LEASE` (10 min) become `maybe_sent`.
  The worker wakes itself (`ProcessQueue`) when the earliest backed-off row
  is due.
- **Handover / one sender per account:** `Engine` keeps one
  `Arc<Mutex<()>>` per account id for its lifetime and hands it to every
  worker of that account; `process_queue` and `process_outbox` run only
  while holding it (`try_lock`; busy → skipped, `SyncNow` retried after
  `HANDOVER_RETRY` 2 s). A replaced worker that `stop_account` had to
  detach (e.g. blocked in an SMTP send) therefore never replays or sends
  concurrently with its successor.
- **NewMail:** only unread rows *created* (not merged) in the `sys:inbox`
  folder after its first sync, never after a UIDVALIDITY reset.
- **Archive with labels** (user decision): on an All Mail account a message with
  labels only leaves the Inbox and keeps them (`actions::archive_keeping_labels`); out
  of the Inbox, Archive does nothing (Spam and Trash aren't labels: it leaves them). Move, Trash and Junk still leave every label.
- **Folder mapping** (`plan::folder_path_for`, order from `plan_folders`: declared
  SPECIAL-USE first, `\All` after the other declared, then by name nearest the top,
  a leading `INBOX` not counting): first folder per role takes the system path; Gmail `\All` becomes `sys:archive` only when no
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
- New server ops: add a `PendingAction` variant (serde-tagged `type`, with
  `uid_validity` / `message_id_hdr`), its replay in `Worker::process_queue`,
  and the producing action. Never drop/rename payload fields: queued rows
  outlive upgrades (`#[serde(default)]` for additions).
- Races between the UI and the worker are tested deterministically with
  `FakeRemote::on_next_call` hooks (the hook acts as the UI on its own
  connection while the worker is "on the network").

## Verification

- `cargo test -p mail_sync`
- `cargo clippy -p mail_sync --all-targets --all-features -- -D warnings`
- `cargo fmt`

## Child DOX Index

| Scope | Doc | Owns |
|---|---|---|
| `crates/mail_sync/src/` | [`.dox/crates/mail_sync/src/AGENTS.md`](src/AGENTS.md) | Full API catalog (every fn/method) |
