//! Root view of the BGMail visual prototype.
//!
//! Reproduces the macOS mail client layout (three columns, a unified top toolbar
//! and a status bar), using the components from the `ui` crate. All interaction
//! here is "mock" — only item selection, theme toggling and language switching.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use std::f32::consts::FRAC_PI_2;

use gpui::{
    anchored, canvas, deferred, ease_in_out, point, radians, size, svg, uniform_list, Animation,
    AnimationExt as _, App, AppContext as _, Bounds, ClipboardItem, Context, Corner, CursorStyle,
    DragMoveEvent, Empty, Entity, FocusHandle, Focusable, FontWeight, Hsla, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, PathBuilder, Point, ScrollHandle, ScrollStrategy,
    Size, Subscription, Svg, TitlebarOptions, Transformation, UniformListScrollHandle, Window,
    WindowBounds, WindowControlArea, WindowHandle, WindowOptions,
};

use crate::command_palette_overlay;
use theme::ActiveTheme;
use ui::prelude::*;
use ui::{
    set_native_text_focus, Button, Color, FocusNext, FocusPrevious, Icon, IconButton, IconName,
    IconSize, Label, LabelSize, ListItem, Scrollbar, ScrollbarState, TextInput, Tooltip,
};

use crate::actions::{
    ExtendSelectionDown, ExtendSelectionUp, FindInMessage, SelectAllMessages, SelectNextMessage,
    SelectPreviousMessage, ToggleCommandPalette,
};
use crate::app_menus;
use crate::attachments;
use crate::command_palette::CommandPaletteState;
use crate::commands::{self, BulkSelection, CommandContext, CommandId};
use crate::compose::{self, ComposeAccount, ComposeInit, ComposeView, QuotedOriginal};
use crate::config::{self, Config};
use crate::crash_guard::CrashGuard;
use crate::data::{self, GlobalMailbox, MailboxKind, MessageBody};
use crate::dates;
use crate::db_seed;
use crate::locale::{self, ActiveLanguage, Key, Language};
use crate::mail_html::{
    email_document, is_openable_link, plain_text, print_document, quotable_html, quotable_text,
    DocumentColors, PrintHeader,
};
use crate::mail_service::{self, MailService, OverallStatus, StartError};
use crate::settings::SettingsView;
use crate::shortcuts;
use html_view::{
    Appearance as PageAppearance, DetachedDocument, DocumentOptions, HitInfo, HtmlDocument,
    HtmlView, HtmlViewEvent, RenderError,
};
use mail_core::{AccountConfig, Address, Draft, ParsedMessage, QuoteLabels, ReplyKind};
use mail_sync::{actions as mail_actions, ActionOutcome, Command, SyncEvent};
use storage::{
    AttachmentRow, MailListQuery, MessageDetail, MessageListItem, OutgoingRow, OutgoingState,
};

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

/// Wayland app_id / X11 WM_CLASS of the main window: must match the packaged
/// `BGMail.desktop` (StartupWMClass) so docks show the app's icon for it.
pub(crate) const MAIN_WINDOW_APP_ID: &str = "com.bgmail.main";
/// Wayland app_id / X11 WM_CLASS of the settings window.
pub(crate) const SETTINGS_WINDOW_APP_ID: &str = "com.bgmail.settings";
/// Wayland app_id / X11 WM_CLASS of the compose window.
pub(crate) const COMPOSE_WINDOW_APP_ID: &str = "com.bgmail.newmail";
/// Below this window width the sidebar collapses automatically and, once
/// reopened, floats over the content instead of pushing the columns.
const NARROW_BREAKPOINT: f32 = 1200.0;
/// Hit area (width) of the draggable divider between two columns, in pixels.
const RESIZE_HANDLE_WIDTH: f32 = 6.0;
/// Duration of the sidebar fold (collapse/expand) animation, in milliseconds.
/// It is fixed (not proportional to the number of rows), so a long list folds in
/// the same time as a short one.
const FOLD_ANIM_MS: u64 = 90;
/// Tilt of the sheets behind a multiple selection's message, in degrees.
const SHEET_TILTS: [f32; 4] = [1.8, -1.3, 2.6, -2.0];
/// How much smaller a multiple selection's message shows.
const STACK_SCALE: f32 = 0.9;
/// Space around it on each side of the pane, as a fraction of the pane.
const STACK_MARGIN: f32 = (1.0 - STACK_SCALE) / 2.0;
/// Tallest a dropdown gets before it scrolls, in pixels.
const MENU_MAX_HEIGHT: f32 = 420.0;
/// Width of a list row's compact action button (`IconSize::Small`) and the gap
/// around it, in pixels.
const ROW_ACTION_WIDTH: f32 = 14.0;
const ROW_ACTION_GAP: f32 = 6.0;
/// Group of a sidebar folder's row, which takes dropped messages.
const FOLDER_ROW_GROUP: &str = "folder-row";
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
/// horizontal padding, inter-child gaps and the compose + get-mail buttons.
const TOOLBAR_FIXED_OVERHEAD: f32 = 140.0;
/// Cumulative widths (incl. separators/gaps) of the centered action groups, used
/// to decide how many fit alongside the always-visible search button. Groups are
/// dropped from the right (flag/move first) so search is never overlapped.
const ACTIONS_1_WIDTH: f32 = 92.0; // reply / reply-all / forward
const ACTIONS_2_WIDTH: f32 = 193.0; // + trash / archive / junk
const ACTIONS_3_WIDTH: f32 = 322.0; // + flag / read / move

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

/// Hover group of a message row; reveals its action buttons.
const MESSAGE_ROW_GROUP: &str = "message-row";

/// The rows selected in a mailbox when the user left it.
#[derive(Debug, Clone)]
struct RememberedSelection {
    ids: Vec<i64>,
    anchor: Option<i64>,
    focus: Option<i64>,
}

/// What is currently selected in the sidebar: a global (unified) mailbox at the
/// top, or a specific folder within an account group.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Selection {
    /// A unified mailbox spanning all accounts.
    Global(GlobalMailbox),
    /// A folder within an account (`account_id` + storage path, e.g. `sys:inbox`).
    Mailbox {
        account_id: i64,
        folder_path: String,
    },
}

/// Rows shown by the message list's filter button.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ListFilter {
    #[default]
    All,
    Unread,
    Starred,
    Attachments,
}

impl ListFilter {
    const OPTIONS: [ListFilter; 4] = [
        ListFilter::All,
        ListFilter::Unread,
        ListFilter::Starred,
        ListFilter::Attachments,
    ];

    fn matches(self, message: &MessageListItem) -> bool {
        match self {
            ListFilter::All => true,
            ListFilter::Unread => message.unread,
            ListFilter::Starred => message.starred,
            ListFilter::Attachments => message.has_attachment,
        }
    }

    fn label(self) -> Key {
        match self {
            ListFilter::All => Key::FilterAll,
            ListFilter::Unread => Key::FilterUnread,
            ListFilter::Starred => Key::FilterStarred,
            ListFilter::Attachments => Key::FilterAttachments,
        }
    }
}

/// Rows selected after clicking `clicked`: Shift takes the range from the
/// `anchor`, Ctrl/Cmd (`toggle`) adds or removes that row, a plain click
/// selects only it. The result follows the list's order.
fn clicked_selection(
    list: &[i64],
    current: &[i64],
    anchor: Option<i64>,
    clicked: i64,
    extend: bool,
    toggle: bool,
) -> Vec<i64> {
    if extend {
        return selection_range(list, anchor.unwrap_or(clicked), clicked);
    }
    if toggle {
        return list
            .iter()
            .copied()
            .filter(|id| current.contains(id) != (*id == clicked))
            .collect();
    }
    vec![clicked]
}

/// Corners of `bounds` rotated by `degrees` around its center, clockwise from
/// the top left.
fn sheet_corners(bounds: Bounds<Pixels>, degrees: f32) -> [Point<Pixels>; 4] {
    let center = bounds.center();
    let (sin, cos) = degrees.to_radians().sin_cos();
    let rotate = |corner: Point<Pixels>| {
        let x = f32::from(corner.x - center.x);
        let y = f32::from(corner.y - center.y);
        point(
            center.x + px(x * cos - y * sin),
            center.y + px(x * sin + y * cos),
        )
    };
    [
        rotate(bounds.origin),
        rotate(bounds.top_right()),
        rotate(bounds.bottom_right()),
        rotate(bounds.bottom_left()),
    ]
}

/// Rows between `from` and `to`, inclusive, in the list's order.
fn selection_range(list: &[i64], from: i64, to: i64) -> Vec<i64> {
    let position = |id| list.iter().position(|row| *row == id);
    let (Some(from), Some(to)) = (position(from), position(to)) else {
        return vec![to];
    };
    list[from.min(to)..=from.max(to)].to_vec()
}

/// Width the subject of a list row gives up for its action buttons, while hovered.
fn row_actions_room(hovered: bool) -> Pixels {
    if hovered {
        px(2.0 * (ROW_ACTION_WIDTH + ROW_ACTION_GAP))
    } else {
        px(0.0)
    }
}

/// Star button of the toolbar: filled and in the list's star color when set.
fn star_icon(starred: bool) -> IconName {
    if starred {
        IconName::StarFilled
    } else {
        IconName::Star
    }
}

fn star_color(starred: bool) -> Color {
    if starred {
        Color::Warning
    } else {
        Color::Default
    }
}

/// How someone is named in a message's recipients line: `me` for the account's own
/// address, the first name, or the address when there is no name.
fn short_recipient(address: &Address, own_email: &str, me: &'static str) -> String {
    if mail_core::same_email(&address.email, own_email) {
        return me.to_string();
    }
    address
        .name
        .as_deref()
        .and_then(|name| name.split_whitespace().next())
        .unwrap_or(&address.email)
        .to_string()
}

/// Recipients in order, each address once (someone can be in both To and Cc).
fn unique_recipients<'a>(list: impl Iterator<Item = &'a Address>) -> Vec<&'a Address> {
    let mut seen: Vec<&Address> = Vec::new();
    for address in list {
        if !seen
            .iter()
            .any(|other| mail_core::same_email(&other.email, &address.email))
        {
            seen.push(address);
        }
    }
    seen
}

/// How far a message must be dragged before it counts, in pixels.
const MESSAGE_DRAG_THRESHOLD: f32 = 100.0;
/// Opacity of the row following the pointer while dragged.
const MESSAGE_DRAG_OPACITY: f32 = 0.6;

/// Whether a drag from `origin` has gone far enough to count.
fn drag_counts(origin: Point<Pixels>, position: Point<Pixels>) -> bool {
    let dx = f32::from(position.x - origin.x);
    let dy = f32::from(position.y - origin.y);
    dx * dx + dy * dy > MESSAGE_DRAG_THRESHOLD * MESSAGE_DRAG_THRESHOLD
}

/// How far a drag went. Once past the threshold it counts for good: the way back to
/// a folder close to where it started mustn't undo it.
#[derive(Default)]
struct DragProgress {
    origin: Cell<Option<Point<Pixels>>>,
    counted: Cell<bool>,
}

impl DragProgress {
    fn start(&self, origin: Point<Pixels>) {
        self.origin.set(Some(origin));
        self.counted.set(false);
    }

    /// Whether the drag counts with the pointer at `position`.
    fn counts_at(&self, position: Point<Pixels>) -> bool {
        if !self.counted.get() {
            let far = self
                .origin
                .get()
                .is_some_and(|origin| drag_counts(origin, position));
            self.counted.set(far);
        }
        self.counted.get()
    }
}

/// The messages being dragged from the list. GPUI starts a drag after a couple of
/// pixels, so its progress is kept to tell when it counts.
#[derive(Clone)]
struct DraggedMessages {
    ids: Vec<i64>,
    /// The account they belong to: `None` for a selection across accounts, which
    /// has no folder in common.
    account_id: Option<i64>,
    progress: Rc<DragProgress>,
}

impl DraggedMessages {
    /// Sets the cursor once the drag counts. GPUI takes the drag out of the app while
    /// drawing its view, so this can't happen there.
    fn update_cursor(&self, position: Point<Pixels>, window: &mut Window, cx: &mut App) {
        let cursor = drag_cursor(DRAG_DRAWS_HAND);
        if self.progress.counts_at(position) && cx.active_drag_cursor_style() != Some(cursor) {
            cx.set_active_drag_cursor_style(cursor, window);
        }
    }

    /// Whether they can be dropped on `folder_path` of `account_id` right now.
    fn can_drop_on(&self, account_id: i64, folder_path: &str, position: Point<Pixels>) -> bool {
        self.progress.counts_at(position)
            && self.account_id == Some(account_id)
            && !storage::is_manual_move_destination_forbidden(folder_path)
    }
}

/// Whether a drag draws its own closed hand: Windows has no such cursor (GPUI gives
/// it the arrow), so the system's is hidden and the hand painted in its place, as
/// browsers do.
const DRAG_DRAWS_HAND: bool = cfg!(target_os = "windows");
/// Size of the painted closed hand and of its outline, in pixels.
const DRAG_HAND_SIZE: f32 = 18.0;
const DRAG_HAND_OUTLINE_SIZE: f32 = 22.0;

/// The system cursor during a drag that counts.
fn drag_cursor(draws_hand: bool) -> CursorStyle {
    if draws_hand {
        CursorStyle::None
    } else {
        CursorStyle::ClosedHand
    }
}

/// Height of the card following the pointer, in pixels.
const DRAG_CARD_HEIGHT: f32 = 56.0;

/// How far inside the card's top left corner the pointer sits, in pixels.
const DRAG_CARD_POINTER_INSET: f32 = 12.0;

/// Where the card goes for its top left corner to be just past the pointer, relative
/// to where GPUI places a drag: the pointer minus where the row was grabbed.
fn drag_card_origin(grab: Point<Pixels>) -> Point<Pixels> {
    point(
        grab.x - px(DRAG_CARD_POINTER_INSET),
        grab.y - px(DRAG_CARD_POINTER_INSET),
    )
}

/// The row that follows the pointer: nothing until the drag counts.
struct DragPreview {
    progress: Rc<DragProgress>,
    /// Where the pointer holds the row, from its top left corner.
    grab: Point<Pixels>,
    width: Pixels,
    title: SharedString,
    subtitle: SharedString,
    draws_hand: bool,
}

impl Render for DragPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.progress.counts_at(window.mouse_position()) {
            return div().into_any_element();
        }
        let colors = cx.theme().colors();
        let card = drag_card_origin(self.grab);
        let hand = |size: f32, color: Hsla| {
            svg()
                .absolute()
                .left(self.grab.x - px(size / 2.0))
                .top(self.grab.y - px(size / 2.0))
                .size(px(size))
                .path(IconName::DragHand.path())
                .text_color(color)
        };
        div()
            .relative()
            .child(
                v_flex()
                    .debug_selector(|| "drag-card".to_string())
                    .absolute()
                    .left(card.x)
                    .top(card.y)
                    .w(self.width)
                    .h(px(DRAG_CARD_HEIGHT))
                    .px_3()
                    .justify_center()
                    .gap_0p5()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.elevated_surface_background)
                    .opacity(MESSAGE_DRAG_OPACITY)
                    .child(
                        Label::new(self.title.clone())
                            .weight(FontWeight::SEMIBOLD)
                            .single_line(),
                    )
                    .child(
                        Label::new(self.subtitle.clone())
                            .size(LabelSize::Small)
                            .single_line(),
                    ),
            )
            // As a system cursor: light with a dark outline, visible on anything.
            .when(self.draws_hand, |el| {
                el.child(hand(DRAG_HAND_OUTLINE_SIZE, gpui::black()))
                    .child(hand(DRAG_HAND_SIZE, gpui::white()))
            })
            .into_any_element()
    }
}

/// Whether a folder's row counts unread mail: what is in the Trash is done with.
fn folder_shows_unread(path: &str) -> bool {
    path != storage::system::TRASH
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
        GlobalMailbox::Flagged => IconName::Star,
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
    /// Remote content loads (always, or allowed for this message).
    remote: bool,
}

/// HTML bodies from this size on are built on a background thread, showing a
/// placeholder meanwhile. Smaller ones take a few ms: building them right away
/// avoids flashing the placeholder at every message.
const BACKGROUND_BUILD_BYTES: usize = 256 * 1024;

/// Everything a reader document is made from, so it can be built on any thread.
struct ReaderJob {
    colors: DocumentColors,
    body: MessageBody,
    remote: bool,
    crashed: bool,
    selection_color: [u8; 4],
    /// Expected layout width: the first layout at the real one is then cheap.
    width: f32,
}

/// A reader document ready to be shown.
struct BuiltReader {
    document: DetachedDocument,
    notice: Option<ReaderNotice>,
    blocked_images: usize,
}

impl ReaderJob {
    fn runs_in_background(&self) -> bool {
        matches!(&self.body, MessageBody::Html(html) if html.len() >= BACKGROUND_BUILD_BYTES)
    }

    /// Sanitizes, themes, parses and lays out the body. A message that crashed
    /// the renderer, or that it refuses (too deep), becomes plain text.
    fn build(self) -> BuiltReader {
        let options = || DocumentOptions {
            appearance: if self.colors.is_dark() {
                PageAppearance::Dark
            } else {
                PageAppearance::Light
            },
            selection_color: Some(self.selection_color),
            dark_page: self.colors.dark_page(),
            resources: html_view::Resources::new().allow_remote(self.remote),
            ..Default::default()
        };
        let as_plain = || match &self.body {
            MessageBody::Html(html) => MessageBody::Text(plain_text(html).into()),
            text => text.clone(),
        };
        let mut notice = (self.crashed && matches!(self.body, MessageBody::Html(_)))
            .then_some(ReaderNotice::Crashed);
        let shown = if notice.is_some() {
            as_plain()
        } else {
            self.body.clone()
        };
        let rendered = email_document(self.colors, &shown, self.remote);
        let mut document = match HtmlDocument::try_parse(&rendered.html, options()) {
            Ok(document) => document,
            Err(RenderError::TooDeep { .. }) => {
                notice = Some(ReaderNotice::Unrenderable);
                let fallback = email_document(self.colors, &as_plain(), false);
                HtmlDocument::parse(&fallback.html, options())
            }
        };
        document.prepare(self.width);
        BuiltReader {
            document: DetachedDocument::new(document),
            blocked_images: if notice.is_some() {
                0
            } else {
                rendered.blocked_images
            },
            notice,
        }
    }
}

/// Why the reader shows a message as plain text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReaderNotice {
    /// The renderer refused it (`RenderError`).
    Unrenderable,
    /// The app closed while rendering it last time (see [`CrashGuard`]).
    Crashed,
}

/// A reader context menu: where it was summoned and what it applies to.
struct ReaderMenu {
    position: Point<Pixels>,
    hit: HitInfo,
    has_selection: bool,
}

