### `src/data.rs`

#### Types / constants

- **`EMBEDDED_IMAGE_BYTES`** (private, L17)
  - Signature: `const EMBEDDED_IMAGE_BYTES: &[u8]`
  - Purpose: Raw PNG bytes baked into the binary for the first sample message.
  - Behavior: Loaded via `include_bytes!` from `assets/tweezers.png`; encoded to a `data:` URI for inline webview rendering without file access.

- **`EMBEDDED_IMAGE_WIDTH`** (private, L22)
  - Signature: `const EMBEDDED_IMAGE_WIDTH: u32 = 700`
  - Purpose: Display width (px) for the embedded sample image.
  - Behavior: Matches asset intrinsic width; referenced in generated HTML `width` attributes and layout tests.

- **`EMBEDDED_IMAGE_HEIGHT`** (private, L23)
  - Signature: `const EMBEDDED_IMAGE_HEIGHT: u32 = 200`
  - Purpose: Display height (px) for the embedded sample image.
  - Behavior: Matches asset intrinsic height; referenced in generated HTML `height` attributes.

- **`MailboxKind`** (pub, L27)
  - Signature: `pub enum MailboxKind { Inbox, Drafts, Sent, Junk, Trash, Archive, Custom }`
  - Purpose: Semantic kind of a mailbox for icons and localized naming.
  - Behavior: Standard kinds map to locale keys; `Custom` carries its label on `Mailbox::label` instead.

- **`GlobalMailbox`** (pub, L66)
  - Signature: `pub enum GlobalMailbox { Inbox, Flagged, Drafts, Sent }`
  - Purpose: Unified sidebar mailbox aggregating the same logical folder across accounts (mock-only).
  - Behavior: Display order defined by `ALL`; each variant has a dedicated locale key.

- **`GlobalMailbox::ALL`** (pub, L75)
  - Signature: `pub const ALL: [GlobalMailbox; 4]`
  - Purpose: Ordered list of global mailboxes for sidebar rendering.
  - Behavior: `[Inbox, Flagged, Drafts, Sent]`.

- **`Mailbox`** (pub, L101)
  - Signature: `pub struct Mailbox { kind, unread, label }`
  - Purpose: One mailbox row within an account in mock data.
  - Behavior: Standard mailboxes use kind-derived names; custom folders set `label` and `kind: Custom`.

- **`Account`** (pub, L140)
  - Signature: `pub struct Account { name, email, mailboxes }`
  - Purpose: Mock connected e-mail account with nested mailboxes.
  - Behavior: Used by sidebar, settings, and compose mock selectors.

- **`MessageBody`** (pub, L149)
  - Signature: `pub enum MessageBody { Html(SharedString), Text(SharedString) }`
  - Purpose: Reader content variant for HTML or plain-text bodies.
  - Behavior: Carries full body string; HTML is rendered in the webview, text in a plain viewer.

- **`Message`** (pub, L156)
  - Signature: `pub struct Message { sender, sender_email, subject, preview, body, time, unread, starred, has_attachment }`
  - Purpose: Mock list/detail message model for the visual prototype.
  - Behavior: Populates message list and seeds storage via `db_seed`.

- **`BASE64_ALPHABET`** (private, L535)
  - Signature: `const BASE64_ALPHABET: &[u8; 64]`
  - Purpose: RFC 4648 base64 alphabet for the inline encoder.
  - Behavior: Used by `base64_encode` when building embedded image data URIs.

#### Functions / methods

##### Context: `MailboxKind`

- **`name_key`** (private, L42)
  - Signature: `fn name_key(self) -> Option<Key>`
  - Purpose: Locale key for standard mailbox names.
  - Behavior: Returns `Some(Key::…)` for built-in kinds and `None` for `Custom`.

- **`display_name`** (pub, L56)
  - Signature: `pub fn display_name(self, language: Language) -> &'static str`
  - Purpose: Localized name for a standard mailbox kind.
  - Behavior: Translates via `name_key`; returns empty string for `Custom` (see `Mailbox::display_name`).

##### Context: `GlobalMailbox`

