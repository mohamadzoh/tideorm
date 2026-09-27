//! Full-Text Search Benchmarks for TideORM
//!
//! Full-text index DDL generation and search-weight rendering.
//!
//! Run with: cargo bench --bench fulltext_benchmarks

use criterion::{Criterion, criterion_group, criterion_main};
use tideorm::config::DatabaseType;
use tideorm::fulltext::{FullTextIndex, PgFullTextIndexType, SearchWeights};

fn bench_index_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("fulltext_index_generation");

    // PostgreSQL index generation
    group.bench_function("postgres_single_column", |b| {
        b.iter(|| {
            let index =
                FullTextIndex::new("idx_articles_title", "articles", vec!["title".to_string()])
                    .language("english");
            index.to_postgres_sql()
        })
    });

    group.bench_function("postgres_multi_column", |b| {
        b.iter(|| {
            let index = FullTextIndex::new(
                "idx_articles_search",
                "articles",
                vec![
                    "title".to_string(),
                    "content".to_string(),
                    "tags".to_string(),
                ],
            )
            .language("english")
            .pg_index_type(PgFullTextIndexType::GIN);
            index.to_postgres_sql()
        })
    });

    group.bench_function("postgres_gist_index", |b| {
        b.iter(|| {
            let index =
                FullTextIndex::new("idx_documents_body", "documents", vec!["body".to_string()])
                    .pg_index_type(PgFullTextIndexType::GiST);
            index.to_postgres_sql()
        })
    });

    // MySQL index generation
    group.bench_function("mysql_single_column", |b| {
        b.iter(|| {
            let index =
                FullTextIndex::new("idx_articles_title", "articles", vec!["title".to_string()]);
            index.to_mysql_sql()
        })
    });

    group.bench_function("mysql_multi_column", |b| {
        b.iter(|| {
            let index = FullTextIndex::new(
                "idx_articles_search",
                "articles",
                vec![
                    "title".to_string(),
                    "content".to_string(),
                    "summary".to_string(),
                ],
            );
            index.to_mysql_sql()
        })
    });

    // SQLite FTS5 generation
    group.bench_function("sqlite_fts5_single", |b| {
        b.iter(|| {
            let index =
                FullTextIndex::new("idx_articles_title", "articles", vec!["title".to_string()]);
            index.to_sqlite_sql()
        })
    });

    group.bench_function("sqlite_fts5_multi", |b| {
        b.iter(|| {
            let index = FullTextIndex::new(
                "idx_articles_search",
                "articles",
                vec![
                    "title".to_string(),
                    "content".to_string(),
                    "tags".to_string(),
                ],
            );
            index.to_sqlite_sql()
        })
    });

    // Generic to_sql with different database types
    group.bench_function("to_sql_postgres", |b| {
        let index =
            FullTextIndex::new("idx", "table", vec!["col1".to_string(), "col2".to_string()]);
        b.iter(|| index.to_sql(DatabaseType::Postgres))
    });

    group.bench_function("to_sql_mysql", |b| {
        let index =
            FullTextIndex::new("idx", "table", vec!["col1".to_string(), "col2".to_string()]);
        b.iter(|| index.to_sql(DatabaseType::MySQL))
    });

    group.bench_function("to_sql_sqlite", |b| {
        let index =
            FullTextIndex::new("idx", "table", vec!["col1".to_string(), "col2".to_string()]);
        b.iter(|| index.to_sql(DatabaseType::SQLite))
    });

    group.finish();
}

fn bench_search_weights(c: &mut Criterion) {
    let weights = SearchWeights::new(1.0, 0.5, 0.25, 0.1);
    c.bench_function("search_weights_to_pg_array", |b| {
        b.iter(|| weights.to_pg_array())
    });
}

criterion_group!(benches, bench_index_generation, bench_search_weights);

criterion_main!(benches);
