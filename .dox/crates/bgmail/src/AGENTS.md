# crates/bgmail/src/ — Source API

## Purpose

Source for the `bgmail` binary: app entry, views, webview/CEF, locale, config, and OS helpers.

## Ownership

- Owns: All modules under `crates/bgmail/src/`.
- Does not own: crate-level dependency/feature policy (parent `.dox/crates/bgmail/AGENTS.md`).

## Local Contracts

- Every Rust source file under this tree has a matching
  `.dox/crates/bgmail/src/<file>.rs.dox` with its **API Catalog**.
- Update the per-file `.rs.dox` when changing that file's signatures or
  semantics. Prefer rustdoc alignment on public items.
- Visibility in entries (`pub` / `private`) reflects the source item.
- Do not dump sibling-module APIs into this folder doc; keep catalogs in the
  matching `.rs.dox`.

## Work Guidance

- After adding/removing/renaming a source file, create/delete/rename the
  matching `.rs.dox` and refresh this Child DOX Index in the same change.
- After changing a function/type, update that file's `.rs.dox` API Catalog.
- Do not weaken parent DOX contracts from root/`crates/`/`bgmail/`.

## Verification

- `cargo test -p bgmail`
- Workspace `fmt` / `clippy -D warnings` as required by root `AGENTS.md`.

## Child DOX Index

| Scope | Doc | Owns |
|---|---|---|
| `crates/bgmail/src/actions.rs` | [`actions.rs.dox`](actions.rs.dox) | GPUI action types for menus, shortcuts, and command dispatch |
| `crates/bgmail/src/app_menus.rs` | [`app_menus.rs.dox`](app_menus.rs.dox) | Builds and syncs the global application menu bar for main and compose surfaces |
| `crates/bgmail/src/cef_osr.rs` | [`cef_osr.rs.dox`](cef_osr.rs.dox) | Chromium Embedded Framework off-screen rendering backend for the HTML reader |
| `crates/bgmail/src/command_palette.rs` | [`command_palette.rs.dox`](command_palette.rs.dox) | In-window command palette filter/selection state |
| `crates/bgmail/src/command_palette_overlay.rs` | [`command_palette_overlay.rs.dox`](command_palette_overlay.rs.dox) | Renders the dimmed command palette overlay UI |
| `crates/bgmail/src/commands.rs` | [`commands.rs.dox`](commands.rs.dox) | Command ids, enablement rules, labels, and palette entry construction |
| `crates/bgmail/src/compose.rs` | [`compose.rs.dox`](compose.rs.dox) | Standalone compose/new-message window view and helpers |
| `crates/bgmail/src/config.rs` | [`config.rs.dox`](config.rs.dox) | Persisted layout and privacy settings (`config.json`) |
| `crates/bgmail/src/data.rs` | [`data.rs.dox`](data.rs.dox) | Mock/sample accounts, messages, and fixture bodies for Stage 1/2 |
| `crates/bgmail/src/db_seed.rs` | [`db_seed.rs.dox`](db_seed.rs.dox) | Adapts mock data into `storage` seed records and folder display helpers |
| `crates/bgmail/src/locale.rs` | [`locale.rs.dox`](locale.rs.dox) | UI language enum, string keys, and EN/PT-BR translations |
| `crates/bgmail/src/main.rs` | [`main.rs.dox`](main.rs.dox) | Application entry point, CEF lifecycle, and global action wiring |
| `crates/bgmail/src/root.rs` | [`root.rs.dox`](root.rs.dox) | Main three-column RootView, settings window, layout, and reader chrome |
| `crates/bgmail/src/shortcuts.rs` | [`shortcuts.rs.dox`](shortcuts.rs.dox) | Key binding registration and display formatting |
| `crates/bgmail/src/startup.rs` | [`startup.rs.dox`](startup.rs.dox) | Debug startup timing milestones |
| `crates/bgmail/src/web_view.rs` | [`web_view.rs.dox`](web_view.rs.dox) | HTML document build/sanitize and portable EmailWebView API |
| `crates/bgmail/src/window_drag.rs` | [`window_drag.rs.dox`](window_drag.rs.dox) | Portable window move, cloak, and layout-settled helpers |
| `crates/bgmail/src/window_frame.rs` | [`window_frame.rs.dox`](window_frame.rs.dox) | Titlebar options and client-side decoration chrome |

