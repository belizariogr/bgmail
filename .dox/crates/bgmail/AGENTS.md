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
- After removing the open message (delete, archive, junk, move), nothing is open and
  the next message isn't marked read, as in Gmail without auto-advance (user
  decision). The arrow keys open the row that took its place.
- Each mailbox remembers its selection for the session (`remembered_selections`,
  user decision): switching away and back selects the same rows again if still
  listed, without marking them read. Not persisted. A search records none of its own:
  it remembers the mailbox's when it starts and gives it back when it ends, unless a
  result the user opened is listed there too (user decision).
- The UI language persists as `Config::language` (`Language::code`, a string so an
  unknown value loses only the language). Change it through `RootView::set_language`,
  from Settings or the first-run welcome step (`settings::language_picker`).
- Commands on messages only act from the main window (`schedule_message_command`), and
  the delete shortcut never from a text field (`NoAction` in `TextInput`/`TextArea`).
- Recipients (user decision): Enter and Tab take a complete typed address (with a dot
  in the domain) over the highlighted suggestion; picking one with the arrows still
  wins.
- One process per profile (user decision): a later launch activates the running window
  and exits; a build from another worktree needs its own `HOME` to run alongside one.
- "Open" on an attachment refuses types that can run code (`attachments::runs_code`,
  user decision); Save still works.
- A message being written is never lost silently: Close (shortcut, X, menu), quitting
  and closing the main window ask through `ComposeView::discard_draft`, and a reply,
  forward or `mailto:` doesn't replace it (`Key::StatusComposeBusy`). Drafts stay a
  TODO item.
- Zoom of the whole app (user decision): Ctrl/Cmd `+`/`-`/`0` (global actions
  `AppZoomIn`/`AppZoomOut`/`AppZoomReset`, also in the View menu) step
  `app_zoom`'s levels (75%–200%) through `RootView::zoom_app`, applied to every open
  window with `Window::set_content_scale` (`vendor/gpui`) and on opening a new one,
  persisted as `Config::ui_scale`. Coordinates from `window_bounds()` aren't scaled;
  compare them with `window_frame::logical_window_size`. The page zoom of the reader
  remains on Ctrl/Cmd + wheel only.
