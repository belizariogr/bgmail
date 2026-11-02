//! UI-owned handle that starts, commands and stops per-account workers.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use mail_core::{CancelToken, MailConnector};

use crate::events::{Command, SyncEvent};
use crate::worker::{run_worker, WorkerSetup};

/// Longest time `stop_account` / `shutdown` wait for threads before
/// detaching them, so the UI never hangs on a stuck socket.
pub const STOP_TIMEOUT: Duration = Duration::from_secs(1);

/// Tuning knobs (defaults suit production; tests shrink the timers).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineOptions {
    /// Full sync interval (also the fallback when IDLE is unsupported).
    pub poll_interval: Duration,
    /// Length of one IDLE wait before it is renewed (servers drop idle
    /// sessions after ~30 min). Zero disables the IDLE thread.
    pub idle_timeout: Duration,
    /// Newest bodies fetched eagerly in the Inbox.
    pub body_prefetch_inbox: usize,
    /// Newest bodies fetched eagerly in every other folder.
    pub body_prefetch: usize,
    /// Bodies larger than this (bytes) are only fetched on demand.
    pub max_body_size: u32,
    /// First reconnect delay; doubles per failure.
    pub retry_base: Duration,
    /// Reconnect delay cap.
    pub retry_max: Duration,
    /// Outbox back-off: delay before retry `n` of a message that failed `n`
    /// times (transient failures and permanent rejections counted
    /// separately); the last entry repeats.
    pub send_retry_delays: Vec<Duration>,
}

impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(5 * 60),
            idle_timeout: Duration::from_secs(25 * 60),
            body_prefetch_inbox: 30,
            body_prefetch: 10,
            max_body_size: 10 * 1024 * 1024,
            retry_base: Duration::from_secs(1),
            retry_max: Duration::from_secs(5 * 60),
            send_retry_delays: [60, 5 * 60, 15 * 60, 60 * 60]
                .map(Duration::from_secs)
                .to_vec(),
        }
    }
}

struct AccountWorker {
    commands: Sender<Command>,
    cancel: CancelToken,
    thread: JoinHandle<()>,
}

/// Owns one worker (+ IDLE) thread per started account. Events arrive on the
/// `async_channel` given to [`Engine::new`] (use an unbounded one: workers
/// never block on it and drop events when it is full). Dropping the engine
/// shuts everything down.
pub struct Engine {
    db_path: PathBuf,
    events: async_channel::Sender<SyncEvent>,
    options: EngineOptions,
    accounts: Mutex<HashMap<i64, AccountWorker>>,
    /// Per-account lock handed to every worker of the account (see
    /// [`Engine::start_account`]); kept for the engine's lifetime.
    account_locks: Mutex<HashMap<i64, Arc<Mutex<()>>>>,
}

impl Engine {
    pub fn new(
        db_path: PathBuf,
        events: async_channel::Sender<SyncEvent>,
        options: EngineOptions,
    ) -> Self {
        Self {
            db_path,
            events,
            options,
            accounts: Mutex::new(HashMap::new()),
            account_locks: Mutex::new(HashMap::new()),
        }
    }

    fn accounts(&self) -> MutexGuard<'_, HashMap<i64, AccountWorker>> {
        self.accounts.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The lock every worker of `account_id` holds while replaying queued
    /// ops or sending.
    pub(crate) fn account_lock(&self, account_id: i64) -> Arc<Mutex<()>> {
        self.account_locks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(account_id)
            .or_default()
            .clone()
    }

    /// Starts syncing `account_id` (replacing a running worker). The worker
    /// opens its own database connection and begins with a full sync.
    ///
    /// Handover: the old worker is cancelled and joined for up to
    /// [`STOP_TIMEOUT`]; one stuck in a blocking call (an SMTP send cannot
    /// be interrupted) is detached but still holds the account lock until
    /// that call returns, and the new worker skips queue/outbox work (and
    /// retries shortly) while it does. Rows being sent are also leased
    /// (`sending`), so they are never sent twice.
    pub fn start_account(&self, account_id: i64, connector: Arc<dyn MailConnector>) {
        self.stop_account(account_id);
        let (commands, receiver) = mpsc::channel();
        let cancel = CancelToken::new();
        let setup = WorkerSetup {
            account_id,
            db_path: self.db_path.clone(),
            connector,
            events: self.events.clone(),
            options: self.options.clone(),
            cancel: cancel.clone(),
            commands: receiver,
            poke: commands.clone(),
            account_lock: self.account_lock(account_id),
        };
        let spawned = std::thread::Builder::new()
            .name(format!("bgmail-sync-{account_id}"))
            .spawn(move || run_worker(setup));
        if let Ok(thread) = spawned {
            self.accounts().insert(
                account_id,
                AccountWorker {
                    commands,
                    cancel,
                    thread,
                },
            );
        }
    }

