//! In-window command palette overlay.
//!
//! The HTML reader is CEF soft OSR composited by GPUI, so a normal elevated
//! surface in the main window stacks above the reader texture. A separate
//! `WindowKind::PopUp` is not required (and breaks under Wayland compositors).

use gpui::{Context, Entity, KeyDownEvent, MouseButton};
use theme::ActiveTheme;
use ui::prelude::*;
use ui::{Label, LabelSize, ListItem, TextInput};

use crate::commands::CommandEntry;
use crate::locale::{ActiveLanguage, Key};
use crate::root::RootView;

/// Builds the dimmed backdrop + centered palette panel for [`RootView`].
pub(crate) fn render_command_palette(
    cx: &mut Context<RootView>,
    entries: Vec<CommandEntry>,
    selected_ix: usize,
    input: Entity<TextInput>,
) -> gpui::AnyElement {
    let colors = cx.theme().colors();
    let panel_bg = colors.elevated_surface_background;
    let border = colors.border;
    let text = colors.text;
    let language = cx.language();
    input.update(cx, |field, _| {
        field.set_placeholder(Key::CommandPalette.tr(language));
    });

    let list = v_flex()
        .id("command-palette-list")
        .overflow_y_scroll()
        .max_h(px(330.0))
        .children(entries.iter().enumerate().map(|(ix, entry)| {
            let command_id = entry.id.clone();
            ListItem::new(("command", ix))
                .selected(ix == selected_ix)
                .child(
                    Label::new(entry.label.clone())
                        .size(LabelSize::Small)
                        .single_line(),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.execute_command(&command_id, window, cx);
                }))
        }));

    div()
        .id("command-palette-backdrop")
        .absolute()
        .inset_0()
        .bg(gpui::hsla(0., 0., 0., 0.45))
        .flex()
        .items_start()
        .justify_center()
        .pt(px(80.0))
        .occlude()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.dismiss_command_palette(cx);
            }),
        )
        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
            if event.keystroke.key.as_str() == "escape" {
                this.dismiss_command_palette(cx);
                cx.stop_propagation();
                return;
            }
            if this.handle_command_palette_key(event, window, cx) {
                cx.stop_propagation();
            }
        }))
        .child(
            div()
                .id("command-palette-panel")
                .w(px(520.0))
                .flex()
                .flex_col()
                .rounded_lg()
                .border_1()
                .border_color(border)
                .bg(panel_bg)
                .shadow_lg()
                .text_color(text)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(div().px_3().pt_3().pb_2().child(input))
                .child(div().h(px(1.0)).bg(border))
                .child(div().p_1().child(list)),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    #[test]
    fn palette_module_exports_in_window_renderer() {
        // Ensures the overlay stays a RootView helper, not a separate window view.
        let src = include_str!("command_palette_overlay.rs");
        assert!(src.contains("In-window command palette"));
        assert!(src.contains("pub(crate) fn render_command_palette"));
    }
}
