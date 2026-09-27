//! Non-database TideORM stability benchmarks.
//!
//! These benches cover stable internal workloads that are useful to keep clean:
//! query debugging, OR-clause construction, schema generation, and Rust-to-SQL
//! type mapping.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use tideorm::prelude::*;
use tideorm::schema::rust_type_to_column_type;

#[derive(Model, PartialEq)]
#[tideorm(table = "audit_events")]
struct AuditEvent {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    tenant_id: i64,
    actor_id: i64,
    status: String,
    severity: String,
    attempts: i32,
    archived: bool,
}

#[derive(Model, PartialEq)]
#[tideorm(table = "members")]
struct Member {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
    email: String,
    status: String,
    role: String,
    department: String,
    age: i32,
    active: bool,
}

fn build_simple_debug_info() -> QueryDebugInfo {
    AuditEvent::query()
        .where_eq("tenant_id", 7)
        .where_eq("archived", false)
        .order_by("id", Order::Desc)
        .limit(25)
        .debug()
}

fn build_complex_debug_info() -> QueryDebugInfo {
    AuditEvent::query()
        .where_eq("tenant_id", 7)
        .where_in("severity", vec!["warn", "error", "critical"])
        .where_eq("status", "open")
        .where_gte("attempts", 2)
        .where_lte("attempts", 8)
        .or_where(|query| {
            query
                .where_eq("actor_id", 10)
                .where_eq("actor_id", 11)
                .where_eq("actor_id", 12)
        })
        .order_by("severity", Order::Desc)
        .order_by("id", Order::Desc)
        .page(3, 50)
        .debug()
}

fn audit_event_schema() -> TableSchema {
    TableSchemaBuilder::new("audit_events")
        .schema("analytics")
        .column(
            ColumnSchema::new("id", "BIGINT")
                .primary_key()
                .auto_increment(),
        )
        .column(ColumnSchema::new("tenant_id", "BIGINT").not_null())
        .column(ColumnSchema::new("actor_id", "BIGINT").not_null())
        .column(ColumnSchema::new("status", "VARCHAR(32)").not_null())
        .column(ColumnSchema::new("severity", "VARCHAR(16)").not_null())
        .column(
            ColumnSchema::new("attempts", "INTEGER")
                .not_null()
                .default("0"),
        )
        .column(
            ColumnSchema::new("archived", "BOOLEAN")
                .not_null()
                .default("false"),
        )
        .index(IndexDefinition::new(
            "idx_audit_events_tenant_status",
            vec!["tenant_id".to_string(), "status".to_string()],
            false,
        ))
        .index(IndexDefinition::new(
            "idx_audit_events_severity_attempts",
            vec!["severity".to_string(), "attempts".to_string()],
            false,
        ))
        .index(IndexDefinition::new(
            "uidx_audit_events_tenant_actor",
            vec!["tenant_id".to_string(), "actor_id".to_string()],
            true,
        ))
        .build()
}

fn bench_query_debug_snapshot(c: &mut Criterion) {
    let mut group = c.benchmark_group("query_debug_snapshot");

    group.bench_function("simple", |b| {
        b.iter(|| black_box(build_simple_debug_info()))
    });
    group.bench_function("complex", |b| {
        b.iter(|| black_box(build_complex_debug_info()))
    });

    group.finish();
}

fn bench_query_debug_rendering(c: &mut Criterion) {
    let simple = build_simple_debug_info();
    let complex = build_complex_debug_info();
    let mut group = c.benchmark_group("query_debug_rendering");

    group.bench_function("simple", |b| b.iter(|| black_box(simple.to_string())));
    group.bench_function("complex", |b| b.iter(|| black_box(complex.to_string())));

    group.finish();
}

