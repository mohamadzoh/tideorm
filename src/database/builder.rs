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

    /// Set the maximum number of pooled connections.
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
    /// Unlike [`Self::connect_timeout`], which bounds opening a brand-new connection,
    /// this bounds checking one out of an already-saturated pool.
    pub fn acquire_timeout(mut self, duration: Duration) -> Self {
        self.acquire_timeout = Some(duration);
        self
    }

    /// Connect using the configured URL and pool settings.
    ///
    /// Returns a configuration error if no URL was provided.
    pub async fn build(self) -> Result<Database> {
        let url = self
            .url
            .ok_or_else(|| Error::configuration("Database URL is required"))?;

        let mut opts = crate::internal::ConnectOptions::new(url.clone());

        if let Some(max) = self.max_connections {
            opts.max_connections(max);
        }
        if let Some(min) = self.min_connections {
            opts.min_connections(min);
        }
        if let Some(timeout) = self.connect_timeout {
            opts.connect_timeout(timeout);
        }
        if let Some(timeout) = self.idle_timeout {
            opts.idle_timeout(timeout);
        }
        if let Some(lifetime) = self.max_lifetime {
            opts.max_lifetime(lifetime);
        }
        if let Some(timeout) = self.acquire_timeout {
            opts.acquire_timeout(timeout);
        }

        let conn = crate::internal::OrmDatabase::connect(opts)
            .await
            .map_err(|err| crate::internal::translate_connect_error(err, &url))?;

        Ok(Database::from_internal_connection(InternalConnection::new(
            conn,
        )))
    }
}
