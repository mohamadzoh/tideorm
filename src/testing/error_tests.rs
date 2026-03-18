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
fn test_error_checks() {
    let err = Error::not_found("User not found");
    assert!(err.is_not_found());
    assert!(!err.is_connection_error());

    let err = Error::connection("test");
    assert!(err.is_connection_error());
    assert!(!err.is_not_found());

    let err = Error::query("test");
    assert!(err.is_query_error());
    assert!(!err.is_not_found());
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