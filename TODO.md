# TODO — BGMail

> Source of truth for progress. **Keep it up to date** (see rule 8 in
> [`AGENTS.md`](AGENTS.md)). Legend: ✅ done · 🔄 in progress · ⬜ pending.

## Stage 0 — Foundation & Planning
- ✅ Define vision, architecture and scope (`docs/PLANEJAMENTO.md`)
- ✅ Write the rules for agents (`AGENTS.md`)
- ✅ Create this `TODO.md`
- ✅ Set up the Cargo workspace + pinned toolchain (`rust-toolchain.toml`)
- ✅ `.gitignore`

## Stage 1 — Visual mock (current)
- ✅ `theme` crate: `ThemeColors`, dark theme (VSCode Dark Modern) and light
      (VSCode Light Modern), `ActiveTheme`, toggle + tests
- ✅ `ui` crate: prelude, `Color`, `Label`, `Icon`, `Button`, `IconButton`,
      `ListItem`, `h_flex`/`v_flex` helpers
- ✅ Sample data (`crates/rmail/src/data.rs`) + tests
- ✅ macOS Mail-style three-column layout (sidebar, list, reader)
- ✅ Unified toolbar laid out like macOS Mail: sidebar toggle (left), mailbox
      title + message count and filter/more over the list, and compose +
      reply/reply-all/forward, archive/trash/junk, move ▾, flag ▾, search box
      over the reader. List title moved out of the list into the toolbar.
- ✅ Sidebar visibility toggle from the toolbar (`show_sidebar` + tests)
- ✅ Resizable columns (draggable dividers): sidebar (min 250px) and message
      list (min 350px) resize via drag handles; the reader keeps a 550px floor,
      and the window enforces a 950px-wide minimum (`WINDOW_MIN_WIDTH`) so the
      docked list + reader can't clip the titlebar window controls. Dragging a
      divider is locked once the reader hits its floor (the `sidebar + list <=
      total - READER_MIN_WIDTH` invariant holds across drags; covered by
      `resize_locks_panels_once_reader_minimum_is_reached`), and the reader's
      toolbar segment is `min_w_0` so it yields width to the (flex-shrink-0)
      window controls instead of pushing the close button off-screen.
      Below a 900px window width the sidebar auto-collapses and, once reopened,
      floats over the content with a dismissable scrim (`sync_layout`/`resize` +
      tests)
