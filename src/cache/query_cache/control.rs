use super::QueryCache;
use crate::cache::GLOBAL_QUERY_CACHE;

use super::{CacheConfig, CacheStats, CacheStrategy};

impl QueryCache {
    pub(super) fn snapshot_stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits.load(std::sync::atomic::Ordering::Relaxed),
            misses: self.misses.load(std::sync::atomic::Ordering::Relaxed),
            entries: self.entries.load(std::sync::atomic::Ordering::Relaxed),
            size_bytes: self.size_bytes.load(std::sync::atomic::Ordering::Relaxed),
            evictions: self.evictions.load(std::sync::atomic::Ordering::Relaxed),
            invalidations: self
                .invalidations
                .load(std::sync::atomic::Ordering::Relaxed),
        }
    }

    pub(super) fn record_entries_len(&self, entries: usize) {
        self.entries
            .store(entries, std::sync::atomic::Ordering::Relaxed);
    }

    pub(super) fn add_size_bytes(&self, bytes: usize) {
        self.size_bytes
            .fetch_add(bytes, std::sync::atomic::Ordering::Relaxed);
    }

    pub(super) fn subtract_size_bytes(&self, bytes: usize) {
        self.size_bytes
            .fetch_sub(bytes, std::sync::atomic::Ordering::Relaxed);
    }

    pub(super) fn overwrite_size_bytes(&self, bytes: usize) {
        self.size_bytes
            .store(bytes, std::sync::atomic::Ordering::Relaxed);
    }

    pub(super) fn next_order(&self) -> u64 {
        self.order_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
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

    /// Enable the cache
    pub fn enable(&self) -> &Self {
        self.config.write().enabled = true;
        self.enabled
            .store(true, std::sync::atomic::Ordering::Release);
        self
    }

    /// Disable the cache
    pub fn disable(&self) -> &Self {
        self.config.write().enabled = false;
        self.enabled
            .store(false, std::sync::atomic::Ordering::Release);
        self
    }

    /// Check if cache is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Set the maximum number of cache entries
    pub fn set_max_entries(&self, max: usize) -> &Self {
        self.config.write().max_entries = max;
        self
    }

    /// Set the default TTL for cache entries
    pub fn set_default_ttl(&self, ttl: std::time::Duration) -> &Self {
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
}
