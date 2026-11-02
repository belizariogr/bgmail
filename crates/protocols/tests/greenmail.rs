//! End-to-end tests against a real IMAP/SMTP server (GreenMail in Docker).
//!
//! Ignored by default and gated by `BGMAIL_GREENMAIL=1`:
//!
//! ```sh
//! crates/protocols/tests/greenmail.sh start
//! BGMAIL_GREENMAIL=1 cargo test -p protocols --test greenmail -- --ignored
//! crates/protocols/tests/greenmail.sh stop
//! ```
//!
//! GreenMail 2.1 announces IDLE, MOVE and UIDPLUS but not CONDSTORE, so the
//! no-modseq paths are what runs here (MOVE's fallback is unit-tested).

use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mail_core::{
    parse_message, AccountConfig, AuthKind, CancelToken, IdleOutcome, MailConnector, MailRemote,
    MailSender, MessageFlags, RemoteErrorKind, Security, ServerConfig,
};
use protocols::{
    test_account, ConnectOptions, Credential, ImapRemote, ImapSmtpConnector, Secret, SmtpSender,
};

const HOST: &str = "127.0.0.1";
const ALICE: &str = "alice@localhost";
const PASSWORD: &str = "secret";

/// Tests share alice's INBOX; run them one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

fn enabled() -> bool {
    std::env::var("BGMAIL_GREENMAIL").as_deref() == Ok("1")
}

macro_rules! gate {
    () => {{
        if !enabled() {
            eprintln!("skipped: set BGMAIL_GREENMAIL=1 (see tests/greenmail.sh)");
            return;
        }
        SERIAL.lock().unwrap_or_else(|p| p.into_inner())
    }};
}

fn server(port: u16, security: Security) -> ServerConfig {
    ServerConfig {
        host: HOST.into(),
        port,
        security,
    }
}

fn insecure() -> ConnectOptions {
    ConnectOptions {
        accept_invalid_certs: true,
    }
}

fn plain_config() -> AccountConfig {
    AccountConfig {
        email: ALICE.into(),
        username: ALICE.into(),
        imap: server(3143, Security::None),
        smtp: server(3025, Security::None),
        auth: AuthKind::Password,
        sync_window: 100,
    }
}

fn nonce() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{nanos:x}")
}

fn imap() -> ImapRemote {
    ImapRemote::connect(
        &server(3143, Security::None),
        ALICE,
        &Secret::Password(PASSWORD.into()),
        ConnectOptions::default(),
        &CancelToken::new(),
    )
    .expect("IMAP login")
}

fn send(subject: &str) {
    let mut sender = SmtpSender::new(
        &server(3025, Security::None),
        ALICE,
        &Secret::Password(PASSWORD.into()),
        ConnectOptions::default(),
    )
    .unwrap();
    let raw = format!(
        "From: Bob <bob@localhost>\r\nTo: Alice <alice@localhost>\r\nSubject: {subject}\r\n\
         Message-ID: <{subject}@localhost>\r\nDate: Sat, 31 Oct 2026 10:00:00 +0000\r\n\r\nHello {subject}\r\n"
    );
    sender
        .send("bob@localhost", &[ALICE.to_string()], raw.as_bytes())
        .expect("SMTP send");
}

/// Waits for a message with `subject` in INBOX; returns its UID.
fn wait_for(remote: &mut ImapRemote, subject: &str) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        remote.select("INBOX").unwrap();
        let uids = remote.uids("INBOX").unwrap();
        let headers = remote.fetch_headers("INBOX", &uids).unwrap();
        if let Some(header) = headers.iter().find(|h| {
            parse_message(&h.header)
                .map(|m| m.subject)
                .is_some_and(|s| s == subject)
        }) {
            return header.uid;
        }
        assert!(Instant::now() < deadline, "{subject} never arrived");
        thread::sleep(Duration::from_millis(200));
    }
}

