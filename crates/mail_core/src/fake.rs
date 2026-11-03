//! In-memory IMAP/SMTP stand-ins for sync-engine tests.
//!
//! Clones share one server state (`Arc<Mutex<..>>`): a test keeps a handle to
//! mutate the "server" (deliver mail, flip flags, reset UIDVALIDITY) while the
//! engine owns another as its `Box<dyn MailRemote>` connection.
//! [`FakeConnector`] hands such clones out as a [`MailConnector`].

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::cancel::CancelToken;

use crate::flags::MessageFlags;
use crate::folder::RemoteFolder;
use crate::parse::{has_attachments, parse_message};
use crate::remote::{
    Capabilities, FolderStatus, IdleOutcome, MailConnector, MailRemote, MailSender, RemoteError,
    RemoteErrorKind, RemoteFlags, RemoteHeader, RemoteResult, SendError,
};

/// Hierarchy delimiter the fake server reports for every folder.
pub const FAKE_DELIMITER: &str = "/";

/// A message as stored on the fake server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FakeMessage {
    pub uid: u32,
    pub raw: Vec<u8>,
    pub flags: MessageFlags,
    pub modseq: u64,
}

#[derive(Debug)]
struct FakeFolder {
    name: String,
    attributes: Vec<String>,
    uid_validity: u32,
    uid_next: u32,
    messages: BTreeMap<u32, FakeMessage>,
    /// Bumped by every change inside the folder; `idle` waits on it.
    version: u64,
}

#[derive(Debug)]
struct ServerState {
    folders: Vec<FakeFolder>,
    capabilities: Capabilities,
    /// Global modseq counter (monotonic across folders, like Dovecot/Gmail).
    modseq: u64,
    next_uid_validity: u32,
    failures: VecDeque<RemoteErrorKind>,
    calls: Vec<String>,
    selected: Option<String>,
    logged_out: bool,
}

impl ServerState {
    fn folder(&self, name: &str) -> RemoteResult<&FakeFolder> {
        self.folders
            .iter()
            .find(|f| f.name == name)
            .ok_or_else(|| not_found(name))
    }

    fn folder_mut(&mut self, name: &str) -> RemoteResult<&mut FakeFolder> {
        self.folders
            .iter_mut()
            .find(|f| f.name == name)
            .ok_or_else(|| not_found(name))
    }

    fn bump_modseq(&mut self) -> u64 {
        self.modseq += 1;
        self.modseq
    }

    fn insert(&mut self, folder: &str, raw: Vec<u8>, flags: MessageFlags) -> RemoteResult<u32> {
        let modseq = self.bump_modseq();
        let folder = self.folder_mut(folder)?;
        let uid = folder.uid_next;
        folder.uid_next += 1;
        folder.version += 1;
        folder.messages.insert(
            uid,
            FakeMessage {
                uid,
                raw,
                flags,
                modseq,
            },
        );
        Ok(uid)
    }
}

fn not_found(folder: &str) -> RemoteError {
    RemoteError::new(RemoteErrorKind::NotFound, format!("no folder {folder}"))
}

