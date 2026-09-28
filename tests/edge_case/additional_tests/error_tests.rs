mod error_exhaustive_variants {
    use tideorm::error::Error;

    /// One error of every variant.
    fn every_variant() -> Vec<Error> {
        vec![
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
            Error::tokenization("encode failed"),
            Error::invalid_token("tampered"),
        ]
    }

    #[test]
    fn test_all_error_variants_have_unique_codes() {
        let errors = every_variant();

        let codes: Vec<&str> = errors.iter().map(|e| e.code()).collect();
        let unique: std::collections::HashSet<&&str> = codes.iter().collect();
        assert_eq!(codes.len(), unique.len(), "all error codes must be unique");
    }

    #[test]
    fn test_all_error_http_statuses_are_valid() {
        let errors = every_variant();

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
        let errors = every_variant();

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

mod error_display {
    use tideorm::error::Error;

    #[test]
    fn test_not_found_with_empty_message() {
        let err = Error::not_found("");
        assert!(matches!(err, Error::NotFound { .. }));
        assert_eq!(err.to_string(), "Record not found: ");
    }

    #[test]
    fn test_message_special_characters_are_kept_verbatim() {
        let err = Error::query("Error: 'invalid' \"syntax\" <tag> & more\nline 2");
        assert!(
            err.to_string()
                .contains("'invalid' \"syntax\" <tag> & more\nline 2")
        );
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
