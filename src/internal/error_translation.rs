//! Engine errors translated into TideORM's, with the driver's
//! classification of the failure carried along.

use crate::error::{DbFailure, DbFailureKind, Error};

#[cfg(feature = "postgres")]
use super::driver;
use super::{ConnAcquireErr, OrmError, RuntimeErr};

/// Read the classification, code, constraint name and table a driver
/// reported. Each driver classifies its backend's own codes.
#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
fn runtime_failure(runtime: &RuntimeErr) -> DbFailure {
    let RuntimeErr::DriverError(driver_error) = runtime else {
        return DbFailure::new(DbFailureKind::Unclassified);
    };
    let Some(database_error) = driver_error.as_database_error() else {
        return DbFailure::new(transport_failure_kind(driver_error));
    };

    DbFailure::new(database_failure_kind(database_error.kind()))
        .with_code(database_error.code().map(|code| code.into_owned()))
        .with_constraint(database_error.constraint().map(str::to_string))
        .with_table(database_error.table().map(str::to_string))
}

#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
pub(super) fn database_failure_kind(kind: crate::orm::driver::error::ErrorKind) -> DbFailureKind {
    use crate::orm::driver::error::ErrorKind;

    match kind {
        ErrorKind::UniqueViolation => DbFailureKind::UniqueViolation,
        ErrorKind::ForeignKeyViolation => DbFailureKind::ForeignKeyViolation,
        ErrorKind::NotNullViolation => DbFailureKind::NotNullViolation,
        ErrorKind::CheckViolation => DbFailureKind::CheckViolation,
        ErrorKind::InvalidValue => DbFailureKind::InvalidValue,
        ErrorKind::SyntaxError => DbFailureKind::SyntaxError,
        ErrorKind::UndefinedColumn => DbFailureKind::UndefinedColumn,
        ErrorKind::UndefinedTable => DbFailureKind::UndefinedTable,
        ErrorKind::InsufficientPrivilege => DbFailureKind::InsufficientPrivilege,
        ErrorKind::AuthenticationFailed => DbFailureKind::AuthenticationFailed,
        ErrorKind::UnknownDatabase => DbFailureKind::UnknownDatabase,
        ErrorKind::Deadlock => DbFailureKind::Deadlock,
        ErrorKind::SerializationFailure => DbFailureKind::SerializationFailure,
        ErrorKind::LockNotAvailable => DbFailureKind::LockNotAvailable,
        ErrorKind::StatementTimeout => DbFailureKind::StatementTimeout,
        ErrorKind::TooManyConnections => DbFailureKind::ConnectionTimeout,
        ErrorKind::ConnectionClosed => DbFailureKind::ConnectionClosed,
        ErrorKind::ExclusionViolation => DbFailureKind::ExclusionViolation,
        ErrorKind::Other => DbFailureKind::Unclassified,
    }
}

/// Classify a driver failure that carries no database error: the connection
/// broke under the statement — MySQL's `KILL` and a server restart close the
/// socket without a word — no connection could be had, or none could be
/// opened: the server refused it, TLS failed, or the URL is invalid.
#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
fn transport_failure_kind(error: &crate::orm::driver::Error) -> DbFailureKind {
    use crate::orm::driver::Error as DriverError;

    match error {
        DriverError::Io(io) if io.kind() == std::io::ErrorKind::ConnectionRefused => {
            DbFailureKind::ConnectionRefused
        }
        DriverError::Io(_) | DriverError::PoolClosed | DriverError::WorkerCrashed => {
            DbFailureKind::ConnectionClosed
        }
        DriverError::PoolTimedOut => DbFailureKind::ConnectionTimeout,
        DriverError::Tls(_) => DbFailureKind::TlsFailed,
        DriverError::Configuration(_) => DbFailureKind::InvalidConfiguration,
        _ => DbFailureKind::Unclassified,
    }
}

/// Without a driver feature compiled in there is no driver error to read, so
/// every runtime failure stays unclassified.
#[cfg(not(any(feature = "postgres", feature = "mysql", feature = "sqlite")))]
fn runtime_failure(_runtime: &RuntimeErr) -> DbFailure {
    DbFailure::new(DbFailureKind::Unclassified)
}

/// Recover the structured driver detail behind an engine error.
///
/// Only the engine variants that actually came back from a driver produce a
/// failure — the rest is engine bookkeeping with nothing to preserve. The engine
/// error itself is kept as the failure's own source, which is what makes
/// `{:#}`-style chains and `anyhow` interop reach the backend instead of
/// stopping at TideORM's rendered message.
pub(crate) fn driver_failure(err: &OrmError) -> Option<DbFailure> {
    let failure = match err {
        // A pool acquire failure never reaches a driver, so it has no SQLSTATE,
        // but the engine already classified it and both cases are transient.
        OrmError::ConnectionAcquire(ConnAcquireErr::Timeout) => {
            DbFailure::new(DbFailureKind::ConnectionTimeout)
        }
        OrmError::ConnectionAcquire(ConnAcquireErr::ConnectionClosed) => {
            DbFailure::new(DbFailureKind::ConnectionClosed)
        }
        OrmError::Conn(runtime) | OrmError::Exec(runtime) | OrmError::Query(runtime) => {
            runtime_failure(runtime)
        }
        // A statement refused on an aborted transaction fails as the
        // statement that aborted it did.
        OrmError::TransactionAborted {
            cause: Some(cause), ..
        } => return driver_failure(cause),
        _ => return None,
    };

    Some(failure.with_source(Box::new(err.clone())))
}

