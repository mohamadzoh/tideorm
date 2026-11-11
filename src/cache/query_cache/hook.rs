//! Telling the application what the cache dropped, so it can drop the same
//! reads in its other processes.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// What one invalidation dropped, as the hook set with
/// [`QueryCache::set_invalidation_hook`](super::QueryCache::set_invalidation_hook)
/// sees it and [`QueryCache::apply_invalidation`](super::QueryCache::apply_invalidation)
/// takes it back. Serializes as `{"Database": ".."}`, `{"Table": ".."}` or `"All"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CacheInvalidation {
    /// Every read of one database, which a write there drops: foreign-key
    /// actions and triggers may change any of its tables. The name is built
    /// from the connection's host, port and database, so processes connected
    /// the same way name it alike.
    Database(String),
    /// Every read of one table (lowercase).
    Table(String),
    /// Every entry.
    All,
}

type Hook = Arc<dyn Fn(&CacheInvalidation) + Send + Sync>;

/// The installed hook, if any.
#[derive(Default)]
pub(super) struct InvalidationHook(RwLock<Option<Hook>>);

impl InvalidationHook {
    pub(super) fn set(&self, hook: Hook) {
        *self.0.write() = Some(hook);
    }

    pub(super) fn clear(&self) {
        *self.0.write() = None;
    }

    /// Call the hook with what `event` builds, built only when one is set.
    pub(super) fn notify(&self, event: impl FnOnce() -> CacheInvalidation) {
        // Called with no lock held, so the hook may use the cache.
        let hook = self.0.read().clone();
        if let Some(hook) = hook {
            hook(&event());
        }
    }
}

impl std::fmt::Debug for InvalidationHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.read().is_some() {
            "Some(..)"
        } else {
            "None"
        })
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/cache_invalidation_hook_tests.rs"]
mod tests;
