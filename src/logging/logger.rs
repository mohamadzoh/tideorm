use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::entry::{LogLevel, QueryLogEntry, QueryStats};
use super::format::{format_debug, format_error, format_slow, format_summary};

/// The slow-query threshold the logger and the profiler both start from.
pub(crate) const DEFAULT_SLOW_QUERY_THRESHOLD_MS: u64 = 100;

/// Process-wide logger state, built from the environment on first use so that
/// `TIDE_LOG_*` applies before anything else and settings made in code land on
/// top of it.
static STATE: LazyLock<LoggerState> = LazyLock::new(LoggerState::from_env);

struct LoggerState {
    enabled: AtomicBool,
    timing: AtomicBool,
    slow_threshold_ms: AtomicU64,
    query_count: AtomicU64,
    slow_query_count: AtomicU64,
    total_time_ns: AtomicU64,
    /// `None` until a level is set in code or through `TIDE_LOG_LEVEL`.
    level: RwLock<Option<LogLevel>>,
    history: RwLock<VecDeque<QueryLogEntry>>,
    history_limit: RwLock<usize>,
}

impl LoggerState {
    /// Read `TIDE_LOG_QUERIES`, `TIDE_LOG_LEVEL`, and `TIDE_SLOW_QUERY_MS`.
    fn from_env() -> Self {
        let level = std::env::var("TIDE_LOG_LEVEL")
            .ok()
            .map(|value| LogLevel::parse_str(&value));
        let enabled =
            super::query_logging_enabled() || level.is_some_and(|level| level != LogLevel::Off);
        let slow_threshold_ms = std::env::var("TIDE_SLOW_QUERY_MS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_SLOW_QUERY_THRESHOLD_MS);

        Self {
            enabled: AtomicBool::new(enabled),
            timing: AtomicBool::new(true),
            slow_threshold_ms: AtomicU64::new(slow_threshold_ms),
            query_count: AtomicU64::new(0),
            slow_query_count: AtomicU64::new(0),
            total_time_ns: AtomicU64::new(0),
            level: RwLock::new(level),
            history: RwLock::new(VecDeque::new()),
            history_limit: RwLock::new(100),
        }
    }
}

/// Query logger for debugging and performance monitoring
pub struct QueryLogger;

impl QueryLogger {
    /// Start configuring the global query logger.
    pub fn global() -> QueryLoggerBuilder {
        QueryLoggerBuilder::new()
    }

    /// Enable logging with the current global settings.
    pub fn enable() {
        STATE.enabled.store(true, Ordering::SeqCst);
    }

    /// Disable logging without clearing counters or history.
    pub fn disable() {
        STATE.enabled.store(false, Ordering::SeqCst);
    }

    /// Return whether global logging is currently active.
    pub fn is_enabled() -> bool {
        STATE.enabled.load(Ordering::SeqCst)
    }

    pub(super) fn timing_enabled() -> bool {
        STATE.timing.load(Ordering::SeqCst)
    }

    /// Return the current global log level; `Off` until one is set.
    pub fn level() -> LogLevel {
        Self::configured_level().unwrap_or_default()
    }

    /// The level set in code or through `TIDE_LOG_LEVEL`, if any.
    pub(super) fn configured_level() -> Option<LogLevel> {
        *STATE.level.read()
    }

    /// Replace the current global log level.
    pub fn set_level(level: LogLevel) {
        *STATE.level.write() = Some(level);
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

        STATE.query_count.fetch_add(1, Ordering::SeqCst);
        if let Some(duration) = entry.duration {
            STATE
                .total_time_ns
                .fetch_add(duration.as_nanos() as u64, Ordering::SeqCst);
        }

        let threshold = STATE.slow_threshold_ms.load(Ordering::SeqCst);
        let is_slow = entry.is_slow(threshold);
        if is_slow {
            STATE.slow_query_count.fetch_add(1, Ordering::SeqCst);
        }

        {
            let mut history = STATE.history.write();
            let limit = *STATE.history_limit.read();
            if limit == 0 {
                history.clear();
            } else {
                while history.len() >= limit {
                    history.pop_front();
                }
                history.push_back(entry.clone());
            }
        }

        let mut entry = entry;
        if !Self::timing_enabled() {
            entry.duration = None;
        }
        if let Some(output) = render(level, &entry, is_slow, threshold) {
            eprintln!("{}", output);
        }
    }

    /// Snapshot the current aggregate query counters.
    pub fn stats() -> QueryStats {
        QueryStats {
            total_queries: STATE.query_count.load(Ordering::SeqCst),
            slow_queries: STATE.slow_query_count.load(Ordering::SeqCst),
            total_time_ns: STATE.total_time_ns.load(Ordering::SeqCst),
            slow_threshold_ms: STATE.slow_threshold_ms.load(Ordering::SeqCst),
        }
    }

    /// Clear the aggregate query counters.
    pub fn reset_stats() {
        STATE.query_count.store(0, Ordering::SeqCst);
        STATE.slow_query_count.store(0, Ordering::SeqCst);
        STATE.total_time_ns.store(0, Ordering::SeqCst);
    }

    /// Return the stored query history as a new vector.
    pub fn history() -> Vec<QueryLogEntry> {
        STATE.history.read().iter().cloned().collect()
    }

    /// Drop all stored history entries.
    pub fn clear_history() {
        STATE.history.write().clear();
    }

    /// Return history entries that meet the current slow-query threshold.
    pub fn slow_queries() -> Vec<QueryLogEntry> {
        let threshold = STATE.slow_threshold_ms.load(Ordering::SeqCst);
        STATE
            .history
            .read()
            .iter()
            .filter(|entry| entry.is_slow(threshold))
            .cloned()
            .collect()
    }
}

/// The output `QueryLogger::log` prints for `entry` at `level`, if any.
///
/// `is_slow` is decided before timing is stripped from `entry`, so a slow
/// query is still reported as slow when durations are hidden.
fn render(level: LogLevel, entry: &QueryLogEntry, is_slow: bool, threshold: u64) -> Option<String> {
    let output = match level {
        LogLevel::Off => return None,
        LogLevel::Error if entry.success => return None,
        LogLevel::Warn if entry.success && !is_slow => return None,
        LogLevel::Trace => entry.format_console(),
        LogLevel::Debug => format_debug(entry),
        _ if is_slow => format_slow(entry, threshold),
        _ if !entry.success => format_error(entry),
        _ => format_summary(entry),
    };
    Some(output)
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

    /// Set the log level to apply.
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
        self.apply();
        QueryLogger::enable();
    }

    /// Apply the builder settings and disable global logging.
    pub fn disable(self) {
        self.apply();
        QueryLogger::disable();
    }

    fn apply(self) {
        if let Some(level) = self.level {
            QueryLogger::set_level(level);
        }
        if let Some(timing) = self.timing {
            STATE.timing.store(timing, Ordering::SeqCst);
        }
        if let Some(ms) = self.threshold_ms {
            STATE.slow_threshold_ms.store(ms, Ordering::SeqCst);
        }
        if let Some(limit) = self.history_limit {
            *STATE.history_limit.write() = limit;
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/logging_logger_tests.rs"]
mod tests;
