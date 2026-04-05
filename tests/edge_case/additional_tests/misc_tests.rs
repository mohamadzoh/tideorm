#[cfg(feature = "fulltext")]
mod fulltext_edge_cases {
    use tideorm::fulltext::{generate_snippet, highlight_text};

    #[test]
    fn test_highlight_empty_query_returns_original() {
        let text = "Hello world, this is a test.";
        let result = highlight_text(text, "", "<b>", "</b>");
        assert_eq!(result, text);
    }

    #[test]
    fn test_highlight_empty_text_returns_empty() {
        let result = highlight_text("", "search", "<b>", "</b>");
        assert_eq!(result, "");
    }

    #[test]
    fn test_highlight_regex_special_chars_in_query() {
        let text = "The price is $100.00 (with tax).";
        let result = highlight_text(text, "$100.00", "<b>", "</b>");
        assert!(!result.is_empty());
    }

    #[test]
    fn test_generate_snippet_no_match_returns_beginning() {
        let text = "Alpha beta gamma delta epsilon zeta eta theta iota kappa";
        let result = generate_snippet(text, "zzzznotfound", 3, "<b>", "</b>");
        assert!(result.contains("Alpha"));
    }

    #[test]
    fn test_generate_snippet_match_at_start() {
        let text = "Rust is a systems programming language focused on safety and performance";
        let result = generate_snippet(text, "Rust", 5, "<b>", "</b>");
        assert!(result.contains("<b>Rust</b>"));
    }

    #[test]
    fn test_highlight_long_text_does_not_timeout() {
        let text = "word ".repeat(10_000);
        let result = highlight_text(&text, "word", "<em>", "</em>");
        assert!(result.contains("<em>word</em>"));
    }
}

mod database_type_default {
    use tideorm::config::DatabaseType;

    #[test]
    fn test_default_database_type_is_postgres() {
        let dt: DatabaseType = Default::default();
        assert_eq!(dt, DatabaseType::Postgres);
    }

    #[test]
    fn test_database_type_is_copy() {
        let a = DatabaseType::MySQL;
        let b = a;
        assert_eq!(a, b);
    }

    #[test]
    fn test_database_type_debug_format() {
        let debug = format!("{:?}", DatabaseType::MariaDB);
        assert_eq!(debug, "MariaDB");
    }
}

mod profiler_lifecycle {
    use std::time::Duration;
    use tideorm::profiling::Profiler;

    #[test]
    fn test_profiler_query_count_increments() {
        let mut profiler = Profiler::start();
        assert_eq!(profiler.query_count(), 0);

        profiler.record("SELECT 1", Duration::from_millis(1));
        assert_eq!(profiler.query_count(), 1);

        profiler.record("SELECT 2", Duration::from_millis(1));
        assert_eq!(profiler.query_count(), 2);
    }

    #[test]
    fn test_profiler_elapsed_increases() {
        let profiler = Profiler::start();
        std::thread::sleep(Duration::from_millis(10));
        let elapsed = profiler.elapsed();
        assert!(elapsed >= Duration::from_millis(5));
    }

    #[test]
    fn test_profiled_query_cached_flag() {
        use tideorm::profiling::ProfiledQuery;
        let q = ProfiledQuery::new("SELECT 1", Duration::from_millis(1)).cached();
        assert!(q.cached);
    }
}

mod cache_empty_results_toggle {
    use std::time::Duration;
    use tideorm::cache::{CacheConfig, CacheStrategy, QueryCache};

    #[test]
    fn test_cache_skips_empty_array_when_disabled() {
        let cache = QueryCache::with_config(CacheConfig {
            enabled: true,
            max_entries: 100,
            default_ttl: Duration::from_secs(60),
            strategy: CacheStrategy::LRU,
            cache_empty_results: false,
            key_prefix: None,
        });

        let empty: Vec<i32> = vec![];
        cache.set("empty", &empty, None, "test").unwrap();
        assert!(!cache.contains("empty"));
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_cache_stores_empty_array_when_enabled() {
        let cache = QueryCache::with_config(CacheConfig {
            enabled: true,
            max_entries: 100,
            default_ttl: Duration::from_secs(60),
            strategy: CacheStrategy::LRU,
            cache_empty_results: true,
            key_prefix: None,
        });

        let empty: Vec<i32> = vec![];
        cache.set("empty", &empty, None, "test").unwrap();
        assert!(cache.contains("empty"));
    }
}
