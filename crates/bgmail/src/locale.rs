//! Lightweight localization (i18n) for the BGMail UI.
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

    /// How the language is saved in the config.
    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Portuguese => "pt-BR",
        }
    }

    /// The language saved as `code`; the default for anything else, such as a language
    /// a newer version added.
    pub fn from_code(code: &str) -> Self {
        Language::ALL
            .into_iter()
            .find(|language| language.code() == code)
            .unwrap_or_default()
    }

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
    ViewZoomIn,
    ViewZoomOut,
    ViewZoomReset,
    OnboardingWelcomeTitle,
    OnboardingWelcomeBody,
    OnboardingFastTitle,
    OnboardingFastBody,
    OnboardingAccountsTitle,
    OnboardingAccountsBody,
    OnboardingPrivateTitle,
    OnboardingPrivateBody,
    OnboardingGetStarted,
    OnboardingThemeTitle,
    OnboardingThemeBody,
    OnboardingAccountTitle,
    OnboardingAccountBody,
    OnboardingAccountConnected,
    OnboardingSkip,
    OnboardingDoneTitle,
    OnboardingDoneBodyAccount,
    OnboardingDoneBodyNoAccount,
    OnboardingDefaultApp,
    OnboardingDefaultAppHint,
    OnboardingFinish,
    OnboardingBack,
    OnboardingContinue,

    DesktopNotifications,

    RemoteImagesLabel,
    RemoteImagesHint,
    AlwaysLoadRemoteImages,
    BlockedElements,

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
    FilterAll,
    FilterUnread,
    FilterStarred,
    FilterAttachments,
    FilterNoResults,
    ToolbarMore,

    CommandPalette,
    CommandDelete,
    CommandDeletePermanent,
    CommandRestore,
    CommandArchive,
    CommandMarkJunk,
    CommandFlag,
    CommandUnflag,
    CommandPrint,
    PrintFrom,
    PrintDate,
    CommandMoveTo,

    ComposeWindowTitle,
    ComposeSend,
    ComposeAttach,
    ComposeDiscard,
    ComposeClose,
    ComposeFrom,
    ComposeTo,
    ComposeCc,
    ComposeBcc,
    ComposeSubject,
    ComposeCcBcc,
    ComposeFieldPlaceholder,
    ComposeBodyPlaceholder,
    // Reader: context menu and find bar.
    CtxCopy,
    CtxSelectAll,
    CtxOpenLink,
    CtxCopyLink,
    FindPlaceholder,
    FindMatches,
    FindNoMatches,
    // Reader: plain-text fallback notices.
    NoticeTooDeep,
    NoticeCrashed,
    StatusBodyFailed,
    StatusForwardNeedsBody,
    StatusMessageSent,
    StatusSendFailed,
    StatusSentCopyFailed,
    ComposeOriginalSending,
    StatusSendRetrying,
    StatusSyncing,
    StatusOffline,
    StatusAuthFailed,
    StatusSyncError,
    ReaderLoadingBody,
    FormatUndo,
    FormatRedo,
    FormatFont,
    FormatSansSerif,
    FormatSerif,
    FormatFixedWidth,
    FormatWide,
    FormatNarrow,
    FormatSize,
    FormatSmall,
    FormatNormal,
    FormatLarge,
    FormatHuge,
    FormatBold,
    FormatItalic,
    FormatUnderline,
    FormatColor,
    FormatAlign,
    FormatAlignLeft,
    FormatAlignCenter,
    FormatAlignRight,
    FormatNumbered,
    FormatBullets,
    FormatIndentLess,
    FormatIndentMore,
    DefaultAppLabel,
    DefaultAppIsDefault,
    DefaultAppNotDefault,
    DefaultAppSet,
    DefaultAppNotInstalled,
    DefaultAppFailed,
    DefaultAppFinishInSettings,
    StatusComposeBusy,
    ReaderPreparingBody,
    ComposeToggleQuote,
    ReaderTo,
    ReaderSentTo,
    ReaderMe,
    ReaderShowDetails,
    ReaderCc,
    WelcomeTitle,
    WelcomeBody,
    CommandMarkRead,
    CommandMarkUnread,
    CommandSyncNow,
    QuoteFrom,
    QuoteDate,
    QuoteSubject,
    QuoteTo,
    ForwardHeader,
    ComposeLocalAccount,
    ComposeNoRecipients,
    ComposeInvalidAddress,
    ComposeAttachTooLarge,
    ComposeAttachReadFailed,
    ComposeRemoveAttachment,
    ComposeAttachLoading,
    ComposeDiscardConfirm,
    ComposeDiscardDetail,
    ComposeDiscardButton,
    ComposeCancelButton,
    ComposeCannotSend,
    ComposeNoAccount,
    AccountAddTitle,
    AccountFieldName,
    AccountFieldEmail,
    AccountFieldPassword,
    AccountNamePlaceholder,
    AccountEmailPlaceholder,
    AccountSignInWithGoogle,
    AccountUseAppPassword,
    AccountUseGoogleSignIn,
    AccountGoogleNotConfigured,
    AccountWaitingForBrowser,
    AccountManualSettings,
    AccountImapServer,
    AccountSmtpServer,
    AccountFieldHost,
    AccountFieldPort,
    AccountFieldUsername,
    AccountSecurityNone,
    AccountConnect,
    AccountChecking,
    AccountLookingUp,
    AccountCancel,
    AccountSave,
    AccountUpdatePassword,
    AccountEditNames,
    AccountFieldSender,
    AccountSenderPlaceholder,
    ComposeRemoveRecipient,
    AccountNewPassword,
    AccountSignInAgain,
    AccountRemove,
    AccountRemoveConfirm,
    AccountRemoveDetail,
    AccountBadgeLocal,
    AccountUpToDate,
    AccountSyncing,
    AccountOffline,
    AccountSignInFailed,
    AccountPasswordMissing,
    AccountGoogleUnavailable,
    AccountLocalOnly,
    AccountNoteAppPassword,
    AccountNoteOAuthRequired,
    AccountNoteProtonBridge,
    AccountNoteEnableImap,
    AccountErrInvalidEmail,
    AccountErrPasswordRequired,
    AccountErrHostRequired,
    AccountErrInvalidPort,
    AccountErrAlreadyAdded,
    AccountErrWrongPassword,
    AccountErrGoogleDenied,
    AccountErrCannotReach,
    AccountErrCertificate,
    AccountErrCancelled,
    AccountErrUnexpected,
    AccountErrDifferentGoogle,
    AccountErrSaveFailed,
    NotificationsHint,
    AttachmentOpen,
    AttachmentSaveAs,
    AttachmentSaveToDownloads,
    StatusAttachmentSaved,
    StatusAttachmentFailed,
    StatusAttachmentOpenFailed,
    StatusAttachmentRunsCode,
    StatusActionFailed,
    StatusRemoveAccountFailed,
    UnsentTitle,
    UnsentNoSubject,
    UnsentMaybeSent,
    UnsentRetrying,
    UnsentBusy,
    UnsentBeingEdited,
    UnsentRetry,
    UnsentEdit,
    UnsentDiscardDetail,
    UnsentActionFailed,
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
            (MailboxFlagged, E) => "Starred",
            (MailboxFlagged, P) => "Com estrela",

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
            (OnboardingWelcomeTitle, E) => "Welcome to BGMail",
            (OnboardingWelcomeTitle, P) => "Boas-vindas ao BGMail",
            (OnboardingWelcomeBody, E) => "A fast, native mail client that stays out of your way. Let's get it ready.",
            (OnboardingWelcomeBody, P) => "Um cliente de e-mail rápido e nativo, que não atrapalha. Vamos deixar tudo pronto.",
            (OnboardingFastTitle, E) => "Fast and native",
            (OnboardingFastTitle, P) => "Rápido e nativo",
            (OnboardingFastBody, E) => "Built in Rust, without a browser underneath.",
            (OnboardingFastBody, P) => "Feito em Rust, sem um navegador por baixo.",
            (OnboardingAccountsTitle, E) => "All your accounts",
            (OnboardingAccountsTitle, P) => "Todas as suas contas",
            (OnboardingAccountsBody, E) => "Gmail and any IMAP account, side by side.",
            (OnboardingAccountsBody, P) => "Gmail e qualquer conta IMAP, lado a lado.",
            (OnboardingPrivateTitle, E) => "Private by default",
            (OnboardingPrivateTitle, P) => "Privado por padrão",
            (OnboardingPrivateBody, E) => "Remote images stay blocked until you allow them.",
            (OnboardingPrivateBody, P) => "Imagens remotas ficam bloqueadas até você permitir.",
            (OnboardingGetStarted, E) => "Get started",
            (OnboardingGetStarted, P) => "Começar",
            (OnboardingThemeTitle, E) => "Choose your look",
            (OnboardingThemeTitle, P) => "Escolha o visual",
            (OnboardingThemeBody, E) => "You can change it any time in Settings.",
            (OnboardingThemeBody, P) => "Você pode mudar quando quiser em Configurações.",
            (OnboardingAccountTitle, E) => "Add your account",
            (OnboardingAccountTitle, P) => "Adicione sua conta",
            (OnboardingAccountBody, E) => "BGMail connects directly to your mail provider.",
            (OnboardingAccountBody, P) => "O BGMail se conecta direto ao seu provedor de e-mail.",
            (OnboardingAccountConnected, E) => "Account connected",
            (OnboardingAccountConnected, P) => "Conta conectada",
            (OnboardingSkip, E) => "Set up later",
            (OnboardingSkip, P) => "Configurar depois",
            (OnboardingDoneTitle, E) => "You're all set",
            (OnboardingDoneTitle, P) => "Tudo pronto",
            (OnboardingDoneBodyAccount, E) => "Your mail is arriving in the background.",
            (OnboardingDoneBodyAccount, P) => "Seus e-mails estão chegando em segundo plano.",
            (OnboardingDoneBodyNoAccount, E) => "Add an account whenever you like, in Settings.",
            (OnboardingDoneBodyNoAccount, P) => "Adicione uma conta quando quiser, em Configurações.",
            (OnboardingDefaultApp, E) => "Use BGMail as my default mail app",
            (OnboardingDefaultApp, P) => "Usar o BGMail como meu app de e-mail padrão",
            (OnboardingDefaultAppHint, E) => "E-mail links on this computer will open here.",
            (OnboardingDefaultAppHint, P) => "Links de e-mail neste computador vão abrir aqui.",
            (OnboardingFinish, E) => "Start using BGMail",
            (OnboardingFinish, P) => "Começar a usar o BGMail",
            (OnboardingBack, E) => "Back",
            (OnboardingBack, P) => "Voltar",
            (OnboardingContinue, E) => "Continue",
            (OnboardingContinue, P) => "Continuar",
            (ViewZoomIn, E) => "Zoom In",
            (ViewZoomIn, P) => "Aumentar zoom",
            (ViewZoomOut, E) => "Zoom Out",
            (ViewZoomOut, P) => "Diminuir zoom",
            (ViewZoomReset, E) => "Actual Size",
            (ViewZoomReset, P) => "Tamanho real",
            (AddAccount, E) => "Add account…",
            (AddAccount, P) => "Adicionar conta…",

            (DesktopNotifications, E) => "Desktop notifications",
            (DesktopNotifications, P) => "Notificações na área de trabalho",

            (RemoteImagesLabel, E) => "Remote images",
            (RemoteImagesLabel, P) => "Imagens remotas",
            (RemoteImagesHint, E) => {
                "Remote images can track when and where you open a message. When they are blocked, the shield next to the subject loads them for that message."
            }
            (RemoteImagesHint, P) => {
                "Imagens remotas podem rastrear quando e onde você abre uma mensagem. Quando estão bloqueadas, o escudo ao lado do assunto as carrega para aquela mensagem."
            }
            (AlwaysLoadRemoteImages, E) => "Always load remote images",
            (AlwaysLoadRemoteImages, P) => "Sempre carregar imagens remotas",
            (BlockedElements, E) => "Blocked images: {} · Click to load",
            (BlockedElements, P) => "Imagens bloqueadas: {} · Clique para carregar",

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
            (ToolbarFlag, E) => "Star message",
            (ToolbarFlag, P) => "Marcar com estrela",
            (ToolbarMove, E) => "Move to folder",
            (ToolbarMove, P) => "Mover para pasta",
            (ToolbarFilter, E) => "Filter messages",
            (ToolbarFilter, P) => "Filtrar mensagens",
            (FilterAll, E) => "All messages",
            (FilterAll, P) => "Todas as mensagens",
            (FilterUnread, E) => "Unread",
            (FilterUnread, P) => "Não lidas",
            (FilterStarred, E) => "Starred",
            (FilterStarred, P) => "Com estrela",
            (FilterAttachments, E) => "With attachments",
            (FilterAttachments, P) => "Com anexos",
            (FilterNoResults, E) => "No messages match this filter",
            (FilterNoResults, P) => "Nenhuma mensagem corresponde ao filtro",
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
            (CommandFlag, E) => "Star",
            (CommandFlag, P) => "Marcar com estrela",
            (CommandUnflag, E) => "Remove star",
            (CommandUnflag, P) => "Remover estrela",
            (CommandPrint, E) => "Print…",
            (CommandPrint, P) => "Imprimir…",
            (PrintFrom, E) => "From",
            (PrintFrom, P) => "De",
            (PrintDate, E) => "Date",
            (PrintDate, P) => "Data",
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
            (ComposeClose, E) => "Close",
            (ComposeClose, P) => "Fechar",
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
            (CtxCopy, E) => "Copy",
            (CtxCopy, P) => "Copiar",
            (CtxSelectAll, E) => "Select all",
            (CtxSelectAll, P) => "Selecionar tudo",
            (CtxOpenLink, E) => "Open link",
            (CtxOpenLink, P) => "Abrir link",
            (CtxCopyLink, E) => "Copy link",
            (CtxCopyLink, P) => "Copiar link",
            (FindPlaceholder, E) => "Find in message",
            (FindPlaceholder, P) => "Buscar na mensagem",
            (FindMatches, E) => "{} of {}",
            (FindMatches, P) => "{} de {}",
            (FindNoMatches, E) => "No matches",
            (FindNoMatches, P) => "Nenhum resultado",
            (NoticeTooDeep, E) => "This message couldn't be displayed formatted, so it is shown as plain text.",
            (NoticeTooDeep, P) => "Não foi possível exibir esta mensagem formatada, por isso ela é exibida como texto simples.",
            (NoticeCrashed, E) => "BGMail closed while showing this message, so it is shown as plain text.",
            (NoticeCrashed, P) => "O BGMail fechou ao exibir esta mensagem, por isso ela é exibida como texto simples.",
            (StatusBodyFailed, E) => "Could not download the message",
            (StatusBodyFailed, P) => "Não foi possível baixar a mensagem",
            (StatusForwardNeedsBody, E) => "The message is still downloading. Try forwarding it again in a moment.",
            (StatusForwardNeedsBody, P) => "A mensagem ainda está sendo baixada. Tente encaminhar de novo em instantes.",
            (StatusMessageSent, E) => "Message sent",
            (StatusMessageSent, P) => "Mensagem enviada",
            (StatusSentCopyFailed, E) => "Sent, but no copy could be saved in Sent",
            (StatusSentCopyFailed, P) => "Enviado, mas sem cópia salva em Enviados",
            (ComposeOriginalSending, E) => {
                "The original message is being sent again. Try again when it finishes."
            }
            (ComposeOriginalSending, P) => {
                "A mensagem original está sendo reenviada. Tente de novo quando terminar."
            }
            (StatusSendFailed, E) => "Sending failed",
            (StatusSendFailed, P) => "Falha no envio",
            (StatusSendRetrying, E) => "Sending failed, will retry",
            (StatusSendRetrying, P) => "Falha no envio, nova tentativa em breve",
            (StatusSyncing, E) => "Updating…",
            (StatusSyncing, P) => "Atualizando…",
            (StatusOffline, E) => "Offline, retrying",
            (StatusOffline, P) => "Sem conexão, tentando novamente",
            (StatusAuthFailed, E) => "Sign-in failed",
            (StatusAuthFailed, P) => "Falha ao entrar na conta",
            (StatusSyncError, E) => "Sync error",
            (StatusSyncError, P) => "Erro de sincronização",
            (ReaderPreparingBody, E) => "Preparing message…",
            (ReaderPreparingBody, P) => "Preparando mensagem…",
            (ComposeToggleQuote, E) => "Show or hide the quoted message",
            (ComposeToggleQuote, P) => "Mostrar ou ocultar a mensagem citada",
            (DefaultAppLabel, E) => "Default mail app",
            (DefaultAppLabel, P) => "App de e-mail padrão",
            (DefaultAppIsDefault, E) => "BGMail is the default",
            (DefaultAppIsDefault, P) => "O BGMail é o padrão",
            (DefaultAppNotDefault, E) => "Another app is the default",
            (DefaultAppNotDefault, P) => "Outro app é o padrão",
            (DefaultAppSet, E) => "Make default",
            (DefaultAppSet, P) => "Tornar padrão",
            (DefaultAppNotInstalled, E) => "Install BGMail to make it the default: this copy isn't registered with the system.",
            (DefaultAppNotInstalled, P) => "Instale o BGMail para torná-lo padrão: esta cópia não está registrada no sistema.",
            (DefaultAppFailed, E) => "Could not make BGMail the default",
            (DefaultAppFailed, P) => "Não foi possível tornar o BGMail padrão",
            (DefaultAppFinishInSettings, E) => "Choose BGMail in the system settings that just opened.",
            (DefaultAppFinishInSettings, P) => "Escolha o BGMail nas configurações do sistema que acabaram de abrir.",
            (StatusComposeBusy, E) => "Finish the message you are writing first",
            (StatusComposeBusy, P) => "Termine primeiro a mensagem que está escrevendo",
            (FormatUndo, E) => "Undo",
            (FormatUndo, P) => "Desfazer",
            (FormatRedo, E) => "Redo",
            (FormatRedo, P) => "Refazer",
            (FormatFont, E) => "Font",
            (FormatFont, P) => "Fonte",
            (FormatSansSerif, E) => "Sans Serif",
            (FormatSansSerif, P) => "Sans Serif",
            (FormatSerif, E) => "Serif",
            (FormatSerif, P) => "Serif",
            (FormatWide, E) => "Wide",
            (FormatWide, P) => "Larga",
            (FormatNarrow, E) => "Narrow",
            (FormatNarrow, P) => "Estreita",
            (FormatFixedWidth, E) => "Fixed width",
            (FormatFixedWidth, P) => "Largura fixa",
            (FormatSize, E) => "Text size",
            (FormatSize, P) => "Tamanho do texto",
            (FormatSmall, E) => "Small",
            (FormatSmall, P) => "Pequeno",
            (FormatNormal, E) => "Normal",
            (FormatNormal, P) => "Normal",
            (FormatLarge, E) => "Large",
            (FormatLarge, P) => "Grande",
            (FormatHuge, E) => "Huge",
            (FormatHuge, P) => "Enorme",
            (FormatBold, E) => "Bold",
            (FormatBold, P) => "Negrito",
            (FormatItalic, E) => "Italic",
            (FormatItalic, P) => "Itálico",
            (FormatUnderline, E) => "Underline",
            (FormatUnderline, P) => "Sublinhado",
            (FormatColor, E) => "Text color",
            (FormatColor, P) => "Cor do texto",
            (FormatAlign, E) => "Align",
            (FormatAlign, P) => "Alinhar",
            (FormatAlignLeft, E) => "Left",
            (FormatAlignLeft, P) => "Esquerda",
            (FormatAlignCenter, E) => "Center",
            (FormatAlignCenter, P) => "Centro",
            (FormatAlignRight, E) => "Right",
            (FormatAlignRight, P) => "Direita",
            (FormatNumbered, E) => "Numbered list",
            (FormatNumbered, P) => "Lista numerada",
            (FormatBullets, E) => "Bulleted list",
            (FormatBullets, P) => "Lista com marcadores",
            (FormatIndentLess, E) => "Decrease indent",
            (FormatIndentLess, P) => "Diminuir recuo",
            (FormatIndentMore, E) => "Increase indent",
            (FormatIndentMore, P) => "Aumentar recuo",
            (ReaderLoadingBody, E) => "Downloading message…",
            (ReaderLoadingBody, P) => "Baixando mensagem…",
            (ReaderSentTo, E) => "to",
            (ReaderSentTo, P) => "para",
            (ReaderMe, E) => "me",
            (ReaderMe, P) => "mim",
            (ReaderShowDetails, E) => "Show details",
            (ReaderShowDetails, P) => "Mostrar detalhes",
            (ReaderTo, E) => "To",
            (ReaderTo, P) => "Para",
            (ReaderCc, E) => "Cc",
            (ReaderCc, P) => "Cc",
            (WelcomeTitle, E) => "Welcome to BGMail",
            (WelcomeTitle, P) => "Bem-vindo ao BGMail",
            (WelcomeBody, E) => "Add an e-mail account to start reading and sending mail.",
            (WelcomeBody, P) => "Adicione uma conta de e-mail para começar a ler e enviar mensagens.",
            (CommandMarkRead, E) => "Mark as Read",
            (CommandMarkRead, P) => "Marcar como lida",
            (CommandMarkUnread, E) => "Mark as Unread",
            (CommandMarkUnread, P) => "Marcar como não lida",
            (CommandSyncNow, E) => "Get New Mail",
            (CommandSyncNow, P) => "Receber novas mensagens",

            (QuoteFrom, E) => "From",
            (QuoteFrom, P) => "De",
            (QuoteDate, E) => "Date",
            (QuoteDate, P) => "Data",
            (QuoteSubject, E) => "Subject",
            (QuoteSubject, P) => "Assunto",
            (QuoteTo, E) => "To",
            (QuoteTo, P) => "Para",
            (ForwardHeader, E) => "---------- Forwarded message ----------",
            (ForwardHeader, P) => "---------- Mensagem encaminhada ----------",
            (ComposeLocalAccount, E) => "This account is local and can't send mail. Add an e-mail account in Settings.",
            (ComposeLocalAccount, P) => "Esta conta é local e não envia e-mails. Adicione uma conta de e-mail nas Configurações.",
            (ComposeNoRecipients, E) => "Add at least one recipient.",
            (ComposeNoRecipients, P) => "Adicione pelo menos um destinatário.",
            (ComposeInvalidAddress, E) => "Invalid address",
            (ComposeInvalidAddress, P) => "Endereço inválido",
            (ComposeAttachTooLarge, E) => "Attachments are limited to 25 MB in total.",
            (ComposeAttachTooLarge, P) => "Os anexos são limitados a 25 MB no total.",
            (ComposeAttachReadFailed, E) => "Couldn't read the file",
            (ComposeAttachReadFailed, P) => "Não foi possível ler o arquivo",
            (ComposeRemoveAttachment, E) => "Remove attachment",
            (ComposeRemoveAttachment, P) => "Remover anexo",
            (ComposeAttachLoading, E) => "Loading attachments…",
            (ComposeAttachLoading, P) => "Carregando anexos…",
            (ComposeDiscardConfirm, E) => "Discard this message?",
            (ComposeDiscardConfirm, P) => "Descartar esta mensagem?",
            (ComposeDiscardDetail, E) => "Your changes will be lost.",
            (ComposeDiscardDetail, P) => "Suas alterações serão perdidas.",
            (ComposeDiscardButton, E) => "Discard",
            (ComposeDiscardButton, P) => "Descartar",
            (ComposeCancelButton, E) => "Cancel",
            (ComposeCancelButton, P) => "Cancelar",
            (ComposeCannotSend, E) => "Can't send",
            (ComposeCannotSend, P) => "Não envia",
            (ComposeNoAccount, E) => "No account",
            (ComposeNoAccount, P) => "Nenhuma conta",
            (AccountAddTitle, E) => "Add an account",
            (AccountAddTitle, P) => "Adicionar uma conta",
            (AccountFieldName, E) => "Account name",
            (AccountFieldName, P) => "Nome da conta",
            (AccountFieldEmail, E) => "E-mail address",
            (AccountFieldEmail, P) => "Endereço de e-mail",
            (AccountFieldPassword, E) => "Password",
            (AccountFieldPassword, P) => "Senha",
            (AccountNamePlaceholder, E) => "Personal, Work…",
            (AccountNamePlaceholder, P) => "Pessoal, Trabalho…",
            (AccountEmailPlaceholder, E) => "you@example.com",
            (AccountEmailPlaceholder, P) => "voce@exemplo.com",
            (AccountSignInWithGoogle, E) => "Sign in with Google",
            (AccountSignInWithGoogle, P) => "Entrar com o Google",
            (AccountUseAppPassword, E) => "Use an app password instead",
            (AccountUseAppPassword, P) => "Usar uma senha de app",
            (AccountUseGoogleSignIn, E) => "Sign in with Google instead",
            (AccountUseGoogleSignIn, P) => "Entrar com o Google",
            (AccountGoogleNotConfigured, E) => "Google sign-in needs BGMAIL_GOOGLE_CLIENT_ID and BGMAIL_GOOGLE_CLIENT_SECRET to be set. You can still connect Gmail over IMAP with an app password (Google Account, Security, App passwords).",
            (AccountGoogleNotConfigured, P) => "O login do Google precisa de BGMAIL_GOOGLE_CLIENT_ID e BGMAIL_GOOGLE_CLIENT_SECRET configurados. Você ainda pode conectar o Gmail por IMAP com uma senha de app (Conta do Google, Segurança, Senhas de app).",
            (AccountWaitingForBrowser, E) => "Finish signing in in your browser…",
            (AccountWaitingForBrowser, P) => "Conclua o login no navegador…",
            (AccountManualSettings, E) => "Manual settings",
            (AccountManualSettings, P) => "Configuração manual",
            (AccountImapServer, E) => "Incoming mail (IMAP)",
            (AccountImapServer, P) => "Recebimento (IMAP)",
            (AccountSmtpServer, E) => "Outgoing mail (SMTP)",
            (AccountSmtpServer, P) => "Envio (SMTP)",
            (AccountFieldHost, E) => "Server",
            (AccountFieldHost, P) => "Servidor",
            (AccountFieldPort, E) => "Port",
            (AccountFieldPort, P) => "Porta",
            (AccountFieldUsername, E) => "User name",
            (AccountFieldUsername, P) => "Nome de usuário",
            (AccountSecurityNone, E) => "None",
            (AccountSecurityNone, P) => "Nenhuma",
            (AccountConnect, E) => "Connect",
            (AccountConnect, P) => "Conectar",
            (AccountChecking, E) => "Checking…",
            (AccountChecking, P) => "Verificando…",
            (AccountLookingUp, E) => "Looking up server settings…",
            (AccountLookingUp, P) => "Procurando as configurações do servidor…",
            (AccountCancel, E) => "Cancel",
            (AccountCancel, P) => "Cancelar",
            (AccountSave, E) => "Save",
            (AccountSave, P) => "Salvar",
            (AccountEditNames, E) => "Edit names",
            (AccountEditNames, P) => "Editar nomes",
            (AccountFieldSender, E) => "Sender name",
            (AccountFieldSender, P) => "Nome de envio",
            (AccountSenderPlaceholder, E) => "Your name",
            (AccountSenderPlaceholder, P) => "Seu nome",
            (ComposeRemoveRecipient, E) => "Remove",
            (ComposeRemoveRecipient, P) => "Remover",
            (AccountUpdatePassword, E) => "Update password",
            (AccountUpdatePassword, P) => "Atualizar senha",
            (AccountNewPassword, E) => "New password",
            (AccountNewPassword, P) => "Nova senha",
            (AccountSignInAgain, E) => "Sign in again",
            (AccountSignInAgain, P) => "Entrar novamente",
            (AccountRemove, E) => "Remove",
            (AccountRemove, P) => "Remover",
            (AccountRemoveConfirm, E) => "Remove this account?",
            (AccountRemoveConfirm, P) => "Remover esta conta?",
            (AccountRemoveDetail, E) => "The mail stored on this computer for this account will be deleted. Messages on the server are not affected.",
            (AccountRemoveDetail, P) => "As mensagens desta conta guardadas neste computador serão apagadas. As mensagens no servidor não são afetadas.",
            (AccountBadgeLocal, E) => "Local",
            (AccountBadgeLocal, P) => "Local",
            (AccountUpToDate, E) => "Up to date",
            (AccountUpToDate, P) => "Atualizada",
            (AccountSyncing, E) => "Syncing…",
            (AccountSyncing, P) => "Sincronizando…",
            (AccountOffline, E) => "Offline",
            (AccountOffline, P) => "Sem conexão",
            (AccountSignInFailed, E) => "Sign-in failed",
            (AccountSignInFailed, P) => "Falha ao entrar",
            (AccountPasswordMissing, E) => "Password missing",
            (AccountPasswordMissing, P) => "Senha ausente",
            (AccountGoogleUnavailable, E) => "Google sign-in not configured",
            (AccountGoogleUnavailable, P) => "Login do Google não configurado",
            (AccountLocalOnly, E) => "Stored on this computer only",
            (AccountLocalOnly, P) => "Guardada somente neste computador",
            (AccountNoteAppPassword, E) => "This provider requires an app password, not your regular password. Create one in your account's security settings.",
            (AccountNoteAppPassword, P) => "Este provedor exige uma senha de app, não a sua senha normal. Crie uma nas configurações de segurança da sua conta.",
            (AccountNoteOAuthRequired, E) => "This provider only accepts sign-in through its own login page, which BGMail doesn't support yet.",
            (AccountNoteOAuthRequired, P) => "Este provedor só aceita login pela própria página de acesso, o que o BGMail ainda não suporta.",
            (AccountNoteProtonBridge, E) => "Proton Mail works through Proton Mail Bridge: keep it running and use the password it shows.",
            (AccountNoteProtonBridge, P) => "O Proton Mail funciona pelo Proton Mail Bridge: mantenha-o aberto e use a senha que ele mostra.",
            (AccountNoteEnableImap, E) => "Turn on IMAP access in your webmail settings before connecting.",
            (AccountNoteEnableImap, P) => "Ative o acesso IMAP nas configurações do seu webmail antes de conectar.",
            (AccountErrInvalidEmail, E) => "Enter a valid e-mail address.",
            (AccountErrInvalidEmail, P) => "Digite um endereço de e-mail válido.",
            (AccountErrPasswordRequired, E) => "Enter your password.",
            (AccountErrPasswordRequired, P) => "Digite sua senha.",
            (AccountErrHostRequired, E) => "Enter the incoming and outgoing server names.",
            (AccountErrHostRequired, P) => "Digite os servidores de recebimento e de envio.",
            (AccountErrInvalidPort, E) => "Ports must be numbers from 1 to 65535.",
            (AccountErrInvalidPort, P) => "As portas devem ser números de 1 a 65535.",
            (AccountErrAlreadyAdded, E) => "This account has already been added.",
            (AccountErrAlreadyAdded, P) => "Esta conta já foi adicionada.",
            (AccountErrWrongPassword, E) => "The server rejected the user name or password. If your provider requires an app password, use it here.",
            (AccountErrWrongPassword, P) => "O servidor recusou o usuário ou a senha. Se o seu provedor exige uma senha de app, use-a aqui.",
            (AccountErrGoogleDenied, E) => "Google did not grant access. Try signing in again.",
            (AccountErrGoogleDenied, P) => "O Google não concedeu acesso. Tente entrar novamente.",
            (AccountErrCannotReach, E) => "Can't reach the server. Check the server name, the port and your connection.",
            (AccountErrCannotReach, P) => "Não foi possível alcançar o servidor. Verifique o servidor, a porta e sua conexão.",
            (AccountErrCertificate, E) => "The server's security certificate could not be verified.",
            (AccountErrCertificate, P) => "Não foi possível verificar o certificado de segurança do servidor.",
            (AccountErrCancelled, E) => "Sign-in was cancelled or timed out.",
            (AccountErrCancelled, P) => "O login foi cancelado ou expirou.",
            (AccountErrUnexpected, E) => "The server answered in an unexpected way.",
            (AccountErrUnexpected, P) => "O servidor respondeu de forma inesperada.",
            (AccountErrDifferentGoogle, E) => "You signed in with a different Google account.",
            (AccountErrDifferentGoogle, P) => "Você entrou com outra conta do Google.",
            (AccountErrSaveFailed, E) => "Could not save the account.",
            (AccountErrSaveFailed, P) => "Não foi possível salvar a conta.",
            (NotificationsHint, E) => "Show a desktop notification when new mail arrives.",
            (NotificationsHint, P) => "Mostrar uma notificação quando chegarem novas mensagens.",
            (AttachmentOpen, E) => "Open",
            (AttachmentOpen, P) => "Abrir",
            (AttachmentSaveAs, E) => "Save As…",
            (AttachmentSaveAs, P) => "Salvar como…",
            (AttachmentSaveToDownloads, E) => "Save to Downloads",
            (AttachmentSaveToDownloads, P) => "Salvar em Downloads",
            (StatusAttachmentSaved, E) => "Attachment saved",
            (StatusAttachmentSaved, P) => "Anexo salvo",
            (StatusAttachmentFailed, E) => "Could not save the attachment",
            (StatusAttachmentFailed, P) => "Não foi possível salvar o anexo",
            (StatusAttachmentOpenFailed, E) => "Could not open the attachment",
            (StatusAttachmentOpenFailed, P) => "Não foi possível abrir o anexo",
            (StatusAttachmentRunsCode, E) => {
                "This type of file can run programs, so it won't open from here. Save it if you trust it."
            }
            (StatusAttachmentRunsCode, P) => {
                "Este tipo de arquivo pode executar programas, então não abre por aqui. Salve-o se confiar nele."
            }
            (StatusActionFailed, E) => "Could not complete the action",
            (StatusActionFailed, P) => "Não foi possível concluir a ação",
            (StatusRemoveAccountFailed, E) => "Could not remove the account",
            (StatusRemoveAccountFailed, P) => "Não foi possível remover a conta",
            (UnsentTitle, E) => "Unsent messages",
            (UnsentTitle, P) => "Mensagens não enviadas",
            (UnsentNoSubject, E) => "(no subject)",
            (UnsentNoSubject, P) => "(sem assunto)",
            (UnsentMaybeSent, E) => "May already have been sent",
            (UnsentMaybeSent, P) => "Pode já ter sido enviada",
            (UnsentRetrying, E) => "Still trying to send",
            (UnsentRetrying, P) => "Ainda tentando enviar",
            (UnsentBusy, E) => "This message is being sent right now",
            (UnsentBusy, P) => "Esta mensagem está sendo enviada agora",
            (UnsentBeingEdited, E) => "This message is open for editing: send or close it first",
            (UnsentBeingEdited, P) => "Esta mensagem está aberta para edição: envie ou feche antes",
            (UnsentRetry, E) => "Retry",
            (UnsentRetry, P) => "Tentar novamente",
            (UnsentEdit, E) => "Edit",
            (UnsentEdit, P) => "Editar",
            (UnsentDiscardDetail, E) => "It will not be sent and cannot be recovered.",
            (UnsentDiscardDetail, P) => "Ela não será enviada e não poderá ser recuperada.",
            (UnsentActionFailed, E) => "Could not update the unsent message",
            (UnsentActionFailed, P) => "Não foi possível atualizar a mensagem não enviada",
        }
    }
}

