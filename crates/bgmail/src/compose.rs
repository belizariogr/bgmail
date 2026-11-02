//! Standalone compose window: new messages, replies and forwards.
//!
//! Opens in its own window (like settings). The view owns the editors and the
//! attachment list; validation, draft building, attachment reading and size
//! formatting are plain functions below so they are unit-tested without GPUI.
//! Delivery goes through [`RootView::send_draft`], which queues the message
//! for the account's sync worker.

use std::path::{Path, PathBuf};

use gpui::{
    anchored, black, deferred, size, white, AnyWindowHandle, App, Bounds, ClickEvent, Context,
    Corner, Entity, ExternalPaths, FocusHandle, Focusable, Global, Hsla, KeyBinding, MouseButton,
    PathPromptOptions, Point, PromptButton, PromptLevel, Render, ScrollHandle, Size, Subscription,
    Task, WeakEntity, Window, WindowBounds,
};
use html_view::{
    Appearance as PageAppearance, DetachedDocument, DocumentOptions, HtmlDocument, HtmlView,
    Resources,
};
use mail_core::{Address, Draft, OutgoingAttachment};
use theme::{ActiveTheme, Theme, ThemeColors};

use crate::data::MessageBody;
use crate::mail_html::{email_document, DocumentColors};
use ui::prelude::*;
use ui::{
    Button, ButtonStyle, Color, ContextMenu, Icon, IconButton, IconName, IconSize, Label,
    LabelSize, TextArea, TextAreaEvent, TextInput, TextInputEvent, Tooltip,
};

use crate::actions::{ComposeAttach, ComposeClose, ComposeDiscard, ComposeSend};
use crate::app_menus;
use crate::attachments::{file_family, human_size};
use crate::locale::{ActiveLanguage, Key, Language};
use crate::root::RootView;

/// Default width of the compose window on first open, in pixels.
pub const COMPOSE_DEFAULT_WIDTH: f32 = 790.0;
/// Default height of the compose window on first open, in pixels.
pub const COMPOSE_DEFAULT_HEIGHT: f32 = 720.0;
/// Minimum width of the compose window, in pixels.
pub const COMPOSE_MIN_WIDTH: f32 = 480.0;
/// Minimum height of the compose window, in pixels.
pub const COMPOSE_MIN_HEIGHT: f32 = 400.0;

/// Sentinel stored in config when the compose window has never been positioned;
/// [`open_bounds`] centers the window on screen instead.
pub const COMPOSE_POSITION_UNSET: f32 = -1.0;

/// Total size allowed for all attachments of one message. Most providers
/// reject messages above ~25 MB (and base64 grows the payload by a third).
pub const ATTACHMENT_LIMIT_BYTES: usize = 25 * 1024 * 1024;

/// Key context of the compose root element (scopes the send shortcut).
const KEY_CONTEXT: &str = "ComposeView";
/// Send shortcut on macOS.
pub const SEND_MAC: &str = "cmd-enter";
/// Send shortcut on Windows / Linux.
pub const SEND_OTHER: &str = "ctrl-enter";

/// Fixed width of the field labels column (From, To, Cc, …), in pixels.
const FIELD_LABEL_WIDTH: f32 = 72.0;
/// Minimum body height in text rows, so the empty body is a big click target.
const BODY_MIN_LINES: usize = 12;

/// Builds the window bounds used when opening the compose window from persisted
/// settings. Negative x/y mean the window has not been placed yet and should
/// open centered.
pub fn open_bounds(
    origin: Point<gpui::Pixels>,
    window_size: Size<gpui::Pixels>,
    cx: &App,
) -> Bounds<gpui::Pixels> {
    let size = clamp_compose_size(window_size);
    if compose_position_is_unset(origin) {
        Bounds::centered(None, size, cx)
    } else {
        Bounds::new(origin, size)
    }
}

/// Clamps a stored compose size to the window minimums.
pub fn clamp_compose_size(window_size: Size<gpui::Pixels>) -> Size<gpui::Pixels> {
    size(
        px(f32::from(window_size.width).max(COMPOSE_MIN_WIDTH)),
        px(f32::from(window_size.height).max(COMPOSE_MIN_HEIGHT)),
    )
}

/// Whether persisted compose coordinates mean "center on first open".
pub fn compose_position_is_unset(origin: Point<gpui::Pixels>) -> bool {
    f32::from(origin.x) < 0.0 || f32::from(origin.y) < 0.0
}

/// An account the compose window can send from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComposeAccount {
    pub id: i64,
    pub name: String,
    pub email: String,
    /// Local/demo accounts have no server to send through.
    pub can_send: bool,
}

/// Content the compose window opens with (reply, reply all, forward).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComposeInit {
    /// Account to send from (the one the original message belongs to).
    pub account_id: Option<i64>,
    pub draft: mail_core::Draft,
    /// Outbox row this message re-creates (editing an unsent message);
    /// deleted once the edited copy is queued.
    pub replaces_outgoing: Option<i64>,
    /// What a reply quotes, to send it with its formatting.
    pub quoted_original: Option<QuotedOriginal>,
}

/// Width the quoted original is first laid out at, before the real one is known.
const QUOTE_FIRST_LAYOUT_WIDTH: f32 = 800.0;

/// The original message of a reply: its `> ` quote for the plain-text version
/// and its HTML, ready to be sent on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuotedOriginal {
    pub block: String,
    pub attribution: String,
    pub html: String,
    /// Whether the user allows remote images for the original.
    pub load_remote: bool,
}

/// The recipient fields that are validated as address lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AddressField {
    To,
    Cc,
    Bcc,
}

/// Which editor gets keyboard focus when the window opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ComposeFocus {
    To,
    Body,
}

/// Raw text of the editors, snapshotted for validation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ComposeFields {
    to: String,
    cc: String,
    bcc: String,
    subject: String,
    body: String,
}

/// Threading headers carried over from the message being answered.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ReplyHeaders {
    in_reply_to: Option<String>,
    references: Vec<String>,
    quoted: Option<QuotedOriginal>,
}

/// Why a draft could not be built: per-field address errors (shown under the
/// field) and/or a window-level message (shown in the banner).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ComposeErrors {
    to: Option<String>,
    cc: Option<String>,
    bcc: Option<String>,
    banner: Option<String>,
}

impl ComposeErrors {
    fn field(&self, field: AddressField) -> Option<&str> {
        match field {
            AddressField::To => self.to.as_deref(),
            AddressField::Cc => self.cc.as_deref(),
            AddressField::Bcc => self.bcc.as_deref(),
        }
    }

    fn field_mut(&mut self, field: AddressField) -> &mut Option<String> {
        match field {
            AddressField::To => &mut self.to,
            AddressField::Cc => &mut self.cc,
            AddressField::Bcc => &mut self.bcc,
        }
    }

    fn has_field_errors(&self) -> bool {
        self.to.is_some() || self.cc.is_some() || self.bcc.is_some()
    }
}

