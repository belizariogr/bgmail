//! End-to-end engine tests: a real worker thread against `FakeConnector` on a
//! temp database, observed through the event channel and a second ("UI")
//! connection.

use std::sync::Arc;
use std::time::{Duration, Instant};

use mail_core::fake::{FakeConnector, FakeRemote, FakeSender};
use mail_core::{
    AccountConfig, Capabilities, MailRemote, MessageFlags, RemoteErrorKind, ServerConfig,
};
use storage::{system, Database, MailListQuery, PROVIDER_GMAIL, PROVIDER_IMAP, PROVIDER_LOCAL};
use tempfile::NamedTempFile;

use crate::actions;
use crate::{AccountStatus, Command, Engine, EngineOptions, SyncEvent};

const WAIT: Duration = Duration::from_secs(10);

fn seen() -> MessageFlags {
    MessageFlags {
        seen: true,
        ..Default::default()
    }
}

fn unread() -> MessageFlags {
    MessageFlags::default()
}

/// A minimal message; `id` doubles as Message-ID and ordering key (date).
fn mail(subject: &str, id: u32) -> Vec<u8> {
    format!(
        "From: Ann <ann@example.com>\r\nTo: me@example.com\r\nSubject: {subject}\r\n\
Message-ID: <{subject}-{id}@example.com>\r\nDate: {}\r\n\r\nBody of {subject}\r\n",
        date(id)
    )
    .into_bytes()
}

fn date(n: u32) -> String {
    format!("Tue, 14 Nov 2023 {:02}:{:02}:00 +0000", n / 60 % 24, n % 60)
}

fn test_options() -> EngineOptions {
    EngineOptions {
        poll_interval: Duration::from_secs(3600),
        idle_timeout: Duration::ZERO,
        body_prefetch_inbox: 0,
        body_prefetch: 0,
        retry_base: Duration::from_millis(10),
        retry_max: Duration::from_millis(40),
        send_retry_delays: vec![Duration::ZERO],
        ..EngineOptions::default()
    }
}

struct Harness {
    _file: NamedTempFile,
    db: Database,
    account: i64,
    server: FakeRemote,
    connector: FakeConnector,
    engine: Engine,
    events: async_channel::Receiver<SyncEvent>,
}

impl Harness {
    /// Server with INBOX/Sent/Drafts/Archive/Trash/Junk; not started yet.
    fn new(provider: &str, config_json: &str, options: EngineOptions) -> Self {
        let file = NamedTempFile::new().unwrap();
        let db = Database::open(file.path()).unwrap();
        let account = db
            .create_account("Me", "me@example.com", provider, config_json)
            .unwrap();
        let server = FakeRemote::new();
        server.add_folder("INBOX", &[]);
        server.add_folder("Sent", &["\\Sent"]);
        server.add_folder("Drafts", &["\\Drafts"]);
        server.add_folder("Archive", &["\\Archive"]);
        server.add_folder("Trash", &["\\Trash"]);
        server.add_folder("Junk", &["\\Junk"]);
        let connector = FakeConnector::new(server.clone(), FakeSender::new());
        let (tx, events) = async_channel::unbounded();
        let engine = Engine::new(file.path().to_path_buf(), tx, options);
        Self {
            _file: file,
            db,
            account,
            server,
            connector,
            engine,
            events,
        }
    }

    fn imap() -> Self {
        Self::new(PROVIDER_IMAP, "{}", test_options())
    }

    fn start(&self) {
        self.engine
            .start_account(self.account, Arc::new(self.connector.clone()));
    }

    /// Starts and waits for the first full sync.
    fn started(self) -> Self {
        self.start();
        self.wait_idle();
        self
    }

    fn drain(&self) {
        while self.events.try_recv().is_ok() {}
    }

    /// Waits for an event matching `pred`, returning every event seen up to
    /// and including it.
    fn wait_for(&self, what: &str, pred: impl Fn(&SyncEvent) -> bool) -> Vec<SyncEvent> {
        let deadline = Instant::now() + WAIT;
        let mut seen = Vec::new();
        while Instant::now() < deadline {
            match self.events.try_recv() {
                Ok(event) => {
                    let done = pred(&event);
                    seen.push(event);
                    if done {
                        return seen;
                    }
                }
                Err(_) => std::thread::sleep(Duration::from_millis(1)),
            }
        }
        panic!("timed out waiting for {what}; got {seen:#?}");
    }

    fn wait_status(&self, what: &str, pred: impl Fn(&AccountStatus) -> bool) -> Vec<SyncEvent> {
        self.wait_for(
            what,
            |event| matches!(event, SyncEvent::Status { status, .. } if pred(status)),
        )
    }

    fn wait_idle(&self) -> Vec<SyncEvent> {
        self.wait_status("Idle", |s| matches!(s, AccountStatus::Idle { .. }))
    }

    /// Sends `command` and waits for the worker to be idle again.
    fn run(&self, command: Command) -> Vec<SyncEvent> {
        self.drain();
        assert!(self.engine.send(self.account, command));
        self.wait_idle()
    }

    fn sync(&self) -> Vec<SyncEvent> {
        self.run(Command::SyncNow)
    }

    fn folder_id(&self, path: &str) -> i64 {
        self.db
            .list_folders(self.account)
            .unwrap()
            .into_iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("no folder {path}"))
            .id
    }

    fn subjects(&self, path: &str) -> Vec<String> {
        let mut subjects: Vec<String> = self
            .db
            .list_messages(&MailListQuery::AccountFolder {
                account_id: self.account,
                folder_path: path.into(),
            })
            .unwrap()
            .into_iter()
            .map(|m| m.subject)
            .collect();
        subjects.sort();
        subjects
    }

    fn id_of(&self, subject: &str) -> i64 {
        self.db
            .conn()
            .query_row(
                "SELECT id FROM messages WHERE subject = ?1 AND account_id = ?2",
                (subject, self.account),
                |row| row.get(0),
            )
            .unwrap_or_else(|_| panic!("no message {subject}"))
    }

    fn message_count(&self) -> i64 {
        self.db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE account_id = ?1",
                [self.account],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn server_subjects(&self, folder: &str) -> Vec<String> {
        let mut subjects: Vec<String> = self
            .server
            .messages(folder)
            .iter()
            .filter_map(|m| mail_core::parse_message(&m.raw).map(|p| p.subject))
            .collect();
        subjects.sort();
        subjects
    }
}