/// Picks the singular or plural form for `count` (both languages use the
/// singular only for exactly one; PT-BR "0 mensagens" is plural too).
fn plural<'a>(count: usize, one: &'a str, many: &'a str) -> &'a str {
    if count == 1 {
        one
    } else {
        many
    }
}

/// "`count` message(s)" in `language`.
fn messages_noun(language: Language, count: usize) -> &'static str {
    match language {
        Language::English => plural(count, "message", "messages"),
        Language::Portuguese => plural(count, "mensagem", "mensagens"),
    }
}

/// "`count` account(s)" in `language`.
fn accounts_noun(language: Language, count: usize) -> &'static str {
    match language {
        Language::English => plural(count, "account", "accounts"),
        Language::Portuguese => plural(count, "conta", "contas"),
    }
}

/// Message list header: number of messages in the current mailbox.
pub fn message_count(language: Language, count: usize) -> String {
    format!("{count} {}", messages_noun(language, count))
}

/// Reader pane while several messages are selected.
pub fn messages_selected(language: Language, count: usize) -> String {
    match language {
        Language::English => format!("{count} messages selected"),
        Language::Portuguese => format!("{count} mensagens selecionadas"),
    }
}

/// Question before deleting a multiple selection for good.
pub fn delete_forever_confirm(language: Language, count: usize) -> String {
    match (language, count) {
        (Language::English, 1) => "Delete this message permanently?".to_string(),
        (Language::Portuguese, 1) => "Excluir esta mensagem permanentemente?".to_string(),
        (Language::English, _) => format!("Delete {count} messages permanently?"),
        (Language::Portuguese, _) => format!("Excluir {count} mensagens permanentemente?"),
    }
}

