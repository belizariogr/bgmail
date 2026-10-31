//! Server discovery for addresses without a preset: Mozilla's ISPDB, the
//! domain's own autoconfig file, then a conventional guess.
//!
//! Format: Thunderbird "clientConfig" v1.1
//! (<https://wiki.mozilla.org/Thunderbird:Autoconfiguration:ConfigFileFormat>).

use std::time::Duration;

use mail_core::{AccountConfig, AuthKind, Security, ServerConfig, DEFAULT_SYNC_WINDOW};

use crate::http;
use crate::presets::{domain, local_part, provider_for_email};

/// Where to look, in order: ISPDB, `autoconfig.<domain>`, then
/// `<domain>/.well-known`. Empty for addresses without a domain.
pub fn autoconfig_urls(email: &str) -> Vec<String> {
    let Some(domain) = domain(email) else {
        return Vec::new();
    };
    let address = http::encode_component(email.trim());
    vec![
        format!("https://autoconfig.thunderbird.net/v1.1/{domain}"),
        format!("https://autoconfig.{domain}/mail/config-v1.1.xml?emailaddress={address}"),
        format!("https://{domain}/.well-known/autoconfig/mail/config-v1.1.xml"),
    ]
}

/// Fetches and parses the first autoconfig document that yields IMAP + SMTP
/// settings (each request bounded by `timeout`); `None` if none does.
pub fn autoconfig(email: &str, timeout: Duration) -> Option<AccountConfig> {
    autoconfig_from(&autoconfig_urls(email), email, timeout)
}

/// [`autoconfig`] over explicit URLs (lets tests use a local server).
fn autoconfig_from(urls: &[String], email: &str, timeout: Duration) -> Option<AccountConfig> {
    urls.iter().find_map(|url| {
        let response = http::get(url, timeout).ok()?;
        if !(200..300).contains(&response.status) {
            return None;
        }
        parse_autoconfig(&response.body, email)
    })
}

/// Best effort for unknown domains: `imap.<domain>:993` (TLS) and
/// `smtp.<domain>:587` (STARTTLS), logging in with the full address.
pub fn guess_config(email: &str) -> Option<AccountConfig> {
    let domain = domain(email)?;
    Some(AccountConfig {
        email: email.trim().to_string(),
        username: email.trim().to_string(),
        imap: ServerConfig {
            host: format!("imap.{domain}"),
            port: 993,
            security: Security::Tls,
        },
        smtp: ServerConfig {
            host: format!("smtp.{domain}"),
            port: 587,
            security: Security::StartTls,
        },
        auth: AuthKind::Password,
        sync_window: DEFAULT_SYNC_WINDOW,
    })
}

/// The whole chain: preset → autoconfig → guess. `None` only for addresses
/// without a domain. Blocking (network); call off the UI thread.
pub fn discover_config(email: &str, timeout: Duration) -> Option<AccountConfig> {
    if let Some(preset) = provider_for_email(email) {
        return Some(preset.account_config(email));
    }
    autoconfig(email, timeout).or_else(|| guess_config(email))
}

/// One `incomingServer` / `outgoingServer` element.
#[derive(Debug)]
struct Server {
    config: ServerConfig,
    username: String,
}

/// Parses a clientConfig document: the first usable `incomingServer
/// type="imap"` and `outgoingServer type="smtp"` (the file lists preferred
/// ones first), with `%EMAILADDRESS%` / `%EMAILLOCALPART%` / `%EMAILDOMAIN%`
/// substituted in the username. `None` unless both are present.
pub fn parse_autoconfig(xml: &str, email: &str) -> Option<AccountConfig> {
    let document = roxmltree::Document::parse(xml).ok()?;
    let provider = document
        .descendants()
        .find(|node| node.has_tag_name("emailProvider"))?;
    let pick = |tag: &str, kind: &str| {
        provider
            .children()
            .filter(|node| node.has_tag_name(tag) && node.attribute("type") == Some(kind))
            .find_map(|node| parse_server(node, email))
    };
    let imap = pick("incomingServer", "imap")?;
    let smtp = pick("outgoingServer", "smtp")?;
    Some(AccountConfig {
        email: email.trim().to_string(),
        username: imap.username,
        imap: imap.config,
        smtp: smtp.config,
        auth: AuthKind::Password,
        sync_window: DEFAULT_SYNC_WINDOW,
    })
}

fn parse_server(node: roxmltree::Node<'_, '_>, email: &str) -> Option<Server> {
    let text = |tag: &str| {
        node.children()
            .find(|child| child.has_tag_name(tag))
            .and_then(|child| child.text())
            .map(|text| text.trim().to_string())
    };
    let security = match text("socketType")?.to_ascii_uppercase().as_str() {
        "SSL" | "TLS" => Security::Tls,
        "STARTTLS" => Security::StartTls,
        "PLAIN" => Security::None,
        _ => return None,
    };
    let host = substitute(&text("hostname")?, email);
    if host.is_empty() {
        return None;
    }
    let port = text("port").and_then(|p| p.parse().ok())?;
    let username = text("username")
        .map(|u| substitute(&u, email))
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| email.trim().to_string());
    Some(Server {
        config: ServerConfig {
            host,
            port,
            security,
        },
        username,
    })
}

