use super::{DbFailure, DbFailureKind, Error};

/// Render what the driver reported about a failure, for appending to a
/// suggestion.
///
/// Empty when the driver exposed neither a constraint name nor a code, which is
/// the normal case on backends that only report a message.
fn failure_detail(failure: &DbFailure) -> String {
    match (failure.constraint(), failure.code()) {
        (Some(constraint), Some(code)) => {
            format!(" on constraint `{}` (SQLSTATE {})", constraint, code)
        }
        (Some(constraint), None) => format!(" on constraint `{}`", constraint),
        (None, Some(code)) => format!(" (SQLSTATE {})", code),
        (None, None) => String::new(),
    }
}

/// Derive the next debugging step from what the driver actually classified.
///
/// Returns `None` for [`DbFailureKind::Unclassified`] so the caller falls back
/// to reading the message — the only place substring matching is still correct,
/// because there is no structured data to consult.
fn structured_suggestion(failure: &DbFailure) -> Option<String> {
    let detail = failure_detail(failure);

    let suggestion = match failure.kind() {
        DbFailureKind::Unclassified => return None,
        DbFailureKind::UniqueViolation => format!(
            "Duplicate key violation{}. The value already exists in a unique column.",
            detail
        ),
        DbFailureKind::ForeignKeyViolation => format!(
            "Foreign key constraint violation{}. The referenced record doesn't exist or can't be deleted.",
            detail
        ),
        DbFailureKind::NotNullViolation => format!(
            "NULL value not allowed{}. Ensure all required fields are provided.",
            detail
        ),
        DbFailureKind::CheckViolation => format!(
            "Check constraint violation{}. The value is outside the range the column allows.",
            detail
        ),
        DbFailureKind::SyntaxError => format!(
            "SQL syntax error{}. Check column names and query structure.",
            detail
        ),
        DbFailureKind::UndefinedColumn => format!(
            "Column doesn't exist{}. Check spelling and run migrations if needed.",
            detail
        ),
        DbFailureKind::UndefinedTable => format!(
            "Table doesn't exist{}. Run migrations: `TideConfig::init().run_migrations(true).connect().await?`",
            detail
        ),
        DbFailureKind::InsufficientPrivilege => format!(
            "Permission denied{}. Check database user privileges.",
            detail
        ),
        DbFailureKind::Deadlock => format!(
            "Deadlock detected{}. Retry the transaction or review query ordering.",
            detail
        ),
        DbFailureKind::SerializationFailure => format!(
            "Serialization failure{}. Replay the transaction; concurrent writes conflicted.",
            detail
        ),
        DbFailureKind::LockNotAvailable => format!(
            "Lock not available{}. Retry, or shorten the transaction holding the lock.",
            detail
        ),
        DbFailureKind::StatementTimeout => format!(
            "Statement timed out{}. Retry, add an index, or raise the statement timeout.",
            detail
        ),
        DbFailureKind::ConnectionTimeout => format!(
            "Timed out waiting for a connection{}. Consider:\n\
             1. Increasing `max_connections` in TideConfig\n\
             2. Reducing connection hold time\n\
             3. Using `acquire_timeout` to wait for connections",
            detail
        ),
        DbFailureKind::ConnectionClosed => format!(
            "The connection was closed{}. Retry; the pool opens a fresh connection.",
            detail
        ),
    };

    Some(suggestion)
}