/// Result of reading picked/dropped files on a background thread.
#[derive(Debug, Default, PartialEq, Eq)]
struct AttachmentBatch {
    files: Vec<OutgoingAttachment>,
    /// Some file did not fit under [`ATTACHMENT_LIMIT_BYTES`].
    too_large: bool,
    /// Names of files that could not be read (missing, directory, I/O error).
    failed: Vec<String>,
}

/// Registered once per app: binds Cmd/Ctrl+Enter to [`ComposeSend`] inside
/// the compose window.
struct ComposeKeysBound;

impl Global for ComposeKeysBound {}

/// Standalone new-message window content.
pub struct ComposeView {
    accounts: Vec<ComposeAccount>,
    /// Index into [`Self::accounts`] for the From address.
    from_account: usize,
    /// Whether the From account picker is open.
    from_menu_open: bool,
    /// Whether Cc and Bcc rows are visible.
    show_cc_bcc: bool,
    to: Entity<TextInput>,
    cc: Entity<TextInput>,
    bcc: Entity<TextInput>,
    subject: Entity<TextInput>,
    body: Entity<TextArea>,
    /// Scroll container of the body; the text area keeps its caret visible in it.
    body_scroll: ScrollHandle,
    attachments: Vec<OutgoingAttachment>,
    /// Background reads still running (Send is disabled meanwhile).
    pending_attachment_loads: usize,
    reply: ReplyHeaders,
    /// See [`ComposeInit::replaces_outgoing`].
    replaces_outgoing: Option<i64>,
    /// Inline address errors + banner message from the last send attempt.
    errors: ComposeErrors,
    /// A send is being handed to the main view (blocks double sends).
    sending: bool,
    /// The user typed or changed attachments (Discard asks for confirmation).
    edited: bool,
    /// Back-reference to the main view for persisting bounds and sending.
    root: WeakEntity<RootView>,
    /// This window, so a send finishing in a task can close it.
    window_handle: AnyWindowHandle,
    /// Last bounds reported to [`RootView`], so we only defer a sync when the
    /// window actually moved or resized (and never re-enter `RootView` during
    /// our own render — that panics with a double lease).
    last_reported_bounds: Option<(Point<gpui::Pixels>, Size<gpui::Pixels>)>,
    /// Last title pushed to the platform window.
    last_title: String,
    /// Cached copy of the white-compose preference. Set at open time and pushed
    /// from [`RootView`] when it changes — never read from `RootView` during
    /// render (that panics while the main view is on the stack).
    white_background: bool,
    /// The quoted original shows, as in Gmail, only after asking for it.
    quote_expanded: bool,
    /// The quoted original, parsed the first time it is shown.
    quote_view: Option<Entity<HtmlView>>,
    _quote_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl ComposeView {
    /// Creates the compose view: real editors for every field, the From
    /// account preselected (`init.account_id`, else the first account that can
    /// send) and, with `init`, everything prefilled from its draft.
    pub fn new(
        accounts: Vec<ComposeAccount>,
        root: WeakEntity<RootView>,
        white_background: bool,
        init: Option<ComposeInit>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        ensure_key_bindings(cx);
        let language = cx.language();
        let to = cx.new(|cx| TextInput::new(Key::ComposeFieldPlaceholder.tr(language), cx));
        let cc = cx.new(|cx| TextInput::new("", cx));
        let bcc = cx.new(|cx| TextInput::new("", cx));
        let subject = cx.new(|cx| TextInput::new("", cx));
        let body_scroll = ScrollHandle::new();
        let body = cx.new(|cx| {
            let mut area = TextArea::new(Key::ComposeBodyPlaceholder.tr(language), cx)
                .min_lines(BODY_MIN_LINES);
            area.track_scroll(&body_scroll);
            area
        });

        let mut subscriptions = Vec::new();
        for (input, field) in [
            (&to, AddressField::To),
            (&cc, AddressField::Cc),
            (&bcc, AddressField::Bcc),
        ] {
            subscriptions.push(cx.subscribe(input, move |this, _, _: &TextInputEvent, cx| {
                this.on_address_changed(field, cx);
            }));
        }
        subscriptions.push(cx.subscribe(&subject, |this, _, _: &TextInputEvent, cx| {
            this.mark_edited(cx);
        }));
        subscriptions.push(cx.subscribe(&body, |this, _, _: &TextAreaEvent, cx| {
            this.mark_edited(cx);
        }));

        let from_account = account_index(&accounts, None);
        let mut view = Self {
            accounts,
            from_account,
            from_menu_open: false,
            show_cc_bcc: false,
            to,
            cc,
            bcc,
            subject,
            body,
            body_scroll,
            attachments: Vec::new(),
            pending_attachment_loads: 0,
            reply: ReplyHeaders::default(),
            replaces_outgoing: None,
            errors: ComposeErrors::default(),
            sending: false,
            edited: false,
            root,
            window_handle: window.window_handle(),
            last_reported_bounds: None,
            last_title: String::new(),
            white_background,
            quote_expanded: false,
            quote_view: None,
            _quote_task: None,
            _subscriptions: subscriptions,
        };
        view.apply_init(init, window, cx);
        view
    }

    /// Replaces the window content with `init` (or clears it for a blank
    /// message) and focuses the body for replies / the To field otherwise.
    /// Also used when a reply is requested while the window is already open.
    pub(crate) fn apply_init(
        &mut self,
        init: Option<ComposeInit>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = initial_focus(init.as_ref());
        let init = init.unwrap_or_default();
        let draft = init.draft;
        self.from_account = account_index(&self.accounts, init.account_id);
        self.replaces_outgoing = init.replaces_outgoing;
        set_input(&self.to, &mail_core::format_address_list(&draft.to), cx);
        set_input(&self.cc, &mail_core::format_address_list(&draft.cc), cx);
        set_input(&self.bcc, &mail_core::format_address_list(&draft.bcc), cx);
        set_input(&self.subject, &draft.subject, cx);
        // `set_content` leaves the caret at the start: above a quoted reply.
        self.body
            .update(cx, |area, cx| area.set_content(&draft.text, cx));
        self.show_cc_bcc = !draft.cc.is_empty() || !draft.bcc.is_empty();
        self.attachments = draft.attachments;
        self.reply = ReplyHeaders {
            in_reply_to: draft.in_reply_to,
            references: draft.references,
            quoted: init.quoted_original,
        };
        self.quote_expanded = false;
        self.quote_view = None;
        self._quote_task = None;
        self.errors = ComposeErrors::default();
        self.from_menu_open = false;
        self.edited = false;
        let handle = match focus {
            ComposeFocus::To => self.to.focus_handle(cx),
            ComposeFocus::Body => self.body.focus_handle(cx),
        };
        window.focus(&handle);
        cx.notify();
    }

    /// Syncs the white-compose preference pushed from the main view.
    pub(crate) fn set_white_background(&mut self, value: bool, cx: &mut Context<Self>) {
        if self.white_background != value {
            self.white_background = value;
            self.quote_view = None;
            self._quote_task = None;
            if self.quote_expanded {
                self.build_quote_view(cx);
            }
            cx.notify();
        }
    }

    fn toggle_quote(&mut self, cx: &mut Context<Self>) {
        self.quote_expanded = !self.quote_expanded;
        if self.quote_expanded && self.quote_view.is_none() {
            self.build_quote_view(cx);
        }
        cx.notify();
    }

    /// Parses the quoted original off the UI thread: it can be a large message.
    fn build_quote_view(&mut self, cx: &mut Context<Self>) {
        let Some(quoted) = self.reply.quoted.clone() else {
            return;
        };
        let theme = cx.theme().colors();
        let colors = if self.white_background {
            DocumentColors {
                background: white(),
                text: black(),
                accent: theme.accent,
            }
        } else {
            DocumentColors {
                background: theme.background,
                text: theme.text,
                accent: theme.accent,
            }
        };
        let build = cx.background_spawn(async move {
            let body = MessageBody::Html(quoted.html.into());
            let rendered = email_document(colors, &body, quoted.load_remote);
            let options = DocumentOptions {
                appearance: if colors.is_dark() {
                    PageAppearance::Dark
                } else {
                    PageAppearance::Light
                },
                resources: Resources::new().allow_remote(quoted.load_remote),
                ..Default::default()
            };
            let mut document = HtmlDocument::parse(&rendered.html, options);
            document.prepare(QUOTE_FIRST_LAYOUT_WIDTH);
            DetachedDocument::new(document)
        });
        self._quote_task = Some(cx.spawn(async move |this, cx| {
            let document = build.await;
            this.update(cx, |this, cx| {
                this.quote_view = Some(cx.new(|cx| HtmlView::new(document.into_inner(), cx)));
                cx.notify();
            })
            .ok();
        }));
    }

    /// Whether the user typed something that replacing the content would lose.
    pub(crate) fn has_edits(&self) -> bool {
        self.edited
    }

    fn mark_edited(&mut self, cx: &mut Context<Self>) {
        self.edited = true;
        cx.notify();
    }

    /// Typing in an address field clears its stale validation error.
    fn on_address_changed(&mut self, field: AddressField, cx: &mut Context<Self>) {
        *self.errors.field_mut(field) = None;
        self.mark_edited(cx);
    }

    /// Toggles visibility of the Cc and Bcc rows.
    fn toggle_cc_bcc(&mut self, cx: &mut Context<Self>) {
        self.show_cc_bcc = !self.show_cc_bcc;
        cx.notify();
    }

    fn toggle_from_menu(&mut self, cx: &mut Context<Self>) {
        self.from_menu_open = !self.from_menu_open && !self.accounts.is_empty();
        cx.notify();
    }

    fn close_from_menu(&mut self, cx: &mut Context<Self>) {
        self.from_menu_open = false;
        cx.notify();
    }

    /// Picks the From account (picker entry).
    fn select_from_account(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.accounts.len() {
            self.from_account = index;
            self.errors.banner = None;
        }
        self.close_from_menu(cx);
    }

    fn selected_account(&self) -> Option<&ComposeAccount> {
        self.accounts.get(self.from_account)
    }

    fn fields(&self, cx: &App) -> ComposeFields {
        ComposeFields {
            to: self.to.read(cx).content().to_string(),
            cc: self.cc.read(cx).content().to_string(),
            bcc: self.bcc.read(cx).content().to_string(),
            subject: self.subject.read(cx).content().to_string(),
            body: self.body.read(cx).content().to_string(),
        }
    }

    fn can_send_now(&self) -> bool {
        self.pending_attachment_loads == 0 && !self.sending
    }

    /// Validates the fields and hands the draft to [`RootView::send_draft`].
    /// The hand-off runs in a task: this is also reached from the global menu
    /// action while the main view is leased, and updating it then would panic.
    /// Success closes the window; failure shows the error banner.
    pub(crate) fn send_message(&mut self, cx: &mut Context<Self>) {
        if !self.can_send_now() {
            return;
        }
        let language = cx.language();
        let fields = self.fields(cx);
        let draft = build_draft(
            self.selected_account(),
            &fields,
            &self.reply,
            &self.attachments,
            language,
        );
        let draft = match draft {
            Ok(draft) => draft,
            Err(errors) => {
                if errors.cc.is_some() || errors.bcc.is_some() {
                    self.show_cc_bcc = true;
                }
                self.errors = errors;
                cx.notify();
                return;
            }
        };
        // `build_draft` only succeeds with a selected account.
        let Some(account_id) = self.selected_account().map(|account| account.id) else {
            return;
        };
        self.errors = ComposeErrors::default();
        self.sending = true;
        cx.notify();
        let root = self.root.clone();
        let window_handle = self.window_handle;
        let replaces = self.replaces_outgoing;
        cx.spawn(async move |this, cx| {
            let result = root
                .update(cx, |root, cx| {
                    root.send_draft(account_id, draft, replaces, cx)
                })
                .unwrap_or_else(|error| Err(error.to_string()));
            let sent = this
                .update(cx, |this, cx| this.finish_send(result, cx))
                .unwrap_or(false);
            if sent {
                let _ = root.update(cx, |root, cx| root.on_compose_window_closed(cx));
                let _ = window_handle.update(cx, |_, window, _| window.remove_window());
            }
        })
        .detach();
    }

    /// Records the outcome of a send; returns whether the window should close.
    fn finish_send(&mut self, result: Result<(), String>, cx: &mut Context<Self>) -> bool {
        self.sending = false;
        let sent = result.is_ok();
        if let Err(message) = result {
            self.errors.banner = Some(message);
        }
        cx.notify();
        sent
    }

    /// Opens the system file picker and attaches the chosen files.
    pub(crate) fn attach_file(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                let _ = this.update(cx, |this, cx| this.attach_paths(paths, cx));
            }
        })
        .detach();
    }

    /// Reads `paths` on a background thread and appends them as attachments
    /// (within [`ATTACHMENT_LIMIT_BYTES`]). Used by the picker and drag-and-drop.
    fn attach_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        self.pending_attachment_loads += 1;
        self.errors.banner = None;
        cx.notify();
        let used = attachments_total(&self.attachments);
        let read =
            cx.background_spawn(
                async move { read_attachments(&paths, used, ATTACHMENT_LIMIT_BYTES) },
            );
        cx.spawn(async move |this, cx| {
            let batch = read.await;
            let _ = this.update(cx, |this, cx| this.finish_attachments(batch, cx));
        })
        .detach();
    }

    /// Merges a finished background read; the cap is re-checked here because
    /// several reads may have run concurrently.
    fn finish_attachments(&mut self, batch: AttachmentBatch, cx: &mut Context<Self>) {
        self.pending_attachment_loads = self.pending_attachment_loads.saturating_sub(1);
        let added = !batch.files.is_empty();
        let rejected =
            merge_attachments(&mut self.attachments, batch.files, ATTACHMENT_LIMIT_BYTES);
        if added {
            self.edited = true;
        }
        let language = cx.language();
        if let Some(message) =
            attachment_error_message(batch.too_large || rejected, &batch.failed, language)
        {
            self.errors.banner = Some(message);
        }
        cx.notify();
    }

    fn remove_attachment(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.attachments.len() {
            self.attachments.remove(index);
            self.mark_edited(cx);
        }
    }

    fn dismiss_error(&mut self, cx: &mut Context<Self>) {
        self.errors.banner = None;
        cx.notify();
    }

    /// Discards the message: closes right away when nothing was edited,
    /// otherwise after the user confirms.
    pub(crate) fn discard_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.edited {
            self.close_window(window, cx);
            return;
        }
        let language = cx.language();
        let answer = window.prompt(
            PromptLevel::Warning,
            Key::ComposeDiscardConfirm.tr(language),
            Some(Key::ComposeDiscardDetail.tr(language)),
            &[
                PromptButton::new(Key::ComposeDiscardButton.tr(language)),
                PromptButton::cancel(Key::ComposeCancelButton.tr(language)),
            ],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await == Ok(0) {
                let _ = this.update_in(cx, |this, window, cx| this.close_window(window, cx));
            }
        })
        .detach();
    }

    /// Closes this compose window and restores the main-window menu bar.
    pub(crate) fn close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let root = self.root.clone();
        window.defer(cx, move |window, cx| {
            if let Some(root) = root.upgrade() {
                root.update(cx, |root, cx| {
                    root.on_compose_window_closed(cx);
                });
            }
            window.remove_window();
        });
    }

    fn sync_menus_if_active(&self, window: &Window, language: Language, cx: &mut Context<Self>) {
        if window.is_window_active() {
            app_menus::sync_compose_menus(cx, language);
        }
    }

    /// Keeps the platform title on the subject (or "New Message").
    fn sync_title(&mut self, window: &mut Window, language: Language, cx: &App) {
        let title = title_for_subject(&self.subject.read(cx).content(), language);
        if title != self.last_title {
            window.set_window_title(&title);
            self.last_title = title;
        }
    }

    fn render_toolbar(&self, language: Language, cx: &mut Context<Self>) -> impl IntoElement {
        let can_send = self.can_send_now();
        h_flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(cx.theme().colors().border)
            .child(
                div()
                    .id("compose-discard")
                    .tooltip(Tooltip::text(Key::ComposeDiscard.tr(language)))
                    .child(
                        IconButton::new("compose-discard", IconName::Trash)
                            .color(Color::Muted)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.discard_draft(window, cx);
                            })),
                    ),
            )
            .child(div().flex_1())
            .when(self.pending_attachment_loads > 0, |el| {
                el.child(
                    Label::new(Key::ComposeAttachLoading.tr(language))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
            })
            .child(
                div()
                    .id("compose-attach")
                    .tooltip(Tooltip::text(Key::ComposeAttach.tr(language)))
                    .child(
                        IconButton::new("compose-attach", IconName::Attachment)
                            .on_click(cx.listener(|this, _, _, cx| this.attach_file(cx))),
                    ),
            )
            .child({
                let button = Button::new("compose-send", Key::ComposeSend.tr(language))
                    .icon(IconName::Send)
                    .style(ButtonStyle::Filled);
                div()
                    .id("compose-send")
                    .tooltip(Tooltip::text(Key::ComposeSend.tr(language)))
                    .when(!can_send, |el| el.opacity(0.5))
                    .child(if can_send {
                        button.on_click(cx.listener(|this, _, _, cx| this.send_message(cx)))
                    } else {
                        button
                    })
            })
    }

    fn render_header(&self, language: Language, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let border = colors.border;
        let field_text = colors.text;
        let field_hover = colors.element_hover;

        let to_row = self.field_row(
            Key::ComposeTo.tr(language),
            self.to.clone().into_any_element(),
            Some(
                self.cc_bcc_toggle(language, field_text, field_hover, cx)
                    .into_any_element(),
            ),
            self.errors.field(AddressField::To),
            border,
        );
        let mut fields = v_flex()
            .px_3()
            .child(
                self.field_row(
                    Key::ComposeFrom.tr(language),
                    self.render_from_picker(language, field_hover, cx)
                        .into_any_element(),
                    None,
                    None,
                    border,
                ),
            )
            .child(to_row);

        if self.show_cc_bcc {
            fields = fields
                .child(self.field_row(
                    Key::ComposeCc.tr(language),
                    self.cc.clone().into_any_element(),
                    None,
                    self.errors.field(AddressField::Cc),
                    border,
                ))
                .child(self.field_row(
                    Key::ComposeBcc.tr(language),
                    self.bcc.clone().into_any_element(),
                    None,
                    self.errors.field(AddressField::Bcc),
                    border,
                ));
        }

        fields.child(self.field_row(
            Key::ComposeSubject.tr(language),
            self.subject.clone().into_any_element(),
            None,
            None,
            border,
        ))
    }

    /// The From value: the chosen account and a chevron; opens the picker.
    fn render_from_picker(
        &self,
        language: Language,
        hover_bg: Hsla,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (label, can_send) = match self.selected_account() {
            Some(account) => (account_label(account), account.can_send),
            None => (Key::ComposeNoAccount.tr(language).to_string(), false),
        };
        v_flex()
            .items_start()
            .child(
                h_flex()
                    .id("compose-from")
                    .gap_1()
                    .px_1()
                    .ml(px(-4.0))
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(move |el| el.bg(hover_bg))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_from_menu(cx)))
                    .child(Label::new(label).size(LabelSize::Small).single_line())
                    .when(!can_send && !self.accounts.is_empty(), |el| {
                        el.child(
                            Label::new(Key::ComposeCannotSend.tr(language))
                                .size(LabelSize::Small)
                                .color(Color::Warning)
                                .single_line(),
                        )
                    })
                    .child(
                        Icon::new(IconName::ChevronDown)
                            .size(IconSize::XSmall)
                            .color(Color::Muted),
                    ),
            )
            .when(self.from_menu_open, |el| {
                el.child(self.render_from_menu(language, cx))
            })
    }

    /// Account picker anchored under the From value. Local accounts are listed
    /// with a "can't send" hint (picking one makes Send explain why).
    fn render_from_menu(&self, language: Language, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.weak_entity();
        let mut menu = ContextMenu::new();
        for (index, account) in self.accounts.iter().enumerate() {
            let view = view.clone();
            let handler = move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                let _ = view.update(cx, |this, cx| this.select_from_account(index, cx));
            };
            let label = account_label(account);
            menu = if account.can_send {
                menu.entry(label, handler)
            } else {
                menu.entry_with_key_hint(label, Key::ComposeCannotSend.tr(language), handler)
            };
        }
        deferred(
            anchored()
                .anchor(Corner::TopLeft)
                .snap_to_window_with_margin(px(8.0))
                .child(div().mt_1().child(menu)),
        )
        .with_priority(1)
    }

    /// Compact Cc/Bcc toggle aligned with the field rows (the default
    /// [`Button`] padding would make the To line taller than the others).
    fn cc_bcc_toggle(
        &self,
        language: Language,
        text_color: Hsla,
        hover_bg: Hsla,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label = Key::ComposeCcBcc.tr(language);
        div()
            .id("compose-cc-bcc")
            .flex_shrink_0()
            .tooltip(Tooltip::text(label))
            .px_1p5()
            .rounded_sm()
            .text_size(ui::ui_text_size(-2.0))
            .text_color(text_color)
            .hover(move |el| el.bg(hover_bg))
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, _, cx| this.toggle_cc_bcc(cx)))
            .child(label)
    }

    /// One labeled row in the compose header (From, To, Subject, …) with an
    /// optional trailing control and an inline error under the value.
    fn field_row(
        &self,
        label: &'static str,
        value: gpui::AnyElement,
        trailing: Option<gpui::AnyElement>,
        error: Option<&str>,
        border: Hsla,
    ) -> impl IntoElement {
        v_flex()
            .py_2()
            .gap_1()
            .border_b_1()
            .border_color(border)
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(FIELD_LABEL_WIDTH))
                            .flex_shrink_0()
                            .child(Label::new(label).size(LabelSize::Small).color(Color::Muted)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(ui::ui_text_size(-1.0))
                            .child(value),
                    )
                    .children(trailing),
            )
            .when_some(error, |el, error| {
                el.child(
                    div().pl(px(FIELD_LABEL_WIDTH + 8.0)).child(
                        Label::new(error.to_string())
                            .size(LabelSize::XSmall)
                            .color(Color::Error),
                    ),
                )
            })
    }

    /// Chips for attachments (file-type icon, name, size, remove), as in the reader.
    fn render_attachments(&self, language: Language, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        h_flex()
            .flex_wrap()
            .gap_1p5()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(colors.border)
            .children(
                self.attachments
                    .iter()
                    .enumerate()
                    .map(|(index, attachment)| {
                        h_flex()
                            .gap_1()
                            .pl_2()
                            .pr_1()
                            .py_0p5()
                            .max_w(px(280.0))
                            .rounded_md()
                            .bg(colors.element_background)
                            .border_1()
                            .border_color(colors.border_variant)
                            .child(
                                Icon::new(
                                    file_family(&attachment.mime_type, &attachment.filename).icon(),
                                )
                                .size(IconSize::XSmall)
                                .color(Color::Muted),
                            )
                            .child(
                                div().min_w_0().child(
                                    Label::new(attachment.filename.clone())
                                        .size(LabelSize::Small)
                                        .single_line(),
                                ),
                            )
                            .child(
                                Label::new(human_size(attachment.data.len() as u64, language))
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted)
                                    .single_line(),
                            )
                            .child(
                                div()
                                    .id(("compose-attachment-remove-tip", index))
                                    .tooltip(Tooltip::text(
                                        Key::ComposeRemoveAttachment.tr(language),
                                    ))
                                    .child(
                                        IconButton::new(
                                            ("compose-attachment-remove", index),
                                            IconName::Clear,
                                        )
                                        .size(IconSize::XSmall)
                                        .color(Color::Muted)
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.remove_attachment(index, cx)
                                            }),
                                        ),
                                    ),
                            )
                    }),
            )
    }

    /// Window-level error (send failure, unreadable / oversized files).
    fn render_error_banner(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let message = self.errors.banner.clone()?;
        let colors = cx.theme().colors();
        Some(
            h_flex()
                .gap_2()
                .px_3()
                .py_1p5()
                .bg(colors.surface_background)
                .border_t_1()
                .border_color(colors.border)
                .child(
                    Icon::new(IconName::Junk)
                        .size(IconSize::Small)
                        .color(Color::Error),
                )
                .child(
                    div().flex_1().min_w_0().child(
                        Label::new(message)
                            .size(LabelSize::Small)
                            .color(Color::Error),
                    ),
                )
                .child(
                    IconButton::new("compose-error-dismiss", IconName::Clear)
                        .size(IconSize::XSmall)
                        .color(Color::Muted)
                        .on_click(cx.listener(|this, _, _, cx| this.dismiss_error(cx))),
                ),
        )
    }

    fn render_body(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (bg, text) = compose_body_colors(self.white_background);
        let body_focus: FocusHandle = self.body.focus_handle(cx);
        div()
            .id("compose-body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.body_scroll)
            .p_3()
            .cursor_text()
            .when_some(bg, |el, bg| el.bg(bg))
            .when_some(text, |el, text| el.text_color(text))
            // Clicks below the text still land in the body.
            .on_mouse_down(MouseButton::Left, move |_, window, _| {
                window.focus(&body_focus);
            })
            .child(self.body.clone())
            .when(self.reply.quoted.is_some(), |el| {
                el.child(self.render_quote(cx))
            })
    }

    /// Below the editor, as in Gmail: a button that reveals the attribution line
    /// and the original message, with its formatting, behind a side bar.
    fn render_quote(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let language = cx.language();
        let border = cx.theme().colors().border;
        let toggle = quote_toggle_colors(self.white_background, cx.theme().colors());
        let attribution = self
            .reply
            .quoted
            .as_ref()
            .map(|quoted| quoted.attribution.clone())
            .unwrap_or_default();
        let height = self
            .quote_view
            .as_ref()
            .map_or(0.0, |view| view.read(cx).content_height());
        v_flex()
            .w_full()
            .mt_4()
            .gap_2()
            .cursor_default()
            // Reading the quote must not move the caret to the editor.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                h_flex().child(
                    h_flex()
                        .id("compose-quote-toggle")
                        .h(px(18.0))
                        .px_2()
                        .justify_center()
                        .rounded_md()
                        .cursor_pointer()
                        .bg(toggle.background)
                        .hover(move |el| el.bg(toggle.hover))
                        .tooltip(Tooltip::text(Key::ComposeToggleQuote.tr(language)))
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_quote(cx)))
                        .child(
                            Icon::new(IconName::More)
                                .size(IconSize::Small)
                                .color(Color::Custom(toggle.icon)),
                        ),
                ),
            )
            .when(self.quote_expanded, |el| {
                el.child(
                    Label::new(attribution)
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .when_some(self.quote_view.clone(), |el, view| {
                    el.child(
                        div()
                            .w_full()
                            .h(px(height))
                            .pl_3()
                            .border_l_1()
                            .border_color(border)
                            .child(view),
                    )
                })
            })
    }
}

