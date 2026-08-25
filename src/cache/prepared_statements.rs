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
/// Tracks the distinct SQL shapes executed through the query builder together
/// with how often each one ran and how long it took, so repeated statements can
/// be found without turning on full query logging.
///
/// Driver-level prepared handles are owned by the pooled connection, not by this
/// type; what lives here is the bookkeeping. The query builder calls
/// [`PreparedStatementCache::observe_execution`] after every statement it runs,
/// so [`PreparedStatementCache::stats`] and
/// [`PreparedStatementCache::cached_statements_info`] describe real traffic once
/// the cache is enabled. It is disabled by default and costs a single atomic
/// load per query while it stays that way.
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
    ///
    /// Any earlier `global()` call already installed a default instance, and the
    /// `OnceLock` behind it cannot be replaced. Rather than silently dropping the
    /// requested configuration, it is applied to the live cache, so a late
    /// `init_global` still takes effect.
    pub fn init_global(config: PreparedStatementConfig) -> &'static PreparedStatementCache {
        if GLOBAL_STMT_CACHE
            .set(PreparedStatementCache::with_config(config.clone()))
            .is_err()
        {
            PreparedStatementCache::global().apply_config(config);
        }
        PreparedStatementCache::global()
    }

    /// Replace this cache's configuration in place
    ///
    /// Cached statements are kept; only the configuration and the enabled flag
    /// change.
    pub fn apply_config(&self, config: PreparedStatementConfig) {
        let enabled = config.enabled;
        *self.config.write() = config;
        self.enabled.store(enabled, Ordering::Release);
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
    ///
    /// The digest is 64 bits wide, so distinct statements can collide. Every
    /// lookup in this cache therefore re-checks the stored SQL before treating a
    /// slot as a match; never use the hash on its own to decide that two
    /// statements are the same.
    pub fn hash_sql(sql: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        sql.hash(&mut hasher);
        hasher.finish()
    }

    /// Get or prepare a statement
    /// Returns (sql, is_cached)
    ///
    /// A slot occupied by a *different* statement that happens to hash the same
    /// counts as a miss: the colliding entry is replaced rather than handed back,
    /// so a collision can never make one statement execute another's SQL.
    pub fn get_or_prepare(&self, sql: &str) -> (String, bool) {
        if !self.is_enabled() {
            return (sql.to_string(), false);
        }

        let hash = Self::hash_sql(sql);
        let max_age = self.config.read().max_age;

        // Fast path: read-only cache hit without taking the write lock.
        {
            let statements = self.statements.read();
            if let Some(stmt) = statements.get(&hash)
                && stmt.sql == sql
                && stmt.prepared_at.elapsed() < max_age
            {
                drop(statements);
                self.hits.fetch_add(1, Ordering::Relaxed);
                return (sql.to_string(), true);
            }
        }

        // Remove expired or colliding entries, and resolve races, under the
        // write lock.
        {
            let mut statements = self.statements.write();
            if let Some(stmt) = statements.get(&hash) {
                if stmt.sql == sql && stmt.prepared_at.elapsed() < max_age {
                    drop(statements);
                    self.hits.fetch_add(1, Ordering::Relaxed);
                    return (sql.to_string(), true);
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

        // A zero budget means "cache nothing". Falling through would spin forever
        // under the write lock: the eviction loop can never bring an empty map
        // below zero entries.
        if max_statements == 0 {
            return;
        }

        let mut statements = self.statements.write();
        while statements.len() >= max_statements {
            let oldest_key = statements
                .iter()
                .min_by_key(|(_, stmt)| stmt.last_used)
                .map(|(key, _)| *key);

            match oldest_key {
                Some(key) => {
                    statements.remove(&key);
                    self.evictions.fetch_add(1, Ordering::Relaxed);
                }
                None => break,
            }
        }

        statements.insert(hash, PreparedStatement::new(sql.to_string()));

        self.cached_count.store(statements.len(), Ordering::Relaxed);
    }

    /// Record one execution of `sql`, registering the statement on first sight
    ///
    /// This is the hook the query builder calls after every statement it runs, so
    /// the reported statistics reflect real traffic. It is a no-op while the
    /// cache is disabled, which is the default.
    pub fn observe_execution(&self, sql: &str, execution_time_us: u64) {
        if !self.is_enabled() {
            return;
        }

        let _ = self.get_or_prepare(sql);
        self.record_execution(sql, execution_time_us);
    }

    /// Record execution of a statement
    ///
    /// Timings are only attributed to a slot holding this exact SQL, so a hash
    /// collision cannot pollute another statement's statistics.
    pub fn record_execution(&self, sql: &str, execution_time_us: u64) {
        if !self.is_enabled() {
            return;
        }

        let hash = Self::hash_sql(sql);

        {
            let mut statements = self.statements.write();
            if let Some(stmt) = statements.get_mut(&hash).filter(|stmt| stmt.sql == sql) {
                stmt.record_execution(execution_time_us);
            }
        }

        self.total_executions.fetch_add(1, Ordering::Relaxed);
    }

    /// Invalidate a specific statement
    ///
    /// Returns `false` without touching the cache when the matching slot holds a
    /// different statement that merely hashes the same.
    pub fn invalidate(&self, sql: &str) -> bool {
        let hash = Self::hash_sql(sql);
        let mut statements = self.statements.write();
        if statements.get(&hash).is_none_or(|stmt| stmt.sql != sql) {
            return false;
        }

        statements.remove(&hash);
        self.cached_count.store(statements.len(), Ordering::Relaxed);
        true
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
                sql_preview: sql_preview(&stmt.sql),
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

/// Shorten SQL to a preview without splitting a UTF-8 character.
///
/// Slicing at a fixed byte offset panics on any statement carrying multi-byte
/// text, so the cut is made on a character boundary instead.
fn sql_preview(sql: &str) -> String {
    const PREVIEW_CHARS: usize = 100;

    match sql.char_indices().nth(PREVIEW_CHARS) {
        Some((index, _)) => format!("{}...", &sql[..index]),
        None => sql.to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Plant `stored` in the slot `probe` hashes to, simulating a 64-bit
    /// collision without having to find a real one.
    fn plant_colliding_statement(cache: &PreparedStatementCache, probe: &str, stored: &str) {
        let hash = PreparedStatementCache::hash_sql(probe);
        cache
            .statements
            .write()
            .insert(hash, PreparedStatement::new(stored.to_string()));
    }

    #[test]
    fn get_or_prepare_never_returns_a_colliding_statements_sql() {
        let cache = PreparedStatementCache::new();
        cache.enable();
        plant_colliding_statement(&cache, "SELECT 1", "DELETE FROM users");

        let (sql, cached) = cache.get_or_prepare("SELECT 1");

        assert_eq!(sql, "SELECT 1");
        assert!(!cached, "a colliding slot must not count as a cache hit");
        assert_eq!(cache.stats().hits, 0);

        // The colliding entry was replaced, so the next lookup is a real hit.
        let (sql, cached) = cache.get_or_prepare("SELECT 1");
        assert_eq!(sql, "SELECT 1");
        assert!(cached);
    }

    #[test]
    fn record_execution_ignores_a_colliding_statement() {
        let cache = PreparedStatementCache::new();
        cache.enable();
        plant_colliding_statement(&cache, "SELECT 1", "DELETE FROM users");

        cache.record_execution("SELECT 1", 1_000);

        let info = cache.cached_statements_info();
        assert_eq!(info.len(), 1);
        assert_eq!(info[0].execution_count, 0);
    }

    #[test]
    fn invalidate_leaves_a_colliding_statement_alone() {
        let cache = PreparedStatementCache::new();
        cache.enable();
        plant_colliding_statement(&cache, "SELECT 1", "DELETE FROM users");

        assert!(!cache.invalidate("SELECT 1"));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn init_global_applies_config_after_a_default_cache_was_installed() {
        // Touching the global cache installs the disabled default that
        // `init_global` used to be unable to replace.
        let previous = PreparedStatementCache::global()
            .config()
            .expect("the global cache always reports a configuration");

        let installed = PreparedStatementCache::init_global(PreparedStatementConfig {
            enabled: true,
            max_statements: 23,
            max_age: Duration::from_secs(11),
        });

        assert!(
            installed.is_enabled(),
            "a late init_global must apply instead of being silently dropped"
        );
        let config = installed
            .config()
            .expect("the global cache always reports a configuration");
        assert_eq!(config.max_statements, 23);
        assert_eq!(config.max_age, Duration::from_secs(11));

        // Leave the process-wide cache as it was found.
        PreparedStatementCache::init_global(previous);
    }
}