- ✅ Collapsible account groups in the sidebar: clicking an account header folds
      its mailbox list with an accordion animation (height grow/shrink + fade,
      fixed duration regardless of item count) and the disclosure chevron rotates
      0↔90° (an SVG served from `ui::Assets`, since font glyphs can't rotate)
      inside a fixed-size box. Per-account `FoldAnim` state + token-guarded
      finalize timer; rows pinned to a fixed height so the height math never snaps
      (`toggle_account`/`clear_fold`/`account_list_visible` + tests). Custom user
      folders supported (`MailboxKind::Custom` + `Mailbox::label`/`custom`), with
      the Work account seeded with five. Webview HTML rebuild memoized
      (`last_webview_sig`) so animation frames don't re-theme the reader.
- ✅ Unified (global) mailboxes pinned to the top of the sidebar above the
      account groups — Inbox, Flagged, Drafts, Sent (`data::GlobalMailbox`,
      localized; new `MailboxFlagged` key). Sidebar selection generalized to a
      `Selection` enum (`Global` | `Mailbox`), defaulting to the unified inbox;
      aggregated counts via `global_unread` (mock: inbox sums every account,
      flagged counts starred). Will fan out across all accounts once the mail
      layer lands.
- ✅ Draggable title bar: the whole top toolbar moves the window (arm on
      mouse-down, start on first move so toolbar buttons still click; double-click
      runs the platform title-bar action). Portable `window_drag` helper —
      `Window::start_window_move` on Linux/Windows, AppKit
      `performWindowDragWithEvent:` on macOS (GPUI 0.2 has no macOS impl)
- ✅ Status bar
- ✅ Real-time light/dark theme switching
- ✅ Zed-style settings in a separate window (General/Accounts/Appearance/
      Notifications): the toolbar gear opens a dedicated `SettingsView` window
      (reused/refocused if already open) instead of replacing the main content;
      theme/language are app globals so changes apply live to both windows
- ✅ First workspace build (`cargo build --workspace`), `cargo clippy` with no
      warnings and `cargo test --workspace` passing; app starts without crashing
- ✅ macOS/Xcode 26: unbundled Metal Toolchain — `xcodebuild -downloadComponent
      MetalToolchain` + `.cargo/config.toml` forcing
      `TOOLCHAINS = "com.apple.dt.toolchain.Metal"` (gpui's build script uses
      `xcrun -sdk macosx metal`, which does not find the stub)
- ✅ Icons as embedded SVGs (no icon font): every `IconName` maps to an SVG under
      `assets/icons/`, embedded in `crates/ui/src/assets.rs` and rendered with
      `gpui::svg()` (tinted by the icon's color). This replaces the FontAwesome
      glyph *font*, whose face/codepoint lookup rendered inconsistently across
      platforms (broke on macOS after a Windows fix). To keep the exact previous
      look, the SVGs are the official FontAwesome 6 Free *Solid* artwork (Star uses
      Regular for the outline variant; icons CC BY 4.0), normalized to a square
      viewBox so every icon shares a consistent footprint. `chevron-right.svg`
      stays a hand-drawn stroke chevron because the sidebar disclosure rotates it.
- ✅ Localization (i18n): `crates/rmail/src/locale.rs` with `Language`
      (English default + Brazilian Portuguese), a string catalog and an
      `ActiveLanguage` global; UI resolves strings at render time, with a
      language picker in the General settings. English is the default everywhere.
- ✅ Custom `ui::Scrollbar` element (vertical, draggable thumb + track click)
      overlaying the message list and sidebar; translucent thumb colors added to
      the theme. Default arrow cursor over the strip. Auto-hide: shown while
      hovering the strip, dragging, or scrolling (incl. mouse wheel), and hidden
      ~250ms after scrolling stops or when the mouse leaves the strip. Tested via
      pure thumb-geometry + scroll-recency functions. Mock expanded (18 messages,
      5 accounts, long bodies) so the panels overflow.
- ✅ HTML e-mail viewer via a **native embedded webview** (`wry`: WKWebView on
      macOS, WebView2 on Windows; CEF OSR → GPUI texture on Linux via default
      `linux-webview`),
      replacing the hand-rolled GPUI renderer. The OS engine handles layout,
      scrolling, text selection and copy natively. The webview is a child of the
      GPUI window, layered over the reader body; a `canvas` element keeps its
      bounds in sync each paint, and it is hidden when the reader isn't on screen.
      `crates/rmail/src/web_view.rs` owns the platform abstraction (`EmailWebView`,
      no-op on unsupported targets) plus a themed `email_document` builder
      (theme-aware CSS, dark/light `color-scheme`); the reader falls back to a
      plain-text view where no webview backend exists. `Message::body` stays a
      `MessageBody::{Html, Text}` and the mock mixes both. The first mock message
      embeds a real 700×200 PNG (`crates/rmail/assets/tweezers.png`) as a
      self-contained base64 `data:` URI with explicit `width`/`height`. Tested:
      HTML escaping, color formatting, document assembly (scheme + body),
      dependency-free base64 (RFC 4648 vectors), the data URI shape and the image
      magic bytes (guards against a mislabeled WebP).
- ✅ Rudimentary persisted settings (`crates/rmail/src/config.rs`): main window
      position + size (the *restored* bounds — while maximized the full-screen
      frame is never persisted), a       `maximized` flag plus the maximized frame (macOS zoom via
      `Window::is_maximized`). On reopen it starts maximized without flicker —
      Windows/Linux open with `WindowBounds::Maximized`; macOS opens *windowed
      directly at the saved maximized frame* (GPUI's zoom is async and animates).
      Dragging the title bar restores to the saved size under the cursor (manual
      `setFrame` on macOS, native elsewhere). Persistence is armed only after the
      window settles, so opening never overwrites the saved layout. Also the two
      resizable column widths (sidebar default 200px, min 150px; list), stored as
      JSON at
      `~/.config/BGMail/config.json` on every platform (fixed path; home resolved
      via `HOME`/`USERPROFILE`, no extra dep). Loaded at startup (clamped to the
      window/column minimums); saved best-effort on a background thread, debounced
      (~500ms, token-guarded) so a live window/column drag only writes once it
      settles. Tested: JSON round-trip, partial/invalid configs falling back to
      defaults, on-disk save/load round-trip and the fixed path shape.
- ✅ **Measure the startup time** with instrumentation (`startup.rs`: monotonic
      milestones logged to stderr in debug builds — `main entered`, `application
      run callback`, `main window opened`, `first ready frame`)
- ✅ Clickable links / external navigation policy for the webview (open in the
      system browser instead of inside the embedded view)
- ✅ Disable the webview's OS Web Inspector (`with_devtools(false)`): wry enables
      devtools by default in debug builds, which added "Inspect Element" to the
      context menu and, once opened, attached an inspector that resized the child
      WKWebView so it overflowed the reader pane. The body now stays sandboxed.
- ✅ Custom in-webview context menus for **images and links**, because WebKit's
      native image menu items can't be wired without subclassing `WKWebView`
      (`willOpenMenu`, Obj-C — disallowed here): "Download Image" never reaches
      the download delegate and exposes no URL (confirmed; Apple/wry limitation).
      An injected content script replaces the context menu for images and links
      (the native menu stays on plain text, so selection/copy still works),
      reading localized labels from `<body data-rm-*>` and colors from the
      document's CSS variables, so they follow the language/theme without
      rebuilding the view. The link menu offers "Open in browser" + "Copy link".
      Actions route over a small namespaced IPC protocol (`H`/`O`/`D`/`C` →
      hover / open / download / copy; `parse_ipc_message`). Foreground-only
      actions (status-bar hover, clipboard via `cx.write_to_clipboard`) are sent
      to GPUI as a `HostEvent` over the channel; open/download run inline:
      - "Open image in browser": http/https/mailto open in the system browser;
        inline `data:` images are base64-decoded to a temp file and handed to the
        OS default viewer (`decode_data_uri`/`extension_for_mime`/`base64_decode`,
        all dependency-free and unit-tested).
      - "Download image": saves straight to `~/Downloads` (no dialog) with a
        non-clobbering ` (n)` name (`downloads_dir`/`unique_download_path`), then
        fires a desktop notification. macOS uses `osascript` (works for the
        unbundled `cargo run` binary, unlike `UNUserNotificationCenter`);
        `applescript_string` safely quotes the text. Windows notification backend
        is still TODO. Localized notification text is shared via `set_notify_text`
        so a live language switch updates it.
      The navigation handler is deliberately left untouched so the initial
      `data:`/`about:` document load isn't intercepted.
- ✅ Suppress the macOS overlay-scrollbar flash in the reader. On a trackpad the
      WKWebView's overlay scrollbars auto-hide and visibly flicker on every
      gesture (incl. the one that pops the context menu); with a plugged-in mouse
      they stay put and don't flicker. The document CSS now styles
      `::-webkit-scrollbar`, which opts WebKit out of overlay scrollbars in favor
      of a themed, always-present bar — matching the steady mouse behavior.
      (The right-click→menu latency itself comes from GPUI and is out of scope.)
- ✅ Sanitize the e-mail HTML before it reaches the web engine (`sanitize_html`
      in `web_view.rs`, applied to `MessageBody::Html`). Uses `lol_html` (a real
      HTML rewriter, not regex, so malformed markup can't smuggle tags through).
      Two passes per element: drop disallowed elements (+content), then neutralize
      XSS-bearing attributes on survivors (`neutralize_attributes`).
      - **Removed elements** (`DISALLOWED_ELEMENTS`): scripts/plugins
        (`script`/`object`/`embed`/`applet`); frames (`iframe`/`frame`/`frameset`
        — our menu script is main-frame-only, so a sub-frame would resurface the
        native "Reload" menu); media (`video`/`audio`/`source`/`track`);
        editable/interactive controls (`input`/`textarea`/`select`/`button`/`form`);
        document/redirect/external-resource heads (`base`/`meta`/`link`); and misc
        scripting surfaces (`canvas`/`dialog`/`portal`). `head`/`title` are kept so
        full-document e-mails don't lose their `<style>`.
      - **`svg` is kept** (inline vector art), but its scriptable parts are removed:
        `script`, `foreignObject` and the SMIL family (`animate`/`animatetransform`/
        `animatemotion`/`set`).
      - **Attribute neutralization**: every inline event handler (`on*`),
        `contenteditable`, dangerous URL schemes on link/source attrs
        (`javascript:`/`vbscript:`, and non-image `data:` incl. `image/svg+xml` —
        `is_dangerous_url`), and `style` declarations carrying legacy CSS script
        vectors (`expression(`/`-moz-binding`/`behavior:`/`url(javascript:`).
      Everything else — including inline styles, tables and links — is preserved
      verbatim; a rewrite failure drops the body rather than render unsanitized.
- ✅ Remote-content (tracking-pixel) blocking, **user-configurable**. New
      persisted `Config.load_remote_images` (default **off**; `#[serde(default)]`
      keeps old config files loading). When off, `sanitize_html`/`neutralize_attributes`
      strip remote (`http(s)`/protocol-relative — `is_remote_url`) URL attributes
      and `srcset`; inline `data:` images always render. Threaded through
      `email_document(load_remote)` and `RootView.sync_webview` (added to the
      memoization `signature` so toggling rebuilds the doc). A new **Privacy**
      settings section (shield icon) hosts an on/off control wired to the main
      view via a `WeakEntity<RootView>` (`set_load_remote_images` → persist +
      re-render). Sample rich body now embeds one remote `<img>` next to the
      inline one so the toggle is observable. Localized (EN/PT) label + hint.
      CSS `url(...)` (background images, web fonts, `@import`) in inline `style`
      and `<style>` blocks is also neutralized when blocking, via a blunt textual
      pass (`strip_css_urls`, `url(...)` → `url()`) — deliberately not a CSS
      parser. `<link>`/media/iframes are removed outright regardless.
      - A blocked remote `<img>` keeps its URL out-of-band in
        `data-rm-blocked-src` (the `src` is still dropped, so nothing loads) and
        gets a themed dashed placeholder. The content script then (a) mirrors that
        URL into the status bar on hover (reusing the `H` hover IPC) and (b) adds
        a "Show remote image" context-menu item that loads just that one image in
        place (sets `src` from the stashed URL — pure JS, no host round-trip).
        Localized label `CtxShowImage` (EN/PT) via the `data-rm-img-show` attr.
        Blocked placeholders now use the default cursor (no `context-menu` hint).
      - Reader-header privacy affordance: `email_document` returns a
        `RenderedEmail { html, has_remote, blocked_images }` (the sanitizer
        reports whether the message has remote `<img>` and how many are still
        blocked); `RootView` caches `content_has_remote` + `content_blocked_count`.
        On the subject line a compact, fixed-size shield slot (so the header
        height is identical with or without it) shows, when the message has
        remote images and the global setting is off: a **red** `Shield`
        (`Color::Error`, tooltip `BlockedElements` with the live count) that on
        click opens a deferred/anchored dropdown "Unblock all remote content"
        (`UnblockRemote`), or a **green** `ShieldSolid`+`Check` overlay
        (`Color::Success`, check in the background color, tooltip
        `RemoteContentLoaded`) once `blocked_images == 0`.
      - Two unblock paths, both **per message** (never the global setting):
        the dropdown fully unblocks (`unblocked_messages: HashSet<usize>` →
        effective `load_remote`), or the in-body "Show remote image" reveals one
        image at a time. The latter now also posts an `S` IPC →
        `HostEvent::ImageShown(url)`; the host records the URL in
        `shown_images: HashMap<usize, HashSet<String>>` (so it stays shown across
        re-renders — `sanitize`/`neutralize_attributes` take the allowlist) and
        decrements the blocked count. When the user reveals **all** images the
        count reaches zero and the shield turns green on its own. A full-window
        catcher closes the menu on outside click. New `ui::Tooltip` (text)
        component, `IconName::Check`/`ShieldSolid`, `IconSize::XXSmall`, and
        EN/PT keys. Mock body now embeds two remote images so the count is
        observable.
- ✅ Scrollbar fade animation for list/sidebar (currently instant show/hide)
- ✅ Windows titlebar controls for the custom transparent main window, with
      explicit minimize/maximize/close actions. The buttons occlude the draggable
      toolbar hitbox, so window dragging still works without changing the macOS
      traffic-light layout
- ✅ Linux client-side decorations (Zed-style): request
      `WindowDecorations::Client`, draw caption buttons in the toolbar, and wrap
      the main UI with shadow/resize chrome when the compositor grants CSD
- ✅ Linux HTML reader via **CEF windowless off-screen rendering** (replaces the
      earlier WebKitGTK/`wry` X11-only child embed), on by default via the
      `linux-webview` feature (`cef` crate downloads the CEF runtime on first
      build). Chromium renders each body to an off-screen BGRA buffer that GPUI
      composites as a `RenderImage` texture in the reader pane — works natively on
      Wayland and X11. `crates/rmail/src/cef_osr.rs` owns the CEF integration
      (app/render/display/request handlers, external message pump, mouse input,
      `data:` URL loading, and the console→IPC bridge that mirrors the `wry`
      `with_ipc_handler`). `web_view.rs` selects the backend: `wry` child webview
      on macOS/Windows (`COMPOSITES_IN_GPUI = false`), CEF OSR on Linux
      (`COMPOSITES_IN_GPUI = true`, reader drives `paint` + pointer forwarding).
      Scroll path tuned for soft OSR: disable Chromium smooth-scrolling (full-buffer
      paints per tick), coalesce trackpad wheel deltas per GPUI frame, reuse the
      BGRA staging buffer across `on_paint`, and keep a short redraw warm loop via
      `Context::notify` so late CEF paints are not dropped. On window resize, a
      stale buffer is drawn at its native pixel size (top-left) instead of being
      stretched into the new bounds, and CEF is `was_resized`/`invalidate`d until a
      matching frame arrives. After focus loss, Chromium would throttle the OSR
      browser and the next message switch could stall >2s: disable background
      timer/renderer throttling, keep a persistent ~8ms CEF message-loop tick
      while the webview exists (GPUI often stops painting when inactive), and wake
      on `LoadHandler::on_load_end` / window activate (`was_hidden(0)`, `set_focus`,
      `invalidate`).
      GTK/XWayland preference and `BGMAIL_NATIVE_WAYLAND` are gone. Disable with
      `--no-default-features`.
      - ⬜ Follow-ups: zero-copy GPU texture import (CEF `accelerated_osr`/DMA-BUF),
        desktop notification backend for image downloads, and production sandbox
        (currently `no_sandbox` for dev). Keyboard + cursor for OSR are wired
        (`send_key_event`, `on_cursor_change`, reader focus, Ctrl/Cmd+C via IPC).
        Context-menu actions (Copy / Show remote image) run on `mousedown`; the
        host only dismisses the menu for clicks *outside* the reader (OSR clicks
        are forwarded into CEF, so an early `__rmCloseMenu` raced the item handler).
        Blocked-image menu checks `data-rm-blocked-src` before `img.src` (empty
        `src` still yields a truthy resolved URL against the `data:` document).
- ✅ Fix Windows WebView2 text compositing in the reader by giving the native
      child webview an explicit opaque background that matches the e-mail document
- ✅ Work around Windows WebView2 GPU/DirectComposition transparency failures:
      preserve wry's default browser args and add Microsoft's recommended GPU
      flags for cases where content is interactive but text/page pixels are not
      visible
- ✅ Avoid creating the Windows WebView2 reader hidden: initialize it as a tiny
      visible child surface, then move it to the measured GPUI canvas bounds on
      first paint, avoiding known WebView2 hidden-initialization blank renders
- ✅ Disable GPUI DirectComposition on Windows before platform initialization
      (`GPUI_DISABLE_DIRECT_COMPOSITION=1`) because its `WS_EX_NOREDIRECTIONBITMAP`
      window path does not compose reliably with WebView2's child HWND
- ✅ Fix the Windows maximized-open regression caused by disabling
      DirectComposition: the maximize-on-open is applied asynchronously
      (`ShowWindowAsync(SW_MAXIMIZE)`) and the `WM_SIZE` that grows GPUI's cached
      viewport can be dropped during the busy open sequence, so the UI laid out
      against the small base viewport while the window was already maximized
      (content too small, empty space until a manual resize). Hold the UI behind a
      plain background (`RootView::content_ready`) until the window settles, and
      poll on open re-posting `WM_SIZE` (`window_drag::nudge_window_resize`)
      whenever GPUI's viewport is stale, so it re-reads the real size without
      un-maximizing
- ✅ Remove the open flicker: cloak the window on open so the first paint (and, on
      Windows, the asynchronous maximize) happens off-screen, then reveal it after a
      short settle delay — the window appears already laid out and fully rendered,
      with no restore→maximize or paint-in flash. Windows creates it hidden
      (`show: false`) and uses DWM cloaking; macOS uses the window's `alphaValue`
      (`window_drag::set_window_cloaked`)
- ⬜ Verify the WebView2 GPU workaround visually on the affected Windows host
      after fully restarting the app process
- ⬜ UI tests with `gpui::TestAppContext` (after stabilizing the mock)
- ✅ Functional search field (filters the mock list)
- ✅ Fix main-window search focus on first click so the text input accepts typing
      immediately on Windows
- ✅ Refine text-input caret metrics in the search field (2px width, 4px vertical padding)
- ✅ Debounce the search filter by 150ms after typing
- ✅ Search clear micro-button inside the expanded field (with text on wide toolbar;
      always visible when compact search is force-expanded; clears and collapses)
- ✅ E-mail composition screen/panel (mock): toolbar compose button opens a
      dedicated `ComposeView` window (reused/refocused if already open) with Send,
      Attach and Discard actions (FontAwesome SVG icons), header fields
      (From/To/Cc/Bcc/Subject) and a body placeholder; Cc/Bcc toggle and From
      account cycling are interactive mocks; compose window position/size persist
      in `config.json` (default 790×720, centered on first open)
## Stage 2 — Domain layer
- ⬜ `mail_core` crate: `Account`, `Mailbox`, `Message`, `Thread`, `Attachment`
- ✅ `storage` crate: SQLite persistence (`accounts`, `folders`, `messages` with
      `plain_text` + `raw_content`, `folders_csv` multi-folder membership,
      accent-insensitive `search_text` + tests)
- ✅ Seed visual mock into `~/.config/BGMail/mail.db` on first open
      (`db_seed` + `storage::seed_if_empty`)
- ✅ `RootView` wired to SQLite: folder selection, global mailboxes, list
      preview from plain text, reader from raw HTML/text, search mode
      ("Searching" / "Buscando") across all accounts
- 🔄 Synchronization state machine

## Stage 3 — Connectivity
- ⬜ `protocols` crate: generic traits (Fetch/Send) + IMAP/POP3/SMTP
- ⬜ Gmail via OAuth2 + API
- ⬜ `accounts` crate: account and credential management (per-platform keychain)

## Stage 4 — Client features (see scope in PLANEJAMENTO.md §6)
- ⬜ Read / mark read-unread
- ⬜ Compose / reply / reply all / forward
- ⬜ Send (SMTP + Gmail)
- ✅ Move / delete / archive / junk (SQLite folder membership + toolbar/menus)
- ✅ Star / flag (toggle starred + `sys:flagged` folder)
- ✅ Command palette (Ctrl/Cmd+P) with contextual message commands
- ✅ macOS menu bar (File/Edit/View/Message; unavailable actions omitted on GPUI 0.2)
- ⬜ Attachments (view, download, attach)
- ⬜ Search (UI mock done — DB-backed accent-insensitive search in Stage 2;
      full IMAP/search scope in Stage 4)

## Stage 5 — Polish
- ✅ Rename product to **BGMail** (binary, window/menu titles, themes, docs;
      config/db under `~/.config/BGMail/`)
- 🔄 Keyboard shortcuts (command palette + Cmd+Q; per-action bindings still TODO)
- ⬜ Accessibility
- ⬜ Native notifications
- ⬜ Packaging (macOS `.app`, Linux, Windows)

## Notes / open decisions
- Reassess GPUI crates.io `0.2.2` vs `main` if some API is missing.
- ~~Define the local storage format (SQLite vs files) in Stage 2.~~ **SQLite**
      (`crates/storage`, `mail.db` under `~/.config/BGMail/`).
- Add more UI languages by extending `locale::Language` and the catalog.
