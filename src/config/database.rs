#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum DatabaseType {
    #[default]
    Postgres,
    MySQL,
    MariaDB,
    SQLite,
}

impl DatabaseType {
    pub fn is_mysql_compatible(&self) -> bool {
        matches!(self, DatabaseType::MySQL | DatabaseType::MariaDB)
    }

    pub fn default_port(&self) -> u16 {
        match self {
            DatabaseType::Postgres => 5432,
            DatabaseType::MySQL | DatabaseType::MariaDB => 3306,
            DatabaseType::SQLite => 0,
        }
    }

    pub fn url_scheme(&self) -> &'static str {
        match self {
            DatabaseType::Postgres => "postgres",
            DatabaseType::MySQL => "mysql",
            DatabaseType::MariaDB => "mariadb",
            DatabaseType::SQLite => "sqlite",
        }
    }

    pub fn supports_json(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,
            DatabaseType::MySQL | DatabaseType::MariaDB => true,
            DatabaseType::SQLite => true,
        }
    }

    pub fn supports_native_json_operators(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,
            DatabaseType::MySQL | DatabaseType::MariaDB => true,
            DatabaseType::SQLite => true,
        }
    }

    pub fn supports_arrays(&self) -> bool {
        matches!(self, DatabaseType::Postgres)
    }

    pub fn supports_returning(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,
            DatabaseType::MySQL => false,
            DatabaseType::MariaDB => true,
            DatabaseType::SQLite => true,
        }
    }

    pub fn supports_upsert(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,
            DatabaseType::MySQL | DatabaseType::MariaDB => true,
            DatabaseType::SQLite => true,
        }
    }

    pub fn supports_fulltext_search(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,
            DatabaseType::MySQL | DatabaseType::MariaDB => true,
            DatabaseType::SQLite => true,
        }
    }

    pub fn supports_window_functions(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,
            DatabaseType::MySQL | DatabaseType::MariaDB => true,
            DatabaseType::SQLite => true,
        }
    }

    pub fn supports_cte(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,
            DatabaseType::MySQL | DatabaseType::MariaDB => true,
            DatabaseType::SQLite => true,
        }
    }

    pub fn supports_schemas(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,
            DatabaseType::MySQL | DatabaseType::MariaDB => false,
            DatabaseType::SQLite => false,
        }
    }

    pub fn optimal_batch_size(&self) -> usize {
        match self {
            DatabaseType::Postgres => 1000,
            DatabaseType::MySQL | DatabaseType::MariaDB => 500,
            DatabaseType::SQLite => 100,
        }
    }

    pub fn param_style(&self) -> &'static str {
        match self {
            DatabaseType::Postgres => "$",
            DatabaseType::MySQL | DatabaseType::MariaDB => "?",
            DatabaseType::SQLite => "?",
        }
    }

    pub fn quote_char(&self) -> char {
        match self {
            DatabaseType::Postgres | DatabaseType::SQLite => '"',
            DatabaseType::MySQL | DatabaseType::MariaDB => '`',
        }
    }

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
