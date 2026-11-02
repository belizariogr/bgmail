//! Real mail-server clients behind `mail_core`'s blocking contracts: IMAP
//! ([`ImapRemote`]), SMTP submission ([`SmtpSender`]), OAuth 2.0 sign-in
//! ([`oauth`]), account wiring ([`ImapSmtpConnector`]) and server discovery
//! ([`presets`], [`autoconfig`]).
//!
//! Everything is blocking and runs on the caller's (background) thread; there
//! is no async runtime. One rustls 0.23 (ring provider, OS trust store via
//! `rustls-platform-verifier`) serves IMAP, SMTP (lettre) and HTTP (ureq).
//! No GPUI, no UI strings beyond the OAuth loopback page.

#![forbid(unsafe_code)]

pub mod autoconfig;
pub mod connector;
mod http;
pub mod imap_remote;
pub mod oauth;
pub mod presets;
pub mod smtp;
pub mod tls;

pub use autoconfig::{
    autoconfig, autoconfig_urls, discover_config, guess_config, parse_autoconfig,
};
pub use connector::{test_account, Credential, ImapSmtpConnector, Secret};
pub use imap_remote::ImapRemote;
pub use oauth::{
    authorize, google_client_from_env, google_provider, oauth_provider, refresh, xoauth2_payload,
    AccessToken, OAuthProvider, OAuthTokens,
};
pub use presets::{provider_for_email, LoginName, PresetNote, ProviderPreset};
pub use smtp::SmtpSender;
pub use tls::ConnectOptions;
