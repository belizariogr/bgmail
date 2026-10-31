//! Settings window (General, Accounts, Appearance, Notifications, Privacy),
//! opened in its own window like Zed's preferences.

use gpui::{Context, FontWeight, IntoElement, SharedString, WeakEntity, Window};
use theme::{ActiveTheme, Appearance};
use ui::prelude::*;
use ui::{
    Button, ButtonStyle, Color, Icon, IconName, IconSize, Label, LabelSize, ListItem, Switch,
};

use crate::actions::ComposeClose;
use crate::data;
use crate::locale::{self, ActiveLanguage, Key, Language};
use crate::root::RootView;

/// Active section of the settings screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsSection {
    General,
    Accounts,
    Appearance,
    Notifications,
    Privacy,
}

impl SettingsSection {
    pub(crate) const ALL: [SettingsSection; 5] = [
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

/// Standalone preferences window content (Zed-style: settings open in their own
/// window). Theme and language are app-wide globals, so changes made here are
/// reflected in the main window immediately.
pub(crate) struct SettingsView {
    accounts: Vec<data::Account>,
    pub(crate) section: SettingsSection,
    /// Back-reference to the main view, used to read and toggle preferences that
    /// live there (e.g. loading remote images). Weak so the settings window can
    /// outlive nothing it shouldn't.
    root: WeakEntity<RootView>,
}

impl SettingsView {
    pub(crate) fn new(accounts: Vec<data::Account>, root: WeakEntity<RootView>) -> Self {
        Self {
            accounts,
            section: SettingsSection::General,
            root,
        }
    }

    /// Switches to `section` (e.g. "Add account" from the welcome screen).
    pub(crate) fn show_section(&mut self, section: SettingsSection, cx: &mut Context<Self>) {
        self.section = section;
        cx.notify();
    }

    pub(crate) fn close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
