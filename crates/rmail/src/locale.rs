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
    MailboxFlagged,

    SettingsGeneral,
    SettingsAccounts,
    SettingsAppearance,
    SettingsNotifications,
    SettingsPrivacy,

    SettingsTitle,
    ThemeLabel,
    ThemeLight,
    ThemeDark,
    ReaderWhiteBackground,
    ComposeWhiteBackground,

    AppNameLabel,
    VersionLabel,
    LanguageLabel,
    AddAccount,

    DesktopNotifications,
    SoundOnNewEmail,
    Enabled,
    Disabled,

    RemoteImagesLabel,
    RemoteImagesHint,
    BlockedElements,
    UnblockRemote,
    RemoteContentLoaded,

    SearchPlaceholder,
    SearchClear,
    SearchActiveTitle,
    SearchNoResults,
    ReaderNoSelection,

    ToolbarToggleSidebar,
    ToolbarReply,
    ToolbarReplyAll,
    ToolbarForward,
    ToolbarFlag,
    ToolbarMove,
    ToolbarFilter,
    ToolbarMore,

    CommandPalette,
    CommandDelete,
    CommandDeletePermanent,
    CommandRestore,
    CommandArchive,
    CommandMarkJunk,
    CommandFlag,
    CommandUnflag,
    CommandMoveTo,

    ComposeWindowTitle,
    ComposeSend,
    ComposeAttach,
    ComposeDiscard,
    ComposeFrom,
    ComposeTo,
    ComposeCc,
    ComposeBcc,
    ComposeSubject,
    ComposeCcBcc,
    ComposeFieldPlaceholder,
    ComposeBodyPlaceholder,

    CtxOpenImage,
    CtxDownloadImage,
    CtxShowImage,
    CtxOpenLink,
    CtxCopyLink,
    CtxCopy,
    ImageDownloaded,
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
            (MailboxFlagged, E) => "Flagged",
            (MailboxFlagged, P) => "Sinalizadas",

            (SettingsGeneral, E) => "General",
            (SettingsGeneral, P) => "Geral",
            (SettingsAccounts, E) => "Accounts",
            (SettingsAccounts, P) => "Contas",
            (SettingsAppearance, E) => "Appearance",
            (SettingsAppearance, P) => "Aparência",
            (SettingsNotifications, E) => "Notifications",
            (SettingsNotifications, P) => "Notificações",
            (SettingsPrivacy, E) => "Privacy",
            (SettingsPrivacy, P) => "Privacidade",

            (SettingsTitle, E) => "Settings",
            (SettingsTitle, P) => "Configurações",
            (ThemeLabel, E) => "Theme",
            (ThemeLabel, P) => "Tema",
            (ThemeLight, E) => "Light",
            (ThemeLight, P) => "Claro",
            (ThemeDark, E) => "Dark",
            (ThemeDark, P) => "Escuro",
            (ReaderWhiteBackground, E) => "Keep the email reader on a white background",
            (ReaderWhiteBackground, P) => "Manter o leitor de e-mail com fundo branco",
            (ComposeWhiteBackground, E) => "Keep the compose message area on a white background",
            (ComposeWhiteBackground, P) => "Manter a área de composição com fundo branco",

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

            (RemoteImagesLabel, E) => "Load remote images",
            (RemoteImagesLabel, P) => "Carregar imagens remotas",
            (RemoteImagesHint, E) => {
                "Off by default. Remote images can track when and where you open a message."
            }
            (RemoteImagesHint, P) => {
                "Desativado por padrão. Imagens remotas podem rastrear quando e onde você abre uma mensagem."
            }
            (BlockedElements, E) => "Blocked images: {}",
            (BlockedElements, P) => "Imagens bloqueadas: {}",
            (UnblockRemote, E) => "Unblock all remote content",
            (UnblockRemote, P) => "Desbloquear todo conteúdo remoto",
            (RemoteContentLoaded, E) => "Remote content loaded",
            (RemoteContentLoaded, P) => "Conteúdo remoto carregado",

            (SearchPlaceholder, E) => "Search",
            (SearchPlaceholder, P) => "Buscar",
            (SearchClear, E) => "Clear search",
            (SearchClear, P) => "Limpar busca",
            (SearchActiveTitle, E) => "Searching",
            (SearchActiveTitle, P) => "Buscando",
            (SearchNoResults, E) => "No messages match your search",
            (SearchNoResults, P) => "Nenhuma mensagem corresponde à busca",
            (ReaderNoSelection, E) => "No Message Selected",
            (ReaderNoSelection, P) => "Nenhuma Mensagem Selecionada",

            (ToolbarToggleSidebar, E) => "Show or hide sidebar",
            (ToolbarToggleSidebar, P) => "Mostrar ou ocultar barra lateral",
            (ToolbarReply, E) => "Reply",
            (ToolbarReply, P) => "Responder",
            (ToolbarReplyAll, E) => "Reply all",
            (ToolbarReplyAll, P) => "Responder a todos",
            (ToolbarForward, E) => "Forward",
            (ToolbarForward, P) => "Encaminhar",
            (ToolbarFlag, E) => "Flag message",
            (ToolbarFlag, P) => "Sinalizar mensagem",
            (ToolbarMove, E) => "Move to folder",
            (ToolbarMove, P) => "Mover para pasta",
            (ToolbarFilter, E) => "Filter messages",
            (ToolbarFilter, P) => "Filtrar mensagens",
            (ToolbarMore, E) => "More actions",
            (ToolbarMore, P) => "Mais ações",

            (CommandPalette, E) => "Command Palette…",
            (CommandPalette, P) => "Paleta de comandos…",
            (CommandDelete, E) => "Move to Trash",
            (CommandDelete, P) => "Mover para a Lixeira",
            (CommandDeletePermanent, E) => "Delete Permanently",
            (CommandDeletePermanent, P) => "Excluir permanentemente",
            (CommandRestore, E) => "Restore from Trash",
            (CommandRestore, P) => "Restaurar da Lixeira",
            (CommandArchive, E) => "Archive",
            (CommandArchive, P) => "Arquivar",
            (CommandMarkJunk, E) => "Mark as Junk",
            (CommandMarkJunk, P) => "Marcar como spam",
            (CommandFlag, E) => "Flag",
            (CommandFlag, P) => "Sinalizar",
            (CommandUnflag, E) => "Unflag",
            (CommandUnflag, P) => "Remover sinalização",
            (CommandMoveTo, E) => "Move To",
            (CommandMoveTo, P) => "Mover para",

            (ComposeWindowTitle, E) => "New Message",
            (ComposeWindowTitle, P) => "Nova mensagem",
            (ComposeSend, E) => "Send",
            (ComposeSend, P) => "Enviar",
            (ComposeAttach, E) => "Attach file",
            (ComposeAttach, P) => "Anexar arquivo",
            (ComposeDiscard, E) => "Discard draft",
            (ComposeDiscard, P) => "Descartar rascunho",
            (ComposeFrom, E) => "From:",
            (ComposeFrom, P) => "De:",
            (ComposeTo, E) => "To:",
            (ComposeTo, P) => "Para:",
            (ComposeCc, E) => "Cc:",
            (ComposeCc, P) => "Cc:",
            (ComposeBcc, E) => "Bcc:",
            (ComposeBcc, P) => "Cco:",
            (ComposeSubject, E) => "Subject:",
            (ComposeSubject, P) => "Assunto:",
            (ComposeCcBcc, E) => "Cc/Bcc",
            (ComposeCcBcc, P) => "Cc/Cco",
            (ComposeFieldPlaceholder, E) => "Type here…",
            (ComposeFieldPlaceholder, P) => "Digite aqui…",
            (ComposeBodyPlaceholder, E) => "Write your message…",
            (ComposeBodyPlaceholder, P) => "Escreva sua mensagem…",

            (CtxOpenImage, E) => "Open image",
            (CtxOpenImage, P) => "Abrir imagem",
            (CtxDownloadImage, E) => "Download image",
            (CtxDownloadImage, P) => "Baixar imagem",
            (CtxShowImage, E) => "Show remote image",
            (CtxShowImage, P) => "Mostrar imagem remota",
            (CtxOpenLink, E) => "Open in browser",
            (CtxOpenLink, P) => "Abrir no navegador",
            (CtxCopyLink, E) => "Copy link",
            (CtxCopyLink, P) => "Copiar link",
            (CtxCopy, E) => "Copy",
            (CtxCopy, P) => "Copiar",
            (ImageDownloaded, E) => "Image saved to your Downloads folder",
            (ImageDownloaded, P) => "Imagem salva na pasta Downloads",
        }
    }
}

