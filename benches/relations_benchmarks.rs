//! Relations Benchmarks for TideORM
//!
//! Eager-load path parsing and relation-tree merging, without a database.
//!
//! Run with: cargo bench --bench relations_benchmarks

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use tideorm::relations::{RelationPath, RelationTree};

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

fn bench_relation_tree_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("relation_tree_operations");

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

    group.bench_function("get_nested", |b| {
        b.iter(|| black_box(&tree).get_nested("posts"));
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_relation_path_parsing,
    bench_relation_tree_operations,
    bench_relation_tree_lookup,
);

criterion_main!(benches);
