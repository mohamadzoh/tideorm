use std::fmt::Write as _;

use super::{DbFailure, DbFailureKind, Error};

/// The headline and next step for each classification, which every
/// classified failure renders from. `None` for
/// [`DbFailureKind::Unclassified`], which has neither.
fn advice(kind: DbFailureKind) -> Option<(&'static str, &'static str)> {
    let advice = match kind {
        DbFailureKind::Unclassified => return None,
        DbFailureKind::UniqueViolation => (
            "Duplicate key violation",
            "The value already exists in a unique column.",
        ),
        DbFailureKind::ForeignKeyViolation => (
            "Foreign key constraint violation",
            "The referenced record doesn't exist or can't be deleted.",
        ),
        DbFailureKind::NotNullViolation => (
            "NULL value not allowed",
            "Ensure all required fields are provided.",
        ),
        DbFailureKind::CheckViolation => (
            "Check constraint violation",
            "The value is outside the range the column allows.",
        ),
        DbFailureKind::ExclusionViolation => (
            "Exclusion constraint violation",
            "The row conflicts with an existing one, such as an overlapping range.",
        ),
        DbFailureKind::InvalidValue => (
            "Value does not fit the column",
            "Check its length, range and format, or validate it before saving.",
        ),
        DbFailureKind::SyntaxError => (
            "SQL syntax error",
            "Check column names and query structure.",
        ),
        DbFailureKind::UndefinedColumn => (
            "Column doesn't exist",
            "Check spelling and run migrations if needed.",
        ),
        DbFailureKind::UndefinedTable => (
            "Table doesn't exist",
            "Run migrations: `TideConfig::init().run_migrations(true).connect().await?`",
        ),
        DbFailureKind::InsufficientPrivilege => {
            ("Permission denied", "Check database user privileges.")
        }
        DbFailureKind::Deadlock => (
            "Deadlock detected",
            "Retry the transaction or review query ordering.",
        ),
        DbFailureKind::SerializationFailure => (
            "Serialization failure",
            "Replay the transaction; concurrent writes conflicted.",
        ),
        DbFailureKind::LockNotAvailable => (
            "Lock not available",
            "Retry, or shorten the transaction holding the lock.",
        ),
        DbFailureKind::StatementTimeout => (
            "Statement timed out",
            "Retry, add an index, or raise the statement timeout.",
        ),
        DbFailureKind::ConnectionTimeout => (
            "Timed out waiting for a connection",
            "Consider:\n\
             1. Increasing `max_connections` in TideConfig\n\
             2. Reducing connection hold time\n\
             3. Using `acquire_timeout` to wait for connections",
        ),
        DbFailureKind::ConnectionClosed => (
            "The connection was closed",
            "Retry; the pool opens a fresh connection.",
        ),
        DbFailureKind::ConnectionRefused => (
            "The server refused the connection",
            "Check that the database server is running, that the URL's host and port are \
             right, and that a firewall allows the connection.",
        ),
        DbFailureKind::AuthenticationFailed => (
            "Authentication failed",
            "Check the user name and password in the connection URL.",
        ),
        DbFailureKind::UnknownDatabase => (
            "The database doesn't exist",
            "Create it first (`CREATE DATABASE ..`), or fix its name in the connection URL.",
        ),
        DbFailureKind::TlsFailed => (
            "The TLS handshake failed",
            "Check the server's certificate and the URL's TLS options.",
        ),
        DbFailureKind::InvalidConfiguration => (
            "Invalid connection settings",
            "Check the database URL and its options, such as \
             postgres://user:pass@host:5432/database.",
        ),
        DbFailureKind::NotConnected => (
            "No database connection",
            "Connect first with `TideConfig::init().database(url).connect().await?` or \
             `Database::init(url).await?`.",
        ),
    };

    Some(advice)
}

