use parking_lot::{Mutex, RwLock};
use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Weak;
use std::sync::{Arc, OnceLock};

use crate::cache::{PendingInvalidations, install_pending_invalidations};
use crate::error::{Error, Result};
use crate::internal::InternalConnection;

use super::{ConnectionRef, Database};

static GLOBAL_DB: OnceLock<Database> = OnceLock::new();
static GLOBAL_CONNECTION: RwLock<Option<Arc<InternalConnection>>> = RwLock::new(None);

thread_local! {
    /// The scope's connection, and the identity of the pool it belongs to.
    static DB_OVERRIDE: RefCell<Option<(ConnectionRef, Option<u64>)>> = const { RefCell::new(None) };
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

struct ResetDbOverride(Option<(ConnectionRef, Option<u64>)>);

impl Drop for ResetDbOverride {
    fn drop(&mut self) {
        DB_OVERRIDE.with(|slot| {
            slot.replace(self.0.take());
        });
    }
}

fn install_db_override(connection: &ConnectionRef, origin: Option<u64>) -> ResetDbOverride {
    let previous = DB_OVERRIDE.with(|slot| slot.replace(Some((connection.clone(), origin))));
    ResetDbOverride(previous)
}

/// The identity of the pool `connection` belongs to: its own for a pooled
/// connection, and for a transaction the one of the scope it was opened in.
pub(crate) fn origin_of(connection: &ConnectionRef) -> Option<u64> {
    match connection {
        ConnectionRef::Database(inner) => Some(connection_identity(inner)),
        ConnectionRef::Transaction(_) => __scope_origin(),
    }
}

/// The identity of the pool the current scope's statements run on: the
/// enclosing transaction's, if any, otherwise the global connection's.
#[doc(hidden)]
pub fn __scope_origin() -> Option<u64> {
    match DB_OVERRIDE.with(|slot| slot.borrow().as_ref().map(|(_, origin)| *origin)) {
        Some(origin) => origin,
        None => GLOBAL_CONNECTION.read().as_ref().map(connection_identity),
    }
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
    match DB_OVERRIDE.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|(connection, _)| connection.clone())
    }) {
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
pub(crate) fn with_connection_override<F>(
    connection: ConnectionRef,
    origin: Option<u64>,
    pending: Option<Arc<Mutex<PendingInvalidations>>>,
    future: F,
) -> impl Future<Output = F::Output>
where
    F: Future,
{
    crate::internal::per_poll(future, move || {
        let guard = install_db_override(&connection, origin);
        let pending = pending.as_ref().map(install_pending_invalidations);
        // A tuple drops its fields in order: the invalidations first.
        (pending, guard)
    })
}

/// The process-wide identities handed out by [`connection_identity`].
static CONNECTION_IDENTITIES: OnceLock<Mutex<ConnectionIdentities>> = OnceLock::new();

/// The identity assigned to every pooled connection a cache key has named.
#[derive(Default)]
struct ConnectionIdentities {
    /// The last identity handed out. Identities are never reused within a
    /// process, so a stale cache entry can never be mistaken for a live one.
    last_id: u64,
    /// Address of the connection's `Arc` allocation to its identity, alongside a
    /// `Weak` to that allocation.
    assigned: HashMap<usize, (Weak<InternalConnection>, u64)>,
}

impl ConnectionIdentities {
    /// The identity of `connection`, assigning one if it has none yet.
    ///
    /// The map is keyed by address, which is only trustworthy because of the
    /// `Weak` stored beside it: a `Weak` keeps its `Arc` allocation reserved even
    /// after the connection itself is dropped, so no second connection can ever
    /// be allocated at the address of an entry that is still on record. Dead
    /// entries are dropped before a new identity is handed out — that releases
    /// the address *and* the mapping together, so a connection that later lands
    /// there misses this map and is issued a fresh identity rather than
    /// inheriting the dropped connection's cached rows.
    fn identify(&mut self, connection: &Arc<InternalConnection>) -> u64 {
        let address = Arc::as_ptr(connection) as usize;
        if let Some((_, id)) = self.assigned.get(&address) {
            return *id;
        }

        self.assigned
            .retain(|_, (tracked, _)| tracked.strong_count() > 0);
        self.last_id += 1;
        self.assigned
            .insert(address, (Arc::downgrade(connection), self.last_id));
        self.last_id
    }
}

/// A process-unique identity for a pooled connection.
///
/// Stable for as long as the connection lives and never handed to another
/// connection afterwards, which is what a cache key or a dirty-tracking
/// baseline needs: the address the connection happens to occupy is neither.
pub(crate) fn connection_identity(connection: &Arc<InternalConnection>) -> u64 {
    CONNECTION_IDENTITIES
        .get_or_init(|| Mutex::new(ConnectionIdentities::default()))
        .lock()
        .identify(connection)
}