/// Colors of the button that reveals the quote.
#[derive(Clone, Copy)]
struct QuoteToggleColors {
    background: Hsla,
    hover: Hsla,
    icon: Hsla,
}

/// The button sits on the page, not on the app: over a white page it takes the
/// light theme's colors, whatever the app's theme is.
fn quote_toggle_colors(white_background: bool, theme: &ThemeColors) -> QuoteToggleColors {
    let light = Theme::light();
    let colors = if white_background {
        light.colors()
    } else {
        theme
    };
    QuoteToggleColors {
        background: colors.element_active,
        hover: colors.element_hover,
        icon: colors.text_muted,
    }
}

/// Background and text colors for the compose body: the white-background
/// preference forces dark text on white, mirroring the reader.
fn compose_body_colors(white_background: bool) -> (Option<Hsla>, Option<Hsla>) {
    if white_background {
        (Some(white()), Some(black()))
    } else {
        (None, None)
    }
}

impl Render for ComposeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let WindowBounds::Windowed(bounds) = window.window_bounds() {
            let origin = bounds.origin;
            let size = bounds.size;
            if self.last_reported_bounds != Some((origin, size)) {
                self.last_reported_bounds = Some((origin, size));
                let root = self.root.clone();
                // Defer so we never update RootView while it is still on the
                // stack (e.g. open_compose runs inside RootView's click handler).
                window.defer(cx, move |_, cx| {
                    let _ = root.update(cx, |root, cx| {
                        root.sync_compose_window_bounds(origin, size, cx);
                    });
                });
            }
        }

        let language = cx.language();
        self.sync_menus_if_active(window, language, cx);
        self.sync_title(window, language, cx);
        let colors = cx.theme().colors();
        let drop_bg = colors.element_hover;
        let has_chips = !self.attachments.is_empty();

        v_flex()
            .relative()
            .size_full()
            .key_context(KEY_CONTEXT)
            .bg(colors.background)
            .text_color(colors.text)
            .font_family("Helvetica")
            .on_action(cx.listener(|this, _: &ComposeSend, _, cx| {
                this.send_message(cx);
            }))
            .on_action(cx.listener(|this, _: &ComposeAttach, _, cx| {
                this.attach_file(cx);
            }))
            .on_action(cx.listener(|this, _: &ComposeDiscard, window, cx| {
                this.discard_draft(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ComposeClose, window, cx| {
                this.close_window(window, cx);
            }))
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(drop_bg))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                this.attach_paths(paths.paths().to_vec(), cx);
            }))
            .child(self.render_header(language, cx))
            .when(has_chips, |el| {
                el.child(self.render_attachments(language, cx))
            })
            .child(self.render_body(cx))
            .children(self.render_error_banner(cx))
            .child(self.render_toolbar(language, cx))
            // Outside clicks close the From picker (the picker itself is
            // deferred, so it paints above this catcher).
            .when(self.from_menu_open, |el| {
                el.child(
                    div()
                        .absolute()
                        .inset_0()
                        .occlude()
                        .on_any_mouse_down(cx.listener(|this, _, _, cx| this.close_from_menu(cx))),
                )
            })
    }
}