/// Message list header: number of messages in the current mailbox.
pub fn message_count(language: Language, count: usize) -> String {
    match language {
        Language::English => format!("{count} messages"),
        Language::Portuguese => format!("{count} mensagens"),
    }
}

/// Status bar (left): account and message counts.
pub fn status_counts(language: Language, accounts: usize, messages: usize) -> String {
    match language {
        Language::English => format!("{accounts} accounts · {messages} messages"),
        Language::Portuguese => format!("{accounts} contas · {messages} mensagens"),
    }
}

/// Status bar (left) while a search filter is active.
pub fn status_search_counts(
    language: Language,
    accounts: usize,
    showing: usize,
    total: usize,
) -> String {
    match language {
        Language::English => format!("{accounts} accounts · {showing} of {total} messages"),
        Language::Portuguese => format!("{accounts} contas · {showing} de {total} mensagens"),
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
    // Like the theme, the language is a global read at render time; redraw every
    // open window so the change applies live everywhere, not just here.
    cx.refresh_windows();
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

    const ALL_KEYS: [Key; 63] = [
        Key::MailboxInbox,
        Key::MailboxDrafts,
        Key::MailboxSent,
        Key::MailboxJunk,
        Key::MailboxTrash,
        Key::MailboxArchive,
        Key::MailboxFlagged,
        Key::SettingsGeneral,
        Key::SettingsAccounts,
        Key::SettingsAppearance,
        Key::SettingsNotifications,
        Key::SettingsPrivacy,
        Key::SettingsTitle,
        Key::ThemeLabel,
        Key::ThemeLight,
        Key::ThemeDark,
        Key::ReaderWhiteBackground,
        Key::ComposeWhiteBackground,
        Key::AppNameLabel,
        Key::VersionLabel,
        Key::LanguageLabel,
        Key::AddAccount,
        Key::DesktopNotifications,
        Key::SoundOnNewEmail,
        Key::Enabled,
        Key::Disabled,
        Key::RemoteImagesLabel,
        Key::RemoteImagesHint,
        Key::BlockedElements,
        Key::UnblockRemote,
        Key::RemoteContentLoaded,
        Key::SearchPlaceholder,
        Key::SearchClear,
        Key::SearchActiveTitle,
        Key::SearchNoResults,
        Key::ReaderNoSelection,
        Key::ToolbarToggleSidebar,
        Key::ToolbarReply,
        Key::ToolbarReplyAll,
        Key::ToolbarForward,
        Key::ToolbarFlag,
        Key::ToolbarMove,
        Key::ToolbarFilter,
        Key::ToolbarMore,
        Key::ComposeWindowTitle,
        Key::ComposeSend,
        Key::ComposeAttach,
        Key::ComposeDiscard,
        Key::ComposeFrom,
        Key::ComposeTo,
        Key::ComposeCc,
        Key::ComposeBcc,
        Key::ComposeSubject,
        Key::ComposeCcBcc,
        Key::ComposeFieldPlaceholder,
        Key::ComposeBodyPlaceholder,
        Key::CtxOpenImage,
        Key::CtxDownloadImage,
        Key::CtxShowImage,
        Key::CtxOpenLink,
        Key::CtxCopyLink,
        Key::CtxCopy,
        Key::ImageDownloaded,
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
