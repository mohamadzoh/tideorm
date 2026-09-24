use std::sync::atomic::{AtomicBool, Ordering};

use super::{CacheConfig, CacheStrategy, QueryCache};
use crate::cache::GLOBAL_QUERY_CACHE;

impl QueryCache {
    pub(super) fn next_order(&self) -> u64 {
        self.order_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Get or initialize the global query cache
    pub fn global() -> &'static QueryCache {
        GLOBAL_QUERY_CACHE.get_or_init(QueryCache::new)
    }

    /// Whether this is the instance model writes invalidate, whose
    /// invalidations an open transaction replays once it commits.
    pub(super) fn is_global(&self) -> bool {
        GLOBAL_QUERY_CACHE
            .get()
            .is_some_and(|global| std::ptr::eq(self, global))
    }

    /// Initialize the global cache (call at startup)
    ///
    /// Any earlier `global()` call already installed a default instance, and the
    /// `OnceLock` behind it cannot be replaced. Because macro-generated model
    /// writes reach for `QueryCache::global()`, a single model operation before
    /// startup is enough to install that default, so the requested configuration
    /// is applied to the live cache instead of being dropped.
    ///
    /// Already-cached entries are kept; see [`QueryCache::apply_config`].
    pub fn init_global(config: CacheConfig) -> &'static QueryCache {
        if GLOBAL_QUERY_CACHE
            .set(QueryCache::with_config(config.clone()))
            .is_err()
        {
            QueryCache::global().apply_config(config);
        }
        QueryCache::global()
    }

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

    /// Warn, once per process, when a query asks for caching while the cache
    /// is off. The cache starts disabled, so `.cache(ttl)` would otherwise do
    /// nothing without a word.
    pub(crate) fn warn_once_if_disabled(&self) {
        static WARNED: AtomicBool = AtomicBool::new(false);
        if !self.is_enabled() && !WARNED.swap(true, Ordering::Relaxed) {
            crate::tide_warn!(
                "a query asked for .cache(..), but the query cache is disabled, so nothing is cached; enable it with QueryCache::global().enable() or QueryCache::init_global(..)"
            );
        }
    }

    /// Set the maximum number of cache entries
    pub fn set_max_entries(&self, max: usize) -> &Self {
        self.config.write().max_entries = max;
        self
    }

    /// Set an upper bound on the total serialized size of cached data
    ///
    /// Entries are evicted with the configured strategy until a new payload fits
    /// inside the budget, and a payload larger than the whole budget is never
    /// cached. Pass `0` (the default) for no byte budget, in which case only
    /// `max_entries` bounds the cache.
    pub fn set_max_size_bytes(&self, max: usize) -> &Self {
        self.config.write().max_size_bytes = max;
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

    /// Set whether to cache empty results
    pub fn set_cache_empty_results(&self, cache_empty: bool) -> &Self {
        self.config.write().cache_empty_results = cache_empty;
        self
    }

    /// Get current configuration
    pub fn config(&self) -> CacheConfig {
        self.config.read().clone()
    }
}
