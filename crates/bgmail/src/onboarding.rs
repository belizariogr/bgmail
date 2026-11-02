//! First-run setup shown in the main window: welcome, theme, account, conclusion.
//!
//! The account step embeds the same form as Settings ▸ Accounts (a
//! [`SettingsView`] in its form-only mode), so discovery, Google sign-in and the
//! login test have a single implementation.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use gpui::{
    ease_in_out, img, linear_color_stop, linear_gradient, svg, Animation, AnimationExt, Background,
    Entity, FontWeight, Image, ImageFormat, ObjectFit, SharedString, Subscription, WeakEntity,
};
use theme::{ActiveTheme, Appearance, Theme, ThemeColors};
use ui::prelude::*;
use ui::{Button, ButtonStyle, Color, Icon, IconName, IconSize, Label, LabelSize, Switch};

use crate::default_mail_app::{self, Status};
use crate::locale::{ActiveLanguage, Key};
use crate::root::RootView;
use crate::settings::{self, SettingsView};

const LOGO_PNG: &[u8] = include_bytes!("../../../assets/appIcon/BGMail-256.png");
const STEP_FADE: Duration = Duration::from_millis(280);
const CHECK_POP: Duration = Duration::from_millis(420);
/// How far a new step slides up while fading in.
const STEP_SLIDE: f32 = 10.0;
const CONTENT_WIDTH: f32 = 640.0;

/// The screens of the setup, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OnboardingStep {
    Welcome,
    Theme,
    Account,
    Done,
}

impl OnboardingStep {
    pub(crate) const ALL: [OnboardingStep; 4] = [
        OnboardingStep::Welcome,
        OnboardingStep::Theme,
        OnboardingStep::Account,
        OnboardingStep::Done,
    ];

    /// Position in [`Self::ALL`].
    fn index(self) -> usize {
        self as usize
    }

    /// The following screen; the last one has none.
    fn next(self) -> Option<Self> {
        Self::ALL.get(self.index() + 1).copied()
    }

    /// The preceding screen; the first one has none.
    fn previous(self) -> Option<Self> {
        self.index().checked_sub(1).map(|index| Self::ALL[index])
    }
}

/// Only a profile that never finished the setup and has no account sees it, so
/// people updating from a version without it go straight to their mail.
pub(crate) fn needs_onboarding(completed: bool, account_count: usize) -> bool {
    !completed && account_count == 0
}

/// What the final button does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FinishAction {
    Close,
    ApplyDefaultApp,
}

/// The system is only asked once: if that didn't settle it, the message is on
/// screen and the next click must leave instead of asking again.
fn finish_action(wants_default: bool, status: Option<Status>, attempted: bool) -> FinishAction {
    if wants_default && !attempted && status != Some(Status::IsDefault) {
        FinishAction::ApplyDefaultApp
    } else {
        FinishAction::Close
    }
}

/// Eases past the end and settles back, so the final check mark pops in.
fn overshoot(t: f32) -> f32 {
    const BACK: f32 = 1.70158;
    let t = t - 1.0;
    1.0 + (BACK + 1.0) * t * t * t + BACK * t * t
}

fn logo() -> Arc<Image> {
    static LOGO: OnceLock<Arc<Image>> = OnceLock::new();
    LOGO.get_or_init(|| Arc::new(Image::from_bytes(ImageFormat::Png, LOGO_PNG.to_vec())))
        .clone()
}

/// The soft accent glow behind the setup, fading into the theme's page color.
pub(crate) fn page_background(colors: &ThemeColors) -> Background {
    linear_gradient(
        160.0,
        linear_color_stop(colors.background.blend(colors.accent.opacity(0.16)), 0.0),
        linear_color_stop(colors.background, 0.55),
    )
}

pub(crate) struct OnboardingView {
    root: WeakEntity<RootView>,
    step: OnboardingStep,
    account_form: Entity<SettingsView>,
    has_account: bool,
    default_status: Option<Status>,
    default_status_requested: bool,
    make_default: bool,
    default_attempted: bool,
    applying_default: bool,
    default_message: Option<SharedString>,
    _root_observation: Subscription,
}

