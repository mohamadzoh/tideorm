//! The configured database backend and the capability questions asked of it.

/// A database backend TideORM can talk to.
///
/// This is the *configured* backend and is deliberately finer-grained than the
/// driver's: MySQL and MariaDB are separate variants here even though the
/// driver reports one dialect for both, so an application can tell which
/// server it reached. Code that needs that distinction must read
/// `TideConfig::get_database_type()` rather than asking the connection.
///
/// Non-exhaustive: match with a `_` arm so a future backend does not break the
/// build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum DatabaseType {
    /// PostgreSQL. The default.
    #[default]
    Postgres,
    /// MySQL.
    MySQL,
    /// MariaDB. Shares MySQL's dialect.
    MariaDB,
    /// SQLite.
    SQLite,
}

impl DatabaseType {
    /// Return whether the backend has a native array column type.
    ///
    /// PostgreSQL only. Elsewhere an "array" column is a JSON array, which is
    /// why the array update operators render differently per backend.
    pub fn supports_arrays(&self) -> bool {
        matches!(self, DatabaseType::Postgres)
    }

    /// Return whether an `UPDATE` can return the rows it wrote.
    ///
    /// `BatchUpdateBuilder::execute_returning()` checks it and refuses MySQL,
    /// which has no `RETURNING`, and MariaDB, which has `UPDATE .. RETURNING`
    /// only from 13.0 (a server version TideORM does not check), before
    /// anything runs. Batch insert does not consult it: it sends multi-row
    /// `INSERT .. RETURNING` statements on PostgreSQL and SQLite, and inserts
    /// without `RETURNING` on MySQL and MariaDB alike, since the engine renders
    /// none for either.
    pub fn supports_returning(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,
            DatabaseType::MySQL => false,
            DatabaseType::MariaDB => false,
            DatabaseType::SQLite => true,
        }
    }

    /// A reasonable number of rows to write per batch on this backend.
    ///
    /// A guideline for chunking bulk work, chosen to stay clear of each
    /// backend's statement and parameter limits — not an enforced cap.
    pub fn optimal_batch_size(&self) -> usize {
        match self {
            DatabaseType::Postgres => 1000,
            DatabaseType::MySQL | DatabaseType::MariaDB => 500,
            DatabaseType::SQLite => 100,
        }
    }

    /// The character this backend quotes identifiers with.
    ///
    /// Informational. Do not build SQL by wrapping a name in it — go through the
    /// shared quoting helpers, which also escape the quote character itself.
    pub fn quote_char(&self) -> char {
        match self {
            DatabaseType::Postgres | DatabaseType::SQLite => '"',
            DatabaseType::MySQL | DatabaseType::MariaDB => '`',
        }
    }

    /// Infer the backend from a connection URL's scheme.
    ///
    /// Recognises `postgres://`, `postgresql://`, `mariadb://`, `mysql://`, and
    /// `sqlite:`; returns `None` for anything else, which is what makes
    /// `TideConfig::connect()` ask for an explicit `database_type`.
    ///
    /// A MariaDB server reached through a `mysql://` URL is reported as
    /// [`DatabaseType::MySQL`] here — `connect()` corrects that afterwards by
    /// asking the server for its version.
    pub fn from_url(url: &str) -> Option<Self> {
        let url_lower = url.to_lowercase();
        if url_lower.starts_with("postgres://") || url_lower.starts_with("postgresql://") {
            Some(DatabaseType::Postgres)
        } else if url_lower.starts_with("mariadb://") {
            Some(DatabaseType::MariaDB)
        } else if url_lower.starts_with("mysql://") {
            Some(DatabaseType::MySQL)
        } else if url_lower.starts_with("sqlite:") {
            Some(DatabaseType::SQLite)
        } else {
            None
        }
    }
}

/// `url` with a `mariadb://` scheme, in any case, spelled `mysql://`, the
/// one the driver takes.
pub(crate) fn rewrite_driver_url(url: &str) -> String {
    match url.split_once("://") {
        Some((scheme, remainder)) if scheme.eq_ignore_ascii_case("mariadb") => {
            format!("mysql://{}", remainder)
        }
        _ => url.to_string(),
    }
}

impl std::fmt::Display for DatabaseType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DatabaseType::Postgres => write!(f, "PostgreSQL"),
            DatabaseType::MySQL => write!(f, "MySQL"),
            DatabaseType::MariaDB => write!(f, "MariaDB"),
            DatabaseType::SQLite => write!(f, "SQLite"),
        }
    }
}