#[test]
#[ignore]
fn smtp_to_imap_full_cycle() {
    let _serial = gate!();
    let tag = format!("cycle-{}", nonce());
    send(&tag);

    let mut remote = imap();
    let caps = remote.capabilities();
    assert!(caps.idle && caps.r#move && caps.uidplus, "{caps:?}");
    assert!(!caps.condstore && !caps.gmail);

    let folders = remote.list_folders().unwrap();
    assert!(folders.iter().any(|f| f.name == "INBOX"), "{folders:?}");

    let status = remote.select("INBOX").unwrap();
    assert!(status.uid_validity > 0);
    assert!(status.uid_next.is_some());
    assert_eq!(status.highest_modseq, None);

    let uid = wait_for(&mut remote, &tag);
    let header = remote.fetch_headers("INBOX", &[uid]).unwrap().remove(0);
    assert!(header.size > 0);
    assert_eq!(header.has_attachments, Some(false));
    assert_eq!(header.gm_msgid, None);

    // BODY.PEEK must not mark it read.
    let body = remote.fetch_body("INBOX", uid).unwrap();
    assert!(String::from_utf8_lossy(&body).contains(&format!("Hello {tag}")));
    let flags = remote.fetch_flags("INBOX", &[uid], None).unwrap();
    assert!(!flags[0].flags.seen, "{flags:?}");
    assert_eq!(
        remote.fetch_body("INBOX", 999_999).unwrap_err().kind,
        RemoteErrorKind::NotFound
    );

    let seen_flagged = MessageFlags {
        seen: true,
        flagged: true,
        ..Default::default()
    };
    remote
        .store_flags("INBOX", &[uid], seen_flagged, MessageFlags::default())
        .unwrap();
    // No CONDSTORE: `changed_since` is ignored and every UID comes back.
    let flags = remote.fetch_flags("INBOX", &[uid], Some(1)).unwrap();
    assert!(flags[0].flags.seen && flags[0].flags.flagged);
    let unflag = MessageFlags {
        flagged: true,
        ..Default::default()
    };
    remote
        .store_flags("INBOX", &[uid], MessageFlags::default(), unflag)
        .unwrap();
    let flags = remote.fetch_flags("INBOX", &[uid], None).unwrap();
    assert!(flags[0].flags.seen && !flags[0].flags.flagged);

    // Move into a fresh folder; GreenMail reports COPYUID.
    let folder = format!("Archive-{}", nonce());
    remote.create_folder(&folder).unwrap();
    assert!(remote
        .list_folders()
        .unwrap()
        .iter()
        .any(|f| f.name == folder));
    let moved = remote.move_messages("INBOX", &[uid], &folder).unwrap();
    assert!(!remote.uids("INBOX").unwrap().contains(&uid));
    let archive_uids = remote.uids(&folder).unwrap();
    assert_eq!(archive_uids.len(), 1);
    if let Some(new_uids) = &moved {
        assert_eq!(new_uids, &archive_uids);
    }
    eprintln!("move returned {moved:?}");

    remote.delete_messages(&folder, &archive_uids).unwrap();
    assert!(remote.uids(&folder).unwrap().is_empty());

    let raw = format!("Subject: appended {tag}\r\nFrom: a@localhost\r\n\r\nbody\r\n");
    let seen = MessageFlags {
        seen: true,
        ..Default::default()
    };
    remote.append(&folder, seen, raw.as_bytes()).unwrap();
    let uids = remote.uids(&folder).unwrap();
    assert_eq!(uids.len(), 1);
    let header = remote.fetch_headers(&folder, &uids).unwrap().remove(0);
    assert!(header.flags.seen);
    assert_eq!(
        parse_message(&header.header).map(|m| m.subject),
        Some(format!("appended {tag}"))
    );

    assert_eq!(
        remote.select("Does-Not-Exist").unwrap_err().kind,
        RemoteErrorKind::NotFound
    );
    remote.noop().unwrap();
    remote.logout();
}

#[test]
#[ignore]
fn idle_wakes_up_on_delivery_and_times_out() {
    let _serial = gate!();
    let mut remote = imap();
    remote.select("INBOX").unwrap();
    let tag = format!("idle-{}", nonce());
    let subject = tag.clone();
    let sender = thread::spawn(move || {
        thread::sleep(Duration::from_millis(800));
        send(&subject);
    });
    let started = Instant::now();
    let outcome = remote
        .idle("INBOX", Duration::from_secs(30), &CancelToken::new())
        .unwrap();
    assert_eq!(outcome, IdleOutcome::Changed);
    assert!(started.elapsed() < Duration::from_secs(20));
    sender.join().unwrap();

    let outcome = remote
        .idle("INBOX", Duration::from_secs(1), &CancelToken::new())
        .unwrap();
    assert_eq!(outcome, IdleOutcome::Timeout);
    // Still usable after DONE.
    remote.noop().unwrap();
    wait_for(&mut remote, &tag);
    remote.logout();
}

#[test]
#[ignore]
fn cancel_during_idle_returns_fast() {
    let _serial = gate!();
    let mut remote = imap();
    let cancel = CancelToken::new();
    let trigger = cancel.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        trigger.cancel();
    });
    let started = Instant::now();
    let outcome = remote
        .idle("INBOX", Duration::from_secs(60), &cancel)
        .unwrap();
    assert_eq!(outcome, IdleOutcome::Stopped);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    remote.logout();
}

