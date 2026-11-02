//! The To / Cc / Bcc fields: addresses as cards, as in Gmail, offering contacts
//! while typing.

use std::rc::Rc;

use gpui::{
    deferred, App, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, KeyDownEvent,
    MouseButton, Render, SharedString, Subscription, Window,
};
use mail_core::{format_address_list, parse_address_list, same_email, Address};
use storage::{fold_for_search, Contact};
use theme::ActiveTheme;
use ui::prelude::*;
use ui::{
    Backspace, Color, FocusNext, Icon, IconName, IconSize, Label, LabelSize, TextInput,
    TextInputEvent, Tooltip,
};

use crate::locale::{ActiveLanguage, Key};

/// Most contacts offered at once.
const MAX_SUGGESTIONS: usize = 6;
/// Narrowest the typing area gets before wrapping below the cards, in pixels.
const INPUT_MIN_WIDTH: f32 = 120.0;
const SUGGESTIONS_WIDTH: f32 = 380.0;

/// Emitted when the addresses or the text being typed change.
pub struct RecipientsChanged;

/// Contacts for what is being typed, leaving out those already added: first those
/// with a word or their address starting with it, then the others containing it,
/// ignoring accents and case.
fn matching_contacts(contacts: &[Contact], added: &[Address], query: &str) -> Vec<Contact> {
    let query = fold_for_search(query.trim());
    if query.is_empty() {
        return Vec::new();
    }
    let rank = |contact: &Contact| {
        let name = fold_for_search(&contact.name);
        let email = fold_for_search(&contact.email);
        if email.starts_with(&query) || name.split_whitespace().any(|w| w.starts_with(&query)) {
            Some(0)
        } else if email.contains(&query) || name.contains(&query) {
            Some(1)
        } else {
            None
        }
    };
    let mut found: Vec<(u8, &Contact)> = contacts
        .iter()
        .filter(|contact| !added.iter().any(|a| same_email(&a.email, &contact.email)))
        .filter_map(|contact| rank(contact).map(|rank| (rank, contact)))
        .collect();
    found.sort_by_key(|(rank, _)| *rank);
    found
        .into_iter()
        .take(MAX_SUGGESTIONS)
        .map(|(_, contact)| contact.clone())
        .collect()
}

/// `address` with the catalog's name, when it came without one.
fn with_known_name(address: Address, contacts: &[Contact]) -> Address {
    if address.name.is_some() {
        return address;
    }
    let name = contacts
        .iter()
        .find(|contact| same_email(&contact.email, &address.email))
        .map(|contact| contact.name.clone());
    Address::new(name, address.email)
}

/// Splits typed text into the addresses that are valid and the rest, which stays
/// in the field for the user to fix.
fn split_typed(text: &str) -> (Vec<Address>, String) {
    let mut valid = Vec::new();
    let mut rest = Vec::new();
    for token in text.split([',', ';']) {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        match parse_address_list(token) {
            Ok(addresses) => valid.extend(addresses),
            Err(_) => rest.push(token),
        }
    }
    (valid, rest.join(", "))
}

fn add_unique(addresses: &mut Vec<Address>, new: Address) {
    if !addresses.iter().any(|a| same_email(&a.email, &new.email)) {
        addresses.push(new);
    }
}

/// Hint of the typing area: none once there are cards.
fn placeholder_for(placeholder: &SharedString, addresses: &[Address]) -> SharedString {
    if addresses.is_empty() {
        placeholder.clone()
    } else {
        SharedString::default()
    }
}

/// What a card shows: the name when there is one, as in Gmail.
fn card_label(address: &Address) -> String {
    address
        .name
        .clone()
        .unwrap_or_else(|| address.email.clone())
}

/// One recipient field.
pub struct RecipientInput {
    addresses: Vec<Address>,
    input: Entity<TextInput>,
    placeholder: SharedString,
    contacts: Rc<[Contact]>,
    suggestions: Vec<Contact>,
    highlighted: usize,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<RecipientsChanged> for RecipientInput {}

impl Focusable for RecipientInput {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.read(cx).focus_handle(cx)
    }
}