/// The attachment chip menu (Open / Save As… / Save to Downloads): where it
/// was summoned and which attachment it acts on.
struct AttachmentMenu {
    position: Point<Pixels>,
    attachment: AttachmentRow,
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

/// Text selection highlight of the reader (RGBA). Over a light page it is the
/// accent already blended with the page and fully opaque: a translucent fill
/// lowers the window's own alpha where it paints, and on a translucent window
/// the dark desktop then shows through a white page, turning it dark blue.
/// Text is painted after it, so opaque hides nothing. A dark page stays
/// translucent to keep dark text on white blocks of the message readable.
fn reader_selection_color(accent: Hsla, page: Hsla) -> [u8; 4] {
    let tint = accent.opacity(0.35);
    let color = if page.l > 0.5 { page.blend(tint) } else { tint };
    let rgba: gpui::Rgba = color.into();
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
    /// Path of [`Self::db`]; the sync engine opens its own connections on it.
    db_path: std::path::PathBuf,
    /// Background sync engine. `None` until [`Self::start_mail`] (and in
    /// unit tests, which never touch the network).
    mail: Option<MailService>,
    /// Accounts that could not start syncing (no keychain secret, …).
    account_start_errors: HashMap<i64, StartError>,
    /// A coalesced reload of accounts/folders/list is already scheduled.
    reload_scheduled: bool,
    /// Short-lived status bar note (message sent, send failed, …) and the
    /// token of the timer that clears it.
    status_note: Option<SharedString>,
    status_note_seq: u64,
    /// Accounts being removed in the background: hidden right away and
    /// filtered out of every reload until their deletion finishes.
    removing_accounts: HashSet<i64>,
    /// Outgoing messages that stopped retrying (status bar "N unsent").
    unsent: Vec<UnsentItem>,
    /// The "unsent" panel above the status bar is open.
    unsent_panel_open: bool,
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
    /// The width the user chose; [`Self::sync_layout`] narrows `sidebar_width` from
    /// it for the moment only, so a smaller window or a zoom doesn't cost it.
    preferred_sidebar_width: Pixels,
    /// User-adjustable width of the message list.
    list_width: Pixels,
    /// As [`Self::preferred_sidebar_width`], for `list_width`.
    preferred_list_width: Pixels,
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
    /// Where settings are written; a field so tests can keep off the real profile.
    config_file: Option<std::path::PathBuf>,
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
    /// Scroll handle (virtualized `uniform_list`) + scrollbar state for the
    /// message list.
    list_scroll: UniformListScrollHandle,
    list_scrollbar: Option<Entity<ScrollbarState>>,
    reader_v_scrollbar: Option<Entity<ScrollbarState>>,
    reader_h_scrollbar: Option<Entity<ScrollbarState>>,
    list_filter: ListFilter,
    /// Where the filter button's menu is open.
    filter_menu: Option<Point<Pixels>>,
    /// Where the Move button's menu is open.
    move_menu: Option<Point<Pixels>>,
    /// Rows of a selection of two or more, in list order. While it is set,
    /// `selected_message_id` is `None`: the reader has no single message.
    multi_selection: Vec<i64>,
    /// Message the reader shows for a multiple selection (the row picked last).
    /// Apart from `selected_detail`, so single-message commands stay off and
    /// looking at it doesn't mark it read.
    multi_preview: Option<MessageDetail>,
    /// Fixed end of a Shift selection and the end that moves.
    selection_anchor: Option<i64>,
    selection_focus: Option<i64>,
    /// What each mailbox had selected, for coming back to it (this session only).
    remembered_selections: HashMap<Selection, RememberedSelection>,
    /// Message whose recipients show in full.
    expanded_recipients: Option<i64>,
    /// Row under the mouse, whose text makes room for its action buttons.
    hovered_row: Option<i64>,
    /// Focus of the message list (key context `MessageList`: arrows/j/k move
    /// the selection, Delete/Backspace trash). Created on first render.
    list_focus: Option<FocusHandle>,
    /// User-facing (non-inline) attachments of the selected message; empty
    /// until its body is downloaded.
    attachments: Vec<AttachmentRow>,
    /// Open attachment chip menu.
    attachment_menu: Option<AttachmentMenu>,
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
    /// Messages whose rendering crashed the app (shown as plain text).
    crashed_messages: HashSet<i64>,
    /// Marker of the message being rendered (crash-loop protection).
    crash_guard: CrashGuard,
    /// Inputs that last produced the reader document (selected message and the
    /// colors it was themed with). Used to skip re-parsing the body on every
    /// render when nothing that affects the document has changed.
    last_reader_sig: Option<ReaderSignature>,
    /// A large message is being built in the background.
    reader_preparing: bool,
    /// Identifies the newest reader build; older ones are dropped when done.
    reader_build_token: u64,
    /// Desktop notifications for new mail (Settings ▸ Notifications).
    pub(crate) notifications_enabled: bool,
    /// Persisted "load remote content" preference. Remote content needs
    /// networking (Stage 3), so it is kept but not offered or applied yet.
    pub(crate) load_remote_images: bool,
    /// Messages whose remote images the user loaded this session.
    remote_allowed: HashSet<i64>,
    /// Whether the reader always paints the message body on a white background
    /// (dark text), ignoring the app theme. Persisted; off by default.
    pub(crate) reader_white_background: bool,
    /// Whether the compose message body uses a white background. Persisted; off
    /// by default.
    pub(crate) compose_white_background: bool,
    /// Persisted copy of the UI language, a global like the theme.
    language: Language,
    /// Zoom of the whole app, applied to every window it opens.
    ui_scale: f32,
    /// Persisted copy of the theme choice (the theme itself is an app global,
    /// which the settings snapshot can't read).
    light_theme: bool,
    /// Whether the first-run setup is over; see `onboarding::needs_onboarding`.
    onboarding_completed: bool,
    /// The running first-run setup, created on first render (needs a context).
    onboarding: Option<Entity<crate::onboarding::OnboardingView>>,
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

/// Minimal parsed form of a stored message whose raw source is missing (local/demo
/// messages, or one whose body wasn't downloaded yet), so reply/forward still work.
fn parsed_from_detail(detail: &MessageDetail) -> ParsedMessage {
    let from = Address::new(
        (!detail.sender.is_empty()).then(|| detail.sender.clone()),
        detail.sender_email.clone(),
    );
    let (text, html) = if detail.raw_format == "html" {
        (None, Some(detail.raw_content.clone()))
    } else {
        (Some(detail.plain_text.clone()), None)
    };
    ParsedMessage {
        message_id: detail.message_id_hdr.clone(),
        in_reply_to: detail.in_reply_to.clone(),
        references: detail
            .references_hdr
            .split_whitespace()
            .map(str::to_string)
            .collect(),
        subject: detail.subject.clone(),
        from: Some(from),
        reply_to: mail_core::valid_addresses(&detail.reply_to),
        to: mail_core::valid_addresses(&detail.to_addrs),
        cc: mail_core::valid_addresses(&detail.cc_addrs),
        text,
        html,
        ..ParsedMessage::default()
    }
}

/// The original's real attachments, carried over when forwarding.
fn forwarded_attachments(raw: &[u8], parsed: &ParsedMessage) -> Vec<mail_core::OutgoingAttachment> {
    parsed
        .attachments
        .iter()
        .filter(|info| !info.inline)
        .filter_map(|info| {
            Some(mail_core::OutgoingAttachment {
                filename: info.filename.clone(),
                mime_type: info.mime_type.clone(),
                data: mail_core::attachment_content(raw, info.index)?,
            })
        })
        .collect()
}

/// An outgoing message that stopped retrying (`failed` / `maybe_sent`), as
/// listed in the status bar's "unsent" panel.
#[derive(Clone, Debug, PartialEq, Eq)]
struct UnsentItem {
    id: i64,
    account_id: i64,
    /// Parsed from the raw message; empty when it has none.
    subject: String,
    /// Envelope recipients (Bcc included), comma-separated.
    recipients: String,
    error: Option<String>,
    /// The connection broke mid-send: the server may have accepted it.
    maybe_sent: bool,
}

/// Summarizes an unsent outbox row (`subject` comes from its parsed raw).
fn unsent_item(row: &OutgoingRow, subject: String) -> UnsentItem {
    UnsentItem {
        id: row.id,
        account_id: row.account_id,
        subject,
        recipients: row.recipients.join(", "),
        error: row
            .last_error
            .as_deref()
            .map(str::trim)
            .filter(|error| !error.is_empty())
            .map(str::to_string),
        maybe_sent: row.state == OutgoingState::MaybeSent,
    }
}

/// Outbox rows that stopped retrying, minus accounts being removed. Subjects
/// of rows already in `previous` are reused instead of re-parsing the raw.
fn load_unsent(
    db: &storage::Database,
    removing: &HashSet<i64>,
    previous: Vec<UnsentItem>,
) -> Vec<UnsentItem> {
    let rows = match db.failed_outgoing() {
        Ok(rows) => rows,
        Err(error) => {
            eprintln!("BGMail: could not read the outbox: {error}");
            return previous;
        }
    };
    let mut subjects: HashMap<i64, String> = previous
        .into_iter()
        .map(|item| (item.id, item.subject))
        .collect();
    // The listing has no raw bytes: read them once, for rows new to the list.
    let subject_of = |id: i64| {
        db.outgoing(id)
            .ok()
            .flatten()
            .and_then(|row| mail_core::parse_message(&row.raw))
            .map(|parsed| parsed.subject)
            .unwrap_or_default()
    };
    rows.iter()
        .filter(|row| !removing.contains(&row.account_id))
        .map(|row| {
            let subject = subjects
                .remove(&row.id)
                .unwrap_or_else(|| subject_of(row.id));
            unsent_item(row, subject)
        })
        .collect()
}

/// Compose content that re-creates an unsent message for editing. Bcc never
/// reaches the headers, so envelope recipients missing from To/Cc are
/// restored as Bcc. Sending it replaces the outbox row.
fn compose_init_from_outgoing(row: &OutgoingRow, parsed: &ParsedMessage) -> ComposeInit {
    let bcc = if parsed.bcc.is_empty() {
        row.recipients
            .iter()
            .filter(|recipient| {
                !parsed
                    .to
                    .iter()
                    .chain(&parsed.cc)
                    .any(|address| mail_core::same_email(&address.email, recipient))
            })
            .map(|recipient| Address::new(None::<String>, recipient.clone()))
            .collect()
    } else {
        parsed.bcc.clone()
    };
    ComposeInit {
        account_id: Some(row.account_id),
        draft: Draft {
            from: parsed
                .from
                .clone()
                .unwrap_or_else(|| Address::new(None::<String>, row.sender.clone())),
            to: parsed.to.clone(),
            cc: parsed.cc.clone(),
            bcc,
            subject: parsed.subject.clone(),
            text: parsed.text.clone().unwrap_or_default(),
            html: None,
            in_reply_to: parsed.in_reply_to.clone(),
            references: parsed.references.clone(),
            attachments: forwarded_attachments(&row.raw, parsed),
        },
        replaces_outgoing: Some(row.id),
        quoted_original: None,
    }
}

/// Process-unique number for Message-IDs (time + counter + random seed).
fn unique_token() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    hasher.finish()
}

/// The attachment chips of `detail`: its non-inline attachments, and none
/// while only the headers are downloaded (`BodyReady` reloads them).
fn visible_attachments(
    db: &storage::Database,
    detail: Option<&MessageDetail>,
) -> Vec<AttachmentRow> {
    let Some(detail) = detail.filter(|detail| detail.body_state != storage::BODY_STATE_HEADERS)
    else {
        return Vec::new();
    };
    db.list_attachments(detail.id)
        .unwrap_or_default()
        .into_iter()
        .filter(|attachment| !attachment.inline)
        .collect()
}

/// Sidebar position of a system folder (Inbox, Drafts, Sent, Archive, Junk,
/// Trash, then Flagged); `None` for user folders.
fn system_folder_rank(path: &str) -> Option<usize> {
    use storage::system;
    [
        system::INBOX,
        system::DRAFTS,
        system::SENT,
        system::ARCHIVE,
        system::JUNK,
        system::TRASH,
        system::FLAGGED,
    ]
    .iter()
    .position(|system_path| *system_path == path)
}

/// Folders in sidebar order: system folders by [`system_folder_rank`], then
/// user folders alphabetically (case-insensitive) by display name.
fn sidebar_ordered(mut folders: Vec<storage::Folder>) -> Vec<storage::Folder> {
    folders.sort_by_cached_key(|folder| {
        let rank = system_folder_rank(&folder.path);
        let name = if folder.display_name.is_empty() {
            folder.path.to_lowercase()
        } else {
            folder.display_name.to_lowercase()
        };
        (rank.is_none(), rank, name)
    });
    folders
}

/// Row index after moving `step` rows from `current` in a list of `len`
/// rows, clamped to the ends. Without a current row, moving down starts at
/// the first row and moving up at the last. `None` for an empty list.
fn adjacent_index(current: Option<usize>, len: usize, step: isize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let last = len - 1;
    Some(match current {
        None if step < 0 => last,
        None => 0,
        Some(ix) => (ix.min(last) as isize + step).clamp(0, last as isize) as usize,
    })
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
        // Sample mail only for demos/previews; a real profile starts empty and
        // shows the "add an account" welcome.
        let seed = std::env::var_os("BGMAIL_DEMO").is_some()
            || std::env::var_os("BGMAIL_SAMPLE_DIR").is_some();
        Self::new_with_database(settings, db_path, seed)
    }