/// Fall back to reading the message when the driver classified nothing.
fn query_suggestion_from_message(message: &str) -> &'static str {
    if message.contains("syntax") || message.contains("Syntax") {
        "SQL syntax error. Check column names and query structure."
    } else if message.contains("duplicate") || message.contains("unique") {
        "Duplicate key violation. The value already exists in a unique column."
    } else if message.contains("foreign key") || message.contains("violates foreign key") {
        "Foreign key constraint violation. The referenced record doesn't exist or can't be deleted."
    } else if message.contains("null") || message.contains("NOT NULL") {
        "NULL value not allowed. Ensure all required fields are provided."
    } else if message.contains("column") && message.contains("does not exist") {
        "Column doesn't exist. Check spelling and run migrations if needed."
    } else if message.contains("table") && message.contains("does not exist") {
        "Table doesn't exist. Run migrations: `TideConfig::init().run_migrations(true).connect().await?`"
    } else if message.contains("permission") || message.contains("denied") {
        "Permission denied. Check database user privileges."
    } else if message.contains("deadlock") {
        "Deadlock detected. Retry the transaction or review query ordering."
    } else {
        "Check the SQL query and ensure all referenced columns/tables exist."
    }
}

/// Fall back to reading the message for connection failures.
fn connection_suggestion_from_message(message: &str) -> &'static str {
    if message.contains("refused") || message.contains("Refused") {
        "Database server is not running or not accepting connections. Check that:\n\
         1. The database server is running\n\
         2. The host and port are correct\n\
         3. Firewall allows the connection"
    } else if message.contains("password") || message.contains("authentication") {
        "Check your database credentials in the connection URL."
    } else if message.contains("does not exist") || message.contains("unknown database") {
        "The database doesn't exist. Create it first: CREATE DATABASE dbname;"
    } else if message.contains("timeout") || message.contains("Timeout") {
        "Connection timed out. Check network connectivity and increase `connect_timeout` if needed."
    } else if message.contains("pool") || message.contains("Pool") {
        "Connection pool exhausted. Consider:\n\
         1. Increasing `max_connections` in TideConfig\n\
         2. Reducing connection hold time\n\
         3. Using `acquire_timeout` to wait for connections"
    } else {
        "Verify your database URL format: postgres://user:pass@host:5432/database"
    }
}

/// Fall back to reading the message for transaction-control failures.
fn transaction_suggestion_from_message(message: &str) -> &'static str {
    if message.contains("timeout") {
        "Transaction timed out. Split into smaller transactions or increase timeout."
    } else if message.contains("rollback") || message.contains("aborted") {
        "Transaction was rolled back. Check for errors in transaction body."
    } else {
        "Transaction failed. Ensure all operations in the transaction are valid."
    }
}