impl OnboardingView {
    /// `root` isn't read here: this runs while the main view is rendering.
    pub(crate) fn new(root: Entity<RootView>, cx: &mut Context<Self>) -> Self {
        let weak_root = root.downgrade();
        let form_root = weak_root.clone();
        Self {
            root: weak_root,
            step: OnboardingStep::Welcome,
            account_form: cx.new(|_| SettingsView::form_only(form_root)),
            has_account: false,
            default_status: None,
            default_status_requested: false,
            make_default: false,
            default_attempted: false,
            applying_default: false,
            default_message: None,
            _root_observation: cx.observe(&root, |this, root, cx| {
                let has_account = !root.read(cx).accounts().is_empty();
                this.on_accounts_changed(has_account, cx);
            }),
        }
    }

    /// The first account finishing its connection moves on by itself.
    fn on_accounts_changed(&mut self, has_account: bool, cx: &mut Context<Self>) {
        let first_account = has_account && !self.has_account;
        self.has_account = has_account;
        if first_account && self.step == OnboardingStep::Account {
            self.go_to(OnboardingStep::Done, cx);
        }
    }

    fn go_to(&mut self, step: OnboardingStep, cx: &mut Context<Self>) {
        self.step = step;
        if step == OnboardingStep::Done && !self.default_status_requested {
            self.request_default_status(cx);
        }
        cx.notify();
    }

