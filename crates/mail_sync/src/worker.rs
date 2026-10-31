//! Per-account worker thread: owns its own `Database` connection and a lazily
//! connected `MailRemote`, executes commands and sync cycles, and keeps an
//! IDLE thread watching the Inbox.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mail_core::{
    parse_message, AccountConfig, CancelToken, IdleOutcome, MailConnector, MailRemote,
    MessageFlags, RemoteError, RemoteErrorKind, DEFAULT_SYNC_WINDOW,
};
use storage::{system, Database, Folder, FolderSyncState, BODY_STATE_FULL, PROVIDER_LOCAL};

use crate::body::body_content;
use crate::engine::EngineOptions;
use crate::events::{AccountStatus, Command, SyncEvent};
use crate::plan::{
    in_flight_uids, new_remote_message, notice_for_new, plan_folder, plan_folders, should_announce,
    sync_order, LocalFolderState, PendingAction,
};

/// Headers fetched per round trip (progress + list refresh after each).
pub(crate) const HEADER_BATCH: usize = 100;
/// UIDs per flag fetch.
pub(crate) const FLAG_BATCH: usize = 500;
/// Failed attempts after which a queued op is dropped (and its folder resynced).
pub(crate) const MAX_OP_ATTEMPTS: u32 = 3;
/// Failed attempts after which an outgoing message is no longer retried.
pub(crate) const MAX_SEND_ATTEMPTS: u32 = 3;
/// Longest single wait of the command loop (keeps it responsive to bugs).
const MAX_WAIT: Duration = Duration::from_secs(3600);
/// Granularity of cancellable sleeps in the IDLE thread.
const SLEEP_STEP: Duration = Duration::from_millis(20);

pub(crate) fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

/// Exponential reconnect delay: `base`, `2·base`, … capped at `max`.
#[derive(Clone, Debug)]
pub(crate) struct Backoff {
    base: Duration,
    max: Duration,
    attempt: u32,
}

impl Backoff {
    pub(crate) fn new(base: Duration, max: Duration) -> Self {
        Self {
            base,
            max,
            attempt: 0,
        }
    }

    /// Delay before the next attempt; grows each call.
    pub(crate) fn next_delay(&mut self) -> Duration {
        let factor = 1u32 << self.attempt.min(20);
        self.attempt = self.attempt.saturating_add(1);
        self.base.saturating_mul(factor).min(self.max)
    }

    pub(crate) fn reset(&mut self) {
        self.attempt = 0;
    }
}

/// Errors that abort the current command.
#[derive(Debug)]
pub(crate) enum Abort {
    Remote(RemoteError),
    Db(rusqlite::Error),
    Shutdown,
}

impl From<RemoteError> for Abort {
    fn from(error: RemoteError) -> Self {
        Self::Remote(error)
    }
}

impl From<rusqlite::Error> for Abort {
    fn from(error: rusqlite::Error) -> Self {
        Self::Db(error)
    }
}

/// Errors that mean the connection (not one folder/message) is unusable.
pub(crate) fn is_connection_error(kind: RemoteErrorKind) -> bool {
    matches!(
        kind,
        RemoteErrorKind::Auth
            | RemoteErrorKind::Network
            | RemoteErrorKind::Tls
            | RemoteErrorKind::Cancelled
    )
}

/// Sends without ever blocking the worker (the UI should use an unbounded
/// channel; a full or closed one just drops the event).
pub(crate) fn emit(events: &async_channel::Sender<SyncEvent>, event: SyncEvent) {
    let _ = events.try_send(event);
}

/// Adds `command` to the pending queue, coalescing: duplicates are dropped,
/// `SyncNow` absorbs queued `SyncFolder`/`ProcessQueue`, and `FetchBody`
/// jumps ahead of everything but earlier body requests. Returns `true` for
/// `Shutdown` (not queued).
pub(crate) fn enqueue_command(queue: &mut VecDeque<Command>, command: Command) -> bool {
    match command {
        Command::Shutdown => return true,
        Command::FetchBody(_) => {
            if !queue.contains(&command) {
                let at = queue
                    .iter()
                    .position(|c| !matches!(c, Command::FetchBody(_)))
                    .unwrap_or(queue.len());
                queue.insert(at, command);
            }
        }
        Command::SyncNow => {
            queue.retain(|c| !matches!(c, Command::SyncFolder(_) | Command::ProcessQueue));
            if !queue.contains(&command) {
                queue.push_back(command);
            }
        }
        Command::SyncFolder(_) | Command::ProcessQueue => {
            if !queue.contains(&Command::SyncNow) && !queue.contains(&command) {
                queue.push_back(command);
            }
        }
        Command::Reconnect => {
            if !queue.contains(&command) {
                queue.push_back(command);
            }
        }
    }
    false
}

/// Sleeps `duration` in small steps; `false` when cancelled meanwhile.
pub(crate) fn sleep_cancellable(cancel: &CancelToken, duration: Duration) -> bool {
    let deadline = Instant::now() + duration;
    while !cancel.is_cancelled() {
        let now = Instant::now();
        if now >= deadline {
            return true;
        }
        std::thread::sleep((deadline - now).min(SLEEP_STEP));
    }
    false
}

