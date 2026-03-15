//! Relations Benchmarks for TideORM
//!
//! These benchmarks measure the performance of relation type operations
//! without database connectivity.
//!
//! Run with: cargo bench --bench relations_benchmarks

use criterion::{Criterion, criterion_group, criterion_main};
use serde_json::json;
use std::hint::black_box;
use tideorm::query::Order;
use tideorm::relations::{
    MorphResult, RelationConstraints, RelationInfo, RelationPath, RelationTree, RelationType,
};

// =============================================================================
// RELATION TYPE BENCHMARKS
// =============================================================================

fn bench_relation_type_display(c: &mut Criterion) {
    let mut group = c.benchmark_group("relation_type_display");

    group.bench_function("has_one", |b| {
        b.iter(|| format!("{}", black_box(RelationType::HasOne)));
    });

    group.bench_function("has_many", |b| {
        b.iter(|| format!("{}", black_box(RelationType::HasMany)));
    });

    group.bench_function("belongs_to", |b| {
        b.iter(|| format!("{}", black_box(RelationType::BelongsTo)));
    });

    group.bench_function("has_many_through", |b| {
        b.iter(|| format!("{}", black_box(RelationType::HasManyThrough)));
    });

    group.finish();
}

fn bench_relation_type_equality(c: &mut Criterion) {
    c.bench_function("relation_type_equality", |b| {
        b.iter(|| {
            let t1 = black_box(RelationType::HasMany);
            let t2 = black_box(RelationType::HasMany);
            t1 == t2
        });
    });
}

// =============================================================================
// RELATION INFO BENCHMARKS
// =============================================================================

fn bench_relation_info_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("relation_info_creation");

    group.bench_function("belongs_to", |b| {
        b.iter(|| {
            RelationInfo::belongs_to(
                black_box("author"),
                black_box("users"),
                black_box("user_id"),
                black_box("id"),
            )
        });
    });

    group.bench_function("has_one", |b| {
        b.iter(|| {
            RelationInfo::has_one(
                black_box("profile"),
                black_box("profiles"),
                black_box("user_id"),
                black_box("id"),
            )
        });
    });

    group.bench_function("has_many", |b| {
        b.iter(|| {
            RelationInfo::has_many(
                black_box("posts"),
                black_box("posts"),
                black_box("user_id"),
                black_box("id"),
            )
        });
    });

    group.bench_function("has_many_through", |b| {
        b.iter(|| {
            RelationInfo::has_many_through(
                black_box("roles"),
                black_box("roles"),
                black_box("user_roles"),
                black_box("user_id"),
                black_box("role_id"),
                black_box("id"),
            )
        });
    });

    group.bench_function("morph_many", |b| {
        b.iter(|| {
            RelationInfo::morph_many(
                black_box("comments"),
                black_box("comments"),
                black_box("commentable_type"),
                black_box("commentable_id"),
                black_box("id"),
            )
        });
    });

    group.finish();
}

// =============================================================================
// RELATION PATH BENCHMARKS
// =============================================================================

fn bench_relation_path_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("relation_path_parsing");

    group.bench_function("simple", |b| {
        b.iter(|| RelationPath::parse(black_box("posts")));
    });

    group.bench_function("nested_2", |b| {
        b.iter(|| RelationPath::parse(black_box("posts.comments")));
    });

    group.bench_function("nested_3", |b| {
        b.iter(|| RelationPath::parse(black_box("posts.comments.author")));
    });

    group.bench_function("nested_5", |b| {
        b.iter(|| RelationPath::parse(black_box("a.b.c.d.e")));
    });

    group.finish();
}

fn bench_relation_path_operations(c: &mut Criterion) {
    let path = RelationPath::parse("posts.comments.author");

    let mut group = c.benchmark_group("relation_path_operations");

    group.bench_function("root", |b| {
        b.iter(|| black_box(&path).root());
    });

    group.bench_function("depth", |b| {
        b.iter(|| black_box(&path).depth());
    });

    group.bench_function("is_nested", |b| {
        b.iter(|| black_box(&path).is_nested());
    });

    group.bench_function("nested", |b| {
        b.iter(|| black_box(&path).nested());
    });

    group.finish();
}

// =============================================================================
// RELATION TREE BENCHMARKS
// =============================================================================

