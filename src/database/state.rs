use parking_lot::RwLock;
use std::cell::RefCell;
use std::future::Future;
use std::sync::{Arc, OnceLock};

use crate::cache::replace_pending_invalidations;
use crate::error::{Error, Result};
use crate::internal::InternalConnection;

use super::{ConnectionRef, Database};

static GLOBAL_DB: OnceLock<Database> = OnceLock::new();
static GLOBAL_CONNECTION: RwLock<Option<Arc<InternalConnection>>> = RwLock::new(None);

/// A connection scope: the connection its statements run on, and the scope
/// it was opened in.
struct Scope {
    connection: ConnectionRef,
    enclosing: Option<Arc<Scope>>,
}

thread_local! {
    /// The innermost connection scope, linked to the scopes enclosing it.
    static DB_OVERRIDE: RefCell<Option<Arc<Scope>>> = const { RefCell::new(None) };
}

const NOT_INITIALIZED: &str = "Global database connection not initialized. \
     Call Database::init() or Database::set_global() before using models.";

/// The error every accessor reports when no global connection has been set.
pub(super) fn not_initialized() -> Error {
    use crate::error::{DbFailure, DbFailureKind};

    Error::connection(NOT_INITIALIZED)
        .with_db_failure(Some(DbFailure::new(DbFailureKind::NotConnected)))
}

/// Install `connection` as the global pooled connection, or clear it with `None`.
pub(super) fn set_global_connection(connection: Option<Arc<InternalConnection>>) {
    // Swap under the lock, but drop the previous connection (which can close its
    // pool) only after the lock is released.
    let previous = std::mem::replace(&mut *GLOBAL_CONNECTION.write(), connection);
    drop(previous);
}

pub(super) fn global_db_handle() -> &'static Database {
    GLOBAL_DB.get_or_init(Database::global_handle)
}

/// The global pooled connection.
pub(super) fn global_connection() -> Result<ConnectionRef> {
    GLOBAL_CONNECTION
        .read()
        .clone()
        .map(ConnectionRef::Database)
        .ok_or_else(not_initialized)
}

/// Get a reference to the global database connection.
///
/// Panics if the global connection has not been initialized; the panic
/// names the caller's location. [`require_db`] returns the error instead.
#[track_caller]
pub fn db() -> &'static Database {
    let db = global_db_handle();
    if !db.is_connected() {
        panic!("{NOT_INITIALIZED} Use require_db() for a non-panicking alternative.");
    }
    db
}

/// Get the global database handle, returning an error if not initialized.
pub fn require_db() -> Result<Database> {
    global_connection().map(Database::from_handle)
}

/// Check whether a global database connection has been initialized.
pub fn has_global_db() -> bool {
    GLOBAL_CONNECTION.read().is_some()
}

struct ResetDbOverride(Option<Arc<Scope>>);

impl Drop for ResetDbOverride {
    fn drop(&mut self) {
        DB_OVERRIDE.with(|slot| {
            slot.replace(self.0.take());
        });
    }
}

fn install_db_override(scope: &Arc<Scope>) -> ResetDbOverride {
    ResetDbOverride(DB_OVERRIDE.with(|slot| slot.replace(Some(scope.clone()))))
}

/// The innermost enclosing transaction a handle on the pool identified by
/// `own` joins: the innermost scope on that pool, if it is a transaction. A
/// scope on another database's pool is looked through, so a handle on an
/// outer database still joins its transaction inside another one's.
pub(crate) fn enclosing_transaction(own: u64) -> Option<ConnectionRef> {
    DB_OVERRIDE.with(|slot| {
        let slot = slot.borrow();
        let mut scope = slot.as_deref();
        while let Some(current) = scope {
            if current.connection.identity() == own {
                return matches!(current.connection, ConnectionRef::Transaction(..))
                    .then(|| current.connection.clone());
            }
            scope = current.enclosing.as_deref();
        }
        None
    })
}

/// The identity of the pool the current scope's statements run on: the
/// enclosing transaction's, if any, otherwise the global connection's.
#[cfg(feature = "dirty-tracking")]
pub(crate) fn scope_origin() -> Option<u64> {
    DB_OVERRIDE
        .with(|slot| {
            slot.borrow()
                .as_ref()
                .map(|scope| scope.connection.identity())
        })
        .or_else(|| {
            GLOBAL_CONNECTION
                .read()
                .as_ref()
                .map(|connection| connection.connection().identity())
        })
}

/// The database handle for the current scope: the enclosing transaction, if
/// any, otherwise the global connection.
#[doc(hidden)]
pub fn __current_db() -> Result<Database> {
    __current_connection().map(Database::from_handle)
}

/// The connection for the current scope: the enclosing transaction, if any,
/// otherwise the global connection.
#[doc(hidden)]
pub fn __current_connection() -> Result<ConnectionRef> {
    match DB_OVERRIDE.with(|slot| slot.borrow().as_ref().map(|scope| scope.connection.clone())) {
        Some(connection) => Ok(connection),
        None => global_connection(),
    }
}

/// Run `future` with `connection` installed as the scope's connection, and
/// its transaction's record, or none for a pool, as the record of what the
/// scope's writes invalidate in the query cache and register to undo.
///
/// The override is a thread-local installed around each `poll` and removed
/// right after, never held across an await, so it follows the future when a
/// work-stealing runtime moves it to another thread. The scope remembers the
/// one this is called in, which encloses it.
pub(crate) fn with_connection_override<F>(
    connection: ConnectionRef,
    future: F,
) -> impl Future<Output = F::Output>
where
    F: Future,
{
    let record = connection.record().cloned();
    let scope = Arc::new(Scope {
        connection,
        enclosing: DB_OVERRIDE.with(|slot| slot.borrow().clone()),
    });
    crate::internal::per_poll(future, move || {
        let guard = install_db_override(&scope);
        let pending = replace_pending_invalidations(record.clone());
        // A tuple drops its fields in order: the invalidations first.
        (pending, guard)
    })
}
