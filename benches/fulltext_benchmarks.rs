//! Full-Text Search Benchmarks for TideORM
//!
//! These benchmarks measure the performance of full-text search operations
//! including SQL generation, highlighting, and snippet generation.
//!
//! Run with: cargo bench --bench fulltext_benchmarks

use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use tideorm::fulltext::{
    FullTextIndex, FullTextConfig, SearchMode, SearchWeights,
    highlight_text, generate_snippet, pg_headline_sql,
    PgFullTextIndexType,
};
use tideorm::config::DatabaseType;

// =============================================================================
// INDEX GENERATION BENCHMARKS
// =============================================================================

fn bench_index_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("fulltext_index_generation");
    
    // PostgreSQL index generation
    group.bench_function("postgres_single_column", |b| {
        b.iter(|| {
            let index = FullTextIndex::new(
                "idx_articles_title",
                "articles",
                vec!["title".to_string()]
            ).language("english");
            index.to_postgres_sql()
        })
    });
    
    group.bench_function("postgres_multi_column", |b| {
        b.iter(|| {
            let index = FullTextIndex::new(
                "idx_articles_search",
                "articles",
                vec!["title".to_string(), "content".to_string(), "tags".to_string()]
            ).language("english").pg_index_type(PgFullTextIndexType::GIN);
            index.to_postgres_sql()
        })
    });
    
    group.bench_function("postgres_gist_index", |b| {
        b.iter(|| {
            let index = FullTextIndex::new(
                "idx_documents_body",
                "documents",
                vec!["body".to_string()]
            ).pg_index_type(PgFullTextIndexType::GiST);
            index.to_postgres_sql()
        })
    });
    
    // MySQL index generation
    group.bench_function("mysql_single_column", |b| {
        b.iter(|| {
            let index = FullTextIndex::new(
                "idx_articles_title",
                "articles",
                vec!["title".to_string()]
            );
            index.to_mysql_sql()
        })
    });
    
    group.bench_function("mysql_multi_column", |b| {
        b.iter(|| {
            let index = FullTextIndex::new(
                "idx_articles_search",
                "articles",
                vec!["title".to_string(), "content".to_string(), "summary".to_string()]
            );
            index.to_mysql_sql()
        })
    });
    
    // SQLite FTS5 generation
    group.bench_function("sqlite_fts5_single", |b| {
        b.iter(|| {
            let index = FullTextIndex::new(
                "idx_articles_title",
                "articles",
                vec!["title".to_string()]
            );
            index.to_sqlite_sql()
        })
    });
    
    group.bench_function("sqlite_fts5_multi", |b| {
        b.iter(|| {
            let index = FullTextIndex::new(
                "idx_articles_search",
                "articles",
                vec!["title".to_string(), "content".to_string(), "tags".to_string()]
            );
            index.to_sqlite_sql()
        })
    });
    
    // Generic to_sql with different database types
    group.bench_function("to_sql_postgres", |b| {
        let index = FullTextIndex::new("idx", "table", vec!["col1".to_string(), "col2".to_string()]);
        b.iter(|| index.to_sql(DatabaseType::Postgres))
    });
    
    group.bench_function("to_sql_mysql", |b| {
        let index = FullTextIndex::new("idx", "table", vec!["col1".to_string(), "col2".to_string()]);
        b.iter(|| index.to_sql(DatabaseType::MySQL))
    });
    
    group.bench_function("to_sql_sqlite", |b| {
        let index = FullTextIndex::new("idx", "table", vec!["col1".to_string(), "col2".to_string()]);
        b.iter(|| index.to_sql(DatabaseType::SQLite))
    });
    
    group.finish();
}

// =============================================================================
// HIGHLIGHTING BENCHMARKS
// =============================================================================

fn bench_highlighting(c: &mut Criterion) {
    let mut group = c.benchmark_group("fulltext_highlighting");
    
    let short_text = "The quick brown fox jumps over the lazy dog.";
    let medium_text = "The quick brown fox jumps over the lazy dog. \
        Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
        Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. \
        Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris.";
    let long_text = medium_text.repeat(10);
    
    // Single word highlighting
    group.bench_function("highlight_single_word_short", |b| {
        b.iter(|| highlight_text(short_text, "fox", "<b>", "</b>"))
    });
    
    group.bench_function("highlight_single_word_medium", |b| {
        b.iter(|| highlight_text(medium_text, "fox", "<b>", "</b>"))
    });
    
    group.bench_function("highlight_single_word_long", |b| {
        b.iter(|| highlight_text(&long_text, "fox", "<b>", "</b>"))
    });
    
    // Multiple word highlighting
    group.bench_function("highlight_multi_word_short", |b| {
        b.iter(|| highlight_text(short_text, "quick fox lazy", "<mark>", "</mark>"))
    });
    
    group.bench_function("highlight_multi_word_medium", |b| {
        b.iter(|| highlight_text(medium_text, "quick fox lazy ipsum dolor", "<mark>", "</mark>"))
    });
    
    group.bench_function("highlight_multi_word_long", |b| {
        b.iter(|| highlight_text(&long_text, "quick fox lazy ipsum dolor", "<mark>", "</mark>"))
    });
    
    // No match highlighting (worst case for search)
    group.bench_function("highlight_no_match_long", |b| {
        b.iter(|| highlight_text(&long_text, "zzzzz", "<b>", "</b>"))
    });
    
    // Different tag lengths
    group.bench_function("highlight_short_tags", |b| {
        b.iter(|| highlight_text(medium_text, "fox", "<b>", "</b>"))
    });
    
    group.bench_function("highlight_long_tags", |b| {
        b.iter(|| highlight_text(medium_text, "fox", "<span class=\"highlight\">", "</span>"))
    });
    
    group.finish();
}

