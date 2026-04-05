mod error_exhaustive_variants {
    use tideorm::error::Error;

    #[test]
    fn test_all_error_variants_have_unique_codes() {
        let errors: Vec<Error> = vec![
            Error::not_found("test"),
            Error::connection("test"),
            Error::query("test"),
            Error::validation("f", "test"),
            Error::conversion("test"),
            Error::transaction("test"),
            Error::configuration("test"),
            Error::internal("test"),
            Error::backend_not_supported("test", "pg"),
            Error::primary_key_not_set("test", "User"),
            Error::insert_returning_not_supported("test", "mysql"),
            Error::tokenization("test"),
            Error::invalid_token("test"),
        ];

        let codes: Vec<&str> = errors.iter().map(|e| e.code()).collect();
        let unique: std::collections::HashSet<&&str> = codes.iter().collect();
        assert_eq!(codes.len(), unique.len(), "all error codes must be unique");
    }

    #[test]
    fn test_all_error_http_statuses_are_valid() {
        let errors: Vec<Error> = vec![
            Error::not_found("test"),
            Error::connection("test"),
            Error::query("test"),
            Error::validation("f", "test"),
            Error::conversion("test"),
            Error::transaction("test"),
            Error::configuration("test"),
            Error::internal("test"),
            Error::backend_not_supported("test", "pg"),
            Error::primary_key_not_set("test", "User"),
            Error::insert_returning_not_supported("test", "mysql"),
            Error::tokenization("test"),
            Error::invalid_token("test"),
        ];

        for err in &errors {
            let status = err.http_status();
            assert!(
                (100..600).contains(&status),
                "error code {} has invalid HTTP status {}",
                err.code(),
                status
            );
        }
    }

    #[test]
    fn test_all_suggestions_are_nonempty() {
        let errors: Vec<Error> = vec![
            Error::not_found("find user"),
            Error::connection("Connection refused"),
            Error::query("syntax error"),
            Error::validation("email", "invalid"),
            Error::conversion("type mismatch"),
            Error::transaction("timeout"),
            Error::configuration("not set"),
            Error::internal("oops"),
            Error::backend_not_supported("arrays", "SQLite"),
            Error::primary_key_not_set("missing pk", "Post"),
            Error::insert_returning_not_supported("no returning", "MySQL"),
            Error::tokenization("encode failed"),
            Error::invalid_token("tampered"),
        ];

        for err in &errors {
            let suggestion = err.suggestion();
            assert!(
                !suggestion.is_empty(),
                "{} should have a suggestion",
                err.code()
            );
        }
    }
}

mod error_retryable_edge_cases {
    use tideorm::error::Error;

    #[test]
    fn test_connection_pool_is_retryable() {
        assert!(Error::connection("pool exhausted").is_retryable());
    }

    #[test]
    fn test_connection_refused_is_retryable() {
        assert!(Error::connection("Connection refused by host").is_retryable());
    }

    #[test]
    fn test_query_lock_timeout_is_retryable() {
        assert!(Error::query("lock wait timeout exceeded").is_retryable());
    }

    #[test]
    fn test_transaction_serialization_is_retryable() {
        assert!(Error::transaction("serialization failure").is_retryable());
    }

    #[test]
    fn test_validation_not_retryable() {
        assert!(!Error::validation("email", "invalid").is_retryable());
    }

    #[test]
    fn test_configuration_not_retryable() {
        assert!(!Error::configuration("missing key").is_retryable());
    }

    #[test]
    fn test_backend_not_supported_not_retryable() {
        assert!(!Error::backend_not_supported("arrays", "sqlite").is_retryable());
    }
}

mod error_context_application {
    use tideorm::error::{Error, ErrorContext};

    #[test]
    fn test_with_context_on_not_found() {
        let ctx = ErrorContext::new().table("users").column("id");
        let err = Error::not_found("user 42").with_context(ctx);
        assert!(err.context().is_some());
        assert_eq!(err.context().unwrap().table.as_deref(), Some("users"));
    }

    #[test]
    fn test_with_context_on_query() {
        let ctx = ErrorContext::new().query("SELECT 1");
        let err = Error::query("bad sql").with_context(ctx);
        assert!(err.context().is_some());
    }

    #[test]
    fn test_with_context_on_other_variants_is_noop() {
        let ctx = ErrorContext::new().table("users");
        let err = Error::connection("refused").with_context(ctx);
        assert!(err.context().is_none());
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
        let err = Error::query_with_context(
            "column \"xyz\" does not exist",
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
