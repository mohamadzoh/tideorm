# TideORM Workspace Instructions

## Scope

- This workspace is a Rust 2024 workspace with two crates: the main `tideorm` crate and the `tideorm-macros` proc-macro crate.
- Prefer minimal, targeted edits. Do not reformat unrelated Rust code or documentation.
- Keep public APIs consistent unless the task explicitly requires a breaking change.

## Core Commands

- Fast smoke test: `cargo test --lib`
- Broad compatibility pass: `cargo test --all-features`
- Default backend test pass: `cargo test --features postgres`
- SQLite integration suite: `cargo test --test sqlite_integration_tests --features sqlite --no-default-features`
- PostgreSQL integration suite: `cargo test --test postgres_integration_tests`
- Advanced PostgreSQL suite: `cargo test --test postgres_advanced_tests`
- MySQL integration suite: `cargo test --test mysql_integration_tests --features mysql`
- Benchmarks: `cargo bench`
- Docs build: `mdbook build`
- Docs preview: `mdbook serve --open`

## Environment And Prerequisites

- Rust `1.85+` is required.
- Integration tests may require database servers and environment variables loaded from `.env` via `dotenvy`.
- PostgreSQL tests default to `postgres://postgres:postgres@localhost:5432/test_tide_orm` when env vars are absent.
- SQLite tests default to `sqlite://./test_tide_orm.db?mode=rwc` when `SQLITE_DATABASE_URL` is absent.
- Skip flags are supported: `SKIP_SQLITE_TESTS`, `SKIP_MYSQL_TESTS`, `SKIP_POSTGRES_TESTS`.
- Benchmarks expect PostgreSQL access through `POSTGRESQL_DATABASE_URL`.

## Architecture Boundaries

- `src/` contains the public ORM runtime. `tideorm-macros/src/` contains proc-macro parsing and code generation.
- Keep SeaORM as an internal implementation detail. The boundary is centered in `src/internal/`; do not expose SeaORM types in the public API unless the task explicitly requires it.
- `src/model/` contains generated-model runtime support and CRUD internals.
- `src/query/` contains query-builder behavior, SQL generation, and DB-specific query helpers.
- `src/database.rs` implements the global connection pattern used by the rest of the crate.
- Optional modules are feature-gated public API only: `attachments`, `translations`, and `fulltext` do not exist unless their feature is enabled.

## Project Conventions

- Preserve feature gating with `#[cfg(feature = ...)]` on optional modules and tests.
- Keep generated-model behavior aligned with the macros crate and runtime crate together; changes often require touching both crates.
- Prefer existing builder and helper layers over ad hoc SQL or duplicated logic.
- Maintain the global database initialization model built around `TideConfig::init()` rather than introducing alternate connection flows unless explicitly requested.
- The crate denies unsafe code. Avoid introducing `unsafe`.

## Safety-Critical Rules

- Do not build SQL with interpolated identifiers or values. Use the shared safety boundary in `src/query/db_sql.rs`, parameterized SQL, or `Expr::cust_with_values`.
- When quoting identifiers, reuse the shared quoting helpers instead of manual escaping.
- Preserve tokenization semantics: missing encryption configuration should surface as `Err`, while invalid or tampered tokens should remain `Ok(None)`.
- Callback dispatch must happen at concrete macro-generated call sites. Do not move callback execution into generic helpers that erase the concrete model type.

## Testing Guidance

- For quick validation after a library change, start with `cargo test --lib`.
- For feature-gated API changes, run the smallest relevant suite plus `cargo test --all-features` when feasible.
- For proc-macro or compile-fail changes, check the `trybuild` coverage in `tests/relation_compile_fail.rs` and related UI fixtures.
- Many integration tests use shared setup modules under `tests/support/` with `OnceLock` and `.env` loading. Follow those patterns instead of introducing one-off env handling.
- Rust 2024 treats process environment mutation as unsafe. If tests need `std::env::set_var` or `remove_var`, use explicit `unsafe` blocks only when unavoidable and serialize the affected tests.

## Documentation Guidance

- User-facing documentation lives in `docs/` and is published through `mdBook` using `book.toml`.
- When behavior changes, update the relevant mdBook chapter and `README.md` if the change affects quick-start or feature documentation.
- Treat `site/` as generated output; edit `docs/` and rebuild instead of hand-editing generated HTML.

## Useful Reference Files

- `Cargo.toml` for workspace structure, features, benches, and test targets.
- `src/lib.rs` for the public module surface and feature-gated exports.
- `src/database.rs` for connection lifecycle behavior.
- `src/model.rs` and `src/model/` for model runtime internals.
- `src/query.rs` and `src/query/db_sql.rs` for query-builder and SQL safety behavior.
- `src/callbacks.rs` for callback dispatch contracts.
- `src/tokenization.rs` for token encode/decode semantics.
- `tests/support/` for integration-test environment patterns.
- `docs/getting-started.md` for documented local test workflows.

## What To Avoid

- Do not expose SeaORM internals through public TideORM APIs by accident.
- Do not bypass feature gates when adding imports, exports, docs, tests, or benchmarks.
- Do not hand-roll SQL escaping or identifier quoting.
- Do not edit generated site output in `site/` directly.
- Do not replace repo-specific test setup with generic test helpers when the existing support modules already cover the backend.