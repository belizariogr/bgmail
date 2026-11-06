<p align="center">
  <img src="assets/appIcon/BGMail.svg" alt="BGMail icon" width="128" height="128">
</p>

<h1 align="center">BGMail</h1>

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

## Install / Build packages

Releases are built by GitHub Actions (`.github/workflows/release.yml`): run the
**Release** workflow by hand (Actions ▸ Release ▸ Run workflow), optionally with a
new version, and it bumps `Cargo.toml`, builds every package, creates the `v*` tag
and publishes the GitHub Release. Pushing a `v*` tag yourself makes a draft
release instead.

| OS | Packages |
|---|---|
| macOS (Apple silicon, Intel) | `BGMail.app` inside a `.dmg` (min. macOS 10.15.7) |
| Linux x86_64 | `.deb`, `.AppImage`, pacman `.tar.gz` + `PKGBUILD` |
| Windows x86_64 | NSIS `-setup.exe` (per-user, no admin) and WiX `.msi` |

Builds are signed (and on macOS notarized) only once the signing secrets are
configured; unsigned, macOS needs right-click ▸ Open the first time (Gatekeeper)
and Windows SmartScreen asks for confirmation. On Linux, account passwords are stored through the Secret
Service, so a keyring (GNOME Keyring, KWallet ≥ 5.97, KeePassXC…) must be
running.

On Linux, these scripts install without a package manager (same layout as
Zed's). Only the system-wide one needs root:

```bash
curl -fL https://github.com/belizariogr/bgmail/releases/latest/download/install.sh | sh  # latest release, into ~/.local
script/install-linux            # build this checkout, into ~/.local
script/install-linux-system     # build this checkout, into /usr (asks for sudo)
script/uninstall.sh             # remove the ~/.local install; asks before deleting mail
script/install-linux-system --uninstall
```

To build packages locally, use [cargo-packager](https://github.com/crabnebula-dev/cargo-packager)
(configured in `[package.metadata.packager]` of `crates/bgmail/Cargo.toml`):

```bash
cargo install cargo-packager --locked
cargo packager --release                     # this OS's default formats
cargo packager --release --formats deb       # just one format
```

Packages are written to `target/release/` (or `target/<triple>/release/` with
`--target`). Linux defaults are deb + AppImage + pacman, macOS app + dmg,
Windows NSIS (`--formats nsis,wix` for the MSI too). On rolling distros whose
libraries use `.relr.dyn` (e.g. Arch), the AppImage step needs `NO_STRIP=1`
(linuxdeploy's bundled `strip` is too old); release AppImages are built on
Ubuntu 22.04 for wider glibc compatibility. Building on Linux needs the GPUI
development packages, e.g. on Debian/Ubuntu:

```bash
sudo apt install pkg-config clang libxkbcommon-dev libxkbcommon-x11-dev \
  libx11-xcb-dev libxcb1-dev libwayland-dev libvulkan1 libfontconfig-dev libfreetype-dev
```

### Google sign-in in local builds

Copy `.env.example` to `.env` and fill in your OAuth client (desktop app type).
`cargo build` and `script/install-linux*` embed it; `.env` is gitignored and an
empty one is created by the first build. Without a client, Gmail is still offered
but sign-in explains that the client isn't configured.

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
