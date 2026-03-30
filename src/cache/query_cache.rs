use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crate::error::{Error, Result};

use super::GLOBAL_QUERY_CACHE;

// =============================================================================
// CACHE CONFIGURATION
// =============================================================================

/// Configuration for query result caching
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Whether caching is enabled
    pub enabled: bool,
    /// Maximum number of entries in the cache
    pub max_entries: usize,
    /// Default TTL for cache entries
    pub default_ttl: Duration,
    /// Cache eviction strategy
    pub strategy: CacheStrategy,
    /// Whether to cache empty results
    pub cache_empty_results: bool,
    /// Prefix applied to every cache key to avoid collisions across namespaces
    pub key_prefix: Option<String>,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_entries: 1000,
            default_ttl: Duration::from_secs(60),
            strategy: CacheStrategy::LRU,
            cache_empty_results: true,
            key_prefix: None,
        }
    }
}

/// Cache eviction strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheStrategy {
    /// Least Recently Used - evict oldest accessed entries
    LRU,
    /// First In First Out - evict oldest added entries
    FIFO,
    /// Time To Live - evict based on expiration time
    TTL,
}

impl std::fmt::Display for CacheStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheStrategy::LRU => write!(f, "LRU"),
            CacheStrategy::FIFO => write!(f, "FIFO"),
            CacheStrategy::TTL => write!(f, "TTL"),
        }
    }
}

// =============================================================================
// QUERY CACHE
// =============================================================================

/// A cache entry storing query results
#[derive(Debug, Clone)]
struct CacheEntry {
    /// Cached data as serialized JSON bytes.
    data: Vec<u8>,
    /// Serialized size in bytes.
    size_bytes: usize,
    /// Absolute expiration time for efficient TTL eviction.
    expires_at: Instant,
    /// The model/table this entry is for (for targeted invalidation)
    model_name: String,
    /// Number of times this entry has been accessed
    hit_count: u64,
    /// Monotonic insertion sequence used by FIFO/TTL eviction indexes.
    insert_order: u64,
    /// Monotonic access sequence used by the LRU eviction index.
    access_order: u64,
}

impl CacheEntry {
    fn new(data: Vec<u8>, size_bytes: usize, ttl: Duration, model_name: &str, order: u64) -> Self {
        let now = Instant::now();
        Self {
            data,
            size_bytes,
            expires_at: now.checked_add(ttl).unwrap_or(now),
            model_name: model_name.to_string(),
            hit_count: 0,
            insert_order: order,
            access_order: order,
        }
    }

    fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }

    fn touch(&mut self, access_order: u64) {
        self.access_order = access_order;
        self.hit_count += 1;
    }

    fn lru_candidate(&self, key: &str) -> OrderCandidate {
        OrderCandidate::new(self.access_order, key)
    }

    fn fifo_candidate(&self, key: &str) -> OrderCandidate {
        OrderCandidate::new(self.insert_order, key)
    }

    fn ttl_candidate(&self, key: &str) -> TtlCandidate {
        TtlCandidate::new(self.expires_at, self.insert_order, key)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct OrderCandidate {
    order: u64,
    key: String,
}

impl OrderCandidate {
    fn new(order: u64, key: &str) -> Self {
        Self {
            order,
            key: key.to_string(),
        }
    }
}

impl Ord for OrderCandidate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.order
            .cmp(&other.order)
            .then_with(|| self.key.cmp(&other.key))
    }
}

impl PartialOrd for OrderCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct TtlCandidate {
    expires_at: Instant,
    order: u64,
    key: String,
}

impl TtlCandidate {
    fn new(expires_at: Instant, order: u64, key: &str) -> Self {
        Self {
            expires_at,
            order,
            key: key.to_string(),
        }
    }
}

impl Ord for TtlCandidate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.expires_at
            .cmp(&other.expires_at)
            .then_with(|| self.order.cmp(&other.order))
            .then_with(|| self.key.cmp(&other.key))
    }
}

impl PartialOrd for TtlCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Default)]
struct CacheStore {
    entries: HashMap<String, CacheEntry>,
    lru_heap: BinaryHeap<Reverse<OrderCandidate>>,
    fifo_heap: BinaryHeap<Reverse<OrderCandidate>>,
    ttl_heap: BinaryHeap<Reverse<TtlCandidate>>,
}

impl CacheStore {
    fn insert(&mut self, key: String, entry: CacheEntry) -> Option<CacheEntry> {
        self.lru_heap.push(Reverse(entry.lru_candidate(&key)));
        self.fifo_heap.push(Reverse(entry.fifo_candidate(&key)));
        self.ttl_heap.push(Reverse(entry.ttl_candidate(&key)));
        self.entries.insert(key, entry)
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.lru_heap.clear();
        self.fifo_heap.clear();
        self.ttl_heap.clear();
    }

