//! The configured database backend and the capability questions asked of it.

/// A database backend TideORM can talk to.
///
/// This is the *configured* backend and is deliberately finer-grained than the
/// driver's: MySQL and MariaDB are separate variants here even though the
/// driver reports one dialect for both, because they disagree on things TideORM
/// has to decide — `RETURNING` support most of all. Code that needs that
/// distinction must read `TideConfig::get_database_type()` rather than asking
/// the connection.
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
    /// MariaDB. Shares MySQL's dialect but supports `RETURNING`.
    MariaDB,
    /// SQLite.
    SQLite,
}

impl DatabaseType {
    /// Return whether this backend speaks the MySQL dialect.
    ///
    /// True for both MySQL and MariaDB. Use it for syntax and quoting; use the
    /// specific capability methods where the two actually differ.
    pub fn is_mysql_compatible(&self) -> bool {
        matches!(self, DatabaseType::MySQL | DatabaseType::MariaDB)
    }

    /// The port this backend listens on when a URL does not name one.
    ///
    /// `0` for SQLite, which is a file rather than a server.
    pub fn default_port(&self) -> u16 {
        match self {
            DatabaseType::Postgres => 5432,
            DatabaseType::MySQL | DatabaseType::MariaDB => 3306,
            DatabaseType::SQLite => 0,
        }
    }

    /// The URL scheme that selects this backend, without `://`.
    ///
    /// Note `mariadb` is TideORM's own spelling: the driver is handed a
    /// `mysql://` URL, since MariaDB has no separate driver.
    pub fn url_scheme(&self) -> &'static str {
        match self {
            DatabaseType::Postgres => "postgres",
            DatabaseType::MySQL => "mysql",
            DatabaseType::MariaDB => "mariadb",
            DatabaseType::SQLite => "sqlite",
        }
    }

    /// Return whether this backend can store JSON documents. True everywhere.
    pub fn supports_json(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,
            DatabaseType::MySQL | DatabaseType::MariaDB => true,
            DatabaseType::SQLite => true,
        }
    }

    /// Return whether JSON paths can be queried in SQL rather than in Rust.
    ///
    /// True everywhere, though the syntax differs per backend — TideORM renders
    /// the right one for you.
    pub fn supports_native_json_operators(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,
            DatabaseType::MySQL | DatabaseType::MariaDB => true,
            DatabaseType::SQLite => true,
        }
    }

    /// Return whether the backend has a native array column type.
    ///
    /// PostgreSQL only. Elsewhere an "array" column is a JSON array, which is
    /// why the array update operators render differently per backend.
    pub fn supports_arrays(&self) -> bool {
        matches!(self, DatabaseType::Postgres)
    }

    /// Return whether a write statement can return the rows it wrote.
    ///
    /// The one capability where MySQL and MariaDB part ways: MariaDB supports
    /// `RETURNING`, plain MySQL does not. It is why batch insert uses a single
    /// `INSERT .. RETURNING` on PostgreSQL and MariaDB but falls back to
    /// individual inserts on MySQL and SQLite, and why `execute_returning()`
    /// is refused on MySQL.
    pub fn supports_returning(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,
            DatabaseType::MySQL => false,
            DatabaseType::MariaDB => true,
            DatabaseType::SQLite => true,
        }
    }

    /// Return whether the backend can turn a conflicting insert into an update.
    ///
    /// True everywhere, so [`Model::on_conflict`](crate::model::Model::on_conflict)
    /// works on every backend.
    pub fn supports_upsert(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,
            DatabaseType::MySQL | DatabaseType::MariaDB => true,
            DatabaseType::SQLite => true,
        }
    }

    /// Return whether the backend has a full-text search facility.
    ///
    /// True everywhere, but the implementations are not equivalent: PostgreSQL
    /// uses `tsvector`, MySQL/MariaDB `MATCH .. AGAINST`, SQLite FTS. Ranking
    /// and tokenization therefore differ between them.
    pub fn supports_fulltext_search(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,
            DatabaseType::MySQL | DatabaseType::MariaDB => true,
            DatabaseType::SQLite => true,
        }
    }

    /// Return whether the backend supports window functions (`OVER (..)`).
    pub fn supports_window_functions(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,
            DatabaseType::MySQL | DatabaseType::MariaDB => true,
            DatabaseType::SQLite => true,
        }
    }

    /// Return whether the backend supports common table expressions (`WITH ..`).
    pub fn supports_cte(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,
            DatabaseType::MySQL | DatabaseType::MariaDB => true,
            DatabaseType::SQLite => true,
        }
    }

    /// Return whether tables can be grouped into named schemas.
    ///
    /// PostgreSQL only. On the others a schema-qualified table name has no
    /// meaning, so multi-tenancy has to be done with separate databases or a
    /// tenant column.
    pub fn supports_schemas(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,
            DatabaseType::MySQL | DatabaseType::MariaDB => false,
            DatabaseType::SQLite => false,
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

    /// The placeholder marker this backend uses for bound parameters.
    ///
    /// `"$"` on PostgreSQL, where placeholders are numbered (`$1`, `$2`, ...);
    /// `"?"` elsewhere, where they are positional.
    pub fn param_style(&self) -> &'static str {
        match self {
            DatabaseType::Postgres => "$",
            DatabaseType::MySQL | DatabaseType::MariaDB => "?",
            DatabaseType::SQLite => "?",
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

pub(crate) fn rewrite_driver_url(url: &str) -> String {
    if let Some(remainder) = url.strip_prefix("mariadb://") {
        format!("mysql://{}", remainder)
    } else {
        url.to_string()
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
