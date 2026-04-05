use std::time::Duration;

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
