# TODO — BGMail

> Source of truth for progress. **Keep it up to date** (see rule 8 in
> [`AGENTS.md`](AGENTS.md)). Legend: ✅ done · 🔄 in progress · ⬜ pending.

## Stage 0 — Foundation & Planning
- ✅ Define vision, architecture and scope (`docs/PLANEJAMENTO.md`)
- ✅ Write the rules for agents (`AGENTS.md`)
- ✅ Create this `TODO.md`
- ✅ Set up the Cargo workspace + pinned toolchain (`rust-toolchain.toml`)
- ✅ `.gitignore`
- ✅ DOX hierarchy: root Child DOX Index + `.dox/` mirrors for `crates/`
      (theme, ui, storage, bgmail), `docs/`, `assets/` (+ icons), `.cargo/`
- ✅ DOX full source API catalogs under `.dox/crates/*/src/AGENTS.md` (every
      function/method: purpose + behavior)
- ✅ Rename app crate path/package `rmail` → `bgmail` (`crates/bgmail`); remove
      `crates/rmail`

## Stage 1 — Visual mock (current)
- ✅ `theme` crate: `ThemeColors`, dark theme (VSCode Dark Modern) and light
      (VSCode Light Modern), `ActiveTheme`, toggle + tests
- ✅ `ui` crate: prelude, `Color`, `Label`, `Icon`, `Button`, `IconButton`,
      `ListItem`, `h_flex`/`v_flex` helpers
- ✅ Sample data (`crates/bgmail/src/data.rs`) + tests
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
- ✅ Localization (i18n): `crates/bgmail/src/locale.rs` with `Language`
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
- ✅ HTML e-mail viewer via **CEF windowless OSR** on all platforms (default
      `cef-osr` feature → GPUI texture; plain text with `--no-default-features`),
      replacing the hand-rolled GPUI renderer. The OS engine handles layout,
      scrolling, text selection and copy natively. The webview is a child of the
      GPUI window, layered over the reader body; a `canvas` element keeps its
      bounds in sync each paint, and it is hidden when the reader isn't on screen.
      `crates/bgmail/src/web_view.rs` owns the platform abstraction (`EmailWebView`,
      no-op on unsupported targets) plus a themed `email_document` builder
      (theme-aware CSS, dark/light `color-scheme`); the reader falls back to a
      plain-text view where no webview backend exists. `Message::body` stays a
      `MessageBody::{Html, Text}` and the mock mixes both. The first mock message
      embeds a real 700×200 PNG (`crates/bgmail/assets/tweezers.png`) as a
      self-contained base64 `data:` URI with explicit `width`/`height`. Tested:
      HTML escaping, color formatting, document assembly (scheme + body),
      dependency-free base64 (RFC 4648 vectors), the data URI shape and the image
      magic bytes (guards against a mislabeled WebP).
- ✅ Rudimentary persisted settings (`crates/bgmail/src/config.rs`): main window
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
      the main UI with shadow/resize chrome when the compositor grants CSD.
      Caption buttons use an Adwaita-like circular GTK style and follow the desktop
      `button-layout` (`gsettings org.gnome.desktop.wm.preferences button-layout`)
      plus compositor `WindowControls`, so GNOME's default close-only titlebar hides
      minimize/maximize; left/right placement from the layout string is respected.
- ✅ CEF OSR browser lifecycle: a single windowless browser per main window;
      message switches only `load_url` (`set_html`). Create counter + `Drop` close
      the host; `sync_webview` retries without spawning IPC tasks when CEF is not
      ready yet.
- ✅ HTML reader via **CEF windowless off-screen rendering** on Windows, Linux and
      macOS, on by default via the `cef-osr` feature (`linux-webview` remains an
      alias). Chromium renders each body to an off-screen BGRA buffer that GPUI
      composites as a `RenderImage` (`COMPOSITES_IN_GPUI = true` everywhere; the
      reader drives `paint` + pointer/keyboard forwarding). `crates/bgmail/src/cef_osr.rs`
      owns the CEF integration (handlers, external message pump, console→IPC).
      The former `wry` child webviews (WKWebView / WebView2) are removed.
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
- ✅ `mail_core` crate: addresses, flags, folder roles (special-use + names,
      IMAP UTF-7), MIME parse/build (`mail-parser`/`mail-builder`),
      reply/forward, threading keys, remote traits (`MailRemote`/`MailSender`)
      + in-memory `FakeRemote`/`FakeSender` (feature `fake`)
