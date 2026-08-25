# TideORM Workspace Instructions

## Scope

- This workspace is a Rust 2024 workspace with two crates: the main `tideorm` crate and the `tideorm-macros` proc-macro crate.
- Prefer minimal, targeted edits. Do not reformat unrelated Rust code or documentation.
- Keep public APIs consistent unless the task explicitly requires a breaking change.

## Core Commands

- Fast smoke test: `cargo test --lib`
- Broad unit compatibility pass: `cargo test --all-features --lib`
- Proc-macro pass: `cargo test -p tideorm-macros --lib`
- Default backend test pass: `cargo test --features postgres`
- SQLite integration suite: `cargo test --test sqlite_integration_tests --features "sqlite runtime-tokio" --no-default-features`
- PostgreSQL integration suite: `cargo test --test postgres_integration_tests`
- Advanced PostgreSQL suite: `cargo test --test postgres_advanced_tests`
- MySQL integration suite: `cargo test --test mysql_integration_tests --features mysql`
- Strict lint pass: `cargo clippy --all-targets --all-features -- -D warnings`
- Benchmarks: always target one bench, e.g. `cargo bench --bench validation_benchmarks`. A bare `cargo bench` is not usable: `crud_benchmarks`, `query_benchmarks`, and `or_clause_benchmarks` require a live PostgreSQL server, and `cache_benchmarks` requires the `sqlite` feature.
- Docs build: `mdbook build`
- Docs preview: `mdbook serve --open`

## Environment And Prerequisites

- Rust `1.85+` is the declared MSRV (`rust-version` in both manifests). CI installs `stable` only, so the MSRV is not enforced anywhere; raise `rust-version` in both manifests together if you rely on a newer API.
- Integration tests may require database servers and environment variables loaded from `.env` via `dotenvy`.
- PostgreSQL tests read `TEST_DATABASE_URL` first, then `POSTGRESQL_DATABASE_URL`, then default to `postgres://postgres:postgres@localhost:5432/test_tide_orm`.
- SQLite tests default to `sqlite://./test_tide_orm.db?mode=rwc` when `SQLITE_DATABASE_URL` is absent.
- The three backends have three different gate semantics:
  - SQLite is opt-out via `SKIP_SQLITE_TESTS`.
  - MySQL is opt-in via `RUN_MYSQL_TESTS` or `MYSQL_DATABASE_URL`. There is no `SKIP_MYSQL_TESTS`; no code reads it. Unset `MYSQL_DATABASE_URL` to turn MySQL tests off.
  - PostgreSQL is always on. `SKIP_POSTGRES_TESTS` gates only `postgres_entity_manager_tests`; the main PostgreSQL suites ignore it and hard-fail without a server.
- Benchmarks expect PostgreSQL access through `POSTGRESQL_DATABASE_URL` only (`TEST_DATABASE_URL` is not consulted by benches).

## Architecture Boundaries

- `src/` contains the public ORM runtime. `tideorm-macros/src/` contains proc-macro parsing and code generation.
- Keep SeaORM as an internal implementation detail. The main boundary is centered in `src/internal/`; do not expose SeaORM types in the public API unless the task explicitly requires it.
- `src/database/` contains the global connection pattern, scoped connection overrides, and transaction helpers used by the rest of the crate.
- `src/model/` contains generated-model runtime support, CRUD internals, nested save behavior, and serialization helpers that rebuild runtime relation wrappers after serde rewrites.
- `src/query/` contains query-builder behavior, SQL generation, and DB-specific query helpers. The main SQL seams are `src/query/sql/`, `src/query/db_sql.rs`, and `src/query/structure/`.
- `src/internal/backend.rs` and `src/internal/sql_safety.rs` are shared boundaries for backend-specific statement construction, identifier quoting, and raw-SQL safety.
- `src/sync/schema.rs` contains TideORM schema-sync type mapping and table creation helpers.
- `tideorm-macros/src/parse.rs` and `tideorm-macros/src/parse/` contain proc-macro attribute parsing, validation parsing, and index parsing.
- Optional modules and feature-gated APIs are public-surface only: `attachments`, `translations`, `fulltext`, `entity-manager`, and dirty-tracking-backed model inspection helpers do not exist unless their feature is enabled.

## Project Conventions

