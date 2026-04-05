mod profiling_n_plus_one {
    use std::time::Duration;
    use tideorm::profiling::{ProfiledQuery, Profiler};

    #[test]
    fn test_n_plus_one_detection_triggers_above_10_queries() {
        let mut profiler = Profiler::start();

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
        assert!(!suggestions.iter().any(|s| s.contains("N+1")));
    }
}

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
        assert!(matches!(
            complexity,
            QueryComplexity::Complex | QueryComplexity::VeryComplex
        ));
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

        GlobalProfiler::record(Duration::from_millis(10));
        GlobalProfiler::record(Duration::from_millis(100));
        GlobalProfiler::record(Duration::from_millis(30));
        GlobalProfiler::record(Duration::from_millis(200));

        let stats = GlobalProfiler::stats();
        assert_eq!(stats.total_queries, 4);
        assert_eq!(stats.slow_queries, 2);
        assert!((stats.slow_percentage() - 50.0).abs() < 0.1);
        assert!(stats.avg_query_time() > Duration::ZERO);

        let display = format!("{}", stats);
        assert!(display.contains("Total Queries:"));
        assert!(display.contains("Slow Queries:"));

        GlobalProfiler::disable();
    }
}
