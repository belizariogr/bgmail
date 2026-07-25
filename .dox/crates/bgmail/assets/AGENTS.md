# crates/rmail/assets/ — App-bundled sample content

## Purpose

Fixture HTML/text messages and images used by the mock/seed path and CEF reader
tests — not product branding assets.

## Ownership

- Owns: sample bodies under `emails/`, embedded fixtures such as
  `tweezers.png`.
- Does not own: toolbar/UI icons (`assets/icons/`), app icon (`assets/appIcon/`).

## Local Contracts

- Content is mock/demo only; safe to replace when real sync lands.
- Prefer self-contained samples (inline `data:` images where needed) so the
  reader works offline.
- Do not put secrets or real account data here.

## Work Guidance

- Keep filenames descriptive; wire new samples through `data` / `db_seed` with
  tests when behavior depends on them.

## Verification

- Existing rmail tests that assert fixture magic bytes / document assembly.

## Child DOX Index

_(none)_
