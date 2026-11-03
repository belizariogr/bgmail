//! SMTP submission over lettre's low-level [`SmtpConnection`]: the message
//! bytes come from `mail_core::build_message` and are submitted command by
//! command (MAIL / RCPT / DATA / message) so a failure can be classified by
//! the phase it happened in — a broken connection *before* the message data
//! is safe to retry, one *after* it may have delivered the message
//! ([`SendFailure::Uncertain`]).
//!
//! lettre owns its sockets, so a [`CancelToken`](mail_core::CancelToken) can
//! only stop a send before it starts; the 60 s I/O timeout bounds the rest.

use lettre::address::{Address, Envelope};
use lettre::transport::smtp as smtp_transport;
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::transport::smtp::client::{SmtpConnection, TlsParameters};
use lettre::transport::smtp::commands::{Data, Mail, Noop, Rcpt};
use lettre::transport::smtp::extension::{ClientId, Extension, MailBodyParameter, MailParameter};
use lettre::transport::smtp::response::Response;
use mail_core::{
    MailSender, RemoteError, RemoteErrorKind, RemoteResult, Security, SendError, ServerConfig,
};

use crate::connector::Secret;
use crate::tls::{ConnectOptions, IO_TIMEOUT};

/// How the session is encrypted.
#[derive(Clone)]
enum SmtpTls {
    /// Implicit TLS from the first byte (port 465).
    Wrapper(TlsParameters),
    /// Mandatory STARTTLS upgrade after EHLO.
    StartTls(TlsParameters),
    None,
}

/// An SMTP submission client for one account (one connection per send).
pub struct SmtpSender {
    host: String,
    port: u16,
    tls: SmtpTls,
    hello: ClientId,
    credentials: Credentials,
    mechanisms: Vec<Mechanism>,
}

impl std::fmt::Debug for SmtpSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmtpSender").finish_non_exhaustive()
    }
}

/// Where a submission failed (see [`classify`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendPhase {
    /// Connect, TLS, EHLO, AUTH: nothing about the message was sent.
    Connect,
    /// MAIL / RCPT / DATA commands: the server has not got the message.
    Envelope,
    /// Message bytes + terminator + final reply: may have been accepted.
    Message,
}

impl SmtpSender {
    /// Prepares the client (no network yet): implicit TLS for
    /// [`Security::Tls`], mandatory STARTTLS for [`Security::StartTls`],
    /// cleartext for [`Security::None`]. Passwords authenticate with
    /// PLAIN/LOGIN, access tokens with XOAUTH2.
    pub fn new(
        server: &ServerConfig,
        username: &str,
        secret: &Secret,
        options: ConnectOptions,
    ) -> RemoteResult<Self> {
        let options = options.for_host(&server.host);
        let params = || {
            TlsParameters::builder(server.host.clone())
                .dangerous_accept_invalid_certs(options.accept_invalid_certs)
                .dangerous_accept_invalid_hostnames(options.accept_invalid_certs)
                .build_rustls()
                .map_err(map_error)
        };
        let tls = match server.security {
            Security::Tls => SmtpTls::Wrapper(params()?),
            Security::StartTls => SmtpTls::StartTls(params()?),
            Security::None => SmtpTls::None,
        };
        let (password, mechanisms) = match secret {
            Secret::Password(password) => {
                (password.clone(), vec![Mechanism::Plain, Mechanism::Login])
            }
            Secret::AccessToken(token) => (token.clone(), vec![Mechanism::Xoauth2]),
        };
        Ok(Self {
            host: server.host.clone(),
            port: server.port,
            tls,
            hello: ClientId::default(),
            credentials: Credentials::new(username.to_string(), password),
            mechanisms,
        })
    }

