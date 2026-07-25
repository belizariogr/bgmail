# docs/ — Planning and durable guidance

## Purpose

Human- and agent-facing durable documentation for vision, architecture, and
product scope. Complements `TODO.md` (living progress) and root `AGENTS.md`
(binding rules).

## Ownership

- Owns: `PLANEJAMENTO.md` and future planning/architecture docs under `docs/`.
- Does not own: day-to-day task checklist (`TODO.md` at repo root), crate-local
  contracts (`.dox/crates/…`), agent DOX rail (root `AGENTS.md`).

## Local Contracts

- `PLANEJAMENTO.md` is the vision/architecture source: stages, crate map, UI
  layout reference, MVP feature list, theme decisions, testing strategy.
- When architecture or stage status changes in code, update the status table and
  recorded decisions here (or note the decision in root/`TODO.md` and reflect
  it here if durable).
- Language: English (same as codebase default).
- Do not duplicate the full agent rule list; link to `AGENTS.md`.

## Work Guidance

- Prefer updating this doc for durable "why" and scope; keep ephemeral progress
  in `TODO.md`.
- New docs under `docs/` get a Child DOX Index entry here when they form a
  distinct durable boundary.

## Verification

- Docs are consistent with `TODO.md` stage and workspace members after
  meaningful architecture changes (manual review; no automated doc test yet).

## Child DOX Index

_(none)_
