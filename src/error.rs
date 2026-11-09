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
//! - match on `failure_kind()` to tell a unique violation from a foreign-key
//!   violation, a deadlock, or a timeout
//! - inspect `db_failure()` when you need the SQLSTATE or the name of the
//!   constraint the database rejected the statement on
//! - use `code()` and `http_status()` only when you need stable external handling for logs or APIs
//!
//! Errors that originate at the driver keep the originating error as their
//! [`source`](std::error::Error::source), so `{:#}`-style chains and `anyhow`
//! interop reach the backend instead of stopping at TideORM's rendered message.

use thiserror::Error;

mod context;
mod db_failure;
mod presentation;

pub use context::ErrorContext;
pub use db_failure::{DbFailure, DbFailureKind};

impl From<crate::internal::OrmError> for Error {
    fn from(err: crate::internal::OrmError) -> Self {
        crate::internal::translate_error(err)
    }
}

/// The `io::Error` stays reachable as the error's
/// [`source`](std::error::Error::source).
impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::Internal {
            message: err.to_string(),
            source: Some(Box::new(err)),
        }
    }
}

/// The `serde_json::Error`, with its line and column, stays reachable as the
/// error's [`source`](std::error::Error::source).
impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Self::Conversion {
            message: err.to_string(),
            source: Some(Box::new(err)),
        }
    }
}

/// Result alias for TideORM operations.
pub type Result<T> = std::result::Result<T, Error>;

