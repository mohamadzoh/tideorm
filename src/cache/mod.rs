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
//! ## Thread Safety
//!
//! The cache types are shared and synchronized internally, so they can be used
//! from multiple async tasks in the same process.

use std::sync::OnceLock;

mod builders;
mod prepared_statements;
mod query_cache;

pub use builders::{CacheKeyBuilder, CacheOptions, CacheWarmer};
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
