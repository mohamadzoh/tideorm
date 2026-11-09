//! Cache invalidations an open transaction replays when it commits, and the
//! in-memory state it restores if it does not.
//!
//! A write invalidates the cached reads of its table at once, but until the
//! transaction commits, every other connection still reads the rows it
//! replaced. A cached read made meanwhile would serve them for its whole TTL,
//! so each transaction records what its writes invalidated and invalidates it
//! again once it has committed.
//!
//! State kept outside the database, such as an entity manager's ids and clean
//! snapshots, is only true once the transaction that wrote it commits. Such
//! state registers how to undo itself, which runs when the transaction rolls
//! back, fails to commit, or is dropped unfinished.
//!
//! A transaction's record travels with its connection, so what a statement on
//! an outer transaction, run from inside another database's, records goes to
//! the transaction it ran on.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::Mutex;

use super::QueryCache;

/// What the writes inside one open transaction invalidated, and what to undo
/// if it never commits. Public only as the record a transaction's
/// [`ConnectionRef`](crate::database::ConnectionRef) carries; nothing outside
/// the crate can name it.
#[doc(hidden)]
#[derive(Default)]
pub struct PendingInvalidations {
    /// The record of the transaction a savepoint is open in, which it hands
    /// its rollback work to.
    enclosing: Option<Record>,
    tables: HashSet<String>,
    databases: HashSet<Arc<str>>,
    all: bool,
    undo: Vec<Box<dyn FnOnce() + Send>>,
    /// The seed lock survives savepoint completion until the enclosing
    /// transaction commits or rolls back. Its mutex also serializes seed
    /// operations sharing that transaction. The boxed guard releases on drop.
    seed_lock: SeedLock,
}

type SeedLock = Arc<futures_util::lock::Mutex<Option<Box<dyn Send>>>>;

/// A transaction's record, shared by the transaction, its connection and the
/// savepoints opened in it.
pub(crate) type Record = Arc<Mutex<PendingInvalidations>>;

impl Drop for PendingInvalidations {
    /// A record dropped before [`replay`](Self::replay) belongs to a
    /// transaction that did not commit.
    fn drop(&mut self) {
        while let Some(undo) = self.undo.pop() {
            undo();
        }
    }
}

thread_local! {
    /// The innermost transaction open in the current poll, installed around
    /// each poll by the transaction scope like its connection is.
    static CURRENT: RefCell<Option<Record>> =
        const { RefCell::new(None) };
}

impl PendingInvalidations {
    /// The record of a transaction begun on its own, or, as a savepoint, in
    /// the transaction `enclosing` records.
    pub(crate) fn new(enclosing: Option<Record>) -> Self {
        let seed_lock = match &enclosing {
            Some(enclosing) => enclosing.lock().seed_lock.clone(),
            None => SeedLock::default(),
        };
        Self {
            enclosing,
            tables: HashSet::new(),
            databases: HashSet::new(),
            all: false,
            undo: Vec::new(),
            seed_lock,
        }
    }

    pub(crate) fn seed_lock(&self) -> SeedLock {
        self.seed_lock.clone()
    }

    pub(super) fn table(&mut self, table: &str) {
        self.tables.insert(table.to_string());
    }

    pub(super) fn all(&mut self) {
        self.all = true;
    }

    pub(super) fn database(&mut self, database: Arc<str>) {
        self.databases.insert(database);
    }

    /// Invalidate again what the transaction's writes invalidated, now that it
    /// has committed. Inside an enclosing transaction this records it there
    /// as well, to be replayed when that one commits. Only a savepoint hands
    /// it undo steps: an independent transaction's commit is already final.
    pub(crate) fn replay(mut self) {
        let undo = std::mem::take(&mut self.undo);
        if let Some(enclosing) = &self.enclosing {
            enclosing.lock().undo.extend(undo);
        }
        self.replay_invalidations();
    }

    /// Invalidate again what the transaction's writes invalidated, recording
    /// it in the enclosing transaction, or in none: the transaction around
    /// this scope may be another database's.
    pub(crate) fn replay_invalidations(&self) {
        let _scope = replace(self.enclosing.clone());

        let cache = QueryCache::global();
        if self.all {
            cache.clear();
        } else {
            for database in &self.databases {
                cache.invalidate_database(database.clone());
            }
            for table in &self.tables {
                cache.invalidate_model(table);
            }
        }
    }
}

/// Restores the enclosing transaction's record when dropped.
pub(crate) struct PendingGuard(Option<Record>);

impl Drop for PendingGuard {
    fn drop(&mut self) {
        CURRENT.with(|slot| {
            slot.replace(self.0.take());
        });
    }
}

/// Record the current poll's invalidations in `pending`.
pub(crate) fn install(pending: &Record) -> PendingGuard {
    replace(Some(pending.clone()))
}

/// Record the current poll's invalidations in `record`, or, with `None`, in
/// no transaction: statements on a pool run outside every transaction.
pub(crate) fn replace(record: Option<Record>) -> PendingGuard {
    PendingGuard(CURRENT.with(|slot| slot.replace(record)))
}

/// Run `undo` if the innermost open transaction does not commit. Outside a
/// transaction nothing can roll back, and `undo` is dropped unrun.
#[cfg(any(feature = "entity-manager", feature = "dirty-tracking"))]
pub(crate) fn undo_on_rollback(undo: impl FnOnce() + Send + 'static) {
    record(|pending| pending.undo.push(Box::new(undo)));
}

/// Note an invalidation against the innermost open transaction, if any.
pub(super) fn record(note: impl FnOnce(&mut PendingInvalidations)) {
    CURRENT.with(|slot| {
        if let Some(pending) = slot.borrow().as_ref() {
            note(&mut pending.lock());
        }
    });
}
