//! Lightweight localization (i18n) for the rMail UI.
//!
//! The active [`Language`] is stored as a [`gpui::Global`] and read through the
//! [`ActiveLanguage`] trait, mirroring how the theme is handled. UI strings are
//! resolved at render time, so switching the language updates the whole UI live.
//!
//! Only UI chrome is translated here (standard mailbox names, settings labels,
//! buttons, status bar). Message/account contents are sample data and stay as
//! provided, since real e-mail arrives in whatever language the sender used.

use gpui::{App, Global};

/// Languages supported by the UI. English is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    /// English (default).
    #[default]
    English,
    /// Brazilian Portuguese.
    Portuguese,
}

impl Language {
    /// All languages, in the order they should appear in a selector.
    pub const ALL: [Language; 2] = [Language::English, Language::Portuguese];

    /// Endonym shown in the UI (a language is always named in itself).
    pub fn label(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Portuguese => "Português (Brasil)",
        }
    }
}

/// Translatable UI strings. Each key maps to one string per [`Language`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    MailboxInbox,
    MailboxDrafts,
    MailboxSent,
    MailboxJunk,
    MailboxTrash,
    MailboxArchive,

    SettingsGeneral,
    SettingsAccounts,
    SettingsAppearance,
    SettingsNotifications,

    SettingsTitle,
    ThemeLabel,
    ThemeLight,
    ThemeDark,

    AppNameLabel,
    VersionLabel,
    LanguageLabel,
    AddAccount,

    DesktopNotifications,
    SoundOnNewEmail,
    Enabled,
    Disabled,
}

impl Key {
    /// Resolves this key to its text in the given language.
    pub fn tr(self, language: Language) -> &'static str {
        use Key::*;
        use Language::{English as E, Portuguese as P};
        match (self, language) {
            (MailboxInbox, E) => "Inbox",
            (MailboxInbox, P) => "Caixa de entrada",
            (MailboxDrafts, E) => "Drafts",
            (MailboxDrafts, P) => "Rascunhos",
            (MailboxSent, E) => "Sent",
            (MailboxSent, P) => "Enviados",
            (MailboxJunk, E) => "Junk",
            (MailboxJunk, P) => "Spam",
            (MailboxTrash, E) => "Trash",
            (MailboxTrash, P) => "Lixeira",
            (MailboxArchive, E) => "Archive",
            (MailboxArchive, P) => "Arquivo",

            (SettingsGeneral, E) => "General",
            (SettingsGeneral, P) => "Geral",
            (SettingsAccounts, E) => "Accounts",
            (SettingsAccounts, P) => "Contas",
            (SettingsAppearance, E) => "Appearance",
            (SettingsAppearance, P) => "Aparência",
            (SettingsNotifications, E) => "Notifications",
            (SettingsNotifications, P) => "Notificações",

            (SettingsTitle, E) => "Settings",
            (SettingsTitle, P) => "Configurações",
            (ThemeLabel, E) => "Theme",
            (ThemeLabel, P) => "Tema",
            (ThemeLight, E) => "Light",
            (ThemeLight, P) => "Claro",
            (ThemeDark, E) => "Dark",
            (ThemeDark, P) => "Escuro",

            (AppNameLabel, E) => "App name",
            (AppNameLabel, P) => "Nome do app",
            (VersionLabel, E) => "Version",
            (VersionLabel, P) => "Versão",
            (LanguageLabel, E) => "Language",
            (LanguageLabel, P) => "Idioma",
            (AddAccount, E) => "Add account…",
            (AddAccount, P) => "Adicionar conta…",

            (DesktopNotifications, E) => "Desktop notifications",
            (DesktopNotifications, P) => "Notificações na área de trabalho",
            (SoundOnNewEmail, E) => "Sound on new email",
            (SoundOnNewEmail, P) => "Som ao receber e-mail",
            (Enabled, E) => "Enabled",
            (Enabled, P) => "Ativadas",
            (Disabled, E) => "Disabled",
            (Disabled, P) => "Desativado",
        }
    }
}

/// Status bar (left): account and message counts.
pub fn status_counts(language: Language, accounts: usize, messages: usize) -> String {
    match language {
        Language::English => format!("{accounts} accounts · {messages} messages"),
        Language::Portuguese => format!("{accounts} contas · {messages} mensagens"),
    }
}

/// Status bar (right): unread count plus a "synced" hint.
pub fn status_unread(language: Language, unread: usize) -> String {
    match language {
        Language::English => format!("{unread} unread · Updated just now"),
        Language::Portuguese => format!("{unread} não lidas · Atualizado agora"),
    }
}

/// Global holding the active UI language.
struct GlobalLanguage(Language);

impl Global for GlobalLanguage {}

/// Initializes the localization system with the given language.
pub fn init(language: Language, cx: &mut App) {
    cx.set_global(GlobalLanguage(language));
}

/// Replaces the active language (updates the UI on the next render).
pub fn set_language(language: Language, cx: &mut App) {
    cx.set_global(GlobalLanguage(language));
}

/// Reads the active language from a context.
pub trait ActiveLanguage {
    /// Returns the active UI language.
    fn language(&self) -> Language;
}

impl ActiveLanguage for App {
    fn language(&self) -> Language {
        self.global::<GlobalLanguage>().0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_KEYS: [Key; 22] = [
        Key::MailboxInbox,
        Key::MailboxDrafts,
        Key::MailboxSent,
        Key::MailboxJunk,
        Key::MailboxTrash,
        Key::MailboxArchive,
        Key::SettingsGeneral,
        Key::SettingsAccounts,
        Key::SettingsAppearance,
        Key::SettingsNotifications,
        Key::SettingsTitle,
        Key::ThemeLabel,
        Key::ThemeLight,
        Key::ThemeDark,
        Key::AppNameLabel,
        Key::VersionLabel,
        Key::LanguageLabel,
        Key::AddAccount,
        Key::DesktopNotifications,
        Key::SoundOnNewEmail,
        Key::Enabled,
        Key::Disabled,
    ];

    #[test]
    fn every_key_has_text_in_every_language() {
        for key in ALL_KEYS {
            for language in Language::ALL {
                assert!(
                    !key.tr(language).is_empty(),
                    "{key:?} is empty in {language:?}"
                );
            }
        }
    }

    #[test]
    fn translations_differ_between_languages() {
        assert_eq!(Key::MailboxInbox.tr(Language::English), "Inbox");
        assert_eq!(
            Key::MailboxInbox.tr(Language::Portuguese),
            "Caixa de entrada"
        );
        assert_ne!(
            Key::SettingsTitle.tr(Language::English),
            Key::SettingsTitle.tr(Language::Portuguese)
        );
    }

    #[test]
    fn default_language_is_english() {
        assert_eq!(Language::default(), Language::English);
    }

    #[test]
    fn formatted_strings_include_arguments() {
        let en = status_counts(Language::English, 2, 8);
        assert!(en.contains('2') && en.contains('8') && en.contains("accounts"));
        let pt = status_unread(Language::Portuguese, 9);
        assert!(pt.contains('9') && pt.contains("não lidas"));
    }
}