- **`name_key`** (private, L83)
  - Signature: `fn name_key(self) -> Key`
  - Purpose: Locale key for a global mailbox label.
  - Behavior: Maps each variant to its corresponding `Key` (Inbox reuses mailbox inbox key).

- **`display_name`** (pub, L93)
  - Signature: `pub fn display_name(self, language: Language) -> &'static str`
  - Purpose: Localized label for a global mailbox.
  - Behavior: Resolves `name_key().tr(language)`.

##### Context: `Mailbox`

- **`new`** (private, L110)
  - Signature: `fn new(kind: MailboxKind, unread: usize) -> Self`
  - Purpose: Constructs a standard mailbox with no custom label.
  - Behavior: Sets `label: None` and stores kind plus unread count.

- **`custom`** (private, L119)
  - Signature: `fn custom(name: impl Into<SharedString>, unread: usize) -> Self`
  - Purpose: Constructs a user-created folder mailbox.
  - Behavior: Sets `kind: Custom` and stores explicit `label`.

- **`display_name`** (pub, L130)
  - Signature: `pub fn display_name(&self, language: Language) -> SharedString`
  - Purpose: Sidebar/settings display name for any mailbox.
  - Behavior: Returns custom label when present; otherwise localized kind name.

##### Context: `module`

- **`default_mailboxes`** (private, L169)
  - Signature: `fn default_mailboxes(inbox_unread: usize) -> Vec<Mailbox>`
  - Purpose: Standard six-mailbox set for mock accounts.
  - Behavior: Inbox (with given unread), Drafts, Sent, Junk (3 unread), Trash, Archive.

- **`sample_accounts`** (pub, L182)
  - Signature: `pub fn sample_accounts() -> Vec<Account>`
  - Purpose: Multi-account mock data exercising sidebar overflow and custom folders.
  - Behavior: Returns five accounts with varied unread counts; Work account adds six custom folders including a long-name truncation case.

- **`sample_messages`** (pub, L230)
  - Signature: `pub fn sample_messages() -> Vec<Message>`
  - Purpose: Mock inbox message list with diverse metadata and bodies.
  - Behavior: Builds 18 messages from static tuples; first message uses rich HTML with embedded `data:` image; others cycle through `sample_email_bodies()`.

- **`html_body`** (private, L450)
  - Signature: `fn html_body(subject: &str, preview: &str, sender: &str, image_src: &str) -> SharedString`
  - Purpose: Generates rich HTML exercising reader features.
  - Behavior: Includes headings, lists, blockquote, code block, inline/remote images, normal and ~2000-character links for status-bar overflow tests.

- **`sample_email_bodies`** (pub, L515)
  - Signature: `pub fn sample_email_bodies() -> Vec<MessageBody>`
  - Purpose: Catalog of reusable HTML/text fixtures from `assets/emails/`.
  - Behavior: Returns 14 `include_str!` bodies (mostly HTML, one plain text) for mocks and tests.

- **`base64_encode`** (private, L540)
  - Signature: `fn base64_encode(input: &[u8]) -> String`
  - Purpose: Minimal dependency-free base64 encoder with padding.
  - Behavior: Encodes 3-byte chunks using `BASE64_ALPHABET`; emits `=` padding as needed.

- **`embedded_image_data_uri`** (private, L564)
  - Signature: `fn embedded_image_data_uri() -> String`
  - Purpose: Self-contained PNG data URI for the first sample message.
  - Behavior: Prefixes `data:image/png;base64,` with base64 of `EMBEDDED_IMAGE_BYTES`.

- **`accounts_have_default_mailboxes`** (private, L576)
  - Signature: `fn accounts_have_default_mailboxes()` (test)
  - Purpose: Validates every sample account has six standard mailboxes in order.
  - Behavior: Asserts five accounts, each with at least six mailboxes matching expected kinds.

- **`work_account_has_custom_folders`** (private, L597)
  - Signature: `fn work_account_has_custom_folders()` (test)
  - Purpose: Ensures Work account custom folder seeding.
  - Behavior: Expects 12 mailboxes total and six `Custom` folders; first custom named "Clients".