    /// Opens an authenticated session (connect, TLS, EHLO, AUTH).
    fn connect(&self) -> Result<SmtpConnection, smtp_transport::Error> {
        let wrapper = match &self.tls {
            SmtpTls::Wrapper(params) => Some(params),
            _ => None,
        };
        let mut conn = SmtpConnection::connect(
            (self.host.as_str(), self.port),
            Some(IO_TIMEOUT),
            &self.hello,
            wrapper,
            None,
        )?;
        let authenticated = (|| {
            if let SmtpTls::StartTls(params) = &self.tls {
                conn.starttls(params, &self.hello)?;
            }
            conn.auth(&self.mechanisms, &self.credentials)
        })();
        match authenticated {
            Ok(_) => Ok(conn),
            Err(err) => {
                conn.abort();
                Err(err)
            }
        }
    }

    /// Connects, negotiates TLS and authenticates, then quits. Used by the
    /// "Add account" dialog to validate settings before saving.
    pub fn verify(&self) -> RemoteResult<()> {
        let mut conn = self.connect().map_err(map_error)?;
        let alive = conn.command(Noop).is_ok();
        let _ = conn.quit();
        if alive {
            Ok(())
        } else {
            Err(RemoteError::new(
                RemoteErrorKind::Network,
                "SMTP server closed the connection",
            ))
        }
    }
}

impl MailSender for SmtpSender {
    fn send(&mut self, from: &str, recipients: &[String], raw: &[u8]) -> Result<(), SendError> {
        let envelope = envelope(from, recipients).map_err(SendError::permanent)?;
        let mut conn = self
            .connect()
            .map_err(|err| classify(err, SendPhase::Connect))?;
        match transaction(&mut conn, &envelope, raw) {
            Ok(()) => {
                let _ = conn.quit();
                Ok(())
            }
            Err((err, phase)) => {
                conn.abort();
                Err(classify(err, phase))
            }
        }
    }
}

/// MAIL FROM (+ SMTPUTF8 / 8BITMIME when needed), RCPT TO each, DATA, then
/// the message; on failure also reports the phase it failed in.
fn transaction(
    conn: &mut SmtpConnection,
    envelope: &Envelope,
    raw: &[u8],
) -> Result<(), (smtp_transport::Error, SendPhase)> {
    let envelope_step = |result: Result<Response, smtp_transport::Error>| {
        result.map(|_| ()).map_err(|err| (err, SendPhase::Envelope))
    };
    let mut options = Vec::new();
    let non_ascii_address = envelope
        .from()
        .into_iter()
        .chain(envelope.to())
        .any(|address| !AsRef::<str>::as_ref(address).is_ascii());
    if non_ascii_address && conn.server_info().supports_feature(Extension::SmtpUtfEight) {
        options.push(MailParameter::SmtpUtfEight);
    }
    if !raw.is_ascii() && conn.server_info().supports_feature(Extension::EightBitMime) {
        options.push(MailParameter::Body(MailBodyParameter::EightBitMime));
    }
    envelope_step(conn.command(Mail::new(envelope.from().cloned(), options)))?;
    for to in envelope.to() {
        envelope_step(conn.command(Rcpt::new(to.clone(), Vec::new())))?;
    }
    envelope_step(conn.command(Data))?;
    conn.message(raw)
        .map(|_| ())
        .map_err(|err| (err, SendPhase::Message))
}

/// Classifies a submission failure:
/// - `Connect`: always transient (nothing about the message was sent; auth
///   or server trouble is the account's problem, not the message's).
/// - `Envelope` / `Message` with a reply code: `4xx` transient, `5xx`
///   permanent (auth codes stay transient).
/// - `Envelope` without a code (connection lost, timeout): transient — the
///   server never received the end of the data.
/// - `Message` without a code: **uncertain** — the terminator may have
///   reached the server, which may have queued the message.
/// - Local client errors (e.g. 8-bit data the server cannot take):
///   permanent.
pub fn classify(err: smtp_transport::Error, phase: SendPhase) -> SendError {
    let code = err.status();
    let local = err.is_client() && code.is_none();
    let error = map_error(err);
    match (phase, code) {
        (SendPhase::Connect, _) => SendError::transient(error),
        _ if error.kind == RemoteErrorKind::Auth => SendError::transient(error),
        (_, Some(code)) if code.to_string().starts_with('4') => SendError::transient(error),
        (_, Some(_)) => SendError::permanent(error),
        _ if local => SendError::permanent(error),
        (SendPhase::Envelope, None) => SendError::transient(error),
        (SendPhase::Message, None) => SendError::uncertain(error),
    }
}

