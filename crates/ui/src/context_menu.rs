use gpui::{px, ClickEvent};

use crate::prelude::*;
use crate::{Label, LabelSize};

type EntryHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// One row of a [`ContextMenu`].
pub enum ContextMenuItem {
    Entry {
        label: SharedString,
        /// Keyboard shortcut shown at the end of the row (e.g. "Ctrl+C").
        key_hint: Option<SharedString>,
        handler: EntryHandler,
    },
    Separator,
}

/// A floating menu of actions (named after Zed's `ContextMenu`). The host
/// positions it (e.g. `anchored()` at the click inside `deferred()`) and
/// closes it from the entry handlers or on an outside click.
#[derive(IntoElement, Default)]
pub struct ContextMenu {
    items: Vec<ContextMenuItem>,
}

impl ContextMenu {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an entry running `handler` when clicked.
    pub fn entry(
        mut self,
        label: impl Into<SharedString>,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.items.push(ContextMenuItem::Entry {
            label: label.into(),
            key_hint: None,
            handler: Box::new(handler),
        });
        self
    }

    /// Adds an entry with a keyboard shortcut hint at its end.
    pub fn entry_with_key_hint(
        mut self,
        label: impl Into<SharedString>,
        key_hint: impl Into<SharedString>,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.items.push(ContextMenuItem::Entry {
            label: label.into(),
            key_hint: Some(key_hint.into()),
            handler: Box::new(handler),
        });
        self
    }

    /// Adds a separator line, unless the menu is empty or already ends with one.
    pub fn separator(mut self) -> Self {
        if matches!(self.items.last(), Some(ContextMenuItem::Entry { .. })) {
            self.items.push(ContextMenuItem::Separator);
        }
        self
    }

    /// The menu's rows.
    pub fn items(&self) -> &[ContextMenuItem] {
        &self.items
    }

    /// Whether the menu has no entries.
    pub fn is_empty(&self) -> bool {
        !self
            .items
            .iter()
            .any(|item| matches!(item, ContextMenuItem::Entry { .. }))
    }
}

impl RenderOnce for ContextMenu {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.theme().colors();
        let mut items = self.items;
        // A trailing separator separates nothing.
        if matches!(items.last(), Some(ContextMenuItem::Separator)) {
            items.pop();
        }
        v_flex()
            .occlude()
            .min_w(px(200.0))
            .p_1()
            .bg(colors.elevated_surface_background)
            .border_1()
            .border_color(colors.border)
            .rounded_md()
            .shadow_lg()
            .children(items.into_iter().enumerate().map(|(ix, item)| {
                match item {
                    ContextMenuItem::Separator => {
                        div().my_1().h(px(1.0)).bg(colors.border).into_any_element()
                    }
                    ContextMenuItem::Entry {
                        label,
                        key_hint,
                        handler,
                    } => h_flex()
                        .id(("context-menu-entry", ix))
                        .debug_selector(|| format!("context-menu-entry-{ix}"))
                        // Stretched by the column, not `w_full`: a percentage has
                        // nothing to resolve against in a menu sized by its content.
                        .gap_4()
                        .justify_between()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .cursor_pointer()
                        .hover(|el| el.bg(colors.element_hover))
                        .child(Label::new(label).size(LabelSize::Small))
                        .when_some(key_hint, |el, hint| {
                            el.child(
                                Label::new(hint)
                                    .size(LabelSize::Small)
                                    .color(crate::Color::Muted),
                            )
                        })
                        .on_click(move |event, window, cx| handler(event, window, cx))
                        .into_any_element(),
                }
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    struct Host;

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            // As the hosts use it: anchored, in a box that scrolls long menus.
            div().child(gpui::deferred(
                gpui::anchored().child(
                    div().id("menu").max_h(px(400.0)).overflow_y_scroll().child(
                        ContextMenu::new()
                            .entry("A", |_, _, _| {})
                            .entry_with_key_hint("Much longer entry", "Ctrl+A", |_, _, _| {}),
                    ),
                ),
            ))
        }
    }

    #[gpui::test]
    fn entries_fill_the_width_of_the_menu(cx: &mut TestAppContext) {
        cx.update(|cx| theme::init(theme::Appearance::Dark, cx));
        let (_, cx) = cx.add_window_view(|_, _| Host);
        cx.run_until_parked();
        let short = cx
            .debug_bounds("context-menu-entry-0")
            .expect("short entry");
        let long = cx.debug_bounds("context-menu-entry-1").expect("long entry");
        assert_eq!(short.size.width, long.size.width);
        // At least the menu's minimum, less its padding (4px) and border (1px).
        assert!(short.size.width >= px(190.0), "{:?}", short.size.width);
    }

    #[test]
    fn separators_only_follow_entries() {
        let menu = ContextMenu::new()
            .separator()
            .entry("Copy", |_, _, _| {})
            .separator()
            .separator()
            .entry_with_key_hint("Select all", "Ctrl+A", |_, _, _| {});
        let kinds: Vec<bool> = menu
            .items()
            .iter()
            .map(|item| matches!(item, ContextMenuItem::Entry { .. }))
            .collect();
        assert_eq!(kinds, [true, false, true]);
    }

    #[test]
    fn empty_menus_have_no_entries() {
        assert!(ContextMenu::new().is_empty());
        assert!(ContextMenu::new().separator().is_empty());
        assert!(!ContextMenu::new().entry("Copy", |_, _, _| {}).is_empty());
    }
}
