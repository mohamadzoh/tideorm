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
    ///
    /// The digest is 64 bits wide and is not collision-free, so it identifies a
    /// key only probabilistically. Store or compare the full key from
    /// [`CacheKeyBuilder::build`] whenever handing back the wrong entry would be
    /// a correctness problem; use the hash purely as a compact bucket id.
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
}

impl CacheOptions {
    /// Create new cache options with TTL
    pub fn new(ttl: Duration) -> Self {
        Self { key: None, ttl }
    }

    /// Set a custom cache key
    pub fn with_key(mut self, key: &str) -> Self {
        self.key = Some(key.to_string());
        self
    }
}