/// Everything a worker thread needs; built by `Engine::start_account`.
pub(crate) struct WorkerSetup {
    pub account_id: i64,
    pub db_path: PathBuf,
    pub connector: Arc<dyn MailConnector>,
    pub events: async_channel::Sender<SyncEvent>,
    pub options: EngineOptions,
    pub cancel: CancelToken,
    pub commands: Receiver<Command>,
    /// Clone of the command sender, handed to the IDLE thread.
    pub poke: Sender<Command>,
}

/// Thread entry point.
pub(crate) fn run_worker(setup: WorkerSetup) {
    let db = match Database::open(&setup.db_path) {
        Ok(db) => db,
        Err(error) => {
            emit(
                &setup.events,
                SyncEvent::Status {
                    account_id: setup.account_id,
                    status: AccountStatus::Error {
                        error: error.to_string(),
                    },
                },
            );
            return;
        }
    };
    Worker::new(setup, db).run();
}

struct Worker {
    account_id: i64,
    db: Database,
    connector: Arc<dyn MailConnector>,
    events: async_channel::Sender<SyncEvent>,
    options: EngineOptions,
    cancel: CancelToken,
    commands: Receiver<Command>,
    poke: Sender<Command>,
    remote: Option<Box<dyn MailRemote>>,
    queue: VecDeque<Command>,
    backoff: Backoff,
    retry_at: Option<Instant>,
    auth_failed: bool,
    next_poll: Instant,
    last_sync: i64,
    window: u32,
    idle: Option<JoinHandle<()>>,
    idle_unsupported: Arc<AtomicBool>,
    shutdown: bool,
}

impl Worker {
    fn new(setup: WorkerSetup, db: Database) -> Self {
        Self {
            account_id: setup.account_id,
            db,
            connector: setup.connector,
            events: setup.events,
            backoff: Backoff::new(setup.options.retry_base, setup.options.retry_max),
            next_poll: Instant::now() + setup.options.poll_interval,
            options: setup.options,
            cancel: setup.cancel,
            commands: setup.commands,
            poke: setup.poke,
            remote: None,
            queue: VecDeque::new(),
            retry_at: None,
            auth_failed: false,
            last_sync: 0,
            window: DEFAULT_SYNC_WINDOW,
            idle: None,
            idle_unsupported: Arc::new(AtomicBool::new(false)),
            shutdown: false,
        }
    }

    fn emit(&self, event: SyncEvent) {
        emit(&self.events, event);
    }

    fn status(&self, status: AccountStatus) {
        self.emit(SyncEvent::Status {
            account_id: self.account_id,
            status,
        });
    }

    fn mailbox_changed(&self) {
        self.emit(SyncEvent::MailboxChanged {
            account_id: self.account_id,
        });
    }

    fn run(mut self) {
        match self.db.get_account(self.account_id) {
            Ok(Some(account)) if account.provider != PROVIDER_LOCAL => {}
            Ok(_) => {
                self.status(AccountStatus::Error {
                    error: "account has no server to sync with".into(),
                });
                return;
            }
            Err(error) => {
                self.status(AccountStatus::Error {
                    error: error.to_string(),
                });
                return;
            }
        }
        self.queue.push_back(Command::SyncNow);
        while !self.shutdown && !self.cancel.is_cancelled() {
            self.drain_channel();
            if self.shutdown {
                break;
            }
            let command = match self.queue.pop_front() {
                Some(command) => command,
                None => match self.wait() {
                    Some(command) => command,
                    None => continue,
                },
            };
            self.execute(command);
        }
        self.finish();
    }

    /// Moves every waiting command into the coalescing queue.
    fn drain_channel(&mut self) {
        loop {
            match self.commands.try_recv() {
                Ok(command) => {
                    if enqueue_command(&mut self.queue, command) {
                        self.shutdown = true;
                    }
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.shutdown = true;
                    return;
                }
            }
        }
    }

    /// Blocks until a command arrives or a timer (retry / poll) fires.
    fn wait(&mut self) -> Option<Command> {
        let now = Instant::now();
        let deadline = match (self.retry_at, self.auth_failed) {
            (Some(retry), _) => retry,
            (None, true) => now + MAX_WAIT,
            (None, false) => self.next_poll,
        };
        match self
            .commands
            .recv_timeout(deadline.saturating_duration_since(now).min(MAX_WAIT))
        {
            Ok(command) => {
                if enqueue_command(&mut self.queue, command) {
                    self.shutdown = true;
                }
                None
            }
            Err(RecvTimeoutError::Timeout) => {
                let now = Instant::now();
                if self.retry_at.is_some_and(|retry| retry <= now) {
                    self.retry_at = None;
                    return Some(Command::SyncNow);
                }
                (self.retry_at.is_none() && !self.auth_failed && now >= self.next_poll)
                    .then_some(Command::SyncNow)
            }
            Err(RecvTimeoutError::Disconnected) => {
                self.shutdown = true;
                None
            }
        }
    }

