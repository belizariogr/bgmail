# crates/mail_sync/src/ — Source API

## Purpose

Source of the `mail_sync` crate: engine handle, worker/IDLE threads, pure
sync planner, body conversion, optimistic actions, events and commands.

## Ownership

- Owns: every module under `crates/mail_sync/src/`.
- Does not own: crate-level dependency/threading policy (parent
  `.dox/crates/mail_sync/AGENTS.md`).

## Local Contracts

- Every function/method is listed under **API Catalog** with purpose and
  behavior; update the entry with any signature/semantic change.
- `plan` and `body` stay pure (no I/O, no threads); `actions` only touch the
  given `Database`; network and threads live in `worker` / `engine`.
- `rusqlite::Result` is the error type of `actions` (storage's API).

## Work Guidance

- Prefer adding planner functions + table tests over logic inside `Worker`.
- End-to-end tests share the `Harness` in `tests.rs`.

## Verification

- `cargo test -p mail_sync`

## Child DOX Index

_(none — modules are files; their APIs are cataloged below.)_

## API Catalog

### `src/lib.rs`

- `#![forbid(unsafe_code)]`; modules `actions`, `body`, `plan` (pub), `engine`, `events`, `worker` (private), `tests` (`cfg(test)`). Re-exports `ActionOutcome`, `body_content`, `BodyContent`, `Engine`, `EngineOptions`, `STOP_TIMEOUT`, `AccountStatus`, `Command`, `NewMailNotice`, `SyncEvent`, `PendingAction`. No functions.

### `src/events.rs`

_(Types only, all `Clone + Debug + PartialEq + Eq`.)_

- **enum `AccountStatus`**: `Connecting`; `Syncing { folder: String, done: u32, total: u32 }` (folder = display name or storage path; header-fetch progress); `Idle { last_sync: i64 }` (unix s, 0 before the first full cycle); `Offline { error, retry_in_secs }`; `AuthFailed { error }`; `Error { error }`.
- **struct `NewMailNotice`**: `message_id` (storage id), `sender`, `subject`.
- **enum `SyncEvent`**: `Status { account_id, status }`, `MailboxChanged { account_id }`, `FoldersChanged { account_id }`, `NewMail { account_id, messages: Vec<NewMailNotice> }`, `BodyReady { message_id }`, `BodyFailed { message_id, error }`, `Sent { account_id, outbox_id }`, `SendFailed { account_id, outbox_id, error, permanent }`, `SentCopyFailed { account_id, outbox_id, error }` (delivered, but the engine gave up on the Sent copy; the row is gone).
- **enum `Command`** (`Copy`, `Hash`): `SyncNow`, `SyncFolder(folder_id)`, `FetchBody(message_id)`, `ProcessQueue`, `Reconnect`, `Shutdown`.

### `src/engine.rs`

#### Types / constants

- **const `STOP_TIMEOUT`**: 1 s — max wait for threads in `stop_account` / `shutdown`.
- **struct `EngineOptions`** (`Clone`, `Eq`): `poll_interval` (5 min), `idle_timeout` (25 min; zero disables IDLE), `body_prefetch_inbox` (30), `body_prefetch` (10, other folders), `max_body_size` (10 MiB; larger bodies only on demand), `retry_base` (1 s), `retry_max` (5 min), `send_retry_delays: Vec<Duration>` (outbox back-off by failure count; 1 m, 5 m, 15 m, 1 h, last repeats).
- **struct `Engine`**: db path, event sender, options, `Mutex<HashMap<account_id, AccountWorker>>`, `account_locks: Mutex<HashMap<account_id, Arc<Mutex<()>>>>` (lives as long as the engine).
- Private **struct `AccountWorker`**: command `Sender`, `CancelToken`, worker `JoinHandle`.

#### Functions / methods

- **`EngineOptions::default`** — the defaults above.
- **`Engine::new`** (pub) — `pub fn new(db_path: PathBuf, events: async_channel::Sender<SyncEvent>, options: EngineOptions) -> Self`: no threads yet.
- **`Engine::accounts`** (private) — poison-tolerant lock of the worker map.
- **`Engine::account_lock`** (pub(crate)) — `fn account_lock(&self, account_id: i64) -> Arc<Mutex<()>>`: get-or-create the account's handover lock (same `Arc` for every call with that id).
- **`Engine::start_account`** (pub) — `pub fn start_account(&self, account_id: i64, connector: Arc<dyn MailConnector>)`: stops a running worker for the id (≤ `STOP_TIMEOUT`, stragglers detached), creates channel + token, spawns `bgmail-sync-{id}` running `run_worker` with the account lock (starts with `SyncNow`). A detached predecessor keeps the lock until its blocking call returns, so the two never replay ops / send concurrently.
- **`Engine::stop_account`** (pub) — `pub fn stop_account(&self, account_id: i64) -> bool`: removes and stops the worker via `stop_workers`; `false` if not running.
- **`Engine::send`** (pub) — `pub fn send(&self, account_id: i64, command: Command) -> bool`: `false` for unknown accounts or a dead channel.
- **`Engine::sync_all`** (pub) — `SyncNow` to every worker.
- **`Engine::is_running`** (pub) — `pub fn is_running(&self, account_id: i64) -> bool`: registered and thread not finished.
- **`Engine::shutdown`** (pub) — drains the map and stops all workers with one shared deadline.
- **`Drop for Engine`** (`drop`) — `shutdown()`.
- **`stop_workers`** (private) — `fn stop_workers(workers: Vec<AccountWorker>)`: cancels every token and sends `Shutdown` first, then `join_until(now + STOP_TIMEOUT)` each.
- **`join_until`** (private) — `fn join_until(thread: JoinHandle<()>, deadline: Instant) -> bool`: polls `is_finished` every 2 ms; joins (`true`) or detaches at the deadline (`false`).
- Tests: `default_options_match_the_design`, `account_locks_are_shared_per_account`, `join_until_detaches_stuck_threads`.

### `src/plan.rs`

#### Types

- **struct `FolderUpsert`**: `path`, `display_name` (empty for system), `remote_name`, `role` (`FolderRole::as_str`), `reset` (existing local folder at `path` pointed at another mailbox).
- **struct `FolderPlan`**: `upserts` (new/changed only), `deletes` (folder ids).
- **struct `LocalFolderState`**: `sync: FolderSyncState`, `uids` with stored locations.
- **struct `FolderSyncPlan`**: `reset`, `removed`, `to_fetch` (newest first), `flags_check`, `flags_changed_since`.
- **enum `PendingAction`** (serde, internally tagged `"type"`, lowercase): `Flags { folder, uid, add: MessageFlags, remove: MessageFlags, uid_validity, message_id_hdr }`, `Move { from, uid, to, to_folder_id, uid_validity, message_id_hdr }`, `Delete { folder, uid, uid_validity, message_id_hdr }` — folder names are server names; `uid_validity: Option<u32>` (source folder's UIDVALIDITY when `uid` was recorded) and `message_id_hdr: Option<String>` are `#[serde(default, skip_serializing_if = "Option::is_none")]` (older payloads decode); `uid == 0` = unresolved (find by Message-ID).

#### Functions / methods

- **`folder_path_for`** (pub) — `pub fn folder_path_for(remote: &RemoteFolder, used_system_paths: &mut HashSet<&'static str>) -> Option<(String, String)>`: non-selectable / `Flagged` / `Important` → `None`; Inbox/Sent/Drafts/Trash/Junk/Archive/All → system path (`All` → `sys:archive`) with empty display when the path was still free (records it); a taken path → user folder, except `All` → `None`; other roles → `(user_folder_path(name), name)` with `name = user_display_name`.
- **`user_display_name`** (private) — segments split by the delimiter, each `decode_imap_utf7`, a leading `INBOX` segment dropped when nested, joined with `/`.
- **`FolderPlan::is_empty`** (pub) — no upserts and no deletes.
- **`plan_folders`** (pub) — `pub fn plan_folders(remote: &[RemoteFolder], local: &[Folder]) -> FolderPlan`: offers folders with a SPECIAL-USE attribute before name-only matches (so `[Gmail]/Rascunhos` gets `sys:drafts` over a label `Rascunhos`) and `All`-role folders last (so `\Archive` wins), maps via `folder_path_for`, skips duplicate user paths, emits upserts only when display/remote name/role differ from the local row (`reset` when the remote name changed), deletes local folders with a `remote_name` whose path was not planned; local-only folders untouched.
- **`sync_rank`** (private) — inbox 0, sent 1, drafts 2, archive 3, user 4, junk 5, trash 6.
- **`sync_order`** (pub) — `pub fn sync_order(folders: &[Folder]) -> Vec<Folder>`: server-backed folders sorted by `(sync_rank, id)`.
- **`plan_folder`** (pub) — `pub fn plan_folder(local: &LocalFolderState, remote: &FolderStatus, remote_uids: &[u32], window: u32, in_flight: &HashSet<u32>) -> FolderSyncPlan`: `reset` when stored UIDVALIDITY differs (then nothing local is known); `removed` = known − server (sorted); `to_fetch` = newest `window` server UIDs − known − in-flight, descending; `flags_check` = known ∩ server − in-flight; empty when both modseqs are equal or on reset; `flags_changed_since = Some(local)` only when local < server modseq (a lower server modseq re-reads everything).
- **`PendingAction::kind`** (pub) — `"flags"` / `"move"` / `"delete"` (`pending_ops.kind`).
- **`PendingAction::encode`** (pub) — JSON payload.
- **`PendingAction::decode`** (pub) — `Option<Self>`; `None` for anything else.
- **`PendingAction::source`** (pub) — `(folder, uid)` acted on (`Move` → `from`; `uid` 0 = unresolved).
- **`PendingAction::uid_validity`** (pub) — recorded UIDVALIDITY of the source folder.
- **`PendingAction::message_id_hdr`** (pub) — recorded Message-ID header.
- **`PendingAction::retarget`** (pub) — `(folder, new_uid, new_validity: Option<u32>)`: rewrites the source and its UIDVALIDITY (`Move` keeps `to`; Message-ID kept); `0` / `None` = unresolved.
- **`in_flight_uids`** (pub) — `pub fn in_flight_uids(ops: &[PendingOp], folder_remote_name: &str) -> HashSet<u32>`: sources of decodable ops in that folder (unresolved `uid` 0 excluded).
- **`messages_with_pending_flags`** (pub) — `pub fn messages_with_pending_flags(ops: &[PendingOp]) -> HashSet<i64>`: storage ids of messages with a decodable queued `Flags` op (sync must not overwrite their flags).
- **`should_announce`** (pub) — `pub fn should_announce(folder_path: &str, before: &FolderSyncState, reset: bool) -> bool`: `sys:inbox`, already synced once, no reset.
- **`is_new_arrival`** (pub) — `pub fn is_new_arrival(created: bool, gm_msgid: Option<u64>, uid: u32, uid_next_before: Option<u32>) -> bool`: a new row, or a Gmail merge whose UID is at or above the folder's previous UIDNEXT (the message was first met under another label).
- **`notice_for_new`** (pub) — `pub fn notice_for_new(message_id: i64, message: &NewRemoteMessage, arrived: bool, announce: bool) -> Option<NewMailNotice>`: only when arrived ∧ announce ∧ unread.
- **`new_remote_message`** (pub) — `pub fn new_remote_message(header: &RemoteHeader, parsed: &ParsedMessage) -> NewRemoteMessage`: sender = From (else Sender) label/email; subject; to/cc/reply-to via `format_address_list`; ids without brackets; references space-joined; `thread_id` = `gm:<X-GM-THRID>` else `thread_key`; `date_ts` (0 when missing) + `received_at`; flags → unread/starred/answered; `has_attachment` = BODYSTRUCTURE hint else `is_multipart_mixed`; size; `gm_msgid`; preview/plain text empty.
- **`is_multipart_mixed`** (private) — unfolds header lines, true when a `Content-Type:` line starts with `multipart/mixed` (case-insensitive).
- **`received_at`** (pub) — `pub fn received_at(date_ts: i64) -> String`: UTC `YYYY-MM-DD HH:MM`; empty for 0.
- **`civil_from_days`** (private) — days since epoch → `(year, month, day)` (Hinnant), negative days supported.
- Tests: `folder_path_for_maps_roles_and_user_folders` (table), `first_folder_of_a_role_wins_and_all_mail_needs_free_archive`, `user_display_name_without_delimiter`, `plan_folders_creates_updates_resets_and_deletes`, `plan_folders_prefers_archive_role_over_all_mail_regardless_of_order`, `plan_folders_is_empty_when_in_sync`, `sync_order_follows_roles`, `plan_folder_table` (first sync, window, window with known, expunge+modseq, equal modseq, lower modseq, UIDVALIDITY reset, in-flight), `pending_action_round_trips_and_retargets`, `payloads_without_validity_or_message_id_still_decode`, `in_flight_uids_filters_by_source_folder`, `messages_with_pending_flags_ignores_moves_and_garbage`, `announce_rules`, `new_remote_message_maps_headers_and_flags`, `new_remote_message_tolerates_missing_headers`, `multipart_mixed_sniff`, `received_at_and_civil_dates`; helpers `remote`, `local`, `status`, `state`, `op`, `flags`, `mv`, `del`.

### `src/body.rs`

- **const `MAX_INLINE_BYTES`**: 5 MiB of images inlined per body. **const `PREVIEW_CHARS`**: 200.
- **struct `BodyContent`**: `content`, `format` (`"html"`/`"text"`), `plain_text` (`\n` endings), `preview`, `attachments: Vec<storage::NewAttachment>`.
- **`body_content`** (pub) — `pub fn body_content(raw: &[u8]) -> BodyContent`: `parse_message`; HTML preferred (with `cid:` images inlined via `inline_images` + `inline_cid_images` when the HTML mentions `cid:`), plain text from the text part or `html_to_text`; text-only → content = text (`\n`); nothing parseable → empty text. Attachments mapped from `AttachmentInfo` (part index, filename, mime, size, content id, inline).
- **`contacts`** (private module): `contacts_in`, `record_message`, `backfill`: fills the contact catalog from synced mail.
- **`repair_stale_previews`** (pub) — `pub fn repair_stale_previews(db: &Database, account_id: i64) -> rusqlite::Result<()>`: re-derives plain text and preview of downloaded messages whose preview still looks like markup; run off the UI thread when a worker starts.
- **`preview_text`** (pub) — `pub fn preview_text(plain: &str, max_chars: usize) -> String`: whitespace collapsed, first `max_chars` chars.
- **`inline_cid_images`** (pub) — `pub fn inline_cid_images(html: &str, images: &[(String, String, Vec<u8>)], max_bytes: usize) -> String`: replaces `cid:` URLs that follow `=` (optionally quoted) or `url(` with `data:<mime>;base64,…` for matching (case-insensitive, `%XX`-decoded) content ids while the byte budget lasts; everything else verbatim.
- **`percent_decode`** (private) — `%XX` decoding, invalid escapes kept.
- **`base64_encode`** (private) — RFC 4648 with padding.
- Tests: `html_preferred_with_text_part_as_plain`, `html_only_inlines_cid_images_and_derives_plain_text`, `text_only_and_attachments`, `unparseable_body_is_empty_text`, `preview_collapses_and_truncates`, `inline_cid_images_variants_and_budget`, `percent_decode_handles_escapes`, `base64_matches_rfc4648_vectors`.

### `src/actions.rs`

All take the UI's `&storage::Database`, return `rusqlite::Result<ActionOutcome>`; unknown messages → `ActionOutcome::default()`; `local` accounts only change locally. Server ops are queued **before** the local write and record the source folder's UIDVALIDITY (`folder_sync_state`) and the message's Message-ID. Rewrites/cancels of queued ops are compare-and-swap; losing a race restarts the whole decision.

- **const `CAS_ROUNDS`** (private): 8 rounds before `with_cas_retry` gives up retrying.
- **struct `ActionOutcome`** (pub, `Copy`, `Default`): `changed` (reload the list), `needs_sync` (send `Command::ProcessQueue`). Private const `LOCAL` = changed only.
- Private **struct `QueuedMove`**: op `id`, `kind`, `payload` as read (CAS key) + decoded `action`.
- **`context`** (private) — `(MessageDetail, Account)` or `None`.
- **`is_local`** (private) — provider `local`.
- **`pending_moves`** (private) — `QueuedMove`s (Move ops) of the message, oldest first.
- **`unresolved_folder`** (private) — `(db, account_id, message) -> Option<String>`: server name of the message's first primary folder, for a message with neither a location nor a queued move (a move without COPYUID, until a sync re-attaches it); ops go there with UID 0 and `resolve_uid` finds it by Message-ID. `None` without a Message-ID.
- **`flags_target`** (private) — `(db, account_id, message: &MessageDetail)` → `(folder, uid, uid_validity)`: first location with a server name (validity from its folder checkpoint), else the source of the last queued move (in-flight message), else `unresolved_folder` with UID 0, else `None`.
- **`enqueue_flags`** (private) — `(db, account_id, message: &MessageDetail, add, remove) -> bool`: queues `Flags` at `flags_target` with the message's Message-ID, read and enqueue in one transaction (`Database::atomically`) so a move finishing in between can't leave the op on the expunged source; whether queued.
- **`flag_change`** (private) — `(mask, set) -> (add, remove)`.
- **`set_read`** (pub) — `pub fn set_read(db, message_id, read: bool)`: no-op when already in that state; queues `\Seen` add/remove for one location; `set_message_unread`.
- **`set_starred`** (pub) — `pub fn set_starred(db, message_id, starred: bool)`: same with `\Flagged`; `toggle_message_starred` (membership follows).
- **`primary_folders`** (private) — csv paths except `sys:flagged`.
- **`with_cas_retry`** (private) — runs a round closure (`Ok(None)` = lost a CAS race) up to `CAS_ROUNDS` times; if the last round still loses, returns `changed + needs_sync`.
- **`move_to_folder`** (pub) — `pub fn move_to_folder(db, message_id, target_path: &str)`: `with_cas_retry(move_to_folder_round)`.
- **`move_to_folder_round`** (private) — no-op for forbidden targets (`sys:sent`/`drafts`/`flagged`) or when already only there; local → `move_message_to_folder`. Server accounts: target must be an account folder with `remote_name` (else no-op); queued moves of the message are re-pointed at the target with `update_op_if_unchanged` (a move whose source is the target is cancelled with `complete_op_if_unchanged` and its location re-attached unless unresolved); any CAS loss → `None`; each location outside the target gets a queued `Move` (with validity + Message-ID) and `detach_location`, except `role = all` locations when the target is not Trash/Junk and another location moves; finally the local primary-folder move.
- **`archive`** / **`junk`** (pub) — `move_to_folder` to `sys:archive` / `sys:junk` (Gmail All Mail is mapped to `sys:archive`).
- **`trash`** (pub) — in Trash → `delete_permanently`; else `move_to_folder(sys:trash)`.
- **`restore_from_trash`** (pub) — only from Trash: `move_to_folder(sys:inbox)`.
- **`delete_permanently`** (pub) — `with_cas_retry(delete_permanently_round)`.
- **`delete_permanently_round`** (private) — server accounts: location-less (in flight) → queued moves rewritten (CAS) into `Delete` at their source, keeping validity/Message-ID; `Delete` per location; then `delete_message`. `changed` = row deleted.
- Tests (fixture `fixture(provider)` with INBOX/Archive (Gmail: `[Gmail]/All Mail`, role `all`)/Trash/Junk/Work, every server folder at UIDVALIDITY 7, + local-only folder; helpers `message`, `ops`, `csv`, `seen`): `set_read_queues_one_flags_op`, `set_starred_queues_flagged_and_updates_membership`, `local_accounts_change_locally_only`, `move_detaches_source_and_queues_move`, `moving_an_in_flight_message_retargets_its_moves`, `gmail_keeps_all_mail_unless_trashing`, `archive_only_message_moves_out_of_all_mail`, `trash_junk_restore_and_delete`, `delete_permanently_queues_a_delete_per_location`, `move_retries_when_the_worker_changed_the_queued_op`, `stale_op_rewrite_restarts_the_round`, `helpers`.

### `src/worker.rs`

#### Constants / types

- **const `HEADER_BATCH`** 100, **`FLAG_BATCH`** 500, **`MAX_OP_ATTEMPTS`** 3, **`MAX_SEND_ATTEMPTS`** 3 (permanent rejections before `failed`), **`MAX_APPEND_DEFERRALS`** 5 (Sent-copy APPEND failures before giving the copy up), **`SEND_LEASE`** 10 min (stale `sending` → `maybe_sent`) (pub(crate)); private `MAX_WAIT` 1 h (longest single wait), `SLEEP_STEP` 20 ms, `HANDOVER_RETRY` 2 s (retry while another worker holds the account lock), `RETARGET_ROUNDS` 4.
- **struct `Backoff`** (pub(crate)): `base`, `max`, `attempt`.
- **enum `Abort`** (pub(crate)): `Remote(RemoteError)`, `Db(rusqlite::Error)`, `Shutdown`; `From` both errors.
- **struct `WorkerSetup`** (pub(crate)): account id, db path, connector, events, options, cancel token, command receiver, `poke` sender (for IDLE), `account_lock: Arc<Mutex<()>>` (from `Engine::account_lock`).
- **struct `IdleSetup`** (pub(crate)): connector, token, poke, inbox folder id + server name, timeout, shared `unsupported` flag, own `Backoff`.
- Private **struct `Worker`**: setup fields + `remote`, coalescing `queue`, `backoff`, `retry_at`, `auth_failed`, `next_poll`, `last_sync`, `window`, IDLE `JoinHandle`, `idle_unsupported`, `shutdown`, `account_lock`, `outbox_wake: Option<Instant>` (earliest backed-off outbox row).

#### Free functions

- **`unix_now`** — unix seconds (0 before epoch).
- **`Backoff::new`** / **`next_delay`** (`base·2^attempt`, capped, attempt grows) / **`reset`**.
- **`is_connection_error`** — Auth, Network, Tls, Cancelled.
- **`emit`** — `try_send`, ignoring full/closed channels.
- **`enqueue_command`** — `(queue, command) -> bool`: coalescing rules (Shutdown → `true`, not queued; `FetchBody` after earlier body requests, deduped; `SyncNow` drops queued `SyncFolder`/`ProcessQueue`, deduped; those two skipped when `SyncNow` queued; `Reconnect` deduped).
- **`sleep_cancellable`** — sleeps in ≤20 ms steps; `false` when cancelled.
- **`run_worker`** — opens the database (error → `Status Error`, exit) and runs `Worker`.
- **`run_idle`** — loop: connect (Auth → exit; other errors → back-off sleep, retry), then `idle(inbox, timeout, cancel)`: `Changed` → send `SyncFolder(inbox)` (closed channel → exit), `Timeout` → again, `Stopped` → exit, `Unsupported` → set flag, logout, exit, Auth → exit, other errors → back-off and reconnect.

#### `Worker` methods (private)

- **`new`**; **`emit`** / **`status`** / **`mailbox_changed`** / **`idle_status`** (`Idle { last_sync }`) — event helpers.
- **`run`** — refuses missing/`local` accounts (`Status Error`); queues `SyncNow`; loop: `drain_channel`, pop or `wait`, `execute`, until shutdown/cancel; then `finish`.
- **`drain_channel`** — non-blocking receive into the queue; disconnect/`Shutdown` → shutdown.
- **`wait`** — `recv_timeout` until the retry deadline (offline / handover), `next_poll` (online) or `MAX_WAIT` (auth failed), or earlier `outbox_wake` (not while auth failed); retry/poll expiry yields `SyncNow`, `outbox_wake` expiry `ProcessQueue`.
- **`execute`** — `SyncNow` (clears auth failure) / `Reconnect` (also resets back-off, drops the connection) → `full_cycle` and re-arm the poll timer; `SyncFolder` / `ProcessQueue` ignored while auth failed; `FetchBody` → `fetch_body_command` (+ `Idle` if it had to connect); errors → `on_remote_error` / `Status Error` / shutdown.
- **`on_remote_error`** — cancelled → shutdown; drops connection; Auth → `AuthFailed` (no retry); other connection errors → `Offline` + `retry_at = now + backoff`; else `Status Error`.
- **`disconnect`** — drops the remote (logout unless cancelled when asked).
- **`finish`** — logout, cancel the token (stops IDLE), wait ≤500 ms for the IDLE thread.
- **`ensure_connected`** — `Connecting` + `connector.connect`; resets back-off/retry on success.
- **`rm`** — the live remote or a Network error.
- **`check_interrupts`** — cancel/`Shutdown` → `Abort::Shutdown`; drains the channel and serves queued `FetchBody` immediately.
- **`full_cycle`** — reads `sync_window` from `AccountConfig` (default 2000), `process_queue`, `process_outbox`, `sync_folder_list`, every folder in `sync_order` (non-connection folder errors → `Status Error`, continue), `purge_orphans`, `Idle`, `ensure_idle_thread`.
- **`folder_cycle`** — connect, queue, `sync_folder` for that account's server folder (refreshes `last_sync`), `Idle`.
- **`queue_cycle`** — only when ops or due outbox rows (`due_outgoing(now)`) exist: connect, queue, outbox, `Idle`.
- **`try_account_lock`** (assoc. fn) — `fn try_account_lock(lock: &Mutex<()>) -> Option<MutexGuard<'_, ()>>`: `try_lock`, poison tolerated; `None` while another worker of the account holds it.
- **`retry_after_handover`** — `retry_at = min(retry_at, now + HANDOVER_RETRY)`.
- **`process_queue`** — under the account lock (busy → skipped + `retry_after_handover`). Snapshot of op ids; each op re-read (`pending_op`) before running: undecodable → dropped; `resolve_uid` (non-connection SELECT errors → `op_failed`); not found → `op_target_missing`; `store_flags` / `move_messages` / `delete_messages` on the resolved UID; success → `op_done`; connection errors abort; others → `op_failed`. Returns whether local state changed.
- **`resolve_uid`** — `(validities cache, op, action) -> Option<u32>`: SELECTs the source folder once per pass; recorded UID when its UIDVALIDITY matches; else a stored location of the message in that folder whose checkpoint matches the server; else `find_uid_by_message_id` (op's header, else the stored message's; a non-connection error is remembered); else (legacy payload without validity) the recorded UID while the folder checkpoint matches; else the remembered search error (→ `op_failed`, not "gone"); else `None`.
- **`op_target_missing`** — `Flags`: `op_failed` ("message not found"); `Move`/`Delete`: CAS-complete, resync source (and Move target). Returns `true`.
- **`op_failed`** — `fail_op`; at `MAX_OP_ATTEMPTS` CAS-completes (a UI rewrite keeps the op), `restore_source` for Move/Delete, `mark_for_resync(source)`. Returns whether dropped.
- **`restore_source`** — `(op, action, resolved_uid)`: message row exists → `attach_location(source folder, uid)`; row deleted → `fetch_headers(folder, [uid])` + `record_remote_message` (ignores the sync window; non-connection errors ignored).
- **`op_done`** — non-Move: CAS-complete. Move: attach the COPYUID location (validity = target checkpoint) **before** CAS-completing; success → `retarget_later_ops` to the landed location (`uid` 0 / `None` without COPYUID); CAS lost → `reconcile_move`.
- **`reconcile_move`** — UI changed the op while the move ran: rewritten Move to where it landed → complete it, keep location, re-point later ops; other rewrite (another target / Delete) → detach the landed location (list shows the rewrite's target via `move_message_to_folder` when no location remains) and re-point the op and later ones to the landed location; cancelled → re-point later ops, detach landed + stale source locations, enqueue a compensating `Move` landed → source (Message-ID, validity) and queue `ProcessQueue`.
- **`retarget_later_ops`** — `(after_op, message_id, from, landed: (folder, uid, validity))`: later ops of the message whose source folder is `from` are re-pointed with `update_op_if_unchanged`; re-read and repeated (≤ `RETARGET_ROUNDS`) when a CAS lost.
- **`mark_for_resync`** — clears the folder's stored modseq and queues `SyncFolder`.
- **`send_retry_delay`** — `(n) -> i64` seconds: `send_retry_delays[n]`, last entry repeating, 0 when empty.
- **`process_outbox`** — under the account lock (busy → skipped + retry). `expire_outgoing_leases(now − SEND_LEASE)` (each id → `SendFailed { permanent: true }`); `due_outgoing(now)`: `append_pending` → `retry_sent_copy`; legacy rows with attempts ≥ 3 → `failed` + `SendFailed { permanent: true }`; `claim_outgoing` (lost → skip); lazily connected sender (connect errors → `SendError::from`, transient); `send` → `sent` / `send_failed` (stop on connection-level transient or uncertain; cancelled → `Abort::Shutdown` after releasing the claim). Ends with `schedule_outbox_wake`.
- **`sent`** — `append_pending` (or delete when the server files the copy), `Sent` event, `retry_sent_copy`.
- **`send_failed`** — `Transient` → `defer_outgoing(now + delay(deferrals))`; `Permanent` → `fail_outgoing(now + delay(attempts), MAX_SEND_ATTEMPTS)`; `Uncertain` → `mark_outgoing_maybe_sent`. Emits `SendFailed { permanent }` (`true` for `failed` / `maybe_sent`). Returns whether the rest of the outbox should wait.
- **`schedule_outbox_wake`** — `outbox_wake` from `Database::next_outgoing_due`.
- **`sent_folder`** — server name of `sys:sent`.
- **`retry_sent_copy`** — lists folders when Sent is unknown (no Sent → row deleted); APPEND with `\Seen`; success → row deleted; failure → `defer_outgoing` with back-off, or deleted after `MAX_APPEND_DEFERRALS`; connection errors abort, others → `Status Error`. Never resends.
- **`sync_folder_list`** — `list_folders` + `plan_folders`; applies upserts (reset → `clear_folder_locations` + default checkpoint) and deletes; `FoldersChanged` (+ `MailboxChanged` when mail was lost).
- **`sync_folder`** — `Syncing` progress, `select`, `uids`, `plan_folder` with in-flight UIDs; reset/removed applied; header batches (`record_remote_message`; merges get `apply_remote_flags` unless the message has a pending `Flags` op; notices via `should_announce` + `is_new_arrival` + `notice_for_new`) each followed by `Syncing` + `MailboxChanged`; flag batches skipping freshly in-flight UIDs and messages with pending `Flags` ops; checkpoint (UIDVALIDITY, UIDNEXT, modseq, now); `NewMail`; `prefetch_bodies`.
- **`prefetch_bodies`** — `folder_messages_without_body(limit = inbox/other option, max_body_size)`; each downloaded → `BodyReady`; non-connection errors skipped; `MailboxChanged` if any stored.
- **`download_body`** — `fetch_body` → `body_content` → `store_body`.
- **`fetch_body_command`** — `try_fetch_body`; `BodyReady` / `BodyFailed`; `Err` only for connection errors or shutdown.
- **`try_fetch_body`** — missing/foreign message → NotFound; body present → done; auth failed → Auth; first server location (none → NotFound); connect; `download_body`.
- **`ensure_idle_thread`** — unless disabled/unsupported/alive: no IDLE capability → mark unsupported; spawns `bgmail-idle-{id}` running `run_idle` on the `sys:inbox` server folder.

#### Tests

`backoff_doubles_up_to_the_cap_and_resets`, `connection_errors`, `enqueue_command_coalesces`, `sleep_cancellable_returns_early_when_cancelled`, `emit_never_blocks_on_a_full_or_closed_channel`, `unix_now_is_recent`, `run_idle_pokes_on_change_and_stops_on_cancel`, `run_idle_flags_unsupported_and_exits_on_auth`, `run_idle_reconnects_after_network_errors_and_exits_when_worker_is_gone`; helper `idle_setup`.

### `src/tests.rs` (`cfg(test)`, end-to-end)

- Helpers: `seen`, `unread`, `mail(subject, id)` (Message-ID `<subject-id@example.com>`, date ordered by `id`), `date`, `test_options` (1 h poll, IDLE off, no prefetch, 10–40 ms back-off, zero send back-off), `idle_options`, `wait_for_idle_call`, `new_mail`, `strings`, `db_path` (for "UI" connections opened inside `FakeRemote::on_next_call` hooks); **struct `Harness`** (temp DB + account + `FakeRemote` with INBOX/Sent/Drafts/Archive/Trash/Junk + `FakeConnector` + `Engine` + event receiver) with `new`, `imap`, `start`, `started`, `drain`, `wait_for`, `wait_status`, `wait_idle`, `run(command)`, `sync`, `folder_id`, `subjects`, `id_of`, `message_count`, `server_subjects`.
- Tests: `initial_sync_maps_folders_and_fills_lists_incrementally`, `archive_role_wins_over_all_mail`, `gmail_without_archive_folder_maps_all_mail` (gm_msgid dedupe, Starred/[Gmail] skipped), `incremental_sync_announces_only_new_unread_inbox_mail`, `flags_sync_in_both_directions` (incl. modseq skip), `server_expunge_removes_the_local_message`, `uid_validity_reset_refetches_without_duplicates_or_notices`, `move_with_uidplus_attaches_the_new_uid` / `move_without_uidplus_reattaches_by_message_id` (shared `move_round_trip`), `queued_move_is_not_undone_by_a_folder_sync`, `trash_then_delete_permanently`, `op_failing_three_times_is_dropped_and_the_folder_resynced`, `offline_queue_is_replayed_after_reconnect`, `auth_failure_stops_retrying_until_reconnect`, `outbox_is_sent_and_filed_in_sent`, `send_failures_retry_then_become_permanent` (+ user retry), `bodies_prefetch_newest_small_ones_and_fetch_others_on_demand`, `cid_images_are_inlined_into_stored_html`, `idle_wakeup_brings_new_mail`, `stop_account_returns_quickly_while_idle_is_blocked`, `servers_without_idle_fall_back_to_polling`, `sync_window_caps_downloaded_headers`, `local_accounts_are_not_synced`, `engine_handles_many_accounts_and_shuts_down`, `shutdown_command_and_unopenable_database`; review fixes: `queued_ops_follow_the_message_across_a_uid_validity_change`, `legacy_payloads_without_validity_are_rechecked`, `move_without_uidplus_keeps_later_flag_edits`, `sync_keeps_local_flags_while_a_flag_op_is_pending`, `trash_then_restore_while_the_move_runs_converges_to_inbox`, `retargeting_a_move_while_it_runs_converges_to_the_new_target`, `dropped_ops_outside_the_window_restore_the_local_copy`, `transient_send_failures_back_off_without_using_attempts`, `uncertain_sends_are_never_resent_automatically`, `a_previous_worker_holding_the_account_lock_blocks_sending`, `rows_claimed_by_a_dead_worker_become_maybe_sent`, `failed_sent_copy_is_retried_without_resending`, `move_without_copyuid_resyncs_the_target_right_away`, `flag_edit_after_a_move_without_copyuid_reaches_the_server`, `flag_edit_during_a_header_fetch_is_not_overwritten`, `move_after_a_move_without_copyuid_reaches_the_server`, `delete_after_a_move_without_copyuid_reaches_the_server`, `failed_message_id_search_is_not_a_missing_message`, `expired_send_lease_tells_the_ui`, `giving_up_on_the_sent_copy_tells_the_ui`; helpers `moved_without_copyuid`, `imap_without_uidplus`, `eventually`.
