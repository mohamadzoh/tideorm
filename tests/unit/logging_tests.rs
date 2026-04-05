use std::time::Duration;

use super::*;

#[test]
fn test_log_level_parsing() {
    assert_eq!(LogLevel::parse_str("debug"), LogLevel::Debug);
    assert_eq!(LogLevel::parse_str("DEBUG"), LogLevel::Debug);
    assert_eq!(LogLevel::parse_str("warn"), LogLevel::Warn);
    assert_eq!(LogLevel::parse_str("4"), LogLevel::Debug);
    assert_eq!(LogLevel::parse_str("invalid"), LogLevel::Off);
}

#[test]
fn test_query_operation_detection() {
    assert_eq!(
        QueryOperation::from_sql("SELECT * FROM users"),
        QueryOperation::Select
    );
    assert_eq!(
        QueryOperation::from_sql("INSERT INTO users"),
        QueryOperation::Insert
    );
    assert_eq!(
        QueryOperation::from_sql("UPDATE users SET"),
        QueryOperation::Update
    );
    assert_eq!(
        QueryOperation::from_sql("DELETE FROM users"),
        QueryOperation::Delete
    );
    assert_eq!(
        QueryOperation::from_sql("BEGIN"),
        QueryOperation::Transaction
    );
}

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
fn test_slow_query_detection() {
    let fast_entry = QueryLogEntry::new("SELECT 1").with_duration(Duration::from_millis(10));

    let slow_entry = QueryLogEntry::new("SELECT 1").with_duration(Duration::from_millis(200));

    assert!(!fast_entry.is_slow(100));
    assert!(slow_entry.is_slow(100));
}

#[test]
fn test_query_stats() {
    let stats = QueryStats {
        total_queries: 100,
        slow_queries: 5,
        total_time_ms: 500,
        threshold_ms: 100,
    };

    assert_eq!(stats.avg_query_time_ms(), 5.0);
    assert_eq!(stats.slow_query_percentage(), 5.0);
}

#[test]
fn test_query_timer() {
    let timer = QueryTimer::start("SELECT * FROM users").with_table("users");

    std::thread::sleep(Duration::from_millis(10));
    let entry = timer.finish_with_rows(5);

    assert!(entry.duration.unwrap() >= Duration::from_millis(10));
    assert_eq!(entry.rows, Some(5));
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
