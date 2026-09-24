//! Cache invalidations an open transaction replays when it commits.
//!
//! A write invalidates the cached reads of its table at once, but until the
//! transaction commits, every other connection still reads the rows it
//! replaced. A cached read made meanwhile would serve them for its whole TTL,
//! so each transaction records what its writes invalidated and invalidates it
//! again once it has committed.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::Mutex;

use super::QueryCache;

/// What the writes inside one open transaction invalidated.
#[derive(Debug, Default)]
pub(crate) struct PendingInvalidations {
    tables: HashSet<String>,
    all: bool,
}

thread_local! {
    /// The innermost transaction open in the current poll, installed around
    /// each poll by the transaction scope like its connection is.
    static CURRENT: RefCell<Option<Arc<Mutex<PendingInvalidations>>>> =
        const { RefCell::new(None) };
}

impl PendingInvalidations {
    pub(super) fn table(&mut self, table: &str) {
        self.tables.insert(table.to_string());
    }

    pub(super) fn all(&mut self) {
        self.all = true;
    }

    /// Invalidate again what the transaction's writes invalidated, now that it
    /// has committed. Inside an enclosing transaction this records it there
    /// as well, to be replayed when that one commits.
    pub(crate) fn replay(self) {
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

/// Note an invalidation against the innermost open transaction, if any.
pub(super) fn record(note: impl FnOnce(&mut PendingInvalidations)) {
    CURRENT.with(|slot| {
        if let Some(pending) = slot.borrow().as_ref() {
            note(&mut pending.lock());
        }
    });
}
