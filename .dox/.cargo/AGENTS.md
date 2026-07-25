# .cargo/ — Workspace Cargo environment

## Purpose

Cargo configuration that applies to this workspace's builds (toolchains, env
overrides) without scattering platform hacks into crate sources.

## Ownership

- Owns: `.cargo/config.toml` and future workspace Cargo config fragments.
- Does not own: `rust-toolchain.toml` (repo root), crate `Cargo.toml` feature
  flags, CEF download behavior of the `cef` crate.

## Local Contracts

- `TOOLCHAINS=com.apple.dt.toolchain.Metal` so GPUI's Metal shader build finds
  Xcode 26+'s unbundled Metal Toolchain. Harmless/ignored on non-macOS.
- Keep overrides minimal and documented; prefer crate-level features for
  optional runtime deps (e.g. `rmail`'s `cef-osr`).
- Do not commit machine-local absolute paths or secrets here.

## Work Guidance

- When changing build env, note the why in this file and in `README.md` if
  developers must install a component (e.g. MetalToolchain).
- Prefer documenting required host packages in `README.md`, not in silent
  config.

## Verification

- `cargo build -p rmail` succeeds on supported hosts after documented setup.

## Child DOX Index

_(none)_
