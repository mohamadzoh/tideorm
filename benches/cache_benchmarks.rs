//! Cache Benchmarks for TideORM
//!
//! Measures performance of query caching and prepared statement caching.
//!
//! Run with: `cargo bench --bench cache_benchmarks`

use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use std::hint::black_box;
use tideorm::cache::{
    QueryCache, PreparedStatementCache, CacheStrategy, CacheKeyBuilder,
};

// =============================================================================
// QUERY CACHE BENCHMARKS
// =============================================================================

fn benchmark_query_cache_set(c: &mut Criterion) {
    let mut group = c.benchmark_group("query_cache_set");
    
    for size in [10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*size as u64));
        
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let cache = QueryCache::new();
            cache.enable();
            cache.set_max_entries(10000);
            cache.set_strategy(CacheStrategy::LRU);
            
            b.iter(|| {
                for i in 0..size {
                    let key = format!("key_{}", i);
                    cache.set(black_box(&key), black_box(&i), None, "bench").ok();
                }
            });
            
            cache.clear();
        });
    }
    
    group.finish();
}

fn benchmark_query_cache_get_hit(c: &mut Criterion) {
    let mut group = c.benchmark_group("query_cache_get_hit");
    
    let cache = QueryCache::new();
    cache.enable();
    cache.set_max_entries(10000);
    cache.set_strategy(CacheStrategy::LRU);
    
    // Pre-populate cache
    for i in 0..1000 {
        let key = format!("key_{}", i);
        cache.set(&key, &i, None, "bench").ok();
    }
    
    group.bench_function("single_hit", |b| {
        b.iter(|| {
            let _: Option<i32> = cache.get(black_box("key_500"));
        });
    });
    
    group.bench_function("1000_hits", |b| {
        b.iter(|| {
            for i in 0..1000 {
                let key = format!("key_{}", i);
                let _: Option<i32> = cache.get(black_box(&key));
            }
        });
    });
    
    cache.clear();
    group.finish();
}

fn benchmark_query_cache_get_miss(c: &mut Criterion) {
    let mut group = c.benchmark_group("query_cache_get_miss");
    
    let cache = QueryCache::new();
    cache.enable();
    cache.set_max_entries(10000);
    cache.set_strategy(CacheStrategy::LRU);
    
    group.bench_function("single_miss", |b| {
        b.iter(|| {
            let _: Option<i32> = cache.get(black_box("nonexistent_key"));
        });
    });
    
    group.bench_function("1000_misses", |b| {
        b.iter(|| {
            for i in 0..1000 {
                let key = format!("missing_key_{}", i);
                let _: Option<i32> = cache.get(black_box(&key));
            }
        });
    });
    
    cache.clear();
    group.finish();
}

fn benchmark_query_cache_strategies(c: &mut Criterion) {
    let mut group = c.benchmark_group("query_cache_strategies");
    
    for strategy in [CacheStrategy::LRU, CacheStrategy::FIFO, CacheStrategy::TTL].iter() {
        let strategy_name = match strategy {
            CacheStrategy::LRU => "LRU",
            CacheStrategy::FIFO => "FIFO",
            CacheStrategy::TTL => "TTL",
        };
        
        group.bench_with_input(BenchmarkId::from_parameter(strategy_name), strategy, |b, strategy| {
            let cache = QueryCache::new();
            cache.enable();
            cache.set_max_entries(100); // Small cache to trigger evictions
            cache.set_strategy(*strategy);
            
            b.iter(|| {
                // Write 200 entries to a cache that holds 100
                for i in 0..200 {
                    let key = format!("key_{}", i);
                    cache.set(black_box(&key), black_box(&i), None, "bench").ok();
                }
            });
            
            cache.clear();
        });
    }
    
    group.finish();
}