/// Binds Cmd/Ctrl+Enter to [`ComposeSend`] in the compose key context, once.
fn ensure_key_bindings(cx: &mut App) {
    if cx.has_global::<ComposeKeysBound>() {
        return;
    }
    cx.bind_keys([
        KeyBinding::new(SEND_MAC, ComposeSend, Some(KEY_CONTEXT)),
        KeyBinding::new(SEND_OTHER, ComposeSend, Some(KEY_CONTEXT)),
    ]);
    cx.set_global(ComposeKeysBound);
}

/// Replaces a single-line editor's text without emitting `Changed`.
fn set_input(input: &Entity<TextInput>, text: &str, cx: &mut App) {
    input.update(cx, |input, cx| input.set_content(text, cx));
}

/// Localized title for the compose window.
pub fn window_title(language: Language) -> &'static str {
    Key::ComposeWindowTitle.tr(language)
}

/// Window title for the current subject: the subject itself, or the localized
/// "New Message" while it is blank.
fn title_for_subject(subject: &str, language: Language) -> String {
    let subject = subject.trim();
    if subject.is_empty() {
        window_title(language).to_string()
    } else {
        subject.to_string()
    }
}

/// Index of the From account: `id` when present among `accounts`, else the
/// first account that can send, else the first one (0 when empty).
fn account_index(accounts: &[ComposeAccount], id: Option<i64>) -> usize {
    id.and_then(|id| accounts.iter().position(|account| account.id == id))
        .or_else(|| accounts.iter().position(|account| account.can_send))
        .unwrap_or(0)
}

