use std::sync::Arc;

use crate::error::{Error, Result};
use crate::internal::InternalConnection;
use crate::tide_warn;

use super::ConnectionRef;
use super::DatabaseHandle;
use super::state::{
    current_scope_handle, global_connection_slot, global_db_handle, panic_missing_global_db,
};

/// Return the transaction installed by an enclosing transaction scope, if any.
///
/// Only a scoped override can produce a transaction handle — the fallback in
/// `current_scope_handle` always resolves to a pooled connection — so a
/// `Transaction` here reliably means "we are inside a transaction closure".
fn ambient_transaction_handle() -> Option<DatabaseHandle> {
    match current_scope_handle() {
        Ok(handle @ DatabaseHandle::Transaction(_)) => Some(handle),
        _ => None,
    }
}

#[derive(Clone)]
enum DatabaseInner {
    Global,
    Handle(DatabaseHandle),
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

    /// Resolve the connection handle statements issued here have to run on.
    ///
    /// An ambient transaction installed by an enclosing `Database::transaction`
    /// scope wins over the handle stored in `self`. Without that, calling
    /// `db.transaction(..)` on a global or pooled handle from inside a
    /// transaction would check out a *different* pooled connection and start a
    /// second, independent top-level transaction — one that commits or rolls
    /// back on its own and cannot see the surrounding scope's uncommitted
    /// writes. Deferring to the ambient transaction makes it nest (SAVEPOINT),
    /// matching `Model::transaction`, which resolves its handle via
    /// `__current_db()`.
    ///
    /// Outside any transaction scope this resolves from `self` exactly as
    /// before, so a plain `db.transaction(..)` still opens a normal top-level
    /// transaction on the caller's own connection.
    ///
    /// This is for *executing* statements. Metadata about a handle — its
    /// backend, whether it is responsive — is never resolved through here: a
    /// question asked of a specific handle has to be answered by that handle,
    /// so those callers read `own_handle` instead.
    pub(super) fn current_handle(&self) -> Result<DatabaseHandle> {
        if let Some(handle) = ambient_transaction_handle() {
            return Ok(handle);
        }

        self.own_handle()
    }

    /// Resolve the handle stored in `self`, ignoring any ambient scope.
    fn own_handle(&self) -> Result<DatabaseHandle> {
        match &self.inner {
            DatabaseInner::Handle(handle) => Ok(handle.clone()),
            DatabaseInner::Global => global_connection_slot()
                .load_full()
                .map(DatabaseHandle::Connection)
                .ok_or_else(|| {
                    Error::connection(
                        "Global database connection not initialized. \
                         Call Database::init() or Database::set_global() before using models."
                            .to_string(),
                    )
                }),
            #[cfg(test)]
            DatabaseInner::Disconnected => Err(Error::connection(
                "Global database connection not initialized. \
                 Call Database::init() or Database::set_global() before using models."
                    .to_string(),
            )),
        }
    }

    /// Resolve `self`'s own connection, ignoring any ambient transaction scope.
    ///
    /// Used by the pure metadata operations — `ping` — so that a handle answers
    /// for itself: a replica pinged from inside a transaction opened on the
    /// primary has to report on the replica.
    ///
    /// Deliberately NOT used for the backend accessors. SQL rendered from a
    /// handle's backend is executed through `__get_connection`, which honours the
    /// ambient transaction, so answering from `own_handle` here would render for
    /// one server and execute on another.
    pub(super) fn own_connection(&self) -> Result<ConnectionRef> {
        Ok(match self.own_handle()? {
            DatabaseHandle::Connection(inner) => ConnectionRef::Database(inner),
            DatabaseHandle::Transaction(tx) => ConnectionRef::Transaction(tx),
            #[cfg(test)]
            DatabaseHandle::TestScope => {
                unreachable!("test scope marker does not carry a connection reference")
            }
        })
    }

    /// Resolve this database's own pooled connection.
    ///
    /// Deliberately reads `own_handle` rather than `current_handle`: callers
    /// such as `set_global` and `__internal_connection` want the pool behind
    /// this handle, not whichever transaction happens to be ambient.
    pub(crate) fn current_inner(&self) -> Result<Arc<InternalConnection>> {
        match self.own_handle()? {
            DatabaseHandle::Connection(inner) => Ok(inner),
            DatabaseHandle::Transaction(_) => Err(Error::connection(
                "Current database context is a transaction, not a pooled database connection"
                    .to_string(),
            )),
            #[cfg(test)]
            DatabaseHandle::TestScope => {
                unreachable!("test scope marker does not carry a pooled connection")
            }
        }
    }

