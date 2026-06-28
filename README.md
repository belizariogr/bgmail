# rMail

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
cargo run -p rmail        # opens the visual prototype
cargo test --workspace    # runs the tests
cargo clippy --workspace  # lint
```

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
└── rmail/   # binary: window, layout (mock), UI state and localization
```

## Project documentation

- [`docs/PLANEJAMENTO.md`](docs/PLANEJAMENTO.md) — vision, architecture, scope and
  planned features.
- [`AGENTS.md`](AGENTS.md) — development rules for AI agents and humans.
- [`TODO.md`](TODO.md) — living implementation progress.

## License

MIT.
