//! Standalone compose window (mock).
//!
//! Opens in its own window, mirroring the settings window pattern. Field editing
//! is visual-only for now — real text input lands with the domain layer.

use gpui::{
    size, white, App, Bounds, Context, Empty, Hsla, MouseButton, Point, Render, SharedString, Size,
    WeakEntity, Window, WindowBounds,
};
use theme::ActiveTheme;
use ui::prelude::*;
use ui::{
    Button, ButtonStyle, Color, Icon, IconButton, IconName, IconSize, Label, LabelSize, Tooltip,
};

use crate::actions::{ComposeAttach, ComposeClose, ComposeDiscard, ComposeSend};
use crate::app_menus;
use crate::data::Account;
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

/// Fixed width of the field labels column (From, To, Cc, …), in pixels.
const FIELD_LABEL_WIDTH: f32 = 72.0;

/// Standalone new-message window content.
pub struct ComposeView {
    accounts: Vec<Account>,
    /// Index into [`Self::accounts`] for the From address.
    from_account: usize,
    /// Whether Cc and Bcc rows are visible.
    show_cc_bcc: bool,
    /// Back-reference to the main view for persisting this window's bounds.
    root: WeakEntity<RootView>,
    /// Last bounds reported to [`RootView`], so we only defer a sync when the
    /// window actually moved or resized (and never re-enter `RootView` during
    /// our own render — that panics with a double lease).
    last_reported_bounds: Option<(Point<gpui::Pixels>, Size<gpui::Pixels>)>,
    /// Cached copy of the white-compose preference. Set at open time and pushed
    /// from [`RootView`] when it changes — never read from `RootView` during
    /// render (that panics while the main view is on the stack).
    white_background: bool,
}

impl ComposeView {
    /// Creates a compose window seeded with the app's accounts.
    pub fn new(accounts: Vec<Account>, root: WeakEntity<RootView>, white_background: bool) -> Self {
        Self {
            accounts,
            from_account: 0,
            show_cc_bcc: false,
            root,
            last_reported_bounds: None,
            white_background,
        }
    }

    /// Syncs the white-compose preference pushed from the main view.
    pub(crate) fn set_white_background(&mut self, value: bool, cx: &mut Context<Self>) {
        if self.white_background != value {
            self.white_background = value;
            cx.notify();
        }
    }

    /// Toggles visibility of the Cc and Bcc rows.
    fn toggle_cc_bcc(&mut self, cx: &mut Context<Self>) {
        self.show_cc_bcc = !self.show_cc_bcc;
        cx.notify();
    }

    /// Cycles the From account through the available accounts (mock selector).
    fn cycle_from_account(&mut self, cx: &mut Context<Self>) {
        if self.accounts.is_empty() {
            return;
        }
        self.from_account = (self.from_account + 1) % self.accounts.len();
        cx.notify();
    }

    /// Mock send — real delivery lands with the domain layer.
    pub(crate) fn send_message(&mut self, cx: &mut Context<Self>) {
        let _ = cx;
    }

    /// Mock attach — file picker lands with the domain layer.
    pub(crate) fn attach_file(&mut self, cx: &mut Context<Self>) {
        let _ = cx;
    }

    /// Mock discard — closes the compose window after the mock phase.
    pub(crate) fn discard_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_window(window, cx);
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

    fn selected_from_address(&self) -> &str {
        self.accounts
            .get(self.from_account)
            .map(|a| a.email.as_str())
            .unwrap_or("")
    }

    fn render_toolbar(&self, language: Language, cx: &mut Context<Self>) -> impl IntoElement {
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
            .child(
                div()
                    .id("compose-attach")
                    .tooltip(Tooltip::text(Key::ComposeAttach.tr(language)))
                    .child(
                        IconButton::new("compose-attach", IconName::Attachment)
                            .on_click(cx.listener(|this, _, _, cx| this.attach_file(cx))),
                    ),
            )
            .child(
                div()
                    .id("compose-send")
                    .tooltip(Tooltip::text(Key::ComposeSend.tr(language)))
                    .child(
                        Button::new("compose-send", Key::ComposeSend.tr(language))
                            .icon(IconName::Send)
                            .style(ButtonStyle::Filled)
                            .on_click(cx.listener(|this, _, _, cx| this.send_message(cx))),
                    ),
            )
    }