fn benchmark_query_cache_invalidation(c: &mut Criterion) {
    let mut group = c.benchmark_group("query_cache_invalidation");
    
    group.bench_function("invalidate_single", |b| {
        let cache = QueryCache::new();
        cache.enable();
        
        // Pre-populate
        for i in 0..1000 {
            let key = format!("key_{}", i);
            cache.set(&key, &i, None, "bench").ok();
        }
        
        let mut idx = 0;
        b.iter(|| {
            let key = format!("key_{}", idx % 1000);
            cache.invalidate(black_box(&key));
            idx += 1;
        });
        
        cache.clear();
    });
    
    group.bench_function("invalidate_model", |b| {
        let cache = QueryCache::new();
        cache.enable();
        
        b.iter(|| {
            // Pre-populate with different models
            for i in 0..100 {
                let key = format!("key_{}", i);
                let model = if i % 2 == 0 { "model_a" } else { "model_b" };
                cache.set(&key, &i, None, model).ok();
            }
            
            // Invalidate one model
            cache.invalidate_model(black_box("model_a"));
        });
        
        cache.clear();
    });
    
    group.bench_function("clear_all", |b| {
        let cache = QueryCache::new();
        cache.enable();
        
        b.iter(|| {
            // Pre-populate
            for i in 0..1000 {
                let key = format!("key_{}", i);
                cache.set(&key, &i, None, "bench").ok();
            }
            
            // Clear all
            cache.clear();
        });
    });
    
    group.finish();
}

// =============================================================================
// PREPARED STATEMENT CACHE BENCHMARKS
// =============================================================================

fn benchmark_prepared_statement_cache_hit(c: &mut Criterion) {
    let mut group = c.benchmark_group("prepared_statement_cache");
    
    let cache = PreparedStatementCache::new();
    cache.enable();
    cache.set_max_statements(1000);
    cache.clear();
    
    // Pre-populate with some statements
    for i in 0..100 {
        let sql = format!("SELECT * FROM table_{} WHERE id = $1", i);
        cache.get_or_prepare(&sql);
    }
    
    group.bench_function("single_hit", |b| {
        b.iter(|| {
            cache.get_or_prepare(black_box("SELECT * FROM table_50 WHERE id = $1"));
        });
    });
    
    group.bench_function("single_miss", |b| {
        let mut idx = 1000;
        b.iter(|| {
            let sql = format!("SELECT * FROM table_{} WHERE id = $1", idx);
            cache.get_or_prepare(black_box(&sql));
            idx += 1;
        });
    });
    
    cache.clear();
    group.finish();
}

fn benchmark_prepared_statement_record_execution(c: &mut Criterion) {
    let mut group = c.benchmark_group("prepared_statement_execution");
    
    let cache = PreparedStatementCache::new();
    cache.enable();
    cache.set_max_statements(1000);
    cache.clear();
    
    let sql = "SELECT * FROM users WHERE id = $1";
    cache.get_or_prepare(sql);
    
    group.bench_function("record_execution", |b| {
        b.iter(|| {
            cache.record_execution(black_box(sql), black_box(100));
        });
    });
    
    cache.clear();
    group.finish();
}

// =============================================================================
// CACHE KEY BUILDER BENCHMARKS
// =============================================================================

fn benchmark_cache_key_builder(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_key_builder");
    
    group.bench_function("simple_key", |b| {
        b.iter(|| {
            CacheKeyBuilder::new()
                .table(black_box("users"))
                .build()
        });
    });
    
    group.bench_function("complex_key", |b| {
        b.iter(|| {
            CacheKeyBuilder::new()
                .table(black_box("users"))
                .condition("active", true)
                .condition("role", "admin")
                .condition("created_at", "2024-01-01")
                .order("name", "asc")
                .limit(100)
                .offset(50)
                .build()
        });
    });
    
    group.bench_function("key_hash", |b| {
        b.iter(|| {
            CacheKeyBuilder::new()
                .table(black_box("users"))
                .condition("active", true)
                .build_hash()
        });
    });
    
    group.finish();
}

// =============================================================================
// COMPLEX SCENARIO BENCHMARKS
// =============================================================================

