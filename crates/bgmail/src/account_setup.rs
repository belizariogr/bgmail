//! Pure logic behind Settings ▸ Accounts: the "Add account" form (validation,
//! building an [`AccountConfig`] from typed / preset / discovered settings),
//! the per-account health shown in the list, and the mapping of protocol
//! errors and provider notes to localized keys. No GPUI here so all of it is
//! unit-tested; `settings` owns the widgets and the background tasks.

use mail_core::{AccountConfig, AuthKind, RemoteError, RemoteErrorKind, Security, ServerConfig};
use mail_sync::AccountStatus;
use protocols::{provider_for_email, PresetNote};

use crate::locale::Key;
use crate::mail_service::StartError;

/// OAuth provider id of Google accounts (`AuthKind::OAuth2 { provider }`).
pub(crate) const GOOGLE: &str = "google";

/// The security modes offered by the segmented control, in display order.
pub(crate) const SECURITY_OPTIONS: [Security; 3] =
    [Security::Tls, Security::StartTls, Security::None];

/// The editable server fields of the "Manual settings" disclosure, kept as
/// typed text so half-typed ports survive re-renders.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ManualFields {
    pub imap_host: String,
    pub imap_port: String,
    pub imap_security: Security,
    pub smtp_host: String,
    pub smtp_port: String,
    pub smtp_security: Security,
    pub username: String,
}

impl ManualFields {
    /// Fields prefilled from a discovered or preset config.
    pub(crate) fn from_config(config: &AccountConfig) -> Self {
        Self {
            imap_host: config.imap.host.clone(),
            imap_port: config.imap.port.to_string(),
            imap_security: config.imap.security,
            smtp_host: config.smtp.host.clone(),
            smtp_port: config.smtp.port.to_string(),
            smtp_security: config.smtp.security,
            username: config.username.clone(),
        }
    }

    /// Whether no server was entered yet (discovery has to run on Connect).
    pub(crate) fn is_blank(&self) -> bool {
        self.imap_host.trim().is_empty() && self.smtp_host.trim().is_empty()
    }
}

/// Why the form can't be submitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormError {
    InvalidEmail,
    PasswordRequired,
    HostRequired,
    InvalidPort,
    AlreadyAdded,
}

impl FormError {
    pub(crate) fn key(self) -> Key {
        match self {
            FormError::InvalidEmail => Key::AccountErrInvalidEmail,
            FormError::PasswordRequired => Key::AccountErrPasswordRequired,
            FormError::HostRequired => Key::AccountErrHostRequired,
            FormError::InvalidPort => Key::AccountErrInvalidPort,
            FormError::AlreadyAdded => Key::AccountErrAlreadyAdded,
        }
    }
}

/// What Connect has to do for a password account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConnectPlan {
    /// The servers are known: test this config.
    Ready(AccountConfig),
    /// No server typed yet: discover the settings (network) first, then test
    /// [`with_password_auth`] of the result.
    /// `username`: what the user typed; with nothing, what discovery finds stands.
    Discover {
        email: String,
        username: Option<String>,
    },
}

/// Loose e-mail shape check: one `@`, a non-empty local part without spaces
/// and a dotted domain. Servers do the real validation.
pub(crate) fn is_valid_email(email: &str) -> bool {
    let email = email.trim();
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    let labels_ok = domain
        .split('.')
        .all(|label| !label.is_empty() && !label.starts_with('-') && !label.ends_with('-'));
    !local.is_empty()
        && !domain.contains('@')
        // Dotless domains are legal (`localhost`, intranet hosts, test servers).
        && labels_ok
        && !email.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// A TCP port typed by the user (`1..=65535`).
pub(crate) fn parse_port(text: &str) -> Option<u16> {
    text.trim().parse::<u16>().ok().filter(|port| *port != 0)
}

/// Port to show after switching security from `old` to `new`: follows the
/// conventional port when the field is empty or still holds `old`'s default,
/// otherwise keeps what the user typed (`None` = leave the field alone).
pub(crate) fn port_after_security_change(
    current: &str,
    old: Security,
    new: Security,
    imap: bool,
) -> Option<String> {
    let default = |security: Security| {
        if imap {
            security.default_imap_port()
        } else {
            security.default_smtp_port()
        }
    };
    let current = current.trim();
    (current.is_empty() || parse_port(current) == Some(default(old)))
        .then(|| default(new).to_string())
}

/// Whether `email` is already one of the `existing` account addresses.
pub(crate) fn is_duplicate<'a>(email: &str, existing: impl IntoIterator<Item = &'a str>) -> bool {
    let email = email.trim();
    existing
        .into_iter()
        .any(|other| other.trim().eq_ignore_ascii_case(email))
}

