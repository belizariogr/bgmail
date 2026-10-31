//! Wires an [`AccountConfig`] + secret into `mail_core::MailConnector`:
//! IMAP sessions ([`ImapRemote`]) and SMTP senders ([`SmtpSender`]), with
//! OAuth access tokens refreshed and cached transparently.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use mail_core::{
    AccountConfig, CancelToken, MailConnector, MailRemote, MailSender, RemoteError,
    RemoteErrorKind, RemoteResult,
};

use crate::imap_remote::ImapRemote;
use crate::oauth::{self, AccessToken, OAuthProvider};
use crate::smtp::SmtpSender;
use crate::tls::ConnectOptions;

/// Seconds before expiry at which a cached access token is refreshed.
const REFRESH_MARGIN: i64 = 60;

/// What a single login presents to the server.
#[derive(Clone, PartialEq, Eq)]
pub enum Secret {
    /// LOGIN / AUTH PLAIN / AUTH LOGIN.
    Password(String),
    /// SASL XOAUTH2 bearer token.
    AccessToken(String),
}

impl std::fmt::Debug for Secret {
    /// Never prints the secret itself.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Password(_) => "Secret::Password(..)",
            Self::AccessToken(_) => "Secret::AccessToken(..)",
        })
    }
}

/// The long-lived credential stored in the keychain for an account.
#[derive(Clone, PartialEq, Eq)]
pub enum Credential {
    Password(String),
    OAuth {
        provider: OAuthProvider,
        refresh_token: String,
    },
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Password(_) => f.write_str("Credential::Password(..)"),
            Self::OAuth { provider, .. } => f
                .debug_struct("Credential::OAuth")
                .field("provider", &provider.id)
                .finish_non_exhaustive(),
        }
    }
}

/// `MailConnector` for IMAP + SMTP accounts (password or OAuth).
pub struct ImapSmtpConnector {
    config: AccountConfig,
    credential: Credential,
    options: ConnectOptions,
    /// Cached OAuth access token (and a rotated refresh token, if any).
    token: Mutex<Option<AccessToken>>,
    /// Set once an IMAP session announced `X-GM-EXT-1`.
    gmail_seen: AtomicBool,
}

impl std::fmt::Debug for ImapSmtpConnector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImapSmtpConnector")
            .field("email", &self.config.email)
            .field("credential", &self.credential)
            .finish_non_exhaustive()
    }
}

impl ImapSmtpConnector {
    pub fn new(config: AccountConfig, credential: Credential, options: ConnectOptions) -> Self {
        Self {
            config,
            credential,
            options,
            token: Mutex::new(None),
            gmail_seen: AtomicBool::new(false),
        }
    }

    pub fn config(&self) -> &AccountConfig {
        &self.config
    }

    /// The refresh token currently in use: the provider may rotate it on
    /// refresh, and the caller should then persist the new one.
    pub fn current_refresh_token(&self) -> Option<String> {
        let Credential::OAuth { refresh_token, .. } = &self.credential else {
            return None;
        };
        let cached = self.lock_token();
        Some(
            cached
                .as_ref()
                .and_then(|t| t.refresh_token.clone())
                .unwrap_or_else(|| refresh_token.clone()),
        )
    }

    /// The secret for the next login: the password, or a valid access token
    /// (refreshed when missing or within [`REFRESH_MARGIN`] of expiry).
    pub fn secret(&self) -> RemoteResult<Secret> {
        let (provider, stored_refresh) = match &self.credential {
            Credential::Password(password) => return Ok(Secret::Password(password.clone())),
            Credential::OAuth {
                provider,
                refresh_token,
            } => (provider, refresh_token),
        };
        let mut cached = self.lock_token();
        if let Some(token) = cached.as_ref() {
            if token.expires_at - REFRESH_MARGIN > oauth::now() {
                return Ok(Secret::AccessToken(token.token.clone()));
            }
        }
        let refresh_with = cached
            .as_ref()
            .and_then(|t| t.refresh_token.clone())
            .unwrap_or_else(|| stored_refresh.clone());
        let mut fresh = oauth::refresh(provider, &refresh_with)?;
        if fresh.refresh_token.is_none() && refresh_with != *stored_refresh {
            // Keep remembering a previously rotated token.
            fresh.refresh_token = Some(refresh_with);
        }
        let secret = Secret::AccessToken(fresh.token.clone());
        *cached = Some(fresh);
        Ok(secret)
    }

