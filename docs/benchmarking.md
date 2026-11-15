# Benchmarking

TideORM's own performance is measured with [Criterion](https://github.com/bheisler/criterion.rs) benchmarks under `benches/` in the `tideorm` repository. This page is for working on TideORM; to measure your application's queries, use [Logging and Profiling](profiling.md).

Benchmark one target at a time. A bare `cargo bench` runs every target the default features allow and takes long, and the ones that need other features or a server are skipped.

## Benchmark Targets

| Target | Measures | Needs |
|---|---|---|
| `validation_benchmarks` | Validation rules | Nothing |
| `stability_benchmarks` | Query building and `debug()` rendering | Nothing |
| `relations_benchmarks` | Eager-load path parsing and relation trees | Nothing |
| `tokenization_benchmarks` | Token encoding and decoding | Nothing |
| `attachments_translations_benchmarks` | File attachment and translation data handling | `--features "attachments translations"` |
| `fulltext_benchmarks` | Full-text index and query generation for each backend | `--features fulltext` |
| `cache_benchmarks` | The query cache under realistic workloads, on SQLite | `--features sqlite` |
| `crud_benchmarks` | Inserts, finds, updates and deletes | A PostgreSQL server |
| `query_benchmarks` | Query-builder reads and subqueries | A PostgreSQL server |
| `or_clause_benchmarks` | `OR` groups against `IN` lists | A PostgreSQL server |

A target whose features are not enabled is skipped. The three PostgreSQL targets need the default features (`postgres` and `runtime-tokio`).

## Running a Benchmark

Targets that need no database:

```bash
cargo bench --bench validation_benchmarks
cargo bench --bench stability_benchmarks
cargo bench --bench relations_benchmarks
cargo bench --bench tokenization_benchmarks
cargo bench --bench attachments_translations_benchmarks --features "attachments translations"
cargo bench --bench fulltext_benchmarks --features fulltext
```

The cache benchmarks open SQLite databases, in memory and in a file under `target/`:

```bash
cargo bench --bench cache_benchmarks --features sqlite
```

The PostgreSQL targets:

```bash
cargo bench --bench crud_benchmarks
cargo bench --bench query_benchmarks
cargo bench --bench or_clause_benchmarks
```

To type-check every target without running any:

```bash
cargo bench --no-run --features "sqlite attachments translations fulltext"
```

## PostgreSQL Benchmarks

The PostgreSQL targets are opt-in, like the PostgreSQL test suites. They run when `TEST_DATABASE_URL`, `POSTGRESQL_DATABASE_URL` or `RUN_POSTGRES_TESTS` is set, and otherwise print `Skipping PostgreSQL benchmarks: set POSTGRESQL_DATABASE_URL, TEST_DATABASE_URL or RUN_POSTGRES_TESTS` and measure nothing. `SKIP_POSTGRES_TESTS` turns them off again.

```bash
POSTGRESQL_DATABASE_URL=postgres://postgres:postgres@localhost:5432/test_tide_orm \
    cargo bench --bench query_benchmarks
```

The URL is `TEST_DATABASE_URL`, then `POSTGRESQL_DATABASE_URL`, then `postgres://postgres:postgres@localhost:5432/test_tide_orm` when only `RUN_POSTGRES_TESTS` is set. A `.env` file in the repository is read too. Once enabled, a benchmark aborts the run if it cannot reach the server.

The benchmarks create their own tables. Keep the server otherwise idle while measuring: other activity swamps small improvements.

## Comparing Against a Baseline

Criterion keeps saved baselines, so a performance change can be measured against the code before it on the same target and workload. Save a baseline before the change:

```bash
cargo bench --bench query_benchmarks -- --save-baseline before-change
```

Then run the changed code against it:

```bash
cargo bench --bench query_benchmarks -- --baseline before-change
```

The HTML and raw reports are under `target/criterion/`. Criterion draws its plots with `plotters`; installing `gnuplot` is optional.

## Workflow Notes

- Benchmark the smallest relevant target first, and avoid running unrelated targets while narrowing a regression.
- Use `cargo bench --no-run` when changing shared benchmark support or feature wiring.
- When a change affects query shape or SQL generation, pair the benchmark with `debug()` and the profiling tools in [Logging and Profiling](profiling.md).