/// Validates the password form and decides how to connect: hosts and ports
/// from the manual fields when any server was entered, discovery otherwise.
pub(crate) fn plan_password_connect<'a>(
    email: &str,
    password: &str,
    manual: &ManualFields,
    existing: impl IntoIterator<Item = &'a str>,
) -> Result<ConnectPlan, FormError> {
    let email = email.trim();
    if !is_valid_email(email) {
        return Err(FormError::InvalidEmail);
    }
    if is_duplicate(email, existing) {
        return Err(FormError::AlreadyAdded);
    }
    if password.is_empty() {
        return Err(FormError::PasswordRequired);
    }
    let typed = manual.username.trim();
    if manual.is_blank() {
        return Ok(ConnectPlan::Discover {
            email: email.to_string(),
            username: (!typed.is_empty()).then(|| typed.to_string()),
        });
    }
    let username = match typed {
        "" => email.to_string(),
        name => name.to_string(),
    };
    if manual.imap_host.trim().is_empty() || manual.smtp_host.trim().is_empty() {
        return Err(FormError::HostRequired);
    }
    let (Some(imap_port), Some(smtp_port)) =
        (parse_port(&manual.imap_port), parse_port(&manual.smtp_port))
    else {
        return Err(FormError::InvalidPort);
    };
    Ok(ConnectPlan::Ready(AccountConfig {
        email: email.to_string(),
        username,
        imap: ServerConfig {
            host: manual.imap_host.trim().to_string(),
            port: imap_port,
            security: manual.imap_security,
        },
        smtp: ServerConfig {
            host: manual.smtp_host.trim().to_string(),
            port: smtp_port,
            security: manual.smtp_security,
        },
        auth: AuthKind::Password,
        sync_window: mail_core::DEFAULT_SYNC_WINDOW,
    }))
}

/// A discovered config turned into a password account for `email` /
/// `username` (presets such as Gmail default to OAuth; the app-password path
/// reuses their servers with a plain login).
pub(crate) fn with_password_auth(
    mut config: AccountConfig,
    email: &str,
    username: &str,
) -> AccountConfig {
    config.email = email.trim().to_string();
    config.username = username.trim().to_string();
    config.auth = AuthKind::Password;
    config
}

/// Whether `email` belongs to a provider signed in through Google OAuth.
pub(crate) fn is_google_address(email: &str) -> bool {
    provider_for_email(email).is_some_and(|preset| preset.oauth == Some(GOOGLE))
}

/// The note the form must show for `email`'s provider, if any.
pub(crate) fn preset_note(email: &str) -> Option<PresetNote> {
    provider_for_email(email).and_then(|preset| preset.note)
}

/// Localized text of a provider note.
pub(crate) fn note_key(note: PresetNote) -> Key {
    match note {
        PresetNote::AppPasswordRequired => Key::AccountNoteAppPassword,
        PresetNote::OAuthRequired => Key::AccountNoteOAuthRequired,
        PresetNote::ProtonBridge => Key::AccountNoteProtonBridge,
        PresetNote::EnableImapFirst => Key::AccountNoteEnableImap,
    }
}

/// The address a Google sign-in is for: what the user typed, else the one
/// Google reported. `None` when neither is a valid address.
pub(crate) fn google_account_email(typed: &str, from_tokens: Option<&str>) -> Option<String> {
    let typed = typed.trim();
    if is_valid_email(typed) {
        return Some(typed.to_string());
    }
    from_tokens
        .map(str::trim)
        .filter(|email| is_valid_email(email))
        .map(str::to_string)
}

/// Config of a Google account: the Gmail preset's servers, XOAUTH2 with the
/// `google` provider, and the address as login.
pub(crate) fn google_config(email: &str) -> AccountConfig {
    let preset = provider_for_email("x@gmail.com").expect("the Gmail preset is built in");
    let mut config = preset.account_config(email);
    config.username = email.trim().to_string();
    config.auth = AuthKind::OAuth2 {
        provider: GOOGLE.to_string(),
    };
    config
}

/// Whether a re-sign-in matched the account: Google's reported address (when
/// it sent one) must be the account's login.
pub(crate) fn same_google_account(expected: &str, from_tokens: Option<&str>) -> bool {
    from_tokens.is_none_or(|email| email.trim().eq_ignore_ascii_case(expected.trim()))
}

