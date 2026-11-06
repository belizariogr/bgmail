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
- **`release.yml`** (manual, tags `v*`; packages without publishing on pull
  requests touching packaging files). `workflow_dispatch` works only once the
  file is on the default branch.
  - `prepare` (ubuntu, `contents: write`) decides the commit, tag and release
    kind (outputs `sha`, `tag`, `publish`, `draft`, `prerelease`):
    - Manual run with `publish` (default on): tag `v<version>`. `version` is
      the `version` input (leading `v` dropped) or, when blank, the
      `[workspace.package]` version in `Cargo.toml` bumped by the `bump` input
      (`patch` default, `minor`, `major`; a prerelease's patch bump is its final
      version, `1.0.0-beta.1` → `1.0.0`). Fails on anything but `X.Y.Z[-pre]`,
      on an existing tag, or when run from a tag instead of a branch. A version
      other than `Cargo.toml`'s is committed first (`Cargo.toml` + `Cargo.lock` via
      `cargo metadata`, as `github-actions[bot]`, "Release vX.Y.Z") and pushed to
      the dispatched branch, so the packages carry it; that push triggers no
      other workflow (GITHUB_TOKEN). `draft` input (default off).
    - Manual run without `publish`, pull requests: build only.
    - Pushed `v*` tag: publishes a **draft** for that tag.
    - A `-` in the tag makes it a prerelease.
  - `package` matrix (checks out `prepare.sha`): linux-x86_64 (ubuntu-22.04:
    deb, AppImage, pacman), macos-aarch64 and macos-x86_64 (both macos-15;
    x86_64 cross-compiled via `CARGO_BUILD_TARGET` + `--target`: app, dmg),
    windows-x86_64 (NSIS, WiX msi). Installs `cargo-packager` (pinned
    `CARGO_PACKAGER_VERSION`), builds the release binary before any secret is
    in the environment, runs `cargo packager --release --formats …`, uploads the
    packages as `BGMail-<name>` artifacts. macOS packaging retries up to 3 times
    (`hdiutil` fails now and then on runners: "Resource busy").
  - `release` (when `prepare.publish`): downloads all artifacts, creates the tag
    at `prepare.sha` through the API if missing (only after every package
    built; a draft would otherwise leave it uncreated), then the GitHub Release
    (`softprops/action-gh-release`, generated notes, `draft`/`prerelease` from
    `prepare`).
  - One run per event and ref at a time (`concurrency`; pull requests cancel the
    older run).
  - Signing is optional and skipped when secrets are absent (unsigned builds
    still succeed). Secrets are only passed to the step that needs them:
    - macOS signing: `APPLE_CERTIFICATE` (base64 .p12 of a **Developer ID
      Application** certificate + key), `APPLE_CERTIFICATE_PASSWORD`,
      `APPLE_SIGNING_IDENTITY` (e.g. `Developer ID Application: … (TEAMID)`;
      injected into the packager config at build time, never committed).
    - macOS notarization (needs signing), either `APPLE_ID` + `APPLE_PASSWORD`
      (app-specific password) + `APPLE_TEAM_ID` (wins when both are set), or an
      App Store Connect API key: `APPLE_API_KEY` (key id), `APPLE_API_ISSUER`,
      `APPLE_API_PRIVATE_KEY` (the .p8 text, written to `$RUNNER_TEMP` for the
      step only). cargo-packager notarizes and staples the `.app`; the workflow
      then notarizes (`notarytool submit --wait`, fails unless `Accepted`,
      prints the log) and staples each `.dmg`, which the packager only signs.
      Signed without credentials: warning, not failure.
    - Windows Authenticode: `WINDOWS_CERTIFICATE` (base64 .pfx),
      `WINDOWS_CERTIFICATE_PASSWORD`; imported into the runner's user store and
      its thumbprint injected as `certificate-thumbprint`. CAs no longer issue
      exportable code-signing keys, so a real certificate needs a cloud signer
      (SignPath, Certum SimplySign) in place of this step.
- Toolchain comes from `rust-toolchain.toml` (`rustup toolchain install`);
  caches via `Swatinem/rust-cache@v2`.
- macOS jobs set `TOOLCHAINS=com.apple.dt.toolchain.XcodeDefault` (runner
  Xcode bundles Metal; overrides `.cargo/config.toml`). Release macOS builds
  use `MACOSX_DEPLOYMENT_TARGET=10.15.7` (= packager `minimum-system-version`).
- Never commit secrets or certificate files; never print secret values.
- `BGMAIL_GOOGLE_CLIENT_ID` / `_SECRET` (repository secrets) go only to the steps
  that compile the app (binary, packager, Linux bundle), and are blank on pull
  requests, so PR artifacts have no Google sign-in. Release binaries embed them.

## Work Guidance

- Keep the apt package list in sync between `ci.yml` and `release.yml`.
- New package formats go in the matrix `formats` and the upload globs.
- When bumping cargo-packager, re-check config keys and the desktop template
  variables (`crates/bgmail/packaging/linux/BGMail.desktop`).

## Verification

- `actionlint .github/workflows/*.yml` (release binary from
  github.com/rhysd/actionlint), or at least
  `python3 -c 'import yaml; [yaml.safe_load(open(f)) for f in [".github/workflows/ci.yml", ".github/workflows/release.yml"]]'`.
- Locally mirror the Linux packaging job: `cargo packager --release`.

## Child DOX Index

_(none)_
