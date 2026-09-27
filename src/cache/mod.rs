//! Query caching and statement statistics for TideORM
//!
//! This module contains two separate caches:
//! - query-result caching for repeated reads
//! - statement statistics for repeated SQL shapes
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
//! - inspect `PreparedStatementCache` stats to see which SQL shapes repeat and what they cost
//!
//! ## Cache Strategies
//!
//! The query cache can evict entries using different strategies:
//!
//! - **TTL (Time To Live)** - Entries expire after a fixed duration
//! - **LRU (Least Recently Used)** - Oldest entries are evicted when cache is full
//!
//! ## Global caches and late configuration
//!
//! Both caches expose a process-wide instance via `global()`, and both are
//! created disabled with default settings the first time anything asks for one.
//! Model writes reach for [`QueryCache::global`] on their own, so that default
//! is frequently installed before application startup code runs.
//!
//! `init_global` therefore *reconfigures* an already-installed cache rather than
//! failing to replace it: [`QueryCache::init_global`] and
//! [`PreparedStatementCache::init_global`] both fall back to `apply_config`, so
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
mod prepared_statements;
mod query_cache;

pub use builders::{CacheKeyBuilder, CacheOptions};
pub use prepared_statements::{
    CachedStatementInfo, PreparedStatementCache, PreparedStatementConfig, PreparedStatementStats,
};
#[cfg(feature = "entity-manager")]
pub(crate) use query_cache::undo_on_rollback;
pub use query_cache::{CacheConfig, CacheStats, CacheStrategy, QueryCache};
pub(crate) use query_cache::{
    PendingInvalidations, install_pending_invalidations, note_failed_statement,
};

static GLOBAL_QUERY_CACHE: OnceLock<QueryCache> = OnceLock::new();
static GLOBAL_STMT_CACHE: OnceLock<PreparedStatementCache> = OnceLock::new();

/// The 64-bit digest both cache keys and statement slots are bucketed by.
fn hash_text(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
#[path = "../../tests/unit/cache_tests.rs"]
mod tests;
