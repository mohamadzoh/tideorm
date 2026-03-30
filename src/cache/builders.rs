use std::hash::{Hash, Hasher};
use std::time::Duration;

// =============================================================================
// CACHE KEY BUILDER
// =============================================================================

/// Builder for generating cache keys from query parameters
#[derive(Debug, Default)]
pub struct CacheKeyBuilder {
    parts: Vec<String>,
}

impl CacheKeyBuilder {
    /// Create a new cache key builder
    pub fn new() -> Self {
        Self { parts: Vec::new() }
    }

    /// Add a table name
    pub fn table(mut self, table: &str) -> Self {
        self.parts.push(format!("t:{}", table));
        self
    }

    /// Add a column condition
    pub fn condition(mut self, column: &str, value: impl std::fmt::Display) -> Self {
        self.parts.push(format!("{}={}", column, value));
        self
    }

    /// Add an order by clause
    pub fn order(mut self, column: &str, direction: &str) -> Self {
        self.parts.push(format!("o:{}:{}", column, direction));
        self
    }

    /// Add a limit
    pub fn limit(mut self, limit: u64) -> Self {
        self.parts.push(format!("l:{}", limit));
        self
    }

    /// Add an offset
    pub fn offset(mut self, offset: u64) -> Self {
        self.parts.push(format!("off:{}", offset));
        self
    }

    /// Add a raw part
    pub fn raw(mut self, part: &str) -> Self {
        self.parts.push(part.to_string());
        self
    }

    /// Build the cache key
    pub fn build(self) -> String {
        self.parts.join(":")
    }

    /// Build and hash the cache key
    pub fn build_hash(self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        let key = self.build();
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish()
    }
}

// =============================================================================
// CACHEABLE QUERY EXTENSION
// =============================================================================

/// Options for caching a query
#[derive(Debug, Clone)]
pub struct CacheOptions {
    /// Custom cache key (if None, generated from query)
    pub key: Option<String>,
    /// TTL for this specific query
    pub ttl: Duration,
    /// Tags for this cache entry (for bulk invalidation)
    pub tags: Vec<String>,
}

impl CacheOptions {
    /// Create new cache options with TTL
    pub fn new(ttl: Duration) -> Self {
        Self {
            key: None,
            ttl,
            tags: Vec::new(),
        }
    }

    /// Set a custom cache key
    pub fn with_key(mut self, key: &str) -> Self {
        self.key = Some(key.to_string());
        self
    }

    /// Add a tag
    pub fn with_tag(mut self, tag: &str) -> Self {
        self.tags.push(tag.to_string());
        self
    }

    /// Add multiple tags
    pub fn with_tags(mut self, tags: &[&str]) -> Self {
        self.tags.extend(tags.iter().map(|s| s.to_string()));
        self
    }
}

// =============================================================================
// CACHE WARMING
// =============================================================================

/// Cache warming configuration
#[derive(Debug, Clone)]
pub struct CacheWarmer {
    queries: Vec<WarmQuery>,
}

/// A query to warm the cache with
#[derive(Debug, Clone)]
struct WarmQuery {
    key: String,
    sql: String,
    ttl: Duration,
}

impl CacheWarmer {
    /// Create a new cache warmer
    pub fn new() -> Self {
        Self {
            queries: Vec::new(),
        }
    }

    /// Add a query to warm
    pub fn add_query(mut self, key: &str, sql: &str, ttl: Duration) -> Self {
        self.queries.push(WarmQuery {
            key: key.to_string(),
            sql: sql.to_string(),
            ttl,
        });
        self
    }

    /// Get the number of queries to warm
    pub fn query_count(&self) -> usize {
        self.queries.len()
    }

    /// Get the configured queries for warming
    pub fn queries(&self) -> impl Iterator<Item = (&str, &str, Duration)> {
        self.queries
            .iter()
            .map(|q| (q.key.as_str(), q.sql.as_str(), q.ttl))
    }
}

impl Default for CacheWarmer {
    fn default() -> Self {
        Self::new()
    }
}