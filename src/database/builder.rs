use std::time::Duration;

use crate::error::{Error, Result};
use crate::internal::InternalConnection;

use super::Database;

impl Database {
    /// Create a new database builder for advanced configuration
    pub fn builder() -> DatabaseBuilder {
        DatabaseBuilder::new()
    }
}

/// Builder for configuring database connections
#[derive(Debug, Clone)]
pub struct DatabaseBuilder {
    url: Option<String>,
    max_connections: Option<u32>,
    min_connections: Option<u32>,
    connect_timeout: Option<Duration>,
    idle_timeout: Option<Duration>,
    max_lifetime: Option<Duration>,
}

impl DatabaseBuilder {
    /// Create a new DatabaseBuilder
    pub fn new() -> Self {
        Self {
            url: None,
            max_connections: None,
            min_connections: None,
            connect_timeout: None,
            idle_timeout: None,
            max_lifetime: None,
        }
    }

    /// Set the database connection URL
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Set the maximum number of connections in the pool
    pub fn max_connections(mut self, n: u32) -> Self {
        self.max_connections = Some(n);
        self
    }

    /// Set the minimum number of connections in the pool
    pub fn min_connections(mut self, n: u32) -> Self {
        self.min_connections = Some(n);
        self
    }

    /// Set the connection timeout
    pub fn connect_timeout(mut self, duration: Duration) -> Self {
        self.connect_timeout = Some(duration);
        self
    }

    /// Set the idle connection timeout
    pub fn idle_timeout(mut self, duration: Duration) -> Self {
        self.idle_timeout = Some(duration);
        self
    }

    /// Set the maximum connection lifetime
    pub fn max_lifetime(mut self, duration: Duration) -> Self {
        self.max_lifetime = Some(duration);
        self
    }

    /// Build and connect to the database with pool configuration
    pub async fn build(self) -> Result<Database> {
        let url = self
            .url
            .ok_or_else(|| Error::configuration("Database URL is required"))?;

        let mut opts = crate::internal::ConnectOptions::new(url);

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

        let conn = crate::internal::SeaDatabase::connect(opts)
            .await
            .map_err(|e| Error::connection(e.to_string()))?;

        Ok(Database::from_internal_connection(InternalConnection {
            conn,
        }))
    }
}

impl Default for DatabaseBuilder {
    fn default() -> Self {
        Self::new()
    }
}
