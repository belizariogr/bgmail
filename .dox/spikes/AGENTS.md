# spikes/ — Throwaway technical evaluations

## Purpose

Time-boxed experiments that inform an architecture decision (e.g. choosing an
HTML engine). Their code is evidence, not product: nothing here ships.

## Ownership

- Owns: one folder per spike (`spikes/<topic>/`) with its harness, inputs,
  findings report and README.
- Does not own: product code (`crates/`), durable decisions (record those in
  `TODO.md` / `docs/PLANEJAMENTO.md` once taken).

## Local Contracts

- Every spike is self-contained: its own Cargo workspace (`[workspace]` in its
  manifest) or Node package, never a member of the root workspace, so it can't
  slow down or break `cargo build --workspace`.
- Each spike has a `README.md` (question, method, caveats, how to reproduce) and
  a findings report; generated artifacts and downloads are git-ignored.
- AGENTS.md rules on tests, `unsafe` and API catalogs do not apply to spike
  harness code; they apply as soon as anything is promoted into `crates/`.

## Work Guidance

- Current spikes:
  - `html_render/`: e-mail render fidelity of Blitz and litehtml against
    headless Chromium on 47 e-mails, then the capture/diff/review loop that
    drives `crates/html_view` fidelity at 375/600/800/1280px
    (`REPORT.md`, `README.md`).

## Verification

_(none: spikes are verified by their own README reproduction steps)_

## Child DOX Index

_(none)_
