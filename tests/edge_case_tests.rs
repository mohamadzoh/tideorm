//! Edge case & comprehensive tests for TideORM v0.7
//!
//! These tests cover gaps identified in the test audit, including:
//! - Error From impls & conversion edge cases
//! - Config: malformed URLs, MariaDB detection, feature matrix
//! - Cache: capacity boundaries, concurrent stress, TTL edge cases
//! - Profiling: N+1 detection, complexity scoring, report formatting
//! - Logging: operation detection, stats accumulation, slow-query thresholds
//! - Fulltext: empty query, regex-special chars, long text
//! - Query analyzer: comprehensive SQL pattern analysis
//! - Soft delete: trait method contracts
//! - DatabaseType: exhaustive feature-flag parity between MySQL & MariaDB

// ============================================================================
// 1. Error From<io::Error> conversion
// ============================================================================
mod error_from_io {
    use tideorm::error::Error;

    #[test]
    fn test_io_error_converts_to_internal() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file gone");
        let tide_err: Error = io_err.into();
        // Should become an Internal variant and preserve the message
        match &tide_err {
            Error::Internal { message } => assert!(message.contains("file gone")),
            other => panic!("expected Internal, got {:?}", other),
        }
        assert_eq!(tide_err.code(), "TIDE_INTERNAL");
        assert_eq!(tide_err.http_status(), 500);
    }

    #[test]
    fn test_io_permission_denied_converts() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let tide_err: Error = io_err.into();
        match &tide_err {
            Error::Internal { message } => assert!(message.contains("access denied")),
            other => panic!("expected Internal, got {:?}", other),
        }
        // Internal errors are not retryable
        assert!(!tide_err.is_retryable());
    }
}

// ============================================================================
// 2. Error From<serde_json::Error> conversion
// ============================================================================
mod error_from_serde {
    use tideorm::error::Error;

    #[test]
    fn test_serde_error_converts_to_conversion() {
        let bad_json = "{ not valid }";
        let serde_err = serde_json::from_str::<serde_json::Value>(bad_json).unwrap_err();
        let tide_err: Error = serde_err.into();
        match &tide_err {
            Error::Conversion { message } => {
                assert!(
                    !message.is_empty(),
                    "message should describe the JSON error"
                );
            }
            other => panic!("expected Conversion, got {:?}", other),
        }
        assert_eq!(tide_err.code(), "TIDE_CONVERSION");
        assert_eq!(tide_err.http_status(), 400);
    }

    #[test]
    fn test_serde_eof_error_converts() {
        let serde_err = serde_json::from_str::<serde_json::Value>("").unwrap_err();
        let tide_err: Error = serde_err.into();
        match &tide_err {
            Error::Conversion { message } => {
                assert!(
                    message.contains("EOF") || message.contains("end of") || !message.is_empty()
                );
            }
            other => panic!("expected Conversion, got {:?}", other),
        }
    }
}

// ============================================================================
// 3. Config: empty and malformed database URLs
// ============================================================================
mod config_url_edge_cases {
    use tideorm::config::DatabaseType;

    #[test]
    fn test_from_url_empty_string() {
        assert_eq!(DatabaseType::from_url(""), None);
    }

    #[test]
    fn test_from_url_bare_scheme() {
        // Just a scheme separator with no body
        assert_eq!(DatabaseType::from_url("://"), None);
    }

    #[test]
    fn test_from_url_unknown_scheme() {
        assert_eq!(DatabaseType::from_url("oracle://host/db"), None);
        assert_eq!(DatabaseType::from_url("mssql://host/db"), None);
        assert_eq!(DatabaseType::from_url("mongodb://host/db"), None);
    }

    #[test]
    fn test_from_url_case_insensitive() {
        assert_eq!(
            DatabaseType::from_url("POSTGRES://host"),
            Some(DatabaseType::Postgres)
        );
        assert_eq!(
            DatabaseType::from_url("MySQL://host"),
            Some(DatabaseType::MySQL)
        );
        assert_eq!(
            DatabaseType::from_url("MARIADB://host"),
            Some(DatabaseType::MariaDB)
        );
        assert_eq!(
            DatabaseType::from_url("SQLite:./db.sqlite"),
            Some(DatabaseType::SQLite)
        );
    }

    #[test]
    fn test_from_url_postgresql_alias() {
        assert_eq!(
            DatabaseType::from_url("postgresql://localhost:5432/db"),
            Some(DatabaseType::Postgres)
        );
    }

    #[test]
    fn test_from_url_no_host() {
        // Still returns Some because from_url only checks the scheme prefix
        assert_eq!(
            DatabaseType::from_url("postgres://"),
            Some(DatabaseType::Postgres)
        );
        assert_eq!(
            DatabaseType::from_url("mysql://"),
            Some(DatabaseType::MySQL)
        );
    }
}

