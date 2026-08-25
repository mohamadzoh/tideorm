//! Query caching and prepared statement caching for TideORM
//!
//! This module contains two separate caches:
//! - query-result caching for repeated reads
//! - prepared-statement caching for repeated SQL shapes
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
//! - inspect `PreparedStatementCache` stats when repeated queries are still paying parse or planning cost
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

use std::sync::OnceLock;

mod builders;
mod prepared_statements;
mod query_cache;

pub use builders::{CacheKeyBuilder, CacheOptions};
pub use prepared_statements::{
    CachedStatementInfo, PreparedStatementCache, PreparedStatementConfig, PreparedStatementStats,
};
pub use query_cache::{CacheConfig, CacheStats, CacheStrategy, QueryCache};

// =============================================================================
// GLOBAL CACHE INSTANCES
// =============================================================================

static GLOBAL_QUERY_CACHE: OnceLock<QueryCache> = OnceLock::new();
static GLOBAL_STMT_CACHE: OnceLock<PreparedStatementCache> = OnceLock::new();

#[cfg(test)]
#[path = "../../tests/unit/cache_tests.rs"]
mod tests;