    pub(super) fn is_connected(&self) -> bool {
        match &self.inner {
            DatabaseInner::Handle(_) => true,
            DatabaseInner::Global => global_connection_slot().load_full().is_some(),
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
        global_connection_slot().store(Some(inner));
        #[cfg(feature = "dirty-tracking")]
        crate::model::__clear_dirty_snapshots();
        Ok(global_db_handle())
    }

    /// Clear the global database connection.
    pub fn reset_global() {
        global_connection_slot().store(None);
        #[cfg(feature = "dirty-tracking")]
        crate::model::__clear_dirty_snapshots();
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
    pub fn __internal_connection(&self) -> Result<crate::internal::OrmConnection> {
        Ok(self.current_inner()?.connection().clone())
    }

    /// Get the database backend type
    ///
    /// This handle's own connection is authoritative: whichever dialect the
    /// driver actually speaks is the one placeholders and identifier quoting
    /// have to target, so a second handle opened against another backend never
    /// inherits the first one's dialect — not even inside a transaction scope
    /// opened on that other handle. Configuration is consulted only where
    /// the handle cannot answer — `internal::Backend` deliberately collapses
    /// MySQL and MariaDB into one variant, so a configured `MariaDB` still wins
    /// over the handle's `MySQL` — and as a fallback when there is no usable
    /// handle at all.
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

    /// Reconcile the handle's real backend with the configured database type.
    ///
    /// The handle wins everywhere except the single distinction it cannot make:
    /// the ORM engine reports MariaDB as MySQL, so a configured `MariaDB` is
    /// preserved on a MySQL-shaped backend.
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
    ///
    /// Reads `own_handle`: reporting the ambient transaction's backend here
    /// would make a handle describe a connection it does not speak for, and the
    /// SQL rendered from that answer would carry the wrong placeholder style
    /// and identifier quoting.
    #[doc(hidden)]
    pub fn __internal_backend(&self) -> Result<crate::internal::Backend> {
        use crate::internal::ConnectionTrait;

        // Deliberately `own_handle`, not `current_handle`. Resolving the ambient
        // scope here answers for a connection this handle does not speak for, and
        // it breaks the case that has no ambient scope to fall back to: an
        // `EntityManager` holding its own database with no global connection set
        // resolves to the *global* slot, fails, and `backend()` then warns and
        // guesses Postgres — so a SQLite-backed manager renders `$1` placeholders.
        //
        // The cross-dialect render/execute mismatch this could cause instead needs
        // two handles on different backends inside one transaction scope, which
        // `Database::backend()` documents as answering from the handle you called
        // it on either way.
        Ok(match self.own_handle()? {
            DatabaseHandle::Connection(inner) => {
                crate::internal::Backend::from(inner.connection().get_database_backend())
            }
            DatabaseHandle::Transaction(tx) => {
                crate::internal::Backend::from(tx.as_ref().get_database_backend())
            }
            #[cfg(test)]
            DatabaseHandle::TestScope => {
                unreachable!("test scope marker does not carry a database backend")
            }
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

#[cfg(test)]
mod tests {
    use super::Database;
    use crate::config::DatabaseType;
    use crate::internal::Backend;

    #[test]
    fn resolved_backend_prefers_the_live_handle_over_configuration() {
        assert_eq!(
            Database::resolve_backend(Some(DatabaseType::Postgres), Backend::Sqlite),
            DatabaseType::SQLite
        );
        assert_eq!(
            Database::resolve_backend(Some(DatabaseType::SQLite), Backend::Postgres),
            DatabaseType::Postgres
        );
        assert_eq!(
            Database::resolve_backend(Some(DatabaseType::Postgres), Backend::MySql),
            DatabaseType::MySQL
        );
    }

    #[test]
    fn resolved_backend_keeps_configured_mariadb_on_a_mysql_handle() {
        assert_eq!(
            Database::resolve_backend(Some(DatabaseType::MariaDB), Backend::MySql),
            DatabaseType::MariaDB
        );
        assert_eq!(
            Database::resolve_backend(Some(DatabaseType::MariaDB), Backend::Sqlite),
            DatabaseType::SQLite
        );
    }

    #[test]
    fn resolved_backend_falls_back_to_the_handle_without_configuration() {
        assert_eq!(
            Database::resolve_backend(None, Backend::MySql),
            DatabaseType::MySQL
        );
        assert_eq!(
            Database::resolve_backend(None, Backend::Sqlite),
            DatabaseType::SQLite
        );
        assert_eq!(
            Database::resolve_backend(None, Backend::Postgres),
            DatabaseType::Postgres
        );
    }

    #[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
    #[tokio::test]
    async fn metadata_answers_for_the_handle_it_was_called_on() {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("sqlite in-memory connection should succeed");
        // Stands in for a second handle whose connection is not the ambient
        // one: whatever it reports has to come from itself, not from the
        // transaction running around it.
        let other = Database::disconnected();

        db.transaction(move |_| {
            Box::pin(async move {
                assert!(
                    other.__internal_backend().is_err(),
                    "a handle must report its own backend, not the ambient transaction's"
                );
                assert!(
                    other.ping().await.is_err(),
                    "a handle must be pinged through its own connection"
                );

                // Execution still joins the ambient transaction: that is what
                // makes a nested `transaction` a SAVEPOINT instead of a second,
                // independent top-level one.
                let nested: crate::error::Result<()> =
                    other.transaction(|_| Box::pin(async { Ok(()) })).await;
                nested.expect("a nested transaction should join the ambient one");

                Ok(())
            })
        })
        .await
        .expect("the outer transaction should commit");
    }
}
