//! Logging and profiling for every statement the ORM engine runs.
//!
//! The engine reports each statement to a callback installed on each
//! connection, from inside the poll that completes the statement, with its
//! duration, error and row count. Every one is timed for the
//! [`GlobalProfiler`]. The query builder logs its statements with more
//! context, so it marks them not to be logged here ([`EngineLog::Off`]), as
//! TideORM does its own catalog probes.

use super::{QueryLogEntry, QueryLogger, query_logging_enabled};
use crate::internal::OrmStatement;
use crate::profiling::GlobalProfiler;

/// Whether the engine's callback logs a statement TideORM runs; it times it
/// either way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EngineLog {
    /// It does, as it does any statement.
    On,
    /// It does not: the caller logs the statement with more context, or it is
    /// a probe of TideORM's own.
    Off,
}

impl EngineLog {
    /// `statement`, marked not to be logged by the engine's callback when this
    /// is `Off`.
    pub(crate) fn apply(self, statement: OrmStatement) -> OrmStatement {
        match self {
            Self::On => statement,
            Self::Off => statement.unlogged(),
        }
    }
}

/// The per-statement callback installed on every connection: times the
/// statement into the [`GlobalProfiler`] and, unless it is marked not to be
/// logged, prints it for `TIDE_LOG_QUERIES` and hands it to the
/// [`QueryLogger`] once it has run.
pub(crate) fn observe_engine_statement(info: &crate::orm::metric::Info<'_>) {
    GlobalProfiler::record(info.elapsed);
    let print = query_logging_enabled();
    if !info.statement.log || (!print && !QueryLogger::is_enabled()) {
        return;
    }

    if print {
        crate::tide_debug!("Query: {}", info.statement.sql);
    }
    let mut entry = QueryLogEntry::new(info.statement.sql.clone()).with_duration(info.elapsed);
    if let Some(rows) = info.rows {
        entry = entry.with_rows(rows);
    }
    if let Some(error) = info.error {
        entry = entry.with_error(error.to_string());
    }
    QueryLogger::log(entry);
}

#[cfg(test)]
#[path = "../../tests/unit/logging_engine_tests.rs"]
mod tests;
