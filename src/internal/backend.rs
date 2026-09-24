use super::{OrmBackend, OrmStatement, Value};
use crate::tide_warn;

/// TideORM-owned runtime backend identifier.
///
/// This intentionally captures only the backend shape TideORM cares about at
/// runtime. MariaDB and MySQL share the same wire/backend layer here and are
/// distinguished later through TideORM configuration when needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// PostgreSQL: `"` identifier quoting, `$n` placeholders.
    Postgres,
    /// MySQL — also reported for MariaDB, which the ORM engine cannot tell
    /// apart. Consult `TideConfig::get_database_type()` when the distinction
    /// matters.
    MySql,
    /// SQLite.
    Sqlite,
}

impl Backend {
    pub(crate) fn as_database_type(self) -> crate::config::DatabaseType {
        match self {
            Self::Postgres => crate::config::DatabaseType::Postgres,
            Self::MySql => crate::config::DatabaseType::MySQL,
            Self::Sqlite => crate::config::DatabaseType::SQLite,
        }
    }

    /// Map an ORM engine backend onto TideORM's runtime backend, if it is one
    /// TideORM knows about.
    ///
    /// The engine's backend enum is `#[non_exhaustive]`, so a future release can
    /// add a backend that this version has never seen. Guessing at that point is
    /// not harmless — the backend decides identifier quoting and parameter
    /// placeholder style, so a wrong guess produces SQL the server rejects (or,
    /// worse, silently misreads). This returns `None` instead, so callers that
    /// have somewhere to put the failure can act on it.
    pub fn from_orm_backend(backend: OrmBackend) -> Option<Self> {
        match backend {
            OrmBackend::Postgres => Some(Self::Postgres),
            OrmBackend::MySql => Some(Self::MySql),
            OrmBackend::Sqlite => Some(Self::Sqlite),
            _ => None,
        }
    }
}

impl From<OrmBackend> for Backend {
    /// Infallible conversion for the call sites that have no error path.
    ///
    /// An unknown backend still has to yield *something* here, so it falls back
    /// to PostgreSQL — but loudly, on stderr, naming the backend and the
    /// consequence. Prefer [`Backend::from_orm_backend`] wherever the gap can be
    /// handled instead of guessed at.
    fn from(backend: OrmBackend) -> Self {
        Self::from_orm_backend(backend).unwrap_or_else(|| {
            tide_warn!(
                "Unrecognized database backend {backend:?} reported by the ORM engine. \
                 Falling back to the PostgreSQL dialect: identifier quoting and parameter \
                 placeholders are likely wrong for this backend. Upgrade TideORM."
            );
            Self::Postgres
        })
    }
}

impl From<Backend> for OrmBackend {
    fn from(backend: Backend) -> Self {
        match backend {
            Backend::Postgres => OrmBackend::Postgres,
            Backend::MySql => OrmBackend::MySql,
            Backend::Sqlite => OrmBackend::Sqlite,
        }
    }
}

pub(crate) fn build_statement(
    backend: impl Into<OrmBackend>,
    sql: impl Into<String>,
) -> OrmStatement {
    OrmStatement::from_string(backend.into(), sql.into())
}

pub(crate) fn build_statement_with_values(
    backend: impl Into<OrmBackend>,
    sql: &str,
    params: Vec<Value>,
) -> OrmStatement {
    OrmStatement::from_sql_and_values(backend.into(), sql, params)
}

#[cfg(test)]
#[path = "../../tests/unit/internal_backend_tests.rs"]
mod tests;
