# crates/storage/src/ — Source API

## Purpose

Source for the `storage` crate: local SQLite schema, queries, folder conventions, message mutations, search helpers, and seed import.

## Ownership

- Owns: All modules under `crates/storage/src/`.
- Does not own: crate-level dependency/feature policy (parent `.dox/crates/storage/AGENTS.md`).

## Local Contracts

- Every Rust source file under this tree has a matching
  `.dox/crates/storage/src/<file>.rs.dox` with its **API Catalog**.
- Update the per-file `.rs.dox` when changing that file's signatures or
  semantics. Prefer rustdoc alignment on public items.
- Visibility in entries (`pub` / `private`) reflects the source item.
- Do not dump sibling-module APIs into this folder doc; keep catalogs in the
  matching `.rs.dox`.

## Work Guidance

- After adding/removing/renaming a source file, create/delete/rename the
  matching `.rs.dox` and refresh this Child DOX Index in the same change.
- After changing a function/type, update that file's `.rs.dox` API Catalog.
- Do not weaken parent DOX contracts from root/`crates/`/`storage/`.

## Verification

- `cargo test -p storage`
- Workspace `fmt` / `clippy -D warnings` as required by root `AGENTS.md`.

## Child DOX Index

| Scope | Doc | Owns |
|---|---|---|
| `crates/storage/src/database.rs` | [`database.rs.dox`](database.rs.dox) | SQLite open/migrate and query API on `Database` |
| `crates/storage/src/folder.rs` | [`folder.rs.dox`](folder.rs.dox) | Folder path conventions and membership CSV helpers |
| `crates/storage/src/lib.rs` | [`lib.rs.dox`](lib.rs.dox) | Crate root re-exports |
| `crates/storage/src/message_ops.rs` | [`message_ops.rs.dox`](message_ops.rs.dox) | Message move/star/trash/archive mutations |
| `crates/storage/src/schema.rs` | [`schema.rs.dox`](schema.rs.dox) | SQL DDL and schema version constants |
| `crates/storage/src/search.rs` | [`search.rs.dox`](search.rs.dox) | Accent-insensitive search text folding and LIKE patterns |
| `crates/storage/src/seed.rs` | [`seed.rs.dox`](seed.rs.dox) | First-run seed import helpers |
| `crates/storage/src/types.rs` | [`types.rs.dox`](types.rs.dox) | UI-agnostic storage data types and list query enum |

