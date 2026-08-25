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
//! - inspect `db_failure()` when you need the SQLSTATE or the name of the
//!   constraint the database rejected the statement on
//! - use `code()` and `http_status()` only when you need stable external handling for logs or APIs
//!
//! Errors that originate at the driver keep the originating error as their
//! [`source`](std::error::Error::source), so `{:#}`-style chains and `anyhow`
//! interop reach the backend instead of stopping at TideORM's rendered message.

use std::fmt;

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

/// How the database classified a failed statement.
///
/// Derived from the driver's own constraint-violation kind first, then from the
/// SQLSTATE the backend reported. [`DbFailureKind::Unclassified`] means the
/// driver gave TideORM nothing to go on; callers that need more should fall
/// back to the error message.
///
/// Marked `#[non_exhaustive]`: classifications are added as drivers expose
/// them, so match with a wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum DbFailureKind {
    /// The driver reported no code TideORM recognizes.
    #[default]
    Unclassified,
    /// Unique or primary-key constraint violation (SQLSTATE `23505`).
    UniqueViolation,
    /// Foreign-key constraint violation (SQLSTATE `23503`).
    ForeignKeyViolation,
    /// `NOT NULL` constraint violation (SQLSTATE `23502`).
    NotNullViolation,
    /// `CHECK` constraint violation (SQLSTATE `23514`).
    CheckViolation,
    /// The statement did not parse (SQLSTATE `42601`, MySQL `42000`).
    SyntaxError,
    /// A referenced column does not exist (SQLSTATE `42703`, MySQL `42S22`).
    UndefinedColumn,
    /// A referenced table does not exist (SQLSTATE `42P01`, MySQL `42S02`).
    UndefinedTable,
    /// The database user lacks the required privilege (SQLSTATE `42501`, `28000`).
    InsufficientPrivilege,
    /// Deadlock detected (SQLSTATE `40P01`).
    Deadlock,
    /// Serialization failure; the transaction can be replayed (SQLSTATE `40001`).
    SerializationFailure,
    /// A lock could not be acquired (SQLSTATE `55P03`).
    LockNotAvailable,
    /// The statement was cancelled, usually by a timeout (SQLSTATE `57014`).
    StatementTimeout,
    /// No connection could be obtained in time (SQLSTATE `53300`, or a pool
    /// acquire timeout reported by the driver itself).
    ConnectionTimeout,
    /// The connection was closed underneath the statement (SQLSTATE class `08`).
    ConnectionClosed,
}

impl DbFailureKind {
    /// Classify a SQLSTATE reported by the backend.
    ///
    /// Expects the five-character SQLSTATE that PostgreSQL and MySQL return.
    /// SQLite reports a native numeric code with no SQLSTATE meaning, so it
    /// classifies as [`DbFailureKind::Unclassified`] here — the driver's own
    /// constraint-violation kind is consulted first and already covers SQLite's
    /// unique, foreign-key and not-null cases.
    pub fn from_sqlstate(sqlstate: &str) -> Self {
        match sqlstate {
            "23505" => Self::UniqueViolation,
            "23503" => Self::ForeignKeyViolation,
            "23502" => Self::NotNullViolation,
            "23514" => Self::CheckViolation,
            "42601" | "42000" => Self::SyntaxError,
            "42703" | "42S22" => Self::UndefinedColumn,
            "42P01" | "42S02" => Self::UndefinedTable,
            "42501" | "28000" => Self::InsufficientPrivilege,
            "40P01" => Self::Deadlock,
            "40001" => Self::SerializationFailure,
            "55P03" => Self::LockNotAvailable,
            "57014" => Self::StatementTimeout,
            "53300" => Self::ConnectionTimeout,
            "08000" | "08001" | "08003" | "08004" | "08006" | "08007" | "57P01" | "57P02"
            | "57P03" => Self::ConnectionClosed,
            _ => Self::Unclassified,
        }
    }

    /// Short human-readable name for the classification.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unclassified => "database error",
            Self::UniqueViolation => "unique constraint violation",
            Self::ForeignKeyViolation => "foreign key constraint violation",
            Self::NotNullViolation => "not-null constraint violation",
            Self::CheckViolation => "check constraint violation",
            Self::SyntaxError => "SQL syntax error",
            Self::UndefinedColumn => "undefined column",
            Self::UndefinedTable => "undefined table",
            Self::InsufficientPrivilege => "insufficient privilege",
            Self::Deadlock => "deadlock",
            Self::SerializationFailure => "serialization failure",
            Self::LockNotAvailable => "lock not available",
            Self::StatementTimeout => "statement timeout",
            Self::ConnectionTimeout => "connection timeout",
            Self::ConnectionClosed => "connection closed",
        }
    }

    /// True when the failure is a constraint violation, which the caller fixes
    /// by changing the data it is writing rather than by retrying.
    pub fn is_constraint_violation(self) -> bool {
        matches!(
            self,
            Self::UniqueViolation
                | Self::ForeignKeyViolation
                | Self::NotNullViolation
                | Self::CheckViolation
        )
    }

    /// True when replaying the statement can plausibly succeed.
    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::Deadlock
                | Self::SerializationFailure
                | Self::LockNotAvailable
                | Self::StatementTimeout
                | Self::ConnectionTimeout
                | Self::ConnectionClosed
        )
    }
}

