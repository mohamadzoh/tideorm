use super::*;

pub(super) fn bench_fluent_or_branch_construction(c: &mut Criterion) {
    use tideorm::query::OrBranch;

    let mut group = c.benchmark_group("fluent_or_branch_construction");

    // Single branch construction
    group.bench_function("single_branch", |b| {
        b.iter(|| {
            let _branch = OrBranch::new()
                .where_eq("role", "admin")
                .where_eq("active", true);
        });
    });

    // Complex branch with many conditions
    group.bench_function("complex_branch", |b| {
        b.iter(|| {
            let _branch = OrBranch::new()
                .where_eq("role", "admin")
                .where_eq("active", true)
                .where_gt("age", 18)
                .where_lt("age", 65)
                .where_not_null("verified_at")
                .where_like("email", "%@company.com")
                .where_in("department", vec!["Engineering", "Marketing"]);
        });
    });

    // All condition types
    group.bench_function("all_condition_types", |b| {
        b.iter(|| {
            let _branch = OrBranch::new()
                .where_eq("a", 1)
                .where_not("b", 2)
                .where_gt("c", 3)
                .where_gte("d", 4)
                .where_lt("e", 5)
                .where_lte("f", 6)
                .where_like("g", "%test%")
                .where_not_like("h", "%bad%")
                .where_in("i", vec![1, 2, 3])
                .where_not_in("j", vec![4, 5])
                .where_null("k")
                .where_not_null("l")
                .where_between("m", 10, 20)
                .where_raw("n = 'test'");
        });
    });

    group.finish();
}

pub(super) fn bench_fluent_or_builder_api(c: &mut Criterion) {
    setup_benchmark_with_data(100);

    let mut group = c.benchmark_group("fluent_or_builder_api");

    // Simple fluent OR with AND
    group.bench_function("simple_fluent_or", |b| {
        b.iter(|| {
            let _query = OrBenchUser::query()
                .where_eq("active", true)
                .begin_or()
                .or_where_eq("role", "admin")
                .and_where_eq("age", 30)
                .or_where_eq("role", "moderator")
                .end_or();
        });
    });

    // Multiple branches
    group.bench_function("multi_branch_fluent_or", |b| {
        b.iter(|| {
            let _query = OrBenchUser::query()
                .where_eq("active", true)
                .begin_or()
                .or_where_eq("role", "admin")
                .and_where_eq("department", "Engineering")
                .and_where_gt("age", 25)
                .or_where_eq("role", "moderator")
                .and_where_like("email", "%@company.com")
                .or_where_eq("role", "superuser")
                .end_or();
        });
    });

    // Using begin_or_where_eq shorthand
    group.bench_function("begin_or_where_shorthand", |b| {
        b.iter(|| {
            let _query = OrBenchUser::query()
                .where_eq("active", true)
                .begin_or_where_eq("role", "admin")
                .and_where_eq("age", 30)
                .or_where_eq("role", "moderator")
                .end_or();
        });
    });

    // Complex real-world scenario
    group.bench_function("complex_fluent_scenario", |b| {
        b.iter(|| {
            let _query = OrBenchUser::query()
                .where_eq("active", true)
                .begin_or()
                .or_where_eq("role", "admin")
                .and_where_not_null("email")
                .and_where_gt("age", 21)
                .or_where_eq("role", "moderator")
                .and_where_in("department", vec!["Engineering", "Marketing"])
                .and_where_between("age", 25, 45)
                .or_where_eq("role", "editor")
                .and_where_like("email", "%@example.com")
                .or_where_eq("status", "vip")
                .end_or()
                .where_not("status", "banned");
        });
    });

    group.finish();
}

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
