use super::*;
use std::error::Error as StdError;

#[test]
fn test_error_suggestions() {
    let err = Error::not_found("User with ID 123 not found");
    let suggestion = err.suggestion();
    assert!(
        suggestion.contains("verify")
            || suggestion.contains("exists")
            || suggestion.contains("ID")
            || suggestion.contains("record")
    );

    let err = Error::connection("Connection refused");
    let suggestion = err.suggestion();
    assert!(suggestion.contains("running") || suggestion.contains("server"));

    let err = Error::query("duplicate key value violates unique constraint");
    let suggestion = err.suggestion();
    assert!(suggestion.contains("Duplicate") || suggestion.contains("unique"));

    let err = Error::query("violates foreign key constraint");
    let suggestion = err.suggestion();
    assert!(suggestion.contains("Foreign key") || suggestion.contains("foreign"));
}

#[test]
fn test_error_codes() {
    assert_eq!(Error::not_found("User not found").code(), "TIDE_NOT_FOUND");
    assert_eq!(Error::connection("test").code(), "TIDE_CONNECTION");
    assert_eq!(Error::query("test").code(), "TIDE_QUERY");
    assert_eq!(
        Error::validation("field", "message").code(),
        "TIDE_VALIDATION"
    );
    assert_eq!(Error::conversion("test").code(), "TIDE_CONVERSION");
    assert_eq!(Error::transaction("test").code(), "TIDE_TRANSACTION");
    assert_eq!(Error::configuration("test").code(), "TIDE_CONFIG");
    assert_eq!(Error::internal("test").code(), "TIDE_INTERNAL");
}

#[test]
fn test_http_status() {
    assert_eq!(Error::not_found("User not found").http_status(), 404);
    assert_eq!(Error::connection("test").http_status(), 503);
    assert_eq!(Error::query("test").http_status(), 400);
    assert_eq!(Error::validation("field", "message").http_status(), 422);
    assert_eq!(Error::conversion("test").http_status(), 400);
    assert_eq!(Error::transaction("test").http_status(), 409);
    assert_eq!(Error::configuration("test").http_status(), 500);
    assert_eq!(Error::internal("test").http_status(), 500);
}

#[test]
fn test_is_retryable() {
    let err = Error::connection("Connection timeout");
    assert!(err.is_retryable());

    let err = Error::connection("connection pool exhausted");
    assert!(err.is_retryable());

    let err = Error::query("deadlock detected");
    assert!(err.is_retryable());

    let err = Error::query("syntax error");
    assert!(!err.is_retryable());

    let err = Error::not_found("User not found");
    assert!(!err.is_retryable());
}

#[test]
fn test_log_format() {
    let err = Error::query_with_context(
        "syntax error at position 10",
        ErrorContext::new()
            .table("users")
            .query("SELECT * FROM users WHERE"),
    );

    let log = err.log_format();
    assert!(log.contains("TIDE_QUERY"));
    assert!(log.contains("syntax error"));
    assert!(log.contains("Table: users"));
    assert!(log.contains("Suggestion:"));
}

#[test]
fn test_error_context() {
    let ctx = ErrorContext::new()
        .table("users")
        .column("email")
        .condition("email = \"alice@example.com\"")
        .operator_chain("email = \"alice@example.com\"")
        .query("SELECT * FROM users");

    assert_eq!(ctx.table, Some("users".to_string()));
    assert_eq!(ctx.column, Some("email".to_string()));
    assert_eq!(
        ctx.conditions,
        vec!["email = \"alice@example.com\"".to_string()]
    );
    assert_eq!(
        ctx.operator_chain,
        Some("email = \"alice@example.com\"".to_string())
    );
    assert_eq!(ctx.query, Some("SELECT * FROM users".to_string()));
}

#[test]
fn test_validation_errors() {
    use crate::validation::ValidationErrors;

    let mut errors = ValidationErrors::new();
    assert!(errors.is_empty());

    errors.add("email", "Invalid email format");
    errors.add("name", "Name is required");

    assert!(!errors.is_empty());
    assert_eq!(errors.len(), 2);

    let display = format!("{}", errors);
    assert!(display.contains("email"));
    assert!(display.contains("name"));
}