/// Picker label: `Name <email>`, or the bare email without a name.
fn account_label(account: &ComposeAccount) -> String {
    let name = account.name.trim();
    if name.is_empty() {
        account.email.clone()
    } else {
        format!("{name} <{}>", account.email)
    }
}

/// Replies (they have recipients or thread headers) start typing in the body,
/// above the quote; new messages and forwards start in To.
fn initial_focus(init: Option<&ComposeInit>) -> ComposeFocus {
    match init {
        Some(init) if init.draft.in_reply_to.is_some() || !init.draft.to.is_empty() => {
            ComposeFocus::Body
        }
        _ => ComposeFocus::To,
    }
}

/// Localized "Invalid address: a, b" for the tokens the parser rejected.
fn invalid_address_message(tokens: &[String], language: Language) -> String {
    format!(
        "{}: {}",
        Key::ComposeInvalidAddress.tr(language),
        tokens.join(", ")
    )
}

/// Parses one recipient field; invalid tokens become its inline error.
fn parse_field(input: &str, language: Language) -> (Vec<Address>, Option<String>) {
    match mail_core::parse_address_list(input) {
        Ok(addresses) => (addresses, None),
        Err(tokens) => (Vec::new(), Some(invalid_address_message(&tokens, language))),
    }
}

/// Validates the fields and builds the outgoing draft (plain text). Errors:
/// invalid addresses per field, then "no recipients", then a missing or
/// local (can't send) From account.
/// HTML of a reply: what was written, and the original with its formatting.
fn reply_html(reply: &ReplyHeaders, body: &str) -> String {
    match &reply.quoted {
        Some(quoted) => {
            mail_core::reply_html_with_original(body, &quoted.attribution, &quoted.html)
        }
        None => mail_core::reply_html(body),
    }
}

