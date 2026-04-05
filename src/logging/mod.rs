//! Query Logging and Debugging
//!
//! Use this module when you need to answer questions like:
//! - what SQL TideORM actually executed
//! - which query is slow
//! - which parameters were bound on a failing query
//!
//! `QueryLogger` controls runtime logging. `QueryBuilder::debug()` is better
//! when you want to inspect one query locally without turning on global logs.
//!
//! Typical workflow:
//! - enable `QueryLogger` globally when you need runtime SQL history or slow-query tracking
//! - use `QueryBuilder::debug()` when you only need to inspect one query locally
//! - raise the level to `Trace` only when you need bound parameters in the output
//!
//! # Log Levels
//!
//! | Level | Output |
//! |-------|--------|
//! | `Off` | No logging |
//! | `Error` | Only errors |
//! | `Warn` | Errors and slow queries |
//! | `Info` | Errors, slow queries, and query summaries |
//! | `Debug` | All queries with timing |
//! | `Trace` | All queries with parameters and execution plan hints |
//!
//! # Environment Variables
//!
//! - `TIDE_LOG_QUERIES=true` - Enable basic query logging
//! - `TIDE_LOG_LEVEL=debug` - Set log level (off, error, warn, info, debug, trace)
//! - `TIDE_SLOW_QUERY_MS=100` - Slow query threshold in milliseconds
//!
//! `QueryBuilder::debug()` is the better first stop when a query shape looks
//! wrong but you do not want global logging enabled for the whole process.

mod debug;
mod entry;
mod format;
mod logger;

pub use self::debug::QueryDebugInfo;
pub use self::entry::{LogLevel, QueryLogEntry, QueryOperation, QueryStats, QueryTimer};
pub use self::logger::{QueryLogger, QueryLoggerBuilder};

#[cfg(test)]
#[path = "../../tests/unit/logging_tests.rs"]
mod tests;

// ============================================================================
// Internal structured logging macros
// ============================================================================
//
// These replace raw `eprintln!` usage across the crate with a consistent
// format: `[TideORM <LEVEL>] message`.
//
// Not part of the public API — these are `#[doc(hidden)]` helpers used
// internally by sync, migration, seeding, and config modules.

/// Internal info-level log (operational status messages)
#[doc(hidden)]
#[macro_export]
macro_rules! tide_info {
    ($($arg:tt)*) => {
        eprintln!("[TideORM] {}", format!($($arg)*));
    };
}

/// Internal warning-level log (non-fatal issues)
#[doc(hidden)]
#[macro_export]
macro_rules! tide_warn {
    ($($arg:tt)*) => {
        eprintln!("[TideORM WARN] {}", format!($($arg)*));
    };
}

/// Internal debug-level log (verbose operational detail)
#[doc(hidden)]
#[macro_export]
macro_rules! tide_debug {
    ($($arg:tt)*) => {
        eprintln!("[TideORM DEBUG] {}", format!($($arg)*));
    };
}