// ============================================================================
// 4. MariaDB vs MySQL feature parity matrix
// ============================================================================
mod mariadb_feature_parity {
    use tideorm::config::DatabaseType;

    /// Verify that MariaDB and MySQL agree on every feature flag except
    /// `supports_returning`, `supports_arrays`, `supports_schemas`, and display name.
    #[test]
    fn test_feature_matrix_parity() {
        let mysql = DatabaseType::MySQL;
        let maria = DatabaseType::MariaDB;

        // Same
        assert_eq!(mysql.supports_json(), maria.supports_json());
        assert_eq!(
            mysql.supports_native_json_operators(),
            maria.supports_native_json_operators()
        );
        assert_eq!(mysql.supports_arrays(), maria.supports_arrays());
        assert_eq!(mysql.supports_upsert(), maria.supports_upsert());
        assert_eq!(
            mysql.supports_fulltext_search(),
            maria.supports_fulltext_search()
        );
        assert_eq!(
            mysql.supports_window_functions(),
            maria.supports_window_functions()
        );
        assert_eq!(mysql.supports_cte(), maria.supports_cte());
        assert_eq!(mysql.supports_schemas(), maria.supports_schemas());
        assert_eq!(mysql.default_port(), maria.default_port());
        assert_eq!(mysql.param_style(), maria.param_style());
        assert_eq!(mysql.quote_char(), maria.quote_char());
        assert_eq!(mysql.optimal_batch_size(), maria.optimal_batch_size());

        // Different: RETURNING
        assert!(!mysql.supports_returning());
        assert!(maria.supports_returning());

        // is_mysql_compatible
        assert!(mysql.is_mysql_compatible());
        assert!(maria.is_mysql_compatible());
    }

    #[test]
    fn test_mariadb_display_differs_from_mysql() {
        assert_eq!(format!("{}", DatabaseType::MySQL), "MySQL");
        assert_eq!(format!("{}", DatabaseType::MariaDB), "MariaDB");
    }

    #[test]
    fn test_mariadb_url_scheme() {
        assert_eq!(DatabaseType::MariaDB.url_scheme(), "mariadb");
        assert_eq!(DatabaseType::MySQL.url_scheme(), "mysql");
    }
}

// ============================================================================
// 5. Cache: capacity-1 boundary + immediate eviction
// ============================================================================
mod cache_capacity_boundary {
    use std::time::Duration;
    use tideorm::cache::{CacheConfig, CacheStrategy, QueryCache};

    #[test]
    fn test_cache_capacity_one_evicts_on_second_insert() {
        let cache = QueryCache::with_config(CacheConfig {
            enabled: true,
            max_entries: 1,
            default_ttl: Duration::from_secs(60),
            strategy: CacheStrategy::LRU,
            cache_empty_results: true,
            key_prefix: None,
        });

        cache
            .set::<String>("k1", &"val1".to_string(), None, "test")
            .unwrap();
        assert_eq!(cache.len(), 1);
        assert!(cache.contains("k1"));

        // Second insert should evict the first
        cache
            .set::<String>("k2", &"val2".to_string(), None, "test")
            .unwrap();
        assert_eq!(cache.len(), 1);
        assert!(cache.contains("k2"));
        assert!(!cache.contains("k1")); // evicted
    }

    #[test]
    fn test_cache_fifo_eviction_order() {
        let cache = QueryCache::with_config(CacheConfig {
            enabled: true,
            max_entries: 2,
            default_ttl: Duration::from_secs(60),
            strategy: CacheStrategy::FIFO,
            cache_empty_results: true,
            key_prefix: None,
        });

        cache.set::<i32>("a", &1, None, "t").unwrap();
        cache.set::<i32>("b", &2, None, "t").unwrap();
        // Touch "a" so it was recently accessed
        let _: Option<i32> = cache.get("a");

        // Insert "c" — FIFO should evict "a" (oldest insert), not "b"
        cache.set::<i32>("c", &3, None, "t").unwrap();
        assert_eq!(cache.len(), 2);
        assert!(!cache.contains("a")); // FIFO: first in, first out
        assert!(cache.contains("b"));
        assert!(cache.contains("c"));
    }
}

// ============================================================================
// 6. Cache: TTL expiration
// ============================================================================
mod cache_ttl_expiration {
    use std::thread;
    use std::time::Duration;
    use tideorm::cache::{CacheConfig, CacheStrategy, QueryCache};

    #[test]
    fn test_cache_entry_expires_after_ttl() {
        let cache = QueryCache::with_config(CacheConfig {
            enabled: true,
            max_entries: 100,
            default_ttl: Duration::from_millis(50),
            strategy: CacheStrategy::LRU,
            cache_empty_results: true,
            key_prefix: None,
        });

        cache
            .set::<String>("k", &"value".to_string(), None, "test")
            .unwrap();
        assert!(cache.get::<String>("k").is_some());

        // Wait for TTL to expire
        thread::sleep(Duration::from_millis(80));

        // Should be expired now
        assert!(cache.get::<String>("k").is_none());
    }
}

