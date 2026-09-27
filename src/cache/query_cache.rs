use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use crate::error::{Error, Result};

mod config;
mod control;
mod pending;
mod store;

pub use config::{CacheConfig, CacheStrategy};
#[cfg(feature = "entity-manager")]
pub(crate) use pending::undo_on_rollback;
pub(crate) use pending::{
    PendingInvalidations, install as install_pending_invalidations, note_failed_statement,
};
pub use store::CacheStats;
use store::{CacheEntry, CacheStore};

/// Query result cache
///
/// Caches the results of database queries to avoid repeated round-trips
/// for frequently accessed data.
#[derive(Debug)]
pub struct QueryCache {
    /// Cache configuration
    config: RwLock<CacheConfig>,
    /// `config.enabled`, readable on every query without taking the lock.
    enabled: AtomicBool,
    /// The actual cache storage
    cache: RwLock<CacheStore>,
    /// Monotonic sequence for eviction indexes.
    order_counter: AtomicU64,
    /// Cache hit counter.
    hits: AtomicU64,
    /// Cache miss counter.
    misses: AtomicU64,
    /// Cache eviction counter.
    evictions: AtomicU64,
    /// Cache invalidation counter.
    invalidations: AtomicU64,
}

impl QueryCache {
    /// Create a new query cache with default configuration
    pub fn new() -> Self {
        Self::with_config(CacheConfig::default())
    }

    /// Create a new query cache with custom configuration
    pub fn with_config(config: CacheConfig) -> Self {
        Self {
            enabled: AtomicBool::new(config.enabled),
            config: RwLock::new(config),
            cache: RwLock::new(CacheStore::default()),
            order_counter: AtomicU64::new(1),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
            invalidations: AtomicU64::new(0),
        }
    }

    /// Generate a cache key from a query
    pub fn generate_key(&self, table: &str, query_hash: u64) -> String {
        let prefix = self.config.read().key_prefix.clone().unwrap_or_default();

        if prefix.is_empty() {
            format!("{}:{}", table, query_hash)
        } else {
            format!("{}:{}:{}", prefix, table, query_hash)
        }
    }

    /// Get a cached value
    pub fn get<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Option<T> {
        if !self.is_enabled() {
            return None;
        }

        let strategy = self.config.read().strategy;

        // Fast path: read-only lookup for misses and non-LRU hits.
        {
            let cache = self.cache.read();

            match cache.get(key) {
                Some(entry) if !entry.is_expired() && strategy != CacheStrategy::LRU => {
                    self.hits.fetch_add(1, Ordering::Relaxed);
                    return serde_json::from_slice(&entry.data).ok();
                }
                Some(_) => {}
                None => {
                    self.misses.fetch_add(1, Ordering::Relaxed);
                    return None;
                }
            }
        }

        // Slow path: LRU hits need touch(), and expired entries need removal.
        let mut cache = self.cache.write();

        match cache.get(key) {
            Some(entry) if entry.is_expired() => {
                cache.remove(key);
                self.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
            Some(_) if strategy == CacheStrategy::LRU => {
                let access_order = self.next_order();
                self.hits.fetch_add(1, Ordering::Relaxed);
                cache
                    .touch(key, access_order)
                    .and_then(|entry| serde_json::from_slice(&entry.data).ok())
            }
            Some(entry) => {
                self.hits.fetch_add(1, Ordering::Relaxed);
                serde_json::from_slice(&entry.data).ok()
            }
            None => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    /// Set a cached value
    ///
    /// The entry is tagged with `model_name` alone; use
    /// [`QueryCache::set_tagged`] for a query that reads more than one table.
    pub fn set<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
        model_name: &str,
    ) -> Result<()> {
        self.store(
            key,
            value,
            ttl,
            HashSet::from([model_name.to_string()]),
            None,
        )
    }

    /// Set a cached value that is invalidated by writes to any of `tables`
    ///
    /// A query reading more than its own table — joins, unions, CTEs, subqueries
    /// — has to be evicted when any of those tables changes, not just when the
    /// primary model does. Passing every table it reads is what makes
    /// [`QueryCache::invalidate_model`] reach the entry.
    pub fn set_tagged<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
        tables: &[String],
    ) -> Result<()> {
        self.store(key, value, ttl, tables.iter().cloned().collect(), None)
    }

    /// Where a read that may fill the cache starts: pass it to
    /// [`fill_tagged`](Self::fill_tagged) with what the read returned.
    pub(crate) fn fill_point(&self) -> u64 {
        self.cache.read().invalidation_seq()
    }

    /// [`set_tagged`](Self::set_tagged) for rows read since `fill_point`,
    /// skipped when one of `tables` was invalidated in the meantime: the rows
    /// may predate that write, and would outlive it for the whole TTL.
    pub(crate) fn fill_tagged<T: Serialize>(
        &self,
        fill_point: u64,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
        tables: &[String],
    ) -> Result<()> {
        self.store(
            key,
            value,
            ttl,
            tables.iter().cloned().collect(),
            Some(fill_point),
        )
    }

    /// Store one entry tagged with every table it reads, unless it was read
    /// before an invalidation of one of them.
    fn store<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
        tables: HashSet<String>,
        fill_point: Option<u64>,
    ) -> Result<()> {
        if !self.is_enabled() {
            return Ok(());
        }

        let config = self.config.read().clone();
        let data = serde_json::to_vec(value)
            .map_err(|e| Error::internal(format!("Failed to serialize cache value: {}", e)))?;

        if data == b"[]" && !config.cache_empty_results {
            return Ok(());
        }

        let entry_size = data.len();
        if config.max_entries == 0 {
            return Ok(());
        }

        let max_size_bytes = config.max_size_bytes;
        if max_size_bytes > 0 && entry_size > max_size_bytes {
            // A payload larger than the whole budget can never fit; caching it
            // would evict every other entry and still overflow.
            return Ok(());
        }

        let ttl = ttl.unwrap_or(config.default_ttl);
        let mut cache = self.cache.write();
        if fill_point.is_some_and(|point| cache.invalidated_since(point, &tables)) {
            return Ok(());
        }
        let entry = CacheEntry::new(data, ttl, tables, self.next_order());
        let replacing_existing = cache.get(key).is_some();

        while !replacing_existing && cache.len() >= config.max_entries {
            if !self.evict_one(&mut cache, config.strategy) {
                break;
            }
        }

        // Make room inside the byte budget as well. `entry_size` is known to fit
        // on its own, so this terminates once the store is empty.
        while max_size_bytes > 0 && cache.size_bytes() + entry_size > max_size_bytes {
            if !self.evict_one(&mut cache, config.strategy) {
                break;
            }
        }

        cache.insert(key.to_string(), entry);

        Ok(())
    }

