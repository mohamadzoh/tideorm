use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use crate::error::{Error, Result};

mod config;
mod control;
mod store;

pub use config::{CacheConfig, CacheStrategy};
pub use store::CacheStats;
use store::{CacheEntry, CacheStore};

// =============================================================================
// QUERY CACHE
// =============================================================================

/// Query result cache
///
/// Caches the results of database queries to avoid repeated round-trips
/// for frequently accessed data.
#[derive(Debug)]
pub struct QueryCache {
    /// Cache configuration
    config: RwLock<CacheConfig>,
    /// Fast path for checking whether caching is enabled.
    enabled: AtomicBool,
    /// The actual cache storage
    cache: RwLock<CacheStore>,
    /// Monotonic sequence for eviction indexes.
    order_counter: AtomicU64,
    /// Cache hit counter.
    hits: AtomicU64,
    /// Cache miss counter.
    misses: AtomicU64,
    /// Current number of entries.
    entries: AtomicUsize,
    /// Approximate serialized size of cached data.
    size_bytes: AtomicUsize,
    /// Upper bound on `size_bytes`, or `0` for no byte budget.
    max_size_bytes: AtomicUsize,
    /// Cache eviction counter.
    evictions: AtomicU64,
    /// Cache invalidation counter.
    invalidations: AtomicU64,
}

impl QueryCache {
    /// Create a new query cache with default configuration
    pub fn new() -> Self {
        Self {
            config: RwLock::new(CacheConfig::default()),
            enabled: AtomicBool::new(false),
            cache: RwLock::new(CacheStore::default()),
            order_counter: AtomicU64::new(1),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            entries: AtomicUsize::new(0),
            size_bytes: AtomicUsize::new(0),
            max_size_bytes: AtomicUsize::new(0),
            evictions: AtomicU64::new(0),
            invalidations: AtomicU64::new(0),
        }
    }

    /// Create a new query cache with custom configuration
    pub fn with_config(config: CacheConfig) -> Self {
        let enabled = config.enabled;
        Self {
            config: RwLock::new(config),
            enabled: AtomicBool::new(enabled),
            cache: RwLock::new(CacheStore::default()),
            order_counter: AtomicU64::new(1),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            entries: AtomicUsize::new(0),
            size_bytes: AtomicUsize::new(0),
            max_size_bytes: AtomicUsize::new(0),
            evictions: AtomicU64::new(0),
            invalidations: AtomicU64::new(0),
        }
    }

    // =========================================================================
    // CACHE OPERATIONS
    // =========================================================================

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

