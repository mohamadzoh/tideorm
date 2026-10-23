//! Invalidation domains, separate from pool identities used in cache keys.
//! Two pools reaching one database must invalidate each other's reads.

#[cfg(feature = "sqlite")]
use crate::internal::ConnectionTrait;
#[cfg(any(feature = "sqlite", feature = "postgres", feature = "mysql"))]
use crate::internal::OrmBackend;
use crate::internal::OrmConnection;
use std::sync::Arc;

pub(crate) type CacheScope = Arc<parking_lot::RwLock<Option<Arc<str>>>>;

#[cfg(any(feature = "sqlite", feature = "postgres", feature = "mysql"))]
pub(crate) async fn identify(conn: &OrmConnection) -> crate::Result<Option<Arc<str>>> {
    let scope = match conn.get_database_backend() {
        #[cfg(feature = "sqlite")]
        OrmBackend::Sqlite => {
            let row = conn
                .query_one_raw(crate::internal::OrmStatement::from_string(
                    OrmBackend::Sqlite,
                    "SELECT file FROM pragma_database_list WHERE name = 'main'",
                ))
                .await
                .map_err(crate::internal::translate_error)?;
            let Some(row) = row else { return Ok(None) };
            let file: String = row
                .try_get("", "file")
                .map_err(crate::internal::translate_error)?;
            if file.is_empty() {
                let options = conn.get_sqlite_connection_pool().connect_options();
                if options.get_filename() == std::path::Path::new(":memory:") {
                    format!("sqlite-private:{}", uuid::Uuid::new_v4())
                } else {
                    format!("sqlite-memory:{:?}", options.get_filename())
                }
            } else {
                let file = std::fs::canonicalize(&file).unwrap_or_else(|_| file.into());
                format!("sqlite-file:{file:?}")
            }
        }
        #[cfg(feature = "postgres")]
        OrmBackend::Postgres => {
            let options = conn.get_postgres_connection_pool().connect_options();
            format!(
                "postgres:{:?}:{:?}:{}:{:?}",
                options.get_socket(),
                options.get_host().to_ascii_lowercase(),
                options.get_port(),
                options.get_database().unwrap_or(options.get_username())
            )
        }
        #[cfg(feature = "mysql")]
        OrmBackend::MySql => {
            let options = conn.get_mysql_connection_pool().connect_options();
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
        _ => return Ok(None),
    };
    Ok(Some(scope.into()))
}

#[cfg(not(any(feature = "sqlite", feature = "postgres", feature = "mysql")))]
pub(crate) async fn identify(_conn: &OrmConnection) -> crate::Result<Option<Arc<str>>> {
    Ok(None)
}

impl super::ConnectionRef {
    pub(crate) fn cache_scope(&self) -> Option<Arc<str>> {
        self.source().cache_scope.read().clone()
    }

    /// Attached SQLite databases can overlap any other pool's database. Keep
    /// conservative global invalidation once this connection uses ATTACH;
    /// DETACH cannot prove every pooled connection detached the same database.
    pub(crate) fn observe_cache_sql(&self, sql: &str) {
        if self.backend() != crate::internal::Backend::Sqlite {
            return;
        }
        let mut statement_start = true;
        let attaches = crate::internal::sql_lexer::sqlite_significant(sql).any(|token| {
            let attach = statement_start && token.is_word(&["attach"]);
            statement_start = token.text == ";";
            attach
        });
        if attaches {
            *self.source().cache_scope.write() = None;
            self.recording(|| crate::cache::QueryCache::global().clear());
        }
    }
}