- Sent mail can only be deleted (user decision): Archive, Junk, Move and dragging
  are unavailable while the list shows Sent, and everywhere for a message that is
  in Sent but not in the Inbox (`commands::message_only_in_sent`; Gmail's All Mail
  copy doesn't change this, mail sent to oneself stays movable from the Inbox). One
  such row blocks them for a whole multiple selection.
- First-run setup (`onboarding::OnboardingView`, rendered by `RootView` instead
  of the toolbar and columns, under a title bar with the window controls): welcome
  → theme → account → conclusion. Shown only while `needs_onboarding` holds (setup
  never finished and no account), so existing profiles skip it; finishing or
  skipping persists `onboarding_completed`. The account step embeds
  `SettingsView::form_only` (one form implementation). The default-mail-app switch
  is opt-in (user decision: a switch on the last step), applied on finish, asked
  once; a system message keeps the screen open until the next click.
- The theme choice persists as `Config::light_theme`; change it through
  `RootView::set_appearance`, never `theme::set_theme` directly from a view.
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
  zoom (Ctrl/Cmd + wheel; `HtmlView`) shows in the
  status bar when not 100%.
  `html_view::init` is called at startup.
- Printing (`MessagePrint`, Cmd/Ctrl+P) goes through the system browser
  (decided with two opposing reviews; GPUI has no print API): `print`
  writes `mail_html::print_document` (light page, header, CSP allowing only
  `data:` images and its own nonce'd `print()` script) to a private temp
  file and `open_with_system` opens it.
- Reader robustness: HTML is always shown formatted, whatever its size (user
  decision). Bodies from 256 KB on are sanitized, parsed and laid out on a
  background thread with a placeholder meanwhile (`ReaderJob`; newest selection
  wins); smaller ones build in place, so no placeholder flashes. No parsed
  document cache: measured, the largest real message builds in about 180 ms, so a
  cache would only save that on reopening at a memory cost. Bodies
  `html_view` refuses (too deep or too complex) show as plain text, and a message that was
  showing formatted when the app crashed or was killed (the `crash_guard` marker survived,
  armed for as long as it shows) as plain text, each with a notice. Reply quotes follow the
  same rules with a marker of their own. A plain-text body shows up to `MAX_SHOWN_TEXT_BYTES`
  (2 MiB, cut at a line, with a notice; decided from Gmail clipping at about 102 KB and
  Thunderbird hanging on huge bodies); replying and forwarding use all of it. A first layout past
  2 s on the UI thread (`UI_LAYOUT_LIMIT`) or 30 s in the background also opens as text, the last
  resort for what no other limit caught. A damaged `mail.db` is kept beside as `mail.db.corrupt`, and
  a new one starts (`Database::open_or_replace_corrupt`), with a status note: accounts are in it, so
  they have to be added again. The sanitizer's
  output is read back with the parser's tokenizer (`mail_html::reads_as_safe`, the last check): if it
  finds what the sanitizer removes, the body shows as text and the quote is left out. `form` and
  `button` are written out as `section` and `span`, keeping what they hold and its style (a whole ASP.NET message
  comes in a `<form>`; Gmail and Thunderbird show it); other controls go with their content. A later attribute name starting with `=` drops
  the tag only if the tag would be rewritten. Documents build
  off the pool's stack (`with_render_stack`). The
  release profile keeps `panic = "abort"` (decided with two opposing
  reviews): unwinding through Blitz/GPUI state isn't safe and stack overflow
  or OOM can't be caught anyway.
- Compose (`compose::ComposeView`, one window at a time): real editors
  (`ui::TextInput` To/Cc/Bcc/Subject, `ui::TextArea` body), From picker
  (`ui::ContextMenu`; local accounts listed but can't send), attachments via
  picker / drag-and-drop read off the UI thread (25 MB total cap). Formatting tools on
  the send button's row, as in Gmail (undo, font, size, B/I/U, color, align, lists, indent) drives
  `ui::TextArea`; formatted text is sent as HTML with a plain-text version, and
  unformatted new mail stays plain text. Send (button, `ComposeSend`, Cmd/Ctrl+Enter) validates with
  `mail_core::parse_address_list` (inline per-field errors) and hands the
  `Draft` to `RootView::send_draft` in a task (never while root is leased);
  success closes the window, errors show in a banner. Replies prefill from
  `ComposeInit` (`RootView::open_compose_with`; an open window is refilled)
  and focus the body; forwards focus To. Discard asks for
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
  - `print` — private print pages (`handoff_dir`: user cache on Linux, temp dir
    elsewhere; Unix 0700/0600, stale pages removed on the next print).
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
  - `recipients` — To/Cc/Bcc field: addresses as cards (name, with the full
    address on hover), contact suggestions while typing, text that isn't an address
    stays to be fixed.
  - `default_mail_app` — status and registration as the system's mail app, with
    OS-specific code behind one API (`unsafe` only for LaunchServices on macOS).
  - `instance` — one process per profile: lock file, and hand-off of links or
    "show the window" from later launches.
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
  (`storage::Database::unsent_outgoing`: `failed`, `maybe_sent`, or stuck in the queue; `RootView::watch_unsent` rereads it every minute) show as
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
- Contacts: `storage` catalog filled by itself, with no screen to manage it:
  senders with their name and recipients without one, by `mail_sync` for synced
  mail (Junk excluded, backfilled once per account) and `send_draft` for what is
  sent. A known name is never overwritten; a blank one is filled by later mail.
- Accounts have two editable names (Settings ▸ Accounts, also when adding one):
  the account name, listed in the sidebar, and the sender name, in the "From" of
  their mail. A blank sender name uses the account name.
- Default mail app: Settings ▸ General shows whether BGMail handles `mailto:`
  and offers "Make default" (`default_mail_app`, one API per OS, run off the UI
  thread and only on that click): Linux `xdg-mime` (refused when no installed
  desktop entry exists), macOS LaunchServices (needs the bundle, which declares
  the scheme through the packager's `deep-link-protocols`), Windows registers the
  app under HKCU and opens the system settings, where only the user can choose.
- `mailto:` links arrive as a process argument (Linux, Windows) or through
  `on_open_urls` (macOS bundle) and become a prefilled compose window
  (`RootView::open_mailto`); they never send and never replace a message with
  edits. Any later launch hands its link, or a request to show the window, to the
  running instance and exits (`instance`: `instance.lock` file lock, loopback TCP,
  private `~/.config/BGMail/instance` file with port and random token).
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
- Reader recipients, as in Gmail: "to" and first names on one line ("me" for the
  account's address), the whole address on hover, To and Cc in full behind a chevron.
- Dragging a list row (its whole selection when it is part of one) onto a folder of
  the same account moves it. It counts once past 100 px, for good: until then nothing
  follows the pointer and folders refuse the drop. While it counts the cursor is the closed hand; Windows has none, so
  the system's is hidden and one painted in its place. A selection across accounts can't be dropped,
  nor can Sent, Drafts or Starred take mail.
- Message rows: Ctrl/Cmd+click toggles, Shift+click and Shift+arrows take a
  range, Ctrl/Cmd+A selects all (list focus only). Two or more selected rows
  (`multi_selection`, with `selected_message_id` = `None`) show, as in macOS
  Mail, the row picked last (`multi_preview`, not marked read) on a sheet 10%
  smaller, its page scaled to match, with tilted, shadowed sheets behind it and a floating bar: Delete, Archive, Junk, star,
  read/unread and Move, or Restore and Delete forever (confirmed) in the Trash.
  In the Trash, the toolbar's delete button, a row's Delete button and the Delete
  key delete for good, after a confirmation. Bulk star and read keep the selection and follow the whole selection (any
  unread: mark all read; not all starred: star all). Any other bulk action never deletes for
  good and leaves nothing selected. On hover Reply / Delete appear centered at
  the right, icon-sized and with no background (their icons brighten on hover);
  only the subject shrinks for them, so the date and star stay in place. Trash shows no unread badge.
- Replies follow Gmail: the editor holds only what the user writes, and below
  it a `⋯` button reveals the attribution line and the original, rendered behind
  a side bar (read-only, parsed off the UI thread when first shown). The sent
  message has `In-Reply-To`/`References`, a `Re:` subject, Gmail's attribution
  line, `gmail_quote` markup around the original's sanitized HTML
  (`mail_html::quotable_html`, without `cid:` images) and a `> ` plain-text
  version. Forwards and edited unsent messages keep their text in the editor.
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

- `cargo test -p bgmail` (sanitize, config, locale, layout helpers, …; drags run in
  simulated windows through GPUI's `test-support`).
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
