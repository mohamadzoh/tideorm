//! Error types
//!
//! All database errors are translated into these types before reaching user code.
//!
//! The goal here is to preserve enough context to answer:
//! - what operation failed
//! - which table or query was involved
//! - whether the problem is configuration, validation, connection, or SQL
//!
//! Practical split:
//! - inspect `suggestion()` first when you need the next debugging step quickly
//! - inspect `context()` when the failure depends on rendered SQL or table metadata
//! - use `code()` and `http_status()` only when you need stable external handling for logs or APIs

use thiserror::Error;

// ── From impls for common external error types ─────────────────────

impl From<crate::internal::OrmError> for Error {
    fn from(err: crate::internal::OrmError) -> Self {
        crate::internal::translate_error(err)
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::Internal {
            message: err.to_string(),
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Self::Conversion {
            message: err.to_string(),
        }
    }
}

/// Result alias for TideORM operations.
pub type Result<T> = std::result::Result<T, Error>;

/// The main error type for TideORM.
///
/// The variants are grouped by failure source so callers can decide whether to
/// retry, fix input, or stop and inspect configuration.
#[derive(Error, Debug)]
pub enum Error {
    /// Requested record was not found.
    #[error("Record not found: {message}")]
    NotFound {
        /// Missing-record description.
        message: String,
        /// Optional query or table context.
        context: Option<Box<ErrorContext>>,
    },

    /// Database connection failed.
    #[error("Connection error: {message}")]
    Connection {
        /// Backend error text.
        message: String,
    },

    /// Query building or execution failed.
    #[error("Query error: {message}")]
    Query {
        /// Backend or validation error text.
        message: String,
        /// Optional rendered SQL context.
        context: Option<Box<ErrorContext>>,
    },

    /// Validation failed before the write reached the database.
    #[error("Validation error: {field} - {message}")]
    Validation {
        /// Field name.
        field: String,
        /// Validation message.
        message: String,
    },

    /// Type conversion failed.
    #[error("Conversion error: {message}")]
    Conversion {
        /// Conversion error text.
        message: String,
    },

    /// Transaction failed.
    #[error("Transaction error: {message}")]
    Transaction {
        /// Transaction error text.
        message: String,
    },

    /// Configuration error.
    #[error("Configuration error: {message}")]
    Configuration {
        /// Configuration error text.
        message: String,
    },

    /// Internal error.
    #[error("Internal error: {message}")]
    Internal {
        /// Internal error text.
        message: String,
    },

    /// Operation is not supported by the active backend.
    #[error("Backend not supported: {message}")]
    BackendNotSupported {
        /// Unsupported-operation message.
        message: String,
        /// Backend name.
        backend: String,
    },

    /// Operation required a primary key that was not set.
    #[error("Primary key not set: {message}")]
    PrimaryKeyNotSet {
        /// Primary-key error text.
        message: String,
        /// Model name.
        model: String,
    },

    /// `INSERT ... RETURNING` is not supported by the active backend.
    #[error("Insert returning not supported: {message}")]
    InsertReturningNotSupported {
        /// Unsupported-RETURNING message.
        message: String,
        /// Backend name.
        backend: String,
    },

    /// Tokenization failed because configuration or encoding work could not proceed.
    #[error("Tokenization error: {message}")]
    Tokenization {
        /// Tokenization error text.
        message: String,
    },

    /// Token was invalid, mismatched, expired, or tampered.
    #[error("Invalid token: {message}")]
    InvalidToken {
        /// Invalid-token message.
        message: String,
    },
}

/// Extra rendered context attached to query-oriented errors.
#[derive(Debug, Clone)]
pub struct ErrorContext {
    /// Table name, if known.
    pub table: Option<String>,
    /// Column name, if known.
    pub column: Option<String>,
    /// Rendered conditions involved in the failure.
    pub conditions: Vec<String>,
    /// Logical operator chain for the rendered conditions.
    pub operator_chain: Option<String>,
    /// Rendered SQL query, if available.
    pub query: Option<String>,
}

impl std::fmt::Display for ErrorContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut parts = Vec::new();
        if let Some(ref table) = self.table {
            parts.push(format!("table: {}", table));
        }
        if let Some(ref column) = self.column {
            parts.push(format!("column: {}", column));
        }
        if !self.conditions.is_empty() {
            parts.push(format!("conditions: {}", self.conditions.join(" | ")));
        }
        if let Some(ref operator_chain) = self.operator_chain {
            parts.push(format!("operator_chain: {}", operator_chain));
        }
        if let Some(ref query) = self.query {
            parts.push(format!("query: {}", query));
        }
        write!(f, "{}", parts.join(", "))
    }
}

impl ErrorContext {
    /// Start building extra table, column, and query details for an error.
    pub fn new() -> Self {
        Self {
            table: None,
            column: None,
            conditions: Vec::new(),
            operator_chain: None,
            query: None,
        }
    }

