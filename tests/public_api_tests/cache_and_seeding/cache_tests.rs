use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tideorm::cache::{CacheConfig, CacheKeyBuilder, CacheStrategy, QueryCache};

fn enabled_cache(max_entries: usize, strategy: CacheStrategy) -> QueryCache {
    QueryCache::with_config(
        CacheConfig::default()
            .enabled(true)
            .max_entries(max_entries)
            .strategy(strategy),
    )
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
    let cache = QueryCache::with_config(
        CacheConfig::default()
            .enabled(true)
            .max_entries(100)
            .default_ttl(Duration::from_millis(50)),
    );

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
    let prefixed = QueryCache::with_config(CacheConfig::default().enabled(true).key_prefix("v7"));
    assert_eq!(prefixed.generate_key("users", 12345), "v7:users:12345");

    let unprefixed = QueryCache::new();
    assert_eq!(unprefixed.generate_key("posts", 999), "posts:999");
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
