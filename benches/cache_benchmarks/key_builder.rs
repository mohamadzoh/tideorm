use super::*;

pub(super) fn benchmark_cache_key_builder(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_key_builder");

    group.bench_function("simple_key", |b| {
        b.iter(|| CacheKeyBuilder::new().table(black_box("users")).build());
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
