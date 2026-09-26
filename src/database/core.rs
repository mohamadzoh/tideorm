use std::sync::Arc;

use crate::error::{Error, Result};
use crate::internal::InternalConnection;
use crate::tide_warn;

use super::ConnectionRef;
use super::state::{global_connection, global_db_handle, has_global_db, set_global_connection};

#[derive(Clone)]
enum DatabaseInner {
    Global,
    Handle(ConnectionRef),
    #[cfg(test)]
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
///
/// # Which connection a call uses
///
/// Statements *executed* through a handle join the transaction an enclosing
/// [`Database::transaction`] scope installed, and only fall back to the
/// handle's own connection outside such a scope. Otherwise `db.transaction(..)`
/// on a pooled handle inside a transaction would check out a second connection
/// and start an independent top-level transaction that cannot see the
/// surrounding scope's uncommitted writes; deferring makes it nest as a
/// SAVEPOINT instead.
///
/// Questions *about* a handle — [`Database::backend`], [`Database::ping`] — are
/// answered by the handle itself, never by the ambient scope: a replica pinged
/// inside a transaction on the primary reports on the replica, and an
/// `EntityManager` holding its own SQLite handle renders SQLite SQL even with no
/// global connection set.
#[derive(Clone)]
pub struct Database {
    inner: DatabaseInner,
}

impl Database {
    #[cfg(test)]
    pub(crate) fn disconnected() -> Self {
        Self {
            inner: DatabaseInner::Disconnected,
        }
    }

    pub(super) fn from_handle(connection: ConnectionRef) -> Self {
        Self {
            inner: DatabaseInner::Handle(connection),
        }
    }

    pub(super) fn global_handle() -> Self {
        Self {
            inner: DatabaseInner::Global,
        }
    }

    pub(super) fn from_internal_connection(inner: InternalConnection) -> Self {
        Self::from_handle(ConnectionRef::Database(Arc::new(inner)))
    }

    /// The connection statements issued through this handle run on: the
    /// ambient transaction if there is one, otherwise the handle's own.
    #[doc(hidden)]
    pub fn __get_connection(&self) -> Result<ConnectionRef> {
        match super::__current_connection() {
            Ok(transaction @ ConnectionRef::Transaction(_)) => Ok(transaction),
            _ => self.own_handle(),
        }
    }

    /// The connection stored in `self`, ignoring any ambient scope.
    pub(super) fn own_handle(&self) -> Result<ConnectionRef> {
        match &self.inner {
            DatabaseInner::Handle(connection) => Ok(connection.clone()),
            DatabaseInner::Global => global_connection(),
            #[cfg(test)]
            DatabaseInner::Disconnected => Err(super::state::not_initialized()),
        }
    }

    /// The pool behind this handle — not whichever transaction is ambient.
    pub(crate) fn current_inner(&self) -> Result<Arc<InternalConnection>> {
        match self.own_handle()? {
            ConnectionRef::Database(inner) => Ok(inner),
            ConnectionRef::Transaction(_) => Err(Error::connection(
                "Current database context is a transaction, not a pooled database connection",
            )),
        }
    }

    pub(super) fn is_connected(&self) -> bool {
        match &self.inner {
            DatabaseInner::Handle(_) => true,
            DatabaseInner::Global => has_global_db(),
            #[cfg(test)]
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
        set_global_connection(Some(inner));
        #[cfg(feature = "dirty-tracking")]
        crate::model::__clear_dirty_snapshots();
        Ok(global_db_handle())
    }

    /// Clear the global database connection.
    pub fn reset_global() {
        set_global_connection(None);
        #[cfg(feature = "dirty-tracking")]
        crate::model::__clear_dirty_snapshots();
    }

    /// Get a reference to the global database connection.
    ///
    /// Panics if it has not been initialized; [`crate::try_db`] is the
    /// non-panicking form.
    pub fn global() -> &'static Self {
        super::db()
    }

    /// Synchronize database schema with registered models
    pub async fn sync(&self) -> Result<()> {
        crate::sync::sync_database(self).await
    }

    /// Get the raw internal connection (for internal use only)
    #[doc(hidden)]
    pub fn __internal_connection(&self) -> Result<crate::internal::OrmConnection> {
        Ok(self.current_inner()?.connection().clone())
    }

    /// Get the database backend type
    ///
    /// The handle's own connection is authoritative. Configuration is only
    /// consulted where the handle cannot answer — `internal::Backend` reports
    /// MariaDB as MySQL, so a configured `MariaDB` wins over a MySQL-shaped
    /// handle — and as a fallback when there is no usable handle at all.
    pub fn backend(&self) -> crate::config::DatabaseType {
        let configured = crate::config::TideConfig::get_database_type();

        match self.__internal_backend() {
            Ok(backend) => Self::resolve_backend(configured, backend),
            Err(err) => configured.unwrap_or_else(|| {
                tide_warn!(
                    "Unable to inspect database backend for disconnected handle: {}. Defaulting to Postgres",
                    err
                );
                crate::config::DatabaseType::Postgres
            }),
        }
    }

    /// The backend of the connection this handle's statements run on: the
    /// enclosing transaction's when there is one, since a handle used inside a
    /// transaction executes on it.
    pub(crate) fn execution_backend(&self) -> crate::config::DatabaseType {
        match self.__get_connection() {
            Ok(connection) => Self::resolve_backend(
                crate::config::TideConfig::get_database_type(),
                connection.backend(),
            ),
            Err(_) => self.backend(),
        }
    }

    /// Reconcile the handle's real backend with the configured database type.
    fn resolve_backend(
        configured: Option<crate::config::DatabaseType>,
        backend: crate::internal::Backend,
    ) -> crate::config::DatabaseType {
        if backend == crate::internal::Backend::MySql
            && configured == Some(crate::config::DatabaseType::MariaDB)
        {
            return crate::config::DatabaseType::MariaDB;
        }

        backend.as_database_type()
    }

    /// Get TideORM's runtime backend identifier (for internal use only)
    #[doc(hidden)]
    pub fn __internal_backend(&self) -> Result<crate::internal::Backend> {
        Ok(self.own_handle()?.backend())
    }
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("connected", &self.is_connected())
            .finish()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/database_core_tests.rs"]
mod tests;
