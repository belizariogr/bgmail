# AGENTS.md — Rules for AI agents (and humans)

This file guides any AI agent working on **BGMail**. Read it in full **before
writing or modifying code**. In case of conflict, these rules take precedence.
See also [`docs/PLANEJAMENTO.md`](docs/PLANEJAMENTO.md) (vision and architecture)
and [`TODO.md`](TODO.md) (current state).

## Non-negotiable rules

1. **Always follow these rules.** Re-read this file at the start of each work
   session and keep it in mind for every decision.
2. **Rust project using Zed's UI foundation** (GPUI framework + patterns from the
   `ui` crate). Do not introduce another UI framework.
3. **Use Zed as a reference.** The source code lives in `../zed`. Whenever you
   need a component, layout pattern or GPUI API, **consult Zed** before inventing.
   Mirror its names and patterns to ease portability.
4. **Best practices and reuse.** Always prefer reusing existing components and
   functions. Extract abstractions when there is repetition — but **without
   creating bloat**: do not add layers, traits or crates "just in case".
5. **Write tests for EVERY function/action implemented.** No logic lands without
   a test. Run `cargo test --workspace` before considering a task done.
6. **Cross-platform (Windows, Linux, macOS).** All OS-specific code lives behind
   an abstraction (trait + `#[cfg(...)]`) with a single, portable API. Do not leak
   platform details into the UI or the domain.
7. **Minimize `unsafe`.** Goal: **zero** `unsafe` in our code. If it is absolutely
   necessary, isolate it, comment the safety justification, and cover it with
   tests. Justify it in the PR/commit.
8. **Keep `TODO.md` up to date.** When you start a task, mark it in progress; when
   you finish it, mark it done and add the next steps you discovered. `TODO.md` is
   the source of truth for progress.
9. **Performance is a requirement, not a detail.** The app must **start fast**.
   Avoid heavy synchronous work at startup; load data asynchronously and
   incrementally. Measure before optimizing.
10. **Beautiful, elegant yet simple design.** No fluff. Consistent spacing,
    alignment and typographic hierarchy. Use the theme's semantic color roles,
    **never** loose hex colors in components.

## Expected workflow

1. Read `AGENTS.md`, `docs/PLANEJAMENTO.md` and `TODO.md`.
2. Pick/update an item in `TODO.md` and mark it in progress.
3. Consult Zed (`~/dev/zed`) for relevant references.
4. Implement following the project's existing patterns.
5. Write the corresponding tests.
6. Run `cargo fmt`, `cargo clippy --workspace -- -D warnings` and
   `cargo test --workspace`.
7. Update `TODO.md` (and the documentation, if needed).
8. Make small, descriptive commits. **Do not** commit secrets/credentials.

## Code conventions

- **Edition**: Rust 2021, toolchain pinned in `rust-toolchain.toml`.
- **Formatting**: `cargo fmt` (default rustfmt). No unformatted code.
- **Lint**: `cargo clippy` with no *warnings* (`-D warnings`).
- **Names**: English for code identifiers.
- **Language**: English is the default language of the app and the codebase
  (identifiers, comments and UI text). The UI also ships a localization layer
  (`crates/bgmail/src/locale.rs`) with English and Brazilian Portuguese; add new
  user-facing strings as keys there instead of hardcoding them.
- **Comments**: explain the *why* (intent, trade-offs), not the *what*. Do not
  narrate the obvious.