/// Localized message for a failed connection test or sign-in. `None` for a
/// cancellation the user asked for (nothing to report).
pub(crate) fn error_key(kind: RemoteErrorKind, oauth: bool, user_cancelled: bool) -> Option<Key> {
    Some(match kind {
        RemoteErrorKind::Auth if oauth => Key::AccountErrGoogleDenied,
        RemoteErrorKind::Auth => Key::AccountErrWrongPassword,
        RemoteErrorKind::Network => Key::AccountErrCannotReach,
        RemoteErrorKind::Tls => Key::AccountErrCertificate,
        RemoteErrorKind::Cancelled if user_cancelled => return None,
        RemoteErrorKind::Cancelled => Key::AccountErrCancelled,
        RemoteErrorKind::Protocol
        | RemoteErrorKind::NotFound
        | RemoteErrorKind::Unsupported
        | RemoteErrorKind::Unreadable
        | RemoteErrorKind::TooLarge
        | RemoteErrorKind::Other => Key::AccountErrUnexpected,
    })
}

/// Whether the raw server message adds information under the localized one
/// (auth/cancel messages are noise or may echo credentials hints).
pub(crate) fn shows_error_detail(kind: RemoteErrorKind) -> bool {
    !matches!(kind, RemoteErrorKind::Auth | RemoteErrorKind::Cancelled)
}

/// Why adding an account or signing in again failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SetupFailure {
    Form(FormError),
    Remote(RemoteError),
    /// A re-sign-in returned another Google account than the one stored.
    DifferentGoogleAccount,
    /// No Google OAuth client is configured in this build/environment.
    GoogleUnavailable,
    /// The account could not be stored (database error text).
    SaveFailed(String),
}

impl From<FormError> for SetupFailure {
    fn from(error: FormError) -> Self {
        SetupFailure::Form(error)
    }
}

impl From<RemoteError> for SetupFailure {
    fn from(error: RemoteError) -> Self {
        SetupFailure::Remote(error)
    }
}

/// A message shown under the form or an account row: a localized key plus
/// an optional raw detail (server text, database error).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FailureMessage {
    pub key: Key,
    pub detail: Option<String>,
}

impl SetupFailure {
    /// What to show for this failure (`None` = the user cancelled, say nothing).
    pub(crate) fn message(&self, oauth: bool, user_cancelled: bool) -> Option<FailureMessage> {
        let (key, detail) = match self {
            SetupFailure::Form(error) => (error.key(), None),
            SetupFailure::Remote(error) => (
                error_key(error.kind, oauth, user_cancelled)?,
                (shows_error_detail(error.kind) && !error.message.is_empty())
                    .then(|| error.message.clone()),
            ),
            SetupFailure::DifferentGoogleAccount => (Key::AccountErrDifferentGoogle, None),
            SetupFailure::GoogleUnavailable => (Key::AccountGoogleNotConfigured, None),
            SetupFailure::SaveFailed(error) => (Key::AccountErrSaveFailed, Some(error.clone())),
        };
        Some(FailureMessage { key, detail })
    }
}

/// Outcome of a password Connect: the config that was tried (so discovered
/// settings can be shown in the manual fields) and the verdict.
pub(crate) struct ConnectOutcome {
    pub tried: Option<AccountConfig>,
    pub result: Result<AccountConfig, SetupFailure>,
}

/// Body of the background Connect job for password accounts: discovers the
/// servers when the plan asks for it, then tests the login. `discover` and
/// `test` are the blocking network calls (injected so this is testable).
pub(crate) fn run_password_connect(
    plan: ConnectPlan,
    discover: impl FnOnce(&str) -> Option<AccountConfig>,
    test: impl FnOnce(&AccountConfig) -> mail_core::RemoteResult<()>,
) -> ConnectOutcome {
    let config = match plan {
        ConnectPlan::Ready(config) => config,
        ConnectPlan::Discover { email, username } => match discover(&email) {
            Some(mut found) => {
                let username = username.unwrap_or_else(|| std::mem::take(&mut found.username));
                with_password_auth(found, &email, &username)
            }
            None => {
                return ConnectOutcome {
                    tried: None,
                    result: Err(FormError::HostRequired.into()),
                }
            }
        },
    };
    let result = test(&config).map(|()| config.clone()).map_err(Into::into);
    ConnectOutcome {
        tried: Some(config),
        result,
    }
}

