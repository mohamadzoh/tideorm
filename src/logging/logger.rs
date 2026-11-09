use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use super::entry::{LogLevel, QueryLogEntry, QueryStats, StatsCounters};
use super::format::{format_debug, format_error, format_slow, format_summary};

/// The slow-query threshold the logger and the profiler both start from.
const DEFAULT_SLOW_QUERY_THRESHOLD: Duration = Duration::from_millis(100);

/// How many entries the history keeps until a limit is set.
const DEFAULT_HISTORY_LIMIT: usize = 100;

/// `TIDE_SLOW_QUERY_MS` as `var` reads it, or the default threshold.
fn slow_query_threshold_from(var: impl Fn(&str) -> Option<String>) -> Duration {
    var("TIDE_SLOW_QUERY_MS")
        .and_then(|value| value.parse().ok())
        .map_or(DEFAULT_SLOW_QUERY_THRESHOLD, Duration::from_millis)
}

/// The slow-query threshold `TIDE_SLOW_QUERY_MS` sets, or the default; the
/// logger and the global profiler both start from it.
pub(crate) fn slow_query_threshold_from_env() -> Duration {
    slow_query_threshold_from(|name| std::env::var(name).ok())
}

/// Where [`QueryLogger`] sends what it prints: the entry, and the text its
/// level renders for it.
pub type QueryLogSink = Arc<dyn Fn(&QueryLogEntry, &str) + Send + Sync>;

/// Process-wide logger state, built from the environment on first use so that
/// `TIDE_LOG_*` applies before anything else and settings made in code land on
/// top of it.
static STATE: LazyLock<LoggerState> = LazyLock::new(LoggerState::from_env);

struct LoggerState {
    enabled: AtomicBool,
    timing: AtomicBool,
    counters: StatsCounters,
    /// `None` until a level is set in code or through `TIDE_LOG_LEVEL`.
    level: RwLock<Option<LogLevel>>,
    history: RwLock<VecDeque<QueryLogEntry>>,
    history_limit: RwLock<usize>,
    /// `None` prints to stderr.
    sink: RwLock<Option<QueryLogSink>>,
}

impl LoggerState {
    /// Read `TIDE_LOG_QUERIES`, `TIDE_LOG_LEVEL`, and `TIDE_SLOW_QUERY_MS`.
    fn from_env() -> Self {
        Self::from_vars(|name| std::env::var(name).ok())
    }

    /// Build the state from `TIDE_LOG_LEVEL` and `TIDE_SLOW_QUERY_MS` as
    /// `var` reads them, and `TIDE_LOG_QUERIES` from the environment. A level
    /// the logger does not know turns it off.
    fn from_vars(var: impl Fn(&str) -> Option<String>) -> Self {
        let level = var("TIDE_LOG_LEVEL").map(|value| value.parse().unwrap_or(LogLevel::Off));
        let enabled =
            super::query_logging_enabled() || level.is_some_and(|level| level != LogLevel::Off);

        Self {
            enabled: AtomicBool::new(enabled),
            timing: AtomicBool::new(true),
            counters: StatsCounters::new(slow_query_threshold_from(var)),
            level: RwLock::new(level),
            history: RwLock::new(VecDeque::new()),
            history_limit: RwLock::new(DEFAULT_HISTORY_LIMIT),
            sink: RwLock::new(None),
        }
    }
}

/// Query logger for debugging and performance monitoring
pub struct QueryLogger;

