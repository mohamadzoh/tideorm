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
        assert!(profiler.elapsed() >= Duration::from_millis(5));
    }
}

mod profiling_query_analyzer {
    use tideorm::profiling::{QueryAnalyzer, QueryComplexity, SuggestionLevel};

    #[test]
    fn test_analyzer_flags_patterns_that_defeat_an_index() {
        for (sql, title) in [
            (
                "SELECT * FROM users WHERE name LIKE '%john'",
                "Leading wildcard in LIKE",
            ),
            (
                "SELECT * FROM users WHERE id NOT IN (1,2,3)",
                "NOT IN detected",
            ),
            (
                "SELECT * FROM users WHERE LOWER(email) = 'a@b.com'",
                "Function in WHERE clause",
            ),
            (
                "SELECT id FROM users ORDER BY created_at",
                "ORDER BY without LIMIT",
            ),
        ] {
            let suggestions = QueryAnalyzer::analyze(sql);
            assert!(
                suggestions.iter().any(|s| s.title == title),
                "{sql}: {suggestions:?}"
            );
        }
    }

    #[test]
    fn test_missing_where_on_update_is_critical() {
        let suggestions = QueryAnalyzer::analyze("UPDATE users SET active = false");
        assert!(
            suggestions
                .iter()
                .any(|s| s.level == SuggestionLevel::Critical && s.title == "Missing WHERE clause"),
            "{suggestions:?}"
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
    fn test_report_display_has_its_header_and_totals() {
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
        assert_eq!(stats.avg_query_time(), Duration::from_millis(85));

        let display = format!("{}", stats);
        assert!(display.contains("Total Queries:"));
        assert!(display.contains("Slow Queries:"));

        GlobalProfiler::set_slow_threshold(100);
        GlobalProfiler::disable();
    }
}