fn bench_or_group_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("or_group_construction");

    group.bench_function("simple_or_group", |b| {
        b.iter(|| {
            OrGroup::new()
                .where_eq("role", "admin")
                .where_eq("role", "moderator")
        })
    });
    group.bench_function("complex_or_group", |b| {
        b.iter(|| {
            OrGroup::new()
                .where_eq("status", "active")
                .where_eq("status", "pending")
                .where_gt("age", 21)
                .where_like("email", "%@company.com")
                .where_in("role", vec!["admin", "moderator", "editor"])
        })
    });
    group.bench_function("nested_or_groups", |b| {
        b.iter(|| {
            OrGroup::new()
                .where_eq("status", "active")
                .nested_and(|inner| inner.where_eq("role", "admin").where_gt("age", 25))
                .nested_or(|inner| {
                    inner
                        .where_eq("department", "Engineering")
                        .where_eq("department", "Marketing")
                })
        })
    });
    group.bench_function("or_group_all_condition_types", |b| {
        b.iter(|| {
            OrGroup::new()
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
                .where_raw("n = 'test'")
        })
    });

    group.finish();
}

fn bench_or_query_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("or_query_construction");

    group.bench_function("callback_or_where", |b| {
        b.iter(|| {
            Member::query()
                .where_eq("active", true)
                .or_where(|q| q.where_eq("status", "active").where_eq("status", "pending"))
                .or_where(|q| q.where_in("department", vec!["Engineering", "Marketing"]))
        })
    });
    group.bench_function("or_where_eq_shorthand", |b| {
        b.iter(|| {
            Member::query()
                .where_eq("active", true)
                .or_where_eq("role", "admin")
                .or_where_eq("role", "moderator")
                .or_where_eq("role", "editor")
        })
    });
    group.bench_function("fluent_multi_branch", |b| {
        b.iter(|| {
            Member::query()
                .where_eq("active", true)
                .begin_or()
                .or_where_eq("role", "admin")
                .and_where_eq("department", "Engineering")
                .and_where_gt("age", 25)
                .or_where_eq("role", "moderator")
                .and_where_like("email", "%@company.com")
                .or_where_eq("role", "superuser")
                .end_or()
        })
    });
    group.bench_function("fluent_complex_scenario", |b| {
        b.iter(|| {
            Member::query()
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
                .where_not("status", "banned")
        })
    });

    group.finish();
}

fn bench_schema_generation(c: &mut Criterion) {
    let table = audit_event_schema();
    let mut group = c.benchmark_group("schema_generation");

    for db_type in [
        DatabaseType::Postgres,
        DatabaseType::MySQL,
        DatabaseType::SQLite,
    ] {
        group.bench_with_input(
            BenchmarkId::new("generate", format!("{db_type:?}")),
            &db_type,
            |b, db_type| {
                let table = table.clone();
                b.iter(|| {
                    let mut generator = SchemaGenerator::new(*db_type);
                    generator.add_table(table.clone());
                    black_box(generator.generate())
                })
            },
        );
    }

    group.finish();
}

fn bench_rust_type_mapping(c: &mut Criterion) {
    let rust_types = [
        "i64",
        "String",
        "chrono::DateTime<chrono::Utc>",
        "Option<Vec<String>>",
        "serde_json::Value",
    ];
    let mut group = c.benchmark_group("rust_type_to_column_type");

    for db_type in [
        DatabaseType::Postgres,
        DatabaseType::MySQL,
        DatabaseType::SQLite,
    ] {
        group.bench_with_input(
            BenchmarkId::new("map_five_types", format!("{db_type:?}")),
            &db_type,
            |b, db_type| {
                b.iter(|| {
                    black_box(
                        rust_types
                            .iter()
                            .map(|rust_type| {
                                rust_type_to_column_type(black_box(rust_type))
                                    .map(|column_type| column_type.to_sql(*db_type))
                            })
                            .collect::<Vec<_>>(),
                    )
                })
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_query_debug_snapshot,
    bench_query_debug_rendering,
    bench_or_group_construction,
    bench_or_query_construction,
    bench_schema_generation,
    bench_rust_type_mapping,
);

criterion_main!(benches);