// ============================================================================
// 7. Cache: concurrent read/write stress
// ============================================================================
mod cache_concurrent_stress {
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;
    use tideorm::cache::{CacheConfig, CacheStrategy, QueryCache};

    #[test]
    fn test_cache_concurrent_read_write_50_threads() {
        let cache = Arc::new(QueryCache::with_config(CacheConfig {
            enabled: true,
            max_entries: 100,
            default_ttl: Duration::from_secs(60),
            strategy: CacheStrategy::LRU,
            cache_empty_results: true,
            key_prefix: None,
        }));

        let mut handles = Vec::new();

        // 25 writer threads
        for i in 0..25 {
            let c = Arc::clone(&cache);
            handles.push(thread::spawn(move || {
                for j in 0..20 {
                    let key = format!("w{}_{}", i, j);
                    c.set::<i32>(&key, &(i * 100 + j), None, "stress").unwrap();
                }
            }));
        }

        // 25 reader threads
        for i in 0..25 {
            let c = Arc::clone(&cache);
            handles.push(thread::spawn(move || {
                for j in 0..20 {
                    let key = format!("w{}_{}", i, j);
                    let _: Option<i32> = c.get(&key);
                }
            }));
        }

        for h in handles {
            h.join().expect("thread should not panic");
        }

        // No panics, no data races — cache is still valid
        let stats = cache.stats();
        assert!(stats.entries <= 100, "should not exceed max_entries");
    }
}

// ============================================================================
// 8. Cache: invalidate_model selective removal
// ============================================================================
mod cache_invalidate_model {
    use std::time::Duration;
    use tideorm::cache::{CacheConfig, CacheStrategy, QueryCache};

    #[test]
    fn test_invalidate_model_only_removes_matching() {
        let cache = QueryCache::with_config(CacheConfig {
            enabled: true,
            max_entries: 100,
            default_ttl: Duration::from_secs(60),
            strategy: CacheStrategy::LRU,
            cache_empty_results: true,
            key_prefix: None,
        });

        cache
            .set::<String>("user:1", &"alice".into(), None, "User")
            .unwrap();
        cache
            .set::<String>("user:2", &"bob".into(), None, "User")
            .unwrap();
        cache
            .set::<String>("post:1", &"hello".into(), None, "Post")
            .unwrap();

        assert_eq!(cache.len(), 3);

        cache.invalidate_model("User");

        assert_eq!(cache.len(), 1);
        assert!(!cache.contains("user:1"));
        assert!(!cache.contains("user:2"));
        assert!(cache.contains("post:1"));
    }
}

// ============================================================================
// 9. Cache: key prefix generation
// ============================================================================
mod cache_key_prefix {
    use std::time::Duration;
    use tideorm::cache::{CacheConfig, CacheStrategy, QueryCache};

    #[test]
    fn test_generate_key_with_prefix() {
        let cache = QueryCache::with_config(CacheConfig {
            enabled: true,
            max_entries: 100,
            default_ttl: Duration::from_secs(60),
            strategy: CacheStrategy::LRU,
            cache_empty_results: true,
            key_prefix: Some("v7".into()),
        });

        let key = cache.generate_key("users", 12345);
        assert_eq!(key, "v7:users:12345");
    }

    #[test]
    fn test_generate_key_without_prefix() {
        let cache = QueryCache::new();
        let key = cache.generate_key("posts", 999);
        assert_eq!(key, "posts:999");
    }
}

// ============================================================================
// 10. Cache: stats accounting
// ============================================================================
mod cache_stats_accounting {
    use std::time::Duration;
    use tideorm::cache::{CacheConfig, CacheStrategy, QueryCache};

    #[test]
    fn test_stats_track_hits_misses_evictions() {
        let cache = QueryCache::with_config(CacheConfig {
            enabled: true,
            max_entries: 2,
            default_ttl: Duration::from_secs(60),
            strategy: CacheStrategy::LRU,
            cache_empty_results: true,
            key_prefix: None,
        });

        // Miss
        let _: Option<i32> = cache.get("nope");

        // Set + hit
        cache.set::<i32>("a", &1, None, "t").unwrap();
        let _: Option<i32> = cache.get("a"); // hit

        // Fill + evict
        cache.set::<i32>("b", &2, None, "t").unwrap();
        cache.set::<i32>("c", &3, None, "t").unwrap(); // evicts "a" (LRU touched earliest since "a" was accessed)

        let stats = cache.stats();
        assert!(stats.misses >= 1);
        assert!(stats.hits >= 1);
        assert!(stats.evictions >= 1);
        assert!(stats.entries <= 2);
        // hit_ratio should be between 0 and 1
        let ratio = stats.hit_ratio();
        assert!((0.0..=1.0).contains(&ratio));
    }
}