/// Body of the background "Sign in with Google" job: interactive sign-in,
/// pick the address, refuse duplicates, then test IMAP/SMTP with the new
/// refresh token. Returns the config and the refresh token to store.
pub(crate) fn run_google_sign_in(
    typed_email: &str,
    existing: &[String],
    authorize: impl FnOnce() -> mail_core::RemoteResult<protocols::OAuthTokens>,
    test: impl FnOnce(&AccountConfig, &str) -> mail_core::RemoteResult<()>,
) -> Result<(AccountConfig, String), SetupFailure> {
    let tokens = authorize()?;
    let email = google_account_email(typed_email, tokens.email.as_deref())
        .ok_or(FormError::InvalidEmail)?;
    if is_duplicate(&email, existing.iter().map(String::as_str)) {
        return Err(FormError::AlreadyAdded.into());
    }
    let config = google_config(&email);
    test(&config, &tokens.refresh_token)?;
    Ok((config, tokens.refresh_token))
}

/// Body of the background "Sign in again" job: interactive sign-in that must
/// return the account's own Google login. Returns the new refresh token.
pub(crate) fn run_google_reauth(
    expected_login: &str,
    authorize: impl FnOnce() -> mail_core::RemoteResult<protocols::OAuthTokens>,
) -> Result<String, SetupFailure> {
    let tokens = authorize()?;
    if !same_google_account(expected_login, tokens.email.as_deref()) {
        return Err(SetupFailure::DifferentGoogleAccount);
    }
    Ok(tokens.refresh_token)
}

/// How an account appears in the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccountHealth {
    /// Local/demo account without a server.
    Local,
    UpToDate,
    Syncing,
    Offline,
    SignInFailed,
    PasswordMissing,
    GoogleUnavailable,
    Error,
}

impl AccountHealth {
    pub(crate) fn key(self) -> Key {
        match self {
            AccountHealth::Local => Key::AccountLocalOnly,
            AccountHealth::UpToDate => Key::AccountUpToDate,
            AccountHealth::Syncing => Key::AccountSyncing,
            AccountHealth::Offline => Key::AccountOffline,
            AccountHealth::SignInFailed => Key::AccountSignInFailed,
            AccountHealth::PasswordMissing => Key::AccountPasswordMissing,
            AccountHealth::GoogleUnavailable => Key::AccountGoogleUnavailable,
            AccountHealth::Error => Key::StatusSyncError,
        }
    }

    /// Whether the state needs the user's attention (shown in the error color).
    pub(crate) fn is_problem(self) -> bool {
        matches!(
            self,
            AccountHealth::SignInFailed
                | AccountHealth::PasswordMissing
                | AccountHealth::GoogleUnavailable
                | AccountHealth::Error
        )
    }
}

/// Folds an account's provider, engine status and start error into one state.
/// A start error wins (the engine isn't running then); no status yet means the
/// account is still connecting.
pub(crate) fn account_health(
    provider: &str,
    uses_oauth: bool,
    status: Option<&AccountStatus>,
    start_error: Option<&StartError>,
) -> AccountHealth {
    if provider == storage::PROVIDER_LOCAL {
        return AccountHealth::Local;
    }
    match start_error {
        Some(StartError::MissingSecret) if uses_oauth => return AccountHealth::SignInFailed,
        Some(StartError::MissingSecret) => return AccountHealth::PasswordMissing,
        Some(StartError::OAuthUnavailable(_)) => return AccountHealth::GoogleUnavailable,
        Some(StartError::NotRemote) => return AccountHealth::Error,
        None => {}
    }
    match status {
        None | Some(AccountStatus::Connecting) | Some(AccountStatus::Syncing { .. }) => {
            AccountHealth::Syncing
        }
        Some(AccountStatus::Idle { .. }) => AccountHealth::UpToDate,
        Some(AccountStatus::Offline { .. }) => AccountHealth::Offline,
        Some(AccountStatus::AuthFailed { .. }) => AccountHealth::SignInFailed,
        Some(AccountStatus::Error { .. }) => AccountHealth::Error,
    }
}

/// Badge of a stored account's provider: a localized "Local", or the
/// protocol/brand name (not translated).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderBadge {
    Local,
    Imap,
    Gmail,
}

impl ProviderBadge {
    pub(crate) fn for_provider(provider: &str) -> Self {
        match provider {
            storage::PROVIDER_GMAIL => ProviderBadge::Gmail,
            storage::PROVIDER_LOCAL => ProviderBadge::Local,
            _ => ProviderBadge::Imap,
        }
    }

    pub(crate) fn label(self, language: crate::locale::Language) -> &'static str {
        match self {
            ProviderBadge::Local => Key::AccountBadgeLocal.tr(language),
            ProviderBadge::Imap => "IMAP",
            ProviderBadge::Gmail => "Gmail",
        }
    }
}

