use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tideorm::cache::{
    CacheConfig, CacheKeyBuilder, CacheStrategy, PreparedStatementCache, QueryCache,
};

fn enabled_cache(max_entries: usize, strategy: CacheStrategy) -> QueryCache {
    QueryCache::with_config(CacheConfig {
        enabled: true,
        max_entries,
        strategy,
        ..CacheConfig::default()
    })
}

#[test]
fn test_query_cache_miss() {
    let cache = QueryCache::new();
    cache.enable();

    let result: Option<String> = cache.get("nonexistent_key");
    assert!(result.is_none());

    let stats = cache.stats();
    assert_eq!(stats.misses, 1);
}

#[test]
fn test_query_cache_enabled_disabled() {
    let cache = QueryCache::new();

    cache.disable();
    cache.set("key", &"value", None, "model").ok();
    let result: Option<String> = cache.get("key");

    assert!(result.is_none());

    cache.enable();
    cache.set("key", &"value", None, "model").unwrap();
    let result: Option<String> = cache.get("key");
    assert!(result.is_some());
}

#[test]
fn test_query_cache_entry_expires_after_default_ttl() {
    let cache = QueryCache::with_config(CacheConfig {
        enabled: true,
        max_entries: 100,
        default_ttl: Duration::from_millis(50),
        ..CacheConfig::default()
    });

    cache
        .set::<String>("k", &"value".to_string(), None, "test")
        .unwrap();
    assert!(cache.get::<String>("k").is_some());

    thread::sleep(Duration::from_millis(80));
    assert!(cache.get::<String>("k").is_none());
}

#[test]
fn test_query_cache_fifo_evicts_oldest_insert_even_when_recently_read() {
    let cache = enabled_cache(2, CacheStrategy::FIFO);

    cache.set::<i32>("a", &1, None, "t").unwrap();
    cache.set::<i32>("b", &2, None, "t").unwrap();
    // A read would save "a" under LRU; FIFO must ignore it.
    let _: Option<i32> = cache.get("a");

    cache.set::<i32>("c", &3, None, "t").unwrap();
    assert_eq!(cache.len(), 2);
    assert!(!cache.contains("a"));
    assert!(cache.contains("b"));
    assert!(cache.contains("c"));
}

#[test]
fn test_query_cache_replacing_existing_key_at_capacity_does_not_evict_other_entry() {
    let cache = QueryCache::new();
    cache.enable();
    cache.set_max_entries(2);
    cache.set_strategy(CacheStrategy::LRU);

    cache.set("key1", &1, None, "model").unwrap();
    cache.set("key2", &2, None, "model").unwrap();

    cache.set("key1", &10, None, "model").unwrap();

    assert_eq!(cache.len(), 2);
    assert_eq!(cache.get::<i32>("key1"), Some(10));
    assert_eq!(cache.get::<i32>("key2"), Some(2));
}

#[test]
fn test_query_cache_stats_count_hits_misses_and_evictions() {
    let cache = enabled_cache(2, CacheStrategy::LRU);

    let _: Option<i32> = cache.get("nope");
    cache.set::<i32>("a", &1, None, "t").unwrap();
    let _: Option<i32> = cache.get("a");
    cache.set::<i32>("b", &2, None, "t").unwrap();
    cache.set::<i32>("c", &3, None, "t").unwrap();

    let stats = cache.stats();
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.evictions, 1);
    assert_eq!(stats.entries, 2);
}

#[test]
fn test_query_cache_evict_expired() {
    let cache = QueryCache::new();
    cache.enable();
    cache.set_default_ttl(Duration::from_millis(10));
    cache.set_strategy(CacheStrategy::TTL);

    cache
        .set("key1", &1, Some(Duration::from_millis(5)), "model")
        .unwrap();
    cache
        .set("key2", &2, Some(Duration::from_millis(5)), "model")
        .unwrap();
    cache
        .set("key3", &3, Some(Duration::from_secs(60)), "model")
        .unwrap();

    assert_eq!(cache.len(), 3);

    std::thread::sleep(Duration::from_millis(10));

    cache.evict_expired();
    assert_eq!(cache.len(), 1);
}

