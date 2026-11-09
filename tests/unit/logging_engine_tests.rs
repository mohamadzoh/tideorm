use std::time::Duration;

use super::*;
use crate::logging::LogLevel;
use crate::orm::metric::Info;
use crate::orm::{DbBackend, DbErr, Statement};
use crate::profiling::GlobalProfiler;

fn report(statement: &Statement, error: Option<&DbErr>, rows: Option<u64>) {
    observe_engine_statement(&Info {
        elapsed: Duration::from_millis(2),
        statement,
        error,
        rows,
    });
}

#[tokio::test]
async fn engine_statements_reach_the_logger_and_the_profiler() {
    QueryLogger::clear_history();
    QueryLogger::global()
        .level(LogLevel::Debug)
        .history_limit(10)
        .enable();
    GlobalProfiler::enable();
    GlobalProfiler::reset();

    let find = Statement::from_string(DbBackend::SQLite, "SELECT \"id\" FROM \"users\"");
    let insert = Statement::from_string(DbBackend::SQLite, "INSERT INTO \"users\" DEFAULT VALUES");
    let probe = Statement::from_string(DbBackend::SQLite, "SELECT 1").unlogged();
    let error = DbErr::Custom("no such table: users".into());
    report(&find, None, Some(3));
    report(&insert, Some(&error), None);
    report(&probe, None, Some(1));

    let history = QueryLogger::history();
    let profiled = GlobalProfiler::stats().total_queries;
    QueryLogger::clear_history();
    QueryLogger::disable();
    GlobalProfiler::disable();
    GlobalProfiler::reset();

    assert_eq!(profiled, 3, "every statement is timed, an unlogged one too");
    assert_eq!(history.len(), 2, "an unlogged statement is not logged");
    assert_eq!(history[0].sql, find.sql);
    assert_eq!(history[0].duration, Some(Duration::from_millis(2)));
    assert_eq!(history[0].rows, Some(3));
    assert!(history[0].success);
    assert!(
        !history[1].success,
        "a failed statement is logged as failed"
    );
    assert!(
        history[1]
            .error
            .as_deref()
            .is_some_and(|message| message.contains("no such table")),
        "{:?}",
        history[1].error
    );
}

#[test]
fn an_unlogged_statement_is_marked_for_the_engine_callback() {
    let statement = Statement::from_string(DbBackend::SQLite, "SELECT 1");
    assert!(EngineLog::On.apply(statement.clone()).log);
    assert!(!EngineLog::Off.apply(statement).log);
}
