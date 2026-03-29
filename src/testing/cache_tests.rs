use super::*;
use std::thread;

#[test]
fn test_cache_config_default() {
    let config = CacheConfig::default();
    assert!(!config.enabled);
    assert_eq!(config.max_entries, 1000);
    assert_eq!(config.default_ttl, Duration::from_secs(60));
}

#[test]
fn test_query_cache_basic() {
    let cache = QueryCache::new();
    cache.enable();

    assert!(cache.is_enabled());
    assert!(cache.is_empty());

    cache
        .set("test_key", &vec![1, 2, 3], None, "test_model")
        .unwrap();

    assert!(!cache.is_empty());
    assert_eq!(cache.len(), 1);

    let result: Option<Vec<i32>> = cache.get("test_key");
    assert_eq!(result, Some(vec![1, 2, 3]));

    let stats = cache.stats();
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.entries, 1);
}

#[test]
fn test_query_cache_respects_empty_results_toggle_for_empty_vectors() {
    let cache = QueryCache::new();
    cache.enable();
    cache.set_cache_empty_results(false);

    cache.set("empty_vec", &Vec::<i32>::new(), None, "test_model").unwrap();

    let result: Option<Vec<i32>> = cache.get("empty_vec");
    assert!(result.is_none());
    assert_eq!(cache.len(), 0);
}

#[test]
fn test_query_cache_invalidation() {
    let cache = QueryCache::new();
    cache.enable();

    cache.set("key1", &"value1", None, "model1").unwrap();
    cache.set("key2", &"value2", None, "model1").unwrap();
    cache.set("key3", &"value3", None, "model2").unwrap();

    assert_eq!(cache.len(), 3);

    cache.invalidate("key1");
    assert_eq!(cache.len(), 2);

    cache.invalidate_model("model1");
    assert_eq!(cache.len(), 1);

    cache.clear();
    assert!(cache.is_empty());
}

#[test]
fn test_query_cache_stats_track_replacements_and_size() {
    let cache = QueryCache::new();
    cache.enable();

    cache.set("key", &"a", None, "model").unwrap();
    let initial = cache.stats();
    assert_eq!(initial.entries, 1);
    assert!(initial.size_bytes > 0);

    cache
        .set(
            "key",
            &"this is a much longer replacement value",
            None,
            "model",
        )
        .unwrap();
    let replaced = cache.stats();
    assert_eq!(replaced.entries, 1);
    assert!(
        replaced.size_bytes > initial.size_bytes,
        "replacement should update tracked cache size"
    );

    assert!(cache.invalidate("key"));
    let invalidated = cache.stats();
    assert_eq!(invalidated.entries, 0);
    assert_eq!(invalidated.size_bytes, 0);
    assert_eq!(invalidated.invalidations, 1);
}

#[test]
fn test_query_cache_reset_stats_preserves_live_cache_state() {
    let cache = QueryCache::new();
    cache.enable();

    cache.set("key1", &"value1", None, "model").unwrap();
    cache.set("key2", &"value2", None, "model").unwrap();

    let _: Option<String> = cache.get("key1");
    let _: Option<String> = cache.get("missing_key");

    let before_reset = cache.stats();
    assert_eq!(before_reset.entries, 2);
    assert_eq!(before_reset.hits, 1);
    assert_eq!(before_reset.misses, 1);
    assert!(before_reset.size_bytes > 0);

    cache.reset_stats();

    let after_reset = cache.stats();
    assert_eq!(after_reset.hits, 0);
    assert_eq!(after_reset.misses, 0);
    assert_eq!(after_reset.evictions, 0);
    assert_eq!(after_reset.invalidations, 0);
    assert_eq!(after_reset.entries, 2);
    assert!(after_reset.size_bytes > 0);
}

#[test]
fn test_query_cache_clear_resets_entries_and_size_bytes() {
    let cache = QueryCache::new();
    cache.enable();

    cache.set("key1", &vec![1, 2, 3], None, "model").unwrap();
    cache.set("key2", &vec![4, 5, 6], None, "model").unwrap();
    assert_eq!(cache.stats().entries, 2);

    cache.clear();

    let stats = cache.stats();
    assert_eq!(stats.entries, 0);
    assert_eq!(stats.size_bytes, 0);
    assert!(cache.is_empty());
}

#[test]
fn test_query_cache_get_updates_lru_recency() {
    let cache = QueryCache::new();
    cache.enable();
    cache.set_max_entries(2);
    cache.set_strategy(CacheStrategy::LRU);

    cache.set("key1", &"value1", None, "model").unwrap();
    thread::sleep(Duration::from_millis(2));
    cache.set("key2", &"value2", None, "model").unwrap();

    let value: Option<String> = cache.get("key1");
    assert_eq!(value.as_deref(), Some("value1"));

    cache.set("key3", &"value3", None, "model").unwrap();

    assert!(
        cache.contains("key1"),
        "recently read LRU entry should stay cached"
    );
    assert!(
        !cache.contains("key2"),
        "older untouched LRU entry should be evicted"
    );
    assert!(cache.contains("key3"));
}

#[test]
fn test_query_cache_get_removes_expired_entries() {
    let cache = QueryCache::new();
    cache.enable();

    cache
        .set(
            "short_lived",
            &"value",
            Some(Duration::from_millis(1)),
            "model",
        )
        .unwrap();

    thread::sleep(Duration::from_millis(5));

    let value: Option<String> = cache.get("short_lived");
    assert!(value.is_none());
    assert!(!cache.contains("short_lived"));

    let stats = cache.stats();
    assert_eq!(stats.entries, 0);
    assert_eq!(stats.misses, 1);
}

#[test]
fn test_prepared_statement_cache() {
    let cache = PreparedStatementCache::new();
    cache.enable();

    let sql = "SELECT * FROM users WHERE id = $1";

    let (_, cached) = cache.get_or_prepare(sql);
    assert!(!cached);

    let (_, cached) = cache.get_or_prepare(sql);
    assert!(cached);

    let stats = cache.stats();
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, 1);
}

#[test]
fn test_cache_key_builder() {
    let key = CacheKeyBuilder::new()
        .table("users")
        .condition("active", true)
        .condition("role", "admin")
        .order("created_at", "desc")
        .limit(10)
        .build();

    assert!(key.contains("t:users"));
    assert!(key.contains("active=true"));
    assert!(key.contains("role=admin"));
    assert!(key.contains("o:created_at:desc"));
    assert!(key.contains("l:10"));
}

#[test]
fn test_cache_stats_hit_ratio() {
    let mut stats = CacheStats::default();
    assert_eq!(stats.hit_ratio(), 0.0);

    stats.hits = 75;
    stats.misses = 25;
    assert!((stats.hit_ratio() - 0.75).abs() < 0.001);
}

#[test]
fn test_cache_strategy_display() {
    assert_eq!(format!("{}", CacheStrategy::LRU), "LRU");
    assert_eq!(format!("{}", CacheStrategy::FIFO), "FIFO");
    assert_eq!(format!("{}", CacheStrategy::TTL), "TTL");
}
