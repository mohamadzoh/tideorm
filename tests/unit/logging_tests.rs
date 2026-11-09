use std::time::Duration;

use super::*;

#[test]
fn test_query_stats() {
    let stats = QueryStats {
        total_queries: 100,
        slow_queries: 5,
        total_time_ns: 500_000_000,
        slow_threshold: Duration::from_millis(100),
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
        .level(LogLevel::Error)
        .slow_query_threshold(std::time::Duration::from_millis(1))
        .history_limit(10)
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
        .slow_query_threshold(std::time::Duration::from_millis(100))
        .history_limit(100)
        .disable();
}

#[test]
fn test_builder_disable_still_applies_its_settings() {
    QueryLogger::clear_history();
    QueryLogger::global()
        .level(LogLevel::Debug)
        .history_limit(1)
        .enable();

    QueryLogger::global().history_limit(3).disable();
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
    QueryLogger::global().history_limit(100).disable();
}

#[test]
fn test_stats_keep_sub_millisecond_query_time() {
    QueryLogger::reset_stats();
    QueryLogger::global().level(LogLevel::Error).enable();

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
    for (limit, kept) in [(2, vec!["SELECT 2", "SELECT 3"]), (0, vec![])] {
        QueryLogger::clear_history();
        QueryLogger::reset_stats();
        QueryLogger::global()
            .level(LogLevel::Debug)
            .history_limit(limit)
            .enable();

        for sql in ["SELECT 1", "SELECT 2", "SELECT 3"] {
            QueryLogger::log(QueryLogEntry::new(sql));
        }

        let history: Vec<String> = QueryLogger::history()
            .into_iter()
            .map(|entry| entry.sql)
            .collect();
        assert_eq!(history, kept, "limit {limit}");
    }

    QueryLogger::clear_history();
    QueryLogger::reset_stats();
    QueryLogger::global().history_limit(100).disable();
}

#[test]
fn test_query_logger_timing_flag_is_applied() {
    QueryLogger::global().timing(false).enable();
    assert!(!super::logger::QueryLogger::timing_enabled());

    QueryLogger::global().timing(true).enable();
    assert!(super::logger::QueryLogger::timing_enabled());

    QueryLogger::disable();
}

#[test]
fn test_slow_query_format_omits_fake_zero_ms_when_timing_is_hidden() {
    let formatted =
        super::format::format_slow(&QueryLogEntry::new("SELECT 1"), Duration::from_millis(100));

    assert!(formatted.contains("exceeded 100ms threshold"));
    assert!(!formatted.contains("0ms > 100ms threshold"));
}

/// Enabling the logger records history and statistics even at level `Off`,
/// the default, which only silences what is printed.
#[test]
fn an_enabled_logger_records_at_level_off() {
    QueryLogger::clear_history();
    QueryLogger::reset_stats();
    QueryLogger::set_level(LogLevel::Off);
    QueryLogger::enable();

    QueryLogger::log(
        QueryLogEntry::new("SELECT 1".to_string()).with_duration(Duration::from_millis(2)),
    );

    assert_eq!(QueryLogger::history().len(), 1);
    assert_eq!(QueryLogger::stats().total_queries, 1);

    QueryLogger::disable();
    QueryLogger::clear_history();
    QueryLogger::reset_stats();
}

#[test]
fn a_sink_takes_what_the_level_prints_until_the_logger_is_reset() {
    use std::sync::{Arc, Mutex};
    QueryLogger::reset();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    QueryLogger::set_sink(move |entry, text| {
        sink.lock()
            .unwrap()
            .push((entry.sql.clone(), text.to_string()));
    });
    QueryLogger::global().level(LogLevel::Error).enable();
    QueryLogger::log(QueryLogEntry::new("SELECT 1"));
    QueryLogger::log(QueryLogEntry::new("SELECT 2").with_error("boom"));
    {
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].0, "SELECT 2");
        assert!(seen[0].1.contains("failed: boom"), "{seen:?}");
    }
    assert_eq!(QueryLogger::history().len(), 2);

    QueryLogger::reset();
    assert!(QueryLogger::history().is_empty());
    assert_eq!(QueryLogger::stats().total_queries, 0);
    QueryLogger::global().level(LogLevel::Error).enable();
    QueryLogger::log(QueryLogEntry::new("SELECT 3").with_error("on stderr"));
    assert_eq!(seen.lock().unwrap().len(), 1, "the sink was cleared");
    QueryLogger::reset();
}

#[test]
fn apply_changes_settings_but_not_whether_logging_runs() {
    QueryLogger::reset();
    QueryLogger::disable();
    QueryLogger::global()
        .slow_query_threshold(Duration::from_micros(500))
        .apply();
    assert!(!QueryLogger::is_enabled());
    assert_eq!(
        QueryLogger::slow_query_threshold(),
        Duration::from_micros(500)
    );

    // A threshold below a millisecond is kept, not rounded to zero.
    QueryLogger::enable();
    for micros in [700, 300] {
        QueryLogger::log(
            QueryLogEntry::new("SELECT 1").with_duration(Duration::from_micros(micros)),
        );
    }
    assert_eq!(QueryLogger::stats().slow_queries, 1);
    assert_eq!(QueryLogger::slow_queries().len(), 1);
    QueryLogger::reset();
}
