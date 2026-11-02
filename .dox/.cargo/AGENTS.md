# .cargo/ — Workspace Cargo environment

## Purpose

Cargo configuration that applies to this workspace's builds (toolchains, env
overrides) without scattering platform hacks into crate sources.

## Ownership

- Owns: `.cargo/config.toml` and future workspace Cargo config fragments.
- Does not own: `rust-toolchain.toml` (repo root), crate `Cargo.toml` feature
  flags.

## Local Contracts

- `TOOLCHAINS=com.apple.dt.toolchain.Metal` so GPUI's Metal shader build finds
  Xcode 26+'s unbundled Metal Toolchain. Harmless/ignored on non-macOS.
  Cargo `[env]` does not override an existing variable: CI macOS jobs export
  `TOOLCHAINS=com.apple.dt.toolchain.XcodeDefault` (runner Xcode bundles
  Metal; see `.github/workflows/`).
- Windows binaries reserve an 8 MB main-thread stack (`/STACK` on MSVC,
  `--stack` on GNU), matching Linux/macOS: the reader lays out mail on the UI
  thread and `html_view::MAX_NESTING_DEPTH` assumes that stack.
- Keep overrides minimal and documented; prefer crate-level features for
  optional runtime deps.
- Do not commit machine-local absolute paths or secrets here.

## Work Guidance

- When changing build env, note the why in this file and in `README.md` if
  developers must install a component (e.g. MetalToolchain).
- Prefer documenting required host packages in `README.md`, not in silent
  config.

## Verification

- `cargo build -p bgmail` succeeds on supported hosts after documented setup.

## Child DOX Index

_(none)_