// =============================================================================
// SNIPPET GENERATION BENCHMARKS
// =============================================================================

fn bench_snippet_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("fulltext_snippets");
    
    let short_text = "The quick brown fox jumps over the lazy dog.";
    let medium_text = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
        The quick brown fox jumps over the lazy dog. \
        Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. \
        Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris.";
    let long_text = medium_text.repeat(20);
    
    // Snippet generation with match
    group.bench_function("snippet_short_text", |b| {
        b.iter(|| generate_snippet(short_text, "fox", 5, "<b>", "</b>"))
    });
    
    group.bench_function("snippet_medium_text", |b| {
        b.iter(|| generate_snippet(medium_text, "fox", 5, "<b>", "</b>"))
    });
    
    group.bench_function("snippet_long_text", |b| {
        b.iter(|| generate_snippet(&long_text, "fox", 5, "<b>", "</b>"))
    });
    
    // Different fragment sizes
    for fragment_words in [3, 5, 10, 20].iter() {
        group.bench_with_input(
            BenchmarkId::new("snippet_fragment_size", fragment_words),
            fragment_words,
            |b, &size| {
                b.iter(|| generate_snippet(medium_text, "fox", size, "<b>", "</b>"))
            }
        );
    }
    
    // Snippet with no match (fallback to beginning)
    group.bench_function("snippet_no_match", |b| {
        b.iter(|| generate_snippet(medium_text, "zzzzz", 5, "<b>", "</b>"))
    });
    
    // Snippet at beginning of text
    group.bench_function("snippet_match_at_start", |b| {
        b.iter(|| generate_snippet(medium_text, "Lorem", 5, "<b>", "</b>"))
    });
    
    // Snippet at end of text
    group.bench_function("snippet_match_at_end", |b| {
        b.iter(|| generate_snippet(medium_text, "laboris", 5, "<b>", "</b>"))
    });
    
    group.finish();
}

// =============================================================================
// CONFIGURATION BENCHMARKS
// =============================================================================

fn bench_config_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("fulltext_config");
    
    group.bench_function("config_default", |b| {
        b.iter(|| FullTextConfig::default())
    });
    
    group.bench_function("config_simple", |b| {
        b.iter(|| {
            FullTextConfig::new()
                .language("english")
                .mode(SearchMode::Natural)
        })
    });
    
    group.bench_function("config_full", |b| {
        b.iter(|| {
            FullTextConfig::new()
                .language("english")
                .mode(SearchMode::Boolean)
                .min_word_length(3)
                .max_word_length(50)
                .stop_words(vec![
                    "the".to_string(), "a".to_string(), "an".to_string(),
                    "and".to_string(), "or".to_string(), "but".to_string(),
                ])
                .weights(SearchWeights::new(1.0, 0.5, 0.25, 0.1))
        })
    });
    
    group.bench_function("search_weights_new", |b| {
        b.iter(|| SearchWeights::new(1.0, 0.5, 0.25, 0.1))
    });
    
    group.bench_function("search_weights_to_pg_array", |b| {
        let weights = SearchWeights::new(1.0, 0.5, 0.25, 0.1);
        b.iter(|| weights.to_pg_array())
    });
    
    group.finish();
}

// =============================================================================
// PG HEADLINE SQL GENERATION
// =============================================================================

fn bench_pg_headline(c: &mut Criterion) {
    let mut group = c.benchmark_group("pg_headline_sql");
    
    group.bench_function("simple_query", |b| {
        b.iter(|| pg_headline_sql("content", "search term", "english", "<b>", "</b>"))
    });
    
    group.bench_function("complex_query", |b| {
        b.iter(|| pg_headline_sql(
            "article_content",
            "rust programming language async await",
            "english",
            "<span class=\"highlight\">",
            "</span>"
        ))
    });
    
    group.bench_function("multi_language", |b| {
        let languages = ["english", "german", "french", "spanish"];
        b.iter(|| {
            for lang in &languages {
                let _ = pg_headline_sql("content", "test", lang, "<b>", "</b>");
            }
        })
    });
    
    group.finish();
}

// =============================================================================
// THROUGHPUT BENCHMARKS
// =============================================================================

fn bench_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("fulltext_throughput");
    
    let text = "The quick brown fox jumps over the lazy dog. \
        Lorem ipsum dolor sit amet, consectetur adipiscing elit.";
    
    for count in [10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*count as u64));
        
        let texts: Vec<&str> = (0..*count).map(|_| text).collect();
        
        group.bench_with_input(
            BenchmarkId::new("highlight_batch", count),
            &texts,
            |b, texts| {
                b.iter(|| {
                    for t in texts {
                        let _ = highlight_text(t, "fox lazy", "<b>", "</b>");
                    }
                })
            }
        );
        
        group.bench_with_input(
            BenchmarkId::new("snippet_batch", count),
            &texts,
            |b, texts| {
                b.iter(|| {
                    for t in texts {
                        let _ = generate_snippet(t, "fox", 5, "<b>", "</b>");
                    }
                })
            }
        );
    }
    
    group.finish();
}

criterion_group!(
    benches,
    bench_index_generation,
    bench_highlighting,
    bench_snippet_generation,
    bench_config_creation,
    bench_pg_headline,
    bench_throughput,
);

criterion_main!(benches);
