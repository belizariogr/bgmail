# crates/mail_core/ — Mail domain vocabulary

## Purpose

Pure, storage-agnostic mail domain types and logic shared by the future sync
engine, storage adapters and protocol clients: addresses, IMAP flags, folder
roles + modified UTF-7, MIME parsing (`mail-parser`) and building
(`mail-builder`), reply/forward drafts, thread keys, and the blocking
`MailRemote` / `MailSender` contracts with in-memory fakes.

## Ownership

- Owns: `Address`, `MessageFlags`, `FolderRole` / `RemoteFolder`,
  `ParsedMessage` / `AttachmentInfo`, `Draft` / `OutgoingAttachment`,
  reply/thread helpers, `remote` traits and value types, `fake` test doubles.
- Does not own: persistence (`storage`), sync orchestration, real IMAP/SMTP
  implementations (future `protocols`), credentials (future `accounts`), UI
  text/i18n and date formatting (`bgmail` passes pre-formatted `QuoteLabels`).

## Local Contracts

- **No GPUI, no network, no filesystem I/O, zero `unsafe`**
  (`#![forbid(unsafe_code)]`). Only the clock is read (`new_message_id`).
- Dependencies: `mail-parser = "0.11"` (1.x needs Rust 1.98; toolchain is
  1.95), `mail-builder = "~2.0"` with `default-features = false` (no
  `gethostname`; we always set Message-ID/Date), `serde` (workspace). Dev:
  `serde_json`. Add nothing else without a real need.
- Message-IDs are stored and passed **without** angle brackets everywhere;
  `build_message` adds them.
- `ParsedMessage.html` / `.text` are the message's own parts only (never
  synthesized from the other kind). Text keeps the source line endings (CRLF).
- `AttachmentInfo.index` is the `mail-parser` part id: stable for the same raw
  bytes; feed it to `attachment_content`.
- Bcc never appears in built headers; it only travels via
  `envelope_recipients`.
- `MailRemote` / `MailSender` are blocking, object-safe and `Send`. IDLE lives
  in the protocols crate, not in the trait (`IdleOutcome` is shared vocabulary).
- Feature `fake` exposes `fake::{FakeRemote, FakeSender}` to other crates'
  tests; it is always on for this crate's own tests. Clones share server state.
- Serde derives only where JSON persistence is expected: `Address`,
  `MessageFlags`, `FolderRole` (lowercase strings), `RemoteFolder`,
  `AttachmentInfo`, `Draft`, `OutgoingAttachment`.

## Work Guidance

- Every public function gets unit tests (hand-written RFC 822 fixtures for
  parsing; build → `parse_message` round-trips for composing).
- Keep localized strings out of this crate; accept them as inputs.
- Name heuristics for folders live in `folder::NAME_ROLES` (folded,
  accent-free); extend there with a test.

## Verification

- `cargo test -p mail_core`
- `cargo clippy -p mail_core --all-targets -- -D warnings` (also with
  `--features fake`)
- `cargo fmt`

## Child DOX Index

| Scope | Doc | Owns |
|---|---|---|
| `crates/mail_core/src/` | [`.dox/crates/mail_core/src/AGENTS.md`](src/AGENTS.md) | Full API catalog (every fn/method) |
