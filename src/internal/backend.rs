use super::{OrmBackend, OrmStatement, Value};

/// TideORM-owned runtime backend identifier.
///
/// This intentionally captures only the backend shape TideORM cares about at
/// runtime. MariaDB and MySQL share the same wire/backend layer here and are
/// distinguished later through TideORM configuration when needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Postgres,
    MySql,
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
}

impl From<OrmBackend> for Backend {
    fn from(backend: OrmBackend) -> Self {
        match backend {
            OrmBackend::Postgres => Self::Postgres,
            OrmBackend::MySql => Self::MySql,
            OrmBackend::Sqlite => Self::Sqlite,
            _ => Self::Postgres,
        }
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

pub(crate) trait StatementBackend {
    fn into_statement_backend(self) -> OrmBackend;
}

impl StatementBackend for Backend {
    fn into_statement_backend(self) -> OrmBackend {
        self.into()
    }
}

impl StatementBackend for OrmBackend {
    fn into_statement_backend(self) -> OrmBackend {
        self
    }
}

pub(crate) fn build_statement<B>(backend: B, sql: impl Into<String>) -> OrmStatement
where
    B: StatementBackend,
{
    OrmStatement::from_string(backend.into_statement_backend(), sql.into())
}

pub(crate) fn build_statement_with_values<B>(
    backend: B,
    sql: &str,
    params: Vec<Value>,
) -> OrmStatement
where
    B: StatementBackend,
{
    OrmStatement::from_sql_and_values(backend.into_statement_backend(), sql, params)
}
