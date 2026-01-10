//! CRUD Operation Benchmarks for TideORM
//!
//! These benchmarks measure the performance of basic CRUD operations
//! against a PostgreSQL database.
//!
//! Requirements:
//! - PostgreSQL running on localhost:5432
//! - Database: test_tide_orm
//! - User: postgres / Password: postgres
//!
//! Run with: cargo bench --bench crud_benchmarks

use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use tideorm::prelude::*;
use tideorm::{TideConfig, Database};
use tokio::runtime::Runtime;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

fn database_url() -> String {
    let _ = dotenvy::dotenv();
    std::env::var("POSTGRESQL_DATABASE_URL")
        .unwrap()
}

// Atomic counter for generating unique values
static COUNTER: AtomicU64 = AtomicU64::new(0);

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

#[derive(Model, PartialEq)]
#[tide(table = "bench_users")]
pub struct BenchUser {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,
    pub age: i32,
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
            let _ = Database::execute("DROP TABLE IF EXISTS bench_users CASCADE").await;
            
            Database::execute(r#"
                CREATE TABLE bench_users (
                    id BIGSERIAL PRIMARY KEY,
                    email VARCHAR(255) NOT NULL,
                    name VARCHAR(255) NOT NULL,
                    age INTEGER NOT NULL,
                    active BOOLEAN NOT NULL DEFAULT true
                )
            "#).await.expect("Failed to create table");
        });
    });
}

fn cleanup_data() {
    get_runtime().block_on(async {
        let _ = Database::execute("TRUNCATE TABLE bench_users RESTART IDENTITY CASCADE").await;
    });
}

fn setup_benchmark() {
    init_database();
    cleanup_data();
}

// =============================================================================
// BENCHMARKS
// =============================================================================

fn bench_single_insert(c: &mut Criterion) {
    setup_benchmark();
    let rt = get_runtime();
    
    let mut group = c.benchmark_group("single_insert");
    group.throughput(Throughput::Elements(1));
    
    group.bench_function("insert_one_user", |b| {
        b.iter(|| {
            let unique_id = COUNTER.fetch_add(1, Ordering::SeqCst);
            rt.block_on(async {
                let user = BenchUser {
                    id: 0,
                    email: format!("bench_{unique_id}@example.com"),
                    name: "Benchmark User".to_string(),
                    age: 25,
                    active: true,
                };
                user.save().await.expect("Insert failed")
            })
        });
    });
    
    group.finish();
}

fn bench_batch_insert(c: &mut Criterion) {
    setup_benchmark();
    let rt = get_runtime();
    
    let mut group = c.benchmark_group("batch_insert");
    group.sample_size(20); // Reduced sample size for batch operations
    
    for size in [10, 50, 100, 500].iter() {
        group.throughput(Throughput::Elements(*size as u64));
        
        // Clean up before each size benchmark
        cleanup_data();
        
        group.bench_with_input(BenchmarkId::new("insert_batch", size), size, |b, &size| {
            b.iter(|| {
                let base = COUNTER.fetch_add(size as u64, Ordering::SeqCst);
                rt.block_on(async {
                    let users: Vec<BenchUser> = (0..size)
                        .map(|i| BenchUser {
                            id: 0,
                            email: format!("batch_{base}_{i}@example.com"),
                            name: format!("Batch User {i}"),
                            age: 20 + (i % 50) as i32,
                            active: i % 2 == 0,
                        })
                        .collect();
                    
                    BenchUser::insert_all(users).await.expect("Batch insert failed")
                })
            });
        });
    }
    
    group.finish();
}

fn bench_find_by_id(c: &mut Criterion) {
    setup_benchmark();
    let rt = get_runtime();
    
    // Setup: Insert users and get their IDs
    let user_ids: Vec<i64> = rt.block_on(async {
        let mut ids = Vec::new();
        for i in 0..100 {
            let user = BenchUser {
                id: 0,
                email: format!("find_{i}@example.com"),
                name: format!("Find User {i}"),
                age: 25 + (i % 30),
                active: true,
            };
            let saved = user.save().await.expect("Insert failed");
            ids.push(saved.id);
        }
        ids
    });
    
    let mut group = c.benchmark_group("find_by_id");
    group.throughput(Throughput::Elements(1));
    
    let counter = AtomicU64::new(0);
    group.bench_function("find_one_user", |b| {
        b.iter(|| {
            let idx = counter.fetch_add(1, Ordering::SeqCst) as usize % user_ids.len();
            let id = user_ids[idx];
            rt.block_on(async {
                BenchUser::find(id).await.expect("Find failed")
            })
        });
    });
    
    group.finish();
}

