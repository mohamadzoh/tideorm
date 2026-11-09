use super::{PoolConfig, TideConfig};
use std::time::Duration;

impl TideConfig {
    /// Replace every pool setting at once, such as a [`PoolConfig`] shared
    /// with [`Database::builder`](crate::database::Database::builder); the setters below change single ones.
    ///
    /// ```
    /// use std::time::Duration;
    /// use tideorm::{PoolConfig, TideConfig};
    ///
    /// let pool = PoolConfig::default()
    ///     .max_connections(20)
    ///     .acquire_timeout(Duration::from_secs(2));
    /// let config = TideConfig::init().pool(pool);
    /// ```
    #[must_use]
    pub fn pool(mut self, pool: PoolConfig) -> Self {
        self.pool = pool;
        self
    }

    /// Cap the connection pool size. Defaults to 10.
    ///
    /// This is the ceiling on concurrent database work for the whole process,
    /// so it should sit below what the server allows across all your instances.
    #[must_use]
    pub fn max_connections(mut self, n: u32) -> Self {
        self.pool.max_connections = n;
        self
    }

    /// Keep at least this many connections open. Defaults to 1.
    ///
    /// Raising it trades idle server connections for fewer cold-start latency
    /// spikes.
    #[must_use]
    pub fn min_connections(mut self, n: u32) -> Self {
        self.pool.min_connections = n;
        self
    }

    /// Bound each attempt to open a connection, TLS, authentication and
    /// connection hooks included (8 seconds by default). A checkout still ends at
    /// [`acquire_timeout`](Self::acquire_timeout) if that comes first.
    #[must_use]
    pub fn connect_timeout(mut self, duration: Duration) -> Self {
        self.pool.connect_timeout = duration;
        self
    }

    /// Close a connection left unused this long (10 minutes by default).
    #[must_use]
    pub fn idle_timeout(mut self, duration: Duration) -> Self {
        self.pool.idle_timeout = duration;
        self
    }

    /// Close a connection this old, however busy (30 minutes by default). Keep it under any idle
    /// or lifetime limit the server, or a proxy in front of it, enforces.
    #[must_use]
    pub fn max_lifetime(mut self, duration: Duration) -> Self {
        self.pool.max_lifetime = duration;
        self
    }

    /// Bound a whole checkout: waiting for a free connection, its ping and
    /// hooks, and opening a new one (8 seconds by default). Each attempt to open one is also
    /// bounded by [`connect_timeout`](Self::connect_timeout).
    #[must_use]
    pub fn acquire_timeout(mut self, duration: Duration) -> Self {
        self.pool.acquire_timeout = duration;
        self
    }
}
