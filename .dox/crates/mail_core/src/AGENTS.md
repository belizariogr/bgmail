# crates/mail_core/src/ — Source API

## Purpose

Source for the `mail_core` crate: address parsing, flags, folder roles and
modified UTF-7, MIME parse/build, reply/forward drafts, threading helpers,
remote contracts and in-memory fakes.

## Ownership

- Owns: all modules under `crates/mail_core/src/`.
- Does not own: crate-level dependency/feature policy (parent
  `.dox/crates/mail_core/AGENTS.md`).

## Local Contracts

- Every function/method in this tree is listed under **API Catalog** with
  purpose and behavior. Update the matching entry when changing signatures or
  semantics.
- Visibility in entries (`pub` / `pub(crate)` / `private`) reflects the source.
- No GPUI, network or filesystem I/O; `#![forbid(unsafe_code)]` in `lib.rs`.

## Work Guidance

- After adding/removing/renaming a function, update this catalog in the same
  change.
- Shared private helpers: `compose::strip_brackets` (ids) and
  `compose::extension_for_mime` (nameless attachments) are reused by `parse`
  and `thread`; `reply::{REPLY_PREFIXES, FORWARD_PREFIXES}` by `thread`.

## Verification

- `cargo test -p mail_core`
- `cargo clippy -p mail_core --all-targets -- -D warnings`

## Child DOX Index

_(none — modules are files; their APIs are cataloged below.)_

## API Catalog

### `src/lib.rs`

- `#![forbid(unsafe_code)]`; declares modules (`fake` only under
  `cfg(any(test, feature = "fake"))`) and re-exports the main types and
  functions of every module at the crate root. No functions.

### `src/account.rs`

- **enum `Security { Tls (default), StartTls, None }`** (serde lowercase).
  - **`default_imap_port`** (pub) — 993 for Tls, 143 otherwise.
  - **`default_smtp_port`** (pub) — 465 Tls, 587 StartTls, 25 None.
- **struct `ServerConfig { host, port, security }`**.
- **enum `AuthKind { Password (default), OAuth2 { provider } }`** — serde tagged by `kind`.
- **struct `AccountConfig { email, username, imap, smtp, auth, sync_window }`** — `auth`/`sync_window` default when absent (`DEFAULT_SYNC_WINDOW` = 2000).
  - **`from_json`** (pub) — `None` for invalid or local (`{}`) configs.
  - **`to_json`** (pub) — serde JSON (falls back to `{}`).
  - **`secret_key`** (pub) — `bgmail://<username>@<imap host>`; keychain key of the password/refresh token.
  - **`uses_oauth`** (pub) — whether `auth` is `OAuth2`.
- **`default_sync_window`** (private) — serde default for `sync_window`: `DEFAULT_SYNC_WINDOW`.
- Tests: `json_round_trips`, `invalid_or_local_json_is_none`, `missing_auth_and_window_use_defaults`, `oauth_serializes_with_a_kind_tag`, `secret_key_depends_on_login_and_host`, `default_ports_follow_security`.

### `src/cancel.rs`

- **struct `CancelToken`** (Clone shares state, Default, Debug shows the flag).
  - **`new`** (pub) — not cancelled.
  - **`cancel`** (pub) — sets the flag once and runs every registered hook once (later calls no-op).
  - **`is_cancelled`** (pub).
  - **`on_cancel(hook) -> CancelGuard`** (pub) — registers a hook (runs at once when already cancelled); the guard unregisters it on drop.
  - `lock_hooks` / `remove` (private) — poison-tolerant hook list access; removal by id.
  - `hook_count` (`cfg(test)`) — registered hooks.
- **struct `CancelGuard`** — `Drop` unregisters its hook.
- Tests: `cancel_sets_flag_and_runs_hooks_once`, `dropped_guard_unregisters_hook`, `hook_registered_after_cancel_runs_immediately`, `debug_shows_state`.

### `src/address.rs`

#### Types / constants

- **struct `Address { name: Option<String>, email: String }`**: one mailbox.
  Derives Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize.
- **const `SPECIALS`** (private): RFC 5322 specials that force quoting a name.

#### Functions / methods

##### Context: `Address`

- **`new`** (pub) — `pub fn new(name: Option<impl Into<String>>, email: impl Into<String>) -> Self`
  - Purpose: canonical constructor.
  - Behavior: trims name and email; blank name → `None`.