/// Plain-text version of a message: what was written, and the `> ` quote.
fn reply_text(reply: &ReplyHeaders, body: &str) -> String {
    match &reply.quoted {
        Some(quoted) => format!("{}\n\n{}", body.trim_end(), quoted.block),
        None => body.to_string(),
    }
}

fn build_draft(
    from: Option<&ComposeAccount>,
    fields: &ComposeFields,
    reply: &ReplyHeaders,
    attachments: &[OutgoingAttachment],
    language: Language,
) -> Result<Draft, ComposeErrors> {
    let (to, to_error) = parse_field(&fields.to, language);
    let (cc, cc_error) = parse_field(&fields.cc, language);
    let (bcc, bcc_error) = parse_field(&fields.bcc, language);
    let mut errors = ComposeErrors {
        to: to_error,
        cc: cc_error,
        bcc: bcc_error,
        banner: None,
    };
    if errors.has_field_errors() {
        return Err(errors);
    }
    if to.is_empty() && cc.is_empty() && bcc.is_empty() {
        errors.banner = Some(Key::ComposeNoRecipients.tr(language).to_string());
        return Err(errors);
    }
    let Some(account) = from.filter(|account| account.can_send) else {
        errors.banner = Some(Key::ComposeLocalAccount.tr(language).to_string());
        return Err(errors);
    };
    Ok(Draft {
        from: Address::new(Some(account.name.clone()), account.email.clone()),
        to,
        cc,
        bcc,
        subject: fields.subject.trim().to_string(),
        text: reply_text(reply, &fields.body),
        // Same structure as Gmail's replies, which it collapses in conversations.
        html: reply
            .in_reply_to
            .is_some()
            .then(|| reply_html(reply, &fields.body)),
        in_reply_to: reply.in_reply_to.clone(),
        references: reply.references.clone(),
        attachments: attachments.to_vec(),
    })
}

