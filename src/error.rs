//! Error types
//!
//! All database errors are translated into these types before reaching user code.
//!
//! ## Error Handling
//!
//! provides detailed, actionable error messages to help you debug issues quickly:
//!
//! ```rust,ignore
//! use tideorm::prelude::*;
//!
//! // Errors include helpful context
//! match User::find(999).await {
//!     Ok(user) => println!("Found: {}", user.name),
//!     Err(e) => {
//!         eprintln!("Error: {}", e);
//!         eprintln!("Suggestion: {}", e.suggestion());
//!         if let Some(ctx) = e.context() {
//!             eprintln!("Table: {:?}", ctx.table);
//!         }
//!     }
//! }
//! ```
//!
//! ## Error Types
//!
//! | Error Type | Description | Common Causes |
//! |------------|-------------|---------------|
//! | `NotFound` | Record doesn't exist | Wrong ID, deleted record |
//! | `Connection` | Can't connect to database | Wrong URL, DB down |
//! | `Query` | SQL execution failed | Syntax error, constraint violation |
//! | `Validation` | Data validation failed | Invalid input |
//! | `Transaction` | Transaction failed | Deadlock, timeout |
//! | `Configuration` | Config issue | Missing settings |

use thiserror::Error;

// ── From impls for common external error types ─────────────────────

impl From<sea_orm::DbErr> for Error {
    fn from(err: sea_orm::DbErr) -> Self {
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

/// A specialized Result type for TideORM operations
///
/// Note: Named `TideResult` to avoid conflicts with `std::result::Result` in derive macros.
/// You can also use the re-exported `Result` from the prelude in most contexts.
pub type Result<T> = std::result::Result<T, Error>;

/// The main error type for TideORM
///
/// These errors are designed to be helpful and actionable for developers.
/// They never expose internal ORM implementation details.
#[derive(Error, Debug)]
pub enum Error {
    /// Record was not found in the database
    #[error("Record not found: {message}")]
    NotFound {
        /// Description of what was not found
        message: String,
        /// Optional table name for context
        context: Option<Box<ErrorContext>>,
    },

    /// Database connection failed
    #[error("Connection error: {message}")]
    Connection {
        /// Details about the connection failure
        message: String,
    },

    /// Query execution failed
    #[error("Query error: {message}")]
    Query {
        /// Details about the query failure
        message: String,
        /// Optional context about the query
        context: Option<Box<ErrorContext>>,
    },

    /// Data validation failed
    #[error("Validation error: {field} - {message}")]
    Validation {
        /// The field that failed validation
        field: String,
        /// Description of the validation failure
        message: String,
    },

    /// Type conversion failed
    #[error("Conversion error: {message}")]
    Conversion {
        /// Details about the conversion failure
        message: String,
    },

    /// Transaction failed
    #[error("Transaction error: {message}")]
    Transaction {
        /// Details about the transaction failure
        message: String,
    },

    /// Configuration error
    #[error("Configuration error: {message}")]
    Configuration {
        /// Details about the configuration issue
        message: String,
    },

    /// Internal error (should rarely happen)
    #[error("Internal error: {message}")]
    Internal {
        /// Details about the internal error
        message: String,
    },

    /// Backend not supported for the requested operation
    ///
    /// Thrown when an operation is attempted that is not supported
    /// by the current database backend.
    #[error("Backend not supported: {message}")]
    BackendNotSupported {
        /// Details about what operation is not supported
        message: String,
        /// The backend that doesn't support the operation
        backend: String,
    },

    /// Primary key not set when required
    ///
    /// Thrown when an operation requires a primary key value
    /// but the model instance doesn't have one set.
    #[error("Primary key not set: {message}")]
    PrimaryKeyNotSet {
        /// Details about the error
        message: String,
        /// The model type involved
        model: String,
    },

    /// Insert with RETURNING not supported by this backend
    ///
    /// Thrown when trying to use insert().returning() on a database
    /// that doesn't support the RETURNING clause.
    #[error("Insert returning not supported: {message}")]
    InsertReturningNotSupported {
        /// Details about the error
        message: String,
        /// The backend that doesn't support RETURNING
        backend: String,
    },

    /// Tokenization error
    ///
    /// Thrown when tokenization operations fail, such as encoding
    /// a record to a token or decoding a token back to a record ID.
    #[error("Tokenization error: {message}")]
    Tokenization {
        /// Details about the tokenization error
        message: String,
    },

    /// Invalid token error
    ///
    /// Thrown when attempting to decode an invalid, expired, or
    /// tampered token.
    #[error("Invalid token: {message}")]
    InvalidToken {
        /// Details about why the token is invalid
        message: String,
    },
}

/// Additional context for errors
#[derive(Debug, Clone)]
pub struct ErrorContext {
    /// Table name involved in the error
    pub table: Option<String>,
    /// Column name involved in the error
    pub column: Option<String>,
    /// Rendered query conditions involved in the error
    pub conditions: Vec<String>,
    /// Logical operator chain used to combine the conditions
    pub operator_chain: Option<String>,
    /// The SQL query that caused the error (if available)
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
    /// Create a new error context
    pub fn new() -> Self {
        Self {
            table: None,
            column: None,
            conditions: Vec::new(),
            operator_chain: None,
            query: None,
        }
    }

    /// Set the table name
    pub fn table(mut self, table: impl Into<String>) -> Self {
        self.table = Some(table.into());
        self
    }

    /// Set the column name
    pub fn column(mut self, column: impl Into<String>) -> Self {
        self.column = Some(column.into());
        self
    }

    /// Add a rendered condition to the context
    pub fn condition(mut self, condition: impl Into<String>) -> Self {
        self.conditions.push(condition.into());
        self
    }

    /// Set all rendered conditions for the context
    pub fn conditions(mut self, conditions: Vec<String>) -> Self {
        self.conditions = conditions;
        self
    }

    /// Set the logical operator chain
    pub fn operator_chain(mut self, operator_chain: impl Into<String>) -> Self {
        self.operator_chain = Some(operator_chain.into());
        self
    }

    /// Set the query
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
    /// Create a NotFound error
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound {
            message: message.into(),
            context: None,
        }
    }

    /// Create a NotFound error with context
    pub fn not_found_with_context(message: impl Into<String>, context: ErrorContext) -> Self {
        Self::NotFound {
            message: message.into(),
            context: Some(Box::new(context)),
        }
    }

    /// Create a Connection error
    pub fn connection(message: impl Into<String>) -> Self {
        Self::Connection {
            message: message.into(),
        }
    }

    /// Create a Query error
    pub fn query(message: impl Into<String>) -> Self {
        Self::Query {
            message: message.into(),
            context: None,
        }
    }

    /// Create a Query error with context
    pub fn query_with_context(message: impl Into<String>, context: ErrorContext) -> Self {
        Self::Query {
            message: message.into(),
            context: Some(Box::new(context)),
        }
    }

    /// Create a Validation error
    pub fn validation(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Validation {
            field: field.into(),
            message: message.into(),
        }
    }

    /// Create a Conversion error
    pub fn conversion(message: impl Into<String>) -> Self {
        Self::Conversion {
            message: message.into(),
        }
    }

    /// Create a Transaction error
    pub fn transaction(message: impl Into<String>) -> Self {
        Self::Transaction {
            message: message.into(),
        }
    }

    /// Create a Configuration error
    pub fn configuration(message: impl Into<String>) -> Self {
        Self::Configuration {
            message: message.into(),
        }
    }

    /// Create an Internal error
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }

