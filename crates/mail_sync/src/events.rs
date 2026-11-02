//! Messages between the UI and the per-account sync workers.

/// What an account's worker is doing, for the sidebar/status bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccountStatus {
    Connecting,
    /// `folder` is the storage path or display name of the folder being
    /// synced; `done` / `total` count header fetches in it.
    Syncing {
        folder: String,
        done: u32,
        total: u32,
    },
    /// Up to date; `last_sync` in unix seconds.
    Idle {
        last_sync: i64,
    },
    /// Network/TLS failure; the worker retries by itself in `retry_in_secs`.
    Offline {
        error: String,
        retry_in_secs: u64,
    },
    /// Credentials rejected; no retries until `Command::SyncNow` /
    /// `Command::Reconnect`.
    AuthFailed {
        error: String,
    },
    /// Unexpected failure (protocol, database); the next poll tries again.
    Error {
        error: String,
    },
}

/// One message worth a desktop notification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewMailNotice {
    pub message_id: i64,
    pub sender: String,
    pub subject: String,
}

/// Engine → UI notifications. Ids are storage ids.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncEvent {
    Status {
        account_id: i64,
        status: AccountStatus,
    },
    /// Messages of the account changed (list/counters must reload).
    MailboxChanged {
        account_id: i64,
    },
    /// The account's folder list changed.
    FoldersChanged {
        account_id: i64,
    },
    /// Unread mail that arrived in the Inbox after its first sync.
    NewMail {
        account_id: i64,
        messages: Vec<NewMailNotice>,
    },
    BodyReady {
        message_id: i64,
    },
    BodyFailed {
        message_id: i64,
        error: String,
    },
    Sent {
        account_id: i64,
        outbox_id: i64,
    },
    /// `permanent`: the engine stopped retrying (the row stays in the outbox
    /// with its `last_error` for the UI to resolve).
    SendFailed {
        account_id: i64,
        outbox_id: i64,
        error: String,
        permanent: bool,
    },
    /// The message was delivered, but the engine gave up filing its copy in
    /// the Sent folder (the outbox row is gone).
    SentCopyFailed {
        account_id: i64,
        outbox_id: i64,
        error: String,
    },
}

/// UI → worker requests. Duplicates waiting in the queue are coalesced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    /// Full cycle: queue, outbox, folder list, every folder.
    SyncNow,
    /// Queue + one folder (storage folder id).
    SyncFolder(i64),
    /// Download one body now, ahead of any running cycle.
    FetchBody(i64),
    /// Replay pending ops and send the outbox.
    ProcessQueue,
    /// Forget back-off/auth-failure state, reconnect and sync.
    Reconnect,
    Shutdown,
}