    fn new_with_database(
        settings: Config,
        db_path: impl AsRef<std::path::Path>,
        seed: bool,
    ) -> Self {
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
        let db_path = db_path.as_ref().to_path_buf();
        let db = storage::Database::open(&db_path).expect("failed to open mail database");
        if seed {
            let mut seed_messages = db_seed::seed_messages();
            if let Some(dir) = std::env::var_os("BGMAIL_SAMPLE_DIR") {
                seed_messages.extend(db_seed::sample_dir_messages(std::path::Path::new(&dir)));
            }
            storage::seed_if_empty(db.conn(), &db_seed::seed_accounts(), &seed_messages)
                .expect("failed to seed mail database");
        }

        let accounts = db.list_accounts().expect("failed to load accounts");
        let mut folders_by_account = HashMap::new();
        for account in &accounts {
            folders_by_account.insert(
                account.id,
                sidebar_ordered(db.list_folders(account.id).expect("failed to load folders")),
            );
        }

        let onboarding_completed =
            !crate::onboarding::needs_onboarding(settings.onboarding_completed, accounts.len());
        let selected_mailbox = Selection::Global(GlobalMailbox::Inbox);
        let list_messages = db
            .list_messages(&mail_list_query(&selected_mailbox, ""))
            .expect("failed to load messages");
        // Opening a message marks it read, so a real profile starts with no
        // selection; the demo keeps its showcase message open.
        let selected_message_id = seed
            .then(|| {
                list_messages
                    .get(3.min(list_messages.len().saturating_sub(1)))
                    .map(|m| m.id)
                    .or_else(|| list_messages.first().map(|m| m.id))
            })
            .flatten();
        let selected_detail = selected_message_id.and_then(|id| db.get_message(id).ok().flatten());
        let attachments = visible_attachments(&db, selected_detail.as_ref());
        let unsent = load_unsent(&db, &HashSet::new(), Vec::new());

        Self {
            db,
            db_path,
            mail: None,
            account_start_errors: HashMap::new(),
            reload_scheduled: false,
            status_note: None,
            status_note_seq: 0,
            removing_accounts: HashSet::new(),
            unsent,
            unsent_panel_open: false,
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
            preferred_sidebar_width: sidebar_width,
            list_width,
            preferred_list_width: list_width,
            narrow: false,
            window_width: px(settings.window_width),
            window_origin,
            window_size,
            maximized: settings.maximized,
            max_origin,
            max_size,
            save_seq: 0,
            persist_ready: false,
            config_file: config::config_path(),
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
            list_scroll: UniformListScrollHandle::new(),
            list_scrollbar: None,
            reader_v_scrollbar: None,
            reader_h_scrollbar: None,
            list_filter: ListFilter::All,
            filter_menu: None,
            move_menu: None,
            multi_selection: Vec::new(),
            multi_preview: None,
            selection_anchor: None,
            remembered_selections: HashMap::new(),
            selection_focus: None,
            expanded_recipients: None,
            hovered_row: None,
            list_focus: None,
            attachments,
            attachment_menu: None,
            sidebar_scroll: ScrollHandle::new(),
            sidebar_scrollbar: None,
            reader_view: None,
            _reader_subscription: None,
            reader_menu: None,
            find_bar: None,
            reader_notice: None,
            crashed_messages: crash_guard.take_unfinished().into_iter().collect(),
            crash_guard,
            last_reader_sig: None,
            reader_preparing: false,
            reader_build_token: 0,
            notifications_enabled: settings.notifications_enabled,
            load_remote_images: settings.load_remote_images,
            remote_allowed: HashSet::new(),
            reader_white_background: settings.reader_white_background,
            compose_white_background: settings.compose_white_background,
            language: settings.language(),
            ui_scale: crate::app_zoom::sanitize(settings.ui_scale),
            light_theme: settings.light_theme,
            onboarding_completed,
            onboarding: None,
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
        if !self.removing_accounts.is_empty() {
            let removing = &self.removing_accounts;
            self.list_messages
                .retain(|message| !removing.contains(&message.account_id));
        }
        if self.list_filter != ListFilter::All {
            // A message being read stays until the selection moves, even if it
            // no longer matches: opening an unread one marks it read.
            let (filter, selected) = (self.list_filter, self.selected_message_id);
            let multi: HashSet<i64> = self.multi_selection.iter().copied().collect();
            self.list_messages.retain(|message| {
                filter.matches(message)
                    || selected == Some(message.id)
                    || multi.contains(&message.id)
            });
        }
        let collapsed = self.prune_multi_selection();
        if let Some(selected) = self.selected_message_id {
            if self.list_messages.iter().any(|m| m.id == selected) {
                self.reload_selected_detail();
                if collapsed {
                    // Now it's an open message, and it may have only its headers.
                    self.request_selected_body();
                }
            } else {
                self.clear_message_selection();
            }
        }
    }

    /// Drops selected rows that left the list; one survivor is a plain selection
    /// again (`true`), and with none left the reader doesn't keep showing a message
    /// that isn't listed.
    fn prune_multi_selection(&mut self) -> bool {
        let listed: HashSet<i64> = self.list_messages.iter().map(|m| m.id).collect();
        self.multi_selection.retain(|id| listed.contains(id));
        if let [only] = self.multi_selection[..] {
            self.selected_message_id = Some(only);
            self.multi_selection.clear();
            self.multi_preview = None;
            return true;
        }
        if !self.multi_selection.is_empty() {
            self.reload_multi_preview();
        } else if self.multi_preview.is_some() {
            self.clear_message_selection();
        }
        false
    }

    /// Re-reads the message shown for a multiple selection and its chips.
    fn reload_multi_preview(&mut self) {
        let id = self
            .selection_focus
            .filter(|id| self.multi_selection.contains(id))
            .or(self.multi_selection.first().copied());
        self.multi_preview = id.and_then(|id| self.db.get_message(id).ok().flatten());
        self.attachments = visible_attachments(&self.db, self.multi_preview.as_ref());
    }

    /// The message in the reader: the selected one or a multiple selection's.
    fn reader_detail(&self) -> Option<&MessageDetail> {
        self.selected_detail
            .as_ref()
            .or(self.multi_preview.as_ref())
    }

    /// Every selected row, however many.
    fn selected_ids(&self) -> Vec<i64> {
        match self.selected_message_id {
            Some(id) => vec![id],
            None => self.multi_selection.clone(),
        }
    }

    fn row_is_selected(&self, id: i64) -> bool {
        self.selected_message_id == Some(id) || self.multi_selection.contains(&id)
    }

    fn list_ids(&self) -> Vec<i64> {
        self.list_messages.iter().map(|m| m.id).collect()
    }

    /// Makes `ids` the selection. Returns whether a single message came out of
    /// it, which the caller then opens.
    fn apply_selection(&mut self, ids: Vec<i64>) -> bool {
        self.clear_message_selection();
        match ids[..] {
            [] => false,
            [id] => {
                self.selected_message_id = Some(id);
                true
            }
            _ => {
                self.multi_selection = ids;
                self.reload_multi_preview();
                false
            }
        }
    }

    fn set_selection(&mut self, ids: Vec<i64>, cx: &mut Context<Self>) {
        if self.apply_selection(ids) {
            self.on_message_selection_changed(cx);
        } else {
            self.fetch_multi_preview_body();
            self.sync_app_menus(cx);
            cx.notify();
        }
    }

    /// Downloads the body of a previewed message that only has its headers.
    fn fetch_multi_preview_body(&self) {
        if let (Some(detail), Some(mail)) = (&self.multi_preview, &self.mail) {
            if detail.body_state == storage::BODY_STATE_HEADERS {
                mail.engine()
                    .send(detail.account_id, Command::FetchBody(detail.id));
            }
        }
    }

    /// Mouse selection of a row: see [`clicked_selection`].
    fn click_message_row(
        &mut self,
        id: i64,
        modifiers: gpui::Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(focus) = &self.list_focus {
            window.focus(focus);
        }
        let ids = clicked_selection(
            &self.list_ids(),
            &self.selected_ids(),
            self.selection_anchor,
            id,
            modifiers.shift,
            modifiers.secondary(),
        );
        if !modifiers.shift {
            self.selection_anchor = Some(id);
        }
        self.selection_focus = Some(id);
        self.set_selection(ids, cx);
    }

    /// Core of Shift+arrow: moves the free end of the selection `step` rows.
    /// Returns the selected rows, or `None` when it can't move.
    fn extended_selection(&mut self, step: isize) -> Option<Vec<i64>> {
        let list = self.list_ids();
        let focus = self.selection_focus.or(self.selected_message_id);
        let current = focus.and_then(|id| list.iter().position(|row| *row == id));
        let target = list[adjacent_index(current, list.len(), step)?];
        let anchor = self.selection_anchor.or(focus).unwrap_or(target);
        self.selection_anchor = Some(anchor);
        self.selection_focus = Some(target);
        Some(selection_range(&list, anchor, target))
    }

    fn extend_selection(&mut self, step: isize, cx: &mut Context<Self>) {
        if let Some(ids) = self.extended_selection(step) {
            if let Some(ix) = self
                .selection_focus
                .and_then(|id| self.list_messages.iter().position(|m| m.id == id))
            {
                self.list_scroll.scroll_to_item(ix, ScrollStrategy::Top);
            }
            self.set_selection(ids, cx);
        }
    }

    fn set_list_filter(&mut self, filter: ListFilter, cx: &mut Context<Self>) {
        self.list_filter = filter;
        self.filter_menu = None;
        self.reload_message_list();
        self.sync_app_menus(cx);
        cx.notify();
    }

    fn clear_message_selection(&mut self) {
        self.multi_selection.clear();
        self.multi_preview = None;
        self.selected_message_id = None;
        self.selected_detail = None;
        self.attachments.clear();
        self.attachment_menu = None;
        self.last_reader_sig = None;
    }

    /// Re-reads the selected message and its attachment chips.
    fn reload_selected_detail(&mut self) {
        self.selected_detail = self
            .selected_message_id
            .and_then(|id| self.db.get_message(id).ok().flatten());
        self.attachments = visible_attachments(&self.db, self.selected_detail.as_ref());
        if self
            .attachment_menu
            .as_ref()
            .is_some_and(|menu| Some(menu.attachment.message_id) != self.selected_message_id)
        {
            self.attachment_menu = None;
        }
    }

    fn on_message_selection_changed(&mut self, cx: &mut Context<Self>) {
        self.reload_selected_detail();
        self.open_selected_message();
        self.sync_app_menus(cx);
        cx.notify();
    }

    /// Selects the message `step` rows below (positive) or above the current
    /// one (keyboard navigation), opens it and keeps it visible. Returns
    /// whether the selection moved.
    fn move_selection(&mut self, step: isize, cx: &mut Context<Self>) -> bool {
        if !self.select_adjacent_message(step) {
            return false;
        }
        self.reveal_selected_message();
        self.on_message_selection_changed(cx);
        true
    }

    /// Core of [`Self::move_selection`] without side effects on the server
    /// or the window: moves `selected_message_id` (no wrap-around).
    fn select_adjacent_message(&mut self, step: isize) -> bool {
        let current = self
            .selected_message_id
            .or(self.selection_focus)
            .and_then(|id| self.list_messages.iter().position(|m| m.id == id));
        // With nothing open, the row the keys are on is the next one to open.
        let nothing_selected =
            self.selected_message_id.is_none() && self.multi_selection.is_empty();
        let target = match current {
            Some(row) if nothing_selected => Some(row),
            _ => adjacent_index(current, self.list_messages.len(), step),
        };
        let Some(target) = target else {
            return false;
        };
        if !nothing_selected && Some(target) == current {
            return false;
        }
        let id = self.list_messages[target].id;
        self.select_only(id);
        true
    }

    /// Makes `id` the only selected row, ending any multiple selection and its preview.
    fn select_only(&mut self, id: i64) {
        self.multi_selection.clear();
        self.selected_message_id = Some(id);
        self.selection_anchor = Some(id);
        self.selection_focus = Some(id);
        self.last_reader_sig = None;
        true
    }

    /// Scrolls the message list so the selected row is visible.
    fn reveal_selected_message(&self) {
        if let Some(ix) = self
            .selected_message_id
            .and_then(|id| self.list_messages.iter().position(|m| m.id == id))
        {
            self.list_scroll.scroll_to_item(ix, ScrollStrategy::Top);
        }
    }

    /// Side effects of opening the selected message: it becomes read (on the
    /// server too) and a body that was not downloaded yet is fetched now.
    fn open_selected_message(&mut self) {
        let Some(detail) = self.selected_detail.as_ref() else {
            return;
        };
        let (id, account_id) = (detail.id, detail.account_id);
        if detail.unread {
            if let Ok(outcome) = mail_actions::set_read(&self.db, id, true) {
                self.after_action(account_id, outcome);
            }
            if let Some(item) = self.list_messages.iter_mut().find(|m| m.id == id) {
                item.unread = false;
            }
            self.reload_selected_detail();
        }
        self.request_selected_body();
    }

    /// Fetches the selected message's body, if it was not downloaded yet.
    fn request_selected_body(&self) {
        let Some(detail) = self
            .selected_detail
            .as_ref()
            .filter(|detail| detail.body_state == storage::BODY_STATE_HEADERS)
        else {
            return;
        };
        if let Some(mail) = &self.mail {
            mail.engine()
                .send(detail.account_id, Command::FetchBody(detail.id));
        }
    }

    /// Hands queued server work to the account's sync worker.
    fn after_action(&self, account_id: i64, outcome: ActionOutcome) {
        if outcome.needs_sync {
            if let Some(mail) = &self.mail {
                mail.process_queue(account_id);
            }
        }
    }

    /// Runs a message action on the selected message through `mail_sync`
    /// (local change now, server op queued); see [`Self::apply_message_action`]
    /// for what stays selected.
    fn run_message_action(
        &mut self,
        cx: &mut Context<Self>,
        action: impl Fn(&storage::Database, i64) -> rusqlite::Result<ActionOutcome>,
    ) {
        if !self.multi_selection.is_empty() {
            self.apply_action_to_selection(action, cx);
            return;
        }
        let Some(id) = self.selected_message_id else {
            return;
        };
        self.run_action_on_message(id, cx, action);
    }

    /// Runs `action` on every row of a multiple selection, even if one fails
    /// (the first error is returned), and leaves nothing selected: opening the
    /// next message would mark it read without it being asked for.
    fn apply_action_to_selection(
        &mut self,
        action: impl Fn(&storage::Database, i64) -> rusqlite::Result<ActionOutcome> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let ids = std::mem::take(&mut self.multi_selection);
        self.run_bulk_action(ids.clone(), action, cx);
        let ids: HashSet<i64> = ids.into_iter().collect();
        self.list_messages
            .retain(|message| !ids.contains(&message.id));
        self.clear_message_selection();
        self.reload_message_list();
        result
    }

    /// Runs `action` on every row of a multiple selection that stays selected
    /// afterwards, because the rows stay listed (read state, star).
    fn apply_action_keeping_selection(
        &mut self,
        action: impl Fn(&storage::Database, i64) -> rusqlite::Result<ActionOutcome> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.run_bulk_action(self.multi_selection.clone(), action, cx);
    }

    /// Runs `action` on `ids` off the UI thread, on a connection of its own: each
    /// message takes several disk writes, and a selection can hold every message of
    /// a folder. Then reloads, reports the first error and wakes the workers.
    fn run_bulk_action(
        &self,
        ids: Vec<i64>,
        action: impl Fn(&storage::Database, i64) -> rusqlite::Result<ActionOutcome> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let accounts: HashMap<i64, i64> = self
            .list_messages
            .iter()
            .map(|message| (message.id, message.account_id))
            .collect();
        let path = self.db_path.clone();
        let work = cx.background_spawn(async move {
            let mut to_wake = HashSet::new();
            let db = match storage::Database::open(&path) {
                Ok(db) => db,
                Err(error) => return (to_wake, Some(error)),
            };
            let mut first_error = None;
            for id in ids {
                match action(&db, id) {
                    Ok(outcome) if outcome.needs_sync => {
                        to_wake.extend(accounts.get(&id).copied());
                    }
                    Ok(_) => {}
                    Err(error) => {
                        first_error.get_or_insert(error);
                    }
                }
            }
            (to_wake, first_error)
        });
        cx.spawn(async move |this, cx| {
            let (to_wake, error) = work.await;
            this.update(cx, |this, cx| {
                for account_id in to_wake {
                    this.after_action(
                        account_id,
                        ActionOutcome {
                            changed: true,
                            needs_sync: true,
                        },
                    );
                }
                this.reload_message_list();
                this.last_reader_sig = None;
                if let Some(error) = error {
                    this.set_status_note(
                        format!("{}: {error}", Key::StatusActionFailed.tr(cx.language())),
                        cx,
                    );
                }
                this.sync_app_menus(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Runs `action` on message `id` and hands its server work to the worker.
    fn act_on_message(
        &self,
        id: i64,
        action: impl FnOnce(&storage::Database, i64) -> rusqlite::Result<ActionOutcome>,
    ) -> rusqlite::Result<()> {
        let account_id = self
            .list_messages
            .iter()
            .find(|message| message.id == id)
            .map(|message| message.account_id)
            .or_else(|| {
                self.selected_detail
                    .as_ref()
                    .filter(|detail| detail.id == id)
                    .map(|detail| detail.account_id)
            });
        action(&self.db, id).map(|outcome| {
            if let Some(account_id) = account_id {
                self.after_action(account_id, outcome);
            }
        })
    }

    /// [`Self::run_message_action`] for any message, selected or not.
    fn run_action_on_message(
        &mut self,
        id: i64,
        cx: &mut Context<Self>,
        action: impl FnOnce(&storage::Database, i64) -> rusqlite::Result<ActionOutcome>,
    ) {
        if let Err(error) = self.apply_message_action(id, action) {
            self.set_status_note(
                format!("{}: {error}", Key::StatusActionFailed.tr(cx.language())),
                cx,
            );
        }
        self.sync_app_menus(cx);
        self.last_reader_sig = None;
        cx.notify();
    }

    /// Runs `action` on message `id` and refreshes the list. As in Gmail, a selected
    /// message that left the list leaves nothing open, rather than opening the next one
    /// and marking it read; the arrow keys carry on from the row that took its place.
    fn apply_message_action(
        &mut self,
        id: i64,
        action: impl FnOnce(&storage::Database, i64) -> rusqlite::Result<ActionOutcome>,
    ) -> rusqlite::Result<()> {
        let position = self.list_messages.iter().position(|m| m.id == id);
        let was_selected = self.selected_message_id == Some(id);
        let result = self.act_on_message(id, action);
        self.reload_message_list();
        if was_selected && self.selected_message_id.is_none() {
            self.selection_focus = position.and_then(|ix| {
                self.list_messages
                    .get(ix)
                    .or_else(|| self.list_messages.last())
                    .map(|m| m.id)
            });
        }
        result
    }

    /// Moves messages dragged onto a folder of their account.
    fn move_messages_to_folder(&mut self, ids: &[i64], folder_path: &str, cx: &mut Context<Self>) {
        let target = folder_path.to_string();
        let action =
            move |db: &storage::Database, id: i64| mail_actions::move_to_folder(db, id, &target);
        match ids {
            [id] => self.run_action_on_message(*id, cx, action),
            _ => {
                self.multi_selection = ids.to_vec();
                self.run_message_action(cx, action);
            }
        }
    }

    /// What dragging row `id` takes along: the whole selection when the row is
    /// part of it. `None` when that includes sent mail, which can't be moved.
    fn dragged_messages(&self, id: i64) -> Option<DraggedMessages> {
        let ids = if self.multi_selection.contains(&id) {
            self.multi_selection.clone()
        } else {
            vec![id]
        };
        let chosen: HashSet<i64> = ids.iter().copied().collect();
        let sent_only = self.list_messages.iter().any(|message| {
            chosen.contains(&message.id) && commands::message_only_in_sent(&message.folders_csv)
        });
        if sent_only || self.showing_sent() {
            return None;
        }
        let mut accounts = self
            .list_messages
            .iter()
            .filter(|message| ids.contains(&message.id))
            .map(|message| message.account_id);
        let first = accounts.next();
        Some(DraggedMessages {
            account_id: first.filter(|first| accounts.all(|other| other == *first)),
            ids,
            progress: Rc::default(),
        })
    }

    /// Toggles read/unread of the selected message (it stays selected).
    pub(crate) fn toggle_message_read(&mut self, cx: &mut Context<Self>) {
        if let Some(bulk) = self.bulk_selection() {
            let read = bulk.has_unread;
            self.apply_action_keeping_selection(
                move |db, id| mail_actions::set_read(db, id, read),
                cx,
            );
            return;
        }
        let Some(detail) = self.selected_detail.as_ref() else {
            return;
        };
        let read = detail.unread;
        self.run_message_action(cx, move |db, id| mail_actions::set_read(db, id, read));
    }

    // ----- Sync engine ------------------------------------------------------

    /// Starts the sync engine and every remote account. Secrets are read from
    /// the keychain asynchronously, so startup never waits on them (or on the
    /// network).
    pub(crate) fn start_mail(&mut self, cx: &mut Context<Self>) {
        if self.mail.is_some() {
            return;
        }
        let (service, events) = MailService::new(self.db_path.clone());
        self.mail = Some(service);
        cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                if this
                    .update(cx, |this, cx| this.on_sync_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        for account in self.accounts.clone() {
            self.start_account(&account, cx);
        }
    }

    /// Reads an account's secret from the keychain and starts its worker.
    fn start_account(&mut self, account: &storage::Account, cx: &mut Context<Self>) {
        if account.provider == storage::PROVIDER_LOCAL {
            return;
        }
        let Some(config) = AccountConfig::from_json(&account.config_json) else {
            self.account_start_errors
                .insert(account.id, StartError::NotRemote);
            return;
        };
        let read = cx.read_credentials(&config.secret_key());
        let account_id = account.id;
        let config_json = account.config_json.clone();
        cx.spawn(async move |this, cx| {
            let secret = read.await.ok().flatten().map(|(_, secret)| secret);
            this.update(cx, |this, cx| {
                this.start_account_with_secret(account_id, &config_json, secret.as_deref(), cx)
            })
            .ok();
        })
        .detach();
    }

    fn start_account_with_secret(
        &mut self,
        account_id: i64,
        config_json: &str,
        secret: Option<&[u8]>,
        cx: &mut Context<Self>,
    ) {
        // Removed while its keychain read was pending: a worker started now
        // would outlive the account, the removal's stop job being superseded.
        if !self.accounts.iter().any(|account| account.id == account_id) {
            return;
        }
        let Some(mail) = self.mail.as_mut() else {
            return;
        };
        let connector = secret
            .ok_or(StartError::MissingSecret)
            .and_then(|secret| mail_service::connector_for(config_json, secret));
        match connector {
            Ok(connector) => {
                self.account_start_errors.remove(&account_id);
                // Replacing a running worker waits for it to wind down.
                let start = mail.start_account(account_id, connector);
                cx.background_spawn(async move { start() }).detach();
            }
            Err(error) => {
                self.account_start_errors.insert(account_id, error);
            }
        }
        cx.notify();
    }

    /// Applies one engine event to the UI.
    fn on_sync_event(&mut self, event: SyncEvent, cx: &mut Context<Self>) {
        match event {
            SyncEvent::Status { account_id, status } => {
                if self.removing_accounts.contains(&account_id) {
                    return;
                }
                if let Some(mail) = self.mail.as_mut() {
                    mail.set_status(account_id, status);
                }
                cx.notify();
            }
            SyncEvent::MailboxChanged { .. } | SyncEvent::FoldersChanged { .. } => {
                self.schedule_reload(cx);
            }
            SyncEvent::NewMail {
                account_id,
                messages,
            } => {
                if self.notifications_enabled {
                    let account = self
                        .accounts
                        .iter()
                        .find(|account| account.id == account_id)
                        .map(|account| account.name.clone())
                        .unwrap_or_default();
                    crate::notifications::notify_new_mail(&account, &messages, cx.language());
                }
                self.schedule_reload(cx);
            }
            SyncEvent::BodyReady { message_id } => {
                if self.selected_message_id == Some(message_id) {
                    self.reload_selected_detail();
                    self.last_reader_sig = None;
                }
                if self
                    .multi_preview
                    .as_ref()
                    .is_some_and(|d| d.id == message_id)
                {
                    self.reload_multi_preview();
                    self.last_reader_sig = None;
                }
                self.schedule_reload(cx);
            }
            SyncEvent::BodyFailed { message_id, error } => {
                if self.selected_message_id == Some(message_id) {
                    self.set_status_note(
                        format!("{}: {error}", Key::StatusBodyFailed.tr(cx.language())),
                        cx,
                    );
                }
            }
            SyncEvent::Sent { .. } => {
                self.set_status_note(Key::StatusMessageSent.tr(cx.language()), cx);
                self.refresh_unsent();
                self.schedule_reload(cx);
            }
            SyncEvent::SendFailed {
                error, permanent, ..
            } => {
                let key = if permanent {
                    Key::StatusSendFailed
                } else {
                    Key::StatusSendRetrying
                };
                self.set_status_note(format!("{}: {error}", key.tr(cx.language())), cx);
                self.refresh_unsent();
            }
            SyncEvent::SentCopyFailed { error, .. } => {
                let failed = Key::StatusSentCopyFailed.tr(cx.language());
                self.set_status_note(format!("{failed}: {error}"), cx);
            }
        }
    }

    /// Shows `note` in the status bar for a few seconds.
    fn set_status_note(&mut self, note: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.status_note = Some(note.into());
        self.status_note_seq += 1;
        let token = self.status_note_seq;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(6)).await;
            this.update(cx, |this, cx| {
                if this.status_note_seq == token {
                    this.status_note = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Reloads accounts, folders and the list once a burst of engine events
    /// settles (an initial sync emits one event per header batch).
    fn schedule_reload(&mut self, cx: &mut Context<Self>) {
        if self.reload_scheduled {
            return;
        }
        self.reload_scheduled = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
            this.update(cx, |this, cx| {
                this.reload_scheduled = false;
                this.reload_from_database();
                this.sync_app_menus(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Re-reads accounts, folders and the current list from the database.
    pub(crate) fn reload_from_database(&mut self) {
        self.accounts = self.db.list_accounts().unwrap_or_default();
        if !self.removing_accounts.is_empty() {
            let removing = &self.removing_accounts;
            self.accounts
                .retain(|account| !removing.contains(&account.id));
        }
        self.folders_by_account = self
            .accounts
            .iter()
            .map(|account| {
                (
                    account.id,
                    sidebar_ordered(self.db.list_folders(account.id).unwrap_or_default()),
                )
            })
            .collect();
        self.reload_message_list();
        self.refresh_unsent();
    }

    /// Re-reads the outbox rows that stopped retrying (subjects already
    /// parsed are reused: the raw may carry large attachments).
    fn refresh_unsent(&mut self) {
        let previous = std::mem::take(&mut self.unsent);
        self.unsent = load_unsent(&self.db, &self.removing_accounts, previous);
        if self.unsent.is_empty() {
            self.unsent_panel_open = false;
        }
    }

    /// Asks every account to sync now ("Get mail").
    pub(crate) fn sync_now(&mut self, cx: &mut Context<Self>) {
        if let Some(mail) = &self.mail {
            for account in &self.accounts {
                mail.engine().send(account.id, Command::Reconnect);
            }
        }
        cx.notify();
    }

    // ----- Accounts -----------------------------------------------------------

    /// The stored accounts (settings window).
    pub(crate) fn accounts(&self) -> &[storage::Account] {
        &self.accounts
    }

    /// Sync status of an account, for the settings window.
    pub(crate) fn account_status(&self, account_id: i64) -> Option<mail_sync::AccountStatus> {
        self.mail
            .as_ref()
            .and_then(|mail| mail.status(account_id).cloned())
    }

    /// Why an account isn't syncing, when it couldn't start.
    pub(crate) fn account_start_error(&self, account_id: i64) -> Option<&StartError> {
        self.account_start_errors.get(&account_id)
    }

    /// Creates a remote account, stores its secret (password or OAuth refresh
    /// token) in the platform keychain and starts syncing it. Returns the new
    /// account id.
    pub(crate) fn add_remote_account(
        &mut self,
        name: &str,
        sender_name: &str,
        config: AccountConfig,
        secret: String,
        cx: &mut Context<Self>,
    ) -> Result<i64, String> {
        let provider = if config.uses_oauth() {
            storage::PROVIDER_GMAIL
        } else {
            storage::PROVIDER_IMAP
        };
        let name = if name.trim().is_empty() {
            config.email.clone()
        } else {
            name.trim().to_string()
        };
        let config_json = config.to_json();
        let account_id = self
            .db
            .create_account(&name, &config.email, provider, &config_json)
            .map_err(|error| error.to_string())?;
        self.db
            .set_account_names(account_id, &name, sender_name.trim())
            .map_err(|error| error.to_string())?;
        self.store_secret(&config, &secret, cx);
        self.reload_from_database();
        // A new worker begins with a full sync; no `SyncNow` needed (and it
        // starts off-thread, so it would not be running yet anyway).
        self.start_account_with_secret(account_id, &config_json, Some(secret.as_bytes()), cx);
        cx.notify();
        Ok(account_id)
    }

    /// Replaces an account's secret (e.g. after a password change) and
    /// reconnects it. The worker restart runs off the UI thread.
    /// Changes the names of an account: `name`, what the sidebar shows, and
    /// `sender_name`, what its mail goes out with. A blank `name` is ignored.
    pub(crate) fn set_account_names(
        &mut self,
        account_id: i64,
        name: &str,
        sender_name: &str,
        cx: &mut Context<Self>,
    ) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        match self
            .db
            .set_account_names(account_id, name, sender_name.trim())
        {
            Ok(()) => self.reload_from_database(),
            Err(error) => self.set_status_note(
                format!("{}: {error}", Key::StatusActionFailed.tr(cx.language())),
                cx,
            ),
        }
        cx.notify();
    }

    pub(crate) fn update_account_secret(
        &mut self,
        account_id: i64,
        secret: String,
        cx: &mut Context<Self>,
    ) {
        let Some(account) = self.accounts.iter().find(|a| a.id == account_id).cloned() else {
            return;
        };
        let Some(config) = AccountConfig::from_json(&account.config_json) else {
            return;
        };
        self.store_secret(&config, &secret, cx);
        self.start_account_with_secret(
            account_id,
            &account.config_json,
            Some(secret.as_bytes()),
            cx,
        );
    }

    fn store_secret(&self, config: &AccountConfig, secret: &str, cx: &mut Context<Self>) {
        let write = cx.write_credentials(&config.secret_key(), &config.username, secret.as_bytes());
        cx.background_spawn(async move {
            if let Err(error) = write.await {
                eprintln!("BGMail: could not save the account secret: {error}");
            }
        })
        .detach();
    }

    /// Stops syncing an account, forgets its secret and deletes its local mail.
    /// The account disappears from the UI at once; stopping its worker (up to
    /// a second) and deleting its mail (seconds for a big account) run in the
    /// background on their own connection, then everything reloads. A failed
    /// delete brings the account back with a status note.
    pub(crate) fn remove_account(&mut self, account_id: i64, cx: &mut Context<Self>) {
        let Some(account) = self.accounts.iter().find(|a| a.id == account_id).cloned() else {
            return;
        };
        let stop = self.mail.as_mut().map(|mail| mail.stop_account(account_id));
        self.account_start_errors.remove(&account_id);
        self.removing_accounts.insert(account_id);
        if self
            .selected_detail
            .as_ref()
            .is_some_and(|detail| detail.account_id == account_id)
        {
            self.clear_message_selection();
        }
        if matches!(self.selected_mailbox, Selection::Mailbox { account_id: selected, .. } if selected == account_id)
        {
            self.selected_mailbox = Selection::Global(GlobalMailbox::Inbox);
        }
        self.collapsed_accounts.clear();
        self.reload_from_database();
        self.sync_app_menus(cx);
        cx.notify();

        let db_path = self.db_path.clone();
        let removal = cx.background_spawn(async move {
            if let Some(stop) = stop {
                stop();
            }
            storage::Database::open(&db_path).and_then(|db| db.delete_account(account_id))
        });
        cx.spawn(async move |this, cx| {
            let result = removal.await;
            this.update(cx, |this, cx| {
                this.removing_accounts.remove(&account_id);
                let failure = result.err();
                if let Some(error) = &failure {
                    let failed = Key::StatusRemoveAccountFailed.tr(cx.language());
                    this.set_status_note(format!("{failed}: {error}"), cx);
                } else if let Some(config) = AccountConfig::from_json(&account.config_json) {
                    // Only now: a failed removal keeps a working account.
                    let delete = cx.delete_credentials(&config.secret_key());
                    cx.background_spawn(async move {
                        delete.await.ok();
                    })
                    .detach();
                }
                this.reload_from_database();
                if failure.is_some() {
                    this.start_account(&account, cx);
                }
                this.sync_app_menus(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // ----- Unsent mail ----------------------------------------------------------

    pub(crate) fn toggle_unsent_panel(&mut self, cx: &mut Context<Self>) {
        self.unsent_panel_open = !self.unsent_panel_open && !self.unsent.is_empty();
        cx.notify();
    }

    /// Shows a failed unsent-message action as a status note.
    fn report_unsent_error(&mut self, error: impl std::fmt::Display, cx: &mut Context<Self>) {
        let failed = Key::UnsentActionFailed.tr(cx.language());
        self.set_status_note(format!("{failed}: {error}"), cx);
    }

    /// Puts an unsent message back in the queue and wakes its worker.
    pub(crate) fn retry_unsent(&mut self, id: i64, cx: &mut Context<Self>) {
        let account_id = self
            .unsent
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.account_id);
        match self.db.retry_outgoing(id) {
            // Not stopped any more: nothing to do, the refresh below shows it.
            Ok(false) => {}
            Ok(true) => {
                if let (Some(mail), Some(account_id)) = (&self.mail, account_id) {
                    mail.process_queue(account_id);
                }
                self.set_status_note(Key::StatusSyncing.tr(cx.language()), cx);
            }
            Err(error) => self.report_unsent_error(error, cx),
        }
        self.refresh_unsent();
        cx.notify();
    }

    /// Opens compose prefilled with an unsent message. The outbox row stays
    /// until the edited message is queued (`send_draft` replaces it), so
    /// closing compose loses nothing.
    pub(crate) fn edit_unsent(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let row = match self.db.outgoing(id) {
            Ok(row) => row.filter(|row| row.state.is_stopped()),
            Err(error) => {
                self.report_unsent_error(error, cx);
                return;
            }
        };
        let Some(row) = row else {
            self.refresh_unsent();
            cx.notify();
            return;
        };
        let parsed = mail_core::parse_message(&row.raw).unwrap_or_default();
        self.unsent_panel_open = false;
        self.open_compose_with(Some(compose_init_from_outgoing(&row, &parsed)), window, cx);
    }

    /// Deletes an unsent message after confirmation.
    pub(crate) fn discard_unsent(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let language = cx.language();
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            Key::ComposeDiscardConfirm.tr(language),
            Some(Key::UnsentDiscardDetail.tr(language)),
            &[
                Key::ComposeDiscardButton.tr(language),
                Key::AccountCancel.tr(language),
            ],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await.ok() != Some(0) {
                return;
            }
            this.update(cx, |this, cx| {
                if let Err(error) = this.db.delete_outgoing(id) {
                    this.report_unsent_error(error, cx);
                }
                this.refresh_unsent();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Summary of a multiple selection for the commands.
    fn bulk_selection(&self) -> Option<BulkSelection> {
        if self.multi_selection.is_empty() {
            return None;
        }
        let selected: HashSet<i64> = self.multi_selection.iter().copied().collect();
        let mut bulk = BulkSelection {
            all_starred: true,
            ..Default::default()
        };
        for message in self
            .list_messages
            .iter()
            .filter(|m| selected.contains(&m.id))
        {
            if !bulk.account_ids.contains(&message.account_id) {
                bulk.account_ids.push(message.account_id);
            }
            bulk.has_unread |= message.unread;
            bulk.all_starred &= message.starred;
            bulk.any_sent_only |= commands::message_only_in_sent(&message.folders_csv);
        }
        Some(bulk)
    }

    /// Builds the command context snapshot used by menus and the palette.
    pub(crate) fn command_context(&self) -> CommandContext {
        CommandContext {
            selected_message_id: self.selected_message_id,
            message_detail: self.selected_detail.clone(),
            folders_by_account: self.folders_by_account.clone(),
            bulk: self.bulk_selection(),
            list_in_trash: self.showing_trash(),
            list_in_sent: self.showing_sent(),
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
            view.discard_draft(window, cx);
        });
    }

    /// Whether the app may quit now: a message being written is asked about first,
    /// in its window, and the user quits again after answering.
    pub(crate) fn may_quit(&mut self, cx: &mut Context<Self>) -> bool {
        !self
            .with_compose_window(cx, |view, window, cx| {
                let busy = view.has_edits();
                if busy {
                    window.activate_window();
                    view.discard_draft(window, cx);
                }
                busy
            })
            .unwrap_or(false)
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
        // The setup replaces the screen that shows the palette.
        if self.onboarding_active() {
            return;
        }
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
            CommandId::MessageDeletePermanent => {
                self.confirm_delete_forever(self.selected_ids(), window, cx)
            }
            CommandId::MessageRestore => self.restore_message(cx),
            CommandId::MessageArchive => self.archive_message(cx),
            CommandId::MessageMarkJunk => self.mark_message_junk(cx),
            CommandId::MessageToggleFlag => self.toggle_message_flag(cx),
            CommandId::MessagePrint => self.print_message(cx),
            CommandId::MoveToFolder(path) => self.move_message_to_folder(path.as_ref(), cx),
            CommandId::MessageToggleRead => self.toggle_message_read(cx),
            CommandId::SyncNow => self.sync_now(cx),
            CommandId::MessageReply => self.reply_to_selected(ReplyKind::Reply, window, cx),
            CommandId::MessageReplyAll => self.reply_to_selected(ReplyKind::ReplyAll, window, cx),
            CommandId::MessageForward => self.reply_to_selected(ReplyKind::Forward, window, cx),
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
        if let Some(id) = self.reader_detail().map(|detail| detail.id) {
            self.remote_allowed.insert(id);
            cx.notify();
        }
    }

    /// Sets "always load remote images", persisting it and re-rendering the
    /// reader.
    pub(crate) fn set_load_remote_images(&mut self, value: bool, cx: &mut Context<Self>) {
        if self.load_remote_images != value {
            self.load_remote_images = value;
            self.request_save(cx);
            cx.refresh_windows();
        }
    }

    /// Turns desktop notifications for new mail on or off (persisted).
    pub(crate) fn set_notifications_enabled(&mut self, value: bool, cx: &mut Context<Self>) {
        if self.notifications_enabled != value {
            self.notifications_enabled = value;
            self.request_save(cx);
            cx.notify();
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

    /// Raw source of the message an attachment belongs to (where its bytes
    /// live); reports a failure in the status bar when it is missing.
    fn attachment_raw(
        &mut self,
        attachment: &AttachmentRow,
        failure: Key,
        cx: &mut Context<Self>,
    ) -> Option<Vec<u8>> {
        let raw = self.db.message_raw(attachment.message_id).ok().flatten();
        if raw.is_none() {
            self.set_status_note(failure.tr(cx.language()), cx);
        }
        raw
    }

    /// "Open": decodes the attachment on a background thread, writes it to
    /// the private temp directory and opens it with the system's default app.
    pub(crate) fn open_attachment(&mut self, attachment: AttachmentRow, cx: &mut Context<Self>) {
        self.attachment_menu = None;
        if attachments::runs_code(&attachment.filename) {
            self.set_status_note(Key::StatusAttachmentRunsCode.tr(cx.language()), cx);
            return;
        }
        let Some(raw) = self.attachment_raw(&attachment, Key::StatusAttachmentOpenFailed, cx)
        else {
            return;
        };
        let task = cx.background_spawn(async move {
            let data = attachments::extract(&raw, attachment.part_index)?;
            attachments::write_for_open(&attachment.filename, &data)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| match result {
                Ok(path) => cx.open_with_system(&path),
                Err(error) => this.set_status_note(
                    format!(
                        "{}: {error}",
                        Key::StatusAttachmentOpenFailed.tr(cx.language())
                    ),
                    cx,
                ),
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// "Save As…": asks where to save (starting in Downloads, suggesting the
    /// sanitized name), then decodes and writes on a background thread.
    pub(crate) fn save_attachment_as(&mut self, attachment: AttachmentRow, cx: &mut Context<Self>) {
        self.attachment_menu = None;
        let Some(raw) = self.attachment_raw(&attachment, Key::StatusAttachmentFailed, cx) else {
            return;
        };
        let suggested = attachments::sanitize_filename(&attachment.filename);
        let prompt = cx.prompt_for_new_path(&attachments::downloads_dir(), Some(&suggested));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = prompt.await else {
                return;
            };
            let result = cx
                .background_spawn(async move {
                    let data = attachments::extract(&raw, attachment.part_index)?;
                    std::fs::write(&path, data).map(|()| path)
                })
                .await;
            this.update(cx, |this, cx| this.report_attachment_saved(result, cx))
                .ok();
        })
        .detach();
        cx.notify();
    }

    /// "Save to Downloads": writes the attachment into the Downloads folder
    /// under a free name (never overwriting) on a background thread.
    pub(crate) fn save_attachment_to_downloads(
        &mut self,
        attachment: AttachmentRow,
        cx: &mut Context<Self>,
    ) {
        self.attachment_menu = None;
        let Some(raw) = self.attachment_raw(&attachment, Key::StatusAttachmentFailed, cx) else {
            return;
        };
        let task = cx.background_spawn(async move {
            let data = attachments::extract(&raw, attachment.part_index)?;
            attachments::write_unique(
                &attachments::downloads_dir(),
                &attachment.filename,
                &data,
                false,
            )
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| this.report_attachment_saved(result, cx))
                .ok();
        })
        .detach();
        cx.notify();
    }

    /// Status bar note for a finished save: the saved path, or the error.
    fn report_attachment_saved(
        &mut self,
        result: std::io::Result<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let language = cx.language();
        let note = match result {
            Ok(path) => format!(
                "{}: {}",
                Key::StatusAttachmentSaved.tr(language),
                path.display()
            ),
            Err(error) => format!("{}: {error}", Key::StatusAttachmentFailed.tr(language)),
        };
        self.set_status_note(note, cx);
    }

    pub(crate) fn delete_message_to_trash(&mut self, cx: &mut Context<Self>) {
        // Trashing what is already in the Trash would delete it for good.
        let selection = std::mem::take(&mut self.multi_selection);
        self.multi_selection = selection
            .into_iter()
            .filter(|id| self.row_can_be_trashed(*id))
            .collect();
        self.run_message_action(cx, mail_actions::trash);
    }

    /// Delete button of a list row: acts on that row, leaving the selection
    /// alone. What is already in the Trash goes for good, once confirmed.
    fn delete_row_message(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        if self.row_can_be_trashed(id) {
            self.run_action_on_message(id, cx, mail_actions::trash);
        } else {
            self.confirm_delete_forever(vec![id], window, cx);
        }
    }

    fn row_can_be_trashed(&self, id: i64) -> bool {
        self.db.message_is_in_trash(id) == Ok(false)
    }

    /// Whether the list shows a Trash folder, whose rows offer no delete button.
    fn showing_sent(&self) -> bool {
        match &self.selected_mailbox {
            Selection::Global(mailbox) => *mailbox == GlobalMailbox::Sent,
            Selection::Mailbox { folder_path, .. } => folder_path == storage::system::SENT,
        }
    }

    fn showing_trash(&self) -> bool {
        matches!(
            &self.selected_mailbox,
            Selection::Mailbox { folder_path, .. } if folder_path == storage::system::TRASH
        )
    }

    /// Asks before deleting `ids` for good, then deletes the ones still in the
    /// Trash.
    fn confirm_delete_forever(
        &mut self,
        ids: Vec<i64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ids.is_empty() {
            return;
        }
        let language = cx.language();
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            &locale::delete_forever_confirm(language, ids.len()),
            None,
            &[
                Key::CommandDeletePermanent.tr(language),
                Key::AccountCancel.tr(language),
            ],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await.ok() != Some(0) {
                return;
            }
            this.update(cx, |this, cx| {
                let mut in_trash: Vec<i64> = ids
                    .into_iter()
                    .filter(|id| this.db.message_is_in_trash(*id) == Ok(true))
                    .collect();
                if let [id] = in_trash[..] {
                    this.run_action_on_message(id, cx, mail_actions::delete_permanently);
                } else if in_trash.len() > 1 {
                    this.multi_selection = std::mem::take(&mut in_trash);
                    this.run_message_action(cx, mail_actions::delete_permanently);
                }
            })
            .ok();
        })
        .detach();
    }

    /// The Delete command wherever it comes from (toolbar, key, menu): to the
    /// Trash, or for good, with confirmation, for what is already there.
    pub(crate) fn request_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ctx = self.command_context();
        if commands::command_enabled(&CommandId::MessageDelete, &ctx) {
            self.delete_message_to_trash(cx);
        } else if commands::command_enabled(&CommandId::MessageDeletePermanent, &ctx) {
            self.confirm_delete_forever(self.selected_ids(), window, cx);
        }
    }

    pub(crate) fn restore_message(&mut self, cx: &mut Context<Self>) {
        self.run_message_action(cx, mail_actions::restore_from_trash);
    }

    pub(crate) fn archive_message(&mut self, cx: &mut Context<Self>) {
        if !self.command_context().selection_movable() {
            return;
        }
        self.run_message_action(cx, mail_actions::archive);
    }

    pub(crate) fn mark_message_junk(&mut self, cx: &mut Context<Self>) {
        if !self.command_context().selection_movable() {
            return;
        }
        self.run_message_action(cx, mail_actions::junk);
    }

    pub(crate) fn toggle_message_flag(&mut self, cx: &mut Context<Self>) {
        if let Some(bulk) = self.bulk_selection() {
            let starred = !bulk.all_starred;
            self.apply_action_keeping_selection(
                move |db, id| mail_actions::set_starred(db, id, starred),
                cx,
            );
            return;
        }
        let Some(detail) = self.selected_detail.as_ref() else {
            return;
        };
        let starred = !detail.starred;
        self.run_message_action(cx, move |db, id| mail_actions::set_starred(db, id, starred));
    }

    pub(crate) fn move_message_to_folder(&mut self, folder_path: &str, cx: &mut Context<Self>) {
        if !self.command_context().selection_movable() {
            return;
        }
        let target = folder_path.to_string();
        self.run_message_action(cx, move |db, id| {
            mail_actions::move_to_folder(db, id, &target)
        });
    }

    /// The palette is modal: Tab must not move focus to the fields behind it.
    fn palette_traps_tab(&self) -> bool {
        self.command_palette.as_ref().is_some_and(|p| p.open)
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
            self.save_config();
        }
    }

    /// Best effort; there is nobody to tell.
    fn save_config(&self) {
        if let Some(path) = &self.config_file {
            let _ = config::save_to(path, &self.current_config());
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
            sidebar_width: f32::from(self.preferred_sidebar_width),
            list_width: f32::from(self.preferred_list_width),
            load_remote_images: self.load_remote_images,
            notifications_enabled: self.notifications_enabled,
            reader_white_background: self.reader_white_background,
            compose_white_background: self.compose_white_background,
            language: self.language.code().to_string(),
            ui_scale: self.ui_scale,
            light_theme: self.light_theme,
            onboarding_completed: self.onboarding_completed,
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
                    this.save_config();
                }
            });
        })
        .detach();
    }

    /// Lazily creates the scrollbar state entities (needs an app context, which
    /// is only available at render time).
    fn ensure_scrollbar_states(&mut self, cx: &mut Context<Self>) {
        for slot in [
            &mut self.list_scrollbar,
            &mut self.sidebar_scrollbar,
            &mut self.reader_v_scrollbar,
            &mut self.reader_h_scrollbar,
        ] {
            slot.get_or_insert_with(|| cx.new(|_| ScrollbarState::new()));
        }
    }

    /// Focus handle of the message list, created on first use.
    fn ensure_list_focus(&mut self, cx: &mut Context<Self>) -> FocusHandle {
        self.list_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone()
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
            self.list_scroll
                .0
                .borrow()
                .base_handle
                .set_offset(point(px(0.0), px(0.0)));
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
        self.list_scroll
            .scroll_to_item_strict(visible, ScrollStrategy::Top);
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
        let Some(detail) = self.reader_detail() else {
            self.last_reader_sig = None;
            self.reader_notice = None;
            self.reader_preparing = false;
            self.reader_build_token += 1;
            return;
        };
        let scale = if self.multi_selection.is_empty() {
            1.0
        } else {
            STACK_SCALE
        };
        if let Some(view) = &self.reader_view {
            view.update(cx, |view, cx| view.set_display_scale(scale, cx));
        }
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
            remote,
        };
        if (self.reader_view.is_some() || self.reader_preparing)
            && self.last_reader_sig == Some(signature)
        {
            return;
        }
        let job = ReaderJob {
            colors: doc_colors,
            body: db_seed::message_body_from_detail(detail),
            remote,
            crashed: self.crashed_messages.contains(&id),
            selection_color: reader_selection_color(colors.accent, background),
            width: f32::from(self.reader_segment_width()),
        };
        // A crash rendering formatted HTML must not repeat on every start:
        // mark the message before parsing it (parsing can crash too) until a
        // frame showing it was drawn.
        let guarded = !job.crashed && matches!(job.body, MessageBody::Html(_));
        if guarded {
            self.crash_guard.begin(id);
        }
        self.reader_build_token += 1;
        self.last_reader_sig = Some(signature);
        self.reader_menu = None;
        if !job.runs_in_background() {
            self.reader_preparing = false;
            self.install_reader(job.build(), guarded, window, cx);
            return;
        }
        self.reader_preparing = true;
        self.reader_notice = None;
        self.content_blocked_count = 0;
        let token = self.reader_build_token;
        let built = cx.background_spawn(async move { job.build() });
        cx.spawn_in(window, async move |this, cx| {
            let built = built.await;
            this.update_in(cx, |this, window, cx| {
                // The user may have moved on while this one was being built.
                if this.reader_build_token == token {
                    this.reader_preparing = false;
                    this.install_reader(built, guarded, window, cx);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Hands a built document to the reader view.
    fn install_reader(
        &mut self,
        built: BuiltReader,
        guarded: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.content_blocked_count = built.blocked_images;
        self.reader_notice = built.notice;
        if guarded {
            let guard = self.crash_guard.clone();
            window.on_next_frame(move |_, _| guard.finish());
        }
        let document = built.document.into_inner();
        match &self.reader_view {
            Some(view) => view.update(cx, |view, cx| view.set_document(document, cx)),
            None => {
                let view = cx.new(|cx| HtmlView::new(document, cx));
                self._reader_subscription = Some(cx.subscribe(&view, Self::on_reader_event));
                self.reader_view = Some(view);
            }
        }
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
        if self.reader_detail().is_none() {
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
    pub(crate) fn set_reader_white_background(&mut self, value: bool, cx: &mut Context<Self>) {
        if self.reader_white_background != value {
            self.reader_white_background = value;
            self.request_save(cx);
            cx.refresh_windows();
        }
    }

    /// Toggles whether the compose body forces a white background, persisting
    /// the preference and updating an open compose window.
    /// Steps the zoom of the whole app (`0` = back to normal), in every window, and
    /// remembers it. `window` is the main one, whose handle is busy with this update.
    pub(crate) fn zoom_app(&mut self, steps: i32, window: &mut Window, cx: &mut Context<Self>) {
        self.ui_scale = if steps == 0 {
            1.0
        } else {
            crate::app_zoom::step(self.ui_scale, steps)
        };
        let scale = self.ui_scale;
        window.set_content_scale(scale, cx);
        if let Some(handle) = self.settings_window {
            let _ = handle.update(cx, |_, window, cx| window.set_content_scale(scale, cx));
        }
        if let Some(handle) = self.compose_window {
            let _ = handle.update(cx, |_, window, cx| window.set_content_scale(scale, cx));
        }
        self.set_status_note(locale::status_zoom(cx.language(), scale), cx);
        self.request_save(cx);
        cx.notify();
    }

    /// Switches the UI language and remembers it for the next launch.
    pub(crate) fn set_language(&mut self, language: Language, cx: &mut Context<Self>) {
        self.language = language;
        locale::set_language(language, cx);
        self.sync_app_menus(cx);
        self.request_save(cx);
        cx.notify();
    }

    /// Switches the app theme and remembers it for the next launch.
    pub(crate) fn set_appearance(&mut self, appearance: theme::Appearance, cx: &mut Context<Self>) {
        self.light_theme = appearance.is_light();
        theme::set_theme(theme::Theme::for_appearance(appearance), cx);
        self.request_save(cx);
        cx.notify();
    }

    /// Whether the first-run setup is on screen.
    pub(crate) fn onboarding_active(&self) -> bool {
        !self.onboarding_completed
    }

    /// Leaves the first-run setup for good and shows the mail.
    pub(crate) fn finish_onboarding(&mut self, cx: &mut Context<Self>) {
        self.onboarding_completed = true;
        self.onboarding = None;
        self.persist_now();
        cx.notify();
    }

    /// The first-run setup, under a title bar that keeps the window movable and
    /// its controls reachable (the toolbar that normally does this isn't shown).
    fn render_onboarding(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let root = cx.entity();
        let view = self
            .onboarding
            .get_or_insert_with(|| cx.new(|cx| crate::onboarding::OnboardingView::new(root, cx)))
            .clone();
        let colors = *cx.theme().colors();
        let title_bar = Self::window_drag_bar(h_flex().h(px(52.0)).w_full().flex_shrink_0(), cx)
            .pl(crate::window_frame::toolbar_left_padding())
            .children(crate::window_frame::render_left_window_controls(window))
            .child(div().flex_1())
            .children(crate::window_frame::render_right_window_controls(window));
        let content = v_flex()
            .size_full()
            .bg(crate::onboarding::page_background(&colors))
            .text_color(colors.text)
            .font_family("Helvetica")
            .child(title_bar)
            .child(view)
            .into_any_element();
        crate::window_frame::wrap_client_decorations(content, window, colors.border)
    }

    pub(crate) fn set_compose_white_background(&mut self, value: bool, cx: &mut Context<Self>) {
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
    pub(crate) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_settings_section(crate::settings::SettingsSection::General, window, cx);
    }

    /// Opens (or focuses) the settings window on `section`.
    pub(crate) fn open_settings_section(
        &mut self,
        section: crate::settings::SettingsSection,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(handle) = self.settings_window {
            if handle
                .update(cx, |view, window, cx| {
                    view.show_section(section, cx);
                    window.activate_window()
                })
                .is_ok()
            {
                return;
            }
            // The window was closed since we last opened it; reopen a fresh one.
            self.settings_window = None;
        }

        let accounts = ui_accounts(&self.accounts);
        let root = cx.entity().downgrade();
        let ui_scale = self.ui_scale;
        let bounds = Bounds::centered(None, size(px(760.0), px(560.0)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(520.0), px(420.0))),
            titlebar: Some(TitlebarOptions {
                title: Some(Key::SettingsTitle.tr(cx.language()).into()),
                appears_transparent: false,
                traffic_light_position: None,
            }),
            app_id: Some(SETTINGS_WINDOW_APP_ID.into()),
            ..Default::default()
        };

        // Opened after this update returns: the settings view reads `RootView`
        // while rendering, and GPUI renders a new window synchronously, which
        // would otherwise re-enter the entity being updated (panic).
        cx.defer(move |cx| {
            let root_for_close = root.clone();
            let root_for_handle = root.clone();
            let Ok(handle) = cx.open_window(options, |window, cx| {
                window.set_content_scale(ui_scale, cx);
                let view = cx.new(|_| {
                    let mut view = SettingsView::new(accounts, root);
                    view.section = section;
                    view
                });
                window.on_window_should_close(cx, move |_, cx| {
                    if let Some(root) = root_for_close.upgrade() {
                        root.update(cx, |root, cx| {
                            root.on_settings_window_closed(cx);
                        });
                    }
                    true
                });
                view
            }) else {
                return;
            };
            let _ = handle.update(cx, |_, window, _| window.activate_window());
            if let Some(root) = root_for_handle.upgrade() {
                root.update(cx, |root, cx| {
                    root.settings_window = Some(handle);
                    root.sync_app_menus(cx);
                    cx.notify();
                });
            }
        });
    }

    /// Opens an empty compose window, or focuses it if it is already open.
    pub(crate) fn open_compose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_compose_with(None, window, cx);
    }

    /// Accounts offered in the compose "From" picker (sending ones first).
    pub(crate) fn compose_accounts(&self) -> Vec<ComposeAccount> {
        let mut accounts: Vec<ComposeAccount> = self
            .accounts
            .iter()
            .map(|account| ComposeAccount {
                id: account.id,
                name: account.from_name().to_string(),
                email: account.email.clone(),
                can_send: account.provider != storage::PROVIDER_LOCAL,
            })
            .collect();
        accounts.sort_by_key(|account| !account.can_send);
        accounts
    }

    /// Opens a reply / reply all / forward of the selected message.
    pub(crate) fn reply_to_selected(
        &mut self,
        kind: ReplyKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(detail) = self.selected_detail.clone() else {
            return;
        };
        let raw = self.db.message_raw(detail.id).ok().flatten();
        let parsed = raw
            .as_deref()
            .and_then(mail_core::parse_message)
            .unwrap_or_else(|| parsed_from_detail(&detail));
        let me: Vec<Address> = self
            .accounts
            .iter()
            .filter(|account| account.id == detail.account_id)
            .map(|account| Address::new(Some(account.from_name()), account.email.clone()))
            .collect();
        let language = cx.language();
        let date = dates::full_date(detail.date_ts, &detail.received_at, language);
        let sender = Address::new(
            (!detail.sender.is_empty()).then(|| detail.sender.clone()),
            detail.sender_email.clone(),
        )
        .display();
        let quote_date = dates::quote_date(detail.date_ts, &detail.received_at, language);
        let labels = QuoteLabels {
            attribution: locale::reply_attribution(language, &quote_date, &sender),
            forward_header: Key::ForwardHeader.tr(language).to_string(),
            from: Key::QuoteFrom.tr(language).to_string(),
            date: Key::QuoteDate.tr(language).to_string(),
            subject: Key::QuoteSubject.tr(language).to_string(),
            to: Key::QuoteTo.tr(language).to_string(),
            formatted_date: date,
        };
        let mut draft = mail_core::prepare_reply(&parsed, kind, &me, &labels);
        // As in Gmail, the editor only holds what the user writes; the quote
        // shows below it and is added when sending.
        let quoted_html = parsed
            .html
            .as_deref()
            .map(quotable_html)
            .or_else(|| parsed.text.as_deref().map(quotable_text))
            .filter(|html| kind != ReplyKind::Forward && !html.trim().is_empty());
        let quoted_original = quoted_html.map(|html| QuotedOriginal {
            block: std::mem::take(&mut draft.text).trim_start().to_string(),
            attribution: labels.attribution.clone(),
            html,
            load_remote: self.remote_allowed_for(detail.id),
        });
        if kind == ReplyKind::Forward {
            if let Some(raw) = raw.as_deref() {
                draft.attachments = forwarded_attachments(raw, &parsed);
            }
        }
        let init = ComposeInit {
            account_id: Some(detail.account_id),
            draft,
            replaces_outgoing: None,
            quoted_original,
        };
        self.open_compose_with(Some(init), window, cx);
    }

    /// Queues `draft` for sending from `account_id` and wakes its worker;
    /// `replaces_outgoing` (an edited unsent message) is deleted once the new
    /// copy is queued. Errors are localized, ready for the compose window.
    pub(crate) fn send_draft(
        &mut self,
        account_id: i64,
        draft: Draft,
        replaces_outgoing: Option<i64>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let language = cx.language();
        let account = self
            .accounts
            .iter()
            .find(|account| account.id == account_id)
            .ok_or_else(|| Key::ComposeLocalAccount.tr(language).to_string())?;
        if account.provider == storage::PROVIDER_LOCAL {
            return Err(Key::ComposeLocalAccount.tr(language).to_string());
        }
        let recipients = mail_core::envelope_recipients(&draft);
        if recipients.is_empty() {
            return Err(Key::ComposeNoRecipients.tr(language).to_string());
        }
        // Whoever mail goes to joins the catalog, by address: their name comes when
        // they write back.
        let contacts: Vec<_> = recipients
            .iter()
            .map(|email| (email.clone(), None))
            .collect();
        if let Err(error) = self.db.record_contacts(&contacts) {
            eprintln!("BGMail: could not record contacts: {error}");
        }
        let message_id = mail_core::new_message_id(&draft.from.email, unique_token());
        let raw = mail_core::build_message(&draft, &message_id, dates::now_unix());
        match replaces_outgoing {
            // An edited unsent message: its original goes in the same step.
            Some(id) => {
                let replaced = self
                    .db
                    .replace_outgoing(
                        id,
                        account_id,
                        &draft.from.email,
                        &recipients,
                        &raw,
                        Some(&message_id),
                    )
                    .map_err(|error| error.to_string())?;
                if replaced.is_none() {
                    return Err(Key::ComposeOriginalSending.tr(language).to_string());
                }
                self.refresh_unsent();
            }
            None => {
                self.db
                    .enqueue_outgoing(
                        account_id,
                        &draft.from.email,
                        &recipients,
                        &raw,
                        Some(&message_id),
                    )
                    .map_err(|error| error.to_string())?;
            }
        }
        if let Some(mail) = &self.mail {
            mail.process_queue(account_id);
        }
        self.set_status_note(Key::StatusSyncing.tr(language), cx);
        Ok(())
    }

    /// Opens the compose window (optionally prefilled), or focuses it if it
    /// is already open.
    /// Starts a message from a `mailto:` link, from the first account that can
    /// send. A message being written is never replaced by one from outside.
    pub(crate) fn open_mailto(&mut self, uri: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = mail_core::draft_from_mailto(uri) else {
            return;
        };
        let init = ComposeInit {
            draft,
            ..Default::default()
        };
        self.open_compose_with(Some(init), window, cx);
    }

    pub(crate) fn open_compose_with(
        &mut self,
        init: Option<ComposeInit>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let contacts = self.db.list_contacts().unwrap_or_default();
        if let Some(handle) = self.compose_window {
            let init_for_open = init.clone();
            let contacts_for_open = contacts.clone();
            match handle.update(cx, |view, window, cx| {
                view.set_contacts(contacts_for_open, cx);
                window.activate_window();
                // A message being written is never replaced by another.
                let busy = init_for_open.is_some() && view.has_edits();
                if !busy && init_for_open.is_some() {
                    view.apply_init(init_for_open, window, cx);
                }
                busy
            }) {
                Ok(true) => {
                    self.set_status_note(Key::StatusComposeBusy.tr(cx.language()), cx);
                    return;
                }
                Ok(false) => return,
                Err(_) => self.compose_window = None,
            }
        }

        let accounts = self.compose_accounts();
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
            app_id: Some(COMPOSE_WINDOW_APP_ID.into()),
            ..Default::default()
        };

        let ui_scale = self.ui_scale;
        if let Ok(handle) = cx.open_window(options, |window, cx| {
            window.set_content_scale(ui_scale, cx);
            let root_for_close = root.clone();
            let view = cx.new(|cx| {
                ComposeView::new(accounts, contacts, root, compose_white, init, window, cx)
            });
            let compose = view.downgrade();
            window.on_window_should_close(cx, move |window, cx| {
                if let Some(compose) = compose.upgrade().filter(|c| c.read(cx).has_edits()) {
                    compose.update(cx, |view, cx| view.discard_draft(window, cx));
                    return false;
                }
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
        } else {
            self.remember_selection();
        }
        self.clear_message_selection();
        self.apply_mailbox_selection(selection);
        self.reload_message_list();
        self.restore_selection();
        self.sync_app_menus(cx);
        cx.notify();
    }

    /// Notes what the current mailbox has selected, before leaving it.
    fn remember_selection(&mut self) {
        let ids = self.selected_ids();
        let mailbox = self.selected_mailbox.clone();
        if ids.is_empty() {
            self.remembered_selections.remove(&mailbox);
        } else {
            self.remembered_selections.insert(
                mailbox,
                RememberedSelection {
                    ids,
                    anchor: self.selection_anchor,
                    focus: self.selection_focus,
                },
            );
        }
    }

    /// Selects again what the current mailbox had when the user left it, as far as
    /// those rows are still listed. Returning isn't opening: a message the user marked
    /// unread stays so, as it would have had they never left.
    fn restore_selection(&mut self) {
        let Some(saved) = self.remembered_selections.get(&self.selected_mailbox) else {
            return;
        };
        let listed: HashSet<i64> = self.list_messages.iter().map(|m| m.id).collect();
        let ids: Vec<i64> = saved
            .ids
            .iter()
            .copied()
            .filter(|id| listed.contains(id))
            .collect();
        let (anchor, focus) = (saved.anchor, saved.focus);
        let Some(&first) = ids.first() else {
            return;
        };
        let still_selected = |id: Option<i64>| id.filter(|id| ids.contains(id)).or(Some(first));
        let (anchor, focus) = (still_selected(anchor), still_selected(focus));
        self.selection_anchor = anchor;
        self.selection_focus = focus;
        if self.apply_selection(ids) {
            self.reload_selected_detail();
            self.request_selected_body();
        }
        self.reveal_selected_message();
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
                        "get-mail",
                        Key::CommandSyncNow.tr(language),
                        Some((shortcuts::GET_MAIL_MAC, shortcuts::GET_MAIL_OTHER)),
                        IconButton::new("get-mail", IconName::Refresh).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.sync_now(cx);
                            },
                        )),
                    ))
                    .child(Self::icon_button_with_tooltip(
                        "filter",
                        Key::ToolbarFilter.tr(language),
                        None,
                        IconButton::new("filter", IconName::Filter)
                            .selected(self.list_filter != ListFilter::All)
                            .on_click(cx.listener(|this, event: &gpui::ClickEvent, _, cx| {
                                this.filter_menu = Some(event.position());
                                cx.notify();
                            })),
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

        // The sidebar yields only what the list's minimum can't: a window or zoom that
        // has room again gets the widths the user chose back.
        let sidebar = if self.show_sidebar && !self.narrow {
            let sidebar_max =
                (total - px(LIST_MIN_WIDTH) - px(READER_MIN_WIDTH)).max(px(SIDEBAR_MIN_WIDTH));
            self.sidebar_width = self
                .preferred_sidebar_width
                .clamp(px(SIDEBAR_MIN_WIDTH), sidebar_max);
            self.sidebar_width
        } else {
            px(0.0)
        };

        // Keep the list within [min, available-for-reader].
        let list_max = (total - sidebar - px(READER_MIN_WIDTH)).max(px(LIST_MIN_WIDTH));
        self.list_width = self
            .preferred_list_width
            .clamp(px(LIST_MIN_WIDTH), list_max);
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
                self.preferred_sidebar_width = self.sidebar_width;
            }
            ResizeHandle::List => {
                let left = if self.show_sidebar && !self.narrow {
                    self.sidebar_width
                } else {
                    px(0.0)
                };
                let max = (total - left - px(READER_MIN_WIDTH)).max(px(LIST_MIN_WIDTH));
                self.list_width = (x - left).clamp(px(LIST_MIN_WIDTH), max);
                self.preferred_list_width = self.list_width;
            }
        }
    }

    // ----- Top toolbar (unified title bar) --------------------------------

    /// Makes `bar` act as the window's title bar.
    fn window_drag_bar(bar: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        bar
            // We arm the drag on
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
    }

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

        let bar = h_flex()
            .h(px(52.0))
            .w_full()
            .flex_shrink_0()
            .bg(bg)
            .border_b_1()
            .border_color(border);
        Self::window_drag_bar(bar, cx)
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
                        let ctx = self.command_context();
                        let enabled = |id: CommandId| commands::command_enabled(&id, &ctx);
                        let read_label = if ctx.message_unread() {
                            Key::CommandMarkRead.tr(language)
                        } else {
                            Key::CommandMarkUnread.tr(language)
                        };
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
                                                Some((
                                                    shortcuts::REPLY_MAC,
                                                    shortcuts::REPLY_OTHER,
                                                )),
                                                IconButton::new("reply", IconName::Reply)
                                                    .disabled(!enabled(CommandId::MessageReply))
                                                    .on_click(cx.listener(
                                                        |this, _, window, cx| {
                                                            this.reply_to_selected(
                                                                ReplyKind::Reply,
                                                                window,
                                                                cx,
                                                            );
                                                        },
                                                    )),
                                            ))
                                            .child(Self::icon_button_with_tooltip(
                                                "reply-all",
                                                Key::ToolbarReplyAll.tr(language),
                                                Some((
                                                    shortcuts::REPLY_ALL_MAC,
                                                    shortcuts::REPLY_ALL_OTHER,
                                                )),
                                                IconButton::new("reply-all", IconName::ReplyAll)
                                                    .disabled(!enabled(CommandId::MessageReplyAll))
                                                    .on_click(cx.listener(
                                                        |this, _, window, cx| {
                                                            this.reply_to_selected(
                                                                ReplyKind::ReplyAll,
                                                                window,
                                                                cx,
                                                            );
                                                        },
                                                    )),
                                            ))
                                            .child(Self::icon_button_with_tooltip(
                                                "forward",
                                                Key::ToolbarForward.tr(language),
                                                Some((
                                                    shortcuts::FORWARD_MAC,
                                                    shortcuts::FORWARD_OTHER,
                                                )),
                                                IconButton::new("forward", IconName::Forward)
                                                    .disabled(!enabled(CommandId::MessageForward))
                                                    .on_click(cx.listener(
                                                        |this, _, window, cx| {
                                                            this.reply_to_selected(
                                                                ReplyKind::Forward,
                                                                window,
                                                                cx,
                                                            );
                                                        },
                                                    )),
                                            )),
                                    )
                                })
                                .when(groups >= 2, |cluster| {
                                    cluster.child(Self::render_toolbar_separator(border)).child(
                                        h_flex()
                                            .gap_1()
                                            .child(Self::icon_button_with_tooltip(
                                                "trash",
                                                if enabled(CommandId::MessageDelete) {
                                                    Key::CommandDelete.tr(language)
                                                } else {
                                                    Key::CommandDeletePermanent.tr(language)
                                                },
                                                Some((
                                                    shortcuts::DELETE_MESSAGE_MAC,
                                                    shortcuts::DELETE_MESSAGE_OTHER,
                                                )),
                                                IconButton::new("trash", IconName::Trash)
                                                    .disabled(
                                                        !enabled(CommandId::MessageDelete)
                                                            && !enabled(
                                                                CommandId::MessageDeletePermanent,
                                                            ),
                                                    )
                                                    .on_click(cx.listener(
                                                        |this, _, window, cx| {
                                                            this.request_delete(window, cx);
                                                        },
                                                    )),
                                            ))
                                            .child(Self::icon_button_with_tooltip(
                                                "archive",
                                                Key::CommandArchive.tr(language),
                                                Some((
                                                    shortcuts::ARCHIVE_MAC,
                                                    shortcuts::ARCHIVE_OTHER,
                                                )),
                                                IconButton::new("archive", IconName::Archive)
                                                    .disabled(!enabled(CommandId::MessageArchive))
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
                                                IconButton::new("junk", IconName::Junk)
                                                    .disabled(!enabled(CommandId::MessageMarkJunk))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.mark_message_junk(cx);
                                                    })),
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
                                                IconButton::new(
                                                    "flag",
                                                    star_icon(ctx.message_starred()),
                                                )
                                                .color(star_color(ctx.message_starred()))
                                                .disabled(!enabled(CommandId::MessageToggleFlag))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.toggle_message_flag(cx);
                                                })),
                                            ))
                                            .child(Self::icon_button_with_tooltip(
                                                "toggle-read",
                                                read_label,
                                                Some((
                                                    shortcuts::TOGGLE_READ_MAC,
                                                    shortcuts::TOGGLE_READ_OTHER,
                                                )),
                                                IconButton::new("toggle-read", IconName::Mail)
                                                    .disabled(!enabled(
                                                        CommandId::MessageToggleRead,
                                                    ))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.toggle_message_read(cx);
                                                    })),
                                            ))
                                            .child(Self::icon_button_with_tooltip(
                                                "move",
                                                Key::ToolbarMove.tr(language),
                                                None,
                                                IconButton::new("move", IconName::Folder)
                                                    .disabled(!enabled(CommandId::MoveToFolder(
                                                        SharedString::default(),
                                                    )))
                                                    .on_click(cx.listener(
                                                        |this, event: &gpui::ClickEvent, _, cx| {
                                                            this.move_menu = Some(event.position());
                                                            cx.notify();
                                                        },
                                                    )),
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
            let unread = if folder_shows_unread(&folder.path) {
                self.db
                    .unread_in_folder(Some(account.id), &folder.path)
                    .unwrap_or(0)
            } else {
                0
            };
            let badge = if unread > 0 {
                Some(self.render_count_badge(unread, selected, cx))
            } else {
                None
            };
            let folder_path = folder.path.clone();
            let account_id = account.id;

            // Keyed by account id + row so accounts with 100+ folders never
            // share element ids.
            let mut item =
                ListItem::new(gpui::ElementId::NamedInteger(
                    format!("mailbox-{}", account.id).into(),
                    mailbox_idx as u64,
                ))
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
            let drop_highlight = cx.theme().colors().element_selected;
            let target_path = folder.path.clone();
            let drop_path = folder.path.clone();
            let accepts = move |dragged: &dyn std::any::Any, window: &mut Window, _: &mut App| {
                dragged
                    .downcast_ref::<DraggedMessages>()
                    .is_some_and(|dragged| {
                        dragged.can_drop_on(account_id, &drop_path, window.mouse_position())
                    })
            };
            // The whole row takes the drop; the highlight goes on the item itself, to
            // look like its selected state: same corners, without the row's indent.
            list = list.child(
                div()
                    .group(FOLDER_ROW_GROUP)
                    .h(px(SIDEBAR_ROW_HEIGHT))
                    .flex_shrink_0()
                    .pl(px(ITEM_INDENT))
                    .can_drop(accepts.clone())
                    .on_drop(cx.listener(move |this, dragged: &DraggedMessages, _, cx| {
                        this.move_messages_to_folder(&dragged.ids, &target_path, cx);
                    }))
                    .child(
                        div()
                            .rounded_md()
                            // GPUI only applies drag styles to elements with a hitbox,
                            // and an empty hover style is what gives this one its own.
                            .group_hover(FOLDER_ROW_GROUP, |style| style)
                            .can_drop(accepts)
                            .group_drag_over::<DraggedMessages>(FOLDER_ROW_GROUP, move |style| {
                                style.bg(drop_highlight)
                            })
                            .child(item),
                    ),
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
        let list_focus = self.ensure_list_focus(cx);

        // Virtualized: only the visible rows are built, so mailboxes with
        // thousands of messages stay cheap. Rows have a uniform height
        // (measured from the first one).
        let no_results = if self.list_filter != ListFilter::All {
            Key::FilterNoResults
        } else {
            Key::SearchNoResults
        };
        let filtering = self.list_filter != ListFilter::All || self.search_is_active(cx);
        let content = if self.list_messages.is_empty() && filtering {
            div()
                .id("message-list")
                .size_full()
                .child(
                    div().w_full().py_8().flex().justify_center().child(
                        Label::new(no_results.tr(language))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    ),
                )
                .into_any_element()
        } else {
            let list = uniform_list(
                "message-list",
                self.list_messages.len(),
                cx.processor(|this, range: std::ops::Range<usize>, _window, cx| {
                    let end = range.end.min(this.list_messages.len());
                    let start = range.start.min(end);
                    let this = &*this;
                    this.list_messages[start..end]
                        .iter()
                        .map(|message| this.render_message_row(message, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .size_full()
            .track_scroll(self.list_scroll.clone())
            .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                Self::note_scroll([this.list_scrollbar.clone()], cx);
            }));
            // A click below the last row (rows swallow their mouse-down)
            // clears the selection.
            div()
                .id("message-list-area")
                .size_full()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.clear_message_selection();
                    this.sync_app_menus(cx);
                    cx.notify();
                }))
                .child(list)
                .into_any_element()
        };

        let scroll_base = self.list_scroll.0.borrow().base_handle.clone();
        let scrollbar = self
            .list_scrollbar
            .clone()
            .map(|state| Scrollbar::vertical(state, scroll_base));

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
            .track_focus(&list_focus)
            .key_context(shortcuts::MESSAGE_LIST_CONTEXT)
            .on_action(cx.listener(|this, _: &SelectNextMessage, _, cx| {
                this.move_selection(1, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectPreviousMessage, _, cx| {
                this.move_selection(-1, cx);
            }))
            .on_action(cx.listener(|this, _: &ExtendSelectionDown, _, cx| {
                this.extend_selection(1, cx);
            }))
            .on_action(cx.listener(|this, _: &ExtendSelectionUp, _, cx| {
                this.extend_selection(-1, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectAllMessages, _, cx| {
                this.set_selection(this.list_ids(), cx);
            }))
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
        let selected = self.row_is_selected(message.id);
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
            Label::new(dates::list_date(
                message.date_ts,
                &message.received_at,
                cx.language(),
            ))
            .size(LabelSize::XSmall)
            .color(Color::Muted),
        );

        let message_id = message.id;
        // The subject, on the line the action buttons sit on, gives way to them.
        // The sender/date line above and the preview below never move.
        let room_for_actions = row_actions_room(self.hovered_row == Some(message_id));
        h_flex()
            .id(("message", message_id as u64))
            .group(MESSAGE_ROW_GROUP)
            .relative()
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
            .when_some(self.dragged_messages(message_id), |row, dragged| {
                row.on_drag(dragged, {
                    let width = self.list_width;
                    let title: SharedString = message.sender.clone().into();
                    let subject: SharedString = message.subject.clone().into();
                    let language = cx.language();
                    move |dragged: &DraggedMessages, grab, window, cx| {
                        dragged.progress.start(window.mouse_position());
                        let (title, subtitle) = if dragged.ids.len() > 1 {
                            (
                                locale::messages_selected(language, dragged.ids.len()).into(),
                                SharedString::default(),
                            )
                        } else {
                            (title.clone(), subject.clone())
                        };
                        let progress = dragged.progress.clone();
                        cx.new(|_| DragPreview {
                            progress,
                            grab,
                            draws_hand: DRAG_DRAWS_HAND,
                            width,
                            title,
                            subtitle,
                        })
                    }
                })
            })
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                let row = hovered.then_some(message_id);
                if *hovered || this.hovered_row == Some(message_id) {
                    this.hovered_row = row;
                    cx.notify();
                }
            }))
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
                        h_flex().mr(room_for_actions).child(
                            div().flex_1().min_w_0().child(
                                Label::new(message.subject.clone())
                                    .size(LabelSize::Small)
                                    .single_line(),
                            ),
                        ),
                    )
                    .child(
                        Label::new(message.preview.clone())
                            .size(LabelSize::Small)
                            .color(Color::Muted)
                            .single_line(),
                    ),
            )
            .child(self.render_message_row_actions(message_id, cx))
            .on_click(
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    this.click_message_row(message_id, event.modifiers(), window, cx);
                }),
            )
    }

    /// Selects and opens a row of the list.
    fn select_message_row(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(focus) = &self.list_focus {
            window.focus(focus);
        }
        self.select_only(id);
        self.on_message_selection_changed(cx);
    }

    /// Reply and Delete, centered at the right of a row while the mouse is on it.
    fn render_message_row_actions(
        &self,
        message_id: i64,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let language = cx.language();
        h_flex()
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            // Lined up with the date's right edge (the row's padding).
            .pr_3()
            .gap(px(ROW_ACTION_GAP))
            .items_center()
            .invisible()
            .group_hover(MESSAGE_ROW_GROUP, |style| style.visible())
            .child(Self::icon_button_with_tooltip(
                "row-reply",
                Key::ToolbarReply.tr(language),
                None,
                IconButton::new("row-reply-button", IconName::Reply)
                    .compact()
                    .size(IconSize::Small)
                    .color(Color::Muted)
                    .hover_color(Color::Default)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        // Replying opens the message first, so its body gets
                        // fetched and the reader shows what is being answered.
                        this.select_message_row(message_id, window, cx);
                        this.reply_to_selected(ReplyKind::Reply, window, cx);
                    })),
            ))
            .child(Self::icon_button_with_tooltip(
                "row-delete",
                if self.showing_trash() {
                    Key::CommandDeletePermanent.tr(language)
                } else {
                    Key::CommandDelete.tr(language)
                },
                None,
                IconButton::new("row-delete-button", IconName::Trash)
                    .compact()
                    .size(IconSize::Small)
                    .color(Color::Muted)
                    .hover_color(Color::Default)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.delete_row_message(message_id, window, cx)
                    })),
            ))
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
        let language = cx.language();
        // The bar can stay open across a change of language.
        bar.input.update(cx, |input, _| {
            input.set_placeholder(Key::FindPlaceholder.tr(language));
        });
        let colors = cx.theme().colors();
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

    /// "To" / "Cc" lines under the sender (stored display-formatted).
    /// Who a message went to, as in Gmail: first names on one line, with everything
    /// on hover and all the details behind a chevron.
    fn render_recipients(
        &self,
        message: &MessageDetail,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let language = cx.language();
        let to = mail_core::parse_address_list(&message.to_addrs).unwrap_or_default();
        let cc = mail_core::parse_address_list(&message.cc_addrs).unwrap_or_default();
        let own_email = self
            .accounts
            .iter()
            .find(|account| account.id == message.account_id)
            .map_or("", |account| account.email.as_str());
        let me = Key::ReaderMe.tr(language);
        let everyone = unique_recipients(to.iter().chain(&cc));
        let count = everyone.len();
        let names = everyone.into_iter().enumerate().map(|(ix, address)| {
            let separator = if ix + 1 < count { "," } else { "" };
            let label = format!("{}{separator}", short_recipient(address, own_email, me));
            div()
                .id(("reader-recipient", ix))
                .cursor_default()
                .tooltip(Tooltip::text(address.display()))
                .child(
                    Label::new(label)
                        .size(LabelSize::Small)
                        .color(Color::Muted)
                        .single_line(),
                )
        });
        let id = message.id;
        let expanded = self.expanded_recipients == Some(id);
        // A header that doesn't parse is still shown as it came.
        let raw = (count == 0)
            .then(|| {
                [message.to_addrs.as_str(), message.cc_addrs.as_str()]
                    .into_iter()
                    .filter(|value| !value.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|raw| !raw.is_empty());
        if count == 0 && raw.is_none() {
            return None;
        }
        let detail = |key: Key, list: &[Address]| {
            (!list.is_empty()).then(|| {
                Label::new(format!(
                    "{}: {}",
                    key.tr(language),
                    mail_core::format_address_list(list)
                ))
                .size(LabelSize::Small)
                .color(Color::Muted)
            })
        };
        Some(
            v_flex()
                .pl(px(40.0))
                .gap_1()
                .child(
                    h_flex()
                        .min_w_0()
                        .flex_wrap()
                        .gap_1()
                        .child(
                            Label::new(Key::ReaderSentTo.tr(language))
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        )
                        .children(names.collect::<Vec<_>>())
                        .children(raw.map(|raw| {
                            Label::new(raw)
                                .size(LabelSize::Small)
                                .color(Color::Muted)
                                .single_line()
                        }))
                        .when(count > 0, |el| {
                            el.child(
                                h_flex()
                                    .id("reader-recipients-toggle")
                                    .cursor_pointer()
                                    .tooltip(Tooltip::text(Key::ReaderShowDetails.tr(language)))
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.expanded_recipients =
                                            (this.expanded_recipients != Some(id)).then_some(id);
                                        cx.notify();
                                    }))
                                    .child(
                                        Icon::new(if expanded {
                                            IconName::ChevronUp
                                        } else {
                                            IconName::ChevronDown
                                        })
                                        .size(IconSize::XSmall)
                                        .color(Color::Muted),
                                    ),
                            )
                        }),
                )
                .when(expanded, |el| {
                    el.children(detail(Key::ReaderTo, &to))
                        .children(detail(Key::ReaderCc, &cc))
                }),
        )
    }

    /// Wrapping row of attachment chips under the reader header: icon by
    /// file family, file name and size. Clicking a chip opens its menu.
    fn render_attachments(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if self.attachments.is_empty() {
            return None;
        }
        let colors = cx.theme().colors();
        let (chip_bg, chip_border, hover) = (
            colors.element_background,
            colors.border_variant,
            colors.element_hover,
        );
        let language = cx.language();
        Some(h_flex().pl(px(40.0)).flex_wrap().gap_2().children(
            self.attachments.iter().enumerate().map(|(ix, attachment)| {
                let family = attachments::file_family(&attachment.mime_type, &attachment.filename);
                let name = if attachment.filename.trim().is_empty() {
                    attachments::sanitize_filename("")
                } else {
                    attachment.filename.clone()
                };
                let menu_attachment = attachment.clone();
                h_flex()
                    .id(("attachment", ix))
                    .max_w(px(280.0))
                    .h(px(28.0))
                    .px_2()
                    .gap_1p5()
                    .items_center()
                    .rounded_md()
                    .border_1()
                    .border_color(chip_border)
                    .bg(chip_bg)
                    .hover(move |el| el.bg(hover))
                    .cursor_pointer()
                    .child(
                        Icon::new(family.icon())
                            .size(IconSize::Small)
                            .color(Color::Muted),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_shrink()
                            .overflow_hidden()
                            .child(Label::new(name).size(LabelSize::Small).single_line()),
                    )
                    .child(
                        div().flex_shrink_0().child(
                            Label::new(attachments::human_size(attachment.size, language))
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                        ),
                    )
                    .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                        this.attachment_menu = Some(AttachmentMenu {
                            position: event.position(),
                            attachment: menu_attachment.clone(),
                        });
                        cx.notify();
                    }))
            }),
        ))
    }

    /// The attachment chip menu: Open, Save As…, Save to Downloads. Anchored
    /// at the click; any outside click closes it.
    fn render_attachment_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(menu) = self.attachment_menu.as_ref() else {
            return div().into_any_element();
        };
        let language = cx.language();
        let root = cx.weak_entity();
        let action = |run: fn(&mut RootView, AttachmentRow, &mut Context<RootView>)| {
            let (root, attachment) = (root.clone(), menu.attachment.clone());
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                let attachment = attachment.clone();
                let _ = root.update(cx, |this, cx| run(this, attachment, cx));
            }
        };
        let items = ui::ContextMenu::new()
            .entry(
                Key::AttachmentOpen.tr(language),
                action(Self::open_attachment),
            )
            .separator()
            .entry(
                Key::AttachmentSaveAs.tr(language),
                action(Self::save_attachment_as),
            )
            .entry(
                Key::AttachmentSaveToDownloads.tr(language),
                action(Self::save_attachment_to_downloads),
            );
        div()
            .absolute()
            .inset_0()
            .occlude()
            .on_any_mouse_down(cx.listener(|this, _, _, cx| {
                this.attachment_menu = None;
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

    /// A dropdown over the window at `position`; a mouse-down outside it runs
    /// `close`.
    fn render_dropdown(
        id: &'static str,
        position: Point<Pixels>,
        items: ui::ContextMenu,
        close: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .absolute()
            .inset_0()
            .occlude()
            .on_any_mouse_down(cx.listener(move |this, _, _, cx| {
                close(this, cx);
                cx.notify();
            }))
            .child(deferred(
                anchored()
                    .position(position)
                    .anchor(Corner::TopLeft)
                    .snap_to_window_with_margin(px(8.0))
                    // Accounts can have more folders than fit on screen.
                    .child(
                        div()
                            .id(id)
                            .max_h(px(MENU_MAX_HEIGHT))
                            .overflow_y_scroll()
                            .occlude()
                            .child(items),
                    ),
            ))
    }

    /// The filter button's menu; the active filter carries a check mark.
    fn render_filter_menu(
        &self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let language = cx.language();
        let root = cx.weak_entity();
        let mut items = ui::ContextMenu::new();
        for filter in ListFilter::OPTIONS {
            let root = root.clone();
            let choose = move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                let _ = root.update(cx, |this, cx| this.set_list_filter(filter, cx));
            };
            let label = filter.label().tr(language);
            items = if filter == self.list_filter {
                items.entry_with_key_hint(label, "✓", choose)
            } else {
                items.entry(label, choose)
            };
        }
        Self::render_dropdown(
            "filter-menu",
            position,
            items,
            |this, _| this.filter_menu = None,
            cx,
        )
    }

    /// The Move button's menu: the folders the selected message can go to.
    fn render_move_menu(
        &self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let root = cx.weak_entity();
        let mut items = ui::ContextMenu::new();
        for (label, path) in self.command_context().move_targets(cx.language()) {
            let root = root.clone();
            items = items.entry(label, move |_, _, cx| {
                let _ = root.update(cx, |this, cx| {
                    this.move_menu = None;
                    this.move_message_to_folder(&path, cx);
                });
            });
        }
        Self::render_dropdown(
            "move-menu",
            position,
            items,
            |this, _| this.move_menu = None,
            cx,
        )
    }

    /// Panel above the status bar listing unsent messages with Retry / Edit /
    /// Discard; a mouse-down outside closes it.
    fn render_unsent_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let language = cx.language();
        let root = cx.weak_entity();
        let action = |id: i64, run: fn(&mut RootView, i64, &mut Window, &mut Context<RootView>)| {
            let root = root.clone();
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                let _ = root.update(cx, |this, cx| run(this, id, window, cx));
            }
        };
        let rows = self.unsent.iter().enumerate().map(|(ix, item)| {
            let subject = if item.subject.is_empty() {
                Key::UnsentNoSubject.tr(language).to_string()
            } else {
                item.subject.clone()
            };
            v_flex()
                .px_2()
                .py_1p5()
                .gap_0p5()
                .when(ix > 0, |el| el.border_t_1().border_color(colors.border))
                .child(Label::new(subject).size(LabelSize::Small).single_line())
                .child(
                    Label::new(format!(
                        "{}: {}",
                        Key::QuoteTo.tr(language),
                        item.recipients
                    ))
                    .size(LabelSize::XSmall)
                    .color(Color::Muted)
                    .single_line(),
                )
                .when_some(item.error.clone(), |el, error| {
                    el.child(
                        Label::new(error)
                            .size(LabelSize::XSmall)
                            .color(Color::Error),
                    )
                })
                .when(item.maybe_sent, |el| {
                    el.child(
                        Label::new(Key::UnsentMaybeSent.tr(language))
                            .size(LabelSize::XSmall)
                            .color(Color::Warning),
                    )
                })
                .child(
                    h_flex()
                        .pt_1()
                        .gap_1()
                        .justify_end()
                        .child(
                            Button::new(("unsent-retry", ix), Key::UnsentRetry.tr(language))
                                .on_click(action(item.id, |this, id, _, cx| {
                                    this.retry_unsent(id, cx)
                                })),
                        )
                        .child(
                            Button::new(("unsent-edit", ix), Key::UnsentEdit.tr(language))
                                .on_click(action(item.id, Self::edit_unsent)),
                        )
                        .child(
                            Button::new(
                                ("unsent-discard", ix),
                                Key::ComposeDiscardButton.tr(language),
                            )
                            .style(ui::ButtonStyle::Ghost)
                            .on_click(action(item.id, Self::discard_unsent)),
                        ),
                )
        });
        div()
            .absolute()
            .inset_0()
            .occlude()
            .on_any_mouse_down(cx.listener(|this, _, _, cx| {
                this.unsent_panel_open = false;
                cx.notify();
            }))
            .child(
                v_flex()
                    .id("unsent-panel")
                    .occlude()
                    .absolute()
                    .right(px(8.0))
                    .bottom(px(28.0))
                    .w(px(380.0))
                    .max_h(px(420.0))
                    .overflow_y_scroll()
                    .p_1()
                    .bg(colors.elevated_surface_background)
                    .border_1()
                    .border_color(colors.border)
                    .rounded_md()
                    .shadow_lg()
                    .child(
                        div().px_2().py_1().child(
                            Label::new(Key::UnsentTitle.tr(language))
                                .size(LabelSize::Small)
                                .semibold(),
                        ),
                    )
                    .children(rows),
            )
    }

    /// Status bar text for the sync engine (or a recent note, which wins).
    fn sync_status_text(&self, language: Language) -> Option<SharedString> {
        if let Some(note) = &self.status_note {
            return Some(note.clone());
        }
        if let Some((_, error)) = self.account_start_errors.iter().next() {
            let _ = error;
            return Some(Key::StatusAuthFailed.tr(language).into());
        }
        let status = self.mail.as_ref()?.overall();
        match status {
            OverallStatus::None => None,
            OverallStatus::Syncing => Some(Key::StatusSyncing.tr(language).into()),
            OverallStatus::UpToDate(at) => {
                Some(locale::status_updated_at(language, &dates::clock_time(at)).into())
            }
            OverallStatus::Offline(_) => Some(Key::StatusOffline.tr(language).into()),
            OverallStatus::AuthFailed(account_id) => {
                let email = self
                    .accounts
                    .iter()
                    .find(|account| account.id == account_id)
                    .map(|account| account.email.as_str())
                    .unwrap_or_default();
                Some(format!("{}: {email}", Key::StatusAuthFailed.tr(language)).into())
            }
            OverallStatus::Error(_) => Some(Key::StatusSyncError.tr(language).into()),
        }
    }

    /// Shown instead of the list and reader while no account exists.
    fn render_welcome(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let language = cx.language();
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(colors.background)
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                Icon::new(IconName::Account)
                    .size(IconSize::Medium)
                    .color(Color::Muted),
            )
            .child(
                Label::new(Key::WelcomeTitle.tr(language))
                    .size(LabelSize::Large)
                    .bold(),
            )
            .child(
                Label::new(Key::WelcomeBody.tr(language))
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(
                Button::new("welcome-add-account", Key::AddAccount.tr(language))
                    .style(ui::ButtonStyle::Filled)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_settings_section(
                            crate::settings::SettingsSection::Accounts,
                            window,
                            cx,
                        );
                    })),
            )
    }

    fn render_reader(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let bg = colors.background;
        let border = colors.border;
        let accent = colors.accent;
        let on_accent = colors.text_on_accent;
        // The thumb must show on the page's color, not the app's.
        let thumb = if self.reader_white_background {
            let light = theme::Theme::light();
            let colors = light.colors();
            (colors.scrollbar_thumb, colors.scrollbar_thumb_hover)
        } else {
            (colors.scrollbar_thumb, colors.scrollbar_thumb_hover)
        };

        let Some(message) = self.reader_detail() else {
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
                        Label::new(dates::full_date(
                            message.date_ts,
                            &message.received_at,
                            cx.language(),
                        ))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                    ),
            )
            .children(self.render_recipients(message, cx))
            .children(self.render_attachments(cx));

        let loading_body = message.body_state == storage::BODY_STATE_HEADERS;
        let preparing = self.reader_preparing;
        let language = cx.language();
        // The body is rendered natively by `html_view` (scrolls on both axes;
        // laid out at the pane width).
        let scrollbars = self
            .reader_view
            .as_ref()
            .filter(|_| !loading_body && !preparing)
            .map(|view| view.read(cx).scroll_handle().clone())
            .into_iter()
            .flat_map(|handle| {
                let vertical = self
                    .reader_v_scrollbar
                    .clone()
                    .map(|state| Scrollbar::vertical(state, handle.clone()));
                let horizontal = self
                    .reader_h_scrollbar
                    .clone()
                    .map(|state| Scrollbar::horizontal(state, handle));
                vertical
                    .into_iter()
                    .chain(horizontal)
                    .map(move |bar| bar.always_visible().thumb_colors(thumb.0, thumb.1))
            })
            .collect::<Vec<_>>();
        let body_area = div()
            .relative()
            .flex_1()
            .min_h_0()
            .min_w_0()
            // Match the forced-white page so no themed background shows at
            // sub-pixel edges.
            .when(self.reader_white_background, |el| el.bg(gpui::white()))
            .when(loading_body || preparing, |el| {
                let text = if loading_body {
                    Key::ReaderLoadingBody
                } else {
                    Key::ReaderPreparingBody
                };
                el.flex().items_center().justify_center().child(
                    Label::new(text.tr(language))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
            })
            .when(!loading_body && !preparing, |el| {
                el.when_some(self.reader_view.clone(), |el, view| el.child(view))
            })
            .children(scrollbars);

        let reader = v_flex()
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
            .child(body_area);
        if self.multi_selection.is_empty() {
            return reader.into_any_element();
        }
        self.render_selection_stack(reader, cx).into_any_element()
    }

    /// Reader pane for a multiple selection, as in macOS Mail: the message on a
    /// sheet, the other selected messages as sheets askew behind it, and the
    /// actions that apply to all of them floating on its top edge. GPUI can't
    /// rotate text, so only the sheets behind are tilted.
    fn render_selection_stack(
        &self,
        reader: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        const ACTIONS_OVERLAP: f32 = 15.0;
        const SHADOW_OFFSET: f32 = 4.0;

        let colors = cx.theme().colors();
        let (bg, border, surface) = (colors.background, colors.border, colors.surface_background);
        // Stand out from the pane: the page's color, or a lighter surface on a dark
        // page.
        let sheet_fill = if self.reader_white_background {
            gpui::white()
        } else {
            colors.element_active
        };
        let sheet_edge = colors.text_disabled;
        let language = cx.language();
        let count = self.multi_selection.len();
        let in_trash = self.showing_trash();
        let movable = self.command_context().selection_movable();
        let bulk = self.bulk_selection().unwrap_or_default();
        let (has_unread, all_starred) = (bulk.has_unread, bulk.all_starred);
        let tilts: Vec<f32> = SHEET_TILTS.iter().copied().take(count - 1).collect();
        let sheets = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let margin = point(
                    bounds.size.width * STACK_MARGIN,
                    bounds.size.height * STACK_MARGIN,
                );
                let sheet =
                    Bounds::from_corners(bounds.origin + margin, bounds.bottom_right() - margin);
                for degrees in tilts.into_iter().rev() {
                    let corners = sheet_corners(sheet, degrees);
                    let outline = |mut builder: PathBuilder, shift: f32| {
                        let at = |corner: Point<Pixels>| corner + point(px(shift), px(shift));
                        builder.move_to(at(corners[0]));
                        for corner in &corners[1..] {
                            builder.line_to(at(*corner));
                        }
                        builder.close();
                        builder.build()
                    };
                    if let Ok(path) = outline(PathBuilder::fill(), SHADOW_OFFSET) {
                        window.paint_path(path, gpui::black().opacity(0.25));
                    }
                    if let Ok(path) = outline(PathBuilder::fill(), 0.0) {
                        window.paint_path(path, sheet_fill);
                    }
                    if let Ok(path) = outline(PathBuilder::stroke(px(1.0)), 0.0) {
                        window.paint_path(path, sheet_edge);
                    }
                }
            },
        )
        .absolute()
        .size_full();

        let action = |id: &'static str,
                      icon: IconName,
                      label: Key,
                      run: fn(&mut Self, &mut Context<Self>),
                      cx: &mut Context<Self>| {
            Self::icon_button_with_tooltip(
                id,
                label.tr(language),
                None,
                IconButton::new(id, icon)
                    .size(IconSize::Small)
                    .on_click(cx.listener(move |this, _, _, cx| run(this, cx))),
            )
        };
        let actions = h_flex()
            .h(px(ACTIONS_OVERLAP * 2.0))
            .px_3()
            .gap_1()
            .occlude()
            .rounded_full()
            .border_1()
            .border_color(border)
            .bg(surface)
            .shadow_md()
            .child(
                div().pr_1().child(
                    Label::new(locale::messages_selected(language, count))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                ),
            )
            .when(in_trash, |el| {
                el.child(action(
                    "bulk-restore",
                    IconName::Inbox,
                    Key::CommandRestore,
                    Self::restore_message,
                    cx,
                ))
                .child(Self::icon_button_with_tooltip(
                    "bulk-delete-forever",
                    Key::CommandDeletePermanent.tr(language),
                    None,
                    IconButton::new("bulk-delete-forever", IconName::Trash)
                        .size(IconSize::Small)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.confirm_delete_forever(this.selected_ids(), window, cx);
                        })),
                ))
            })
            .when(!in_trash, |el| {
                el.child(action(
                    "bulk-trash",
                    IconName::Trash,
                    Key::CommandDelete,
                    Self::delete_message_to_trash,
                    cx,
                ))
                .when(movable, |el| {
                    el.child(action(
                        "bulk-archive",
                        IconName::Archive,
                        Key::CommandArchive,
                        Self::archive_message,
                        cx,
                    ))
                    .child(action(
                        "bulk-junk",
                        IconName::Junk,
                        Key::CommandMarkJunk,
                        Self::mark_message_junk,
                        cx,
                    ))
                })
                .child(action(
                    "bulk-star",
                    star_icon(all_starred),
                    if all_starred {
                        Key::CommandUnflag
                    } else {
                        Key::CommandFlag
                    },
                    Self::toggle_message_flag,
                    cx,
                ))
                .child(action(
                    "bulk-read",
                    IconName::Mail,
                    if has_unread {
                        Key::CommandMarkRead
                    } else {
                        Key::CommandMarkUnread
                    },
                    Self::toggle_message_read,
                    cx,
                ))
                .when(movable, |el| {
                    el.child(Self::icon_button_with_tooltip(
                        "bulk-move",
                        Key::ToolbarMove.tr(language),
                        None,
                        IconButton::new("bulk-move", IconName::Folder)
                            .size(IconSize::Small)
                            .on_click(cx.listener(|this, event: &gpui::ClickEvent, _, cx| {
                                this.move_menu = Some(event.position());
                                cx.notify();
                            })),
                    ))
                })
            });

        div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(bg)
            // In a tall, narrow pane the tilted corners must not reach the list.
            .overflow_hidden()
            .child(sheets)
            .child(
                v_flex()
                    .absolute()
                    .top(relative(STACK_MARGIN))
                    .bottom(relative(STACK_MARGIN))
                    .left(relative(STACK_MARGIN))
                    .right(relative(STACK_MARGIN))
                    .overflow_hidden()
                    .rounded_md()
                    .border_1()
                    .border_color(border)
                    .shadow_xl()
                    .child(reader),
            )
            .child(
                h_flex()
                    .absolute()
                    .top(relative(STACK_MARGIN))
                    .mt(px(-ACTIONS_OVERLAP))
                    .left_0()
                    .right_0()
                    .justify_center()
                    .child(actions),
            )
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
                    .when(!self.unsent.is_empty(), |this| {
                        this.child(
                            div()
                                .id("unsent-status")
                                .cursor_pointer()
                                .child(
                                    Label::new(locale::status_unsent(language, self.unsent.len()))
                                        .size(LabelSize::XSmall)
                                        .color(Color::Warning),
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.toggle_unsent_panel(cx);
                                })),
                        )
                    })
                    .when_some(self.sync_status_text(language), |this, text| {
                        this.child(Label::new(text).size(LabelSize::XSmall).color(Color::Muted))
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

        if self.onboarding_active() {
            return self.render_onboarding(window, cx);
        }

        // Make sure the scrollbar state entities exist before the panels render.
        self.ensure_scrollbar_states(cx);
        // Keyboard navigation lives on the message list: give it focus
        // whenever nothing in the window has it (startup, after the search
        // field blurs), so arrows/Delete work without a click first.
        let list_focus = self.ensure_list_focus(cx);
        if window.focused(cx).is_none()
            && !self
                .command_palette
                .as_ref()
                .is_some_and(|palette| palette.open)
        {
            window.defer(cx, move |window, _| window.focus(&list_focus));
        }
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
        row = if self.accounts.is_empty() {
            row.child(self.render_welcome(cx))
        } else {
            row.child(self.render_message_list(cx))
                .child(self.render_reader(cx))
        };

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
            .on_drag_move(|event: &DragMoveEvent<DraggedMessages>, window, cx| {
                let dragged = event.drag(cx).clone();
                dragged.update_cursor(event.event.position, window, cx);
            })
            .capture_action(cx.listener(|this, _: &FocusNext, _, cx| {
                if this.palette_traps_tab() {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &FocusPrevious, _, cx| {
                if this.palette_traps_tab() {
                    cx.stop_propagation();
                }
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
            .when(self.attachment_menu.is_some(), |el| {
                el.child(self.render_attachment_menu(cx))
            })
            .when_some(self.filter_menu, |el, position| {
                el.child(self.render_filter_menu(position, cx))
            })
            .when_some(self.move_menu, |el, position| {
                el.child(self.render_move_menu(position, cx))
            })
            .when(self.unsent_panel_open, |el| {
                el.child(self.render_unsent_panel(cx))
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
impl RootView {
    /// A view on an empty database, as on a first launch.
    pub(crate) fn status_note_for_tests(&self) -> Option<&SharedString> {
        self.status_note.as_ref()
    }

    pub(crate) fn compose_window_for_tests(&self) -> Option<WindowHandle<ComposeView>> {
        self.compose_window
    }

    pub(crate) fn for_tests(path: &std::path::Path) -> Self {
        Self::new_with_database(Config::default(), path, false)
    }

    pub(crate) fn onboarding_view(&self) -> Option<Entity<crate::onboarding::OnboardingView>> {
        self.onboarding.clone()
    }

    pub(crate) fn command_palette_is_open(&self) -> bool {
        self.command_palette
            .as_ref()
            .is_some_and(|palette| palette.open)
    }

    pub(crate) fn persisted_onboarding_completed(&self) -> bool {
        self.current_config().onboarding_completed
    }

    pub(crate) fn persisted_light_theme(&self) -> bool {
        self.current_config().light_theme
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{self, CommandId};
    use tempfile::TempDir;

    fn test_view() -> (TempDir, RootView) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mail.db");
        let view = RootView::new_with_database(Config::default(), &path, true);
        (dir, view)
    }

    #[test]
    fn a_profile_with_mail_skips_the_setup_and_records_it() {
        let (_dir, view) = test_view();
        assert!(!view.onboarding_active());
        assert!(view.current_config().onboarding_completed);
    }

    #[test]
    fn an_empty_profile_starts_in_the_setup() {
        let dir = tempfile::tempdir().expect("tempdir");
        let view = RootView::for_tests(&dir.path().join("mail.db"));
        assert!(view.onboarding_active());
    }

    #[test]
    fn a_finished_setup_stays_finished_without_accounts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = Config {
            onboarding_completed: true,
            ..Config::default()
        };
        let view = RootView::new_with_database(config, dir.path().join("mail.db"), false);
        assert!(!view.onboarding_active());
    }

    #[test]
    fn sidebar_starts_visible() {
        let (_dir, view) = test_view();
        assert!(view.show_sidebar);
    }

    fn job(html: &str, crashed: bool) -> ReaderJob {
        ReaderJob {
            colors: DocumentColors {
                background: gpui::white(),
                text: gpui::black(),
                accent: gpui::black(),
            },
            body: MessageBody::Html(html.to_string().into()),
            remote: false,
            crashed,
            selection_color: [0, 0, 0, 255],
            width: 500.0,
        }
    }

    #[test]
    fn only_large_html_is_built_in_the_background() {
        assert!(!job("<p>small</p>", false).runs_in_background());
        let large = "x".repeat(BACKGROUND_BUILD_BYTES);
        assert!(job(&large, false).runs_in_background());
        let mut text = job("", false);
        text.body = MessageBody::Text(large.into());
        assert!(!text.runs_in_background());
    }

    #[test]
    fn reader_documents_build_on_another_thread() {
        let built = std::thread::spawn(|| job("<p>Hello</p>", false).build())
            .join()
            .expect("worker");
        assert_eq!(built.notice, None);
        let mut document = built.document.into_inner();
        assert!(document.content_height() > 0.0);
        assert_eq!(document.find("hello"), 1);
    }

    #[test]
    fn a_message_that_crashed_is_built_as_plain_text() {
        let built = job("<p>Hello</p><img src=\"https://a.test/x.png\">", true).build();
        assert_eq!(built.notice, Some(ReaderNotice::Crashed));
        assert_eq!(built.blocked_images, 0);
    }

    #[test]
    fn find_steps_wrap_around() {
        assert_eq!(step_match(0, 3, 1), 1);
        assert_eq!(step_match(2, 3, 1), 0);
        assert_eq!(step_match(0, 3, -1), 2);
        assert_eq!(step_match(0, 0, 1), 0);
    }

    #[test]
    fn reader_selection_is_opaque_on_light_pages_only() {
        let accent = gpui::rgb(0x0078d4).into();
        let on_white = reader_selection_color(accent, gpui::white());
        assert_eq!(on_white[3], 255);
        assert!(on_white[0] > 150 && on_white[2] > 220, "{on_white:?}");
        let on_dark = reader_selection_color(accent, gpui::black());
        assert_eq!(on_dark[3], 89);
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
        view.preferred_list_width = px(900.0);
        view.sync_layout(px(1400.0));
        let reader = px(1400.0) - view.sidebar_width - view.list_width;
        assert!(reader >= px(READER_MIN_WIDTH));
        assert!(view.list_width >= px(LIST_MIN_WIDTH));
        assert!(view.list_width < px(900.0));
    }

    #[test]
    fn a_smaller_window_or_zoom_does_not_cost_the_chosen_widths() {
        let (_dir, mut view) = test_view();
        view.preferred_sidebar_width = px(300.0);
        view.preferred_list_width = px(900.0);
        view.sync_layout(px(1536.0));
        assert_eq!(view.list_width, px(1536.0 - 300.0 - READER_MIN_WIDTH));
        assert_eq!(view.current_config().list_width, 900.0);

        view.sync_layout(px(1920.0));
        assert_eq!(view.sidebar_width, px(300.0));
        assert_eq!(view.list_width, px(900.0));
    }

    #[test]
    fn dragging_a_divider_chooses_the_width() {
        let (_dir, mut view) = test_view();
        view.narrow = false;
        view.show_sidebar = true;
        view.resize(ResizeHandle::Sidebar, px(300.0), px(1920.0));
        view.resize(ResizeHandle::List, px(800.0), px(1920.0));
        assert_eq!(view.preferred_sidebar_width, px(300.0));
        assert_eq!(view.preferred_list_width, px(500.0));
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
    fn trash_folder_shows_no_unread_count() {
        assert!(!folder_shows_unread(storage::system::TRASH));
        assert!(folder_shows_unread(storage::system::INBOX));
        assert!(folder_shows_unread(storage::system::JUNK));
    }

    #[test]
    fn only_a_hovered_row_makes_room_for_its_buttons() {
        assert_eq!(row_actions_room(false), px(0.0));
        assert_eq!(row_actions_room(true), px(40.0));
    }

    #[test]
    fn toolbar_star_matches_the_list_when_set() {
        assert_eq!(star_icon(true), IconName::StarFilled);
        assert_eq!(star_color(true), Color::Warning);
        assert_eq!(star_icon(false), IconName::Star);
        assert_eq!(star_color(false), Color::Default);
    }

    #[test]
    fn trashed_mail_leaves_starred_and_comes_back_when_restored() {
        let (_dir, view) = test_view();
        let flagged = storage::system::FLAGGED;
        let query = MailListQuery::GlobalSystemFolder(flagged.to_string());
        let id = view
            .list_messages
            .iter()
            .find(|message| message.unread)
            .map(|message| message.id)
            .expect("an unread message");
        mail_actions::set_starred(&view.db, id, true).unwrap();
        let unread = view.db.global_unread(flagged).unwrap();
        let listed = |view: &RootView| {
            view.db
                .list_messages(&query)
                .unwrap()
                .iter()
                .any(|message| message.id == id)
        };
        assert!(listed(&view));

        mail_actions::trash(&view.db, id).unwrap();
        assert!(!listed(&view));
        assert_eq!(view.db.global_unread(flagged).unwrap(), unread - 1);
        assert_eq!(
            view.db.message_count_for_query(&query).unwrap(),
            view.db.list_messages(&query).unwrap().len()
        );

        mail_actions::restore_from_trash(&view.db, id).unwrap();
        assert!(listed(&view));
    }

    #[test]
    fn recipients_go_by_first_name_address_or_me() {
        let named = Address::new(Some("Paulo Mello"), "paulo@x.test");
        assert_eq!(short_recipient(&named, "me@x.test", "mim"), "Paulo");
        let nameless = Address::new(None::<String>, "fabio@x.test");
        assert_eq!(
            short_recipient(&nameless, "me@x.test", "mim"),
            "fabio@x.test"
        );
        let own = Address::new(Some("Belizário Ribeiro"), "ME@x.test");
        assert_eq!(short_recipient(&own, "me@x.test", "mim"), "mim");
    }

    #[test]
    fn recipients_in_both_to_and_cc_show_once() {
        let to = [Address::new(Some("Ann"), "a@x.test")];
        let cc = [
            Address::new(None::<String>, "A@X.test"),
            Address::new(Some("Bob"), "b@x.test"),
        ];
        let everyone = unique_recipients(to.iter().chain(&cc));
        assert_eq!(everyone.len(), 2);
        assert_eq!(everyone[0].name.as_deref(), Some("Ann"));
    }

    #[test]
    fn the_drag_cursor_is_the_closed_hand_or_hidden_for_a_painted_one() {
        assert_eq!(drag_cursor(false), CursorStyle::ClosedHand);
        assert_eq!(drag_cursor(true), CursorStyle::None);
    }

    /// A row and the app's root listener, as they are wired in the real views.
    struct DragHost {
        dragged: DraggedMessages,
    }

    impl Render for DragHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .on_drag_move(|event: &DragMoveEvent<DraggedMessages>, window, cx| {
                    let dragged = event.drag(cx).clone();
                    dragged.update_cursor(event.event.position, window, cx);
                })
                .child(div().id("row").w(px(300.0)).h(px(60.0)).on_drag(
                    self.dragged.clone(),
                    |dragged, grab, window, cx| {
                        dragged.progress.start(window.mouse_position());
                        let progress = dragged.progress.clone();
                        cx.new(|_| DragPreview {
                            progress,
                            grab,
                            width: px(200.0),
                            title: "Ann".into(),
                            subtitle: "Hello".into(),
                            draws_hand: false,
                        })
                    },
                ))
        }
    }

    #[gpui::test]
    fn dragging_past_the_threshold_sets_the_cursor_and_places_the_card(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| theme::init(theme::Appearance::Dark, cx));
        let dragged = DraggedMessages {
            ids: vec![1],
            account_id: Some(1),
            progress: Rc::default(),
        };
        let (_, cx) = cx.add_window_view(|_, _| DragHost { dragged });
        cx.run_until_parked();
        let held = Some(MouseButton::Left);
        cx.simulate_mouse_down(
            point(px(50.0), px(30.0)),
            MouseButton::Left,
            Default::default(),
        );

        // A short drag: GPUI has started it, but it doesn't count yet.
        cx.simulate_mouse_move(point(px(60.0), px(30.0)), held, Default::default());
        cx.run_until_parked();
        assert!(cx.update(|_, cx| cx.has_active_drag()));
        assert_eq!(cx.update(|_, cx| cx.active_drag_cursor_style()), None);
        assert_eq!(cx.debug_bounds("drag-card"), None);

        // Past the threshold, and back: it keeps counting.
        let far = point(px(400.0), px(300.0));
        cx.simulate_mouse_move(far, held, Default::default());
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| cx.active_drag_cursor_style()),
            Some(drag_cursor(DRAG_DRAWS_HAND))
        );
        let card = cx.debug_bounds("drag-card").expect("the card shows");
        assert_eq!(card.origin, pointer_inset(far));

        let back = point(px(70.0), px(40.0));
        cx.simulate_mouse_move(back, held, Default::default());
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| cx.active_drag_cursor_style()),
            Some(drag_cursor(DRAG_DRAWS_HAND))
        );
        assert_eq!(
            cx.debug_bounds("drag-card").expect("still shows").origin,
            pointer_inset(back)
        );
    }

    /// Where the card's corner belongs for a pointer at `pointer`.
    fn pointer_inset(pointer: Point<Pixels>) -> Point<Pixels> {
        point(pointer.x - px(12.0), pointer.y - px(12.0))
    }

    #[test]
    fn the_pointer_sits_12px_inside_the_top_left_corner_of_the_drag_card() {
        let grab = point(px(40.0), px(10.0));
        assert_eq!(drag_card_origin(grab), pointer_inset(grab));
    }

    #[test]
    fn a_drag_counts_only_past_the_threshold() {
        let origin = point(px(100.0), px(100.0));
        assert!(!drag_counts(origin, point(px(100.0), px(100.0))));
        assert!(!drag_counts(origin, point(px(200.0), px(100.0))));
        assert!(!drag_counts(origin, point(px(160.0), px(160.0))));
        assert!(drag_counts(origin, point(px(201.0), px(100.0))));
        assert!(drag_counts(origin, point(px(100.0), px(-1.0))));
    }

    #[test]
    fn dropping_needs_the_distance_a_folder_of_the_account_and_a_valid_target() {
        let far = point(px(300.0), px(0.0));
        let near = point(px(50.0), px(0.0));
        let started = || {
            let progress = DragProgress::default();
            progress.start(point(px(0.0), px(0.0)));
            DraggedMessages {
                ids: vec![1],
                account_id: Some(7),
                progress: Rc::new(progress),
            }
        };
        let folder = storage::system::ARCHIVE;
        assert!(!started().can_drop_on(7, folder, near));
        assert!(started().can_drop_on(7, folder, far));
        assert!(!started().can_drop_on(8, folder, far));
        assert!(!started().can_drop_on(7, storage::system::SENT, far));
        assert!(!started().can_drop_on(7, storage::system::DRAFTS, far));

        let not_started = DraggedMessages {
            progress: Rc::default(),
            ..started()
        };
        assert!(!not_started.can_drop_on(7, folder, far));
        let across_accounts = DraggedMessages {
            account_id: None,
            ..started()
        };
        assert!(!across_accounts.can_drop_on(7, folder, far));
    }

    #[test]
    fn past_the_threshold_a_drag_counts_even_back_near_its_start() {
        let progress = DragProgress::default();
        progress.start(point(px(0.0), px(0.0)));
        assert!(!progress.counts_at(point(px(50.0), px(0.0))));
        assert!(progress.counts_at(point(px(150.0), px(0.0))));
        assert!(progress.counts_at(point(px(10.0), px(0.0))));
        // A new drag starts over.
        progress.start(point(px(0.0), px(0.0)));
        assert!(!progress.counts_at(point(px(10.0), px(0.0))));
    }

    #[test]
    fn dragging_a_selected_row_takes_the_selection_along() {
        let (_dir, mut view) = test_view();
        let ids = view.list_ids();
        view.apply_selection(ids[..2].to_vec());
        assert_eq!(view.dragged_messages(ids[0]).unwrap().ids, ids[..2]);
        assert_eq!(view.dragged_messages(ids[2]).unwrap().ids, [ids[2]]);
        assert!(view.dragged_messages(ids[2]).unwrap().account_id.is_some());
    }

    #[test]
    fn window_classes_are_distinct() {
        assert_eq!(MAIN_WINDOW_APP_ID, "com.bgmail.main");
        assert_eq!(SETTINGS_WINDOW_APP_ID, "com.bgmail.settings");
        assert_eq!(COMPOSE_WINDOW_APP_ID, "com.bgmail.newmail");
    }

    #[test]
    fn clicks_select_one_toggle_or_take_the_range() {
        let list = [1, 2, 3, 4, 5];
        assert_eq!(
            clicked_selection(&list, &[1, 2], Some(1), 4, false, false),
            vec![4]
        );
        assert_eq!(
            clicked_selection(&list, &[1], Some(1), 3, false, true),
            vec![1, 3]
        );
        assert_eq!(
            clicked_selection(&list, &[1, 3], Some(1), 3, false, true),
            vec![1]
        );
        assert_eq!(
            clicked_selection(&list, &[2], Some(2), 4, true, false),
            vec![2, 3, 4]
        );
        assert_eq!(
            clicked_selection(&list, &[4], Some(4), 2, true, false),
            vec![2, 3, 4]
        );
        // The anchor left the list: only the clicked row.
        assert_eq!(
            clicked_selection(&list, &[], Some(99), 2, true, false),
            vec![2]
        );
    }

    #[test]
    fn multiple_selection_leaves_the_reader_empty_until_one_row_remains() {
        let (_dir, mut view) = test_view();
        let ids = view.list_ids();
        assert!(ids.len() >= 3);
        assert!(view.apply_selection(vec![ids[0]]));
        assert_eq!(view.selected_message_id, Some(ids[0]));

        assert!(!view.apply_selection(vec![ids[0], ids[1]]));
        assert_eq!(view.selected_message_id, None);
        assert!(view.row_is_selected(ids[1]));
        assert!(view.command_context().bulk.is_some());
        assert_eq!(view.selected_detail, None);
        assert_eq!(view.reader_detail().map(|d| d.id), Some(ids[0]));
        view.selection_focus = Some(ids[1]);
        view.reload_multi_preview();
        assert_eq!(view.reader_detail().map(|d| d.id), Some(ids[1]));
        assert!(view.command_context().message_detail.is_none());

        view.multi_selection = vec![ids[0], -1];
        view.prune_multi_selection();
        assert!(view.multi_selection.is_empty());
        assert_eq!(view.selected_message_id, Some(ids[0]));
    }

    #[test]
    fn a_multiple_selection_that_left_the_list_leaves_the_reader_empty() {
        let (_dir, mut view) = test_view();
        let ids = view.list_ids();
        view.apply_selection(vec![ids[0], ids[1]]);
        assert!(view.reader_detail().is_some());

        view.list_messages.clear();
        assert!(!view.prune_multi_selection());
        assert!(view.multi_selection.is_empty());
        assert!(view.reader_detail().is_none());
        assert!(view.attachments.is_empty());
    }

    #[test]
    fn a_plain_selection_survives_pruning_an_empty_multiple_one() {
        let (_dir, mut view) = test_view();
        let ids = view.list_ids();
        view.apply_selection(vec![ids[0]]);
        assert!(!view.prune_multi_selection());
        assert_eq!(view.selected_message_id, Some(ids[0]));
    }

    #[test]
    fn a_multiple_selection_down_to_one_row_counts_as_collapsed() {
        let (_dir, mut view) = test_view();
        let ids = view.list_ids();
        view.apply_selection(vec![ids[0], ids[1]]);
        view.list_messages.retain(|message| message.id == ids[1]);
        assert!(view.prune_multi_selection());
        assert_eq!(view.selected_message_id, Some(ids[1]));
    }

    #[test]
    fn a_reply_without_the_body_still_follows_reply_to() {
        let (_dir, view) = test_view();
        let id = view.list_ids()[0];
        let mut detail = view.db.get_message(id).unwrap().unwrap();
        detail.reply_to = "Support <support@example.com>".into();
        detail.to_addrs = "ok@example.com, not an address".into();
        let parsed = parsed_from_detail(&detail);
        assert_eq!(parsed.reply_to[0].email, "support@example.com");
        assert_eq!(parsed.to.len(), 1);
        assert_eq!(parsed.to[0].email, "ok@example.com");
    }

    #[test]
    fn previewing_a_multiple_selection_keeps_messages_unread() {
        let (_dir, mut view) = test_view();
        let unread: Vec<i64> = view
            .list_messages
            .iter()
            .filter(|message| message.unread)
            .map(|message| message.id)
            .take(2)
            .collect();
        view.apply_selection(unread.clone());
        assert!(view.reader_detail().is_some());
        for id in unread {
            assert!(view.db.get_message(id).unwrap().unwrap().unread);
        }
        view.clear_message_selection();
        assert!(view.reader_detail().is_none());
    }

    fn bulk_view(
        dir: &TempDir,
        cx: &mut gpui::TestAppContext,
    ) -> (gpui::Entity<RootView>, Vec<i64>) {
        cx.update(|cx| {
            theme::init(theme::Appearance::Dark, cx);
            locale::init(Language::default(), cx);
        });
        let path = dir.path().join("mail.db");
        let view = cx.new(|_| RootView::new_with_database(Config::default(), &path, true));
        let ids = view.read_with(cx, |view, _| view.list_ids()[..2].to_vec());
        view.update(cx, |view, _| view.apply_selection(ids.clone()));
        (view, ids)
    }

    #[gpui::test]
    fn bulk_toggles_keep_the_selection_and_cover_every_row(cx: &mut gpui::TestAppContext) {
        let dir = tempfile::tempdir().expect("tempdir");
        let (view, ids) = bulk_view(&dir, cx);
        let starred = view.read_with(cx, |view, _| !view.bulk_selection().unwrap().all_starred);
        assert!(starred);
        view.update(cx, |view, cx| {
            view.apply_action_keeping_selection(
                move |db, id| mail_actions::set_starred(db, id, starred),
                cx,
            );
        });
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert_eq!(view.multi_selection, ids);
            assert!(view.bulk_selection().unwrap().all_starred);
        });

        view.update(cx, |view, cx| {
            view.apply_action_keeping_selection(|db, id| mail_actions::set_read(db, id, true), cx);
        });
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert!(!view.bulk_selection().unwrap().has_unread);
            for id in &ids {
                let detail = view.db.get_message(*id).unwrap().unwrap();
                assert!(detail.starred && !detail.unread);
            }
        });
    }

    #[gpui::test]
    fn bulk_restore_brings_trashed_rows_back(cx: &mut gpui::TestAppContext) {
        let dir = tempfile::tempdir().expect("tempdir");
        let (view, ids) = bulk_view(&dir, cx);
        view.update(cx, |view, cx| {
            view.apply_action_to_selection(mail_actions::trash, cx);
        });
        cx.run_until_parked();
        view.update(cx, |view, cx| {
            view.apply_selection(ids.clone());
            view.apply_action_to_selection(mail_actions::restore_from_trash, cx);
        });
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            for id in &ids {
                assert_eq!(view.db.message_is_in_trash(*id), Ok(false));
            }
        });
    }

    #[test]
    fn sheets_turn_around_their_center() {
        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(100.0), px(50.0)));
        assert_eq!(sheet_corners(bounds, 0.0)[2], point(px(100.0), px(50.0)));
        let [a, _, c, _] = sheet_corners(bounds, 2.0);
        assert!((f32::from(a.x + c.x) - 100.0).abs() < 0.01);
        assert!((f32::from(a.y + c.y) - 50.0).abs() < 0.01);
        assert!(f32::from(a.y) < 0.0 || f32::from(a.x) > 0.0);
    }

    #[test]
    fn shift_arrow_grows_and_shrinks_from_the_anchor() {
        let (_dir, mut view) = test_view();
        let ids = view.list_ids();
        view.selected_message_id = Some(ids[1]);
        assert_eq!(view.extended_selection(1), Some(ids[1..=2].to_vec()));
        assert_eq!(view.extended_selection(-1), Some(vec![ids[1]]));
        assert_eq!(view.extended_selection(-1), Some(ids[0..=1].to_vec()));
    }

    #[gpui::test]
    fn bulk_action_reaches_every_row_and_clears_the_selection(cx: &mut gpui::TestAppContext) {
        let dir = tempfile::tempdir().expect("tempdir");
        let (view, chosen) = bulk_view(&dir, cx);
        view.update(cx, |view, cx| {
            view.apply_action_to_selection(mail_actions::trash, cx);
            // Gone at once, even with the disk work still ahead.
            assert!(view.multi_selection.is_empty());
            assert_eq!(view.selected_message_id, None);
            assert!(!view.list_messages.iter().any(|m| chosen.contains(&m.id)));
        });
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            for id in &chosen {
                assert_eq!(view.db.message_is_in_trash(*id), Ok(true));
                assert!(!view.list_messages.iter().any(|m| m.id == *id));
            }
        });
    }

    #[test]
    fn an_arrow_out_of_a_multiple_selection_survives_the_next_reload() {
        let (_dir, mut view) = test_view();
        let ids = view.list_ids();
        view.apply_selection(vec![ids[0], ids[1]]);
        assert!(view.multi_preview.is_some());
        view.select_only(ids[2]);
        assert!(view.multi_preview.is_none());
        view.reload_message_list();
        assert_eq!(view.selected_message_id, Some(ids[2]));
    }

    #[test]
    fn filter_narrows_the_list_but_keeps_the_open_message() {
        let (_dir, mut view) = test_view();
        let all = view.list_messages.len();
        let read = view
            .list_messages
            .iter()
            .find(|m| !m.unread)
            .map(|m| m.id)
            .expect("a read message");
        view.clear_message_selection();
        view.list_filter = ListFilter::Unread;
        view.reload_message_list();
        assert!(view.list_messages.len() < all);
        assert!(view.list_messages.iter().all(|m| m.unread));

        view.selected_message_id = Some(read);
        view.reload_message_list();
        assert!(view.list_messages.iter().any(|m| m.id == read));

        view.selected_message_id = None;
        view.list_filter = ListFilter::All;
        view.reload_message_list();
        assert_eq!(view.list_messages.len(), all);
    }

    #[test]
    fn row_action_on_another_message_keeps_the_selection() {
        let (_dir, mut view) = test_view();
        let selected = view.selected_message_id.expect("seed selects a message");
        let other = view
            .list_messages
            .iter()
            .map(|message| message.id)
            .find(|id| *id != selected)
            .expect("seed has more messages");
        view.apply_message_action(other, mail_actions::trash)
            .unwrap();
        assert_eq!(view.selected_message_id, Some(selected));
        assert!(view.list_messages.iter().all(|m| m.id != other));
        assert!(view.db.message_is_in_trash(other).unwrap());
    }

    #[test]
    fn removing_the_selected_message_leaves_nothing_open() {
        let (_dir, mut view) = test_view();
        let selected = view.selected_message_id.expect("seed selects a message");
        let position = view
            .list_messages
            .iter()
            .position(|m| m.id == selected)
            .unwrap();
        let neighbor = view.list_messages[position + 1].id;
        view.db.set_message_unread(neighbor, true).unwrap();
        view.reload_message_list();

        view.apply_message_action(selected, mail_actions::trash)
            .unwrap();

        assert_eq!(view.selected_message_id, None);
        assert!(view.selected_detail.is_none());
        assert!(view.db.get_message(neighbor).unwrap().unwrap().unread);
    }

    #[test]
    fn the_arrow_keys_carry_on_from_the_row_that_took_the_place() {
        for step in [1, -1] {
            let (_dir, mut view) = test_view();
            let selected = view.selected_message_id.expect("seed selects a message");
            let position = view
                .list_messages
                .iter()
                .position(|m| m.id == selected)
                .unwrap();
            let neighbor = view.list_messages[position + 1].id;
            view.apply_message_action(selected, mail_actions::trash)
                .unwrap();
            assert!(view.select_adjacent_message(step));
            assert_eq!(view.selected_message_id, Some(neighbor));
        }
    }

    #[test]
    fn removing_the_last_row_leaves_the_keys_on_the_new_last_one() {
        let (_dir, mut view) = test_view();
        let last = view.list_messages.last().unwrap().id;
        view.selected_message_id = Some(last);
        view.reload_selected_detail();
        view.apply_message_action(last, mail_actions::trash)
            .unwrap();
        assert_eq!(view.selected_message_id, None);
        assert_eq!(
            view.selection_focus,
            view.list_messages.last().map(|m| m.id)
        );
    }

    #[test]
    fn a_row_action_with_nothing_selected_selects_nothing() {
        let (_dir, mut view) = test_view();
        let other = view.list_messages[0].id;
        view.clear_message_selection();
        view.apply_message_action(other, mail_actions::trash)
            .unwrap();
        assert_eq!(view.selected_message_id, None);
        assert!(view.selected_detail.is_none());
    }

    #[test]
    fn a_row_action_leaves_a_multiple_selection_alone() {
        let (_dir, mut view) = test_view();
        let ids = view.list_ids();
        view.apply_selection(ids[1..3].to_vec());
        view.apply_message_action(ids[0], mail_actions::trash)
            .unwrap();
        assert_eq!(view.selected_message_id, None);
        assert_eq!(view.multi_selection, ids[1..3]);
    }

    #[test]
    fn row_delete_refuses_a_message_already_in_trash() {
        let (_dir, view) = test_view();
        let id = view.selected_message_id.expect("seed selects a message");
        assert!(view.row_can_be_trashed(id));
        view.db.move_message_to_trash(id).unwrap();
        assert!(!view.row_can_be_trashed(id));
    }

    #[test]
    fn nothing_listed_in_sent_can_be_dragged() {
        let (_dir, mut view) = test_view();
        let id = view.selected_message_id.unwrap();
        assert!(view.dragged_messages(id).is_some());
        view.apply_mailbox_selection(Selection::Global(GlobalMailbox::Sent));
        assert!(view.dragged_messages(id).is_none());
    }

    #[test]
    fn a_sent_row_keeps_its_selection_from_being_dragged() {
        let (_dir, mut view) = test_view();
        let ids: Vec<i64> = view.list_messages.iter().take(2).map(|m| m.id).collect();
        view.multi_selection = ids.clone();
        assert!(view.dragged_messages(ids[0]).is_some());
        view.list_messages[1].folders_csv = ",sys:sent,".into();
        assert!(view.dragged_messages(ids[0]).is_none());
        assert!(view
            .command_context()
            .bulk
            .is_some_and(|bulk| bulk.any_sent_only));
    }

    #[gpui::test]
    fn archive_junk_and_move_leave_sent_mail_where_it_is(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            theme::init(theme::Appearance::Dark, cx);
            locale::init(Language::default(), cx);
        });
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mail.db");
        let view = cx.new(|_| RootView::new_with_database(Config::default(), &path, true));
        view.update(cx, |view, cx| {
            let id = view.selected_message_id.expect("seeded mail");
            let before = view.db.get_message(id).unwrap().unwrap().folders_csv;
            // The sample data has no sent mail: only what the rule reads says so.
            view.selected_detail.as_mut().unwrap().folders_csv = ",sys:sent,".into();

            view.archive_message(cx);
            view.mark_message_junk(cx);
            view.move_message_to_folder(storage::system::ARCHIVE, cx);

            let after = view.db.get_message(id).unwrap().unwrap().folders_csv;
            assert_eq!(before, after);
        });
    }

    #[gpui::test]
    fn archive_still_moves_received_mail(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            theme::init(theme::Appearance::Dark, cx);
            locale::init(Language::default(), cx);
        });
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mail.db");
        let view = cx.new(|_| RootView::new_with_database(Config::default(), &path, true));
        view.update(cx, |view, cx| {
            let id = view.selected_message_id.expect("seeded mail");
            let before = view.db.get_message(id).unwrap().unwrap().folders_csv;
            view.archive_message(cx);
            let after = view.db.get_message(id).unwrap().unwrap().folders_csv;
            assert_ne!(before, after);
        });
    }

    /// A view on the seeded mail with a message selected in the Inbox and the user gone
    /// to another mailbox, through the path the sidebar takes.
    fn switch_mailbox(view: &mut RootView, selection: Selection) {
        view.remember_selection();
        view.clear_message_selection();
        view.apply_mailbox_selection(selection);
        view.reload_message_list();
        view.restore_selection();
    }

    fn a_folder_with_mail(view: &RootView) -> (Selection, Vec<i64>) {
        view.accounts
            .iter()
            .map(|account| Selection::Mailbox {
                account_id: account.id,
                folder_path: storage::system::INBOX.into(),
            })
            .find_map(|selection| {
                let query = mail_list_query(&selection, "");
                let ids: Vec<i64> = view
                    .db
                    .list_messages(&query)
                    .unwrap_or_default()
                    .iter()
                    .map(|m| m.id)
                    .collect();
                (ids.len() >= 2).then_some((selection, ids))
            })
            .expect("seeded mail")
    }

    #[test]
    fn each_mailbox_gets_its_selected_message_back() {
        let (_dir, mut view) = test_view();
        let global = Selection::Global(GlobalMailbox::Inbox);
        let global_id = view.list_messages[1].id;
        view.selected_message_id = Some(global_id);
        view.reload_selected_detail();

        let (folder, ids) = a_folder_with_mail(&view);
        switch_mailbox(&mut view, folder.clone());
        // Nothing was selected here yet.
        assert_eq!(view.selected_message_id, None);
        view.selected_message_id = Some(ids[1]);
        view.reload_selected_detail();

        switch_mailbox(&mut view, global.clone());
        assert_eq!(view.selected_message_id, Some(global_id));
        assert_eq!(view.selected_detail.as_ref().map(|d| d.id), Some(global_id));

        switch_mailbox(&mut view, folder);
        assert_eq!(view.selected_message_id, Some(ids[1]));
    }

    #[test]
    fn leaving_with_nothing_selected_stays_that_way() {
        let (_dir, mut view) = test_view();
        let global = Selection::Global(GlobalMailbox::Inbox);
        let (folder, _) = a_folder_with_mail(&view);
        switch_mailbox(&mut view, folder.clone());
        switch_mailbox(&mut view, global.clone());
        assert!(view.selected_message_id.is_some());

        view.clear_message_selection();
        switch_mailbox(&mut view, folder);
        switch_mailbox(&mut view, global);
        assert_eq!(view.selected_message_id, None);
    }

    #[test]
    fn a_selected_message_that_left_the_mailbox_is_not_restored() {
        let (_dir, mut view) = test_view();
        let id = view.list_messages[1].id;
        view.selected_message_id = Some(id);
        view.reload_selected_detail();
        let (folder, _) = a_folder_with_mail(&view);
        switch_mailbox(&mut view, folder);
        view.db.move_message_to_trash(id).unwrap();

        switch_mailbox(&mut view, Selection::Global(GlobalMailbox::Inbox));
        assert_eq!(view.selected_message_id, None);
        assert!(view.selected_detail.is_none());
    }

    #[test]
    fn coming_back_does_not_mark_a_message_read() {
        let (_dir, mut view) = test_view();
        let id = view.list_messages[1].id;
        view.db.set_message_unread(id, true).unwrap();
        view.selected_message_id = Some(id);
        view.reload_selected_detail();
        let (folder, _) = a_folder_with_mail(&view);
        switch_mailbox(&mut view, folder);

        switch_mailbox(&mut view, Selection::Global(GlobalMailbox::Inbox));
        assert_eq!(view.selected_message_id, Some(id));
        assert!(view.db.get_message(id).unwrap().unwrap().unread);
    }

    #[test]
    fn a_multiple_selection_comes_back_too() {
        let (_dir, mut view) = test_view();
        let ids = view.list_ids();
        view.apply_selection(ids[..3].to_vec());
        view.selection_focus = Some(ids[2]);
        let (folder, _) = a_folder_with_mail(&view);
        switch_mailbox(&mut view, folder);
        assert!(view.multi_selection.is_empty());

        switch_mailbox(&mut view, Selection::Global(GlobalMailbox::Inbox));
        assert_eq!(view.multi_selection, ids[..3]);
        assert_eq!(view.selection_focus, Some(ids[2]));
        assert_eq!(view.multi_preview.as_ref().map(|d| d.id), Some(ids[2]));
    }

    #[gpui::test]
    fn the_sidebar_path_restores_the_selection(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            theme::init(theme::Appearance::Dark, cx);
            locale::init(Language::default(), cx);
        });
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mail.db");
        let view = cx.new(|_| RootView::new_with_database(Config::default(), &path, true));
        view.update(cx, |view, cx| {
            let id = view.selected_message_id.expect("seeded mail");
            let (folder, _) = a_folder_with_mail(view);
            view.select_mailbox(folder, cx);
            view.select_mailbox(Selection::Global(GlobalMailbox::Inbox), cx);
            assert_eq!(view.selected_message_id, Some(id));
        });
    }

    #[gpui::test]
    fn choosing_a_language_applies_and_remembers_it(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            theme::init(theme::Appearance::Dark, cx);
            locale::init(Language::default(), cx);
        });
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mail.db");
        let view = cx.new(|_| RootView::new_with_database(Config::default(), &path, true));
        view.update(cx, |view, cx| {
            view.set_language(Language::Portuguese, cx);
            assert_eq!(cx.language(), Language::Portuguese);
            assert_eq!(view.current_config().language(), Language::Portuguese);
        });
    }

    #[gpui::test]
    fn the_saved_language_is_kept_by_later_saves(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            theme::init(theme::Appearance::Dark, cx);
            locale::init(Language::default(), cx);
        });
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("config.json");
        let path = dir.path().join("mail.db");
        let view = cx.new(|_| RootView::new_with_database(Config::default(), &path, true));
        view.update(cx, |view, cx| {
            view.config_file = Some(file.clone());
            view.enable_persistence();
            view.set_language(Language::Portuguese, cx);
        });
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        assert_eq!(
            config::load_from(file.clone()).language(),
            Language::Portuguese
        );

        view.update(cx, |view, cx| {
            view.set_appearance(theme::Appearance::Light, cx);
        });
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        let saved = config::load_from(file);
        assert!(saved.light_theme);
        assert_eq!(saved.language(), Language::Portuguese);
    }

    #[gpui::test]
    fn an_attachment_that_runs_code_is_not_opened(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            theme::init(theme::Appearance::Dark, cx);
            locale::init(Language::default(), cx);
        });
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mail.db");
        let view = cx.new(|_| RootView::new_with_database(Config::default(), &path, true));
        view.update(cx, |view, cx| {
            view.open_attachment(
                AttachmentRow {
                    id: 1,
                    message_id: 1,
                    filename: "invoice.pdf.exe".into(),
                    mime_type: "application/octet-stream".into(),
                    size: 1,
                    part_index: 0,
                    content_id: None,
                    inline: false,
                },
                cx,
            );
            assert_eq!(
                view.status_note_for_tests().map(|note| note.as_ref()),
                Some(Key::StatusAttachmentRunsCode.tr(Language::default()))
            );
        });
    }

    #[gpui::test]
    fn zooming_changes_the_window_and_is_remembered(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            theme::init(theme::Appearance::Dark, cx);
            locale::init(Language::default(), cx);
        });
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mail.db");
        let (view, cx) =
            cx.add_window_view(|_, _| RootView::new_with_database(Config::default(), &path, true));
        view.update_in(cx, |view, window, cx| {
            view.zoom_app(1, window, cx);
            assert_eq!(window.content_scale(), 1.1);
            assert_eq!(view.current_config().ui_scale, 1.1);
            view.zoom_app(-1, window, cx);
            view.zoom_app(-1, window, cx);
            assert_eq!(window.content_scale(), 0.9);
            view.zoom_app(0, window, cx);
            assert_eq!(window.content_scale(), 1.0);
            assert_eq!(view.current_config().ui_scale, 1.0);
        });
    }

    #[test]
    fn showing_sent_follows_the_mailbox_selection() {
        let (_dir, mut view) = test_view();
        assert!(!view.showing_sent());
        view.apply_mailbox_selection(Selection::Global(GlobalMailbox::Sent));
        assert!(view.showing_sent());
        view.apply_mailbox_selection(Selection::Mailbox {
            account_id: 1,
            folder_path: storage::system::SENT.into(),
        });
        assert!(view.showing_sent());
    }

    #[test]
    fn showing_trash_follows_the_mailbox_selection() {
        let (_dir, mut view) = test_view();
        assert!(!view.showing_trash());
        view.apply_mailbox_selection(Selection::Mailbox {
            account_id: 1,
            folder_path: storage::system::TRASH.into(),
        });
        assert!(view.showing_trash());
    }

    #[test]
    fn palette_traps_tab_only_while_open() {
        let (_dir, mut view) = test_view();
        assert!(!view.palette_traps_tab());
        view.command_palette = Some(CommandPaletteState {
            open: true,
            ..Default::default()
        });
        assert!(view.palette_traps_tab());
        view.command_palette.as_mut().unwrap().open = false;
        assert!(!view.palette_traps_tab());
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

    fn folder(path: &str, display_name: &str) -> storage::Folder {
        storage::Folder {
            id: 0,
            account_id: 1,
            path: path.into(),
            display_name: display_name.into(),
            remote_name: None,
            role: None,
        }
    }

    #[test]
    fn sidebar_orders_system_folders_then_user_folders_alphabetically() {
        use storage::system;
        let folders = vec![
            folder("user:zeta", "zeta"),
            folder(system::TRASH, ""),
            folder("user:Alpha", "Alpha"),
            folder(system::SENT, ""),
            folder(system::INBOX, ""),
            folder("user:beta", "beta"),
            folder(system::JUNK, ""),
            folder(system::ARCHIVE, ""),
            folder(system::DRAFTS, ""),
        ];
        let paths: Vec<_> = sidebar_ordered(folders)
            .into_iter()
            .map(|folder| folder.path)
            .collect();
        assert_eq!(
            paths,
            [
                system::INBOX,
                system::DRAFTS,
                system::SENT,
                system::ARCHIVE,
                system::JUNK,
                system::TRASH,
                "user:Alpha",
                "user:beta",
                "user:zeta",
            ]
        );
        assert_eq!(system_folder_rank(system::INBOX), Some(0));
        assert_eq!(system_folder_rank("user:Inbox"), None);
    }

    #[test]
    fn adjacent_index_clamps_and_starts_from_the_ends() {
        assert_eq!(adjacent_index(None, 0, 1), None);
        assert_eq!(adjacent_index(None, 5, 1), Some(0));
        assert_eq!(adjacent_index(None, 5, -1), Some(4));
        assert_eq!(adjacent_index(Some(2), 5, 1), Some(3));
        assert_eq!(adjacent_index(Some(2), 5, -1), Some(1));
        assert_eq!(adjacent_index(Some(4), 5, 1), Some(4));
        assert_eq!(adjacent_index(Some(0), 5, -1), Some(0));
        assert_eq!(adjacent_index(Some(9), 5, 1), Some(4));
    }

    #[test]
    fn keyboard_selection_moves_through_the_list() {
        let (_dir, mut view) = test_view();
        assert!(view.list_messages.len() > 2);
        view.clear_message_selection();
        assert!(view.select_adjacent_message(1));
        assert_eq!(view.selected_message_id, Some(view.list_messages[0].id));
        // Already at the top: moving up does nothing.
        assert!(!view.select_adjacent_message(-1));
        assert!(view.select_adjacent_message(1));
        assert_eq!(view.selected_message_id, Some(view.list_messages[1].id));
        view.reveal_selected_message();
        let last = view.list_messages.len() - 1;
        view.selected_message_id = Some(view.list_messages[last].id);
        assert!(!view.select_adjacent_message(1));
    }

    #[test]
    fn attachments_follow_the_selected_message_body() {
        let (_dir, mut view) = test_view();
        let id = view.selected_message_id.expect("seed selects a message");
        let attachment = |part_index: u32, name: &str, inline: bool| storage::NewAttachment {
            part_index,
            filename: name.into(),
            mime_type: "application/pdf".into(),
            size: 2048,
            content_id: None,
            inline,
        };
        view.db
            .store_body(
                id,
                b"raw",
                "body",
                "text",
                "body",
                "body",
                &[
                    attachment(2, "a.pdf", false),
                    attachment(3, "logo.png", true),
                ],
            )
            .unwrap();
        view.reload_selected_detail();
        let names: Vec<_> = view
            .attachments
            .iter()
            .map(|attachment| attachment.filename.as_str())
            .collect();
        assert_eq!(names, ["a.pdf"]);

        // Headers-only bodies show no chips (they appear on BodyReady).
        let mut detail = view.selected_detail.clone().unwrap();
        detail.body_state = storage::BODY_STATE_HEADERS;
        assert!(visible_attachments(&view.db, Some(&detail)).is_empty());
        assert!(visible_attachments(&view.db, None).is_empty());

        view.clear_message_selection();
        assert!(view.attachments.is_empty());
        assert!(view.attachment_menu.is_none());
    }

    fn outgoing_row(raw: Vec<u8>, recipients: &[&str], state: OutgoingState) -> OutgoingRow {
        OutgoingRow {
            id: 42,
            account_id: 7,
            sender: "me@example.com".into(),
            recipients: recipients.iter().map(|r| r.to_string()).collect(),
            raw,
            message_id_hdr: None,
            attempts: 3,
            last_error: Some("  550 mailbox unavailable ".into()),
            created_at: 0,
            state,
            next_attempt_at: 0,
            deferrals: 0,
        }
    }

    #[test]
    fn unsent_item_summarizes_the_row() {
        let row = outgoing_row(
            Vec::new(),
            &["a@x.com", "b@y.com"],
            OutgoingState::MaybeSent,
        );
        let item = unsent_item(&row, "Hello".into());
        assert_eq!(
            item,
            UnsentItem {
                id: 42,
                account_id: 7,
                subject: "Hello".into(),
                recipients: "a@x.com, b@y.com".into(),
                error: Some("550 mailbox unavailable".into()),
                maybe_sent: true,
            }
        );
        let mut row = outgoing_row(Vec::new(), &[], OutgoingState::Failed);
        row.last_error = Some("   ".into());
        let item = unsent_item(&row, String::new());
        assert_eq!(item.error, None);
        assert!(!item.maybe_sent);
    }

    #[test]
    fn compose_init_from_outgoing_restores_the_draft() {
        let draft = Draft {
            from: Address::new(Some("Me"), "me@example.com"),
            to: vec![Address::new(Some("Ana"), "ana@x.com")],
            cc: vec![Address::new(None::<String>, "bob@y.com")],
            bcc: vec![Address::new(None::<String>, "hidden@z.com")],
            subject: "Report".into(),
            text: "See attached.".into(),
            in_reply_to: Some("orig@x.com".into()),
            references: vec!["orig@x.com".into()],
            attachments: vec![mail_core::OutgoingAttachment {
                filename: "a.txt".into(),
                mime_type: "text/plain".into(),
                data: b"hello".to_vec(),
            }],
            ..Draft::default()
        };
        let raw = mail_core::build_message(&draft, "id@example.com", 0);
        let recipients = mail_core::envelope_recipients(&draft);
        let recipients: Vec<&str> = recipients.iter().map(String::as_str).collect();
        let row = outgoing_row(raw, &recipients, OutgoingState::Failed);
        let parsed = mail_core::parse_message(&row.raw).unwrap();
        let init = compose_init_from_outgoing(&row, &parsed);
        assert_eq!(init.account_id, Some(7));
        assert_eq!(init.replaces_outgoing, Some(42));
        let emails = |list: &[Address]| -> Vec<String> {
            list.iter().map(|address| address.email.clone()).collect()
        };
        assert_eq!(emails(&init.draft.to), ["ana@x.com"]);
        assert_eq!(emails(&init.draft.cc), ["bob@y.com"]);
        assert_eq!(emails(&init.draft.bcc), ["hidden@z.com"]);
        assert_eq!(init.draft.from.email, "me@example.com");
        assert_eq!(init.draft.subject, "Report");
        assert_eq!(init.draft.text.trim(), "See attached.");
        assert_eq!(init.draft.in_reply_to.as_deref(), Some("orig@x.com"));
        assert_eq!(init.draft.attachments.len(), 1);
        assert_eq!(init.draft.attachments[0].filename, "a.txt");
        assert_eq!(init.draft.attachments[0].data, b"hello");
    }

    #[test]
    fn removing_an_account_hides_it_until_reload_clears_it() {
        let (_dir, mut view) = test_view();
        let account_id = view.accounts[0].id;
        let before = view.accounts.len();
        view.removing_accounts.insert(account_id);
        view.reload_from_database();
        assert_eq!(view.accounts.len(), before - 1);
        assert!(!view.folders_by_account.contains_key(&account_id));
        assert!(view
            .list_messages
            .iter()
            .all(|message| message.account_id != account_id));
        view.removing_accounts.remove(&account_id);
        view.reload_from_database();
        assert_eq!(view.accounts.len(), before);
    }
}