    /// Create a BackendNotSupported error
    ///
    /// Use when an operation is not supported by the current database backend.
    pub fn backend_not_supported(message: impl Into<String>, backend: impl Into<String>) -> Self {
        Self::BackendNotSupported {
            message: message.into(),
            backend: backend.into(),
        }
    }

    /// Create a PrimaryKeyNotSet error
    ///
    /// Use when an operation requires a primary key but it's not set.
    pub fn primary_key_not_set(message: impl Into<String>, model: impl Into<String>) -> Self {
        Self::PrimaryKeyNotSet {
            message: message.into(),
            model: model.into(),
        }
    }

    /// Create an InsertReturningNotSupported error
    ///
    /// Use when trying to use RETURNING on a database that doesn't support it.
    pub fn insert_returning_not_supported(
        message: impl Into<String>,
        backend: impl Into<String>,
    ) -> Self {
        Self::InsertReturningNotSupported {
            message: message.into(),
            backend: backend.into(),
        }
    }

    /// Create a Tokenization error
    ///
    /// Use when token encoding/decoding fails.
    pub fn tokenization(message: impl Into<String>) -> Self {
        Self::Tokenization {
            message: message.into(),
        }
    }

    /// Create an InvalidToken error
    ///
    /// Use when a token is invalid, tampered, or for the wrong model.
    pub fn invalid_token(message: impl Into<String>) -> Self {
        Self::InvalidToken {
            message: message.into(),
        }
    }

    /// Create an invalid query error (semantic query issues, not DB errors)
    ///
    /// Use this for errors like using soft_delete() on a non-soft-delete model,
    /// or other query builder usage errors.
    pub fn invalid_query(message: impl Into<String>) -> Self {
        Self::Query {
            message: message.into(),
            context: None,
        }
    }

    /// Get the error context if available
    pub fn context(&self) -> Option<&ErrorContext> {
        match self {
            Self::NotFound { context, .. } => context.as_deref(),
            Self::Query { context, .. } => context.as_deref(),
            _ => None,
        }
    }