/// Total bytes of `attachments`.
fn attachments_total(attachments: &[OutgoingAttachment]) -> usize {
    attachments.iter().map(|a| a.data.len()).sum()
}

/// File name shown on the chip and sent in the MIME part.
fn attachment_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// Reads `paths` (blocking; run on a background thread). `used` bytes are
/// already attached; files that would push the total over `cap` are skipped
/// (checked from metadata first, so huge files are never read).
fn read_attachments(paths: &[PathBuf], mut used: usize, cap: usize) -> AttachmentBatch {
    let mut batch = AttachmentBatch::default();
    for path in paths {
        let name = attachment_name(path);
        let len = match std::fs::metadata(path) {
            Ok(meta) if meta.is_file() => usize::try_from(meta.len()).unwrap_or(usize::MAX),
            _ => {
                batch.failed.push(name);
                continue;
            }
        };
        if used.saturating_add(len) > cap {
            batch.too_large = true;
            continue;
        }
        let Ok(data) = std::fs::read(path) else {
            batch.failed.push(name);
            continue;
        };
        // The file may have grown since `metadata`.
        if used.saturating_add(data.len()) > cap {
            batch.too_large = true;
            continue;
        }
        used += data.len();
        batch.files.push(OutgoingAttachment {
            mime_type: mail_core::guess_mime_type(&name).to_string(),
            filename: name,
            data,
        });
    }
    batch
}

/// Appends `files` to `existing` while the total stays within `cap`. Returns
/// whether any file was rejected.
fn merge_attachments(
    existing: &mut Vec<OutgoingAttachment>,
    files: Vec<OutgoingAttachment>,
    cap: usize,
) -> bool {
    let mut total = attachments_total(existing);
    let mut rejected = false;
    for file in files {
        if total.saturating_add(file.data.len()) > cap {
            rejected = true;
            continue;
        }
        total += file.data.len();
        existing.push(file);
    }
    rejected
}