// ============================================================================
// 11. Profiling: N+1 detection via ProfileReport::suggestions
// ============================================================================
mod profiling_n_plus_one {
    use std::time::Duration;
    use tideorm::profiling::{ProfiledQuery, Profiler};

    #[test]
    fn test_n_plus_one_detection_triggers_above_10_queries() {
        let mut profiler = Profiler::start();

        // Simulate an N+1: 15 individual selects on the same table
        for i in 0..15 {
            let q = ProfiledQuery::new(
                format!("SELECT * FROM posts WHERE id = {}", i),
                Duration::from_micros(500),
            )
            .with_table("posts");
            profiler.record_full(q);
        }

        let report = profiler.stop();
        let suggestions = report.suggestions();
        assert!(
            suggestions.iter().any(|s| s.contains("N+1")),
            "should detect N+1 pattern, got: {:?}",
            suggestions
        );
    }

    #[test]
    fn test_no_n_plus_one_below_threshold() {
        let mut profiler = Profiler::start();
        for i in 0..5 {
            let q = ProfiledQuery::new(
                format!("SELECT * FROM posts WHERE id = {}", i),
                Duration::from_micros(500),
            )
            .with_table("posts");
            profiler.record_full(q);
        }
        let report = profiler.stop();
        let suggestions = report.suggestions();
        assert!(
            !suggestions.iter().any(|s| s.contains("N+1")),
            "should NOT detect N+1 with only 5 queries"
        );
    }
}

// ============================================================================
// 12. Profiling: QueryAnalyzer comprehensive SQL pattern analysis
// ============================================================================
mod profiling_query_analyzer {
    use tideorm::profiling::{QueryAnalyzer, QueryComplexity, SuggestionLevel};

    #[test]
    fn test_analyzer_detects_leading_wildcard_like() {
        let suggestions = QueryAnalyzer::analyze("SELECT * FROM users WHERE name LIKE '%john'");
        assert!(suggestions.iter().any(|s| s.title.contains("wildcard")));
    }

    #[test]
    fn test_analyzer_detects_not_in() {
        let suggestions = QueryAnalyzer::analyze("SELECT * FROM users WHERE id NOT IN (1,2,3)");
        assert!(suggestions.iter().any(|s| s.title.contains("NOT IN")));
    }

    #[test]
    fn test_analyzer_detects_function_in_where() {
        let suggestions =
            QueryAnalyzer::analyze("SELECT * FROM users WHERE LOWER(email) = 'a@b.com'");
        assert!(suggestions.iter().any(|s| s.title.contains("Function")));
    }

    #[test]
    fn test_analyzer_order_by_without_limit() {
        let suggestions = QueryAnalyzer::analyze("SELECT id FROM users ORDER BY created_at");
        assert!(
            suggestions
                .iter()
                .any(|s| s.title.contains("ORDER BY") && s.title.contains("LIMIT"))
        );
    }

    #[test]
    fn test_complexity_very_complex_query() {
        let sql = "SELECT u.*, p.*, c.*, t.* \
                    FROM users u \
                    JOIN posts p ON p.user_id = u.id \
                    JOIN comments c ON c.post_id = p.id \
                    JOIN tags t ON t.post_id = p.id \
                    WHERE u.active = true \
                    GROUP BY u.id \
                    HAVING COUNT(p.id) > 5 \
                    ORDER BY u.created_at DESC";
        let complexity = QueryAnalyzer::estimate_complexity(sql);
        assert!(
            matches!(
                complexity,
                QueryComplexity::Complex | QueryComplexity::VeryComplex
            ),
            "expected Complex or VeryComplex, got {:?}",
            complexity
        );
    }

    #[test]
    fn test_complexity_simple_insert() {
        let complexity =
            QueryAnalyzer::estimate_complexity("INSERT INTO users (name) VALUES ('Alice')");
        assert!(matches!(
            complexity,
            QueryComplexity::Simple | QueryComplexity::Moderate
        ));
    }

    #[test]
    fn test_missing_where_on_update_is_critical() {
        let suggestions = QueryAnalyzer::analyze("UPDATE users SET active = false");
        assert!(
            suggestions
                .iter()
                .any(|s| s.level == SuggestionLevel::Critical && s.title.contains("WHERE"))
        );
    }
}

// ============================================================================
// 13. Profiling: ProfileReport formatting & statistics
// ============================================================================
mod profiling_report_stats {
    use std::time::Duration;
    use tideorm::profiling::{ProfiledQuery, Profiler};

    #[test]
    fn test_empty_profiler_report() {
        let profiler = Profiler::start();
        std::thread::sleep(Duration::from_millis(5));
        let report = profiler.stop();

        assert_eq!(report.query_count(), 0);
        assert_eq!(report.avg_query_time(), Duration::ZERO);
        assert_eq!(report.query_time_percentage(), 0.0);
        assert!(
            report
                .queries_slower_than(Duration::from_secs(1))
                .is_empty()
        );
    }

