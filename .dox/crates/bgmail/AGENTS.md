# crates/rmail/ — Application binary

## Purpose

BGMail executable (`BGMail`): windows, macOS Mail–style layout, UI state,
localization, settings/compose, HTML reader (CEF OSR), and wiring to `storage`.

## Ownership

- Owns: `main`, `RootView`, compose/settings windows, toolbar/sidebar/list/
  reader chrome, locale catalog, persisted `config.json`, CEF/HTML sanitization
  pipeline, command palette, menus/shortcuts, window drag/CSD helpers, DB seed
  bridge.
- Does not own: theme palettes (`theme`), reusable widgets (`ui`), SQLite
  schema/API (`storage`), shared icon SVG sources (`assets/icons`).

## Local Contracts

- Binary name is `BGMail` (menu title on unbundled macOS). Package crate name
  remains `rmail`.
- Default feature `cef-osr` enables CEF windowless HTML reader; plain-text
  fallback with `--no-default-features`.
- **Modules (flat under `src/`):**
  - `root` — main three-column UI + selection/layout state.
  - `data` — mock/sample structures (keep isolated for replacement).
  - `db_seed` — seeds `storage` from mock data on first open.
  - `locale` — `Language` + string keys (EN default, PT-BR); no hardcoded
    user-facing strings in views.
  - `config` — `~/.config/BGMail/config.json` (window/columns/privacy prefs).
  - `web_view` / `cef_osr` — HTML document build, sanitize (`lol_html`), remote
    image policy, OSR paint/input; platform details stay behind portable APIs.
  - `compose`, `command_palette*`, `commands`, `actions`, `shortcuts`,
    `app_menus` — app actions and overlays.
  - `window_drag` / `window_frame` — portable window move/CSD; OS code behind
    `cfg`.
  - `startup` — debug startup milestones (keep startup path light).
- UI state: `Entity` + `impl Render`; mutate via `cx.listener` + `cx.notify()`.
- OS-specific code only in abstracted helpers (`window_drag`, notifications,
  etc.), never leaked into layout/domain types.
- No Stage 3 networking (IMAP/SMTP/OAuth) in this crate yet.
- Unsafe: avoid; isolate and justify if unavoidable.

## Work Guidance

- Consult Zed for layout/GPUI patterns before inventing.
- New UI strings → `locale` keys (EN + PT-BR together).
- Prefer `ui` components and `storage::Database` over duplicating logic.
- Keep heavy work off the startup path; persist/save debounced on background
  threads where already patterned.
- Update `TODO.md` when starting/finishing work here.

## Verification

- `cargo test -p rmail` (sanitize, config, locale, layout helpers, IPC parsers,
  …).
- `cargo run -p rmail` (and `--no-default-features` for text-only reader).
- Workspace `fmt` / `clippy -D warnings` / `test --workspace`.

## Child DOX Index

| Scope | Doc | Owns |
|---|---|---|
| `crates/rmail/src/` | [`.dox/crates/rmail/src/AGENTS.md`](src/AGENTS.md) | Full API catalog (every fn/method in the binary) |
| `crates/rmail/assets/` | [`.dox/crates/rmail/assets/AGENTS.md`](assets/AGENTS.md) | Bundled sample e-mails and reader fixture images |