fn new_mail(events: &[SyncEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            SyncEvent::NewMail { messages, .. } => Some(messages),
            _ => None,
        })
        .flatten()
        .map(|n| n.subject.clone())
        .collect()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn initial_sync_maps_folders_and_fills_lists_incrementally() {
    let h = Harness::imap();
    h.server.add_folder("Clients/Acme", &[]);
    h.server.add_folder("[Gmail]/Starred", &["\\Flagged"]);
    h.server.deliver("INBOX", &mail("one", 1), unread());
    h.server.deliver("INBOX", &mail("two", 2), seen());
    h.server.deliver("Sent", &mail("sent", 3), seen());
    h.server.deliver("Clients/Acme", &mail("acme", 4), unread());
    h.start();
    let events = h.wait_idle();

    assert!(matches!(
        &events[0],
        SyncEvent::Status {
            status: AccountStatus::Connecting,
            ..
        }
    ));
    assert!(events
        .iter()
        .any(|e| matches!(e, SyncEvent::FoldersChanged { .. })));
    assert!(events.iter().any(|e| matches!(
        e,
        SyncEvent::Status { status: AccountStatus::Syncing { folder, done: 2, total: 2 }, .. }
            if folder == system::INBOX
    )));
    assert!(events
        .iter()
        .any(|e| matches!(e, SyncEvent::MailboxChanged { .. })));
    assert!(new_mail(&events).is_empty(), "never on the initial sync");

    let folders = h.db.list_folders(h.account).unwrap();
    let mut paths: Vec<&str> = folders.iter().map(|f| f.path.as_str()).collect();
    paths.sort();
    assert_eq!(
        paths,
        vec![
            system::ARCHIVE,
            system::DRAFTS,
            system::INBOX,
            system::JUNK,
            system::SENT,
            system::TRASH,
            "user:Clients/Acme"
        ]
    );
    let acme = folders
        .iter()
        .find(|f| f.path == "user:Clients/Acme")
        .unwrap();
    assert_eq!(acme.display_name, "Clients/Acme");
    assert_eq!(acme.remote_name.as_deref(), Some("Clients/Acme"));

    assert_eq!(h.subjects(system::INBOX), strings(&["one", "two"]));
    assert_eq!(h.subjects(system::SENT), strings(&["sent"]));
    assert_eq!(h.subjects("user:Clients/Acme"), strings(&["acme"]));
    let one = h.db.get_message(h.id_of("one")).unwrap().unwrap();
    assert!(one.unread);
    assert_eq!(one.sender, "Ann");
    assert_eq!(one.message_id_hdr.as_deref(), Some("one-1@example.com"));
    assert_eq!(one.body_state, 0, "prefetch disabled");
    assert!(!h.db.get_message(h.id_of("two")).unwrap().unwrap().unread);

    let state = h.db.folder_sync_state(h.folder_id(system::INBOX)).unwrap();
    assert!(state.uid_validity.is_some() && state.synced_at.is_some());
    assert_eq!(state.uid_next, Some(3));
    assert!(state.highest_modseq.is_some());
    assert!(h.engine.is_running(h.account));
}

#[test]
fn archive_role_wins_over_all_mail() {
    let h = Harness::imap();
    h.server.add_folder("All Mail", &["\\All"]);
    h.server
        .deliver("All Mail", &mail("everything", 1), unread());
    h.server.deliver("INBOX", &mail("inboxed", 2), unread());
    let h = h.started();
    let archive = h.folder_id(system::ARCHIVE);
    assert_eq!(
        h.db.folder_by_id(archive)
            .unwrap()
            .unwrap()
            .remote_name
            .as_deref(),
        Some("Archive")
    );
    assert_eq!(
        h.message_count(),
        1,
        "All Mail is not synced next to Archive"
    );
    assert!(!h.server.calls().iter().any(|c| c.ends_with("All Mail")));
}

#[test]
fn gmail_without_archive_folder_maps_all_mail() {
    let file = NamedTempFile::new().unwrap();
    let db = Database::open(file.path()).unwrap();
    let account = db
        .create_account("G", "g@gmail.com", PROVIDER_GMAIL, "{}")
        .unwrap();
    let server = FakeRemote::new();
    server.set_capabilities(Capabilities {
        gmail: true,
        ..server.capabilities()
    });
    server.add_folder("INBOX", &[]);
    server.add_folder("[Gmail]", &["\\Noselect"]);
    server.add_folder("[Gmail]/All Mail", &["\\All"]);
    server.add_folder("[Gmail]/Sent Mail", &["\\Sent"]);
    server.add_folder("[Gmail]/Starred", &["\\Flagged"]);
    let inboxed = mail("inboxed", 1);
    server.deliver("INBOX", &inboxed, unread());
    server.deliver("[Gmail]/All Mail", &inboxed, unread());
    server.deliver("[Gmail]/All Mail", &mail("archived", 2), seen());
    server.deliver("[Gmail]/Starred", &inboxed, unread());
    let connector = FakeConnector::new(server, FakeSender::new());
    let (tx, events) = async_channel::unbounded();
    let engine = Engine::new(file.path().to_path_buf(), tx, test_options());
    engine.start_account(account, Arc::new(connector));
    let h = Harness {
        _file: file,
        db,
        account,
        server: FakeRemote::new(),
        connector: FakeConnector::default(),
        engine,
        events,
    };
    h.wait_idle();
    assert_eq!(h.message_count(), 2);
    assert_eq!(h.subjects(system::INBOX), strings(&["inboxed"]));
    assert_eq!(
        h.subjects(system::ARCHIVE),
        strings(&["archived", "inboxed"])
    );
    let inboxed = h.db.get_message(h.id_of("inboxed")).unwrap().unwrap();
    assert_eq!(inboxed.folders_csv, ",sys:archive,sys:inbox,");
    let paths: Vec<String> =
        h.db.list_folders(account)
            .unwrap()
            .into_iter()
            .map(|f| f.path)
            .collect();
    assert!(!paths
        .iter()
        .any(|p| p.contains("Starred") || p.contains("Gmail")));
}

#[test]
fn incremental_sync_announces_only_new_unread_inbox_mail() {
    let h = Harness::imap();
    h.server.deliver("INBOX", &mail("old", 1), unread());
    let h = h.started();
    h.server.deliver("INBOX", &mail("fresh", 2), unread());
    h.server
        .deliver("INBOX", &mail("read elsewhere", 3), seen());
    h.server.deliver("Archive", &mail("archived", 4), unread());
    let events = h.sync();
    assert_eq!(new_mail(&events), strings(&["fresh"]));
    let notice = events
        .iter()
        .find_map(|e| match e {
            SyncEvent::NewMail {
                account_id,
                messages,
            } => Some((*account_id, messages[0].clone())),
            _ => None,
        })
        .unwrap();
    assert_eq!(notice.0, h.account);
    assert_eq!(notice.1.sender, "Ann");
    assert_eq!(notice.1.message_id, h.id_of("fresh"));
    assert_eq!(
        h.subjects(system::INBOX),
        strings(&["fresh", "old", "read elsewhere"])
    );
    assert!(
        new_mail(&h.sync()).is_empty(),
        "nothing new the second time"
    );
}

#[test]
fn gmail_mail_seen_first_in_all_mail_is_still_announced() {
    let h = Harness::new(PROVIDER_GMAIL, "{}", test_options());
    h.server.set_capabilities(Capabilities {
        gmail: true,
        ..h.server.capabilities()
    });
    h.server.deliver("INBOX", &mail("old", 1), unread());
    h.server.deliver("Archive", &mail("old", 1), unread());
    let h = h.started();
    let raw = mail("fresh", 2);
    // Delivered right after the Inbox step of a pass: All Mail sees it first.
    h.server.deliver("Archive", &raw, unread());
    assert!(new_mail(&h.sync()).is_empty(), "not in the Inbox yet");
    h.server.deliver("INBOX", &raw, unread());
    assert_eq!(new_mail(&h.sync()), strings(&["fresh"]));
    assert!(new_mail(&h.sync()).is_empty(), "announced once");
}