    fn execute(&mut self, command: Command) {
        let was_connected = self.remote.is_some();
        let result = match command {
            Command::Shutdown => {
                self.shutdown = true;
                return;
            }
            Command::SyncNow => {
                self.auth_failed = false;
                self.full_cycle()
            }
            Command::Reconnect => {
                self.auth_failed = false;
                self.backoff.reset();
                self.retry_at = None;
                self.disconnect(true);
                self.full_cycle()
            }
            Command::SyncFolder(folder_id) => {
                if self.auth_failed {
                    return;
                }
                self.folder_cycle(folder_id)
            }
            Command::ProcessQueue => {
                if self.auth_failed {
                    return;
                }
                self.queue_cycle()
            }
            Command::FetchBody(message_id) => self.fetch_body_command(message_id).map(|()| {
                if !was_connected && self.remote.is_some() {
                    self.status(AccountStatus::Idle {
                        last_sync: self.last_sync,
                    });
                }
            }),
        };
        if command == Command::SyncNow || command == Command::Reconnect {
            self.next_poll = Instant::now() + self.options.poll_interval;
        }
        match result {
            Ok(()) => {}
            Err(Abort::Shutdown) => self.shutdown = true,
            Err(Abort::Remote(error)) => self.on_remote_error(error),
            Err(Abort::Db(error)) => self.status(AccountStatus::Error {
                error: error.to_string(),
            }),
        }
    }

    /// Drops the connection (and its state) after a failure; classifies it.
    fn on_remote_error(&mut self, error: RemoteError) {
        if self.cancel.is_cancelled() {
            self.shutdown = true;
            return;
        }
        self.disconnect(false);
        match error.kind {
            RemoteErrorKind::Auth => {
                self.auth_failed = true;
                self.retry_at = None;
                self.status(AccountStatus::AuthFailed {
                    error: error.to_string(),
                });
            }
            kind if is_connection_error(kind) => {
                let delay = self.backoff.next_delay();
                self.retry_at = Some(Instant::now() + delay);
                self.status(AccountStatus::Offline {
                    error: error.to_string(),
                    retry_in_secs: delay.as_secs(),
                });
            }
            _ => self.status(AccountStatus::Error {
                error: error.to_string(),
            }),
        }
    }

    fn disconnect(&mut self, logout: bool) {
        if let Some(mut remote) = self.remote.take() {
            if logout && !self.cancel.is_cancelled() {
                remote.logout();
            }
        }
    }

