# Contributing Guidelines

Thank you for contributing to TideORM.

This repository is a Rust workspace with two crates:

- `tideorm` for the runtime ORM API
- `tideorm-macros` for proc-macro parsing and code generation

Before contributing, please read the [Code of Conduct](CODE_OF_CONDUCT.md).

## Before You Start

- Search existing issues and pull requests before opening a new one.
- Keep pull requests focused. Small, targeted changes are easier to review and safer to merge.
- If you are changing public behavior, include tests and update the relevant docs.

## Development Setup

- Rust `1.85+` is required.
- Install dependencies with standard Rust tooling; no extra bootstrap script is required.
- Some integration tests require local database access and environment variables loaded from `.env` through `dotenvy`.

Default test database fallbacks used by the project:

- PostgreSQL: `postgres://postgres:postgres@localhost:5432/test_tide_orm`
- SQLite: `sqlite://./test_tide_orm.db?mode=rwc`

Supported skip flags:

- `SKIP_SQLITE_TESTS`
- `SKIP_MYSQL_TESTS`
- `SKIP_POSTGRES_TESTS`

## Project Layout

- `src/` contains the main TideORM runtime.
- `tideorm-macros/src/` contains the proc-macro crate.
- `tests/` contains integration, compile-fail, and shared support coverage.
- `tests/support/` contains shared backend test helpers and setup utilities.
- `docs/` contains the mdBook source.
- `site/` is generated output and should not be edited directly.

## Coding Guidelines

- Prefer minimal, targeted edits over broad refactors.
- Keep public APIs stable unless the change intentionally introduces a breaking change.
- Preserve feature gating with `#[cfg(feature = "...")]` across code, tests, exports, and docs.
- Do not expose SeaORM internals through TideORM’s public API unless the change explicitly requires it.
- Reuse existing helper layers instead of duplicating SQL generation or backend-specific behavior.
- Do not build SQL with interpolated identifiers or values. Use the shared safety helpers and parameterized paths already used by the project.
- Avoid introducing `unsafe`; the crate denies unsafe code.

## Testing

Use the smallest relevant test command first.

Fast validation:

```bash
cargo test --lib
```

Broad feature coverage:

```bash
cargo test --all-features
```

Default backend test pass:

```bash
cargo test --features postgres
```

Backend-specific integration suites:

```bash
cargo test --test sqlite_integration_tests --features "sqlite runtime-tokio" --no-default-features
cargo test --test postgres_integration_tests
cargo test --test postgres_advanced_tests
cargo test --test mysql_integration_tests --features mysql
```

Proc-macro and compile-fail coverage:

```bash
cargo test --test relation_compile_fail
```

If your change touches docs, also build the book:

```bash
mdbook build
```

## Documentation

- Update `README.md` when quick-start behavior or public-facing features change.
- Update the relevant file under `docs/` when behavior, workflows, or feature guidance changes.
- Do not edit generated files in `site/`; rebuild from `docs/` instead.

## Pull Requests

Before opening a pull request, make sure:

- The change is scoped and clearly described.
- Relevant tests pass locally.
- New behavior has test coverage where practical.
- Documentation is updated if the change affects users or contributors.
- Feature-gated code paths stay consistent across runtime, macros, tests, and docs.

## Questions And Discussion

If you are unsure about a design direction, open an issue or draft pull request with the tradeoffs and proposed approach. Clear technical context makes review substantially faster.

For usage questions or troubleshooting help, see [SUPPORT.md](SUPPORT.md) and use the question/support issue path.
