# AGENTS.md — Rules for AI agents (and humans)

This file guides any AI agent working on **rMail**. Read it in full **before
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
  (`crates/rmail/src/locale.rs`) with English and Brazilian Portuguese; add new
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
rMail/
├── AGENTS.md ← you are here (rules)
├── README.md ← overview and how to run
├── TODO.md ← living progress
├── docs/
│ └── PLANEJAMENTO.md ← vision, architecture and scope
└── crates/
 ├── theme/ ← themes and colors (light/dark)
 ├── ui/ ← reusable visual components
 └── rmail/ ← binary (window + layout + state + localization)
```

## Scope limits in this phase (mock)

- Do **not** implement networking, OAuth, IMAP/SMTP or persistence yet. The
  current phase is only the **visual mock** (see `docs/PLANEJAMENTO.md`, section 2).
- Keep the sample data isolated in `crates/rmail/src/data.rs` so it is easy to
  replace with the real domain layer later.