#[test]
fn test_error_context_is_not_reported_as_source() {
    let err = Error::query_with_context(
        "syntax error",
        ErrorContext::new()
            .table("users")
            .query("SELECT * FROM users WHERE"),
    );

    assert!(StdError::source(&err).is_none());
    assert!(err.context().is_some());
}

#[test]
fn a_unique_violation_names_the_constraint_that_fired() {
    let err = Error::query("duplicate key value violates unique constraint").with_db_failure(Some(
        DbFailure::new(DbFailureKind::UniqueViolation)
            .with_code(Some("23505".to_string()))
            .with_constraint(Some("users_email_key".to_string())),
    ));

    assert_eq!(err.failure_kind(), DbFailureKind::UniqueViolation);
    assert!(!err.is_foreign_key_violation());
    assert_eq!(err.sqlstate(), Some("23505"));
    assert_eq!(err.constraint(), Some("users_email_key"));

    let suggestion = err.suggestion();
    assert!(suggestion.contains("users_email_key"), "{suggestion}");
    assert!(suggestion.contains("23505"), "{suggestion}");
}

#[test]
fn foreign_key_violations_are_distinguishable_from_unique_ones() {
    let err = Error::query("violates foreign key constraint").with_db_failure(Some(
        DbFailure::new(DbFailureKind::ForeignKeyViolation)
            .with_code(Some("23503".to_string()))
            .with_constraint(Some("posts_user_id_fkey".to_string())),
    ));

    assert!(err.is_foreign_key_violation());
    assert_ne!(err.failure_kind(), DbFailureKind::UniqueViolation);
    assert!(err.failure_kind().is_constraint_violation());
    assert!(err.log_format().contains("posts_user_id_fkey"));
}

#[test]
fn the_driver_classification_beats_the_message_for_retries() {
    // The message reads "deadlock", which the substring fallback would call
    // retryable; the driver says 23505, which never is.
    let err = Error::query("deadlock while checking duplicate key").with_db_failure(Some(
        DbFailure::new(DbFailureKind::UniqueViolation).with_code(Some("23505".to_string())),
    ));
    assert!(!err.is_retryable());

    // And the reverse: a real serialization failure does not depend on the
    // backend having worded its message the way the fallback expects.
    let err = Error::query("could not complete because of conflict").with_db_failure(Some(
        DbFailure::new(DbFailureKind::SerializationFailure).with_code(Some("40001".to_string())),
    ));
    assert!(err.is_retryable());
}

#[test]
fn an_unclassified_failure_still_falls_back_to_the_message() {
    let err = Error::query("syntax error at or near \"slect\"")
        .with_db_failure(Some(DbFailure::new(DbFailureKind::Unclassified)));

    assert_eq!(err.failure_kind(), DbFailureKind::Unclassified);
    assert!(err.suggestion().contains("SQL syntax error"));
    assert!(!err.is_retryable());
}

#[test]
fn the_message_fallback_and_the_driver_classification_give_the_same_advice() {
    let from_message = Error::query("duplicate key value violates unique constraint");
    let from_driver = Error::query("constraint failed")
        .with_db_failure(Some(DbFailure::new(DbFailureKind::UniqueViolation)));
    assert_eq!(from_message.suggestion(), from_driver.suggestion());

    let pool_exhausted = Error::connection("connection pool exhausted").suggestion();
    let acquire_timeout = Error::connection("timed out")
        .with_db_failure(Some(DbFailure::new(DbFailureKind::ConnectionTimeout)))
        .suggestion();
    assert!(
        pool_exhausted.contains("`max_connections`"),
        "{pool_exhausted}"
    );
    assert!(
        acquire_timeout.contains("`max_connections`"),
        "{acquire_timeout}"
    );
}

