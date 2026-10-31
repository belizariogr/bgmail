//! SMTP submission via lettre, used purely as a transport: the message bytes
//! come from `mail_core::build_message` and go out with `send_raw`.
//!
//! lettre owns its sockets, so a [`CancelToken`](mail_core::CancelToken) can
//! only stop a send before it starts; the 60 s I/O timeout bounds the rest.

use lettre::address::{Address, Envelope};
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::transport::smtp::{self as smtp_transport, SmtpTransport};
use lettre::Transport;
use mail_core::{MailSender, RemoteError, RemoteErrorKind, RemoteResult, Security, ServerConfig};

use crate::connector::Secret;
use crate::tls::{ConnectOptions, IO_TIMEOUT};

/// An SMTP submission client for one account.
pub struct SmtpSender {
    transport: SmtpTransport,
}

impl std::fmt::Debug for SmtpSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmtpSender").finish_non_exhaustive()
    }
}

impl SmtpSender {
    /// Builds the transport (no network yet): implicit TLS for
    /// [`Security::Tls`], mandatory STARTTLS for [`Security::StartTls`],
    /// cleartext for [`Security::None`]. Passwords authenticate with
    /// PLAIN/LOGIN, access tokens with XOAUTH2.
    pub fn new(
        server: &ServerConfig,
        username: &str,
        secret: &Secret,
        options: ConnectOptions,
    ) -> RemoteResult<Self> {
        let tls = || {
            TlsParameters::builder(server.host.clone())
                .dangerous_accept_invalid_certs(options.accept_invalid_certs)
                .dangerous_accept_invalid_hostnames(options.accept_invalid_certs)
                .build_rustls()
                .map_err(map_error)
        };
        let tls = match server.security {
            Security::Tls => Tls::Wrapper(tls()?),
            Security::StartTls => Tls::Required(tls()?),
            Security::None => Tls::None,
        };
        let (password, mechanisms) = match secret {
            Secret::Password(password) => {
                (password.clone(), vec![Mechanism::Plain, Mechanism::Login])
            }
            Secret::AccessToken(token) => (token.clone(), vec![Mechanism::Xoauth2]),
        };
        let transport = SmtpTransport::builder_dangerous(server.host.clone())
            .port(server.port)
            .tls(tls)
            .timeout(Some(IO_TIMEOUT))
            .credentials(Credentials::new(username.to_string(), password))
            .authentication(mechanisms)
            .build();
        Ok(Self { transport })
    }

    /// Connects, negotiates TLS and authenticates, then quits. Used by the
    /// "Add account" dialog to validate settings before saving.
    pub fn verify(&self) -> RemoteResult<()> {
        match self.transport.test_connection() {
            Ok(true) => Ok(()),
            Ok(false) => Err(RemoteError::new(
                RemoteErrorKind::Network,
                "SMTP server closed the connection",
            )),
            Err(err) => Err(map_error(err)),
        }
    }
}

impl MailSender for SmtpSender {
    fn send(&mut self, from: &str, recipients: &[String], raw: &[u8]) -> RemoteResult<()> {
        let envelope = envelope(from, recipients)?;
        self.transport
            .send_raw(&envelope, raw)
            .map(|_| ())
            .map_err(map_error)
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
/// replies → Protocol (permanent 5xx or transient 4xx alike), TLS → Tls,
/// connection/timeouts → Network.
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
                writer.write_all(b"250 ok\r\n")?;
            } else if upper.starts_with("RCPT TO:") {
                let to = line[8..].trim().to_string();
                if to.contains("reject") {
                    writer.write_all(b"550 5.1.1 no such user\r\n")?;
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
                log.lock().unwrap().push(std::mem::take(&mut current));
                writer.write_all(b"250 queued\r\n")?;
            } else if upper == "QUIT" {
                writer.write_all(b"221 bye\r\n")?;
                if !current.auth.is_empty() {
                    log.lock().unwrap().push(current);
                }
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
        assert_eq!(err.kind, RemoteErrorKind::Protocol, "{err}");
    }

    #[test]
    fn unreachable_server_is_network_error() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let sender = SmtpSender::new(
            &server(port),
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
        // The fixture certificate is for "localhost"; connect by IP so the
        // host name check would fail too.
        let config = ServerConfig {
            host: "127.0.0.1".into(),
            port,
            security: Security::Tls,
        };
        let secret = Secret::Password("x".into());
        let strict = SmtpSender::new(&config, "me", &secret, ConnectOptions::default()).unwrap();
        assert_eq!(strict.verify().unwrap_err().kind, RemoteErrorKind::Tls);
        let lax = SmtpSender::new(
            &config,
            "me",
            &secret,
            ConnectOptions {
                accept_invalid_certs: true,
            },
        )
        .unwrap();
        lax.verify().unwrap();
    }

    #[test]
    fn auth_reply_detection() {
        assert!(is_auth_reply("454 4.7.0 Temporary authentication failure"));
        assert!(!is_auth_reply("454 TLS not available"));
    }
}