#[test]
#[ignore]
fn connector_cancel_interrupts_the_session() {
    let _serial = gate!();
    let connector = ImapSmtpConnector::new(
        plain_config(),
        Credential::Password(PASSWORD.into()),
        ConnectOptions::default(),
    );
    let cancel = CancelToken::new();
    let mut remote = connector.connect(&cancel).unwrap();
    remote.select("INBOX").unwrap();
    let trigger = cancel.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        trigger.cancel();
    });
    let started = Instant::now();
    // Session token cancels IDLE even with an unrelated idle token.
    let outcome = remote
        .idle("INBOX", Duration::from_secs(60), &CancelToken::new())
        .unwrap();
    assert_eq!(outcome, IdleOutcome::Stopped);
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(remote.noop().unwrap_err().kind, RemoteErrorKind::Cancelled);
    remote.logout();
}

#[test]
#[ignore]
fn tls_ports_with_self_signed_certificate() {
    let _serial = gate!();
    let mut remote = ImapRemote::connect(
        &server(3993, Security::Tls),
        ALICE,
        &Secret::Password(PASSWORD.into()),
        insecure(),
        &CancelToken::new(),
    )
    .expect("IMAPS login");
    assert!(!remote.list_folders().unwrap().is_empty());
    remote.select("INBOX").unwrap();
    // IDLE timeout over TLS leaves the stream usable.
    assert_eq!(
        remote
            .idle("INBOX", Duration::from_millis(500), &CancelToken::new())
            .unwrap(),
        IdleOutcome::Timeout
    );
    remote.noop().unwrap();
    remote.logout();

    let err = ImapRemote::connect(
        &server(3993, Security::Tls),
        ALICE,
        &Secret::Password(PASSWORD.into()),
        ConnectOptions::default(),
        &CancelToken::new(),
    )
    .unwrap_err();
    assert_eq!(err.kind, RemoteErrorKind::Tls, "{err}");

    let tag = format!("smtps-{}", nonce());
    let mut sender = SmtpSender::new(
        &server(3465, Security::Tls),
        ALICE,
        &Secret::Password(PASSWORD.into()),
        insecure(),
    )
    .unwrap();
    sender.verify().unwrap();
    sender
        .send(
            ALICE,
            &[ALICE.to_string()],
            format!("Subject: {tag}\r\n\r\nvia smtps\r\n").as_bytes(),
        )
        .unwrap();
    let mut remote = imap();
    wait_for(&mut remote, &tag);
    remote.logout();

    let strict = SmtpSender::new(
        &server(3465, Security::Tls),
        ALICE,
        &Secret::Password(PASSWORD.into()),
        ConnectOptions::default(),
    )
    .unwrap();
    assert_eq!(strict.verify().unwrap_err().kind, RemoteErrorKind::Tls);
}

#[test]
#[ignore]
fn wrong_password_is_an_auth_error() {
    let _serial = gate!();
    let err = ImapRemote::connect(
        &server(3143, Security::None),
        ALICE,
        &Secret::Password("wrong".into()),
        ConnectOptions::default(),
        &CancelToken::new(),
    )
    .unwrap_err();
    assert_eq!(err.kind, RemoteErrorKind::Auth, "{err}");
    let sender = SmtpSender::new(
        &server(3025, Security::None),
        ALICE,
        &Secret::Password("wrong".into()),
        ConnectOptions::default(),
    )
    .unwrap();
    let err = sender.verify().unwrap_err();
    assert_eq!(err.kind, RemoteErrorKind::Auth, "{err}");
    let err = test_account(
        plain_config(),
        Credential::Password("wrong".into()),
        ConnectOptions::default(),
    )
    .unwrap_err();
    assert_eq!(err.kind, RemoteErrorKind::Auth);
}

#[test]
#[ignore]
fn test_account_and_connector_sender() {
    let _serial = gate!();
    test_account(
        plain_config(),
        Credential::Password(PASSWORD.into()),
        ConnectOptions::default(),
    )
    .unwrap();
    let connector = ImapSmtpConnector::new(
        plain_config(),
        Credential::Password(PASSWORD.into()),
        ConnectOptions::default(),
    );
    assert!(!connector.saves_sent_copy());
    let tag = format!("connector-{}", nonce());
    let mut sender = connector.connect_sender(&CancelToken::new()).unwrap();
    sender
        .send(
            ALICE,
            &[ALICE.to_string()],
            format!("Subject: {tag}\r\n\r\nhi\r\n").as_bytes(),
        )
        .unwrap();
    let mut remote = imap();
    wait_for(&mut remote, &tag);
    remote.logout();
}

