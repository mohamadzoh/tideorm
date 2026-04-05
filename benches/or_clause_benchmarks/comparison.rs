use super::*;

pub(super) fn bench_or_vs_in_comparison(c: &mut Criterion) {
    setup_benchmark_with_data(1000);

    let rt = runtime();
    let mut group = c.benchmark_group("or_vs_in_comparison");
    group.measurement_time(Duration::from_secs(10));

    // Using OR clause
    group.bench_function("using_or_clause", |b| {
        b.iter(|| {
            rt.block_on(async {
                let _results = OrBenchUser::query()
                    .or_where(|q| {
                        q.where_eq("role", "admin")
                            .where_eq("role", "moderator")
                            .where_eq("role", "editor")
                    })
                    .get()
                    .await
                    .unwrap();
            });
        });
    });

    // Using IN clause (should be equivalent but potentially more efficient)
    group.bench_function("using_in_clause", |b| {
        b.iter(|| {
            rt.block_on(async {
                let _results = OrBenchUser::query()
                    .where_in("role", vec!["admin", "moderator", "editor"])
                    .get()
                    .await
                    .unwrap();
            });
        });
    });

    // Complex OR that can't be simplified to IN
    group.bench_function("complex_or_not_in_equivalent", |b| {
        b.iter(|| {
            rt.block_on(async {
                let _results = OrBenchUser::query()
                    .or_where(|q| {
                        q.where_eq("role", "admin")
                            .where_like("email", "%@admin.com")
                            .where_gt("age", 40)
                    })
                    .get()
                    .await
                    .unwrap();
            });
        });
    });

    group.finish();
}
