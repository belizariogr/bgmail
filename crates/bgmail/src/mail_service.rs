//! Bridge between the UI and the sync engine (`mail_sync`).
//!
//! Owns the [`Engine`], turns stored account configs + keychain secrets into
//! protocol connectors, and keeps the latest [`AccountStatus`] of every
//! account for the status bar. The event loop itself lives in `RootView`
//! (it needs a GPUI context); this module stays plain Rust so it is testable.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use mail_core::{AccountConfig, AuthKind, MailConnector};
use mail_sync::{AccountStatus, Engine, EngineOptions, SyncEvent};
use protocols::{ConnectOptions, Credential, ImapSmtpConnector};

/// Why an account could not be started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartError {
    /// `config_json` is not a remote account config (local/demo accounts).
    NotRemote,
    /// The keychain has no secret for the account (or it isn't UTF-8).
    MissingSecret,
    /// The account uses an OAuth provider this build has no client for.
    OAuthUnavailable(String),
}

/// Builds the credential for `config` from the secret stored in the keychain:
/// the password, or the OAuth refresh token.
pub fn credential_for(config: &AccountConfig, secret: &[u8]) -> Result<Credential, StartError> {
    let secret = String::from_utf8(secret.to_vec()).map_err(|_| StartError::MissingSecret)?;
    if secret.is_empty() {
        return Err(StartError::MissingSecret);
    }
    match &config.auth {
        AuthKind::Password => Ok(Credential::Password(secret)),
        AuthKind::OAuth2 { provider } => {
            let provider_config = protocols::oauth::oauth_provider(provider)
                .ok_or_else(|| StartError::OAuthUnavailable(provider.clone()))?;
            Ok(Credential::OAuth {
                provider: provider_config,
                refresh_token: secret,
            })
        }
    }
}

/// Builds the connector the engine uses for an account.
pub fn connector_for(
    config_json: &str,
    secret: &[u8],
) -> Result<Arc<dyn MailConnector>, StartError> {
    let config = AccountConfig::from_json(config_json).ok_or(StartError::NotRemote)?;
    let credential = credential_for(&config, secret)?;
    Ok(Arc::new(ImapSmtpConnector::new(
        config,
        credential,
        ConnectOptions::default(),
    )))
}

/// Summary of every account's sync state, for the status bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverallStatus {
    /// No remote account is running.
    None,
    /// At least one account is connecting or syncing.
    Syncing,
    /// Every account is up to date; newest `last_sync` (unix seconds).
    UpToDate(i64),
    /// Some account can't reach its server (message of the first one).
    Offline(String),
    /// Some account's credentials were rejected (its id).
    AuthFailed(i64),
    /// Some account hit an unexpected error.
    Error(String),
}

/// Folds per-account statuses into one, most urgent first: auth failure,
/// error, offline, syncing, then up to date.
pub fn overall_status<'a>(
    statuses: impl IntoIterator<Item = (&'a i64, &'a AccountStatus)>,
) -> OverallStatus {
    let mut auth_failed = None;
    let mut error = None;
    let mut offline = None;
    let mut syncing = false;
    let mut last_sync: Option<i64> = None;
    let mut any = false;
    for (&account_id, status) in statuses {
        any = true;
        match status {
            AccountStatus::AuthFailed { .. } => {
                auth_failed.get_or_insert(account_id);
            }
            AccountStatus::Error { error: message } => {
                error.get_or_insert_with(|| message.clone());
            }
            AccountStatus::Offline { error: message, .. } => {
                offline.get_or_insert_with(|| message.clone());
            }
            AccountStatus::Connecting | AccountStatus::Syncing { .. } => syncing = true,
            AccountStatus::Idle { last_sync: at } => {
                last_sync = Some(last_sync.map_or(*at, |seen| seen.max(*at)));
            }
        }
    }
    if let Some(account_id) = auth_failed {
        OverallStatus::AuthFailed(account_id)
    } else if let Some(message) = error {
        OverallStatus::Error(message)
    } else if let Some(message) = offline {
        OverallStatus::Offline(message)
    } else if syncing {
        OverallStatus::Syncing
    } else if let Some(at) = last_sync {
        OverallStatus::UpToDate(at)
    } else if any {
        OverallStatus::Syncing
    } else {
        OverallStatus::None
    }
}

/// The running engine plus the last status reported by each account.
pub struct MailService {
    engine: Engine,
    statuses: HashMap<i64, AccountStatus>,
}

