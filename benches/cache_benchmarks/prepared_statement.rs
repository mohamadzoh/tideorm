use super::*;

pub(super) fn benchmark_prepared_statement_cache_hit(c: &mut Criterion) {
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

pub(super) fn benchmark_prepared_statement_record_execution(c: &mut Criterion) {
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