            match cache.entries.get(key) {
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

        match cache.entries.get(key) {
            Some(entry) if entry.is_expired() => {
                if let Some(expired_entry) = cache.entries.remove(key) {
                    cache.maybe_rebuild_indexes();
                    self.record_entries_len(cache.entries.len());
                    self.subtract_size_bytes(expired_entry.size_bytes);
                }
                self.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
            Some(_) if strategy == CacheStrategy::LRU => {
                let access_order = self.next_order();
                if let Some(entry) = cache.entries.get_mut(key) {
                    entry.touch(access_order);
                    let candidate = entry.lru_candidate(key);
                    let value = serde_json::from_slice(&entry.data).ok();
                    cache.lru_heap.push(Reverse(candidate));
                    cache.maybe_rebuild_indexes();
                    self.hits.fetch_add(1, Ordering::Relaxed);
                    value
                } else {
                    self.misses.fetch_add(1, Ordering::Relaxed);
                    None
                }
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
        self.store(key, value, ttl, HashSet::from([model_name.to_string()]))
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
        self.store(key, value, ttl, tables.iter().cloned().collect())
    }

    /// Store one entry tagged with every table it reads.
    fn store<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
        tables: HashSet<String>,
    ) -> Result<()> {
        if !self.is_enabled() {
            return Ok(());
        }

        let config = self.config.read();

        let ttl = ttl.unwrap_or(config.default_ttl);
        let max_entries = config.max_entries;
        drop(config);

        let data = serde_json::to_vec(value)
            .map_err(|e| Error::internal(format!("Failed to serialize cache value: {}", e)))?;

        // Check if we should cache empty results
        if data == b"[]" {
            let should_cache = self.config.read().cache_empty_results;
            if !should_cache {
                return Ok(());
            }
        }

        let entry_size = data.len();
        if max_entries == 0 {
            return Ok(());
        }

        let max_size_bytes = self.max_size_bytes.load(Ordering::Relaxed);
        if max_size_bytes > 0 && entry_size > max_size_bytes {
            // A payload larger than the whole budget can never fit; caching it
            // would evict every other entry and still overflow.
            return Ok(());
        }

        let entry = CacheEntry::new(data, entry_size, ttl, tables, self.next_order());

        let mut cache = self.cache.write();
        let replacing_existing = cache.entries.contains_key(key);

        // Evict if necessary
        while !replacing_existing && cache.entries.len() >= max_entries {
            if !self.evict_one(&mut cache) {
                break;
            }
        }

        // Then make room for the new payload inside the byte budget. `entry_size`
        // is known to fit on its own, so this terminates once the store is empty.
        while max_size_bytes > 0
            && self.size_bytes.load(Ordering::Relaxed) + entry_size > max_size_bytes
        {
            if !self.evict_one(&mut cache) {
                break;
            }
        }

        let replaced_entry = cache.insert(key.to_string(), entry);
        cache.maybe_rebuild_indexes();
        self.record_entries_len(cache.entries.len());

        match replaced_entry {
            Some(previous) if previous.size_bytes >= entry_size => {
                self.subtract_size_bytes(previous.size_bytes - entry_size);
            }
            Some(previous) => {
                self.add_size_bytes(entry_size - previous.size_bytes);
            }
            None => {
                self.add_size_bytes(entry_size);
            }
        }

        Ok(())
    }

    /// Remove a specific cache entry
    pub fn invalidate(&self, key: &str) -> bool {
        let mut cache = self.cache.write();
        if let Some(removed) = cache.entries.remove(key) {
            cache.maybe_rebuild_indexes();
            self.invalidations.fetch_add(1, Ordering::Relaxed);
            self.record_entries_len(cache.entries.len());
            self.subtract_size_bytes(removed.size_bytes);
            true
        } else {
            false
        }
    }

    /// Invalidate all cache entries reading a specific model/table
    ///
    /// An entry is removed when the table is anywhere in its tag set, so a
    /// cached join, union, or CTE result is dropped by a write to any of its
    /// source tables and not only to the primary one.
    pub fn invalidate_model(&self, model_name: &str) {
        let mut cache = self.cache.write();
        let keys_to_remove: Vec<String> = cache
            .entries
            .iter()
            .filter(|(_, entry)| entry.reads_table(model_name))
            .map(|(key, _)| key.clone())
            .collect();

        let count = keys_to_remove.len();
        let mut removed_size = 0;
        for key in keys_to_remove {
            if let Some(entry) = cache.entries.remove(&key) {
                removed_size += entry.size_bytes;
            }
        }

        if count > 0 {
            cache.maybe_rebuild_indexes();
            self.invalidations
                .fetch_add(count as u64, Ordering::Relaxed);
            self.record_entries_len(cache.entries.len());
            self.subtract_size_bytes(removed_size);
        }
    }

    /// Clear the entire cache
    pub fn clear(&self) {
        let mut cache = self.cache.write();
        let count = cache.entries.len();
        let removed_size = cache
            .entries
            .values()
            .map(|entry| entry.size_bytes)
            .sum::<usize>();
        cache.clear();

        if count > 0 {
            self.invalidations
                .fetch_add(count as u64, Ordering::Relaxed);
            self.record_entries_len(0);
            self.subtract_size_bytes(removed_size);
        }
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        self.snapshot_stats()
    }

    /// Reset cache statistics
    pub fn reset_stats(&self) {
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
        self.evictions.store(0, Ordering::Relaxed);
        self.invalidations.store(0, Ordering::Relaxed);

        let cache = self.cache.read();
        self.record_entries_len(cache.entries.len());
        self.overwrite_size_bytes(cache.entries.values().map(|entry| entry.size_bytes).sum());
    }

    /// Evict expired entries
    pub fn evict_expired(&self) {
        let mut cache = self.cache.write();
        let keys_to_remove: Vec<String> = cache
            .entries
            .iter()
            .filter(|(_, entry)| entry.is_expired())
            .map(|(key, _)| key.clone())
            .collect();

        let count = keys_to_remove.len();
        let mut removed_size = 0;
        for key in keys_to_remove {
            if let Some(entry) = cache.entries.remove(&key) {
                removed_size += entry.size_bytes;
            }
        }

        if count > 0 {
            cache.maybe_rebuild_indexes();
            self.evictions.fetch_add(count as u64, Ordering::Relaxed);
            self.record_entries_len(cache.entries.len());
            self.subtract_size_bytes(removed_size);
        }
    }

    /// Evict one entry based on the configured strategy
    fn evict_one(&self, cache: &mut CacheStore) -> bool {
        let strategy = self.config.read().strategy;

        let removed = match strategy {
            CacheStrategy::LRU => loop {
                match cache.lru_heap.pop() {
                    Some(Reverse(candidate)) => {
                        let should_remove = cache
                            .entries
                            .get(&candidate.key)
                            .map(|entry| entry.access_order == candidate.order)
                            .unwrap_or(false);

                        if should_remove {
                            break cache.entries.remove(&candidate.key);
                        }
                    }
                    None => break None,
                }
            },
            CacheStrategy::FIFO => loop {
                match cache.fifo_heap.pop() {
                    Some(Reverse(candidate)) => {
                        let should_remove = cache
                            .entries
                            .get(&candidate.key)
                            .map(|entry| entry.insert_order == candidate.order)
                            .unwrap_or(false);

                        if should_remove {
                            break cache.entries.remove(&candidate.key);
                        }
                    }
                    None => break None,
                }
            },
            CacheStrategy::TTL => loop {
                match cache.ttl_heap.pop() {
                    Some(Reverse(candidate)) => {
                        let should_remove = cache
                            .entries
                            .get(&candidate.key)
                            .map(|entry| {
                                entry.insert_order == candidate.order
                                    && entry.expires_at == candidate.expires_at
                            })
                            .unwrap_or(false);

                        if should_remove {
                            break cache.entries.remove(&candidate.key);
                        }
                    }
                    None => break None,
                }
            },
        };

        if let Some(entry) = removed {
            cache.maybe_rebuild_indexes();
            self.evictions.fetch_add(1, Ordering::Relaxed);
            self.record_entries_len(cache.entries.len());
            self.subtract_size_bytes(entry.size_bytes);
            return true;
        }

        false
    }

    /// Check if a key exists in the cache (without updating access time)
    pub fn contains(&self, key: &str) -> bool {
        let cache = self.cache.read();
        if let Some(entry) = cache.entries.get(key) {
            return !entry.is_expired();
        }
        false
    }

    /// Set an upper bound on the total serialized size of cached data
    ///
    /// Entries are evicted with the configured strategy until a new payload fits
    /// inside the budget, and a payload larger than the whole budget is never
    /// cached. Pass `0` (the default) for no byte budget, in which case only
    /// `max_entries` bounds the cache.
    pub fn set_max_size_bytes(&self, max: usize) -> &Self {
        self.max_size_bytes.store(max, Ordering::Relaxed);
        self
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
        self.entries.load(Ordering::Relaxed)
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