#[test]
fn test_query_cache_concurrent_writers_respect_max_entries() {
    let cache = Arc::new(enabled_cache(100, CacheStrategy::LRU));

    let mut handles = Vec::new();
    for i in 0..25 {
        let c = Arc::clone(&cache);
        handles.push(thread::spawn(move || {
            for j in 0..20 {
                let key = format!("w{}_{}", i, j);
                c.set::<i32>(&key, &(i * 100 + j), None, "stress").unwrap();
            }
        }));
    }
    for i in 0..25 {
        let c = Arc::clone(&cache);
        handles.push(thread::spawn(move || {
            for j in 0..20 {
                let key = format!("w{}_{}", i, j);
                let _: Option<i32> = c.get(&key);
            }
        }));
    }

    for h in handles {
        h.join().expect("thread should not panic");
    }

    assert!(
        cache.stats().entries <= 100,
        "should not exceed max_entries"
    );
}

#[test]
fn test_query_cache_generate_key_applies_prefix() {
    let prefixed = QueryCache::with_config(CacheConfig {
        enabled: true,
        key_prefix: Some("v7".into()),
        ..CacheConfig::default()
    });
    assert_eq!(prefixed.generate_key("users", 12345), "v7:users:12345");

    let unprefixed = QueryCache::new();
    assert_eq!(unprefixed.generate_key("posts", 999), "posts:999");
}

#[test]
fn test_prepared_statement_cache_stats() {
    let cache = PreparedStatementCache::new();
    cache.enable();

    let sql = "SELECT * FROM users";

    cache.get_or_prepare(sql);
    cache.get_or_prepare(sql);
    cache.get_or_prepare(sql);
    cache.get_or_prepare("SELECT * FROM posts");

    let stats = cache.stats();
    assert_eq!(stats.cached_count, 2);
    assert_eq!(stats.hits, 2);
    assert_eq!(stats.misses, 2);
    assert!((stats.hit_ratio() - 0.5).abs() < 0.01);
}

#[test]
fn test_prepared_statement_record_execution() {
    let cache = PreparedStatementCache::new();
    cache.enable();

    let sql = "SELECT * FROM users WHERE id = $1";
    cache.get_or_prepare(sql);

    cache.record_execution(sql, 100);
    cache.record_execution(sql, 200);
    cache.record_execution(sql, 300);

    let stats = cache.stats();
    assert_eq!(stats.total_executions, 3);

    let statements = cache.cached_statements_info();
    assert!(!statements.is_empty());
    let stmt = &statements[0];
    assert_eq!(stmt.execution_count, 3);
    assert_eq!(stmt.avg_execution_time_us, 200);
}

#[test]
fn test_prepared_statement_enabled_disabled() {
    let cache = PreparedStatementCache::new();

    cache.disable();
    let (_, cached) = cache.get_or_prepare("SELECT 1");
    assert!(!cached);
    assert_eq!(cache.len(), 0);

    cache.enable();
    cache.get_or_prepare("SELECT 1");
    let (_, cached) = cache.get_or_prepare("SELECT 1");
    assert!(cached);
}

#[test]
fn test_cache_key_builder_hash() {
    let hash1 = CacheKeyBuilder::new()
        .table("users")
        .condition("id", 1)
        .build_hash();

    let hash2 = CacheKeyBuilder::new()
        .table("users")
        .condition("id", 1)
        .build_hash();

    assert_eq!(hash1, hash2);

    let hash3 = CacheKeyBuilder::new()
        .table("users")
        .condition("id", 2)
        .build_hash();

    assert_ne!(hash1, hash3);
}

#[test]
fn test_global_query_cache() {
    let cache1 = QueryCache::global();
    let cache2 = QueryCache::global();

    cache1.enable();
    cache1.set("global_test", &42, None, "test").unwrap();

    let result: Option<i32> = cache2.get("global_test");
    assert_eq!(result, Some(42));

    cache1.clear();
}

#[test]
fn test_global_prepared_statement_cache() {
    let cache1 = PreparedStatementCache::global();
    let cache2 = PreparedStatementCache::global();

    cache1.enable();
    cache1.clear();

    let (sql1, _) = cache1.get_or_prepare("SELECT * FROM global_test");
    let (sql2, cached) = cache2.get_or_prepare("SELECT * FROM global_test");

    assert_eq!(sql1, sql2);
    assert!(cached);

    cache1.clear();
}