    /// Forgets the cached access token (after the server rejected it).
    fn invalidate_token(&self) {
        if let Some(token) = self.lock_token().as_mut() {
            token.expires_at = 0;
        }
    }

    fn lock_token(&self) -> std::sync::MutexGuard<'_, Option<AccessToken>> {
        self.token.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Runs `login` with the current secret; with OAuth, an `Auth` failure
    /// is retried once with a freshly refreshed token (revoked/rotated
    /// tokens can be rejected before their nominal expiry).
    fn with_secret<T>(&self, login: impl Fn(&Secret) -> RemoteResult<T>) -> RemoteResult<T> {
        let secret = self.secret()?;
        match login(&secret) {
            Err(err)
                if err.kind == RemoteErrorKind::Auth
                    && matches!(self.credential, Credential::OAuth { .. }) =>
            {
                self.invalidate_token();
                login(&self.secret()?)
            }
            other => other,
        }
    }

    fn connect_imap(&self, cancel: &CancelToken) -> RemoteResult<ImapRemote> {
        let remote = self.with_secret(|secret| {
            ImapRemote::connect(
                &self.config.imap,
                &self.config.username,
                secret,
                self.options,
                cancel,
            )
        })?;
        if remote.capabilities().gmail {
            self.gmail_seen.store(true, Ordering::Relaxed);
        }
        Ok(remote)
    }

    fn sender(&self) -> RemoteResult<SmtpSender> {
        let secret = self.secret()?;
        SmtpSender::new(
            &self.config.smtp,
            &self.config.username,
            &secret,
            self.options,
        )
    }
}

impl MailConnector for ImapSmtpConnector {
    fn connect(&self, cancel: &CancelToken) -> RemoteResult<Box<dyn MailRemote>> {
        Ok(Box::new(self.connect_imap(cancel)?))
    }

    fn connect_sender(&self, cancel: &CancelToken) -> RemoteResult<Box<dyn MailSender>> {
        if cancel.is_cancelled() {
            return Err(RemoteError::new(RemoteErrorKind::Cancelled, "cancelled"));
        }
        Ok(Box::new(self.sender()?))
    }

    /// Gmail files every sent message itself.
    fn saves_sent_copy(&self) -> bool {
        is_gmail_host(&self.config.imap.host) || self.gmail_seen.load(Ordering::Relaxed)
    }
}

/// Whether `host` is Gmail's IMAP/SMTP endpoint.
pub fn is_gmail_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    matches!(
        host.as_str(),
        "imap.gmail.com" | "imap.googlemail.com" | "smtp.gmail.com" | "smtp.googlemail.com"
    )
}

