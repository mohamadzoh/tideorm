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

use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use serde::{Deserialize, Serialize};
use tideorm::prelude::*;
use tideorm::{TideConfig, Database};
use tokio::runtime::Runtime;
use std::sync::OnceLock;
use std::time::Duration;

fn database_url() -> String {
    let _ = dotenvy::dotenv();
    std::env::var("POSTGRESQL_DATABASE_URL")
        .unwrap()
}

// Global runtime for all benchmarks
static RUNTIME: OnceLock<Runtime> = OnceLock::new();

// Database initialization flag
static DB_INITIALIZED: OnceLock<()> = OnceLock::new();

fn get_runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| Runtime::new().unwrap())
}

// =============================================================================
// BENCHMARK MODEL
// =============================================================================

#[derive(Debug, Clone, Model, Serialize, Deserialize, PartialEq)]
#[tide(table = "bench_products")]
pub struct BenchProduct {
    #[tide(primary_key, auto_increment)]
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
    DB_INITIALIZED.get_or_init(|| {
        get_runtime().block_on(async {
            TideConfig::init()
                .database(&database_url())
                .max_connections(50)
                .min_connections(5)
                .acquire_timeout(Duration::from_secs(30))
                .connect()
                .await
                .expect("Failed to connect to database");
            
            // Create benchmark table
            let _ = Database::execute("DROP TABLE IF EXISTS bench_products CASCADE").await;
            
            Database::execute(r#"
                CREATE TABLE bench_products (
                    id BIGSERIAL PRIMARY KEY,
                    name VARCHAR(255) NOT NULL,
                    category VARCHAR(100) NOT NULL,
                    price INTEGER NOT NULL,
                    stock INTEGER NOT NULL,
                    active BOOLEAN NOT NULL DEFAULT true
                )
            "#).await.expect("Failed to create table");
            
            // Create indexes for query benchmarks
            let _ = Database::execute("CREATE INDEX idx_bench_products_category ON bench_products(category)").await;
            let _ = Database::execute("CREATE INDEX idx_bench_products_price ON bench_products(price)").await;
            let _ = Database::execute("CREATE INDEX idx_bench_products_active ON bench_products(active)").await;
        });
    });
}

fn cleanup_data() {
    get_runtime().block_on(async {
        let _ = Database::execute("TRUNCATE TABLE bench_products RESTART IDENTITY CASCADE").await;
    });
}

fn seed_data(count: usize) {
    let rt = get_runtime();
    let categories = ["Electronics", "Clothing", "Books", "Home", "Sports"];
    
    rt.block_on(async {
        // Insert in batches
        let batch_size = 500;
        let batches = count / batch_size;
        let remainder = count % batch_size;
        
        for batch in 0..batches {
            let products: Vec<BenchProduct> = (0..batch_size)
                .map(|i| {
                    let global_i = batch * batch_size + i;
                    BenchProduct {
                        id: 0,
                        name: format!("Product {global_i}"),
                        category: categories[global_i % categories.len()].to_string(),
                        price: 100 + (global_i % 10000) as i32,
                        stock: (global_i % 1000) as i32,
                        active: global_i % 3 != 0, // ~66% active
                    }
                })
                .collect();
            BenchProduct::insert_all(products).await.expect("Batch insert failed");
        }
        
        if remainder > 0 {
            let products: Vec<BenchProduct> = (0..remainder)
                .map(|i| {
                    let global_i = batches * batch_size + i;
                    BenchProduct {
                        id: 0,
                        name: format!("Product {global_i}"),
                        category: categories[global_i % categories.len()].to_string(),
                        price: 100 + (global_i % 10000) as i32,
                        stock: (global_i % 1000) as i32,
                        active: global_i % 3 != 0,
                    }
                })
                .collect();
            BenchProduct::insert_all(products).await.expect("Batch insert failed");
        }
    });
}

fn setup_benchmark_with_data(count: usize) {
    init_database();
    cleanup_data();
    seed_data(count);
}

// =============================================================================
// BENCHMARKS
// =============================================================================

fn bench_simple_where(c: &mut Criterion) {
    let rt = get_runtime();
    
    let mut group = c.benchmark_group("simple_where");
    
    for data_size in [1000, 10000].iter() {
        // Setup with data
        setup_benchmark_with_data(*data_size);
        
        group.bench_with_input(
            BenchmarkId::new("where_eq_indexed", data_size),
            data_size,
            |b, _| {
                b.iter(|| {
                    rt.block_on(async {
                        BenchProduct::query()
                            .where_eq("category", "Electronics")
                            .get()
                            .await
                            .expect("Query failed")
                    })
                });
            },
        );
        
        group.bench_with_input(
            BenchmarkId::new("where_eq_boolean", data_size),
            data_size,
            |b, _| {
                b.iter(|| {
                    rt.block_on(async {
                        BenchProduct::query()
                            .where_eq("active", true)
                            .get()
                            .await
                            .expect("Query failed")
                    })
                });
            },
        );
    }
    
    group.finish();
}

fn bench_range_queries(c: &mut Criterion) {
    let rt = get_runtime();
    
    // Setup with 10000 records
    setup_benchmark_with_data(10000);
    
    let mut group = c.benchmark_group("range_queries");
    
    group.bench_function("where_gt", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchProduct::query()
                    .where_gt("price", 5000)
                    .get()
                    .await
                    .expect("Query failed")
            })
        });
    });
    
    group.bench_function("where_lt", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchProduct::query()
                    .where_lt("price", 500)
                    .get()
                    .await
                    .expect("Query failed")
            })
        });
    });
    
    group.bench_function("where_between", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchProduct::query()
                    .where_gte("price", 1000)
                    .where_lte("price", 2000)
                    .get()
                    .await
                    .expect("Query failed")
            })
        });
    });
    
    group.finish();
}