// --- Dovecot (optional): CONDSTORE, MOVE, STARTTLS -------------------------

fn dovecot_enabled() -> bool {
    std::env::var("BGMAIL_DOVECOT").as_deref() == Ok("1")
}

fn dovecot(security: Security, port: u16, user: &str) -> ImapRemote {
    ImapRemote::connect(
        &server(port, security),
        user,
        &Secret::Password("pass".into()),
        insecure(),
        &CancelToken::new(),
    )
    .expect("Dovecot login")
}

#[test]
#[ignore]
fn dovecot_starttls_condstore_and_move() {
    if !dovecot_enabled() {
        eprintln!("skipped: set BGMAIL_DOVECOT=1 (see tests/greenmail.sh)");
        return;
    }
    // A fresh user per run gets an empty mailbox.
    let user = format!("u{}", nonce());
    let mut remote = dovecot(Security::StartTls, 31143, &user);
    let caps = remote.capabilities();
    assert!(
        caps.idle && caps.r#move && caps.uidplus && caps.condstore,
        "{caps:?}"
    );

    let seen = MessageFlags {
        seen: true,
        ..Default::default()
    };
    for n in 0..3 {
        let raw = format!("Subject: d{n}\r\nFrom: a@b.c\r\n\r\nbody {n}\r\n");
        remote
            .append("INBOX", MessageFlags::default(), raw.as_bytes())
            .unwrap();
    }
    let status = remote.select("INBOX").unwrap();
    assert_eq!(status.exists, 3);
    let modseq = status.highest_modseq.expect("HIGHESTMODSEQ with CONDSTORE");
    let uids = remote.uids("INBOX").unwrap();
    assert_eq!(uids.len(), 3);
    let headers = remote.fetch_headers("INBOX", &uids).unwrap();
    assert!(headers.iter().all(|h| h.modseq.is_some()));

    remote
        .store_flags("INBOX", &uids[..1], seen, MessageFlags::default())
        .unwrap();
    let changed = remote.fetch_flags("INBOX", &uids, Some(modseq)).unwrap();
    assert_eq!(changed.len(), 1, "{changed:?}");
    assert_eq!(changed[0].uid, uids[0]);
    assert!(changed[0].flags.seen && changed[0].modseq > Some(modseq));

    remote.create_folder("Archive").unwrap();
    let moved = remote
        .move_messages("INBOX", &uids[1..], "Archive")
        .unwrap()
        .expect("COPYUID from UID MOVE");
    assert_eq!(moved, remote.uids("Archive").unwrap());
    assert_eq!(remote.uids("INBOX").unwrap(), vec![uids[0]]);
    remote.logout();

    // Implicit TLS on 31993 sees the same mailbox.
    let mut tls = dovecot(Security::Tls, 31993, &user);
    assert_eq!(tls.uids("Archive").unwrap().len(), 2);
    tls.logout();
}

/// UID re-resolution by Message-ID (used after a UIDVALIDITY change or a
/// move without COPYUID) against a real server's `UID SEARCH HEADER`.
#[test]
#[ignore]
fn find_uid_by_message_id_against_greenmail() {
    let _serial = gate!();
    let tag = format!("find-{}", nonce());
    send(&tag);
    let mut remote = imap();
    let uid = wait_for(&mut remote, &tag);
    let id = format!("{tag}@localhost");
    assert_eq!(
        remote.find_uid_by_message_id("INBOX", &id).unwrap(),
        Some(uid)
    );
    assert_eq!(
        remote
            .find_uid_by_message_id("INBOX", &format!("<{id}>"))
            .unwrap(),
        Some(uid)
    );
    assert_eq!(
        remote
            .find_uid_by_message_id("INBOX", &format!("missing-{id}"))
            .unwrap(),
        None
    );
    remote.delete_messages("INBOX", &[uid]).unwrap();
    assert_eq!(remote.find_uid_by_message_id("INBOX", &id).unwrap(), None);
    remote.logout();
}

/// SMTP failures are classified by phase against a real server: a rejected
/// recipient (before DATA) is never "maybe sent".
#[test]
#[ignore]
fn smtp_rejections_before_data_are_not_uncertain() {
    let _serial = gate!();
    let mut sender = SmtpSender::new(
        &server(3025, Security::None),
        ALICE,
        &Secret::Password(PASSWORD.into()),
        ConnectOptions::default(),
    )
    .unwrap();
    let err = sender
        .send("bob@localhost", &["not an address".to_string()], b"x\r\n")
        .unwrap_err();
    assert_eq!(err.failure, mail_core::SendFailure::Permanent);
}
