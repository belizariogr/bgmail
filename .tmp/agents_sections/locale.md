### `src/locale.rs`

#### Types / constants

- **`Language`** (pub, L15)
  - Signature: `pub enum Language { English, Portuguese }`
  - Purpose: Supported UI languages (English default).
  - Behavior: Stored as a GPUI global; resolved at render time via `ActiveLanguage`.

- **`Language::ALL`** (pub, L25)
  - Signature: `pub const ALL: [Language; 2]`
  - Purpose: Ordered list for language selectors and exhaustive translation tests.
  - Behavior: `[English, Portuguese]`.

- **`Key`** (pub, L38)
  - Signature: `pub enum Key { … }` (64 UI string keys)
  - Purpose: Typed catalog of translatable UI chrome strings.
  - Behavior: Each variant maps to per-language text via `tr`; covers mailboxes, settings, toolbar, commands, compose, context menu, and status strings.

- **`GlobalLanguage`** (private, L332)
  - Signature: `struct GlobalLanguage(Language)`
  - Purpose: GPUI global holding the active UI language.
  - Behavior: Wrapped by `init` / `set_language`; read through `ActiveLanguage for App`.

- **`ActiveLanguage`** (pub, L350)
  - Signature: `pub trait ActiveLanguage { fn language(&self) -> Language; }`
  - Purpose: Trait for reading the active language from GPUI contexts.
  - Behavior: Implemented for `App` to return the inner `Language` from `GlobalLanguage`.

#### Functions / methods

##### Context: `Language`

- **`label`** (pub, L28)
  - Signature: `pub fn label(self) -> &'static str`
  - Purpose: Endonym shown in language picker UI.
  - Behavior: Returns "English" or "Português (Brasil)" for each variant.

##### Context: `Key`

- **`tr`** (pub, L126)
  - Signature: `pub fn tr(self, language: Language) -> &'static str`
  - Purpose: Resolves a UI string key to localized text.
  - Behavior: Large `match` on `(self, language)` returning static English or Brazilian Portuguese strings; includes format placeholders like `{}` for `BlockedElements`.

##### Context: `module`

- **`message_count`** (pub, L295)
  - Signature: `pub fn message_count(language: Language, count: usize) -> String`
  - Purpose: Localized message-list header count.
  - Behavior: English `"{count} messages"`; Portuguese `"{count} mensagens"`.

- **`status_counts`** (pub, L303)
  - Signature: `pub fn status_counts(language: Language, accounts: usize, messages: usize) -> String`
  - Purpose: Status bar left segment with account and message totals.
  - Behavior: English `"{accounts} accounts · {messages} messages"`; Portuguese uses "contas" and "mensagens".

- **`status_search_counts`** (pub, L311)
  - Signature: `pub fn status_search_counts(language: Language, accounts: usize, showing: usize, total: usize) -> String`
  - Purpose: Status bar left segment while search filtering is active.
  - Behavior: English `"{accounts} accounts · {showing} of {total} messages"`; Portuguese uses "de" phrasing.

- **`status_unread`** (pub, L324)
  - Signature: `pub fn status_unread(language: Language, unread: usize) -> String`
  - Purpose: Status bar right segment with unread count and sync hint.
  - Behavior: English `"{unread} unread · Updated just now"`; Portuguese uses "não lidas · Atualizado agora".

- **`init`** (pub, L337)
  - Signature: `pub fn init(language: Language, cx: &mut App)`
  - Purpose: Initializes localization at app startup.
  - Behavior: Sets `GlobalLanguage(language)` on the GPUI app.

- **`set_language`** (pub, L342)
  - Signature: `pub fn set_language(language: Language, cx: &mut App)`
  - Purpose: Switches active language live across all windows.
  - Behavior: Updates global and calls `cx.refresh_windows()` so every open window re-renders with new strings.

- **`every_key_has_text_in_every_language`** (private, L433)
  - Signature: `fn every_key_has_text_in_every_language()` (test)
  - Purpose: Ensures translation completeness for all 64 keys.
  - Behavior: Iterates `ALL_KEYS` × `Language::ALL` asserting non-empty `tr` results.

- **`translations_differ_between_languages`** (private, L445)
  - Signature: `fn translations_differ_between_languages()` (test)
  - Purpose: Spot-checks that locales actually differ where expected.
  - Behavior: Compares inbox and settings title strings across English and Portuguese.

- **`default_language_is_english`** (private, L458)
  - Signature: `fn default_language_is_english()` (test)
  - Purpose: Confirms `Language::default()` is English.
  - Behavior: Equality assertion on default enum value.

- **`formatted_strings_include_arguments`** (private, L463)
  - Signature: `fn formatted_strings_include_arguments()` (test)
  - Purpose: Verifies dynamic status strings embed numeric arguments.
  - Behavior: Checks English status counts contain digits and "accounts"; Portuguese unread string contains count and "não lidas".

##### Context: `ActiveLanguage for App`

- **`language`** (private, L356)
  - Signature: `fn language(&self) -> Language`
  - Purpose: Reads active language from the GPUI app global.
  - Behavior: Returns `self.global::<GlobalLanguage>().0`.
