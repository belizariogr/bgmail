//! Root view of the BGMail visual prototype.
//!
//! Reproduces the macOS mail client layout (three columns, a unified top toolbar
//! and a status bar), using the components from the `ui` crate. All interaction
//! here is "mock" — only item selection, theme toggling and language switching.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use std::f32::consts::FRAC_PI_2;

use gpui::{
    anchored, deferred, ease_in_out, point, radians, size, svg, Animation, AnimationExt as _, App,
    AppContext as _, Bounds, ClipboardItem, Context, Corner, DragMoveEvent, Empty, Entity,
    Focusable, FontWeight, Hsla, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, Point,
    ScrollHandle, Size, Subscription, Svg, TitlebarOptions, Transformation, WeakEntity, Window,
    WindowBounds, WindowControlArea, WindowHandle, WindowOptions,
};

use crate::command_palette_overlay;
use theme::{ActiveTheme, Appearance};
use ui::prelude::*;
use ui::{
    set_native_text_focus, Button, ButtonStyle, Color, Icon, IconButton, IconName, IconSize, Label,
    LabelSize, ListItem, Scrollbar, ScrollbarState, Switch, TextInput, Tooltip,
};

use crate::actions::{ComposeClose, FindInMessage, ToggleCommandPalette};
use crate::app_menus;
use crate::command_palette::CommandPaletteState;
use crate::commands::{CommandContext, CommandId};
use crate::compose::{self, ComposeView};
use crate::config::{self, Config};
use crate::crash_guard::CrashGuard;
use crate::data::{self, GlobalMailbox, MailboxKind, MessageBody};
use crate::db_seed;
use crate::locale::{self, ActiveLanguage, Key, Language};
use crate::mail_html::{
    email_document, is_openable_link, plain_text, print_document, DocumentColors, PrintHeader,
};
use crate::shortcuts;
use html_view::{
    Appearance as PageAppearance, DocumentOptions, HitInfo, HtmlDocument, HtmlView, HtmlViewEvent,
    RenderError,
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

/// Memoization key for the reader: the inputs that affect the rendered HTML
/// document. When unchanged between renders (e.g. across animation frames)
/// the body is not re-sanitized nor re-parsed.
#[derive(Clone, Copy, PartialEq)]
struct ReaderSignature {
    selected: i64,
    colors: DocumentColors,
    /// The user asked to show a large message formatted anyway.
    formatted_anyway: bool,
    /// Remote content loads (always, or allowed for this message).
    remote: bool,
}

/// HTML bodies larger than this are shown as plain text unless the user asks
/// for the formatted version: formatting runs on the UI thread (~100 ms per
/// MB measured), so a huge newsletter would freeze the window.
const MAX_FORMATTED_HTML_BYTES: usize = 1024 * 1024;

/// Why the reader shows a message as plain text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReaderNotice {
    /// Bigger than [`MAX_FORMATTED_HTML_BYTES`] (can be shown formatted).
    TooLarge,
    /// The renderer refused it (`RenderError`).
    Unrenderable,
    /// The app closed while rendering it last time (see [`CrashGuard`]).
    Crashed,
}

/// Plain-text fallback reason for an HTML body (`None`: render formatted):
/// a crash while rendering it wins, then the size limit unless overridden.
fn plain_text_reason(
    html_len: usize,
    crashed: bool,
    formatted_anyway: bool,
) -> Option<ReaderNotice> {
    if crashed {
        Some(ReaderNotice::Crashed)
    } else if html_len > MAX_FORMATTED_HTML_BYTES && !formatted_anyway {
        Some(ReaderNotice::TooLarge)
    } else {
        None
    }
}

/// A reader context menu: where it was summoned and what it applies to.
struct ReaderMenu {
    position: Point<Pixels>,
    hit: HitInfo,
    has_selection: bool,
}

/// The reader's find bar: query field and the current match.
struct FindBar {
    input: Entity<TextInput>,
    /// Number of matches of the current query.
    count: usize,
    /// Index of the selected match (`< count` when there are matches).
    current: usize,
    _observation: Subscription,
}

/// Index of the find match after moving `step` (±1) from `current` among
/// `count` matches, wrapping around both ends.
fn step_match(current: usize, count: usize, step: isize) -> usize {
    if count == 0 {
        return 0;
    }
    (current as isize + step).rem_euclid(count as isize) as usize
}

