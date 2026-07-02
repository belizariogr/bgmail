//! Popup window that hosts the command palette above native webviews.
//!
//! GPUI paints into a Metal layer while the e-mail reader uses a child OS webview
//! that always stacks above that layer. A separate popup window (`WindowKind::PopUp`)
//! sits above the main window without hiding the webview.

use gpui::{Context, Entity, Focusable, KeyDownEvent, MouseButton, Render, WeakEntity, Window};
use theme::ActiveTheme;
use ui::prelude::*;
use ui::{Label, LabelSize, ListItem, TextInput};

use crate::commands::CommandEntry;
use crate::locale::{ActiveLanguage, Key};
use crate::root::RootView;

pub struct CommandPaletteOverlay {
    root: WeakEntity<RootView>,
    focus_requested: bool,
}

impl CommandPaletteOverlay {
    pub fn new(root: WeakEntity<RootView>) -> Self {
        Self {
            root,
            focus_requested: true,
        }
    }

    /// Closes the popup on the next frame. Never call [`Window::remove_window`]
    /// synchronously from an input handler on this window — GPUI deadlocks.
    fn dismiss_popup(window: &mut Window, root: WeakEntity<RootView>, cx: &mut Context<Self>) {
        window.defer(cx, move |window, cx| {
            if let Some(root) = root.upgrade() {
                root.update(cx, |root, cx| root.dismiss_command_palette(cx));
            }
            window.remove_window();
        });
    }

    fn handle_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key.as_str() == "escape" {
            Self::dismiss_popup(window, self.root.clone(), cx);
            cx.stop_propagation();
            return;
        }

        let Some(root) = self.root.upgrade() else {
            return;
        };
        let handled = root.update(cx, |root, cx| {
            root.handle_command_palette_key(event, window, cx)
        });
        if handled {
            cx.stop_propagation();
        }
    }

    fn render_palette(
        &self,
        cx: &mut Context<Self>,
        entries: Vec<CommandEntry>,
        selected_ix: usize,
        input: Entity<TextInput>,
    ) -> gpui::AnyElement {
        let colors = cx.theme().colors();
        let panel_bg = colors.elevated_surface_background;
        let border = colors.border;
        let text = colors.text;
        let root = self.root.clone();

        let list = v_flex()
            .id("command-palette-list")
            .overflow_y_scroll()
            .max_h(px(330.0))
            .children(entries.iter().enumerate().map(|(ix, entry)| {
                let command_id = entry.id.clone();
                let root = root.clone();
                ListItem::new(("command", ix))
                    .selected(ix == selected_ix)
                    .child(
                        Label::new(entry.label.clone())
                            .size(LabelSize::Small)
                            .single_line(),
                    )
                    .on_click(cx.listener(move |_, _, window, cx| {
                        if let Some(root) = root.upgrade() {
                            root.update(cx, |root, cx| {
                                root.execute_command(&command_id, window, cx);
                            });
                        }
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
            .on_click(cx.listener({
                let root = self.root.clone();
                move |_, _, window, cx| {
                    Self::dismiss_popup(window, root.clone(), cx);
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
}

impl Render for CommandPaletteOverlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.focus_requested {
            self.focus_requested = false;
            if let Some(root_entity) = self.root.upgrade() {
                let input = root_entity
                    .read(cx)
                    .command_palette
                    .as_ref()
                    .and_then(|palette| palette.input.clone());
                if let Some(input) = input {
                    let focus_handle = input.read(cx).focus_handle(cx);
                    window.defer(cx, move |window, _| {
                        window.focus(&focus_handle);
                    });
                }
            }
        }

        let Some(root_entity) = self.root.upgrade() else {
            return div().size_full().into_any_element();
        };
        let root = root_entity.read(cx);
        let Some(palette) = root.command_palette.as_ref().filter(|palette| palette.open) else {
            return div().size_full().into_any_element();
        };

        let language = cx.language();
        let ctx = root.command_context();
        let entries = palette.filtered_entries(language, &ctx);
        let selected_ix = if entries.is_empty() {
            0
        } else {
            palette.selected_ix.min(entries.len() - 1)
        };
        let Some(input) = palette.input.clone() else {
            return div().size_full().into_any_element();
        };
        input.update(cx, |field, _| {
            field.set_placeholder(Key::CommandPalette.tr(language));
        });

        div()
            .size_full()
            .on_key_down(cx.listener(|this, event, window, cx| {
                this.handle_key(event, window, cx);
            }))
            .child(self.render_palette(cx, entries, selected_ix, input))
            .into_any_element()
    }
}
