use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use super::GLOBAL_STMT_CACHE;

/// A cached prepared statement
#[derive(Debug, Clone)]
struct PreparedStatement {
    /// The SQL query template (with placeholders)
    sql: String,
    /// When this statement was prepared
    prepared_at: Instant,
    /// When this statement was last used
    last_used: Instant,
    /// Number of times this statement has been executed
    execution_count: u64,
    /// Average execution time in microseconds
    avg_execution_time_us: u64,
}

impl PreparedStatement {
    fn new(sql: String) -> Self {
        let now = Instant::now();
        Self {
            sql,
            prepared_at: now,
            last_used: now,
            execution_count: 0,
            avg_execution_time_us: 0,
        }
    }

    fn record_execution(&mut self, execution_time_us: u64) {
        self.last_used = Instant::now();
        let total = self.avg_execution_time_us * self.execution_count + execution_time_us;
        self.execution_count += 1;
        self.avg_execution_time_us = total / self.execution_count;
    }
}

/// Statistics for prepared statement cache
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PreparedStatementStats {
    /// Total number of cache hits (statement reused)
    pub hits: u64,
    /// Total number of cache misses (new statement prepared)
    pub misses: u64,
    /// Current number of cached statements
    pub cached_count: usize,
    /// Total number of statement executions
    pub total_executions: u64,
    /// Number of evictions
    pub evictions: u64,
}

impl PreparedStatementStats {
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

/// Configuration for prepared statement caching
#[derive(Debug, Clone)]
pub struct PreparedStatementConfig {
    /// Whether caching is enabled
    pub enabled: bool,
    /// Maximum number of cached statements
    pub max_statements: usize,
    /// Maximum age of cached statements (they'll be re-prepared after this)
    pub max_age: Duration,
}

impl Default for PreparedStatementConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_statements: 500,
            max_age: Duration::from_secs(3600),
        }
    }
}

/// Prepared statement cache
///
/// Caches prepared SQL statements to avoid repeated parsing and planning.
/// This is especially beneficial for queries that are executed frequently
/// with different parameter values.
#[derive(Debug)]
pub struct PreparedStatementCache {
    /// Cache configuration
    config: RwLock<PreparedStatementConfig>,
    /// Fast path for checking whether caching is enabled.
    enabled: AtomicBool,
    /// Cached statements keyed by SQL hash
    statements: RwLock<HashMap<u64, PreparedStatement>>,
    /// Cache hit counter.
    hits: AtomicU64,
    /// Cache miss counter.
    misses: AtomicU64,
    /// Current number of cached statements.
    cached_count: AtomicUsize,
    /// Total number of statement executions.
    total_executions: AtomicU64,
    /// Number of evictions.
    evictions: AtomicU64,
}

impl PreparedStatementCache {
    /// Create a new prepared statement cache
    pub fn new() -> Self {
        Self {
            config: RwLock::new(PreparedStatementConfig::default()),
            enabled: AtomicBool::new(false),
            statements: RwLock::new(HashMap::new()),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            cached_count: AtomicUsize::new(0),
            total_executions: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
        }
    }

    /// Create with custom configuration
    pub fn with_config(config: PreparedStatementConfig) -> Self {
        let enabled = config.enabled;
        Self {
            config: RwLock::new(config),
            enabled: AtomicBool::new(enabled),
            statements: RwLock::new(HashMap::new()),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            cached_count: AtomicUsize::new(0),
            total_executions: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
        }
    }

