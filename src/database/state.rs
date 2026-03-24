use arc_swap::ArcSwapOption;
use std::cell::RefCell;
use std::future::Future;
use std::sync::{Arc, OnceLock};

use crate::error::{Error, Result};
use crate::internal::{DbBackend, InternalConnection};

use super::{ConnectionRef, Database};

static GLOBAL_DB: OnceLock<Database> = OnceLock::new();
static GLOBAL_CONNECTION: OnceLock<ArcSwapOption<InternalConnection>> = OnceLock::new();

thread_local! {
    pub(super) static THREAD_DB_OVERRIDE: RefCell<Option<DatabaseHandle>> = const { RefCell::new(None) };
}

#[derive(Clone)]
pub(crate) enum DatabaseHandle {
    Connection(Arc<InternalConnection>),
    Transaction(Arc<crate::internal::DatabaseTransaction>),
}

pub(super) fn global_connection_slot() -> &'static ArcSwapOption<InternalConnection> {
    GLOBAL_CONNECTION.get_or_init(|| ArcSwapOption::new(None))
}

pub(super) fn global_db_handle() -> &'static Database {
    GLOBAL_DB.get_or_init(Database::global_handle)
}

pub(super) fn panic_missing_global_db(message: &str) -> ! {
    panic!("{}", message)
}

/// Get a reference to the global database connection.
///
/// Panics if the global connection has not been initialized.
pub fn db() -> &'static Database {
    let db = global_db_handle();
    if db.is_connected() {
        db
    } else {
        panic_missing_global_db(
            "Global database connection not initialized. \
             Call Database::init() or Database::set_global() before using models. \
             Use try_db() for a non-panicking alternative.",
        )
    }
}

/// Get the global database handle, returning an error if not initialized.
pub fn require_db() -> Result<Database> {
    global_connection_slot()
        .load_full()
        .map(|inner| Database::from_handle(DatabaseHandle::Connection(inner)))
        .ok_or_else(|| {
            Error::connection(
                "Global database connection not initialized. \
                 Call Database::init() or Database::set_global() before using models."
                    .to_string(),
            )
        })
}

/// Try to get the global database handle.
pub fn try_db() -> Option<Database> {
    global_connection_slot()
        .load_full()
        .map(|inner| Database::from_handle(DatabaseHandle::Connection(inner)))
}

/// Check whether a global database connection has been initialized.
pub fn has_global_db() -> bool {
    global_connection_slot().load_full().is_some()
}

#[doc(hidden)]
pub fn __current_db() -> Result<Database> {
    if let Some(handle) = THREAD_DB_OVERRIDE.with(|slot| slot.borrow().clone()) {
        return Ok(Database::from_handle(handle));
    }

    require_db()
}

pub(super) fn current_scope_handle() -> Result<DatabaseHandle> {
    if let Some(handle) = THREAD_DB_OVERRIDE.with(|slot| slot.borrow().clone()) {
        return Ok(handle);
    }

    global_connection_slot()
        .load_full()
        .map(DatabaseHandle::Connection)
        .ok_or_else(|| {
            Error::connection(
                "Global database connection not initialized. \
                 Call Database::init() or Database::set_global() before using models."
                    .to_string(),
            )
        })
}

struct ThreadOverrideGuard {
    previous: Option<DatabaseHandle>,
}

impl ThreadOverrideGuard {
    fn install(handle: DatabaseHandle) -> Self {
        Self {
            previous: Database::replace_thread_override(Some(handle)),
        }
    }
}

impl Drop for ThreadOverrideGuard {
    fn drop(&mut self) {
        Database::set_thread_override(self.previous.take());
    }
}

pub(crate) fn poll_with_thread_override<F>(
    future: std::pin::Pin<&mut F>,
    cx: &mut std::task::Context<'_>,
    handle: &DatabaseHandle,
) -> std::task::Poll<F::Output>
where
    F: Future + ?Sized,
{
    let _guard = ThreadOverrideGuard::install(handle.clone());
    future.poll(cx)
}

#[doc(hidden)]
pub fn __current_connection() -> Result<ConnectionRef> {
    Ok(match current_scope_handle()? {
        DatabaseHandle::Connection(inner) => ConnectionRef::Database(inner),
        DatabaseHandle::Transaction(tx) => ConnectionRef::Transaction(tx),
    })
}

#[doc(hidden)]
pub fn __current_backend() -> Result<DbBackend> {
    use crate::internal::ConnectionTrait;

    Ok(match current_scope_handle()? {
        DatabaseHandle::Connection(inner) => inner.connection().get_database_backend(),
        DatabaseHandle::Transaction(tx) => tx.as_ref().get_database_backend(),
    })
}
