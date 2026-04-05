use super::*;

pub(super) fn bench_subquery(c: &mut Criterion) {
    let rt = runtime();

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
                    .where_in_subquery(
                        "category",
                        BenchProduct::query()
                            .select(vec!["category"])
                            .where_gt("price", 5000),
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
                    .where_not_in_subquery(
                        "category",
                        BenchProduct::query()
                            .select(vec!["category"])
                            .where_lt("stock", 10),
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

pub(super) fn bench_raw_expressions(c: &mut Criterion) {
    let rt = runtime();

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

pub(super) fn bench_bulk_delete(c: &mut Criterion) {
    let rt = runtime();

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

pub(super) fn bench_combined_features(c: &mut Criterion) {
    let rt = runtime();

    // Setup with 10000 records
    setup_benchmark_with_data(10000);

    let mut group = c.benchmark_group("combined_features");
    group.sample_size(20);

    // Subquery + ordering
    group.bench_function("subquery_with_ordering", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchProduct::query()
                    .where_in_subquery(
                        "category",
                        BenchProduct::query()
                            .select(vec!["category"])
                            .where_eq("active", true),
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
                    .where_in_subquery(
                        "category",
                        BenchProduct::query()
                            .select(vec!["category"])
                            .group_by("category")
                            .having("COUNT(*) > 100"),
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
