//! End to end: sync engine + real IMAP/SMTP clients + SQLite against
//! GreenMail. Ignored by default; run with
//!
//! ```text
//! crates/protocols/tests/greenmail.sh start
//! BGMAIL_GREENMAIL=1 cargo test -p mail_sync --test greenmail_e2e -- --ignored
//! crates/protocols/tests/greenmail.sh stop
//! ```
//!
//! Every run uses unique subjects and folder names, so the long-lived
//! container can be reused.

use std::sync::Arc;
use std::time::{Duration, Instant};

use mail_core::{
    AccountConfig, Address, AuthKind, CancelToken, Draft, MailConnector, MailRemote, MailSender,
    MessageFlags, OutgoingAttachment, Security, ServerConfig,
};
use mail_sync::{actions, Command, Engine, EngineOptions, SyncEvent};
use protocols::{ConnectOptions, Credential, ImapRemote, ImapSmtpConnector, Secret, SmtpSender};
use storage::{system, Database, MailListQuery};

const PASSWORD: &str = "secret";
const WAIT: Duration = Duration::from_secs(30);

fn enabled() -> bool {
    std::env::var_os("BGMAIL_GREENMAIL").is_some()
}

fn config(user: &str) -> AccountConfig {
    AccountConfig {
        email: user.into(),
        username: user.into(),
        imap: ServerConfig {
            host: "127.0.0.1".into(),
            port: 3143,
            security: Security::None,
        },
        smtp: ServerConfig {
            host: "127.0.0.1".into(),
            port: 3025,
            security: Security::None,
        },
        auth: AuthKind::Password,
        sync_window: 500,
    }
}

