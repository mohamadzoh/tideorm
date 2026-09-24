use parking_lot::{Mutex, RwLock};
use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};

use crate::cache::{PendingInvalidations, install_pending_invalidations};
use crate::error::{Error, Result};
use crate::internal::InternalConnection;

use super::{ConnectionRef, Database};

static GLOBAL_DB: OnceLock<Database> = OnceLock::new();
static GLOBAL_CONNECTION: RwLock<Option<Arc<InternalConnection>>> = RwLock::new(None);

thread_local! {
    static DB_OVERRIDE: RefCell<Option<ConnectionRef>> = const { RefCell::new(None) };
}

const NOT_INITIALIZED: &str = "Global database connection not initialized. \
     Call Database::init() or Database::set_global() before using models.";

/// The error every accessor reports when no global connection has been set.
pub(super) fn not_initialized() -> Error {
    Error::connection(NOT_INITIALIZED)
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
/// Panics if the global connection has not been initialized.
pub fn db() -> &'static Database {
    let db = global_db_handle();
    if !db.is_connected() {
        panic!("{NOT_INITIALIZED} Use try_db() for a non-panicking alternative.");
    }
    db
}

/// Get the global database handle, returning an error if not initialized.
pub fn require_db() -> Result<Database> {
    global_connection().map(Database::from_handle)
}

/// Try to get the global database handle.
pub fn try_db() -> Option<Database> {
    require_db().ok()
}

/// Check whether a global database connection has been initialized.
pub fn has_global_db() -> bool {
    GLOBAL_CONNECTION.read().is_some()
}

struct ResetDbOverride(Option<ConnectionRef>);

impl Drop for ResetDbOverride {
    fn drop(&mut self) {
        DB_OVERRIDE.with(|slot| {
            slot.replace(self.0.take());
        });
    }
}

fn install_db_override(connection: &ConnectionRef) -> ResetDbOverride {
    let previous = DB_OVERRIDE.with(|slot| slot.replace(Some(connection.clone())));
    ResetDbOverride(previous)
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
    match DB_OVERRIDE.with(|slot| slot.borrow().clone()) {
        Some(connection) => Ok(connection),
        None => global_connection(),
    }
}

/// Run `future` with `connection` installed as the scope's connection, and
/// for a transaction scope, `pending` as the record of what its writes
/// invalidate in the query cache.
///
/// The override is a thread-local installed around each `poll` and removed
/// right after, never held across an await, so it follows the future when a
/// work-stealing runtime moves it to another thread.
pub(super) fn with_connection_override<F>(
    connection: ConnectionRef,
    pending: Option<Arc<Mutex<PendingInvalidations>>>,
    future: F,
) -> impl Future<Output = F::Output>
where
    F: Future,
{
    struct ScopedOverrideFuture<F> {
        connection: Option<ConnectionRef>,
        pending: Option<Arc<Mutex<PendingInvalidations>>>,
        future: Pin<Box<F>>,
    }

    impl<F> Future for ScopedOverrideFuture<F>
    where
        F: Future,
    {
        type Output = F::Output;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let this = self.get_mut();
            let result = match this.connection.as_ref() {
                Some(connection) => {
                    let guard = install_db_override(connection);
                    let pending = this.pending.as_ref().map(install_pending_invalidations);
                    let result = this.future.as_mut().poll(cx);
                    drop(pending);
                    drop(guard);
                    result
                }
                None => this.future.as_mut().poll(cx),
            };
            // Release the scoped connection as soon as the wrapped future
            // completes — while we are still being polled inside the async
            // runtime — instead of retaining it until this wrapper future is
            // dropped. The wrapper may be dropped outside any runtime context
            // (e.g. after a cross-thread move), and dropping a pooled database
            // connection there panics with "requires a Tokio context".
            if result.is_ready() {
                this.connection = None;
                this.pending = None;
            }
            result
        }
    }

    ScopedOverrideFuture {
        connection: Some(connection),
        pending,
        future: Box::pin(future),
    }
}
