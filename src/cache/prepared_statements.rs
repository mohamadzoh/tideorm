use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::GLOBAL_STMT_CACHE;
use super::toggle::{Switchable, Toggle, hit_ratio};

/// A tracked SQL statement and its execution statistics
#[derive(Debug, Clone)]
struct PreparedStatement {
    /// The SQL query template (with placeholders)
    sql: String,
    /// When this statement was first registered
    prepared_at: Instant,
    /// When this statement was last used
    last_used: Instant,
    /// Number of times this statement has been executed
    execution_count: u64,
    /// Accumulated duration; divide only when reporting an average.
    total_execution_time_us: u128,
}

impl PreparedStatement {
    fn new(sql: String) -> Self {
        let now = Instant::now();
        Self {
            sql,
            prepared_at: now,
            last_used: now,
            execution_count: 0,
            total_execution_time_us: 0,
        }
    }

    fn record_execution(&mut self, execution_time_us: u64) {
        self.last_used = Instant::now();
        self.total_execution_time_us += u128::from(execution_time_us);
        self.execution_count += 1;
    }
}

/// Statistics for prepared statement cache
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PreparedStatementStats {
    /// Total number of cache hits (statement seen before)
    pub hits: u64,
    /// Total number of cache misses (statement seen for the first time)
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
        hit_ratio(self.hits, self.misses)
    }
}

/// Configuration for prepared statement caching
#[derive(Debug, Clone)]
pub struct PreparedStatementConfig {
    /// Whether caching is enabled
    pub enabled: bool,
    /// Maximum number of cached statements
    pub max_statements: usize,
    /// Maximum age of a cached statement; an older one is dropped and
    /// registered afresh on its next use
    pub max_age: Duration,
}

impl Switchable for PreparedStatementConfig {
    fn enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
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
    config: Toggle<PreparedStatementConfig>,
    /// Cached statements keyed by SQL hash
    statements: RwLock<HashMap<u64, PreparedStatement>>,
    /// Cache hit counter.
    hits: AtomicU64,
    /// Cache miss counter.
    misses: AtomicU64,
    /// Total number of statement executions.
    total_executions: AtomicU64,
    /// Number of evictions.
    evictions: AtomicU64,
}

impl PreparedStatementCache {
    /// Create a new prepared statement cache
    pub fn new() -> Self {
        Self::with_config(PreparedStatementConfig::default())
    }

    /// Create with custom configuration
    pub fn with_config(config: PreparedStatementConfig) -> Self {
        Self {
            config: Toggle::new(config),
            statements: RwLock::new(HashMap::new()),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            total_executions: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
        }
    }

    /// Get or initialize the global prepared statement cache
    pub fn global() -> &'static PreparedStatementCache {
        GLOBAL_STMT_CACHE.get_or_init(PreparedStatementCache::new)
    }

    /// Initialize the global cache (call at startup)
    ///
    /// Any earlier `global()` call already installed a default instance, and the
    /// `OnceLock` behind it cannot be replaced, so the requested configuration is
    /// applied to the live cache instead of being dropped.
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
        self.config.apply(config);
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
    pub fn config(&self) -> PreparedStatementConfig {
        self.config.read().clone()
    }

    /// Hash a SQL query for cache lookup
    ///
    /// The digest is 64 bits wide, so distinct statements can collide. Every
    /// lookup in this cache therefore re-checks the stored SQL before treating a
    /// slot as a match; never use the hash on its own to decide that two
    /// statements are the same.
    pub fn hash_sql(sql: &str) -> u64 {
        super::hash_text(sql)
    }

    /// Register `sql` and report whether it was already cached
    ///
    /// Returns `(sql, is_cached)`, handing `sql` back unchanged: nothing is
    /// prepared here, since the driver owns the real prepared handles.
    ///
    /// A slot occupied by a *different* statement that happens to hash the same
    /// counts as a miss: the colliding entry is replaced rather than handed back,
    /// so a collision can never make one statement execute another's SQL.
    pub fn get_or_prepare(&self, sql: &str) -> (String, bool) {
        (sql.to_string(), self.register(sql, None))
    }

    // Registration and optional timing update share one lock and one hash.
    fn register(&self, sql: &str, execution_time_us: Option<u64>) -> bool {
        if !self.is_enabled() {
            return false;
        }
        let config = self.config.read().clone();
        let hash = Self::hash_sql(sql);
        let mut statements = self.statements.write();
        let hit = statements
            .get(&hash)
            .is_some_and(|stmt| stmt.sql == sql && stmt.prepared_at.elapsed() < config.max_age);
        if hit {
            self.hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
            statements.remove(&hash);
            if config.max_statements > 0 {
                while statements.len() >= config.max_statements {
                    let oldest = statements
                        .iter()
                        .min_by_key(|(_, stmt)| stmt.last_used)
                        .map(|(key, _)| *key);
                    if let Some(key) = oldest {
                        statements.remove(&key);
                        self.evictions.fetch_add(1, Ordering::Relaxed);
                    } else {
                        break;
                    }
                }
                statements.insert(hash, PreparedStatement::new(sql.to_string()));
            }
        }
        if let Some(duration) = execution_time_us {
            if let Some(stmt) = statements.get_mut(&hash) {
                stmt.record_execution(duration);
            }
            self.total_executions.fetch_add(1, Ordering::Relaxed);
        }
        hit
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

        self.register(sql, Some(execution_time_us));
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

        if let Some(stmt) = self
            .statements
            .write()
            .get_mut(&hash)
            .filter(|stmt| stmt.sql == sql)
        {
            stmt.record_execution(execution_time_us);
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
        true
    }

    /// Clear all cached statements
    pub fn clear(&self) {
        self.statements.write().clear();
    }

    /// Get cache statistics
    pub fn stats(&self) -> PreparedStatementStats {
        PreparedStatementStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            cached_count: self.len(),
            total_executions: self.total_executions.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
        }
    }

    /// Reset statistics
    ///
    /// The cached-statement count describes the cache itself and is
    /// unaffected.
    pub fn reset_stats(&self) {
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
        self.total_executions.store(0, Ordering::Relaxed);
        self.evictions.store(0, Ordering::Relaxed);
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
                avg_execution_time_us: (stmt.total_execution_time_us
                    / u128::from(stmt.execution_count.max(1)))
                    as u64,
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
#[path = "../../tests/unit/cache_prepared_statements_tests.rs"]
mod tests;
