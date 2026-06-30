# rMail Planning

> Guidance document for humans **and AI agents**. Before writing any code, also
> read [`AGENTS.md`](../AGENTS.md) (mandatory rules) and [`TODO.md`](../TODO.md)
> (current implementation state).

## 1. Vision

**rMail** is a **fast** (near-instant startup), **simple** desktop e-mail client
with a **beautiful and elegant design**. It reuses the UI foundation of the
[Zed](https://github.com/zed-industries/zed) editor — the **GPUI** framework and
the component patterns from the `ui` crate — to deliver a native, GPU-accelerated
and lightweight interface.

- **Layout**: like the **macOS** mail client (three columns + unified top toolbar
  + status bar), but built with Zed's elements.
- **Platforms**: Windows, Linux and macOS (GPUI is cross-platform).
- **Language**: Rust.
- **Philosophy**: no fluff, but with impeccable visual finish.

> Local Zed reference: `~/dev/zed`. Consult it whenever you need to understand a
> component, layout pattern or GPUI API.

## 2. Execution strategy (in parts)

1. **Visual mock (current stage).** Build the full interface with static data to
   validate the layout and — most importantly — the **startup speed**. No
   networking, no persistence, no e-mail logic.
2. **Domain layer.** Models (`Account`, `Mailbox`, `Message`, `Thread`), local
   storage and a synchronization state machine.
3. **Connectivity.** Generic IMAP/POP3 + SMTP and then **Gmail** (OAuth2).
4. **E-mail client features.** Read, compose, reply, forward, move, mark, search,
   attachments (full list in section 6).
5. **Polish.** Keyboard shortcuts, accessibility, subtle animations, packaging.

Each stage only starts once the previous one is tested and stable.

## 3. Why GPUI / Zed

- **Fast startup**: native binary, no web/Electron runtime.
- **GPU rendering**: smooth scrolling and resizing.
- **"Tailwind in Rust" style**: `div().flex().px_3().bg(...)` — productive and
  readable.
- **Mature theme system**, which we mirror in a lean `theme` crate.

We depend on `gpui` **published on crates.io** (version `0.2.2`) to keep the
project **reproducible and portable**, rather than depending on the local Zed
checkout. The local crate in `~/dev/zed` is used only as a **read-only reference**.

## 4. Crate architecture

The project is a Cargo *workspace*. Splitting into crates favors reuse, isolated
testing and shorter incremental compile times.

| Crate            | Role                                                                  | Status |
| ---------------- | --------------------------------------------------------------------- | ------ |
| `crates/theme`   | Theme and color definitions (light/dark). Mirrors Zed's `theme`.      | ✅ mock |
| `crates/ui`      | Component library (`Label`, `Icon`, `Button`, `ListItem`…).           | ✅ mock |
| `crates/rmail`   | Binary: window, layout, UI state and localization (currently the mock). | ✅ mock |
| `crates/mail_core` *(future)* | Domain models and business rules, no UI dependency.      | ⬜ |
| `crates/storage` *(future)*   | Local persistence (SQLite via `sqlez`/`rusqlite`).       | ⬜ |
| `crates/protocols` *(future)* | IMAP/POP3/SMTP/Gmail abstractions behind *traits*.       | ⬜ |
| `crates/accounts` *(future)*  | Account and credential management (per-platform keychain). | ⬜ |

**Dependency rule:** the UI depends on the domain, never the other way around.
Domain and protocols do not know about GPUI.

### Cross-platform without *bloat*

- All OS-dependent functionality (keychain, data directories, notifications)
  lives behind a **trait** with per-platform implementations
  (`#[cfg(target_os = ...)]`), exposed through a single API.
- Prefer existing cross-platform crates (`directories`, `keyring`,
  `notify-rust`) over reimplementing. Create a custom abstraction **only** when
  necessary.
- Minimize `unsafe`: ideally **zero** in our code. Any use must be isolated,
  commented with a justification and covered by tests.

## 5. UI layout (reference: macOS Mail)

```
┌──────────────────────────────────────────────────────────────────────┐
│  ⦿⦿⦿   [✍ new] [↻]            [↩][↩↩][↪][🗄][⚑][⌦]   [Theme] [⚙]        │  ← Toolbar (transparent titlebar)
├───────────────┬───────────────────────┬───────────────────────────────┤
│  ACCOUNTS      │  Inbox              ⌕ │  Message subject              │
│  ▾ Personal    │ ● GitHub      09:42 📎│  ◉  Sender                     │
│    ✉ Inbox   7 │   New release v0.200  │     sender@domain.com     today│
│    ✎ Drafts    │   The new version...  │ ───────────────────────────── │
│    ↗ Sent      │ ● Mary        09:05 ★ │  Message body...               │
│    ⚠ Junk    3 │   Meeting...          │                                │
│  ▾ Work        │   ...                 │                                │
├───────────────┴───────────────────────┴───────────────────────────────┤
│  2 accounts · 12 messages                   9 unread · Updated just now │  ← Status bar
└──────────────────────────────────────────────────────────────────────┘
```

- **Column 1 — Sidebar** (`panel_background`, ~240px): accounts and their
  mailboxes (Inbox, Drafts, Sent, Junk, Trash, Archive), with unread counters.
- **Column 2 — Message list** (`surface_background`, ~360px): sender, subject,
  preview, timestamp, unread indicator, star and attachment clip.
- **Column 3 — Reader** (`background`, flexible): header (subject, avatar, sender,
  date) + body.
- **Toolbar** unified with the transparent title bar (macOS style).
- **Settings screen** in the Zed style: navigation on the left + content on the
  right (General, Accounts, Appearance, Notifications).

## 6. E-mail client features (scope)

### MVP (minimum viable)
- [ ] Connect accounts: **IMAP/POP3** (generic) and **Gmail** (OAuth2).
- [ ] List mailboxes/folders per account.
- [ ] List messages in a mailbox (with pagination/scroll).
- [ ] Read a message (sanitized basic text and HTML).
- [ ] Mark as read/unread.
- [ ] Compose, **reply**, **reply all**, **forward**.
- [ ] Send via **SMTP** (and the Gmail API).
- [ ] Move/delete/archive; mark as junk.
- [ ] Star and flag.
- [ ] Attachments: view the list, download, attach when composing.
- [ ] Search (subject/sender/body).
- [ ] **2 themes**: light and dark, with runtime switching.
- [ ] Settings screen.

### Post-MVP (desirable)
- [ ] Multiple accounts with a unified inbox.
- [ ] Grouped threads/conversations.
- [ ] Drafts with auto-save.
- [ ] Native notifications for new e-mail.
- [ ] Configurable keyboard shortcuts.
- [ ] Filters/rules and signatures.
- [ ] Offline mode with synchronization.

## 7. Themes

Two built-in themes, switchable at runtime:

- **Dark** — palette based on
  [`vscode_dark_modern.zed`](https://github.com/kevcamel/vscode_dark_modern.zed)
  (VSCode *Dark Modern*). Key colors: background `#1f1f1f`, surface `#181818`,
  text `#cccccc`, accent `#0078d4`, selection `#04395e`.
- **Light** — palette based on
  [`zed-theme-vscode-light-modern`](https://github.com/XiangpengHao/zed-theme-vscode-light-modern)
  (VSCode *Light Modern*). Key colors: background `#ffffff`, surface `#f8f8f8`,
  text `#3b3b3b`, accent `#005fb8`.

The colors live in `crates/theme/src/theme.rs` (`ThemeColors`). Components never
use literal colors: they use semantic roles via `ui::Color` resolved against the
active theme.

## 8. Testing strategy

- **Every function/action implemented must have tests** (project rule).
- Pure logic (themes, parsing, domain, protocols, localization) → `#[cfg(test)]`
  unit tests.
- Visual/component and UI flows → tests with GPUI's `test-support`
  (`gpui::TestAppContext`) once the logic stage begins.
- Always run: `cargo test --workspace` and `cargo clippy --workspace -- -D warnings`.

## 9. Recorded decisions

- **GPUI via crates.io (`0.2.2`)** instead of path/git to the local checkout —
  for reproducibility. (Reassess if we need APIs only present on `main`.)
- **Icons as embedded SVGs**, abstracted by `IconName`. Each icon resolves to an
  SVG under `assets/icons/` (embedded in `crates/ui/src/assets.rs`) and is rendered
  with `gpui::svg()`, which tints it with the icon's color. SVGs are used instead
  of a glyph font so rendering can't depend on a platform font being matched
  correctly (the FontAwesome font broke across platforms).
- **UI language**: English by default, with a localization layer
  (`crates/rmail/src/locale.rs`) that also ships Brazilian Portuguese and can be
  switched at runtime from the settings.

## 10. How to run

```bash
cargo run -p rmail        # opens the visual mock
cargo test --workspace    # runs the tests
cargo clippy --workspace  # lint
```
