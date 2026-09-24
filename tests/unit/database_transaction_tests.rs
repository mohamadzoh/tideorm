use super::{Backend, can_rollback_leaked_transaction};
use super::{Error, LEAKED_TRANSACTION_MESSAGE, LeakedRollback, leaked_transaction_error};

#[test]
fn top_level_leaks_are_rolled_back_where_the_driver_resynchronizes() {
    assert!(can_rollback_leaked_transaction(
        Some(Backend::Postgres),
        false
    ));
    assert!(can_rollback_leaked_transaction(Some(Backend::MySql), false));
}

#[test]
fn sqlite_and_unknown_backends_keep_the_drop_time_rollback() {
    assert!(!can_rollback_leaked_transaction(
        Some(Backend::Sqlite),
        false
    ));
    assert!(!can_rollback_leaked_transaction(None, false));
}

#[test]
fn a_leaked_savepoint_never_issues_a_bare_rollback() {
    for backend in [
        Some(Backend::Postgres),
        Some(Backend::MySql),
        Some(Backend::Sqlite),
        None,
    ] {
        assert!(
            !can_rollback_leaked_transaction(backend, true),
            "a bare ROLLBACK would abort the enclosing transaction: {backend:?}"
        );
    }
}

#[test]
fn the_rollback_outcome_is_part_of_the_reported_leak() {
    let rolled_back = leaked_transaction_error(LeakedRollback::RolledBack, None).to_string();
    assert!(
        rolled_back.contains(LEAKED_TRANSACTION_MESSAGE),
        "{rolled_back}"
    );
    assert!(
        rolled_back.contains("rolled back explicitly"),
        "{rolled_back}"
    );

    let deferred = leaked_transaction_error(LeakedRollback::DeferredToDrop, None).to_string();
    assert!(deferred.contains(LEAKED_TRANSACTION_MESSAGE), "{deferred}");
    assert!(
        deferred.contains("when the stray handle drops"),
        "{deferred}"
    );
}

#[test]
fn a_rejected_rollback_is_never_swallowed() {
    let rollback = LeakedRollback::Failed(Error::connection("connection closed"));
    let message = leaked_transaction_error(rollback, None).to_string();

    assert!(message.contains(LEAKED_TRANSACTION_MESSAGE), "{message}");
    assert!(message.contains("rolling it back failed"), "{message}");
    assert!(message.contains("connection closed"), "{message}");
}

#[test]
fn the_closure_error_survives_the_leak_report() {
    let closure_error = Some(Error::validation("email", "is required"));
    let message = leaked_transaction_error(LeakedRollback::RolledBack, closure_error);
    let message = message.to_string();

    assert!(message.contains(LEAKED_TRANSACTION_MESSAGE), "{message}");
    assert!(message.contains("email"), "{message}");
}