- ✅ `storage` schema v2: remote folders/UIDs (`message_locations`), raw
      bodies (BLOB), attachments, pending server ops, outbox, FTS5 trigram
      search, WAL; v1 databases migrate in place (accounts become `local`)
- ✅ `ui::TextArea`: multi-line editor (soft wrap, selection, undo/redo, IME)
      for the compose body; `TextInput`/`TextArea` emit `Changed`
- ⬜ GPUI `TestAppContext` tests for `TextArea`'s element code (needs gpui
      `test-support`)
- ✅ `storage` crate: SQLite persistence (`accounts`, `folders`, `messages` with
      `plain_text` + `raw_content`, `folders_csv` multi-folder membership,
      accent-insensitive `search_text` + tests)
- ✅ Seed visual mock into `~/.config/BGMail/mail.db` on first open
      (`db_seed` + `storage::seed_if_empty`)
- ✅ `RootView` wired to SQLite: folder selection, global mailboxes, list
      preview from plain text, reader from raw HTML/text, search mode
      ("Searching" / "Buscando") across all accounts
- ✅ Synchronization state machine (`mail_sync`): per-account worker + IDLE
      threads, pure planner (folder mapping, UIDVALIDITY reset, windowed
      header fetch, CONDSTORE flag refresh), optimistic actions + server-op
      queue, outbox (send then APPEND to Sent unless Gmail), body prefetch,
      `cid:` images inlined, orphan purge; tested against `FakeRemote`

## Stage 3 — Connectivity
- ✅ `protocols` crate: IMAP (`imap` 3.0-alpha + raw commands parsed with
      `imap-proto`: X-GM-*, COPYUID, HIGHESTMODSEQ), STARTTLS/TLS on one
      rustls 0.23 + platform verifier, SMTP (`lettre` transport), IDLE with
      prompt cancellation; GreenMail + Dovecot integration tests
      (`tests/greenmail.sh`, env-gated)
- ✅ Gmail via OAuth2 (PKCE loopback, refresh, id_token e-mail) + XOAUTH2 on
      IMAP/SMTP; client id from `BGMAIL_GOOGLE_CLIENT_ID`/`_SECRET` (GitHub secrets
      in release builds, gitignored `.env` locally)
- 🔄 Validate Google sign-in with a real account (client configured)
- ✅ Provider presets (Gmail, Outlook, Yahoo, iCloud, Fastmail, UOL, …) +
      Mozilla autoconfig/ISPDB + guess
- ✅ ~~`accounts` crate~~ dropped (decision above); keychain via GPUI
- ⬜ POP3 (postponed, see decision)
- ⬜ Microsoft OAuth (Outlook.com requires it; presets flag it)
- ⬜ Persist rotated OAuth refresh tokens (kept in memory today)

## Stage 4 — Client features (see scope in PLANEJAMENTO.md §6)
- ✅ Read / mark read-unread (opening marks read on the server; toggle via
      toolbar, menu, palette, Cmd/Ctrl+Shift+U · Ctrl+Shift+M)
- ✅ Compose / reply / reply all / forward (real fields, From picker,
      `ui::TextArea` body, quoting, forwarded attachments, address validation)
- ✅ Send (SMTP + Gmail XOAUTH2) through the outbox; copy APPENDed to Sent
      unless the server files it (Gmail)
