//! Settings window (General, Accounts, Appearance, Notifications, Privacy),
//! opened in its own window like Zed's preferences.
//!
//! The Accounts section lists the stored accounts with their live sync state
//! and hosts the inline "Add account" form. Everything that touches the
//! network (server discovery, login tests, Google sign-in) runs on the
//! background executor; the pure decisions live in `account_setup`.

use std::collections::HashMap;
use std::time::Duration;

use gpui::{
    App, AppContext, Context, Entity, Focusable, FontWeight, IntoElement, PromptLevel,
    SharedString, Subscription, Task, WeakEntity, Window,
};
use mail_core::{AccountConfig, AuthKind, CancelToken, Security};
use protocols::{ConnectOptions, Credential, OAuthProvider};
use theme::{ActiveTheme, Appearance};
use ui::prelude::*;
use ui::{
    Button, ButtonStyle, Color, Icon, IconName, IconSize, Label, LabelSize, ListItem, Switch,
    TextInput, TextInputEvent,
};

use crate::account_setup::{
    self, AccountHealth, ConnectOutcome, FailureMessage, ManualFields, ProviderBadge, SetupFailure,
    SECURITY_OPTIONS,
};
use crate::actions::ComposeClose;
use crate::data;
use crate::default_mail_app;
use crate::locale::{self, ActiveLanguage, Key, Language};
use crate::root::RootView;

/// Pause after the last keystroke in the e-mail field before discovering
/// server settings (avoids one lookup per typed character).
const DISCOVERY_DELAY: Duration = Duration::from_millis(600);
/// Per-request bound of the autoconfig lookups.
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(8);
/// How long the browser sign-in may take before it is abandoned.
const OAUTH_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Active section of the settings screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsSection {
    General,
    Accounts,
    Appearance,
    Notifications,
    Privacy,
}

impl SettingsSection {
    pub(crate) const ALL: [SettingsSection; 5] = [
        SettingsSection::General,
        SettingsSection::Accounts,
        SettingsSection::Appearance,
        SettingsSection::Notifications,
        SettingsSection::Privacy,
    ];

    fn title_key(self) -> Key {
        match self {
            SettingsSection::General => Key::SettingsGeneral,
            SettingsSection::Accounts => Key::SettingsAccounts,
            SettingsSection::Appearance => Key::SettingsAppearance,
            SettingsSection::Notifications => Key::SettingsNotifications,
            SettingsSection::Privacy => Key::SettingsPrivacy,
        }
    }

    fn icon(self) -> IconName {
        match self {
            SettingsSection::General => IconName::Settings,
            SettingsSection::Accounts => IconName::Account,
            SettingsSection::Appearance => IconName::Palette,
            SettingsSection::Notifications => IconName::Flag,
            SettingsSection::Privacy => IconName::Shield,
        }
    }
}

/// The inline "Add account" form. Text lives in `TextInput` entities; the
/// rest is the state of the background lookups.
struct AccountForm {
    name: Entity<TextInput>,
    email: Entity<TextInput>,
    password: Entity<TextInput>,
    imap_host: Entity<TextInput>,
    imap_port: Entity<TextInput>,
    smtp_host: Entity<TextInput>,
    smtp_port: Entity<TextInput>,
    username: Entity<TextInput>,
    imap_security: Security,
    smtp_security: Security,
    show_manual: bool,
    /// The user typed in a server field: discovery stops overwriting them.
    manual_edited: bool,
    /// Gmail over IMAP with an app password instead of Google sign-in.
    gmail_app_password: bool,
    /// Google OAuth client of this build (`None` = sign-in unavailable).
    google_client: Option<OAuthProvider>,
    /// Bumped on every e-mail edit so stale discovery results are dropped.
    discovery_seq: u64,
    looking_up: bool,
    discovery_task: Option<Task<()>>,
    /// A login test (or Google sign-in) is running.
    checking: bool,
    connect_task: Option<Task<()>>,
    message: Option<FailureMessage>,
    _subscriptions: Vec<Subscription>,
}

impl AccountForm {
    fn new(language: Language, cx: &mut Context<SettingsView>) -> Self {
        let mut input = |placeholder: &str| {
            let placeholder = placeholder.to_string();
            cx.new(|cx| TextInput::new(placeholder, cx))
        };
        let name = input(Key::AccountNamePlaceholder.tr(language));
        let email = input(Key::AccountEmailPlaceholder.tr(language));
        let password = input("");
        let imap_host = input("imap.example.com");
        let imap_port = input("993");
        let smtp_host = input("smtp.example.com");
        let smtp_port = input("587");
        let username = input("");
        password.update(cx, |input, cx| input.set_masked(true, cx));

        let mut subscriptions = vec![cx.subscribe(
            &email,
            |this: &mut SettingsView, _, _: &TextInputEvent, cx| this.on_email_changed(cx),
        )];
        for field in [&name, &password] {
            subscriptions.push(cx.subscribe(
                field,
                |this: &mut SettingsView, _, _: &TextInputEvent, cx| {
                    if let Some(form) = this.form.as_mut() {
                        form.message = None;
                        cx.notify();
                    }
                },
            ));
        }
        for field in [&imap_host, &imap_port, &smtp_host, &smtp_port, &username] {
            subscriptions.push(cx.subscribe(
                field,
                |this: &mut SettingsView, _, _: &TextInputEvent, cx| {
                    if let Some(form) = this.form.as_mut() {
                        form.manual_edited = true;
                        form.message = None;
                        cx.notify();
                    }
                },
            ));
        }

        Self {
            name,
            email,
            password,
            imap_host,
            imap_port,
            smtp_host,
            smtp_port,
            username,
            imap_security: Security::Tls,
            smtp_security: Security::StartTls,
            show_manual: false,
            manual_edited: false,
            gmail_app_password: false,
            google_client: protocols::oauth_provider(account_setup::GOOGLE),
            discovery_seq: 0,
            looking_up: false,
            discovery_task: None,
            checking: false,
            connect_task: None,
            message: None,
            _subscriptions: subscriptions,
        }
    }

    fn text(input: &Entity<TextInput>, cx: &App) -> String {
        input.read(cx).content().to_string()
    }

    fn email(&self, cx: &App) -> String {
        Self::text(&self.email, cx).trim().to_string()
    }

