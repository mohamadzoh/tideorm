use std::time::Duration;

/// Configuration for query result caching
///
/// Start from [`CacheConfig::default`] and change what differs:
///
/// ```
/// use std::time::Duration;
/// use tideorm::cache::{CacheConfig, QueryCache};
///
/// let cache = QueryCache::with_config(
///     CacheConfig::default()
///         .enabled(true)
///         .max_entries(5_000)
///         .default_ttl(Duration::from_secs(300))
///         .key_prefix("v2"),
/// );
/// assert!(cache.is_enabled());
/// ```
///
/// Marked `#[non_exhaustive]`, so a setting added later is not a breaking
/// change; the fields stay public to read and assign.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CacheConfig {
    /// Whether caching is enabled
    pub enabled: bool,
    /// Maximum number of entries in the cache
    pub max_entries: usize,
    /// Upper bound on the total serialized size of cached data (64 MiB by
    /// default), or `0` for no byte budget
    pub max_size_bytes: usize,
    /// Default TTL for cache entries
    pub default_ttl: Duration,
    /// Cache eviction strategy
    pub strategy: CacheStrategy,
    /// Whether to cache empty results
    pub cache_empty_results: bool,
    /// Prefix applied to every cache key to avoid collisions across namespaces
    pub key_prefix: Option<String>,
}

impl crate::cache::toggle::Switchable for CacheConfig {
    fn enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_entries: 1000,
            max_size_bytes: 64 << 20,
            default_ttl: Duration::from_secs(60),
            strategy: CacheStrategy::LRU,
            cache_empty_results: true,
            key_prefix: None,
        }
    }
}

impl CacheConfig {
    /// Turn caching on or off (off by default).
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Keep at most `entries` results (1,000 by default).
    #[must_use]
    pub fn max_entries(mut self, entries: usize) -> Self {
        self.max_entries = entries;
        self
    }

    /// Keep at most `bytes` of serialized results (64 MiB by default), or `0`
    /// for no byte budget.
    #[must_use]
    pub fn max_size_bytes(mut self, bytes: usize) -> Self {
        self.max_size_bytes = bytes;
        self
    }

    /// How long a result stays cached when the query names no TTL (60 seconds
    /// by default).
    #[must_use]
    pub fn default_ttl(mut self, ttl: Duration) -> Self {
        self.default_ttl = ttl;
        self
    }

    /// Which entries make room for new ones ([`CacheStrategy::LRU`] by default).
    #[must_use]
    pub fn strategy(mut self, strategy: CacheStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Whether a query that found nothing is cached too (`true` by default).
    #[must_use]
    pub fn cache_empty_results(mut self, cache: bool) -> Self {
        self.cache_empty_results = cache;
        self
    }

    /// Put `prefix` in front of every key, so caches of two namespaces never
    /// share an entry.
    #[must_use]
    pub fn key_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.key_prefix = Some(prefix.into());
        self
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