/// Banner text after attaching: the size-limit notice and/or the unreadable
/// files. `None` when everything was attached.
fn attachment_error_message(
    too_large: bool,
    failed: &[String],
    language: Language,
) -> Option<String> {
    let mut parts = Vec::new();
    if too_large {
        parts.push(Key::ComposeAttachTooLarge.tr(language).to_string());
    }
    if !failed.is_empty() {
        parts.push(format!(
            "{}: {}",
            Key::ComposeAttachReadFailed.tr(language),
            failed.join(", ")
        ));
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::point;

    fn account(id: i64, can_send: bool) -> ComposeAccount {
        ComposeAccount {
            id,
            name: format!("User {id}"),
            email: format!("user{id}@example.com"),
            can_send,
        }
    }

    fn fields(to: &str) -> ComposeFields {
        ComposeFields {
            to: to.into(),
            subject: "  Hello  ".into(),
            body: "Body\n> quote".into(),
            ..Default::default()
        }
    }

    fn attachment(name: &str, len: usize) -> OutgoingAttachment {
        OutgoingAttachment {
            filename: name.into(),
            mime_type: mail_core::guess_mime_type(name).into(),
            data: vec![0; len],
        }
    }

    #[test]
    fn compose_body_uses_white_background_when_reader_pref_is_on() {
        let (bg, text) = compose_body_colors(true);
        assert_eq!(bg, Some(white()));
        assert_eq!(text, Some(black()));
        assert_eq!(compose_body_colors(false), (None, None));
    }

    #[test]
    fn clamp_compose_size_enforces_minimums() {
        let clamped = clamp_compose_size(size(px(100.0), px(200.0)));
        assert_eq!(f32::from(clamped.width), COMPOSE_MIN_WIDTH);
        assert_eq!(f32::from(clamped.height), COMPOSE_MIN_HEIGHT);
    }

    #[test]
    fn clamp_compose_size_leaves_large_sizes_unchanged() {
        let clamped = clamp_compose_size(size(px(900.0), px(800.0)));
        assert_eq!(f32::from(clamped.width), 900.0);
        assert_eq!(f32::from(clamped.height), 800.0);
    }

    #[test]
    fn compose_position_unset_detects_negative_coordinates() {
        assert!(compose_position_is_unset(point(
            px(COMPOSE_POSITION_UNSET),
            px(0.0)
        )));
        assert!(compose_position_is_unset(point(px(0.0), px(-1.0))));
        assert!(!compose_position_is_unset(point(px(0.0), px(0.0))));
    }

    #[test]
    fn window_title_is_localized() {
        assert_eq!(window_title(Language::English), "New Message");
        assert_eq!(window_title(Language::Portuguese), "Nova mensagem");
    }

    #[test]
    fn title_follows_subject_or_falls_back() {
        assert_eq!(
            title_for_subject("  Re: Lunch ", Language::English),
            "Re: Lunch"
        );
        assert_eq!(title_for_subject("   ", Language::English), "New Message");
        assert_eq!(title_for_subject("", Language::Portuguese), "Nova mensagem");
    }

    #[test]
    fn send_shortcuts_parse() {
        assert!(gpui::Keystroke::parse(SEND_MAC).is_ok());
        assert!(gpui::Keystroke::parse(SEND_OTHER).is_ok());
    }

    #[test]
    fn account_index_prefers_requested_then_sending_account() {
        let accounts = vec![account(1, false), account(2, true), account(3, true)];
        assert_eq!(account_index(&accounts, Some(3)), 2);
        assert_eq!(account_index(&accounts, Some(1)), 0);
        assert_eq!(account_index(&accounts, Some(99)), 1);
        assert_eq!(account_index(&accounts, None), 1);
        assert_eq!(account_index(&[account(1, false)], None), 0);
        assert_eq!(account_index(&[], Some(1)), 0);
    }

    #[test]
    fn account_label_shows_name_and_email() {
        assert_eq!(
            account_label(&account(7, true)),
            "User 7 <user7@example.com>"
        );
        let mut nameless = account(8, true);
        nameless.name = "  ".into();
        assert_eq!(account_label(&nameless), "user8@example.com");
    }

    #[test]
    fn initial_focus_is_body_for_replies_and_to_otherwise() {
        assert_eq!(initial_focus(None), ComposeFocus::To);
        let mut init = ComposeInit::default();
        assert_eq!(initial_focus(Some(&init)), ComposeFocus::To);
        init.draft.to = vec![Address::new(None::<String>, "a@b.c")];
        assert_eq!(initial_focus(Some(&init)), ComposeFocus::Body);
        init.draft.to.clear();
        init.draft.in_reply_to = Some("id@x".into());
        assert_eq!(initial_focus(Some(&init)), ComposeFocus::Body);
    }

    #[test]
    fn build_draft_maps_fields_and_reply_headers() {
        let from = account(1, true);
        let mut input = fields("Ann <ann@example.com>, bob@example.com");
        input.bcc = "secret@example.com".into();
        let reply = ReplyHeaders {
            in_reply_to: Some("parent@x".into()),
            references: vec!["root@x".into(), "parent@x".into()],
            quoted: None,
        };
        let files = vec![attachment("a.pdf", 3)];
        let draft = build_draft(Some(&from), &input, &reply, &files, Language::English).unwrap();
        assert_eq!(
            draft.from,
            Address::new(Some("User 1"), "user1@example.com")
        );
        assert_eq!(draft.to.len(), 2);
        assert_eq!(draft.to[0], Address::new(Some("Ann"), "ann@example.com"));
        assert!(draft.cc.is_empty());
        assert_eq!(draft.bcc[0].email, "secret@example.com");
        assert_eq!(draft.subject, "Hello");
        assert_eq!(draft.text, "Body\n> quote");
        assert_eq!(draft.in_reply_to.as_deref(), Some("parent@x"));
        assert_eq!(
            draft.html.as_deref(),
            Some(mail_core::reply_html(&draft.text).as_str())
        );
        assert_eq!(draft.references, reply.references);
        assert_eq!(draft.attachments, files);
    }

    #[test]
    fn a_reply_is_sent_with_the_quote_kept_out_of_the_editor() {
        let reply = ReplyHeaders {
            in_reply_to: Some("parent@x".into()),
            quoted: Some(QuotedOriginal {
                block: "Ann wrote:\n> Hello".into(),
                attribution: "Ann wrote:".into(),
                html: "<b>Hello</b>".into(),
                load_remote: false,
            }),
            ..Default::default()
        };
        assert_eq!(reply_text(&reply, "Hi\n\n"), "Hi\n\nAnn wrote:\n> Hello");
        let html = reply_html(&reply, "Hi");
        assert!(html.contains("<b>Hello</b>") && html.contains("Hi"));

        let plain = ReplyHeaders::default();
        assert_eq!(reply_text(&plain, "Hi\n"), "Hi\n");
    }

    #[test]
    fn quote_toggle_follows_the_page_not_the_app_theme() {
        let dark = Theme::dark();
        let on_white = quote_toggle_colors(true, dark.colors());
        assert!(on_white.background.l > 0.7, "{:?}", on_white.background);
        assert!(on_white.icon.l < 0.5, "{:?}", on_white.icon);
        let on_dark = quote_toggle_colors(false, dark.colors());
        assert_eq!(on_dark.background, dark.colors().element_active);
    }

    #[test]
    fn build_draft_reports_invalid_tokens_per_field() {
        let mut input = fields("ok@example.com, nope");
        input.cc = "also bad".into();
        let errors = build_draft(
            Some(&account(1, true)),
            &input,
            &ReplyHeaders::default(),
            &[],
            Language::English,
        )
        .unwrap_err();
        assert_eq!(errors.to.as_deref(), Some("Invalid address: nope"));
        assert_eq!(errors.cc.as_deref(), Some("Invalid address: also bad"));
        assert_eq!(errors.bcc, None);
        assert_eq!(errors.banner, None);
        assert_eq!(errors.field(AddressField::Cc), errors.cc.as_deref());
    }

    #[test]
    fn build_draft_requires_a_recipient_and_a_sending_account() {
        let none = build_draft(
            Some(&account(1, true)),
            &fields("  "),
            &ReplyHeaders::default(),
            &[],
            Language::English,
        )
        .unwrap_err();
        assert_eq!(none.banner.as_deref(), Some("Add at least one recipient."));
        assert!(!none.has_field_errors());

        let local = Key::ComposeLocalAccount.tr(Language::English);
        for from in [Some(account(1, false)), None] {
            let errors = build_draft(
                from.as_ref(),
                &fields("a@example.com"),
                &ReplyHeaders::default(),
                &[],
                Language::English,
            )
            .unwrap_err();
            assert_eq!(errors.banner.as_deref(), Some(local));
        }
    }

    #[test]
    fn compose_errors_field_mut_targets_the_field() {
        let mut errors = ComposeErrors::default();
        *errors.field_mut(AddressField::Bcc) = Some("x".into());
        assert_eq!(errors.bcc.as_deref(), Some("x"));
        assert!(errors.has_field_errors());
        *errors.field_mut(AddressField::Bcc) = None;
        assert!(!errors.has_field_errors());
    }

    #[test]
    fn invalid_address_message_is_localized() {
        let tokens = vec!["a".to_string(), "b".to_string()];
        assert_eq!(
            invalid_address_message(&tokens, Language::Portuguese),
            "Endereço inválido: a, b"
        );
    }

    #[test]
    fn read_attachments_reads_files_and_reports_failures() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("note.TXT");
        let big = dir.path().join("big.pdf");
        std::fs::write(&small, b"hello").unwrap();
        std::fs::write(&big, vec![1u8; 20]).unwrap();
        let missing = dir.path().join("missing.png");
        let paths = vec![
            small.clone(),
            missing,
            dir.path().to_path_buf(),
            big.clone(),
        ];
        let batch = read_attachments(&paths, 0, 16);
        assert_eq!(batch.files.len(), 1);
        assert_eq!(batch.files[0].filename, "note.TXT");
        assert_eq!(batch.files[0].mime_type, "text/plain");
        assert_eq!(batch.files[0].data, b"hello");
        assert!(batch.too_large);
        assert_eq!(batch.failed.len(), 2);
        assert_eq!(batch.failed[0], "missing.png");

        // Already-attached bytes count toward the cap.
        let batch = read_attachments(&[small], 12, 16);
        assert!(batch.files.is_empty());
        assert!(batch.too_large);
    }

    #[test]
    fn merge_attachments_enforces_cap() {
        let mut existing = vec![attachment("a", 10)];
        let rejected = merge_attachments(
            &mut existing,
            vec![attachment("b", 5), attachment("c", 6), attachment("d", 1)],
            16,
        );
        assert!(rejected);
        let names: Vec<&str> = existing.iter().map(|a| a.filename.as_str()).collect();
        assert_eq!(names, ["a", "b", "d"]);
        assert_eq!(attachments_total(&existing), 16);
        assert!(!merge_attachments(&mut existing, Vec::new(), 16));
    }

    #[test]
    fn attachment_name_uses_file_name() {
        assert_eq!(
            attachment_name(Path::new("/tmp/x/report.pdf")),
            "report.pdf"
        );
        assert_eq!(attachment_name(Path::new("/")), "/");
    }

    #[test]
    fn attachment_error_message_combines_parts() {
        assert_eq!(
            attachment_error_message(false, &[], Language::English),
            None
        );
        assert_eq!(
            attachment_error_message(true, &[], Language::English).as_deref(),
            Some("Attachments are limited to 25 MB in total.")
        );
        let failed = vec!["a.txt".to_string()];
        assert_eq!(
            attachment_error_message(true, &failed, Language::English).as_deref(),
            Some("Attachments are limited to 25 MB in total. Couldn't read the file: a.txt")
        );
    }

    #[test]
    fn attachment_limit_is_25_mb() {
        assert_eq!(ATTACHMENT_LIMIT_BYTES, 26_214_400);
    }
}
