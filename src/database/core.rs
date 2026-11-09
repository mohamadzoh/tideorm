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

    pub(crate) fn from_handle(connection: ConnectionRef) -> Self {
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

    /// A handle on one connection of its own to this handle's database,
    /// opened with its pool's settings but outside the pool, so it takes none
    /// of the pool's connections. What a statement sets for its session (a
    /// `SET`, a `PRAGMA`, `GET_LOCK`) holds for every later statement through
    /// the handle, which a pool, handing each statement whichever connection
    /// is free, cannot promise. Transactions opened on it run on that
    /// connection too.
    ///
    /// The connection closes with [`close`](Self::close) or once the last
    /// clone of the handle drops, and is never replaced: once it is lost, the
    /// handle's statements fail. On a session, this opens another one. Fails
    /// on a handle to a transaction, which belongs to its scope, and on a
    /// private SQLite database (in memory without a shared cache, or a
    /// temporary one), where a new connection would open an empty database.
    pub async fn pin_session(&self) -> Result<Database> {
        self.session_beside(false).await
    }

    /// [`pin_session`](Self::pin_session) under this handle's identity: a
    /// statement through a handle on this database joins a transaction opened
    /// on the session, as one opened on the pool itself.
    #[cfg(feature = "sqlite")]
    pub(crate) async fn pin_session_sharing_identity(&self) -> Result<Database> {
        self.session_beside(true).await
    }

    /// A session on this handle's database, sharing its query-cache scope,
    /// and its identity when `shared`.
    async fn session_beside(&self, shared: bool) -> Result<Database> {
        let pool = self.current_inner()?;
        let session = if shared {
            pool.connection().pin_session_sharing_identity().await
        } else {
            pool.connection().pin_session().await
        }
        .map_err(crate::internal::translate_connection_error)?;
        let mut inner = InternalConnection::new(session);
        inner.cache_scope = pool.cache_scope.clone();
        #[cfg(feature = "sqlite")]
        {
            inner.sqlite_file = pool.sqlite_file.clone();
        }
        Ok(Self::from_internal_connection(inner))
    }

    /// The connection statements issued through this handle run on: the
    /// innermost enclosing transaction opened on this handle's database,
    /// otherwise the handle's own. Another database's transaction is not
    /// joined, or a statement meant for one database would run on the other;
    /// it is looked through, so an outer transaction of this database is still
    /// joined inside it.
    #[doc(hidden)]
    pub fn __get_connection(&self) -> Result<ConnectionRef> {
        let own = self.own_handle()?;
        Ok(super::state::enclosing_transaction(own.identity()).unwrap_or(own))
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
            ConnectionRef::Transaction(..) => Err(Error::connection(
                "Current database context is a transaction, not a pooled database connection",
            )),
        }
    }

    /// A handle on the pool behind this one: this handle's own for a pool or
    /// a session, the one its transaction was begun on for a transaction's,
    /// whose own connection belongs to the transaction's scope.
    pub(crate) fn pool_handle(&self) -> Result<Database> {
        Ok(Self::from_handle(ConnectionRef::Database(Arc::clone(
            self.own_handle()?.pool(),
        ))))
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
        Self::builder().url(url).build().await
    }

    /// Initialize the global database connection
    pub async fn init(url: &str) -> Result<&'static Self> {
        let db = Self::connect(url).await?;
        Self::set_global(db)
    }

    /// The database a TideORM call that names no handle runs on here: the
    /// enclosing transaction's, else [`scope`](Self::scope)'s, else the
    /// global one. `Database::current()?.atomically(..)` opens a transaction
    /// on it, a savepoint inside an enclosing transaction.
    ///
    /// Inside a transaction this is the transaction's handle: drop it before
    /// the transaction ends, or the commit fails with "transaction handle
    /// leaked outside the transaction scope". Fails as
    /// [`require_db`](crate::require_db) does when there is no database.
    pub fn current() -> Result<Self> {
        super::__current_db()
    }

    /// Set an existing database connection as the global connection
    pub fn set_global(db: Self) -> Result<&'static Self> {
        set_global_connection(Some(db.current_inner()?));
        #[cfg(feature = "dirty-tracking")]
        crate::model::clear_dirty_snapshots();
        Ok(global_db_handle())
    }

    /// Clear the global database connection.
    pub fn reset_global() {
        set_global_connection(None);
        #[cfg(feature = "dirty-tracking")]
        crate::model::clear_dirty_snapshots();
    }

    /// Synchronize database schema with registered models
    pub async fn sync(&self) -> Result<()> {
        crate::sync::sync_database(self).await
    }

    /// Close the connection pool behind this handle, or its session's
    /// connection ([`pin_session`](Self::pin_session)).
    ///
    /// Waits for checked-out connections to come back, then closes every
    /// connection, so a process can shut down cleanly (and SQLite releases
    /// its database file). Every clone of the handle shares the pool, so
    /// statements on any of them fail with a connection error afterwards; on
    /// the global handle, [`Database::reset_global`] also forgets the closed
    /// pool. Fails on a handle to a transaction, which belongs to its scope.
    pub async fn close(&self) -> Result<()> {
        self.current_inner()?
            .connection()
            .close_by_ref()
            .await
            .map_err(crate::internal::translate_connection_error)
    }

    /// Get the raw internal connection (for internal use only)
    #[doc(hidden)]
    pub fn __internal_connection(&self) -> Result<crate::internal::OrmConnection> {
        Ok(self.current_inner()?.connection().clone())
    }

    /// Get the database backend type
    ///
    /// The handle's own connection is authoritative: a pool on a
    /// MySQL-protocol server asks it whether it is MariaDB when it opens, and
    /// a transaction carries its pool's answer. The configured type is only a
    /// fallback when there is no usable handle at all, and with none of those
    /// either this answers `Postgres`; [`Database::try_backend`] reports that
    /// case as an error instead.
    pub fn backend(&self) -> crate::config::DatabaseType {
        match self.own_handle() {
            Ok(handle) => handle.backend(),
            Err(err) => crate::config::TideConfig::get_database_type().unwrap_or_else(|| {
                tide_warn!(
                    "Unable to inspect database backend for disconnected handle: {}. Defaulting to Postgres",
                    err
                );
                crate::config::DatabaseType::Postgres
            }),
        }
    }

    /// Get the database backend type, or the connection error of a handle
    /// that has no connection.
    ///
    /// [`Database::backend`] without its guess: a disconnected handle is an
    /// error here, even when `TideConfig` declares a type.
    pub fn try_backend(&self) -> Result<crate::config::DatabaseType> {
        Ok(self.own_handle()?.backend())
    }

    /// The backend of the connection this handle's statements run on: the
    /// enclosing transaction's when there is one, since a handle used inside a
    /// transaction executes on it.
    pub(crate) fn execution_backend(&self) -> crate::config::DatabaseType {
        match self.__get_connection() {
            Ok(connection) => connection.backend(),
            Err(_) => self.backend(),
        }
    }
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("connected", &self.is_connected())
            .finish()
    }
}
