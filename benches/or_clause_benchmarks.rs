//! OR Clause Benchmarks for TideORM
//!
//! These benchmarks measure OR-clause query execution against a PostgreSQL
//! database. Building the queries without executing them is benchmarked in
//! `stability_benchmarks`.
//!
//! Requirements:
//! - PostgreSQL running on localhost:5432
//! - Database: test_tide_orm
//! - User: postgres / Password: postgres
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

// `criterion_main!`, run only when a PostgreSQL server is configured.
fn main() {
    if support::postgres_benchmarks_enabled() {
        benches();
        Criterion::default().configure_from_args().final_summary();
    }
}
