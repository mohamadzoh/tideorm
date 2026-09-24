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
    let previous = PreparedStatementCache::global().config();

    let installed = PreparedStatementCache::init_global(PreparedStatementConfig {
        enabled: true,
        max_statements: 23,
        max_age: Duration::from_secs(11),
    });

    assert!(
        installed.is_enabled(),
        "a late init_global must apply instead of being silently dropped"
    );
    let config = installed.config();
    assert_eq!(config.max_statements, 23);
    assert_eq!(config.max_age, Duration::from_secs(11));

    // Leave the process-wide cache as it was found.
    PreparedStatementCache::init_global(previous);
}
