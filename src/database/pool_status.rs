//! What a pool holds, for metrics and health checks.

use crate::error::{Error, Result};

use super::Database;

/// The connections a pool holds, to export as metrics or check in a health
/// endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolStatus {
    /// Open connections, idle ones included.
    pub size: u32,
    /// Open connections no one has checked out.
    pub idle: u32,
    /// The most connections the pool opens.
    pub max_connections: u32,
}

impl PoolStatus {
    /// Connections checked out now.
    pub fn in_use(&self) -> u32 {
        self.size.saturating_sub(self.idle)
    }
}

impl Database {
    /// The connections this handle's pool holds now. A pinned session's or
    /// a transaction's handle answers with the pool it came from.
    ///
    /// Run this example with the `sqlite` feature.
    ///
    /// ```
    /// # #[cfg(feature = "sqlite")]
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// let db = tideorm::Database::connect("sqlite::memory:").await?;
    /// let status = db.pool_status()?;
    /// assert!(status.in_use() <= status.size && status.size <= status.max_connections);
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// ```
    #[cfg_attr(
        not(any(feature = "postgres", feature = "mysql", feature = "sqlite")),
        allow(unused_macros, unused_variables)
    )]
    pub fn pool_status(&self) -> Result<PoolStatus> {
        let pool = self.own_handle()?.pool().clone();
        let conn = &pool.conn;
        macro_rules! status {
            ($pool:expr) => {
                $pool.map(|pool| PoolStatus {
                    size: pool.size(),
                    // At most `size`, a `u32`.
                    idle: pool.num_idle() as u32,
                    max_connections: pool.options().get_max_connections(),
                })
            };
        }
        let status = None;
        #[cfg(feature = "postgres")]
        let status = status.or_else(|| status!(conn.get_postgres_connection_pool()));
        #[cfg(feature = "mysql")]
        let status = status.or_else(|| status!(conn.get_mysql_connection_pool()));
        #[cfg(feature = "sqlite")]
        let status = status.or_else(|| status!(conn.get_sqlite_connection_pool()));
        status.ok_or_else(|| Error::connection("this handle has no connection pool"))
    }
}
