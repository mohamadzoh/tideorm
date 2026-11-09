mod logging_operation_detection {
    use tideorm::logging::QueryOperation;

    #[test]
    fn test_operation_comes_from_the_leading_keyword() {
        for (sql, operation) in [
            ("SELECT * FROM users", QueryOperation::Select),
            ("  select * from users", QueryOperation::Select),
            ("INSERT INTO users VALUES (1)", QueryOperation::Insert),
            ("  insert into users", QueryOperation::Insert),
            ("UPDATE users SET name = 'x'", QueryOperation::Update),
            ("DELETE FROM users WHERE id = 1", QueryOperation::Delete),
            ("BEGIN", QueryOperation::Transaction),
            ("COMMIT", QueryOperation::Transaction),
            ("ROLLBACK", QueryOperation::Transaction),
            ("CREATE TABLE foo (id INT)", QueryOperation::Unknown),
        ] {
            assert_eq!(QueryOperation::from_sql(sql), operation, "{sql:?}");
        }
    }

    #[test]
    fn test_operation_display_matches_as_str() {
        for (op, text) in [
            (QueryOperation::Select, "SELECT"),
            (QueryOperation::Insert, "INSERT"),
            (QueryOperation::Update, "UPDATE"),
            (QueryOperation::Delete, "DELETE"),
            (QueryOperation::Transaction, "TRANSACTION"),
            (QueryOperation::Unknown, "UNKNOWN"),
        ] {
            assert_eq!(op.as_str(), text);
            assert_eq!(op.to_string(), text);
        }
    }
}

mod logging_entry_formatting {
    use std::time::Duration;
    use tideorm::logging::QueryLogEntry;

    #[test]
    fn test_log_entry_is_slow_at_or_past_the_threshold() {
        let entry = QueryLogEntry::new("SELECT 1").with_duration(Duration::from_millis(150));

        assert!(entry.is_slow(Duration::from_millis(100)));
        assert!(entry.is_slow(Duration::from_millis(150)));
        assert!(!entry.is_slow(Duration::from_millis(200)));
        assert!(
            !QueryLogEntry::new("SELECT 1").is_slow(Duration::from_millis(0)),
            "an entry with no duration is never slow"
        );
    }

    #[test]
    fn test_log_entry_format_console_includes_sql() {
        let entry = QueryLogEntry::new("SELECT * FROM users WHERE id = 1")
            .with_table("users")
            .with_duration(Duration::from_millis(42))
            .with_rows(3)
            .with_params(vec!["1".to_string()]);

        assert_eq!(
            entry.format_console(),
            "[TideORM][SELECT] users (42ms) [3 rows]\n  SQL: SELECT * FROM users WHERE id = 1\n  Params: [\"1\"]"
        );
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
    use std::time::Duration;
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
        assert_eq!(stats.avg_query_time(), Duration::ZERO);
        assert_eq!(stats.slow_percentage(), 0.0);
    }
}

mod logging_level_parsing {
    use tideorm::logging::LogLevel;

    #[test]
    fn test_log_level_parse_names_and_numbers() {
        for (text, level) in [
            ("off", LogLevel::Off),
            ("none", LogLevel::Off),
            ("0", LogLevel::Off),
            ("error", LogLevel::Error),
            ("1", LogLevel::Error),
            ("warn", LogLevel::Warn),
            ("warning", LogLevel::Warn),
            ("2", LogLevel::Warn),
            ("info", LogLevel::Info),
            ("3", LogLevel::Info),
            ("debug", LogLevel::Debug),
            ("4", LogLevel::Debug),
            ("TRACE", LogLevel::Trace),
            ("all", LogLevel::Trace),
            ("5", LogLevel::Trace),
        ] {
            assert_eq!(text.parse::<LogLevel>().unwrap(), level, "{text:?}");
        }
        // Anything else is refused, naming the text.
        for text in ["verbose", "", "6"] {
            let error = text.parse::<LogLevel>().unwrap_err();
            assert!(
                matches!(error, tideorm::Error::Configuration { .. })
                    && error.to_string().contains(&format!("'{text}'")),
                "{error}"
            );
        }
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
    fn test_query_timer_finish_measures_elapsed_time() {
        let timer = QueryTimer::start("INSERT INTO users VALUES (1)").with_table("users");
        std::thread::sleep(Duration::from_millis(10));
        let entry = timer.finish();
        assert_eq!(entry.table, Some("users".to_string()));
        assert!(entry.success);
        assert!(
            entry
                .duration
                .is_some_and(|duration| duration >= Duration::from_millis(5)),
            "{:?}",
            entry.duration
        );
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