    fn finish(&mut self) {
        self.disconnect(true);
        // Stops the IDLE thread too (shared token).
        self.cancel.cancel();
        if let Some(idle) = self.idle.take() {
            let deadline = Instant::now() + Duration::from_millis(500);
            while !idle.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(2));
            }
            if idle.is_finished() {
                let _ = idle.join();
            }
        }
    }

    fn ensure_connected(&mut self) -> Result<(), Abort> {
        if self.cancel.is_cancelled() {
            return Err(Abort::Shutdown);
        }
        if self.remote.is_none() {
            self.status(AccountStatus::Connecting);
            self.remote = Some(self.connector.connect(&self.cancel)?);
            self.backoff.reset();
            self.retry_at = None;
        }
        Ok(())
    }

    /// The live connection; only called after `ensure_connected`.
    fn rm(&mut self) -> Result<&mut dyn MailRemote, Abort> {
        match self.remote.as_deref_mut() {
            Some(remote) => Ok(remote),
            None => Err(RemoteError::new(RemoteErrorKind::Network, "not connected").into()),
        }
    }

    /// Between folders/batches: serves body requests right away, queues the
    /// rest, aborts on shutdown.
    fn check_interrupts(&mut self) -> Result<(), Abort> {
        if self.cancel.is_cancelled() {
            return Err(Abort::Shutdown);
        }
        self.drain_channel();
        if self.shutdown {
            return Err(Abort::Shutdown);
        }
        while let Some(&Command::FetchBody(message_id)) = self.queue.front() {
            self.queue.pop_front();
            self.fetch_body_command(message_id)?;
        }
        Ok(())
    }

    fn idle_status(&mut self) {
        self.status(AccountStatus::Idle {
            last_sync: self.last_sync,
        });
    }

    /// Queue → outbox → folder list → every folder → orphan purge.
    fn full_cycle(&mut self) -> Result<(), Abort> {
        self.ensure_connected()?;
        self.window = self
            .db
            .get_account(self.account_id)?
            .and_then(|account| AccountConfig::from_json(&account.config_json))
            .map_or(DEFAULT_SYNC_WINDOW, |config| config.sync_window);
        let mut changed = self.process_queue()?;
        self.process_outbox()?;
        self.sync_folder_list()?;
        for folder in sync_order(&self.db.list_folders(self.account_id)?) {
            self.check_interrupts()?;
            match self.sync_folder(&folder) {
                Ok(()) => {}
                // The folder vanished or the server refused it: skip it.
                Err(Abort::Remote(error)) if !is_connection_error(error.kind) => {
                    self.status(AccountStatus::Error {
                        error: error.to_string(),
                    });
                }
                Err(other) => return Err(other),
            }
        }
        changed |= self.db.purge_orphans(self.account_id)? > 0;
        if changed {
            self.mailbox_changed();
        }
        self.last_sync = unix_now();
        self.idle_status();
        self.ensure_idle_thread();
        Ok(())
    }

    /// Queue + one folder.
    fn folder_cycle(&mut self, folder_id: i64) -> Result<(), Abort> {
        self.ensure_connected()?;
        if self.process_queue()? {
            self.mailbox_changed();
        }
        if let Some(folder) = self.db.folder_by_id(folder_id)? {
            if folder.account_id == self.account_id && folder.remote_name.is_some() {
                self.sync_folder(&folder)?;
            }
        }
        self.idle_status();
        Ok(())
    }

    /// Queue + outbox.
    fn queue_cycle(&mut self) -> Result<(), Abort> {
        let has_work = !self.db.pending_ops(self.account_id)?.is_empty()
            || self
                .db
                .outbox(self.account_id)?
                .iter()
                .any(|row| row.attempts < MAX_SEND_ATTEMPTS);
        if !has_work {
            return Ok(());
        }
        self.ensure_connected()?;
        if self.process_queue()? {
            self.mailbox_changed();
        }
        self.process_outbox()?;
        self.idle_status();
        Ok(())
    }

    /// Replays queued ops oldest first. Returns whether any completed.
    fn process_queue(&mut self) -> Result<bool, Abort> {
        let mut changed = false;
        for op in self.db.pending_ops(self.account_id)? {
            self.check_interrupts()?;
            let Some(action) = PendingAction::decode(&op.payload) else {
                self.db.complete_op(op.id)?;
                continue;
            };
            let remote = self.rm()?;
            let result = match &action {
                PendingAction::Flags {
                    folder,
                    uid,
                    add,
                    remove,
                } => remote
                    .store_flags(folder, &[*uid], *add, *remove)
                    .map(|()| None),
                PendingAction::Move { from, uid, to, .. } => {
                    remote.move_messages(from, &[*uid], to)
                }
                PendingAction::Delete { folder, uid } => {
                    remote.delete_messages(folder, &[*uid]).map(|()| None)
                }
            };
            match result {
                Ok(new_uids) => {
                    self.db.complete_op(op.id)?;
                    changed = true;
                    if let (
                        PendingAction::Move {
                            from,
                            uid,
                            to,
                            to_folder_id,
                        },
                        Some(new_uid),
                        Some(message_id),
                    ) = (
                        &action,
                        new_uids.and_then(|uids| uids.first().copied()),
                        op.message_id,
                    ) {
                        self.db
                            .attach_location(message_id, *to_folder_id, new_uid)?;
                        self.retarget_later_ops(op.id, message_id, (from, *uid), to, new_uid)?;
                    }
                }
                Err(error) if is_connection_error(error.kind) => return Err(error.into()),
                Err(error) => {
                    self.db.fail_op(op.id, &error.to_string())?;
                    if op.attempts + 1 >= MAX_OP_ATTEMPTS {
                        self.db.complete_op(op.id)?;
                        self.mark_for_resync(action.source().0)?;
                        changed = true;
                    }
                }
            }
        }
        Ok(changed)
    }

    /// After a move reported the new UID, ops queued later for the same
    /// message at its old location now act on the new one.
    fn retarget_later_ops(
        &mut self,
        after_op: i64,
        message_id: i64,
        old: (&str, u32),
        folder: &str,
        new_uid: u32,
    ) -> Result<(), Abort> {
        for op in self.db.pending_ops(self.account_id)? {
            if op.id <= after_op || op.message_id != Some(message_id) {
                continue;
            }
            let Some(mut action) = PendingAction::decode(&op.payload) else {
                continue;
            };
            if action.source() == old {
                action.retarget(folder, new_uid);
                self.db.update_op(op.id, action.kind(), &action.encode())?;
            }
        }
        Ok(())
    }

    /// A dropped op left local state diverged: forget the folder's modseq
    /// (forcing a full flag check) and sync it again.
    fn mark_for_resync(&mut self, remote_name: &str) -> Result<(), Abort> {
        if let Some(folder) = self
            .db
            .folder_by_remote_name(self.account_id, remote_name)?
        {
            let mut state = self.db.folder_sync_state(folder.id)?;
            state.highest_modseq = None;
            self.db.set_folder_sync_state(folder.id, &state)?;
            enqueue_command(&mut self.queue, Command::SyncFolder(folder.id));
        }
        Ok(())
    }

    /// Sends queued mail; files a copy in Sent unless the server does.
    fn process_outbox(&mut self) -> Result<(), Abort> {
        let rows: Vec<_> = self
            .db
            .outbox(self.account_id)?
            .into_iter()
            .filter(|row| row.attempts < MAX_SEND_ATTEMPTS)
            .collect();
        if rows.is_empty() {
            return Ok(());
        }
        let mut sender = None;
        for row in rows {
            self.check_interrupts()?;
            let attempt = match &mut sender {
                Some(sender) => Ok(sender),
                None => self
                    .connector
                    .connect_sender(&self.cancel)
                    .map(|connected| sender.insert(connected)),
            };
            let result =
                attempt.and_then(|sender| sender.send(&row.sender, &row.recipients, &row.raw));
            match result {
                Ok(()) => {
                    // Completed before the APPEND: a failure there must never
                    // send the message twice.
                    self.db.complete_outgoing(row.id)?;
                    self.emit(SyncEvent::Sent {
                        account_id: self.account_id,
                        outbox_id: row.id,
                    });
                    self.save_sent_copy(&row.raw)?;
                }
                Err(error) if self.cancel.is_cancelled() => {
                    let _ = error;
                    return Err(Abort::Shutdown);
                }
                Err(error) if is_connection_error(error.kind) => {
                    self.emit(SyncEvent::SendFailed {
                        account_id: self.account_id,
                        outbox_id: row.id,
                        error: error.to_string(),
                        permanent: false,
                    });
                    // Same server, same problem: retry the rest next cycle.
                    break;
                }
                Err(error) => {
                    self.db.fail_outgoing(row.id, &error.to_string())?;
                    self.emit(SyncEvent::SendFailed {
                        account_id: self.account_id,
                        outbox_id: row.id,
                        error: error.to_string(),
                        permanent: row.attempts + 1 >= MAX_SEND_ATTEMPTS,
                    });
                }
            }
        }
        Ok(())
    }

    fn sent_folder(&self) -> Result<Option<String>, Abort> {
        Ok(self
            .db
            .list_folders(self.account_id)?
            .into_iter()
            .find(|f| f.path == system::SENT)
            .and_then(|f| f.remote_name))
    }

    fn save_sent_copy(&mut self, raw: &[u8]) -> Result<(), Abort> {
        if self.connector.saves_sent_copy() {
            return Ok(());
        }
        let mut sent = self.sent_folder()?;
        if sent.is_none() {
            // First cycle of a new account: folders are not listed yet.
            self.sync_folder_list()?;
            sent = self.sent_folder()?;
        }
        let Some(sent) = sent else {
            return Ok(());
        };
        let seen = MessageFlags {
            seen: true,
            ..Default::default()
        };
        match self.rm()?.append(&sent, seen, raw) {
            Ok(()) => Ok(()),
            Err(error) if is_connection_error(error.kind) => Err(error.into()),
            Err(error) => {
                self.status(AccountStatus::Error {
                    error: error.to_string(),
                });
                Ok(())
            }
        }
    }

    /// Applies the server folder list; emits `FoldersChanged` on changes.
    fn sync_folder_list(&mut self) -> Result<(), Abort> {
        let remote = self.rm()?.list_folders()?;
        let local = self.db.list_folders(self.account_id)?;
        let plan = plan_folders(&remote, &local);
        if plan.is_empty() {
            return Ok(());
        }
        let mut lost_mail = !plan.deletes.is_empty();
        for upsert in &plan.upserts {
            let id = self.db.upsert_remote_folder(
                self.account_id,
                &upsert.path,
                &upsert.display_name,
                &upsert.remote_name,
                upsert.role.as_deref(),
            )?;
            if upsert.reset {
                self.db.clear_folder_locations(id)?;
                self.db
                    .set_folder_sync_state(id, &FolderSyncState::default())?;
                lost_mail = true;
            }
        }
        for id in plan.deletes {
            self.db.delete_folder(id)?;
        }
        self.emit(SyncEvent::FoldersChanged {
            account_id: self.account_id,
        });
        if lost_mail {
            self.mailbox_changed();
        }
        Ok(())
    }

    fn sync_folder(&mut self, folder: &Folder) -> Result<(), Abort> {
        let Some(name) = folder.remote_name.clone() else {
            return Ok(());
        };
        let label = if folder.display_name.is_empty() {
            folder.path.clone()
        } else {
            folder.display_name.clone()
        };
        let progress = |done: usize, total: usize| AccountStatus::Syncing {
            folder: label.clone(),
            done: done as u32,
            total: total as u32,
        };
        self.status(progress(0, 0));

        let status = self.rm()?.select(&name)?;
        let remote_uids = self.rm()?.uids(&name)?;
        let before = self.db.folder_sync_state(folder.id)?;
        let local = LocalFolderState {
            sync: before,
            uids: self
                .db
                .folder_uids(folder.id)?
                .into_iter()
                .map(|(uid, _)| uid)
                .collect(),
        };
        let in_flight = in_flight_uids(&self.db.pending_ops(self.account_id)?, &name);
        let plan = plan_folder(&local, &status, &remote_uids, self.window, &in_flight);

        if plan.reset {
            self.db.clear_folder_locations(folder.id)?;
        }
        if !plan.removed.is_empty() {
            self.db.remove_locations(folder.id, &plan.removed)?;
        }
        if plan.reset || !plan.removed.is_empty() {
            self.mailbox_changed();
        }

        let announce = should_announce(&folder.path, &before, plan.reset);
        let total = plan.to_fetch.len();
        let mut done = 0;
        let mut notices = Vec::new();
        for batch in plan.to_fetch.chunks(HEADER_BATCH) {
            self.check_interrupts()?;
            for header in self.rm()?.fetch_headers(&name, batch)? {
                let parsed = parse_message(&header.header).unwrap_or_default();
                let message = new_remote_message(&header, &parsed);
                let insert = self.db.record_remote_message(
                    self.account_id,
                    folder.id,
                    header.uid,
                    &message,
                )?;
                if !insert.created {
                    // Merged into a known message (other label, moved mail):
                    // the server's flags for this copy are current.
                    self.db.apply_remote_flags(
                        folder.id,
                        header.uid,
                        message.unread,
                        message.starred,
                        message.answered,
                    )?;
                }
                notices.extend(notice_for_new(
                    insert.message_id,
                    &message,
                    insert.created,
                    announce,
                ));
            }
            done += batch.len();
            self.status(progress(done, total));
            self.mailbox_changed();
        }

        let mut flags_changed = false;
        for chunk in plan.flags_check.chunks(FLAG_BATCH) {
            self.check_interrupts()?;
            let flags = self
                .rm()?
                .fetch_flags(&name, chunk, plan.flags_changed_since)?;
            // Re-read: the UI may have queued an edit while we fetched.
            let in_flight = in_flight_uids(&self.db.pending_ops(self.account_id)?, &name);
            for remote in flags {
                if in_flight.contains(&remote.uid) {
                    continue;
                }
                flags_changed |= self.db.apply_remote_flags(
                    folder.id,
                    remote.uid,
                    !remote.flags.seen,
                    remote.flags.flagged,
                    remote.flags.answered,
                )?;
            }
        }
        if flags_changed {
            self.mailbox_changed();
        }

        self.db.set_folder_sync_state(
            folder.id,
            &FolderSyncState {
                uid_validity: Some(status.uid_validity),
                uid_next: status.uid_next,
                highest_modseq: status.highest_modseq,
                synced_at: Some(unix_now()),
            },
        )?;
        if !notices.is_empty() {
            self.emit(SyncEvent::NewMail {
                account_id: self.account_id,
                messages: notices,
            });
        }
        self.prefetch_bodies(folder, &name)
    }

    /// Downloads the newest headers-only bodies of the folder (small ones).
    fn prefetch_bodies(&mut self, folder: &Folder, name: &str) -> Result<(), Abort> {
        let limit = if folder.path == system::INBOX {
            self.options.body_prefetch_inbox
        } else {
            self.options.body_prefetch
        };
        if limit == 0 {
            return Ok(());
        }
        let candidates = self.db.folder_messages_without_body(
            folder.id,
            limit,
            u64::from(self.options.max_body_size),
        )?;
        let mut stored = false;
        for (message_id, uid) in candidates {
            self.check_interrupts()?;
            match self.download_body(message_id, name, uid) {
                Ok(()) => {
                    stored = true;
                    self.emit(SyncEvent::BodyReady { message_id });
                }
                Err(Abort::Remote(error)) if !is_connection_error(error.kind) => {}
                Err(other) => return Err(other),
            }
        }
        if stored {
            self.mailbox_changed();
        }
        Ok(())
    }

    fn download_body(&mut self, message_id: i64, folder: &str, uid: u32) -> Result<(), Abort> {
        let raw = self.rm()?.fetch_body(folder, uid)?;
        let body = body_content(&raw);
        self.db.store_body(
            message_id,
            &raw,
            &body.content,
            body.format,
            &body.plain_text,
            &body.preview,
            &body.attachments,
        )?;
        Ok(())
    }

    /// On-demand body: emits `BodyReady` / `BodyFailed`; returns `Err` only
    /// when the connection (or the worker) must stop.
    fn fetch_body_command(&mut self, message_id: i64) -> Result<(), Abort> {
        match self.try_fetch_body(message_id) {
            Ok(()) => {
                self.emit(SyncEvent::BodyReady { message_id });
                Ok(())
            }
            Err(Abort::Shutdown) => Err(Abort::Shutdown),
            Err(Abort::Remote(error)) => {
                self.emit(SyncEvent::BodyFailed {
                    message_id,
                    error: error.to_string(),
                });
                if is_connection_error(error.kind) {
                    Err(error.into())
                } else {
                    Ok(())
                }
            }
            Err(Abort::Db(error)) => {
                self.emit(SyncEvent::BodyFailed {
                    message_id,
                    error: error.to_string(),
                });
                Ok(())
            }
        }
    }

    fn try_fetch_body(&mut self, message_id: i64) -> Result<(), Abort> {
        let not_found = |what: &str| RemoteError::new(RemoteErrorKind::NotFound, what);
        let Some(message) = self.db.get_message(message_id)? else {
            return Err(not_found("message no longer exists").into());
        };
        if message.account_id != self.account_id {
            return Err(not_found("message belongs to another account").into());
        }
        if message.body_state == BODY_STATE_FULL {
            return Ok(());
        }
        if self.auth_failed {
            return Err(RemoteError::new(RemoteErrorKind::Auth, "sign-in failed").into());
        }
        let Some((folder, uid)) = self
            .db
            .message_locations(message_id)?
            .into_iter()
            .find_map(|l| l.remote_name.map(|name| (name, l.uid)))
        else {
            return Err(not_found("message is not on the server yet").into());
        };
        self.ensure_connected()?;
        self.download_body(message_id, &folder, uid)
    }

    /// Starts (or restarts) the IDLE thread on the Inbox when the server
    /// supports it.
    fn ensure_idle_thread(&mut self) {
        if self.options.idle_timeout.is_zero() || self.idle_unsupported.load(Ordering::SeqCst) {
            return;
        }
        if self.idle.as_ref().is_some_and(|idle| !idle.is_finished()) {
            return;
        }
        if let Some(remote) = self.remote.as_deref() {
            if !remote.capabilities().idle {
                self.idle_unsupported.store(true, Ordering::SeqCst);
                return;
            }
        }
        let Ok(folders) = self.db.list_folders(self.account_id) else {
            return;
        };
        let Some(inbox) = folders.into_iter().find(|f| f.path == system::INBOX) else {
            return;
        };
        let Some(folder_name) = inbox.remote_name else {
            return;
        };
        let setup = IdleSetup {
            connector: self.connector.clone(),
            cancel: self.cancel.clone(),
            poke: self.poke.clone(),
            folder_id: inbox.id,
            folder_name,
            timeout: self.options.idle_timeout,
            unsupported: self.idle_unsupported.clone(),
            backoff: Backoff::new(self.options.retry_base, self.options.retry_max),
        };
        self.idle = std::thread::Builder::new()
            .name(format!("bgmail-idle-{}", self.account_id))
            .spawn(move || run_idle(setup))
            .ok();
    }
}

