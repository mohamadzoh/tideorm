# Benchmarking

TideORM uses Criterion benchmarks under `benches/` for both database-backed workloads and pure library workloads.

## Benchmark Groups

The current benchmark targets fall into three categories:

- PostgreSQL-backed benchmarks: `crud_benchmarks`, `query_benchmarks`, and `or_clause_benchmarks`
- Feature-gated library benchmarks: `attachments_translations_benchmarks` and `fulltext_benchmarks`
- Library-only benchmarks: `cache_benchmarks`, `relations_benchmarks`, `stability_benchmarks`, `tokenization_benchmarks`, and `validation_benchmarks`

`cargo bench` runs the default-enabled benchmark targets. Benches with `required-features` are skipped unless you enable the matching features explicitly.

## Prerequisites

For PostgreSQL-backed benches, TideORM uses the same fallback URL as the test suite unless you override it:

```bash
POSTGRESQL_DATABASE_URL=postgres://postgres:postgres@localhost:5432/test_tide_orm
```

The benchmark support layer loads `.env` through `dotenvy`, so a local `.env` file works too.

Criterion can render plots with `plotters` out of the box. Installing `gnuplot` is optional.

## Quick Start

Run the default benchmark set:

```bash
cargo bench
```

Prefer a focused target when you are investigating a bottleneck:

```bash
cargo bench --bench query_benchmarks
cargo bench --bench crud_benchmarks
cargo bench --bench or_clause_benchmarks
```

Run library-only benchmarks that do not require a database:

```bash
cargo bench --bench cache_benchmarks
cargo bench --bench relations_benchmarks
cargo bench --bench stability_benchmarks
cargo bench --bench tokenization_benchmarks
cargo bench --bench validation_benchmarks
```

Run feature-gated benchmarks explicitly:

```bash
cargo bench --bench attachments_translations_benchmarks --features "attachments translations"
cargo bench --bench fulltext_benchmarks --features fulltext
```

Compile all benchmark targets without executing them:

```bash
cargo bench --no-run --features "attachments translations fulltext"
```

## Comparing Bottlenecks

Criterion supports saved baselines. Use them when you are working on a performance change so the comparison stays on the same target and workload.

Save a baseline:

```bash
cargo bench --bench query_benchmarks -- --save-baseline before-change
```

Run the modified code against that baseline:

```bash
cargo bench --bench query_benchmarks -- --baseline before-change
```

The HTML and raw report output lives under `target/criterion/`.

## Workflow Notes

- Benchmark the smallest relevant target first. Avoid running unrelated benches while narrowing a regression.
- Keep the PostgreSQL instance idle when measuring database-backed benches; noisy local activity will swamp small improvements.
- Use compile-only `cargo bench --no-run` checks when changing shared benchmark support or feature wiring.
- If a change affects query shape or SQL generation, pair the benchmark run with the profiling tools described in `docs/profiling.md`.