- **`global_mailboxes_are_localized`** (private, L616)
  - Signature: `fn global_mailboxes_are_localized()` (test)
  - Purpose: Checks global mailbox translations and inbox label reuse.
  - Behavior: Compares English/Portuguese Flagged labels; Inbox matches `MailboxKind::Inbox` label.

- **`custom_folder_name_is_not_localized`** (private, L633)
  - Signature: `fn custom_folder_name_is_not_localized()` (test)
  - Purpose: Ensures custom folder labels are language-independent.
  - Behavior: Same display name in English and Portuguese for a custom folder.

- **`sample_messages_are_populated`** (private, L642)
  - Signature: `fn sample_messages_are_populated()` (test)
  - Purpose: Sanity-checks mock message list diversity.
  - Behavior: Non-empty list with at least one unread and one attachment flag.

- **`sample_email_bodies_has_at_least_ten_varied_contents`** (private, L650)
  - Signature: `fn sample_email_bodies_has_at_least_ten_varied_contents()` (test)
  - Purpose: Guards fixture catalog size.
  - Behavior: Asserts at least ten bodies in `sample_email_bodies()`.

- **`sample_email_bodies_are_all_non_empty`** (private, L660)
  - Signature: `fn sample_email_bodies_are_all_non_empty()` (test)
  - Purpose: Ensures no empty fixture files.
  - Behavior: Trims each HTML/text body and rejects empty strings.

- **`sample_email_bodies_mix_html_and_text`** (private, L670)
  - Signature: `fn sample_email_bodies_mix_html_and_text()` (test)
  - Purpose: Confirms fixture catalog includes both formats.
  - Behavior: Asserts at least one Html and one Text variant in the catalog.

- **`sample_messages_include_html_and_text_bodies`** (private, L677)
  - Signature: `fn sample_messages_include_html_and_text_bodies()` (test)
  - Purpose: Confirms assembled messages use both body kinds.
  - Behavior: Scans `sample_messages()` for Html and Text bodies.

- **`first_message_embeds_the_image_as_a_data_uri`** (private, L688)
  - Signature: `fn first_message_embeds_the_image_as_a_data_uri()` (test)
  - Purpose: Validates inline image embedding in the first HTML message.
  - Behavior: Expects `data:image/png;base64,` substring and explicit width/height attributes.

- **`base64_encodes_known_vectors`** (private, L705)
  - Signature: `fn base64_encodes_known_vectors()` (test)
  - Purpose: RFC 4648 vector coverage for the inline encoder.
  - Behavior: Checks empty input and classic padding cases (`f`, `fo`, `foo`, etc.).

- **`embedded_image_data_uri_is_well_formed`** (private, L716)
  - Signature: `fn embedded_image_data_uri_is_well_formed()` (test)
  - Purpose: Ensures data URI prefix and non-empty payload.
  - Behavior: Starts with PNG data-URI scheme and has content beyond the prefix.

- **`embedded_image_is_a_decodable_raster`** (private, L723)
  - Signature: `fn embedded_image_is_a_decodable_raster()` (test)
  - Purpose: Guards against non-raster bytes in the embedded asset.
  - Behavior: Asserts PNG or JPEG magic bytes at start of `EMBEDDED_IMAGE_BYTES`.

- **`embedded_image_is_wider_than_a_typical_reading_pane`** (private, L737)
  - Signature: `fn embedded_image_is_wider_than_a_typical_reading_pane()` (test)
  - Purpose: Ensures horizontal scrollbar exercise in the reader.
  - Behavior: Requires `EMBEDDED_IMAGE_WIDTH >= 640`.

- **`unread_count_matches_first_account`** (private, L744)
  - Signature: `fn unread_count_matches_first_account()` (test)
  - Purpose: Spot-checks Personal account inbox unread mock count.
  - Behavior: Expects 5000 unread on first account's inbox.

- **`mailbox_names_are_localized`** (private, L750)
  - Signature: `fn mailbox_names_are_localized()` (test)
  - Purpose: Verifies inbox localization in English and Portuguese.
  - Behavior: Expects "Inbox" and "Caixa de entrada" for `MailboxKind::Inbox`.