#[test]
fn flags_sync_in_both_directions() {
    let h = Harness::imap();
    let uid = h.server.deliver("INBOX", &mail("a", 1), unread());
    let h = h.started();
    let id = h.id_of("a");

    // Server → local (another client read and starred it).
    h.server.set_flags(
        "INBOX",
        uid,
        MessageFlags {
            seen: true,
            flagged: true,
            ..Default::default()
        },
    );
    h.sync();
    let detail = h.db.get_message(id).unwrap().unwrap();
    assert!(!detail.unread && detail.starred);
    assert_eq!(detail.folders_csv, ",sys:flagged,sys:inbox,");

    // Unchanged modseq: no flag fetch at all.
    h.server.clear_calls();
    h.sync();
    assert!(!h
        .server
        .calls()
        .iter()
        .any(|c| c.starts_with("fetch_flags")));

    // Local → server.
    assert!(actions::set_read(&h.db, id, false).unwrap().needs_sync);
    assert!(actions::set_starred(&h.db, id, false).unwrap().needs_sync);
    h.run(Command::ProcessQueue);
    let flags = h.server.messages("INBOX")[0].flags;
    assert!(!flags.seen && !flags.flagged);
    assert!(h.db.pending_ops(h.account).unwrap().is_empty());
    h.sync();
    let detail = h.db.get_message(id).unwrap().unwrap();
    assert!(detail.unread && !detail.starred, "server agrees now");
}

#[test]
fn server_expunge_removes_the_local_message() {
    let h = Harness::imap();
    let uid = h.server.deliver("INBOX", &mail("gone", 1), unread());
    h.server.deliver("INBOX", &mail("kept", 2), unread());
    let h = h.started();
    h.server.expunge("INBOX", uid);
    h.sync();
    assert_eq!(h.subjects(system::INBOX), strings(&["kept"]));
    assert_eq!(h.message_count(), 1);
}

#[test]
fn uid_validity_reset_refetches_without_duplicates_or_notices() {
    let h = Harness::imap();
    h.server.deliver("INBOX", &mail("a", 1), unread());
    h.server.deliver("INBOX", &mail("b", 2), unread());
    let h = h.started();
    h.server.expunge("INBOX", 1);
    let validity = h.server.reset_uid_validity("INBOX");
    let events = h.sync();
    assert!(new_mail(&events).is_empty());
    assert_eq!(h.subjects(system::INBOX), strings(&["b"]));
    assert_eq!(h.message_count(), 1);
    let inbox = h.folder_id(system::INBOX);
    assert_eq!(h.db.folder_uids(inbox).unwrap()[0].0, 1, "renumbered");
    assert_eq!(
        h.db.folder_sync_state(inbox).unwrap().uid_validity,
        Some(validity)
    );
}

fn move_round_trip(uidplus: bool) {
    let h = Harness::imap();
    if !uidplus {
        h.server.set_capabilities(Capabilities {
            uidplus: false,
            ..h.server.capabilities()
        });
    }
    h.server.deliver("INBOX", &mail("mover", 1), unread());
    h.server.deliver("INBOX", &mail("stayer", 2), unread());
    let h = h.started();
    let id = h.id_of("mover");

    let outcome = actions::archive(&h.db, id).unwrap();
    assert!(outcome.changed && outcome.needs_sync);
    assert_eq!(
        h.subjects(system::ARCHIVE),
        strings(&["mover"]),
        "optimistic"
    );
    assert_eq!(h.subjects(system::INBOX), strings(&["stayer"]));

    // A sync before the queue ran must not re-attach it to the Inbox: the
    // full cycle replays the queue first.
    h.sync();
    assert_eq!(h.server_subjects("Archive"), strings(&["mover"]));
    assert_eq!(h.server_subjects("INBOX"), strings(&["stayer"]));
    assert_eq!(h.subjects(system::ARCHIVE), strings(&["mover"]));
    assert_eq!(h.subjects(system::INBOX), strings(&["stayer"]));
    assert_eq!(h.message_count(), 2, "no duplicate");
    assert_eq!(
        h.id_of("mover"),
        id,
        "same row kept (merged by Message-ID or COPYUID)"
    );
    assert_eq!(
        h.db.folder_uids(h.folder_id(system::ARCHIVE)).unwrap(),
        vec![(1, id)]
    );
    assert!(h.db.pending_ops(h.account).unwrap().is_empty());
    h.sync();
    assert_eq!(h.message_count(), 2);
}

#[test]
fn move_with_uidplus_attaches_the_new_uid() {
    move_round_trip(true);
}

#[test]
fn move_without_uidplus_reattaches_by_message_id() {
    move_round_trip(false);
}

#[test]
fn queued_move_is_not_undone_by_a_folder_sync() {
    let h = Harness::imap();
    h.server.deliver("INBOX", &mail("mover", 1), unread());
    let h = h.started();
    let id = h.id_of("mover");
    // Queue a move but make the server refuse it once: the op stays queued
    // while the Inbox is synced again.
    h.server.fail_next(RemoteErrorKind::Protocol);
    actions::move_to_folder(&h.db, id, system::TRASH).unwrap();
    h.run(Command::SyncFolder(h.folder_id(system::INBOX)));
    assert_eq!(h.db.pending_ops(h.account).unwrap().len(), 1);
    assert_eq!(h.subjects(system::INBOX), Vec::<String>::new());
    assert_eq!(h.subjects(system::TRASH), strings(&["mover"]));
    h.sync();
    assert_eq!(h.server_subjects("Trash"), strings(&["mover"]));
    assert_eq!(h.subjects(system::TRASH), strings(&["mover"]));
    assert_eq!(h.message_count(), 1);
}

#[test]
fn trash_then_delete_permanently() {
    let h = Harness::imap();
    h.server.deliver("INBOX", &mail("doomed", 1), unread());
    let h = h.started();
    let id = h.id_of("doomed");
    actions::trash(&h.db, id).unwrap();
    h.sync();
    assert_eq!(h.server_subjects("Trash"), strings(&["doomed"]));
    let id = h.id_of("doomed");
    assert!(h.db.message_is_in_trash(id).unwrap());
    let outcome = actions::trash(&h.db, id).unwrap();
    assert!(outcome.changed && outcome.needs_sync);
    assert!(h.db.get_message(id).unwrap().is_none());
    h.run(Command::ProcessQueue);
    assert!(h.server.messages("Trash").is_empty());
    h.sync();
    assert_eq!(h.message_count(), 0);
}

#[test]
fn op_failing_three_times_is_dropped_and_the_folder_resynced() {
    let h = Harness::imap();
    h.server.deliver("INBOX", &mail("a", 1), unread());
    let h = h.started();
    let id = h.id_of("a");
    actions::set_read(&h.db, id, true).unwrap();
    for attempt in 1..=3 {
        h.server.fail_next(RemoteErrorKind::Protocol);
        h.run(Command::ProcessQueue);
        let ops = h.db.pending_ops(h.account).unwrap();
        if attempt < 3 {
            assert_eq!(ops[0].attempts, attempt);
        } else {
            assert!(ops.is_empty(), "dropped after 3 attempts");
        }
    }
    // The queued SyncFolder restores the server truth (still unread) with a
    // full flag check.
    h.wait_idle();
    assert!(h.db.get_message(id).unwrap().unwrap().unread);
    let inbox = h.folder_id(system::INBOX);
    assert!(h
        .db
        .folder_sync_state(inbox)
        .unwrap()
        .highest_modseq
        .is_some());
}