/// Expands the clientConfig placeholders.
fn substitute(template: &str, email: &str) -> String {
    let email = email.trim();
    template
        .replace("%EMAILADDRESS%", email)
        .replace("%EMAILLOCALPART%", local_part(email))
        .replace("%EMAILDOMAIN%", &domain(email).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::test_server;

    const GMX: &str = include_str!("../tests/fixtures/autoconfig-gmx.net.xml");
    const UOL: &str = include_str!("../tests/fixtures/autoconfig-uol.com.br.xml");
    const YAHOO: &str = include_str!("../tests/fixtures/autoconfig-yahoo.com.xml");

    #[test]
    fn parses_ispdb_gmx() {
        let config = parse_autoconfig(GMX, "joe@gmx.net").unwrap();
        assert_eq!(config.imap.host, "imap.gmx.net");
        assert_eq!(
            (config.imap.port, config.imap.security),
            (993, Security::Tls)
        );
        assert_eq!(config.smtp.host, "mail.gmx.net");
        assert_eq!(
            (config.smtp.port, config.smtp.security),
            (465, Security::Tls)
        );
        assert_eq!(config.username, "joe@gmx.net");
        assert_eq!(config.auth, AuthKind::Password);
    }

    #[test]
    fn parses_ispdb_uol_with_local_part_login() {
        let config = parse_autoconfig(UOL, "maria@uol.com.br").unwrap();
        assert_eq!(config.imap.host, "imap.uol.com.br");
        assert_eq!(config.username, "maria");
        assert_eq!(config.smtp.host, "smtps.uol.com.br");
        assert_eq!(config.smtp.security, Security::StartTls);
        assert_eq!(config.email, "maria@uol.com.br");
    }

    #[test]
    fn parses_ispdb_yahoo_with_oauth_and_password() {
        let config = parse_autoconfig(YAHOO, "a@yahoo.com").unwrap();
        assert_eq!(config.imap.host, "imap.mail.yahoo.com");
        assert_eq!(config.smtp.port, 465);
    }

    #[test]
    fn rejects_incomplete_or_invalid_documents() {
        assert!(parse_autoconfig("not xml", "a@b.c").is_none());
        assert!(parse_autoconfig("<clientConfig/>", "a@b.c").is_none());
        let pop_only = r#"<clientConfig><emailProvider id="x">
            <incomingServer type="pop3"><hostname>pop.x</hostname><port>995</port><socketType>SSL</socketType></incomingServer>
            <outgoingServer type="smtp"><hostname>smtp.x</hostname><port>587</port><socketType>STARTTLS</socketType></outgoingServer>
            </emailProvider></clientConfig>"#;
        assert!(parse_autoconfig(pop_only, "a@x").is_none());
        let odd = r#"<clientConfig><emailProvider id="x">
            <incomingServer type="imap"><hostname>imap.x</hostname><port>abc</port><socketType>SSL</socketType></incomingServer>
            <incomingServer type="imap"><hostname>imap2.%EMAILDOMAIN%</hostname><port>143</port><socketType>plain</socketType></incomingServer>
            <outgoingServer type="smtp"><hostname>smtp.x</hostname><port>25</port><socketType>weird</socketType></outgoingServer>
            <outgoingServer type="smtp"><hostname>smtp.x</hostname><port>587</port><socketType>STARTTLS</socketType></outgoingServer>
            </emailProvider></clientConfig>"#;
        let config = parse_autoconfig(odd, "a@x.org").unwrap();
        assert_eq!(config.imap.host, "imap2.x.org");
        assert_eq!(config.imap.security, Security::None);
        assert_eq!(config.username, "a@x.org");
        assert_eq!(config.smtp.port, 587);
    }

    #[test]
    fn urls_follow_the_thunderbird_order() {
        let urls = autoconfig_urls("Joe+x@Example.org");
        assert_eq!(
            urls[0],
            "https://autoconfig.thunderbird.net/v1.1/example.org"
        );
        assert_eq!(
            urls[1],
            "https://autoconfig.example.org/mail/config-v1.1.xml?emailaddress=Joe%2Bx%40Example.org"
        );
        assert_eq!(
            urls[2],
            "https://example.org/.well-known/autoconfig/mail/config-v1.1.xml"
        );
        assert!(autoconfig_urls("nodomain").is_empty());
        assert!(autoconfig("nodomain", Duration::from_secs(1)).is_none());
    }

    #[test]
    fn fetch_falls_through_to_the_first_good_document() {
        let (base, log) = test_server::serve(|req| match req.target.as_str() {
            "/missing" => (404, String::new()),
            "/garbage" => (200, "<html/>".into()),
            _ => (200, UOL.to_string()),
        });
        let urls = vec![
            format!("{base}/missing"),
            format!("{base}/garbage"),
            format!("{base}/good"),
        ];
        let config = autoconfig_from(&urls, "maria@uol.com.br", Duration::from_secs(5)).unwrap();
        assert_eq!(config.imap.host, "imap.uol.com.br");
        assert_eq!(log.lock().unwrap().len(), 3);
        assert!(autoconfig_from(&urls[..2], "m@uol.com.br", Duration::from_secs(5)).is_none());
    }

    #[test]
    fn guess_uses_conventional_hosts() {
        let config = guess_config("me@Example.org").unwrap();
        assert_eq!(config.imap.host, "imap.example.org");
        assert_eq!(
            (config.imap.port, config.imap.security),
            (993, Security::Tls)
        );
        assert_eq!(config.smtp.host, "smtp.example.org");
        assert_eq!(
            (config.smtp.port, config.smtp.security),
            (587, Security::StartTls)
        );
        assert!(guess_config("broken").is_none());
    }

    #[test]
    fn discover_prefers_presets_without_network() {
        let config = discover_config("x@gmail.com", Duration::from_millis(1)).unwrap();
        assert_eq!(config.imap.host, "imap.gmail.com");
        assert!(discover_config("broken", Duration::from_millis(1)).is_none());
    }

    #[test]
    fn substitute_placeholders() {
        assert_eq!(
            substitute("%EMAILLOCALPART%@%EMAILDOMAIN% %EMAILADDRESS%", "a@B.org"),
            "a@b.org a@B.org"
        );
    }
}