    fn manual_fields(&self, cx: &App) -> ManualFields {
        ManualFields {
            imap_host: Self::text(&self.imap_host, cx),
            imap_port: Self::text(&self.imap_port, cx),
            imap_security: self.imap_security,
            smtp_host: Self::text(&self.smtp_host, cx),
            smtp_port: Self::text(&self.smtp_port, cx),
            smtp_security: self.smtp_security,
            username: Self::text(&self.username, cx),
        }
    }

    /// Shows `fields` in the manual inputs (no `Changed` events: not an edit).
    fn set_manual(&mut self, fields: &ManualFields, cx: &mut Context<SettingsView>) {
        for (input, value) in [
            (&self.imap_host, &fields.imap_host),
            (&self.imap_port, &fields.imap_port),
            (&self.smtp_host, &fields.smtp_host),
            (&self.smtp_port, &fields.smtp_port),
            (&self.username, &fields.username),
        ] {
            input.update(cx, |input, cx| input.set_content(value, cx));
        }
        self.imap_security = fields.imap_security;
        self.smtp_security = fields.smtp_security;
    }

    /// Whether the address is a Gmail one and Google sign-in replaces the
    /// password field.
    fn uses_google_sign_in(&self, email: &str) -> bool {
        account_setup::is_google_address(email)
            && self.google_client.is_some()
            && !self.gmail_app_password
    }
}

/// What a running Google sign-in is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OAuthTarget {
    NewAccount,
    Account(i64),
}

/// A running browser sign-in (one at a time: it owns a loopback listener).
struct OAuthFlow {
    target: OAuthTarget,
    cancel: CancelToken,
    user_cancelled: bool,
    _tasks: [Task<()>; 2],
}

/// Standalone preferences window content (Zed-style: settings open in their own
/// window). Theme and language are app-wide globals, so changes made here are
/// reflected in the main window immediately.
pub(crate) struct SettingsView {
    pub(crate) section: SettingsSection,
    /// Whether BGMail is the default mail app, once the system answered.
    default_app: Option<default_mail_app::Status>,
    default_app_checked: bool,
    /// What happened the last time the user asked to make it the default.
    default_app_message: Option<SharedString>,
    /// Back-reference to the main view, which owns accounts and preferences.
    /// Weak so the settings window never keeps it alive.
    root: WeakEntity<RootView>,
    /// Re-renders this window when the main view changes (account list, sync
    /// status, preferences toggled elsewhere).
    root_observation: Option<Subscription>,
    /// Created on first use of the Accounts section (needs a context).
    form: Option<AccountForm>,
    /// Whether the user opened the form ("Add account…"). With no remote
    /// account yet the form is always shown.
    form_open: bool,
    /// Account whose password is being replaced inline.
    password_edit: Option<(i64, Entity<TextInput>)>,
    /// Account being renamed, with the field holding its new name.
    rename_edit: Option<(i64, Entity<TextInput>)>,
    oauth: Option<OAuthFlow>,
    /// Last failure of a per-account action (re-sign-in), shown under its row.
    account_messages: HashMap<i64, FailureMessage>,
}

impl SettingsView {
    /// `_accounts` is the caller's snapshot; the live list is read from
    /// `root` on every render instead.
    pub(crate) fn new(_accounts: Vec<data::Account>, root: WeakEntity<RootView>) -> Self {
        Self {
            default_app: None,
            default_app_checked: false,
            default_app_message: None,
            section: SettingsSection::General,
            root,
            root_observation: None,
            form: None,
            form_open: false,
            password_edit: None,
            rename_edit: None,
            oauth: None,
            account_messages: HashMap::new(),
        }
    }

    /// Switches to `section` (e.g. "Add account" from the welcome screen,
    /// which also opens the form).
    pub(crate) fn show_section(&mut self, section: SettingsSection, cx: &mut Context<Self>) {
        self.section = section;
        if section == SettingsSection::Accounts {
            self.form_open = true;
        }
        cx.notify();
    }