impl Error {
    /// Return a user-facing next step derived from the variant and message.
    ///
    /// Driver failures answer from the structured [`DbFailure`] where one
    /// survived translation — including the name of the constraint that fired —
    /// and only fall back to reading the message when the driver classified
    /// nothing.
    pub fn suggestion(&self) -> String {
        match self {
            Self::NotFound { message, .. } => {
                if message.contains("find") || message.contains("Find") {
                    "Check that the ID exists. Use `Model::exists(id).await?` to verify before find.".to_string()
                } else {
                    "Verify the record exists and hasn't been soft-deleted. Use `.with_trashed()` to include deleted records.".to_string()
                }
            }
            Self::Connection { message, source } => source
                .as_deref()
                .and_then(structured_suggestion)
                .unwrap_or_else(|| connection_suggestion_from_message(message).to_string()),
            Self::Query {
                message,
                context,
                source,
            } => {
                let base_suggestion = source
                    .as_deref()
                    .and_then(structured_suggestion)
                    .unwrap_or_else(|| query_suggestion_from_message(message).to_string());

                match context.as_deref().and_then(|ctx| ctx.query.as_deref()) {
                    Some(query) => format!("{}\n\nQuery: {}", base_suggestion, query),
                    None => base_suggestion,
                }
            }
            Self::Validation { field, message: _ } => {
                format!("Validate the '{}' field before saving. Use Model::validate() for custom validation.", field)
            }
            Self::Conversion { message } => {
                if message.contains("type") {
                    "Type mismatch. Check that Rust types match database column types.".to_string()
                } else {
                    "Data conversion failed. Verify the data format matches expected type.".to_string()
                }
            }
            Self::Transaction { message, source } => source
                .as_deref()
                .and_then(structured_suggestion)
                .unwrap_or_else(|| transaction_suggestion_from_message(message).to_string()),
            Self::Configuration { message } => {
                if message.contains("initialized") || message.contains("not set") {
                    "Database not initialized. Call `TideConfig::init().database(url).connect().await?` first.".to_string()
                } else if message.contains("already") {
                    "Configuration already set. TideConfig::init() should only be called once.".to_string()
                } else {
                    format!("Check your TideConfig settings: {}", message)
                }
            }
            Self::Internal { .. } => {
                "Internal error. Please report this issue at https://github.com/mohamadzoh/tideorm/issues".to_string()
            }
            Self::AccessDenied {
                permission,
                resource,
            } => {
                format!(
                    "The connected role may not `{}` on `{}`.\n\
                     Grant the permission, or run the operation as a role that already has it.",
                    permission, resource
                )
            }
            Self::Rbac { .. } => {
                "The access-control check itself failed. Verify the RBAC tables are populated and reachable.".to_string()
            }
            Self::BackendNotSupported { backend, message } => {
                format!(
                    "Operation not supported on {} backend. {}\n\
                     Consider using a database-agnostic approach or checking backend with `db.backend()`.",
                    backend, message
                )
            }
            Self::PrimaryKeyNotSet { model, .. } => {
                format!(
                    "Set the primary key on your {} instance before this operation.\n\
                     Use `Model::find(id)` to load an existing record, or ensure auto-increment is configured.",
                    model
                )
            }
            Self::InsertReturningNotSupported { backend, .. } => {
                format!(
                    "{} does not support INSERT ... RETURNING syntax.\n\
                     Options:\n\
                     1. Use separate insert() and find() calls\n\
                     2. For MySQL, use last_insert_id() after insert\n\
                     3. Consider using PostgreSQL which supports RETURNING",
                    backend
                )
            }
            Self::Tokenization { message } => {
                format!(
                    "Tokenization failed: {}\n\
                     Ensure:\n\
                     1. An encryption key is configured via TideConfig::encryption_key()\n\
                     2. The model has tokenization enabled via #[tideorm(tokenize)]\n\
                     3. The record has a valid primary key",
                    message
                )
            }
            Self::InvalidToken { message } => {
                format!(
                    "Invalid token: {}\n\
                     Possible causes:\n\
                     1. Token was tampered with or corrupted\n\
                     2. Token is for a different model type\n\
                     3. Encryption key has changed since token was created",
                    message
                )
            }
        }
    }

