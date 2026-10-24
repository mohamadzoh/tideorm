use super::{Backend, can_rollback_leaked_transaction};
use super::{Error, LEAKED_TRANSACTION_MESSAGE, LeakedRollback, leaked_transaction_error};

/// A leaked top-level transaction is rolled back explicitly where the driver
/// resynchronizes the connection afterwards; SQLite and an unknown backend
/// keep the drop-time rollback. A leaked savepoint never issues a bare
/// `ROLLBACK`, which would abort the enclosing transaction.
#[test]
fn leaked_transactions_are_rolled_back_explicitly_only_where_that_is_safe() {
    for (backend, savepoint, expected) in [
        (Some(Backend::Postgres), false, true),
        (Some(Backend::MySql), false, true),
        (Some(Backend::Sqlite), false, false),
        (None, false, false),
        (Some(Backend::Postgres), true, false),
        (Some(Backend::MySql), true, false),
        (Some(Backend::Sqlite), true, false),
        (None, true, false),
    ] {
        assert_eq!(
            can_rollback_leaked_transaction(backend, savepoint),
            expected,
            "{backend:?}, savepoint: {savepoint}"
        );
    }
}

#[test]
fn the_leak_report_names_the_rollback_outcome_and_keeps_the_closure_error() {
    for (rollback, closure_error, fragments) in [
        (
            LeakedRollback::RolledBack,
            None,
            vec!["rolled back explicitly"],
        ),
        (
            LeakedRollback::DeferredToDrop,
            None,
            vec!["when the stray handle drops"],
        ),
        // A rejected rollback is never swallowed.
        (
            LeakedRollback::Failed(Error::connection("connection closed")),
            None,
            vec!["rolling it back failed", "connection closed"],
        ),
        (
            LeakedRollback::RolledBack,
            Some(Error::validation("email", "is required")),
            vec!["rolled back explicitly", "email"],
        ),
    ] {
        let message = leaked_transaction_error(rollback, closure_error).to_string();
        assert!(message.contains(LEAKED_TRANSACTION_MESSAGE), "{message}");
        for fragment in fragments {
            assert!(message.contains(fragment), "{fragment:?}: {message}");
        }
    }
}