#[test]
fn offline_queue_is_replayed_after_reconnect() {
    let h = Harness::imap();
    h.server.deliver("INBOX", &mail("a", 1), unread());
    let h = h.started();
    let id = h.id_of("a");

    h.connector
        .set_connect_failure(Some(RemoteErrorKind::Network));
    h.drain();
    h.engine.send(h.account, Command::Reconnect);
    let events = h.wait_status("Offline", |s| matches!(s, AccountStatus::Offline { .. }));
    assert!(events.iter().any(|e| matches!(
        e,
        SyncEvent::Status { status: AccountStatus::Offline { error, .. }, .. } if error.contains("network")
    )));
    // Edits while offline only touch the database.
    actions::set_read(&h.db, id, true).unwrap();
    actions::archive(&h.db, id).unwrap();
    // Several retries back off and stay offline…
    h.wait_status("second Offline", |s| {
        matches!(s, AccountStatus::Offline { .. })
    });
    // …until the server is back: the worker reconnects by itself.
    h.connector.set_connect_failure(None);
    h.wait_idle();
    let archived = h.server.messages("Archive");
    assert_eq!(archived.len(), 1);
    assert!(archived[0].flags.seen);
    assert!(h.db.pending_ops(h.account).unwrap().is_empty());
}

#[test]
fn auth_failure_stops_retrying_until_reconnect() {
    let h = Harness::imap();
    h.connector.set_connect_failure(Some(RemoteErrorKind::Auth));
    h.start();
    h.wait_status("AuthFailed", |s| {
        matches!(s, AccountStatus::AuthFailed { .. })
    });
    // No automatic retries, and queue/folder commands are ignored.
    h.engine.send(h.account, Command::ProcessQueue);
    std::thread::sleep(Duration::from_millis(100));
    assert!(h.events.try_recv().is_err(), "worker stays quiet");
    h.engine.send(h.account, Command::FetchBody(1));
    h.wait_for("BodyFailed", |e| matches!(e, SyncEvent::BodyFailed { .. }));
    h.connector.set_connect_failure(None);
    h.run(Command::Reconnect);
    assert_eq!(h.connector.connects(), 1);
}

#[test]
fn outbox_is_sent_and_filed_in_sent() {
    let h = Harness::imap().started();
    let raw = mail("outgoing", 9);
    let outbox_id =
        h.db.enqueue_outgoing(
            h.account,
            "me@example.com",
            &strings(&["b@x.com"]),
            &raw,
            None,
        )
        .unwrap();
    let events = h.run(Command::ProcessQueue);
    assert!(events.contains(&SyncEvent::Sent {
        account_id: h.account,
        outbox_id
    }));
    let sent = h.connector.sender().sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].recipients, strings(&["b@x.com"]));
    assert_eq!(sent[0].raw, raw);
    let copies = h.server.messages("Sent");
    assert_eq!(copies.len(), 1);
    assert!(copies[0].flags.seen);
    assert_eq!(h.db.outbox_count().unwrap(), 0);
    h.sync();
    assert_eq!(h.subjects(system::SENT), strings(&["outgoing"]));

    // Gmail-like servers file the copy themselves.
    h.connector.set_saves_sent_copy(true);
    h.db.enqueue_outgoing(
        h.account,
        "me@example.com",
        &strings(&["c@x.com"]),
        &raw,
        None,
    )
    .unwrap();
    h.run(Command::ProcessQueue);
    assert_eq!(h.connector.sender().sent().len(), 2);
    assert_eq!(h.server.messages("Sent").len(), 1, "no APPEND");
}

#[test]
fn send_failures_retry_then_become_permanent() {
    let h = Harness::imap().started();
    let outbox_id =
        h.db.enqueue_outgoing(
            h.account,
            "me@example.com",
            &strings(&["b@x.com"]),
            b"raw",
            None,
        )
        .unwrap();
    let sender = h.connector.sender();

    sender.fail_next(RemoteErrorKind::Network);
    let events = h.run(Command::ProcessQueue);
    assert!(events.iter().any(|e| matches!(
        e,
        SyncEvent::SendFailed { outbox_id: id, permanent: false, .. } if *id == outbox_id
    )));
    assert_eq!(
        h.db.outbox(h.account).unwrap()[0].attempts,
        0,
        "kept for retry"
    );

    for attempt in 1..=3u32 {
        sender.fail_next(RemoteErrorKind::Protocol);
        let events = h.run(Command::ProcessQueue);
        let permanent = events
            .iter()
            .find_map(|e| match e {
                SyncEvent::SendFailed { permanent, .. } => Some(*permanent),
                _ => None,
            })
            .unwrap();
        assert_eq!(permanent, attempt == 3);
        assert_eq!(h.db.outbox(h.account).unwrap()[0].attempts, attempt);
    }
    // No more automatic attempts; the row stays for the UI.
    h.sync();
    assert!(sender.sent().is_empty());
    assert_eq!(h.db.outbox_count().unwrap(), 0);
    let failed = h.db.unsent_outgoing().unwrap();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].state, storage::OutgoingState::Failed);
    // The user retries: sent on the next cycle.
    h.db.retry_outgoing(outbox_id).unwrap();
    h.run(Command::ProcessQueue);
    assert_eq!(sender.sent().len(), 1);
    assert!(h.db.unsent_outgoing().unwrap().is_empty());
}

#[test]
fn bodies_prefetch_newest_small_ones_and_fetch_others_on_demand() {
    let options = EngineOptions {
        body_prefetch_inbox: 2,
        max_body_size: 2_000,
        ..test_options()
    };
    let h = Harness::new(PROVIDER_IMAP, "{}", options);
    h.server.deliver("INBOX", &mail("oldest", 1), unread());
    h.server.deliver("INBOX", &mail("middle", 2), unread());
    let mut big = mail("big", 3);
    big.extend(std::iter::repeat_n(b'x', 3_000));
    h.server.deliver("INBOX", &big, unread());
    h.server.deliver("Archive", &mail("archived", 4), unread());
    h.start();
    let events = h.wait_idle();
    let middle = h.id_of("middle");
    assert!(events.contains(&SyncEvent::BodyReady { message_id: middle }));
    let state = |subject: &str| {
        h.db.get_message(h.id_of(subject))
            .unwrap()
            .unwrap()
            .body_state
    };
    // Newest two are "big" (too large) and "middle".
    assert_eq!(
        (
            state("big"),
            state("middle"),
            state("oldest"),
            state("archived")
        ),
        (0, 1, 0, 0)
    );
    let detail = h.db.get_message(middle).unwrap().unwrap();
    assert_eq!(detail.raw_format, "text");
    assert!(detail.plain_text.contains("Body of middle"));
    let preview: String =
        h.db.conn()
            .query_row(
                "SELECT preview FROM messages WHERE id = ?1",
                [middle],
                |row| row.get(0),
            )
            .unwrap();
    assert_eq!(preview, "Body of middle");
    assert!(detail.unread, "fetching a body never marks it read");

    // On demand: any size, and the server flags stay untouched.
    let big_id = h.id_of("big");
    h.drain();
    h.engine.send(h.account, Command::FetchBody(big_id));
    h.wait_for("BodyReady", |e| {
        *e == SyncEvent::BodyReady { message_id: big_id }
    });
    assert_eq!(state("big"), 1);
    assert!(!h.server.messages("INBOX")[2].flags.seen);
    // Already downloaded: ready again without a server round trip.
    h.server.clear_calls();
    h.engine.send(h.account, Command::FetchBody(big_id));
    h.wait_for("BodyReady", |e| {
        *e == SyncEvent::BodyReady { message_id: big_id }
    });
    assert!(!h.server.calls().iter().any(|c| c.starts_with("fetch_body")));
    // Unknown message.
    h.engine.send(h.account, Command::FetchBody(9999));
    h.wait_for("BodyFailed", |e| {
        matches!(
            e,
            SyncEvent::BodyFailed {
                message_id: 9999,
                ..
            }
        )
    });
}