- Preserve feature gating with `#[cfg(feature = ...)]` on optional modules, optional APIs, and tests.
- Keep generated-model behavior aligned with the macros crate and runtime crate together; changes often require touching both crates.
- Prefer existing builder and helper layers over ad hoc SQL or duplicated logic.
- Maintain the global database initialization model built around `TideConfig::init()` rather than introducing alternate connection flows unless explicitly requested.
- When a non-test Rust file grows large, prefer extracting focused sibling modules instead of continuing to grow a monolith. Keep non-test helper `mod` declarations near the top of the file.
- Preserve runtime-only relation helper state across serialization-based model overwrites by keeping the `refresh_runtime_relations_from` flow intact.
- The crate denies unsafe code. Avoid introducing `unsafe`.

## Safety-Critical Rules

- Do not build SQL with interpolated identifiers or values. Use the shared safety boundary in `src/query/db_sql.rs`, parameterized SQL, or `Expr::cust_with_values`.
- When quoting identifiers, reuse the shared quoting helpers instead of manual escaping.
- Do not pass user-controlled full-text search input directly into PostgreSQL or SQLite full-text expressions. Reuse the sanitizers in `src/internal/sql_safety.rs`.
- Preserve the explicit-filter bulk-mutation guards in `src/query/sql/execution/mutation_safety.rs`; unfiltered destructive mutations should stay blocked unless the public API explicitly allows them.
- Preserve tokenization semantics: missing encryption configuration should surface as `Err`, while invalid or tampered tokens should remain `Ok(None)`.
- Callback dispatch must happen at concrete macro-generated call sites. Do not move callback execution into generic helpers that erase the concrete model type.

## Testing Guidance

- For quick validation after a library change, start with `cargo test --lib`.
- For feature-gated API changes, run the smallest relevant suite plus `cargo test --all-features --lib` when feasible.
- For proc-macro changes, run `cargo test -p tideorm-macros --lib`.
- Before calling work release-ready, prefer a strict lint pass with `cargo clippy --all-targets --all-features -- -D warnings` when feasible.
- For proc-macro or compile-fail changes, check the `trybuild` coverage in `tests/relation_compile_fail.rs` and related UI fixtures.
- Many integration tests use shared setup modules under `tests/support/` with `OnceLock` and `.env` loading. Follow those patterns instead of introducing one-off env handling.
- Rust 2024 treats process environment mutation as unsafe. If tests need `std::env::set_var` or `remove_var`, use explicit `unsafe` blocks only when unavoidable and serialize the affected tests.

## Release Guidance

- Publish order matters: release `tideorm-macros` before `tideorm`.
- `cargo package -p tideorm-macros --allow-dirty` is a meaningful local pre-publish check for the macros crate.
- `cargo package` or `cargo publish --dry-run` for the main `tideorm` crate resolves packaged dependencies through crates.io, so it will fail until the matching `tideorm-macros` version is already published.
- If you only need to inspect the main crate tarball before publishing the macros crate, `cargo package --allow-dirty --no-verify` is acceptable, but it is not a substitute for the final post-publish dry run.

## Documentation Guidance

- User-facing documentation lives in `docs/` and is published through `mdBook` using `book.toml`.
- When behavior changes, update the relevant mdBook chapter and `README.md` if the change affects quick-start or feature documentation.
- Treat `site/` as generated output; edit `docs/` and rebuild instead of hand-editing generated HTML.

## Useful Reference Files

- `Cargo.toml` for workspace structure, features, benches, and test targets.
- `src/lib.rs` for the public module surface and feature-gated exports.
- `src/database/mod.rs` and `src/database/` for connection lifecycle behavior.
- `src/model/mod.rs`, `src/model/`, and `src/model/serialization.rs` for model runtime internals and serde/relation preservation behavior.
- `src/query/mod.rs`, `src/query/sql/`, `src/query/structure/`, and `src/query/db_sql.rs` for query-builder and SQL safety behavior.
- `src/internal/mod.rs`, `src/internal/backend.rs`, and `src/internal/sql_safety.rs` for TideORM-owned backend and SQL-safety seams.
- `src/callbacks.rs` for callback dispatch contracts.
- `src/tokenization.rs` for token encode/decode semantics.
- `src/sync/schema.rs` for schema-sync type mapping behavior.
- `tideorm-macros/src/parse.rs` and `tideorm-macros/src/parse/` for attribute parsing and proc-macro validation/index logic.
- `tests/support/` for integration-test environment patterns.
- `docs/getting-started.md` for documented local test workflows.

## What To Avoid

- Do not expose SeaORM internals through public TideORM APIs by accident.
- Do not bypass feature gates when adding imports, exports, docs, tests, or benchmarks.
- Do not hand-roll SQL escaping or identifier quoting.
- Do not bypass the runtime relation refresh path in serialization helpers when overwriting models from serde data.
- Do not edit generated site output in `site/` directly.
- Do not replace repo-specific test setup with generic test helpers when the existing support modules already cover the backend.