    /// Add context to an error
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

    /// Check if this is a NotFound error
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound { .. })
    }

    /// Check if this is a Connection error
    pub fn is_connection_error(&self) -> bool {
        matches!(self, Self::Connection { .. })
    }

    /// Check if this is a Validation error
    pub fn is_validation_error(&self) -> bool {
        matches!(self, Self::Validation { .. })
    }

    /// Check if this is a Query error
    pub fn is_query_error(&self) -> bool {
        matches!(self, Self::Query { .. })
    }

    /// Check if this is a Transaction error
    pub fn is_transaction_error(&self) -> bool {
        matches!(self, Self::Transaction { .. })
    }

    /// Check if this is a Configuration error
    pub fn is_configuration_error(&self) -> bool {
        matches!(self, Self::Configuration { .. })
    }

    /// Check if this is a BackendNotSupported error
    pub fn is_backend_not_supported(&self) -> bool {
        matches!(self, Self::BackendNotSupported { .. })
    }

    /// Check if this is a PrimaryKeyNotSet error
    pub fn is_primary_key_not_set(&self) -> bool {
        matches!(self, Self::PrimaryKeyNotSet { .. })
    }

    /// Check if this is an InsertReturningNotSupported error
    pub fn is_insert_returning_not_supported(&self) -> bool {
        matches!(self, Self::InsertReturningNotSupported { .. })
    }

    /// Get a helpful suggestion for fixing this error
    ///
    /// Returns actionable advice based on the error type and message.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// match User::find(999).await {
    ///     Err(e) => {
    ///         eprintln!("Error: {}", e);
    ///         eprintln!("Suggestion: {}", e.suggestion());
    ///     }
    ///     Ok(user) => println!("Found user"),
    /// }
    /// ```
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

    /// Get the error code for programmatic handling
    ///
    /// Returns a unique code for each error type that can be used
    /// for error handling, logging, or API responses.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let error_response = json!({
    ///     "error": {
    ///         "code": e.code(),
    ///         "message": e.to_string(),
    ///         "suggestion": e.suggestion(),
    ///     }
    /// });
    /// ```
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

    /// Get the HTTP status code appropriate for this error
    ///
    /// Useful when building REST APIs.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // In an Actix-web handler
    /// match result {
    ///     Ok(data) => HttpResponse::Ok().json(data),
    ///     Err(e) => HttpResponse::build(
    ///         actix_web::http::StatusCode::from_u16(e.http_status()).unwrap()
    ///     ).json(json!({"error": e.to_string()})),
    /// }
    /// ```
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

    /// Check if this error is retryable
    ///
    /// Some errors (like connection timeouts or deadlocks) may succeed on retry.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut retries = 3;
    /// loop {
    ///     match operation().await {
    ///         Ok(result) => return Ok(result),
    ///         Err(e) if e.is_retryable() && retries > 0 => {
    ///             retries -= 1;
    ///             tokio::time::sleep(Duration::from_millis(100)).await;
    ///             continue;
    ///         }
    ///         Err(e) => return Err(e),
    ///     }
    /// }
    /// ```
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

    /// Format error for logging with full details
    ///
    /// Includes error type, message, context, and suggestion.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// tracing::error!("{}", e.log_format());
    /// ```
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
mod tests {
    use super::*;
    use std::error::Error as StdError;

    #[test]
    fn test_error_suggestions() {
        // Test NotFound suggestion
        let err = Error::not_found("User with ID 123 not found");
        let suggestion = err.suggestion();
        assert!(
            suggestion.contains("verify")
                || suggestion.contains("exists")
                || suggestion.contains("ID")
                || suggestion.contains("record")
        );

        // Test Connection suggestion for refused
        let err = Error::connection("Connection refused");
        let suggestion = err.suggestion();
        assert!(suggestion.contains("running") || suggestion.contains("server"));

        // Test Query suggestion for duplicate
        let err = Error::query("duplicate key value violates unique constraint");
        let suggestion = err.suggestion();
        assert!(suggestion.contains("Duplicate") || suggestion.contains("unique"));

        // Test Query suggestion for foreign key
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
        // Connection errors with timeout should be retryable
        let err = Error::connection("Connection timeout");
        assert!(err.is_retryable());

        // Connection errors with pool should be retryable
        let err = Error::connection("connection pool exhausted");
        assert!(err.is_retryable());

        // Query errors with deadlock should be retryable
        let err = Error::query("deadlock detected");
        assert!(err.is_retryable());

        // Regular query errors should not be retryable
        let err = Error::query("syntax error");
        assert!(!err.is_retryable());

        // Not found is not retryable
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
}