    /// Cancels the account's blocking I/O and waits up to [`STOP_TIMEOUT`]
    /// for its threads (detaching them after). Returns whether it was running.
    pub fn stop_account(&self, account_id: i64) -> bool {
        let Some(worker) = self.accounts().remove(&account_id) else {
            return false;
        };
        stop_workers(vec![worker]);
        true
    }

    /// Sends a command; `false` when the account is not running.
    pub fn send(&self, account_id: i64, command: Command) -> bool {
        self.accounts()
            .get(&account_id)
            .is_some_and(|worker| worker.commands.send(command).is_ok())
    }

    /// `SyncNow` to every running account.
    pub fn sync_all(&self) {
        for worker in self.accounts().values() {
            let _ = worker.commands.send(Command::SyncNow);
        }
    }

    /// Whether the account's worker thread is alive.
    pub fn is_running(&self, account_id: i64) -> bool {
        self.accounts()
            .get(&account_id)
            .is_some_and(|worker| !worker.thread.is_finished())
    }

    /// Stops every account (one shared [`STOP_TIMEOUT`]).
    pub fn shutdown(&self) {
        let workers: Vec<AccountWorker> = self.accounts().drain().map(|(_, w)| w).collect();
        stop_workers(workers);
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Cancels all first (so they wind down in parallel), then joins each until
/// the shared deadline; stragglers are detached.
fn stop_workers(workers: Vec<AccountWorker>) {
    for worker in &workers {
        worker.cancel.cancel();
        let _ = worker.commands.send(Command::Shutdown);
    }
    let deadline = Instant::now() + STOP_TIMEOUT;
    for worker in workers {
        join_until(worker.thread, deadline);
    }
}

/// Joins `thread` if it finishes before `deadline`; otherwise detaches it.
/// Returns whether it was joined.
fn join_until(thread: JoinHandle<()>, deadline: Instant) -> bool {
    while !thread.is_finished() {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    thread.join().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_options_match_the_design() {
        let options = EngineOptions::default();
        assert_eq!(options.poll_interval, Duration::from_secs(300));
        assert_eq!(options.idle_timeout, Duration::from_secs(1500));
        assert_eq!(
            (options.body_prefetch_inbox, options.body_prefetch),
            (30, 10)
        );
        assert_eq!(options.max_body_size, 10 * 1024 * 1024);
        assert_eq!(options.retry_base, Duration::from_secs(1));
        assert_eq!(options.retry_max, Duration::from_secs(300));
        assert_eq!(
            options.send_retry_delays,
            [60, 300, 900, 3600].map(Duration::from_secs).to_vec()
        );
    }

    #[test]
    fn account_locks_are_shared_per_account() {
        let (tx, _rx) = async_channel::unbounded();
        let engine = Engine::new(PathBuf::from("unused.db"), tx, EngineOptions::default());
        let first = engine.account_lock(1);
        assert!(Arc::ptr_eq(&first, &engine.account_lock(1)));
        assert!(!Arc::ptr_eq(&first, &engine.account_lock(2)));
    }

    #[test]
    fn join_until_detaches_stuck_threads() {
        let quick = std::thread::spawn(|| {});
        assert!(join_until(quick, Instant::now() + Duration::from_secs(5)));
        let (tx, rx) = mpsc::channel::<()>();
        let stuck = std::thread::spawn(move || {
            let _ = rx.recv();
        });
        let started = Instant::now();
        assert!(!join_until(
            stuck,
            Instant::now() + Duration::from_millis(20)
        ));
        assert!(started.elapsed() < Duration::from_secs(1));
        drop(tx);
    }
}