impl RecipientInput {
    pub fn new(
        placeholder: impl Into<SharedString>,
        contacts: Rc<[Contact]>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let placeholder = placeholder.into();
        let input = cx.new(|cx| TextInput::new(placeholder.clone(), cx));
        let focus = input.read(cx).focus_handle(cx);
        let subscriptions = vec![
            cx.subscribe(&input, |this, _, _: &TextInputEvent, cx| {
                this.on_typed(cx);
            }),
            // Leaving the field turns what is complete into cards.
            cx.on_focus_out(&focus, window, |this, _, _, cx| {
                this.suggestions.clear();
                this.commit_typed(cx);
            }),
        ];
        Self {
            addresses: Vec::new(),
            input,
            placeholder,
            contacts,
            suggestions: Vec::new(),
            highlighted: 0,
            _subscriptions: subscriptions,
        }
    }

    /// The hint only makes sense while there are no cards.
    fn sync_placeholder(&mut self, cx: &mut Context<Self>) {
        let placeholder = placeholder_for(&self.placeholder, &self.addresses);
        self.input.update(cx, |input, cx| {
            input.set_placeholder(placeholder);
            cx.notify();
        });
    }

    /// Replaces the catalog, which may have grown since the window opened.
    pub fn set_contacts(&mut self, contacts: Rc<[Contact]>) {
        self.contacts = contacts;
    }

    /// Replaces the field's content.
    pub fn set_addresses(&mut self, addresses: &[Address], cx: &mut Context<Self>) {
        self.addresses = addresses
            .iter()
            .cloned()
            .map(|address| with_known_name(address, &self.contacts))
            .collect();
        self.suggestions.clear();
        self.input.update(cx, |input, cx| input.clear(cx));
        self.sync_placeholder(cx);
        cx.notify();
    }

    /// The field as an address list: its cards and whatever is still being typed,
    /// so a send validates and includes it.
    pub fn content(&self, cx: &App) -> String {
        let cards = format_address_list(&self.addresses);
        let typed = self.input.read(cx).content();
        match (cards.is_empty(), typed.trim().is_empty()) {
            (_, true) => cards,
            (true, false) => typed.to_string(),
            (false, false) => format!("{cards}, {typed}"),
        }
    }

    fn typed(&self, cx: &App) -> SharedString {
        self.input.read(cx).content()
    }

    fn on_typed(&mut self, cx: &mut Context<Self>) {
        let typed = self.typed(cx);
        // A separator typed or pasted ends an address.
        if typed.contains([',', ';']) {
            self.commit_typed(cx);
        } else {
            self.suggestions = matching_contacts(&self.contacts, &self.addresses, &typed);
            self.highlighted = 0;
        }
        cx.emit(RecipientsChanged);
        cx.notify();
    }

    /// Turns valid typed addresses into cards; invalid text stays to be fixed.
    fn commit_typed(&mut self, cx: &mut Context<Self>) {
        let typed = self.typed(cx);
        let (valid, rest) = split_typed(&typed);
        if valid.is_empty() && rest == typed.trim() {
            return;
        }
        for address in valid {
            add_unique(
                &mut self.addresses,
                with_known_name(address, &self.contacts),
            );
        }
        self.input
            .update(cx, |input, cx| input.set_content(rest, cx));
        self.suggestions.clear();
        self.sync_placeholder(cx);
        cx.emit(RecipientsChanged);
        cx.notify();
    }

    fn accept(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(contact) = self.suggestions.get(index).cloned() else {
            return;
        };
        add_unique(
            &mut self.addresses,
            Address::new(Some(contact.name), contact.email),
        );
        self.input.update(cx, |input, cx| input.clear(cx));
        self.suggestions.clear();
        self.sync_placeholder(cx);
        window.focus(&self.input.read(cx).focus_handle(cx));
        cx.emit(RecipientsChanged);
        cx.notify();
    }

