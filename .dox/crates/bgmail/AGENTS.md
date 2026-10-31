# crates/bgmail/ — Application binary

## Purpose

BGMail executable (`BGMail`): windows, macOS Mail–style layout, UI state,
localization, settings/compose, the HTML reader (native, via `html_view`), and
wiring to `storage`.

## Ownership

- Owns: `main`, `RootView`, compose/settings windows, toolbar/sidebar/list/
  reader chrome, locale catalog, persisted `config.json`, mail HTML
  sanitization/theming (`mail_html`), command palette, menus/shortcuts, window drag/CSD helpers, DB seed
  bridge.
- Does not own: theme palettes (`theme`), reusable widgets (`ui`), SQLite
  schema/API (`storage`), shared icon SVG sources (`assets/icons`).

## Local Contracts

- Binary name is `BGMail` (menu title on unbundled macOS). Package crate name
  is `bgmail`.
- The reader shows bodies with `html_view::HtmlView` (sanitized + themed by
  `mail_html`). Remote images are blocked unless the user loads them:
  clicking the privacy shield next to the subject loads them for that
  message (this session), and Settings ▸ Privacy ▸ "Always load remote
  images" (`load_remote_images`, persisted) loads them everywhere.
- Reader interaction comes from `HtmlView` events: hovered links go to the
  status bar, clicked links open outside the app only when
  `mail_html::is_openable_link` (http/https/mailto), secondary clicks open a
  `ui::ContextMenu` (open/copy link, copy, select all), and Cmd/Ctrl+F
  (`FindInMessage`, also a global action) opens the find bar. The reader
  zoom (Ctrl/Cmd `+`/`-`/`0`, Ctrl/Cmd + wheel; `HtmlView`) shows in the
  status bar when not 100%.
  `html_view::init` is called at startup.
- Printing (`MessagePrint`, Cmd/Ctrl+P) goes through the system browser
  (decided with two opposing reviews; GPUI has no print API): `print`
  writes `mail_html::print_document` (light page, header, CSP allowing only
  `data:` images and its own nonce'd `print()` script) to a private temp
  file and `open_with_system` opens it.
- Reader robustness: HTML over 1 MB is shown as plain text (with "Show
  formatted"), bodies `html_view` refuses (too deep) as plain text, and a
  message whose formatted rendering was interrupted by a crash (the
  `crash_guard` marker survived) as plain text, each with a notice. The
  release profile keeps `panic = "abort"` (decided with two opposing
  reviews): unwinding through Blitz/GPUI state isn't safe and stack overflow
  or OOM can't be caught anyway.
- **Modules (flat under `src/`):**
  - `root` — main three-column UI + selection/layout state.
  - `data` — mock/sample structures (keep isolated for replacement).
  - `db_seed` — seeds `storage` from mock data on first open.
  - `locale` — `Language` + string keys (EN default, PT-BR); no hardcoded
    user-facing strings in views.
  - `config` — `~/.config/BGMail/config.json` (window/columns/privacy prefs).
  - `mail_html` — e-mail HTML sanitizing (`lol_html`), remote image policy,
    the themed document handed to `html_view`, and the plain-text fallback.
  - `crash_guard` — marker of the message being rendered (crash loops).
  - `print` — private print pages in the temp dir (Unix 0700/0600, stale
    pages removed on the next print).
  - `compose`, `command_palette*`, `commands`, `actions`, `shortcuts`,
    `app_menus` — app actions and overlays.
  - `window_drag` / `window_frame` — portable window move/CSD; OS code behind
    `cfg`.
  - `startup` — debug startup milestones (keep startup path light).
- Development preview: `BGMAIL_DB` opens another database instead of
  `~/.config/BGMail/mail.db`, and `BGMAIL_SAMPLE_DIR` adds its `*.html` to
  the seed (only when that database is first created);
  `spikes/html_render/tools/run_preview.sh` uses both.
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

- `cargo test -p bgmail` (sanitize, config, locale, layout helpers, …).
- `cargo run -p bgmail`.
- Workspace `fmt` / `clippy -D warnings` / `test --workspace`.

## Child DOX Index

| Scope | Doc | Owns |
|---|---|---|
| `crates/bgmail/src/` | [`.dox/crates/bgmail/src/AGENTS.md`](src/AGENTS.md) | Full API catalog (every fn/method in the binary) |
| `crates/bgmail/assets/` | [`.dox/crates/bgmail/assets/AGENTS.md`](assets/AGENTS.md) | Bundled sample e-mails and reader fixture images |