    pub(crate) fn close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let root = self.root.clone();
        window.defer(cx, move |window, cx| {
            if let Some(root) = root.upgrade() {
                root.update(cx, |root, cx| {
                    root.on_settings_window_closed(cx);
                });
            }
            window.remove_window();
        });
    }

    // ----- Accounts: data from the main view ----------------------------------

    fn accounts(&self, cx: &App) -> Vec<storage::Account> {
        self.root
            .upgrade()
            .map(|root| root.read(cx).accounts().to_vec())
            .unwrap_or_default()
    }

    fn existing_emails(&self, cx: &App) -> Vec<String> {
        self.accounts(cx)
            .into_iter()
            .map(|account| account.email)
            .collect()
    }

    fn health(&self, account: &storage::Account, cx: &App) -> AccountHealth {
        let uses_oauth =
            AccountConfig::from_json(&account.config_json).is_some_and(|c| c.uses_oauth());
        let Some(root) = self.root.upgrade() else {
            return AccountHealth::Error;
        };
        let root = root.read(cx);
        account_setup::account_health(
            &account.provider,
            uses_oauth,
            root.account_status(account.id).as_ref(),
            root.account_start_error(account.id),
        )
    }

    // ----- Add account: discovery ---------------------------------------------

    /// Prefills the server fields for the typed address: instantly from a
    /// preset, otherwise from a debounced background discovery. Never
    /// overwrites fields the user edited.
    fn on_email_changed(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        form.message = None;
        form.discovery_seq += 1;
        form.looking_up = false;
        form.discovery_task = None;
        cx.notify();
        if form.manual_edited {
            return;
        }
        let email = form.email(cx);
        if !account_setup::is_valid_email(&email) {
            form.set_manual(&ManualFields::default(), cx);
            return;
        }
        if let Some(preset) = protocols::provider_for_email(&email) {
            form.set_manual(
                &ManualFields::from_config(&preset.account_config(&email)),
                cx,
            );
            return;
        }
        form.set_manual(&ManualFields::default(), cx);
        let seq = form.discovery_seq;
        form.discovery_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DISCOVERY_DELAY).await;
            let still_current = this
                .update(cx, |this, cx| {
                    let Some(form) = this.form.as_mut().filter(|f| f.discovery_seq == seq) else {
                        return false;
                    };
                    form.looking_up = true;
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            if !still_current {
                return;
            }
            let lookup = email.clone();
            let found = cx
                .background_spawn(
                    async move { protocols::discover_config(&lookup, DISCOVERY_TIMEOUT) },
                )
                .await;
            this.update(cx, |this, cx| {
                let Some(form) = this.form.as_mut().filter(|f| f.discovery_seq == seq) else {
                    return;
                };
                form.looking_up = false;
                if let (Some(config), false) = (found, form.manual_edited) {
                    form.set_manual(&ManualFields::from_config(&config), cx);
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// Switches a server's security; the port follows the conventional one
    /// unless the user typed a custom port.
    fn set_security(&mut self, imap: bool, security: Security, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        let (old, port) = if imap {
            (
                std::mem::replace(&mut form.imap_security, security),
                &form.imap_port,
            )
        } else {
            (
                std::mem::replace(&mut form.smtp_security, security),
                &form.smtp_port,
            )
        };
        let current = AccountForm::text(port, cx);
        if let Some(next) = account_setup::port_after_security_change(&current, old, security, imap)
        {
            port.update(cx, |input, cx| input.set_content(next, cx));
        }
        form.manual_edited = true;
        form.message = None;
        cx.notify();
    }

    // ----- Add account: connect -----------------------------------------------

    /// Validates the password form, then discovers (if needed) and tests the
    /// login in the background; success adds the account.
    fn connect(&mut self, cx: &mut Context<Self>) {
        let existing = self.existing_emails(cx);
        let Some(form) = self.form.as_mut() else {
            return;
        };
        if form.checking || self.oauth.is_some() {
            return;
        }
        let email = form.email(cx);
        let password = AccountForm::text(&form.password, cx);
        let plan = account_setup::plan_password_connect(
            &email,
            &password,
            &form.manual_fields(cx),
            existing.iter().map(String::as_str),
        );
        let plan = match plan {
            Ok(plan) => plan,
            Err(error) => {
                form.message = SetupFailure::from(error).message(false, false);
                cx.notify();
                return;
            }
        };
        let name = AccountForm::text(&form.name, cx);
        form.checking = true;
        form.message = None;
        form.connect_task = Some(cx.spawn(async move |this, cx| {
            let secret = password.clone();
            let outcome = cx
                .background_spawn(async move {
                    account_setup::run_password_connect(
                        plan,
                        |email| protocols::discover_config(email, DISCOVERY_TIMEOUT),
                        |config| {
                            protocols::test_account(
                                config.clone(),
                                Credential::Password(secret),
                                ConnectOptions::default(),
                            )
                        },
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                this.finish_password_connect(name, password, outcome, cx)
            })
            .ok();
        }));
        cx.notify();
    }

    fn finish_password_connect(
        &mut self,
        name: String,
        password: String,
        outcome: ConnectOutcome,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        form.checking = false;
        form.connect_task = None;
        // Show what was tried so a failure can be fixed by hand.
        if let (Some(tried), false) = (&outcome.tried, form.manual_edited) {
            form.set_manual(&ManualFields::from_config(tried), cx);
        }
        match outcome.result {
            Ok(config) => self.add_account(name, config, password, cx),
            Err(failure) => {
                if matches!(&failure, SetupFailure::Remote(error)
                    if error.kind != mail_core::RemoteErrorKind::Auth)
                    || matches!(failure, SetupFailure::Form(_))
                {
                    form.show_manual = true;
                }
                form.message = failure.message(false, false);
            }
        }
        cx.notify();
    }

    /// Stores the verified account through the main view and resets the form.
    fn add_account(
        &mut self,
        name: String,
        config: AccountConfig,
        secret: String,
        cx: &mut Context<Self>,
    ) {
        let added = self.root.update(cx, |root, cx| {
            root.add_remote_account(&name, config, secret, cx)
        });
        match added {
            Ok(Ok(_)) => {
                self.form = None;
                self.form_open = false;
            }
            Ok(Err(error)) => {
                if let Some(form) = self.form.as_mut() {
                    form.message = SetupFailure::SaveFailed(error).message(false, false);
                }
            }
            Err(_) => {}
        }
        cx.notify();
    }

    // ----- Google sign-in -----------------------------------------------------

    /// Starts a browser sign-in on the background executor. The authorization
    /// URL comes back over a channel and is opened from the UI thread.
    /// `job` receives the authorize closure and returns what `finish` needs.
    fn start_oauth<R: Send + 'static>(
        &mut self,
        target: OAuthTarget,
        provider: OAuthProvider,
        job: impl FnOnce(&dyn Fn() -> mail_core::RemoteResult<protocols::OAuthTokens>) -> R
            + Send
            + 'static,
        finish: impl FnOnce(&mut Self, R, bool, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let cancel = CancelToken::new();
        let (url_sender, url_receiver) = async_channel::bounded::<String>(1);
        let job_cancel = cancel.clone();
        let work = cx.background_spawn(async move {
            let open_browser = move |url: &str| {
                url_sender.try_send(url.to_string()).ok();
            };
            let authorize =
                || protocols::authorize(&provider, &open_browser, &job_cancel, OAUTH_TIMEOUT);
            job(&authorize)
        });
        let finish_task = cx.spawn(async move |this, cx| {
            let result = work.await;
            this.update(cx, |this, cx| {
                let user_cancelled = this.oauth.take().is_some_and(|flow| flow.user_cancelled);
                finish(this, result, user_cancelled, cx);
                cx.notify();
            })
            .ok();
        });
        let browser_task = cx.spawn(async move |_, cx| {
            if let Ok(url) = url_receiver.recv().await {
                cx.update(|cx| cx.open_url(&url)).ok();
            }
        });
        self.oauth = Some(OAuthFlow {
            target,
            cancel,
            user_cancelled: false,
            _tasks: [finish_task, browser_task],
        });
        cx.notify();
    }

    /// "Sign in with Google" in the form: sign in, test, then add the account.
    fn sign_in_with_google(&mut self, cx: &mut Context<Self>) {
        let existing = self.existing_emails(cx);
        let Some(form) = self.form.as_mut() else {
            return;
        };
        if form.checking || self.oauth.is_some() {
            return;
        }
        let Some(provider) = form.google_client.clone() else {
            form.message = SetupFailure::GoogleUnavailable.message(true, false);
            cx.notify();
            return;
        };
        let typed = form.email(cx);
        if !typed.is_empty() && !account_setup::is_valid_email(&typed) {
            form.message =
                SetupFailure::from(account_setup::FormError::InvalidEmail).message(true, false);
            cx.notify();
            return;
        }
        let name = AccountForm::text(&form.name, cx);
        form.checking = true;
        form.message = None;
        let test_provider = provider.clone();
        self.start_oauth(
            OAuthTarget::NewAccount,
            provider,
            move |authorize| {
                account_setup::run_google_sign_in(&typed, &existing, authorize, |config, token| {
                    protocols::test_account(
                        config.clone(),
                        Credential::OAuth {
                            provider: test_provider,
                            refresh_token: token.to_string(),
                        },
                        ConnectOptions::default(),
                    )
                })
            },
            move |this, result, user_cancelled, cx| {
                if let Some(form) = this.form.as_mut() {
                    form.checking = false;
                }
                match result {
                    Ok((config, refresh_token)) => {
                        this.add_account(name, config, refresh_token, cx)
                    }
                    Err(failure) => {
                        if let Some(form) = this.form.as_mut() {
                            form.message = failure.message(true, user_cancelled);
                        }
                    }
                }
            },
            cx,
        );
    }

    /// "Sign in again" for an OAuth account: new refresh token → keychain.
    fn sign_in_again(&mut self, account_id: i64, cx: &mut Context<Self>) {
        if self.oauth.is_some() {
            return;
        }
        let Some(account) = self.accounts(cx).into_iter().find(|a| a.id == account_id) else {
            return;
        };
        let Some(config) = AccountConfig::from_json(&account.config_json) else {
            return;
        };
        let AuthKind::OAuth2 { provider } = &config.auth else {
            return;
        };
        self.account_messages.remove(&account_id);
        let Some(provider) = protocols::oauth_provider(provider) else {
            if let Some(message) = SetupFailure::GoogleUnavailable.message(true, false) {
                self.account_messages.insert(account_id, message);
            }
            cx.notify();
            return;
        };
        let login = config.username.clone();
        self.start_oauth(
            OAuthTarget::Account(account_id),
            provider,
            move |authorize| account_setup::run_google_reauth(&login, authorize),
            move |this, result, user_cancelled, cx| match result {
                Ok(refresh_token) => {
                    this.root
                        .update(cx, |root, cx| {
                            root.update_account_secret(account_id, refresh_token, cx)
                        })
                        .ok();
                }
                Err(failure) => {
                    if let Some(message) = failure.message(true, user_cancelled) {
                        this.account_messages.insert(account_id, message);
                    }
                }
            },
            cx,
        );
    }

    /// Cancel button of a running sign-in: unblocks `authorize`; the finish
    /// handler then resets the UI without reporting an error.
    fn cancel_oauth(&mut self, cx: &mut Context<Self>) {
        if let Some(flow) = self.oauth.as_mut() {
            flow.user_cancelled = true;
            flow.cancel.cancel();
        }
        cx.notify();
    }

    // ----- Per-account actions ------------------------------------------------

    fn begin_password_edit(
        &mut self,
        account_id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let placeholder = Key::AccountNewPassword.tr(cx.language());
        let input = cx.new(|cx| {
            let mut input = TextInput::new(placeholder, cx);
            input.set_masked(true, cx);
            input
        });
        input.read(cx).focus_handle(cx).focus(window);
        self.password_edit = Some((account_id, input));
        cx.notify();
    }

    fn begin_rename(
        &mut self,
        account_id: i64,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| {
            let mut input = TextInput::new(Key::AccountNewName.tr(cx.language()), cx);
            input.set_content(name, cx);
            input
        });
        input.read(cx).focus_handle(cx).focus(window);
        self.rename_edit = Some((account_id, input));
        cx.notify();
    }

    fn save_rename(&mut self, cx: &mut Context<Self>) {
        let Some((account_id, input)) = self.rename_edit.take() else {
            return;
        };
        let name = AccountForm::text(&input, cx);
        if name.trim().is_empty() {
            self.rename_edit = Some((account_id, input));
            return;
        }
        self.root
            .update(cx, |root, cx| root.rename_account(account_id, &name, cx))
            .ok();
        cx.notify();
    }

    fn save_password(&mut self, cx: &mut Context<Self>) {
        let Some((account_id, input)) = self.password_edit.take() else {
            return;
        };
        let secret = AccountForm::text(&input, cx);
        if secret.is_empty() {
            self.password_edit = Some((account_id, input));
            return;
        }
        self.account_messages.remove(&account_id);
        self.root
            .update(cx, |root, cx| {
                root.update_account_secret(account_id, secret, cx)
            })
            .ok();
        cx.notify();
    }

    /// Asks for confirmation (local mail is deleted), then removes the account.
    fn confirm_remove(&mut self, account_id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let language = cx.language();
        let answer = window.prompt(
            PromptLevel::Warning,
            Key::AccountRemoveConfirm.tr(language),
            Some(Key::AccountRemoveDetail.tr(language)),
            &[
                Key::AccountRemove.tr(language),
                Key::AccountCancel.tr(language),
            ],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await.ok() != Some(0) {
                return;
            }
            this.update(cx, |this, cx| {
                if this
                    .password_edit
                    .as_ref()
                    .is_some_and(|(id, _)| *id == account_id)
                {
                    this.password_edit = None;
                }
                this.account_messages.remove(&account_id);
                this.root
                    .update(cx, |root, cx| root.remove_account(account_id, cx))
                    .ok();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // ----- Rendering ----------------------------------------------------------

    /// Asks the system off the UI thread: it means running a command.
    fn refresh_default_app(&mut self, cx: &mut Context<Self>) {
        self.default_app_checked = true;
        let status = cx.background_spawn(async { default_mail_app::status() });
        cx.spawn(async move |this, cx| {
            let status = status.await;
            this.update(cx, |this, cx| {
                this.default_app = Some(status);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn make_default_app(&mut self, cx: &mut Context<Self>) {
        self.default_app_message = None;
        let outcome = cx.background_spawn(async { default_mail_app::set_default() });
        cx.spawn(async move |this, cx| {
            let outcome = outcome.await;
            this.update(cx, |this, cx| {
                this.default_app_message = default_app_message(&outcome, cx.language());
                this.refresh_default_app(cx);
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn render_default_mail_app(
        &mut self,
        language: Language,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        if !self.default_app_checked {
            self.refresh_default_app(cx);
        }
        let status = self.default_app;
        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .justify_between()
                    .gap_4()
                    .py_1()
                    .child(Label::new(Key::DefaultAppLabel.tr(language)).color(Color::Muted))
                    .child(match status {
                        Some(default_mail_app::Status::IsDefault) => {
                            Label::new(Key::DefaultAppIsDefault.tr(language)).into_any_element()
                        }
                        Some(status) => h_flex()
                            .gap_3()
                            .when(status == default_mail_app::Status::NotDefault, |el| {
                                el.child(
                                    Label::new(Key::DefaultAppNotDefault.tr(language))
                                        .color(Color::Muted),
                                )
                            })
                            .child(
                                Button::new("make-default-app", Key::DefaultAppSet.tr(language))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.make_default_app(cx);
                                    })),
                            )
                            .into_any_element(),
                        None => div().into_any_element(),
                    }),
            )
            .children(self.default_app_message.clone().map(|message| {
                Label::new(message)
                    .size(LabelSize::Small)
                    .color(Color::Muted)
            }))
    }

    fn render_nav(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let language = cx.language();

        let mut nav = v_flex()
            .w(px(220.0))
            .flex_shrink_0()
            .h_full()
            .bg(colors.panel_background)
            .border_r_1()
            .border_color(colors.border)
            .p_2()
            .gap_0p5()
            .child(
                div().px_2().py_2().child(
                    Label::new(Key::SettingsTitle.tr(language))
                        .size(LabelSize::Large)
                        .bold(),
                ),
            );

        for (ix, section) in SettingsSection::ALL.into_iter().enumerate() {
            let selected = self.section == section;
            nav = nav.child(
                ListItem::new(("settings-nav", ix))
                    .selected(selected)
                    .start_slot(Icon::new(section.icon()).size(IconSize::Small).color(
                        if selected {
                            Color::Accent
                        } else {
                            Color::Muted
                        },
                    ))
                    .child(Label::new(section.title_key().tr(language)).size(LabelSize::Small))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.section = section;
                        cx.notify();
                    })),
            );
        }

        nav
    }

    fn render_content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let section = self.section;
        let appearance = cx.theme().appearance();
        let language = cx.language();

        let content = match section {
            SettingsSection::General => v_flex()
                .gap_2()
                .child(settings_row(Key::AppNameLabel.tr(language), "BGMail"))
                .child(settings_row(
                    Key::VersionLabel.tr(language),
                    env!("CARGO_PKG_VERSION"),
                ))
                .child(
                    h_flex()
                        .justify_between()
                        .gap_4()
                        .py_1()
                        .child(Label::new(Key::LanguageLabel.tr(language)).color(Color::Muted))
                        .child(self.render_language_picker(language, cx)),
                )
                .child(self.render_default_mail_app(language, cx))
                .into_any_element(),
            SettingsSection::Accounts => self.render_accounts(window, cx).into_any_element(),
            SettingsSection::Appearance => {
                let reader_white = self
                    .root
                    .upgrade()
                    .map(|root| root.read(cx).reader_white_background)
                    .unwrap_or(false);
                let compose_white = self
                    .root
                    .upgrade()
                    .map(|root| root.read(cx).compose_white_background)
                    .unwrap_or(false);
                v_flex()
                    .gap_3()
                    .child(Label::new(Key::ThemeLabel.tr(language)).weight(FontWeight::SEMIBOLD))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("theme-light", Key::ThemeLight.tr(language))
                                    .style(if appearance == Appearance::Light {
                                        ButtonStyle::Filled
                                    } else {
                                        ButtonStyle::Subtle
                                    })
                                    .on_click(cx.listener(|_, _, _, cx| {
                                        theme::set_theme(theme::Theme::light(), cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("theme-dark", Key::ThemeDark.tr(language))
                                    .style(if appearance == Appearance::Dark {
                                        ButtonStyle::Filled
                                    } else {
                                        ButtonStyle::Subtle
                                    })
                                    .on_click(cx.listener(|_, _, _, cx| {
                                        theme::set_theme(theme::Theme::dark(), cx);
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        // Forcing a white reader only makes sense against a dark
                        // theme; in light mode the page is already light, so the
                        // toggle is disabled.
                        Switch::new("reader-white-bg", reader_white)
                            .label(Key::ReaderWhiteBackground.tr(language))
                            .disabled(appearance == Appearance::Light)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let _ = this.root.update(cx, |root, cx| {
                                    root.set_reader_white_background(!reader_white, cx);
                                });
                                cx.notify();
                            })),
                    )
                    .child(
                        Switch::new("compose-white-bg", compose_white)
                            .label(Key::ComposeWhiteBackground.tr(language))
                            .disabled(appearance == Appearance::Light)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let _ = this.root.update(cx, |root, cx| {
                                    root.set_compose_white_background(!compose_white, cx);
                                });
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            }
            SettingsSection::Notifications => {
                let enabled = self
                    .root
                    .upgrade()
                    .map(|root| root.read(cx).notifications_enabled)
                    .unwrap_or(false);
                v_flex()
                    .gap_2()
                    .child(
                        Switch::new("desktop-notifications", enabled)
                            .label(Key::DesktopNotifications.tr(language))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let _ = this.root.update(cx, |root, cx| {
                                    root.set_notifications_enabled(!enabled, cx);
                                });
                                cx.notify();
                            })),
                    )
                    .child(
                        Label::new(Key::NotificationsHint.tr(language))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .into_any_element()
            }
            SettingsSection::Privacy => {
                let always = self
                    .root
                    .upgrade()
                    .map(|root| root.read(cx).load_remote_images)
                    .unwrap_or(false);
                v_flex()
                    .gap_2()
                    .child(
                        Label::new(Key::RemoteImagesLabel.tr(language))
                            .weight(FontWeight::SEMIBOLD),
                    )
                    .child(
                        Label::new(Key::RemoteImagesHint.tr(language))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .child(
                        Switch::new("always-load-remote", always)
                            .label(Key::AlwaysLoadRemoteImages.tr(language))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let _ = this.root.update(cx, |root, cx| {
                                    root.set_load_remote_images(!always, cx);
                                });
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            }
        };

        v_flex()
            .p_6()
            .gap_4()
            .child(
                Label::new(section.title_key().tr(language))
                    .size(LabelSize::Large)
                    .bold(),
            )
            .child(content)
    }

    /// The account list followed by the "Add account" form (or its button).
    fn render_accounts(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let language = cx.language();
        let colors = cx.theme().colors();
        let accounts = self.accounts(cx);
        let has_remote = accounts
            .iter()
            .any(|account| account.provider != storage::PROVIDER_LOCAL);
        let show_form = self.form_open || !has_remote;

        let mut section = v_flex().gap_4();
        if !accounts.is_empty() {
            let count = accounts.len();
            let mut list = v_flex()
                .rounded_lg()
                .border_1()
                .border_color(colors.border)
                .bg(colors.surface_background);
            for (ix, account) in accounts.iter().enumerate() {
                list = list.child(self.render_account_row(account, ix + 1 == count, window, cx));
            }
            section = section.child(list);
        }

        if show_form {
            if self.form.is_none() {
                self.form = Some(AccountForm::new(language, cx));
            }
            section = section.child(self.render_form(has_remote, window, cx));
        } else {
            section = section.child(
                h_flex().child(
                    Button::new("add-account", Key::AddAccount.tr(language))
                        .style(ButtonStyle::Filled)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.form_open = true;
                            cx.notify();
                        })),
                ),
            );
        }
        section
    }

    fn render_account_row(
        &self,
        account: &storage::Account,
        last: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let language = cx.language();
        let colors = cx.theme().colors();
        let id = account.id;
        let health = self.health(account, cx);
        let uses_oauth =
            AccountConfig::from_json(&account.config_json).is_some_and(|c| c.uses_oauth());
        let is_local = account.provider == storage::PROVIDER_LOCAL;
        let badge = ProviderBadge::for_provider(&account.provider).label(language);
        let status_color = if health.is_problem() {
            Color::Error
        } else if health == AccountHealth::Offline {
            Color::Warning
        } else if health == AccountHealth::UpToDate {
            Color::Success
        } else {
            Color::Muted
        };
        let signing_in = self
            .oauth
            .as_ref()
            .is_some_and(|flow| flow.target == OAuthTarget::Account(id));

        let mut actions = h_flex().gap_1().flex_shrink_0();
        let current_name = account.name.clone();
        actions = actions.child(
            Button::new(
                ("account-rename", id as usize),
                Key::AccountRename.tr(language),
            )
            .style(ButtonStyle::Subtle)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.begin_rename(id, &current_name, window, cx)
            })),
        );
        if signing_in {
            actions = actions.child(
                Button::new(
                    ("account-cancel-oauth", id as usize),
                    Key::AccountCancel.tr(language),
                )
                .style(ButtonStyle::Subtle)
                .on_click(cx.listener(|this, _, _, cx| this.cancel_oauth(cx))),
            );
        } else if uses_oauth {
            actions = actions.child(
                Button::new(
                    ("account-reauth", id as usize),
                    Key::AccountSignInAgain.tr(language),
                )
                .style(ButtonStyle::Subtle)
                .on_click(cx.listener(move |this, _, _, cx| this.sign_in_again(id, cx))),
            );
        } else if !is_local {
            actions =
                actions.child(
                    Button::new(
                        ("account-password", id as usize),
                        Key::AccountUpdatePassword.tr(language),
                    )
                    .style(ButtonStyle::Subtle)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.begin_password_edit(id, window, cx)
                    })),
                );
        }
        actions = actions.child(
            Button::new(
                ("account-remove", id as usize),
                Key::AccountRemove.tr(language),
            )
            .style(ButtonStyle::Ghost)
            .on_click(cx.listener(move |this, _, window, cx| this.confirm_remove(id, window, cx))),
        );

        let status_text: SharedString = if signing_in {
            Key::AccountWaitingForBrowser.tr(language).into()
        } else {
            health.key().tr(language).into()
        };

        let mut row = v_flex()
            .gap_2()
            .px_4()
            .py_3()
            .when(!last, |row| {
                row.border_b_1().border_color(colors.border_variant)
            })
            .child(
                h_flex()
                    .gap_3()
                    .justify_between()
                    .child(
                        v_flex()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        Label::new(account.name.clone()).semibold().single_line(),
                                    )
                                    .child(badge_element(badge, cx)),
                            )
                            .child(
                                Label::new(account.email.clone())
                                    .size(LabelSize::Small)
                                    .color(Color::Muted)
                                    .single_line(),
                            )
                            .child(
                                Label::new(status_text)
                                    .size(LabelSize::XSmall)
                                    .color(status_color),
                            ),
                    )
                    .child(actions),
            );

        if let Some((_, input)) = self.rename_edit.as_ref().filter(|(edit, _)| *edit == id) {
            row = row.child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().child(input_box(input, window, cx)))
                    .child(
                        Button::new(
                            ("account-rename-save", id as usize),
                            Key::AccountSave.tr(language),
                        )
                        .style(ButtonStyle::Filled)
                        .on_click(cx.listener(|this, _, _, cx| this.save_rename(cx))),
                    )
                    .child(
                        Button::new(
                            ("account-rename-cancel", id as usize),
                            Key::AccountCancel.tr(language),
                        )
                        .style(ButtonStyle::Subtle)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.rename_edit = None;
                            cx.notify();
                        })),
                    ),
            );
        }
        if let Some((_, input)) = self.password_edit.as_ref().filter(|(edit, _)| *edit == id) {
            row = row.child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().child(input_box(input, window, cx)))
                    .child(
                        Button::new(
                            ("account-password-save", id as usize),
                            Key::AccountSave.tr(language),
                        )
                        .style(ButtonStyle::Filled)
                        .on_click(cx.listener(|this, _, _, cx| this.save_password(cx))),
                    )
                    .child(
                        Button::new(
                            ("account-password-cancel", id as usize),
                            Key::AccountCancel.tr(language),
                        )
                        .style(ButtonStyle::Subtle)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.password_edit = None;
                            cx.notify();
                        })),
                    ),
            );
        }
        if let Some(message) = self.account_messages.get(&id) {
            row = row.child(message_element(message, language));
        }
        row
    }

    fn render_form(
        &self,
        has_remote: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let language = cx.language();
        let colors = cx.theme().colors();
        let form = self
            .form
            .as_ref()
            .expect("form is created before rendering");
        let email = form.email(cx);
        let is_google = account_setup::is_google_address(&email);
        let google_sign_in = form.uses_google_sign_in(&email);
        let signing_in = self
            .oauth
            .as_ref()
            .is_some_and(|flow| flow.target == OAuthTarget::NewAccount);
        let busy = form.checking || signing_in;

        let mut card = v_flex()
            .gap_3()
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(colors.border)
            .bg(colors.surface_background)
            .child(Label::new(Key::AccountAddTitle.tr(language)).weight(FontWeight::SEMIBOLD))
            .child(labeled_field(
                Key::AccountFieldName.tr(language),
                &form.name,
                window,
                cx,
            ))
            .child(labeled_field(
                Key::AccountFieldEmail.tr(language),
                &form.email,
                window,
                cx,
            ));

        if let Some(note) = account_setup::preset_note(&email) {
            card = card.child(note_element(account_setup::note_key(note).tr(language)));
        }

        if google_sign_in {
            card = card.child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("google-sign-in", Key::AccountSignInWithGoogle.tr(language))
                            .icon(IconName::Account)
                            .style(ButtonStyle::Filled)
                            .when(!busy, |button| {
                                button.on_click(
                                    cx.listener(|this, _, _, cx| this.sign_in_with_google(cx)),
                                )
                            }),
                    )
                    .when(!busy, |row| {
                        row.child(
                            Button::new(
                                "use-app-password",
                                Key::AccountUseAppPassword.tr(language),
                            )
                            .style(ButtonStyle::Ghost)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(form) = this.form.as_mut() {
                                    form.gmail_app_password = true;
                                    form.message = None;
                                }
                                cx.notify();
                            })),
                        )
                    }),
            );
        } else {
            if is_google && form.google_client.is_none() {
                card = card.child(note_element(Key::AccountGoogleNotConfigured.tr(language)));
            }
            card = card.child(labeled_field(
                Key::AccountFieldPassword.tr(language),
                &form.password,
                window,
                cx,
            ));
            if is_google && form.google_client.is_some() && !busy {
                card = card.child(
                    h_flex().child(
                        Button::new("use-google", Key::AccountUseGoogleSignIn.tr(language))
                            .style(ButtonStyle::Ghost)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(form) = this.form.as_mut() {
                                    form.gmail_app_password = false;
                                    form.message = None;
                                }
                                cx.notify();
                            })),
                    ),
                );
            }
        }

        // Manual settings disclosure (Google sign-in uses the fixed Gmail servers).
        if !google_sign_in {
            card = card.child(
                h_flex()
                    .id("manual-settings")
                    .gap_1()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(form) = this.form.as_mut() {
                            form.show_manual = !form.show_manual;
                        }
                        cx.notify();
                    }))
                    .child(
                        Icon::new(if form.show_manual {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .size(IconSize::XSmall)
                        .color(Color::Muted),
                    )
                    .child(
                        Label::new(Key::AccountManualSettings.tr(language))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .when(form.looking_up, |row| {
                        row.child(
                            Label::new(Key::AccountLookingUp.tr(language))
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                        )
                    }),
            );
            if form.show_manual {
                card = card.child(self.render_manual(form, window, cx));
            }
        }

        if let Some(message) = &form.message {
            card = card.child(message_element(message, language));
        }

        let mut footer = h_flex().gap_2().justify_end();
        if signing_in {
            footer = footer
                .child(
                    div().flex_1().child(
                        Label::new(Key::AccountWaitingForBrowser.tr(language))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    ),
                )
                .child(
                    Button::new("cancel-sign-in", Key::AccountCancel.tr(language))
                        .style(ButtonStyle::Subtle)
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_oauth(cx))),
                );
        } else {
            if has_remote && !busy {
                footer = footer.child(
                    Button::new("cancel-add-account", Key::AccountCancel.tr(language))
                        .style(ButtonStyle::Subtle)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.form = None;
                            this.form_open = false;
                            cx.notify();
                        })),
                );
            }
            if !google_sign_in {
                let label = if form.checking {
                    Key::AccountChecking
                } else {
                    Key::AccountConnect
                };
                footer = footer.child(
                    Button::new("connect-account", label.tr(language))
                        .style(if busy {
                            ButtonStyle::Subtle
                        } else {
                            ButtonStyle::Filled
                        })
                        .when(!busy, |button| {
                            button.on_click(cx.listener(|this, _, _, cx| this.connect(cx)))
                        }),
                );
            } else if form.checking {
                footer = footer.child(
                    Label::new(Key::AccountChecking.tr(language))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                );
            }
        }
        card.child(footer)
    }

    /// IMAP / SMTP host, port and security, plus the login name.
    fn render_manual(
        &self,
        form: &AccountForm,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let language = cx.language();
        let server = |title: Key,
                      host: &Entity<TextInput>,
                      port: &Entity<TextInput>,
                      security: Security,
                      imap: bool,
                      window: &mut Window,
                      cx: &mut Context<Self>| {
            v_flex()
                .gap_2()
                .child(
                    Label::new(title.tr(language))
                        .size(LabelSize::Small)
                        .weight(FontWeight::SEMIBOLD),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .items_end()
                        .child(div().flex_1().child(labeled_field(
                            Key::AccountFieldHost.tr(language),
                            host,
                            window,
                            cx,
                        )))
                        .child(div().w(px(96.0)).child(labeled_field(
                            Key::AccountFieldPort.tr(language),
                            port,
                            window,
                            cx,
                        ))),
                )
                .child(security_picker(imap, security, language, cx))
        };
        v_flex()
            .gap_3()
            .pl_4()
            .child(server(
                Key::AccountImapServer,
                &form.imap_host,
                &form.imap_port,
                form.imap_security,
                true,
                window,
                cx,
            ))
            .child(server(
                Key::AccountSmtpServer,
                &form.smtp_host,
                &form.smtp_port,
                form.smtp_security,
                false,
                window,
                cx,
            ))
            .child(labeled_field(
                Key::AccountFieldUsername.tr(language),
                &form.username,
                window,
                cx,
            ))
    }

    /// Language selector used in the General section. Switching the language
    /// re-renders the whole app through the locale global.
    fn render_language_picker(
        &self,
        language: Language,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut row = h_flex().gap_2();
        for option in Language::ALL {
            let selected = option == language;
            row = row.child(
                Button::new(("language", option as usize), option.label())
                    .style(if selected {
                        ButtonStyle::Filled
                    } else {
                        ButtonStyle::Subtle
                    })
                    .on_click(cx.listener(move |_, _, _, cx| {
                        locale::set_language(option, cx);
                        cx.notify();
                    })),
            );
        }
        row
    }
}

