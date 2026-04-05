//! OR Clause Benchmarks for TideORM
//!
//! These benchmarks measure the performance of OR clause query building
//! and execution against a PostgreSQL database.
//!
//! Requirements:
//! - PostgreSQL running on localhost:5432
//! - Database: test_tide_orm
//! - User: postgres / Password: postgres
//!
//! Run with: cargo bench --bench or_clause_benchmarks

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::sync::OnceLock;
use std::time::Duration;
use tideorm::prelude::*;
mod support;

use support::{init_postgres_database, runtime, truncate_table};

#[path = "or_clause_benchmarks/comparison.rs"]
mod comparison;
#[path = "or_clause_benchmarks/construction.rs"]
mod construction;
#[path = "or_clause_benchmarks/execution.rs"]
mod execution;
#[path = "or_clause_benchmarks/fluent.rs"]
mod fluent;
#[path = "or_clause_benchmarks/models.rs"]
mod models;
#[path = "or_clause_benchmarks/setup.rs"]
mod setup;

use comparison::*;
use construction::*;
use execution::*;
use fluent::*;
use models::*;
use setup::*;

// Database initialization flag
static DB_INITIALIZED: OnceLock<()> = OnceLock::new();

criterion_group!(
    benches,
    bench_or_group_construction,
    bench_query_builder_with_or,
    bench_or_clause_query_execution,
    bench_or_vs_in_comparison,
    bench_or_clause_scaling,
    bench_or_conditions_count,
    bench_fluent_or_branch_construction,
    bench_fluent_or_builder_api,
    bench_fluent_or_execution,
);

criterion_main!(benches);
