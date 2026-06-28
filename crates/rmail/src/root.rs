//! Root view of the rMail visual prototype.
//!
//! Reproduces the macOS mail client layout (three columns, a unified top toolbar
//! and a status bar), using the components from the `ui` crate. All interaction
//! here is "mock" — only item selection, theme toggling and language switching.

use std::time::Duration;

use gpui::{
    canvas, AppContext as _, Context, DragMoveEvent, Empty, Entity, FontWeight, Hsla, MouseButton,
    MouseDownEvent, ScrollHandle, Window, WindowControlArea,
};
use theme::{ActiveTheme, Appearance};
use ui::prelude::*;
use ui::{
    Button, ButtonStyle, Color, Icon, IconButton, IconName, IconSize, Label, LabelSize, ListItem,
    Scrollbar, ScrollbarState,
};

use crate::data::{self, Account, MailboxKind, Message, MessageBody};
use crate::locale::{self, ActiveLanguage, Key, Language};
use crate::web_view::{email_document, EmailWebView, WEBVIEW_SUPPORTED};

/// Minimum width of the accounts/folders sidebar, in pixels.
const SIDEBAR_MIN_WIDTH: f32 = 250.0;
/// Minimum width of the message list, in pixels.
const LIST_MIN_WIDTH: f32 = 350.0;
/// Minimum width reserved for the reading pane (e-mail content), in pixels.
const READER_MIN_WIDTH: f32 = 400.0;
/// Below this window width the sidebar collapses automatically and, once
/// reopened, floats over the content instead of pushing the columns.
const NARROW_BREAKPOINT: f32 = 1100.0;
/// Hit area (width) of the draggable divider between two columns, in pixels.
const RESIZE_HANDLE_WIDTH: f32 = 6.0;
/// Width of the expanded search field in the toolbar, in pixels.
const SEARCH_FIELD_WIDTH: f32 = 220.0;
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

/// Currently displayed screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppView {
    Mail,
    Settings,
}

/// Active section of the settings screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsSection {
    General,
    Accounts,
    Appearance,
    Notifications,
}

impl SettingsSection {
    const ALL: [SettingsSection; 4] = [
        SettingsSection::General,
        SettingsSection::Accounts,
        SettingsSection::Appearance,
        SettingsSection::Notifications,
    ];

    fn title_key(self) -> Key {
        match self {
            SettingsSection::General => Key::SettingsGeneral,
            SettingsSection::Accounts => Key::SettingsAccounts,
            SettingsSection::Appearance => Key::SettingsAppearance,
            SettingsSection::Notifications => Key::SettingsNotifications,
        }
    }

    fn icon(self) -> IconName {
        match self {
            SettingsSection::General => IconName::Settings,
            SettingsSection::Accounts => IconName::Account,
            SettingsSection::Appearance => IconName::Star,
            SettingsSection::Notifications => IconName::Flag,
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
    }
}

/// Application state (mock).
pub struct RootView {
    accounts: Vec<Account>,
    messages: Vec<Message>,
    /// Selected (account index, mailbox index) in the sidebar.
    selected_mailbox: (usize, usize),
    /// Index of the message selected in the list.
    selected_message: usize,
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
    /// Set on mouse-down in the draggable toolbar; the actual window move only
    /// starts on the first subsequent mouse-move, so plain clicks on toolbar
    /// buttons still register.
    should_move: bool,
    view: AppView,
    settings_section: SettingsSection,
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
}

impl RootView {
    pub fn new() -> Self {
        Self {
            accounts: data::sample_accounts(),
            messages: data::sample_messages(),
            selected_mailbox: (0, 0),
            selected_message: 3,
            show_sidebar: true,
            sidebar_width: px(SIDEBAR_MIN_WIDTH),
            list_width: px(360.0),
            narrow: false,
            window_width: px(1100.0),
            should_move: false,
            view: AppView::Mail,
            settings_section: SettingsSection::Appearance,
            list_scroll: ScrollHandle::new(),
            list_scrollbar: None,
            sidebar_scroll: ScrollHandle::new(),
            sidebar_scrollbar: None,
            email_webview: None,
        }
    }

