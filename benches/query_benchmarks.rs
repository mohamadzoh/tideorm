//! Query Builder Benchmarks for TideORM
//!
//! These benchmarks measure the performance of various query operations
//! against a PostgreSQL database.
//!
//! Requirements:
//! - PostgreSQL running on localhost:5432
//! - Database: test_tide_orm
//! - User: postgres / Password: postgres
//!
//! Run with: cargo bench --bench query_benchmarks

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::sync::OnceLock;
use tideorm::prelude::*;
mod support;

use support::{for_each_batch, init_postgres_database, runtime, truncate_table};

#[path = "query_benchmarks/advanced_benches.rs"]
mod advanced_benches;
#[path = "query_benchmarks/basic_benches.rs"]
mod basic_benches;

use advanced_benches::*;
use basic_benches::*;

// Database initialization flag
static DB_INITIALIZED: OnceLock<()> = OnceLock::new();

// =============================================================================
// BENCHMARK MODEL
// =============================================================================

#[derive(Model, PartialEq)]
#[tideorm(table = "bench_products")]
pub struct BenchProduct {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub category: String,
    pub price: i32,
    pub stock: i32,
    pub active: bool,
}

// =============================================================================
// SETUP HELPERS
// =============================================================================

fn init_database() {
    init_postgres_database(
        &DB_INITIALIZED,
        &[
            "DROP TABLE IF EXISTS bench_products CASCADE",
            r#"
                CREATE TABLE bench_products (
                    id BIGSERIAL PRIMARY KEY,
                    name VARCHAR(255) NOT NULL,
                    category VARCHAR(100) NOT NULL,
                    price INTEGER NOT NULL,
                    stock INTEGER NOT NULL,
                    active BOOLEAN NOT NULL DEFAULT true
                )
            "#,
            "CREATE INDEX idx_bench_products_category ON bench_products(category)",
            "CREATE INDEX idx_bench_products_price ON bench_products(price)",
            "CREATE INDEX idx_bench_products_active ON bench_products(active)",
        ],
    );
}

fn cleanup_data() {
    truncate_table("bench_products");
}

fn seed_data(count: usize) {
    let rt = runtime();
    let categories = ["Electronics", "Clothing", "Books", "Home", "Sports"];

    for_each_batch(
        count,
        500,
        |global_i| BenchProduct {
            id: 0,
            name: format!("Product {global_i}"),
            category: categories[global_i % categories.len()].to_string(),
            price: 100 + (global_i % 10000) as i32,
            stock: (global_i % 1000) as i32,
            active: global_i % 3 != 0,
        },
        |products| {
            rt.block_on(async {
                BenchProduct::insert_all(products)
                    .await
                    .expect("Batch insert failed");
            });
        },
    );
}

fn setup_benchmark_with_data(count: usize) {
    init_database();
    cleanup_data();
    seed_data(count);
}

// =============================================================================
// BENCHMARKS
// =============================================================================

fn bench_first_query(c: &mut Criterion) {
    let rt = runtime();

    setup_benchmark_with_data(10000);

    let mut group = c.benchmark_group("first_query");
    group.sample_size(30);

    group.bench_function("first_with_where", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchProduct::query()
                    .where_eq("category", "Electronics")
                    .first()
                    .await
                    .expect("Query failed")
            })
        });
    });

    group.bench_function("first_ordered", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchProduct::query()
                    .order_by("price", tideorm::query::Order::Desc)
                    .first()
                    .await
                    .expect("Query failed")
            })
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_simple_where,
    bench_range_queries,
    bench_compound_queries,
    bench_ordering,
    bench_pagination,
    bench_aggregations,
    bench_where_in,
    bench_like_queries,
    bench_first_query,
    bench_subquery,
    bench_raw_expressions,
    bench_bulk_delete,
    bench_combined_features
);
criterion_main!(benches);