    #[test]
    fn test_report_slowest_queries_ordered() {
        let mut profiler = Profiler::start();
        profiler.record("SELECT 1", Duration::from_millis(10));
        profiler.record("SELECT 2", Duration::from_millis(200));
        profiler.record("SELECT 3", Duration::from_millis(50));

        let report = profiler.stop();
        assert_eq!(report.slowest.len(), 3);
        // Should be sorted descending by duration
        assert!(report.slowest[0].duration >= report.slowest[1].duration);
        assert!(report.slowest[1].duration >= report.slowest[2].duration);
    }

    #[test]
    fn test_report_display_does_not_panic() {
        let mut profiler = Profiler::start();
        profiler.record("SELECT * FROM users WHERE id = 1", Duration::from_millis(5));
        let q = ProfiledQuery::new(
            "UPDATE posts SET title = 'test' WHERE id = 1",
            Duration::from_millis(150),
        )
        .with_table("posts")
        .with_rows(1);
        profiler.record_full(q);
        let report = profiler.stop();

        // Should not panic when displaying the report
        let output = format!("{}", report);
        assert!(output.contains("TIDEORM PERFORMANCE PROFILE REPORT"));
        assert!(output.contains("Total Queries"));
    }

    #[test]
    fn test_queries_slower_than_threshold() {
        let mut profiler = Profiler::start();
        profiler.record("fast", Duration::from_millis(5));
        profiler.record("slow", Duration::from_millis(500));
        profiler.record("medium", Duration::from_millis(50));
        let report = profiler.stop();

        let slow = report.queries_slower_than(Duration::from_millis(100));
        assert_eq!(slow.len(), 1);
        assert!(slow[0].sql.contains("slow"));
    }
}

// ============================================================================
// 14. Profiling: GlobalProfiler reset & stat accumulation
// ============================================================================
mod profiling_global {
    use std::time::Duration;
    use tideorm::profiling::GlobalProfiler;

    #[test]
    fn test_global_profiler_reset_clears_stats() {
        GlobalProfiler::enable();
        GlobalProfiler::reset();

        GlobalProfiler::record(Duration::from_millis(10));
        GlobalProfiler::record(Duration::from_millis(20));

        let stats = GlobalProfiler::stats();
        assert_eq!(stats.total_queries, 2);

        GlobalProfiler::reset();
        let stats = GlobalProfiler::stats();
        assert_eq!(stats.total_queries, 0);
        assert_eq!(stats.slow_queries, 0);
        assert_eq!(stats.total_time_ns, 0);

        GlobalProfiler::disable();
    }

    #[test]
    fn test_global_profiler_disabled_does_not_record() {
        GlobalProfiler::disable();
        GlobalProfiler::reset();

        GlobalProfiler::record(Duration::from_millis(999));

        let stats = GlobalProfiler::stats();
        assert_eq!(stats.total_queries, 0);
    }

    #[test]
    fn test_global_stats_avg_and_slow_percentage() {
        GlobalProfiler::enable();
        GlobalProfiler::reset();
        GlobalProfiler::set_slow_threshold(50);

        GlobalProfiler::record(Duration::from_millis(10)); // fast
        GlobalProfiler::record(Duration::from_millis(100)); // slow
        GlobalProfiler::record(Duration::from_millis(30)); // fast
        GlobalProfiler::record(Duration::from_millis(200)); // slow

        let stats = GlobalProfiler::stats();
        assert_eq!(stats.total_queries, 4);
        assert_eq!(stats.slow_queries, 2);
        assert!((stats.slow_percentage() - 50.0).abs() < 0.1);
        assert!(stats.avg_query_time() > Duration::ZERO);

        // Display
        let display = format!("{}", stats);
        assert!(display.contains("Total Queries:"));
        assert!(display.contains("Slow Queries:"));

        GlobalProfiler::disable();
    }
}

// ============================================================================
// 15. Logging: QueryOperation detection from SQL
// ============================================================================
mod logging_operation_detection {
    use tideorm::logging::QueryOperation;

    #[test]
    fn test_all_operations_detected_correctly() {
        assert_eq!(
            QueryOperation::from_sql("SELECT * FROM users"),
            QueryOperation::Select
        );
        assert_eq!(
            QueryOperation::from_sql("INSERT INTO users VALUES (1)"),
            QueryOperation::Insert
        );
        assert_eq!(
            QueryOperation::from_sql("UPDATE users SET name = 'x'"),
            QueryOperation::Update
        );
        assert_eq!(
            QueryOperation::from_sql("DELETE FROM users WHERE id = 1"),
            QueryOperation::Delete
        );
        assert_eq!(
            QueryOperation::from_sql("BEGIN"),
            QueryOperation::Transaction
        );
        assert_eq!(
            QueryOperation::from_sql("COMMIT"),
            QueryOperation::Transaction
        );
        assert_eq!(
            QueryOperation::from_sql("ROLLBACK"),
            QueryOperation::Transaction
        );
        assert_eq!(
            QueryOperation::from_sql("CREATE TABLE foo (id INT)"),
            QueryOperation::Unknown
        );
    }