fn bench_compound_queries(c: &mut Criterion) {
    let rt = get_runtime();
    
    // Setup with 10000 records
    setup_benchmark_with_data(10000);
    
    let mut group = c.benchmark_group("compound_queries");
    
    group.bench_function("two_conditions", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchProduct::query()
                    .where_eq("category", "Electronics")
                    .where_eq("active", true)
                    .get()
                    .await
                    .expect("Query failed")
            })
        });
    });
    
    group.bench_function("three_conditions", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchProduct::query()
                    .where_eq("category", "Electronics")
                    .where_eq("active", true)
                    .where_gt("price", 1000)
                    .get()
                    .await
                    .expect("Query failed")
            })
        });
    });
    
    group.bench_function("complex_query", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchProduct::query()
                    .where_eq("active", true)
                    .where_gte("price", 500)
                    .where_lte("price", 5000)
                    .where_gt("stock", 100)
                    .get()
                    .await
                    .expect("Query failed")
            })
        });
    });
    
    group.finish();
}

fn bench_ordering(c: &mut Criterion) {
    let rt = get_runtime();
    
    // Setup with 10000 records
    setup_benchmark_with_data(10000);
    
    let mut group = c.benchmark_group("ordering");
    
    group.bench_function("order_by_indexed", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchProduct::query()
                    .order_by("price", Order::Desc)
                    .limit(100)
                    .get()
                    .await
                    .expect("Query failed")
            })
        });
    });
    
    group.bench_function("order_by_non_indexed", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchProduct::query()
                    .order_by("name", Order::Asc)
                    .limit(100)
                    .get()
                    .await
                    .expect("Query failed")
            })
        });
    });
    
    group.finish();
}

fn bench_pagination(c: &mut Criterion) {
    let rt = get_runtime();
    
    // Setup with 10000 records
    setup_benchmark_with_data(10000);
    
    let mut group = c.benchmark_group("pagination");
    
    for page_size in [10, 50, 100].iter() {
        group.throughput(Throughput::Elements(*page_size as u64));
        
        group.bench_with_input(
            BenchmarkId::new("first_page", page_size),
            page_size,
            |b, &size| {
                b.iter(|| {
                    rt.block_on(async {
                        BenchProduct::query()
                            .order_by("id", Order::Asc)
                            .page(1, size as u64)
                            .get()
                            .await
                            .expect("Query failed")
                    })
                });
            },
        );
        
        group.bench_with_input(
            BenchmarkId::new("middle_page", page_size),
            page_size,
            |b, &size| {
                b.iter(|| {
                    rt.block_on(async {
                        BenchProduct::query()
                            .order_by("id", Order::Asc)
                            .page(50, size as u64)
                            .get()
                            .await
                            .expect("Query failed")
                    })
                });
            },
        );
    }
    
    group.finish();
}

fn bench_aggregations(c: &mut Criterion) {
    let rt = get_runtime();
    
    let mut group = c.benchmark_group("aggregations");
    group.sample_size(20);
    
    for data_size in [1000, 10000, 50000].iter() {
        // Setup with data
        setup_benchmark_with_data(*data_size);
        
        group.bench_with_input(
            BenchmarkId::new("count_all", data_size),
            data_size,
            |b, _| {
                b.iter(|| {
                    rt.block_on(async {
                        BenchProduct::count().await.expect("Count failed")
                    })
                });
            },
        );
        
        group.bench_with_input(
            BenchmarkId::new("count_with_condition", data_size),
            data_size,
            |b, _| {
                b.iter(|| {
                    rt.block_on(async {
                        BenchProduct::query()
                            .where_eq("active", true)
                            .count()
                            .await
                            .expect("Count failed")
                    })
                });
            },
        );
    }
    
    group.finish();
}