    /// Remove a specific cache entry
    pub fn invalidate(&self, key: &str) -> bool {
        let removed = self.cache.write().remove(key).is_some();
        if removed {
            self.invalidations.fetch_add(1, Ordering::Relaxed);
        }
        removed
    }

    /// Invalidate all cache entries reading a specific model/table
    ///
    /// An entry is removed when the table is anywhere in its tag set, so a
    /// cached join, union, or CTE result is dropped by a write to any of its
    /// source tables and not only to the primary one.
    pub fn invalidate_model(&self, model_name: &str) {
        let removed = {
            let mut cache = self.cache.write();
            cache.note_invalidated(model_name);
            cache.remove_where(|entry| entry.reads_table(model_name))
        };
        self.invalidations
            .fetch_add(removed as u64, Ordering::Relaxed);
        if self.is_global() {
            pending::record(|pending| pending.table(model_name));
        }
    }

    /// Clear the entire cache
    pub fn clear(&self) {
        if self.is_global() {
            pending::record(PendingInvalidations::all);
        }
        let removed = self.cache.write().clear();
        self.invalidations
            .fetch_add(removed as u64, Ordering::Relaxed);
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        let cache = self.cache.read();
        CacheStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            entries: cache.len(),
            size_bytes: cache.size_bytes(),
            evictions: self.evictions.load(Ordering::Relaxed),
            invalidations: self.invalidations.load(Ordering::Relaxed),
        }
    }

    /// Reset cache statistics
    ///
    /// Only the hit, miss, eviction and invalidation counters restart; the
    /// entry count and size describe the cache itself and are unaffected.
    pub fn reset_stats(&self) {
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
        self.evictions.store(0, Ordering::Relaxed);
        self.invalidations.store(0, Ordering::Relaxed);
    }

    /// Evict expired entries
    pub fn evict_expired(&self) {
        let removed = self.cache.write().remove_where(CacheEntry::is_expired);
        self.evictions.fetch_add(removed as u64, Ordering::Relaxed);
    }

    /// Evict one entry based on the configured strategy
    fn evict_one(&self, cache: &mut CacheStore, strategy: CacheStrategy) -> bool {
        let evicted = cache.evict(strategy).is_some();
        if evicted {
            self.evictions.fetch_add(1, Ordering::Relaxed);
        }
        evicted
    }

    /// Check if a key exists in the cache (without updating access time)
    pub fn contains(&self, key: &str) -> bool {
        self.cache
            .read()
            .get(key)
            .is_some_and(|entry| !entry.is_expired())
    }

    /// Replace this cache's configuration in place
    ///
    /// Cached entries are kept; only the configuration and the enabled flag
    /// change. This is what lets a late `QueryCache::init_global` still take
    /// effect after the global cache has already been created with defaults.
    pub fn apply_config(&self, config: CacheConfig) {
        let enabled = config.enabled;
        *self.config.write() = config;
        self.enabled.store(enabled, Ordering::Release);
    }

    /// Get the number of entries in the cache
    pub fn len(&self) -> usize {
        self.cache.read().len()
    }

    /// Check if the cache is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for QueryCache {
    fn default() -> Self {
        Self::new()
    }
}
