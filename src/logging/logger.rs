use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use super::entry::{LogLevel, QueryLogEntry, QueryStats};
use super::format::{format_debug, format_error, format_slow};

/// Global query logger configuration
static LOGGER_ENABLED: AtomicBool = AtomicBool::new(false);
static LOGGER_TIMING: AtomicBool = AtomicBool::new(true);
static SLOW_QUERY_THRESHOLD_MS: AtomicU64 = AtomicU64::new(100);
static QUERY_COUNT: AtomicU64 = AtomicU64::new(0);
static SLOW_QUERY_COUNT: AtomicU64 = AtomicU64::new(0);
static TOTAL_QUERY_TIME_MS: AtomicU64 = AtomicU64::new(0);

static LOG_LEVEL: RwLock<LogLevel> = RwLock::new(LogLevel::Off);
static QUERY_HISTORY: RwLock<VecDeque<QueryLogEntry>> = RwLock::new(VecDeque::new());
static HISTORY_LIMIT: RwLock<usize> = RwLock::new(100);

/// Query logger for debugging and performance monitoring
pub struct QueryLogger;

impl QueryLogger {
    /// Start configuring the global query logger.
    pub fn global() -> QueryLoggerBuilder {
        QueryLoggerBuilder::new()
    }

    /// Enable logging with the current global settings.
    pub fn enable() {
        LOGGER_ENABLED.store(true, Ordering::SeqCst);
    }

    /// Disable logging without clearing counters or history.
    pub fn disable() {
        LOGGER_ENABLED.store(false, Ordering::SeqCst);
    }

    /// Return whether global logging is currently active.
    pub fn is_enabled() -> bool {
        LOGGER_ENABLED.load(Ordering::SeqCst)
    }

    pub(super) fn timing_enabled() -> bool {
        LOGGER_TIMING.load(Ordering::SeqCst)
    }

    /// Return the current global log level.
    pub fn level() -> LogLevel {
        *LOG_LEVEL.read()
    }

    /// Replace the current global log level.
    pub fn set_level(level: LogLevel) {
        *LOG_LEVEL.write() = level;
    }

    /// Record one query entry, update counters, and emit output if the level allows it.
    pub fn log(entry: QueryLogEntry) {
        if !Self::is_enabled() {
            return;
        }

        let level = Self::level();
        if level == LogLevel::Off {
            return;
        }

        QUERY_COUNT.fetch_add(1, Ordering::SeqCst);
        if let Some(duration) = entry.duration {
            TOTAL_QUERY_TIME_MS.fetch_add(duration.as_millis() as u64, Ordering::SeqCst);
        }

        let threshold = SLOW_QUERY_THRESHOLD_MS.load(Ordering::SeqCst);
        let is_slow = entry.is_slow(threshold);
        if is_slow {
            SLOW_QUERY_COUNT.fetch_add(1, Ordering::SeqCst);
        }

        {
            let mut history = QUERY_HISTORY.write();
            let limit = *HISTORY_LIMIT.read();
            if limit == 0 {
                history.clear();
            } else {
                while history.len() >= limit {
                    history.pop_front();
                }
                history.push_back(entry.clone());
            }
        }

        let should_log = match level {
            LogLevel::Off => false,
            LogLevel::Error => !entry.success,
            LogLevel::Warn => !entry.success || is_slow,
            LogLevel::Info => !entry.success || is_slow,
            LogLevel::Debug => true,
            LogLevel::Trace => true,
        };

        if should_log {
            let output_entry = if Self::timing_enabled() {
                entry.clone()
            } else {
                let mut output_entry = entry.clone();
                output_entry.duration = None;
                output_entry
            };

            let output = if level == LogLevel::Trace {
                output_entry.format_console()
            } else if level >= LogLevel::Debug {
                format_debug(&output_entry)
            } else if is_slow {
                format_slow(&output_entry, threshold)
            } else {
                format_error(&output_entry)
            };

            eprintln!("{}", output);
        }
    }

    /// Log a successful query using just SQL and duration.
    pub fn log_timed(sql: impl Into<String>, duration: Duration) {
        if !Self::is_enabled() {
            return;
        }
        let entry = QueryLogEntry::new(sql).with_duration(duration);
        Self::log(entry);
    }

    /// Log a failed query using just SQL and an error message.
    pub fn log_error(sql: impl Into<String>, error: impl Into<String>) {
        if !Self::is_enabled() {
            return;
        }
        let entry = QueryLogEntry::new(sql).with_error(error);
        Self::log(entry);
    }

