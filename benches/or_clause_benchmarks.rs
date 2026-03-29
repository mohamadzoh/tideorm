//! OR Clause Benchmarks for TideORM
//!
//! These benchmarks measure the performance of OR clause query building
//! and execution against a PostgreSQL database.
//!
//! Requirements:
//! - PostgreSQL running on localhost:5432
//! - Database: test_tide_orm
//! - User: postgres / Password: postgres
//!
//! Run with: cargo bench --bench or_clause_benchmarks

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::sync::OnceLock;
use std::time::Duration;
use tideorm::prelude::*;
mod support;

use support::{init_postgres_database, runtime, truncate_table};

// Database initialization flag
static DB_INITIALIZED: OnceLock<()> = OnceLock::new();

// =============================================================================
// BENCHMARK MODEL
// =============================================================================

#[derive(Model, PartialEq)]
#[tideorm(table = "or_bench_users")]
pub struct OrBenchUser {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub email: String,
    pub status: String,
    pub role: String,
    pub department: String,
    pub age: i32,
    pub active: bool,
}

// =============================================================================
// SETUP HELPERS
// =============================================================================

fn init_database() {
    init_postgres_database(
        &DB_INITIALIZED,
        &[
            "DROP TABLE IF EXISTS or_bench_users CASCADE",
            r#"
                CREATE TABLE or_bench_users (
                    id BIGSERIAL PRIMARY KEY,
                    name VARCHAR(255) NOT NULL,
                    email VARCHAR(255) NOT NULL,
                    status VARCHAR(50) NOT NULL,
                    role VARCHAR(50) NOT NULL,
                    department VARCHAR(100) NOT NULL,
                    age INTEGER NOT NULL,
                    active BOOLEAN NOT NULL DEFAULT true
                )
            "#,
            "CREATE INDEX idx_or_bench_status ON or_bench_users(status)",
            "CREATE INDEX idx_or_bench_role ON or_bench_users(role)",
            "CREATE INDEX idx_or_bench_department ON or_bench_users(department)",
            "CREATE INDEX idx_or_bench_age ON or_bench_users(age)",
            "CREATE INDEX idx_or_bench_active ON or_bench_users(active)",
        ],
    );
}

fn cleanup_data() {
    truncate_table("or_bench_users");
}

fn seed_data(count: usize) {
    let rt = runtime();
    let statuses = ["active", "pending", "inactive", "banned"];
    let roles = ["admin", "moderator", "editor", "user", "guest"];
    let departments = ["Engineering", "Marketing", "Sales", "Support", "HR"];

    rt.block_on(async {
        let mut users = Vec::with_capacity(count);
        for i in 0..count {
            users.push(OrBenchUser {
                id: 0,
                name: format!("User {}", i),
                email: format!("user{}@example.com", i),
                status: statuses[i % statuses.len()].to_string(),
                role: roles[i % roles.len()].to_string(),
                department: departments[i % departments.len()].to_string(),
                age: 20 + (i % 50) as i32,
                active: i % 3 != 0,
            });
        }

        // Batch insert
        let _ = OrBenchUser::insert_all(users).await;
    });
}

// =============================================================================
// QUERY BUILDER CONSTRUCTION BENCHMARKS (no database)
// =============================================================================

fn bench_or_group_construction(c: &mut Criterion) {
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

fn bench_query_builder_with_or(c: &mut Criterion) {
    init_database();
    cleanup_data();
    seed_data(100);

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

// =============================================================================
// DATABASE QUERY EXECUTION BENCHMARKS
// =============================================================================

fn bench_or_clause_query_execution(c: &mut Criterion) {
    init_database();
    cleanup_data();
    seed_data(1000);

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

// =============================================================================
// COMPARISON: OR vs IN clause
// =============================================================================

fn bench_or_vs_in_comparison(c: &mut Criterion) {
    init_database();
    cleanup_data();
    seed_data(1000);

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

// =============================================================================
// SCALING BENCHMARKS
// =============================================================================

fn bench_or_clause_scaling(c: &mut Criterion) {
    init_database();

    let rt = runtime();
    let mut group = c.benchmark_group("or_clause_scaling");
    group.measurement_time(Duration::from_secs(10));

    for data_size in [100, 500, 1000, 5000].iter() {
        group.throughput(Throughput::Elements(*data_size as u64));

        cleanup_data();
        seed_data(*data_size);

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

// =============================================================================
// OR GROUP CONDITION COUNT BENCHMARKS
// =============================================================================

fn bench_or_conditions_count(c: &mut Criterion) {
    init_database();
    cleanup_data();
    seed_data(1000);

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

// =============================================================================
// FLUENT OR BRANCH BUILDER BENCHMARKS
// =============================================================================

fn bench_fluent_or_branch_construction(c: &mut Criterion) {
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

fn bench_fluent_or_builder_api(c: &mut Criterion) {
    init_database();
    cleanup_data();
    seed_data(100);

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

fn bench_fluent_or_execution(c: &mut Criterion) {
    init_database();
    cleanup_data();
    seed_data(1000);

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

criterion_group!(
    benches,
    bench_or_group_construction,
    bench_query_builder_with_or,
    bench_or_clause_query_execution,
    bench_or_vs_in_comparison,
    bench_or_clause_scaling,
    bench_or_conditions_count,
    bench_fluent_or_branch_construction,
    bench_fluent_or_builder_api,
    bench_fluent_or_execution,
);

criterion_main!(benches);