#[test]
fn the_not_initialized_error_says_to_connect_first() {
    let err = crate::database::Database::disconnected()
        .current_inner()
        .err()
        .expect("a disconnected handle has no connection to hand out");

    assert!(matches!(err, Error::Connection { .. }), "{err:?}");
    let suggestion = err.suggestion();
    assert!(suggestion.contains("TideConfig::init()"), "{suggestion}");
    assert!(suggestion.contains("Database::init(url)"), "{suggestion}");
    assert!(!suggestion.contains("URL format"), "{suggestion}");
}

#[test]
fn errors_that_never_reached_a_driver_report_no_failure() {
    let err = Error::invalid_query("where_eq called without a column");

    assert!(err.db_failure().is_none());
    assert_eq!(err.failure_kind(), DbFailureKind::Unclassified);
    assert!(err.sqlstate().is_none());
    assert!(err.constraint().is_none());
}

#[test]
fn sqlstate_classification_covers_what_the_driver_kind_does_not() {
    assert_eq!(
        DbFailureKind::from_sqlstate("40P01"),
        DbFailureKind::Deadlock
    );
    assert_eq!(
        DbFailureKind::from_sqlstate("42P01"),
        DbFailureKind::UndefinedTable
    );
    assert_eq!(
        DbFailureKind::from_sqlstate("42501"),
        DbFailureKind::InsufficientPrivilege
    );
    // SQLite reports a native code, not a SQLSTATE; it stays unclassified
    // here and is classified from the driver's own kind instead.
    assert_eq!(
        DbFailureKind::from_sqlstate("2067"),
        DbFailureKind::Unclassified
    );
}

/// A value that does not fit its column is the caller's input to fix, not a
/// failure to retry.
#[test]
fn data_exceptions_classify_as_invalid_values() {
    // Too long, out of range, a bad datetime, invalid text for the type.
    for sqlstate in ["22001", "22003", "22007", "22P02"] {
        let kind = DbFailureKind::from_sqlstate(sqlstate);
        assert_eq!(kind, DbFailureKind::InvalidValue, "{sqlstate}");
        assert!(!kind.is_retryable(), "{sqlstate}");
    }
}

#[test]
fn mysql_error_numbers_classify_what_their_sqlstate_does_not() {
    for (number, kind) in [
        (1205, DbFailureKind::LockNotAvailable),
        (1213, DbFailureKind::Deadlock),
        (3024, DbFailureKind::StatementTimeout),
        (1969, DbFailureKind::StatementTimeout),
        (1142, DbFailureKind::InsufficientPrivilege),
        (1053, DbFailureKind::ConnectionClosed),
        (1366, DbFailureKind::InvalidValue),
        (1064, DbFailureKind::Unclassified),
    ] {
        assert_eq!(DbFailureKind::from_mysql_code(number), kind, "{number}");
    }
    assert!(DbFailureKind::from_mysql_code(1205).is_retryable());
    assert_eq!(
        DbFailureKind::from_sqlstate("08S01"),
        DbFailureKind::ConnectionClosed
    );
}

#[test]
fn sqlite_codes_classify_missing_objects_and_lock_contention() {
    for (code, message, kind) in [
        ("1", "no such table: users", DbFailureKind::UndefinedTable),
        ("1", "no such column: nme", DbFailureKind::UndefinedColumn),
        (
            "1",
            "table users has no column named nme",
            DbFailureKind::UndefinedColumn,
        ),
        (
            "1",
            "near \"SELEC\": syntax error",
            DbFailureKind::SyntaxError,
        ),
        ("5", "database is locked", DbFailureKind::LockNotAvailable),
        ("517", "database is locked", DbFailureKind::LockNotAvailable),
        (
            "6",
            "database table is locked",
            DbFailureKind::LockNotAvailable,
        ),
        ("1", "some other failure", DbFailureKind::Unclassified),
    ] {
        assert_eq!(
            DbFailureKind::from_sqlite_code(code, message),
            kind,
            "{code} {message}"
        );
    }
    assert!(DbFailureKind::from_sqlite_code("5", "database is locked").is_retryable());
}