impl MailService {
    /// Creates the engine on `db_path`; events arrive on the returned receiver.
    pub fn new(db_path: PathBuf) -> (Self, async_channel::Receiver<SyncEvent>) {
        let (sender, receiver) = async_channel::unbounded();
        let engine = Engine::new(db_path, sender, EngineOptions::default());
        (
            Self {
                engine,
                statuses: HashMap::new(),
            },
            receiver,
        )
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Starts (or restarts) syncing `account_id` with `connector`.
    pub fn start_account(&mut self, account_id: i64, connector: Arc<dyn MailConnector>) {
        self.statuses.insert(account_id, AccountStatus::Connecting);
        self.engine.start_account(account_id, connector);
    }

    /// Stops an account's threads and forgets its status.
    pub fn stop_account(&mut self, account_id: i64) {
        self.engine.stop_account(account_id);
        self.statuses.remove(&account_id);
    }

    /// Records a status event (ignored for accounts that were stopped).
    pub fn set_status(&mut self, account_id: i64, status: AccountStatus) {
        if self.engine.is_running(account_id) {
            self.statuses.insert(account_id, status);
        }
    }

    pub fn status(&self, account_id: i64) -> Option<&AccountStatus> {
        self.statuses.get(&account_id)
    }

    pub fn overall(&self) -> OverallStatus {
        overall_status(&self.statuses)
    }

    /// Asks the account's worker to replay queued server ops and the outbox.
    pub fn process_queue(&self, account_id: i64) {
        self.engine
            .send(account_id, mail_sync::Command::ProcessQueue);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_core::{Security, ServerConfig};

    fn config(auth: AuthKind) -> AccountConfig {
        AccountConfig {
            email: "me@example.com".into(),
            username: "me@example.com".into(),
            imap: ServerConfig {
                host: "imap.example.com".into(),
                port: 993,
                security: Security::Tls,
            },
            smtp: ServerConfig {
                host: "smtp.example.com".into(),
                port: 465,
                security: Security::Tls,
            },
            auth,
            sync_window: 100,
        }
    }

    #[test]
    fn password_secret_becomes_password_credential() {
        let credential = credential_for(&config(AuthKind::Password), b"hunter2").unwrap();
        assert!(matches!(credential, Credential::Password(p) if p == "hunter2"));
    }

    #[test]
    fn empty_or_invalid_secret_is_missing() {
        assert_eq!(
            credential_for(&config(AuthKind::Password), b"").unwrap_err(),
            StartError::MissingSecret
        );
        assert_eq!(
            credential_for(&config(AuthKind::Password), &[0xff, 0xfe]).unwrap_err(),
            StartError::MissingSecret
        );
    }

    #[test]
    fn unknown_oauth_provider_is_unavailable() {
        let auth = AuthKind::OAuth2 {
            provider: "nowhere".into(),
        };
        assert_eq!(
            credential_for(&config(auth), b"refresh").unwrap_err(),
            StartError::OAuthUnavailable("nowhere".into())
        );
    }

    #[test]
    fn local_accounts_have_no_connector() {
        assert_eq!(connector_for("{}", b"x").err(), Some(StartError::NotRemote));
        assert!(connector_for(&config(AuthKind::Password).to_json(), b"pw").is_ok());
    }

    #[test]
    fn overall_status_prefers_the_most_urgent() {
        let mut statuses = HashMap::new();
        assert_eq!(overall_status(&statuses), OverallStatus::None);
        statuses.insert(1, AccountStatus::Idle { last_sync: 10 });
        statuses.insert(2, AccountStatus::Idle { last_sync: 20 });
        assert_eq!(overall_status(&statuses), OverallStatus::UpToDate(20));
        statuses.insert(3, AccountStatus::Connecting);
        assert_eq!(overall_status(&statuses), OverallStatus::Syncing);
        statuses.insert(
            4,
            AccountStatus::Offline {
                error: "down".into(),
                retry_in_secs: 5,
            },
        );
        assert_eq!(
            overall_status(&statuses),
            OverallStatus::Offline("down".into())
        );
        statuses.insert(5, AccountStatus::Error { error: "db".into() });
        assert_eq!(overall_status(&statuses), OverallStatus::Error("db".into()));
        statuses.insert(6, AccountStatus::AuthFailed { error: "no".into() });
        assert_eq!(overall_status(&statuses), OverallStatus::AuthFailed(6));
    }

    #[test]
    fn service_tracks_status_of_running_accounts_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mail.db");
        storage::Database::open(&path).unwrap();
        let (mut service, _events) = MailService::new(path);
        service.set_status(7, AccountStatus::Connecting);
        assert_eq!(service.status(7), None);
        assert_eq!(service.overall(), OverallStatus::None);
        service.stop_account(7);
        service.engine().shutdown();
    }
}
