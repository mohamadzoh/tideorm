use std::sync::atomic::{AtomicBool, Ordering};

use super::{CacheConfig, CacheInvalidation, CacheStrategy, QueryCache};
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
        let mut config = Some(config);
        let cache = GLOBAL_QUERY_CACHE
            .get_or_init(|| QueryCache::with_config(config.take().unwrap_or_default()));
        // Left over when the cache was already installed, by `global()` or a racing call.
        if let Some(config) = config {
            cache.apply_config(config);
        }
        cache
    }

    /// Enable the cache
    pub fn enable(&self) -> &Self {
        self.config.set_enabled(true);
        self
    }

    /// Disable the cache
    pub fn disable(&self) -> &Self {
        self.config.set_enabled(false);
        self
    }

    /// Check if cache is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.is_enabled()
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
    ///
    /// Lowering it evicts entries with the configured strategy right away.
    pub fn set_max_entries(&self, max: usize) -> &Self {
        self.config.write().max_entries = max;
        self.enforce_limits();
        self
    }

    /// Set an upper bound on the total serialized size of cached data
    ///
    /// Entries are evicted with the configured strategy until a new payload fits
    /// inside the budget, and a payload larger than the whole budget is never
    /// cached. The default is 64 MiB; pass `0` for no byte budget, in which
    /// case only `max_entries` bounds the cache. Lowering it evicts entries
    /// right away.
    pub fn set_max_size_bytes(&self, max: usize) -> &Self {
        self.config.write().max_size_bytes = max;
        self.enforce_limits();
        self
    }

    /// Evict with the configured strategy until the cache is inside its
    /// entry and byte limits, which a setter may just have lowered.
    pub(super) fn enforce_limits(&self) {
        let (max_entries, max_bytes, strategy) = {
            let config = self.config.read();
            (config.max_entries, config.max_size_bytes, config.strategy)
        };
        let mut cache = self.cache.write();
        while cache.len() > max_entries || (max_bytes > 0 && cache.size_bytes() > max_bytes) {
            if !self.evict_one(&mut cache, strategy) {
                break;
            }
        }
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

    /// Call `hook` after each invalidation of this cache: a write's (as it
    /// runs, and again when its transaction commits),
    /// [`invalidate_model`](Self::invalidate_model)'s and
    /// [`clear`](Self::clear)'s, but not a single key's
    /// [`invalidate`](Self::invalidate) or an expiry. To keep the caches of
    /// several processes in step, publish each event on a channel of your own
    /// (Redis, NATS, ..) and hand what the other processes publish to
    /// [`apply_invalidation`](Self::apply_invalidation). The hook runs on the
    /// writing task: hand the event off rather than wait on the network.
    ///
    /// ```ignore
    /// let (events, mut outgoing) = tokio::sync::mpsc::unbounded_channel();
    /// QueryCache::global().set_invalidation_hook(move |event| { let _ = events.send(event.clone()); });
    /// // a task publishes `outgoing`; one subscribed to the other processes calls
    /// QueryCache::global().apply_invalidation(&received);
    /// ```
    pub fn set_invalidation_hook(
        &self,
        hook: impl Fn(&CacheInvalidation) + Send + Sync + 'static,
    ) -> &Self {
        self.invalidation_hook.set(std::sync::Arc::new(hook));
        self
    }

    /// Stop calling the hook [`set_invalidation_hook`](Self::set_invalidation_hook) set.
    pub fn clear_invalidation_hook(&self) -> &Self {
        self.invalidation_hook.clear();
        self
    }
}