impl Drop for SettingsView {
    /// Closing the window must not leave a sign-in waiting on its loopback
    /// port for minutes.
    fn drop(&mut self) {
        if let Some(flow) = &self.oauth {
            flow.cancel.cancel();
        }
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.root_observation.is_none() {
            if let Some(root) = self.root.upgrade() {
                self.root_observation = Some(cx.observe(&root, |_, _, cx| cx.notify()));
            }
        }
        let colors = cx.theme().colors();

        h_flex()
            .size_full()
            .bg(colors.background)
            .text_color(colors.text)
            .font_family("Helvetica")
            .on_action(cx.listener(|this, _: &ComposeClose, window, cx| {
                this.close_window(window, cx);
            }))
            .child(self.render_nav(cx))
            .child(
                div()
                    .id("settings-content")
                    .flex_1()
                    .h_full()
                    .overflow_y_scroll()
                    .child(self.render_content(window, cx)),
            )
    }
}

/// A "label → value" row used in the settings.
fn settings_row(
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
) -> impl IntoElement {
    h_flex()
        .justify_between()
        .gap_4()
        .py_1()
        .child(Label::new(label.into()).color(Color::Muted))
        .child(Label::new(value.into()))
}

/// What to tell the user after asking to become the default mail app.
fn default_app_message(
    outcome: &Result<default_mail_app::Outcome, default_mail_app::Error>,
    language: Language,
) -> Option<SharedString> {
    match outcome {
        Ok(default_mail_app::Outcome::Done) => None,
        Ok(default_mail_app::Outcome::FinishInSystemSettings) => {
            Some(Key::DefaultAppFinishInSettings.tr(language).into())
        }
        Err(default_mail_app::Error::NotInstalled) => {
            Some(Key::DefaultAppNotInstalled.tr(language).into())
        }
        Err(default_mail_app::Error::Failed(detail)) => {
            Some(format!("{}: {detail}", Key::DefaultAppFailed.tr(language)).into())
        }
    }
}