fn bench_relation_tree_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("relation_tree_operations");

    group.bench_function("create_empty", |b| {
        b.iter(|| RelationTree::new());
    });

    group.bench_function("add_single_path", |b| {
        b.iter(|| {
            let mut tree = RelationTree::new();
            tree.add_path(&RelationPath::parse(black_box("posts")));
            tree
        });
    });

    group.bench_function("add_nested_path", |b| {
        b.iter(|| {
            let mut tree = RelationTree::new();
            tree.add_path(&RelationPath::parse(black_box("posts.comments.author")));
            tree
        });
    });

    group.bench_function("add_multiple_paths", |b| {
        b.iter(|| {
            let mut tree = RelationTree::new();
            tree.add_path(&RelationPath::parse(black_box("posts")));
            tree.add_path(&RelationPath::parse(black_box("profile")));
            tree.add_path(&RelationPath::parse(black_box("posts.comments")));
            tree.add_path(&RelationPath::parse(black_box("posts.tags")));
            tree
        });
    });

    group.finish();
}

fn bench_relation_tree_lookup(c: &mut Criterion) {
    let mut tree = RelationTree::new();
    tree.add_path(&RelationPath::parse("posts.comments.author"));
    tree.add_path(&RelationPath::parse("profile"));
    tree.add_path(&RelationPath::parse("roles"));

    let mut group = c.benchmark_group("relation_tree_lookup");

    group.bench_function("roots", |b| {
        b.iter(|| black_box(&tree).roots());
    });

    group.bench_function("has_nested_true", |b| {
        b.iter(|| black_box(&tree).has_nested("posts"));
    });

    group.bench_function("has_nested_false", |b| {
        b.iter(|| black_box(&tree).has_nested("profile"));
    });

    group.bench_function("get_nested", |b| {
        b.iter(|| black_box(&tree).get_nested("posts"));
    });

    group.bench_function("is_empty", |b| {
        b.iter(|| black_box(&tree).is_empty());
    });

    group.finish();
}

// =============================================================================
// RELATION CONSTRAINTS BENCHMARKS
// =============================================================================

fn bench_relation_constraints(c: &mut Criterion) {
    let mut group = c.benchmark_group("relation_constraints");

    group.bench_function("default", |b| {
        b.iter(|| RelationConstraints::default());
    });

    group.bench_function("where_eq", |b| {
        b.iter(|| {
            RelationConstraints::default().where_eq(black_box("status"), black_box(json!("active")))
        });
    });

    group.bench_function("chained_operations", |b| {
        b.iter(|| {
            RelationConstraints::default()
                .where_eq(black_box("active"), black_box(json!(true)))
                .where_eq(black_box("published"), black_box(json!(true)))
                .order_by(black_box("created_at"), Order::Desc)
                .limit(black_box(10))
                .offset(black_box(0))
        });
    });

    group.bench_function("clone", |b| {
        let constraints = RelationConstraints::default()
            .where_eq("status", json!("active"))
            .limit(10);

        b.iter(|| black_box(&constraints).clone());
    });

    group.finish();
}

// =============================================================================
// MORPH RESULT BENCHMARKS
// =============================================================================

#[derive(Debug, Clone, PartialEq)]
struct Post {
    id: i32,
}

#[derive(Debug, Clone, PartialEq)]
struct Video {
    id: i32,
}

fn bench_morph_result_operations(c: &mut Criterion) {
    let post = Post { id: 1 };
    let result_a: MorphResult<Post, Video> = MorphResult::TypeA(post.clone());
    let result_b: MorphResult<Post, Video> = MorphResult::TypeB(Video { id: 1 });

    let mut group = c.benchmark_group("morph_result_operations");

    group.bench_function("create_type_a", |b| {
        b.iter(|| MorphResult::<Post, Video>::TypeA(black_box(Post { id: 1 })));
    });

    group.bench_function("is_type_a", |b| {
        b.iter(|| black_box(&result_a).is_type_a());
    });

    group.bench_function("is_type_b", |b| {
        b.iter(|| black_box(&result_b).is_type_b());
    });

    group.bench_function("as_type_a", |b| {
        b.iter(|| black_box(&result_a).as_type_a());
    });

    group.bench_function("clone", |b| {
        b.iter(|| black_box(&result_a).clone());
    });

    group.finish();
}

// =============================================================================
// BENCHMARK GROUPS
// =============================================================================

criterion_group!(
    relation_type_benches,
    bench_relation_type_display,
    bench_relation_type_equality,
);

criterion_group!(relation_info_benches, bench_relation_info_creation,);

criterion_group!(
    relation_path_benches,
    bench_relation_path_parsing,
    bench_relation_path_operations,
);

criterion_group!(
    relation_tree_benches,
    bench_relation_tree_operations,
    bench_relation_tree_lookup,
);

criterion_group!(relation_constraints_benches, bench_relation_constraints,);

criterion_group!(morph_result_benches, bench_morph_result_operations,);

criterion_main!(
    relation_type_benches,
    relation_info_benches,
    relation_path_benches,
    relation_tree_benches,
    relation_constraints_benches,
    morph_result_benches,
);