/// Label of a security mode in the segmented control.
pub(crate) fn security_label(
    security: Security,
    language: crate::locale::Language,
) -> &'static str {
    match security {
        Security::Tls => "TLS",
        Security::StartTls => "STARTTLS",
        Security::None => Key::AccountSecurityNone.tr(language),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locale::Language;

    fn manual() -> ManualFields {
        ManualFields {
            imap_host: " imap.example.com ".into(),
            imap_port: "993".into(),
            imap_security: Security::Tls,
            smtp_host: "smtp.example.com".into(),
            smtp_port: " 587 ".into(),
            smtp_security: Security::StartTls,
            username: String::new(),
        }
    }

    #[test]
    fn email_shape() {
        for ok in [
            "me@example.com",
            " a.b+c@sub.example.com.br ",
            "x@y.io",
            "alice@localhost",
        ] {
            assert!(is_valid_email(ok), "{ok}");
        }
        for bad in [
            "",
            "me",
            "me@",
            "@example.com",
            "me@@example.com",
            "me@exa mple.com",
            "me@example..com",
            "me@.example.com",
            "me@-example.com",
            "me@a@b.com",
        ] {
            assert!(!is_valid_email(bad), "{bad}");
        }
    }

    #[test]
    fn ports_are_1_to_65535() {
        assert_eq!(parse_port("993"), Some(993));
        assert_eq!(parse_port(" 25 "), Some(25));
        assert_eq!(parse_port("65535"), Some(65535));
        assert_eq!(parse_port("0"), None);
        assert_eq!(parse_port("65536"), None);
        assert_eq!(parse_port("-1"), None);
        assert_eq!(parse_port("abc"), None);
        assert_eq!(parse_port(""), None);
    }

    #[test]
    fn security_change_follows_default_ports_only() {
        assert_eq!(
            port_after_security_change("993", Security::Tls, Security::StartTls, true).as_deref(),
            Some("143")
        );
        assert_eq!(
            port_after_security_change("", Security::Tls, Security::None, false).as_deref(),
            Some("25")
        );
        assert_eq!(
            port_after_security_change("465", Security::Tls, Security::StartTls, false).as_deref(),
            Some("587")
        );
        assert_eq!(
            port_after_security_change("3143", Security::Tls, Security::None, true),
            None
        );
    }

    #[test]
    fn duplicates_ignore_case_and_spaces() {
        assert!(is_duplicate(" Me@Example.com", ["me@example.com"]));
        assert!(!is_duplicate("you@example.com", ["me@example.com"]));
    }

    #[test]
    fn manual_fields_build_a_password_config() {
        let plan = plan_password_connect(" me@example.com ", "pw", &manual(), []).unwrap();
        let ConnectPlan::Ready(config) = plan else {
            panic!("expected a ready config");
        };
        assert_eq!(config.email, "me@example.com");
        assert_eq!(config.username, "me@example.com");
        assert_eq!(config.imap.host, "imap.example.com");
        assert_eq!(config.imap.port, 993);
        assert_eq!(config.smtp.port, 587);
        assert_eq!(config.smtp.security, Security::StartTls);
        assert_eq!(config.auth, AuthKind::Password);

        let mut custom = manual();
        custom.username = " login ".into();
        let Ok(ConnectPlan::Ready(config)) =
            plan_password_connect("me@example.com", "pw", &custom, [])
        else {
            panic!("expected a ready config");
        };
        assert_eq!(config.username, "login");
    }

    #[test]
    fn blank_servers_mean_discovery() {
        assert_eq!(
            plan_password_connect("me@example.com", "pw", &ManualFields::default(), []),
            Ok(ConnectPlan::Discover {
                email: "me@example.com".into(),
                username: None,
            })
        );
    }

    #[test]
    fn validation_errors_in_order() {
        let none: [&str; 0] = [];
        assert_eq!(
            plan_password_connect("nope", "pw", &manual(), none),
            Err(FormError::InvalidEmail)
        );
        assert_eq!(
            plan_password_connect("me@example.com", "pw", &manual(), ["ME@example.com"]),
            Err(FormError::AlreadyAdded)
        );
        assert_eq!(
            plan_password_connect("me@example.com", "", &manual(), none),
            Err(FormError::PasswordRequired)
        );
        let mut missing = manual();
        missing.smtp_host = " ".into();
        assert_eq!(
            plan_password_connect("me@example.com", "pw", &missing, none),
            Err(FormError::HostRequired)
        );
        let mut bad_port = manual();
        bad_port.imap_port = "99999".into();
        assert_eq!(
            plan_password_connect("me@example.com", "pw", &bad_port, none),
            Err(FormError::InvalidPort)
        );
        for error in [
            FormError::InvalidEmail,
            FormError::PasswordRequired,
            FormError::HostRequired,
            FormError::InvalidPort,
            FormError::AlreadyAdded,
        ] {
            assert!(!error.key().tr(Language::English).is_empty());
        }
    }

    #[test]
    fn manual_fields_round_trip_a_config() {
        let config = google_config("me@gmail.com");
        let fields = ManualFields::from_config(&config);
        assert_eq!(fields.imap_host, "imap.gmail.com");
        assert_eq!(fields.imap_port, "993");
        assert_eq!(fields.smtp_port, "465");
        assert_eq!(fields.username, "me@gmail.com");
        assert!(!fields.is_blank());
        assert!(ManualFields::default().is_blank());
    }

    #[test]
    fn discovered_config_becomes_password_auth() {
        let config = with_password_auth(google_config("me@gmail.com"), " me@gmail.com ", " me ");
        assert_eq!(config.auth, AuthKind::Password);
        assert_eq!(config.email, "me@gmail.com");
        assert_eq!(config.username, "me");
        assert_eq!(config.imap.host, "imap.gmail.com");
    }

    #[test]
    fn gmail_detection() {
        assert!(is_google_address("someone@gmail.com"));
        assert!(is_google_address("Someone@GoogleMail.com"));
        assert!(!is_google_address("someone@outlook.com"));
        assert!(!is_google_address("someone@example.com"));
        assert!(!is_google_address("gmail.com"));
    }

    #[test]
    fn google_config_uses_oauth_and_gmail_servers() {
        let config = google_config(" me@gmail.com ");
        assert_eq!(config.email, "me@gmail.com");
        assert_eq!(config.username, "me@gmail.com");
        assert_eq!(
            config.auth,
            AuthKind::OAuth2 {
                provider: "google".into()
            }
        );
        assert_eq!(config.imap.host, "imap.gmail.com");
        assert_eq!(config.smtp.host, "smtp.gmail.com");
    }

    #[test]
    fn google_email_prefers_typed_then_tokens() {
        assert_eq!(
            google_account_email("me@gmail.com", Some("other@gmail.com")).as_deref(),
            Some("me@gmail.com")
        );
        assert_eq!(
            google_account_email("", Some(" me@gmail.com ")).as_deref(),
            Some("me@gmail.com")
        );
        assert_eq!(google_account_email("", None), None);
        assert_eq!(google_account_email("nope", Some("nope")), None);
    }

    #[test]
    fn reauth_must_match_the_account() {
        assert!(same_google_account("me@gmail.com", Some("ME@gmail.com")));
        assert!(same_google_account("me@gmail.com", None));
        assert!(!same_google_account(
            "me@gmail.com",
            Some("other@gmail.com")
        ));
    }

    #[test]
    fn preset_notes_are_localized() {
        assert_eq!(
            preset_note("a@yahoo.com"),
            Some(PresetNote::AppPasswordRequired)
        );
        assert_eq!(
            preset_note("a@outlook.com"),
            Some(PresetNote::OAuthRequired)
        );
        assert_eq!(preset_note("a@proton.me"), Some(PresetNote::ProtonBridge));
        assert_eq!(preset_note("a@zoho.com"), Some(PresetNote::EnableImapFirst));
        assert_eq!(preset_note("a@gmail.com"), None);
        assert_eq!(preset_note("a@example.com"), None);
        assert_eq!(
            note_key(PresetNote::AppPasswordRequired),
            Key::AccountNoteAppPassword
        );
        assert_eq!(
            note_key(PresetNote::OAuthRequired),
            Key::AccountNoteOAuthRequired
        );
        assert_eq!(
            note_key(PresetNote::ProtonBridge),
            Key::AccountNoteProtonBridge
        );
        assert_eq!(
            note_key(PresetNote::EnableImapFirst),
            Key::AccountNoteEnableImap
        );
    }

    #[test]
    fn remote_errors_map_to_messages() {
        use RemoteErrorKind as K;
        assert_eq!(
            error_key(K::Auth, false, false),
            Some(Key::AccountErrWrongPassword)
        );
        assert_eq!(
            error_key(K::Auth, true, false),
            Some(Key::AccountErrGoogleDenied)
        );
        assert_eq!(
            error_key(K::Network, false, false),
            Some(Key::AccountErrCannotReach)
        );
        assert_eq!(
            error_key(K::Tls, false, false),
            Some(Key::AccountErrCertificate)
        );
        assert_eq!(
            error_key(K::Cancelled, true, false),
            Some(Key::AccountErrCancelled)
        );
        assert_eq!(error_key(K::Cancelled, true, true), None);
        for kind in [K::Protocol, K::NotFound, K::Unsupported, K::Other] {
            assert_eq!(
                error_key(kind, false, false),
                Some(Key::AccountErrUnexpected)
            );
            assert!(shows_error_detail(kind));
        }
        assert!(!shows_error_detail(K::Auth));
        assert!(!shows_error_detail(K::Cancelled));
        assert!(shows_error_detail(K::Network));
    }

    fn tokens(email: Option<&str>) -> protocols::OAuthTokens {
        protocols::OAuthTokens {
            access_token: "access".into(),
            refresh_token: "refresh".into(),
            expires_at: 0,
            email: email.map(str::to_string),
        }
    }

    #[test]
    fn password_connect_tests_ready_configs() {
        let Ok(plan) = plan_password_connect("me@example.com", "pw", &manual(), []) else {
            panic!("valid form");
        };
        let outcome = run_password_connect(plan, |_| panic!("no discovery"), |_| Ok(()));
        assert_eq!(outcome.result.unwrap().imap.host, "imap.example.com");
        assert!(outcome.tried.is_some());
    }

    #[test]
    fn discovery_keeps_its_login_unless_one_was_typed() {
        let found = |email: &str| {
            let mut config = google_config(email);
            config.username = "maria".into();
            Some(config)
        };
        for (typed, expected) in [(None, "maria"), (Some("typed"), "typed")] {
            let plan = ConnectPlan::Discover {
                email: "maria@x.test".into(),
                username: typed.map(str::to_string),
            };
            let seen = std::cell::RefCell::new(String::new());
            run_password_connect(plan, found, |config| {
                *seen.borrow_mut() = config.username.clone();
                Ok(())
            });
            assert_eq!(*seen.borrow(), expected);
        }
    }

    #[test]
    fn password_connect_discovers_then_reports_failures() {
        let plan = ConnectPlan::Discover {
            email: "me@gmail.com".into(),
            username: Some("me@gmail.com".into()),
        };
        let outcome = run_password_connect(
            plan,
            |email| Some(google_config(email)),
            |config| {
                assert_eq!(config.auth, AuthKind::Password);
                Err(RemoteError::new(RemoteErrorKind::Auth, "bad"))
            },
        );
        assert_eq!(outcome.tried.unwrap().imap.host, "imap.gmail.com");
        assert!(matches!(
            outcome.result,
            Err(SetupFailure::Remote(RemoteError {
                kind: RemoteErrorKind::Auth,
                ..
            }))
        ));
        let nothing = run_password_connect(
            ConnectPlan::Discover {
                email: "me@x.com".into(),
                username: Some("me".into()),
            },
            |_| None,
            |_| Ok(()),
        );
        assert!(nothing.tried.is_none());
        assert_eq!(
            nothing.result.unwrap_err(),
            SetupFailure::Form(FormError::HostRequired)
        );
    }

    #[test]
    fn google_sign_in_flow() {
        let (config, refresh) = run_google_sign_in(
            "",
            &[],
            || Ok(tokens(Some("me@gmail.com"))),
            |config, token| {
                assert!(config.uses_oauth());
                assert_eq!(token, "refresh");
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(config.email, "me@gmail.com");
        assert_eq!(refresh, "refresh");

        assert_eq!(
            run_google_sign_in(
                "",
                &["ME@gmail.com".to_string()],
                || Ok(tokens(Some("me@gmail.com"))),
                |_, _| Ok(())
            )
            .unwrap_err(),
            SetupFailure::Form(FormError::AlreadyAdded)
        );
        assert_eq!(
            run_google_sign_in("", &[], || Ok(tokens(None)), |_, _| Ok(())).unwrap_err(),
            SetupFailure::Form(FormError::InvalidEmail)
        );
        let cancelled = RemoteError::new(RemoteErrorKind::Cancelled, "cancelled");
        assert_eq!(
            run_google_sign_in(
                "me@gmail.com",
                &[],
                || Err(cancelled.clone()),
                |_, _| Ok(())
            )
            .unwrap_err(),
            SetupFailure::Remote(cancelled)
        );
    }

    #[test]
    fn google_reauth_checks_the_account() {
        assert_eq!(
            run_google_reauth("me@gmail.com", || Ok(tokens(Some("me@gmail.com")))).unwrap(),
            "refresh"
        );
        assert_eq!(
            run_google_reauth("me@gmail.com", || Ok(tokens(Some("x@gmail.com")))).unwrap_err(),
            SetupFailure::DifferentGoogleAccount
        );
    }

    #[test]
    fn failures_become_messages() {
        let form: SetupFailure = FormError::InvalidPort.into();
        assert_eq!(
            form.message(false, false),
            Some(FailureMessage {
                key: Key::AccountErrInvalidPort,
                detail: None
            })
        );
        let network: SetupFailure =
            RemoteError::new(RemoteErrorKind::Network, "connection refused").into();
        assert_eq!(
            network.message(false, false),
            Some(FailureMessage {
                key: Key::AccountErrCannotReach,
                detail: Some("connection refused".into())
            })
        );
        let auth: SetupFailure = RemoteError::new(RemoteErrorKind::Auth, "LOGIN failed").into();
        assert_eq!(auth.message(false, false).unwrap().detail, None);
        let cancelled: SetupFailure = RemoteError::new(RemoteErrorKind::Cancelled, "").into();
        assert_eq!(cancelled.message(true, true), None);
        assert_eq!(
            SetupFailure::DifferentGoogleAccount
                .message(true, false)
                .unwrap()
                .key,
            Key::AccountErrDifferentGoogle
        );
        assert_eq!(
            SetupFailure::GoogleUnavailable
                .message(true, false)
                .unwrap()
                .key,
            Key::AccountGoogleNotConfigured
        );
        assert_eq!(
            SetupFailure::SaveFailed("disk full".into()).message(false, false),
            Some(FailureMessage {
                key: Key::AccountErrSaveFailed,
                detail: Some("disk full".into())
            })
        );
    }

    #[test]
    fn health_prefers_start_errors_then_status() {
        let imap = storage::PROVIDER_IMAP;
        assert_eq!(
            account_health(storage::PROVIDER_LOCAL, false, None, None),
            AccountHealth::Local
        );
        assert_eq!(
            account_health(
                imap,
                false,
                Some(&AccountStatus::Idle { last_sync: 1 }),
                Some(&StartError::MissingSecret)
            ),
            AccountHealth::PasswordMissing
        );
        assert_eq!(
            account_health(imap, true, None, Some(&StartError::MissingSecret)),
            AccountHealth::SignInFailed
        );
        assert_eq!(
            account_health(
                imap,
                true,
                None,
                Some(&StartError::OAuthUnavailable("google".into()))
            ),
            AccountHealth::GoogleUnavailable
        );
        assert_eq!(
            account_health(imap, false, None, Some(&StartError::NotRemote)),
            AccountHealth::Error
        );
        let cases = [
            (None, AccountHealth::Syncing),
            (Some(AccountStatus::Connecting), AccountHealth::Syncing),
            (
                Some(AccountStatus::Syncing {
                    folder: "INBOX".into(),
                    done: 1,
                    total: 2,
                }),
                AccountHealth::Syncing,
            ),
            (
                Some(AccountStatus::Idle { last_sync: 5 }),
                AccountHealth::UpToDate,
            ),
            (
                Some(AccountStatus::Offline {
                    error: "down".into(),
                    retry_in_secs: 5,
                }),
                AccountHealth::Offline,
            ),
            (
                Some(AccountStatus::AuthFailed { error: "no".into() }),
                AccountHealth::SignInFailed,
            ),
            (
                Some(AccountStatus::Error { error: "db".into() }),
                AccountHealth::Error,
            ),
        ];
        for (status, expected) in cases {
            assert_eq!(account_health(imap, false, status.as_ref(), None), expected);
        }
        assert!(AccountHealth::SignInFailed.is_problem());
        assert!(AccountHealth::PasswordMissing.is_problem());
        assert!(!AccountHealth::Offline.is_problem());
        assert!(!AccountHealth::UpToDate.is_problem());
        assert_eq!(AccountHealth::Error.key(), Key::StatusSyncError);
        assert_eq!(AccountHealth::Local.key(), Key::AccountLocalOnly);
    }

    #[test]
    fn provider_badges_and_security_labels() {
        assert_eq!(
            ProviderBadge::for_provider(storage::PROVIDER_LOCAL),
            ProviderBadge::Local
        );
        assert_eq!(
            ProviderBadge::for_provider(storage::PROVIDER_GMAIL),
            ProviderBadge::Gmail
        );
        assert_eq!(
            ProviderBadge::for_provider(storage::PROVIDER_IMAP),
            ProviderBadge::Imap
        );
        assert_eq!(ProviderBadge::Local.label(Language::English), "Local");
        assert_eq!(ProviderBadge::Gmail.label(Language::Portuguese), "Gmail");
        assert_eq!(
            security_label(Security::StartTls, Language::English),
            "STARTTLS"
        );
        assert_eq!(
            security_label(Security::None, Language::Portuguese),
            "Nenhuma"
        );
        assert_eq!(SECURITY_OPTIONS.len(), 3);
    }
}