/// Stand-in for Gmail's `X-GM-MSGID`: stable for the same raw bytes, so a
/// message keeps its id across labels and moves (FNV-1a, 63 bits so it fits
/// SQLite's signed integers).
pub fn fake_gm_msgid(raw: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in raw {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash >> 1
}

fn uid_list(uids: &[u32]) -> String {
    uids.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// A callback run when a matching call reaches the fake server.
type CallHook = Box<dyn FnOnce() + Send>;

/// Shared in-memory mail server implementing [`MailRemote`].
#[derive(Clone)]
pub struct FakeRemote {
    state: Arc<Mutex<ServerState>>,
    /// Signalled after every mutation (and on cancellation) to wake `idle`.
    changed: Arc<Condvar>,
    /// `(call prefix, hook)` pairs, see [`FakeRemote::on_next_call`].
    hooks: Arc<Mutex<Vec<(String, CallHook)>>>,
}

impl std::fmt::Debug for FakeRemote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeRemote")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl Default for FakeRemote {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeRemote {
    /// Empty server advertising IDLE, MOVE, UIDPLUS, CONDSTORE and
    /// SPECIAL-USE (not Gmail extensions).
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(ServerState {
                folders: Vec::new(),
                capabilities: Capabilities {
                    idle: true,
                    r#move: true,
                    uidplus: true,
                    condstore: true,
                    gmail: false,
                    special_use: true,
                },
                modseq: 0,
                next_uid_validity: 1,
                failures: VecDeque::new(),
                calls: Vec::new(),
                selected: None,
                logged_out: false,
            })),
            changed: Arc::new(Condvar::new()),
            hooks: Arc::default(),
        }
    }

    /// Runs `hook` once, the next time a call whose log line (see
    /// [`Self::calls`]) starts with `prefix` arrives — before the call takes
    /// effect and without holding the server lock. Lets tests act as
    /// another party (the UI, another client) while the engine is "on the
    /// network".
    pub fn on_next_call(&self, prefix: &str, hook: impl FnOnce() + Send + 'static) {
        self.hooks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((prefix.to_string(), Box::new(hook)));
    }

    /// Takes the first hook registered for `call`, if any.
    fn take_hook(&self, call: &str) -> Option<CallHook> {
        let mut hooks = self.hooks.lock().unwrap_or_else(|e| e.into_inner());
        let index = hooks
            .iter()
            .position(|(prefix, _)| call.starts_with(prefix.as_str()))?;
        Some(hooks.remove(index).1)
    }

    /// Wakes `idle` waiters; called after every mutation.
    fn notify(&self) {
        self.changed.notify_all();
    }

    fn lock(&self) -> MutexGuard<'_, ServerState> {
        // A panicking test thread must not cascade into every other handle.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Runs a matching hook, records the call, then fails with the next
    /// queued error if any.
    fn enter(&self, call: String) -> RemoteResult<MutexGuard<'_, ServerState>> {
        if let Some(hook) = self.take_hook(&call) {
            hook();
        }
        let mut state = self.lock();
        state.calls.push(call);
        match state.failures.pop_front() {
            Some(kind) => Err(RemoteError::new(kind, "injected failure")),
            None => Ok(state),
        }
    }

    pub fn set_capabilities(&self, capabilities: Capabilities) {
        self.lock().capabilities = capabilities;
    }

    /// Creates `name` (no-op if it exists). `attributes` are LIST flags such
    /// as `\Sent`; returns the folder's UIDVALIDITY.
    /// Removes a folder, as if deleted elsewhere; `false` if there was none.
    pub fn remove_folder(&self, name: &str) -> bool {
        let mut state = self.lock();
        let before = state.folders.len();
        state.folders.retain(|folder| folder.name != name);
        state.folders.len() != before
    }

    pub fn add_folder(&self, name: &str, attributes: &[&str]) -> u32 {
        let mut state = self.lock();
        if let Ok(folder) = state.folder(name) {
            return folder.uid_validity;
        }
        let uid_validity = state.next_uid_validity;
        state.next_uid_validity += 1;
        state.folders.push(FakeFolder {
            name: name.to_string(),
            attributes: attributes.iter().map(|a| a.to_string()).collect(),
            uid_validity,
            uid_next: 1,
            messages: BTreeMap::new(),
            version: 0,
        });
        uid_validity
    }

    /// Appends a message server-side (as if mail arrived) and returns its UID.
    ///
    /// # Panics
    /// If `folder` does not exist (test setup error).
    pub fn deliver(&self, folder: &str, raw: &[u8], flags: MessageFlags) -> u32 {
        let uid = self
            .lock()
            .insert(folder, raw.to_vec(), flags)
            .unwrap_or_else(|e| panic!("deliver: {e}"));
        self.notify();
        uid
    }

    /// Replaces a message's flags (another client changed them). Returns false
    /// if the message does not exist.
    pub fn set_flags(&self, folder: &str, uid: u32, flags: MessageFlags) -> bool {
        let mut state = self.lock();
        let modseq = state.bump_modseq();
        let Ok(folder) = state.folder_mut(folder) else {
            return false;
        };
        let found = match folder.messages.get_mut(&uid) {
            Some(message) => {
                message.flags = flags;
                message.modseq = modseq;
                true
            }
            None => false,
        };
        if found {
            folder.version += 1;
            drop(state);
            self.notify();
        }
        found
    }

    /// Removes a message server-side (another client expunged it).
    pub fn expunge(&self, folder: &str, uid: u32) -> bool {
        let mut state = self.lock();
        let removed = state.folder_mut(folder).is_ok_and(|f| {
            let removed = f.messages.remove(&uid).is_some();
            f.version += u64::from(removed);
            removed
        });
        drop(state);
        self.notify();
        removed
    }

    /// Simulates a UIDVALIDITY change: new validity and every message
    /// renumbered from UID 1, so any cached UID is now meaningless.
    pub fn reset_uid_validity(&self, folder: &str) -> u32 {
        let mut state = self.lock();
        let uid_validity = state.next_uid_validity;
        state.next_uid_validity += 1;
        let folder = state
            .folder_mut(folder)
            .unwrap_or_else(|e| panic!("reset_uid_validity: {e}"));
        let messages = std::mem::take(&mut folder.messages);
        folder.uid_validity = uid_validity;
        folder.uid_next = 1;
        folder.version += 1;
        for (_, mut message) in messages {
            message.uid = folder.uid_next;
            folder.messages.insert(folder.uid_next, message);
            folder.uid_next += 1;
        }
        drop(state);
        self.notify();
        uid_validity
    }

    /// Snapshot of a folder's messages, ascending by UID (empty if missing).
    pub fn messages(&self, folder: &str) -> Vec<FakeMessage> {
        self.lock()
            .folder(folder)
            .map(|f| f.messages.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Names of all folders, in creation order.
    pub fn folder_names(&self) -> Vec<String> {
        self.lock().folders.iter().map(|f| f.name.clone()).collect()
    }

    /// Makes the next fallible `MailRemote` call fail with `kind`. Calls
    /// queue, so N calls fail the next N operations.
    pub fn fail_next(&self, kind: RemoteErrorKind) {
        self.lock().failures.push_back(kind);
    }

    /// Log of `MailRemote` calls, e.g. `"fetch_headers INBOX 1,2"`.
    pub fn calls(&self) -> Vec<String> {
        self.lock().calls.clone()
    }

    pub fn clear_calls(&self) {
        self.lock().calls.clear();
    }

    /// Folder chosen by the last successful `select`.
    pub fn selected(&self) -> Option<String> {
        self.lock().selected.clone()
    }

    pub fn is_logged_out(&self) -> bool {
        self.lock().logged_out
    }
}

/// Header block of a raw message (through the blank separator line).
fn header_block(raw: &[u8]) -> Vec<u8> {
    let end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
        .or_else(|| raw.windows(2).position(|w| w == b"\n\n").map(|p| p + 2))
        .unwrap_or(raw.len());
    raw[..end].to_vec()
}

fn apply(flags: &mut MessageFlags, mask: MessageFlags, value: bool) {
    if mask.seen {
        flags.seen = value;
    }
    if mask.flagged {
        flags.flagged = value;
    }
    if mask.answered {
        flags.answered = value;
    }
    if mask.draft {
        flags.draft = value;
    }
    if mask.deleted {
        flags.deleted = value;
    }
}

impl MailRemote for FakeRemote {
    fn capabilities(&self) -> Capabilities {
        self.lock().capabilities
    }

    fn list_folders(&mut self) -> RemoteResult<Vec<RemoteFolder>> {
        let state = self.enter("list_folders".into())?;
        Ok(state
            .folders
            .iter()
            .map(|f| RemoteFolder::new(f.name.clone(), Some(FAKE_DELIMITER), f.attributes.clone()))
            .collect())
    }

    fn select(&mut self, folder: &str) -> RemoteResult<FolderStatus> {
        let mut state = self.enter(format!("select {folder}"))?;
        let condstore = state.capabilities.condstore;
        let f = state.folder(folder)?;
        let status = FolderStatus {
            uid_validity: f.uid_validity,
            uid_next: Some(f.uid_next),
            exists: f.messages.len() as u32,
            highest_modseq: condstore
                .then(|| f.messages.values().map(|m| m.modseq).max().unwrap_or(0)),
        };
        state.selected = Some(folder.to_string());
        Ok(status)
    }

    fn uids(&mut self, folder: &str) -> RemoteResult<Vec<u32>> {
        let state = self.enter(format!("uids {folder}"))?;
        Ok(state.folder(folder)?.messages.keys().copied().collect())
    }

    fn fetch_headers_plain(
        &mut self,
        folder: &str,
        uids: &[u32],
    ) -> RemoteResult<Vec<RemoteHeader>> {
        drop(self.enter(format!("fetch_headers_plain {folder} {}", uid_list(uids)))?);
        self.fetch_headers(folder, uids)
    }

    fn fetch_headers(&mut self, folder: &str, uids: &[u32]) -> RemoteResult<Vec<RemoteHeader>> {
        let state = self.enter(format!("fetch_headers {folder} {}", uid_list(uids)))?;
        let condstore = state.capabilities.condstore;
        let gmail = state.capabilities.gmail;
        let f = state.folder(folder)?;
        Ok(uids
            .iter()
            .filter_map(|uid| f.messages.get(uid))
            .map(|m| RemoteHeader {
                uid: m.uid,
                flags: m.flags,
                size: m.raw.len() as u32,
                gm_msgid: gmail.then(|| fake_gm_msgid(&m.raw)),
                gm_thrid: None,
                modseq: condstore.then_some(m.modseq),
                header: header_block(&m.raw),
                has_attachments: parse_message(&m.raw).map(|p| has_attachments(&p)),
            })
            .collect())
    }

    fn fetch_flags(
        &mut self,
        folder: &str,
        uids: &[u32],
        changed_since: Option<u64>,
    ) -> RemoteResult<Vec<RemoteFlags>> {
        let call = match changed_since {
            Some(since) => format!("fetch_flags {folder} {} since {since}", uid_list(uids)),
            None => format!("fetch_flags {folder} {}", uid_list(uids)),
        };
        let state = self.enter(call)?;
        let condstore = state.capabilities.condstore;
        let f = state.folder(folder)?;
        Ok(uids
            .iter()
            .filter_map(|uid| f.messages.get(uid))
            .filter(|m| changed_since.is_none_or(|since| m.modseq > since))
            .map(|m| RemoteFlags {
                uid: m.uid,
                flags: m.flags,
                modseq: condstore.then_some(m.modseq),
            })
            .collect())
    }

    /// Matches the parsed `Message-ID` of every message (brackets ignored).
    fn find_uid_by_message_id(
        &mut self,
        folder: &str,
        message_id: &str,
    ) -> RemoteResult<Option<u32>> {
        let mut state = self.enter(format!("find_uid {folder} {message_id}"))?;
        let wanted = message_id
            .trim()
            .trim_start_matches('<')
            .trim_end_matches('>');
        if wanted.is_empty() {
            return Ok(None);
        }
        let found = state
            .folder(folder)?
            .messages
            .values()
            .filter(|m| {
                parse_message(&header_block(&m.raw))
                    .and_then(|p| p.message_id)
                    .is_some_and(|id| id.trim_start_matches('<').trim_end_matches('>') == wanted)
            })
            .map(|m| m.uid)
            .max();
        state.selected = Some(folder.to_string());
        Ok(found)
    }

    fn fetch_body(&mut self, folder: &str, uid: u32) -> RemoteResult<Vec<u8>> {
        let state = self.enter(format!("fetch_body {folder} {uid}"))?;
        state
            .folder(folder)?
            .messages
            .get(&uid)
            .map(|m| m.raw.clone())
            .ok_or_else(|| RemoteError::new(RemoteErrorKind::NotFound, format!("no uid {uid}")))
    }

    fn store_flags(
        &mut self,
        folder: &str,
        uids: &[u32],
        add: MessageFlags,
        remove: MessageFlags,
    ) -> RemoteResult<()> {
        let mut state = self.enter(format!(
            "store_flags {folder} {} +{} -{}",
            uid_list(uids),
            add.to_imap().join(" "),
            remove.to_imap().join(" ")
        ))?;
        state.folder(folder)?;
        let modseq = state.bump_modseq();
        let f = state.folder_mut(folder)?;
        for uid in uids {
            if let Some(m) = f.messages.get_mut(uid) {
                apply(&mut m.flags, add, true);
                apply(&mut m.flags, remove, false);
                m.modseq = modseq;
            }
        }
        f.version += 1;
        drop(state);
        self.notify();
        Ok(())
    }

    fn move_messages(
        &mut self,
        from: &str,
        uids: &[u32],
        to: &str,
    ) -> RemoteResult<Option<Vec<u32>>> {
        let mut state = self.enter(format!("move_messages {from} {} {to}", uid_list(uids)))?;
        state.folder(to)?;
        let uidplus = state.capabilities.uidplus;
        let moved: Vec<FakeMessage> = {
            let source = state.folder_mut(from)?;
            source.version += 1;
            uids.iter()
                .filter_map(|uid| source.messages.remove(uid))
                .collect()
        };
        let mut new_uids = Vec::with_capacity(moved.len());
        for message in moved {
            new_uids.push(state.insert(to, message.raw, message.flags)?);
        }
        drop(state);
        self.notify();
        Ok(uidplus.then_some(new_uids))
    }

    fn delete_messages(&mut self, folder: &str, uids: &[u32]) -> RemoteResult<()> {
        let mut state = self.enter(format!("delete_messages {folder} {}", uid_list(uids)))?;
        let f = state.folder_mut(folder)?;
        for uid in uids {
            f.messages.remove(uid);
        }
        f.version += 1;
        drop(state);
        self.notify();
        Ok(())
    }

    fn append(&mut self, folder: &str, flags: MessageFlags, raw: &[u8]) -> RemoteResult<()> {
        let mut state = self.enter(format!("append {folder}"))?;
        state.insert(folder, raw.to_vec(), flags)?;
        drop(state);
        self.notify();
        Ok(())
    }

    fn create_folder(&mut self, name: &str) -> RemoteResult<()> {
        let state = self.enter(format!("create_folder {name}"))?;
        if state.folder(name).is_ok() {
            return Err(RemoteError::new(
                RemoteErrorKind::Protocol,
                format!("folder {name} already exists"),
            ));
        }
        drop(state);
        self.add_folder(name, &[]);
        Ok(())
    }

    fn noop(&mut self) -> RemoteResult<()> {
        self.enter("noop".into()).map(|_| ())
    }

    fn logout(&mut self) {
        let mut state = self.lock();
        state.calls.push("logout".into());
        state.logged_out = true;
    }

    /// Blocks until something changes in `folder` (any mutation through any
    /// handle), `timeout` passes or `cancel` fires (→ `Stopped`). Without the
    /// IDLE capability returns `Unsupported`, like a real server.
    fn idle(
        &mut self,
        folder: &str,
        timeout: Duration,
        cancel: &CancelToken,
    ) -> RemoteResult<IdleOutcome> {
        let start = {
            let state = self.enter(format!("idle {folder}"))?;
            if !state.capabilities.idle {
                return Err(RemoteError::new(
                    RemoteErrorKind::Unsupported,
                    "IDLE is not supported",
                ));
            }
            state.folder(folder)?.version
        };
        // Taking the lock before notifying means a waiter can never miss the
        // wake-up between its `is_cancelled` check and `wait_timeout`.
        let (state_ref, changed) = (self.state.clone(), self.changed.clone());
        let _hook = cancel.on_cancel(move || {
            let _state = state_ref.lock().unwrap_or_else(|e| e.into_inner());
            changed.notify_all();
        });
        let deadline = Instant::now() + timeout;
        let mut state = self.lock();
        loop {
            if cancel.is_cancelled() {
                return Ok(IdleOutcome::Stopped);
            }
            if state.folder(folder).map(|f| f.version).ok() != Some(start) {
                return Ok(IdleOutcome::Changed);
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(IdleOutcome::Timeout);
            }
            state = self
                .changed
                .wait_timeout(state, deadline - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
}

/// A message handed to [`FakeSender::send`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SentMail {
    pub from: String,
    pub recipients: Vec<String>,
    pub raw: Vec<u8>,
}

/// A queued `FakeSender` failure; `delivered` records the mail first (the
/// server accepted it before the connection broke).
#[derive(Debug)]
struct SendFault {
    error: SendError,
    delivered: bool,
}

#[derive(Debug, Default)]
struct SenderState {
    sent: Vec<SentMail>,
    failures: VecDeque<SendFault>,
    sends: usize,
}

/// Records submissions instead of sending them; clones share the log.
#[derive(Clone, Debug, Default)]
pub struct FakeSender {
    state: Arc<Mutex<SenderState>>,
}

impl FakeSender {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, SenderState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Everything successfully "sent", oldest first.
    pub fn sent(&self) -> Vec<SentMail> {
        self.lock().sent.clone()
    }

    /// Makes the next `send` fail with `kind`, classified like an error
    /// raised before the message data (`SendError::from`). Queued like
    /// `FakeRemote`.
    pub fn fail_next(&self, kind: RemoteErrorKind) {
        self.fail_next_with(SendError::from(RemoteError::new(kind, "injected failure")));
    }

    /// Makes the next `send` fail with exactly `error` (nothing recorded).
    pub fn fail_next_with(&self, error: SendError) {
        self.lock().failures.push_back(SendFault {
            error,
            delivered: false,
        });
    }

    /// The next `send` is accepted (recorded in `sent`) but reports an
    /// [`crate::remote::SendFailure::Uncertain`] network error, like a
    /// connection that broke while waiting for the final reply.
    pub fn deliver_then_fail_next(&self) {
        self.lock().failures.push_back(SendFault {
            error: SendError::uncertain(RemoteError::new(
                RemoteErrorKind::Network,
                "connection reset after DATA",
            )),
            delivered: true,
        });
    }

    /// `send` calls so far (successful or not).
    pub fn sends(&self) -> usize {
        self.lock().sends
    }
}

impl MailSender for FakeSender {
    fn send(&mut self, from: &str, recipients: &[String], raw: &[u8]) -> Result<(), SendError> {
        let mut state = self.lock();
        state.sends += 1;
        let fault = state.failures.pop_front();
        if fault.as_ref().is_none_or(|fault| fault.delivered) {
            state.sent.push(SentMail {
                from: from.to_string(),
                recipients: recipients.to_vec(),
                raw: raw.to_vec(),
            });
        }
        match fault {
            Some(fault) => Err(fault.error),
            None => Ok(()),
        }
    }
}

#[derive(Debug, Default)]
struct ConnectorState {
    failures: VecDeque<RemoteErrorKind>,
    persistent_failure: Option<RemoteErrorKind>,
    connects: usize,
    sender_connects: usize,
    saves_sent_copy: bool,
}

/// [`MailConnector`] handing out clones of one [`FakeRemote`] /
/// [`FakeSender`] pair (so every connection sees the same server). Clones
/// share connection counters and failure injection.
#[derive(Clone, Debug, Default)]
pub struct FakeConnector {
    remote: FakeRemote,
    sender: FakeSender,
    state: Arc<Mutex<ConnectorState>>,
}

impl FakeConnector {
    pub fn new(remote: FakeRemote, sender: FakeSender) -> Self {
        Self {
            remote,
            sender,
            state: Arc::default(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, ConnectorState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The shared server handle (mutate it to simulate other clients).
    pub fn remote(&self) -> FakeRemote {
        self.remote.clone()
    }

    /// The shared submission log.
    pub fn sender(&self) -> FakeSender {
        self.sender.clone()
    }

    /// Makes the next `connect` / `connect_sender` fail with `kind` (FIFO).
    pub fn fail_next_connect(&self, kind: RemoteErrorKind) {
        self.lock().failures.push_back(kind);
    }

    /// Makes every connection attempt fail with `kind` until cleared with
    /// `None` (server down, password revoked).
    pub fn set_connect_failure(&self, kind: Option<RemoteErrorKind>) {
        self.lock().persistent_failure = kind;
    }

    /// Whether the "server" files sent mail itself (Gmail behaviour).
    pub fn set_saves_sent_copy(&self, saves: bool) {
        self.lock().saves_sent_copy = saves;
    }

    /// Successful `connect` calls so far.
    pub fn connects(&self) -> usize {
        self.lock().connects
    }

    /// Successful `connect_sender` calls so far.
    pub fn sender_connects(&self) -> usize {
        self.lock().sender_connects
    }

    /// Cancellation, then queued failures, then the persistent failure.
    fn check(&self, cancel: &CancelToken) -> RemoteResult<MutexGuard<'_, ConnectorState>> {
        if cancel.is_cancelled() {
            return Err(RemoteError::new(RemoteErrorKind::Cancelled, "cancelled"));
        }
        let mut state = self.lock();
        match state.failures.pop_front().or(state.persistent_failure) {
            Some(kind) => Err(RemoteError::new(kind, "injected connect failure")),
            None => Ok(state),
        }
    }
}

impl MailConnector for FakeConnector {
    fn connect(&self, cancel: &CancelToken) -> RemoteResult<Box<dyn MailRemote>> {
        self.check(cancel)?.connects += 1;
        Ok(Box::new(self.remote.clone()))
    }

    fn connect_sender(&self, cancel: &CancelToken) -> RemoteResult<Box<dyn MailSender>> {
        self.check(cancel)?.sender_connects += 1;
        Ok(Box::new(self.sender.clone()))
    }

    fn saves_sent_copy(&self) -> bool {
        self.lock().saves_sent_copy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folder::FolderRole;

    const MAIL: &[u8] = b"From: a@x\r\nSubject: Hi\r\n\r\nBody\r\n";

    fn seen() -> MessageFlags {
        MessageFlags {
            seen: true,
            ..Default::default()
        }
    }

    fn server() -> FakeRemote {
        let server = FakeRemote::new();
        server.add_folder("INBOX", &[]);
        server.add_folder("Sent", &["\\Sent"]);
        server
    }

    #[test]
    fn clones_share_state() {
        let server = server();
        let mut conn: Box<dyn MailRemote> = Box::new(server.clone());
        assert_eq!(server.deliver("INBOX", MAIL, seen()), 1);
        assert_eq!(conn.uids("INBOX").unwrap(), vec![1]);
        assert_eq!(server.calls(), vec!["uids INBOX"]);
        assert!(FakeRemote::default().folder_names().is_empty());
    }

    #[test]
    fn list_and_select() {
        let mut conn = server();
        assert_eq!(conn.add_folder("INBOX", &[]), 1, "existing folder kept");
        let folders = conn.list_folders().unwrap();
        assert_eq!(folders.len(), 2);
        assert_eq!(folders[0].role, Some(FolderRole::Inbox));
        assert_eq!(folders[1].role, Some(FolderRole::Sent));
        assert_eq!(folders[1].delimiter.as_deref(), Some(FAKE_DELIMITER));
        conn.deliver("INBOX", MAIL, MessageFlags::default());
        let status = conn.select("INBOX").unwrap();
        assert_eq!(
            status,
            FolderStatus {
                uid_validity: 1,
                uid_next: Some(2),
                exists: 1,
                highest_modseq: Some(1),
            }
        );
        assert_eq!(conn.selected().as_deref(), Some("INBOX"));
        assert_eq!(
            conn.select("Nope").unwrap_err().kind,
            RemoteErrorKind::NotFound
        );
        assert_eq!(conn.folder_names(), vec!["INBOX", "Sent"]);
    }

    #[test]
    fn headers_flags_and_body() {
        let mut conn = server();
        let uid = conn.deliver("INBOX", MAIL, seen());
        let headers = conn.fetch_headers("INBOX", &[uid, 99]).unwrap();
        assert_eq!(headers.len(), 1, "unknown uids are skipped");
        assert_eq!(headers[0].header, b"From: a@x\r\nSubject: Hi\r\n\r\n");
        assert_eq!(headers[0].size as usize, MAIL.len());
        assert_eq!(headers[0].has_attachments, Some(false));
        assert!(headers[0].flags.seen);
        assert_eq!(conn.fetch_body("INBOX", uid).unwrap(), MAIL);
        assert_eq!(
            conn.fetch_body("INBOX", 42).unwrap_err().kind,
            RemoteErrorKind::NotFound
        );
        assert!(conn.messages("INBOX")[0].flags.seen, "body fetch is a peek");
    }

    #[test]
    fn flag_changes_bump_modseq_and_filter() {
        let mut conn = server();
        let a = conn.deliver("INBOX", MAIL, MessageFlags::default());
        let b = conn.deliver("INBOX", MAIL, MessageFlags::default());
        let base = conn.select("INBOX").unwrap().highest_modseq.unwrap();
        assert!(conn.set_flags("INBOX", b, seen()));
        assert!(!conn.set_flags("INBOX", 99, seen()));
        assert!(!conn.set_flags("Nope", b, seen()));
        let changed = conn.fetch_flags("INBOX", &[a, b], Some(base)).unwrap();
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].uid, b);
        assert!(changed[0].flags.seen);
        assert_eq!(conn.fetch_flags("INBOX", &[a, b], None).unwrap().len(), 2);
    }

    #[test]
    fn store_flags_applies_only_true_fields() {
        let mut conn = server();
        let uid = conn.deliver(
            "INBOX",
            MAIL,
            MessageFlags {
                seen: true,
                answered: true,
                ..Default::default()
            },
        );
        let add = MessageFlags {
            flagged: true,
            ..Default::default()
        };
        conn.store_flags("INBOX", &[uid], add, seen()).unwrap();
        let flags = conn.messages("INBOX")[0].flags;
        assert!(flags.flagged && flags.answered && !flags.seen);
        assert_eq!(
            conn.calls().last().unwrap(),
            "store_flags INBOX 1 +\\Flagged -\\Seen"
        );
    }

    #[test]
    fn move_delete_append_create() {
        let mut conn = server();
        let a = conn.deliver("INBOX", MAIL, seen());
        let b = conn.deliver("INBOX", MAIL, MessageFlags::default());
        conn.deliver("Sent", MAIL, MessageFlags::default());
        assert_eq!(
            conn.move_messages("INBOX", &[a, b], "Sent").unwrap(),
            Some(vec![2, 3])
        );
        assert!(conn.messages("INBOX").is_empty());
        assert!(conn.messages("Sent")[1].flags.seen);
        conn.set_capabilities(Capabilities::default());
        assert!(!conn.capabilities().uidplus);
        conn.deliver("INBOX", MAIL, MessageFlags::default());
        assert_eq!(conn.move_messages("INBOX", &[3], "Sent").unwrap(), None);
        assert!(conn.move_messages("INBOX", &[1], "Nope").is_err());
        conn.delete_messages("Sent", &[1, 2]).unwrap();
        assert_eq!(conn.uids("Sent").unwrap(), vec![3, 4]);
        conn.append("Sent", seen(), MAIL).unwrap();
        assert_eq!(conn.uids("Sent").unwrap(), vec![3, 4, 5]);
        conn.create_folder("Work").unwrap();
        assert_eq!(
            conn.create_folder("Work").unwrap_err().kind,
            RemoteErrorKind::Protocol
        );
        assert!(conn.append("Nope", seen(), MAIL).is_err());
    }

    #[test]
    fn server_side_expunge_and_uid_validity_reset() {
        let mut conn = server();
        conn.deliver("INBOX", MAIL, MessageFlags::default());
        conn.deliver("INBOX", MAIL, MessageFlags::default());
        conn.deliver("INBOX", MAIL, MessageFlags::default());
        assert!(conn.expunge("INBOX", 1));
        assert!(!conn.expunge("INBOX", 1));
        let old = conn.select("INBOX").unwrap().uid_validity;
        let new = conn.reset_uid_validity("INBOX");
        assert_ne!(old, new);
        assert_eq!(conn.uids("INBOX").unwrap(), vec![1, 2]);
        let status = conn.select("INBOX").unwrap();
        assert_eq!(status.uid_validity, new);
        assert_eq!(status.uid_next, Some(3));
        assert!(conn.messages("Nope").is_empty());
    }

    #[test]
    fn failure_injection_and_call_log() {
        let mut conn = server();
        conn.fail_next(RemoteErrorKind::Network);
        conn.fail_next(RemoteErrorKind::Auth);
        assert_eq!(conn.noop().unwrap_err().kind, RemoteErrorKind::Network);
        assert_eq!(conn.list_folders().unwrap_err().kind, RemoteErrorKind::Auth);
        conn.noop().unwrap();
        conn.fetch_flags("INBOX", &[1, 2], Some(7)).unwrap();
        conn.logout();
        assert!(conn.is_logged_out());
        assert_eq!(
            conn.calls(),
            vec![
                "noop",
                "list_folders",
                "noop",
                "fetch_flags INBOX 1,2 since 7",
                "logout"
            ]
        );
        conn.clear_calls();
        assert!(conn.calls().is_empty());
    }

    #[test]
    fn header_block_variants() {
        assert_eq!(header_block(b"A: b\n\nbody"), b"A: b\n\n");
        assert_eq!(header_block(b"A: b\r\n"), b"A: b\r\n");
    }

    #[test]
    fn gmail_mode_reports_stable_gm_msgid() {
        let mut conn = server();
        let uid = conn.deliver("INBOX", MAIL, MessageFlags::default());
        assert_eq!(
            conn.fetch_headers("INBOX", &[uid]).unwrap()[0].gm_msgid,
            None
        );
        conn.set_capabilities(Capabilities {
            gmail: true,
            ..conn.capabilities()
        });
        let first = conn.fetch_headers("INBOX", &[uid]).unwrap()[0].gm_msgid;
        assert_eq!(first, Some(fake_gm_msgid(MAIL)));
        let copy = conn.deliver("Sent", MAIL, MessageFlags::default());
        assert_eq!(
            conn.fetch_headers("Sent", &[copy]).unwrap()[0].gm_msgid,
            first
        );
        assert_ne!(fake_gm_msgid(b"other"), fake_gm_msgid(MAIL));
        assert!(fake_gm_msgid(MAIL) <= i64::MAX as u64);
    }

    #[test]
    fn idle_wakes_on_change_in_its_folder_only() {
        let server = server();
        let mut conn = server.clone();
        let cancel = CancelToken::new();
        let handle = std::thread::spawn(move || {
            conn.idle("INBOX", Duration::from_secs(10), &cancel)
                .unwrap()
        });
        // Wait until the idle call is registered, then poke another folder
        // (ignored) and the idled one (wakes it).
        while !server.calls().iter().any(|c| c == "idle INBOX") {
            std::thread::yield_now();
        }
        server.deliver("Sent", MAIL, MessageFlags::default());
        server.deliver("INBOX", MAIL, MessageFlags::default());
        assert_eq!(handle.join().unwrap(), IdleOutcome::Changed);
    }

    #[test]
    fn idle_times_out_stops_on_cancel_and_needs_capability() {
        let mut conn = server();
        let cancel = CancelToken::new();
        assert_eq!(
            conn.idle("INBOX", Duration::from_millis(5), &cancel)
                .unwrap(),
            IdleOutcome::Timeout
        );
        let mut waiting = conn.clone();
        let token = cancel.clone();
        let started = Instant::now();
        let handle = std::thread::spawn(move || {
            waiting
                .idle("INBOX", Duration::from_secs(30), &token)
                .unwrap()
        });
        std::thread::sleep(Duration::from_millis(20));
        cancel.cancel();
        assert_eq!(handle.join().unwrap(), IdleOutcome::Stopped);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(
            conn.idle("INBOX", Duration::from_secs(1), &cancel).unwrap(),
            IdleOutcome::Stopped,
            "already cancelled"
        );
        assert_eq!(
            conn.idle("Nope", Duration::from_secs(1), &CancelToken::new())
                .unwrap_err()
                .kind,
            RemoteErrorKind::NotFound
        );
        conn.set_capabilities(Capabilities::default());
        assert_eq!(
            conn.idle("INBOX", Duration::from_secs(1), &CancelToken::new())
                .unwrap_err()
                .kind,
            RemoteErrorKind::Unsupported
        );
    }

    #[test]
    fn mutations_bump_folder_version() {
        let mut conn = server();
        let version = |c: &FakeRemote| c.lock().folder("INBOX").unwrap().version;
        let mut last = version(&conn);
        let mut step = |c: &FakeRemote, what: &str| {
            let now = version(c);
            assert!(now > last, "{what} must bump the version");
            last = now;
        };
        let uid = conn.deliver("INBOX", MAIL, MessageFlags::default());
        step(&conn, "deliver");
        conn.set_flags("INBOX", uid, seen());
        step(&conn, "set_flags");
        conn.store_flags("INBOX", &[uid], seen(), MessageFlags::default())
            .unwrap();
        step(&conn, "store_flags");
        conn.append("INBOX", seen(), MAIL).unwrap();
        step(&conn, "append");
        conn.move_messages("INBOX", &[uid], "Sent").unwrap();
        step(&conn, "move");
        conn.expunge("INBOX", 2);
        step(&conn, "expunge");
        conn.deliver("INBOX", MAIL, MessageFlags::default());
        step(&conn, "deliver");
        conn.delete_messages("INBOX", &[3]).unwrap();
        step(&conn, "delete");
        conn.reset_uid_validity("INBOX");
        step(&conn, "reset");
    }

    #[test]
    fn connector_hands_out_shared_connections_and_injects_failures() {
        let connector = FakeConnector::new(server(), FakeSender::new());
        let cancel = CancelToken::new();
        let mut conn = connector.connect(&cancel).unwrap();
        connector.remote().deliver("INBOX", MAIL, seen());
        assert_eq!(conn.uids("INBOX").unwrap(), vec![1]);
        let mut sender = connector.connect_sender(&cancel).unwrap();
        sender.send("a@x", &["b@x".into()], MAIL).unwrap();
        assert_eq!(connector.sender().sent().len(), 1);
        assert_eq!((connector.connects(), connector.sender_connects()), (1, 1));

        connector.fail_next_connect(RemoteErrorKind::Auth);
        assert_eq!(
            connector.connect(&cancel).err().unwrap().kind,
            RemoteErrorKind::Auth
        );
        connector.set_connect_failure(Some(RemoteErrorKind::Network));
        for _ in 0..2 {
            assert_eq!(
                connector.connect_sender(&cancel).err().unwrap().kind,
                RemoteErrorKind::Network
            );
        }
        connector.set_connect_failure(None);
        assert!(connector.clone().connect(&cancel).is_ok());
        assert_eq!(connector.connects(), 2);

        assert!(!connector.saves_sent_copy());
        connector.set_saves_sent_copy(true);
        assert!(connector.saves_sent_copy());

        cancel.cancel();
        assert_eq!(
            connector.connect(&cancel).err().unwrap().kind,
            RemoteErrorKind::Cancelled
        );
        assert!(FakeConnector::default().remote().folder_names().is_empty());
    }

    #[test]
    fn sender_records_and_fails() {
        let sender = FakeSender::new();
        let mut conn: Box<dyn MailSender> = Box::new(sender.clone());
        sender.fail_next(RemoteErrorKind::Auth);
        let to = vec!["b@x".to_string()];
        let err = conn.send("a@x", &to, MAIL).unwrap_err();
        assert_eq!(err.error.kind, RemoteErrorKind::Auth);
        assert_eq!(err.failure, crate::remote::SendFailure::Transient);
        conn.send("a@x", &to, MAIL).unwrap();
        assert_eq!(
            sender.sent(),
            vec![SentMail {
                from: "a@x".into(),
                recipients: to.clone(),
                raw: MAIL.to_vec(),
            }]
        );

        sender.fail_next_with(SendError::permanent(RemoteError::new(
            RemoteErrorKind::Protocol,
            "550",
        )));
        let err = conn.send("a@x", &to, MAIL).unwrap_err();
        assert_eq!(err.failure, crate::remote::SendFailure::Permanent);
        assert_eq!(sender.sent().len(), 1, "rejected mail is not recorded");

        sender.deliver_then_fail_next();
        let err = conn.send("a@x", &to, MAIL).unwrap_err();
        assert_eq!(err.failure, crate::remote::SendFailure::Uncertain);
        assert_eq!(sender.sent().len(), 2, "accepted before the failure");
        assert_eq!(sender.sends(), 4);
    }

    #[test]
    fn hooks_run_once_before_matching_calls() {
        let server = server();
        let hits = Arc::new(Mutex::new(Vec::new()));
        let log = hits.clone();
        let handle = server.clone();
        server.on_next_call("uids INBOX", move || {
            // The server lock is free: the hook may act as another client.
            handle.deliver("INBOX", MAIL, MessageFlags::default());
            log.lock().unwrap().push("hook");
        });
        let mut remote: Box<dyn MailRemote> = Box::new(server.clone());
        remote.select("INBOX").unwrap();
        assert!(hits.lock().unwrap().is_empty(), "prefix did not match");
        let uids = remote.uids("INBOX").unwrap();
        assert_eq!(uids.len(), 1, "hook ran before the call took effect");
        remote.uids("INBOX").unwrap();
        assert_eq!(*hits.lock().unwrap(), vec!["hook"]);
        assert!(format!("{server:?}").contains("FakeRemote"));
    }

    #[test]
    fn find_uid_by_message_id_matches_parsed_header() {
        let server = FakeRemote::new();
        server.add_folder("INBOX", &[]);
        let raw = |id: &str| format!("Message-ID: <{id}>\r\nSubject: s\r\n\r\nb\r\n").into_bytes();
        server.deliver("INBOX", &raw("a@x"), MessageFlags::default());
        server.deliver("INBOX", &raw("b@x"), MessageFlags::default());
        let second_a = server.deliver("INBOX", &raw("a@x"), MessageFlags::default());
        let mut remote: Box<dyn MailRemote> = Box::new(server.clone());
        assert_eq!(
            remote.find_uid_by_message_id("INBOX", "a@x").unwrap(),
            Some(second_a)
        );
        assert_eq!(
            remote.find_uid_by_message_id("INBOX", "<b@x>").unwrap(),
            Some(2)
        );
        assert_eq!(remote.find_uid_by_message_id("INBOX", "c@x").unwrap(), None);
        assert_eq!(remote.find_uid_by_message_id("INBOX", " ").unwrap(), None);
        assert!(remote.find_uid_by_message_id("Nope", "a@x").is_err());
        assert!(server.calls().contains(&"find_uid INBOX a@x".to_string()));
    }
}