    /// Asks off the UI thread: it means running a command.
    fn request_default_status(&mut self, cx: &mut Context<Self>) {
        self.default_status_requested = true;
        let status = cx.background_spawn(async { default_mail_app::status() });
        cx.spawn(async move |this, cx| {
            let status = status.await;
            this.update(cx, |this, cx| {
                this.default_status = Some(status);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn finish(&mut self, cx: &mut Context<Self>) {
        if self.applying_default {
            return;
        }
        match finish_action(
            self.make_default,
            self.default_status,
            self.default_attempted,
        ) {
            FinishAction::Close => self.close(cx),
            FinishAction::ApplyDefaultApp => self.apply_default_app(cx),
        }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.root
            .update(cx, |root, cx| root.finish_onboarding(cx))
            .ok();
    }

    /// Stays on screen when the system has something to say about it (not
    /// installed, finish in the system settings); otherwise goes to the mail.
    fn apply_default_app(&mut self, cx: &mut Context<Self>) {
        self.applying_default = true;
        self.default_attempted = true;
        self.default_message = None;
        let outcome = cx.background_spawn(async { default_mail_app::set_default() });
        cx.spawn(async move |this, cx| {
            let outcome = outcome.await;
            this.update(cx, |this, cx| {
                this.applying_default = false;
                this.default_message = settings::default_app_message(&outcome, cx.language());
                if this.default_message.is_none() {
                    this.close(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn render_welcome(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let language = cx.language();
        v_flex()
            .items_center()
            .gap_6()
            .child(img(logo()).size(px(104.0)).object_fit(ObjectFit::Contain))
            .child(step_heading(
                Key::OnboardingWelcomeTitle.tr(language),
                Key::OnboardingWelcomeBody.tr(language),
                34.0,
            ))
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .items_start()
                    .child(feature_card(
                        IconName::Send,
                        Key::OnboardingFastTitle.tr(language),
                        Key::OnboardingFastBody.tr(language),
                        cx,
                    ))
                    .child(feature_card(
                        IconName::Inbox,
                        Key::OnboardingAccountsTitle.tr(language),
                        Key::OnboardingAccountsBody.tr(language),
                        cx,
                    ))
                    .child(feature_card(
                        IconName::Shield,
                        Key::OnboardingPrivateTitle.tr(language),
                        Key::OnboardingPrivateBody.tr(language),
                        cx,
                    )),
            )
            .child(settings::language_picker(
                "onboarding-language",
                self.root.clone(),
                language,
            ))
    }

    fn render_theme(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let language = cx.language();
        v_flex()
            .items_center()
            .gap_6()
            .child(step_heading(
                Key::OnboardingThemeTitle.tr(language),
                Key::OnboardingThemeBody.tr(language),
                28.0,
            ))
            .child(
                h_flex()
                    .w_full()
                    .gap_4()
                    .child(self.render_theme_card(Appearance::Light, cx))
                    .child(self.render_theme_card(Appearance::Dark, cx)),
            )
    }

    /// Clicking applies the theme at once: the setup itself is the big preview.
    fn render_theme_card(
        &self,
        appearance: Appearance,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = *cx.theme().colors();
        let language = cx.language();
        let selected = cx.theme().appearance() == appearance;
        let (id, label) = match appearance {
            Appearance::Light => ("onboarding-theme-light", Key::ThemeLight),
            Appearance::Dark => ("onboarding-theme-dark", Key::ThemeDark),
        };
        v_flex()
            .id(id)
            .flex_1()
            .min_w_0()
            .gap_2()
            .p_2()
            .rounded_xl()
            .border_2()
            .border_color(if selected {
                colors.accent
            } else {
                colors.border_variant
            })
            .bg(colors.surface_background)
            .cursor_pointer()
            .hover(|style| style.border_color(colors.accent.opacity(0.6)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.root
                    .update(cx, |root, cx| root.set_appearance(appearance, cx))
                    .ok();
                cx.notify();
            }))
            .child(theme_preview(Theme::for_appearance(appearance).colors()))
            .child(
                h_flex()
                    .justify_between()
                    .px_1()
                    .pb_0p5()
                    .child(Label::new(label.tr(language)).weight(FontWeight::SEMIBOLD))
                    .when(selected, |row| {
                        row.child(
                            Icon::new(IconName::Check)
                                .size(IconSize::Medium)
                                .color(Color::Accent),
                        )
                    }),
            )
    }

    fn render_account(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let language = cx.language();
        let colors = cx.theme().colors();
        let connected = self
            .root
            .upgrade()
            .and_then(|root| root.read(cx).accounts().first().cloned());
        v_flex()
            .items_center()
            .gap_6()
            .child(step_heading(
                Key::OnboardingAccountTitle.tr(language),
                Key::OnboardingAccountBody.tr(language),
                28.0,
            ))
            .child(match connected {
                Some(account) => h_flex()
                    .w_full()
                    .gap_3()
                    .p_4()
                    .rounded_lg()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.surface_background)
                    .child(
                        Icon::new(IconName::Check)
                            .size(IconSize::Medium)
                            .color(Color::Accent),
                    )
                    .child(
                        v_flex()
                            .min_w_0()
                            .child(
                                Label::new(Key::OnboardingAccountConnected.tr(language))
                                    .weight(FontWeight::SEMIBOLD),
                            )
                            .child(
                                Label::new(account.email)
                                    .size(LabelSize::Small)
                                    .color(Color::Muted)
                                    .single_line(),
                            ),
                    )
                    .into_any_element(),
                None => div()
                    .w_full()
                    .child(self.account_form.clone())
                    .into_any_element(),
            })
    }

    fn render_done(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let language = cx.language();
        let colors = cx.theme().colors();
        let body = if self.has_account {
            Key::OnboardingDoneBodyAccount
        } else {
            Key::OnboardingDoneBodyNoAccount
        };
        let make_default = self.make_default;
        let offer_default = self
            .default_status
            .is_some_and(|status| status != Status::IsDefault);
        v_flex()
            .items_center()
            .gap_6()
            .child(
                div()
                    .size(px(96.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(colors.accent)
                            .child(
                                svg()
                                    .path(ui::CHECK)
                                    .size(px(36.0))
                                    .text_color(colors.text_on_accent),
                            )
                            .with_animation(
                                "onboarding-done-check",
                                Animation::new(CHECK_POP),
                                |el, delta| el.size(px(72.0 * overshoot(delta))),
                            ),
                    ),
            )
            .child(step_heading(
                Key::OnboardingDoneTitle.tr(language),
                body.tr(language),
                34.0,
            ))
            .when(offer_default, |column| {
                column.child(
                    v_flex()
                        .w_full()
                        .gap_2()
                        .p_4()
                        .rounded_lg()
                        .border_1()
                        .border_color(colors.border)
                        .bg(colors.surface_background)
                        .child(
                            Switch::new("onboarding-default-app", make_default)
                                .label(Key::OnboardingDefaultApp.tr(language))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.make_default = !make_default;
                                    this.default_attempted = false;
                                    this.default_message = None;
                                    cx.notify();
                                })),
                        )
                        .child(
                            Label::new(Key::OnboardingDefaultAppHint.tr(language))
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        )
                        .children(self.default_message.clone().map(|message| {
                            Label::new(message)
                                .size(LabelSize::Small)
                                .color(Color::Muted)
                        })),
                )
            })
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let language = cx.language();
        let step = self.step;
        let primary = match step {
            OnboardingStep::Welcome => {
                Button::new("onboarding-next", Key::OnboardingGetStarted.tr(language))
                    .style(ButtonStyle::Filled)
            }
            OnboardingStep::Theme => {
                Button::new("onboarding-next", Key::OnboardingContinue.tr(language))
                    .style(ButtonStyle::Filled)
            }
            OnboardingStep::Account if self.has_account => {
                Button::new("onboarding-next", Key::OnboardingContinue.tr(language))
                    .style(ButtonStyle::Filled)
            }
            OnboardingStep::Account => {
                Button::new("onboarding-next", Key::OnboardingSkip.tr(language))
                    .style(ButtonStyle::Subtle)
            }
            OnboardingStep::Done => {
                Button::new("onboarding-finish", Key::OnboardingFinish.tr(language)).style(
                    if self.applying_default {
                        ButtonStyle::Subtle
                    } else {
                        ButtonStyle::Filled
                    },
                )
            }
        }
        .on_click(cx.listener(move |this, _, _, cx| match step.next() {
            Some(next) => this.go_to(next, cx),
            None => this.finish(cx),
        }));

        h_flex()
            .w_full()
            .flex_shrink_0()
            .px_6()
            .pb_5()
            .pt_3()
            .items_center()
            .child(
                h_flex()
                    .flex_1()
                    .when_some(step.previous(), |el, previous| {
                        el.child(
                            Button::new("onboarding-back", Key::OnboardingBack.tr(language))
                                .style(ButtonStyle::Ghost)
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.go_to(previous, cx)),
                                ),
                        )
                    }),
            )
            .child(step_dots(step, cx))
            .child(h_flex().flex_1().justify_end().child(primary))
    }
}

impl Render for OnboardingView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.step {
            OnboardingStep::Welcome => self.render_welcome(cx).into_any_element(),
            OnboardingStep::Theme => self.render_theme(cx).into_any_element(),
            OnboardingStep::Account => self.render_account(cx).into_any_element(),
            OnboardingStep::Done => self.render_done(cx).into_any_element(),
        };
        v_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(
                // The spacers center short steps; unlike centering the column, a
                // form taller than the window still scrolls from its top.
                v_flex()
                    .id("onboarding-scroll")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .items_center()
                    .px_6()
                    .overflow_y_scroll()
                    .child(div().flex_1().min_h(px(8.0)))
                    .child(
                        div()
                            .flex_shrink_0()
                            .w_full()
                            .max_w(px(CONTENT_WIDTH))
                            .child(content)
                            .with_animation(
                                ("onboarding-step", self.step.index()),
                                Animation::new(STEP_FADE).with_easing(ease_in_out),
                                |el, delta| el.opacity(delta).mt(px(STEP_SLIDE * (1.0 - delta))),
                            ),
                    )
                    .child(div().flex_1().min_h(px(8.0))),
            )
            .child(self.render_footer(cx))
    }
}

/// A large title with its supporting sentence, centered.
fn step_heading(title: &'static str, body: &'static str, title_size: f32) -> impl IntoElement {
    v_flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .text_size(px(title_size))
                .font_weight(FontWeight::BOLD)
                .text_center()
                .child(title),
        )
        .child(
            div()
                .max_w(px(520.0))
                .text_center()
                .child(Label::new(body).color(Color::Muted)),
        )
}

fn feature_card(
    icon: IconName,
    title: &'static str,
    body: &'static str,
    cx: &App,
) -> impl IntoElement {
    let colors = cx.theme().colors();
    v_flex()
        .flex_1()
        .min_w_0()
        .gap_2()
        .p_4()
        .rounded_xl()
        .border_1()
        .border_color(colors.border_variant)
        .bg(colors.surface_background)
        .child(Icon::new(icon).size(IconSize::Medium).color(Color::Accent))
        .child(Label::new(title).weight(FontWeight::SEMIBOLD))
        .child(Label::new(body).size(LabelSize::Small).color(Color::Muted))
}

/// Where the setup is: done steps stay tinted and the current one is longer.
fn step_dots(current: OnboardingStep, cx: &App) -> impl IntoElement {
    let colors = cx.theme().colors();
    h_flex()
        .gap_1p5()
        .children(OnboardingStep::ALL.into_iter().map(|step| {
            let is_current = step == current;
            div()
                .h(px(6.0))
                .w(px(if is_current { 20.0 } else { 6.0 }))
                .rounded_full()
                .bg(if step.index() <= current.index() {
                    colors.accent
                } else {
                    colors.border
                })
        }))
}

/// A miniature of the main window drawn with `colors`, whichever theme is active.
fn theme_preview(colors: &ThemeColors) -> impl IntoElement {
    let colors = *colors;
    let line = move |width: f32, color: gpui::Hsla| {
        div().h(px(4.0)).w(relative(width)).rounded_full().bg(color)
    };
    let sidebar_row = move |width: f32, selected: bool| {
        h_flex()
            .h(px(14.0))
            .px_1()
            .rounded_sm()
            .when(selected, |row| row.bg(colors.element_selected))
            .child(line(width, colors.text_muted.opacity(0.7)))
    };
    let message_row = move |selected: bool| {
        v_flex()
            .gap_1()
            .px_2()
            .py_1p5()
            .border_b_1()
            .border_color(colors.border_variant)
            .when(selected, |row| row.bg(colors.accent))
            .child(line(
                0.55,
                if selected {
                    colors.text_on_accent
                } else {
                    colors.text
                },
            ))
            .child(line(
                0.85,
                if selected {
                    colors.text_on_accent.opacity(0.7)
                } else {
                    colors.text_muted.opacity(0.7)
                },
            ))
    };
    v_flex()
        .h(px(190.0))
        .w_full()
        .rounded_lg()
        .overflow_hidden()
        .border_1()
        .border_color(colors.border)
        .bg(colors.background)
        .child(
            div()
                .h(px(18.0))
                .flex_shrink_0()
                .bg(colors.title_bar_background)
                .border_b_1()
                .border_color(colors.border),
        )
        .child(
            h_flex()
                .flex_1()
                .min_h_0()
                .items_start()
                .child(
                    v_flex()
                        .h_full()
                        .w(relative(0.24))
                        .flex_shrink_0()
                        .gap_1()
                        .p_1()
                        .bg(colors.panel_background)
                        .border_r_1()
                        .border_color(colors.border)
                        .child(sidebar_row(0.8, true))
                        .child(sidebar_row(0.6, false))
                        .child(sidebar_row(0.7, false))
                        .child(sidebar_row(0.5, false)),
                )
                .child(
                    v_flex()
                        .h_full()
                        .w(relative(0.34))
                        .flex_shrink_0()
                        .bg(colors.surface_background)
                        .border_r_1()
                        .border_color(colors.border)
                        .child(message_row(true))
                        .child(message_row(false))
                        .child(message_row(false))
                        .child(message_row(false)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_2()
                        .p_2()
                        .child(line(0.7, colors.text))
                        .child(line(0.4, colors.text_muted.opacity(0.7)))
                        .child(line(0.95, colors.text_muted.opacity(0.5)))
                        .child(line(0.9, colors.text_muted.opacity(0.5)))
                        .child(line(0.6, colors.text_muted.opacity(0.5))),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_run_in_order_and_stop_at_both_ends() {
        assert_eq!(OnboardingStep::Welcome.previous(), None);
        assert_eq!(OnboardingStep::Welcome.next(), Some(OnboardingStep::Theme));
        assert_eq!(OnboardingStep::Theme.next(), Some(OnboardingStep::Account));
        assert_eq!(OnboardingStep::Account.next(), Some(OnboardingStep::Done));
        assert_eq!(OnboardingStep::Done.next(), None);
        assert_eq!(
            OnboardingStep::Done.previous(),
            Some(OnboardingStep::Account)
        );
    }

    #[test]
    fn only_a_new_profile_sees_the_setup() {
        assert!(needs_onboarding(false, 0));
        assert!(!needs_onboarding(true, 0));
        // Updating from a version without the setup must not show it.
        assert!(!needs_onboarding(false, 2));
    }

    #[test]
    fn default_app_is_only_requested_when_it_would_change_something() {
        use FinishAction::*;
        assert_eq!(finish_action(false, Some(Status::NotDefault), false), Close);
        assert_eq!(
            finish_action(true, Some(Status::NotDefault), false),
            ApplyDefaultApp
        );
        assert_eq!(
            finish_action(true, Some(Status::Unknown), false),
            ApplyDefaultApp
        );
        assert_eq!(finish_action(true, Some(Status::IsDefault), false), Close);
    }

    #[test]
    fn a_failed_default_app_request_does_not_trap_the_user() {
        assert_eq!(
            finish_action(true, Some(Status::NotDefault), true),
            FinishAction::Close
        );
    }

    #[test]
    fn check_mark_pop_starts_empty_passes_full_size_and_settles() {
        assert!(overshoot(0.0).abs() < 0.001);
        assert!((overshoot(1.0) - 1.0).abs() < 0.001);
        assert!(overshoot(0.7) > 1.0);
    }

    #[test]
    fn the_logo_is_a_decodable_image() {
        assert!(image_header_is_png(LOGO_PNG));
    }

    fn image_header_is_png(bytes: &[u8]) -> bool {
        bytes.starts_with(&[0x89, b'P', b'N', b'G'])
    }

    fn init(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            theme::init(Appearance::Dark, cx);
            crate::locale::init(crate::locale::Language::default(), cx);
        });
    }

    fn fresh_root(cx: &mut gpui::TestAppContext) -> (tempfile::TempDir, Entity<RootView>) {
        init(cx);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mail.db");
        let root = cx.new(|_| RootView::for_tests(&path));
        (dir, root)
    }

    #[gpui::test]
    fn the_first_account_moves_the_account_step_on(cx: &mut gpui::TestAppContext) {
        let (_dir, root) = fresh_root(cx);
        let view = cx.new(|cx| OnboardingView::new(root, cx));
        view.update(cx, |view, cx| {
            view.step = OnboardingStep::Account;
            view.default_status_requested = true;
            view.on_accounts_changed(true, cx);
            assert_eq!(view.step, OnboardingStep::Done);
        });
    }

    #[gpui::test]
    fn an_account_added_on_another_step_does_not_jump(cx: &mut gpui::TestAppContext) {
        let (_dir, root) = fresh_root(cx);
        let view = cx.new(|cx| OnboardingView::new(root, cx));
        view.update(cx, |view, cx| {
            view.step = OnboardingStep::Theme;
            view.on_accounts_changed(true, cx);
            assert_eq!(view.step, OnboardingStep::Theme);
            assert!(view.has_account);
        });
    }

    #[gpui::test]
    fn going_back_to_the_account_step_stays_there(cx: &mut gpui::TestAppContext) {
        let (_dir, root) = fresh_root(cx);
        let view = cx.new(|cx| OnboardingView::new(root, cx));
        view.update(cx, |view, cx| {
            view.has_account = true;
            view.step = OnboardingStep::Account;
            view.on_accounts_changed(true, cx);
            assert_eq!(view.step, OnboardingStep::Account);
        });
    }

    #[gpui::test]
    fn finishing_without_the_default_app_closes_the_setup(cx: &mut gpui::TestAppContext) {
        let (_dir, root) = fresh_root(cx);
        assert!(root.read_with(cx, |root, _| root.onboarding_active()));
        let view = cx.new(|cx| OnboardingView::new(root.clone(), cx));
        view.update(cx, |view, cx| view.finish(cx));
        assert!(!root.read_with(cx, |root, _| root.onboarding_active()));
        assert!(root.read_with(cx, |root, _| root.persisted_onboarding_completed()));
    }

    #[gpui::test]
    fn choosing_a_theme_applies_and_remembers_it(cx: &mut gpui::TestAppContext) {
        let (_dir, root) = fresh_root(cx);
        root.update(cx, |root, cx| root.set_appearance(Appearance::Light, cx));
        cx.update(|cx| assert_eq!(cx.theme().appearance(), Appearance::Light));
        assert!(root.read_with(cx, |root, _| root.persisted_light_theme()));
    }

    #[gpui::test]
    fn the_main_window_shows_the_setup_until_it_is_finished(cx: &mut gpui::TestAppContext) {
        init(cx);
        cx.update(|cx| {
            ui::bind_keys(cx);
            html_view::init(cx);
        });
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mail.db");
        let (root, cx) = cx.add_window_view(|_, _| RootView::for_tests(&path));
        // Windows shows only a background until the window has settled.
        root.update(cx, |root, cx| root.mark_content_ready(cx));
        cx.run_until_parked();
        let setup = root.read_with(cx, |root, _| root.onboarding_view());
        let setup = setup.expect("the first render creates the setup");
        setup.update(cx, |view, cx| view.finish(cx));
        cx.run_until_parked();
        root.read_with(cx, |root, _| {
            assert!(!root.onboarding_active());
            assert!(root.onboarding_view().is_none());
        });
    }

    #[gpui::test]
    fn the_command_palette_stays_closed_during_the_setup(cx: &mut gpui::TestAppContext) {
        init(cx);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mail.db");
        let (root, cx) = cx.add_window_view(|_, _| RootView::for_tests(&path));
        cx.run_until_parked();
        root.update_in(cx, |root, window, cx| {
            root.toggle_command_palette(window, cx);
            assert!(!root.command_palette_is_open());
        });
    }

    #[gpui::test]
    fn the_account_form_follows_a_language_chosen_after_it_was_created(
        cx: &mut gpui::TestAppContext,
    ) {
        init(cx);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mail.db");
        let root = cx.new(|_| RootView::for_tests(&path));
        let (view, cx) = cx.add_window_view(move |_, cx| {
            let mut view = OnboardingView::new(root, cx);
            view.step = OnboardingStep::Account;
            view
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            crate::locale::set_language(crate::locale::Language::Portuguese, cx);
        });
        cx.run_until_parked();
        view.update(cx, |view, cx| {
            assert_eq!(
                view.account_form.read(cx).form_email_placeholder(cx),
                Some(
                    crate::locale::Key::AccountEmailPlaceholder
                        .tr(crate::locale::Language::Portuguese)
                        .to_string()
                )
            );
        });
    }

    #[gpui::test]
    fn every_step_renders(cx: &mut gpui::TestAppContext) {
        init(cx);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mail.db");
        let root = cx.new(|_| RootView::for_tests(&path));
        for step in OnboardingStep::ALL {
            let root = root.clone();
            let (view, cx) = cx.add_window_view(move |_, cx| {
                let mut view = OnboardingView::new(root, cx);
                view.step = step;
                view
            });
            cx.run_until_parked();
            view.update(cx, |view, _| assert_eq!(view.step, step));
        }
    }
}
