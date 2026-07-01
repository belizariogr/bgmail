//! Root view of the rMail visual prototype.
//!
//! Reproduces the macOS mail client layout (three columns, a unified top toolbar
//! and a status bar), using the components from the `ui` crate. All interaction
//! here is "mock" — only item selection, theme toggling and language switching.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use std::f32::consts::FRAC_PI_2;

use gpui::{
    anchored, canvas, deferred, ease_in_out, point, radians, size, svg, Animation,
    AnimationExt as _, AppContext as _, Bounds, ClickEvent, ClipboardItem, Context, Corner,
    DragMoveEvent, Empty, Entity, Focusable, FontWeight, Hsla, MouseButton, MouseDownEvent,
    MouseMoveEvent, Point, ScrollHandle, Size, Svg, TitlebarOptions, Transformation, WeakEntity,
    Window, WindowBounds, WindowControlArea, WindowHandle, WindowOptions,
};
use theme::{ActiveTheme, Appearance};
use ui::prelude::*;
use ui::{
    set_native_text_focus, Button, ButtonStyle, Color, Icon, IconButton, IconName, IconSize, Label,
    LabelSize, ListItem, Scrollbar, ScrollbarState, Switch, TextInput, Tooltip,
};

use crate::compose::{self, ComposeView};
use crate::config::{self, Config};
use crate::data::{self, GlobalMailbox, MailboxKind};
use crate::db_seed;
use crate::locale::{self, ActiveLanguage, Key, Language};
use crate::web_view::{
    email_document, ContextMenuLabels, DocumentColors, EmailWebView, HostEvent, WEBVIEW_SUPPORTED,
};
use storage::{MailListQuery, MessageDetail, MessageListItem};

/// Minimum width of the accounts/folders sidebar, in pixels.
const SIDEBAR_MIN_WIDTH: f32 = 150.0;
/// Minimum width of the message list, in pixels.
const LIST_MIN_WIDTH: f32 = 350.0;
/// Minimum width reserved for the reading pane (e-mail content), in pixels.
const READER_MIN_WIDTH: f32 = 550.0;
/// Minimum window width, in pixels. Kept above `LIST_MIN_WIDTH +
/// READER_MIN_WIDTH + RESIZE_HANDLE_WIDTH` so that, even with the sidebar
/// collapsed, the docked list + reader can't shrink the titlebar enough to clip
/// the window controls (e.g. the close button).
pub(crate) const WINDOW_MIN_WIDTH: f32 = 950.0;
/// Minimum window height, in pixels.
pub(crate) const WINDOW_MIN_HEIGHT: f32 = 480.0;
/// Below this window width the sidebar collapses automatically and, once
/// reopened, floats over the content instead of pushing the columns.
const NARROW_BREAKPOINT: f32 = 1200.0;
/// Hit area (width) of the draggable divider between two columns, in pixels.
const RESIZE_HANDLE_WIDTH: f32 = 6.0;
/// Duration of the sidebar fold (collapse/expand) animation, in milliseconds.
/// It is fixed (not proportional to the number of rows), so a long list folds in
/// the same time as a short one.
const FOLD_ANIM_MS: u64 = 90;
/// Fixed height of a sidebar row (account header and mailbox rows alike), in
/// pixels. Pinning it keeps the fold (accordion) animation's height math exact,
/// so the list never snaps at the end of the transition.
const SIDEBAR_ROW_HEIGHT: f32 = 32.0;
/// Fixed square footprint of the account header's disclosure chevron, in pixels.
/// Keeping the box constant stops the header from shifting as the chevron rotates.
const CHEVRON_BOX: f32 = 16.0;
/// Rendered size of the chevron glyph itself, in pixels.
const CHEVRON_ICON: f32 = 12.0;
/// Left indentation applied to every mailbox/folder row (i.e. everything that
/// isn't an account header), so the rows read as nested under their account and
/// the sidebar gains a tree-like feel. This indents the whole row (highlight
/// included).
const ITEM_INDENT: f32 = 16.0;
/// Extra left padding inside each sidebar row, nudging its icon/text to the right
/// while the hover/selection highlight keeps filling the full row.
const ITEM_PADDING: f32 = 4.0;
/// Distance (pixels) the cursor must travel after a toolbar mouse-down before it
/// counts as a window drag. Kept small so dragging feels immediate (a maximized
/// window restores almost at once), while still large enough that a plain click —
/// which barely moves — never starts a move / un-maximizes.
const DRAG_THRESHOLD: f32 = 2.0;
/// Width of the expanded search field in the toolbar, in pixels.
const SEARCH_FIELD_WIDTH: f32 = 220.0;
/// Delay after typing before the message list applies the search query.
const SEARCH_DEBOUNCE_MS: u64 = 150;
/// Body text color used when the reader is forced onto a white background. A near
/// black (not pure) for comfortable contrast on white, independent of the theme.
/// Shared with the compose body when the same preference is enabled.
pub(crate) const READER_LIGHT_TEXT: Hsla = Hsla {
    h: 0.0,
    s: 0.0,
    l: 0.13,
    a: 1.0,
};
/// Width of the collapsed search button (icon only), in pixels.
const SEARCH_ICON_WIDTH: f32 = 28.0;
/// When the container that holds the reader's toolbar buttons (compose, action
/// groups and search) is narrower than this, the search field collapses into a
/// single magnifying-glass button. Tweak this to fine-tune the breakpoint.
const SEARCH_COLLAPSE_WIDTH: f32 = 700.0;
/// Fixed reader-segment overhead that always precedes the action groups:
/// horizontal padding, inter-child gaps and the compose button.
const TOOLBAR_FIXED_OVERHEAD: f32 = 100.0;
/// Cumulative widths (incl. separators/gaps) of the centered action groups, used
/// to decide how many fit alongside the always-visible search button. Groups are
/// dropped from the right (flag/move first) so search is never overlapped.
const ACTIONS_1_WIDTH: f32 = 92.0; // reply / reply-all / forward
const ACTIONS_2_WIDTH: f32 = 193.0; // + trash / archive / junk
const ACTIONS_3_WIDTH: f32 = 290.0; // + flag / move

/// Which inter-column divider is currently being dragged.
#[derive(Debug, Clone, Copy)]
enum ResizeHandle {
    /// Divider between the sidebar and the message list.
    Sidebar,
    /// Divider between the message list and the reading pane.
    List,
}

/// Drag payload used to resize a column by dragging its right-edge handle.
#[derive(Debug, Clone, Copy)]
struct ResizeDrag(ResizeHandle);

/// What is currently selected in the sidebar: a global (unified) mailbox at the
/// top, or a specific folder within an account group.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Selection {
    /// A unified mailbox spanning all accounts.
    Global(GlobalMailbox),
    /// A folder within an account (`account_id` + storage path, e.g. `sys:inbox`).
    Mailbox {
        account_id: i64,
        folder_path: String,
    },
}

/// Builds the SQLite list query for the current sidebar selection or search.
fn mail_list_query(selection: &Selection, search: &str) -> MailListQuery {
    if !search.trim().is_empty() {
        MailListQuery::Search(search.to_string())
    } else {
        match selection {
            Selection::Global(global) => {
                MailListQuery::GlobalSystemFolder(db_seed::global_folder_path(*global).to_string())
            }
            Selection::Mailbox {
                account_id,
                folder_path,
            } => MailListQuery::AccountFolder {
                account_id: *account_id,
                folder_path: folder_path.clone(),
            },
        }
    }
}

/// An in-flight fold (collapse/expand) animation for one sidebar account.
#[derive(Debug, Clone, Copy)]
struct FoldAnim {
    /// True while collapsing (animating closed); false while expanding.
    collapsing: bool,
    /// Unique token used to (re)key the GPUI animation so it replays on every
    /// toggle, and to let a stale finalize timer detect a newer toggle.
    token: u64,
}

/// Active section of the settings screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsSection {
    General,
    Accounts,
    Appearance,
    Notifications,
    Privacy,
}