/// Text selection highlight of the reader (RGBA): the accent at a strength
/// that keeps dark and light text readable on any page.
fn reader_selection_color(accent: Hsla) -> [u8; 4] {
    let rgba: gpui::Rgba = accent.opacity(0.35).into();
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    [
        channel(rgba.r),
        channel(rgba.g),
        channel(rgba.b),
        channel(rgba.a),
    ]
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
    /// Destination URL of the link currently under the cursor in the reader
    /// (empty when none). Shown centered in the status bar.
    hovered_link: String,
    /// The reader's page zoom (1 = 100%), shown in the status bar otherwise.
    reader_zoom: f32,
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
    /// Native HTML view showing the selected message's body (`html_view`:
    /// Blitz layout painted by GPUI). Created on first use.
    reader_view: Option<Entity<HtmlView>>,
    /// Subscription to the reader's events (hovered/clicked links, menus).
    _reader_subscription: Option<Subscription>,
    /// Context menu opened by a secondary click in the reader.
    reader_menu: Option<ReaderMenu>,
    /// Find bar of the reader, while open.
    find_bar: Option<FindBar>,
    /// Why the current message is shown as plain text, if it is.
    reader_notice: Option<ReaderNotice>,
    /// Large messages the user chose to show formatted anyway.
    formatted_anyway: HashSet<i64>,
    /// Messages whose rendering crashed the app (shown as plain text).
    crashed_messages: HashSet<i64>,
    /// Marker of the message being rendered (crash-loop protection).
    crash_guard: CrashGuard,
    /// Inputs that last produced the reader document (selected message and the
    /// colors it was themed with). Used to skip re-parsing the body on every
    /// render when nothing that affects the document has changed.
    last_reader_sig: Option<ReaderSignature>,
    /// Persisted "load remote content" preference. Remote content needs
    /// networking (Stage 3), so it is kept but not offered or applied yet.
    load_remote_images: bool,
    /// Messages whose remote images the user loaded this session.
    remote_allowed: HashSet<i64>,
    /// Whether the reader always paints the message body on a white background
    /// (dark text), ignoring the app theme. Persisted; off by default.
    reader_white_background: bool,
    /// Whether the compose message body uses a white background. Persisted; off
    /// by default.
    compose_white_background: bool,
    /// How many remote images were blocked in the current message (the privacy
    /// shield shows while non-zero). Recomputed when the document rebuilds.
    content_blocked_count: usize,
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
    /// Command palette state (rendered as an in-window overlay — see
    /// [`command_palette_overlay`]).
    pub(crate) command_palette: Option<CommandPaletteState>,
    /// Focus the palette search field once after open.
    command_palette_focus_pending: bool,
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
        // Development: `BGMAIL_DB` opens another database (e.g. one seeded
        // with `BGMAIL_SAMPLE_DIR`), leaving the real one untouched.
        let db_path = std::env::var_os("BGMAIL_DB")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(storage::database_path);
        Self::new_with_database(settings, db_path)
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
        let crash_guard = CrashGuard::beside(db_path.as_ref());
        let db = storage::Database::open(db_path).expect("failed to open mail database");
        let mut seed_messages = db_seed::seed_messages();
        if let Some(dir) = std::env::var_os("BGMAIL_SAMPLE_DIR") {
            seed_messages.extend(db_seed::sample_dir_messages(std::path::Path::new(&dir)));
        }
        storage::seed_if_empty(db.conn(), &db_seed::seed_accounts(), &seed_messages)
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
            reader_zoom: 1.0,
            settings_window: None,
            compose_window: None,
            compose_origin,
            compose_size,
            list_scroll: ScrollHandle::new(),
            list_scrollbar: None,
            sidebar_scroll: ScrollHandle::new(),
            sidebar_scrollbar: None,
            reader_view: None,
            _reader_subscription: None,
            reader_menu: None,
            find_bar: None,
            reader_notice: None,
            formatted_anyway: HashSet::new(),
            crashed_messages: crash_guard.take_unfinished().into_iter().collect(),
            crash_guard,
            last_reader_sig: None,
            load_remote_images: settings.load_remote_images,
            remote_allowed: HashSet::new(),
            reader_white_background: settings.reader_white_background,
            compose_white_background: settings.compose_white_background,
            content_blocked_count: 0,
            search_input: None,
            search_force_expanded: false,
            last_search_query: String::new(),
            search_debounce_seq: 0,
            pre_search_mailbox: None,
            command_palette: None,
            command_palette_focus_pending: false,
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
        self.last_reader_sig = None;
    }

    fn reload_selected_detail(&mut self) {
        self.selected_detail = self
            .selected_message_id
            .and_then(|id| self.db.get_message(id).ok().flatten());
    }

    fn on_message_selection_changed(&mut self, cx: &mut Context<Self>) {
        self.reload_selected_detail();
        self.sync_app_menus(cx);
        cx.notify();
    }

    /// Builds the command context snapshot used by menus and the palette.
    pub(crate) fn command_context(&self) -> CommandContext {
        CommandContext {
            selected_message_id: self.selected_message_id,
            message_detail: self.selected_detail.clone(),
            folders_by_account: self.folders_by_account.clone(),
        }
    }

    /// Refreshes the global menu bar (disabled states follow the current selection).
    pub(crate) fn sync_app_menus(&self, cx: &mut Context<Self>) {
        if self.compose_window_open_and_active(cx) {
            return;
        }
        let ctx = self.command_context();
        let language = cx.language();
        app_menus::sync_menus(cx, &ctx, language);
    }

    /// Called when the compose window closes (menu, shortcut, or traffic lights).
    pub(crate) fn on_compose_window_closed(&mut self, cx: &mut Context<Self>) {
        self.compose_window = None;
        self.persist_now();
        self.sync_app_menus(cx);
    }

    /// Called when the settings window closes (menu, shortcut, or traffic lights).
    pub(crate) fn on_settings_window_closed(&mut self, cx: &mut Context<Self>) {
        self.settings_window = None;
        self.sync_app_menus(cx);
        cx.notify();
    }

    /// Closes the auxiliary window that currently has focus (settings or compose).
    pub(crate) fn close_focused_auxiliary_window(&mut self, cx: &mut Context<Self>) {
        if self
            .settings_window
            .as_ref()
            .is_some_and(|handle| handle.is_active(cx).unwrap_or(false))
        {
            self.close_settings_window(cx);
            return;
        }
        let _ = self.with_compose_window(cx, |view, window, cx| {
            view.close_window(window, cx);
        });
    }

    /// Closes the settings window when it is open (global shortcut / menu fallback).
    pub(crate) fn close_settings_window(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.settings_window.as_ref() {
            let _ = handle.update(cx, |view, window, cx| {
                view.close_window(window, cx);
            });
        }
    }

    fn compose_window_open_and_active(&self, cx: &mut Context<Self>) -> bool {
        self.compose_window
            .as_ref()
            .is_some_and(|handle| handle.is_active(cx).unwrap_or(false))
    }

    pub(crate) fn with_compose_window<R>(
        &self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut ComposeView, &mut Window, &mut Context<ComposeView>) -> R,
    ) -> Option<R> {
        self.compose_window.as_ref().and_then(|compose| {
            compose
                .update(cx, |view, window, cx| f(view, window, cx))
                .ok()
        })
    }

    fn refresh_after_message_action(&mut self, cx: &mut Context<Self>) {
        self.reload_message_list();
        self.sync_app_menus(cx);
        self.last_reader_sig = None;
        cx.notify();
    }

    fn ensure_command_palette(&mut self, cx: &mut Context<Self>) {
        if self.command_palette.is_none() {
            let input = cx.new(|cx| TextInput::new("", cx));
            let root = cx.weak_entity();
            cx.observe(&input, move |_, _, cx| {
                let root = root.clone();
                cx.defer(move |cx| {
                    if let Some(root) = root.upgrade() {
                        root.update(cx, |this, cx| {
                            this.sync_command_palette_query(cx);
                        });
                    }
                });
            })
            .detach();
            self.command_palette = Some(CommandPaletteState {
                open: false,
                query: String::new(),
                selected_ix: 0,
                input: Some(input),
            });
        }
    }

    fn sync_command_palette_query(&mut self, cx: &mut Context<Self>) {
        let Some(palette) = &mut self.command_palette else {
            return;
        };
        let query = palette
            .input
            .as_ref()
            .map(|input| input.read(cx).content().to_string())
            .unwrap_or_default();
        if query != palette.query {
            palette.on_query_change(query);
            cx.notify();
        }
    }

    pub(crate) fn toggle_command_palette(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.toggle_command_palette_internal(window, cx);
    }

    fn toggle_command_palette_internal(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.ensure_command_palette(cx);
        let Some(palette) = &mut self.command_palette else {
            return;
        };
        if palette.open {
            self.close_command_palette(cx);
            return;
        }
        if !can_open_command_palette(window.is_window_active()) {
            return;
        }
        palette.open = true;
        palette.query.clear();
        palette.selected_ix = 0;
        if let Some(input) = &palette.input {
            input.update(cx, |field, cx| field.clear(cx));
        }
        self.command_palette_focus_pending = true;
        cx.notify();
    }

    pub(crate) fn dismiss_command_palette(&mut self, cx: &mut Context<Self>) {
        if let Some(palette) = &mut self.command_palette {
            palette.open = false;
        }
        self.command_palette_focus_pending = false;
        cx.notify();
    }

    pub(crate) fn close_command_palette(&mut self, cx: &mut Context<Self>) {
        self.dismiss_command_palette(cx);
    }

    /// Snapshot of palette rows for the in-window overlay.
    fn command_palette_render_args(
        &self,
        cx: &App,
    ) -> Option<(Vec<crate::commands::CommandEntry>, usize, Entity<TextInput>)> {
        let palette = self.command_palette.as_ref().filter(|p| p.open)?;
        let language = cx.language();
        let ctx = self.command_context();
        let entries = palette.filtered_entries(language, &ctx);
        let selected_ix = if entries.is_empty() {
            0
        } else {
            palette.selected_ix.min(entries.len() - 1)
        };
        let input = palette.input.clone()?;
        Some((entries, selected_ix, input))
    }

    pub(crate) fn execute_command(
        &mut self,
        id: &CommandId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match id {
            CommandId::ComposeNew => self.open_compose(window, cx),
            CommandId::OpenSettings => self.open_settings(window, cx),
            CommandId::ToggleSidebar => {
                self.toggle_sidebar();
                self.sync_app_menus(cx);
                cx.notify();
            }
            CommandId::MessageDelete => self.delete_message_to_trash(cx),
            CommandId::MessageDeletePermanent => self.delete_message_permanently(cx),
            CommandId::MessageRestore => self.restore_message(cx),
            CommandId::MessageArchive => self.archive_message(cx),
            CommandId::MessageMarkJunk => self.mark_message_junk(cx),
            CommandId::MessageToggleFlag => self.toggle_message_flag(cx),
            CommandId::MessagePrint => self.print_message(cx),
            CommandId::MoveToFolder(path) => self.move_message_to_folder(path.as_ref(), cx),
        }
        self.close_command_palette(cx);
    }

    /// Whether message `id` loads remote content: always (Settings ▸
    /// Privacy) or because the user loaded it from the privacy shield.
    fn remote_allowed_for(&self, id: i64) -> bool {
        self.load_remote_images || self.remote_allowed.contains(&id)
    }

    /// Loads the remote images of the selected message (privacy shield).
    fn load_remote_for_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected_message_id {
            self.remote_allowed.insert(id);
            cx.notify();
        }
    }

    /// Sets "always load remote images", persisting it and re-rendering the
    /// reader.
    fn set_load_remote_images(&mut self, value: bool, cx: &mut Context<Self>) {
        if self.load_remote_images != value {
            self.load_remote_images = value;
            self.request_save(cx);
            cx.refresh_windows();
        }
    }

    /// Prints the selected message: a standalone print page (light theme,
    /// header, remote content blocked; `mail_html::print_document`) written
    /// to a private temporary file and opened in the system browser, which
    /// shows the print dialog.
    pub(crate) fn print_message(&mut self, cx: &mut Context<Self>) {
        let Some(detail) = self.selected_detail.as_ref() else {
            return;
        };
        let language = cx.language();
        let from = if detail.sender_email.is_empty() {
            detail.sender.clone()
        } else {
            format!("{} <{}>", detail.sender, detail.sender_email)
        };
        let header = PrintHeader {
            subject: &detail.subject,
            from_label: Key::PrintFrom.tr(language),
            from: &from,
            date_label: Key::PrintDate.tr(language),
            date: &detail.received_at,
        };
        let body = db_seed::message_body_from_detail(detail);
        let nonce = crate::print::nonce();
        let html = print_document(&header, &body, &nonce, self.remote_allowed_for(detail.id));
        match crate::print::write_print_page(&crate::print::print_dir(), &html, &nonce) {
            Ok(path) => cx.open_with_system(&path),
            Err(error) => eprintln!("BGMail: could not write the print page: {error}"),
        }
    }

    pub(crate) fn delete_message_to_trash(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_message_id else {
            return;
        };
        let _ = self.db.move_message_to_trash(id);
        self.refresh_after_message_action(cx);
    }

    pub(crate) fn delete_message_permanently(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_message_id else {
            return;
        };
        if self.db.delete_message_permanently(id).unwrap_or(false) {
            self.clear_message_selection();
            self.refresh_after_message_action(cx);
        }
    }

    pub(crate) fn restore_message(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_message_id else {
            return;
        };
        let _ = self.db.restore_message_from_trash(id);
        self.refresh_after_message_action(cx);
    }

    pub(crate) fn archive_message(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_message_id else {
            return;
        };
        let _ = self.db.archive_message(id);
        self.refresh_after_message_action(cx);
    }

    pub(crate) fn mark_message_junk(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_message_id else {
            return;
        };
        let _ = self.db.mark_message_junk(id);
        self.refresh_after_message_action(cx);
    }

    pub(crate) fn toggle_message_flag(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_message_id else {
            return;
        };
        let _ = self.db.toggle_message_starred(id);
        self.refresh_after_message_action(cx);
    }

    pub(crate) fn move_message_to_folder(&mut self, folder_path: &str, cx: &mut Context<Self>) {
        let Some(id) = self.selected_message_id else {
            return;
        };
        let _ = self.db.move_message_to_folder(id, folder_path);
        self.refresh_after_message_action(cx);
    }

    pub(crate) fn handle_command_palette_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.command_palette.as_ref().is_some_and(|p| p.open) {
            return false;
        }
        let language = cx.language();
        let ctx = self.command_context();
        let entries = self
            .command_palette
            .as_ref()
            .expect("palette open")
            .filtered_entries(language, &ctx);
        if let Some(palette) = &mut self.command_palette {
            palette.clamp_selection(entries.len());
        }
        match event.keystroke.key.as_str() {
            "escape" => {
                self.dismiss_command_palette(cx);
                true
            }
            "up" => {
                let changed = self
                    .command_palette
                    .as_mut()
                    .expect("palette open")
                    .move_selection(-1, entries.len());
                if changed {
                    cx.notify();
                }
                true
            }
            "down" => {
                let changed = self
                    .command_palette
                    .as_mut()
                    .expect("palette open")
                    .move_selection(1, entries.len());
                if changed {
                    cx.notify();
                }
                true
            }
            "enter" => {
                if let Some(id) = self
                    .command_palette
                    .as_ref()
                    .and_then(|palette| palette.selected_command(&entries))
                    .cloned()
                {
                    self.execute_command(&id, window, cx);
                }
                true
            }
            _ => false,
        }
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
    fn ensure_search_input(
        &mut self,
        language: Language,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
            let focus_handle = input.read(cx).focus_handle(cx);
            cx.on_blur(&focus_handle, window, |this, _, cx| this.on_search_blur(cx))
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

    /// Whether the clear affordance inside the expanded search field should stay
    /// visible even when the query is empty (compact toolbar forced open).
    fn show_search_clear_in_field(&self, cx: &App) -> bool {
        self.show_search_clear_in_field_for_query(&self.search_query(cx))
    }

    fn show_search_clear_in_field_for_query(&self, query: &str) -> bool {
        if self.search_is_compact() && self.search_force_expanded {
            return true;
        }
        !query.trim().is_empty()
    }

    /// Clears the query, restores the pre-search mailbox when needed, and
    /// collapses a force-expanded compact search back to the icon button.
    fn clear_search(&mut self, cx: &mut Context<Self>) {
        let collapse = self.search_is_compact() && self.search_force_expanded;
        let had_text = !self.search_query(cx).trim().is_empty();

        if let Some(input) = &self.search_input {
            input.update(cx, |field, cx| field.clear(cx));
        }
        // Drop any pending debounced apply so stale text cannot re-enter search.
        self.next_search_debounce_token();

        let was_active = !self.last_search_query.trim().is_empty();
        if was_active || had_text {
            if let Some(prev) = self.pre_search_mailbox.take() {
                self.selected_mailbox = prev;
            }
            self.last_search_query.clear();
            self.reload_message_list();
            self.sync_search_selection();
        }

        if collapse {
            self.search_force_expanded = false;
        }
        cx.notify();
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

    fn search_input_focused(&self, window: &Window, cx: &App) -> bool {
        self.search_input
            .as_ref()
            .is_some_and(|input| input.read(cx).focus_handle(cx).is_focused(window))
    }

    /// Drops keyboard focus from the search field when it is active.
    fn blur_search_if_focused(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_input_focused(window, cx) {
            window.blur();
        }
    }

    /// Whether a compact, force-expanded search should collapse once it loses focus.
    fn should_collapse_search_after_blur(&self, query: &str) -> bool {
        self.search_is_compact() && self.search_force_expanded && query.trim().is_empty()
    }

    /// Collapses a compact search opened via the icon when it blurs while empty.
    fn on_search_blur(&mut self, cx: &mut Context<Self>) {
        if self.should_collapse_search_after_blur(&self.search_query(cx)) {
            self.search_force_expanded = false;
            cx.notify();
        }
    }

    /// Refreshes the custom caption layout when the main window is activated.
    pub(crate) fn on_main_window_activated(&mut self, cx: &mut Context<Self>) {
        crate::window_frame::refresh_caption_layout();
        cx.notify();
    }

    /// Keeps the reader document in sync with the selected message and theme:
    /// sanitizes and themes the body, parses it with `html_view` and hands it
    /// to the reader view, which lays it out at the pane width.
    fn sync_reader(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(detail) = self.selected_detail.as_ref() else {
            self.last_reader_sig = None;
            self.reader_notice = None;
            return;
        };
        let colors = cx.theme().colors();
        // Force a white page (dark text) when the user keeps the reader light,
        // regardless of the app theme; otherwise follow the theme.
        let (background, text) = if self.reader_white_background {
            (gpui::white(), READER_LIGHT_TEXT)
        } else {
            (colors.background, colors.text)
        };
        let doc_colors = DocumentColors {
            background,
            text,
            accent: colors.accent,
        };
        let id = detail.id;
        let remote = self.remote_allowed_for(id);
        let signature = ReaderSignature {
            selected: id,
            colors: doc_colors,
            formatted_anyway: self.formatted_anyway.contains(&id),
            remote,
        };
        if self.reader_view.is_some() && self.last_reader_sig == Some(signature) {
            return;
        }
        let body = db_seed::message_body_from_detail(detail);
        let mut notice = match &body {
            MessageBody::Html(html) => plain_text_reason(
                html.len(),
                self.crashed_messages.contains(&id),
                signature.formatted_anyway,
            ),
            MessageBody::Text(_) => None,
        };
        let options = || DocumentOptions {
            appearance: if doc_colors.is_dark() {
                PageAppearance::Dark
            } else {
                PageAppearance::Light
            },
            selection_color: Some(reader_selection_color(colors.accent)),
            resources: html_view::Resources::new().allow_remote(remote),
            ..Default::default()
        };
        let as_plain = |body: &MessageBody| match body {
            MessageBody::Html(html) => MessageBody::Text(plain_text(html).into()),
            text => text.clone(),
        };
        let shown = if notice.is_some() {
            as_plain(&body)
        } else {
            body.clone()
        };
        let rendered = email_document(doc_colors, &shown, remote);
        let document = match HtmlDocument::try_parse(&rendered.html, options()) {
            Ok(document) => document,
            Err(RenderError::TooDeep { .. }) => {
                notice = Some(ReaderNotice::Unrenderable);
                let fallback = email_document(doc_colors, &as_plain(&body), false);
                HtmlDocument::parse(&fallback.html, options())
            }
        };
        self.content_blocked_count = if notice.is_some() {
            0
        } else {
            rendered.blocked_images
        };
        self.reader_notice = notice;
        // A crash rendering formatted HTML must not repeat on every start:
        // mark the message until a frame showing it was drawn.
        if notice.is_none() && matches!(body, MessageBody::Html(_)) {
            self.crash_guard.begin(id);
            let guard = self.crash_guard.clone();
            window.on_next_frame(move |_, _| guard.finish());
        }
        self.reader_menu = None;
        match &self.reader_view {
            Some(view) => view.update(cx, |view, cx| view.set_document(document, cx)),
            None => {
                let view = cx.new(|cx| HtmlView::new(document, cx));
                self._reader_subscription = Some(cx.subscribe(&view, Self::on_reader_event));
                self.reader_view = Some(view);
            }
        }
        self.last_reader_sig = Some(signature);
        // Keep an open find bar's query applied to the new document.
        self.run_find(cx);
    }

    /// Notice above a message shown as plain text, explaining why; large
    /// messages offer to show the formatted version anyway.
    fn render_reader_notice(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(notice) = self.reader_notice else {
            return div().into_any_element();
        };
        let colors = cx.theme().colors();
        let language = cx.language();
        let text = match notice {
            ReaderNotice::TooLarge => Key::NoticeTooLarge,
            ReaderNotice::Unrenderable => Key::NoticeTooDeep,
            ReaderNotice::Crashed => Key::NoticeCrashed,
        }
        .tr(language);
        h_flex()
            .flex_shrink_0()
            .w_full()
            .px_6()
            .py_1p5()
            .gap_3()
            .items_center()
            .border_b_1()
            .border_color(colors.border)
            .bg(colors.surface_background)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Label::new(text).size(LabelSize::Small).color(Color::Muted)),
            )
            .when(notice == ReaderNotice::TooLarge, |el| {
                el.child(
                    Button::new("show-formatted", Key::NoticeShowFormatted.tr(language)).on_click(
                        cx.listener(|this, _, _, cx| {
                            if let Some(id) = this.selected_message_id {
                                this.formatted_anyway.insert(id);
                                cx.notify();
                            }
                        }),
                    ),
                )
            })
            .into_any_element()
    }

    /// Reacts to the reader: link under the pointer (status bar), link clicks
    /// (opened outside the app), page zoom (status bar) and context menus.
    fn on_reader_event(
        &mut self,
        view: Entity<HtmlView>,
        event: &HtmlViewEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            HtmlViewEvent::LinkHovered(link) => {
                self.hovered_link = link.clone().unwrap_or_default();
                cx.notify();
            }
            HtmlViewEvent::LinkActivated(url) => {
                if is_openable_link(url) {
                    cx.open_url(url);
                }
            }
            HtmlViewEvent::ZoomChanged(zoom) => {
                self.reader_zoom = *zoom;
                cx.notify();
            }
            HtmlViewEvent::ContextMenu { position, hit } => {
                self.reader_menu = Some(ReaderMenu {
                    position: *position,
                    hit: hit.clone(),
                    has_selection: view.read(cx).has_selection(),
                });
                cx.notify();
            }
        }
    }

    /// Opens (or focuses) the reader's find bar.
    pub(crate) fn open_find_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_detail.is_none() {
            return;
        }
        if self.find_bar.is_none() {
            let placeholder = Key::FindPlaceholder.tr(cx.language());
            let input = cx.new(|cx| TextInput::new(placeholder, cx));
            // Defer: the input notifies while it is being updated.
            let root = cx.weak_entity();
            let observation = cx.observe(&input, move |_, _, cx| {
                let root = root.clone();
                cx.defer(move |cx| {
                    let _ = root.update(cx, |this, cx| this.run_find(cx));
                });
            });
            self.find_bar = Some(FindBar {
                input,
                count: 0,
                current: 0,
                _observation: observation,
            });
        }
        if let Some(bar) = &self.find_bar {
            let focus = bar.input.read(cx).focus_handle(cx);
            window.focus(&focus);
        }
        cx.notify();
    }

    /// Closes the find bar, clearing its highlight, and focuses the reader.
    fn close_find_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find_bar.take().is_none() {
            return;
        }
        if let Some(view) = &self.reader_view {
            view.update(cx, |view, cx| {
                view.find("", cx);
            });
            let focus = view.read(cx).focus_handle(cx);
            window.focus(&focus);
        }
        cx.notify();
    }

    /// Searches the reader for the find bar's query and selects the first match.
    fn run_find(&mut self, cx: &mut Context<Self>) {
        let (Some(bar), Some(view)) = (&mut self.find_bar, &self.reader_view) else {
            return;
        };
        let query = bar.input.read(cx).content().to_string();
        bar.count = view.update(cx, |view, cx| view.find(&query, cx));
        bar.current = 0;
        if bar.count > 0 {
            view.update(cx, |view, cx| view.select_match(0, cx));
        }
        cx.notify();
    }

    /// Moves to the next (`step` = 1) or previous (-1) find match.
    fn step_find(&mut self, step: isize, cx: &mut Context<Self>) {
        let (Some(bar), Some(view)) = (&mut self.find_bar, &self.reader_view) else {
            return;
        };
        if bar.count == 0 {
            return;
        }
        bar.current = step_match(bar.current, bar.count, step);
        let current = bar.current;
        view.update(cx, |view, cx| view.select_match(current, cx));
        cx.notify();
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
    pub(crate) fn open_settings(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
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
                title: Some("BGMail Settings".into()),
                appears_transparent: false,
                traffic_light_position: None,
            }),
            ..Default::default()
        };

        if let Ok(handle) = cx.open_window(options, |window, cx| {
            let root_for_close = root.clone();
            let view = cx.new(|_| SettingsView::new(accounts, root));
            window.on_window_should_close(cx, move |_, cx| {
                if let Some(root) = root_for_close.upgrade() {
                    root.update(cx, |root, cx| {
                        root.on_settings_window_closed(cx);
                    });
                }
                true
            });
            view
        }) {
            let _ = handle.update(cx, |_, window, _| window.activate_window());
            self.settings_window = Some(handle);
            cx.notify();
        }
    }

    /// Opens the compose window, or focuses it if it is already open.
    pub(crate) fn open_compose(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
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
                    root.update(cx, |root, cx| {
                        root.on_compose_window_closed(cx);
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
    pub(crate) fn toggle_sidebar(&mut self) {
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
            - crate::window_frame::left_controls_reserved_width()
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
                        None,
                        IconButton::new("filter", IconName::Filter),
                    ))
                    .child(Self::icon_button_with_tooltip(
                        "more",
                        Key::ToolbarMore.tr(language),
                        Some((
                            shortcuts::COMMAND_PALETTE_MAC,
                            shortcuts::COMMAND_PALETTE_OTHER,
                        )),
                        IconButton::new("more", IconName::More).on_click(cx.listener(
                            |this, _, window, cx| {
                                this.toggle_command_palette(window, cx);
                            },
                        )),
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
                    .children(crate::window_frame::render_left_window_controls(window))
                    .child(Self::icon_button_with_tooltip(
                        "toggle-sidebar",
                        Key::ToolbarToggleSidebar.tr(language),
                        Some((
                            shortcuts::TOGGLE_SIDEBAR_MAC,
                            shortcuts::TOGGLE_SIDEBAR_OTHER,
                        )),
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
                        Some((shortcuts::SETTINGS_MAC, shortcuts::SETTINGS_OTHER)),
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
                        Some((shortcuts::COMPOSE_MAC, shortcuts::COMPOSE_OTHER)),
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
                                                None,
                                                IconButton::new("reply", IconName::Reply),
                                            ))
                                            .child(Self::icon_button_with_tooltip(
                                                "reply-all",
                                                Key::ToolbarReplyAll.tr(language),
                                                None,
                                                IconButton::new("reply-all", IconName::ReplyAll),
                                            ))
                                            .child(Self::icon_button_with_tooltip(
                                                "forward",
                                                Key::ToolbarForward.tr(language),
                                                None,
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
                                                Key::CommandDelete.tr(language),
                                                Some((
                                                    shortcuts::DELETE_MESSAGE_MAC,
                                                    shortcuts::DELETE_MESSAGE_OTHER,
                                                )),
                                                IconButton::new("trash", IconName::Trash).on_click(
                                                    cx.listener(|this, _, _, cx| {
                                                        this.delete_message_to_trash(cx);
                                                    }),
                                                ),
                                            ))
                                            .child(Self::icon_button_with_tooltip(
                                                "archive",
                                                Key::CommandArchive.tr(language),
                                                Some((
                                                    shortcuts::ARCHIVE_MAC,
                                                    shortcuts::ARCHIVE_OTHER,
                                                )),
                                                IconButton::new("archive", IconName::Archive)
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.archive_message(cx);
                                                    })),
                                            ))
                                            .child(Self::icon_button_with_tooltip(
                                                "junk",
                                                Key::CommandMarkJunk.tr(language),
                                                Some((
                                                    shortcuts::MARK_JUNK_MAC,
                                                    shortcuts::MARK_JUNK_OTHER,
                                                )),
                                                IconButton::new("junk", IconName::Junk).on_click(
                                                    cx.listener(|this, _, _, cx| {
                                                        this.mark_message_junk(cx);
                                                    }),
                                                ),
                                            )),
                                    )
                                })
                                .when(groups >= 3, |cluster| {
                                    cluster.child(Self::render_toolbar_separator(border)).child(
                                        h_flex()
                                            .gap_1()
                                            .child(Self::icon_button_with_tooltip(
                                                "flag",
                                                Key::ToolbarFlag.tr(language),
                                                Some((
                                                    shortcuts::TOGGLE_FLAG_MAC,
                                                    shortcuts::TOGGLE_FLAG_OTHER,
                                                )),
                                                IconButton::new("flag", IconName::Flag).on_click(
                                                    cx.listener(|this, _, _, cx| {
                                                        this.toggle_message_flag(cx);
                                                    }),
                                                ),
                                            ))
                                            .child(Self::icon_button_with_tooltip(
                                                "move",
                                                Key::ToolbarMove.tr(language),
                                                None,
                                                IconButton::new("move", IconName::Folder),
                                            )),
                                    )
                                }),
                        )
                    })
                    .child(div().flex_1())
                    .child(self.render_search_control(compact_search, search_bg, language, cx)),
            )
            .children(crate::window_frame::render_right_window_controls(window))
    }

    /// Search control: compact magnifying-glass icon, or expanded field with clear.
    fn render_search_control(
        &self,
        compact_search: bool,
        search_bg: Hsla,
        language: Language,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if compact_search {
            return div()
                .id("search")
                .child(Self::icon_button_with_tooltip(
                    "search-open",
                    Key::SearchPlaceholder.tr(language),
                    None,
                    IconButton::new("search", IconName::Search).on_click(cx.listener(
                        |this, _, window, cx| {
                            this.focus_search_input(window, cx);
                        },
                    )),
                ))
                .into_any_element();
        }

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
            .when(self.show_search_clear_in_field(cx), |el| {
                el.child(Self::render_search_clear_button(language, cx))
            })
            .into_any_element()
    }

    /// Micro clear control at the end of the expanded search field.
    fn render_search_clear_button(language: Language, cx: &mut Context<Self>) -> impl IntoElement {
        let hover_bg = cx.theme().colors().element_hover;
        div()
            .id("search-clear")
            .tooltip(Tooltip::text(Key::SearchClear.tr(language)))
            .flex()
            .flex_shrink_0()
            .justify_center()
            .items_center()
            .size(px(20.0))
            .rounded_md()
            .hover(move |el| el.bg(hover_bg))
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, _, cx| this.clear_search(cx)))
            .child(
                Icon::new(IconName::Clear)
                    .size(IconSize::XXSmall)
                    .color(Color::Muted),
            )
    }

    /// Wraps an [`IconButton`] with a localized tooltip (GPUI attaches tooltips to
    /// elements via `div`, not to `IconButton` directly).
    fn icon_button_with_tooltip(
        id: &'static str,
        label: &'static str,
        shortcut: Option<(&'static str, &'static str)>,
        button: IconButton,
    ) -> impl IntoElement {
        if let Some((mac, other)) = shortcut {
            let formatted = shortcuts::format_binding(shortcuts::primary_binding(mac, other));
            div()
                .id(id)
                .tooltip(Tooltip::with_shortcut(label, formatted))
                .child(button)
        } else {
            div().id(id).tooltip(Tooltip::text(label)).child(button)
        }
    }

    /// A thin vertical divider used to separate toolbar button groups.
    fn render_toolbar_separator(color: Hsla) -> impl IntoElement {
        div().w(px(1.0)).h(px(20.0)).bg(color)
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
            .text_size(ui::ui_text_size(-4.0))
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
                this.last_reader_sig = None;
                this.on_message_selection_changed(cx);
            }))
    }

    // ----- Reading pane ---------------------------------------------------

    /// Privacy affordance shown on the subject line: a red shield whose tooltip
    /// counts the remote images blocked in the message; clicking it loads
    /// them (for this message, this session). A same-sized empty slot is
    /// reserved otherwise, so the header keeps its height.
    fn render_privacy_shield(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let slot = div().flex_shrink_0().size(px(20.0));
        if self.content_blocked_count == 0 {
            return slot.into_any_element();
        }
        let language = cx.language();
        slot.id("privacy-shield")
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .tooltip(Tooltip::text(
                Key::BlockedElements
                    .tr(language)
                    .replace("{}", &self.content_blocked_count.to_string()),
            ))
            .on_click(cx.listener(|this, _, _, cx| this.load_remote_for_selected(cx)))
            .child(
                Icon::new(IconName::Shield)
                    .size(IconSize::Small)
                    .color(Color::Error),
            )
            .into_any_element()
    }

    /// The reader's find bar: query field, "n of m" count, previous/next and
    /// close. Enter goes to the next match (Shift+Enter: previous), Escape
    /// closes it.
    fn render_find_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(bar) = &self.find_bar else {
            return div().into_any_element();
        };
        let colors = cx.theme().colors();
        let language = cx.language();
        let status = if bar.input.read(cx).content().is_empty() {
            String::new()
        } else if bar.count == 0 {
            Key::FindNoMatches.tr(language).to_string()
        } else {
            Key::FindMatches
                .tr(language)
                .replacen("{}", &(bar.current + 1).to_string(), 1)
                .replacen("{}", &bar.count.to_string(), 1)
        };
        h_flex()
            .flex_shrink_0()
            .w_full()
            .px_6()
            .py_1p5()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(colors.border)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "enter" => {
                        let step = if event.keystroke.modifiers.shift {
                            -1
                        } else {
                            1
                        };
                        this.step_find(step, cx);
                        cx.stop_propagation();
                    }
                    "escape" => {
                        this.close_find_bar(window, cx);
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            }))
            .child(
                h_flex()
                    .flex_1()
                    .max_w(px(320.0))
                    .h(px(28.0))
                    .px_2()
                    .gap_1p5()
                    .items_center()
                    .rounded_md()
                    .bg(colors.element_background)
                    .child(
                        Icon::new(IconName::Search)
                            .size(IconSize::Small)
                            .color(Color::Muted),
                    )
                    .child(div().flex_1().min_w_0().child(bar.input.clone())),
            )
            .child(
                Label::new(status)
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(div().flex_1())
            .child(
                IconButton::new("find-previous", IconName::ChevronUp)
                    .on_click(cx.listener(|this, _, _, cx| this.step_find(-1, cx))),
            )
            .child(
                IconButton::new("find-next", IconName::ChevronDown)
                    .on_click(cx.listener(|this, _, _, cx| this.step_find(1, cx))),
            )
            .child(
                IconButton::new("find-close", IconName::Clear)
                    .on_click(cx.listener(|this, _, window, cx| this.close_find_bar(window, cx))),
            )
            .into_any_element()
    }

    /// The reader's context menu (secondary click): copy/select all for text,
    /// open/copy for links. Anchored at the click; any outside click closes it.
    fn render_reader_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some((menu, view)) = self.reader_menu.as_ref().zip(self.reader_view.clone()) else {
            return div().into_any_element();
        };
        let language = cx.language();
        let root = cx.weak_entity();
        let close = move |cx: &mut App| {
            let _ = root.update(cx, |this, cx| {
                this.reader_menu = None;
                cx.notify();
            });
        };
        let hint = |source_mac: &str, source_other: &str| {
            crate::shortcuts::format_binding(crate::shortcuts::primary_binding(
                source_mac,
                source_other,
            ))
        };
        let mut items = ui::ContextMenu::new();
        if let Some(link) = menu.hit.link.clone().filter(|link| is_openable_link(link)) {
            let (open_link, copy_link) = (link.clone(), link);
            let (close_open, close_copy) = (close.clone(), close.clone());
            items = items
                .entry(Key::CtxOpenLink.tr(language), move |_, _, cx| {
                    cx.open_url(&open_link);
                    close_open(cx);
                })
                .entry(Key::CtxCopyLink.tr(language), move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copy_link.clone()));
                    close_copy(cx);
                })
                .separator();
        }
        if menu.has_selection {
            let (view, close) = (view.clone(), close.clone());
            items = items.entry_with_key_hint(
                Key::CtxCopy.tr(language),
                hint("cmd-c", "ctrl-c"),
                move |_, _, cx| {
                    view.update(cx, |view, cx| view.copy_selection(cx));
                    close(cx);
                },
            );
        }
        let select_close = close.clone();
        items = items.entry_with_key_hint(
            Key::CtxSelectAll.tr(language),
            hint("cmd-a", "ctrl-a"),
            move |_, _, cx| {
                view.update(cx, |view, cx| view.select_all(cx));
                select_close(cx);
            },
        );
        div()
            .absolute()
            .inset_0()
            .occlude()
            .on_any_mouse_down(cx.listener(|this, _, _, cx| {
                this.reader_menu = None;
                cx.notify();
            }))
            .child(deferred(
                anchored()
                    .position(menu.position)
                    .anchor(Corner::TopLeft)
                    .snap_to_window_with_margin(px(8.0))
                    .child(items),
            ))
            .into_any_element()
    }

    fn render_reader(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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
                        .text_size(ui::ui_text_size(10.0))
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
                            .text_size(ui::ui_text_size(1.0))
                            // Tight line height so the glyph (not the looser line
                            // box) is what gets centered vertically.
                            .line_height(ui::ui_text_size(1.0))
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

        // The body is rendered natively by `html_view` (scrolls on both axes;
        // laid out at the pane width).
        let body_area = div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            // Match the forced-white page so no themed background shows at
            // sub-pixel edges.
            .when(self.reader_white_background, |el| el.bg(gpui::white()))
            .when_some(self.reader_view.clone(), |el, view| el.child(view));

        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(bg)
            .child(header)
            .when(self.reader_notice.is_some(), |el| {
                el.child(self.render_reader_notice(cx))
            })
            .when(self.find_bar.is_some(), |el| {
                el.child(self.render_find_bar(cx))
            })
            .child(body_area)
            .into_any_element()
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
                h_flex()
                    .gap_3()
                    .when((self.reader_zoom - 1.0).abs() > 0.001, |this| {
                        this.child(
                            Label::new(locale::status_zoom(language, self.reader_zoom))
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                        )
                    })
                    .child(
                        Label::new(locale::status_unread(language, total_unread))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    ),
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

    fn close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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

        if window.is_window_active()
            && app_menus::active_menu_surface(cx) != app_menus::MenuSurface::Main
        {
            self.sync_app_menus(cx);
        }

        // Make sure the scrollbar state entities exist before the panels render.
        self.ensure_scrollbar_states(cx);
        self.ensure_search_input(cx.language(), window, cx);
        if self.command_palette_focus_pending {
            self.command_palette_focus_pending = false;
            if let Some(input) = self
                .command_palette
                .as_ref()
                .and_then(|palette| palette.input.clone())
            {
                let focus_handle = input.read(cx).focus_handle(cx);
                window.defer(cx, move |window, _| {
                    window.focus(&focus_handle);
                });
            }
        }
        if !self.search_is_compact() {
            self.search_force_expanded = false;
        }
        // Keep the reader document in sync with the selection and theme.
        self.sync_reader(window, cx);

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

        let content = v_flex()
            .size_full()
            .bg(background)
            .text_color(text)
            .font_family("Helvetica")
            .on_action(cx.listener(|this, _: &FindInMessage, window, cx| {
                this.open_find_bar(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleCommandPalette, window, cx| {
                this.toggle_command_palette(window, cx);
            }))
            .capture_any_mouse_down(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                this.blur_search_if_focused(window, cx);
            }))
            .child(self.render_toolbar(window, cx))
            .child(body)
            .child(self.render_status_bar(cx))
            .when(self.reader_menu.is_some(), |el| {
                el.child(self.render_reader_menu(cx))
            })
            .when(
                self.command_palette
                    .as_ref()
                    .is_some_and(|palette| palette.open),
                |el| {
                    if let Some((entries, selected_ix, input)) =
                        self.command_palette_render_args(cx)
                    {
                        el.child(command_palette_overlay::render_command_palette(
                            cx,
                            entries,
                            selected_ix,
                            input,
                        ))
                    } else {
                        el
                    }
                },
            )
            .into_any_element();

        // Linux CSD: wrap with shadow/resize chrome when the compositor granted
        // client-side decorations (requested in `main` via WindowOptions).
        crate::window_frame::wrap_client_decorations(content, window, cx.theme().colors().border)
    }
}

