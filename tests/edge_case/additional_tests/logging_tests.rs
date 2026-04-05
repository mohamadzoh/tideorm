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

mod logging_entry_formatting {
    use std::time::Duration;
    use tideorm::logging::QueryLogEntry;

    #[test]
    fn test_log_entry_is_slow_with_various_thresholds() {
        let entry = QueryLogEntry::new("SELECT 1").with_duration(Duration::from_millis(150));

        assert!(entry.is_slow(100));
        assert!(entry.is_slow(150));
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

mod logging_stats {
    use tideorm::logging::QueryLogger;

    #[test]
    fn test_query_stats_display_format() {
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