/// Validates settings for the "Add account" dialog: logs in to IMAP (then
/// logs out) and authenticates to SMTP.
pub fn test_account(
    config: AccountConfig,
    credential: Credential,
    options: ConnectOptions,
) -> RemoteResult<()> {
    let connector = ImapSmtpConnector::new(config, credential, options);
    let mut remote = connector.connect_imap(&CancelToken::new())?;
    mail_core::MailRemote::logout(&mut remote);
    connector.sender()?.verify()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::test_server;
    use crate::imap_remote::test_server as imap_server;
    use mail_core::{AuthKind, Security, ServerConfig};
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    fn local(port: u16) -> ServerConfig {
        ServerConfig {
            host: "127.0.0.1".into(),
            port,
            security: Security::None,
        }
    }

    fn config(imap_port: u16, smtp_port: u16) -> AccountConfig {
        AccountConfig {
            email: "me@example.com".into(),
            username: "me".into(),
            imap: local(imap_port),
            smtp: local(smtp_port),
            auth: AuthKind::Password,
            sync_window: 10,
        }
    }

    fn token_server(expires_in: i64) -> (String, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        let (base, _) = test_server::serve(move |_| {
            let n = counter.fetch_add(1, Ordering::SeqCst) + 1;
            (
                200,
                format!("{{\"access_token\":\"at{n}\",\"expires_in\":{expires_in}}}"),
            )
        });
        (base, hits)
    }

    fn oauth_credential(base: &str) -> Credential {
        Credential::OAuth {
            provider: OAuthProvider {
                id: "test".into(),
                auth_url: format!("{base}/auth"),
                token_url: format!("{base}/token"),
                scopes: vec![],
                client_id: "cid".into(),
                client_secret: None,
                extra_params: vec![],
            },
            refresh_token: "rt".into(),
        }
    }

    #[test]
    fn password_secret_and_debug_hide_values() {
        let connector = ImapSmtpConnector::new(
            config(1, 1),
            Credential::Password("hunter2".into()),
            ConnectOptions::default(),
        );
        assert_eq!(
            connector.secret().unwrap(),
            Secret::Password("hunter2".into())
        );
        assert_eq!(connector.current_refresh_token(), None);
        assert_eq!(connector.config().username, "me");
        let debug = format!("{connector:?} {:?}", connector.secret().unwrap());
        assert!(!debug.contains("hunter2"), "{debug}");
        assert!(!format!("{:?}", Secret::AccessToken("tok".into())).contains("tok"));
    }

    #[test]
    fn oauth_token_is_cached_until_near_expiry() {
        let (base, hits) = token_server(3600);
        let connector = ImapSmtpConnector::new(
            config(1, 1),
            oauth_credential(&base),
            ConnectOptions::default(),
        );
        assert_eq!(
            connector.secret().unwrap(),
            Secret::AccessToken("at1".into())
        );
        assert_eq!(
            connector.secret().unwrap(),
            Secret::AccessToken("at1".into())
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        connector.invalidate_token();
        assert_eq!(
            connector.secret().unwrap(),
            Secret::AccessToken("at2".into())
        );
        assert_eq!(connector.current_refresh_token().as_deref(), Some("rt"));

        // Tokens inside the refresh margin are refreshed every time.
        let (base, hits) = token_server(30);
        let connector = ImapSmtpConnector::new(
            config(1, 1),
            oauth_credential(&base),
            ConnectOptions::default(),
        );
        connector.secret().unwrap();
        connector.secret().unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn rotated_refresh_token_is_used_and_exposed() {
        let (base, log) = test_server::serve(|req| {
            (
                200,
                if req.body.contains("refresh_token=rt2") {
                    "{\"access_token\":\"b\",\"expires_in\":1}".into()
                } else {
                    "{\"access_token\":\"a\",\"expires_in\":1,\"refresh_token\":\"rt2\"}".into()
                },
            )
        });
        let connector = ImapSmtpConnector::new(
            config(1, 1),
            oauth_credential(&base),
            ConnectOptions::default(),
        );
        connector.secret().unwrap();
        assert_eq!(connector.current_refresh_token().as_deref(), Some("rt2"));
        assert_eq!(connector.secret().unwrap(), Secret::AccessToken("b".into()));
        assert_eq!(connector.current_refresh_token().as_deref(), Some("rt2"));
        assert!(log.lock().unwrap()[1].body.contains("refresh_token=rt2"));
    }

    #[test]
    fn oauth_auth_failure_retries_once_with_a_fresh_token() {
        let (base, hits) = token_server(3600);
        let (port, handle) = imap_server::start_n(
            "* OK hi\r\n",
            vec![
                ("AUTHENTICATE XOAUTH2", "+ \r\n"),
                ("=*", "{tag} NO bad token\r\n"),
            ],
            2,
        );
        let connector = ImapSmtpConnector::new(
            config(port, 1),
            oauth_credential(&base),
            ConnectOptions::default(),
        );
        let err = connector.connect(&CancelToken::new()).err().unwrap();
        assert_eq!(err.kind, RemoteErrorKind::Auth);
        assert_eq!(hits.load(Ordering::SeqCst), 2);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn connect_marks_gmail_and_test_account_checks_both_servers() {
        let (imap_port, imap_handle) = imap_server::start(
            "* OK hi\r\n",
            vec![
                ("LOGIN \"me\" \"pw\"", "{tag} OK hi\r\n"),
                (
                    "CAPABILITY",
                    "* CAPABILITY IMAP4rev1 X-GM-EXT-1\r\n{tag} OK done\r\n",
                ),
                ("LOGOUT", "{tag} OK bye\r\n"),
            ],
        );
        let (smtp_port, smtp_log) = crate::smtp::test_server::start("pw");
        let connector = ImapSmtpConnector::new(
            config(imap_port, smtp_port),
            Credential::Password("pw".into()),
            ConnectOptions::default(),
        );
        assert!(!connector.saves_sent_copy());
        let mut remote = connector.connect(&CancelToken::new()).unwrap();
        assert!(connector.saves_sent_copy());
        remote.logout();
        imap_handle.join().unwrap().unwrap();
        let mut sender = connector.connect_sender(&CancelToken::new()).unwrap();
        sender
            .send(
                "me@example.com",
                &["you@example.com".into()],
                b"Subject: x\r\n\r\nhi\r\n",
            )
            .unwrap();
        assert!(!smtp_log.lock().unwrap().is_empty());

        let cancelled = CancelToken::new();
        cancelled.cancel();
        assert_eq!(
            connector.connect_sender(&cancelled).err().map(|e| e.kind),
            Some(RemoteErrorKind::Cancelled)
        );
    }

    #[test]
    fn test_account_reports_smtp_auth_failure() {
        let (imap_port, _imap) = imap_server::start(
            "* OK hi\r\n",
            vec![
                ("LOGIN", "{tag} OK hi\r\n"),
                ("CAPABILITY", "* CAPABILITY IMAP4rev1\r\n{tag} OK done\r\n"),
                ("LOGOUT", "{tag} OK bye\r\n"),
            ],
        );
        let (smtp_port, _log) = crate::smtp::test_server::start("other");
        let err = test_account(
            config(imap_port, smtp_port),
            Credential::Password("pw".into()),
            ConnectOptions::default(),
        )
        .unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::Auth);
    }

    #[test]
    fn test_account_succeeds_against_fakes() {
        let (imap_port, _imap) = imap_server::start(
            "* OK hi\r\n",
            vec![
                ("LOGIN", "{tag} OK hi\r\n"),
                ("CAPABILITY", "* CAPABILITY IMAP4rev1\r\n{tag} OK done\r\n"),
                ("LOGOUT", "{tag} OK bye\r\n"),
            ],
        );
        let (smtp_port, _log) = crate::smtp::test_server::start("pw");
        test_account(
            config(imap_port, smtp_port),
            Credential::Password("pw".into()),
            ConnectOptions::default(),
        )
        .unwrap();
    }

    #[test]
    fn gmail_hosts() {
        assert!(is_gmail_host("imap.gmail.com"));
        assert!(is_gmail_host("IMAP.GMAIL.COM."));
        assert!(!is_gmail_host("imap.example.com"));
        let mut cfg = config(1, 1);
        cfg.imap.host = "imap.gmail.com".into();
        let connector = ImapSmtpConnector::new(
            cfg,
            Credential::Password("x".into()),
            ConnectOptions::default(),
        );
        assert!(connector.saves_sent_copy());
        assert!(format!("{:?}", oauth_credential("http://x")).contains("test"));
    }
}
