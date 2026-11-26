//! Settings ▸ Signatures, as in Gmail's signature settings: the saved signatures,
//! each a name and the compose window's editor (formatting and images), and the
//! "Signature defaults" of each account: one for new mail, one for replies and
//! forwards.
//!
//! Edits save themselves after a pause, as the other settings apply at once.

use std::time::Duration;

use gpui::{
    deferred, App, ClickEvent, Context, Entity, Focusable, FontWeight, MouseButton, PromptButton,
    PromptLevel, Render, ScrollHandle, Subscription, Task, WeakEntity, Window,
};
use theme::ActiveTheme;
use ui::prelude::*;
use ui::{
    Button, ButtonStyle, Color, ContextMenu, Icon, IconName, IconSize, Label, LabelSize, ListItem,
    TextArea, TextAreaEvent, TextInput, TextInputEvent,
};

use crate::format_bar::{FormatBar, FormatBarEvent};
use crate::locale::{ActiveLanguage, Key, Language};
use crate::root::RootView;
use crate::settings::input_box;
use crate::signatures::{new_signature_name, SignatureEntry, SignatureSlot};

/// Pause after the last edit before it is saved.
const SAVE_DELAY: Duration = Duration::from_millis(400);

/// Largest image file in a signature: it goes out with every message.
pub const SIGNATURE_IMAGE_LIMIT_BYTES: usize = 1024 * 1024;

/// Text rows the signature editor shows at least.
const EDITOR_MIN_LINES: usize = 5;

/// The Signatures section of the settings window.
pub(crate) struct SignaturesPage {
    root: WeakEntity<RootView>,
    /// The database, written straight when the page goes with edits still waiting:
    /// at quit the main view may already be gone.
    db_path: Option<std::path::PathBuf>,
    signatures: Vec<SignatureEntry>,
    selected: Option<i64>,
    name: Entity<TextInput>,
    editor: Entity<TextArea>,
    format_bar: Entity<FormatBar>,
    editor_scroll: ScrollHandle,
    /// The last thing that went wrong (an image refused).
    message: Option<String>,
    /// Edits waiting for [`SAVE_DELAY`].
    pending_save: Option<Task<()>>,
    /// A default's dropdown: the account and which of its signatures.
    defaults_menu: Option<(i64, SignatureSlot)>,
    _subscriptions: Vec<Subscription>,
}

impl SignaturesPage {
    pub(crate) fn new(root: WeakEntity<RootView>, cx: &mut Context<Self>) -> Self {
        let language = cx.language();
        let name = cx.new(|cx| TextInput::new(Key::SignatureName.tr(language), cx));
        let editor_scroll = ScrollHandle::new();
        let editor = cx.new(|cx| {
            let mut area = TextArea::new(Key::SignaturePlaceholder.tr(language), cx)
                .min_lines(EDITOR_MIN_LINES);
            area.track_scroll(&editor_scroll);
            area
        });
        let format_bar =
            cx.new(|cx| FormatBar::new(editor.clone(), SIGNATURE_IMAGE_LIMIT_BYTES, cx));
        let subscriptions = vec![
            cx.subscribe(&name, |this, _, _: &TextInputEvent, cx| {
                this.schedule_save(cx)
            }),
            cx.subscribe(&editor, |this, _, _: &TextAreaEvent, cx| {
                this.schedule_save(cx)
            }),
            cx.subscribe(
                &format_bar,
                |this, _, event: &FormatBarEvent, cx| match event {
                    FormatBarEvent::ImageFailed(message) => {
                        this.message = Some(message.clone());
                        cx.notify();
                    }
                },
            ),
            // Closing the window or quitting right after typing still keeps what
            // was typed.
            cx.on_release(|this, cx| this.save_on_release(cx)),
        ];
        let db_path = root
            .upgrade()
            .map(|root| root.read(cx).database_path().to_path_buf());
        let mut page = Self {
            root,
            db_path,
            signatures: Vec::new(),
            selected: None,
            name,
            editor,
            format_bar,
            editor_scroll,
            message: None,
            pending_save: None,
            defaults_menu: None,
            _subscriptions: subscriptions,
        };
        page.reload(cx);
        let first = page.signatures.first().map(|entry| entry.id);
        page.select(first, cx);
        page
    }