/// Status bar (left): account and message counts.
pub fn status_counts(language: Language, accounts: usize, messages: usize) -> String {
    format!(
        "{accounts} {} · {messages} {}",
        accounts_noun(language, accounts),
        messages_noun(language, messages)
    )
}

/// Status bar (left) while a search filter is active.
pub fn status_search_counts(
    language: Language,
    accounts: usize,
    showing: usize,
    total: usize,
) -> String {
    let of = match language {
        Language::English => "of",
        Language::Portuguese => "de",
    };
    format!(
        "{accounts} {} · {showing} {of} {total} {}",
        accounts_noun(language, accounts),
        messages_noun(language, total)
    )
}

/// Status bar (right): unread count.
pub fn status_unread(language: Language, unread: usize) -> String {
    let noun = match language {
        Language::English => "unread",
        Language::Portuguese => plural(unread, "não lida", "não lidas"),
    };
    format!("{unread} {noun}")
}

/// Status bar (right): outgoing messages that stopped retrying.
pub fn status_unsent(language: Language, unsent: usize) -> String {
    let noun = match language {
        Language::English => "unsent",
        Language::Portuguese => plural(unsent, "não enviada", "não enviadas"),
    };
    format!("{unsent} {noun}")
}

/// Attribution line above a quoted reply.
pub fn reply_attribution(language: Language, date: &str, sender: &str) -> String {
    match language {
        Language::English => format!("On {date} {sender} wrote:"),
        Language::Portuguese => format!("Em {date}, {sender} escreveu:"),
    }
}