#[test]
fn cid_images_are_inlined_into_stored_html() {
    let h = Harness::imap();
    let raw = b"From: a@x.com\r\nSubject: logo\r\nMessage-ID: <logo@x>\r\n\
Content-Type: multipart/related; boundary=\"r\"\r\n\r\n\
--r\r\nContent-Type: text/html\r\n\r\n<img src=\"cid:logo@x\">\r\n\
--r\r\nContent-Type: image/png\r\nContent-ID: <logo@x>\r\n\
Content-Transfer-Encoding: base64\r\n\r\niVBORw0K\r\n--r--\r\n";
    h.server.deliver("INBOX", raw, unread());
    let h = h.started();
    let id = h.id_of("logo");
    h.drain();
    h.engine.send(h.account, Command::FetchBody(id));
    h.wait_for("BodyReady", |e| {
        *e == SyncEvent::BodyReady { message_id: id }
    });
    let detail = h.db.get_message(id).unwrap().unwrap();
    assert_eq!(detail.raw_format, "html");
    assert!(detail
        .raw_content
        .contains("src=\"data:image/png;base64,iVBORw0K\""));
    assert!(!detail.has_attachment, "inline image is not an attachment");
    assert_eq!(h.db.list_attachments(id).unwrap().len(), 1);
    assert_eq!(h.db.message_raw(id).unwrap().unwrap(), raw.to_vec());
}

fn idle_options() -> EngineOptions {
    EngineOptions {
        idle_timeout: Duration::from_secs(30),
        ..test_options()
    }
}

fn wait_for_idle_call(server: &FakeRemote) {
    let deadline = Instant::now() + WAIT;
    while !server.calls().iter().any(|c| c == "idle INBOX") {
        assert!(Instant::now() < deadline, "IDLE never started");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn idle_wakeup_brings_new_mail() {
    let h = Harness::new(PROVIDER_IMAP, "{}", idle_options()).started();
    wait_for_idle_call(&h.server);
    h.drain();
    h.server.deliver("INBOX", &mail("pushed", 1), unread());
    let events = h.wait_for("NewMail", |e| matches!(e, SyncEvent::NewMail { .. }));
    assert_eq!(new_mail(&events), strings(&["pushed"]));
}

#[test]
fn stop_account_returns_quickly_while_idle_is_blocked() {
    let h = Harness::new(PROVIDER_IMAP, "{}", idle_options()).started();
    wait_for_idle_call(&h.server);
    let started = Instant::now();
    assert!(h.engine.stop_account(h.account));
    assert!(started.elapsed() < Duration::from_millis(900));
    assert!(!h.engine.is_running(h.account));
    assert!(!h.engine.stop_account(h.account));
    assert!(!h.engine.send(h.account, Command::SyncNow));
}

#[test]
fn servers_without_idle_fall_back_to_polling() {
    let options = EngineOptions {
        poll_interval: Duration::from_millis(30),
        ..idle_options()
    };
    let h = Harness::new(PROVIDER_IMAP, "{}", options);
    h.server.set_capabilities(Capabilities {
        idle: false,
        ..h.server.capabilities()
    });
    let h = h.started();
    h.server.deliver("INBOX", &mail("polled", 1), unread());
    let events = h.wait_for("NewMail", |e| matches!(e, SyncEvent::NewMail { .. }));
    assert_eq!(new_mail(&events), strings(&["polled"]));
    assert!(!h.server.calls().iter().any(|c| c.starts_with("idle")));
}

#[test]
fn sync_window_caps_downloaded_headers() {
    let config = AccountConfig {
        email: "me@example.com".into(),
        username: "me@example.com".into(),
        imap: ServerConfig::default(),
        smtp: ServerConfig::default(),
        sync_window: 2,
        ..Default::default()
    };
    let h = Harness::new(PROVIDER_IMAP, &config.to_json(), test_options());
    for n in 1..=5 {
        h.server
            .deliver("INBOX", &mail(&format!("m{n}"), n), unread());
    }
    let h = h.started();
    assert_eq!(h.subjects(system::INBOX), strings(&["m4", "m5"]));
    h.server.clear_calls();
    h.server.deliver("INBOX", &mail("m6", 6), unread());
    h.sync();
    assert_eq!(h.subjects(system::INBOX), strings(&["m4", "m5", "m6"]));
    assert!(h
        .server
        .calls()
        .contains(&"fetch_headers INBOX 6".to_string()));
}

#[test]
fn local_accounts_are_not_synced() {
    let h = Harness::new(PROVIDER_LOCAL, "{}", test_options());
    h.start();
    h.wait_status("Error", |s| matches!(s, AccountStatus::Error { .. }));
    let deadline = Instant::now() + WAIT;
    while h.engine.is_running(h.account) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(h.connector.connects(), 0);
}

#[test]
fn engine_handles_many_accounts_and_shuts_down() {
    let h = Harness::imap().started();
    assert!(!h.engine.send(9999, Command::SyncNow));
    assert!(!h.engine.is_running(9999));
    h.drain();
    h.engine.sync_all();
    h.wait_idle();
    // Restarting replaces the worker.
    h.start();
    h.wait_idle();
    assert!(h.engine.is_running(h.account));
    let started = Instant::now();
    h.engine.shutdown();
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(!h.engine.is_running(h.account));
    h.engine.sync_all(); // no accounts: no-op
    drop(h); // Drop after shutdown is fine.
}

#[test]
fn shutdown_command_and_unopenable_database() {
    let h = Harness::imap().started();
    assert!(h.engine.send(h.account, Command::Shutdown));
    let deadline = Instant::now() + WAIT;
    while h.engine.is_running(h.account) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }

    let dir = tempfile::tempdir().unwrap();
    let (tx, events) = async_channel::unbounded();
    // A directory is not a database file.
    let engine = Engine::new(dir.path().to_path_buf(), tx, test_options());
    engine.start_account(1, Arc::new(FakeConnector::default()));
    let event = events.recv_blocking().unwrap();
    assert!(matches!(
        event,
        SyncEvent::Status {
            account_id: 1,
            status: AccountStatus::Error { .. }
        }
    ));
}

// --- Review findings: stale UIDs, races, outbox state machine ---

/// The engine's database file, for a second ("UI") connection opened from a
/// hook running on the worker thread.
fn db_path(h: &Harness) -> std::path::PathBuf {
    h._file.path().to_path_buf()
}

#[test]
fn queued_ops_follow_the_message_across_a_uid_validity_change() {
    let h = Harness::imap();
    h.server.deliver("INBOX", &mail("a", 1), unread());
    h.server.deliver("INBOX", &mail("b", 2), unread());
    let h = h.started();
    let b = h.id_of("b");
    // Queued against b = UID 2 under the old UIDVALIDITY…
    actions::set_starred(&h.db, b, true).unwrap();
    actions::archive(&h.db, b).unwrap();
    // …then the server renumbers: b is UID 1 and a new message takes UID 2.
    h.server.expunge("INBOX", 1);
    h.server.reset_uid_validity("INBOX");
    h.server.deliver("INBOX", &mail("c", 3), unread());
    h.run(Command::ProcessQueue);
    assert_eq!(h.server_subjects("Archive"), strings(&["b"]));
    assert_eq!(h.server_subjects("INBOX"), strings(&["c"]), "c untouched");
    assert!(h.server.messages("Archive")[0].flags.flagged);
    assert!(!h.server.messages("INBOX")[0].flags.flagged);
    assert!(h.db.pending_ops(h.account).unwrap().is_empty());
    h.sync();
    assert_eq!(h.subjects(system::ARCHIVE), strings(&["b"]));
    assert_eq!(h.subjects(system::INBOX), strings(&["c"]));
}

#[test]
fn legacy_payloads_without_validity_are_rechecked() {
    let h = Harness::imap();
    h.server.deliver("INBOX", &mail("a", 1), unread());
    h.server.deliver("INBOX", &mail("b", 2), unread());
    let h = h.started();
    let (a, b) = (h.id_of("a"), h.id_of("b"));
    let legacy_flag = |uid: u32| {
        format!(
            r#"{{"type":"flags","folder":"INBOX","uid":{uid},
            "add":{{"seen":true,"flagged":false,"answered":false,"draft":false,"deleted":false}},
            "remove":{{"seen":false,"flagged":false,"answered":false,"draft":false,"deleted":false}}}}"#
        )
    };
    // Checkpoint unchanged: the recorded UID is trusted.
    h.db.enqueue_op(h.account, "flags", Some(a), &legacy_flag(1))
        .unwrap();
    h.run(Command::ProcessQueue);
    assert!(h.server.messages("INBOX")[0].flags.seen);
    // After a renumbering the stored message's Message-ID finds it.
    h.server.expunge("INBOX", 1);
    h.server.reset_uid_validity("INBOX");
    h.server.deliver("INBOX", &mail("c", 3), unread());
    h.db.enqueue_op(h.account, "flags", Some(b), &legacy_flag(2))
        .unwrap();
    h.run(Command::ProcessQueue);
    let inbox = h.server.messages("INBOX");
    assert!(inbox[0].flags.seen, "b (now UID 1) was marked");
    assert!(!inbox[1].flags.seen, "c (UID 2) was not");
}