    #[test]
    fn test_operation_case_insensitive() {
        assert_eq!(
            QueryOperation::from_sql("  select * from users"),
            QueryOperation::Select
        );
        assert_eq!(
            QueryOperation::from_sql("  insert into users"),
            QueryOperation::Insert
        );
    }

    #[test]
    fn test_operation_as_str_roundtrip() {
        for op in [
            QueryOperation::Select,
            QueryOperation::Insert,
            QueryOperation::Update,
            QueryOperation::Delete,
            QueryOperation::Raw,
            QueryOperation::Transaction,
            QueryOperation::Unknown,
        ] {
            let s = op.as_str();
            assert!(!s.is_empty());
            assert_eq!(format!("{}", op), s);
        }
    }
}

// ============================================================================
// 16. Logging: QueryLogEntry formatting & slow query detection
// ============================================================================
mod logging_entry_formatting {
    use std::time::Duration;
    use tideorm::logging::QueryLogEntry;

    #[test]
    fn test_log_entry_is_slow_with_various_thresholds() {
        let entry = QueryLogEntry::new("SELECT 1").with_duration(Duration::from_millis(150));

        assert!(entry.is_slow(100));
        assert!(entry.is_slow(150)); // >= threshold
        assert!(!entry.is_slow(200));
    }

    #[test]
    fn test_log_entry_no_duration_is_not_slow() {
        let entry = QueryLogEntry::new("SELECT 1");
        assert!(!entry.is_slow(0));
    }

    #[test]
    fn test_log_entry_format_console_includes_sql() {
        let entry = QueryLogEntry::new("SELECT * FROM users WHERE id = 1")
            .with_table("users")
            .with_duration(Duration::from_millis(42))
            .with_rows(3)
            .with_params(vec!["1".to_string()]);

        let output = entry.format_console();
        assert!(output.contains("[TIDE]"));
        assert!(output.contains("SELECT"));
        assert!(output.contains("users"));
        assert!(output.contains("42ms"));
        assert!(output.contains("3 rows"));
        assert!(output.contains("Params:"));
    }

    #[test]
    fn test_log_entry_failed_format() {
        let entry = QueryLogEntry::new("INSERT INTO users VALUES (1)").with_error("duplicate key");

        let output = entry.format_console();
        assert!(output.contains("FAILED"));
        assert!(output.contains("duplicate key"));
        assert!(!entry.success);
    }
}

// ============================================================================
// 17. Logging: QueryStats display & calculations
// ============================================================================
mod logging_stats {
    use tideorm::logging::QueryLogger;

    #[test]
    fn test_query_stats_display_format() {
        // Reset stats for a clean baseline
        QueryLogger::reset_stats();

        let stats = QueryLogger::stats();
        let display = format!("{}", stats);
        assert!(display.contains("TIDEORM QUERY STATISTICS"));
        assert!(display.contains("Total Queries:"));
        assert!(display.contains("Slow Queries:"));
        assert!(display.contains("Avg Query Time:"));
    }

    #[test]
    fn test_query_stats_zero_queries_no_division_by_zero() {
        QueryLogger::reset_stats();
        let stats = QueryLogger::stats();
        assert_eq!(stats.avg_query_time_ms(), 0.0);
        assert_eq!(stats.slow_query_percentage(), 0.0);
    }
}

// ============================================================================
// 18. Logging: LogLevel parsing edge cases
// ============================================================================
mod logging_level_parsing {
    use tideorm::logging::LogLevel;

    #[test]
    fn test_log_level_parse_all_variants() {
        assert_eq!(LogLevel::parse_str("off"), LogLevel::Off);
        assert_eq!(LogLevel::parse_str("none"), LogLevel::Off);
        assert_eq!(LogLevel::parse_str("0"), LogLevel::Off);
        assert_eq!(LogLevel::parse_str("error"), LogLevel::Error);
        assert_eq!(LogLevel::parse_str("1"), LogLevel::Error);
        assert_eq!(LogLevel::parse_str("warn"), LogLevel::Warn);
        assert_eq!(LogLevel::parse_str("warning"), LogLevel::Warn);
        assert_eq!(LogLevel::parse_str("2"), LogLevel::Warn);
        assert_eq!(LogLevel::parse_str("info"), LogLevel::Info);
        assert_eq!(LogLevel::parse_str("3"), LogLevel::Info);
        assert_eq!(LogLevel::parse_str("debug"), LogLevel::Debug);
        assert_eq!(LogLevel::parse_str("4"), LogLevel::Debug);
        assert_eq!(LogLevel::parse_str("trace"), LogLevel::Trace);
        assert_eq!(LogLevel::parse_str("all"), LogLevel::Trace);
        assert_eq!(LogLevel::parse_str("5"), LogLevel::Trace);
    }

