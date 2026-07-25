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

- Members (see root `Cargo.toml`): `theme`, `ui`, `storage`, `bgmail`.
- **Dependency direction:** UI depends on domain/storage, never the reverse.
  Domain and protocols must not know about GPUI. Today: `bgmail` → `ui` →
  `theme`; `bgmail` → `storage`; `storage` has no GPUI/`ui`/`theme` deps.
- Future crates (planned, not members yet): `mail_core`, `protocols`,
  `accounts` — see `docs/PLANEJAMENTO.md` §4.
- Prefer published `gpui` from crates.io (`0.2.2` workspace pin). Local Zed
  (`~/dev/zed`) is read-only reference only.
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
| `crates/bgmail/` | [`.dox/crates/bgmail/AGENTS.md`](bgmail/AGENTS.md) | App binary: windows, layout, webview, i18n |