/// The command palette may only be opened while the main window is key.
pub(crate) fn can_open_command_palette(main_window_active: bool) -> bool {
    main_window_active
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{self, CommandId};
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
    fn plain_text_reasons_follow_crash_size_and_override() {
        let big = MAX_FORMATTED_HTML_BYTES + 1;
        assert_eq!(plain_text_reason(10, false, false), None);
        assert_eq!(
            plain_text_reason(big, false, false),
            Some(ReaderNotice::TooLarge)
        );
        assert_eq!(plain_text_reason(big, false, true), None);
        // A crash wins over the override.
        assert_eq!(
            plain_text_reason(10, true, true),
            Some(ReaderNotice::Crashed)
        );
    }

    #[test]
    fn find_steps_wrap_around() {
        assert_eq!(step_match(0, 3, 1), 1);
        assert_eq!(step_match(2, 3, 1), 0);
        assert_eq!(step_match(0, 3, -1), 2);
        assert_eq!(step_match(0, 0, 1), 0);
    }

    #[test]
    fn reader_selection_is_a_translucent_accent() {
        let [r, g, b, a] = reader_selection_color(gpui::hsla(0.0, 1.0, 0.5, 1.0));
        assert_eq!((r, g, b), (255, 0, 0));
        assert_eq!(a, 89);
    }

    #[test]
    fn command_palette_only_opens_when_main_window_is_focused() {
        assert!(can_open_command_palette(true));
        assert!(!can_open_command_palette(false));
    }

    #[test]
    fn command_palette_starts_closed() {
        let (_dir, view) = test_view();
        assert!(view
            .command_palette
            .as_ref()
            .is_none_or(|palette| !palette.open));
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

    #[test]
    fn search_clear_shows_in_expanded_field_only_with_text_on_wide_toolbar() {
        let (_dir, mut view) = test_view();
        view.sync_layout(px(1600.0));
        assert!(!view.show_search_clear_in_field_for_query(""));
        assert!(view.show_search_clear_in_field_for_query("hello"));
    }

    #[test]
    fn search_clear_always_shows_in_force_expanded_compact_field() {
        let (_dir, mut view) = test_view();
        view.sync_layout(px(NARROW_BREAKPOINT));
        view.search_force_expanded = true;
        assert!(view.show_search_clear_in_field_for_query(""));
    }

    #[test]
    fn search_collapses_after_blur_only_when_compact_expanded_and_empty() {
        let (_dir, mut view) = test_view();
        view.sync_layout(px(NARROW_BREAKPOINT));
        view.search_force_expanded = true;
        assert!(view.should_collapse_search_after_blur(""));
        assert!(!view.should_collapse_search_after_blur("hello"));

        view.sync_layout(px(1600.0));
        view.search_force_expanded = true;
        assert!(!view.should_collapse_search_after_blur(""));
    }

    #[test]
    fn move_to_trash_updates_folder_membership() {
        let (_dir, mut view) = test_view();
        let id = view.selected_message_id.expect("seed selects a message");
        view.db.move_message_to_trash(id).unwrap();
        view.reload_message_list();
        let csv = view
            .db
            .get_message(id)
            .unwrap()
            .expect("message")
            .folders_csv;
        assert!(csv.contains(storage::system::TRASH));
    }

    #[test]
    fn command_context_reflects_trash_state() {
        let (_dir, mut view) = test_view();
        let id = view.selected_message_id.unwrap();
        view.db.move_message_to_trash(id).unwrap();
        view.reload_selected_detail();
        let ctx = view.command_context();
        assert!(commands::command_enabled(&CommandId::MessageRestore, &ctx));
        assert!(!commands::command_enabled(&CommandId::MessageDelete, &ctx));
    }
}