    /// Rereads the saved signatures.
    fn reload(&mut self, cx: &mut Context<Self>) {
        self.signatures = self
            .root
            .upgrade()
            .map(|root| root.read(cx).signatures())
            .unwrap_or_default();
    }

    /// Shows signature `id` in the editor, saving the one shown first.
    fn select(&mut self, id: Option<i64>, cx: &mut Context<Self>) {
        // Its own row: nothing to load, and the editor keeps its undo.
        if id.is_some() && id == self.selected {
            return;
        }
        self.save_now(cx);
        // An image still being picked was for the signature going away.
        self.format_bar.update(cx, |bar, _| bar.cancel_image());
        self.selected = id.filter(|id| self.signatures.iter().any(|entry| entry.id == *id));
        self.message = None;
        let entry = self
            .selected
            .and_then(|id| self.signatures.iter().find(|entry| entry.id == id))
            .cloned();
        let (name, document) = entry
            .map(|entry| (entry.name, entry.document))
            .unwrap_or_default();
        self.name
            .update(cx, |input, cx| input.set_content(&name, cx));
        self.editor
            .update(cx, |area, cx| area.set_document(document, cx));
        cx.notify();
    }

    /// Adds a numbered signature and selects its name to be typed over.
    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_now(cx);
        let name = new_signature_name(
            Key::SignatureDefaultName.tr(cx.language()),
            &self.signatures,
        );
        let created = self
            .root
            .update(cx, |root, cx| root.create_signature(&name, cx))
            .ok()
            .flatten();
        self.reload(cx);
        if let Some(id) = created {
            self.select(Some(id), cx);
            window.focus(&self.name.focus_handle(cx));
            self.name
                .update(cx, |input, cx| input.select_all_content(cx));
        }
    }

    /// Deletes signature `id` once the user confirms.
    fn confirm_delete(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let language = cx.language();
        let Some(entry) = self.signatures.iter().find(|entry| entry.id == id) else {
            return;
        };
        let question = Key::SignatureDeleteConfirm
            .tr(language)
            .replace("{}", &entry.name);
        let answer = window.prompt(
            PromptLevel::Warning,
            &question,
            Some(Key::SignatureDeleteDetail.tr(language)),
            &[
                PromptButton::new(Key::SignatureDelete.tr(language)),
                PromptButton::cancel(Key::ComposeCancelButton.tr(language)),
            ],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await == Ok(0) {
                let _ = this.update(cx, |this, cx| this.delete(id, cx));
            }
        })
        .detach();
    }

    fn delete(&mut self, id: i64, cx: &mut Context<Self>) {
        // What waits to be saved belongs to the signature going away.
        self.pending_save = None;
        let _ = self
            .root
            .update(cx, |root, cx| root.delete_signature(id, cx));
        self.reload(cx);
        let next = self.signatures.first().map(|entry| entry.id);
        self.select(next, cx);
    }

    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        if self.selected.is_none() {
            return;
        }
        self.pending_save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            let _ = this.update(cx, |this, cx| this.save_now(cx));
        }));
    }

    /// The signature shown, as edited: its id, name (the old one while the field is
    /// blank) and document.
    fn edited_signature(&self, cx: &App) -> Option<(i64, String, ui::RichDocument)> {
        let id = self.selected?;
        let typed = self.name.read(cx).content().trim().to_string();
        let name = if typed.is_empty() {
            self.signatures
                .iter()
                .find(|entry| entry.id == id)
                .map(|entry| entry.name.clone())
                .unwrap_or_default()
        } else {
            typed
        };
        Some((id, name, self.editor.read(cx).document()))
    }

    /// Writes the edits still waiting when the page goes, on a connection of its own,
    /// synchronously: nothing after this would save them.
    fn save_on_release(&mut self, cx: &mut App) {
        if self.pending_save.take().is_none() {
            return;
        }
        let Some((id, name, document)) = self.edited_signature(cx) else {
            return;
        };
        // Through the main view while it is there (an open compose window learns of
        // it); at quit it may be gone.
        if let Some(root) = self.root.upgrade() {
            root.update(cx, |root, cx| root.save_signature(id, &name, &document, cx));
            return;
        }
        let Some(path) = self.db_path.as_ref() else {
            return;
        };
        let saved = storage::Database::open(path).and_then(|db| {
            db.update_signature(id, &name, &crate::signatures::document_json(&document))
        });
        if let Err(error) = saved {
            eprintln!("BGMail: could not save the signature: {error}");
        }
    }

    /// Saves the edits waiting for the pause, if any.
    fn save_now(&mut self, cx: &mut Context<Self>) {
        if self.pending_save.take().is_none() {
            return;
        }
        let Some((id, name, document)) = self.edited_signature(cx) else {
            return;
        };
        if let Some(entry) = self.signatures.iter_mut().find(|entry| entry.id == id) {
            entry.name = name.clone();
            entry.document = document.clone();
        }
        let _ = self.root.update(cx, |root, cx| {
            root.save_signature(id, &name, &document, cx);
        });
        cx.notify();
    }

    pub(crate) fn toggle_defaults_menu(
        &mut self,
        key: (i64, SignatureSlot),
        cx: &mut Context<Self>,
    ) {
        self.defaults_menu = (self.defaults_menu != Some(key)).then_some(key);
        cx.notify();
    }

    /// Closes the defaults dropdown and the format bar's (also when the section
    /// changes, as their catcher only covers the page).
    pub(crate) fn close_menus(&mut self, cx: &mut Context<Self>) {
        self.defaults_menu = None;
        self.format_bar.update(cx, |bar, cx| bar.close_menu(cx));
        cx.notify();
    }

    /// Makes `id` the signature account `account_id` puts in messages of `slot`.
    fn set_default(
        &mut self,
        account_id: i64,
        slot: SignatureSlot,
        id: Option<i64>,
        cx: &mut Context<Self>,
    ) {
        self.defaults_menu = None;
        let Some(root) = self.root.upgrade() else {
            return;
        };
        let Some(account) = root
            .read(cx)
            .accounts()
            .iter()
            .find(|account| account.id == account_id)
            .cloned()
        else {
            return;
        };
        let (new_mail, replies) = match slot {
            SignatureSlot::NewMail => (id, account.signature_reply),
            SignatureSlot::Reply => (account.signature_new, id),
        };
        root.update(cx, |root, cx| {
            root.set_account_signatures(account_id, new_mail, replies, cx);
        });
        cx.notify();
    }

    #[cfg(test)]
    pub(crate) fn defaults_menu_open(&self) -> bool {
        self.defaults_menu.is_some()
    }

    fn render_list(&self, language: Language, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        if self.signatures.is_empty() {
            return div()
                .child(
                    Label::new(Key::SignatureEmpty.tr(language))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .into_any_element();
        }
        let mut list = v_flex()
            .p_1()
            .gap_0p5()
            .rounded_lg()
            .border_1()
            .border_color(colors.border)
            .bg(colors.surface_background);
        for (ix, entry) in self.signatures.iter().enumerate() {
            let id = entry.id;
            let selected = self.selected == Some(id);
            list = list.child(
                ListItem::new(("signature-row", ix))
                    .selected(selected)
                    .start_slot(Icon::new(IconName::Signature).size(IconSize::Small).color(
                        if selected {
                            Color::Accent
                        } else {
                            Color::Muted
                        },
                    ))
                    .child(Label::new(entry.name.clone()).size(LabelSize::Small))
                    .on_click(cx.listener(move |this, _, _, cx| this.select(Some(id), cx))),
            );
        }
        list.into_any_element()
    }

    fn render_editor(
        &self,
        id: i64,
        language: Language,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors();
        let focused = self.editor.read(cx).focus_handle(cx).is_focused(window);
        let editor = self.editor.clone();
        v_flex()
            .gap_3()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new(Key::SignatureName.tr(language))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .child(input_box(&self.name, window, cx)),
            )
            .child(
                v_flex()
                    .rounded_md()
                    .border_1()
                    .border_color(if focused {
                        colors.border_focused
                    } else {
                        colors.border
                    })
                    .bg(colors.background)
                    .child(
                        div()
                            .id("signature-editor")
                            .max_h(px(280.0))
                            .overflow_y_scroll()
                            .track_scroll(&self.editor_scroll)
                            .p_3()
                            .cursor_text()
                            // Clicks around the text still place the caret.
                            .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                                editor.update(cx, |area, cx| area.press_outside(event, window, cx));
                            })
                            .child(self.editor.clone()),
                    )
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .border_t_1()
                            .border_color(colors.border)
                            .child(self.format_bar.clone()),
                    ),
            )
            .children(self.message.clone().map(|message| {
                Label::new(message)
                    .size(LabelSize::Small)
                    .color(Color::Error)
            }))
            .child(
                h_flex().child(
                    Button::new("signature-delete", Key::SignatureDelete.tr(language))
                        .icon(IconName::Trash)
                        .style(ButtonStyle::Subtle)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.confirm_delete(id, window, cx);
                        })),
                ),
            )
    }

    /// Gmail's "Signature defaults": per account, for new mail and for replies.
    fn render_defaults(&self, language: Language, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let (border, surface) = (colors.border, colors.surface_background);
        let accounts: Vec<storage::Account> = self
            .root
            .upgrade()
            .map(|root| root.read(cx).accounts().to_vec())
            .unwrap_or_default()
            .into_iter()
            .filter(|account| account.provider != storage::PROVIDER_LOCAL)
            .collect();
        let mut section = v_flex()
            .gap_2()
            .child(Label::new(Key::SignatureDefaults.tr(language)).weight(FontWeight::SEMIBOLD));
        if accounts.is_empty() {
            return section.child(
                Label::new(Key::SignatureNoAccounts.tr(language))
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            );
        }
        let mut list = v_flex()
            .rounded_lg()
            .border_1()
            .border_color(border)
            .bg(surface);
        let count = accounts.len();
        for (ix, account) in accounts.iter().enumerate() {
            let title = if account.name.trim().is_empty() {
                account.email.clone()
            } else {
                account.name.clone()
            };
            list = list.child(
                v_flex()
                    .px_3()
                    .py_2()
                    .gap_2()
                    .when(ix + 1 < count, |el| el.border_b_1().border_color(border))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(Label::new(title).size(LabelSize::Small).bold())
                            .child(
                                Label::new(account.email.clone())
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_4()
                            .flex_wrap()
                            .child(self.render_default_picker(
                                account,
                                SignatureSlot::NewMail,
                                language,
                                cx,
                            ))
                            .child(self.render_default_picker(
                                account,
                                SignatureSlot::Reply,
                                language,
                                cx,
                            )),
                    ),
            );
        }
        section = section.child(list);
        section
    }

    fn render_default_picker(
        &self,
        account: &storage::Account,
        slot: SignatureSlot,
        language: Language,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let hover = cx.theme().colors().element_hover;
        let border = cx.theme().colors().border;
        let (label, current) = match slot {
            SignatureSlot::NewMail => (Key::SignatureForNew, account.signature_new),
            SignatureSlot::Reply => (Key::SignatureForReplies, account.signature_reply),
        };
        let current_name = current
            .and_then(|id| self.signatures.iter().find(|entry| entry.id == id))
            .map_or_else(
                || Key::SignatureNone.tr(language).to_string(),
                |entry| entry.name.clone(),
            );
        let key = (account.id, slot);
        let menu = (self.defaults_menu == Some(key)).then(|| {
            let view = cx.weak_entity();
            let account_id = account.id;
            let pick = |id: Option<i64>| {
                let view = view.clone();
                move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                    let _ = view.update(cx, |this, cx| this.set_default(account_id, slot, id, cx));
                }
            };
            let entry = |menu: ContextMenu, label: SharedString, checked: bool, id: Option<i64>| {
                if checked {
                    menu.entry_with_key_hint(label, "✓", pick(id))
                } else {
                    menu.entry(label, pick(id))
                }
            };
            let mut menu = entry(
                ContextMenu::new(),
                Key::SignatureNone.tr(language).into(),
                current.is_none(),
                None,
            );
            for signature in &self.signatures {
                menu = entry(
                    menu,
                    signature.name.clone().into(),
                    current == Some(signature.id),
                    Some(signature.id),
                );
            }
            deferred(div().absolute().left_0().top(px(30.0)).child(menu)).with_priority(1)
        });
        let slot_ix = match slot {
            SignatureSlot::NewMail => 0,
            SignatureSlot::Reply => 1,
        };
        h_flex()
            .gap_2()
            .child(
                Label::new(label.tr(language))
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(
                h_flex()
                    .id(("signature-default", account.id as usize * 2 + slot_ix))
                    .relative()
                    .h(px(26.0))
                    .px_2()
                    .gap_1()
                    .rounded_md()
                    .border_1()
                    .border_color(border)
                    .cursor_pointer()
                    .hover(move |el| el.bg(hover))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_defaults_menu(key, cx)))
                    .child(Label::new(current_name).size(LabelSize::Small))
                    .child(
                        Icon::new(IconName::CaretDown)
                            .size(IconSize::XXSmall)
                            .color(Color::Muted),
                    )
                    .children(menu),
            )
    }
}

