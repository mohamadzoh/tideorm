mod error_exhaustive_variants {
    use tideorm::error::Error;

    /// Every variant reports its own stable code, an HTTP status and a
    /// suggestion.
    #[test]
    fn test_every_variant_has_a_code_a_status_and_a_suggestion() {
        for (error, code, status) in [
            (Error::not_found("find user"), "TIDE_NOT_FOUND", 404),
            (
                Error::connection("Connection refused"),
                "TIDE_CONNECTION",
                503,
            ),
            (Error::query("syntax error"), "TIDE_QUERY", 400),
            (
                Error::validation("email", "invalid"),
                "TIDE_VALIDATION",
                422,
            ),
            (Error::conversion("type mismatch"), "TIDE_CONVERSION", 400),
            (Error::transaction("timeout"), "TIDE_TRANSACTION", 409),
            (Error::configuration("not set"), "TIDE_CONFIG", 500),
            (Error::internal("oops"), "TIDE_INTERNAL", 500),
            (
                Error::access_denied("delete", "posts"),
                "TIDE_ACCESS_DENIED",
                403,
            ),
            (Error::rbac("no role"), "TIDE_RBAC", 500),
            (
                Error::backend_not_supported("arrays", "SQLite"),
                "TIDE_BACKEND_NOT_SUPPORTED",
                501,
            ),
            (
                Error::primary_key_not_set("missing pk", "Post"),
                "TIDE_PRIMARY_KEY_NOT_SET",
                400,
            ),
            (
                Error::tokenization("encode failed"),
                "TIDE_TOKENIZATION",
                400,
            ),
            (Error::invalid_token("tampered"), "TIDE_INVALID_TOKEN", 401),
        ] {
            assert_eq!(error.code(), code, "{error:?}");
            assert_eq!(error.http_status(), status, "{error:?}");
            assert!(!error.suggestion().is_empty(), "{error:?}");
        }
    }
}

mod error_retryable_edge_cases {
    use tideorm::error::Error;

    /// Retries follow how the database classified a failure. An error built
    /// from a message alone has no classified failure, so it is not
    /// retryable, however its message reads.
    #[test]
    fn test_errors_without_a_classified_failure_are_not_retryable() {
        for error in [
            Error::connection("Connection refused by host"),
            Error::connection("connection pool exhausted"),
            Error::query("lock wait timeout exceeded"),
            Error::transaction("serialization failure"),
            Error::query("deadlock detected"),
            Error::query("syntax error"),
            Error::not_found("User not found"),
            Error::validation("email", "invalid"),
            Error::configuration("missing key"),
            Error::backend_not_supported("arrays", "sqlite"),
        ] {
            assert!(!error.is_retryable(), "{error:?}");
        }
    }
}

mod error_display {
    use tideorm::error::Error;

    #[test]
    fn test_display_prefixes_the_kind_and_keeps_the_message_verbatim() {
        assert_eq!(Error::not_found("").to_string(), "Record not found: ");
        assert_eq!(
            Error::query("Error: 'invalid' \"syntax\" <tag> & more\nline 2").to_string(),
            "Query error: Error: 'invalid' \"syntax\" <tag> & more\nline 2"
        );
    }
}

mod error_context_application {
    use tideorm::error::{Error, ErrorContext};

    #[test]
    fn test_with_context_is_kept_by_not_found_and_query_errors_only() {
        let ctx = || ErrorContext::new().table("users").column("id");

        for error in [
            Error::not_found("user 42").with_context(ctx()),
            Error::query("bad sql").with_context(ctx()),
        ] {
            let kept = error.context().expect("the context is kept");
            assert_eq!(kept.table.as_deref(), Some("users"), "{error:?}");
        }
        assert!(
            Error::connection("refused")
                .with_context(ctx())
                .context()
                .is_none()
        );
    }

    #[test]
    fn test_error_context_display() {
        let ctx = ErrorContext::new()
            .table("orders")
            .column("total")
            .query("UPDATE orders SET total = -1");
        let display = format!("{}", ctx);
        assert!(display.contains("table: orders"));
        assert!(display.contains("column: total"));
        assert!(display.contains("query: UPDATE"));
    }
}

mod error_log_format_comprehensive {
    use tideorm::error::{Error, ErrorContext};

    #[test]
    fn test_log_format_with_full_context() {
        let err = Error::query("column \"xyz\" does not exist").with_context(
            ErrorContext::new()
                .table("users")
                .column("xyz")
                .query("SELECT xyz FROM users"),
        );

        let log = err.log_format();
        assert!(log.contains("[TIDE_QUERY]"));
        assert!(log.contains("column \"xyz\" does not exist"));
        assert!(log.contains("Table: users"));
        assert!(log.contains("Column: xyz"));
        assert!(log.contains("Query: SELECT xyz FROM users"));
        assert!(log.contains("Suggestion:"));
    }

    #[test]
    fn test_log_format_without_context() {
        let err = Error::internal("something broke");
        let log = err.log_format();
        assert!(log.contains("[TIDE_INTERNAL]"));
        assert!(log.contains("something broke"));
        assert!(log.contains("Suggestion:"));
        assert!(!log.contains("Table:"));
    }
}