impl SettingsSection {
    const ALL: [SettingsSection; 5] = [
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

fn mailbox_icon(kind: MailboxKind) -> IconName {
    match kind {
        MailboxKind::Inbox => IconName::Inbox,
        MailboxKind::Drafts => IconName::Drafts,
        MailboxKind::Sent => IconName::Sent,
        MailboxKind::Junk => IconName::Junk,
        MailboxKind::Trash => IconName::Trash,
        MailboxKind::Archive => IconName::Archive,
        MailboxKind::Custom => IconName::Folder,
    }
}

fn global_mailbox_icon(kind: GlobalMailbox) -> IconName {
    match kind {
        GlobalMailbox::Inbox => IconName::Inbox,
        GlobalMailbox::Flagged => IconName::Flag,
        GlobalMailbox::Drafts => IconName::Drafts,
        GlobalMailbox::Sent => IconName::Sent,
    }
}

/// A right-pointing chevron rendered from an SVG (so it can be rotated, unlike
/// the font-glyph icons) at the given color, rotated `angle` radians about its
/// center. 0 points right (collapsed); `FRAC_PI_2` points down (expanded).
fn chevron_svg(color: Hsla, angle: f32) -> Svg {
    svg()
        .w(px(CHEVRON_ICON))
        .h(px(CHEVRON_ICON))
        .text_color(color)
        .path(ui::CHEVRON_RIGHT)
        .with_transformation(Transformation::rotate(radians(angle)))
}

/// Memoization key for the reader webview: the set of inputs that affect the
/// rendered HTML document. When unchanged between renders (e.g. across animation
/// frames) the expensive HTML rebuild + diff is skipped.
#[derive(Clone, Copy, PartialEq)]
struct WebviewSignature {
    selected: i64,
    colors: DocumentColors,
    language: Language,
    load_remote: bool,
    reader_white_background: bool,
}

/// Application state.
pub struct RootView {
    db: storage::Database,
    accounts: Vec<storage::Account>,
    /// Folders keyed by account id (loaded from SQLite).
    folders_by_account: std::collections::HashMap<i64, Vec<storage::Folder>>,
    /// Messages currently shown in the list column.
    list_messages: Vec<MessageListItem>,
    /// Full body of the selected message (for the reader).
    selected_detail: Option<MessageDetail>,
    /// Current sidebar selection (a global mailbox or an account folder).
    selected_mailbox: Selection,
    /// Indices of the accounts whose mailbox list is collapsed (target state).
    collapsed_accounts: HashSet<usize>,
    /// Fold animations currently playing, keyed by account index. An entry is
    /// present only while the collapse/expand transition is in flight.
    fold_anim: HashMap<usize, FoldAnim>,
    /// Monotonic counter handing out unique [`FoldAnim`] tokens.
    fold_seq: u64,
    /// Id of the message selected in the list.
    selected_message_id: Option<i64>,
    /// Whether the accounts sidebar is visible (toggled from the toolbar).
    show_sidebar: bool,
    /// User-adjustable width of the accounts/folders sidebar.
    sidebar_width: Pixels,
    /// User-adjustable width of the message list.
    list_width: Pixels,
    /// Whether the window is currently below `NARROW_BREAKPOINT`. When narrow the
    /// sidebar is collapsed and, if reopened, floats over the content.
    narrow: bool,
    /// Live window width, tracked at render time. Used to decide toolbar layout
    /// (e.g. collapsing the search field into an icon when space is tight).
    window_width: Pixels,
    /// Last observed windowed (non-maximized) position and size of the main
    /// window, tracked at render time and persisted so it reopens where/how the
    /// user left it. While maximized these keep the pre-maximize values (the
    /// restore target), so we never persist the full-screen frame.
    window_origin: Point<Pixels>,
    window_size: Size<Pixels>,
    /// Whether the window is currently maximized (macOS: zoomed). Persisted so the
    /// app reopens maximized.
    maximized: bool,
    /// Last observed maximized frame. Persisted so the window can open directly at
    /// this size when reopening maximized (avoids the macOS zoom flicker).
    max_origin: Point<Pixels>,
    max_size: Size<Pixels>,
    /// Monotonic counter handing out tokens for the debounced settings save, so a
    /// burst of changes (e.g. a live drag) only writes the file once it settles.
    save_seq: u64,
    /// Gate that suppresses persistence during startup. It is flipped on shortly
    /// after launch, once the initial window state (including a restored maximize)
    /// has settled, so the opening sequence never overwrites the saved sizes.
    persist_ready: bool,
    /// Gate that holds the UI behind a plain background until the window has
    /// reached its final size. On Windows the maximize-on-open is asynchronous and
    /// the `WM_SIZE` that grows GPUI's cached viewport can be lost during the busy
    /// open sequence, so laying out immediately would size the UI to the small
    /// base viewport (leaving the maximized window mostly empty). We start `false`
    /// there and flip it once the viewport matches the real window size; every
    /// other platform starts ready.
    content_ready: bool,
    /// Set on mouse-down in the draggable toolbar; the actual window move only
    /// starts once the cursor leaves a small threshold around [`Self::drag_start`],
    /// so plain clicks (and their sub-pixel jitter) never start a drag — which
    /// matters because starting a drag also un-maximizes the window.
    should_move: bool,
    /// Cursor position captured on the toolbar mouse-down, used as the origin for
    /// the drag threshold.
    drag_start: Point<Pixels>,
    /// Window top-left captured on the toolbar mouse-down. If the window's origin
    /// later differs, the window has actually moved (e.g. a slow native title-bar
    /// drag where the cursor barely moves relative to the window), which also
    /// triggers the drag/un-maximize even if the cursor threshold wasn't reached.
    drag_anchor: Point<Pixels>,
    /// Destination URL of the link currently under the cursor in the reader's
    /// webview (empty when none). Shown centered in the status bar.
    hovered_link: String,
    /// Handle to the separate settings window while it is open (like Zed, the
    /// preferences live in their own window). `None` when it has never been
    /// opened or was closed.
    settings_window: Option<WindowHandle<SettingsView>>,
    /// Handle to the separate compose window while it is open. `None` when it
    /// has never been opened or was closed.
    compose_window: Option<WindowHandle<ComposeView>>,
    /// Last observed position and size of the compose window, persisted so it
    /// reopens where/how the user left it.
    compose_origin: Point<Pixels>,
    compose_size: Size<Pixels>,
    /// Scroll handle + scrollbar state for the message list.
    list_scroll: ScrollHandle,
    list_scrollbar: Option<Entity<ScrollbarState>>,
    /// Scroll handle + scrollbar state for the sidebar.
    sidebar_scroll: ScrollHandle,
    sidebar_scrollbar: Option<Entity<ScrollbarState>>,
    /// Native webview that renders the selected message's HTML body. Scrolling,
    /// text selection and copy are handled by the OS engine. `None` on targets
    /// without a webview backend (Linux) or until it has been created.
    email_webview: Option<EmailWebView>,
    /// Inputs that last produced the webview document (selected message index and
    /// the theme colors it was themed with). Used to skip rebuilding the HTML on
    /// every render — e.g. on every frame of an animation — when nothing that
    /// affects the document has changed.
    last_webview_sig: Option<WebviewSignature>,
    /// Whether to load remote content (e.g. images) in e-mail bodies. Persisted;
    /// off by default to block tracking pixels until the user opts in.
    load_remote_images: bool,
    /// Whether the reader always paints the message body on a white background
    /// (dark text), ignoring the app theme. Persisted; off by default.
    reader_white_background: bool,
    /// Whether the compose message body uses a white background. Persisted; off
    /// by default.
    compose_white_background: bool,
    /// Whether the currently displayed message contains remote images. Drives
    /// whether the privacy shield appears. Recomputed when the document rebuilds.
    content_has_remote: bool,
    /// How many remote images are still blocked in the current message. Red
    /// shield while non-zero, green once it reaches zero. Recomputed on rebuild
    /// and decremented as the user reveals individual images.
    content_blocked_count: usize,
    /// Indices of messages the user fully unblocked via the shield menu.
    /// Unblocking is per message (not the global setting).
    unblocked_messages: HashSet<i64>,
    /// Per message, the remote image URLs the user revealed one at a time (via
    /// the in-body "Show remote image" menu). Kept so those images stay shown
    /// across re-renders (theme/language switches, reselecting the message).
    shown_images: HashMap<i64, HashSet<String>>,
    /// Whether the privacy menu (offering to unblock remote content) is open,
    /// and the window-space anchor where it was summoned.
    privacy_menu_open: bool,
    privacy_menu_pos: Point<Pixels>,
    /// Toolbar search field. Created lazily on first render.
    search_input: Option<Entity<TextInput>>,
    /// When the toolbar is narrow the search field collapses to an icon; this
    /// flag keeps it expanded after the user clicks that icon.
    search_force_expanded: bool,
    /// Last search query applied to the message list.
    last_search_query: String,
    /// Token for the delayed search application. New keystrokes invalidate older
    /// timers so only the latest query reloads the list.
    search_debounce_seq: u64,
    /// Sidebar selection restored when the search box is cleared.
    pre_search_mailbox: Option<Selection>,
}

/// Adapts stored accounts for compose/settings views that still use [`data::Account`].
fn ui_accounts(accounts: &[storage::Account]) -> Vec<data::Account> {
    accounts
        .iter()
        .map(|account| data::Account {
            name: account.name.clone().into(),
            email: account.email.clone().into(),
            mailboxes: Vec::new(),
        })
        .collect()
}

impl RootView {
    pub fn new(settings: Config) -> Self {
        Self::new_with_database(settings, storage::database_path())
    }

    fn new_with_database(settings: Config, db_path: impl AsRef<std::path::Path>) -> Self {
        // Restore persisted column widths, clamped to their minimums so a stale
        // config can't produce an unusable layout.
        let sidebar_width = px(settings.sidebar_width.max(SIDEBAR_MIN_WIDTH));
        let list_width = px(settings.list_width.max(LIST_MIN_WIDTH));
        let window_origin = point(px(settings.window_x), px(settings.window_y));
        let window_size = size(
            px(settings.window_width.max(WINDOW_MIN_WIDTH)),
            px(settings.window_height.max(WINDOW_MIN_HEIGHT)),
        );
        let max_origin = point(px(settings.max_x), px(settings.max_y));
        let max_size = size(px(settings.max_width), px(settings.max_height));
        let compose_origin = point(px(settings.compose_x), px(settings.compose_y));
        let compose_size = size(
            px(settings.compose_width.max(compose::COMPOSE_MIN_WIDTH)),
            px(settings.compose_height.max(compose::COMPOSE_MIN_HEIGHT)),
        );
        let db = storage::Database::open(db_path).expect("failed to open mail database");
        storage::seed_if_empty(
            db.conn(),
            &db_seed::seed_accounts(),
            &db_seed::seed_messages(),
        )
        .expect("failed to seed mail database");

        let accounts = db.list_accounts().expect("failed to load accounts");
        let mut folders_by_account = HashMap::new();
        for account in &accounts {
            folders_by_account.insert(
                account.id,
                db.list_folders(account.id).expect("failed to load folders"),
            );
        }

        let selected_mailbox = Selection::Global(GlobalMailbox::Inbox);
        let list_messages = db
            .list_messages(&mail_list_query(&selected_mailbox, ""))
            .expect("failed to load messages");
        let selected_message_id = list_messages
            .get(3.min(list_messages.len().saturating_sub(1)))
            .map(|m| m.id)
            .or_else(|| list_messages.first().map(|m| m.id));
        let selected_detail = selected_message_id.and_then(|id| db.get_message(id).ok().flatten());

        Self {
            db,
            accounts,
            folders_by_account,
            list_messages,
            selected_detail,
            selected_mailbox,
            collapsed_accounts: settings.collapsed_accounts.iter().copied().collect(),
            fold_anim: HashMap::new(),
            fold_seq: 0,
            selected_message_id,
            show_sidebar: true,
            sidebar_width,
            list_width,
            narrow: false,
            window_width: px(settings.window_width),
            window_origin,
            window_size,
            maximized: settings.maximized,
            max_origin,
            max_size,
            save_seq: 0,
            persist_ready: false,
            // Only Windows defers the first layout (see the field docs); elsewhere
            // the window already has its final size when the UI first renders.
            content_ready: !cfg!(target_os = "windows"),
            should_move: false,
            drag_start: point(px(0.0), px(0.0)),
            drag_anchor: point(px(0.0), px(0.0)),
            hovered_link: String::new(),
            settings_window: None,
            compose_window: None,
            compose_origin,
            compose_size,
            list_scroll: ScrollHandle::new(),
            list_scrollbar: None,
            sidebar_scroll: ScrollHandle::new(),
            sidebar_scrollbar: None,
            email_webview: None,
            last_webview_sig: None,
            load_remote_images: settings.load_remote_images,
            reader_white_background: settings.reader_white_background,
            compose_white_background: settings.compose_white_background,
            content_has_remote: false,
            content_blocked_count: 0,
            unblocked_messages: HashSet::new(),
            shown_images: HashMap::new(),
            privacy_menu_open: false,
            privacy_menu_pos: point(px(0.0), px(0.0)),
            search_input: None,
            search_force_expanded: false,
            last_search_query: String::new(),
            search_debounce_seq: 0,
            pre_search_mailbox: None,
        }
    }

    fn reload_message_list(&mut self) {
        let query = mail_list_query(&self.selected_mailbox, &self.last_search_query);
        if !self.last_search_query.trim().is_empty() {
            self.list_messages = self
                .db
                .list_messages(&MailListQuery::Search(self.last_search_query.clone()))
                .unwrap_or_default();
        } else {
            self.list_messages = self.db.list_messages(&query).unwrap_or_default();
        }
        if let Some(selected) = self.selected_message_id {
            if self.list_messages.iter().any(|m| m.id == selected) {
                self.reload_selected_detail();
            } else {
                self.clear_message_selection();
            }
        }
    }

    fn clear_message_selection(&mut self) {
        self.selected_message_id = None;
        self.selected_detail = None;
        self.last_webview_sig = None;
    }

    fn reload_selected_detail(&mut self) {
        self.selected_detail = self
            .selected_message_id
            .and_then(|id| self.db.get_message(id).ok().flatten());
    }

    /// Writes the current settings to disk immediately (synchronous, best-effort).
    /// Used to flush on close, bypassing the debounced [`Self::request_save`].
    /// No-op until persistence is armed, so closing mid-startup can't clobber the
    /// saved sizes.
    pub(crate) fn persist_now(&self) {
        if self.persist_ready {
            config::save(&self.current_config());
        }
    }

    /// Arms settings persistence once the initial window state has settled.
    pub(crate) fn enable_persistence(&mut self) {
        self.persist_ready = true;
    }

    /// Reveals the UI once the window has reached its final size (see
    /// [`Self::content_ready`]). Idempotent; redraws on the transition.
    // Only the Windows open sequence calls this; other platforms reveal eagerly.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(crate) fn mark_content_ready(&mut self, cx: &mut Context<Self>) {
        if !self.content_ready {
            self.content_ready = true;
            cx.notify();
        }
    }

    /// Snapshot of the persisted settings, taken from the live layout state.
    fn current_config(&self) -> Config {
        Config {
            window_x: f32::from(self.window_origin.x),
            window_y: f32::from(self.window_origin.y),
            window_width: f32::from(self.window_size.width),
            window_height: f32::from(self.window_size.height),
            maximized: self.maximized,
            max_x: f32::from(self.max_origin.x),
            max_y: f32::from(self.max_origin.y),
            max_width: f32::from(self.max_size.width),
            max_height: f32::from(self.max_size.height),
            sidebar_width: f32::from(self.sidebar_width),
            list_width: f32::from(self.list_width),
            load_remote_images: self.load_remote_images,
            reader_white_background: self.reader_white_background,
            compose_white_background: self.compose_white_background,
            collapsed_accounts: {
                let mut groups: Vec<usize> = self.collapsed_accounts.iter().copied().collect();
                groups.sort_unstable();
                groups
            },
            compose_x: f32::from(self.compose_origin.x),
            compose_y: f32::from(self.compose_origin.y),
            compose_width: f32::from(self.compose_size.width),
            compose_height: f32::from(self.compose_size.height),
        }
    }

    /// Records a move/resize of the compose window and debounces a settings
    /// write, mirroring the main window's persistence path.
    pub(crate) fn sync_compose_window_bounds(
        &mut self,
        origin: Point<Pixels>,
        window_size: Size<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if !self.persist_ready {
            return;
        }
        if origin != self.compose_origin || window_size != self.compose_size {
            self.compose_origin = origin;
            self.compose_size = window_size;
            self.request_save(cx);
        }
    }

    /// Schedules a debounced settings write. Each call invalidates any pending
    /// save, so a stream of changes (a live window/column resize) only hits disk
    /// once it has been quiet for a short moment. The write runs on a background
    /// thread and is best-effort.
    fn request_save(&mut self, cx: &mut Context<Self>) {
        if !self.persist_ready {
            return;
        }
        self.save_seq += 1;
        let token = self.save_seq;
        let timer = cx.background_executor().timer(Duration::from_millis(500));
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, _| {
                // Skip if a newer change has since scheduled its own save.
                if this.save_seq == token {
                    config::save(&this.current_config());
                }
            });
        })
        .detach();
    }

    /// Lazily creates the scrollbar state entities (needs an app context, which
    /// is only available at render time).
    fn ensure_scrollbar_states(&mut self, cx: &mut Context<Self>) {
        for slot in [&mut self.list_scrollbar, &mut self.sidebar_scrollbar] {
            slot.get_or_insert_with(|| cx.new(|_| ScrollbarState::new()));
        }
    }

    /// Lazily creates the toolbar search field.
    fn ensure_search_input(&mut self, language: Language, cx: &mut Context<Self>) {
        if self.search_input.is_none() {
            let placeholder = Key::SearchPlaceholder.tr(language);
            let input = cx.new(|cx| TextInput::new(placeholder, cx));
            // Apply the search after a short debounce. Defer so we never update
            // this view while it is already on the stack (GPUI panics).
            let root = cx.weak_entity();
            cx.observe(&input, move |_, _, cx| {
                let root = root.clone();
                cx.defer(move |cx| {
                    if let Some(root) = root.upgrade() {
                        root.update(cx, |this, cx| this.schedule_search_sync(cx));
                    }
                });
            })
            .detach();
            self.search_input = Some(input);
        } else if let Some(input) = &self.search_input {
            input.update(cx, |field, _| {
                field.set_placeholder(Key::SearchPlaceholder.tr(language));
            });
        }
    }

    /// Reconciles selection/scroll when the search query changed since the last
    /// frame. Called from [`Render::render`] so we never observe the input
    /// entity (that would try to update this view while it is already on the
    /// stack and panic).
    fn sync_search_if_needed(&mut self, cx: &mut Context<Self>) {
        let query = self.search_query(cx);
        if query == self.last_search_query {
            return;
        }
        let was_active = !self.last_search_query.trim().is_empty();
        let now_active = !query.trim().is_empty();
        if now_active && !was_active {
            self.pre_search_mailbox = Some(self.selected_mailbox.clone());
            self.clear_message_selection();
        }
        if !now_active && was_active {
            if let Some(prev) = self.pre_search_mailbox.take() {
                self.selected_mailbox = prev;
            }
            self.clear_message_selection();
        }
        self.last_search_query = query;
        self.reload_message_list();
        self.sync_search_selection();
    }

    fn next_search_debounce_token(&mut self) -> u64 {
        self.search_debounce_seq = self.search_debounce_seq.wrapping_add(1);
        self.search_debounce_seq
    }

    fn search_debounce_token_current(&self, token: u64) -> bool {
        self.search_debounce_seq == token
    }

    fn schedule_search_sync(&mut self, cx: &mut Context<Self>) {
        let token = self.next_search_debounce_token();
        let timer = cx
            .background_executor()
            .timer(Duration::from_millis(SEARCH_DEBOUNCE_MS));
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.search_debounce_token_current(token) {
                    this.sync_search_if_needed(cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Current search query (empty when the field has not been created yet).
    fn search_query(&self, cx: &App) -> String {
        self.search_input
            .as_ref()
            .map(|input| input.read(cx).content().to_string())
            .unwrap_or_default()
    }

    /// Whether the search query is non-empty.
    fn search_is_active(&self, _cx: &App) -> bool {
        !self.last_search_query.trim().is_empty()
    }

    /// Whether the toolbar should show the expanded search field (as opposed to
    /// the compact magnifying-glass button).
    fn show_search_expanded(&self) -> bool {
        !self.search_is_compact() || self.search_force_expanded
    }

    /// Keeps the compact toolbar search expanded so its input can receive focus.
    fn prepare_search_focus(&mut self) {
        self.search_force_expanded = true;
    }

    /// Keeps the selected message inside the filtered list and scrolls back to
    /// the top when the query changes.
    fn sync_search_selection(&mut self) {
        if self.list_messages.is_empty() {
            self.list_scroll.scroll_to_top_of_item(0);
            return;
        }
        if let Some(selected) = self.selected_message_id {
            if !self.list_messages.iter().any(|m| m.id == selected) {
                self.clear_message_selection();
            }
        }
        let visible = self
            .selected_message_id
            .and_then(|id| self.list_messages.iter().position(|m| m.id == id))
            .unwrap_or(0);
        self.list_scroll.scroll_to_top_of_item(visible);
    }

    fn focus_search_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prepare_search_focus();
        set_native_text_focus(window);
        if let Some(input) = &self.search_input {
            let focus_handle = input.read(cx).focus_handle(cx);
            window.defer(cx, move |window, _| {
                set_native_text_focus(window);
                window.focus(&focus_handle);
            });
        }
        cx.notify();
    }

    /// Creates (on first use) and updates the embedded webview to reflect the
    /// selected message and the current theme. Hides it when the reader is not
    /// on screen so it doesn't float over other views.
    fn sync_webview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(detail) = self.selected_detail.as_ref() else {
            self.last_webview_sig = None;
            if let Some(webview) = &mut self.email_webview {
                webview.hide();
            }
            return;
        };
        let message_id = detail.id;
        let colors = cx.theme().colors();
        let language = cx.language();
        let load_remote = self.load_remote_images || self.unblocked_messages.contains(&message_id);
        // Force a white page (dark text) when the user keeps the reader light,
        // regardless of the app theme; otherwise follow the theme.
        let (body_bg, body_text) = if self.reader_white_background {
            (gpui::white(), READER_LIGHT_TEXT)
        } else {
            (colors.background, colors.text)
        };
        // Context-menu popups always follow the app theme (dark in dark mode),
        // even when the page is forced white.
        let doc_colors = DocumentColors {
            background: body_bg,
            text: body_text,
            accent: colors.accent,
            menu_bg: colors.background,
            menu_text: colors.text,
        };
        let signature = WebviewSignature {
            selected: message_id,
            colors: doc_colors,
            language,
            load_remote,
            reader_white_background: self.reader_white_background,
        };
        if self.email_webview.is_some() && self.last_webview_sig == Some(signature) {
            return;
        }
        self.last_webview_sig = Some(signature);

        let empty_shown: HashSet<String> = HashSet::new();
        let body = db_seed::message_body_from_detail(detail);
        let rendered = email_document(
            doc_colors,
            &body,
            ContextMenuLabels {
                image_open: Key::CtxOpenImage.tr(language),
                image_download: Key::CtxDownloadImage.tr(language),
                image_show: Key::CtxShowImage.tr(language),
                link_open: Key::CtxOpenLink.tr(language),
                link_copy: Key::CtxCopyLink.tr(language),
                selection_copy: Key::CtxCopy.tr(language),
                // ⌘ (U+2318) is the macOS Command symbol; Ctrl elsewhere.
                copy_shortcut: if cfg!(target_os = "macos") {
                    "\u{2318}C"
                } else {
                    "Ctrl+C"
                },
            },
            load_remote,
            self.shown_images.get(&message_id).unwrap_or(&empty_shown),
        );
        self.content_has_remote = rendered.has_remote;
        self.content_blocked_count = rendered.blocked_images;
        // The unblock menu only makes sense while the shield is in its blocked
        // (red) state; close it once nothing is blocked for this message.
        if !self.shield_is_blocked() {
            self.privacy_menu_open = false;
        }
        let document = rendered.html;
        let notify_body = Key::ImageDownloaded.tr(language).to_string();

        match &mut self.email_webview {
            Some(webview) => {
                webview.set_html(&document);
                webview.set_notify_text(notify_body);
            }
            None => {
                // The webview's IPC callback can fire from any thread, so we hand
                // events off through a channel and drain them on the GPUI
                // foreground (status bar updates, clipboard writes).
                let (tx, rx) = async_channel::unbounded::<HostEvent>();
                self.email_webview = EmailWebView::new(window, &document, tx, notify_body);
                cx.spawn(async move |this, cx| {
                    while let Ok(event) = rx.recv().await {
                        if this
                            .update(cx, |root, cx| root.handle_host_event(event, cx))
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .detach();
            }
        }
    }

    /// Applies an action requested by the webview on the GPUI foreground.
    fn handle_host_event(&mut self, event: HostEvent, cx: &mut Context<Self>) {
        match event {
            HostEvent::HoverLink(url) => self.set_hovered_link(url, cx),
            HostEvent::CopyToClipboard(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
            HostEvent::ImageShown(url) => self.image_shown(url, cx),
            HostEvent::BodyMouseDown => {
                // A click inside the webview never reaches the GPUI overlay's
                // outside-click catcher, so close the privacy dropdown here.
                if self.privacy_menu_open {
                    self.privacy_menu_open = false;
                    cx.notify();
                }
            }
        }
    }

    /// Records that the user revealed one blocked remote image in the body. The
    /// image is kept in this message's allowlist (so it stays shown across
    /// re-renders) and the blocked count drops; when it hits zero the shield
    /// flips to its loaded (green) state. The webview already swapped the image
    /// in place, so no document rebuild is needed here.
    fn image_shown(&mut self, url: String, cx: &mut Context<Self>) {
        if let Some(id) = self.selected_message_id {
            self.shown_images.entry(id).or_default().insert(url);
        }
        self.content_blocked_count = self.content_blocked_count.saturating_sub(1);
        cx.notify();
    }

    /// Updates the hovered-link URL shown in the status bar, re-rendering only
    /// when it actually changes.
    fn set_hovered_link(&mut self, url: String, cx: &mut Context<Self>) {
        if self.hovered_link != url {
            self.hovered_link = url;
            cx.notify();
        }
    }

    /// Toggles whether remote content loads in e-mail bodies, persisting the new
    /// preference and re-rendering so the open message is re-sanitized.
    fn set_load_remote_images(&mut self, value: bool, cx: &mut Context<Self>) {
        if self.load_remote_images != value {
            self.load_remote_images = value;
            self.request_save(cx);
            cx.notify();
        }
    }

    /// Toggles whether the reader forces a white background, persisting the
    /// preference and re-rendering so the open message is re-themed.
    fn set_reader_white_background(&mut self, value: bool, cx: &mut Context<Self>) {
        if self.reader_white_background != value {
            self.reader_white_background = value;
            self.request_save(cx);
            cx.refresh_windows();
        }
    }

    /// Toggles whether the compose body forces a white background, persisting
    /// the preference and updating an open compose window.
    fn set_compose_white_background(&mut self, value: bool, cx: &mut Context<Self>) {
        if self.compose_white_background != value {
            self.compose_white_background = value;
            self.request_save(cx);
            if let Some(handle) = self.compose_window {
                let _ = handle.update(cx, |view, _, cx| {
                    view.set_white_background(value, cx);
                });
            }
            cx.refresh_windows();
        }
    }

    /// Opens the preferences in their own window (like Zed). If it is already
    /// open, just brings it to the front rather than spawning a duplicate.
    fn open_settings(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(handle) = self.settings_window {
            if handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
            // The window was closed since we last opened it; reopen a fresh one.
            self.settings_window = None;
        }

        let accounts = ui_accounts(&self.accounts);
        let root = cx.entity().downgrade();
        let bounds = Bounds::centered(None, size(px(760.0), px(560.0)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(520.0), px(420.0))),
            titlebar: Some(TitlebarOptions {
                title: Some("rMail Settings".into()),
                appears_transparent: false,
                traffic_light_position: None,
            }),
            ..Default::default()
        };

        if let Ok(handle) = cx.open_window(options, |_window, cx| {
            cx.new(|_| SettingsView::new(accounts, root))
        }) {
            let _ = handle.update(cx, |_, window, _| window.activate_window());
            self.settings_window = Some(handle);
            cx.notify();
        }
    }

    /// Opens the compose window, or focuses it if it is already open.
    fn open_compose(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(handle) = self.compose_window {
            if handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
            self.compose_window = None;
        }

        let accounts = ui_accounts(&self.accounts);
        let root = cx.entity().downgrade();
        let compose_white = self.compose_white_background;
        let title = compose::window_title(cx.language());
        let bounds = compose::open_bounds(self.compose_origin, self.compose_size, cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(
                px(compose::COMPOSE_MIN_WIDTH),
                px(compose::COMPOSE_MIN_HEIGHT),
            )),
            titlebar: Some(TitlebarOptions {
                title: Some(title.into()),
                appears_transparent: false,
                traffic_light_position: None,
            }),
            ..Default::default()
        };

        if let Ok(handle) = cx.open_window(options, |window, cx| {
            let root_for_close = root.clone();
            let view = cx.new(|_| ComposeView::new(accounts, root, compose_white));
            window.on_window_should_close(cx, move |_, cx| {
                if let Some(root) = root_for_close.upgrade() {
                    root.update(cx, |root, _| {
                        root.compose_window = None;
                        root.persist_now();
                    });
                }
                true
            });
            view
        }) {
            let _ = handle.update(cx, |_, window, _| window.activate_window());
            self.compose_window = Some(handle);
            cx.notify();
        }
    }

    /// Selects a sidebar mailbox and redraws. See [`Self::apply_mailbox_selection`]
    /// for the dismissal behavior in the narrow layout.
    fn select_mailbox(&mut self, selection: Selection, cx: &mut Context<Self>) {
        if self.search_is_active(cx) {
            if let Some(input) = &self.search_input {
                input.update(cx, |field, cx| field.clear(cx));
            }
            self.last_search_query.clear();
            self.pre_search_mailbox = None;
        }
        self.clear_message_selection();
        self.apply_mailbox_selection(selection);
        self.reload_message_list();
        cx.notify();
    }

    /// Core of [`Self::select_mailbox`], without the redraw (so it is unit-testable
    /// without a window context). Sets the selection and, in the narrow layout —
    /// where the sidebar floats over the content as an overlay — dismisses it
    /// (like a mobile navigation drawer). When docked, the sidebar stays put.
    fn apply_mailbox_selection(&mut self, selection: Selection) {
        self.selected_mailbox = selection;
        if self.narrow {
            self.show_sidebar = false;
        }
    }

    /// Marks the given scrollbar as just-scrolled and schedules a re-render once
    /// the auto-hide window elapses, so the bar fades out after scrolling stops.
    fn note_scroll(
        states: impl IntoIterator<Item = Option<Entity<ScrollbarState>>>,
        cx: &mut Context<Self>,
    ) {
        for state in states.into_iter().flatten() {
            state.update(cx, |state, _| state.note_scroll());
        }
        cx.notify();

        // Re-render slightly after the auto-hide window so visibility is
        // re-evaluated (and the bar hidden) when scrolling has stopped.
        let timer = cx
            .background_executor()
            .timer(ui::AUTO_HIDE + Duration::from_millis(100));
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    /// Show or hide the accounts sidebar (toolbar toggle, like Mail).
    fn toggle_sidebar(&mut self) {
        self.show_sidebar = !self.show_sidebar;
    }

    /// Whether the given account's mailbox list is collapsed (target state).
    fn is_account_collapsed(&self, account_idx: usize) -> bool {
        self.collapsed_accounts.contains(&account_idx)
    }

    /// Toggles an account between collapsed and expanded, starting the fold
    /// animation and returning its descriptor so the caller can schedule the
    /// finalize timer that clears it once the transition completes.
    fn toggle_account(&mut self, account_idx: usize) -> FoldAnim {
        let collapsing = !self.collapsed_accounts.contains(&account_idx);
        if collapsing {
            self.collapsed_accounts.insert(account_idx);
        } else {
            self.collapsed_accounts.remove(&account_idx);
        }
        self.fold_seq += 1;
        let anim = FoldAnim {
            collapsing,
            token: self.fold_seq,
        };
        self.fold_anim.insert(account_idx, anim);
        anim
    }

    /// Clears a finished fold animation, but only if it is still the current one
    /// (a newer toggle bumps the token, so a stale timer is ignored). Returns
    /// whether the state changed and a re-render is therefore warranted.
    fn clear_fold(&mut self, account_idx: usize, token: u64) -> bool {
        if self
            .fold_anim
            .get(&account_idx)
            .is_some_and(|anim| anim.token == token)
        {
            self.fold_anim.remove(&account_idx);
            true
        } else {
            false
        }
    }

    /// Whether the account's mailbox list should be rendered: while expanded, or
    /// while a collapse animation is still playing it out.
    fn account_list_visible(&self, account_idx: usize) -> bool {
        !self.is_account_collapsed(account_idx)
            || self
                .fold_anim
                .get(&account_idx)
                .is_some_and(|anim| anim.collapsing)
    }

    /// Whether the sidebar occupies a column in the layout (vs. hidden or
    /// floating). When it isn't docked, the message-list controls move from the
    /// top toolbar into a header at the top of the list.
    fn sidebar_docked(&self) -> bool {
        self.show_sidebar && !self.narrow
    }

    /// Horizontal space the reader's toolbar segment (compose + actions + search)
    /// gets, i.e. the window width minus the sidebar and list segments.
    fn reader_segment_width(&self) -> Pixels {
        let sidebar_segment = if self.sidebar_docked() {
            self.sidebar_width
        } else {
            px(240.0)
        };
        let list_segment = if self.sidebar_docked() {
            self.list_width
        } else {
            px(0.0)
        };
        self.window_width
            - sidebar_segment
            - list_segment
            - crate::window_frame::right_controls_reserved_width()
    }

    /// Whether the toolbar search field should collapse into an icon button,
    /// i.e. when the container holding the reader's toolbar buttons is narrower
    /// than `SEARCH_COLLAPSE_WIDTH`.
    fn search_is_compact(&self) -> bool {
        self.reader_segment_width() < px(SEARCH_COLLAPSE_WIDTH)
    }

    /// How many of the centered action groups (reply, trash, flag/move) fit in
    /// the reader segment without pushing into the always-visible search button.
    /// Groups are dropped from the right as space shrinks.
    fn visible_action_groups(&self) -> usize {
        let search_width = if self.search_is_compact() {
            SEARCH_ICON_WIDTH
        } else {
            SEARCH_FIELD_WIDTH
        };
        let budget = f32::from(self.reader_segment_width()) - TOOLBAR_FIXED_OVERHEAD - search_width;
        if budget >= ACTIONS_3_WIDTH {
            3
        } else if budget >= ACTIONS_2_WIDTH {
            2
        } else if budget >= ACTIONS_1_WIDTH {
            1
        } else {
            0
        }
    }

    /// The message-list controls (mailbox title + count on the left, filter/more
    /// on the right). Rendered in the top toolbar while the sidebar is docked,
    /// and inside the list's own header otherwise.
    fn render_list_controls(&self, show_count: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let language = cx.language();
        let list_title: SharedString = if self.search_is_active(cx) {
            Key::SearchActiveTitle.tr(language).into()
        } else {
            match &self.selected_mailbox {
                Selection::Global(global) => global.display_name(language).into(),
                Selection::Mailbox {
                    account_id,
                    folder_path,
                } => self
                    .folders_by_account
                    .get(account_id)
                    .and_then(|folders| folders.iter().find(|f| f.path == *folder_path))
                    .map(|folder| {
                        db_seed::folder_display_name(&folder.path, &folder.display_name, language)
                    })
                    .unwrap_or_else(|| folder_path.clone().into()),
            }
        };
        let list_count = locale::message_count(language, self.list_messages.len());

        h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .child(
                v_flex()
                    .child(Label::new(list_title).weight(FontWeight::SEMIBOLD))
                    .when(show_count, |this| {
                        this.child(
                            Label::new(list_count)
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                        )
                    }),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(Self::icon_button_with_tooltip(
                        "filter",
                        Key::ToolbarFilter.tr(language),
                        IconButton::new("filter", IconName::Filter),
                    ))
                    .child(Self::icon_button_with_tooltip(
                        "more",
                        Key::ToolbarMore.tr(language),
                        IconButton::new("more", IconName::More),
                    )),
            )
    }

    /// Reconciles layout state with the current window width: tracks the narrow
    /// breakpoint (auto-collapsing the sidebar when crossing into it and
    /// restoring it when leaving) and clamps the column widths so every column
    /// keeps its minimum and the reader never shrinks past `READER_MIN_WIDTH`.
    fn sync_layout(&mut self, total: Pixels) {
        let width_changed = total != self.window_width;
        self.window_width = total;
        let now_narrow = total < px(NARROW_BREAKPOINT);

        if now_narrow {
            // Below the breakpoint the sidebar is collapsed. It can still be
            // reopened as a floating overlay via the toolbar toggle, but entering
            // the narrow range — or any resize while inside it — re-collapses it.
            if !self.narrow || width_changed {
                self.show_sidebar = false;
            }
        } else if self.narrow {
            // Restore the docked sidebar once the window grows back.
            self.show_sidebar = true;
        }
        self.narrow = now_narrow;

        let sidebar = if self.show_sidebar && !self.narrow {
            self.sidebar_width
        } else {
            px(0.0)
        };

        // Keep the list within [min, available-for-reader].
        let list_max = (total - sidebar - px(READER_MIN_WIDTH)).max(px(LIST_MIN_WIDTH));
        self.list_width = self.list_width.clamp(px(LIST_MIN_WIDTH), list_max);

        // Keep the sidebar within [min, available-for-list-and-reader].
        if self.show_sidebar && !self.narrow {
            let sidebar_max =
                (total - self.list_width - px(READER_MIN_WIDTH)).max(px(SIDEBAR_MIN_WIDTH));
            self.sidebar_width = self.sidebar_width.clamp(px(SIDEBAR_MIN_WIDTH), sidebar_max);
        }
    }

    /// Applies a divider drag: `x` is the cursor position relative to the row's
    /// left edge and `total` is the row width. Widths are clamped so the reader
    /// keeps at least `READER_MIN_WIDTH`.
    fn resize(&mut self, handle: ResizeHandle, x: Pixels, total: Pixels) {
        match handle {
            ResizeHandle::Sidebar => {
                let max =
                    (total - self.list_width - px(READER_MIN_WIDTH)).max(px(SIDEBAR_MIN_WIDTH));
                self.sidebar_width = x.clamp(px(SIDEBAR_MIN_WIDTH), max);
            }
            ResizeHandle::List => {
                let left = if self.show_sidebar && !self.narrow {
                    self.sidebar_width
                } else {
                    px(0.0)
                };
                let max = (total - left - px(READER_MIN_WIDTH)).max(px(LIST_MIN_WIDTH));
                self.list_width = (x - left).clamp(px(LIST_MIN_WIDTH), max);
            }
        }
    }

    // ----- Top toolbar (unified title bar) --------------------------------

    fn render_toolbar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let bg = colors.title_bar_background;
        let border = colors.border;
        let search_bg = colors.element_background;
        let language = cx.language();
        // Keep the toolbar segments aligned with the resizable columns below. When
        // the sidebar is docked its segment matches the sidebar width; otherwise it
        // only reserves room for the traffic lights + toggle controls.
        let sidebar_docked = self.sidebar_docked();
        let sidebar_segment_width = if sidebar_docked {
            self.sidebar_width
        } else {
            px(240.0)
        };

        // The list controls live in the toolbar only while the sidebar is docked;
        // otherwise they move into the list's own header (see `render_message_list`).
        let list_segment = sidebar_docked.then(|| {
            h_flex()
                .w(self.list_width)
                .flex_shrink_0()
                .items_center()
                .px_3()
                .child(self.render_list_controls(true, cx))
        });

        // When the reader's toolbar segment gets too tight the full search field
        // would be clipped, so we collapse it into a single icon button unless
        // the user explicitly expanded it.
        let compact_search = !self.show_search_expanded();

        h_flex()
            .h(px(52.0))
            .w_full()
            .flex_shrink_0()
            .bg(bg)
            .border_b_1()
            .border_color(border)
            // The whole toolbar acts as a draggable title bar. We arm the drag on
            // mouse-down and only begin moving on the first mouse-move, so clicks
            // on the toolbar's own buttons keep working. A double-click performs
            // the platform's default title-bar action (zoom/minimize on macOS).
            .window_control_area(WindowControlArea::Drag)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, _| {
                    // The native click count already honors the platform's
                    // double-click time/distance, so a slow click-pause-click is
                    // two single clicks, not a double-click.
                    if event.click_count == 2 {
                        this.should_move = false;
                        window.titlebar_double_click();
                    } else {
                        this.should_move = true;
                        this.drag_start = event.position;
                        this.drag_anchor = window.bounds().origin;
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.should_move = false),
            )
            .on_mouse_down_out(cx.listener(|this, _, _, _| this.should_move = false))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, _| {
                if !this.should_move {
                    return;
                }
                // Begin the drag (and un-maximize) once either the cursor has
                // clearly moved or the window itself has moved from where it was at
                // mouse-down. The latter catches slow native title-bar drags, where
                // the window keeps up with the cursor so the cursor delta stays
                // near zero. A plain click moves neither, so it never triggers.
                let dx = f32::from(event.position.x - this.drag_start.x).abs();
                let dy = f32::from(event.position.y - this.drag_start.y).abs();
                let window_moved = window.bounds().origin != this.drag_anchor;
                if window_moved || dx > DRAG_THRESHOLD || dy > DRAG_THRESHOLD {
                    this.should_move = false;
                    crate::window_drag::start_window_drag(window, this.maximized, this.window_size);
                }
            }))
            // Segment 1: over the sidebar — titlebar spacing + sidebar toggle
            // and app settings (theme switching lives in Settings ▸ Appearance).
            .child(
                h_flex()
                    .w(sidebar_segment_width)
                    .flex_shrink_0()
                    .items_center()
                    .gap_1()
                    .pl(crate::window_frame::toolbar_left_padding())
                    .child(Self::icon_button_with_tooltip(
                        "toggle-sidebar",
                        Key::ToolbarToggleSidebar.tr(language),
                        IconButton::new("toggle-sidebar", IconName::Sidebar)
                            .selected(self.show_sidebar)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_sidebar();
                                cx.notify();
                            })),
                    ))
                    .child(Self::icon_button_with_tooltip(
                        "settings",
                        Key::SettingsTitle.tr(language),
                        IconButton::new("settings", IconName::Settings).on_click(cx.listener(
                            |this, _, window, cx| {
                                this.open_settings(window, cx);
                            },
                        )),
                    )),
            )
            // Segment 2: over the message list — title + count on the left,
            // filter/more on the right. Only present while the sidebar is docked.
            .children(list_segment)
            // Segment 3: over the reader — compose, action groups, search, app controls.
            .child(
                h_flex()
                    .flex_1()
                    // Let this segment yield width to the window controls beside it
                    // (which are `flex_shrink_0`). Without it the segment refuses to
                    // shrink below its content and pushes the caption buttons — the
                    // close button included — off the right edge of the window.
                    .min_w_0()
                    .items_center()
                    .gap_3()
                    .px_3()
                    // Left-aligned: compose.
                    .child(Self::icon_button_with_tooltip(
                        "compose",
                        Key::ComposeWindowTitle.tr(language),
                        IconButton::new("compose", IconName::Compose)
                            .size(IconSize::Medium)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_compose(window, cx);
                            })),
                    ))
                    .child(div().flex_1())
                    // Centered action groups, divided like macOS Mail. Whole groups
                    // are dropped (right to left) as space shrinks so they never
                    // overlap the search button on the right.
                    .map(|el| {
                        let groups = self.visible_action_groups();
                        el.child(
                            h_flex()
                                .gap_2()
                                .when(groups >= 1, |cluster| {
                                    cluster.child(
                                        h_flex()
                                            .gap_1()
                                            .child(Self::icon_button_with_tooltip(
                                                "reply",
                                                Key::ToolbarReply.tr(language),
                                                IconButton::new("reply", IconName::Reply),
                                            ))
                                            .child(Self::icon_button_with_tooltip(
                                                "reply-all",
                                                Key::ToolbarReplyAll.tr(language),
                                                IconButton::new("reply-all", IconName::ReplyAll),
                                            ))
                                            .child(Self::icon_button_with_tooltip(
                                                "forward",
                                                Key::ToolbarForward.tr(language),
                                                IconButton::new("forward", IconName::Forward),
                                            )),
                                    )
                                })
                                .when(groups >= 2, |cluster| {
                                    cluster.child(Self::render_toolbar_separator(border)).child(
                                        h_flex()
                                            .gap_1()
                                            .child(Self::icon_button_with_tooltip(
                                                "trash",
                                                Key::MailboxTrash.tr(language),
                                                IconButton::new("trash", IconName::Trash),
                                            ))
                                            .child(Self::icon_button_with_tooltip(
                                                "archive",
                                                Key::MailboxArchive.tr(language),
                                                IconButton::new("archive", IconName::Archive),
                                            ))
                                            .child(Self::icon_button_with_tooltip(
                                                "junk",
                                                Key::MailboxJunk.tr(language),
                                                IconButton::new("junk", IconName::Junk),
                                            )),
                                    )
                                })
                                .when(groups >= 3, |cluster| {
                                    cluster.child(Self::render_toolbar_separator(border)).child(
                                        h_flex()
                                            .gap_1()
                                            .child(Self::render_dropdown_button(
                                                "flag",
                                                IconName::Flag,
                                                Key::ToolbarFlag.tr(language),
                                            ))
                                            .child(Self::render_dropdown_button(
                                                "move",
                                                IconName::Folder,
                                                Key::ToolbarMove.tr(language),
                                            )),
                                    )
                                }),
                        )
                    })
                    .child(div().flex_1())
                    .child(if compact_search {
                        div()
                            .id("search")
                            .tooltip(Tooltip::text(Key::SearchPlaceholder.tr(language)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.focus_search_input(window, cx);
                                }),
                            )
                            .child(IconButton::new("search", IconName::Search))
                            .into_any_element()
                    } else {
                        h_flex()
                            .id("search")
                            .w(px(SEARCH_FIELD_WIDTH))
                            .h(px(28.0))
                            .px_2()
                            .gap_1p5()
                            .items_center()
                            .rounded_md()
                            .bg(search_bg)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.focus_search_input(window, cx);
                                }),
                            )
                            .child(
                                Icon::new(IconName::Search)
                                    .size(IconSize::Small)
                                    .color(Color::Muted),
                            )
                            .child(
                                self.search_input
                                    .clone()
                                    .expect("search input is created before render"),
                            )
                            .into_any_element()
                    }),
            )
            .children(crate::window_frame::render_right_window_controls(window))
    }

    /// Wraps an [`IconButton`] with a localized tooltip (GPUI attaches tooltips to
    /// elements via `div`, not to `IconButton` directly).
    fn icon_button_with_tooltip(
        id: &'static str,
        tooltip: &'static str,
        button: IconButton,
    ) -> impl IntoElement {
        div().id(id).tooltip(Tooltip::text(tooltip)).child(button)
    }

    /// A thin vertical divider used to separate toolbar button groups.
    fn render_toolbar_separator(color: Hsla) -> impl IntoElement {
        div().w(px(1.0)).h(px(20.0)).bg(color)
    }

    /// A toolbar control that pairs an icon with a small chevron to hint a
    /// dropdown menu (mirrors the "move to folder" / "flag" buttons in Mail).
    fn render_dropdown_button(
        id: &'static str,
        icon: IconName,
        tooltip: &'static str,
    ) -> impl IntoElement {
        div().id(id).tooltip(Tooltip::text(tooltip)).child(
            h_flex()
                .items_center()
                .gap_0p5()
                .child(IconButton::new(id, icon))
                .child(
                    Icon::new(IconName::ChevronDown)
                        .size(IconSize::XSmall)
                        .color(Color::Muted),
                ),
        )
    }

    // ----- Sidebar (accounts and mailboxes) -------------------------------

    fn render_sidebar(&self, floating: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let bg = colors.panel_background;
        let border = colors.border;

        let mut content = v_flex()
            .id("sidebar")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.sidebar_scroll)
            .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                Self::note_scroll([this.sidebar_scrollbar.clone()], cx);
            }))
            .pt_2();

        content = content.child(self.render_global_section(cx));

        for (account_idx, account) in self.accounts.iter().enumerate() {
            content = content.child(self.render_account(account_idx, account, cx));
        }

        let scrollbar = self
            .sidebar_scrollbar
            .clone()
            .map(|state| Scrollbar::vertical(state, self.sidebar_scroll.clone()));

        div()
            .relative()
            .w(self.sidebar_width)
            .flex_shrink_0()
            .h_full()
            .bg(bg)
            .border_r_1()
            .border_color(border)
            .when(floating, |el| el.shadow_lg())
            .child(content)
            .children(scrollbar)
            .child(self.render_resize_handle(ResizeHandle::Sidebar, cx))
    }

    /// A thin draggable strip overlaid on a column's right edge that resizes the
    /// column. The actual width update happens in the row-level `on_drag_move`
    /// handler (see `render`), which has the full row bounds to work with.
    fn render_resize_handle(
        &self,
        kind: ResizeHandle,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = match kind {
            ResizeHandle::Sidebar => "resize-handle-sidebar",
            ResizeHandle::List => "resize-handle-list",
        };
        div()
            .id(id)
            .absolute()
            .top_0()
            .right(px(-RESIZE_HANDLE_WIDTH / 2.0))
            .w(px(RESIZE_HANDLE_WIDTH))
            .h_full()
            .cursor_col_resize()
            .occlude()
            .on_drag(ResizeDrag(kind), |_, _, _, cx| cx.new(|_| Empty))
    }

    /// Aggregated unread count for a global (unified) mailbox. In the mock this
    /// is derived from the sample data; it will come from the real mail layer
    /// later. Inbox sums every account's inbox; Flagged counts starred messages.
    fn global_unread(&self, global: GlobalMailbox) -> usize {
        let path = db_seed::global_folder_path(global);
        self.db.global_unread(path).unwrap_or(0)
    }

    /// The unified mailboxes pinned to the top of the sidebar, above the account
    /// groups. They are not nested under any account (no indentation), and a thin
    /// divider separates them from the groups below.
    fn render_global_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let language = cx.language();
        let divider = cx.theme().colors().border_variant;

        let mut section = v_flex().px_2().pb_2();
        for global in GlobalMailbox::ALL {
            let selected = self.selected_mailbox == Selection::Global(global);
            let count = self.global_unread(global);
            let badge = (count > 0).then(|| self.render_count_badge(count, selected, cx));

            let mut item = ListItem::new(("global", global as usize))
                .selected(selected)
                .inset(px(ITEM_PADDING))
                .start_slot(
                    Icon::new(global_mailbox_icon(global))
                        .size(IconSize::Small)
                        .color(if selected {
                            Color::Accent
                        } else {
                            Color::Muted
                        }),
                )
                .child(
                    Label::new(global.display_name(language))
                        .size(LabelSize::Small)
                        .single_line(),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select_mailbox(Selection::Global(global), cx);
                }));

            if let Some(badge) = badge {
                item = item.end_slot(badge);
            }

            section = section.child(div().h(px(SIDEBAR_ROW_HEIGHT)).flex_shrink_0().child(item));
        }

        section.child(div().h(px(1.0)).mx_2().mt_1().bg(divider))
    }

    fn render_account(
        &self,
        account_idx: usize,
        account: &storage::Account,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let language = cx.language();
        let chevron_color = cx.theme().colors().text_muted;
        let section = v_flex().px_2().pb_3().child(
            h_flex()
                .id(("account-header", account_idx))
                .h(px(SIDEBAR_ROW_HEIGHT))
                .items_center()
                .px_2()
                .gap_2()
                .rounded_md()
                .cursor_pointer()
                .hover(|el| el.bg(cx.theme().colors().element_hover))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let token = this.toggle_account(account_idx).token;
                    // Persist the new open/closed state (debounced).
                    this.request_save(cx);
                    // Clear the animation once it has played out so the list and
                    // chevron settle into their final (static) state.
                    let timer = cx
                        .background_executor()
                        .timer(Duration::from_millis(FOLD_ANIM_MS + 30));
                    cx.spawn(async move |this, cx| {
                        timer.await;
                        let _ = this.update(cx, |this, cx| {
                            if this.clear_fold(account_idx, token) {
                                cx.notify();
                            }
                        });
                    })
                    .detach();
                    cx.notify();
                }))
                .child(self.render_account_chevron(account_idx, chevron_color))
                .child(
                    Label::new(account.name.clone())
                        .size(LabelSize::XSmall)
                        .color(Color::Muted)
                        .weight(FontWeight::SEMIBOLD),
                ),
        );

        if !self.account_list_visible(account_idx) {
            return section;
        }

        let mut list = v_flex();
        let folders = self
            .folders_by_account
            .get(&account.id)
            .cloned()
            .unwrap_or_default();
        for (mailbox_idx, folder) in folders.iter().enumerate() {
            let kind = db_seed::folder_kind_from_path(&folder.path);
            let selected = self.selected_mailbox
                == Selection::Mailbox {
                    account_id: account.id,
                    folder_path: folder.path.clone(),
                };
            let unread = self
                .db
                .unread_in_folder(Some(account.id), &folder.path)
                .unwrap_or(0);
            let badge = if unread > 0 {
                Some(self.render_count_badge(unread, selected, cx))
            } else {
                None
            };
            let folder_path = folder.path.clone();
            let account_id = account.id;

            let mut item = ListItem::new(("mailbox", account_idx * 100 + mailbox_idx))
                .selected(selected)
                .inset(px(ITEM_PADDING))
                .start_slot(Icon::new(mailbox_icon(kind)).size(IconSize::Small).color(
                    if selected {
                        Color::Accent
                    } else {
                        Color::Muted
                    },
                ))
                .child(
                    Label::new(db_seed::folder_display_name(
                        &folder.path,
                        &folder.display_name,
                        language,
                    ))
                    .size(LabelSize::Small)
                    .single_line(),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select_mailbox(
                        Selection::Mailbox {
                            account_id,
                            folder_path: folder_path.clone(),
                        },
                        cx,
                    );
                }));

            if let Some(badge) = badge {
                item = item.end_slot(badge);
            }

            // Each row is pinned to a fixed height so the accordion animation
            // below can compute the list's total height exactly. The whole row
            // (highlight included) is indented so it reads as nested under its
            // account header.
            list = list.child(
                div()
                    .h(px(SIDEBAR_ROW_HEIGHT))
                    .flex_shrink_0()
                    .pl(px(ITEM_INDENT))
                    .child(item),
            );
        }

        // While a fold animation is in flight, play it as an accordion: the list
        // lives inside an `overflow_hidden` box whose height grows (expand) or
        // shrinks (collapse), so the rows below slide down and up. It also fades
        // in/out so the reveal isn't purely geometric. Keyed by token so each
        // toggle replays from the start; once cleared the list renders at its
        // natural (and identical) height and full opacity, so there is no snap.
        let folder_count = self
            .folders_by_account
            .get(&account.id)
            .map(|f| f.len())
            .unwrap_or(0);
        let content_height = px(folder_count as f32 * SIDEBAR_ROW_HEIGHT);
        let list_el = match self.fold_anim.get(&account_idx).copied() {
            Some(anim) => div()
                .overflow_hidden()
                .child(list)
                .with_animation(
                    ("account-reveal", anim.token),
                    Animation::new(Duration::from_millis(FOLD_ANIM_MS)).with_easing(ease_in_out),
                    move |el, delta| {
                        let shown = if anim.collapsing { 1.0 - delta } else { delta };
                        el.h(content_height * shown).opacity(shown)
                    },
                )
                .into_any_element(),
            None => list.into_any_element(),
        };

        section.child(list_el)
    }

    /// The account header's disclosure chevron, rendered inside a fixed-size box
    /// so it never shifts the header. It points right when collapsed and down
    /// when expanded; during a fold it animates by rotating between the two
    /// (0 ↔ 90°), keyed by token so it replays per toggle.
    fn render_account_chevron(&self, account_idx: usize, color: Hsla) -> gpui::AnyElement {
        let inner = match self.fold_anim.get(&account_idx).copied() {
            Some(anim) => chevron_svg(color, 0.0)
                .with_animation(
                    ("account-chevron", anim.token),
                    Animation::new(Duration::from_millis(FOLD_ANIM_MS)).with_easing(ease_in_out),
                    move |chevron, delta| {
                        let t = if anim.collapsing { 1.0 - delta } else { delta };
                        chevron.with_transformation(Transformation::rotate(radians(t * FRAC_PI_2)))
                    },
                )
                .into_any_element(),
            None => {
                let angle = if self.is_account_collapsed(account_idx) {
                    0.0
                } else {
                    FRAC_PI_2
                };
                chevron_svg(color, angle).into_any_element()
            }
        };

        div()
            .w(px(CHEVRON_BOX))
            .h(px(CHEVRON_BOX))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .child(inner)
            .into_any_element()
    }

    fn render_count_badge(
        &self,
        count: usize,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors();
        let bg = if selected {
            colors.accent
        } else {
            colors.element_active
        };
        let text = if selected {
            colors.text_on_accent
        } else {
            colors.text_muted
        };

        // A fixed height paired with a line height tight to the glyph keeps the
        // digit optically centered: with the default (looser) line height the
        // text box is taller than the number, leaving it ~1px off center.
        h_flex()
            .h(px(19.0))
            .min_w(px(21.0))
            .px_1p5()
            .justify_center()
            .rounded_full()
            .bg(bg)
            .text_size(px(10.0))
            .text_color(text)
            .child(count.to_string())
    }

    // ----- Message list ---------------------------------------------------

    fn render_message_list(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        // Match the reader pane (the email panel) rather than the sidebar, so the
        // list and the open message share one surface.
        let bg = colors.background;
        let border = colors.border;
        let header_bg = colors.panel_background;
        let language = cx.language();
        let filtered = &self.list_messages;

        let mut content = v_flex()
            .id("message-list")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.list_scroll)
            .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                Self::note_scroll([this.list_scrollbar.clone()], cx);
            }))
            .on_click(cx.listener(|this, _, _, cx| {
                this.clear_message_selection();
                cx.notify();
            }));

        if filtered.is_empty() && self.search_is_active(cx) {
            content = content.child(
                div().w_full().py_8().flex().justify_center().child(
                    Label::new(Key::SearchNoResults.tr(language))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                ),
            );
        } else {
            for message in filtered {
                content = content.child(self.render_message_row(message, cx));
            }
        }

        let scrollbar = self
            .list_scrollbar
            .clone()
            .map(|state| Scrollbar::vertical(state, self.list_scroll.clone()));

        // When the sidebar isn't docked, the list controls that normally live in
        // the top toolbar move here, pinned to the top of the list.
        let header = (!self.sidebar_docked()).then(|| {
            h_flex()
                .w_full()
                .flex_shrink_0()
                .px_3()
                .py_2()
                // Match the sidebar surface: in this collapsed layout the list's
                // own toolbar stands in for the (hidden) sidebar's top region.
                .bg(header_bg)
                .border_b_1()
                .border_color(border)
                .child(self.render_list_controls(false, cx))
        });

        v_flex()
            .relative()
            .w(self.list_width)
            .flex_shrink_0()
            .h_full()
            .bg(bg)
            .border_r_1()
            .border_color(border)
            .children(header)
            .child(
                // Inner relative wrapper so the scrollbar overlay aligns with the
                // scrollable area (below the header) instead of the whole column.
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(content)
                    .children(scrollbar),
            )
            .child(self.render_resize_handle(ResizeHandle::List, cx))
    }

    fn render_message_row(
        &self,
        message: &MessageListItem,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors();
        let selected = self.selected_message_id == Some(message.id);
        let bg_selected = colors.element_selected;
        let hover = colors.element_hover;
        let border = colors.border_variant;
        let accent = colors.accent;

        let sender_weight = if message.unread {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        };

        // Unread indicator (blue dot) or equivalent spacing.
        let unread_dot = div().w(px(8.0)).flex_shrink_0().child(if message.unread {
            div()
                .size(px(8.0))
                .rounded_full()
                .bg(accent)
                .into_any_element()
        } else {
            div().into_any_element()
        });

        let mut meta = h_flex().gap_1();
        if message.starred {
            meta = meta.child(
                Icon::new(IconName::StarFilled)
                    .size(IconSize::XSmall)
                    .color(Color::Warning),
            );
        }
        if message.has_attachment {
            meta = meta.child(
                Icon::new(IconName::Attachment)
                    .size(IconSize::XSmall)
                    .color(Color::Muted),
            );
        }
        meta = meta.child(
            Label::new(message.received_at.clone())
                .size(LabelSize::XSmall)
                .color(Color::Muted),
        );

        let message_id = message.id;
        h_flex()
            .id(("message", message_id as u64))
            .w_full()
            .items_start()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(border)
            .when(selected, |el| el.bg(bg_selected))
            .when(!selected, |el| el.hover(move |el| el.bg(hover)))
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().pt_1().child(unread_dot))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .justify_between()
                            .gap_2()
                            .child(
                                Label::new(message.sender.clone())
                                    .weight(sender_weight)
                                    .single_line(),
                            )
                            .child(meta),
                    )
                    .child(
                        Label::new(message.subject.clone())
                            .size(LabelSize::Small)
                            .single_line(),
                    )
                    .child(
                        Label::new(message.preview.clone())
                            .size(LabelSize::Small)
                            .color(Color::Muted)
                            .single_line(),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selected_message_id = Some(message_id);
                this.last_webview_sig = None;
                this.reload_selected_detail();
                cx.notify();
            }))
    }

    // ----- Reading pane ---------------------------------------------------

    /// Whether the reader should show the *blocked* (red) privacy shield for the
    /// current message: blocking is globally on, the message has remote content,
    /// and the user hasn't unblocked this specific message yet.
    fn shield_is_blocked(&self) -> bool {
        !self.load_remote_images && self.content_has_remote && self.content_blocked_count > 0
    }

    /// Whether the reader should show the *loaded* (green) privacy shield: the
    /// message has remote images and none remain blocked, because the user
    /// unblocked them — all at once via the menu, or one by one in the body.
    fn shield_is_loaded(&self) -> bool {
        !self.load_remote_images && self.content_has_remote && self.content_blocked_count == 0
    }

    /// Privacy affordance shown on the subject line. A same-sized empty slot is
    /// reserved when there is nothing to show, so the header keeps an identical
    /// height either way. When the message has remote content it shows either a
    /// red shield (blocked — click to open the unblock menu) or a green
    /// shield-with-check (this message was unblocked).
    fn render_privacy_shield(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let slot = div().flex_shrink_0().size(px(20.0));
        let language = cx.language();
        let colors = cx.theme().colors();

        if self.shield_is_loaded() {
            // Green shield with a check overlaid in the center, signaling that
            // this message's remote content is now loaded.
            return slot
                .id("privacy-shield")
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .tooltip(Tooltip::text(Key::RemoteContentLoaded.tr(language)))
                .child(
                    Icon::new(IconName::ShieldSolid)
                        .size(IconSize::Medium)
                        .color(Color::Success),
                )
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div().relative().top(px(-1.0)).child(
                                Icon::new(IconName::Check)
                                    .size(IconSize::XXSmall)
                                    .color(Color::Custom(colors.background)),
                            ),
                        ),
                )
                .into_any_element();
        }

        if !self.shield_is_blocked() {
            return slot.into_any_element();
        }

        slot.id("privacy-shield")
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .cursor_pointer()
            .hover(|el| el.bg(colors.element_hover))
            .tooltip(Tooltip::text(
                Key::BlockedElements
                    .tr(language)
                    .replace("{}", &self.content_blocked_count.to_string()),
            ))
            .child(
                Icon::new(IconName::Shield)
                    .size(IconSize::Small)
                    .color(Color::Error),
            )
            .on_click(cx.listener(|this, event: &ClickEvent, _, cx| {
                this.privacy_menu_pos = event.position();
                this.privacy_menu_open = !this.privacy_menu_open;
                cx.notify();
            }))
            .into_any_element()
    }

    /// The dropdown summoned by the privacy shield. Anchored to the click and
    /// deferred so it paints above everything; the embedded webview is hidden
    /// while it is open (a native child view would otherwise occlude it).
    fn render_privacy_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let language = cx.language();
        let menu = v_flex()
            .occlude()
            .min_w(px(240.0))
            .p_1()
            .bg(colors.elevated_surface_background)
            .border_1()
            .border_color(colors.border)
            .rounded_md()
            .shadow_lg()
            .child(
                h_flex()
                    .id("unblock-remote")
                    .w_full()
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(|el| el.bg(colors.element_hover))
                    .child(
                        Icon::new(IconName::Shield)
                            .size(IconSize::Small)
                            .color(Color::Muted),
                    )
                    .child(Label::new(Key::UnblockRemote.tr(language)).size(LabelSize::Small))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.privacy_menu_open = false;
                        if let Some(id) = this.selected_message_id {
                            this.unblocked_messages.insert(id);
                        }
                        cx.notify();
                    })),
            );

        // Full-window catcher closes the menu on any outside click; the menu
        // itself occludes so its own clicks don't fall through to it.
        div()
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.privacy_menu_open = false;
                    cx.notify();
                }),
            )
            .child(
                deferred(
                    anchored()
                        .position(self.privacy_menu_pos)
                        .anchor(Corner::TopRight)
                        .snap_to_window_with_margin(px(8.0))
                        .child(menu),
                )
                .with_priority(1),
            )
    }

    fn render_reader(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let bg = colors.background;
        let border = colors.border;
        let accent = colors.accent;
        let on_accent = colors.text_on_accent;

        let Some(message) = self.selected_detail.as_ref() else {
            let language = cx.language();
            return v_flex()
                .flex_1()
                .min_w_0()
                .h_full()
                .bg(bg)
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_size(px(24.0))
                        .text_color(colors.text_muted)
                        .child(Key::ReaderNoSelection.tr(language)),
                )
                .into_any_element();
        };

        let initial = message
            .sender
            .chars()
            .next()
            .unwrap_or('?')
            .to_uppercase()
            .to_string();

        // Fixed header (does not scroll with the body, so it stays put while the
        // content scrolls vertically or horizontally).
        let header = v_flex()
            .flex_shrink_0()
            .w_full()
            .px_6()
            .py_2p5()
            .gap_2()
            .border_b_1()
            .border_color(border)
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        div().flex_1().min_w_0().child(
                            Label::new(message.subject.clone())
                                .size(LabelSize::Large)
                                .bold()
                                .single_line(),
                        ),
                    )
                    .child(self.render_privacy_shield(cx)),
            )
            .child(
                h_flex()
                    .gap_2p5()
                    .items_center()
                    .child(
                        div()
                            .size(px(30.0))
                            .flex_shrink_0()
                            .rounded_full()
                            .bg(accent)
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(on_accent)
                            .text_size(px(15.0))
                            // Tight line height so the glyph (not the looser line
                            // box) is what gets centered vertically.
                            .line_height(px(15.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(initial),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                Label::new(message.sender.clone())
                                    .weight(FontWeight::SEMIBOLD)
                                    .single_line(),
                            )
                            .child(
                                Label::new(message.sender_email.clone())
                                    .size(LabelSize::Small)
                                    .color(Color::Muted)
                                    .single_line(),
                            ),
                    )
                    .child(
                        Label::new(message.received_at.clone())
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    ),
            );

        // The body is rendered by the native webview, which we lay out over this
        // region on every paint (the webview engine handles scrolling, text
        // selection and copy). On targets without a webview backend we fall back
        // to a simple text view so the app still works.
        let body_area = if WEBVIEW_SUPPORTED {
            let view = cx.weak_entity();
            div()
                .flex_1()
                .min_h_0()
                .min_w_0()
                // Match the forced-white body so no themed background shows behind
                // the webview while it loads or at sub-pixel edges.
                .when(self.reader_white_background, |el| el.bg(gpui::white()))
                .child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, _window, cx| {
                            let _ = view.update(cx, |this, _| {
                                if let Some(webview) = &mut this.email_webview {
                                    webview.position(bounds);
                                }
                            });
                        },
                    )
                    .size_full(),
                )
                .into_any_element()
        } else {
            self.render_text_fallback(message, cx).into_any_element()
        };

        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(bg)
            .child(header)
            .child(body_area)
            .into_any_element()
    }

    /// Plain-text reader used where the embedded webview isn't available.
    fn render_text_fallback(
        &self,
        message: &MessageDetail,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors();
        let text = if message.raw_format == "text" {
            message.raw_content.clone().into()
        } else {
            SharedString::from("HTML preview is only available on macOS and Windows for now.")
        };

        div()
            .id("reader-fallback")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_6()
            .py_4()
            .text_size(px(14.0))
            .text_color(colors.text)
            .child(text)
    }

    // ----- Status bar -----------------------------------------------------

    fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let language = cx.language();
        let total_unread: usize = self.db.global_unread(storage::system::INBOX).unwrap_or(0);
        let total_messages = self
            .db
            .message_count_all()
            .unwrap_or(self.list_messages.len());
        let left_status = if self.search_is_active(cx) {
            locale::status_search_counts(
                language,
                self.accounts.len(),
                self.list_messages.len(),
                total_messages,
            )
        } else {
            locale::status_counts(language, self.accounts.len(), self.list_messages.len())
        };
        h_flex()
            .relative()
            .h(px(24.0))
            .w_full()
            .flex_shrink_0()
            .px_3()
            .gap_2()
            .bg(colors.status_bar_background)
            .border_t_1()
            .border_color(colors.border)
            .justify_between()
            .child(
                Label::new(left_status)
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
            .child(
                Label::new(locale::status_unread(language, total_unread))
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
            // Hovered link URL, centered over the bar (a chip-colored backing
            // keeps it legible above the side counters when it gets long).
            .when(!self.hovered_link.is_empty(), |this| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .max_w(px(560.0))
                                .px_2()
                                .bg(colors.status_bar_background)
                                .child(
                                    Label::new(self.hovered_link.clone())
                                        .size(LabelSize::XSmall)
                                        .color(Color::Muted)
                                        .single_line(),
                                ),
                        ),
                )
            })
    }
}

