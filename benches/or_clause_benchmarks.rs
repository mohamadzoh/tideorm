//! OR Clause Benchmarks for TideORM
//!
//! These benchmarks measure OR-clause query execution against a PostgreSQL
//! database. Building the queries without executing them is benchmarked in
//! `stability_benchmarks`.
//!
//! Opt-in like the PostgreSQL test suites: set `POSTGRESQL_DATABASE_URL`,
//! `TEST_DATABASE_URL` or `RUN_POSTGRES_TESTS`, or the bench prints a note and
//! exits.
//!
//! Run with: cargo bench --bench or_clause_benchmarks

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group};
use std::sync::OnceLock;
use std::time::Duration;
use tideorm::prelude::*;
mod support;

use support::{init_postgres_database, runtime, truncate_table};

#[path = "or_clause_benchmarks/comparison.rs"]
mod comparison;
#[path = "or_clause_benchmarks/execution.rs"]
mod execution;
#[path = "or_clause_benchmarks/fluent.rs"]
mod fluent;
#[path = "or_clause_benchmarks/models.rs"]
mod models;
#[path = "or_clause_benchmarks/setup.rs"]
mod setup;

use comparison::*;
use execution::*;
use fluent::*;
use models::*;
use setup::*;

static DB_INITIALIZED: OnceLock<()> = OnceLock::new();

criterion_group!(
    benches,
    bench_or_clause_query_execution,
    bench_or_vs_in_comparison,
    bench_or_clause_scaling,
    bench_or_conditions_count,
    bench_fluent_or_execution,
);

support::postgres_bench_main!();
