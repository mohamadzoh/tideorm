use super::*;

pub(super) fn benchmark_realistic_workload(c: &mut Criterion) {
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
                cache
                    .set(black_box(&key), black_box(&idx), None, "bench")
                    .ok();
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
            let handles: Vec<_> = (0..4)
                .map(|thread_id| {
                    let cache = Arc::clone(&cache);
                    thread::spawn(move || {
                        for i in 0..100 {
                            let key = format!("key_{}_{}", thread_id, i);
                            cache.set(&key, &i, None, "bench").ok();
                            let _: Option<i32> = cache.get(&key);
                        }
                    })
                })
                .collect();

            for handle in handles {
                handle.join().unwrap();
            }
        });

        cache.clear();
    });

    group.finish();
}

pub(super) fn benchmark_cache_with_serialization(c: &mut Criterion) {
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
            cache
                .set(black_box(&key), black_box(&complex_data), None, "bench")
                .ok();
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
    let vec_data: Vec<ComplexData> = (0..100)
        .map(|i| ComplexData {
            id: i,
            name: format!("User {}", i),
            email: format!("user{}@example.com", i),
            tags: vec!["tag1".to_string()],
            metadata: std::collections::HashMap::new(),
        })
        .collect();

    group.bench_function("cache_vec_100_structs", |b| {
        let mut idx = 0;
        b.iter(|| {
            let key = format!("vec_{}", idx);
            cache
                .set(black_box(&key), black_box(&vec_data), None, "bench")
                .ok();
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

pub(super) fn benchmark_end_to_end_query_cache_paths(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    let db = rt
        .block_on(Database::connect("sqlite::memory:"))
        .expect("failed to connect to benchmark sqlite database");

    rt.block_on(async {
        let conn = db
            .__internal_connection()
            .expect("benchmark sqlite connection should be available");

        conn.execute_unprepared(
            r#"
                CREATE TABLE bench_cache_users (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    email TEXT NOT NULL,
                    name TEXT NOT NULL,
                    active INTEGER NOT NULL DEFAULT 1
                )
            "#,
        )
        .await
        .expect("failed to create benchmark table");

        for i in 0..100 {
            BenchCacheUser {
                id: 0,
                email: format!("user_{i}@example.com"),
                name: format!("User {i}"),
                active: i % 2 == 0,
            }
            .into_active_model()
            .insert(&conn)
            .await
            .expect("failed to seed benchmark row");
        }
    });

    let cache = QueryCache::global();
    cache.disable();
    cache.clear();
    cache.reset_stats();

    let mut group = c.benchmark_group("end_to_end_query_cache");

    group.bench_function("uncached_cache_disabled", |b| {
        b.to_async(&rt).iter(|| async {
            let results = BenchCacheUser::query_with(&db)
                .where_eq("email", "user_42@example.com")
                .get()
                .await
                .expect("uncached query should succeed");
            black_box(results)
        });
    });

    cache.enable();
    cache.clear();
    cache.reset_stats();

    group.bench_function("uncached_cache_enabled", |b| {
        b.to_async(&rt).iter(|| async {
            let results = BenchCacheUser::query_with(&db)
                .where_eq("email", "user_42@example.com")
                .get()
                .await
                .expect("uncached query should succeed");
            black_box(results)
        });
    });

    cache.clear();
    cache.reset_stats();

    group.bench_function("cached_query_enabled", |b| {
        b.to_async(&rt).iter(|| async {
            let results = BenchCacheUser::query_with(&db)
                .where_eq("email", "user_42@example.com")
                .cache(Duration::from_secs(60))
                .get()
                .await
                .expect("cached query should succeed");
            black_box(results)
        });
    });

    cache.disable();
    cache.clear();
    cache.reset_stats();
    group.finish();
}

pub(super) fn benchmark_uncached_query_concurrency(c: &mut Criterion) {
    let db_url = "sqlite://target/bench_cache_concurrency.db?mode=rwc";
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    let db = rt
        .block_on(Database::connect(db_url))
        .expect("failed to connect to concurrency benchmark sqlite database");

    rt.block_on(async {
        let conn = db
            .__internal_connection()
            .expect("benchmark sqlite connection should be available");

        conn.execute_unprepared("DROP TABLE IF EXISTS bench_cache_users")
            .await
            .expect("failed to drop benchmark table");
        conn.execute_unprepared(
            r#"
                CREATE TABLE bench_cache_users (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    email TEXT NOT NULL,
                    name TEXT NOT NULL,
                    active INTEGER NOT NULL DEFAULT 1
                )
            "#,
        )
        .await
        .expect("failed to create benchmark table");

        for i in 0..200 {
            BenchCacheUser {
                id: 0,
                email: format!("concurrent_{i}@example.com"),
                name: format!("Concurrent User {i}"),
                active: i % 2 == 0,
            }
            .into_active_model()
            .insert(&conn)
            .await
            .expect("failed to seed concurrency benchmark row");
        }
    });

    let db = Arc::new(db);
    let cache = QueryCache::global();
    let mut group = c.benchmark_group("uncached_query_concurrency");
    let threads = 4;
    let queries_per_thread = 50;
    let total_queries = (threads * queries_per_thread) as u64;
    group.throughput(Throughput::Elements(total_queries));

    cache.disable();
    cache.clear();
    cache.reset_stats();

    group.bench_function("cache_disabled", |b| {
        b.iter(|| {
            let handles: Vec<_> = (0..threads)
                .map(|thread_id| {
                    let db = Arc::clone(&db);
                    thread::spawn(move || {
                        let rt = tokio::runtime::Runtime::new()
                            .expect("failed to create per-thread runtime");
                        rt.block_on(async move {
                            for query_id in 0..queries_per_thread {
                                let user_index = (thread_id * queries_per_thread + query_id) % 200;
                                let email = format!("concurrent_{user_index}@example.com");
                                let results = BenchCacheUser::query_with(db.as_ref())
                                    .where_eq("email", email)
                                    .get()
                                    .await
                                    .expect("uncached concurrent query should succeed");
                                black_box(results);
                            }
                        });
                    })
                })
                .collect();

            for handle in handles {
                handle.join().expect("benchmark thread should join");
            }
        });
    });

    cache.enable();
    cache.clear();
    cache.reset_stats();

    group.bench_function("cache_enabled_opt_out", |b| {
        b.iter(|| {
            let handles: Vec<_> = (0..threads)
                .map(|thread_id| {
                    let db = Arc::clone(&db);
                    thread::spawn(move || {
                        let rt = tokio::runtime::Runtime::new()
                            .expect("failed to create per-thread runtime");
                        rt.block_on(async move {
                            for query_id in 0..queries_per_thread {
                                let user_index = (thread_id * queries_per_thread + query_id) % 200;
                                let email = format!("concurrent_{user_index}@example.com");
                                let results = BenchCacheUser::query_with(db.as_ref())
                                    .where_eq("email", email)
                                    .get()
                                    .await
                                    .expect("uncached concurrent query should succeed");
                                black_box(results);
                            }
                        });
                    })
                })
                .collect();

            for handle in handles {
                handle.join().expect("benchmark thread should join");
            }
        });
    });

    cache.disable();
    cache.clear();
    cache.reset_stats();
    group.finish();
}
