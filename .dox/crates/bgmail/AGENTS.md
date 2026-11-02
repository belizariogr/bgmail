# crates/bgmail/ — Application binary

## Purpose

BGMail executable (`BGMail`): windows, macOS Mail–style layout, UI state,
localization, settings/compose, the HTML reader (native, via `html_view`), and
wiring to `storage`.

## Ownership

- Owns: `main`, `build.rs` (Windows icon), packaging config
  (`[package.metadata.packager]`, `packaging/`), `RootView`, compose/settings windows, toolbar/sidebar/list/
  reader chrome, locale catalog, persisted `config.json`, mail HTML
  sanitization/theming (`mail_html`), command palette, menus/shortcuts, window drag/CSD helpers, DB seed
  bridge.
- Does not own: theme palettes (`theme`), reusable widgets (`ui`), SQLite
  schema/API (`storage`), shared icon SVG sources (`assets/icons`).

## Local Contracts

- Binary name is `BGMail` (menu title on unbundled macOS). Package crate name
  is `bgmail`.
- **Packaging** (cargo-packager, decided with two opposing reviews) is
  configured once in `[package.metadata.packager]` of `Cargo.toml` (paths
  relative to this crate): product `BGMail`, identifier
  `io.github.belizariogr.bgmail`, icons from `assets/appIcon/`, per-OS default
  formats (macOS app+dmg, Linux deb+AppImage+pacman, Windows NSIS; CI adds
  WiX). Run `cargo packager --release` (its before-packaging hook builds the
  release binary); outputs land in `target/[<triple>/]release/`.
  - Linux: `packaging/linux/BGMail.desktop` is the Handlebars desktop template
    for deb/AppImage/pacman (`Categories=Network;Email;`, mailto handler,
    `StartupWMClass=com.bgmail.main`). Each window has its own GPUI `app_id`
    (user decision): main `com.bgmail.main`, settings `com.bgmail.settings`,
    compose `com.bgmail.newmail` (`root::*_WINDOW_APP_ID`); only the main one
    matches the desktop entry. Deb `Depends` lists the
    linked/dlopen'ed libs (xcb, xkbcommon, wayland, Vulkan + ICD, fontconfig);
    secrets go over D-Bus Secret Service (pure Rust), so a keyring daemon
    (gnome-keyring, KeePassXC…) is needed at runtime but not a package dep.
  - macOS: `minimum-system-version` 10.15.7 (Zed's floor); signing identity
    is injected by CI only (never committed).
  - Windows: `build.rs` embeds `assets/appIcon/BGMail.ico` in the exe
    (`main` calls `embed_windows_icon` only on a Windows host building for
    Windows: writes `OUT_DIR/bgmail-icon.rc` with the absolute icon path and
    compiles it with `embed-resource`; no-op elsewhere). NSIS shortcuts
    carry AppUserModelID = `identifier`; `notifications` passes the same id
    to `notify-rust` (`app_id`) when an installer Start Menu shortcut
    (`BGMail.lnk` or `BGMail\BGMail.lnk` under `%APPDATA%`/`%PROGRAMDATA%`)
    exists; unpackaged runs keep notify-rust's PowerShell id (Windows drops
    toasts from an unregistered id). WiX shortcuts are assumed to carry the
    same id.
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
- Reader robustness: HTML is always shown formatted, whatever its size
  (user decision; large bodies still parse on the UI thread). Bodies
  `html_view` refuses (too deep) show as plain text, and a message whose formatted rendering was interrupted by a crash (the
  `crash_guard` marker survived) as plain text, each with a notice. The
  release profile keeps `panic = "abort"` (decided with two opposing
  reviews): unwinding through Blitz/GPUI state isn't safe and stack overflow
  or OOM can't be caught anyway.
