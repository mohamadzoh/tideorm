//! Query Logging and Debugging
//!
//! Use this module when you need to answer questions like:
//! - what SQL TideORM actually executed
//! - which query is slow
//! - which queries failed, and with what error
//!
//! `QueryLogger` controls runtime logging. `QueryBuilder::debug()` is better
//! when you want to inspect one query locally without turning on global logs.
//!
//! Typical workflow:
//! - enable `QueryLogger` globally when you need runtime SQL history or slow-query tracking
//! - use `QueryBuilder::debug()` when you only need to inspect one query locally,
//!   including the values it binds
//!
//! # Log Levels
//!
//! | Level | Output |
//! |-------|--------|
//! | `Off` | No logging |
//! | `Error` | Only errors |
//! | `Warn` | Errors and slow queries |
//! | `Info` | Errors, slow queries, and a one-line summary of every other query |
//! | `Debug` | All queries with timing |
//! | `Trace` | All queries with timing, row counts, and any parameters attached to the entry |
//!
//! # Environment Variables
//!
//! Read once, the first time the logger is used; settings made in code
//! afterwards take precedence.
//!
//! - `TIDE_LOG_QUERIES=true` - Print every statement: query-builder and migration ones before
//!   they run, the typed CRUD paths and raw SQL once they complete
//! - `TIDE_LOG_LEVEL=debug` - Enable logging at this level (off, error, warn, info, debug, trace)
//! - `TIDE_SLOW_QUERY_MS=100` - Slow query threshold in milliseconds
//!
//! `QueryBuilder::debug()` is the better first stop when a query shape looks
//! wrong but you do not want global logging enabled for the whole process.

mod debug;
mod engine;
mod entry;
mod format;
mod logger;

pub use self::debug::QueryDebugInfo;
pub(crate) use self::engine::{log_engine_statement, log_statement, logged_by_caller};
pub use self::entry::{LogLevel, QueryLogEntry, QueryOperation, QueryStats, QueryTimer};
pub(crate) use self::logger::DEFAULT_SLOW_QUERY_THRESHOLD_MS;
pub use self::logger::{QueryLogger, QueryLoggerBuilder};

/// Whether the environment variable `name` is set to `1` or `true` (any case).
///
/// Anything else, including `false` and `0`, leaves the flag off.
pub(crate) fn env_flag_enabled(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
}

/// Whether `TIDE_LOG_QUERIES` asks for executed SQL to be printed.
///
/// Every query asks, so the environment is read once, on first use, as the
/// logger reads `TIDE_LOG_LEVEL` and `TIDE_SLOW_QUERY_MS`.
pub(crate) fn query_logging_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| env_flag_enabled("TIDE_LOG_QUERIES"))
}

/// Whether an internal `tide_*!` message at `level` should be printed.
///
/// Warnings and status messages print unless the logger level was set to
/// `Off` or `Error`. Debug detail prints at `Debug` and `Trace`, or when
/// `TIDE_LOG_QUERIES` asks for executed SQL.
#[doc(hidden)]
pub fn __message_enabled(level: LogLevel) -> bool {
    if level >= LogLevel::Debug {
        QueryLogger::level() >= LogLevel::Debug || query_logging_enabled()
    } else {
        QueryLogger::configured_level().is_none_or(|configured| configured >= LogLevel::Warn)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/logging_tests.rs"]
mod tests;

// Internal structured logging macros: one `[TideORM ...]` prefix for every
// operational message the crate prints. Not part of the public API.

/// Internal info-level log (operational status messages)
#[doc(hidden)]
#[macro_export]
macro_rules! tide_info {
    ($($arg:tt)*) => {
        if $crate::logging::__message_enabled($crate::logging::LogLevel::Info) {
            eprintln!("[TideORM] {}", format_args!($($arg)*));
        }
    };
}

/// Internal warning-level log (non-fatal issues)
#[doc(hidden)]
#[macro_export]
macro_rules! tide_warn {
    ($($arg:tt)*) => {
        if $crate::logging::__message_enabled($crate::logging::LogLevel::Warn) {
            eprintln!("[TideORM WARN] {}", format_args!($($arg)*));
        }
    };
}

/// Internal debug-level log (verbose operational detail)
#[doc(hidden)]
#[macro_export]
macro_rules! tide_debug {
    ($($arg:tt)*) => {
        if $crate::logging::__message_enabled($crate::logging::LogLevel::Debug) {
            eprintln!("[TideORM DEBUG] {}", format_args!($($arg)*));
        }
    };
}
