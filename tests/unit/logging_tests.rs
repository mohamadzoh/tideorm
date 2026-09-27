use std::time::Duration;

use super::*;

#[test]
fn test_query_log_entry() {
    let entry = QueryLogEntry::new("SELECT * FROM users")
        .with_table("users")
        .with_duration(Duration::from_millis(50))
        .with_rows(10);

    assert_eq!(entry.operation, QueryOperation::Select);
    assert_eq!(entry.table, Some("users".to_string()));
    assert_eq!(entry.rows, Some(10));
    assert!(entry.success);
}

#[test]
fn test_query_stats() {
    let stats = QueryStats {
        total_queries: 100,
        slow_queries: 5,
        total_time_ns: 500_000_000,
        slow_threshold_ms: 100,
    };

    assert_eq!(stats.total_time(), Duration::from_millis(500));
    assert_eq!(stats.avg_query_time(), Duration::from_millis(5));
    assert_eq!(stats.slow_percentage(), 5.0);
}

#[test]
fn test_logger_records_duration_and_failure_from_a_finished_timer() {
    QueryLogger::clear_history();
    QueryLogger::reset_stats();
    QueryLogger::disable();

    QueryLogger::global()
        .set_level(LogLevel::Error)
        .set_slow_query_threshold_ms(1)
        .set_history_limit(10)
        .enable();

    let timer = QueryTimer::start("SELECT * FROM users").with_table("users");
    std::thread::sleep(Duration::from_millis(5));
    QueryLogger::log(timer.finish_with_error("connection reset"));

    let history = QueryLogger::history();
    assert_eq!(history.len(), 1);
    assert!(
        !history[0].success,
        "a failed query must be recorded as failed"
    );
    assert!(
        history[0].duration.is_some(),
        "a finished query must carry its measured duration"
    );

    let stats = QueryLogger::stats();
    assert!(stats.total_time() >= Duration::from_millis(5));
    assert_eq!(stats.slow_queries, 1);
    assert_eq!(QueryLogger::slow_queries().len(), 1);

    QueryLogger::clear_history();
    QueryLogger::reset_stats();
    QueryLogger::global()
        .set_slow_query_threshold_ms(100)
        .set_history_limit(100)
        .disable();
}

#[test]
fn test_builder_disable_still_applies_its_settings() {
    QueryLogger::clear_history();
    QueryLogger::global()
        .set_level(LogLevel::Debug)
        .set_history_limit(1)
        .enable();

    QueryLogger::global().set_history_limit(3).disable();
    assert!(!QueryLogger::is_enabled());

    QueryLogger::enable();
    for sql in ["SELECT 1", "SELECT 2", "SELECT 3"] {
        QueryLogger::log(QueryLogEntry::new(sql));
    }
    assert_eq!(
        QueryLogger::history().len(),
        3,
        "the history limit set on a disabling builder must be kept"
    );

    QueryLogger::clear_history();
    QueryLogger::reset_stats();
    QueryLogger::global().set_history_limit(100).disable();
}

#[test]
fn test_stats_keep_sub_millisecond_query_time() {
    QueryLogger::reset_stats();
    QueryLogger::global().set_level(LogLevel::Error).enable();

    QueryLogger::log(QueryLogEntry::new("SELECT 1").with_duration(Duration::from_micros(400)));
    QueryLogger::log(QueryLogEntry::new("SELECT 2").with_duration(Duration::from_micros(600)));

    let stats = QueryLogger::stats();
    assert_eq!(stats.total_queries, 2);
    assert_eq!(stats.total_time(), Duration::from_millis(1));
    assert_eq!(stats.avg_query_time(), Duration::from_micros(500));

    QueryLogger::clear_history();
    QueryLogger::reset_stats();
    QueryLogger::disable();
}

#[test]
fn test_query_history_limit_drops_oldest_entries() {
    QueryLogger::clear_history();
    QueryLogger::reset_stats();
    QueryLogger::disable();

    QueryLogger::global()
        .set_level(LogLevel::Debug)
        .set_history_limit(2)
        .enable();

    QueryLogger::log(QueryLogEntry::new("SELECT 1"));
    QueryLogger::log(QueryLogEntry::new("SELECT 2"));
    QueryLogger::log(QueryLogEntry::new("SELECT 3"));

    let history = QueryLogger::history();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].sql, "SELECT 2");
    assert_eq!(history[1].sql, "SELECT 3");

    QueryLogger::clear_history();
    QueryLogger::global().set_history_limit(100).disable();
}

#[test]
fn test_query_history_limit_zero_keeps_no_entries() {
    QueryLogger::clear_history();
    QueryLogger::reset_stats();
    QueryLogger::disable();

    QueryLogger::global()
        .set_level(LogLevel::Debug)
        .set_history_limit(0)
        .enable();

    QueryLogger::log(QueryLogEntry::new("SELECT 1"));

    assert!(QueryLogger::history().is_empty());

    QueryLogger::clear_history();
    QueryLogger::global().set_history_limit(100).disable();
}

#[test]
fn test_query_debug_display_labels_preview_as_non_executable() {
    let debug_info = QueryDebugInfo::new("users")
        .with_sql("-- DEBUG PREVIEW (not executable, values are approximate)\nSELECT * FROM \"users\" WHERE \"id\" = 1");

    let rendered = format!("{}", debug_info);

    assert!(rendered.contains("SQL Preview (non-executable):"));
    assert!(rendered.contains("-- DEBUG PREVIEW (not executable, values are approximate)"));
    assert!(!rendered.contains("\nSQL: -- DEBUG PREVIEW"));
}

#[test]
fn test_query_logger_timing_flag_is_applied() {
    QueryLogger::global().enable_timing(false).enable();
    assert!(!super::logger::QueryLogger::timing_enabled());

    QueryLogger::global().enable_timing(true).enable();
    assert!(super::logger::QueryLogger::timing_enabled());

    QueryLogger::disable();
}

#[test]
fn test_slow_query_format_omits_fake_zero_ms_when_timing_is_hidden() {
    let formatted = super::format::format_slow(&QueryLogEntry::new("SELECT 1"), 100);

    assert!(formatted.contains("exceeded 100ms threshold"));
    assert!(!formatted.contains("0ms > 100ms threshold"));
}
