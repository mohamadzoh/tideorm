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
pub use query_cache::{CacheConfig, CacheInvalidation, CacheStats, CacheStrategy, QueryCache};
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

/// A [`Hasher`] feeding a 256-bit BLAKE2s digest, which names a query's cache
/// entry. `DefaultHasher`'s 64 bits under a fixed key can be steered: a query
/// whose bound values were searched for a collision would share another
/// query's entry and be served its rows.
pub(crate) struct KeyDigest(blake2::Blake2s256);

impl KeyDigest {
    pub(crate) fn new() -> Self {
        Self(blake2::Digest::new())
    }

    /// The digest, as lowercase hex.
    pub(crate) fn hex(self) -> String {
        use std::fmt::Write;
        blake2::Digest::finalize(self.0)
            .iter()
            .fold(String::with_capacity(64), |mut hex, byte| {
                let _ = write!(hex, "{byte:02x}");
                hex
            })
    }
}

impl Hasher for KeyDigest {
    fn write(&mut self, bytes: &[u8]) {
        blake2::Digest::update(&mut self.0, bytes);
    }

    fn finish(&self) -> u64 {
        let digest = blake2::Digest::finalize(self.0.clone());
        u64::from_le_bytes(digest[..8].try_into().expect("a digest has 32 bytes"))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/cache_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "../../tests/unit/cache_key_digest_tests.rs"]
mod key_digest_tests;