    fn maybe_rebuild_indexes(&mut self) {
        const INDEX_REBUILD_MULTIPLIER: usize = 4;
        const INDEX_REBUILD_SLACK: usize = 64;

        let entry_count = self.entries.len();
        let threshold = entry_count
            .saturating_mul(INDEX_REBUILD_MULTIPLIER)
            .saturating_add(INDEX_REBUILD_SLACK);

        if entry_count == 0
            || self.lru_heap.len() > threshold
            || self.fifo_heap.len() > threshold
            || self.ttl_heap.len() > threshold
        {
            self.rebuild_indexes();
        }
    }

    fn rebuild_indexes(&mut self) {
        self.lru_heap.clear();
        self.fifo_heap.clear();
        self.ttl_heap.clear();

        for (key, entry) in &self.entries {
            self.lru_heap.push(Reverse(entry.lru_candidate(key)));
            self.fifo_heap.push(Reverse(entry.fifo_candidate(key)));
            self.ttl_heap.push(Reverse(entry.ttl_candidate(key)));
        }
    }
}

/// Statistics for the query cache
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheStats {
    /// Total number of cache hits
    pub hits: u64,
    /// Total number of cache misses
    pub misses: u64,
    /// Current number of entries in cache
    pub entries: usize,
    /// Total size of cached data in bytes (approximate)
    pub size_bytes: usize,
    /// Number of evictions
    pub evictions: u64,
    /// Number of invalidations
    pub invalidations: u64,
}

impl CacheStats {
    /// Calculate the cache hit ratio
    pub fn hit_ratio(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

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
            evictions: AtomicU64::new(0),
            invalidations: AtomicU64::new(0),
        }
    }

    fn snapshot_stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            entries: self.entries.load(Ordering::Relaxed),
            size_bytes: self.size_bytes.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
            invalidations: self.invalidations.load(Ordering::Relaxed),
        }
    }

    fn record_entries_len(&self, entries: usize) {
        self.entries.store(entries, Ordering::Relaxed);
    }

    fn add_size_bytes(&self, bytes: usize) {
        self.size_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    fn subtract_size_bytes(&self, bytes: usize) {
        self.size_bytes.fetch_sub(bytes, Ordering::Relaxed);
    }

    fn overwrite_size_bytes(&self, bytes: usize) {
        self.size_bytes.store(bytes, Ordering::Relaxed);
    }

    fn next_order(&self) -> u64 {
        self.order_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Get or initialize the global query cache
    pub fn global() -> &'static QueryCache {
        GLOBAL_QUERY_CACHE.get_or_init(QueryCache::new)
    }

    /// Initialize the global cache (call at startup)
    pub fn init_global(config: CacheConfig) -> &'static QueryCache {
        let _ = GLOBAL_QUERY_CACHE.set(QueryCache::with_config(config));
        QueryCache::global()
    }

    // =========================================================================
    // CONFIGURATION
    // =========================================================================

    /// Enable the cache
    pub fn enable(&self) -> &Self {
        self.config.write().enabled = true;
        self.enabled.store(true, Ordering::Release);
        self
    }

    /// Disable the cache
    pub fn disable(&self) -> &Self {
        self.config.write().enabled = false;
        self.enabled.store(false, Ordering::Release);
        self
    }

    /// Check if cache is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// Set the maximum number of cache entries
    pub fn set_max_entries(&self, max: usize) -> &Self {
        self.config.write().max_entries = max;
        self
    }

    /// Set the default TTL for cache entries
    pub fn set_default_ttl(&self, ttl: Duration) -> &Self {
        self.config.write().default_ttl = ttl;
        self
    }

    /// Set the cache eviction strategy
    pub fn set_strategy(&self, strategy: CacheStrategy) -> &Self {
        self.config.write().strategy = strategy;
        self
    }

    /// Set the key prefix
    pub fn set_key_prefix(&self, prefix: &str) -> &Self {
        self.config.write().key_prefix = Some(prefix.to_string());
        self
    }

    /// Set whether to cache empty results
    pub fn set_cache_empty_results(&self, cache_empty: bool) -> &Self {
        self.config.write().cache_empty_results = cache_empty;
        self
    }

    /// Get current configuration
    pub fn config(&self) -> Option<CacheConfig> {
        Some(self.config.read().clone())
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
                let entry = cache
                    .entries
                    .get_mut(key)
                    .expect("entry must exist after successful immutable lookup");
                entry.touch(access_order);
                let candidate = entry.lru_candidate(key);
                let value = serde_json::from_slice(&entry.data).ok();
                cache.lru_heap.push(Reverse(candidate));
                cache.maybe_rebuild_indexes();
                self.hits.fetch_add(1, Ordering::Relaxed);
                value
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
    pub fn set<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
        model_name: &str,
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

        let entry = CacheEntry::new(data, entry_size, ttl, model_name, self.next_order());

        let mut cache = self.cache.write();
        let replacing_existing = cache.entries.contains_key(key);

        // Evict if necessary
        while !replacing_existing && cache.entries.len() >= max_entries {
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

    /// Invalidate all cache entries for a specific model/table
    pub fn invalidate_model(&self, model_name: &str) {
        let mut cache = self.cache.write();
        let keys_to_remove: Vec<String> = cache
            .entries
            .iter()
            .filter(|(_, entry)| entry.model_name == model_name)
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