    /// Return a stable error code for logs, metrics, or API responses.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound { .. } => "TIDE_NOT_FOUND",
            Self::Connection { .. } => "TIDE_CONNECTION",
            Self::Query { .. } => "TIDE_QUERY",
            Self::Validation { .. } => "TIDE_VALIDATION",
            Self::Conversion { .. } => "TIDE_CONVERSION",
            Self::Transaction { .. } => "TIDE_TRANSACTION",
            Self::Configuration { .. } => "TIDE_CONFIG",
            Self::Internal { .. } => "TIDE_INTERNAL",
            Self::AccessDenied { .. } => "TIDE_ACCESS_DENIED",
            Self::Rbac { .. } => "TIDE_RBAC",
            Self::BackendNotSupported { .. } => "TIDE_BACKEND_NOT_SUPPORTED",
            Self::PrimaryKeyNotSet { .. } => "TIDE_PRIMARY_KEY_NOT_SET",
            Self::InsertReturningNotSupported { .. } => "TIDE_INSERT_RETURNING_NOT_SUPPORTED",
            Self::Tokenization { .. } => "TIDE_TOKENIZATION",
            Self::InvalidToken { .. } => "TIDE_INVALID_TOKEN",
        }
    }

    /// Map the error to a generic HTTP status code.
    pub fn http_status(&self) -> u16 {
        match self {
            Self::NotFound { .. } => 404,
            Self::Connection { .. } => 503,
            Self::Query { .. } => 400,
            Self::Validation { .. } => 422,
            Self::Conversion { .. } => 400,
            Self::Transaction { .. } => 409,
            Self::Configuration { .. } => 500,
            Self::Internal { .. } => 500,
            Self::AccessDenied { .. } => 403,
            Self::Rbac { .. } => 500,
            Self::BackendNotSupported { .. } => 501,
            Self::PrimaryKeyNotSet { .. } => 400,
            Self::InsertReturningNotSupported { .. } => 501,
            Self::Tokenization { .. } => 400,
            Self::InvalidToken { .. } => 401,
        }
    }

    /// Report whether retrying the operation can plausibly succeed.
    ///
    /// The driver's own classification decides it whenever one survived
    /// translation: a `23505` is never retryable and a `40001` always is, no
    /// matter how the backend worded the message. Only an unclassified failure
    /// falls back to matching transient-looking phrases.
    pub fn is_retryable(&self) -> bool {
        let kind = self.failure_kind();
        if kind != DbFailureKind::Unclassified {
            return kind.is_retryable();
        }

        match self {
            Self::Connection { message, .. } => {
                message.contains("timeout")
                    || message.contains("timed out")
                    || message.contains("pool")
                    || message.contains("refused")
            }
            Self::Query { message, .. } => {
                // Match transient lock phrases, never the bare word "lock":
                // column and table names such as `blocklist`, `locked_at`, or
                // `unlocked` otherwise make permanent schema errors look
                // retryable.
                let message = message.to_ascii_lowercase();
                message.contains("deadlock")
                    || message.contains("timeout")
                    || message.contains("database is locked")
                    || message.contains("lock is not available")
                    || message.contains("could not obtain lock")
                    || message.contains("try restarting transaction")
            }
            Self::Transaction { message, .. } => {
                message.contains("deadlock")
                    || message.contains("timeout")
                    || message.contains("serialization")
            }
            _ => false,
        }
    }

    /// Render the error with its code, context, and suggestion for logs.
    pub fn log_format(&self) -> String {
        let mut output = format!("[{}] {}", self.code(), self);

        if let Some(ctx) = self.context() {
            if let Some(ref table) = ctx.table {
                output.push_str(&format!("\n  Table: {}", table));
            }
            if let Some(ref column) = ctx.column {
                output.push_str(&format!("\n  Column: {}", column));
            }
            if !ctx.conditions.is_empty() {
                output.push_str(&format!("\n  Conditions: {}", ctx.conditions.join(" | ")));
            }
            if let Some(ref operator_chain) = ctx.operator_chain {
                output.push_str(&format!("\n  Operator chain: {}", operator_chain));
            }
            if let Some(ref query) = ctx.query {
                output.push_str(&format!("\n  Query: {}", query));
            }
        }

        if let Some(failure) = self.db_failure() {
            output.push_str(&format!("\n  Database: {}", failure));
        }

        output.push_str(&format!("\n  Suggestion: {}", self.suggestion()));
        output
    }
}

#[cfg(test)]
mod structured_presentation_tests {
    use super::{DbFailure, DbFailureKind, Error};

    #[test]
    fn a_unique_violation_names_the_constraint_that_fired() {
        let err =
            Error::query("duplicate key value violates unique constraint").with_db_failure(Some(
                DbFailure::new(DbFailureKind::UniqueViolation)
                    .with_code(Some("23505".to_string()))
                    .with_constraint(Some("users_email_key".to_string())),
            ));

        assert!(err.is_unique_violation());
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
        assert!(!err.is_unique_violation());
        assert!(err.is_constraint_violation());
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

        // And the reverse: a real serialization failure no longer depends on
        // the backend having worded its message the way the fallback expects.
        let err = Error::query("could not complete because of conflict").with_db_failure(Some(
            DbFailure::new(DbFailureKind::SerializationFailure)
                .with_code(Some("40001".to_string())),
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
}