    fn snapshot_stats(&self) -> PreparedStatementStats {
        PreparedStatementStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            cached_count: self.cached_count.load(Ordering::Relaxed),
            total_executions: self.total_executions.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
        }
    }

    /// Get or initialize the global prepared statement cache
    pub fn global() -> &'static PreparedStatementCache {
        GLOBAL_STMT_CACHE.get_or_init(PreparedStatementCache::new)
    }

    /// Initialize the global cache (call at startup)
    pub fn init_global(config: PreparedStatementConfig) -> &'static PreparedStatementCache {
        let _ = GLOBAL_STMT_CACHE.set(PreparedStatementCache::with_config(config));
        PreparedStatementCache::global()
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

    /// Set the maximum number of cached statements
    pub fn set_max_statements(&self, max: usize) -> &Self {
        self.config.write().max_statements = max;
        self
    }

    /// Set the maximum age for cached statements
    pub fn set_max_age(&self, age: Duration) -> &Self {
        self.config.write().max_age = age;
        self
    }

    /// Get current configuration
    pub fn config(&self) -> Option<PreparedStatementConfig> {
        Some(self.config.read().clone())
    }

    // =========================================================================
    // CACHE OPERATIONS
    // =========================================================================

    /// Hash a SQL query for cache lookup
    pub fn hash_sql(sql: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        sql.hash(&mut hasher);
        hasher.finish()
    }

    /// Get or prepare a statement
    /// Returns (sql, is_cached)
    pub fn get_or_prepare(&self, sql: &str) -> (String, bool) {
        if !self.is_enabled() {
            return (sql.to_string(), false);
        }

        let hash = Self::hash_sql(sql);
        let max_age = self.config.read().max_age;

        // Fast path: read-only cache hit without taking the write lock.
        {
            let statements = self.statements.read();
            if let Some(stmt) = statements.get(&hash) {
                if stmt.prepared_at.elapsed() < max_age {
                    let sql = stmt.sql.clone();
                    drop(statements);
                    self.hits.fetch_add(1, Ordering::Relaxed);
                    return (sql, true);
                }
            }
        }

        // Remove expired entries or resolve races under the write lock.
        {
            let mut statements = self.statements.write();
            if let Some(stmt) = statements.get(&hash) {
                if stmt.prepared_at.elapsed() < max_age {
                    let sql = stmt.sql.clone();
                    drop(statements);
                    self.hits.fetch_add(1, Ordering::Relaxed);
                    return (sql, true);
                }

                statements.remove(&hash);
            }
        }

        // Cache miss - prepare and cache
        self.cache_statement(sql);

        self.misses.fetch_add(1, Ordering::Relaxed);

        (sql.to_string(), false)
    }

    /// Cache a statement
    fn cache_statement(&self, sql: &str) {
        let hash = Self::hash_sql(sql);
        let max_statements = self.config.read().max_statements;

        let mut statements = self.statements.write();
        while statements.len() >= max_statements {
            let oldest_key = statements
                .iter()
                .min_by_key(|(_, stmt)| stmt.last_used)
                .map(|(key, _)| *key);

            if let Some(key) = oldest_key {
                statements.remove(&key);
                self.evictions.fetch_add(1, Ordering::Relaxed);
            }
        }

        statements.insert(hash, PreparedStatement::new(sql.to_string()));

        self.cached_count.store(statements.len(), Ordering::Relaxed);
    }

    /// Record execution of a statement
    pub fn record_execution(&self, sql: &str, execution_time_us: u64) {
        if !self.is_enabled() {
            return;
        }

        let hash = Self::hash_sql(sql);

        {
            let mut statements = self.statements.write();
            if let Some(stmt) = statements.get_mut(&hash) {
                stmt.record_execution(execution_time_us);
            }
        }

        self.total_executions.fetch_add(1, Ordering::Relaxed);
    }

    /// Invalidate a specific statement
    pub fn invalidate(&self, sql: &str) -> bool {
        let hash = Self::hash_sql(sql);
        let mut statements = self.statements.write();
        let removed = statements.remove(&hash).is_some();
        if removed {
            self.cached_count.store(statements.len(), Ordering::Relaxed);
        }
        removed
    }

    /// Clear all cached statements
    pub fn clear(&self) {
        let mut statements = self.statements.write();
        statements.clear();
        self.cached_count.store(0, Ordering::Relaxed);
    }

    /// Get cache statistics
    pub fn stats(&self) -> PreparedStatementStats {
        self.snapshot_stats()
    }

    /// Reset statistics
    pub fn reset_stats(&self) {
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
        self.total_executions.store(0, Ordering::Relaxed);
        self.evictions.store(0, Ordering::Relaxed);
        self.cached_count
            .store(self.statements.read().len(), Ordering::Relaxed);
    }

    /// Get the number of cached statements
    pub fn len(&self) -> usize {
        self.statements.read().len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get information about cached statements
    pub fn cached_statements_info(&self) -> Vec<CachedStatementInfo> {
        let statements = self.statements.read();
        statements
            .iter()
            .map(|(hash, stmt)| CachedStatementInfo {
                hash: *hash,
                sql_preview: if stmt.sql.len() > 100 {
                    format!("{}...", &stmt.sql[..100])
                } else {
                    stmt.sql.clone()
                },
                execution_count: stmt.execution_count,
                avg_execution_time_us: stmt.avg_execution_time_us,
                age_secs: stmt.prepared_at.elapsed().as_secs(),
            })
            .collect()
    }
}

impl Default for PreparedStatementCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about a cached statement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedStatementInfo {
    /// Hash of the statement
    pub hash: u64,
    /// Preview of the SQL (truncated)
    pub sql_preview: String,
    /// Number of times executed
    pub execution_count: u64,
    /// Average execution time in microseconds
    pub avg_execution_time_us: u64,
    /// Age of the cached statement in seconds
    pub age_secs: u64,
}