    fn remove(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.addresses.len() {
            self.addresses.remove(index);
            self.sync_placeholder(cx);
            cx.emit(RecipientsChanged);
            cx.notify();
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.control || keystroke.modifiers.platform || keystroke.modifiers.alt {
            return;
        }
        let open = !self.suggestions.is_empty();
        match keystroke.key.as_str() {
            "down" if open => {
                self.highlighted = (self.highlighted + 1) % self.suggestions.len();
            }
            "up" if open => {
                self.highlighted =
                    (self.highlighted + self.suggestions.len() - 1) % self.suggestions.len();
            }
            "escape" if open => self.suggestions.clear(),
            "enter" if open => self.accept(self.highlighted, window, cx),
            "enter" if !self.typed(cx).trim().is_empty() => self.commit_typed(cx),
            // With Shift it is another character (`<`, `:`): a name being typed.
            "," | ";" if !keystroke.modifiers.shift => self.commit_typed(cx),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn render_card(
        &self,
        index: usize,
        address: &Address,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let language = cx.language();
        let colors = cx.theme().colors();
        h_flex()
            .id(("recipient", index))
            .flex_shrink_0()
            .h(px(22.0))
            .pl_2()
            .pr_1()
            .gap_1()
            .rounded_full()
            .border_1()
            .border_color(colors.border)
            .bg(colors.element_background)
            .tooltip(Tooltip::text(address.display()))
            .child(
                Label::new(card_label(address))
                    .size(LabelSize::Small)
                    .single_line(),
            )
            .child(
                div()
                    .id(("recipient-remove", index))
                    .cursor_pointer()
                    .tooltip(Tooltip::text(Key::ComposeRemoveRecipient.tr(language)))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, _, cx| this.remove(index, cx)))
                    .child(
                        Icon::new(IconName::Clear)
                            .size(IconSize::XSmall)
                            .color(Color::Muted),
                    ),
            )
    }

    fn render_suggestions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let (surface, border, hover, selected) = (
            colors.elevated_surface_background,
            colors.border,
            colors.element_hover,
            colors.element_selected,
        );
        let rows = self.suggestions.iter().enumerate().map(|(ix, contact)| {
            let named = !contact.name.is_empty();
            v_flex()
                .id(("recipient-suggestion", ix))
                .px_2()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .when(ix == self.highlighted, |el| el.bg(selected))
                .hover(move |el| el.bg(hover))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, window, cx| this.accept(ix, window, cx)))
                .when(named, |el| {
                    el.child(
                        Label::new(contact.name.clone())
                            .size(LabelSize::Small)
                            .weight(FontWeight::SEMIBOLD)
                            .single_line(),
                    )
                })
                .child(
                    Label::new(contact.email.clone())
                        .size(LabelSize::XSmall)
                        .color(if named { Color::Muted } else { Color::Default })
                        .single_line(),
                )
        });
        deferred(
            v_flex()
                .absolute()
                .top_full()
                .left_0()
                .mt_1()
                .w(px(SUGGESTIONS_WIDTH))
                .p_1()
                .occlude()
                .rounded_md()
                .border_1()
                .border_color(border)
                .bg(surface)
                .shadow_lg()
                .children(rows.collect::<Vec<_>>()),
        )
        .with_priority(2)
    }
}

