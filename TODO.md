# TODO — rMail

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
- ✅ Status bar
- ✅ Real-time light/dark theme switching
- ✅ Zed-style settings screen (General/Accounts/Appearance/Notifications)
- ✅ First workspace build (`cargo build --workspace`), `cargo clippy` with no
      warnings and `cargo test --workspace` passing; app starts without crashing
- ✅ macOS/Xcode 26: unbundled Metal Toolchain — `xcodebuild -downloadComponent
      MetalToolchain` + `.cargo/config.toml` forcing
      `TOOLCHAINS = "com.apple.dt.toolchain.Metal"` (gpui's build script uses
      `xcrun -sdk macosx metal`, which does not find the stub)
- ✅ Icons via the FontAwesome 6 Free font (replacing the Unicode glyphs):
      fonts embedded and registered in `ui::init`, `IconName` maps to style
      (solid/regular via weight) + codepoint; solid/regular resolve to distinct
      fonts (verified at runtime)
- ✅ Localization (i18n): `crates/rmail/src/locale.rs` with `Language`
      (English default + Brazilian Portuguese), a string catalog and an
      `ActiveLanguage` global; UI resolves strings at render time, with a
      language picker in the General settings. English is the default everywhere.
- ✅ Custom `ui::Scrollbar` element (vertical, draggable thumb + track click)
      overlaying the message list, sidebar and reader pane; translucent thumb
      colors added to the theme. Default arrow cursor over the strip. Auto-hide:
      shown while hovering the strip, dragging, or scrolling (incl. mouse wheel),
      and hidden ~250ms after scrolling stops or when the mouse leaves the strip.
      Tested via pure thumb-geometry + scroll-recency functions. Mock expanded
      (18 messages, 5 accounts, long bodies) so all three panels overflow.
- 🔄 **Measure the startup time** with instrumentation (still informal)
- ⬜ Scrollbar fade animation (currently instant show/hide); horizontal axis if needed
- ⬜ UI tests with `gpui::TestAppContext` (after stabilizing the mock)
- ⬜ Functional search field (filters the mock list)
- ⬜ E-mail composition screen/panel (mock)
- ⬜ Column resizing (draggable dividers)

## Stage 2 — Domain layer
- ⬜ `mail_core` crate: `Account`, `Mailbox`, `Message`, `Thread`, `Attachment`
- ⬜ `storage` crate: local persistence (SQLite) + tests
- ⬜ Synchronization state machine

## Stage 3 — Connectivity
- ⬜ `protocols` crate: generic traits (Fetch/Send) + IMAP/POP3/SMTP
- ⬜ Gmail via OAuth2 + API
- ⬜ `accounts` crate: account and credential management (per-platform keychain)

## Stage 4 — Client features (see scope in PLANEJAMENTO.md §6)
- ⬜ Read / mark read-unread
- ⬜ Compose / reply / reply all / forward
- ⬜ Send (SMTP + Gmail)
- ⬜ Move / delete / archive / junk
- ⬜ Star / flag
- ⬜ Attachments (view, download, attach)
- ⬜ Search

## Stage 5 — Polish
- ⬜ Keyboard shortcuts
- ⬜ Accessibility
- ⬜ Native notifications
- ⬜ Packaging (macOS `.app`, Linux, Windows)

## Notes / open decisions
- Reassess GPUI crates.io `0.2.2` vs `main` if some API is missing.
- Define the local storage format (SQLite vs files) in Stage 2.
- Add more UI languages by extending `locale::Language` and the catalog.
