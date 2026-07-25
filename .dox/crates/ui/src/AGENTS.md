# crates/ui/src/ — Source API

## Purpose

Source for the `ui` crate: reusable GPUI components, semantic colors, layout
helpers, embedded SVG assets, and platform text-input integration. Every public
item here is re-exported from `ui` via `ui.rs`.

## Ownership

- Owns: All modules under `crates/ui/src/`.
- Does not own: crate-level dependency/feature policy (parent
  `.dox/crates/ui/AGENTS.md`).

## Local Contracts

- Every Rust source file under this tree has a matching
  `.dox/crates/ui/src/<file>.rs.dox` with its **API Catalog**.
- Update the per-file `.rs.dox` when changing that file's signatures or
  semantics. Prefer rustdoc alignment on public items.
- Visibility in entries (`pub` / `private`) reflects the source item.
- Do not dump sibling-module APIs into this folder doc; keep catalogs in the
  matching `.rs.dox`.

## Work Guidance

- After adding/removing/renaming a source file, create/delete/rename the
  matching `.rs.dox` and refresh this Child DOX Index in the same change.
- After changing a function/type, update that file's `.rs.dox` API Catalog.
- Do not weaken parent DOX contracts from root/`crates/`/`ui/`.

## Verification

- `cargo test -p ui`
- Workspace `fmt` / `clippy -D warnings` as required by root `AGENTS.md`.

## Child DOX Index

| Scope | Doc | Owns |
|---|---|---|
| `crates/ui/src/assets.rs` | [`assets.rs.dox`](assets.rs.dox) | Embedded SVG icon AssetSource |
| `crates/ui/src/button.rs` | [`button.rs.dox`](button.rs.dox) | Button and IconButton components |
| `crates/ui/src/color.rs` | [`color.rs.dox`](color.rs.dox) | Semantic Color roles resolved against ActiveTheme |
| `crates/ui/src/icon.rs` | [`icon.rs.dox`](icon.rs.dox) | IconName enum and tinted Icon component |
| `crates/ui/src/label.rs` | [`label.rs.dox`](label.rs.dox) | Themed Label text component |
| `crates/ui/src/list_item.rs` | [`list_item.rs.dox`](list_item.rs.dox) | Clickable ListItem row component |
| `crates/ui/src/prelude.rs` | [`prelude.rs.dox`](prelude.rs.dox) | Convenience re-exports and flex layout helpers |
| `crates/ui/src/scrollbar.rs` | [`scrollbar.rs.dox`](scrollbar.rs.dox) | Custom Scrollbar element and state |
| `crates/ui/src/switch.rs` | [`switch.rs.dox`](switch.rs.dox) | Toggle Switch component |
| `crates/ui/src/text_input.rs` | [`text_input.rs.dox`](text_input.rs.dox) | Single-line TextInput with platform IME integration |
| `crates/ui/src/tooltip.rs` | [`tooltip.rs.dox`](tooltip.rs.dox) | Hover Tooltip view factories |
| `crates/ui/src/ui.rs` | [`ui.rs.dox`](ui.rs.dox) | Crate root: submodule declarations and public re-exports |

