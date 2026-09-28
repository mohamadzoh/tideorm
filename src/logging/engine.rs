//! Logging for the statements the ORM engine renders itself: the typed `find`,
//! `create`, `update`, `delete` and upsert paths, and raw SQL.
//!
//! The engine reports every prepared statement it runs to a callback installed
//! on each connection, from inside the poll that completes the statement; the
//! unprepared ones `Database::exec_raw` sends it does not report, so that logs
//! its own. The query builder and the migrator log their statements with more
//! context, so they mark the futures that run them and neither logs those.

use std::cell::Cell;
use std::future::Future;
use std::time::Duration;

use super::{QueryLogEntry, QueryLogger, query_logging_enabled};

thread_local! {
    /// Set while a future whose statements its caller logs is polled.
    static LOGGED_BY_CALLER: Cell<bool> = const { Cell::new(false) };
}

/// Run `future`, whose statements the caller logs itself, without the engine
/// callback logging them a second time.
///
/// The flag is set around each `poll` and cleared right after, never held
/// across an await, so it follows the future across a work-stealing runtime's
/// threads the way the scoped connection overrides do.
pub(crate) fn logged_by_caller<F: Future>(future: F) -> impl Future<Output = F::Output> {
    /// Puts the flag back when the poll ends, even by a panic, which would
    /// otherwise leave every later statement on the thread unlogged.
    struct Restore(bool);

    impl Drop for Restore {
        fn drop(&mut self) {
            LOGGED_BY_CALLER.set(self.0);
        }
    }

    crate::internal::per_poll(future, || Restore(LOGGED_BY_CALLER.replace(true)))
}

/// The per-statement callback installed on every connection.
pub(crate) fn log_engine_statement(info: &crate::orm::metric::Info<'_>) {
    log_statement(&info.statement.sql, info.elapsed, info.failed);
}

/// Print `sql` for `TIDE_LOG_QUERIES` and hand it to the [`QueryLogger`] once
/// it has run, unless its caller logs it.
pub(crate) fn log_statement(sql: &str, elapsed: Duration, failed: bool) {
    if LOGGED_BY_CALLER.get() {
        return;
    }
    let print = query_logging_enabled();
    if !print && !QueryLogger::is_enabled() {
        return;
    }

    if print {
        crate::tide_debug!("Query: {}", sql);
    }
    let entry = QueryLogEntry::new(sql.to_string()).with_duration(elapsed);
    QueryLogger::log(if failed {
        entry.with_error("the statement failed")
    } else {
        entry
    });
}

#[cfg(test)]
#[path = "../../tests/unit/logging_engine_tests.rs"]
mod tests;