/// SMTP envelope (`MAIL FROM` / `RCPT TO`); invalid addresses are `Other`.
pub fn envelope(from: &str, recipients: &[String]) -> RemoteResult<Envelope> {
    let parse = |value: &str| {
        value.trim().parse::<Address>().map_err(|err| {
            RemoteError::new(
                RemoteErrorKind::Other,
                format!("invalid address {value:?}: {err}"),
            )
        })
    };
    let from = parse(from)?;
    let to = recipients
        .iter()
        .map(|r| parse(r))
        .collect::<RemoteResult<Vec<_>>>()?;
    Envelope::new(Some(from), to)
        .map_err(|err| RemoteError::new(RemoteErrorKind::Other, err.to_string()))
}

/// lettre errors → kinds: 530/534/535 (authentication) → Auth, other server
/// replies → Protocol, TLS → Tls, connection/timeouts → Network. The kind is
/// descriptive only; whether a send may be retried is decided by
/// [`classify`].
pub fn map_error(err: smtp_transport::Error) -> RemoteError {
    let message = err.to_string();
    let kind = if let Some(code) = err.status() {
        match code.to_string().as_str() {
            "530" | "534" | "535" => RemoteErrorKind::Auth,
            "454" if is_auth_reply(&message) => RemoteErrorKind::Auth,
            _ => RemoteErrorKind::Protocol,
        }
    } else if err.is_tls() || caused_by_tls(&err) {
        RemoteErrorKind::Tls
    } else if err.is_timeout() || err.is_client() || err.is_transport_shutdown() {
        RemoteErrorKind::Network
    } else if err.is_response() {
        RemoteErrorKind::Protocol
    } else {
        RemoteErrorKind::Network
    };
    RemoteError::new(kind, message)
}

/// Whether a rustls error sits anywhere in the source chain (lettre reports
/// certificate failures during the handshake as connection errors).
fn caused_by_tls(err: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(err);
    while let Some(err) = current {
        if err.downcast_ref::<rustls::Error>().is_some() {
            return true;
        }
        if let Some(io) = err.downcast_ref::<std::io::Error>() {
            if io
                .get_ref()
                .is_some_and(|inner| inner.downcast_ref::<rustls::Error>().is_some())
            {
                return true;
            }
        }
        current = err.source();
    }
    false
}

/// 454 is "temporary authentication failure" only when the text says so
/// (it is also used for "TLS not available").
fn is_auth_reply(message: &str) -> bool {
    message.to_ascii_lowercase().contains("auth")
}

#[cfg(test)]
pub(crate) mod test_server {
    //! Minimal SMTP server: AUTH PLAIN/LOGIN/XOAUTH2 accepting one secret,
    //! records the envelope and data of each message.

    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    #[derive(Clone, Debug, Default)]
    pub struct Received {
        pub auth: Vec<String>,
        pub from: String,
        pub to: Vec<String>,
        pub data: String,
    }

