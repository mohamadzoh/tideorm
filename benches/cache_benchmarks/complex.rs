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

/// Connect to `url`, recreate `bench_cache_users`, and seed it with `rows` rows
/// whose emails are `user_{i}@example.com`.
fn seeded_users_db(rt: &Runtime, url: &str, rows: usize) -> Database {
    rt.block_on(async {
        let db = Database::connect(url)
            .await
            .expect("failed to connect to benchmark sqlite database");
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

        for i in 0..rows {
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

        db
    })
}

pub(super) fn benchmark_end_to_end_query_cache_paths(c: &mut Criterion) {
    let rt = Runtime::new().expect("failed to create tokio runtime");
    let db = seeded_users_db(&rt, "sqlite::memory:", 100);

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
    const TASKS: usize = 4;
    const QUERIES_PER_TASK: usize = 50;
    const ROWS: usize = 200;

    let rt = Runtime::new().expect("failed to create tokio runtime");
    // A file database: every pooled connection to `sqlite::memory:` would be a
    // separate, empty database.
    let db = Arc::new(seeded_users_db(
        &rt,
        "sqlite://target/bench_cache_concurrency.db?mode=rwc",
        ROWS,
    ));
    let cache = QueryCache::global();
    let mut group = c.benchmark_group("uncached_query_concurrency");
    group.throughput(Throughput::Elements((TASKS * QUERIES_PER_TASK) as u64));

    // A query that never opts into caching must cost the same either way.
    for (name, cache_enabled) in [("cache_disabled", false), ("cache_enabled_opt_out", true)] {
        if cache_enabled {
            cache.enable();
        } else {
            cache.disable();
        }
        cache.clear();
        cache.reset_stats();

        group.bench_function(name, |b| {
            b.to_async(&rt).iter(|| {
                let db = Arc::clone(&db);
                async move {
                    let tasks: Vec<_> = (0..TASKS)
                        .map(|task_id| {
                            let db = Arc::clone(&db);
                            tokio::spawn(async move {
                                for query_id in 0..QUERIES_PER_TASK {
                                    let user_index = (task_id * QUERIES_PER_TASK + query_id) % ROWS;
                                    let results = BenchCacheUser::query_with(db.as_ref())
                                        .where_eq("email", format!("user_{user_index}@example.com"))
                                        .get()
                                        .await
                                        .expect("uncached concurrent query should succeed");
                                    black_box(results);
                                }
                            })
                        })
                        .collect();
                    for task in tasks {
                        task.await.expect("benchmark task should finish");
                    }
                }
            });
        });
    }

    cache.disable();
    cache.clear();
    cache.reset_stats();
    group.finish();
}
