//! Invalidation domains, separate from pool identities used in cache keys.
//! Two pools reaching one database must invalidate each other's reads.

#[cfg(any(feature = "sqlite", feature = "postgres", feature = "mysql"))]
use crate::config::DatabaseType;
use crate::internal::InternalConnection;
use std::sync::Arc;

pub(crate) type CacheScope = Arc<parking_lot::RwLock<Option<Arc<str>>>>;

#[cfg(any(feature = "sqlite", feature = "postgres", feature = "mysql"))]
pub(crate) fn identify(connection: &InternalConnection) -> Option<Arc<str>> {
    let conn = &connection.conn;
    let scope = match conn.get_database_backend() {
        #[cfg(feature = "sqlite")]
        DatabaseType::SQLite => match &connection.sqlite_file {
            Some(file) => format!("sqlite-file:{file:?}"),
            None => {
                let pool = conn.get_sqlite_connection_pool()?;
                let options = pool.connect_options();
                if options.is_connection_private() {
                    format!("sqlite-private:{}", uuid::Uuid::new_v4())
                } else {
                    format!("sqlite-memory:{:?}", options.get_filename())
                }
            }
        },
        #[cfg(feature = "postgres")]
        DatabaseType::Postgres => {
            let pool = conn.get_postgres_connection_pool()?;
            let options = pool.connect_options();
            format!(
                "postgres:{:?}:{:?}:{}:{:?}",
                options.get_socket(),
                options.get_host().to_ascii_lowercase(),
                options.get_port(),
                options.get_database().unwrap_or(options.get_username())
            )
        }
        #[cfg(feature = "mysql")]
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            let pool = conn.get_mysql_connection_pool()?;
            let options = pool.connect_options();
            // MySQL triggers and foreign keys can cross database/schema names
            // on one server, so that server remains one invalidation domain.
            format!(
                "mysql:{:?}:{:?}:{}",
                options.get_socket(),
                options.get_host().to_ascii_lowercase(),
                options.get_port()
            )
        }
        #[allow(unreachable_patterns)]
        _ => return None,
    };
    Some(scope.into())
}

#[cfg(not(any(feature = "sqlite", feature = "postgres", feature = "mysql")))]
pub(crate) fn identify(_connection: &InternalConnection) -> Option<Arc<str>> {
    None
}

impl super::ConnectionRef {
    pub(crate) fn cache_scope(&self) -> Option<Arc<str>> {
        self.observe_attach();
        self.pool().cache_scope.read().clone()
    }

    /// Attached SQLite databases can overlap any other pool's database. Keep
    /// conservative global invalidation once a connection of this pool has
    /// attached one, as the driver notes for every `ATTACH` it prepares;
    /// DETACH cannot prove every pooled connection detached the same database.
    /// SQLite refuses `ATTACH` inside a transaction, so only a pool's own
    /// statements attach; a transaction takes the scope its pool has when it
    /// begins.
    pub(crate) fn observe_attach(&self) {
        #[cfg(feature = "sqlite")]
        if let Self::Database(pool) = self
            && pool
                .conn
                .get_sqlite_connection_pool()
                .is_some_and(|sqlite| sqlite.connect_options().has_attached())
        {
            let widened = pool.cache_scope.write().take().is_some();
            if widened {
                self.recording(|| crate::cache::QueryCache::global().clear());
            }
        }
    }
}