    #[test]
    fn test_log_level_parse_unknown_defaults_to_off() {
        assert_eq!(LogLevel::parse_str("verbose"), LogLevel::Off);
        assert_eq!(LogLevel::parse_str(""), LogLevel::Off);
        assert_eq!(LogLevel::parse_str("garbage"), LogLevel::Off);
    }

    #[test]
    fn test_log_level_ordering() {
        assert!(LogLevel::Off < LogLevel::Error);
        assert!(LogLevel::Error < LogLevel::Warn);
        assert!(LogLevel::Warn < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Debug);
        assert!(LogLevel::Debug < LogLevel::Trace);
    }
}

// ============================================================================
// 19. Fulltext: highlight_text with empty query
// ============================================================================
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
        // Query containing regex metacharacters should not crash
        let text = "The price is $100.00 (with tax).";
        let result = highlight_text(text, "$100.00", "<b>", "</b>");
        // Should not panic — the regex::escape ensures safety
        assert!(!result.is_empty());
    }

    #[test]
    fn test_generate_snippet_no_match_returns_beginning() {
        let text = "Alpha beta gamma delta epsilon zeta eta theta iota kappa";
        let result = generate_snippet(text, "zzzznotfound", 3, "<b>", "</b>");
        // Should return the first few words
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
        // 10,000 word text
        let text = "word ".repeat(10_000);
        let result = highlight_text(&text, "word", "<em>", "</em>");
        // Should complete without timeout and contain highlights
        assert!(result.contains("<em>word</em>"));
    }
}

// ============================================================================
// 20. Error: all 13 variant constructors + is_* checks
// ============================================================================
mod error_exhaustive_variants {
    use tideorm::error::Error;

    #[test]
    fn test_all_error_variants_have_unique_codes() {
        let errors: Vec<Error> = vec![
            Error::not_found("test"),
            Error::connection("test"),
            Error::query("test"),
            Error::validation("f", "test"),
            Error::conversion("test"),
            Error::transaction("test"),
            Error::configuration("test"),
            Error::internal("test"),
            Error::backend_not_supported("test", "pg"),
            Error::primary_key_not_set("test", "User"),
            Error::insert_returning_not_supported("test", "mysql"),
            Error::tokenization("test"),
            Error::invalid_token("test"),
        ];

        let codes: Vec<&str> = errors.iter().map(|e| e.code()).collect();
        let unique: std::collections::HashSet<&&str> = codes.iter().collect();
        assert_eq!(codes.len(), unique.len(), "all error codes must be unique");
    }

    #[test]
    fn test_all_error_http_statuses_are_valid() {
        let errors: Vec<Error> = vec![
            Error::not_found("test"),
            Error::connection("test"),
            Error::query("test"),
            Error::validation("f", "test"),
            Error::conversion("test"),
            Error::transaction("test"),
            Error::configuration("test"),
            Error::internal("test"),
            Error::backend_not_supported("test", "pg"),
            Error::primary_key_not_set("test", "User"),
            Error::insert_returning_not_supported("test", "mysql"),
            Error::tokenization("test"),
            Error::invalid_token("test"),
        ];

        for err in &errors {
            let status = err.http_status();
            assert!(
                (100..600).contains(&status),
                "error code {} has invalid HTTP status {}",
                err.code(),
                status
            );
        }
    }

    #[test]
    fn test_all_suggestions_are_nonempty() {
        let errors: Vec<Error> = vec![
            Error::not_found("find user"),
            Error::connection("Connection refused"),
            Error::query("syntax error"),
            Error::validation("email", "invalid"),
            Error::conversion("type mismatch"),
            Error::transaction("timeout"),
            Error::configuration("not set"),
            Error::internal("oops"),
            Error::backend_not_supported("arrays", "SQLite"),
            Error::primary_key_not_set("missing pk", "Post"),
            Error::insert_returning_not_supported("no returning", "MySQL"),
            Error::tokenization("encode failed"),
            Error::invalid_token("tampered"),
        ];

        for err in &errors {
            let suggestion = err.suggestion();
            assert!(
                !suggestion.is_empty(),
                "{} should have a suggestion",
                err.code()
            );
        }
    }
}

// ============================================================================
// 21. Error: retryable classification edge cases
// ============================================================================
mod error_retryable_edge_cases {
    use tideorm::error::Error;

    #[test]
    fn test_connection_pool_is_retryable() {
        assert!(Error::connection("pool exhausted").is_retryable());
    }

    #[test]
    fn test_connection_refused_is_retryable() {
        assert!(Error::connection("Connection refused by host").is_retryable());
    }

    #[test]
    fn test_query_lock_timeout_is_retryable() {
        assert!(Error::query("lock wait timeout exceeded").is_retryable());
    }

    #[test]
    fn test_transaction_serialization_is_retryable() {
        assert!(Error::transaction("serialization failure").is_retryable());
    }

