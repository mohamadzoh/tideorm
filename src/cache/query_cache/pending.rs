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

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::Mutex;

use super::QueryCache;

/// What the writes inside one open transaction invalidated, and what to undo
/// if it never commits.
#[derive(Default)]
pub(crate) struct PendingInvalidations {
    /// Only a savepoint can hand rollback work to its enclosing transaction.
    nested: bool,
    tables: HashSet<String>,
    all: bool,
    undo: Vec<Box<dyn FnOnce() + Send>>,
    /// Whether a statement failed inside the transaction, which on PostgreSQL
    /// aborts it.
    statement_failed: bool,
    /// Whether a statement deadlocked, which rolls the whole transaction back
    /// on the server: MySQL and MariaDB then run what follows outside it.
    deadlocked: bool,
    sqlite_aborted: bool,
    depth: usize,
}

impl Drop for PendingInvalidations {
    /// A record dropped before [`replay`](Self::replay) belongs to a
    /// transaction that did not commit. A deadlock ended every enclosing
    /// transaction too, so they learn of it.
    fn drop(&mut self) {
        while let Some(undo) = self.undo.pop() {
            undo();
        }
        if self.sqlite_aborted && self.nested {
            record(|enclosing| enclosing.sqlite_aborted = true);
        }
        if self.deadlocked && self.nested {
            record(|enclosing| enclosing.deadlocked = true);
        }
    }
}

thread_local! {
    /// The innermost transaction open in the current poll, installed around
    /// each poll by the transaction scope like its connection is.
    static CURRENT: RefCell<Option<Arc<Mutex<PendingInvalidations>>>> =
        const { RefCell::new(None) };
}

impl PendingInvalidations {
    pub(crate) fn new(nested: bool) -> Self {
        Self {
            nested,
            tables: HashSet::new(),
            all: false,
            undo: Vec::new(),
            statement_failed: false,
            deadlocked: false,
            sqlite_aborted: false,
            depth: if nested { transaction_depth() + 1 } else { 1 },
        }
    }
    pub(super) fn table(&mut self, table: &str) {
        self.tables.insert(table.to_string());
    }

    pub(super) fn all(&mut self) {
        self.all = true;
    }

    /// Whether a statement failed inside the transaction.
    pub(crate) fn statement_failed(&self) -> bool {
        self.statement_failed
    }

    /// Whether a statement inside the transaction, or one it enclosed,
    /// deadlocked.
    pub(crate) fn deadlocked(&self) -> bool {
        self.deadlocked
    }

    pub(crate) fn sqlite_aborted(&self) -> bool {
        self.sqlite_aborted
    }

    /// Invalidate again what the transaction's writes invalidated, now that it
    /// has committed. Inside an enclosing transaction this records it there
    /// as well, to be replayed when that one commits. Only a savepoint hands
    /// it undo steps: an independent transaction's commit is already final.
    pub(crate) fn replay(mut self) {
        let undo = std::mem::take(&mut self.undo);
        if self.nested && !undo.is_empty() {
            record(|enclosing| enclosing.undo.extend(undo));
        }

        let cache = QueryCache::global();
        if self.all {
            cache.clear();
        } else {
            for table in &self.tables {
                cache.invalidate_model(table);
            }
        }
    }
}

/// Restores the enclosing transaction's record when dropped.
pub(crate) struct PendingGuard(Option<Arc<Mutex<PendingInvalidations>>>);

impl Drop for PendingGuard {
    fn drop(&mut self) {
        CURRENT.with(|slot| {
            slot.replace(self.0.take());
        });
    }
}

/// Record the current poll's invalidations in `pending`.
pub(crate) fn install(pending: &Arc<Mutex<PendingInvalidations>>) -> PendingGuard {
    PendingGuard(CURRENT.with(|slot| slot.replace(Some(pending.clone()))))
}

/// Run `undo` if the innermost open transaction does not commit. Outside a
/// transaction nothing can roll back, and `undo` is dropped unrun.
#[cfg(any(feature = "entity-manager", feature = "dirty-tracking"))]
pub(crate) fn undo_on_rollback(undo: impl FnOnce() + Send + 'static) {
    record(|pending| pending.undo.push(Box::new(undo)));
}

/// Note that a statement failed inside the innermost open transaction, if
/// any, and whether it deadlocked.
pub(crate) fn note_failed_statement(deadlocked: bool) {
    record(|pending| {
        pending.statement_failed = true;
        pending.deadlocked |= deadlocked;
    });
}

/// Note an invalidation against the innermost open transaction, if any.
pub(super) fn record(note: impl FnOnce(&mut PendingInvalidations)) {
    CURRENT.with(|slot| {
        if let Some(pending) = slot.borrow().as_ref() {
            note(&mut pending.lock());
        }
    });
}

/// A server-side deadlock may have ended the transaction already. Never let
/// further statements on its executor escape into autocommit mode.
pub(crate) fn transaction_deadlocked() -> bool {
    CURRENT.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|pending| pending.lock().deadlocked)
    })
}

/// SQLite can abort the whole transaction on a constraint or trigger failure.
pub(crate) fn note_sqlite_aborted(aborted: bool) {
    record(|pending| pending.sqlite_aborted = aborted);
}

pub(crate) fn transaction_sqlite_aborted() -> bool {
    CURRENT.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|pending| pending.lock().sqlite_aborted)
    })
}

pub(crate) fn transaction_depth() -> usize {
    CURRENT.with(|slot| {
        slot.borrow()
            .as_ref()
            .map_or(0, |pending| pending.lock().depth)
    })
}