- ✅ Mail list and reader round (user list, 2026-11-02): Ctrl/Cmd+K palette;
      `com.bgmail.main|settings|newmail` window classes; word/field selection on
      repeated clicks in `TextInput`; multiple selection (Ctrl/Cmd/Shift, Shift+
      arrows, Ctrl/Cmd+A) with the message on a stack of sheets, as in macOS Mail, and bulk Delete/Archive/Junk; row
      actions centered at the right, icon-sized, only the subject giving way;
      working Archive on Gmail, Move menu and list filter; Get mail moved above
      the list; star icon and "Starred"/"Com estrela"; no badge on Trash; always
      formatted; always-visible reader scrollbars; `[Gmail]/Rascunhos` as Drafts;
      replies with Gmail's markup; readable previews and quotes (HTML in the
      text part, all named entities; stored previews repaired on worker start);
      opaque selection over white pages; web fonts registered per file
      (messages no longer turn italic).
- ✅ Bulk star, read/unread and move; bulk restore and confirmed permanent
      deletion in the Trash
- ⬜ Selection on a dark page is still translucent, so a translucent window
      shows the desktop through it
- ✅ Replies quote the original's formatted HTML while the quote is intact
      (images embedded with `cid:` are left out)
- ⬜ Confirm on a real account: Archive, Move, Drafts folder, a sent reply
- ⬜ Parse large HTML bodies off the UI thread (they are always formatted now)
- ⬜ Drafts (save/resume) and HTML composing (plain text only today)
- ⬜ Set `\\Answered` on the original after a reply is sent
- ✅ Move / delete / archive / junk, reflected on the server (optimistic
      local change + queued op, MOVE or COPY/EXPUNGE)
- ✅ Star / flag (`\\Flagged` on the server + `sys:flagged` folder)
- ✅ Command palette (Ctrl/Cmd+Shift+P) with contextual message commands — in-window
      GPUI overlay (no `WindowKind::PopUp`; OSR composites under it on all platforms)
- ✅ macOS menu bar (File/Edit/View/Message; unavailable actions omitted on GPUI 0.2)
- ✅ Attachments: reader chips (Open / Save As… / Save to Downloads,
      sanitized private temp files), attach in compose (picker, drag & drop,
      25 MB cap), `cid:` images inlined
- ✅ Search: local FTS5 trigram over every synced message (accent-insensitive)
- ⬜ Server-side search for mail outside the sync window (IMAP SEARCH)
- ✅ Accounts: welcome screen, Settings ▸ Accounts (presets, autoconfig,
      manual servers, connection test, Google sign-in, update password,
      remove), secrets in the platform keychain
- ✅ Status bar sync state (Updating… / Updated at / Offline / sign-in
      failed), message list virtualized (`uniform_list`), keyboard navigation
- ✅ Validated end to end: `mail_sync/tests/greenmail_e2e.rs` (engine +
      real IMAP/SMTP + SQLite) and the running app against GreenMail (add
      account in the UI, read, reply, trash, IDLE notification, attachment,
      search, remove account)
- ✅ Unsent mail panel in the status bar (Retry / Edit / Discard): outbox
      state machine `queued → sending → append_pending` with `failed` /
      `maybe_sent` waiting for the user; an edit replaces its original
      atomically, so both never go out
- ✅ Two independent reviews of the engine, UI and packaging, all confirmed
      findings fixed (stale UIDs after UIDVALIDITY, EXPUNGE sparing other
      clients' `\Deleted`, 4xx back-off, duplicate sends, op/UI races, lost
      flags after moves without COPYUID, Sent copy retries, autoconfig
      preferring plaintext, OAuth redirects with a foreign state)
- ⬜ Known limitation: on servers without MOVE, if COPY works and the expunge
      fails, each replay leaves another copy in the target. Accepted: skipping
      the copy could lose mail another client flagged `\Deleted`
- ✅ Linux install scripts as in Zed (`script/`): tarball into `~/.local`
      (from a release or this checkout), or system-wide into `/usr`
- ⬜ Known limitation (Windows): cancelling a network read waits for the peer
      to close or for the read timeout, since `shutdown` doesn't wake it. Stopping
      an account still returns at once and detaches the worker
- ⬜ Threads/conversations view (thread ids are stored; list is flat)