pub(crate) struct IdleSetup {
    pub connector: Arc<dyn MailConnector>,
    pub cancel: CancelToken,
    pub poke: Sender<Command>,
    pub folder_id: i64,
    pub folder_name: String,
    pub timeout: Duration,
    pub unsupported: Arc<AtomicBool>,
    pub backoff: Backoff,
}

/// IDLE loop on its own connection: every change pokes the worker with
/// `SyncFolder(inbox)`. Exits on cancellation, a closed worker, auth failure
/// or `Unsupported` (then the worker relies on polling alone).
pub(crate) fn run_idle(mut setup: IdleSetup) {
    while !setup.cancel.is_cancelled() {
        let mut remote = match setup.connector.connect(&setup.cancel) {
            Ok(remote) => remote,
            Err(error) if error.kind == RemoteErrorKind::Auth => return,
            Err(_) => {
                if !sleep_cancellable(&setup.cancel, setup.backoff.next_delay()) {
                    return;
                }
                continue;
            }
        };
        setup.backoff.reset();
        loop {
            match remote.idle(&setup.folder_name, setup.timeout, &setup.cancel) {
                Ok(IdleOutcome::Changed) => {
                    if setup
                        .poke
                        .send(Command::SyncFolder(setup.folder_id))
                        .is_err()
                    {
                        return;
                    }
                }
                Ok(IdleOutcome::Timeout) => {}
                Ok(IdleOutcome::Stopped) => return,
                Err(error) if error.kind == RemoteErrorKind::Unsupported => {
                    setup.unsupported.store(true, Ordering::SeqCst);
                    remote.logout();
                    return;
                }
                Err(error) if error.kind == RemoteErrorKind::Auth => return,
                Err(_) => break,
            }
            if setup.cancel.is_cancelled() {
                return;
            }
        }
        if !sleep_cancellable(&setup.cancel, setup.backoff.next_delay()) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_core::fake::{FakeConnector, FakeRemote, FakeSender};

    #[test]
    fn backoff_doubles_up_to_the_cap_and_resets() {
        let mut backoff = Backoff::new(Duration::from_secs(1), Duration::from_secs(300));
        let delays: Vec<u64> = (0..12).map(|_| backoff.next_delay().as_secs()).collect();
        assert_eq!(
            delays,
            vec![1, 2, 4, 8, 16, 32, 64, 128, 256, 300, 300, 300]
        );
        for _ in 0..100 {
            backoff.next_delay();
        }
        assert_eq!(backoff.next_delay(), Duration::from_secs(300));
        backoff.reset();
        assert_eq!(backoff.next_delay(), Duration::from_secs(1));
    }

    #[test]
    fn connection_errors() {
        use RemoteErrorKind::*;
        for kind in [Auth, Network, Tls, Cancelled] {
            assert!(is_connection_error(kind), "{kind:?}");
        }
        for kind in [Protocol, NotFound, Unsupported, Other] {
            assert!(!is_connection_error(kind), "{kind:?}");
        }
    }

    #[test]
    fn enqueue_command_coalesces() {
        let mut queue = VecDeque::new();
        assert!(!enqueue_command(&mut queue, Command::SyncFolder(1)));
        enqueue_command(&mut queue, Command::SyncFolder(1));
        enqueue_command(&mut queue, Command::ProcessQueue);
        enqueue_command(&mut queue, Command::FetchBody(5));
        enqueue_command(&mut queue, Command::FetchBody(6));
        enqueue_command(&mut queue, Command::FetchBody(5));
        assert_eq!(
            Vec::from(queue.clone()),
            vec![
                Command::FetchBody(5),
                Command::FetchBody(6),
                Command::SyncFolder(1),
                Command::ProcessQueue
            ]
        );
        enqueue_command(&mut queue, Command::SyncNow);
        enqueue_command(&mut queue, Command::SyncFolder(2));
        enqueue_command(&mut queue, Command::ProcessQueue);
        enqueue_command(&mut queue, Command::SyncNow);
        enqueue_command(&mut queue, Command::Reconnect);
        enqueue_command(&mut queue, Command::Reconnect);
        assert_eq!(
            Vec::from(queue.clone()),
            vec![
                Command::FetchBody(5),
                Command::FetchBody(6),
                Command::SyncNow,
                Command::Reconnect
            ]
        );
        assert!(enqueue_command(&mut queue, Command::Shutdown));
        assert_eq!(queue.len(), 4);
    }

    #[test]
    fn sleep_cancellable_returns_early_when_cancelled() {
        let cancel = CancelToken::new();
        assert!(sleep_cancellable(&cancel, Duration::from_millis(1)));
        let token = cancel.clone();
        let started = Instant::now();
        let handle = std::thread::spawn(move || sleep_cancellable(&token, Duration::from_secs(30)));
        std::thread::sleep(Duration::from_millis(30));
        cancel.cancel();
        assert!(!handle.join().unwrap());
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn emit_never_blocks_on_a_full_or_closed_channel() {
        let (tx, rx) = async_channel::bounded(1);
        emit(&tx, SyncEvent::BodyReady { message_id: 1 });
        emit(&tx, SyncEvent::BodyReady { message_id: 2 });
        assert_eq!(
            rx.try_recv().unwrap(),
            SyncEvent::BodyReady { message_id: 1 }
        );
        drop(rx);
        emit(&tx, SyncEvent::BodyReady { message_id: 3 });
    }

    #[test]
    fn unix_now_is_recent() {
        assert!(unix_now() > 1_700_000_000);
    }

    fn idle_setup(connector: &FakeConnector) -> (IdleSetup, Receiver<Command>) {
        let (poke, commands) = std::sync::mpsc::channel();
        let setup = IdleSetup {
            connector: Arc::new(connector.clone()),
            cancel: CancelToken::new(),
            poke,
            folder_id: 7,
            folder_name: "INBOX".into(),
            timeout: Duration::from_secs(30),
            unsupported: Arc::new(AtomicBool::new(false)),
            backoff: Backoff::new(Duration::from_millis(1), Duration::from_millis(5)),
        };
        (setup, commands)
    }

    #[test]
    fn run_idle_pokes_on_change_and_stops_on_cancel() {
        let server = FakeRemote::new();
        server.add_folder("INBOX", &[]);
        let connector = FakeConnector::new(server.clone(), FakeSender::new());
        connector.fail_next_connect(RemoteErrorKind::Network); // retried
        let (setup, commands) = idle_setup(&connector);
        let cancel = setup.cancel.clone();
        let handle = std::thread::spawn(move || run_idle(setup));
        while !server.calls().iter().any(|c| c == "idle INBOX") {
            std::thread::sleep(Duration::from_millis(1));
        }
        server.deliver("INBOX", b"Subject: x\r\n\r\n", MessageFlags::default());
        assert_eq!(
            commands.recv_timeout(Duration::from_secs(5)).unwrap(),
            Command::SyncFolder(7)
        );
        cancel.cancel();
        handle.join().unwrap();
    }

    #[test]
    fn run_idle_flags_unsupported_and_exits_on_auth() {
        let server = FakeRemote::new();
        server.add_folder("INBOX", &[]);
        server.set_capabilities(mail_core::Capabilities::default());
        let connector = FakeConnector::new(server.clone(), FakeSender::new());
        let (setup, _commands) = idle_setup(&connector);
        let unsupported = setup.unsupported.clone();
        run_idle(setup);
        assert!(unsupported.load(Ordering::SeqCst));
        assert!(server.is_logged_out());

        connector.fail_next_connect(RemoteErrorKind::Auth);
        let (setup, _commands) = idle_setup(&connector);
        let unsupported = setup.unsupported.clone();
        run_idle(setup); // returns instead of retrying forever
        assert!(!unsupported.load(Ordering::SeqCst));

        // IDLE failing with auth on an open connection also exits.
        let server = FakeRemote::new();
        server.add_folder("INBOX", &[]);
        server.fail_next(RemoteErrorKind::Auth);
        let (setup, _commands) = idle_setup(&FakeConnector::new(server, FakeSender::new()));
        run_idle(setup);
    }

    #[test]
    fn run_idle_reconnects_after_network_errors_and_exits_when_worker_is_gone() {
        let server = FakeRemote::new();
        server.add_folder("INBOX", &[]);
        server.fail_next(RemoteErrorKind::Network); // first idle fails → reconnect
        let connector = FakeConnector::new(server.clone(), FakeSender::new());
        let (setup, commands) = idle_setup(&connector);
        drop(commands);
        let handle = std::thread::spawn(move || run_idle(setup));
        while connector.connects() < 2 {
            std::thread::sleep(Duration::from_millis(1));
        }
        while server.calls().iter().filter(|c| *c == "idle INBOX").count() < 2 {
            std::thread::sleep(Duration::from_millis(1));
        }
        server.deliver("INBOX", b"Subject: x\r\n\r\n", MessageFlags::default());
        handle.join().unwrap(); // poke fails → exits
    }
}
