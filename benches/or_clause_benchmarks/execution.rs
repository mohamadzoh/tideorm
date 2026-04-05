use super::*;

pub(super) fn bench_or_clause_query_execution(c: &mut Criterion) {
    setup_benchmark_with_data(1000);

    let rt = runtime();
    let mut group = c.benchmark_group("or_clause_execution");
    group.measurement_time(Duration::from_secs(10));

    // Simple OR query
    group.bench_function("simple_or_query", |b| {
        b.iter(|| {
            rt.block_on(async {
                let _results = OrBenchUser::query()
                    .or_where(|q| q.where_eq("role", "admin").where_eq("role", "moderator"))
                    .get()
                    .await
                    .unwrap();
            });
        });
    });

    // Complex OR query with AND conditions
    group.bench_function("complex_or_and_query", |b| {
        b.iter(|| {
            rt.block_on(async {
                let _results = OrBenchUser::query()
                    .where_eq("active", true)
                    .or_where(|q| {
                        q.where_eq("role", "admin")
                            .where_eq("role", "moderator")
                            .where_eq("role", "editor")
                    })
                    .where_gt("age", 25)
                    .get()
                    .await
                    .unwrap();
            });
        });
    });

    // Nested OR groups query
    group.bench_function("nested_or_query", |b| {
        b.iter(|| {
            rt.block_on(async {
                let _results = OrBenchUser::query()
                    .where_eq("active", true)
                    .or_where(|q| {
                        q.where_eq("status", "active")
                            .nested_and(|inner| inner.where_eq("role", "admin").where_gt("age", 30))
                    })
                    .get()
                    .await
                    .unwrap();
            });
        });
    });

    // OR with COUNT
    group.bench_function("or_count_query", |b| {
        b.iter(|| {
            rt.block_on(async {
                let _count = OrBenchUser::query()
                    .or_where(|q| q.where_eq("status", "active").where_eq("status", "pending"))
                    .count()
                    .await
                    .unwrap();
            });
        });
    });

    // Multiple shorthand OR methods
    group.bench_function("shorthand_or_methods", |b| {
        b.iter(|| {
            rt.block_on(async {
                let _results = OrBenchUser::query()
                    .where_eq("active", true)
                    .or_where_eq("role", "admin")
                    .or_where_like("email", "%@company.com")
                    .or_where_between("age", 25, 35)
                    .get()
                    .await
                    .unwrap();
            });
        });
    });

    group.finish();
}

pub(super) fn bench_or_clause_scaling(c: &mut Criterion) {
    init_database();

    let rt = runtime();
    let mut group = c.benchmark_group("or_clause_scaling");
    group.measurement_time(Duration::from_secs(10));

    for data_size in [100, 500, 1000, 5000].iter() {
        group.throughput(Throughput::Elements(*data_size as u64));

        reset_data(*data_size);

        group.bench_with_input(
            BenchmarkId::new("or_query_with_dataset", data_size),
            data_size,
            |b, &_size| {
                b.iter(|| {
                    rt.block_on(async {
                        let _results = OrBenchUser::query()
                            .or_where(|q| {
                                q.where_eq("status", "active").where_eq("status", "pending")
                            })
                            .where_eq("active", true)
                            .limit(100)
                            .get()
                            .await
                            .unwrap();
                    });
                });
            },
        );
    }

    group.finish();
}

pub(super) fn bench_or_conditions_count(c: &mut Criterion) {
    setup_benchmark_with_data(1000);

    let rt = runtime();
    let mut group = c.benchmark_group("or_conditions_count");
    group.measurement_time(Duration::from_secs(10));

    // 2 OR conditions
    group.bench_function("2_or_conditions", |b| {
        b.iter(|| {
            rt.block_on(async {
                let _results = OrBenchUser::query()
                    .or_where(|q| q.where_eq("role", "admin").where_eq("role", "moderator"))
                    .get()
                    .await
                    .unwrap();
            });
        });
    });

    // 5 OR conditions
    group.bench_function("5_or_conditions", |b| {
        b.iter(|| {
            rt.block_on(async {
                let _results = OrBenchUser::query()
                    .or_where(|q| {
                        q.where_eq("role", "admin")
                            .where_eq("role", "moderator")
                            .where_eq("role", "editor")
                            .where_eq("role", "user")
                            .where_eq("role", "guest")
                    })
                    .get()
                    .await
                    .unwrap();
            });
        });
    });

    // 10 OR conditions across different columns
    group.bench_function("10_mixed_or_conditions", |b| {
        b.iter(|| {
            rt.block_on(async {
                let _results = OrBenchUser::query()
                    .or_where(|q| {
                        q.where_eq("role", "admin")
                            .where_eq("role", "moderator")
                            .where_eq("status", "active")
                            .where_eq("status", "pending")
                            .where_gt("age", 30)
                            .where_lt("age", 20)
                            .where_like("email", "%@gmail.com")
                            .where_like("email", "%@yahoo.com")
                            .where_eq("department", "Engineering")
                            .where_eq("department", "Marketing")
                    })
                    .get()
                    .await
                    .unwrap();
            });
        });
    });

    group.finish();
}