#[test]
fn move_without_uidplus_keeps_later_flag_edits() {
    let h = Harness::imap();
    h.server.set_capabilities(Capabilities {
        uidplus: false,
        ..h.server.capabilities()
    });
    h.server.deliver("INBOX", &mail("m", 1), unread());
    let h = h.started();
    let id = h.id_of("m");
    actions::archive(&h.db, id).unwrap();
    actions::set_starred(&h.db, id, true).unwrap();
    actions::set_read(&h.db, id, true).unwrap();
    h.run(Command::ProcessQueue);
    let archived = h.server.messages("Archive");
    assert_eq!(archived.len(), 1);
    assert!(archived[0].flags.flagged && archived[0].flags.seen);
    assert!(h.db.pending_ops(h.account).unwrap().is_empty());
    h.sync();
    let detail = h.db.get_message(h.id_of("m")).unwrap().unwrap();
    assert!(detail.starred && !detail.unread, "the UI never flips back");
    assert_eq!(h.message_count(), 1);
}

/// A move without COPYUID, run but not yet followed by a sync: the message
/// has no location and no queued op.
fn moved_without_copyuid(h: &Harness, subject: &str) -> i64 {
    let id = h.id_of(subject);
    actions::archive(&h.db, id).unwrap();
    h.run(Command::ProcessQueue);
    assert!(h.db.pending_ops(h.account).unwrap().is_empty());
    // The worker resyncs the target right away; the state before that sync
    // lands is what these tests are about.
    h.db.detach_locations(id).unwrap();
    id
}

fn imap_without_uidplus() -> Harness {
    let h = Harness::imap();
    h.server.set_capabilities(Capabilities {
        uidplus: false,
        ..h.server.capabilities()
    });
    h
}

