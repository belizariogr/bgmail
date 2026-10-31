# BGMail

A **fast**, **simple** and **elegant** desktop e-mail client, written in Rust on
top of **GPUI** (the GPU-accelerated UI framework from
[Zed](https://github.com/zed-industries/zed)).

> **Status:** visual prototype (mock). The interface — inspired by the macOS mail
> client — is already assembled with sample data, to validate the layout and the
> startup speed. The e-mail logic (IMAP/POP3/Gmail) will be implemented in stages.
> See [`docs/PLANEJAMENTO.md`](docs/PLANEJAMENTO.md).

## Highlights

- **Fast startup**: native binary, no Electron/web runtime.
- **macOS Mail-style layout**: accounts sidebar, message list and reading pane,
  with a unified toolbar and a status bar.
- **Two themes**: light (VSCode *Light Modern*) and dark (VSCode *Dark Modern*),
  switchable at runtime.
- **Multi-language UI**: English (default) and Brazilian Portuguese, switchable
  in the settings.
- **Cross-platform**: Windows, Linux and macOS.

## Running

Requires the Rust toolchain defined in `rust-toolchain.toml` (installed
automatically by `rustup`).

```bash
cargo run -p bgmail        # opens the visual prototype
cargo test --workspace    # runs the tests
cargo clippy --workspace  # lint
```

### HTML reader (native)

Message bodies are rendered **natively inside GPUI** by `crates/html_view`: no
webview, no browser runtime. Blitz (vendored in `vendor/blitz`) parses, styles
and lays out the HTML; the result is painted with GPUI primitives and GPUI's
own text system. Bodies are sanitized first (`lol_html`), remote content is
blocked (loading it on request comes with account sync), and mail wider than
the pane scrolls horizontally. Fidelity against Chromium is tracked in
`spikes/html_render/REPORT.md`.

On Linux the renderer uses fontconfig (loaded at runtime, like GPUI) to pick
fonts the way browsers do.

### macOS: Metal Toolchain (Xcode 26+)

GPUI compiles Metal shaders at build time. On Xcode 26 the Metal Toolchain was
unbundled and must be installed separately:

```bash
xcodebuild -downloadComponent MetalToolchain
```

The project's `.cargo/config.toml` sets `TOOLCHAINS = "com.apple.dt.toolchain.Metal"`
so GPUI's `xcrun -sdk macosx metal` build step finds the installed toolchain.

## Structure

```
crates/
├── theme/   # themes and colors (light/dark)
├── ui/      # reusable visual components (Label, Icon, Button, ListItem…)
├── storage/ # SQLite persistence (mail.db under ~/.config/BGMail/)
└── bgmail/   # binary: window, layout, UI state and localization
```

## Project documentation

- [`docs/PLANEJAMENTO.md`](docs/PLANEJAMENTO.md) — vision, architecture, scope and
  planned features.
- [`AGENTS.md`](AGENTS.md) — development rules and DOX rail for AI agents and humans.
- [`.dox/`](.dox/) — child DOX contracts per subtree (`crates/`, `docs/`, `assets/`, …).
- [`TODO.md`](TODO.md) — living implementation progress.

## License

MIT.
