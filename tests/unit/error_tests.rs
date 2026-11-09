use super::*;
use std::error::Error as StdError;

/// Without a classified failure, the suggestion is the variant's own advice,
/// whatever the message says.
#[test]
fn unclassified_errors_get_their_variants_advice() {
    for (err, expected) in [
        (Error::not_found("find user 123"), "`.with_trashed()`"),
        (Error::connection("Connection refused"), "connection URL"),
        (
            Error::query("duplicate key value violates unique constraint"),
            "Check the SQL query",
        ),
        (Error::transaction("deadlock"), "run the transaction again"),
    ] {
        let suggestion = err.suggestion();
        assert!(suggestion.contains(expected), "{err}: {suggestion}");
    }
}

#[test]
fn test_error_context_is_not_reported_as_source() {
    let err = Error::query("syntax error").with_context(
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

/// The message of an unclassified failure is never read: one that reads
/// like a syntax error or a deadlock is neither advised nor retried as one.
#[test]
fn an_unclassified_failure_is_not_judged_by_its_message() {
    let err = Error::query("syntax error at or near \"slect\"")
        .with_db_failure(Some(DbFailure::new(DbFailureKind::Unclassified)));

    assert_eq!(err.failure_kind(), DbFailureKind::Unclassified);
    assert!(!err.suggestion().contains("SQL syntax error"));
    assert!(!err.is_retryable());
    assert!(!Error::query("deadlock detected").is_retryable());
    assert!(!Error::connection("connection refused").is_retryable());
}

#[test]
fn connection_failures_are_advised_and_retried_by_their_classification() {
    for (kind, expected, retryable) in [
        (DbFailureKind::ConnectionTimeout, "`max_connections`", true),
        (DbFailureKind::ConnectionRefused, "server is running", true),
        (
            DbFailureKind::AuthenticationFailed,
            "user name and password",
            false,
        ),
        (DbFailureKind::UnknownDatabase, "CREATE DATABASE", false),
        (DbFailureKind::TlsFailed, "certificate", false),
        (DbFailureKind::InvalidConfiguration, "database URL", false),
    ] {
        let err = Error::connection("failed").with_db_failure(Some(DbFailure::new(kind)));
        let suggestion = err.suggestion();
        assert!(suggestion.contains(expected), "{kind}: {suggestion}");
        assert_eq!(err.is_retryable(), retryable, "{kind}");
    }
}

#[test]
fn the_not_initialized_error_says_to_connect_first() {
    let err = crate::database::Database::disconnected()
        .current_inner()
        .err()
        .expect("a disconnected handle has no connection to hand out");

    assert!(matches!(err, Error::Connection { .. }), "{err:?}");
    assert_eq!(err.failure_kind(), DbFailureKind::NotConnected);
    assert!(!err.is_retryable());
    let suggestion = err.suggestion();
    assert!(suggestion.contains("TideConfig::init()"), "{suggestion}");
    assert!(suggestion.contains("Database::init(url)"), "{suggestion}");
    assert!(!suggestion.contains("URL format"), "{suggestion}");
}

#[test]
fn errors_that_never_reached_a_driver_report_no_failure() {
    let err = Error::query("where_eq called without a column");

    assert!(err.db_failure().is_none());
    assert_eq!(err.failure_kind(), DbFailureKind::Unclassified);
    assert!(err.sqlstate().is_none());
    assert!(err.constraint().is_none());
}

#[test]
fn bad_input_is_not_retried_and_lock_contention_is() {
    // A value that does not fit its column is the caller's input to fix.
    assert!(!DbFailureKind::InvalidValue.is_retryable());
    assert!(DbFailureKind::LockNotAvailable.is_retryable());
    assert!(DbFailureKind::Deadlock.is_retryable());
}