/// Standalone preferences window content (Zed-style: settings open in their own
/// window). Theme and language are app-wide globals, so changes made here are
/// reflected in the main window immediately.
struct SettingsView {
    accounts: Vec<data::Account>,
    section: SettingsSection,
    /// Back-reference to the main view, used to read and toggle preferences that
    /// live there (e.g. loading remote images). Weak so the settings window can
    /// outlive nothing it shouldn't.
    root: WeakEntity<RootView>,
}

impl SettingsView {
    fn new(accounts: Vec<data::Account>, root: WeakEntity<RootView>) -> Self {
        Self {
            accounts,
            section: SettingsSection::General,
            root,
        }
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

    fn render_content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let section = self.section;
        let appearance = cx.theme().appearance();
        let language = cx.language();

        let content = match section {
            SettingsSection::General => v_flex()
                .gap_2()
                .child(settings_row(Key::AppNameLabel.tr(language), "rMail"))
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
                .into_any_element(),
            SettingsSection::Accounts => {
                let mut list = v_flex().gap_2();
                for account in &self.accounts {
                    list = list.child(settings_row(account.name.clone(), account.email.clone()));
                }
                list.child(
                    Button::new("add-account", Key::AddAccount.tr(language))
                        .style(ButtonStyle::Filled),
                )
                .into_any_element()
            }
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
            SettingsSection::Notifications => v_flex()
                .gap_2()
                .child(settings_row(
                    Key::DesktopNotifications.tr(language),
                    Key::Enabled.tr(language),
                ))
                .child(settings_row(
                    Key::SoundOnNewEmail.tr(language),
                    Key::Disabled.tr(language),
                ))
                .into_any_element(),
            SettingsSection::Privacy => {
                let load_remote = self
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
                        h_flex()
                            .gap_2()
                            .pt_1()
                            .child(self.remote_images_button(
                                "remote-on",
                                Key::Enabled.tr(language),
                                load_remote,
                                true,
                                cx,
                            ))
                            .child(self.remote_images_button(
                                "remote-off",
                                Key::Disabled.tr(language),
                                !load_remote,
                                false,
                                cx,
                            )),
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

    /// One button of the remote-images on/off segmented control. `selected` marks
    /// the active choice (filled); clicking sets the preference to `value` on the
    /// main view.
    fn remote_images_button(
        &self,
        id: &'static str,
        label: &'static str,
        selected: bool,
        value: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        Button::new(id, label)
            .style(if selected {
                ButtonStyle::Filled
            } else {
                ButtonStyle::Subtle
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                let _ = this
                    .root
                    .update(cx, |root, cx| root.set_load_remote_images(value, cx));
                cx.notify();
            }))
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();

        h_flex()
            .size_full()
            .bg(colors.background)
            .text_color(colors.text)
            .font_family("Helvetica")
            .child(self.render_nav(cx))
            .child(
                div()
                    .id("settings-content")
                    .flex_1()
                    .h_full()
                    .overflow_y_scroll()
                    .child(self.render_content(cx)),
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

impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Until the window has settled at its final size (Windows maximize-on-open
        // is asynchronous; see `content_ready`), show only a full-size background.
        // Laying the real UI out against the not-yet-grown viewport would size it
        // to the small base bounds and leave the maximized window mostly empty.
        if !self.content_ready {
            return div()
                .size_full()
                .bg(cx.theme().colors().background)
                .into_any_element();
        }

        static FIRST_READY_FRAME: std::sync::OnceLock<()> = std::sync::OnceLock::new();
        if FIRST_READY_FRAME.set(()).is_ok() {
            crate::startup::log_milestone("first ready frame");
        }

        // Make sure the scrollbar state entities exist before the panels render.
        self.ensure_scrollbar_states(cx);
        self.ensure_search_input(cx.language(), cx);
        if !self.search_is_compact() {
            self.search_force_expanded = false;
        }
        // Keep the embedded e-mail webview in sync with the selection and theme.
        self.sync_webview(window, cx);

        let background = cx.theme().colors().background;
        let text = cx.theme().colors().text;
        let scrim = cx.theme().colors().elevated_surface_background;

        // Reconcile responsive state with the live window width before laying out.
        self.sync_layout(window.viewport_size().width);

        // Persist (debounced) the maximize state and, while *not* maximized, the
        // window position/size. When maximized we deliberately keep the previous
        // (restore) bounds so the full-screen frame is never persisted. Skipped
        // entirely until persistence is armed, so startup never overwrites the
        // saved layout.
        if self.persist_ready {
            let maximized = window.is_maximized();
            let mut changed = maximized != self.maximized;
            self.maximized = maximized;
            if let WindowBounds::Windowed(bounds) = window.window_bounds() {
                if maximized {
                    // Capture the maximized frame so we can reopen directly at it,
                    // while keeping the restore (non-maximized) bounds untouched.
                    if bounds.origin != self.max_origin || bounds.size != self.max_size {
                        self.max_origin = bounds.origin;
                        self.max_size = bounds.size;
                        changed = true;
                    }
                } else if bounds.origin != self.window_origin || bounds.size != self.window_size {
                    self.window_origin = bounds.origin;
                    self.window_size = bounds.size;
                    changed = true;
                }
            }
            if changed {
                self.request_save(cx);
            }
        }

        let docked_sidebar = self.show_sidebar && !self.narrow;
        let floating_sidebar = self.show_sidebar && self.narrow;

        let mut row = h_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .on_drag_move(
                cx.listener(|this, event: &DragMoveEvent<ResizeDrag>, _, cx| {
                    let ResizeDrag(handle) = *event.drag(cx);
                    let total = event.bounds.size.width;
                    let x = event.event.position.x - event.bounds.left();
                    this.resize(handle, x, total);
                    this.request_save(cx);
                    cx.notify();
                }),
            );
        if docked_sidebar {
            row = row.child(self.render_sidebar(false, cx));
        }
        row = row
            .child(self.render_message_list(cx))
            .child(self.render_reader(cx));

        let body = if floating_sidebar {
            // Wrap the columns so the sidebar can float on top. The scrim and the
            // floating sidebar both occlude the mouse so hovering them never
            // reaches the columns beneath.
            v_flex()
                .relative()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(row)
                .child(
                    div()
                        .id("sidebar-scrim")
                        .absolute()
                        .inset_0()
                        .occlude()
                        .bg(scrim.opacity(0.4))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_sidebar = false;
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .h_full()
                        .occlude()
                        .child(self.render_sidebar(true, cx)),
                )
                .into_any_element()
        } else {
            row.into_any_element()
        };

        v_flex()
            .size_full()
            .bg(background)
            .text_color(text)
            .font_family("Helvetica")
            // Any click on the GPUI UI dismisses a context menu open inside the
            // webview: such clicks neither blur the webview nor reach its DOM, so
            // the menu would otherwise linger. Capture phase so it still fires
            // when children stop propagation (e.g. buttons).
            .capture_any_mouse_down(cx.listener(|this, _, _, _| {
                if let Some(webview) = &this.email_webview {
                    webview.dismiss_context_menu();
                }
            }))
            .child(self.render_toolbar(window, cx))
            .child(body)
            .child(self.render_status_bar(cx))
            .when(self.privacy_menu_open, |el| {
                el.child(self.render_privacy_menu(cx))
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_view() -> (TempDir, RootView) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mail.db");
        let view = RootView::new_with_database(Config::default(), &path);
        (dir, view)
    }

    #[test]
    fn sidebar_starts_visible() {
        let (_dir, view) = test_view();
        assert!(view.show_sidebar);
    }

    #[test]
    fn toggle_sidebar_flips_visibility() {
        let (_dir, mut view) = test_view();
        view.toggle_sidebar();
        assert!(!view.show_sidebar);
        view.toggle_sidebar();
        assert!(view.show_sidebar);
    }

    #[test]
    fn selecting_in_narrow_layout_dismisses_floating_sidebar() {
        let (_dir, mut view) = test_view();
        view.narrow = true;
        view.show_sidebar = true;
        view.apply_mailbox_selection(Selection::Global(GlobalMailbox::Sent));
        assert_eq!(
            view.selected_mailbox,
            Selection::Global(GlobalMailbox::Sent)
        );
        assert!(
            !view.show_sidebar,
            "narrow selection should close the sidebar"
        );
    }

    #[test]
    fn selecting_in_wide_layout_keeps_sidebar_open() {
        let (_dir, mut view) = test_view();
        view.narrow = false;
        view.show_sidebar = true;
        let account_id = view.accounts[0].id;
        let folder_path = view.folders_by_account[&account_id][0].path.clone();
        view.apply_mailbox_selection(Selection::Mailbox {
            account_id,
            folder_path: folder_path.clone(),
        });
        assert_eq!(
            view.selected_mailbox,
            Selection::Mailbox {
                account_id,
                folder_path,
            }
        );
        assert!(view.show_sidebar, "docked sidebar should stay open");
    }

    #[test]
    fn mailbox_change_clears_reader_selection() {
        let (_dir, mut view) = test_view();
        assert!(view.selected_message_id.is_some());
        view.clear_message_selection();
        view.apply_mailbox_selection(Selection::Global(GlobalMailbox::Sent));
        view.reload_message_list();
        assert!(view.selected_message_id.is_none());
        assert!(view.selected_detail.is_none());
    }

    #[test]
    fn default_selection_is_global_inbox() {
        let (_dir, view) = test_view();
        assert_eq!(
            view.selected_mailbox,
            Selection::Global(GlobalMailbox::Inbox)
        );
    }

    #[test]
    fn global_inbox_has_unread() {
        let (_dir, view) = test_view();
        assert!(view.global_unread(GlobalMailbox::Inbox) > 0);
    }

    #[test]
    fn global_flagged_counts_flagged_folder() {
        let (_dir, view) = test_view();
        let flagged = view.db.global_unread(storage::system::FLAGGED).unwrap();
        assert_eq!(view.global_unread(GlobalMailbox::Flagged), flagged);
        assert!(flagged > 0);
    }

    #[test]
    fn global_drafts_and_sent_have_no_unread() {
        let (_dir, view) = test_view();
        assert_eq!(view.global_unread(GlobalMailbox::Drafts), 0);
        assert_eq!(view.global_unread(GlobalMailbox::Sent), 0);
    }

    #[test]
    fn accounts_start_expanded() {
        let (_dir, view) = test_view();
        for idx in 0..view.accounts.len() {
            assert!(!view.is_account_collapsed(idx));
        }
    }

    #[test]
    fn toggle_account_collapses_and_expands() {
        let (_dir, mut view) = test_view();
        let collapsing = view.toggle_account(1);
        assert!(collapsing.collapsing);
        assert!(view.is_account_collapsed(1));
        // Other accounts are unaffected.
        assert!(!view.is_account_collapsed(0));
        let expanding = view.toggle_account(1);
        assert!(!expanding.collapsing);
        assert!(!view.is_account_collapsed(1));
    }

    #[test]
    fn toggle_account_hands_out_unique_tokens() {
        let (_dir, mut view) = test_view();
        let first = view.toggle_account(0).token;
        let second = view.toggle_account(0).token;
        assert_ne!(first, second);
    }

    #[test]
    fn collapsing_account_stays_visible_until_finalized() {
        let (_dir, mut view) = test_view();
        let anim = view.toggle_account(2);
        // Mid-collapse the list is still rendered so it can animate out.
        assert!(view.is_account_collapsed(2));
        assert!(view.account_list_visible(2));
        // Once the animation finalizes the list is gone.
        assert!(view.clear_fold(2, anim.token));
        assert!(!view.account_list_visible(2));
    }

    #[test]
    fn clear_fold_ignores_stale_token() {
        let (_dir, mut view) = test_view();
        let stale = view.toggle_account(0).token;
        // A second toggle supersedes the first, bumping the token.
        let _current = view.toggle_account(0);
        assert!(!view.clear_fold(0, stale));
        // The current animation is untouched and still in flight.
        assert!(view.fold_anim.contains_key(&0));
    }

    #[test]
    fn expanded_account_is_visible_without_animation() {
        let (_dir, view) = test_view();
        assert!(view.account_list_visible(0));
    }

    #[test]
    fn narrow_window_auto_collapses_sidebar() {
        let (_dir, mut view) = test_view();
        view.sync_layout(px(800.0));
        assert!(view.narrow);
        assert!(!view.show_sidebar);
    }

    #[test]
    fn resizing_while_narrow_recollapses_floating_sidebar() {
        let (_dir, mut view) = test_view();
        view.sync_layout(px(850.0));
        // User reopens the sidebar as a floating overlay.
        view.show_sidebar = true;
        // Any resize while still narrow collapses it again.
        view.sync_layout(px(840.0));
        assert!(view.narrow);
        assert!(!view.show_sidebar);
    }

    #[test]
    fn floating_sidebar_stays_open_without_resize() {
        let (_dir, mut view) = test_view();
        view.sync_layout(px(850.0));
        view.show_sidebar = true;
        // Re-render at the same width (e.g. from an unrelated notify) keeps it.
        view.sync_layout(px(850.0));
        assert!(view.show_sidebar);
    }

    #[test]
    fn widening_window_restores_sidebar() {
        let (_dir, mut view) = test_view();
        view.sync_layout(px(800.0));
        assert!(!view.show_sidebar);
        view.sync_layout(px(1200.0));
        assert!(!view.narrow);
        assert!(view.show_sidebar);
    }

    #[test]
    fn resize_respects_sidebar_minimum() {
        let (_dir, mut view) = test_view();
        view.sync_layout(px(1400.0));
        view.resize(ResizeHandle::Sidebar, px(50.0), px(1400.0));
        assert_eq!(view.sidebar_width, px(SIDEBAR_MIN_WIDTH));
    }

    #[test]
    fn resize_respects_list_minimum() {
        let (_dir, mut view) = test_view();
        view.sync_layout(px(1400.0));
        // Drag the list/reader divider far to the left (x near the sidebar edge).
        view.resize(
            ResizeHandle::List,
            view.sidebar_width + px(10.0),
            px(1400.0),
        );
        assert_eq!(view.list_width, px(LIST_MIN_WIDTH));
    }

    #[test]
    fn resize_keeps_reader_minimum() {
        let total = px(1400.0);
        let (_dir, mut view) = test_view();
        view.sync_layout(total);
        // Try to make the list fill the whole row; the reader floor must hold.
        view.resize(ResizeHandle::List, total, total);
        let reader = total - view.sidebar_width - view.list_width;
        assert!(reader >= px(READER_MIN_WIDTH));
    }

    /// Dragging either divider hard to the right must never grow a column past
    /// the point where the reader would drop below `READER_MIN_WIDTH`. Once that
    /// floor is reached the divider is locked, so the reader (and the window
    /// controls riding above it) can't be pushed off-screen.
    #[test]
    fn resize_locks_panels_once_reader_minimum_is_reached() {
        let total = px(1400.0);
        let (_dir, mut view) = test_view();
        view.sync_layout(total);

        // Grow the list as far right as the cursor can go.
        view.resize(ResizeHandle::List, total * 2.0, total);
        // Then try to grow the sidebar past the list, and hammer both again.
        view.resize(ResizeHandle::Sidebar, total * 2.0, total);
        view.resize(ResizeHandle::List, total * 2.0, total);
        view.resize(ResizeHandle::Sidebar, total * 2.0, total);

        let used = view.sidebar_width + view.list_width;
        assert!(
            used <= total - px(READER_MIN_WIDTH),
            "sidebar+list ({used:?}) left the reader below its {READER_MIN_WIDTH}px floor",
        );
    }

    #[test]
    fn search_collapses_when_reader_segment_is_narrow() {
        let (_dir, mut view) = test_view();
        // Just-docked layout (at the narrow breakpoint): the sidebar and list eat
        // most of the row, leaving the reader's toolbar segment below the collapse
        // threshold, so the search field turns into an icon. Holds on every
        // platform (the Windows caption-button reserve only makes it narrower).
        view.sync_layout(px(NARROW_BREAKPOINT));
        assert!(view.search_is_compact());
    }

    #[test]
    fn search_expands_on_wide_reader_segment() {
        let (_dir, mut view) = test_view();
        // Wide docked layout leaves the reader segment well above the threshold.
        view.sync_layout(px(1600.0));
        assert!(!view.search_is_compact());
    }

    #[test]
    fn all_action_groups_show_on_wide_window() {
        let (_dir, mut view) = test_view();
        view.sync_layout(px(1600.0));
        assert_eq!(view.visible_action_groups(), 3);
    }

    #[test]
    fn action_groups_drop_as_reader_segment_shrinks() {
        let (_dir, mut view) = test_view();
        view.narrow = false;
        view.show_sidebar = true;
        view.sidebar_width = px(SIDEBAR_MIN_WIDTH);
        view.list_width = px(LIST_MIN_WIDTH);

        // Reader segment = window - 250 - 350. Shrinking it drops groups, but the
        // search button must always remain (groups never reach a negative count).
        view.window_width = px(1100.0);
        let wide = view.visible_action_groups();
        view.window_width = px(820.0);
        let narrow = view.visible_action_groups();
        assert!(narrow < wide);
    }

    #[test]
    fn action_groups_vanish_but_search_survives_when_tiny() {
        let (_dir, mut view) = test_view();
        view.narrow = true;
        view.show_sidebar = false;
        // Artificially tiny reader segment: every action group is dropped.
        view.window_width = px(400.0);
        assert_eq!(view.visible_action_groups(), 0);
        // The search button is independent of the groups and stays visible.
        assert!(view.search_is_compact());
    }

    #[test]
    fn sync_layout_shrinks_columns_to_fit() {
        let (_dir, mut view) = test_view();
        // Wide (docked) window so the sidebar keeps its column; an oversized list
        // must shrink to preserve the reader's minimum width.
        view.list_width = px(900.0);
        view.sync_layout(px(1400.0));
        let reader = px(1400.0) - view.sidebar_width - view.list_width;
        assert!(reader >= px(READER_MIN_WIDTH));
        assert!(view.list_width >= px(LIST_MIN_WIDTH));
    }

    #[test]
    fn search_force_expanded_overrides_compact_layout() {
        let (_dir, mut view) = test_view();
        view.sync_layout(px(NARROW_BREAKPOINT));
        assert!(view.search_is_compact());
        assert!(!view.show_search_expanded());
        view.prepare_search_focus();
        assert!(view.show_search_expanded());
    }

    #[test]
    fn search_debounce_token_invalidates_previous_timer() {
        let (_dir, mut view) = test_view();
        let first = view.next_search_debounce_token();
        assert!(view.search_debounce_token_current(first));

        let second = view.next_search_debounce_token();
        assert!(!view.search_debounce_token_current(first));
        assert!(view.search_debounce_token_current(second));
    }
}