    /// Lazily creates the scrollbar state entities (needs an app context, which
    /// is only available at render time).
    fn ensure_scrollbar_states(&mut self, cx: &mut Context<Self>) {
        for slot in [&mut self.list_scrollbar, &mut self.sidebar_scrollbar] {
            slot.get_or_insert_with(|| cx.new(|_| ScrollbarState::new()));
        }
    }

    /// Creates (on first use) and updates the embedded webview to reflect the
    /// selected message and the current theme. Hides it when the reader is not
    /// on screen so it doesn't float over other views.
    fn sync_webview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let colors = cx.theme().colors();
        let document = email_document(
            colors.background,
            colors.text,
            colors.accent,
            &self.messages[self.selected_message].body,
        );

        match &mut self.email_webview {
            Some(webview) => webview.set_html(&document),
            None => self.email_webview = EmailWebView::new(window, &document),
        }

        if self.view != AppView::Mail {
            if let Some(webview) = &mut self.email_webview {
                webview.hide();
            }
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
        let list_segment = if self.sidebar_docked() && self.view == AppView::Mail {
            self.list_width
        } else {
            px(0.0)
        };
        self.window_width - sidebar_segment - list_segment
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
        if self.view != AppView::Mail {
            return 0;
        }
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
    fn render_list_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let language = cx.language();
        let (account_idx, mailbox_idx) = self.selected_mailbox;
        let mailbox = &self.accounts[account_idx].mailboxes[mailbox_idx];
        let list_title = mailbox.kind.display_name(language).to_string();
        let list_count = locale::message_count(language, self.messages.len());

        h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .child(
                v_flex()
                    .child(Label::new(list_title).weight(FontWeight::SEMIBOLD))
                    .child(
                        Label::new(list_count)
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(IconButton::new("filter", IconName::Filter))
                    .child(IconButton::new("more", IconName::More)),
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

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let bg = colors.title_bar_background;
        let border = colors.border;
        let search_bg = colors.element_background;
        let language = cx.language();
        let in_settings = self.view == AppView::Settings;

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
        let list_segment = (sidebar_docked && !in_settings).then(|| {
            h_flex()
                .w(self.list_width)
                .flex_shrink_0()
                .items_center()
                .px_3()
                .child(self.render_list_controls(cx))
        });

        // When the reader's toolbar segment gets too tight the full search field
        // would be clipped, so we collapse it into a single icon button.
        let compact_search = self.search_is_compact();

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
                    if event.click_count == 2 {
                        window.titlebar_double_click();
                    } else {
                        this.should_move = true;
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.should_move = false),
            )
            .on_mouse_down_out(cx.listener(|this, _, _, _| this.should_move = false))
            .on_mouse_move(cx.listener(|this, _, window, _| {
                if this.should_move {
                    this.should_move = false;
                    crate::window_drag::start_window_drag(window);
                }
            }))
            // Segment 1: over the sidebar — traffic-light spacing + sidebar toggle
            // and app settings (theme switching lives in Settings ▸ Appearance).
            .child(
                h_flex()
                    .w(sidebar_segment_width)
                    .flex_shrink_0()
                    .items_center()
                    .gap_1()
                    // 72px clears the traffic lights, +18px keeps the first button
                    // from crowding the window close button.
                    .pl(px(90.0))
                    .child(
                        IconButton::new("toggle-sidebar", IconName::Sidebar)
                            .selected(self.show_sidebar)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_sidebar();
                                cx.notify();
                            })),
                    )
                    .child(
                        IconButton::new("settings", IconName::Settings)
                            .selected(in_settings)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.view = if this.view == AppView::Settings {
                                    AppView::Mail
                                } else {
                                    AppView::Settings
                                };
                                cx.notify();
                            })),
                    ),
            )
            // Segment 2: over the message list — title + count on the left,
            // filter/more on the right. Only present while the sidebar is docked.
            .children(list_segment)
            // Segment 3: over the reader — compose, action groups, search, app controls.
            .child(
                h_flex()
                    .flex_1()
                    .items_center()
                    .gap_3()
                    .px_3()
                    // Left-aligned: compose.
                    .child(IconButton::new("compose", IconName::Compose).size(IconSize::Medium))
                    .child(div().flex_1())
                    // Centered action groups, divided like macOS Mail. Whole groups
                    // are dropped (right to left) as space shrinks so they never
                    // overlap the search button on the right.
                    .when(!in_settings, |el| {
                        let groups = self.visible_action_groups();
                        el.child(
                            h_flex()
                                .gap_2()
                                .when(groups >= 1, |cluster| {
                                    cluster.child(
                                        h_flex()
                                            .gap_1()
                                            .child(IconButton::new("reply", IconName::Reply))
                                            .child(IconButton::new("reply-all", IconName::ReplyAll))
                                            .child(IconButton::new("forward", IconName::Forward)),
                                    )
                                })
                                .when(groups >= 2, |cluster| {
                                    cluster.child(Self::render_toolbar_separator(border)).child(
                                        h_flex()
                                            .gap_1()
                                            .child(IconButton::new("trash", IconName::Trash))
                                            .child(IconButton::new("archive", IconName::Archive))
                                            .child(IconButton::new("junk", IconName::Junk)),
                                    )
                                })
                                .when(groups >= 3, |cluster| {
                                    cluster.child(Self::render_toolbar_separator(border)).child(
                                        h_flex()
                                            .gap_1()
                                            .child(Self::render_dropdown_button(
                                                "flag",
                                                IconName::Flag,
                                            ))
                                            .child(Self::render_dropdown_button(
                                                "move",
                                                IconName::Folder,
                                            )),
                                    )
                                }),
                        )
                    })
                    .child(div().flex_1())
                    .child(if compact_search {
                        IconButton::new("search", IconName::Search).into_any_element()
                    } else {
                        h_flex()
                            .w(px(SEARCH_FIELD_WIDTH))
                            .h(px(28.0))
                            .px_2()
                            .gap_1p5()
                            .items_center()
                            .rounded_md()
                            .bg(search_bg)
                            .child(
                                Icon::new(IconName::Search)
                                    .size(IconSize::Small)
                                    .color(Color::Muted),
                            )
                            .child(
                                Label::new(Key::SearchPlaceholder.tr(language))
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            )
                            .into_any_element()
                    }),
            )
    }

    /// A thin vertical divider used to separate toolbar button groups.
    fn render_toolbar_separator(color: Hsla) -> impl IntoElement {
        div().w(px(1.0)).h(px(20.0)).bg(color)
    }

    /// A toolbar control that pairs an icon with a small chevron to hint a
    /// dropdown menu (mirrors the "move to folder" / "flag" buttons in Mail).
    fn render_dropdown_button(id: &'static str, icon: IconName) -> impl IntoElement {
        h_flex()
            .items_center()
            .gap_0p5()
            .child(IconButton::new(id, icon))
            .child(
                Icon::new(IconName::ChevronDown)
                    .size(IconSize::XSmall)
                    .color(Color::Muted),
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

    fn render_account(
        &self,
        account_idx: usize,
        account: &Account,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let language = cx.language();
        let mut section = v_flex().px_2().pb_3().child(
            h_flex()
                .px_2()
                .py_1()
                .gap_2()
                .child(
                    Icon::new(IconName::ChevronDown)
                        .size(IconSize::XSmall)
                        .color(Color::Muted),
                )
                .child(
                    Label::new(account.name.clone())
                        .size(LabelSize::XSmall)
                        .color(Color::Muted)
                        .weight(FontWeight::SEMIBOLD),
                ),
        );

        for (mailbox_idx, mailbox) in account.mailboxes.iter().enumerate() {
            let selected = self.selected_mailbox == (account_idx, mailbox_idx);
            let badge = if mailbox.unread > 0 {
                Some(self.render_count_badge(mailbox.unread, selected, cx))
            } else {
                None
            };

            let mut item = ListItem::new(("mailbox", account_idx * 100 + mailbox_idx))
                .selected(selected)
                .start_slot(
                    Icon::new(mailbox_icon(mailbox.kind))
                        .size(IconSize::Small)
                        .color(if selected {
                            Color::Accent
                        } else {
                            Color::Muted
                        }),
                )
                .child(Label::new(mailbox.kind.display_name(language)).size(LabelSize::Small))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.selected_mailbox = (account_idx, mailbox_idx);
                    cx.notify();
                }));

            if let Some(badge) = badge {
                item = item.end_slot(badge);
            }

            section = section.child(item);
        }

        section
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

        h_flex()
            .px_1p5()
            .min_w(px(20.0))
            .justify_center()
            .rounded_full()
            .bg(bg)
            .text_size(px(11.0))
            .text_color(text)
            .child(count.to_string())
    }

    // ----- Message list ---------------------------------------------------

    fn render_message_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let bg = colors.surface_background;
        let border = colors.border;

        let mut content = v_flex()
            .id("message-list")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.list_scroll)
            .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                Self::note_scroll([this.list_scrollbar.clone()], cx);
            }));

        for (idx, message) in self.messages.iter().enumerate() {
            content = content.child(self.render_message_row(idx, message, cx));
        }

        let scrollbar = self
            .list_scrollbar
            .clone()
            .map(|state| Scrollbar::vertical(state, self.list_scroll.clone()));

        // When the sidebar isn't docked, the list controls that normally live in
        // the top toolbar move here, pinned to the top of the list.
        let header = (!self.sidebar_docked() && self.view == AppView::Mail).then(|| {
            h_flex()
                .w_full()
                .flex_shrink_0()
                .px_3()
                .py_2()
                .border_b_1()
                .border_color(border)
                .child(self.render_list_controls(cx))
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
        idx: usize,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors();
        let selected = self.selected_message == idx;
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
            Label::new(message.time.clone())
                .size(LabelSize::XSmall)
                .color(Color::Muted),
        );

        h_flex()
            .id(("message", idx))
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
                this.selected_message = idx;
                cx.notify();
            }))
    }

    // ----- Reading pane ---------------------------------------------------

    fn render_reader(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let bg = colors.background;
        let border = colors.border;
        let accent = colors.accent;
        let on_accent = colors.text_on_accent;

        let message = &self.messages[self.selected_message];
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
                Label::new(message.subject.clone())
                    .size(LabelSize::Large)
                    .bold(),
            )
            .child(
                h_flex()
                    .gap_2p5()
                    .items_center()
                    .child(
                        div()
                            .size(px(32.0))
                            .flex_shrink_0()
                            .rounded_full()
                            .bg(accent)
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(on_accent)
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
                        Label::new(message.time.clone())
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
    }

    /// Plain-text reader used where the embedded webview isn't available.
    fn render_text_fallback(&self, message: &Message, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let text = match &message.body {
            MessageBody::Text(plain) => plain.clone(),
            MessageBody::Html(_) => {
                SharedString::from("HTML preview is only available on macOS and Windows for now.")
            }
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
        let total_unread: usize = self
            .accounts
            .iter()
            .flat_map(|a| a.mailboxes.iter())
            .map(|m| m.unread)
            .sum();

        h_flex()
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
                Label::new(locale::status_counts(
                    language,
                    self.accounts.len(),
                    self.messages.len(),
                ))
                .size(LabelSize::XSmall)
                .color(Color::Muted),
            )
            .child(
                Label::new(locale::status_unread(language, total_unread))
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
    }

    // ----- Settings screen (Zed-style) ------------------------------------

    fn render_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
            let selected = self.settings_section == section;
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
                        this.settings_section = section;
                        cx.notify();
                    })),
            );
        }

        h_flex()
            .flex_1()
            .h_full()
            .bg(colors.background)
            .child(nav)
            .child(
                div()
                    .id("settings-content")
                    .flex_1()
                    .h_full()
                    .overflow_y_scroll()
                    .child(self.render_settings_content(cx)),
            )
    }

    fn render_settings_content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let section = self.settings_section;
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
            SettingsSection::Appearance => v_flex()
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
                .into_any_element(),
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

    /// Language selector used in the General settings section. Switching the
    /// language re-renders the whole UI through the locale global.
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
        // Make sure the scrollbar state entities exist before the panels render.
        self.ensure_scrollbar_states(cx);
        // Keep the embedded e-mail webview in sync with the selection and theme.
        self.sync_webview(window, cx);

        let background = cx.theme().colors().background;
        let text = cx.theme().colors().text;
        let scrim = cx.theme().colors().elevated_surface_background;

        // Reconcile responsive state with the live window width before laying out.
        self.sync_layout(window.viewport_size().width);

        let body = match self.view {
            AppView::Mail => {
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
                            cx.notify();
                        }),
                    );
                if docked_sidebar {
                    row = row.child(self.render_sidebar(false, cx));
                }
                row = row
                    .child(self.render_message_list(cx))
                    .child(self.render_reader(cx));

                if floating_sidebar {
                    // Wrap the columns so the sidebar can float on top. The scrim
                    // and the floating sidebar both occlude the mouse so hovering
                    // them never reaches the columns beneath.
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
                }
            }
            AppView::Settings => self.render_settings(cx).into_any_element(),
        };

        v_flex()
            .size_full()
            .bg(background)
            .text_color(text)
            .font_family("Helvetica")
            .child(self.render_toolbar(cx))
            .child(body)
            .child(self.render_status_bar(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidebar_starts_visible() {
        assert!(RootView::new().show_sidebar);
    }

    #[test]
    fn toggle_sidebar_flips_visibility() {
        let mut view = RootView::new();
        view.toggle_sidebar();
        assert!(!view.show_sidebar);
        view.toggle_sidebar();
        assert!(view.show_sidebar);
    }

    #[test]
    fn narrow_window_auto_collapses_sidebar() {
        let mut view = RootView::new();
        view.sync_layout(px(800.0));
        assert!(view.narrow);
        assert!(!view.show_sidebar);
    }

    #[test]
    fn resizing_while_narrow_recollapses_floating_sidebar() {
        let mut view = RootView::new();
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
        let mut view = RootView::new();
        view.sync_layout(px(850.0));
        view.show_sidebar = true;
        // Re-render at the same width (e.g. from an unrelated notify) keeps it.
        view.sync_layout(px(850.0));
        assert!(view.show_sidebar);
    }

    #[test]
    fn widening_window_restores_sidebar() {
        let mut view = RootView::new();
        view.sync_layout(px(800.0));
        assert!(!view.show_sidebar);
        view.sync_layout(px(1200.0));
        assert!(!view.narrow);
        assert!(view.show_sidebar);
    }

    #[test]
    fn resize_respects_sidebar_minimum() {
        let mut view = RootView::new();
        view.sync_layout(px(1400.0));
        view.resize(ResizeHandle::Sidebar, px(50.0), px(1400.0));
        assert_eq!(view.sidebar_width, px(SIDEBAR_MIN_WIDTH));
    }

    #[test]
    fn resize_respects_list_minimum() {
        let mut view = RootView::new();
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
        let mut view = RootView::new();
        view.sync_layout(total);
        // Try to make the list fill the whole row; the reader floor must hold.
        view.resize(ResizeHandle::List, total, total);
        let reader = total - view.sidebar_width - view.list_width;
        assert!(reader >= px(READER_MIN_WIDTH));
    }

    #[test]
    fn search_collapses_when_reader_segment_is_narrow() {
        let mut view = RootView::new();
        // Docked layout: reader segment = 1100 - 250 - 360 = 490, below the
        // collapse threshold, so the search field turns into an icon.
        view.sync_layout(px(1100.0));
        assert!(view.search_is_compact());
    }

    #[test]
    fn search_expands_on_wide_reader_segment() {
        let mut view = RootView::new();
        // Wide docked layout leaves the reader segment well above the threshold.
        view.sync_layout(px(1600.0));
        assert!(!view.search_is_compact());
    }

    #[test]
    fn all_action_groups_show_on_wide_window() {
        let mut view = RootView::new();
        view.sync_layout(px(1600.0));
        assert_eq!(view.visible_action_groups(), 3);
    }

    #[test]
    fn action_groups_drop_as_reader_segment_shrinks() {
        let mut view = RootView::new();
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
        let mut view = RootView::new();
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
        let mut view = RootView::new();
        // Wide (docked) window so the sidebar keeps its column; an oversized list
        // must shrink to preserve the reader's minimum width.
        view.list_width = px(900.0);
        view.sync_layout(px(1400.0));
        let reader = px(1400.0) - view.sidebar_width - view.list_width;
        assert!(reader >= px(READER_MIN_WIDTH));
        assert!(view.list_width >= px(LIST_MIN_WIDTH));
    }
}
