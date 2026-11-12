//! The driver's classification of a failed statement.

use std::fmt::{self, Write as _};

/// How the database classified a failed statement.
///
/// The driver reads its backend's own report: PostgreSQL's SQLSTATE, MySQL's
/// error number and SQLSTATE, SQLite's result code and message.
/// [`DbFailureKind::Unclassified`] means the driver gave TideORM nothing to go
/// on; callers that need more should fall back to the error message.
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
    /// PostgreSQL `EXCLUDE` constraint violation, such as overlapping
    /// reservations (SQLSTATE `23P01`).
    ExclusionViolation,
    /// A value does not fit its column: too long, out of range, or not valid
    /// for the column's type (SQLSTATE class `22`, such as `22001`).
    InvalidValue,
    /// The statement did not parse (SQLSTATE `42601`, MySQL `42000`).
    SyntaxError,
    /// A referenced column does not exist (SQLSTATE `42703`, MySQL `42S22`).
    UndefinedColumn,
    /// A referenced table does not exist (SQLSTATE `42P01`, MySQL `42S02`).
    UndefinedTable,
    /// The database user lacks the required privilege (SQLSTATE `42501`).
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
    /// The server refused the connection: nothing listens at the host and
    /// port, or the server is not up yet.
    ConnectionRefused,
    /// The server rejected the user or password (SQLSTATE `28P01`, `28000`,
    /// MySQL `1045`).
    AuthenticationFailed,
    /// The database the connection names does not exist (SQLSTATE `3D000`,
    /// MySQL `1049`).
    UnknownDatabase,
    /// The TLS handshake with the server failed.
    TlsFailed,
    /// The connection URL or one of its options is invalid.
    InvalidConfiguration,
    /// No connection is set up: the global connection was never initialized,
    /// or the handle is disconnected.
    NotConnected,
}

impl DbFailureKind {
    /// Short human-readable name for the classification.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unclassified => "database error",
            Self::UniqueViolation => "unique constraint violation",
            Self::ForeignKeyViolation => "foreign key constraint violation",
            Self::NotNullViolation => "not-null constraint violation",
            Self::CheckViolation => "check constraint violation",
            Self::ExclusionViolation => "exclusion constraint violation",
            Self::InvalidValue => "invalid value",
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
            Self::ConnectionRefused => "connection refused",
            Self::AuthenticationFailed => "authentication failed",
            Self::UnknownDatabase => "unknown database",
            Self::TlsFailed => "TLS failure",
            Self::InvalidConfiguration => "invalid connection settings",
            Self::NotConnected => "not connected",
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
                | Self::ExclusionViolation
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
                | Self::ConnectionRefused
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
/// What survives TideORM's error translation: the classification, the
/// SQLSTATE, and the constraint and table names where the driver exposes them.
///
/// It is the [`source`](std::error::Error::source) of the [`Error`](enum@super::Error) it is
/// attached to, and its own source is the originating driver error, so
/// `{:#}`-style chains and `anyhow` interop walk all the way down to the
/// backend. Reach it directly with [`Error::db_failure`](super::Error::db_failure).
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

impl DbFailure {
    /// The SQLSTATE and constraint the driver reported, for appending to a
    /// description; empty when it exposed neither, the normal case on backends
    /// that only report a message.
    pub(crate) fn detail(&self) -> String {
        let mut detail = String::new();
        if let Some(ref code) = self.code {
            let _ = write!(detail, " (SQLSTATE {code})");
        }
        if let Some(ref constraint) = self.constraint {
            let _ = write!(detail, " on constraint `{constraint}`");
        }
        detail
    }
}

impl fmt::Display for DbFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.kind, self.detail())?;
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