- Compose (`compose::ComposeView`, one window at a time): real editors
  (`ui::TextInput` To/Cc/Bcc/Subject, `ui::TextArea` body), From picker
  (`ui::ContextMenu`; local accounts listed but can't send), attachments via
  picker / drag-and-drop read off the UI thread (25 MB total cap), plain-text
  send only. Send (button, `ComposeSend`, Cmd/Ctrl+Enter) validates with
  `mail_core::parse_address_list` (inline per-field errors) and hands the
  `Draft` to `RootView::send_draft` in a task (never while root is leased);
  success closes the window, errors show in a banner. Replies prefill from
  `ComposeInit` (`RootView::open_compose_with`; an open window is refilled)
  and focus the body above the quote; forwards focus To. Discard asks for
  confirmation only after edits; closing the window does not prompt.
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
  - `attachments` — attachment chip presentation (family icon, human size)
    and file writing: sanitized names, never-overwrite ` (n)` dedupe,
    private temp copies for Open (Unix 0700/0600, stale after a day).
  - `compose`, `command_palette*`, `commands`, `actions`, `shortcuts`,
    `app_menus` — app actions and overlays.
  - `window_drag` / `window_frame` — portable window move/CSD; OS code behind
    `cfg`.
  - `startup` — debug startup milestones (keep startup path light).
  - `settings` — Settings window. Accounts: live list (provider badge,
    health from `RootView::account_status`/`account_start_error`), per-account
    "Update password" / "Sign in again" (Google OAuth) / "Remove" (confirmed
    with `window.prompt`; local mail is deleted), and the inline "Add account"
    form (preset/discovered servers, "Manual settings" with TLS/STARTTLS/None,
    Gmail → "Sign in with Google" when `BGMAIL_GOOGLE_CLIENT_ID` is set, else
    the app-password IMAP path). Notifications: switch bound to
    `notifications_enabled` (persisted).
  - `account_setup` — GPUI-free form/validation/config building, account
    health, and error/preset-note → locale key mapping for `settings`.
  - `mail_service` — owns the `mail_sync::Engine`, builds connectors from
    `AccountConfig` + keychain secret, folds account statuses for the status
    bar. `RootView::start_mail` (called from `main` after the window opens)
    awaits engine events in a GPUI task; secrets are read with
    `cx.read_credentials` asynchronously (GPUI keychain: Keychain /
    Credential Manager / Secret Service), never on the startup path.
  - `dates` — local-time list/reader dates (chrono `clock`).
  - `notifications` — desktop notifications (`notify-rust`) on a short-lived
    thread; bursts summarized.
- Mail data flow: real profiles start empty (welcome screen → Settings ▸
  Accounts); sample mail is seeded only with `BGMAIL_DEMO`/`BGMAIL_SAMPLE_DIR`.
  Message actions call `mail_sync::actions` (local change + queued server
  op) and then `Command::ProcessQueue`; opening a message marks it read and
  fetches a missing body (`Command::FetchBody`). Sending enqueues the built
  message in the outbox (`RootView::send_draft`). A failed action shows a
  localized status note (`StatusActionFailed: {error}`), never only a log.
- Unsent mail is never silently stuck: outbox rows that stopped retrying
  (`storage::Database::failed_outgoing`: `failed` / `maybe_sent`) show as
  "N unsent" (`Color::Warning`) in the status bar, refreshed on reload and on
  `Sent`/`SendFailed`. Clicking it opens a panel listing subject (parsed from
  the raw), recipients, last error and, for `maybe_sent`, "May already have
  been sent", with Retry (`retry_outgoing` + `process_queue`), Edit (compose
  prefilled from the raw with `ComposeInit::replaces_outgoing`; the row is
  deleted only once the edited copy is queued) and Discard (confirmed with
  `window.prompt`, `delete_outgoing`).
- Account removal and secret updates never block the UI thread:
  `MailService` start/stop return `BlockingJob`s (last request per account
  wins, run one at a time) executed on `cx.background_spawn`; removal hides
  the account at once (`removing_accounts`), deletes its mail on its own
  `storage::Database` connection, then reloads (errors → status note).
- Windows that read `RootView` while rendering (settings) are opened with
  `cx.defer`, outside `RootView`'s update (GPUI renders new windows
  synchronously; re-entering the entity panics).
- Message list: virtualized with GPUI `uniform_list` (uniform row height;
  only visible rows are built) and the `ui::Scrollbar` on the list handle's
  `base_handle`. It owns a focus handle with key context `MessageList`
  (focused on startup / whenever nothing has focus, and on row click):
  `down`/`j` and `up`/`k` select + open the next/previous message (kept
  visible via `scroll_to_item`), `delete`/`backspace` move it to Trash.
  These plain keys are bound only in that context, never in text fields.