/// The main error type for TideORM.
///
/// The variants are grouped by failure source so callers can decide whether to
/// retry, fix input, or stop and inspect configuration.
///
/// The three variants a driver error can land on — `Connection`, `Query` and
/// `Transaction` — carry a [`DbFailure`] as their
/// [`source`](std::error::Error::source) when the translation boundary
/// recovered one. That is what makes SQLSTATE codes and constraint names
/// reachable; see [`Error::db_failure`].
///
/// Marked `#[non_exhaustive]`: variants are added as TideORM learns to
/// distinguish more failures, so match with a wildcard arm.
#[derive(Error, Debug)]
#[non_exhaustive]
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
        /// Structured driver failure, when the error came from a driver.
        #[source]
        source: Option<Box<DbFailure>>,
    },

    /// Query building or execution failed.
    #[error("Query error: {message}")]
    Query {
        /// Backend or validation error text.
        message: String,
        /// Optional rendered SQL context.
        context: Option<Box<ErrorContext>>,
        /// Structured driver failure, when the error came from a driver.
        ///
        /// Absent for query-builder misuse, which never reaches the database.
        #[source]
        source: Option<Box<DbFailure>>,
    },

    /// Validation failed before the write reached the database.
    ///
    /// `field` and `message` name the first failure; `errors` holds every
    /// one, as [`Validate::validate`](crate::validation::Validate::validate)
    /// collected them.
    #[error("Validation error: {field} - {message}")]
    Validation {
        /// The first failing field.
        field: String,
        /// The first failure's message.
        message: String,
        /// Every failure, the first one included.
        errors: Box<crate::validation::ValidationErrors>,
    },

    /// Type conversion failed.
    #[error("Conversion error: {message}")]
    Conversion {
        /// Conversion error text.
        message: String,
        /// The error the conversion failed with, when it came from one: a
        /// `serde_json::Error` turned into this with `?`.
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Transaction failed.
    #[error("Transaction error: {message}")]
    Transaction {
        /// Transaction error text.
        message: String,
        /// Structured driver failure, when the error came from a driver.
        #[source]
        source: Option<Box<DbFailure>>,
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
        /// The error behind it, when it came from one: an `std::io::Error`
        /// turned into this with `?`.
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// An access-control check rejected the operation.
    #[error("Access denied: cannot perform `{permission}` on `{resource}`")]
    AccessDenied {
        /// Permission that was required.
        permission: String,
        /// Resource the permission was required on.
        resource: String,
    },

    /// A role-based access-control check could not be evaluated.
    #[error("RBAC error: {message}")]
    Rbac {
        /// Access-control error text.
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

    /// Construct a connection error.
    pub fn connection(message: impl Into<String>) -> Self {
        Self::Connection {
            message: message.into(),
            source: None,
        }
    }

    /// Construct a query error without extra context.
    pub fn query(message: impl Into<String>) -> Self {
        Self::Query {
            message: message.into(),
            context: None,
            source: None,
        }
    }

    /// Construct a validation error for one field.
    pub fn validation(field: impl Into<String>, message: impl Into<String>) -> Self {
        let mut errors = crate::validation::ValidationErrors::new();
        errors.add(field, message);
        errors.into()
    }

    /// Construct a conversion error.
    pub fn conversion(message: impl Into<String>) -> Self {
        Self::Conversion {
            message: message.into(),
            source: None,
        }
    }

    /// Construct a transaction error.
    pub fn transaction(message: impl Into<String>) -> Self {
        Self::Transaction {
            message: message.into(),
            source: None,
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
            source: None,
        }
    }

    /// Construct an access-denied error for a rejected permission check.
    pub fn access_denied(permission: impl Into<String>, resource: impl Into<String>) -> Self {
        Self::AccessDenied {
            permission: permission.into(),
            resource: resource.into(),
        }
    }

    /// Construct an error for an access-control check that could not run.
    pub fn rbac(message: impl Into<String>) -> Self {
        Self::Rbac {
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

    /// Return attached context for `NotFound` and `Query` errors.
    pub fn context(&self) -> Option<&ErrorContext> {
        match self {
            Self::NotFound { context, .. } | Self::Query { context, .. } => context.as_deref(),
            _ => None,
        }
    }

    /// Attach context to `NotFound` or `Query` errors.
    ///
    /// Other variants are returned unchanged. An already-attached driver
    /// failure survives, so context can be added after translation without
    /// losing the SQLSTATE or the source chain.
    pub fn with_context(self, ctx: ErrorContext) -> Self {
        match self {
            Self::NotFound { message, .. } => Self::NotFound {
                message,
                context: Some(Box::new(ctx)),
            },
            Self::Query {
                message, source, ..
            } => Self::Query {
                message,
                context: Some(Box::new(ctx)),
                source,
            },
            other => other,
        }
    }

    /// Attach the driver failure recovered at the translation boundary.
    ///
    /// Variants that never originate from a driver are returned unchanged, and
    /// a `None` failure leaves the error untouched, so callers can pass the
    /// result of a best-effort recovery straight through.
    pub(crate) fn with_db_failure(self, failure: Option<DbFailure>) -> Self {
        let Some(failure) = failure else {
            return self;
        };
        let failure = Some(Box::new(failure));

        match self {
            Self::Connection { message, .. } => Self::Connection {
                message,
                source: failure,
            },
            Self::Query {
                message, context, ..
            } => Self::Query {
                message,
                context,
                source: failure,
            },
            Self::Transaction { message, .. } => Self::Transaction {
                message,
                source: failure,
            },
            other => other,
        }
    }

    /// Take the driver failure out of a database error.
    ///
    /// Used where a translated error is reclassified onto a different variant
    /// and the structured driver detail has to survive the move.
    pub(crate) fn into_db_failure(self) -> Option<Box<DbFailure>> {
        match self {
            Self::Connection { source, .. }
            | Self::Query { source, .. }
            | Self::Transaction { source, .. } => source,
            _ => None,
        }
    }

    /// Return the structured driver failure behind this error.
    ///
    /// `None` when the failure never reached a driver — query-builder misuse,
    /// validation, configuration — or when the driver exposed nothing to
    /// recover.
    pub fn db_failure(&self) -> Option<&DbFailure> {
        match self {
            Self::Connection { source, .. }
            | Self::Query { source, .. }
            | Self::Transaction { source, .. } => source.as_deref(),
            _ => None,
        }
    }

    /// SQLSTATE the backend reported, or its native code where it has none.
    pub fn sqlstate(&self) -> Option<&str> {
        self.db_failure().and_then(DbFailure::code)
    }

    /// Name of the constraint the backend reported as violated.
    ///
    /// Only PostgreSQL populates this; see [`DbFailure::constraint`].
    pub fn constraint(&self) -> Option<&str> {
        self.db_failure().and_then(DbFailure::constraint)
    }

    /// How the database classified this failure.
    ///
    /// [`DbFailureKind::Unclassified`] when nothing structured survived, which
    /// is also what non-database errors report.
    pub fn failure_kind(&self) -> DbFailureKind {
        self.db_failure()
            .map_or(DbFailureKind::Unclassified, DbFailure::kind)
    }

    /// True when the database reported a foreign-key violation.
    ///
    /// Answered from the driver's own classification, so it does not depend on
    /// the wording of the backend's message.
    pub fn is_foreign_key_violation(&self) -> bool {
        self.failure_kind() == DbFailureKind::ForeignKeyViolation
    }

    /// True when the database reported a unique or primary-key violation,
    /// classified as [`is_foreign_key_violation`](Self::is_foreign_key_violation) is.
    pub fn is_unique_violation(&self) -> bool {
        self.failure_kind() == DbFailureKind::UniqueViolation
    }

    /// True when the database reported a `NOT NULL` violation, classified as
    /// [`is_foreign_key_violation`](Self::is_foreign_key_violation) is.
    pub fn is_not_null_violation(&self) -> bool {
        self.failure_kind() == DbFailureKind::NotNullViolation
    }

    /// Every failure behind an [`Error::Validation`], in the order the
    /// fields failed; `None` for any other error.
    pub fn validation_errors(&self) -> Option<&crate::validation::ValidationErrors> {
        match self {
            Self::Validation { errors, .. } => Some(errors),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/error_tests.rs"]
mod tests;