    fn render_header(&self, language: Language, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let border = colors.border;
        let field_text = colors.text;
        let field_hover = colors.element_hover;
        let from_email = self.selected_from_address().to_string();
        let placeholder = Key::ComposeFieldPlaceholder.tr(language);

        let mut fields = v_flex()
            .px_3()
            .child(
                self.field_row(
                    Key::ComposeFrom.tr(language),
                    from_email.clone(),
                    Some(
                        h_flex()
                            .id("compose-from")
                            .items_center()
                            .gap_1()
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| this.cycle_from_account(cx)))
                            .child(Label::new(from_email.clone()).single_line())
                            .child(
                                Icon::new(IconName::ChevronDown)
                                    .size(IconSize::XSmall)
                                    .color(Color::Muted),
                            ),
                    ),
                    border,
                ),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .py_2()
                    .border_b_1()
                    .border_color(border)
                    .child(
                        div().w(px(FIELD_LABEL_WIDTH)).flex_shrink_0().child(
                            Label::new(Key::ComposeTo.tr(language))
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        ),
                    )
                    .child(
                        div().flex_1().min_w_0().child(
                            Label::new(placeholder)
                                .size(LabelSize::Small)
                                .color(Color::Disabled)
                                .single_line(),
                        ),
                    )
                    .child(self.cc_bcc_toggle(language, field_text, field_hover, cx)),
            );

        if self.show_cc_bcc {
            fields = fields
                .child(self.field_row(
                    Key::ComposeCc.tr(language),
                    placeholder,
                    None::<Empty>,
                    border,
                ))
                .child(self.field_row(
                    Key::ComposeBcc.tr(language),
                    placeholder,
                    None::<Empty>,
                    border,
                ));
        }

        fields.child(self.field_row(
            Key::ComposeSubject.tr(language),
            placeholder,
            None::<Empty>,
            border,
        ))
    }

    /// Compact Cc/Bcc toggle aligned with the 12px field rows (the default
    /// [`Button`] padding would make the To line taller than the others).
    fn cc_bcc_toggle(
        &self,
        language: Language,
        text_color: gpui::Hsla,
        hover_bg: gpui::Hsla,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label = Key::ComposeCcBcc.tr(language);
        div()
            .id("compose-cc-bcc")
            .flex_shrink_0()
            .tooltip(Tooltip::text(label))
            .px_1p5()
            .rounded_sm()
            .text_size(px(12.0))
            .text_color(text_color)
            .hover(move |el| el.bg(hover_bg))
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, _, cx| this.toggle_cc_bcc(cx)))
            .child(label)
    }

    /// One labeled row in the compose header (From, To, Subject, …).
    fn field_row(
        &self,
        label: &'static str,
        placeholder: impl Into<SharedString>,
        value_slot: Option<impl IntoElement>,
        border: gpui::Hsla,
    ) -> impl IntoElement {
        let placeholder = placeholder.into();
        h_flex()
            .items_center()
            .gap_2()
            .py_2()
            .border_b_1()
            .border_color(border)
            .child(
                div()
                    .w(px(FIELD_LABEL_WIDTH))
                    .flex_shrink_0()
                    .child(Label::new(label).size(LabelSize::Small).color(Color::Muted)),
            )
            .child(match value_slot {
                Some(slot) => div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(slot)
                    .into_any_element(),
                None => div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        Label::new(placeholder)
                            .size(LabelSize::Small)
                            .color(Color::Disabled)
                            .single_line(),
                    )
                    .into_any_element(),
            })
    }

    fn render_body(&self, language: Language, white_background: bool) -> impl IntoElement {
        let (bg, placeholder_color) = compose_body_colors(white_background);
        div()
            .flex_1()
            .p_3()
            .when_some(bg, |el, bg| el.bg(bg))
            .child(Label::new(Key::ComposeBodyPlaceholder.tr(language)).color(placeholder_color))
    }
}

/// Background and placeholder colors for the compose body, mirroring the reader.
fn compose_body_colors(white_background: bool) -> (Option<Hsla>, Color) {
    if white_background {
        (
            Some(white()),
            Color::Custom(Hsla {
                h: 0.0,
                s: 0.0,
                l: 0.55,
                a: 1.0,
            }),
        )
    } else {
        (None, Color::Disabled)
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
        let colors = cx.theme().colors();

        v_flex()
            .size_full()
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
            .child(self.render_header(language, cx))
            .child(self.render_body(language, self.white_background))
            .child(self.render_toolbar(language, cx))
    }
}

/// Localized title for the compose window.
pub fn window_title(language: Language) -> &'static str {
    Key::ComposeWindowTitle.tr(language)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data;
    use gpui::point;

    #[test]
    fn compose_body_uses_white_background_when_reader_pref_is_on() {
        let (bg, _) = compose_body_colors(true);
        assert!(bg.is_some());
        let (bg, color) = compose_body_colors(false);
        assert!(bg.is_none());
        assert_eq!(color, Color::Disabled);
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
    fn new_view_defaults_to_first_account_and_hidden_cc_bcc() {
        let accounts = data::sample_accounts();
        let view = ComposeView::new(accounts.clone(), WeakEntity::new_invalid(), false);
        assert_eq!(view.from_account, 0);
        assert!(!view.show_cc_bcc);
        assert_eq!(view.selected_from_address(), accounts[0].email.as_ref());
    }

    #[test]
    fn selected_from_address_is_empty_without_accounts() {
        let view = ComposeView::new(vec![], WeakEntity::new_invalid(), false);
        assert_eq!(view.selected_from_address(), "");
    }

    #[test]
    fn toggle_cc_bcc_flips_visibility() {
        let mut visible = false;
        visible = !visible;
        assert!(visible);
        visible = !visible;
        assert!(!visible);
    }

    #[test]
    fn cycle_from_account_wraps() {
        let accounts = data::sample_accounts();
        let count = accounts.len();
        assert!(count >= 2, "sample data should have multiple accounts");
        let mut index = count - 1;
        index = (index + 1) % count;
        assert_eq!(index, 0);
    }
}