/// Status bar (right): time of the last completed sync (`time` preformatted).
pub fn status_updated_at(language: Language, time: &str) -> String {
    match language {
        Language::English => format!("Updated at {time}"),
        Language::Portuguese => format!("Atualizado às {time}"),
    }
}

/// Status bar note for the reader's page zoom (`zoom` = 1 for 100%).
pub fn status_zoom(language: Language, zoom: f32) -> String {
    let percent = (zoom * 100.0).round();
    match language {
        Language::English => format!("Zoom {percent}%"),
        Language::Portuguese => format!("Zoom {percent}%"),
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

    #[test]
    fn every_language_survives_its_saved_code() {
        for language in Language::ALL {
            assert_eq!(Language::from_code(language.code()), language);
        }
    }

    #[test]
    fn an_unknown_saved_language_falls_back_to_the_default() {
        assert_eq!(Language::from_code("xx"), Language::default());
        assert_eq!(Language::from_code(""), Language::default());
    }

    const ALL_KEYS: [Key; 249] = [
        Key::ViewZoomIn,
        Key::ViewZoomOut,
        Key::ViewZoomReset,
        Key::OnboardingWelcomeTitle,
        Key::OnboardingWelcomeBody,
        Key::OnboardingFastTitle,
        Key::OnboardingFastBody,
        Key::OnboardingAccountsTitle,
        Key::OnboardingAccountsBody,
        Key::OnboardingPrivateTitle,
        Key::OnboardingPrivateBody,
        Key::OnboardingGetStarted,
        Key::OnboardingThemeTitle,
        Key::OnboardingThemeBody,
        Key::OnboardingAccountTitle,
        Key::OnboardingAccountBody,
        Key::OnboardingAccountConnected,
        Key::OnboardingSkip,
        Key::OnboardingDoneTitle,
        Key::OnboardingDoneBodyAccount,
        Key::OnboardingDoneBodyNoAccount,
        Key::OnboardingDefaultApp,
        Key::OnboardingDefaultAppHint,
        Key::OnboardingFinish,
        Key::OnboardingBack,
        Key::OnboardingContinue,
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
        Key::RemoteImagesLabel,
        Key::RemoteImagesHint,
        Key::AlwaysLoadRemoteImages,
        Key::BlockedElements,
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
        Key::FilterAll,
        Key::FilterUnread,
        Key::FilterStarred,
        Key::FilterAttachments,
        Key::FilterNoResults,
        Key::ToolbarMore,
        Key::ComposeWindowTitle,
        Key::ComposeSend,
        Key::ComposeAttach,
        Key::ComposeDiscard,
        Key::ComposeClose,
        Key::ComposeFrom,
        Key::ComposeTo,
        Key::ComposeCc,
        Key::ComposeBcc,
        Key::ComposeSubject,
        Key::ComposeCcBcc,
        Key::ComposeFieldPlaceholder,
        Key::ComposeBodyPlaceholder,
        Key::CtxCopy,
        Key::CtxSelectAll,
        Key::CtxOpenLink,
        Key::CtxCopyLink,
        Key::FindPlaceholder,
        Key::FindMatches,
        Key::FindNoMatches,
        Key::NoticeTooDeep,
        Key::NoticeCrashed,
        Key::CommandPrint,
        Key::PrintFrom,
        Key::PrintDate,
        Key::StatusBodyFailed,
        Key::StatusForwardNeedsBody,
        Key::StatusMessageSent,
        Key::StatusSendFailed,
        Key::StatusSentCopyFailed,
        Key::ComposeOriginalSending,
        Key::StatusSendRetrying,
        Key::StatusSyncing,
        Key::StatusOffline,
        Key::StatusAuthFailed,
        Key::StatusSyncError,
        Key::ReaderLoadingBody,
        Key::FormatUndo,
        Key::FormatRedo,
        Key::FormatFont,
        Key::FormatSansSerif,
        Key::FormatSerif,
        Key::FormatFixedWidth,
        Key::FormatWide,
        Key::FormatNarrow,
        Key::FormatSize,
        Key::FormatSmall,
        Key::FormatNormal,
        Key::FormatLarge,
        Key::FormatHuge,
        Key::FormatBold,
        Key::FormatItalic,
        Key::FormatUnderline,
        Key::FormatColor,
        Key::FormatAlign,
        Key::FormatAlignLeft,
        Key::FormatAlignCenter,
        Key::FormatAlignRight,
        Key::FormatNumbered,
        Key::FormatBullets,
        Key::FormatIndentLess,
        Key::FormatIndentMore,
        Key::DefaultAppLabel,
        Key::DefaultAppIsDefault,
        Key::DefaultAppNotDefault,
        Key::DefaultAppSet,
        Key::DefaultAppNotInstalled,
        Key::DefaultAppFailed,
        Key::DefaultAppFinishInSettings,
        Key::StatusComposeBusy,
        Key::ReaderPreparingBody,
        Key::ComposeToggleQuote,
        Key::ReaderTo,
        Key::ReaderSentTo,
        Key::ReaderMe,
        Key::ReaderShowDetails,
        Key::ReaderCc,
        Key::WelcomeTitle,
        Key::WelcomeBody,
        Key::CommandMarkRead,
        Key::CommandMarkUnread,
        Key::CommandSyncNow,
        Key::QuoteFrom,
        Key::QuoteDate,
        Key::QuoteSubject,
        Key::QuoteTo,
        Key::ForwardHeader,
        Key::ComposeLocalAccount,
        Key::ComposeNoRecipients,
        Key::ComposeInvalidAddress,
        Key::ComposeAttachTooLarge,
        Key::ComposeAttachReadFailed,
        Key::ComposeRemoveAttachment,
        Key::ComposeAttachLoading,
        Key::ComposeDiscardConfirm,
        Key::ComposeDiscardDetail,
        Key::ComposeDiscardButton,
        Key::ComposeCancelButton,
        Key::ComposeCannotSend,
        Key::ComposeNoAccount,
        Key::AccountAddTitle,
        Key::AccountFieldName,
        Key::AccountFieldEmail,
        Key::AccountFieldPassword,
        Key::AccountNamePlaceholder,
        Key::AccountEmailPlaceholder,
        Key::AccountSignInWithGoogle,
        Key::AccountUseAppPassword,
        Key::AccountUseGoogleSignIn,
        Key::AccountGoogleNotConfigured,
        Key::AccountWaitingForBrowser,
        Key::AccountManualSettings,
        Key::AccountImapServer,
        Key::AccountSmtpServer,
        Key::AccountFieldHost,
        Key::AccountFieldPort,
        Key::AccountFieldUsername,
        Key::AccountSecurityNone,
        Key::AccountConnect,
        Key::AccountChecking,
        Key::AccountLookingUp,
        Key::AccountCancel,
        Key::AccountSave,
        Key::AccountUpdatePassword,
        Key::AccountEditNames,
        Key::AccountFieldSender,
        Key::AccountSenderPlaceholder,
        Key::ComposeRemoveRecipient,
        Key::AccountNewPassword,
        Key::AccountSignInAgain,
        Key::AccountRemove,
        Key::AccountRemoveConfirm,
        Key::AccountRemoveDetail,
        Key::AccountBadgeLocal,
        Key::AccountUpToDate,
        Key::AccountSyncing,
        Key::AccountOffline,
        Key::AccountSignInFailed,
        Key::AccountPasswordMissing,
        Key::AccountGoogleUnavailable,
        Key::AccountLocalOnly,
        Key::AccountNoteAppPassword,
        Key::AccountNoteOAuthRequired,
        Key::AccountNoteProtonBridge,
        Key::AccountNoteEnableImap,
        Key::AccountErrInvalidEmail,
        Key::AccountErrPasswordRequired,
        Key::AccountErrHostRequired,
        Key::AccountErrInvalidPort,
        Key::AccountErrAlreadyAdded,
        Key::AccountErrWrongPassword,
        Key::AccountErrGoogleDenied,
        Key::AccountErrCannotReach,
        Key::AccountErrCertificate,
        Key::AccountErrCancelled,
        Key::AccountErrUnexpected,
        Key::AccountErrDifferentGoogle,
        Key::AccountErrSaveFailed,
        Key::NotificationsHint,
        Key::AttachmentOpen,
        Key::AttachmentSaveAs,
        Key::AttachmentSaveToDownloads,
        Key::StatusAttachmentSaved,
        Key::StatusAttachmentFailed,
        Key::StatusAttachmentOpenFailed,
        Key::StatusAttachmentRunsCode,
        Key::StatusActionFailed,
        Key::StatusRemoveAccountFailed,
        Key::UnsentTitle,
        Key::UnsentNoSubject,
        Key::UnsentMaybeSent,
        Key::UnsentRetrying,
        Key::UnsentBusy,
        Key::UnsentBeingEdited,
        Key::UnsentRetry,
        Key::UnsentEdit,
        Key::UnsentDiscardDetail,
        Key::UnsentActionFailed,
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
    fn counts_use_singular_only_for_one() {
        let (en, pt) = (Language::English, Language::Portuguese);
        assert_eq!(status_counts(en, 1, 1), "1 account · 1 message");
        assert_eq!(status_counts(en, 0, 2), "0 accounts · 2 messages");
        assert_eq!(status_counts(pt, 1, 1), "1 conta · 1 mensagem");
        assert_eq!(status_counts(pt, 2, 0), "2 contas · 0 mensagens");
        assert_eq!(
            status_search_counts(en, 1, 0, 1),
            "1 account · 0 of 1 message"
        );
        assert_eq!(
            status_search_counts(pt, 3, 1, 5),
            "3 contas · 1 de 5 mensagens"
        );
        assert_eq!(message_count(en, 1), "1 message");
        assert_eq!(message_count(pt, 7), "7 mensagens");
        assert_eq!(status_unread(en, 1), "1 unread");
        assert_eq!(status_unread(pt, 1), "1 não lida");
        assert_eq!(status_unread(pt, 3), "3 não lidas");
        assert_eq!(status_unsent(en, 2), "2 unsent");
        assert_eq!(status_unsent(pt, 1), "1 não enviada");
        assert_eq!(status_unsent(pt, 4), "4 não enviadas");
    }

    #[test]
    fn formatted_strings_include_arguments() {
        let en = status_counts(Language::English, 2, 8);
        assert!(en.contains('2') && en.contains('8') && en.contains("accounts"));
        let pt = status_unread(Language::Portuguese, 9);
        assert!(pt.contains('9') && pt.contains("não lidas"));
        assert_eq!(status_zoom(Language::English, 1.25), "Zoom 125%");
        assert_eq!(
            status_updated_at(Language::Portuguese, "14:05"),
            "Atualizado às 14:05"
        );
        assert_eq!(
            status_updated_at(Language::English, "9:00"),
            "Updated at 9:00"
        );
        assert_eq!(
            messages_selected(Language::Portuguese, 3),
            "3 mensagens selecionadas"
        );
        assert_eq!(
            delete_forever_confirm(Language::English, 2),
            "Delete 2 messages permanently?"
        );
        assert_eq!(
            reply_attribution(Language::Portuguese, "31/10/2026 14:07", "Ana"),
            "Em 31/10/2026 14:07, Ana escreveu:"
        );
    }
}