fn bench_update(c: &mut Criterion) {
    setup_benchmark();
    let rt = get_runtime();
    
    // Setup: Insert users
    let user_ids: Vec<i64> = rt.block_on(async {
        let mut ids = Vec::new();
        for i in 0..100 {
            let user = BenchUser {
                id: 0,
                email: format!("update_{i}@example.com"),
                name: format!("Update User {i}"),
                age: 25,
                active: true,
            };
            let saved = user.save().await.expect("Insert failed");
            ids.push(saved.id);
        }
        ids
    });
    
    let mut group = c.benchmark_group("update");
    group.throughput(Throughput::Elements(1));
    
    let counter = AtomicU64::new(0);
    group.bench_function("update_one_user", |b| {
        b.iter(|| {
            let idx = counter.fetch_add(1, Ordering::SeqCst) as usize % user_ids.len();
            let id = user_ids[idx];
            rt.block_on(async {
                let mut user = BenchUser::find(id).await.expect("Find failed").unwrap();
                user.age += 1;
                user.update().await.expect("Update failed")
            })
        });
    });
    
    group.finish();
}

fn bench_delete(c: &mut Criterion) {
    setup_benchmark();
    let rt = get_runtime();
    
    let mut group = c.benchmark_group("delete");
    group.throughput(Throughput::Elements(1));
    group.sample_size(20);
    
    group.bench_function("delete_one_user", |b| {
        b.iter_custom(|iters| {
            // Setup: Create users for this iteration batch
            cleanup_data();
            
            let ids: Vec<i64> = rt.block_on(async {
                let mut ids = Vec::with_capacity(iters as usize);
                for i in 0..iters {
                    let user = BenchUser {
                        id: 0,
                        email: format!("delete_{i}@example.com"),
                        name: format!("Delete User {i}"),
                        age: 25,
                        active: true,
                    };
                    let saved = user.save().await.expect("Insert failed");
                    ids.push(saved.id);
                }
                ids
            });
            
            // Measure delete time
            let start = std::time::Instant::now();
            rt.block_on(async {
                for id in ids {
                    BenchUser::destroy(id).await.expect("Delete failed");
                }
            });
            start.elapsed()
        });
    });
    
    group.finish();
}

fn bench_count(c: &mut Criterion) {
    setup_benchmark();
    let rt = get_runtime();
    
    let mut group = c.benchmark_group("count");
    group.sample_size(20);
    
    for size in [100, 1000, 10000].iter() {
        // Setup: Insert many users
        cleanup_data();
        
        rt.block_on(async {
            // Insert in batches for speed
            let batch_size = 500;
            let batches = *size / batch_size;
            let remainder = *size % batch_size;
            
            for batch in 0..batches {
                let users: Vec<BenchUser> = (0..batch_size)
                    .map(|i| BenchUser {
                        id: 0,
                        email: format!("count_{batch}_{i}@example.com"),
                        name: format!("Count User {i}"),
                        age: 20 + (i % 50) as i32,
                        active: i % 2 == 0,
                    })
                    .collect();
                BenchUser::insert_all(users).await.expect("Batch insert failed");
            }
            
            if remainder > 0 {
                let users: Vec<BenchUser> = (0..remainder)
                    .map(|i| BenchUser {
                        id: 0,
                        email: format!("count_rem_{i}@example.com"),
                        name: format!("Count User Rem {i}"),
                        age: 20 + (i % 50) as i32,
                        active: i % 2 == 0,
                    })
                    .collect();
                BenchUser::insert_all(users).await.expect("Batch insert failed");
            }
        });
        
        group.bench_with_input(BenchmarkId::new("count_all", size), size, |b, _size| {
            b.iter(|| {
                rt.block_on(async {
                    BenchUser::count().await.expect("Count failed")
                })
            });
        });
    }
    
    group.finish();
}

criterion_group!(
    benches,
    bench_single_insert,
    bench_batch_insert,
    bench_find_by_id,
    bench_update,
    bench_delete,
    bench_count,
);

criterion_main!(benches);
