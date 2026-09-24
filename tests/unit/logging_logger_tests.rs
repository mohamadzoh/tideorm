use std::time::Duration;

use super::*;
use crate::logging::{__message_enabled, query_logging_enabled};

fn fast_select() -> QueryLogEntry {
    QueryLogEntry::new("SELECT * FROM users")
        .with_table("users")
        .with_duration(Duration::from_millis(5))
        .with_rows(3)
}

#[test]
fn info_summarizes_every_query_that_warn_leaves_out() {
    let entry = fast_select();

    assert_eq!(
        render(LogLevel::Info, &entry, false, 100).as_deref(),
        Some("[TideORM][SELECT] users (5ms) [3 rows]")
    );
    assert!(render(LogLevel::Warn, &entry, false, 100).is_none());
    assert!(render(LogLevel::Error, &entry, false, 100).is_none());
    assert!(render(LogLevel::Off, &entry, false, 100).is_none());
    assert!(
        render(LogLevel::Debug, &entry, false, 100)
            .is_some_and(|output| output.contains("SELECT * FROM users"))
    );
}

#[test]
fn failures_and_slow_queries_reach_the_quieter_levels() {
    let failed = fast_select().with_error("boom");
    assert!(
        render(LogLevel::Error, &failed, false, 100)
            .is_some_and(|output| output.contains("failed: boom"))
    );

    let slow = fast_select();
    for level in [LogLevel::Warn, LogLevel::Info] {
        assert!(
            render(level, &slow, true, 1).is_some_and(|output| output.contains("SLOW QUERY")),
            "{level}"
        );
    }
}

#[test]
#[allow(unsafe_code)]
fn state_starts_from_the_documented_environment_variables() {
    // SAFETY: `.cargo/config.toml` runs tests on a single thread, and both
    // variables are removed again right after the state has read them.
    unsafe {
        std::env::set_var("TIDE_LOG_LEVEL", "debug");
        std::env::set_var("TIDE_SLOW_QUERY_MS", "250");
    }
    let configured = LoggerState::from_env();
    unsafe {
        std::env::remove_var("TIDE_LOG_LEVEL");
        std::env::remove_var("TIDE_SLOW_QUERY_MS");
    }

    assert!(configured.enabled.load(Ordering::SeqCst));
    assert_eq!(*configured.level.read(), Some(LogLevel::Debug));
    assert_eq!(configured.slow_threshold_ms.load(Ordering::SeqCst), 250);

    let unset = LoggerState::from_env();
    assert_eq!(*unset.level.read(), None);
    assert_eq!(
        unset.slow_threshold_ms.load(Ordering::SeqCst),
        DEFAULT_SLOW_QUERY_THRESHOLD_MS
    );
}

#[test]
fn internal_messages_follow_the_logger_level() {
    let previous = *STATE.level.read();

    *STATE.level.write() = None;
    assert!(__message_enabled(LogLevel::Warn));
    assert!(__message_enabled(LogLevel::Info));
    assert_eq!(
        __message_enabled(LogLevel::Debug),
        query_logging_enabled(),
        "debug detail stays quiet unless asked for"
    );

    for quiet in [LogLevel::Off, LogLevel::Error] {
        QueryLogger::set_level(quiet);
        assert!(!__message_enabled(LogLevel::Warn), "{quiet}");
        assert!(!__message_enabled(LogLevel::Info), "{quiet}");
    }

    QueryLogger::set_level(LogLevel::Debug);
    assert!(__message_enabled(LogLevel::Debug));
    assert!(__message_enabled(LogLevel::Warn));

    *STATE.level.write() = previous;
}
