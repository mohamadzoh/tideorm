//! Cache Benchmarks for TideORM
//!
//! Measures performance of query caching and prepared statement caching.
//!
//! Run with: `cargo bench --bench cache_benchmarks`

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tideorm::Database;
use tideorm::cache::{CacheKeyBuilder, CacheStrategy, PreparedStatementCache, QueryCache};
use tideorm::internal::{ActiveModelTrait, ConnectionTrait, InternalModel};
use tideorm::prelude::*;

#[path = "cache_benchmarks/complex.rs"]
mod complex;
#[path = "cache_benchmarks/key_builder.rs"]
mod key_builder;
#[path = "cache_benchmarks/models.rs"]
mod models;
#[path = "cache_benchmarks/prepared_statement.rs"]
mod prepared_statement;
#[path = "cache_benchmarks/query_cache.rs"]
mod query_cache;

use complex::*;
use key_builder::*;
use models::*;
use prepared_statement::*;
use query_cache::*;

// =============================================================================
// CRITERION GROUPS
// =============================================================================

criterion_group!(
    query_cache_benches,
    benchmark_query_cache_set,
    benchmark_query_cache_get_hit,
    benchmark_query_cache_get_miss,
    benchmark_query_cache_strategies,
    benchmark_query_cache_invalidation,
);

criterion_group!(
    prepared_statement_benches,
    benchmark_prepared_statement_cache_hit,
    benchmark_prepared_statement_record_execution,
);

criterion_group!(key_builder_benches, benchmark_cache_key_builder,);

criterion_group!(
    complex_benches,
    benchmark_realistic_workload,
    benchmark_cache_with_serialization,
    benchmark_end_to_end_query_cache_paths,
    benchmark_uncached_query_concurrency,
);

criterion_main!(
    query_cache_benches,
    prepared_statement_benches,
    key_builder_benches,
    complex_benches,
);