- ✅ Tab / Shift+Tab move between text fields (`ui::FocusNext`/`FocusPrevious`;
      `TextInput`/`TextArea` are tab stops; the command palette traps Tab)
- ✅ Message list rows show Reply and Delete on hover (right edge); Delete keeps
      the selection and is omitted in Trash
- ✅ New-mail notifications carry the app icon on Linux (bundle icon or theme
      name + `desktop-entry` hint)
- ✅ Gmail: new mail met in All Mail before the Inbox is still announced
      (`plan::is_new_arrival`)
- 🔄 Confirm why a Gmail message arrived with no notification on 2026-11-02:
      delivery works and config is on, but the cause wasn't reproduced. Errors go
      only to stderr (`/dev/null` when launched from the desktop); log them if it
      happens again

## Stage 5 — Polish
- ✅ Rename product to **BGMail** (binary, window/menu titles, themes, docs;
      config/db under `~/.config/BGMail/`)
- 🔄 Keyboard shortcuts (command palette + Cmd+Q; per-action bindings still TODO)
- ⬜ Accessibility
- ✅ Native notifications (`notify-rust`, bursts summarized, Settings switch)
- ✅ App icon: italic **BG** with speed trails on the blue envelope
      (`assets/appIcon/BGMail.svg`; PNG 16–1024, `BGMail.ico`, `BGMail.icns`)
- 🔄 Packaging: `cargo-packager` metadata, icons, desktop entry, CI + release
      workflows (GitHub Actions matrix). deb/AppImage/pacman built locally;
      **macOS and Windows jobs never ran yet** (check the first CI run);
      signing/notarization secrets are optional and unset

## Notes / open decisions
- ✅ **Mail stack decided** (two opposing reviews, 2026-10-31):
      sync std threads per account (IDLE + command thread, IDLE cancelled by
      `shutdown()` on a cloned socket) pushing events over `async-channel`;
      `imap` 3.0.0-alpha.15 without default features over our own rustls 0.23
      stream; `lettre` 0.11 as SMTP transport only (`send_raw`);
      `mail-parser` 0.11 (1.0 needs Rust 1.98) + `mail-builder` 2.0; Gmail =
      IMAP/SMTP with XOAUTH2 (hand-written PKCE loopback OAuth on `ureq`),
      deduped by `X-GM-MSGID`; secrets through GPUI's credential API
      (Keychain / Credential Manager / oo7) — so **no `accounts` crate**
      (config in `storage`, OAuth/presets in `protocols`); crates `mail_core`,
      `protocols`, `mail_sync`; storage v2 = `messages` + `message_locations`
      + BLOB bodies + FTS5; **POP3 postponed** (no flags/folders/IDLE; would
      bend the remote traits around the weakest protocol); packaging with
      `cargo-packager` in a GitHub Actions matrix; GreenMail (docker) for
      env-gated integration tests. Risk noted: Google's restricted
      `mail.google.com` scope needs app verification beyond 100 test users.
- ✅ Spike: native HTML engine to replace CEF (`spikes/html_render/REPORT.md`).
      47 e-mails, Chromium as reference. Mean fidelity: **Blitz 7.9/10**
      (31/47 ≥ 8), **litehtml 6.6/10** (15/47 ≥ 8). Blitz bugs found:
      XHTML doctype/`xmlns` sniffing parses mail as XML (workaround: force the
      HTML parser), `<td style="display:block">` content vanishes, `align=center`
      tables not centered. litehtml: `max-width` on `<table>` ignored (breaks
      22/47), litehtml-rs sample painter ignores image scaling.
      With 3 small engine fixes (+48 lines C++: table `max-width`, table
      `align`/`<center>` centering, intrinsic width honoring `max-width`;
      `spikes/html_render/patches/`) litehtml reaches **8.1/10** (35/47 ≥ 8,
      none ≤ 4), on par with Blitz; leftovers are mostly painter-side
      (underlines, generic font fallback) plus no `box-shadow`.
      Blitz with 2 engine fixes (anonymous table cell for non-cell row
      children, HTML `table align`; `patches/blitz-*.patch`, Blitz tests pass)
      reaches **8.2/10** (36/47 ≥ 8, none ≤ 4).