impl Render for RecipientInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let addresses = self.addresses.clone();
        let cards: Vec<_> = addresses
            .iter()
            .enumerate()
            .map(|(ix, address)| self.render_card(ix, address, cx).into_any_element())
            .collect();
        let suggestions = (!self.suggestions.is_empty()).then(|| self.render_suggestions(cx));
        let focus = self.focus_handle(cx);
        h_flex()
            .relative()
            .w_full()
            .flex_wrap()
            .gap_1()
            .cursor_text()
            // Clicks beside the cards go to the typing area.
            .on_mouse_down(MouseButton::Left, move |_, window, _| window.focus(&focus))
            .on_key_down(cx.listener(Self::on_key_down))
            .capture_action(cx.listener(|this, _: &FocusNext, window, cx| {
                if !this.suggestions.is_empty() {
                    this.accept(this.highlighted, window, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &Backspace, _, cx| {
                if this.typed(cx).is_empty() && !this.addresses.is_empty() {
                    this.remove(this.addresses.len() - 1, cx);
                    cx.stop_propagation();
                }
            }))
            .children(cards)
            .child(
                div()
                    .flex_1()
                    .min_w(px(INPUT_MIN_WIDTH))
                    .child(self.input.clone()),
            )
            .children(suggestions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contact(name: &str, email: &str) -> Contact {
        Contact {
            name: name.into(),
            email: email.into(),
        }
    }

    fn emails(contacts: &[Contact]) -> Vec<&str> {
        contacts.iter().map(|c| c.email.as_str()).collect()
    }

    #[test]
    fn contacts_match_by_name_or_address_ignoring_accents() {
        let contacts = [
            contact("João Silva", "joao@x.test"),
            contact("Maria", "maria@joaopessoa.test"),
            contact("", "other@x.test"),
        ];
        let found = matching_contacts(&contacts, &[], "joa");
        // A word starting with it comes before an address merely containing it.
        assert_eq!(emails(&found), ["joao@x.test", "maria@joaopessoa.test"]);
        assert_eq!(emails(&matching_contacts(&contacts, &[], "JOÃO")).len(), 2);
        assert_eq!(
            emails(&matching_contacts(&contacts, &[], "oth")),
            ["other@x.test"]
        );
        assert!(matching_contacts(&contacts, &[], "  ").is_empty());
        assert!(matching_contacts(&contacts, &[], "zzz").is_empty());
    }

    #[test]
    fn suggestions_skip_who_was_added_and_are_limited() {
        let contacts: Vec<_> = (0..20)
            .map(|ix| contact("Ann", &format!("ann{ix}@x.test")))
            .collect();
        assert_eq!(
            matching_contacts(&contacts, &[], "ann").len(),
            MAX_SUGGESTIONS
        );
        let added = [Address::new(None::<String>, "ANN0@x.test")];
        assert!(!emails(&matching_contacts(&contacts, &added, "ann")).contains(&"ann0@x.test"));
    }

    #[test]
    fn a_full_address_takes_the_name_from_the_catalog() {
        let contacts = [contact("Ann", "ann@x.test")];
        let typed = Address::new(None::<String>, "ANN@x.test");
        let known = with_known_name(typed, &contacts);
        assert_eq!(known.name.as_deref(), Some("Ann"));
        assert_eq!(known.email, "ANN@x.test");

        let named = Address::new(Some("Annie"), "ann@x.test");
        assert_eq!(
            with_known_name(named, &contacts).name.as_deref(),
            Some("Annie")
        );
        let unknown = Address::new(None::<String>, "bob@x.test");
        assert_eq!(with_known_name(unknown, &contacts).name, None);
        // A contact with no name yet gives none.
        let nameless = [contact("", "bob@x.test")];
        let bob = Address::new(None::<String>, "bob@x.test");
        assert_eq!(with_known_name(bob, &nameless).name, None);
    }

    #[test]
    fn typed_text_splits_into_valid_addresses_and_what_needs_fixing() {
        let (valid, rest) = split_typed("a@x.test, nope; Bob <b@x.test>, ");
        assert_eq!(valid.len(), 2);
        assert_eq!(valid[1].name.as_deref(), Some("Bob"));
        assert_eq!(rest, "nope");
        assert_eq!(split_typed(""), (Vec::new(), String::new()));
    }

    #[test]
    fn the_same_address_is_added_once() {
        let mut addresses = vec![Address::new(None::<String>, "a@x.test")];
        add_unique(&mut addresses, Address::new(None::<String>, "A@X.test"));
        assert_eq!(addresses.len(), 1);
        add_unique(&mut addresses, Address::new(None::<String>, "b@x.test"));
        assert_eq!(addresses.len(), 2);
    }

    #[test]
    fn the_hint_shows_only_without_cards() {
        let hint = SharedString::from("Type here");
        assert_eq!(placeholder_for(&hint, &[]), hint);
        let cards = [Address::new(None::<String>, "a@x.test")];
        assert_eq!(placeholder_for(&hint, &cards), "");
    }

    #[test]
    fn cards_show_the_name_when_there_is_one() {
        assert_eq!(card_label(&Address::new(Some("Ann"), "a@x.test")), "Ann");
        assert_eq!(
            card_label(&Address::new(None::<String>, "a@x.test")),
            "a@x.test"
        );
    }
}