fn bench_where_in(c: &mut Criterion) {
    let rt = get_runtime();
    
    // Setup with 10000 records
    setup_benchmark_with_data(10000);
    
    let mut group = c.benchmark_group("where_in");
    
    for in_size in [5, 10, 50, 100].iter() {
        let ids: Vec<i64> = (1..=*in_size as i64).collect();
        
        group.bench_with_input(
            BenchmarkId::new("where_in_ids", in_size),
            &ids,
            |b, ids| {
                b.iter(|| {
                    let ids = ids.clone();
                    rt.block_on(async {
                        BenchProduct::query()
                            .where_in("id", ids)
                            .get()
                            .await
                            .expect("Query failed")
                    })
                });
            },
        );
    }
    
    group.finish();
}

fn bench_like_queries(c: &mut Criterion) {
    let rt = get_runtime();
    
    // Setup with 10000 records
    setup_benchmark_with_data(10000);
    
    let mut group = c.benchmark_group("like_queries");
    
    group.bench_function("starts_with", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchProduct::query()
                    .where_like("name", "Product 1%")
                    .get()
                    .await
                    .expect("Query failed")
            })
        });
    });
    
    group.bench_function("contains", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchProduct::query()
                    .where_like("name", "%100%")
                    .get()
                    .await
                    .expect("Query failed")
            })
        });
    });
    
    group.bench_function("ends_with", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchProduct::query()
                    .where_like("name", "%00")
                    .get()
                    .await
                    .expect("Query failed")
            })
        });
    });
    
    group.finish();
}

fn bench_first_query(c: &mut Criterion) {
    let rt = get_runtime();
    
    let mut group = c.benchmark_group("first_query");
    
    for data_size in [1000, 10000].iter() {
        // Setup with data
        setup_benchmark_with_data(*data_size);
        
        group.bench_with_input(
            BenchmarkId::new("first_no_condition", data_size),
            data_size,
            |b, _| {
                b.iter(|| {
                    rt.block_on(async {
                        BenchProduct::query()
                            .first()
                            .await
                            .expect("Query failed")
                    })
                });
            },
        );
        
        group.bench_with_input(
            BenchmarkId::new("first_with_condition", data_size),
            data_size,
            |b, _| {
                b.iter(|| {
                    rt.block_on(async {
                        BenchProduct::query()
                            .where_eq("category", "Electronics")
                            .first()
                            .await
                            .expect("Query failed")
                    })
                });
            },
        );
    }
    
    group.finish();
}

// =============================================================================
// SUBQUERY BENCHMARKS
// =============================================================================

fn bench_subquery(c: &mut Criterion) {
    let rt = get_runtime();
    
    // Setup with 10000 records
    setup_benchmark_with_data(10000);
    
    let mut group = c.benchmark_group("subquery");
    group.sample_size(20);
    
    // Benchmark WHERE IN (subquery)
    group.bench_function("where_in_subquery", |b| {
        b.iter(|| {
            rt.block_on(async {
                // Find products in categories that have high-priced items
                BenchProduct::query()
                    .where_in_subquery("category",
                        BenchProduct::query()
                            .select(vec!["category"])
                            .where_gt("price", 5000)
                    )
                    .get()
                    .await
                    .expect("Query failed")
            })
        });
    });
    
    // Benchmark WHERE NOT IN (subquery)
    group.bench_function("where_not_in_subquery", |b| {
        b.iter(|| {
            rt.block_on(async {
                // Find products NOT in categories with low stock items
                BenchProduct::query()
                    .where_not_in_subquery("category",
                        BenchProduct::query()
                            .select(vec!["category"])
                            .where_lt("stock", 10)
                    )
                    .get()
                    .await
                    .expect("Query failed")
            })
        });
    });
    
    // Compare with equivalent WHERE IN (list)
    group.bench_function("where_in_list_equivalent", |b| {
        b.iter(|| {
            rt.block_on(async {
                // First get the categories, then use them in IN clause
                let categories: Vec<String> = BenchProduct::query()
                    .where_gt("price", 5000)
                    .get()
                    .await
                    .expect("Query failed")
                    .iter()
                    .map(|p| p.category.clone())
                    .collect::<std::collections::HashSet<_>>()
                    .into_iter()
                    .collect();
                
                BenchProduct::query()
                    .where_in("category", categories)
                    .get()
                    .await
                    .expect("Query failed")
            })
        });
    });
    
    group.finish();
}

// =============================================================================
// RAW EXPRESSION BENCHMARKS
// =============================================================================