    /// Snapshot the current aggregate query counters.
    pub fn stats() -> QueryStats {
        QueryStats {
            total_queries: QUERY_COUNT.load(Ordering::SeqCst),
            slow_queries: SLOW_QUERY_COUNT.load(Ordering::SeqCst),
            total_time_ms: TOTAL_QUERY_TIME_MS.load(Ordering::SeqCst),
            threshold_ms: SLOW_QUERY_THRESHOLD_MS.load(Ordering::SeqCst),
        }
    }

    /// Clear the aggregate query counters.
    pub fn reset_stats() {
        QUERY_COUNT.store(0, Ordering::SeqCst);
        SLOW_QUERY_COUNT.store(0, Ordering::SeqCst);
        TOTAL_QUERY_TIME_MS.store(0, Ordering::SeqCst);
    }

    /// Return the stored query history as a new vector.
    pub fn history() -> Vec<QueryLogEntry> {
        QUERY_HISTORY.read().iter().cloned().collect()
    }

    /// Drop all stored history entries.
    pub fn clear_history() {
        QUERY_HISTORY.write().clear();
    }

    /// Return history entries that meet the current slow-query threshold.
    pub fn slow_queries() -> Vec<QueryLogEntry> {
        let threshold = SLOW_QUERY_THRESHOLD_MS.load(Ordering::SeqCst);
        QUERY_HISTORY
            .read()
            .iter()
            .filter(|entry| entry.is_slow(threshold))
            .cloned()
            .collect()
    }

    /// Load logger settings from TIDE_LOG_QUERIES, TIDE_LOG_LEVEL, and TIDE_SLOW_QUERY_MS.
    pub fn init_from_env() {
        if let Ok(val) = std::env::var("TIDE_LOG_QUERIES") {
            if val == "1" || val.to_lowercase() == "true" {
                LOGGER_ENABLED.store(true, Ordering::SeqCst);
            }
        }

        if let Ok(val) = std::env::var("TIDE_LOG_LEVEL") {
            let level = LogLevel::parse_str(&val);
            *LOG_LEVEL.write() = level;
            if level != LogLevel::Off {
                LOGGER_ENABLED.store(true, Ordering::SeqCst);
            }
        }

        if let Ok(val) = std::env::var("TIDE_SLOW_QUERY_MS") {
            if let Ok(ms) = val.parse::<u64>() {
                SLOW_QUERY_THRESHOLD_MS.store(ms, Ordering::SeqCst);
            }
        }
    }
}

/// Builder for configuring the query logger
pub struct QueryLoggerBuilder {
    level: Option<LogLevel>,
    timing: Option<bool>,
    threshold_ms: Option<u64>,
    history_limit: Option<usize>,
}

impl QueryLoggerBuilder {
    fn new() -> Self {
        Self {
            level: None,
            timing: None,
            threshold_ms: None,
            history_limit: None,
        }
    }

    /// Set the log level that will be applied on enable.
    pub fn set_level(mut self, level: LogLevel) -> Self {
        self.level = Some(level);
        self
    }

    /// Control whether timing data should be emitted with log output.
    pub fn enable_timing(mut self, enable: bool) -> Self {
        self.timing = Some(enable);
        self
    }

    /// Set the slow-query threshold in milliseconds.
    pub fn set_slow_query_threshold_ms(mut self, ms: u64) -> Self {
        self.threshold_ms = Some(ms);
        self
    }

    /// Cap how many recent queries are kept in memory.
    pub fn set_history_limit(mut self, limit: usize) -> Self {
        self.history_limit = Some(limit);
        self
    }

    /// Apply the builder settings and enable global logging.
    pub fn enable(self) {
        if let Some(level) = self.level {
            *LOG_LEVEL.write() = level;
        }
        if let Some(timing) = self.timing {
            LOGGER_TIMING.store(timing, Ordering::SeqCst);
        }
        if let Some(ms) = self.threshold_ms {
            SLOW_QUERY_THRESHOLD_MS.store(ms, Ordering::SeqCst);
        }
        if let Some(limit) = self.history_limit {
            *HISTORY_LIMIT.write() = limit;
        }
        LOGGER_ENABLED.store(true, Ordering::SeqCst);
    }

    /// Disable global logging.
    pub fn disable(self) {
        LOGGER_ENABLED.store(false, Ordering::SeqCst);
    }
}