impl QueryLogger {
    /// Start configuring the global query logger; nothing changes until the
    /// builder is [`enable`](QueryLoggerBuilder::enable)d,
    /// [`disable`](QueryLoggerBuilder::disable)d or
    /// [`apply`](QueryLoggerBuilder::apply)'d.
    pub fn global() -> QueryLoggerBuilder {
        QueryLoggerBuilder::default()
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

    /// The duration at or above which a query counts as slow.
    pub fn slow_query_threshold() -> Duration {
        STATE.counters.slow_threshold()
    }

    /// Send what the logger prints to `sink` instead of stderr, such as a
    /// `tracing` event or a test's buffer. The sink receives each entry the
    /// level prints, with the text it renders for it; it runs on the thread
    /// that executed the statement, so keep it quick.
    ///
    /// ```
    /// use tideorm::logging::{LogLevel, QueryLogger};
    ///
    /// QueryLogger::set_sink(|entry, text| {
    ///     if !entry.success {
    ///         eprintln!("query failed: {text}");
    ///     }
    /// });
    /// QueryLogger::global().level(LogLevel::Error).enable();
    /// # QueryLogger::reset();
    /// ```
    pub fn set_sink(sink: impl Fn(&QueryLogEntry, &str) + Send + Sync + 'static) {
        *STATE.sink.write() = Some(Arc::new(sink));
    }

    /// Print to stderr again.
    pub fn clear_sink() {
        *STATE.sink.write() = None;
    }

    /// Put the logger back as the environment configures it, as on first
    /// use: `TIDE_LOG_LEVEL` and `TIDE_SLOW_QUERY_MS` read again, counters and
    /// history emptied, the history limit at 100, timing shown, and output on
    /// stderr. For tests that change the global logger.
    pub fn reset() {
        let fresh = LoggerState::from_env();
        STATE
            .enabled
            .store(fresh.enabled.into_inner(), Ordering::SeqCst);
        STATE.timing.store(true, Ordering::SeqCst);
        STATE.counters.reset();
        STATE
            .counters
            .set_slow_threshold(fresh.counters.slow_threshold());
        *STATE.level.write() = fresh.level.into_inner();
        STATE.history.write().clear();
        *STATE.history_limit.write() = DEFAULT_HISTORY_LIMIT;
        *STATE.sink.write() = None;
    }

    /// Record one query entry and update counters while logging is enabled,
    /// and print it if the level allows: the level, `Off` until one is set,
    /// decides only what is printed, so `QueryLogger::enable()` or
    /// `TIDE_LOG_QUERIES` alone still keeps the history and statistics.
    pub fn log(mut entry: QueryLogEntry) {
        if !Self::is_enabled() {
            return;
        }

        let threshold = STATE.counters.slow_threshold();
        let is_slow = STATE.counters.record(entry.duration);

        if let Some(output) = Self::render_for_level(&mut entry, is_slow, threshold) {
            // Cloned out, so a sink that calls back into the logger finds no
            // lock held.
            let sink = STATE.sink.read().clone();
            match sink {
                Some(sink) => sink(&entry, &output),
                None => eprintln!("{}", output),
            }
        }

        let mut history = STATE.history.write();
        let limit = *STATE.history_limit.read();
        if limit == 0 {
            history.clear();
        } else {
            while history.len() >= limit {
                history.pop_front();
            }
            history.push_back(entry);
        }
    }

    /// What the current level prints for `entry`, without its duration when
    /// timing is hidden; the entry keeps its duration for the history.
    fn render_for_level(
        entry: &mut QueryLogEntry,
        is_slow: bool,
        threshold: Duration,
    ) -> Option<String> {
        let level = Self::level();
        if level == LogLevel::Off {
            return None;
        }
        let duration = entry.duration;
        if !Self::timing_enabled() {
            entry.duration = None;
        }
        let output = render(level, entry, is_slow, threshold);
        entry.duration = duration;
        output
    }

    /// Snapshot the current aggregate query counters.
    pub fn stats() -> QueryStats {
        STATE.counters.snapshot()
    }

    /// Clear the aggregate query counters.
    pub fn reset_stats() {
        STATE.counters.reset();
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
        let threshold = STATE.counters.slow_threshold();
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
fn render(
    level: LogLevel,
    entry: &QueryLogEntry,
    is_slow: bool,
    threshold: Duration,
) -> Option<String> {
    let output = match level {
        LogLevel::Off => return None,
        LogLevel::Error if entry.success => return None,
        LogLevel::Warn if entry.success && !is_slow => return None,
        LogLevel::Trace => entry.format_console(),
        // A failure reads as one at every level, slow or not.
        _ if !entry.success => format_error(entry),
        LogLevel::Debug => format_debug(entry),
        _ if is_slow => format_slow(entry, threshold),
        _ => format_summary(entry),
    };
    Some(output)
}

/// Builder for configuring the query logger, from [`QueryLogger::global`].
///
/// Settings left unset keep their current value.
///
/// ```
/// use std::time::Duration;
/// use tideorm::logging::{LogLevel, QueryLogger};
///
/// QueryLogger::global()
///     .level(LogLevel::Warn)
///     .slow_query_threshold(Duration::from_millis(250))
///     .history_limit(500)
///     .enable();
/// # QueryLogger::reset();
/// ```
#[derive(Debug, Default)]
#[must_use = "a logger builder changes nothing until it is enabled, disabled or applied"]
pub struct QueryLoggerBuilder {
    level: Option<LogLevel>,
    timing: Option<bool>,
    threshold: Option<Duration>,
    history_limit: Option<usize>,
}

impl QueryLoggerBuilder {
    /// Set the log level to apply.
    pub fn level(mut self, level: LogLevel) -> Self {
        self.level = Some(level);
        self
    }

    /// Control whether timing data should be emitted with log output.
    pub fn timing(mut self, enable: bool) -> Self {
        self.timing = Some(enable);
        self
    }

    /// Set the duration at or above which a query counts as slow.
    pub fn slow_query_threshold(mut self, threshold: Duration) -> Self {
        self.threshold = Some(threshold);
        self
    }

    /// Cap how many recent queries are kept in memory; `0` keeps none.
    pub fn history_limit(mut self, limit: usize) -> Self {
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

    /// Apply the builder settings, leaving logging enabled or disabled as it is.
    pub fn apply(self) {
        if let Some(level) = self.level {
            QueryLogger::set_level(level);
        }
        if let Some(timing) = self.timing {
            STATE.timing.store(timing, Ordering::SeqCst);
        }
        if let Some(threshold) = self.threshold {
            STATE.counters.set_slow_threshold(threshold);
        }
        if let Some(limit) = self.history_limit {
            *STATE.history_limit.write() = limit;
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/logging_logger_tests.rs"]
mod tests;