/// Translate ORM engine errors to TideORM errors.
///
/// The structured driver detail rides along, so the returned error can answer
/// for its SQLSTATE and constraint name and its `source` chain still reaches the
/// driver.
pub(crate) fn translate_error(err: OrmError) -> Error {
    let failure = driver_failure(&err);
    translate_engine_error(err).with_db_failure(failure)
}
/// Translate an engine error from reaching or probing the server.
///
/// Whatever the driver called it, such a failure is a connection failure; the
/// structured driver detail is carried over so the SQLSTATE and the source
/// chain survive the reclassification.
pub(crate) fn translate_connection_error(err: OrmError) -> Error {
    reclassify_error(err, |message, source| Error::Connection { message, source })
}

/// Translate `err` and file it under the variant `class` builds, unless it
/// is a connection failure, which keeps its own. The variant changes, so the
/// translated message and the structured driver failure are carried over
/// explicitly: otherwise a serialization failure would arrive with no
/// SQLSTATE and no source chain.
pub(crate) fn reclassify_error(
    err: OrmError,
    class: impl FnOnce(String, Option<Box<crate::error::DbFailure>>) -> Error,
) -> Error {
    match translate_error(err) {
        connection @ Error::Connection { .. } => connection,
        Error::Query {
            message, source, ..
        }
        | Error::Transaction { message, source } => class(message, source),
        other => class(other.to_string(), None),
    }
}

/// Map an engine error onto the TideORM variant that describes it.
fn translate_engine_error(err: OrmError) -> Error {
    match err {
        OrmError::RecordNotFound(msg) => Error::not_found(msg),
        OrmError::ConnectionAcquire(e) => Error::connection(e.to_string()),
        OrmError::Conn(e) => Error::connection(e.to_string()),
        OrmError::Exec(e) | OrmError::Query(e) => Error::query(e.to_string()),
        OrmError::ConvertFromU64(msg) => Error::conversion(msg),
        OrmError::TryIntoErr { from, into, source } => {
            Error::conversion(format!("Error converting `{from}` into `{into}`: {source}"))
        }
        OrmError::Type(msg) | OrmError::Json(msg) => Error::conversion(msg),
        // Raised by `TryFrom<ActiveModel>` when an attribute was never set, which
        // is a per-field completeness failure and carries the field name — the
        // exact shape of `Error::Validation`.
        OrmError::AttrNotSet(attribute) => Error::validation(attribute, "attribute is not set"),
        OrmError::KeyArityMismatch { expected, received } => Error::query(format!(
            "Primary key arity mismatch: expected {expected} key column(s), received {received}"
        )),
        OrmError::UnpackInsertId => Error::query("Failed to get insert ID".to_string()),
        OrmError::UpdateGetPrimaryKey => {
            Error::query("Failed to get primary key after update".to_string())
        }
        OrmError::BackendNotSupported { db, ctx } => Error::backend_not_supported(ctx, db),
        OrmError::PrimaryKeyNotSet { ctx } => {
            Error::primary_key_not_set(format!("primary key not set for {ctx}"), "model")
        }
        OrmError::RecordNotInserted => Error::query("None of the records are inserted".to_string()),
        OrmError::RecordNotUpdated => Error::not_found("None of the records are updated"),
        OrmError::TransactionAborted { reason, .. } => Error::transaction(reason),
        OrmError::Decode { column, source } => match source.downcast_ref::<Error>() {
            // An encrypted field's decryption, which names the field itself.
            Some(Error::Configuration { message }) => Error::configuration(message.clone()),
            Some(Error::Tokenization { message }) => Error::tokenization(message.clone()),
            Some(Error::Conversion { message, .. }) => Error::conversion(message.clone()),
            _ => Error::conversion(format!(
                "column '{column}' cannot be read: {source}{}",
                json_read_hint(&*source)
            )),
        },
        // An engine failure with no TideORM counterpart stays internal.
        OrmError::Custom(msg) => Error::internal(msg),
    }
}

/// How to read a value no Rust type can hold, when JSON reads it: a
/// PostgreSQL `infinity` or `TIME '24:00:00'` comes back as its text.
#[cfg(feature = "postgres")]
fn json_read_hint(source: &(dyn std::error::Error + Send + Sync + 'static)) -> &'static str {
    if source.is::<driver::postgres::types::PgUnrepresentableMoment>() {
        "; read it with get_json() or raw JSON"
    } else {
        ""
    }
}

#[cfg(not(feature = "postgres"))]
fn json_read_hint(_source: &(dyn std::error::Error + Send + Sync + 'static)) -> &'static str {
    ""
}