fn imap(user: &str) -> ImapRemote {
    ImapRemote::connect(
        &config(user).imap,
        user,
        &Secret::Password(PASSWORD.into()),
        ConnectOptions::default(),
        &CancelToken::new(),
    )
    .expect("imap login")
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn deliver(from: &str, to: &str, subject: &str, body: &str) {
    let draft = Draft {
        from: Address::new(Some("Sender"), from),
        to: vec![Address::new(None::<String>, to)],
        subject: subject.into(),
        text: body.into(),
        ..Draft::default()
    };
    let raw =
        mail_core::build_message(&draft, &format!("{}@e2e", subject.replace(' ', ".")), now());
    let mut smtp = SmtpSender::new(
        &config(from).smtp,
        from,
        &Secret::Password(PASSWORD.into()),
        ConnectOptions::default(),
    )
    .expect("smtp");
    smtp.send(from, &[to.to_string()], &raw).expect("send");
}

/// Waits until `check` holds, collecting engine events meanwhile.
fn wait_until(
    events: &async_channel::Receiver<SyncEvent>,
    seen: &mut Vec<SyncEvent>,
    mut check: impl FnMut(&[SyncEvent]) -> bool,
) {
    let deadline = Instant::now() + WAIT;
    loop {
        while let Ok(event) = events.try_recv() {
            seen.push(event);
        }
        if check(seen) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out; events: {seen:#?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn folder_subjects(db: &Database, account_id: i64, path: &str, token: &str) -> Vec<String> {
    let mut subjects: Vec<String> = db
        .list_messages(&MailListQuery::AccountFolder {
            account_id,
            folder_path: path.into(),
        })
        .unwrap()
        .into_iter()
        .map(|m| m.subject)
        .filter(|s| s.contains(token))
        .collect();
    subjects.sort();
    subjects
}

fn message_id_by_subject(db: &Database, account_id: i64, subject: &str) -> i64 {
    db.list_messages(&MailListQuery::Search(subject.into()))
        .unwrap()
        .into_iter()
        .find(|m| m.account_id == account_id && m.subject == subject)
        .unwrap_or_else(|| panic!("no local message {subject}"))
        .id
}

fn server_subjects(
    remote: &mut ImapRemote,
    folder: &str,
    token: &str,
) -> Vec<(String, MessageFlags)> {
    let uids = remote.uids(folder).unwrap_or_default();
    let mut out: Vec<(String, MessageFlags)> = remote
        .fetch_headers(folder, &uids)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|header| {
            let parsed = mail_core::parse_message(&header.header)?;
            parsed
                .subject
                .contains(token)
                .then_some((parsed.subject, header.flags))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
#[ignore = "needs GreenMail (BGMAIL_GREENMAIL=1, crates/protocols/tests/greenmail.sh)"]
fn full_mail_cycle_against_greenmail() {
    if !enabled() {
        return;
    }
    let alice = "alice@localhost";
    let bob = "bob@localhost";
    let token = format!("e2e{}", now() % 1_000_000_000);
    let projects = format!("Projects{token}");

    // Server-side folders a real account would have.
    let mut admin = imap(alice);
    for folder in ["Trash", "Sent", projects.as_str()] {
        let _ = admin.create_folder(folder);
    }
    deliver(bob, alice, &format!("{token} first"), "Hello one");
    deliver(bob, alice, &format!("{token} second"), "Hello two");

    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("mail.db");
    let db = Database::open(&db_path).unwrap();
    let account_config = config(alice);
    let account_id = db
        .create_account(
            "Alice",
            alice,
            storage::PROVIDER_IMAP,
            &account_config.to_json(),
        )
        .unwrap();
    let connector: Arc<dyn MailConnector> = Arc::new(ImapSmtpConnector::new(
        account_config,
        Credential::Password(PASSWORD.into()),
        ConnectOptions::default(),
    ));
    let (sender, events) = async_channel::unbounded();
    let engine = Engine::new(
        db_path.clone(),
        sender,
        EngineOptions {
            poll_interval: Duration::from_secs(2),
            ..EngineOptions::default()
        },
    );
    let mut seen = Vec::new();

    // 1. Initial sync: both messages and the server's folders.
    engine.start_account(account_id, connector);
    wait_until(&events, &mut seen, |_| {
        folder_subjects(&db, account_id, system::INBOX, &token).len() == 2
    });
    let first = message_id_by_subject(&db, account_id, &format!("{token} first"));
    let second = message_id_by_subject(&db, account_id, &format!("{token} second"));
    let folders = db.list_folders(account_id).unwrap();
    assert!(folders.iter().any(|f| f.path == system::TRASH));
    assert!(folders.iter().any(|f| f.path == system::SENT));
    assert!(folders.iter().any(|f| f.display_name == projects));

    // 2. Body on demand.
    engine.send(account_id, Command::FetchBody(first));
    wait_until(&events, &mut seen, |_| {
        db.get_message(first)
            .unwrap()
            .is_some_and(|m| m.body_state == storage::BODY_STATE_FULL)
    });
    assert!(db
        .get_message(first)
        .unwrap()
        .unwrap()
        .plain_text
        .contains("Hello one"));

    // 3. Read + star reach the server.
    actions::set_read(&db, first, true).unwrap();
    actions::set_starred(&db, first, true).unwrap();
    engine.send(account_id, Command::ProcessQueue);
    let mut server = imap(alice);
    wait_until(&events, &mut seen, |_| {
        server_subjects(&mut server, "INBOX", &token)
            .iter()
            .any(|(s, flags)| s.ends_with("first") && flags.seen && flags.flagged)
    });

    // 4. Move to a user folder: the server moves it, the local copy follows
    //    without duplicates.
    let projects_path = storage::user_folder_path(&projects);
    actions::move_to_folder(&db, second, &projects_path).unwrap();
    engine.send(account_id, Command::ProcessQueue);
    wait_until(&events, &mut seen, |_| {
        server_subjects(&mut server, &projects, &token).len() == 1
    });
    engine.send(account_id, Command::SyncNow);
    wait_until(&events, &mut seen, |_| {
        folder_subjects(&db, account_id, &projects_path, &token).len() == 1
            && folder_subjects(&db, account_id, system::INBOX, &token).len() == 1
    });
    let total = db
        .list_messages(&MailListQuery::Search(token.clone()))
        .unwrap()
        .len();
    assert_eq!(total, 2, "moved message duplicated");

    // 5. Trash.
    actions::trash(&db, first).unwrap();
    engine.send(account_id, Command::ProcessQueue);
    wait_until(&events, &mut seen, |_| {
        server_subjects(&mut server, "Trash", &token).len() == 1
            && server_subjects(&mut server, "INBOX", &token).is_empty()
    });

    // 6. Send with an attachment: delivered to Bob, copy filed in Sent.
    let draft = Draft {
        from: Address::new(Some("Alice"), alice),
        to: vec![Address::new(Some("Bob"), bob)],
        subject: format!("{token} outgoing"),
        text: "See attached".into(),
        attachments: vec![OutgoingAttachment {
            filename: "notes.txt".into(),
            mime_type: "text/plain".into(),
            data: b"attached notes".to_vec(),
        }],
        ..Draft::default()
    };
    let message_id = format!("{token}.out@localhost");
    let raw = mail_core::build_message(&draft, &message_id, now());
    db.enqueue_outgoing(
        account_id,
        alice,
        &mail_core::envelope_recipients(&draft),
        &raw,
        Some(&message_id),
    )
    .unwrap();
    engine.send(account_id, Command::ProcessQueue);
    wait_until(&events, &mut seen, |events| {
        events.iter().any(|e| matches!(e, SyncEvent::Sent { .. }))
    });
    let outgoing = format!("{token} outgoing");
    let mut bob_imap = imap(bob);
    assert_eq!(server_subjects(&mut bob_imap, "INBOX", &outgoing).len(), 1);
    let bob_uids = bob_imap.uids("INBOX").unwrap();
    let bob_raw = bob_imap
        .fetch_body("INBOX", *bob_uids.last().unwrap())
        .unwrap();
    let parsed = mail_core::parse_message(&bob_raw).unwrap();
    assert!(parsed.attachments.iter().any(|a| a.filename == "notes.txt"));
    assert_eq!(server_subjects(&mut server, "Sent", &outgoing).len(), 1);
    assert_eq!(db.outbox_count().unwrap(), 0);

    // 7. New mail while idle → NewMail event (the notification source).
    deliver(bob, alice, &format!("{token} late"), "Hello again");
    wait_until(&events, &mut seen, |events| {
        events.iter().any(|e| match e {
            SyncEvent::NewMail { messages, .. } => {
                messages.iter().any(|m| m.subject.ends_with("late"))
            }
            _ => false,
        })
    });

    engine.shutdown();
}
