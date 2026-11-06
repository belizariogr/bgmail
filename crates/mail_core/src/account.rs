//! Server settings of a mail account, persisted as JSON in
//! `storage::Account::config_json` and read by the protocol clients.
//!
//! Secrets (passwords, OAuth refresh tokens) are never part of this struct:
//! they live in the platform keychain, keyed by [`AccountConfig::secret_key`].

use serde::{Deserialize, Serialize};

/// Transport security of one server connection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Security {
    /// TLS from the first byte (IMAPS 993, SMTPS 465).
    #[default]
    Tls,
    /// Plain connection upgraded with STARTTLS (IMAP 143, submission 587).
    StartTls,
    /// No encryption. Only offered for local test servers.
    None,
}

impl Security {
    /// Conventional IMAP port for this security mode.
    pub fn default_imap_port(self) -> u16 {
        match self {
            Self::Tls => 993,
            Self::StartTls | Self::None => 143,
        }
    }

    /// Conventional SMTP submission port for this security mode.
    pub fn default_smtp_port(self) -> u16 {
        match self {
            Self::Tls => 465,
            Self::StartTls => 587,
            Self::None => 25,
        }
    }
}

/// Host, port and security of one server.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub security: Security,
}

/// How the account authenticates.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum AuthKind {
    /// LOGIN / AUTH PLAIN with a password from the keychain.
    #[default]
    Password,
    /// SASL XOAUTH2 with an access token refreshed from the stored refresh token.
    OAuth2 { provider: String },
}

/// Everything needed to reach an account's servers (except the secret).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountConfig {
    pub email: String,
    /// Login name (usually the e-mail address).
    pub username: String,
    pub imap: ServerConfig,
    pub smtp: ServerConfig,
    #[serde(default)]
    pub auth: AuthKind,
    /// Newest messages kept per folder; older ones are not downloaded.
    #[serde(default = "default_sync_window")]
    pub sync_window: u32,
}

/// Default for [`AccountConfig::sync_window`].
pub const DEFAULT_SYNC_WINDOW: u32 = 2000;

fn default_sync_window() -> u32 {
    DEFAULT_SYNC_WINDOW
}

impl AccountConfig {
    /// Parses the JSON stored in the database (`None` for local/invalid configs).
    pub fn from_json(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }

    /// Serializes for `storage::Database::create_account`.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Keychain key of this account's secret. Stable across renames because
    /// it only depends on the login and the IMAP host.
    pub fn secret_key(&self) -> String {
        format!("bgmail://{}@{}", self.username, self.imap.host)
    }

    /// Whether the account signs in with OAuth (no password prompt).
    pub fn uses_oauth(&self) -> bool {
        matches!(self.auth, AuthKind::OAuth2 { .. })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> AccountConfig {
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
                port: 587,
                security: Security::StartTls,
            },
            auth: AuthKind::Password,
            sync_window: 100,
        }
    }

    #[test]
    fn json_round_trips() {
        let config = sample();
        assert_eq!(AccountConfig::from_json(&config.to_json()), Some(config));
    }

    #[test]
    fn invalid_or_local_json_is_none() {
        assert_eq!(AccountConfig::from_json("{}"), None);
        assert_eq!(AccountConfig::from_json("not json"), None);
    }

    #[test]
    fn missing_auth_and_window_use_defaults() {
        let mut json: serde_json::Value = serde_json::from_str(&sample().to_json()).unwrap();
        json.as_object_mut().unwrap().remove("auth");
        json.as_object_mut().unwrap().remove("sync_window");
        let config = AccountConfig::from_json(&json.to_string()).unwrap();
        assert_eq!(config.auth, AuthKind::Password);
        assert_eq!(config.sync_window, DEFAULT_SYNC_WINDOW);
    }

    #[test]
    fn oauth_serializes_with_a_kind_tag() {
        let mut config = sample();
        config.auth = AuthKind::OAuth2 {
            provider: "google".into(),
        };
        assert!(config.to_json().contains(r#""kind":"oauth2""#));
        assert!(config.uses_oauth());
        assert!(!sample().uses_oauth());
    }

    #[test]
    fn secret_key_depends_on_login_and_host() {
        assert_eq!(
            sample().secret_key(),
            "bgmail://me@example.com@imap.example.com"
        );
    }

    #[test]
    fn default_ports_follow_security() {
        assert_eq!(Security::Tls.default_imap_port(), 993);
        assert_eq!(Security::StartTls.default_imap_port(), 143);
        assert_eq!(Security::None.default_imap_port(), 143);
        assert_eq!(Security::Tls.default_smtp_port(), 465);
        assert_eq!(Security::StartTls.default_smtp_port(), 587);
        assert_eq!(Security::None.default_smtp_port(), 25);
    }
}