- ✅ Reader engine decided: **Blitz** (pure Rust) for parse/style/layout,
      painted natively by GPUI in a new `crates/html_view` (no webview, no
      child window, no bitmap).
- ✅ `crates/html_view` milestone 1: Blitz-based HTML element for GPUI.
      Builds next to `gpui 0.2.2` (fontique `fontconfig-dlopen`; patched
      `vendor/serde_fmt`); painter vendored from `blitz-paint` records a
      display list replayed with GPUI quads/paths/shadows/images/content masks;
      text drawn word by word with GPUI's `shape_line` at Blitz positions
      (face resolved from Parley's font); radial/sweep/multi-stop gradients
      sampled to cached textures; offline resources (`data:`/`cid:`, remote
      blocked + recorded); `viewer` example. 59 tests.
- ✅ `html_view` fidelity loop vs Chromium (47-mail corpus, `spikes/html_render`,
      captures of the real GPUI window): visual score **8.16 → 9.67/10**
      (41/47 ≥ 9.5, min 8.0); mean SSIM 0.739 → 0.975. Blitz/Taffy now vendored
      (`vendor/blitz`, `vendor/taffy`) with fixes: legacy table/font attributes
      (`cellspacing`/`cellpadding`/`border`/`<font>`/named `bgcolor`), td padding
      UA rule, browser automatic table layout, compressible replaced
      min-content, replaced elements avoiding floats, float/padding height,
      `table align`, anonymous cells, `-apple-system`→system-ui, line height
      quirk ((limited-)quirks docs), line-fit tolerance, inline padding spacers.
      Painter: per-cluster text placement, colour emoji from CBDT/sbix bitmaps
      (GPUI 0.2.2 can't draw them), rounded boxes/clips, dashes, grayscale/
      opacity filters, canvas to viewport bottom, non-finite geometry guard,
      hierarchical default base URL (relative `src` crashed Blitz).
- ✅ `html_view` fidelity round 2: score **9.67 → 9.77** (46/47 ≥ 9.5, min
      9.2), SSIM 0.985. Doctype-less mail renders in standards mode (references
      regenerated the same way); `dir="rtl"` box-only lines mirrored; keyword
      `max-width` (`min-content`/`max-content`); anonymous-cell margin
      collapsing; list markers drawn as Chromium-sized shapes.
- ✅ `html_view` fidelity round 3 (multi-width): validated at 375/600/800/1280
      px; reviewed combinations average **9.75** (57/60 ≥ 9.5), SSIM 0.969 /
      0.982 / 0.989 / 0.994. Browser font policy (one font per generic,
      per-character fallback like Chromium, default `serif`); responsive
      stacking (`display:block` cells share one anonymous cell); horizontal
      scrolling for overflowing mail; collapsed borders counted once; images
      pre-downscaled; rounded clip corners on partial fills; definite image
      heights under `max-width`; inline-block baselines (buttons, tables);
      percent columns summing to 100%; markers on a block item's first line.
- ⬜ `html_view` remaining: weight 800 picks "Arial Black" when Windows fonts
      are installed on Linux (Chromium picks Bold; mailgun/plaid 9.0–9.4);
      quirks-mode strut for text lines (legacy doctypes only; needs Parley);
      ≤5px `<br>`/line-height drifts; `inline-table` with block content in its
      first cell keeps a text baseline (Chromium: bottom).
- ⬜ GPUI 0.2.2 renders CBDT colour emoji as monochrome masks on Linux (whole
      app, not just mail); `html_view` works around it; upstream/upgrade later.
- ✅ **Reader roadmap (agreed order, after the fidelity block):**
  1. ✅ `html_view` milestone 2 — reader integrated and CEF era removed: the
     reader shows bodies through `HtmlView` (sanitized + themed by the new
     `mail_html` module, dark/light page, forced-white option, per-message
     document cache via `ReaderSignature`). Removed `cef_osr.rs`,
     `web_view.rs` (~3.3k lines), the `cef`/`cef-osr`/`linux-webview`
     features and deps (`cef`, `open`, `image`, `smallvec`, `urlencoding`,
     `async-channel`), CEF init/shutdown/sub-process handling, the OSR tick,
     focus, input forwarding and IPC plumbing, the text fallback, and the
     remote-content unblock UI (decided with two opposing reviews: no
     networking before Stage 3; the shield now only reports blocked images,
     Settings ▸ Privacy explains it). Palette overlay, global menu handlers and
     the palette key interceptor kept (still needed without a webview).
  2. ✅ Milestone 3 — interaction: `html_view` drives Blitz's event driver
     (hover, drag selection, link clicks captured, never navigated),
     double/triple click word/paragraph, `hit_info`, select all, find with
     match scrolling, theme selection color; `HtmlView` routes window-level
     mouse events, sets hand/I-beam cursors, emits `HtmlViewEvent`, handles
     copy/select-all/arrow/page/home/end keys. BGMail: status-bar link URL,
     open http/https/mailto in the browser, `ui::ContextMenu` (new, Zed API)
     with open/copy link, copy, select all, and a find bar (Cmd/Ctrl+F,
     Enter/Shift+Enter, Esc). Fixed Blitz multi-root selection order with
     anonymous blocks. Verified in the running app (uinput clicks, wtype).
     Gaps: selections can't start in blank space; find highlights only the
     current match.
  3. ✅ Milestone 4 — robustness: nesting deeper than 256 is refused
     (`html_view::try_parse`; ~2,000 levels overflowed the 8 MB stack) and
     never laid out; HTML over 1 MB shown as plain text with "Show
     formatted"; a crash while rendering a message (panic, stack overflow,
     OOM: all abort) is remembered by a marker file and that message opens
     as plain text next time; plain-text extraction is a linear scanner.
     Windows main thread gets an 8 MB stack. Kept `panic = "abort"` (two
     opposing reviews: unwinding through Blitz/GPUI state is unsound and
     doesn't cover overflow/OOM). Measured: 660 KB mail ~60 ms, 2.6 MB
     ~260 ms (release, UI thread; the Blitz document is not `Send`).
  4. ✅ Real-mail round. The corpus is `spikes/html_render/corpus_real/`
     (git-ignored): 25 newsletters from 2002, mostly in quirks mode, plus 20
     current templates.
     - Body heights against Chromium at 800px: mean deviation 0.51% over 89
       mails, counting the template corpus.
     - Capture 7 SSIM (375/600/800/1280): 0.689 / 0.739 / 0.788 / 0.846, up
       from 0.584 / 0.590 / 0.658 / 0.744 (capture 3).
     - Agent review average: 9.06 (capture 7), up from 8.78 (capture 6) and
       about 7.3 before the round.
     - Fixed:
       - broken-image rendering (alt text keeping the image's font, frames
         only from 18px, floats, spacing, width-only blocks);
       - the quirks-mode line height rule (strut, nested spans, `&nbsp;`,
         lone `<br>`);
       - quirky margins, quirks.css and quirks-mode parsing (`<p><table>`);
       - the rowspan column cursor;
       - spanning cells (fixed columns first, widths not given to their
         first column), short fixed columns, table width/height as
         minimums, capped padded percentage columns, collapsed tables
         without padding;
       - collapsed borders painted per cell;
       - Chromium's inset/outset shading and plain `<hr>` bevel;
       - inline width ignored around blocks;
       - cell `vertical-align` growing lines;
       - empty-inline blocks collapsing margins;
       - `valign`, image `align`/`hspace`, `<body link>`, `<hr>` attributes;
       - inline borders drawn as rings;
       - `<select>` (closed drop-down with its option and ▾) and
         `<input size>` with Chromium's metrics;
       - Windows-1252 C1 characters;
       - `align=justify`;
       - the reader keeping the message's doctype.
     - Next:
       - recapture and re-review;
       - ✅ ham-17: rowspan cells spread their extra height like Chromium
         (rows starting other rowspans first) and colspans overlapping
         rowspan cells keep their HTML column;
       - remaining drift in ham-01, ham-16 and ham-05 (fonts missing here,
         iframe ads, listbox `<select size>`);
       - alt-text boxes: clipping and wrapping match Chromium; only in
         cramped table rows Chromium keeps the alt box atomic (MasterClass
         social icons) where html_view flows it as text;
       - ~~fonts registered under another family~~ decided: keep the
         system's family names; "<Family> Black" is drawn as the family at
         weight 900 (Arial Black filed under Arial gets its black face);
       - CJK/RTL checked with synthetic Chinese, Japanese, Korean, Arabic
         and Hebrew mails: box geometry within 3px of Chromium, shaping
         and bidi order identical. Multi-MB mail timed in milestone 4.
  5. ✅ Zoom, print, animated GIF.
     - ✅ Zoom: page zoom as Blitz's scale factor (text reflows and stays
       sharp); Ctrl/Cmd `+`/`-`/`0` and Ctrl/Cmd + wheel over the reader,
       Chromium's levels (25%–500%), kept across messages; the status bar
       shows it when it isn't 100%. Not persisted between runs yet.
     - ✅ Print (Cmd/Ctrl+P, Message menu, palette): decided with two
       opposing reviews to print through the system browser (native
       dialog, pagination, "Save as PDF"): a light print page with a header
       and the sanitized body, locked down by a CSP (only `data:` images
       and its own nonce'd `print()` script), written to a private temp
       file (0600 in a 0700 dir; stale pages removed on the next print).
       Later: a native "Save as PDF" from the display list if needed.
     - ✅ Animated GIF: Blitz keeps the frames (capped), replay draws the
       current one and the view repaints only when a painted animation's
       next frame is due (GIF `background-image` too). Loop counts are not
       honoured (always loops, like most mail GIFs); APNG/WebP stay still.
- ✅ Remote images on request (user decision, ahead of Stage 3): `html_view`
      fetches `http(s)` only for documents that allow it (4 lazy threads,
      15s timeout, 15 MB cap, no cookies/referrer, 64 MB cache) and
      re-lays out as images arrive; the reader's shield loads them for one
      message, Settings ▸ Privacy ▸ "Always load remote images" everywhere.
      Later: per-image unblock, an optional privacy proxy, a disk cache.
- ✅ Bigger text: reader base font 16px (Chromium's default; was 14px);
      UI text scale from one `ui::UI_FONT_SIZE` (15px, was 14px), all UI
      sizes derived from it. Reader CSS no longer forces `img` to block.
- ✅ Reader review fixes: plain-text fallback no longer panics on multi-byte
      text at entity/tag cuts and skips blocks in linear time; the crash
      guard is set before parsing; protocol-relative (`//host/…`) images load
      as `https:` when remote content is allowed; the reader's paint cache
      drops the previous message's images.
- ✅ Ctrl/Cmd+P prints; the command palette moved to Ctrl/Cmd+Shift+P (as
      in Zed), so the two no longer share a chord.
- ✅ CSS `url(...)` is blanked only in `style` attributes and `<style>`
      contents (lol_html handlers), never in the message's text.
- ⬜ GPUI's sprite atlas still keeps textures of past messages (needs
      `window.drop_image` when the paint cache forgets them); `HtmlDocument::
      layout` re-resolves and walks the tree for overflow on every frame.
- ⬜ Upstream the `vendor/blitz` and `vendor/taffy` fixes (each is marked
      `BGMail`), then drop the vendored copies. Either way: behind a `crates/html_view` API, with
      "Open in browser" as the escape hatch.
- Reassess GPUI crates.io `0.2.2` vs `main` if some API is missing.
- ~~Define the local storage format (SQLite vs files) in Stage 2.~~ **SQLite**
      (`crates/storage`, `mail.db` under `~/.config/BGMail/`).
- Add more UI languages by extending `locale::Language` and the catalog.