fn benchmark_realistic_workload(c: &mut Criterion) {
    let mut group = c.benchmark_group("realistic_workload");
    
    group.bench_function("mixed_read_write_80_20", |b| {
        let cache = QueryCache::new();
        cache.enable();
        cache.set_max_entries(1000);
        cache.set_strategy(CacheStrategy::LRU);
        
        // Pre-populate
        for i in 0..500 {
            let key = format!("key_{}", i);
            cache.set(&key, &i, None, "bench").ok();
        }
        
        let mut idx = 0;
        b.iter(|| {
            // 80% reads, 20% writes
            for _ in 0..80 {
                let key = format!("key_{}", idx % 500);
                let _: Option<i32> = cache.get(black_box(&key));
                idx += 1;
            }
            for _ in 0..20 {
                let key = format!("key_{}", idx);
                cache.set(black_box(&key), black_box(&idx), None, "bench").ok();
                idx += 1;
            }
        });
        
        cache.clear();
    });
    
    group.bench_function("high_contention", |b| {
        use std::sync::Arc;
        use std::thread;
        
        let cache = Arc::new(QueryCache::new());
        cache.enable();
        cache.set_max_entries(1000);
        cache.set_strategy(CacheStrategy::LRU);
        
        b.iter(|| {
            let handles: Vec<_> = (0..4).map(|thread_id| {
                let cache = Arc::clone(&cache);
                thread::spawn(move || {
                    for i in 0..100 {
                        let key = format!("key_{}_{}", thread_id, i);
                        cache.set(&key, &i, None, "bench").ok();
                        let _: Option<i32> = cache.get(&key);
                    }
                })
            }).collect();
            
            for handle in handles {
                handle.join().unwrap();
            }
        });
        
        cache.clear();
    });
    
    group.finish();
}

fn benchmark_cache_with_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_serialization");
    
    #[derive(serde::Serialize, serde::Deserialize, Clone)]
    struct ComplexData {
        id: i64,
        name: String,
        email: String,
        tags: Vec<String>,
        metadata: std::collections::HashMap<String, String>,
    }
    
    let complex_data = ComplexData {
        id: 1,
        name: "Test User".to_string(),
        email: "test@example.com".to_string(),
        tags: vec!["tag1".to_string(), "tag2".to_string(), "tag3".to_string()],
        metadata: {
            let mut m = std::collections::HashMap::new();
            m.insert("key1".to_string(), "value1".to_string());
            m.insert("key2".to_string(), "value2".to_string());
            m
        },
    };
    
    let cache = QueryCache::new();
    cache.enable();
    
    group.bench_function("cache_complex_struct", |b| {
        let mut idx = 0;
        b.iter(|| {
            let key = format!("complex_{}", idx);
            cache.set(black_box(&key), black_box(&complex_data), None, "bench").ok();
            idx += 1;
        });
    });
    
    // Pre-populate for read test
    cache.set("complex_read", &complex_data, None, "bench").ok();
    
    group.bench_function("retrieve_complex_struct", |b| {
        b.iter(|| {
            let _: Option<ComplexData> = cache.get(black_box("complex_read"));
        });
    });
    
    // Benchmark Vec of complex structs
    let vec_data: Vec<ComplexData> = (0..100).map(|i| ComplexData {
        id: i,
        name: format!("User {}", i),
        email: format!("user{}@example.com", i),
        tags: vec!["tag1".to_string()],
        metadata: std::collections::HashMap::new(),
    }).collect();
    
    group.bench_function("cache_vec_100_structs", |b| {
        let mut idx = 0;
        b.iter(|| {
            let key = format!("vec_{}", idx);
            cache.set(black_box(&key), black_box(&vec_data), None, "bench").ok();
            idx += 1;
        });
    });
    
    cache.set("vec_read", &vec_data, None, "bench").ok();
    
    group.bench_function("retrieve_vec_100_structs", |b| {
        b.iter(|| {
            let _: Option<Vec<ComplexData>> = cache.get(black_box("vec_read"));
        });
    });
    
    cache.clear();
    group.finish();
}

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

criterion_group!(
    key_builder_benches,
    benchmark_cache_key_builder,
);

criterion_group!(
    complex_benches,
    benchmark_realistic_workload,
    benchmark_cache_with_serialization,
);

criterion_main!(
    query_cache_benches,
    prepared_statement_benches,
    key_builder_benches,
    complex_benches,
);
