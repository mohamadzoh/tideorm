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

- Both manifests declare `rust-version = "1.94"`. Treat that as the supported minimum. The `msrv` CI
  job installs exactly 1.94.0 and runs `cargo check --workspace`, so a post-1.94 API in the library
  or the proc-macro crate fails the build.
- The MSRV is not a free choice — it is the ceiling of the dependency tree, and `resolver = "3"`
  enforces it. `sea-orm` 2.x declares `rust-version = "1.94.0"`, so cargo refuses to build this
  workspace on anything older regardless of what our own code uses. Raising the floor is what
  tracking a sea-orm release costs; lowering it means not tracking one.
- Raising the MSRV touches **five** places, and they must move in the same change. Missing one
  leaves the crate advertising support it does not have:
  1. `rust-version` in `Cargo.toml`
  2. `rust-version` in `tideorm-macros/Cargo.toml`
  3. the `dtolnay/rust-toolchain@<ver>` ref in `.github/workflows/ci.yml` (and the comment above the
     `msrv` job, which repeats the literal)
  4. the `rust-<ver>+` badge at the top of `README.md`
  5. this section of `CONTRIBUTING.md`
- The MSRV covers the published crates only. The `msrv` job intentionally omits `--all-targets`, so
  dev-dependencies (criterion, trybuild, tokio) are free to require a newer toolchain.
- Raising `rust-version` also raises the floor clippy lints against, so lints that were suppressed
  as MSRV-incompatible switch on. Re-run `cargo clippy --workspace --all-targets -- -D warnings`
  after any MSRV change; a bump is not lint-neutral.
- There is deliberately **no `rust-toolchain.toml`**. It would pin every contributor's `cargo` in
  this tree to a single toolchain: pinned to the MSRV you would lint and format with an old
  clippy/rustfmt and drift from the `stable` CI jobs; pinned to `stable` it would not check the
  MSRV at all. Develop on `stable` and let the `msrv` job be the gate.
- Install dependencies with standard Rust tooling; no extra bootstrap script is required.
- Some integration tests require local database access and environment variables loaded from `.env` through `dotenvy`.

### Test database environment variables

| Variable | Read by | Effect |
| --- | --- | --- |
| `TEST_DATABASE_URL` | PostgreSQL test helper | Checked **first**, before `POSTGRESQL_DATABASE_URL` |
| `POSTGRESQL_DATABASE_URL` | PostgreSQL test helper, benchmarks | Fallback for tests; the only variable the benches read |
| `MYSQL_DATABASE_URL` | MySQL test helper | Sets the URL **and enables** the MySQL suites |
| `RUN_MYSQL_TESTS` | MySQL test helper | Enables the MySQL suites on their own |
| `SQLITE_DATABASE_URL` | SQLite test helper | SQLite URL |
| `SKIP_SQLITE_TESTS` | SQLite test helper | Skips the SQLite integration and entity-manager suites |
| `SKIP_POSTGRES_TESTS` | `postgres_entity_manager_tests` only | Does **not** gate `postgres_integration_tests` or `postgres_advanced_tests` |

Default fallbacks when nothing is set:

- PostgreSQL: `postgres://postgres:postgres@localhost:5432/test_tide_orm`
- MySQL: `mysql://root:@localhost:3306/test_tide_orm`
- SQLite: `sqlite://./test_tide_orm.db?mode=rwc`

Gate semantics differ per backend, so read them literally:

- **SQLite** is opt-**out** via `SKIP_SQLITE_TESTS`.
- **MySQL** is opt-**in**: the suites run only when `RUN_MYSQL_TESTS` or `MYSQL_DATABASE_URL` is set.
  There is **no `SKIP_MYSQL_TESTS`** — no code reads it. Because setting `MYSQL_DATABASE_URL` in your
  `.env` is itself the opt-in, the way to turn MySQL tests off is to unset it.
- **PostgreSQL** is always on. `postgres_integration_tests` and `postgres_advanced_tests` connect
  unconditionally and hard-fail without a server; only `postgres_entity_manager_tests` honours
  `SKIP_POSTGRES_TESTS`.

**Run the backend suites one at a time.** They create and drop fixed-name tables in a shared
database and take no isolation between runs, so two of them against the same server will interfere
and fail in ways that look like real defects. `.cargo/config.toml` already pins
`RUST_TEST_THREADS=1`, which serializes tests *within* a target but does nothing across separate
`cargo test --test ..` invocations. This matters for CI: adding a `services:` block does not make
these suites safe to run as parallel jobs against one database instance.

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

## Benchmarking

A bare `cargo bench` is not a usable entry point. Always benchmark a single target.

Benches that need **no database** — safe on any machine:

```bash
cargo bench --bench validation_benchmarks
cargo bench --bench stability_benchmarks
cargo bench --bench relations_benchmarks
cargo bench --bench tokenization_benchmarks
cargo bench --bench attachments_translations_benchmarks --features "attachments translations"
cargo bench --bench fulltext_benchmarks --features fulltext
```

Benches that open a **SQLite** connection (they carry `required-features = ["sqlite", "runtime-tokio"]`,
so they are skipped rather than panicking under the default feature set):

```bash
cargo bench --bench cache_benchmarks --features sqlite
```

Benches that need a **live PostgreSQL server**. They call `.expect()` on connect and abort the run if
one is not reachable, so start a server first:

```bash
cargo bench --bench query_benchmarks
cargo bench --bench crud_benchmarks
cargo bench --bench or_clause_benchmarks
```

The PostgreSQL-backed benches read `POSTGRESQL_DATABASE_URL` only (not `TEST_DATABASE_URL`) and fall back to `postgres://postgres:postgres@localhost:5432/test_tide_orm` when it is unset.

To type-check every bench without running any of them:

```bash
cargo bench --no-run --features "sqlite attachments translations fulltext"
```

For the full benchmark matrix and Criterion baseline workflow, see [docs/benchmarking.md](docs/benchmarking.md).

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
