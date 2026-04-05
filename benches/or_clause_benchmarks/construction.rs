use super::*;

pub(super) fn bench_or_group_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("or_clause_construction");

    // Simple OR group construction
    group.bench_function("simple_or_group", |b| {
        b.iter(|| {
            let _group = OrGroup::new()
                .where_eq("role", "admin")
                .where_eq("role", "moderator");
        });
    });

    // Multiple conditions OR group
    group.bench_function("complex_or_group", |b| {
        b.iter(|| {
            let _group = OrGroup::new()
                .where_eq("status", "active")
                .where_eq("status", "pending")
                .where_gt("age", 21)
                .where_like("email", "%@company.com")
                .where_in("role", vec!["admin", "moderator", "editor"]);
        });
    });

    // Nested OR groups
    group.bench_function("nested_or_groups", |b| {
        b.iter(|| {
            let _group = OrGroup::new()
                .where_eq("status", "active")
                .nested_and(|inner| inner.where_eq("role", "admin").where_gt("age", 25))
                .nested_or(|inner| {
                    inner
                        .where_eq("department", "Engineering")
                        .where_eq("department", "Marketing")
                });
        });
    });

    group.finish();
}

pub(super) fn bench_query_builder_with_or(c: &mut Criterion) {
    setup_benchmark_with_data(100);

    let mut group = c.benchmark_group("query_builder_or_methods");

    // Simple or_where
    group.bench_function("or_where_simple", |b| {
        b.iter(|| {
            let _query = OrBenchUser::query()
                .where_eq("active", true)
                .or_where(|q| q.where_eq("role", "admin").where_eq("role", "moderator"));
        });
    });

    // Multiple or_where calls
    group.bench_function("or_where_multiple", |b| {
        b.iter(|| {
            let _query = OrBenchUser::query()
                .where_eq("active", true)
                .or_where(|q| q.where_eq("status", "active").where_eq("status", "pending"))
                .or_where(|q| q.where_in("department", vec!["Engineering", "Marketing"]));
        });
    });

    // Using shorthand or_where_eq
    group.bench_function("or_where_eq_shorthand", |b| {
        b.iter(|| {
            let _query = OrBenchUser::query()
                .where_eq("active", true)
                .or_where_eq("role", "admin")
                .or_where_eq("role", "moderator")
                .or_where_eq("role", "editor");
        });
    });

    group.finish();
}
