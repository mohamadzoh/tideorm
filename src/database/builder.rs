use std::time::Duration;

use crate::error::{Error, Result};
use crate::internal::InternalConnection;

use super::Database;

impl Database {
    /// Create a database builder when `Database::connect(url)` is not enough.
    pub fn builder() -> DatabaseBuilder {
        DatabaseBuilder::new()
    }
}

/// Builder for connection-pool settings such as limits and timeouts.
///
/// A setting left unset keeps the driver's default, which is not always
/// `TideConfig`'s: the driver keeps no idle connection and waits 30 seconds
/// for a free one, where `TideConfig` keeps one and waits 8.
///
/// An in-memory SQLite database lives only while a connection to it is open,
/// so its pool keeps one open and closes none for being idle or old, whatever
/// those settings say.
#[derive(Clone, Default)]
pub struct DatabaseBuilder {
    url: Option<String>,
    max_connections: Option<u32>,
    min_connections: Option<u32>,
    connect_timeout: Option<Duration>,
    idle_timeout: Option<Duration>,
    max_lifetime: Option<Duration>,
    acquire_timeout: Option<Duration>,
}

/// Masks the URL's credentials: `{:?}` is how configuration ends up in logs.
impl std::fmt::Debug for DatabaseBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DatabaseBuilder")
            .field(
                "url",
                &self
                    .url
                    .as_deref()
                    .map(crate::internal::mask_url_credentials),
            )
            .field("max_connections", &self.max_connections)
            .field("min_connections", &self.min_connections)
            .field("connect_timeout", &self.connect_timeout)
            .field("idle_timeout", &self.idle_timeout)
            .field("max_lifetime", &self.max_lifetime)
            .field("acquire_timeout", &self.acquire_timeout)
            .finish()
    }
}

impl DatabaseBuilder {
    /// Create a new builder with no URL or pool overrides configured yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the database connection URL.
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
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

    /// Set how long to wait while opening a connection.
    ///
    /// The driver has one timeout for handing out a connection, opening one
    /// when the pool has room, so this and
    /// [`acquire_timeout`](Self::acquire_timeout) set the same limit, and the
    /// longer of the two applies.
    pub fn connect_timeout(mut self, duration: Duration) -> Self {
        self.connect_timeout = Some(duration);
        self
    }

    /// Set how long an idle pooled connection may stay open.
    pub fn idle_timeout(mut self, duration: Duration) -> Self {
        self.idle_timeout = Some(duration);
        self
    }

    /// Set the maximum lifetime for a pooled connection before recycle.
    pub fn max_lifetime(mut self, duration: Duration) -> Self {
        self.max_lifetime = Some(duration);
        self
    }

    /// Set how long to wait for a free connection when the pool is exhausted.
    ///
    /// The driver bounds checking one out and opening a new one with a single
    /// timeout, so this and [`connect_timeout`](Self::connect_timeout) set
    /// the same limit, and the longer of the two applies.
    pub fn acquire_timeout(mut self, duration: Duration) -> Self {
        self.acquire_timeout = Some(duration);
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

    /// Connect using the configured URL and pool settings.
    ///
    /// Returns a configuration error if no URL was provided.
    pub async fn build(self) -> Result<Database> {
        let url = self
            .url
            .ok_or_else(|| Error::configuration("Database URL is required"))?;
        let url = crate::config::rewrite_driver_url(&url);
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
        let memory = sqlite_memory(&url);
        let in_memory = memory.is_some();
        let private_memory = memory == Some(Cache::Private);
        if private_memory
            && (self.max_connections.is_some_and(|max| max > 1)
                || self.min_connections.is_some_and(|min| min > 1))
        {
            return Err(Error::configuration(
                "an in-memory SQLite database with cache=private requires a pool size of one",
            ));
        }

        let mut opts = crate::internal::ConnectOptions::new(url.clone());
        crate::internal::quiet_driver_logging(&mut opts);

        if private_memory {
            opts.max_connections(1);
        } else if let Some(max) = self.max_connections {
            opts.max_connections(max);
        }
        match self.min_connections {
            Some(min) if in_memory => opts.min_connections(min.max(1)),
            Some(min) => opts.min_connections(min),
            None if in_memory => opts.min_connections(1),
            None => &mut opts,
        };
        // sqlx has one checkout timeout, which also covers opening a
        // connection when the pool has room, and SeaORM maps both settings
        // onto it, the later one winning. The longer is passed, so neither
        // cuts the other short.
        let checkout_timeout = match (self.connect_timeout, self.acquire_timeout) {
            (Some(connect), Some(acquire)) => Some(connect.max(acquire)),
            (connect, acquire) => connect.or(acquire),
        };
        if let Some(timeout) = checkout_timeout {
            opts.acquire_timeout(timeout);
        }
        if in_memory {
            opts.idle_timeout(None).max_lifetime(None);
        } else {
            if let Some(timeout) = self.idle_timeout {
                opts.idle_timeout(timeout);
            }
            if let Some(lifetime) = self.max_lifetime {
                opts.max_lifetime(lifetime);
            }
        }
        let conn = crate::internal::OrmDatabase::connect(opts)
            .await
            .map_err(|err| crate::internal::translate_connect_error(err, &url))?;

        let connection = InternalConnection::open(conn).await?;
        if private_memory {
            // Named private-cache memory databases do not share data even
            // when their filenames match another pool's.
            *connection.cache_scope.write() =
                Some(format!("sqlite-private:{}", uuid::Uuid::new_v4()).into());
        }
        Ok(Database::from_internal_connection(connection))
    }
}

/// How an in-memory SQLite database shares its pages between connections.
#[derive(PartialEq)]
enum Cache {
    Shared,
    /// Each connection would see a database of its own.
    Private,
}

/// The cache of the in-memory SQLite database `url` opens, as sqlx reads it
/// (`:memory:`, an empty path, or `mode=memory`); `None` for any other URL.
fn sqlite_memory(url: &str) -> Option<Cache> {
    let (scheme, rest) = url.split_once(':')?;
    if !scheme.eq_ignore_ascii_case("sqlite") {
        return None;
    }
    let rest = rest.strip_prefix("//").unwrap_or(rest);
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let mut pairs = url::form_urlencoded::parse(query.as_bytes());
    let in_memory = path.is_empty()
        || path == ":memory:"
        || pairs
            .clone()
            .any(|(key, value)| key == "mode" && value == "memory");
    if !in_memory {
        return None;
    }
    Some(
        if pairs.any(|(key, value)| key == "cache" && value == "private") {
            Cache::Private
        } else {
            Cache::Shared
        },
    )
}
