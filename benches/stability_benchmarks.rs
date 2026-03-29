//! Non-database TideORM stability benchmarks.
//!
//! These benches cover stable internal workloads that are useful to keep clean:
//! query debugging, schema generation, and Rust-to-SQL type mapping.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use tideorm::prelude::*;
use tideorm::schema::rust_type_to_sql;

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
    let mut group = c.benchmark_group("rust_type_to_sql");

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
                            .map(|rust_type| rust_type_to_sql(black_box(rust_type), *db_type))
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
    bench_schema_generation,
    bench_rust_type_mapping,
);

criterion_main!(benches);