- **`display`** (pub) — `pub fn display(&self) -> String`
  - Purpose: header-style rendering that re-parses.
  - Behavior: no/blank name → bare email; name with specials → `"escaped" <email>` (escapes `\` and `"`); else `Name <email>`.
- **`label`** (pub) — `pub fn label(&self) -> &str`
  - Purpose: short label for chips/lists.
  - Behavior: non-empty name, else email.

##### Context: module

- **`parse_address_list`** (pub) — `pub fn parse_address_list(input: &str) -> Result<Vec<Address>, Vec<String>>`
  - Purpose: parse a user-typed To/Cc field.
  - Behavior: splits on `,`/`;` outside quotes/angle brackets; skips empty tokens (trailing separators, whitespace); accepts `email`, `Name <email>`, `"Quoted, Name" <email>`, `<email>`; `Err` lists every invalid trimmed token (in order) if any; validity = exactly one `@`, non-empty local/domain, no whitespace or `<>,;"`, nothing after `>`.
- **`format_address_list`** (pub) — `pub fn format_address_list(addresses: &[Address]) -> String`
  - Purpose: inverse of `parse_address_list`.
  - Behavior: `display()` of each joined by `", "`; empty slice → `""`.
- **`same_email`** (pub) — `pub fn same_email(a: &str, b: &str) -> bool`
  - Purpose: address identity for dedupe / "is this me".
  - Behavior: trimmed, Unicode-lowercased equality.
- **`split_tokens`** (private) — splits on unquoted, un-bracketed `,`/`;` honoring `\` escapes inside quotes.
- **`parse_single`** (private) — parses one token into an `Address` or `None` (see validity rules above).
- **`find_unquoted`** (private) — byte index of the first `needle` outside double quotes.
- **`unquote`** (private) — strips surrounding `"` and resolves `\x` escapes.
- **`is_valid_email`** (private) — the basic validity rule above.

#### Tests (contracts)

- `new_trims_and_drops_blank_names`, `display_plain_named_and_quoted`, `label_prefers_name`, `parses_common_shapes`, `reports_invalid_tokens` (Err lists exactly the bad tokens), `format_round_trips` (format → parse is identity), `same_email_ignores_case_and_whitespace`, `serde_round_trip`.

### `src/flags.rs`

#### Types

- **struct `MessageFlags { seen, flagged, answered, draft, deleted: bool }`**: Copy, Default, Hash, serde.

#### Functions / methods

##### Context: `MessageFlags`

- **`from_imap`** (pub) — `pub fn from_imap<I, S>(flags: I) -> Self where I: IntoIterator<Item = S>, S: AsRef<str>`
  - Purpose: read IMAP flag atoms.
  - Behavior: case-insensitive `\Seen \Flagged \Answered \Draft \Deleted`; other atoms/keywords ignored. Accepts arrays, slices, `&Vec<String>`, `Vec<&str>`.
- **`to_imap`** (pub) — `pub fn to_imap(&self) -> Vec<&'static str>`
  - Purpose: canonical atoms for STORE/APPEND.
  - Behavior: set flags in order Seen, Flagged, Answered, Draft, Deleted.

#### Tests

- `from_imap_is_case_insensitive_and_ignores_keywords`, `to_imap_round_trips`.

### `src/folder.rs`

#### Types / constants

- **enum `FolderRole`**: `Inbox, Sent, Drafts, Trash, Junk, Archive, All, Flagged, Important, Custom`; serde as lowercase strings.
- **struct `RemoteFolder { name, delimiter: Option<String>, attributes: Vec<String>, role: Option<FolderRole>, selectable: bool }`**: LIST entry; `name` stays modified-UTF-7 encoded (used in commands). Serde.
- **const `NAME_ROLES`** (private): folded well-known names per role (EN/PT/ES/DE/FR).
- **const `UTF7_ALPHABET`** (private): modified BASE64 alphabet (`,` for `/`).

#### Functions / methods

##### Context: `FolderRole`

- **`as_str`** (pub) — `pub fn as_str(self) -> &'static str` — lowercase key, identical to the serde form.
- **`from_str_opt`** (pub) — `pub fn from_str_opt(value: &str) -> Option<Self>` — exact inverse of `as_str`; unknown/other casing → `None`.

##### Context: `RemoteFolder`

- **`new`** (pub) — `pub fn new(name: impl Into<String>, delimiter: Option<&str>, attributes: Vec<String>) -> Self`
  - Behavior: `role` = `role_from_special_use(attributes)` else `role_from_name(name, delimiter)`; `selectable` = false if any attribute is `\Noselect` or `\NonExistent` (case-insensitive).
- **`display_name`** (pub) — `pub fn display_name(&self) -> String` — last segment after `delimiter`, modified-UTF-7 decoded.

##### Context: module

- **`role_from_special_use`** (pub) — `pub fn role_from_special_use<S: AsRef<str>>(attributes: &[S]) -> Option<FolderRole>`
  - Behavior: first case-insensitive match of RFC 6154 `\Sent \Drafts \Trash \Junk \Archive \All \Flagged \Important` (plus XLIST `\Inbox`).
- **`role_from_name`** (pub) — `pub fn role_from_name(name: &str, delimiter: Option<&str>) -> Option<FolderRole>`
  - Behavior: whole name `INBOX` (any case) → Inbox; otherwise decodes UTF-7 of the last segment, folds case/accents and looks it up in `NAME_ROLES` (Sent/Drafts/Trash/Junk/Archive only). Nested `X/Inbox` is not Inbox.
- **`decode_imap_utf7`** (pub) — `pub fn decode_imap_utf7(input: &str) -> String`
  - Behavior: RFC 3501 §5.1.3; `&-` → `&`; `&…-` runs decoded as UTF-16BE; malformed or unterminated runs are kept verbatim.
- **`encode_imap_utf7`** (pub) — `pub fn encode_imap_utf7(input: &str) -> String`
  - Behavior: printable ASCII literal, `&` → `&-`, other runs → `&<modified base64 of UTF-16BE, no padding>-`.
- **`last_segment`** (private) — text after the last delimiter (whole name when none/empty).
- **`fold`** (private) — trim + lowercase + strip common Latin accents.
- **`decode_utf7_run`** (private) — one base64 run → `String`; `None` on bad chars, odd byte count or invalid UTF-16.
- **`flush_utf7_run`** (private) — writes pending UTF-16 units as an `&…-` run and clears them.

#### Tests

- `role_str_round_trips_and_matches_serde`, `special_use_attributes`, `name_heuristics_use_last_segment_and_fold_accents`, `remote_folder_derives_role_and_selectable` (special-use wins over name), `display_name_is_decoded_last_segment`, `utf7_known_vectors` (`&AOk-`↔`é`, `&-`↔`&`, RFC 3501 example), `utf7_round_trips_and_tolerates_garbage`.

### `src/parse.rs`

#### Types

- **struct `ParsedMessage`**: `message_id`, `in_reply_to` (no `<>`), `references: Vec<String>`, `subject`, `from`/`sender: Option<Address>`, `to`/`cc`/`bcc`/`reply_to: Vec<Address>`, `date: Option<i64>` (unix UTC), `html`/`text: Option<String>`, `attachments: Vec<AttachmentInfo>`. Default, Clone, Eq.
- **struct `AttachmentInfo { index, filename, mime_type, size, content_id, inline }`**: serde.

#### Functions

- **`parse_message`** (pub) — `pub fn parse_message(raw: &[u8]) -> Option<ParsedMessage>`
  - Behavior: `MessageParser::default()`; `None` when no headers. Works on header-only input (bodies `None`). Ids stripped of `<>`; In-Reply-To = first id; subject trimmed (RFC 2047 decoded); groups flattened; addresses without email skipped; date via `to_timestamp()` (tz-aware). `html` only from a real `text/html` part at `html_body[0]`, `text` only from a real text part at `text_body[0]`; empty bodies → `None`.
- **`attachment_content`** (pub) — `pub fn attachment_content(raw: &[u8], index: usize) -> Option<Vec<u8>>` — decoded bytes of part `index`; `None` for missing or multipart parts; nested messages return their raw bytes.
- **`inline_images`** (pub) — `pub fn inline_images(raw: &[u8]) -> Vec<(String, String, Vec<u8>)>` — `(cid, mime, bytes)` for listed parts with a Content-ID and an `image/*` type.
- **`has_attachments`** (pub) — `pub fn has_attachments(message: &ParsedMessage) -> bool` — any attachment with `inline == false`.
- **`attachment_parts`** (private) — iterator of `(part id, part)`: binary/message parts in `attachments` or carrying a Content-ID; text/html parts only when in `attachments` and not a body part; never multiparts.
- **`describe`** (private) — builds `AttachmentInfo`: filename from disposition/`name` else `attachment.<ext>` (or `attachment`); `inline` = an `image/*` whose Content-ID the HTML body references (`html_references_cid`: the whole id, any case, percent-encoded or not) and whose disposition is not `attachment`; size = decoded length.
- **`mime_type`** (private) — lowercase `type/subtype` from Content-Type, else by body kind.
- **`part_bytes`** (private) — decoded bytes of a leaf part.
- **`own_body`** (private) — accepts only the requested body kind (prevents mail-parser's text↔html synthesis).
- **`addresses`** (private) — mail-parser `Address` → `Vec<Address>`.
- **`id_list`** (private) — id header (single or list) → cleaned, non-empty ids.
- **`clean_id`** (private) — `strip_brackets` to `String`.

#### Tests

- `parses_alternative_headers_and_bodies`, `date_honours_timezone`, `lists_mixed_attachments_and_decodes_content` (base64 PDF, nameless part, bad index), `related_cid_image_is_inline` (html-only → `text` None), `quoted_printable_latin1_and_rfc2047` (text-only → `html` None, missing Date → None), `headers_only_input`, `garbage_is_none`.

### `src/compose.rs`

#### Types / constants

- **struct `OutgoingAttachment { filename, mime_type, data: Vec<u8> }`**: serde.
- **struct `Draft { from, to, cc, bcc, subject, text, html, in_reply_to, references, attachments }`**: ids without `<>`. Default, serde.
- **const `MIME_TABLE`** (private): extension ↔ MIME pairs; first entry per MIME is canonical.

#### Functions

- **`build_message`** (pub) — `pub fn build_message(draft: &Draft, message_id: &str, date_unix: i64) -> Vec<u8>`
  - Behavior: via `mail-builder`: Message-ID (brackets stripped then re-added), Date, From, To/Cc (omitted when empty), **no Bcc**, Subject (RFC 2047 as needed), In-Reply-To / References as `<id>`; text body always, html → `multipart/alternative`, attachments → `multipart/mixed`. Deterministic except MIME boundaries.
- **`new_message_id`** (pub) — `pub fn new_message_id(domain_from_email: &str, unique: u64) -> String`
  - Behavior: `<unix secs>.<unique hex>.bgmail@<domain>` without brackets; domain = text after the last `@` (or the whole input), `localhost` when empty. Reads the system clock.
- **`envelope_recipients`** (pub) — `pub fn envelope_recipients(draft: &Draft) -> Vec<String>` — to + cc + bcc emails, first spelling wins, case-insensitive dedupe, empty emails skipped.
- **`guess_mime_type`** (pub) — `pub fn guess_mime_type(filename: &str) -> &'static str` — case-insensitive extension lookup, else `application/octet-stream`.
- **`extension_for_mime`** (pub(crate)) — `fn extension_for_mime(mime: &str) -> Option<&'static str>` — canonical extension (no dot) for a MIME type.
- **`strip_brackets`** (pub(crate)) — trims and removes one surrounding `<…>`.
- **`to_builder`** / **`to_builder_list`** (private) — `Address` → mail-builder address / list.

#### Tests

- `builds_plain_text_message` (Bcc absent, round-trip via `parse_message`), `builds_alternative_and_mixed_with_threading`, `text_plus_attachment_is_mixed_without_alternative`, `message_id_format`, `envelope_dedupes_case_insensitively`, `guesses_mime_types`, `extension_for_mime_uses_canonical_entry`, `strip_brackets_trims`.

### `src/reply.rs`

#### Types / constants

- **enum `ReplyKind { Reply, ReplyAll, Forward }`**.
- **struct `QuoteLabels { attribution, forward_header, from, date, subject, to, formatted_date }`**: localized, pre-formatted strings from the UI.
- **consts `REPLY_PREFIXES` (`re: res: aw: sv:`) / `FORWARD_PREFIXES` (`fwd: fw: enc: wg:`)** (pub(crate)).

#### Functions

- **`reply_subject`** (pub) — `pub fn reply_subject(subject: &str) -> String` — trimmed subject, prefixed `Re: ` unless it already starts (case-insensitively) with a reply prefix.
- **`forward_subject`** (pub) — `pub fn forward_subject(subject: &str) -> String` — same with `Fwd: ` and forward prefixes.
- **`prefixed_subject`** (private) — shared prefix logic.
- **`quote_text`** (pub) — `pub fn quote_text(text: &str) -> String` — trims trailing whitespace, `> ` per line, `>` for blank lines, `\n` joined (CRLF accepted).
- **`prepare_reply`** (pub) — `pub fn prepare_reply(original: &ParsedMessage, kind: ReplyKind, me: &[Address], labels: &QuoteLabels) -> Draft`
  - Behavior: `from` = `me[0]` (or default). Body source = `readable_text(text, html)`. Reply: `to` = reply_to or from (if those are all me → original `to`), minus me, deduped. ReplyAll: additionally `cc` = original to + cc minus me and minus `to`, deduped. Reply kinds set `in_reply_to` = original id and `references` = original references + id (no duplicate); text = `"\n\n" + attribution + "\n" + quote_text(body)`. Forward: no recipients/threading; text = `"\n\n" + forward_header + From/Date/Subject/To lines (labels; From/Date/To omitted when empty) + "\n\n" + body`. Attachments not copied.
- **`unique`** (private) — keeps first occurrence per email, skipping `exclude`.
- **`html_to_text`** (pub) — `pub fn html_to_text(html: &str) -> String` — drops comments, tags, `script/style/head/title` content; decodes `&amp; &lt; &gt; &quot; &apos; &#39; &nbsp;` and numeric entities (unknown kept literally); collapses whitespace; line breaks for `br p div tr li h1-6 blockquote table ul ol hr pre` (li/tr on open only); trims lines and keeps at most one blank line.
- **`push_text`** (private) — appends entity-decoded text collapsing whitespace (`&nbsp;` → space).
- **`is_invisible_filler`** (private) — soft hyphen, zero-width space/non-joiner, word joiner and BOM, dropped from converted text (the zero width joiner stays for emoji).
- **`readable_text`** (pub) — `pub fn readable_text(text: Option<&str>, html: Option<&str>) -> String`: the text part, or the converted HTML when that part is missing, blank or HTML itself.
- **`looks_like_html`** (private) — text starting with `<!doctype`, `<html`, `<head` or `<body`.
- **`reply_html`** (pub) — `pub fn reply_html(text: &str) -> String`: HTML alternative of a reply with Gmail's markup (`div.gmail_quote`, `div.gmail_attr`, nested `blockquote.gmail_quote`); text below a quote stays outside it.
- **`draft_from_mailto`** (pub, `mailto`) — `pub fn draft_from_mailto(uri: &str) -> Option<Draft>`: RFC 6068 link to a draft; only to/cc/bcc/subject/body are honored, invalid addresses dropped, line breaks removed from the subject. Helpers `addresses`, `single_line`, `percent_decode`.
- **`reply_html_with_original`** (pub) — `pub fn reply_html_with_original(reply: &str, attribution: &str, original_html: &str) -> String`: `reply_html` of what was written, followed by a Gmail quote of `original_html`.
- **`quote_depth`** (private) — quote depth of a line and its text without the `>` markers.
- **`tidy_lines`** (private) — trims lines, collapses blank runs, drops leading/trailing blanks.

#### Tests

- `subjects_avoid_double_prefixes`, `quote_text_marks_lines`, `reply_targets_reply_to_or_from`, `reply_all_excludes_me_and_duplicates`, `replying_to_my_own_message_targets_its_recipients`, `forward_has_header_block_and_no_threading`, `html_only_original_is_quoted_as_text`, `html_to_text_basics`.

### `src/thread.rs`

- **`normalize_subject`** (pub) — `pub fn normalize_subject(subject: &str) -> String` — lowercases, repeatedly strips reply/forward prefixes (incl. `re[2]:`, `re (3):` counters), trims. Bracket tags like `[dev]` are kept.
- **`strip_prefix_with_counter`** (private) — strips `prefix` or `word[digits]:` / `word (digits):`.
- **`thread_key`** (pub) — `pub fn thread_key(message_id: Option<&str>, in_reply_to: Option<&str>, references: &[String]) -> Option<String>` — first non-empty of: first reference, in_reply_to, own id (brackets stripped); `None` when none.
- Tests: `normalize_strips_prefix_chains`, `thread_key_prefers_root_reference`.

### `src/remote.rs`

#### Types

- **enum `RemoteErrorKind { Auth, Network, Tls, Protocol, NotFound, Unsupported, Cancelled, Other }`**.
- **struct `RemoteError { kind, message }`**: Display `"<kind> error: <message>"`, `std::error::Error`.
- **type `RemoteResult<T> = Result<T, RemoteError>`**.
- **struct `FolderStatus { uid_validity: u32, uid_next: Option<u32>, exists: u32, highest_modseq: Option<u64> }`**.
- **struct `RemoteHeader { uid, flags, size: u32, gm_msgid, gm_thrid, modseq, header: Vec<u8>, has_attachments: Option<bool> }`**.
- **struct `RemoteFlags { uid, flags, modseq }`**.
- **struct `Capabilities { idle, r#move, uidplus, condstore, gmail, special_use }`**: Default all false.
- **enum `IdleOutcome { Changed, Timeout, Stopped }`**.
- **enum `SendFailure { Transient, Permanent, Uncertain }`**: what a failed submission means for the message. `Transient`: certainly not accepted, likely to work later (connection/TLS/auth before the data, `4xx`). `Permanent`: `5xx` or unsendable. `Uncertain`: broke after the data started, the server may have accepted it; never resent automatically.
- **struct `SendError { failure, error: RemoteError }`**: Display delegates to `error`, `std::error::Error`.

#### Functions / methods

- **`RemoteErrorKind::as_str`** (pub) — distinct lowercase names (`"not found"` for NotFound).
- **`RemoteError::new`** (pub) — `pub fn new(kind: RemoteErrorKind, message: impl Into<String>) -> Self`.
- **`Display for RemoteError::fmt`** — format above.
- **`SendError::transient`** / **`permanent`** / **`uncertain`** (pub) — `(error: RemoteError) -> Self`.
- **`From<RemoteError> for SendError`** — errors raised before any message data: `Auth`/`Network`/`Tls`/`Cancelled` → transient, the rest permanent.

#### Trait `MailRemote: Send` (object-safe, blocking)

- `capabilities(&self) -> Capabilities`; `list_folders(&mut self) -> RemoteResult<Vec<RemoteFolder>>`; `select(&mut self, folder) -> RemoteResult<FolderStatus>` (EXAMINE/SELECT); `uids(&mut self, folder) -> RemoteResult<Vec<u32>>` (ascending); `fetch_headers(&mut self, folder, uids) -> RemoteResult<Vec<RemoteHeader>>` (unknown UIDs skipped); `fetch_flags(&mut self, folder, uids, changed_since: Option<u64>) -> RemoteResult<Vec<RemoteFlags>>` (only modseq > since when given); `fetch_body(&mut self, folder, uid) -> RemoteResult<Vec<u8>>` (BODY.PEEK[], never sets `\Seen`); `store_flags(&mut self, folder, uids, add, remove) -> RemoteResult<()>` (only `true` fields applied); `move_messages(&mut self, from, uids, to) -> RemoteResult<Option<Vec<u32>>>` (new UIDs when COPYUID known); `delete_messages(&mut self, folder, uids) -> RemoteResult<()>` (`\Deleted` + expunge); `append(&mut self, folder, flags, raw) -> RemoteResult<()>`; `create_folder(&mut self, name) -> RemoteResult<()>`; `noop(&mut self) -> RemoteResult<()>`; `logout(&mut self)` (infallible); `idle(&mut self, folder, timeout, cancel) -> RemoteResult<IdleOutcome>` (default: `Unsupported`).

- `find_uid_by_message_id(&mut self, folder, message_id) -> RemoteResult<Option<u32>>` — UID of the message with that `Message-ID` (brackets optional), selecting the folder; the highest when several match; the default (servers that can't search) reports `None`. Re-finds a message after its cached UID went stale. `delete_messages` must not expunge other `\Deleted` messages.

#### Trait `MailConnector: Send + Sync`

- `connect(&self, cancel) -> RemoteResult<Box<dyn MailRemote>>`; `connect_sender(&self, cancel) -> RemoteResult<Box<dyn MailSender>>`; `saves_sent_copy(&self) -> bool` (default false; Gmail files sent mail itself).

#### Trait `MailSender: Send`

- `send(&mut self, from: &str, recipients: &[String], raw: &[u8]) -> Result<(), SendError>` — implementors classify failures honestly (`Uncertain` whenever the server may have accepted it); `raw` sent as-is; `recipients` is the envelope.

#### Tests

- `default_idle_and_search_are_unsupported`, `send_errors_classify_by_kind`, `error_display_and_source`, `kind_names_are_distinct`, `traits_are_object_safe_and_send`.

### `src/fake.rs` (`cfg(any(test, feature = "fake"))`)

#### Types / constants

- **const `FAKE_DELIMITER = "/"`**: delimiter reported for every folder.
- **struct `FakeMessage { uid, raw, flags, modseq }`**: server-side message snapshot.
- **struct `FakeRemote`** (Clone shares `Arc<Mutex<ServerState>>` + an `Arc<Condvar>` signalled after every mutation; Default = `new()`).
- **struct `SentMail { from, recipients, raw }`**; **struct `FakeSender`** (Clone shares log; Default).
- **struct `FakeConnector`** (Clone shares counters/failure injection; Default = empty server + sender): `MailConnector` handing out clones of one `FakeRemote` / `FakeSender`.
- Private: `FakeFolder` (name, attributes, uid_validity, uid_next, `BTreeMap` messages, `version` bumped by every change in the folder), `ServerState` (folders, capabilities, global modseq, next UIDVALIDITY, failure queue, call log, selected, logged_out), `SenderState`, `ConnectorState` (queued failures, persistent failure, connect counters, saves_sent_copy).

#### Functions / methods

##### Context: `ServerState` (private)

- **`folder` / `folder_mut`** — lookup by exact name; `NotFound` error otherwise.
- **`bump_modseq`** — increments and returns the global modseq.
- **`insert`** — stores a message at `uid_next` with a fresh modseq; returns the UID.

##### Context: module (private)

- **`not_found`** — `NotFound` `RemoteError` for a folder.
- **`uid_list`** — comma-joined UIDs for the call log.
- **`header_block`** — raw bytes through the first blank line (`\r\n\r\n` or `\n\n`), else all.
- **`apply`** — sets/clears the flags whose mask field is `true`.
- **`fake_gm_msgid`** (pub) — `fn fake_gm_msgid(raw: &[u8]) -> u64`: FNV-1a of the raw bytes shifted to 63 bits (fits SQLite); stand-in `X-GM-MSGID`, stable across folders/moves.

##### Context: `FakeRemote`

- **`new`** (pub) — empty server; capabilities idle/move/uidplus/condstore/special_use = true, gmail = false.
- **`lock`** (private) — locks state, recovering from poison.
- **`notify`** (private) — `notify_all` on the change condvar (wakes `idle`).
- **`enter`** (private) — logs the call, then pops a queued failure (→ `Err(kind, "injected failure")`) or returns the guard.
- **`on_next_call`** (pub) — `(prefix, hook)`: runs `hook` once, the next time a call whose log line starts with `prefix` arrives, before it takes effect and outside the server lock; lets tests act as the UI or another client while the engine is "on the network".
- **`take_hook`** (private) — removes the first hook registered for a call.
- **`set_capabilities`** (pub) — replaces advertised capabilities (affects modseq reporting and COPYUID).
- **`add_folder`** (pub) — `(name, attributes: &[&str]) -> u32`; creates with a new UIDVALIDITY (1, 2, …) and uid_next 1; existing folder untouched; returns its UIDVALIDITY.
- **`deliver`** (pub) — `(folder, raw, flags) -> u32`; appends server-side; panics if the folder is missing.
- **`set_flags`** (pub) — `(folder, uid, flags) -> bool`; replaces flags and bumps modseq; false if missing.
- **`expunge`** (pub) — `(folder, uid) -> bool`; removes server-side.
- **`reset_uid_validity`** (pub) — `(folder) -> u32`; new UIDVALIDITY and renumbers messages from UID 1; panics if missing.
- **`messages`** (pub) — snapshot ascending by UID; empty for unknown folder.
- **`folder_names`** (pub) — folder names in creation order.
- **`fail_next`** (pub) — queues a failure for the next fallible `MailRemote` call (FIFO).
- **`calls`** / **`clear_calls`** (pub) — call log, e.g. `"select INBOX"`, `"fetch_headers INBOX 1,2"`, `"fetch_flags INBOX 1,2 since 7"`, `"store_flags INBOX 1 +\\Flagged -\\Seen"`, `"move_messages A 1,2 B"`, `"logout"`.
- **`selected`** (pub) — last successfully selected folder.
- **`is_logged_out`** (pub) — whether `logout` was called.

##### Context: `FakeSender` (additions)

- **`fail_next`** (pub) — queues a failure classified by `SendError::from(kind)`.
- **`fail_next_with`** (pub) — queues an exact `SendError`.
- **`deliver_then_fail_next`** (pub) — next send is recorded as delivered, then fails `Uncertain` (the duplicate-send scenario).
- **`sends`** (pub) — `send` calls so far, successful or not.

##### Context: `impl MailRemote for FakeRemote`

- **`find_uid_by_message_id`** — highest UID whose parsed `Message-ID` matches (brackets optional); blank → `None`; logged as `find_uid <folder> <id>`.
- All fallible methods go through `enter` (logged + failure injection); unknown folders → `NotFound`.
- `capabilities` (not logged); `list_folders` (`RemoteFolder::new` with `/`); `select` (status with `highest_modseq` = max message modseq when CONDSTORE; records selection); `uids`; `fetch_headers` (header block, size, modseq when CONDSTORE, `has_attachments` from `parse_message`); `fetch_flags` (filters by `changed_since`); `fetch_body` (raw, flags untouched; missing UID → `NotFound`); `store_flags` (one new modseq for the batch); `move_messages` (removes from source, inserts into target with new UIDs/modseqs; `Some(new_uids)` only with UIDPLUS); `delete_messages`; `append`; `create_folder` (existing → `Protocol`); `noop`; `logout` (logged, sets flag, never fails).
- `fetch_headers` reports `gm_msgid = fake_gm_msgid(raw)` only when `capabilities.gmail`.
- Every mutation (test-side `deliver`/`set_flags`/`expunge`/`reset_uid_validity` and `store_flags`/`move_messages` (source + target)/`delete_messages`/`append`) bumps the affected folder's `version` and calls `notify`.
- **`idle`** — `(folder, timeout, cancel) -> RemoteResult<IdleOutcome>`: logged as `"idle {folder}"` through `enter` (failure injection applies); no IDLE capability → `Unsupported`; unknown folder → `NotFound`. Snapshots the folder `version`, registers a cancel hook that locks state and notifies (no lost wake-up), then waits on the condvar: cancelled → `Stopped`, version changed (or folder gone) → `Changed`, deadline → `Timeout`.

##### Context: `FakeSender`

- **`new`** (pub), **`lock`** (private), **`sent`** (pub, oldest first), **`fail_next`** (pub, FIFO queue).
- **`MailSender::send`** — pops a queued failure or records `SentMail`.

##### Context: `FakeConnector`

- **`new`** (pub) — `(remote: FakeRemote, sender: FakeSender) -> Self`.
- **`lock`** (private) — connector state, poison-tolerant.
- **`remote`** / **`sender`** (pub) — shared handles (clones).
- **`fail_next_connect`** (pub) — `(kind)`: next `connect`/`connect_sender` fails (FIFO, shared by both).
- **`set_connect_failure`** (pub) — `(Option<kind>)`: every attempt fails until cleared.
- **`set_saves_sent_copy`** (pub) — value returned by `saves_sent_copy`.
- **`connects`** / **`sender_connects`** (pub) — successful connection counts.
- **`check`** (private) — cancelled token → `Cancelled`; else queued failure, else persistent failure, else the state guard.
- **`MailConnector::connect`** / **`connect_sender`** — `check`, count, then `Box` a clone of the shared remote/sender. **`saves_sent_copy`** — configured flag (default `false`).

#### Tests

- `clones_share_state`, `list_and_select`, `headers_flags_and_body`, `flag_changes_bump_modseq_and_filter`, `store_flags_applies_only_true_fields`, `move_delete_append_create`, `server_side_expunge_and_uid_validity_reset`, `failure_injection_and_call_log`, `header_block_variants`, `sender_records_and_fails`, `gmail_mode_reports_stable_gm_msgid`, `idle_wakes_on_change_in_its_folder_only`, `idle_times_out_stops_on_cancel_and_needs_capability`, `mutations_bump_folder_version`, `connector_hands_out_shared_connections_and_injects_failures`.