#[test]
fn move_without_copyuid_resyncs_the_target_right_away() {
    let h = imap_without_uidplus();
    h.server.deliver("INBOX", &mail("m", 1), unread());
    let h = h.started();
    let id = h.id_of("m");
    actions::archive(&h.db, id).unwrap();
    h.run(Command::ProcessQueue);
    let archive = h.folder_id(system::ARCHIVE);
    let deadline = Instant::now() + WAIT;
    while h.db.folder_uids(archive).unwrap().is_empty() {
        assert!(Instant::now() < deadline, "never re-attached");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(h.db.folder_uids(archive).unwrap()[0].1, id);
}

/// Polls `check`: the resync after a move without COPYUID may still be
/// running when a later command's `Idle` arrives.
fn eventually(what: &str, check: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn flag_edit_after_a_move_without_copyuid_reaches_the_server() {
    let h = imap_without_uidplus();
    h.server.deliver("INBOX", &mail("m", 1), unread());
    let h = h.started();
    let id = moved_without_copyuid(&h, "m");
    actions::set_starred(&h.db, id, true).unwrap();
    h.run(Command::ProcessQueue);
    eventually("the flag on the server", || {
        h.server.messages("Archive")[0].flags.flagged
    });
    h.sync();
    eventually("the local copy to settle", || {
        h.message_count() == 1 && h.db.get_message(h.id_of("m")).unwrap().unwrap().starred
    });
}

#[test]
fn flag_edit_during_a_header_fetch_is_not_overwritten() {
    let h = imap_without_uidplus();
    h.server.deliver("INBOX", &mail("m", 1), unread());
    let h = h.started();
    let id = h.id_of("m");
    let path = db_path(&h);
    h.server.on_next_call("fetch_headers Archive", move || {
        let ui = Database::open(&path).unwrap();
        actions::set_starred(&ui, id, true).unwrap();
    });
    actions::archive(&h.db, id).unwrap();
    h.run(Command::ProcessQueue);
    eventually("the merged header", || {
        !h.db
            .folder_uids(h.folder_id(system::ARCHIVE))
            .unwrap()
            .is_empty()
    });
    assert!(
        h.db.get_message(id).unwrap().unwrap().starred,
        "the header fetched before the edit must not undo it"
    );
    h.run(Command::ProcessQueue);
    eventually("the flag on the server", || {
        h.server.messages("Archive")[0].flags.flagged
    });
}

#[test]
fn move_after_a_move_without_copyuid_reaches_the_server() {
    let h = imap_without_uidplus();
    h.server.deliver("INBOX", &mail("m", 1), unread());
    let h = h.started();
    let id = moved_without_copyuid(&h, "m");
    actions::trash(&h.db, id).unwrap();
    h.run(Command::ProcessQueue);
    eventually("the move on the server", || {
        h.server.messages("Archive").is_empty() && h.server.messages("Trash").len() == 1
    });
    h.sync();
    eventually("the local copy to settle", || {
        h.message_count() == 1 && h.db.pending_ops(h.account).unwrap().is_empty()
    });
}

#[test]
fn delete_after_a_move_without_copyuid_reaches_the_server() {
    let h = imap_without_uidplus();
    h.server.deliver("INBOX", &mail("m", 1), unread());
    let h = h.started();
    let id = moved_without_copyuid(&h, "m");
    actions::delete_permanently(&h.db, id).unwrap();
    h.run(Command::ProcessQueue);
    eventually("the delete on the server", || {
        h.server.messages("Archive").is_empty()
    });
    h.sync();
    eventually("the row to stay gone", || {
        h.db.pending_ops(h.account).unwrap().is_empty() && h.message_count() == 0
    });
}

#[test]
fn failed_message_id_search_is_not_a_missing_message() {
    let h = Harness::imap();
    h.server.deliver("INBOX", &mail("m", 1), unread());
    let h = h.started();
    let id = h.id_of("m");
    actions::archive(&h.db, id).unwrap();
    h.server.reset_uid_validity("INBOX");
    h.server.on_next_call("find_uid", {
        let server = h.server.clone();
        move || server.fail_next(RemoteErrorKind::Protocol)
    });
    h.run(Command::ProcessQueue);
    let ops = h.db.pending_ops(h.account).unwrap();
    assert_eq!(ops.len(), 1, "kept for another attempt");
    assert_eq!(ops[0].attempts, 1);
    assert!(h.server.messages("Archive").is_empty());
    h.run(Command::ProcessQueue);
    assert_eq!(h.server.messages("Archive").len(), 1);
    assert!(h.db.pending_ops(h.account).unwrap().is_empty());
}

#[test]
fn expired_send_lease_tells_the_ui() {
    let h = Harness::imap().started();
    let id =
        h.db.enqueue_outgoing(
            h.account,
            "me@example.com",
            &strings(&["b@x.com"]),
            &mail("outgoing", 9),
            None,
        )
        .unwrap();
    h.db.claim_outgoing(id, 0).unwrap();
    let events = h.run(Command::SyncNow);
    assert!(events.iter().any(|event| matches!(
        event,
        SyncEvent::SendFailed { outbox_id, permanent: true, .. } if *outbox_id == id
    )));
    assert_eq!(
        h.db.outgoing(id).unwrap().unwrap().state,
        storage::OutgoingState::MaybeSent
    );
    assert!(h.connector.sender().sent().is_empty(), "never resent");
}

#[test]
fn giving_up_on_the_sent_copy_tells_the_ui() {
    let h = Harness::imap().started();
    h.db.enqueue_outgoing(
        h.account,
        "me@example.com",
        &strings(&["b@x.com"]),
        &mail("outgoing", 9),
        None,
    )
    .unwrap();
    let mut events = Vec::new();
    for _ in 0..crate::worker::MAX_APPEND_DEFERRALS {
        h.server.on_next_call("append Sent", {
            let server = h.server.clone();
            move || server.fail_next(RemoteErrorKind::Protocol)
        });
        for row in h.db.outbox(h.account).unwrap() {
            h.db.conn()
                .execute(
                    "UPDATE outbox SET next_attempt_at = 0 WHERE id = ?1",
                    [row.id],
                )
                .unwrap();
        }
        events = h.run(Command::ProcessQueue);
    }
    assert!(h.db.outbox(h.account).unwrap().is_empty());
    assert_eq!(h.connector.sender().sent().len(), 1, "sent once");
    assert!(events
        .iter()
        .any(|event| matches!(event, SyncEvent::SentCopyFailed { .. })));
}

#[test]
fn sync_keeps_local_flags_while_a_flag_op_is_pending() {
    let h = Harness::imap();
    let uid = h.server.deliver("INBOX", &mail("m", 1), unread());
    let h = h.started();
    let id = h.id_of("m");
    // The server refuses the STORE once; meanwhile another client changes
    // an unrelated flag so a flag check runs.
    actions::set_starred(&h.db, id, true).unwrap();
    h.server.on_next_call("store_flags", {
        let server = h.server.clone();
        move || {
            server.fail_next(RemoteErrorKind::Protocol);
        }
    });
    h.server.set_flags(
        "INBOX",
        uid,
        MessageFlags {
            answered: true,
            ..Default::default()
        },
    );
    h.run(Command::SyncFolder(h.folder_id(system::INBOX)));
    assert_eq!(h.db.pending_ops(h.account).unwrap().len(), 1);
    assert!(h.db.get_message(id).unwrap().unwrap().starred, "kept");
    h.run(Command::ProcessQueue);
    assert!(h.server.messages("INBOX")[0].flags.flagged);
}

#[test]
fn trash_then_restore_while_the_move_runs_converges_to_inbox() {
    let h = Harness::imap();
    h.server.deliver("INBOX", &mail("m", 1), unread());
    let h = h.started();
    let id = h.id_of("m");
    actions::trash(&h.db, id).unwrap();
    let path = db_path(&h);
    h.server.on_next_call("move_messages INBOX", move || {
        let ui = Database::open(&path).unwrap();
        actions::restore_from_trash(&ui, id).unwrap();
    });
    h.run(Command::ProcessQueue);
    // The compensating move runs right after.
    let deadline = Instant::now() + WAIT;
    while !h.db.pending_ops(h.account).unwrap().is_empty() {
        assert!(Instant::now() < deadline, "compensating move never ran");
        h.run(Command::ProcessQueue);
    }
    assert_eq!(h.server_subjects("INBOX"), strings(&["m"]));
    assert!(h.server.messages("Trash").is_empty());
    assert_eq!(h.subjects(system::INBOX), strings(&["m"]));
    h.sync();
    assert_eq!(h.subjects(system::INBOX), strings(&["m"]));
    assert!(h.subjects(system::TRASH).is_empty());
    assert_eq!(h.message_count(), 1);
}

#[test]
fn retargeting_a_move_while_it_runs_converges_to_the_new_target() {
    let h = Harness::imap();
    h.server.deliver("INBOX", &mail("m", 1), unread());
    let h = h.started();
    let id = h.id_of("m");
    actions::trash(&h.db, id).unwrap();
    let path = db_path(&h);
    h.server.on_next_call("move_messages INBOX", move || {
        let ui = Database::open(&path).unwrap();
        actions::archive(&ui, id).unwrap();
    });
    h.run(Command::ProcessQueue);
    assert_eq!(
        h.subjects(system::ARCHIVE),
        strings(&["m"]),
        "UI intent kept"
    );
    let deadline = Instant::now() + WAIT;
    while !h.db.pending_ops(h.account).unwrap().is_empty() {
        assert!(Instant::now() < deadline, "retargeted move never ran");
        h.run(Command::ProcessQueue);
    }
    assert_eq!(h.server_subjects("Archive"), strings(&["m"]));
    assert!(h.server.messages("Trash").is_empty());
    h.sync();
    assert_eq!(h.subjects(system::ARCHIVE), strings(&["m"]));
    assert_eq!(h.message_count(), 1);
}

#[test]
fn dropped_ops_outside_the_window_restore_the_local_copy() {
    let config = AccountConfig {
        email: "me@example.com".into(),
        username: "me@example.com".into(),
        sync_window: 1,
        ..Default::default()
    };
    let h = Harness::new(PROVIDER_IMAP, &config.to_json(), test_options());
    h.server.deliver("INBOX", &mail("old", 1), unread());
    let h = h.started();
    // "old" falls out of the 1-message window.
    h.server.deliver("INBOX", &mail("new", 2), unread());
    h.server.deliver("INBOX", &mail("doomed", 3), unread());
    h.sync();
    let old = h.id_of("old");
    actions::archive(&h.db, old).unwrap();
    for _ in 0..crate::worker::MAX_OP_ATTEMPTS {
        h.server.on_next_call("move_messages", {
            let server = h.server.clone();
            move || server.fail_next(RemoteErrorKind::Protocol)
        });
        h.run(Command::ProcessQueue);
    }
    assert!(h.db.pending_ops(h.account).unwrap().is_empty(), "dropped");
    h.sync(); // full cycle: purge_orphans runs
    assert_eq!(h.id_of("old"), old, "local copy kept");
    assert!(h.subjects(system::INBOX).contains(&"old".to_string()));
    assert!(h.subjects(system::ARCHIVE).is_empty());

    // A refused permanent delete: the row is gone locally, so the header
    // is fetched again regardless of the window.
    let doomed = h.id_of("doomed");
    actions::delete_permanently(&h.db, doomed).unwrap();
    assert!(h.db.get_message(doomed).unwrap().is_none());
    for _ in 0..crate::worker::MAX_OP_ATTEMPTS {
        h.server.on_next_call("delete_messages", {
            let server = h.server.clone();
            move || server.fail_next(RemoteErrorKind::Protocol)
        });
        h.run(Command::ProcessQueue);
    }
    h.sync();
    assert!(h.subjects(system::INBOX).contains(&"doomed".to_string()));
}

#[test]
fn transient_send_failures_back_off_without_using_attempts() {
    let options = EngineOptions {
        send_retry_delays: vec![Duration::from_secs(60)],
        ..test_options()
    };
    let h = Harness::new(PROVIDER_IMAP, "{}", options).started();
    let id =
        h.db.enqueue_outgoing(
            h.account,
            "me@example.com",
            &strings(&["b@x.com"]),
            b"raw",
            None,
        )
        .unwrap();
    let sender = h.connector.sender();
    for _ in 0..5 {
        sender.fail_next_with(mail_core::SendError::transient(
            mail_core::RemoteError::new(RemoteErrorKind::Protocol, "451 try later"),
        ));
    }
    let events = h.run(Command::ProcessQueue);
    assert!(events.iter().any(|e| matches!(
        e,
        SyncEvent::SendFailed { permanent: false, outbox_id, .. } if *outbox_id == id
    )));
    let row = &h.db.outbox(h.account).unwrap()[0];
    assert_eq!((row.attempts, row.deferrals), (0, 1));
    assert_eq!(row.state, storage::OutgoingState::Queued);
    assert!(row.next_attempt_at >= crate::worker::unix_now() + 50);
    // Not due yet: further cycles do not retry.
    h.sync();
    h.sync();
    assert_eq!(sender.sends(), 1);
    assert_eq!(h.db.outbox(h.account).unwrap()[0].deferrals, 1);
}

#[test]
fn a_message_stops_being_retried_after_about_three_days() {
    let h = Harness::imap().started();
    let id =
        h.db.enqueue_outgoing(
            h.account,
            "me@example.com",
            &strings(&["b@x.com"]),
            b"raw",
            None,
        )
        .unwrap();
    h.db.conn()
        .execute(
            "UPDATE outbox SET deferrals = ?1 WHERE id = ?2",
            [i64::from(crate::worker::MAX_SEND_DEFERRALS) - 1, id],
        )
        .unwrap();
    h.connector
        .sender()
        .fail_next_with(mail_core::SendError::transient(
            mail_core::RemoteError::new(RemoteErrorKind::Network, "connect"),
        ));
    let events = h.run(Command::ProcessQueue);
    assert!(events.iter().any(|e| matches!(
        e,
        SyncEvent::SendFailed {
            permanent: true,
            ..
        }
    )));
    let row = &h.db.outbox(h.account).unwrap()[0];
    assert_eq!(row.state, storage::OutgoingState::Failed);
}

#[test]
fn uncertain_sends_are_never_resent_automatically() {
    let h = Harness::imap().started();
    let id =
        h.db.enqueue_outgoing(
            h.account,
            "me@example.com",
            &strings(&["b@x.com"]),
            b"raw",
            None,
        )
        .unwrap();
    let sender = h.connector.sender();
    sender.deliver_then_fail_next();
    let events = h.run(Command::ProcessQueue);
    assert!(events.iter().any(|e| matches!(
        e,
        SyncEvent::SendFailed { permanent: true, outbox_id, .. } if *outbox_id == id
    )));
    h.sync();
    h.sync();
    assert_eq!(sender.sent().len(), 1, "delivered once, never resent");
    let stuck = h.db.unsent_outgoing().unwrap();
    assert_eq!(stuck.len(), 1);
    assert_eq!(stuck[0].state, storage::OutgoingState::MaybeSent);
    // The user discards it.
    h.db.delete_outgoing(id).unwrap();
    assert!(h.db.unsent_outgoing().unwrap().is_empty());
}

#[test]
fn a_previous_worker_holding_the_account_lock_blocks_sending() {
    let h = Harness::imap().started();
    let lock = h.engine.account_lock(h.account);
    let guard = lock.lock().unwrap();
    h.db.enqueue_outgoing(
        h.account,
        "me@example.com",
        &strings(&["b@x.com"]),
        b"raw",
        None,
    )
    .unwrap();
    h.run(Command::ProcessQueue);
    assert!(h.connector.sender().sent().is_empty(), "old worker owns it");
    drop(guard);
    // The new worker retries on its own shortly after.
    h.wait_for("Sent", |e| matches!(e, SyncEvent::Sent { .. }));
    assert_eq!(h.connector.sender().sent().len(), 1);
}

#[test]
fn rows_claimed_by_a_dead_worker_become_maybe_sent() {
    let h = Harness::imap().started();
    let fresh =
        h.db.enqueue_outgoing(
            h.account,
            "me@example.com",
            &strings(&["b@x.com"]),
            b"1",
            None,
        )
        .unwrap();
    let stale =
        h.db.enqueue_outgoing(
            h.account,
            "me@example.com",
            &strings(&["c@x.com"]),
            b"2",
            None,
        )
        .unwrap();
    let now = crate::worker::unix_now();
    // Another (live) worker is sending `fresh`; `stale` was claimed by one
    // that died long ago.
    assert!(h.db.claim_outgoing(fresh, now).unwrap());
    assert!(h.db.claim_outgoing(stale, now - 3600).unwrap());
    h.sync();
    assert!(h.connector.sender().sent().is_empty());
    let rows = h.db.outbox(h.account).unwrap();
    assert_eq!(rows[0].state, storage::OutgoingState::Sending);
    assert_eq!(rows[1].state, storage::OutgoingState::MaybeSent);
}

#[test]
fn failed_sent_copy_is_retried_without_resending() {
    let h = Harness::imap().started();
    let raw = mail("outgoing", 9);
    h.db.enqueue_outgoing(
        h.account,
        "me@example.com",
        &strings(&["b@x.com"]),
        &raw,
        None,
    )
    .unwrap();
    h.server.on_next_call("append Sent", {
        let server = h.server.clone();
        move || server.fail_next(RemoteErrorKind::Protocol)
    });
    h.run(Command::ProcessQueue);
    assert_eq!(h.connector.sender().sent().len(), 1);
    assert!(h.server.messages("Sent").is_empty());
    let row = &h.db.outbox(h.account).unwrap()[0];
    assert_eq!(row.state, storage::OutgoingState::AppendPending);
    assert_eq!(h.db.outbox_count().unwrap(), 0, "already sent");
    h.run(Command::ProcessQueue);
    assert_eq!(h.server.messages("Sent").len(), 1);
    assert_eq!(h.connector.sender().sent().len(), 1, "not sent twice");
    assert!(h.db.outbox(h.account).unwrap().is_empty());
}
