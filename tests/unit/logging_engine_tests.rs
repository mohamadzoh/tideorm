use std::time::Duration;

use super::*;
use crate::logging::LogLevel;
use crate::orm::metric::Info;
use crate::orm::{DbBackend, Statement};

fn report(statement: &Statement, failed: bool) {
    log_engine_statement(&Info {
        elapsed: Duration::from_millis(2),
        statement,
        failed,
    });
}

#[tokio::test]
async fn engine_statements_reach_the_logger_unless_their_caller_logs_them() {
    QueryLogger::clear_history();
    QueryLogger::global()
        .set_level(LogLevel::Debug)
        .set_history_limit(10)
        .enable();

    let find = Statement::from_string(DbBackend::Sqlite, "SELECT \"id\" FROM \"users\"");
    let insert = Statement::from_string(DbBackend::Sqlite, "INSERT INTO \"users\" DEFAULT VALUES");
    report(&find, false);
    report(&insert, true);
    logged_by_caller(async { report(&find, false) }).await;

    let history = QueryLogger::history();
    QueryLogger::clear_history();
    QueryLogger::disable();

    assert_eq!(
        history.len(),
        2,
        "a caller-logged statement is not logged again"
    );
    assert_eq!(history[0].sql, find.sql);
    assert_eq!(history[0].duration, Some(Duration::from_millis(2)));
    assert!(history[0].success);
    assert!(
        !history[1].success,
        "a failed statement is logged as failed"
    );
}

#[tokio::test]
async fn the_caller_mark_is_cleared_after_each_poll() {
    logged_by_caller(async {
        assert!(LOGGED_BY_CALLER.get());
        tokio::task::yield_now().await;
        assert!(LOGGED_BY_CALLER.get());
    })
    .await;
    assert!(!LOGGED_BY_CALLER.get());
}

#[test]
fn a_statement_that_panics_leaves_the_thread_logging() {
    // Tokio keeps a worker thread after a task panics, so a mark left set
    // there would silence every later statement the thread runs.
    let panicked = std::panic::catch_unwind(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        runtime.block_on(logged_by_caller(async {
            panic!("the statement panicked");
        }));
    });
    assert!(panicked.is_err());
    assert!(!LOGGED_BY_CALLER.get());
}