/// Derive the next debugging step from how the failure was classified: the
/// table entry for its kind, with its detail right after the headline.
///
/// Returns `None` for [`DbFailureKind::Unclassified`], which leaves the
/// caller's general advice for its variant.
fn structured_suggestion(failure: &DbFailure) -> Option<String> {
    advice(failure.kind())
        .map(|(headline, next_step)| format!("{headline}{}. {next_step}", failure.detail()))
}

impl Error {
    /// Return a user-facing next step for the error: the advice for its
    /// classified [`DbFailure`] where it has one — naming the constraint that
    /// fired — and otherwise its variant's. The message is never read.
    pub fn suggestion(&self) -> String {
        match self {
            Self::NotFound { .. } => "Check that the record exists and hasn't been soft-deleted: \
                 `Model::exists(id).await?` checks a key, and `.with_trashed()` includes deleted \
                 records."
                .to_string(),
            Self::Connection { source, .. } => source
                .as_deref()
                .and_then(structured_suggestion)
                .unwrap_or_else(|| {
                    "Check that the database server is reachable and that the connection URL \
                     names the right host, port, user, password and database."
                        .to_string()
                }),
            Self::Query {
                context, source, ..
            } => {
                let base_suggestion = source
                    .as_deref()
                    .and_then(structured_suggestion)
                    .unwrap_or_else(|| {
                        "Check the SQL query and ensure all referenced columns/tables exist."
                            .to_string()
                    });

                match context.as_deref().and_then(|ctx| ctx.query.as_deref()) {
                    Some(query) => format!("{}\n\nQuery: {}", base_suggestion, query),
                    None => base_suggestion,
                }
            }
            Self::Validation { field, .. } => {
                format!("Validate the '{}' field before saving. Use Model::validate() for custom validation.", field)
            }
            Self::Conversion { .. } => "Check that the Rust field types match the column types, \
                 and that the data has the format its type expects."
                .to_string(),
            Self::Transaction { source, .. } => source
                .as_deref()
                .and_then(structured_suggestion)
                .unwrap_or_else(|| {
                    "The transaction failed and was rolled back. Fix the error that ended it, \
                     then run the transaction again."
                        .to_string()
                }),
            Self::Configuration { message } => {
                format!("Check your TideConfig settings: {}", message)
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
            Self::Tokenization { .. } => "TIDE_TOKENIZATION",
            Self::InvalidToken { .. } => "TIDE_INVALID_TOKEN",
        }
    }

    /// Map the error to a generic HTTP status code.
    pub fn http_status(&self) -> u16 {
        match self {
            Self::NotFound { .. } => 404,
            Self::Connection { .. } => 503,
            Self::Query { .. }
            | Self::Conversion { .. }
            | Self::PrimaryKeyNotSet { .. }
            | Self::Tokenization { .. } => 400,
            Self::Validation { .. } => 422,
            Self::Transaction { .. } => 409,
            Self::Configuration { .. } | Self::Internal { .. } | Self::Rbac { .. } => 500,
            Self::AccessDenied { .. } => 403,
            Self::BackendNotSupported { .. } => 501,
            Self::InvalidToken { .. } => 401,
        }
    }

    /// Report whether retrying the operation can plausibly succeed.
    ///
    /// The failure's classification decides it: a `23505` is never retryable
    /// and a `40001` always is, however the backend worded the message. An
    /// error with no classified failure is not retryable.
    pub fn is_retryable(&self) -> bool {
        self.failure_kind().is_retryable()
    }

    /// Render the error with its code, context, and suggestion for logs.
    pub fn log_format(&self) -> String {
        let mut output = format!("[{}] {}", self.code(), self);

        for (name, value) in self.context().map(|ctx| ctx.entries()).unwrap_or_default() {
            // `operator_chain` reads `Operator chain`.
            let mut label = name.replace('_', " ");
            label[..1].make_ascii_uppercase();
            let _ = write!(output, "\n  {label}: {value}");
        }

        if let Some(failure) = self.db_failure() {
            let _ = write!(output, "\n  Database: {}", failure);
        }

        let _ = write!(output, "\n  Suggestion: {}", self.suggestion());
        output
    }
}