impl fmt::Display for DbFailureKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The driver-level failure behind a database error.
///
/// TideORM translates driver errors at a single boundary, and that boundary
/// used to keep only the rendered message — so a caller could not tell a unique
/// violation from a foreign-key violation, let alone learn which constraint
/// fired. This is what survives translation instead: the classification, the
/// SQLSTATE, and the constraint and table names where the driver exposes them.
///
/// It is the [`source`](std::error::Error::source) of the [`Error`](enum@Error) it is
/// attached to, and its own source is the originating driver error, so
/// `{:#}`-style chains and `anyhow` interop walk all the way down to the
/// backend. Reach it directly with [`Error::db_failure`].
#[derive(Debug)]
pub struct DbFailure {
    kind: DbFailureKind,
    code: Option<String>,
    constraint: Option<String>,
    table: Option<String>,
    source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
}

impl DbFailure {
    /// Start a failure carrying only its classification.
    pub(crate) fn new(kind: DbFailureKind) -> Self {
        Self {
            kind,
            code: None,
            constraint: None,
            table: None,
            source: None,
        }
    }

    /// Attach the SQLSTATE, or the driver-native code where there is none.
    pub(crate) fn with_code(mut self, code: Option<String>) -> Self {
        self.code = code;
        self
    }

    /// Attach the name of the constraint the backend reported as violated.
    pub(crate) fn with_constraint(mut self, constraint: Option<String>) -> Self {
        self.constraint = constraint;
        self
    }

    /// Attach the table the failing statement was operating on.
    pub(crate) fn with_table(mut self, table: Option<String>) -> Self {
        self.table = table;
        self
    }

    /// Keep the originating driver error as this failure's own source.
    pub(crate) fn with_source(
        mut self,
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    ) -> Self {
        self.source = Some(source);
        self
    }

    /// How the database classified the failure.
    pub fn kind(&self) -> DbFailureKind {
        self.kind
    }

    /// SQLSTATE reported by the backend, or the driver's native code where the
    /// backend has no SQLSTATE (SQLite).
    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    /// Name of the constraint the backend reported as violated.
    ///
    /// Only PostgreSQL populates this today; MySQL and SQLite name the
    /// constraint inside the message instead.
    pub fn constraint(&self) -> Option<&str> {
        self.constraint.as_deref()
    }

    /// Table the failing statement was operating on, when the backend reports it.
    pub fn table(&self) -> Option<&str> {
        self.table.as_deref()
    }
}

impl fmt::Display for DbFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)?;
        if let Some(ref code) = self.code {
            write!(f, " (SQLSTATE {})", code)?;
        }
        if let Some(ref constraint) = self.constraint {
            write!(f, " on constraint `{}`", constraint)?;
        }
        if let Some(ref table) = self.table {
            write!(f, " in table `{}`", table)?;
        }
        Ok(())
    }
}

impl std::error::Error for DbFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn std::error::Error + 'static))
    }
}

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

    /// Construct a query error and attach rendered SQL context.
    pub fn query_with_context(message: impl Into<String>, context: ErrorContext) -> Self {
        Self::Query {
            message: message.into(),
            context: Some(Box::new(context)),
            source: None,
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
            source: None,
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

    /// True when the database reported a unique or primary-key violation.
    ///
    /// Answered from the driver's own classification, so it does not depend on
    /// the wording of the backend's message.
    pub fn is_unique_violation(&self) -> bool {
        self.failure_kind() == DbFailureKind::UniqueViolation
    }

    /// True when the database reported a foreign-key violation.
    pub fn is_foreign_key_violation(&self) -> bool {
        self.failure_kind() == DbFailureKind::ForeignKeyViolation
    }

    /// True when the database reported a `NOT NULL` violation.
    pub fn is_not_null_violation(&self) -> bool {
        self.failure_kind() == DbFailureKind::NotNullViolation
    }

    /// True when the database reported a `CHECK` violation.
    pub fn is_check_violation(&self) -> bool {
        self.failure_kind() == DbFailureKind::CheckViolation
    }

    /// True when the database rejected the statement on any constraint.
    pub fn is_constraint_violation(&self) -> bool {
        self.failure_kind().is_constraint_violation()
    }
}

#[cfg(test)]
#[path = "../tests/unit/error_tests.rs"]
mod tests;
