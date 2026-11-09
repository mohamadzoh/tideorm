//! Query-result caching for TideORM
//!
//! Start here when a query is correct but slower than expected, or when you
//! need to understand why cached reads are not being reused.
//!
//! Common causes of cache misses are:
//! - a different generated SQL shape than expected
//! - a different explicit cache key
//! - writes invalidating model-scoped cache entries
//! - TTL expiry
//!
//! Practical split:
//! - enable `QueryCache` when repeated reads should return the same payload for a while
//! - use explicit cache keys only when the generated SQL shape is not enough to describe reuse
//!
//! ## Cache Strategies
//!
//! Every entry expires after its TTL. When the cache is full, its strategy picks
//! the entry that makes room:
//!
//! - **LRU (Least Recently Used)** - the entry read least recently
//! - **FIFO (First In, First Out)** - the entry stored first
//! - **TTL (Time To Live)** - the entry closest to expiring
//!
//! ## The global cache and late configuration
//!
//! The cache exposes a process-wide instance via `global()`, created disabled
//! with default settings the first time anything asks for it. Model writes
//! reach for [`QueryCache::global`] on their own, so that default is frequently
//! installed before application startup code runs.
//!
//! [`QueryCache::init_global`] therefore *reconfigures* an already-installed
//! cache rather than failing to replace it: it falls back to `apply_config`, so
//! configuration passed after the first `global()` call still takes effect.
//! Entries cached up to that point are kept.
//!
//! ## Thread Safety
//!
//! The cache types are shared and synchronized internally, so they can be used
//! from multiple async tasks in the same process.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::OnceLock;

mod builders;
mod query_cache;
mod toggle;

pub use builders::{CacheKeyBuilder, CacheOptions};
#[cfg(any(feature = "entity-manager", feature = "dirty-tracking"))]
pub(crate) use query_cache::undo_on_rollback;
pub use query_cache::{CacheConfig, CacheStats, CacheStrategy, QueryCache};
pub(crate) use query_cache::{
    PendingInvalidations, Record, install_pending_invalidations, replace_pending_invalidations,
};

static GLOBAL_QUERY_CACHE: OnceLock<QueryCache> = OnceLock::new();

/// The 64-bit digest cache keys are bucketed by.
fn hash_text(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
#[path = "../../tests/unit/cache_tests.rs"]
mod tests;
