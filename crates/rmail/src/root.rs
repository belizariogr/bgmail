//! Root view of the rMail visual prototype.
//!
//! Reproduces the macOS mail client layout (three columns, a unified top toolbar
//! and a status bar), using the components from the `ui` crate. All interaction
//! here is "mock" — only item selection, theme toggling and language switching.

use std::time::Duration;

use gpui::{canvas, AppContext as _, Context, Entity, FontWeight, Hsla, ScrollHandle, Window};
use theme::{ActiveTheme, Appearance};
use ui::prelude::*;
use ui::{
    Button, ButtonStyle, Color, Icon, IconButton, IconName, IconSize, Label, LabelSize, ListItem,
    Scrollbar, ScrollbarState,
};

use crate::data::{self, Account, MailboxKind, Message, MessageBody};
use crate::locale::{self, ActiveLanguage, Key, Language};
use crate::web_view::{email_document, EmailWebView, WEBVIEW_SUPPORTED};

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
            selected_message: 0,
            show_sidebar: true,
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

    // ----- Top toolbar (unified title bar) --------------------------------

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let bg = colors.title_bar_background;
        let border = colors.border;
        let search_bg = colors.element_background;
        let language = cx.language();
        let in_settings = self.view == AppView::Settings;

        let (account_idx, mailbox_idx) = self.selected_mailbox;
        let mailbox = &self.accounts[account_idx].mailboxes[mailbox_idx];
        let list_title = mailbox.kind.display_name(language).to_string();
        let list_count = locale::message_count(language, self.messages.len());

        h_flex()
            .h(px(52.0))
            .w_full()
            .flex_shrink_0()
            .bg(bg)
            .border_b_1()
            .border_color(border)
            // Segment 1: over the sidebar — traffic-light spacing + sidebar toggle
            // and app settings (theme switching lives in Settings ▸ Appearance).
            .child(
                h_flex()
                    .w(px(240.0))
                    .flex_shrink_0()
                    .items_center()
                    .gap_1()
                    .pl(px(72.0))
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
            // Segment 2: over the message list — title + count on the left, filter/more on the right.
            .child(
                h_flex()
                    .w(px(360.0))
                    .flex_shrink_0()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .when(!in_settings, |el| {
                        el.child(
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
                    }),
            )
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
                    // Centered action groups, divided like macOS Mail.
                    .when(!in_settings, |el| {
                        el.child(
                            h_flex()
                                .gap_1()
                                .child(IconButton::new("reply", IconName::Reply))
                                .child(IconButton::new("reply-all", IconName::ReplyAll))
                                .child(IconButton::new("forward", IconName::Forward)),
                        )
                        .child(Self::render_toolbar_separator(border))
                        .child(
                            h_flex()
                                .gap_1()
                                .child(IconButton::new("trash", IconName::Trash))
                                .child(IconButton::new("archive", IconName::Archive))
                                .child(IconButton::new("junk", IconName::Junk)),
                        )
                        .child(Self::render_toolbar_separator(border))
                        .child(
                            h_flex()
                                .gap_1()
                                .child(Self::render_dropdown_button("flag", IconName::Flag))
                                .child(Self::render_dropdown_button("move", IconName::Folder)),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        h_flex()
                            .w(px(220.0))
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
                            ),
                    ),
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

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
            .w(px(240.0))
            .flex_shrink_0()
            .h_full()
            .bg(bg)
            .border_r_1()
            .border_color(border)
            .child(content)
            .children(scrollbar)
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

        div()
            .relative()
            .w(px(360.0))
            .flex_shrink_0()
            .h_full()
            .bg(bg)
            .border_r_1()
            .border_color(border)
            .child(content)
            .children(scrollbar)
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
            .py_4()
            .gap_3()
            .border_b_1()
            .border_color(border)
            .child(
                Label::new(message.subject.clone())
                    .size(LabelSize::Large)
                    .bold(),
            )
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .size(px(40.0))
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

        let body = match self.view {
            AppView::Mail => {
                let mut row = h_flex().flex_1().min_h_0().w_full();
                if self.show_sidebar {
                    row = row.child(self.render_sidebar(cx));
                }
                row.child(self.render_message_list(cx))
                    .child(self.render_reader(cx))
                    .into_any_element()
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
}