/// Bordered box around a text field; the border follows focus.
fn input_box(input: &Entity<TextInput>, window: &Window, cx: &App) -> impl IntoElement {
    let colors = cx.theme().colors();
    let focused = input.read(cx).focus_handle(cx).is_focused(window);
    h_flex()
        .px_2()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(if focused {
            colors.border_focused
        } else {
            colors.border
        })
        .bg(colors.background)
        .text_size(ui::ui_text_size(-1.0))
        .child(input.clone())
}

/// A small caption above a text field.
fn labeled_field(
    label: &'static str,
    input: &Entity<TextInput>,
    window: &Window,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .gap_1()
        .child(Label::new(label).size(LabelSize::Small).color(Color::Muted))
        .child(input_box(input, window, cx))
}

/// The TLS / STARTTLS / None segmented control of one server.
fn security_picker(
    imap: bool,
    selected: Security,
    language: Language,
    cx: &mut Context<SettingsView>,
) -> impl IntoElement {
    let id = if imap {
        "imap-security"
    } else {
        "smtp-security"
    };
    let mut row = h_flex().gap_1();
    for (ix, option) in SECURITY_OPTIONS.into_iter().enumerate() {
        row = row.child(
            Button::new((id, ix), account_setup::security_label(option, language))
                .style(if option == selected {
                    ButtonStyle::Filled
                } else {
                    ButtonStyle::Subtle
                })
                .on_click(cx.listener(move |this, _, _, cx| this.set_security(imap, option, cx))),
        );
    }
    row
}