impl Render for SignaturesPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let language = cx.language();
        self.name.update(cx, |input, _| {
            input.set_placeholder(Key::SignatureName.tr(language));
        });
        self.editor.update(cx, |area, _| {
            area.set_placeholder(Key::SignaturePlaceholder.tr(language));
        });
        let menu_open = self.defaults_menu.is_some() || self.format_bar.read(cx).menu_open();
        v_flex()
            .relative()
            .gap_4()
            .child(
                Label::new(Key::SignaturesHint.tr(language))
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(self.render_list(language, cx))
            .child(
                h_flex().child(
                    Button::new("signature-new", Key::SignatureNew.tr(language))
                        .icon(IconName::Plus)
                        .style(ButtonStyle::Filled)
                        .on_click(cx.listener(|this, _, window, cx| this.create(window, cx))),
                ),
            )
            .children(
                self.selected
                    .map(|id| self.render_editor(id, language, window, cx)),
            )
            .child(div().h(px(1.0)).bg(cx.theme().colors().border))
            .child(self.render_defaults(language, cx))
            // Outside clicks close a dropdown (they are deferred, above this catcher).
            .when(menu_open, |el| {
                el.child(
                    div()
                        .absolute()
                        .inset_0()
                        .occlude()
                        .on_any_mouse_down(cx.listener(|this, _, _, cx| this.close_menus(cx))),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui::RichDocument;

    /// The page over a database with one sending account, in a window.
    fn page<'a>(
        path: &std::path::Path,
        cx: &'a mut gpui::TestAppContext,
    ) -> (
        i64,
        Entity<RootView>,
        Entity<SignaturesPage>,
        &'a mut gpui::VisualTestContext,
    ) {
        cx.update(|cx| {
            theme::init(theme::Appearance::Dark, cx);
            crate::locale::init(Language::default(), cx);
        });
        let account = storage::Database::open(path)
            .unwrap()
            .create_account("Ann", "ann@x.test", storage::PROVIDER_IMAP, "{}")
            .unwrap();
        let (root, cx) = cx.add_window_view(|_, _| RootView::for_tests(path));
        let weak = root.downgrade();
        let page = cx.new(|cx| SignaturesPage::new(weak, cx));
        (account, root, page, cx)
    }

    #[gpui::test]
    fn a_new_signature_is_named_and_saves_its_edits(cx: &mut gpui::TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (_, root, page, cx) = page(&dir.path().join("mail.db"), cx);
        page.update_in(cx, |page, window, cx| page.create(window, cx));
        let id = page
            .read_with(cx, |page, _| page.selected)
            .expect("selected");
        page.update(cx, |page, cx| {
            let mut bold = RichDocument::plain("Ann");
            bold.rich.update_chars(0..3, |style| style.bold = true);
            page.editor.update(cx, |area, cx| {
                area.replace_range_with_document(0..0, &bold, cx);
                // As typing does.
                cx.emit(TextAreaEvent::Changed);
            });
            page.name
                .update(cx, |input, cx| input.set_content("  ", cx));
            // A name typed blank keeps the one it had.
            assert_eq!(page.edited_signature(cx).unwrap().1, "Signature 1");
        });
        cx.executor().advance_clock(SAVE_DELAY * 2);
        cx.run_until_parked();
        let saved = root.read_with(cx, |root, _| root.signatures());
        assert_eq!(saved.len(), 1);
        assert_eq!((saved[0].id, saved[0].name.as_str()), (id, "Signature 1"));
        assert_eq!(saved[0].document.text, "Ann");
        assert!(!saved[0].document.rich.is_plain());

        // Typing a name, then switching away, saves without waiting.
        page.update_in(cx, |page, window, cx| {
            page.name
                .update(cx, |input, cx| input.set_content("Work", cx));
            page.schedule_save(cx);
            page.create(window, cx);
        });
        let names: Vec<String> = root
            .read_with(cx, |root, _| root.signatures())
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(names, ["Work", "Signature 2"]);
    }

    #[gpui::test]
    fn edits_still_waiting_are_saved_when_the_page_goes(cx: &mut gpui::TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (_, root, page, cx) = page(&dir.path().join("mail.db"), cx);
        page.update_in(cx, |page, window, cx| {
            page.create(window, cx);
            page.name
                .update(cx, |input, cx| input.set_content("Closing", cx));
            page.schedule_save(cx);
        });
        // The window closes (or the app quits) before the pause is over.
        drop(page);
        cx.update(|_, _| {});
        cx.run_until_parked();
        let names: Vec<String> = root
            .read_with(cx, |root, _| root.signatures())
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(names, ["Closing"]);
    }

    #[gpui::test]
    fn an_image_still_being_picked_stays_with_its_signature(cx: &mut gpui::TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (_, _, page, cx) = page(&dir.path().join("mail.db"), cx);
        page.update_in(cx, |page, window, cx| {
            page.create(window, cx);
            page.format_bar
                .update(cx, |bar, _| bar.start_image_for_tests());
            // Another signature is shown before the file is picked.
            page.create(window, cx);
            assert!(!page.format_bar.read(cx).image_pending());
        });
    }

    #[gpui::test]
    fn at_quit_edits_still_waiting_go_straight_to_the_database(cx: &mut gpui::TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mail.db");
        let (_, root, page, cx) = page(&path, cx);
        page.update_in(cx, |page, window, cx| {
            page.create(window, cx);
            page.name
                .update(cx, |input, cx| input.set_content("At quit", cx));
            page.schedule_save(cx);
            // The main view is already gone.
            page.root = WeakEntity::new_invalid();
        });
        drop(page);
        cx.update(|_, _| {});
        cx.run_until_parked();
        let stored = storage::Database::open(&path)
            .unwrap()
            .list_signatures()
            .unwrap();
        assert_eq!(stored[0].name, "At quit");
        drop(root);
    }

    #[gpui::test]
    fn clicking_the_signature_shown_keeps_its_undo(cx: &mut gpui::TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (_, _, page, cx) = page(&dir.path().join("mail.db"), cx);
        page.update_in(cx, |page, window, cx| {
            page.create(window, cx);
            page.editor.update(cx, |area, cx| {
                area.replace_range_with_document(0..0, &RichDocument::plain("Ann"), cx);
            });
            page.select(page.selected, cx);
            assert!(page.editor.read(cx).can_undo());
            assert_eq!(page.editor.read(cx).content(), "Ann");
        });
    }

    #[gpui::test]
    fn its_dropdowns_close_together(cx: &mut gpui::TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (account, _, page, cx) = page(&dir.path().join("mail.db"), cx);
        page.update(cx, |page, cx| {
            page.toggle_defaults_menu((account, SignatureSlot::NewMail), cx);
            assert!(page.defaults_menu_open());
            page.toggle_defaults_menu((account, SignatureSlot::NewMail), cx);
            assert!(!page.defaults_menu_open(), "a second click closes it");
            page.toggle_defaults_menu((account, SignatureSlot::Reply), cx);
            page.close_menus(cx);
            assert!(!page.defaults_menu_open());
        });
    }

    #[gpui::test]
    fn each_account_picks_its_signatures_and_deleting_one_clears_them(
        cx: &mut gpui::TestAppContext,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let (account, root, page, cx) = page(&dir.path().join("mail.db"), cx);
        page.update_in(cx, |page, window, cx| page.create(window, cx));
        let id = page.read_with(cx, |page, _| page.selected).unwrap();
        page.update(cx, |page, cx| {
            page.toggle_defaults_menu((account, SignatureSlot::Reply), cx);
            assert_eq!(page.defaults_menu, Some((account, SignatureSlot::Reply)));
            page.set_default(account, SignatureSlot::Reply, Some(id), cx);
            assert_eq!(page.defaults_menu, None);
            page.set_default(account, SignatureSlot::NewMail, Some(id), cx);
        });
        let chosen = |root: &Entity<RootView>, cx: &mut gpui::VisualTestContext| {
            root.read_with(cx, |root, _| {
                let account = root.accounts()[0].clone();
                (account.signature_new, account.signature_reply)
            })
        };
        assert_eq!(chosen(&root, cx), (Some(id), Some(id)));
        page.update(cx, |page, cx| page.delete(id, cx));
        assert_eq!(chosen(&root, cx), (None, None));
        page.read_with(cx, |page, _| {
            assert!(page.signatures.is_empty() && page.selected.is_none());
        });
    }
}
