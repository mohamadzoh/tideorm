use std::sync::Arc;

use crate::error::{Error, Result};
use crate::internal::InternalConnection;
use crate::tide_warn;

use super::DatabaseHandle;
use super::state::{global_connection_slot, global_db_handle, panic_missing_global_db};

#[derive(Clone)]
enum DatabaseInner {
    Global,
    Handle(DatabaseHandle),
    #[allow(dead_code)]
    Disconnected,
}

/// Database connection handle
///
/// This is the main entry point for all database operations in TideORM.
/// It manages the connection pool and provides transaction support.
///
/// # Thread Safety
///
/// `Database` is `Clone`, `Send`, and `Sync`. It can be safely shared across
/// threads and cloned without duplicating the underlying connection pool.
#[derive(Clone)]
pub struct Database {
    inner: DatabaseInner,
}

impl Database {
    #[allow(dead_code)]
    pub(crate) fn disconnected() -> Self {
        Self {
            inner: DatabaseInner::Disconnected,
        }
    }

    pub(super) fn from_handle(handle: DatabaseHandle) -> Self {
        Self {
            inner: DatabaseInner::Handle(handle),
        }
    }

    pub(super) fn global_handle() -> Self {
        Self {
            inner: DatabaseInner::Global,
        }
    }

    pub(super) fn from_internal_connection(inner: InternalConnection) -> Self {
        Self::from_handle(DatabaseHandle::Connection(Arc::new(inner)))
    }

    pub(super) fn current_handle(&self) -> Result<DatabaseHandle> {
        match &self.inner {
            DatabaseInner::Handle(handle) => Ok(handle.clone()),
            DatabaseInner::Global => {
                global_connection_slot().load_full().map(DatabaseHandle::Connection).ok_or_else(|| {
                    Error::connection(
                        "Global database connection not initialized. \
                         Call Database::init() or Database::set_global() before using models."
                            .to_string(),
                    )
                })
            }
            DatabaseInner::Disconnected => Err(Error::connection(
                "Global database connection not initialized. \
                 Call Database::init() or Database::set_global() before using models."
                    .to_string(),
            )),
        }
    }

    pub(crate) fn current_inner(&self) -> Result<Arc<InternalConnection>> {
        match self.current_handle()? {
            DatabaseHandle::Connection(inner) => Ok(inner),
            DatabaseHandle::Transaction(_) => Err(Error::connection(
                "Current database context is a transaction, not a pooled database connection"
                    .to_string(),
            )),
        }
    }

    pub(super) fn replace_thread_override(
        handle: Option<DatabaseHandle>,
    ) -> Option<DatabaseHandle> {
        super::state::THREAD_DB_OVERRIDE.with(|slot| slot.replace(handle))
    }

    pub(super) fn set_thread_override(handle: Option<DatabaseHandle>) {
        super::state::THREAD_DB_OVERRIDE.with(|slot| *slot.borrow_mut() = handle);
    }

    pub(super) fn is_connected(&self) -> bool {
        match &self.inner {
            DatabaseInner::Handle(_) => true,
            DatabaseInner::Global => global_connection_slot().load_full().is_some(),
            DatabaseInner::Disconnected => false,
        }
    }

    /// Connect to a database using a connection URL
    pub async fn connect(url: &str) -> Result<Self> {
        let inner = InternalConnection::connect(url).await?;
        Ok(Self::from_internal_connection(inner))
    }

    /// Initialize the global database connection
    pub async fn init(url: &str) -> Result<&'static Self> {
        let db = Self::connect(url).await?;
        Self::set_global(db)
    }

    /// Set an existing database connection as the global connection
    pub fn set_global(db: Self) -> Result<&'static Self> {
        let inner = db.current_inner()?;
        global_connection_slot().store(Some(inner.clone()));
        Self::set_thread_override(Some(DatabaseHandle::Connection(inner)));
        Ok(global_db_handle())
    }

    /// Clear the global database connection and current thread override.
    pub fn reset_global() {
        global_connection_slot().store(None);
        Self::set_thread_override(None);
    }

    /// Get a reference to the global database connection
    pub fn global() -> &'static Self {
        let db = global_db_handle();
        if db.is_connected() {
            db
        } else {
            panic_missing_global_db(
                "Global database connection not initialized. \
                 Call Database::init() or Database::set_global() before using models.",
            )
        }
    }

    /// Try to get the global database connection
    pub fn try_global() -> Option<Self> {
        super::try_db()
    }

    /// Synchronize database schema with registered models
    pub async fn sync(&self) -> Result<()> {
        crate::sync::sync_database(self).await
    }

    /// Get the raw internal connection (for internal use only)
    #[doc(hidden)]
    pub fn __internal_connection(&self) -> Result<crate::internal::DatabaseConnection> {
        Ok(self.current_inner()?.connection().clone())
    }

    /// Get the database backend type
    pub fn backend(&self) -> crate::config::DatabaseType {
        if let Some(db_type) = crate::config::TideConfig::get_database_type() {
            return db_type;
        }

        use crate::internal::DbBackend;
        match self.__internal_backend() {
            Ok(DbBackend::Postgres) => crate::config::DatabaseType::Postgres,
            Ok(DbBackend::MySql) => crate::config::DatabaseType::MySQL,
            Ok(DbBackend::Sqlite) => crate::config::DatabaseType::SQLite,
            Ok(other) => {
                tide_warn!(
                    "Unknown database backend {:?}, defaulting to Postgres",
                    other
                );
                crate::config::DatabaseType::Postgres
            }
            Err(err) => {
                tide_warn!(
                    "Unable to inspect database backend for disconnected handle: {}. Defaulting to Postgres",
                    err
                );
                crate::config::DatabaseType::Postgres
            }
        }
    }

    /// Get the raw SeaORM database backend (for internal use only)
    #[doc(hidden)]
    pub fn __internal_backend(&self) -> Result<crate::internal::DbBackend> {
        use crate::internal::ConnectionTrait;

        Ok(match self.current_handle()? {
            DatabaseHandle::Connection(inner) => inner.connection().get_database_backend(),
            DatabaseHandle::Transaction(tx) => tx.as_ref().get_database_backend(),
        })
    }
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("connected", &self.is_connected())
            .finish()
    }
}
