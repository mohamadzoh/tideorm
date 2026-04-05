use super::*;

pub(super) fn benchmark_query_cache_set(c: &mut Criterion) {
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
                    cache
                        .set(black_box(&key), black_box(&i), None, "bench")
                        .ok();
                }
            });

            cache.clear();
        });
    }

    group.finish();
}

pub(super) fn benchmark_query_cache_get_hit(c: &mut Criterion) {
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

pub(super) fn benchmark_query_cache_get_miss(c: &mut Criterion) {
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

pub(super) fn benchmark_query_cache_strategies(c: &mut Criterion) {
    let mut group = c.benchmark_group("query_cache_strategies");

    for strategy in [CacheStrategy::LRU, CacheStrategy::FIFO, CacheStrategy::TTL].iter() {
        let strategy_name = match strategy {
            CacheStrategy::LRU => "LRU",
            CacheStrategy::FIFO => "FIFO",
            CacheStrategy::TTL => "TTL",
        };

        group.bench_with_input(
            BenchmarkId::from_parameter(strategy_name),
            strategy,
            |b, strategy| {
                let cache = QueryCache::new();
                cache.enable();
                cache.set_max_entries(100); // Small cache to trigger evictions
                cache.set_strategy(*strategy);

                b.iter(|| {
                    // Write 200 entries to a cache that holds 100
                    for i in 0..200 {
                        let key = format!("key_{}", i);
                        cache
                            .set(black_box(&key), black_box(&i), None, "bench")
                            .ok();
                    }
                });

                cache.clear();
            },
        );
    }

    group.finish();
}

pub(super) fn benchmark_query_cache_invalidation(c: &mut Criterion) {
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
