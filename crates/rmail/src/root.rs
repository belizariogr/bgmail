//! View raiz do protótipo visual do rMail.
//!
//! Reproduz o layout do cliente de e-mail do macOS (três colunas + toolbar
//! superior unificada + barra de status), usando os componentes do crate `ui`.
//! Toda a interação aqui é "mock" — apenas seleção de itens e troca de tema.

use gpui::{Context, FontWeight, Window};
use theme::{ActiveTheme, Appearance};
use ui::prelude::*;
use ui::{
    Button, ButtonStyle, Color, Icon, IconButton, IconName, IconSize, Label, LabelSize, ListItem,
};

use crate::data::{self, Account, MailboxKind, Message};

/// Tela atualmente exibida.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppView {
    Mail,
    Settings,
}

/// Seção ativa da tela de configurações.
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

    fn title(self) -> &'static str {
        match self {
            SettingsSection::General => "Geral",
            SettingsSection::Accounts => "Contas",
            SettingsSection::Appearance => "Aparência",
            SettingsSection::Notifications => "Notificações",
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

/// Estado da aplicação (mock).
pub struct RootView {
    accounts: Vec<Account>,
    messages: Vec<Message>,
    /// (índice da conta, índice da caixa) selecionada na barra lateral.
    selected_mailbox: (usize, usize),
    /// Índice da mensagem selecionada na lista.
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

    // ----- Toolbar superior (barra de título unificada) -------------------

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let bg = colors.title_bar_background;
        let border = colors.border;
        let appearance = cx.theme().appearance();
        let theme_label = if appearance == Appearance::Dark {
            "Escuro"
        } else {
            "Claro"
        };
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
            // Reserva espaço para os semáforos do macOS à esquerda.
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

    // ----- Barra lateral (contas e caixas) --------------------------------

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
                .child(Label::new(mailbox.name.clone()).size(LabelSize::Small))
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

    // ----- Lista de mensagens ---------------------------------------------

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

        // Cabeçalho da lista com o nome da caixa.
        list = list.child(
            h_flex()
                .px_3()
                .py_2()
                .justify_between()
                .border_b_1()
                .border_color(border)
                .child(Label::new(mailbox.name.clone()).weight(FontWeight::SEMIBOLD))
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

        // Indicador de não-lida (ponto azul) ou espaço equivalente.
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

    // ----- Painel de leitura ----------------------------------------------

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
            // Cabeçalho da mensagem.
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
            // Corpo da mensagem.
            .child(
                div()
                    .px_6()
                    .py_4()
                    .text_size(px(14.0))
                    .text_color(colors.text)
                    .child(message.body.clone()),
            )
    }

    // ----- Barra de status ------------------------------------------------

    fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
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
                Label::new(format!(
                    "{} contas · {} mensagens",
                    self.accounts.len(),
                    self.messages.len()
                ))
                .size(LabelSize::XSmall)
                .color(Color::Muted),
            )
            .child(
                Label::new(format!("{total_unread} não lidas · Atualizado agora"))
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
    }

    // ----- Tela de configurações (estilo Zed) -----------------------------

    fn render_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();

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
                div()
                    .px_2()
                    .py_2()
                    .child(Label::new("Configurações").size(LabelSize::Large).bold()),
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
                    .child(Label::new(section.title()).size(LabelSize::Small))
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

        let content = match section {
            SettingsSection::General => v_flex()
                .gap_2()
                .child(settings_row("Nome do app", "rMail"))
                .child(settings_row("Versão", env!("CARGO_PKG_VERSION")))
                .child(settings_row("Idioma", "Português (Brasil)"))
                .into_any_element(),
            SettingsSection::Accounts => {
                let mut list = v_flex().gap_2();
                for account in &self.accounts {
                    list = list.child(settings_row(account.name.clone(), account.email.clone()));
                }
                list.child(
                    Button::new("add-account", "Adicionar conta…").style(ButtonStyle::Filled),
                )
                .into_any_element()
            }
            SettingsSection::Appearance => v_flex()
                .gap_3()
                .child(Label::new("Tema").weight(FontWeight::SEMIBOLD))
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("theme-light", "Claro")
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
                            Button::new("theme-dark", "Escuro")
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
                .child(settings_row("Notificações na área de trabalho", "Ativadas"))
                .child(settings_row("Som ao receber e-mail", "Desativado"))
                .into_any_element(),
        };

        v_flex()
            .p_6()
            .gap_4()
            .child(Label::new(section.title()).size(LabelSize::Large).bold())
            .child(content)
    }
}

/// Linha "rótulo → valor" usada nas configurações.
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
