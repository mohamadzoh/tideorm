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

#[test]
fn average_uses_accumulated_duration() {
    let cache = PreparedStatementCache::new();
    cache.enable();
    for time in [1, 2, 3] {
        cache.observe_execution("SELECT 1", time);
    }
    assert_eq!(cache.cached_statements_info()[0].avg_execution_time_us, 2);
}

#[test]
fn concurrent_registration_preserves_all_executions() {
    let cache = PreparedStatementCache::new();
    cache.enable();
    let barrier = std::sync::Barrier::new(8);
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                barrier.wait();
                for _ in 0..100 {
                    cache.observe_execution("SELECT 1", 3);
                }
            });
        }
    });
    assert_eq!(cache.cached_statements_info()[0].execution_count, 800);
    assert_eq!(cache.stats().misses, 1);
}

#[test]
fn registration_hits_refresh_recency() {
    let cache = PreparedStatementCache::new();
    cache.enable();
    cache.set_max_statements(2);
    cache.get_or_prepare("SELECT 1");
    std::thread::sleep(Duration::from_millis(2));
    cache.get_or_prepare("SELECT 2");
    std::thread::sleep(Duration::from_millis(2));
    assert!(cache.get_or_prepare("SELECT 1").1);
    cache.get_or_prepare("SELECT 3");
    assert!(cache.get_or_prepare("SELECT 1").1);
    assert!(!cache.get_or_prepare("SELECT 2").1);
}

#[test]
fn reset_stats_preserves_cached_count() {
    let cache = PreparedStatementCache::new();
    cache.enable();

    let sql = "SELECT * FROM users WHERE id = $1";

    let _ = cache.get_or_prepare(sql);
    cache.record_execution(sql, 250);

    let before_reset = cache.stats();
    assert_eq!(before_reset.cached_count, 1);
    assert_eq!(before_reset.misses, 1);
    assert_eq!(before_reset.total_executions, 1);

    cache.reset_stats();

    let after_reset = cache.stats();
    assert_eq!(after_reset.hits, 0);
    assert_eq!(after_reset.misses, 0);
    assert_eq!(after_reset.evictions, 0);
    assert_eq!(after_reset.total_executions, 0);
    assert_eq!(after_reset.cached_count, 1);
}

#[test]
fn zero_max_statements_caches_nothing() {
    let cache = PreparedStatementCache::new();
    cache.enable();
    cache.set_max_statements(0);

    // Before the guard existed this spun forever holding the write lock.
    let (sql, cached) = cache.get_or_prepare("SELECT 1");

    assert_eq!(sql, "SELECT 1");
    assert!(!cached);
    assert!(cache.is_empty());
}

#[test]
fn cached_info_handles_multibyte_sql() {
    let cache = PreparedStatementCache::new();
    cache.enable();

    let sql = format!("SELECT '{}' AS label", "数".repeat(150));
    let _ = cache.get_or_prepare(&sql);

    let info = cache.cached_statements_info();

    assert_eq!(info.len(), 1);
    assert!(info[0].sql_preview.ends_with("..."));
}

#[test]
fn observe_execution_records_real_traffic() {
    let disabled = PreparedStatementCache::new();
    disabled.observe_execution("SELECT 1", 10);
    assert_eq!(disabled.stats().total_executions, 0);

    let cache = PreparedStatementCache::new();
    cache.enable();

    cache.observe_execution("SELECT 1", 120);
    cache.observe_execution("SELECT 1", 80);

    let stats = cache.stats();
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.total_executions, 2);
    assert_eq!(stats.cached_count, 1);
}

#[test]
fn cached_count_follows_a_statement_dropped_without_replacement() {
    let cache = PreparedStatementCache::new();
    cache.enable();
    cache.get_or_prepare("SELECT 1");
    assert_eq!(cache.stats().cached_count, 1);

    // The expired statement is dropped and a zero budget registers nothing in
    // its place; the reported count used to stay at 1 over an empty cache.
    cache.set_max_age(Duration::ZERO);
    cache.set_max_statements(0);
    cache.get_or_prepare("SELECT 1");

    assert!(cache.is_empty());
    assert_eq!(cache.stats().cached_count, 0);
}