- Sidebar folders: system folders first (Inbox, Drafts, Sent, Archive, Junk,
  Trash, Flagged; names localized), then user folders alphabetically by
  `display_name`.
- Reader attachments: non-inline `storage::list_attachments` rows show as
  chips under the header once the body is downloaded (`BodyReady` reloads
  them). A chip opens a `ui::ContextMenu`: Open (decoded on a background task
  with `mail_core::attachment_content`, written to a private temp copy,
  `open_with_system`), Save As… (`prompt_for_new_path` starting in
  Downloads), Save to Downloads. Results/errors go to the status bar.
- List header: Get mail (sync now), filter (All / Unread / Starred /
  Attachments; session state, the open message stays listed even if it stops
  matching) and more. Toolbar: compose on the left; reply / reply all /
  forward, trash / archive / junk, flag / mark read|unread / move centered;
  the star button is filled and yellow on a starred message;
  message buttons are disabled when their command isn't available. Move opens
  a dropdown of `CommandContext::move_targets`.
- Message rows: Ctrl/Cmd+click toggles, Shift+click and Shift+arrows take a
  range, Ctrl/Cmd+A selects all (list focus only). Two or more selected rows
  (`multi_selection`, with `selected_message_id` = `None`) show, as in macOS
  Mail, the row picked last (`multi_preview`, not marked read) on a sheet, with
  tilted sheets behind it and a floating bar: Delete, Archive, Junk, star,
  read/unread and Move, or Restore and Delete forever (confirmed) in the Trash.
  Bulk star and read keep the selection and follow the whole selection (any
  unread: mark all read; not all starred: star all). Any other bulk action never deletes for
  good and leaves nothing selected. On hover Reply / Delete appear centered at
  the right, icon-sized and with no background (their icons brighten on hover);
  only the subject shrinks for them, so the date and star stay in place. Trash shows no unread badge.
- Replies carry Gmail's structure: `In-Reply-To`/`References`, a `Re:`
  subject, Gmail's attribution line and an HTML alternative: the original's
  sanitized HTML (`mail_html::quotable_html`, without `cid:` images) while the
  prefilled quote is intact, otherwise built from the edited text.
- Command palette: `cmd-k` / `ctrl-k`. Mail shortcuts (mac / other): Reply `cmd-r`/`ctrl-r`, Reply All
  `cmd-shift-r`/`ctrl-shift-r`, Forward `cmd-shift-f`/`ctrl-shift-f`, Mark
  read/unread `cmd-shift-u`/`ctrl-shift-m` (Ctrl+Shift+U is the Linux IME
  Unicode key), Get New Mail `cmd-shift-n`/`ctrl-shift-n`; also in the
  Message and Mailbox menus and the command palette.
- Development preview: `BGMAIL_DB` opens another database instead of
  `~/.config/BGMail/mail.db`, and `BGMAIL_SAMPLE_DIR` adds its `*.html` to
  the seed (only when that database is first created);
  `spikes/html_render/tools/run_preview.sh` uses both.
- UI state: `Entity` + `impl Render`; mutate via `cx.listener` + `cx.notify()`.
- OS-specific code only in abstracted helpers (`window_drag`, notifications,
  etc.), never leaked into layout/domain types.
- Network I/O only through `protocols` / `mail_sync`, never on the UI
  thread: discovery, login tests and Google sign-in run on the background
  executor; a running sign-in has a Cancel button (`CancelToken`) and is
  cancelled when the settings window closes. Account secrets go to the
  platform keychain via `RootView` (never into `config_json`).
- Password fields use `TextInput::set_masked(true)`.
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
- Packaging: `cargo packager --release` (on Linux add `NO_STRIP=1` when the
  host libs use `.relr.dyn`, which linuxdeploy's bundled `strip` rejects);
  inspect the deb/AppImage `.desktop` and icons.
- Workspace `fmt` / `clippy -D warnings` / `test --workspace`.

## Child DOX Index

| Scope | Doc | Owns |
|---|---|---|
| `crates/bgmail/src/` | [`.dox/crates/bgmail/src/AGENTS.md`](src/AGENTS.md) | Full API catalog (every fn/method in the binary) |
| `crates/bgmail/assets/` | [`.dox/crates/bgmail/assets/AGENTS.md`](assets/AGENTS.md) | Bundled sample e-mails and reader fixture images |