    /// `secret` is matched as a substring of the decoded AUTH payload.
    pub fn start(secret: &'static str) -> (u16, Arc<Mutex<Vec<Received>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let log = Arc::new(Mutex::new(Vec::new()));
        let out = log.clone();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let log = out.clone();
                thread::spawn(move || {
                    let _ = session(stream, secret, log);
                });
            }
        });
        (port, log)
    }

    fn session(
        stream: std::net::TcpStream,
        secret: &str,
        log: Arc<Mutex<Vec<Received>>>,
    ) -> std::io::Result<()> {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD;
        let mut writer = stream.try_clone()?;
        let mut reader = BufReader::new(stream);
        let mut current = Received::default();
        writer.write_all(b"220 fake ESMTP\r\n")?;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line)? == 0 {
                return Ok(());
            }
            let line = line.trim_end().to_string();
            let upper = line.to_ascii_uppercase();
            if upper.starts_with("EHLO") {
                writer.write_all(b"250-fake\r\n250 AUTH PLAIN LOGIN XOAUTH2\r\n")?;
            } else if let Some(rest) = upper.strip_prefix("AUTH ") {
                let mechanism = rest.split_whitespace().next().unwrap_or("").to_string();
                let mut payload = line.split_whitespace().nth(2).map(str::to_string);
                if mechanism == "LOGIN" {
                    writer.write_all(b"334 VXNlcm5hbWU6\r\n")?;
                    let mut user = String::new();
                    reader.read_line(&mut user)?;
                    writer.write_all(b"334 UGFzc3dvcmQ6\r\n")?;
                    let mut pass = String::new();
                    reader.read_line(&mut pass)?;
                    payload = Some(pass.trim().to_string());
                } else if payload.is_none() {
                    writer.write_all(b"334 \r\n")?;
                    let mut p = String::new();
                    reader.read_line(&mut p)?;
                    payload = Some(p.trim().to_string());
                }
                let decoded = payload
                    .and_then(|p| b64.decode(p).ok())
                    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                    .unwrap_or_default();
                current.auth.push(format!("{mechanism}:{decoded}"));
                if decoded.contains(secret) {
                    writer.write_all(b"235 ok\r\n")?;
                } else {
                    writer.write_all(b"535 5.7.8 Authentication credentials invalid\r\n")?;
                }
            } else if upper.starts_with("MAIL FROM:") {
                current.from = line[10..].trim().to_string();
                if current.from.contains("hangup") {
                    return Ok(()); // connection lost before the data
                }
                writer.write_all(b"250 ok\r\n")?;
            } else if upper.starts_with("RCPT TO:") {
                let to = line[8..].trim().to_string();
                if to.contains("reject") {
                    writer.write_all(b"550 5.1.1 no such user\r\n")?;
                } else if to.contains("tempfail") {
                    writer.write_all(b"451 4.3.0 try again later\r\n")?;
                } else {
                    current.to.push(to);
                    writer.write_all(b"250 ok\r\n")?;
                }
            } else if upper == "DATA" {
                writer.write_all(b"354 go\r\n")?;
                loop {
                    let mut data = String::new();
                    reader.read_line(&mut data)?;
                    if data == ".\r\n" {
                        break;
                    }
                    current.data.push_str(&data);
                }
                let data = current.data.clone();
                log.lock().unwrap().push(std::mem::take(&mut current));
                if data.contains("X-Test: hangup") {
                    return Ok(()); // accepted, but the final reply is lost
                } else if data.contains("X-Test: defer") {
                    writer.write_all(b"452 4.3.1 queue full\r\n")?;
                } else if data.contains("X-Test: refuse") {
                    writer.write_all(b"554 5.7.1 looks like spam\r\n")?;
                } else {
                    writer.write_all(b"250 queued\r\n")?;
                }
            } else if upper == "QUIT" {
                // Logged before replying so a client that saw `221` can
                // assert on the log without racing this thread.
                if !current.auth.is_empty() {
                    log.lock().unwrap().push(current);
                }
                writer.write_all(b"221 bye\r\n")?;
                return Ok(());
            } else {
                writer.write_all(b"250 ok\r\n")?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_core::SendFailure;

    fn server(port: u16) -> ServerConfig {
        ServerConfig {
            host: "127.0.0.1".into(),
            port,
            security: Security::None,
        }
    }

    #[test]
    fn sends_raw_with_password() {
        let (port, log) = test_server::start("secret");
        let mut sender = SmtpSender::new(
            &server(port),
            "me@example.com",
            &Secret::Password("secret".into()),
            ConnectOptions::default(),
        )
        .unwrap();
        let raw = b"From: me@example.com\r\nTo: you@example.com\r\nSubject: hi\r\n\r\nbody\r\n";
        sender
            .send(
                "me@example.com",
                &["you@example.com".into(), "bcc@example.com".into()],
                raw,
            )
            .unwrap();
        let received = log.lock().unwrap().clone();
        let message = received.iter().find(|r| !r.data.is_empty()).unwrap();
        assert_eq!(message.from, "<me@example.com>");
        assert_eq!(message.to, vec!["<you@example.com>", "<bcc@example.com>"]);
        assert!(message.data.contains("Subject: hi"));
        assert!(message.auth[0].starts_with("PLAIN:"));
        assert!(format!("{sender:?}").contains("SmtpSender"));
    }

    #[test]
    fn xoauth2_and_verify() {
        let (port, log) = test_server::start("auth=Bearer tok");
        let sender = SmtpSender::new(
            &server(port),
            "me@example.com",
            &Secret::AccessToken("tok".into()),
            ConnectOptions::default(),
        )
        .unwrap();
        sender.verify().unwrap();
        let received = log.lock().unwrap().clone();
        assert!(received[0].auth[0].starts_with("XOAUTH2:user=me@example.com"));
    }

    #[test]
    fn wrong_password_is_auth_error() {
        let (port, _log) = test_server::start("secret");
        let sender = SmtpSender::new(
            &server(port),
            "me",
            &Secret::Password("nope".into()),
            ConnectOptions::default(),
        )
        .unwrap();
        assert_eq!(sender.verify().unwrap_err().kind, RemoteErrorKind::Auth);
    }

    #[test]
    fn rejected_recipient_is_protocol_error() {
        let (port, _log) = test_server::start("secret");
        let mut sender = SmtpSender::new(
            &server(port),
            "me",
            &Secret::Password("secret".into()),
            ConnectOptions::default(),
        )
        .unwrap();
        let err = sender
            .send("me@example.com", &["reject@example.com".into()], b"x\r\n")
            .unwrap_err();
        assert_eq!(err.error.kind, RemoteErrorKind::Protocol, "{err}");
        assert_eq!(err.failure, SendFailure::Permanent);
    }

    #[test]
    fn send_failures_are_classified_by_phase() {
        let (port, log) = test_server::start("secret");
        let mut sender = SmtpSender::new(
            &server(port),
            "me",
            &Secret::Password("secret".into()),
            ConnectOptions::default(),
        )
        .unwrap();
        let to = |r: &str| vec![r.to_string()];
        let mut send = |from: &str, rcpt: &str, raw: &[u8]| {
            sender.send(from, &to(rcpt), raw).map_err(|e| e.failure)
        };
        // 4xx on RCPT: transient.
        assert_eq!(
            send("me@example.com", "tempfail@example.com", b"x\r\n"),
            Err(SendFailure::Transient)
        );
        // Connection lost before DATA: transient (nothing delivered).
        assert_eq!(
            send("hangup@example.com", "a@example.com", b"x\r\n"),
            Err(SendFailure::Transient)
        );
        // 4xx / 5xx after the data: not accepted.
        assert_eq!(
            send(
                "me@example.com",
                "a@example.com",
                b"X-Test: defer\r\n\r\nx\r\n"
            ),
            Err(SendFailure::Transient)
        );
        assert_eq!(
            send(
                "me@example.com",
                "a@example.com",
                b"X-Test: refuse\r\n\r\nx\r\n"
            ),
            Err(SendFailure::Permanent)
        );
        // Connection lost after the terminator: maybe delivered.
        assert_eq!(
            send(
                "me@example.com",
                "a@example.com",
                b"X-Test: hangup\r\n\r\nx\r\n"
            ),
            Err(SendFailure::Uncertain)
        );
        assert!(log
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.data.contains("X-Test: hangup")));
        // A bad address never reaches the network.
        assert_eq!(
            send("not an address", "a@example.com", b"x\r\n"),
            Err(SendFailure::Permanent)
        );
    }

    #[test]
    fn connect_phase_failures_are_transient() {
        let (port, _log) = test_server::start("secret");
        let mut sender = SmtpSender::new(
            &server(port),
            "me",
            &Secret::Password("wrong".into()),
            ConnectOptions::default(),
        )
        .unwrap();
        let err = sender
            .send("me@example.com", &["a@example.com".into()], b"x\r\n")
            .unwrap_err();
        assert_eq!(err.error.kind, RemoteErrorKind::Auth);
        assert_eq!(err.failure, SendFailure::Transient);
    }

    #[test]
    fn unreachable_server_is_network_error() {
        let sender = SmtpSender::new(
            &server(crate::tls::test_support::REFUSED_PORT),
            "me",
            &Secret::Password("x".into()),
            ConnectOptions::default(),
        )
        .unwrap();
        assert_eq!(sender.verify().unwrap_err().kind, RemoteErrorKind::Network);
    }

    #[test]
    fn tls_modes_build() {
        for security in [Security::Tls, Security::StartTls] {
            let config = ServerConfig {
                host: "smtp.example.com".into(),
                port: 465,
                security,
            };
            assert!(SmtpSender::new(
                &config,
                "me",
                &Secret::Password("x".into()),
                ConnectOptions::default()
            )
            .is_ok());
        }
    }

    #[test]
    fn envelope_validates_addresses() {
        let env = envelope("me@example.com", &["a@b.com".into()]).unwrap();
        assert_eq!(env.to().len(), 1);
        assert_eq!(
            envelope("not an address", &["a@b.com".into()])
                .unwrap_err()
                .kind,
            RemoteErrorKind::Other
        );
        assert_eq!(
            envelope("me@example.com", &[]).unwrap_err().kind,
            RemoteErrorKind::Other
        );
    }

    #[test]
    fn tls_errors_are_found_in_the_source_chain() {
        let io = std::io::Error::new(std::io::ErrorKind::InvalidData, rustls::Error::DecryptError);
        assert!(caused_by_tls(&io));
        assert!(caused_by_tls(&rustls::Error::DecryptError));
        assert!(!caused_by_tls(&std::io::Error::from(
            std::io::ErrorKind::ConnectionRefused
        )));
    }

    #[test]
    fn self_signed_smtps_needs_accept_invalid_certs() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for tcp in listener.incoming().flatten() {
                std::thread::spawn(move || {
                    use std::io::{BufRead, BufReader, Write};
                    let Ok(mut tls) = crate::tls::test_support::accept_tls(tcp) else {
                        return;
                    };
                    let _ = tls.write_all(b"220 fake\r\n");
                    let _ = tls.flush();
                    let mut reader = BufReader::new(tls);
                    let mut line = String::new();
                    while reader.read_line(&mut line).unwrap_or(0) > 0 {
                        let reply: &[u8] = if line.starts_with("EHLO") {
                            b"250-fake\r\n250 AUTH PLAIN\r\n"
                        } else if line.starts_with("AUTH") {
                            b"235 ok\r\n"
                        } else if line.starts_with("QUIT") {
                            b"221 bye\r\n"
                        } else {
                            b"250 ok\r\n"
                        };
                        let _ = reader.get_mut().write_all(reply);
                        let _ = reader.get_mut().flush();
                        line.clear();
                    }
                });
            }
        });
        // By name, a self-signed certificate takes the option.
        let named = ServerConfig {
            host: "localhost".into(),
            port,
            security: Security::Tls,
        };
        let secret = Secret::Password("x".into());
        let strict = SmtpSender::new(&named, "me", &secret, ConnectOptions::default()).unwrap();
        assert_eq!(strict.verify().unwrap_err().kind, RemoteErrorKind::Tls);
        let lax = SmtpSender::new(
            &named,
            "me",
            &secret,
            ConnectOptions {
                accept_invalid_certs: true,
            },
        )
        .unwrap();
        lax.verify().unwrap();

        // On a loopback address it isn't validated, as for Proton Bridge. The fixture
        // certificate is for "localhost", so the host name would fail too.
        let loopback = ServerConfig {
            host: "127.0.0.1".into(),
            port,
            security: Security::Tls,
        };
        SmtpSender::new(&loopback, "me", &secret, ConnectOptions::default())
            .unwrap()
            .verify()
            .unwrap();
    }

    #[test]
    fn auth_reply_detection() {
        assert!(is_auth_reply("454 4.7.0 Temporary authentication failure"));
        assert!(!is_auth_reply("454 TLS not available"));
    }
}