/// Provider badge next to an account name.
fn badge_element(label: &'static str, cx: &App) -> impl IntoElement {
    let colors = cx.theme().colors();
    div()
        .px_1p5()
        .rounded_sm()
        .border_1()
        .border_color(colors.border)
        .bg(colors.element_background)
        .child(
            Label::new(label)
                .size(LabelSize::XSmall)
                .color(Color::Muted),
        )
}

/// An informational note (provider requirements, Google sign-in setup).
fn note_element(text: &'static str) -> impl IntoElement {
    h_flex()
        .gap_2()
        .items_start()
        .child(
            Icon::new(IconName::Shield)
                .size(IconSize::XSmall)
                .color(Color::Warning),
        )
        .child(
            div()
                .flex_1()
                .child(Label::new(text).size(LabelSize::Small).color(Color::Muted)),
        )
}

/// A failure message with the optional raw server detail below it.
fn message_element(message: &FailureMessage, language: Language) -> impl IntoElement {
    v_flex()
        .gap_0p5()
        .child(
            Label::new(message.key.tr(language))
                .size(LabelSize::Small)
                .color(Color::Error),
        )
        .when_some(message.detail.clone(), |el, detail| {
            el.child(
                Label::new(detail)
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_app_outcomes_tell_the_user_what_to_do() {
        use default_mail_app::{Error, Outcome};
        let language = Language::Portuguese;
        assert_eq!(default_app_message(&Ok(Outcome::Done), language), None);
        assert_eq!(
            default_app_message(&Ok(Outcome::FinishInSystemSettings), language),
            Some(Key::DefaultAppFinishInSettings.tr(language).into())
        );
        assert_eq!(
            default_app_message(&Err(Error::NotInstalled), language),
            Some(Key::DefaultAppNotInstalled.tr(language).into())
        );
        let failed = default_app_message(&Err(Error::Failed("boom".into())), language).unwrap();
        assert!(failed.ends_with(": boom"));
    }
}
