//! Root view of the rMail visual prototype.
//!
//! Reproduces the macOS mail client layout (three columns, a unified top toolbar
//! and a status bar), using the components from the `ui` crate. All interaction
//! here is "mock" — only item selection, theme toggling and language switching.

use gpui::{Context, FontWeight, Window};
use theme::{ActiveTheme, Appearance};
use ui::prelude::*;
use ui::{
    Button, ButtonStyle, Color, Icon, IconButton, IconName, IconSize, Label, LabelSize, ListItem,
};

use crate::data::{self, Account, MailboxKind, Message};
use crate::locale::{self, ActiveLanguage, Key, Language};

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
    view: AppView,
    settings_section: SettingsSection,
}

impl RootView {
    pub fn new() -> Self {
        Self {
            accounts: data::sample_accounts(),
            messages: data::sample_messages(),
            selected_mailbox: (0, 0),
            selected_message: 0,
            view: AppView::Mail,
            settings_section: SettingsSection::Appearance,
        }
    }

    // ----- Top toolbar (unified title bar) --------------------------------

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let bg = colors.title_bar_background;
        let border = colors.border;
        let appearance = cx.theme().appearance();
        let theme_label = if appearance == Appearance::Dark {
            Key::ThemeDark
        } else {
            Key::ThemeLight
        }
        .tr(cx.language());
        let in_settings = self.view == AppView::Settings;

        h_flex()
            .h(px(48.0))
            .w_full()
            .flex_shrink_0()
            .px_3()
            .gap_2()
            .bg(bg)
            .border_b_1()
            .border_color(border)
            .justify_between()
            // Reserve space for the macOS traffic lights on the left.
            .child(
                h_flex()
                    .gap_1()
                    .pl(px(72.0))
                    .child(
                        IconButton::new("compose", IconName::Compose)
                            .size(IconSize::Medium)
                            .on_click(cx.listener(|_, _, _, cx| cx.notify())),
                    )
                    .child(
                        IconButton::new("refresh", IconName::Refresh)
                            .on_click(cx.listener(|_, _, _, cx| cx.notify())),
                    ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .when(!in_settings, |el| {
                        el.child(IconButton::new("reply", IconName::Reply))
                            .child(IconButton::new("reply-all", IconName::ReplyAll))
                            .child(IconButton::new("forward", IconName::Forward))
                            .child(IconButton::new("archive", IconName::Archive))
                            .child(IconButton::new("flag", IconName::Flag))
                            .child(IconButton::new("trash", IconName::Trash))
                    })
                    .child(
                        Button::new("toggle-theme", theme_label)
                            .style(ButtonStyle::Subtle)
                            .on_click(cx.listener(|_, _, _, cx| {
                                theme::toggle_appearance(cx);
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
    }

    // ----- Sidebar (accounts and mailboxes) -------------------------------

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let bg = colors.panel_background;
        let border = colors.border;

        let mut sidebar = v_flex()
            .id("sidebar")
            .w(px(240.0))
            .flex_shrink_0()
            .h_full()
            .bg(bg)
            .border_r_1()
            .border_color(border)
            .overflow_y_scroll()
            .pt_2();

        for (account_idx, account) in self.accounts.iter().enumerate() {
            sidebar = sidebar.child(self.render_account(account_idx, account, cx));
        }

        sidebar
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
        let (_, mailbox_idx) = self.selected_mailbox;
        let mailbox = &self.accounts[self.selected_mailbox.0].mailboxes[mailbox_idx];

        let mut list = v_flex()
            .id("message-list")
            .w(px(360.0))
            .flex_shrink_0()
            .h_full()
            .bg(bg)
            .border_r_1()
            .border_color(border)
            .overflow_y_scroll();

        // List header with the mailbox name.
        list = list.child(
            h_flex()
                .px_3()
                .py_2()
                .justify_between()
                .border_b_1()
                .border_color(border)
                .child(
                    Label::new(mailbox.kind.display_name(cx.language()))
                        .weight(FontWeight::SEMIBOLD),
                )
                .child(
                    h_flex().gap_1().child(
                        Icon::new(IconName::Search)
                            .size(IconSize::Small)
                            .color(Color::Muted),
                    ),
                ),
        );

        for (idx, message) in self.messages.iter().enumerate() {
            list = list.child(self.render_message_row(idx, message, cx));
        }

        list
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

        v_flex()
            .id("reader")
            .flex_1()
            .h_full()
            .bg(bg)
            .overflow_y_scroll()
            // Message header.
            .child(
                v_flex()
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
                    ),
            )
            // Message body.
            .child(
                div()
                    .px_6()
                    .py_4()
                    .text_size(px(14.0))
                    .text_color(colors.text)
                    .child(message.body.clone()),
            )
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let background = cx.theme().colors().background;
        let text = cx.theme().colors().text;

        let body = match self.view {
            AppView::Mail => h_flex()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(self.render_sidebar(cx))
                .child(self.render_message_list(cx))
                .child(self.render_reader(cx))
                .into_any_element(),
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