    /// Attach the table name involved in the failure.
    pub fn table(mut self, table: impl Into<String>) -> Self {
        self.table = Some(table.into());
        self
    }

    /// Attach the column name involved in the failure.
    pub fn column(mut self, column: impl Into<String>) -> Self {
        self.column = Some(column.into());
        self
    }

    /// Add one rendered condition to the context.
    pub fn condition(mut self, condition: impl Into<String>) -> Self {
        self.conditions.push(condition.into());
        self
    }

    /// Replace the collected rendered conditions.
    pub fn conditions(mut self, conditions: Vec<String>) -> Self {
        self.conditions = conditions;
        self
    }

    /// Attach the rendered logical operator chain.
    pub fn operator_chain(mut self, operator_chain: impl Into<String>) -> Self {
        self.operator_chain = Some(operator_chain.into());
        self
    }

    /// Attach the rendered SQL query.
    pub fn query(mut self, query: impl Into<String>) -> Self {
        self.query = Some(query.into());
        self
    }
}

impl Default for ErrorContext {
    fn default() -> Self {
        Self::new()
    }
}

impl Error {
    /// Construct a missing-record error without extra context.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound {
            message: message.into(),
            context: None,
        }
    }

    /// Construct a missing-record error and attach table or query context.
    pub fn not_found_with_context(message: impl Into<String>, context: ErrorContext) -> Self {
        Self::NotFound {
            message: message.into(),
            context: Some(Box::new(context)),
        }
    }

    /// Construct a connection error.
    pub fn connection(message: impl Into<String>) -> Self {
        Self::Connection {
            message: message.into(),
        }
    }

    /// Construct a query error without extra context.
    pub fn query(message: impl Into<String>) -> Self {
        Self::Query {
            message: message.into(),
            context: None,
        }
    }

    /// Construct a query error and attach rendered SQL context.
    pub fn query_with_context(message: impl Into<String>, context: ErrorContext) -> Self {
        Self::Query {
            message: message.into(),
            context: Some(Box::new(context)),
        }
    }

    /// Construct a validation error for one field.
    pub fn validation(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Validation {
            field: field.into(),
            message: message.into(),
        }
    }

    /// Construct a conversion error.
    pub fn conversion(message: impl Into<String>) -> Self {
        Self::Conversion {
            message: message.into(),
        }
    }

    /// Construct a transaction error.
    pub fn transaction(message: impl Into<String>) -> Self {
        Self::Transaction {
            message: message.into(),
        }
    }

    /// Construct a configuration error.
    pub fn configuration(message: impl Into<String>) -> Self {
        Self::Configuration {
            message: message.into(),
        }
    }

    /// Construct an internal error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }

    /// Construct an error for a backend-specific unsupported operation.
    pub fn backend_not_supported(message: impl Into<String>, backend: impl Into<String>) -> Self {
        Self::BackendNotSupported {
            message: message.into(),
            backend: backend.into(),
        }
    }

    /// Construct an error for operations that require a missing primary key.
    pub fn primary_key_not_set(message: impl Into<String>, model: impl Into<String>) -> Self {
        Self::PrimaryKeyNotSet {
            message: message.into(),
            model: model.into(),
        }
    }

    /// Construct an error for `INSERT ... RETURNING` on an unsupported backend.
    pub fn insert_returning_not_supported(
        message: impl Into<String>,
        backend: impl Into<String>,
    ) -> Self {
        Self::InsertReturningNotSupported {
            message: message.into(),
            backend: backend.into(),
        }
    }

    /// Construct a tokenization error.
    pub fn tokenization(message: impl Into<String>) -> Self {
        Self::Tokenization {
            message: message.into(),
        }
    }

    /// Construct an invalid-token error.
    pub fn invalid_token(message: impl Into<String>) -> Self {
        Self::InvalidToken {
            message: message.into(),
        }
    }

    /// Construct a query-builder misuse error before any SQL runs.
    pub fn invalid_query(message: impl Into<String>) -> Self {
        Self::Query {
            message: message.into(),
            context: None,
        }
    }

    /// Return attached context for `NotFound` and `Query` errors.
    pub fn context(&self) -> Option<&ErrorContext> {
        match self {
            Self::NotFound { context, .. } => context.as_deref(),
            Self::Query { context, .. } => context.as_deref(),
            _ => None,
        }
    }

    /// Attach context to `NotFound` or `Query` errors.
    ///
    /// Other variants are returned unchanged.
    pub fn with_context(self, ctx: ErrorContext) -> Self {
        match self {
            Self::NotFound { message, .. } => Self::NotFound {
                message,
                context: Some(Box::new(ctx)),
            },
            Self::Query { message, .. } => Self::Query {
                message,
                context: Some(Box::new(ctx)),
            },
            other => other,
        }
    }

    /// True when the variant is `NotFound`.
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound { .. })
    }

    /// True when the variant is `Connection`.
    pub fn is_connection_error(&self) -> bool {
        matches!(self, Self::Connection { .. })
    }

    /// True when the variant is `Validation`.
    pub fn is_validation_error(&self) -> bool {
        matches!(self, Self::Validation { .. })
    }

    /// True when the variant is `Query`.
    pub fn is_query_error(&self) -> bool {
        matches!(self, Self::Query { .. })
    }

    /// True when the variant is `Transaction`.
    pub fn is_transaction_error(&self) -> bool {
        matches!(self, Self::Transaction { .. })
    }

    /// True when the variant is `Configuration`.
    pub fn is_configuration_error(&self) -> bool {
        matches!(self, Self::Configuration { .. })
    }

    /// True when the variant is `BackendNotSupported`.
    pub fn is_backend_not_supported(&self) -> bool {
        matches!(self, Self::BackendNotSupported { .. })
    }

    /// True when the variant is `PrimaryKeyNotSet`.
    pub fn is_primary_key_not_set(&self) -> bool {
        matches!(self, Self::PrimaryKeyNotSet { .. })
    }

    /// True when the variant is `InsertReturningNotSupported`.
    pub fn is_insert_returning_not_supported(&self) -> bool {
        matches!(self, Self::InsertReturningNotSupported { .. })
    }

    /// Return a user-facing next step derived from the variant and message.
    pub fn suggestion(&self) -> String {
        match self {
            Self::NotFound { message, .. } => {
                if message.contains("find") || message.contains("Find") {
                    "Check that the ID exists. Use `Model::exists(id).await?` to verify before find.".to_string()
                } else {
                    "Verify the record exists and hasn't been soft-deleted. Use `.with_trashed()` to include deleted records.".to_string()
                }
            }
            Self::Connection { message } => {
                if message.contains("refused") || message.contains("Refused") {
                    "Database server is not running or not accepting connections. Check that:\n\
                     1. The database server is running\n\
                     2. The host and port are correct\n\
                     3. Firewall allows the connection".to_string()
                } else if message.contains("password") || message.contains("authentication") {
                    "Check your database credentials in the connection URL.".to_string()
                } else if message.contains("does not exist") || message.contains("unknown database") {
                    "The database doesn't exist. Create it first: CREATE DATABASE dbname;".to_string()
                } else if message.contains("timeout") || message.contains("Timeout") {
                    "Connection timed out. Check network connectivity and increase `connect_timeout` if needed.".to_string()
                } else if message.contains("pool") || message.contains("Pool") {
                    "Connection pool exhausted. Consider:\n\
                     1. Increasing `max_connections` in TideConfig\n\
                     2. Reducing connection hold time\n\
                     3. Using `acquire_timeout` to wait for connections".to_string()
                } else {
                    "Verify your database URL format: postgres://user:pass@host:5432/database".to_string()
                }
            }
            Self::Query { message, context } => {
                let base_suggestion = if message.contains("syntax") || message.contains("Syntax") {
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
                };

                if let Some(ctx) = context {
                    if let Some(ref query) = ctx.query {
                        format!("{}\n\nQuery: {}", base_suggestion, query)
                    } else {
                        base_suggestion.to_string()
                    }
                } else {
                    base_suggestion.to_string()
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
            Self::Transaction { message } => {
                if message.contains("timeout") {
                    "Transaction timed out. Split into smaller transactions or increase timeout.".to_string()
                } else if message.contains("rollback") || message.contains("aborted") {
                    "Transaction was rolled back. Check for errors in transaction body.".to_string()
                } else {
                    "Transaction failed. Ensure all operations in the transaction are valid.".to_string()
                }
            }
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
            Self::Connection { .. } => 503, // Service Unavailable
            Self::Query { .. } => 400,      // Bad Request
            Self::Validation { .. } => 422, // Unprocessable Entity
            Self::Conversion { .. } => 400,
            Self::Transaction { .. } => 409, // Conflict
            Self::Configuration { .. } => 500,
            Self::Internal { .. } => 500,
            Self::BackendNotSupported { .. } => 501, // Not Implemented
            Self::PrimaryKeyNotSet { .. } => 400,    // Bad Request
            Self::InsertReturningNotSupported { .. } => 501, // Not Implemented
            Self::Tokenization { .. } => 400,        // Bad Request
            Self::InvalidToken { .. } => 401,        // Unauthorized
        }
    }

    /// Best-effort retry hint based on transient-looking error messages.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Connection { message } => {
                message.contains("timeout")
                    || message.contains("pool")
                    || message.contains("refused")
            }
            Self::Query { message, .. } => {
                message.contains("deadlock")
                    || message.contains("lock")
                    || message.contains("timeout")
            }
            Self::Transaction { message } => {
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

        output.push_str(&format!("\n  Suggestion: {}", self.suggestion()));
        output
    }
}

#[cfg(test)]
#[path = "testing/error_tests.rs"]
mod tests;
