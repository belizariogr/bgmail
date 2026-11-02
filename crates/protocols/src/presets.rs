//! Built-in server settings for popular providers, so most users only type
//! their address. Consulted before [`crate::autoconfig`].

use mail_core::{AccountConfig, AuthKind, Security, ServerConfig, DEFAULT_SYNC_WINDOW};

/// How the provider expects the login name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LoginName {
    /// The full address (`me@example.com`).
    #[default]
    EmailAddress,
    /// Only the part before `@`.
    LocalPart,
}

/// Something the "Add account" UI must tell the user. A stable key, not a
/// sentence: the UI owns the localized text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresetNote {
    /// The provider refuses plain passwords; needs an app-specific password.
    AppPasswordRequired,
    /// Password login is disabled; only OAuth works (not supported yet for
    /// this provider).
    OAuthRequired,
    /// Proton Bridge must be running locally (self-signed certificate).
    ProtonBridge,
    /// IMAP access must be switched on in the webmail settings first.
    EnableImapFirst,
}

/// Known settings for a provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderPreset {
    pub name: &'static str,
    pub imap: ServerConfig,
    pub smtp: ServerConfig,
    pub login: LoginName,
    /// OAuth provider id (`"google"`) when sign-in goes through OAuth.
    pub oauth: Option<&'static str>,
    pub note: Option<PresetNote>,
}

impl ProviderPreset {
    /// The account config for `email` with these servers.
    pub fn account_config(&self, email: &str) -> AccountConfig {
        let email = email.trim();
        let username = match self.login {
            LoginName::EmailAddress => email.to_string(),
            LoginName::LocalPart => local_part(email).to_string(),
        };
        AccountConfig {
            email: email.to_string(),
            username,
            imap: self.imap.clone(),
            smtp: self.smtp.clone(),
            auth: match self.oauth {
                Some(provider) => AuthKind::OAuth2 {
                    provider: provider.to_string(),
                },
                None => AuthKind::Password,
            },
            sync_window: DEFAULT_SYNC_WINDOW,
        }
    }
}

