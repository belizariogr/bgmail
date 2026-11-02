# .github/ — GitHub Actions workflows

## Purpose

Continuous integration on the three desktop platforms and release packaging
(installable macOS/Linux/Windows packages built by cargo-packager).

## Ownership

- Owns: `.github/workflows/ci.yml`, `.github/workflows/release.yml`, and any
  future GitHub config (issue templates, dependabot).
- Does not own: the packaging config itself (`[package.metadata.packager]` in
  `crates/bgmail/Cargo.toml`, `crates/bgmail/packaging/`, `build.rs` — see
  `.dox/crates/bgmail/AGENTS.md`), app icons (`.dox/assets/AGENTS.md`), Cargo
  build env (`.dox/.cargo/AGENTS.md`).

## Local Contracts

- **`ci.yml`** (push to any branch, pull requests, manual):
  - `linux` (ubuntu-22.04): installs GPUI build deps via apt (xkbcommon,
    x11-xcb/xcb, wayland, fontconfig/freetype, Vulkan loader, clang,
    pkg-config), then `cargo fmt --all --check`,
    `cargo clippy --workspace --all-targets -- -D warnings`,
    `cargo test --workspace --no-fail-fast` (one failing crate must not hide the others on platforms nobody runs locally).
  - The Linux job also runs `script/test-install`.
  - `macos` (macos-15) and `windows` (windows-latest): `cargo build -p bgmail`
    + `cargo test --workspace --no-fail-fast`.
  - `greenmail` (ubuntu-22.04): GreenMail `greenmail/standalone:2.1.15`
    service container with the users/ports of
    `crates/protocols/tests/greenmail.sh`; waits for the IMAP greeting, then
    runs the ignored `protocols --test greenmail` and
    `mail_sync --test greenmail_e2e` tests with `BGMAIL_GREENMAIL=1`.
  - Reads no secrets.
- **`release.yml`** (tags `v*`, manual):
  Also packages (without publishing) on pull requests touching packaging files; `workflow_dispatch` works only once the file is on the default branch.
  - macOS packaging retries up to 3 times: `hdiutil` fails now and then on runners ("Resource busy").
  - `package` matrix: linux-x86_64 (ubuntu-22.04: deb, AppImage, pacman),
    macos-aarch64 and macos-x86_64 (both macos-15; x86_64 cross-compiled via
    `CARGO_BUILD_TARGET` + `--target`: app, dmg), windows-x86_64 (NSIS, WiX
    msi). Installs `cargo-packager` (pinned `CARGO_PACKAGER_VERSION`), builds
    the release binary before any secret is in the environment, runs
    `cargo packager --release --formats …`, uploads the packages as
    `BGMail-<name>` artifacts.
  - `release` (tags only): downloads all artifacts and creates a **draft**
    GitHub Release (`softprops/action-gh-release`, generated notes). Needs
    `contents: write`; everything else is read-only.
  - Signing is optional and skipped when secrets are absent (unsigned builds
    still succeed). Secrets are only passed to the step that needs them:
    - macOS signing: `APPLE_CERTIFICATE` (base64 .p12),
      `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY` (e.g.
      `Developer ID Application: … (TEAMID)`; injected into the packager
      config at build time, never committed).
    - macOS notarization (needs signing): `APPLE_ID`, `APPLE_PASSWORD`
      (app-specific password), `APPLE_TEAM_ID`.
    - Windows Authenticode: `WINDOWS_CERTIFICATE` (base64 .pfx),
      `WINDOWS_CERTIFICATE_PASSWORD`; imported into the runner's user store and
      its thumbprint injected as `certificate-thumbprint`.
- Toolchain comes from `rust-toolchain.toml` (`rustup toolchain install`);
  caches via `Swatinem/rust-cache@v2`.
- macOS jobs set `TOOLCHAINS=com.apple.dt.toolchain.XcodeDefault` (runner
  Xcode bundles Metal; overrides `.cargo/config.toml`). Release macOS builds
  use `MACOSX_DEPLOYMENT_TARGET=10.15.7` (= packager `minimum-system-version`).
- Never commit secrets or certificate files; never print secret values.

## Work Guidance

- Keep the apt package list in sync between `ci.yml` and `release.yml`.
- New package formats go in the matrix `formats` and the upload globs.
- When bumping cargo-packager, re-check config keys and the desktop template
  variables (`crates/bgmail/packaging/linux/BGMail.desktop`).

## Verification

- `python3 -c 'import yaml; [yaml.safe_load(open(f)) for f in [".github/workflows/ci.yml", ".github/workflows/release.yml"]]'`
  (or `actionlint` when available).
- Locally mirror the Linux packaging job: `cargo packager --release`.

## Child DOX Index

_(none)_