    #[test]
    fn test_validation_not_retryable() {
        assert!(!Error::validation("email", "invalid").is_retryable());
    }

    #[test]
    fn test_configuration_not_retryable() {
        assert!(!Error::configuration("missing key").is_retryable());
    }

    #[test]
    fn test_backend_not_supported_not_retryable() {
        assert!(!Error::backend_not_supported("arrays", "sqlite").is_retryable());
    }
}

// ============================================================================
// 22. Error: with_context only applies to NotFound and Query
// ============================================================================
mod error_context_application {
    use tideorm::error::{Error, ErrorContext};

    #[test]
    fn test_with_context_on_not_found() {
        let ctx = ErrorContext::new().table("users").column("id");
        let err = Error::not_found("user 42").with_context(ctx);
        assert!(err.context().is_some());
        assert_eq!(err.context().unwrap().table.as_deref(), Some("users"));
    }

    #[test]
    fn test_with_context_on_query() {
        let ctx = ErrorContext::new().query("SELECT 1");
        let err = Error::query("bad sql").with_context(ctx);
        assert!(err.context().is_some());
    }

    #[test]
    fn test_with_context_on_other_variants_is_noop() {
        let ctx = ErrorContext::new().table("users");
        let err = Error::connection("refused").with_context(ctx);
        // Connection errors don't carry context
        assert!(err.context().is_none());
    }

    #[test]
    fn test_error_context_display() {
        let ctx = ErrorContext::new()
            .table("orders")
            .column("total")
            .query("UPDATE orders SET total = -1");
        let display = format!("{}", ctx);
        assert!(display.contains("table: orders"));
        assert!(display.contains("column: total"));
        assert!(display.contains("query: UPDATE"));
    }
}

// ============================================================================
// 23. DatabaseType: Default trait gives Postgres
// ============================================================================
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
        let b = a; // Copy
        assert_eq!(a, b);
    }

    #[test]
    fn test_database_type_debug_format() {
        let debug = format!("{:?}", DatabaseType::MariaDB);
        assert_eq!(debug, "MariaDB");
    }
}

// ============================================================================
// 25. Profiling: Profiler inactive after stop
// ============================================================================
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

// ============================================================================
// 26. Logging: QueryTimer lifecycle
// ============================================================================
mod logging_query_timer {
    use std::time::Duration;
    use tideorm::logging::QueryTimer;

    #[test]
    fn test_query_timer_stop_returns_duration() {
        let timer = QueryTimer::start("SELECT 1");
        std::thread::sleep(Duration::from_millis(10));
        let duration = timer.stop();
        assert!(duration >= Duration::from_millis(5));
    }

    #[test]
    fn test_query_timer_finish_creates_entry() {
        let timer = QueryTimer::start("INSERT INTO users VALUES (1)").with_table("users");
        std::thread::sleep(Duration::from_millis(5));
        let entry = timer.finish();
        assert_eq!(entry.table, Some("users".to_string()));
        assert!(entry.duration.is_some());
        assert!(entry.success);
    }

    #[test]
    fn test_query_timer_finish_with_error() {
        let timer = QueryTimer::start("BAD SQL");
        let entry = timer.finish_with_error("syntax error near BAD");
        assert!(!entry.success);
        assert_eq!(entry.error, Some("syntax error near BAD".to_string()));
    }

    #[test]
    fn test_query_timer_finish_with_rows() {
        let timer = QueryTimer::start("SELECT * FROM users");
        let entry = timer.finish_with_rows(42);
        assert_eq!(entry.rows, Some(42));
    }
}

// ============================================================================
// 27. Cache: empty results caching toggle
// ============================================================================
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
        // Should NOT have stored it
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

// ============================================================================
// 28. Error: log_format includes all sections
// ============================================================================
mod error_log_format_comprehensive {
    use tideorm::error::{Error, ErrorContext};

    #[test]
    fn test_log_format_with_full_context() {
        let err = Error::query_with_context(
            "column \"xyz\" does not exist",
            ErrorContext::new()
                .table("users")
                .column("xyz")
                .query("SELECT xyz FROM users"),
        );

        let log = err.log_format();
        assert!(log.contains("[TIDE_QUERY]"));
        assert!(log.contains("column \"xyz\" does not exist"));
        assert!(log.contains("Table: users"));
        assert!(log.contains("Column: xyz"));
        assert!(log.contains("Query: SELECT xyz FROM users"));
        assert!(log.contains("Suggestion:"));
    }

    #[test]
    fn test_log_format_without_context() {
        let err = Error::internal("something broke");
        let log = err.log_format();
        assert!(log.contains("[TIDE_INTERNAL]"));
        assert!(log.contains("something broke"));
        assert!(log.contains("Suggestion:"));
        // Should not contain Table: or Column: lines
        assert!(!log.contains("Table:"));
    }
}
