use std::time::Duration;

use crate::error::{Error, Result};
use crate::internal::{ConnectTarget, InternalConnection};

use super::Database;

impl Database {
    /// Create a database builder when `Database::connect(url)` is not enough.
    pub fn builder() -> DatabaseBuilder {
        DatabaseBuilder::new()
    }
}

/// Builder for connection-pool settings such as limits and timeouts.
///
/// The database is named by a URL or, for settings a URL does not name, by
/// the driver's own options ([`postgres_options`](Self::postgres_options),
/// [`mysql_options`](Self::mysql_options), [`sqlite_options`](Self::sqlite_options)).
/// `{:?}` masks a URL's credentials and leaves driver options out, since
/// they hold the password.
///
/// A setting left unset keeps the driver's default, which is not always
/// `TideConfig`'s: the driver keeps no idle connection and waits 30 seconds
/// for a free one, where `TideConfig` keeps one and waits 8.
///
/// An in-memory SQLite database lives only while a connection to it is open,
/// so its pool keeps one open and closes none for being idle or old, whatever
/// those settings say. One with `cache=private` gives each connection a
/// database of its own, so its pool must hold one connection.
#[derive(Clone, Debug, Default)]
pub struct DatabaseBuilder {
    target: Option<ConnectTarget>,
    max_connections: Option<u32>,
    min_connections: Option<u32>,
    connect_timeout: Option<Duration>,
    idle_timeout: Option<Duration>,
    max_lifetime: Option<Duration>,
    acquire_timeout: Option<Duration>,
    statement_cache_capacity: Option<usize>,
}

impl DatabaseBuilder {
    /// Create a new builder with no URL or pool overrides configured yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the database connection URL.
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.target = Some(ConnectTarget::Url(url.into()));
        self
    }

    /// Connect to PostgreSQL with the driver's options instead of a URL.
    #[cfg(feature = "postgres")]
    pub fn postgres_options(mut self, options: crate::prelude::PgConnectOptions) -> Self {
        self.target = Some(options.into());
        self
    }

    /// Connect to MySQL or MariaDB with the driver's options instead of a URL.
    #[cfg(feature = "mysql")]
    pub fn mysql_options(mut self, options: crate::prelude::MySqlConnectOptions) -> Self {
        self.target = Some(options.into());
        self
    }

    /// Connect to SQLite with the driver's options instead of a URL.
    #[cfg(feature = "sqlite")]
    pub fn sqlite_options(mut self, options: crate::prelude::SqliteConnectOptions) -> Self {
        self.target = Some(options.into());
        self
    }

    /// Set the maximum number of pooled connections, at least one.
    pub fn max_connections(mut self, n: u32) -> Self {
        self.max_connections = Some(n);
        self
    }

    /// Set the minimum number of pooled connections.
    pub fn min_connections(mut self, n: u32) -> Self {
        self.min_connections = Some(n);
        self
    }

    /// Bound each attempt to open a connection, TLS, authentication and
    /// connection hooks included. A checkout still ends at
    /// [`acquire_timeout`](Self::acquire_timeout) if that comes first.
    pub fn connect_timeout(mut self, duration: Duration) -> Self {
        self.connect_timeout = Some(duration);
        self
    }

    /// Close a connection left unused this long.
    pub fn idle_timeout(mut self, duration: Duration) -> Self {
        self.idle_timeout = Some(duration);
        self
    }

    /// Close a connection this old, however busy. Keep it under any idle
    /// or lifetime limit the server, or a proxy in front of it, enforces.
    pub fn max_lifetime(mut self, duration: Duration) -> Self {
        self.max_lifetime = Some(duration);
        self
    }

    /// Bound a whole checkout: waiting for a free connection, its ping and
    /// hooks, and opening a new one. Each attempt to open one is also
    /// bounded by [`connect_timeout`](Self::connect_timeout).
    pub fn acquire_timeout(mut self, duration: Duration) -> Self {
        self.acquire_timeout = Some(duration);
        self
    }

    /// Keep up to `capacity` prepared statements per connection, dropping the
    /// least recently used past that; the default keeps 100. With 0 none is
    /// kept and every statement is prepared for its run, as a proxy pooling
    /// transactions (PgBouncer) needs.
    pub fn statement_cache_capacity(mut self, capacity: usize) -> Self {
        self.statement_cache_capacity = Some(capacity);
        self
    }

    /// Take every pool setting from `TideConfig`'s, defaults included.
    pub(crate) fn pool(self, pool: &crate::config::PoolConfig) -> Self {
        self.max_connections(pool.max_connections)
            .min_connections(pool.min_connections)
            .connect_timeout(pool.connect_timeout)
            .idle_timeout(pool.idle_timeout)
            .max_lifetime(pool.max_lifetime)
            .acquire_timeout(pool.acquire_timeout)
    }

    /// Connect using the configured URL or driver options and pool settings.
    ///
    /// Returns a configuration error if neither was provided.
    pub async fn build(self) -> Result<Database> {
        let target = self
            .target
            .ok_or_else(|| Error::configuration("Database URL or driver options are required"))?;
        match (self.min_connections, self.max_connections) {
            (_, Some(0)) => {
                return Err(Error::configuration(
                    "max_connections must be at least 1: a pool of none can hand out no connection",
                ));
            }
            (Some(min), Some(max)) if min > max => {
                return Err(Error::configuration(format!(
                    "min_connections ({min}) exceeds max_connections ({max})"
                )));
            }
            _ => {}
        }
        let mut opts = crate::internal::ConnectOptions::new(target);
        if let Some(max) = self.max_connections {
            opts.max_connections(max);
        }
        if let Some(min) = self.min_connections {
            opts.min_connections(min);
        }
        if let Some(timeout) = self.connect_timeout {
            opts.connect_timeout(timeout);
        }
        if let Some(timeout) = self.acquire_timeout {
            opts.acquire_timeout(timeout);
        }
        if let Some(timeout) = self.idle_timeout {
            opts.idle_timeout(timeout);
        }
        if let Some(lifetime) = self.max_lifetime {
            opts.max_lifetime(lifetime);
        }
        if let Some(capacity) = self.statement_cache_capacity {
            opts.statement_cache_capacity(capacity);
        }
        let conn = crate::internal::OrmDatabase::connect(opts)
            .await
            .map_err(crate::internal::translate_connection_error)?;

        let connection = InternalConnection::open(conn).await?;
        Ok(Database::from_internal_connection(connection))
    }
}
