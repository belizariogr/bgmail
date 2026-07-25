# crates/theme/src/ — Source API

## Purpose

Source for the `theme` crate: appearance, color roles, built-in palettes, and active-theme globals for GPUI.

## Ownership

- Owns: `src/theme.rs` API surface.
- Does not own: crate-level dependency/feature policy (parent `.dox/crates/theme/AGENTS.md`).

## Local Contracts

- Every Rust source file under this tree has a matching
  `.dox/crates/theme/src/<file>.rs.dox` with its **API Catalog**.
- Update the per-file `.rs.dox` when changing that file's signatures or
  semantics. Prefer rustdoc alignment on public items.
- Visibility in entries (`pub` / `private`) reflects the source item.
- Do not dump sibling-module APIs into this folder doc; keep catalogs in the
  matching `.rs.dox`.

## Work Guidance

- After adding/removing/renaming a source file, create/delete/rename the
  matching `.rs.dox` and refresh this Child DOX Index in the same change.
- After changing a function/type, update that file's `.rs.dox` API Catalog.
- Do not weaken parent DOX contracts from root/`crates/`/`theme/`.

## Verification

- `cargo test -p theme`
- Workspace `fmt` / `clippy -D warnings` as required by root `AGENTS.md`.

## Child DOX Index

| Scope | Doc | Owns |
|---|---|---|
| `crates/theme/src/theme.rs` | [`theme.rs.dox`](theme.rs.dox) | Appearance, palettes, and ActiveTheme globals |