fn bench_raw_expressions(c: &mut Criterion) {
    let rt = get_runtime();
    
    // Setup with 10000 records
    setup_benchmark_with_data(10000);
    
    let mut group = c.benchmark_group("raw_expressions");
    group.sample_size(20);
    
    // Benchmark raw WHERE clause
    group.bench_function("where_raw_simple", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchProduct::query()
                    .where_raw("price > 1000 AND stock > 50")
                    .get()
                    .await
                    .expect("Query failed")
            })
        });
    });
    
    // Compare with standard builder methods
    group.bench_function("where_builder_equivalent", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchProduct::query()
                    .where_gt("price", 1000)
                    .where_gt("stock", 50)
                    .get()
                    .await
                    .expect("Query failed")
            })
        });
    });
    
    // Benchmark raw SELECT with aggregation
    group.bench_function("select_raw_aggregation", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchProduct::query()
                    .group_by("category")
                    .select_raw("category, COUNT(*) as count, AVG(price) as avg_price")
                    .get_json()
                    .await
                    .expect("Query failed")
            })
        });
    });
    
    // Benchmark complex raw expression
    group.bench_function("where_raw_complex", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchProduct::query()
                    .where_raw("(price * stock) > 10000 AND active = true")
                    .get()
                    .await
                    .expect("Query failed")
            })
        });
    });
    
    group.finish();
}

// =============================================================================
// BULK DELETE BENCHMARKS
// =============================================================================

fn bench_bulk_delete(c: &mut Criterion) {
    let rt = get_runtime();
    
    let mut group = c.benchmark_group("bulk_delete");
    group.sample_size(10); // Fewer samples since we're modifying data
    
    // Benchmark delete with simple condition
    group.bench_function("delete_simple_condition", |b| {
        b.iter_batched(
            || {
                // Setup: seed fresh data before each iteration
                setup_benchmark_with_data(1000);
            },
            |_| {
                rt.block_on(async {
                    // Delete inactive products
                    BenchProduct::query()
                        .where_eq("active", false)
                        .delete()
                        .await
                        .expect("Delete failed")
                })
            },
            criterion::BatchSize::PerIteration,
        );
    });
    
    // Benchmark delete with multiple conditions
    group.bench_function("delete_multiple_conditions", |b| {
        b.iter_batched(
            || {
                setup_benchmark_with_data(1000);
            },
            |_| {
                rt.block_on(async {
                    // Delete low stock inactive products
                    BenchProduct::query()
                        .where_eq("active", false)
                        .where_lt("stock", 100)
                        .delete()
                        .await
                        .expect("Delete failed")
                })
            },
            criterion::BatchSize::PerIteration,
        );
    });
    
    // Benchmark delete with raw condition
    group.bench_function("delete_raw_condition", |b| {
        b.iter_batched(
            || {
                setup_benchmark_with_data(1000);
            },
            |_| {
                rt.block_on(async {
                    // Delete with calculated condition
                    BenchProduct::query()
                        .where_raw("price < 200 AND stock < 50")
                        .delete()
                        .await
                        .expect("Delete failed")
                })
            },
            criterion::BatchSize::PerIteration,
        );
    });
    
    // Benchmark force_delete
    group.bench_function("force_delete", |b| {
        b.iter_batched(
            || {
                setup_benchmark_with_data(1000);
            },
            |_| {
                rt.block_on(async {
                    BenchProduct::query()
                        .where_eq("active", false)
                        .force_delete()
                        .await
                        .expect("Force delete failed")
                })
            },
            criterion::BatchSize::PerIteration,
        );
    });
    
    group.finish();
}

// =============================================================================
// COMBINED FEATURE BENCHMARKS
// =============================================================================

fn bench_combined_features(c: &mut Criterion) {
    let rt = get_runtime();
    
    // Setup with 10000 records
    setup_benchmark_with_data(10000);
    
    let mut group = c.benchmark_group("combined_features");
    group.sample_size(20);
    
    // Subquery + ordering
    group.bench_function("subquery_with_ordering", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchProduct::query()
                    .where_in_subquery("category",
                        BenchProduct::query()
                            .select(vec!["category"])
                            .where_eq("active", true)
                    )
                    .order_by("price", Order::Desc)
                    .limit(50)
                    .get()
                    .await
                    .expect("Query failed")
            })
        });
    });
    
    // Raw expression + pagination
    group.bench_function("raw_with_pagination", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchProduct::query()
                    .where_raw("price > 500 AND active = true")
                    .order_by("price", Order::Asc)
                    .page(5, 20)
                    .get()
                    .await
                    .expect("Query failed")
            })
        });
    });
    
    // Subquery + raw expression + aggregation
    group.bench_function("complex_query", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchProduct::query()
                    .where_raw("stock > 0")
                    .where_in_subquery("category",
                        BenchProduct::query()
                            .select(vec!["category"])
                            .group_by("category")
                            .having("COUNT(*) > 100")
                    )
                    .order_by("price", Order::Desc)
                    .limit(100)
                    .get()
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
    bench_combined_features,
);

criterion_main!(benches);