/// Static description of one preset (expanded into [`ProviderPreset`]).
struct Entry {
    domains: &'static [&'static str],
    name: &'static str,
    imap: (&'static str, u16, Security),
    smtp: (&'static str, u16, Security),
    login: LoginName,
    oauth: Option<&'static str>,
    note: Option<PresetNote>,
}

const TLS: Security = Security::Tls;
const STARTTLS: Security = Security::StartTls;

const ENTRIES: &[Entry] = &[
    Entry {
        domains: &["gmail.com", "googlemail.com"],
        name: "Gmail",
        imap: ("imap.gmail.com", 993, TLS),
        smtp: ("smtp.gmail.com", 465, TLS),
        login: LoginName::EmailAddress,
        oauth: Some("google"),
        note: None,
    },
    Entry {
        domains: &["outlook.com", "hotmail.com", "live.com", "msn.com"],
        name: "Outlook.com",
        imap: ("outlook.office365.com", 993, TLS),
        smtp: ("smtp-mail.outlook.com", 587, STARTTLS),
        login: LoginName::EmailAddress,
        oauth: None,
        note: Some(PresetNote::OAuthRequired),
    },
    Entry {
        domains: &["yahoo.com", "yahoo.com.br", "ymail.com", "rocketmail.com"],
        name: "Yahoo Mail",
        imap: ("imap.mail.yahoo.com", 993, TLS),
        smtp: ("smtp.mail.yahoo.com", 465, TLS),
        login: LoginName::EmailAddress,
        oauth: None,
        note: Some(PresetNote::AppPasswordRequired),
    },
    Entry {
        domains: &["icloud.com", "me.com", "mac.com"],
        name: "iCloud Mail",
        imap: ("imap.mail.me.com", 993, TLS),
        smtp: ("smtp.mail.me.com", 587, STARTTLS),
        login: LoginName::EmailAddress,
        oauth: None,
        note: Some(PresetNote::AppPasswordRequired),
    },
    Entry {
        domains: &["fastmail.com", "fastmail.fm"],
        name: "Fastmail",
        imap: ("imap.fastmail.com", 993, TLS),
        smtp: ("smtp.fastmail.com", 465, TLS),
        login: LoginName::EmailAddress,
        oauth: None,
        note: Some(PresetNote::AppPasswordRequired),
    },
    Entry {
        domains: &["zoho.com", "zohomail.com"],
        name: "Zoho Mail",
        imap: ("imap.zoho.com", 993, TLS),
        smtp: ("smtp.zoho.com", 465, TLS),
        login: LoginName::EmailAddress,
        oauth: None,
        note: Some(PresetNote::EnableImapFirst),
    },
    Entry {
        domains: &["aol.com"],
        name: "AOL Mail",
        imap: ("imap.aol.com", 993, TLS),
        smtp: ("smtp.aol.com", 465, TLS),
        login: LoginName::EmailAddress,
        oauth: None,
        note: Some(PresetNote::AppPasswordRequired),
    },
    Entry {
        domains: &["gmx.com"],
        name: "GMX",
        imap: ("imap.gmx.com", 993, TLS),
        smtp: ("mail.gmx.com", 587, STARTTLS),
        login: LoginName::EmailAddress,
        oauth: None,
        note: Some(PresetNote::EnableImapFirst),
    },
    Entry {
        domains: &["gmx.net", "gmx.de", "gmx.at", "gmx.ch"],
        name: "GMX",
        imap: ("imap.gmx.net", 993, TLS),
        smtp: ("mail.gmx.net", 587, STARTTLS),
        login: LoginName::EmailAddress,
        oauth: None,
        note: Some(PresetNote::EnableImapFirst),
    },
    Entry {
        domains: &["uol.com.br", "bol.com.br"],
        name: "UOL",
        imap: ("imap.uol.com.br", 993, TLS),
        smtp: ("smtps.uol.com.br", 587, STARTTLS),
        login: LoginName::EmailAddress,
        oauth: None,
        note: Some(PresetNote::EnableImapFirst),
    },
    Entry {
        domains: &["terra.com.br"],
        name: "Terra",
        imap: ("imap.terra.com.br", 993, TLS),
        smtp: ("smtp.terra.com.br", 587, STARTTLS),
        login: LoginName::EmailAddress,
        oauth: None,
        note: None,
    },
    Entry {
        domains: &["yandex.com", "yandex.ru", "ya.ru"],
        name: "Yandex Mail",
        imap: ("imap.yandex.com", 993, TLS),
        smtp: ("smtp.yandex.com", 465, TLS),
        login: LoginName::EmailAddress,
        oauth: None,
        note: Some(PresetNote::AppPasswordRequired),
    },
    Entry {
        domains: &["mail.com"],
        name: "mail.com",
        imap: ("imap.mail.com", 993, TLS),
        smtp: ("smtp.mail.com", 587, STARTTLS),
        login: LoginName::EmailAddress,
        oauth: None,
        note: Some(PresetNote::EnableImapFirst),
    },
    Entry {
        domains: &["proton.me", "protonmail.com", "protonmail.ch", "pm.me"],
        name: "Proton Mail (Bridge)",
        imap: ("127.0.0.1", 1143, STARTTLS),
        smtp: ("127.0.0.1", 1025, STARTTLS),
        login: LoginName::EmailAddress,
        oauth: None,
        note: Some(PresetNote::ProtonBridge),
    },
];

/// The preset for `email`'s domain (case-insensitive), if we know it.
pub fn provider_for_email(email: &str) -> Option<ProviderPreset> {
    let domain = domain(email)?;
    let entry = ENTRIES
        .iter()
        .find(|entry| entry.domains.contains(&domain.as_str()))?;
    let server = |(host, port, security): (&str, u16, Security)| ServerConfig {
        host: host.to_string(),
        port,
        security,
    };
    Some(ProviderPreset {
        name: entry.name,
        imap: server(entry.imap),
        smtp: server(entry.smtp),
        login: entry.login,
        oauth: entry.oauth,
        note: entry.note,
    })
}

/// Lowercased domain of an address (`None` without a non-empty domain).
pub fn domain(email: &str) -> Option<String> {
    let (_, domain) = email.trim().rsplit_once('@')?;
    let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    (!domain.is_empty()).then_some(domain)
}

/// The part before the last `@` (the whole input when there is none).
pub fn local_part(email: &str) -> &str {
    email
        .trim()
        .rsplit_once('@')
        .map_or(email.trim(), |(local, _)| local)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gmail_uses_oauth() {
        let preset = provider_for_email("Someone@GMail.com").unwrap();
        assert_eq!(preset.name, "Gmail");
        assert_eq!(preset.oauth, Some("google"));
        assert_eq!(preset.imap.host, "imap.gmail.com");
        let config = preset.account_config(" someone@gmail.com ");
        assert_eq!(config.username, "someone@gmail.com");
        assert!(config.uses_oauth());
        assert_eq!(config.sync_window, DEFAULT_SYNC_WINDOW);
        assert!(provider_for_email("x@googlemail.com").is_some());
    }

    #[test]
    fn outlook_family_notes_oauth_requirement() {
        for email in ["a@outlook.com", "a@hotmail.com", "a@live.com", "a@msn.com"] {
            let preset = provider_for_email(email).unwrap();
            assert_eq!(preset.imap.host, "outlook.office365.com");
            assert_eq!(preset.smtp.host, "smtp-mail.outlook.com");
            assert_eq!(preset.smtp.port, 587);
            assert_eq!(preset.smtp.security, Security::StartTls);
            assert_eq!(preset.note, Some(PresetNote::OAuthRequired));
        }
    }

    #[test]
    fn brazilian_and_other_providers() {
        let uol = provider_for_email("a@bol.com.br").unwrap();
        assert_eq!(uol.imap.host, "imap.uol.com.br");
        assert_eq!(
            (uol.smtp.host.as_str(), uol.smtp.port),
            ("smtps.uol.com.br", 587)
        );
        assert!(provider_for_email("a@terra.com.br").is_some());
        assert!(provider_for_email("a@yahoo.com.br").is_some());
        let icloud = provider_for_email("a@me.com").unwrap();
        assert_eq!(icloud.smtp.host, "smtp.mail.me.com");
        for email in [
            "a@fastmail.com",
            "a@zoho.com",
            "a@aol.com",
            "a@gmx.com",
            "a@gmx.net",
            "a@yandex.com",
            "a@mail.com",
        ] {
            let preset = provider_for_email(email).unwrap();
            assert!(preset.imap.port == 993, "{email}");
            assert_eq!(preset.account_config(email).auth, AuthKind::Password);
        }
    }

    #[test]
    fn proton_points_at_bridge() {
        let proton = provider_for_email("a@proton.me").unwrap();
        assert_eq!(
            (proton.imap.host.as_str(), proton.imap.port),
            ("127.0.0.1", 1143)
        );
        assert_eq!(proton.smtp.port, 1025);
        assert_eq!(proton.note, Some(PresetNote::ProtonBridge));
    }

    #[test]
    fn unknown_or_invalid_addresses() {
        assert!(provider_for_email("me@example.org").is_none());
        assert!(provider_for_email("not-an-email").is_none());
        assert!(provider_for_email("me@").is_none());
    }

    #[test]
    fn login_name_styles() {
        let mut preset = provider_for_email("a@gmx.net").unwrap();
        preset.login = LoginName::LocalPart;
        assert_eq!(preset.account_config("joe@gmx.net").username, "joe");
        assert_eq!(LoginName::default(), LoginName::EmailAddress);
    }

    #[test]
    fn domain_and_local_part_helpers() {
        assert_eq!(domain(" A@B.Com. ").as_deref(), Some("b.com"));
        assert_eq!(domain("x"), None);
        assert_eq!(local_part("joe@x.com"), "joe");
        assert_eq!(local_part("a@b@c.com"), "a@b");
        assert_eq!(local_part("plain"), "plain");
    }

    #[test]
    fn every_domain_is_unique() {
        let mut all: Vec<&str> = ENTRIES
            .iter()
            .flat_map(|e| e.domains.iter().copied())
            .collect();
        let len = all.len();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), len);
    }
}