- **Colors**: defined only in `crates/theme`; components use `ui::Color`.
- **Components**: follow the `#[derive(IntoElement)]` + `impl RenderOnce` pattern,
  with chainable *builder methods* (as in Zed's `ui`).
- **UI state**: views are `Entity` with `impl Render`; mutation via
  `cx.listener(...)` + `cx.notify()`.

## Repository structure

```
BGMail/
├── AGENTS.md ← you are here (rules)
├── README.md ← overview and how to run
├── TODO.md ← living progress
├── docs/
│ └── PLANEJAMENTO.md ← vision, architecture and scope
├── assets/ ← shared icons, fonts, app icon
└── crates/
    ├── theme/ ← themes and colors (light/dark)
    ├── ui/ ← reusable visual components
    ├── storage/ ← SQLite persistence (mail.db)
    └── bgmail/ ← binary (window + layout + state + localization)
```

## Scope limits (current)

- **Stage 1 (visual mock)** is largely done; **Stage 2 (domain/storage)** is in
  progress — see `TODO.md`.
- Do **not** implement networking, OAuth, IMAP/POP3/SMTP, or Gmail API yet
  (Stage 3). Keep protocol code out until Stage 2 sync/storage is stable.
- Sample / seed data stays isolated (`crates/bgmail/src/data.rs`,
  `crates/bgmail/src/db_seed.rs`, `storage::seed`) so it can be replaced by the
  real mail layer later.
- Child DOX docs under `.dox/` own crate-local contracts; this file owns
  repo-wide rules.



# DOX framework

- DOX is highly performant AGENTS.md hierarchy installed here
- Agent must follow DOX instructions across any edits

## .dox Storage

Child docs live under `.dox/`, mirroring the scope directory they govern. Source trees stay free of scattered AGENTS.md files.

| Scope (subtree root) | Doc path |
|---|---|
| Repository root | `AGENTS.md` |
| `src/api/` | `.dox/src/api/AGENTS.md` |
| `src/api/handlers/` | `.dox/src/api/handlers/AGENTS.md` |

Rules:

- Only the root rail stays at `AGENTS.md`. Every other doc goes in `.dox/<mirrored-path>/AGENTS.md`.
- `<mirrored-path>` is the scope folder relative to the repo root, without a leading slash.
- Never create `AGENTS.md` beside source files or inside code directories.
- When creating, moving, or deleting a child doc, update the matching path under `.dox/` and keep the mirror aligned with the scope folder.
- Child DOX Index entries use the `.dox/...` path and name the scope folder they cover.

Resolution: for a target at `src/api/foo.ts`, walk `src/`, then `src/api/`, and read `.dox/src/AGENTS.md` and `.dox/src/api/AGENTS.md` when present. The nearest applicable doc is the deepest mirror on that walk.

## Core Contract

- AGENTS.md files are binding work contracts for their subtrees
- Work products, source materials, instructions, records, assets, and durable docs must stay understandable from the nearest applicable AGENTS.md plus every parent AGENTS.md above it

## Read Before Editing

1. Read the root `AGENTS.md`
2. Identify every file or folder you expect to touch, in case it doesnt exists, create it for every file you touch
3. Walk from the repository root to each target path
4. Along each route, read every mirrored doc at `.dox/<path>/AGENTS.md` for directories on that walk
5. If a parent doc lists a child doc whose scope contains the path, read that child at its `.dox/...` path and continue from there
6. Use the nearest applicable doc as the local contract and parent docs for repo-wide rules
7. If docs conflict, the closer doc controls local work details, but no child doc may weaken DOX

Do not rely on memory. Re-read the applicable DOX chain in the current session before editing.

## Update After Editing

Every meaningful change requires a DOX pass before the task is done.

Update the closest owning doc when a change affects:

- purpose, scope, ownership, or responsibilities
- durable structure, contracts, workflows, or operating rules
- required inputs, outputs, permissions, constraints, side effects, or artifacts
- user preferences about behavior, communication, process, organization, or quality
- doc creation, deletion, move, rename, or Child DOX Index contents under `.dox/`

Update parent docs when parent-level structure, ownership, workflow, or child index changes. Update child docs under `.dox/` when parent changes alter local rules. Remove stale or contradictory text immediately. Small edits that do not change behavior or contracts may leave docs unchanged, but the DOX pass still must happen.

If DOX doc doest not exists, you have to create it!!

## Hierarchy

- Root `AGENTS.md` is the DOX rail: project-wide instructions, global preferences, durable workflow rules, and the top-level Child DOX Index
- Child docs live at `.dox/<mirrored-path>/AGENTS.md` and own domain-specific instructions plus their own Child DOX Index
- Each parent explains what its direct children cover and what stays owned by the parent
- The closer a doc is to the work, the more specific and practical it must be

## Child Doc Shape

- Create a child doc at `.dox/<mirrored-path>/AGENTS.md` when a folder becomes a durable boundary with its own purpose, rules, responsibilities, workflow, materials, or quality standards
- Work Guidance must reflect the current standards of the project or user instructions; if there are no specific standards or instructions yet, leave it empty
- Verification must reflect an existing check; if no verification framework exists yet, leave it empty and update it when one exists

Default section order:
- Purpose
- Ownership
- Local Contracts
- Work Guidance
- Verification
- Child DOX Index
- API Catalog _(required for `.dox/crates/*/src/` source scopes: every
  function/method with purpose and behavior; see User Preferences)_

## Style

- Keep docs concise, current, and operational
- Document stable contracts, not diary entries
- Put broad rules in parent docs and concrete details in child docs
- Prefer direct bullets with explicit names
- Do not duplicate rules across many files unless each scope needs a local version
- Delete stale notes instead of explaining history
- Trim obvious statements, repeated rules, misplaced detail, and warnings for risks that no longer exist

## Closeout

1. Re-check changed paths against the DOX chain (resolve child docs via `.dox/` mirror)
2. Update nearest owning docs and any affected parents or children under `.dox/`
3. Refresh every affected Child DOX Index
4. Remove stale or contradictory text and delete orphaned `.dox/` mirrors when scopes are removed
5. Run existing verification when relevant
6. Report any docs intentionally left unchanged and why

## User Preferences

When the user requests a durable behavior change, record it here or in the relevant child doc under `.dox/`

- **Full source API catalogs in DOX:** for every Rust source scope under
  `.dox/crates/*/src/`, document **every** function and method — purpose and
  behavior — in an `## API Catalog` section (grouped by module / `impl`). Keep
  entries current when signatures or behavior change. Prefer rustdoc + observed
  control flow over narration. Tests (`#[cfg(test)]`) are included when they
  encode behavioral contracts.

## Child DOX Index

| Scope | Doc | Owns |
|---|---|---|
| `crates/` | [`.dox/crates/AGENTS.md`](.dox/crates/AGENTS.md) | Cargo workspace crates, dependency direction, crate boundaries |
| `docs/` | [`.dox/docs/AGENTS.md`](.dox/docs/AGENTS.md) | Planning and durable human/agent guidance docs |
| `assets/` | [`.dox/assets/AGENTS.md`](.dox/assets/AGENTS.md) | Shared binary assets (icons, fonts, app icon) |
| `.cargo/` | [`.dox/.cargo/AGENTS.md`](.dox/.cargo/AGENTS.md) | Workspace Cargo/build environment overrides |