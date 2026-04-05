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

mod context;
mod presentation;

pub use context::ErrorContext;

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
}

#[cfg(test)]
#[path = "../tests/unit/error_tests.rs"]
mod tests;
