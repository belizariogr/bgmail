# crates/ — Workspace crates

## Purpose

Cargo workspace members that make up BGMail. Splitting crates favors reuse,
isolated tests, and shorter incremental compiles.

## Ownership

- Owns: crate boundaries, workspace dependency direction, which crate may depend
  on which.
- Does not own: repo-wide agent rules (root `AGENTS.md`), planning narrative
  (`docs/`), shared assets (`assets/`).

## Local Contracts

- Members (see root `Cargo.toml`): `theme`, `ui`, `storage`, `mail_core`,
  `protocols`, `html_view`, `mail_sync`, `auto_update`, `bgmail`.
- **Dependency direction:** UI depends on domain/storage, never the reverse.
  Domain and protocols must not know about GPUI. Today: `bgmail` → `ui` →
  `theme`; `bgmail` → `storage`; `storage` has no GPUI/`ui`/`theme` deps.
  `html_view` depends on `gpui` + Blitz only (no `ui`/`theme`/`storage`).
  `mail_core` is a leaf domain crate (no GPUI/`ui`/`theme`/`storage`, no I/O);
  sync/protocol/storage adapters depend on it, never the reverse.
  `mail_sync` (sync engine) → `mail_core` + `storage`; no GPUI and no protocol
  crate: it only sees `mail_core::MailConnector`, so the UI injects the real
  connector (`protocols`) and tests inject `mail_core::fake::FakeConnector`.
  `protocols` (IMAP/SMTP/OAuth/discovery/BIMI) → `mail_core` only (no GPUI, no
  `storage`/`ui`/`theme`); blocking clients on std threads, one rustls 0.23
  (ring) shared by `imap`, `lettre` and `ureq`.
  `auto_update` (app self-updates) depends on `sha2` only: no GPUI, no
  network (the app passes a `ReleaseSource` over `protocols::release`), no C
  deps, so it checks for every target; `bgmail` → `auto_update`.
- No `accounts` crate (decided): config in `storage`, secrets via GPUI's
  credential API in `bgmail`, OAuth/presets in `protocols`.
- `gpui` is `0.2.2` from crates.io, replaced by a patched copy in `vendor/gpui`
  (see `.dox/vendor/AGENTS.md`). Local Zed (`~/dev/zed`) is read-only reference
  only.
- Empty / unused directories under `crates/` are not workspace members and must
  not receive DOX until they become real crates.
- **Source API catalogs** live at `.dox/crates/<crate>/src/AGENTS.md` and must
  list every function/method (purpose + behavior). Keep them in sync with code.

## Work Guidance

- Add a new crate only when there is a durable boundary (domain, protocols,
  accounts). Do not invent crates "just in case".
- After adding/removing a workspace member, update this doc, the root Child DOX
  Index if top-level scope changes, and `docs/PLANEJAMENTO.md` architecture
  table when status changes.
- Mirror Zed names/patterns when porting UI; keep domain crates GPUI-free.

## Verification

- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `cargo fmt`

## Child DOX Index

| Scope | Doc | Owns |
|---|---|---|
| `crates/theme/` | [`.dox/crates/theme/AGENTS.md`](theme/AGENTS.md) | Theme colors, appearance, `ActiveTheme` |
| `crates/ui/` | [`.dox/crates/ui/AGENTS.md`](ui/AGENTS.md) | Reusable GPUI components and semantic `Color` |
| `crates/storage/` | [`.dox/crates/storage/AGENTS.md`](storage/AGENTS.md) | SQLite mail store, schema, seed, search |
| `crates/mail_core/` | [`.dox/crates/mail_core/AGENTS.md`](mail_core/AGENTS.md) | Mail domain types, MIME parse/build, reply/thread helpers, `MailRemote`/`MailSender` + fakes |
| `crates/protocols/` | [`.dox/crates/protocols/AGENTS.md`](protocols/AGENTS.md) | IMAP (`ImapRemote`), SMTP (`SmtpSender`), OAuth PKCE loopback, `ImapSmtpConnector`, provider presets + autoconfig |
| `crates/mail_sync/` | [`.dox/crates/mail_sync/AGENTS.md`](mail_sync/AGENTS.md) | Sync engine: per-account worker/IDLE threads, sync planner, op/outbox replay, optimistic UI actions |
| `crates/html_view/` | [`.dox/crates/html_view/AGENTS.md`](html_view/AGENTS.md) | Native GPUI HTML rendering (Blitz layout, GPUI paint/text) |
| `crates/auto_update/` | [`.dox/crates/auto_update/AGENTS.md`](auto_update/AGENTS.md) | Self-updates from GitHub Releases: versions, install detection, release file choice, digest check, platform installers |
| `crates/bgmail/` | [`.dox/crates/bgmail/AGENTS.md`](bgmail/AGENTS.md) | App binary: windows, layout, reader, i18n |
