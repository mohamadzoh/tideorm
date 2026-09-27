use super::*;

pub(super) fn bench_fluent_or_execution(c: &mut Criterion) {
    setup_benchmark_with_data(1000);

    let rt = runtime();
    let mut group = c.benchmark_group("fluent_or_execution");
    group.measurement_time(Duration::from_secs(10));

    // Execute simple fluent OR query
    group.bench_function("simple_fluent_execution", |b| {
        b.iter(|| {
            rt.block_on(async {
                let _results = OrBenchUser::query()
                    .where_eq("active", true)
                    .begin_or()
                    .or_where_eq("role", "admin")
                    .and_where_gt("age", 25)
                    .or_where_eq("role", "moderator")
                    .end_or()
                    .get()
                    .await
                    .unwrap();
            });
        });
    });

    // Execute complex fluent OR query
    group.bench_function("complex_fluent_execution", |b| {
        b.iter(|| {
            rt.block_on(async {
                let _results = OrBenchUser::query()
                    .where_eq("active", true)
                    .begin_or()
                    .or_where_eq("role", "admin")
                    .and_where_in("department", vec!["Engineering", "Marketing"])
                    .and_where_gt("age", 25)
                    .or_where_eq("role", "moderator")
                    .and_where_like("email", "%@example.com")
                    .or_where_eq("status", "active")
                    .end_or()
                    .limit(100)
                    .get()
                    .await
                    .unwrap();
            });
        });
    });

    // Compare fluent vs callback API performance
    group.bench_function("fluent_vs_callback_fluent", |b| {
        b.iter(|| {
            rt.block_on(async {
                let _results = OrBenchUser::query()
                    .where_eq("active", true)
                    .begin_or()
                    .or_where_eq("role", "admin")
                    .and_where_gt("age", 25)
                    .or_where_eq("role", "moderator")
                    .and_where_gt("age", 30)
                    .end_or()
                    .get()
                    .await
                    .unwrap();
            });
        });
    });

    group.bench_function("fluent_vs_callback_callback", |b| {
        b.iter(|| {
            rt.block_on(async {
                let _results = OrBenchUser::query()
                    .where_eq("active", true)
                    .or_where(|q| {
                        q.nested_and(|inner| inner.where_eq("role", "admin").where_gt("age", 25))
                            .nested_and(|inner| {
                                inner.where_eq("role", "moderator").where_gt("age", 30)
                            })
                    })
                    .get()
                    .await
                    .unwrap();
            });
        });
    });

    group.finish();
}
