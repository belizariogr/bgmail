//! Confirmation dialogs in the app's theme, where the platform has no native
//! ones (GPUI's fallback is always white). Mirrors Zed's `ui_prompt`.

use gpui::{
    App, EventEmitter, FocusHandle, Focusable, KeyDownEvent, PromptButton, PromptHandle,
    PromptLevel, PromptResponse, RenderablePromptHandle, SharedString,
};

use crate::prelude::*;
use crate::{Button, ButtonStyle};

/// Uses these dialogs where the platform doesn't draw its own, which already
/// follow the system's appearance.
pub fn init(cx: &mut App) {
    if cfg!(not(any(target_os = "macos", target_os = "windows"))) {
        cx.set_prompt_builder(themed_prompt);
    }
}

fn themed_prompt(
    _level: PromptLevel,
    message: &str,
    detail: Option<&str>,
    actions: &[PromptButton],
    handle: PromptHandle,
    window: &mut Window,
    cx: &mut App,
) -> RenderablePromptHandle {
    let view = cx.new(|cx| ThemedPrompt {
        message: SharedString::from(message.to_string()),
        detail: detail
            .filter(|detail| !detail.is_empty())
            .map(|detail| SharedString::from(detail.to_string())),
        actions: actions.to_vec(),
        focus: cx.focus_handle(),
    });
    handle.with_view(view, window, cx)
}

fn is_cancel(action: &PromptButton) -> bool {
    matches!(action, PromptButton::Cancel(_))
}

/// Action a key answers with: Enter takes the first that doesn't cancel, Escape
/// the one that cancels, or the last, where callers put it.
fn action_for_key(key: &str, actions: &[PromptButton]) -> Option<usize> {
    match key {
        "enter" => actions.iter().position(|action| !is_cancel(action)),
        "escape" => actions
            .iter()
            .position(is_cancel)
            .or(actions.len().checked_sub(1)),
        _ => None,
    }
}

struct ThemedPrompt {
    message: SharedString,
    detail: Option<SharedString>,
    actions: Vec<PromptButton>,
    focus: FocusHandle,
}

impl EventEmitter<PromptResponse> for ThemedPrompt {}

impl Focusable for ThemedPrompt {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ThemedPrompt {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let (surface, border, text) = (
            colors.elevated_surface_background,
            colors.border,
            colors.text,
        );
        let primary = self.actions.iter().position(|action| !is_cancel(action));
        let buttons = self.actions.iter().enumerate().map(|(ix, action)| {
            Button::new(("prompt-action", ix), action.label().clone())
                .style(if Some(ix) == primary {
                    ButtonStyle::Filled
                } else {
                    ButtonStyle::Subtle
                })
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(PromptResponse(ix))))
        });
        let card = v_flex()
            .id("prompt")
            .occlude()
            .cursor_default()
            .w(px(380.0))
            .p_4()
            .gap_3()
            .rounded_lg()
            .border_1()
            .border_color(border)
            .bg(surface)
            .text_color(text)
            .shadow_lg()
            .child(Label::new(self.message.clone()))
            .children(self.detail.clone().map(|detail| {
                Label::new(detail)
                    .size(LabelSize::Small)
                    .color(Color::Muted)
            }))
            .child(h_flex().justify_end().gap_2().children(buttons));
        div()
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if let Some(ix) = action_for_key(&event.keystroke.key, &this.actions) {
                    cx.emit(PromptResponse(ix));
                }
            }))
            .child(
                div()
                    .absolute()
                    .size_full()
                    .top_0()
                    .left_0()
                    .occlude()
                    .bg(gpui::black().opacity(0.45)),
            )
            .child(
                h_flex()
                    .absolute()
                    .size_full()
                    .top_0()
                    .left_0()
                    .justify_center()
                    .child(card),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn actions() -> Vec<PromptButton> {
        vec![PromptButton::new("Delete"), PromptButton::new("Keep")]
    }

    #[test]
    fn keys_answer_with_the_expected_action() {
        assert_eq!(action_for_key("enter", &actions()), Some(0));
        assert_eq!(action_for_key("escape", &actions()), Some(1));
        assert_eq!(action_for_key("x", &actions()), None);
        assert_eq!(action_for_key("enter", &[]), None);
        assert_eq!(action_for_key("escape", &[]), None);

        let flagged = [PromptButton::cancel("Keep"), PromptButton::new("Delete")];
        assert_eq!(action_for_key("enter", &flagged), Some(1));
        assert_eq!(action_for_key("escape", &flagged), Some(0));
    }

    struct Host;

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full()
        }
    }

    #[gpui::test]
    fn the_dialog_answers_from_the_keyboard(cx: &mut TestAppContext) {
        cx.update(|cx| {
            theme::init(theme::Appearance::Dark, cx);
            cx.set_prompt_builder(themed_prompt);
        });
        let (_, cx) = cx.add_window_view(|_, _| Host);
        let mut answer = cx.update(|window, cx| {
            window.prompt(
                PromptLevel::Warning,
                "Sure?",
                Some("Details"),
                &["Delete", "Keep"],
                cx,
            )
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert_eq!(answer.try_recv(), Ok(Some(1)));
    }
}
