use super::*;

pub(super) fn bench_simple_where(c: &mut Criterion) {
    let rt = runtime();

    let mut group = c.benchmark_group("simple_where");
    group.sample_size(60);

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

pub(super) fn bench_range_queries(c: &mut Criterion) {
    let rt = runtime();

    // Setup with 10000 records
    setup_benchmark_with_data(10000);

    let mut group = c.benchmark_group("range_queries");
    group.sample_size(30);

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

pub(super) fn bench_compound_queries(c: &mut Criterion) {
    let rt = runtime();

    // Setup with 10000 records
    setup_benchmark_with_data(10000);

    let mut group = c.benchmark_group("compound_queries");
    group.sample_size(30);

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

pub(super) fn bench_ordering(c: &mut Criterion) {
    let rt = runtime();

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

pub(super) fn bench_pagination(c: &mut Criterion) {
    let rt = runtime();

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

pub(super) fn bench_aggregations(c: &mut Criterion) {
    let rt = runtime();

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
                    rt.block_on(async { BenchProduct::count().await.expect("Count failed") })
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

pub(super) fn bench_where_in(c: &mut Criterion) {
    let rt = runtime();

    // Setup with 10000 records
    setup_benchmark_with_data(10000);

    let mut group = c.benchmark_group("where_in");

    for in_size in [5, 10, 50, 100].iter() {
        let ids: Vec<i64> = (1..=*in_size as i64).collect();

        group.bench_with_input(BenchmarkId::new("where_in_ids", in_size), &ids, |b, ids| {
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
        });
    }

    group.finish();
}

pub(super) fn bench_like_queries(c: &mut Criterion) {
    let rt = runtime();

    // Setup with 10000 records
    setup_benchmark_with_data(10000);

    let mut group = c.benchmark_group("like_queries");
    group.sample_size(50);

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
