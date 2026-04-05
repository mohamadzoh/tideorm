// CACHE MODULE TESTS
// =============================================================================

use std::time::Duration;
use tideorm::cache::{
    CacheKeyBuilder, CacheOptions, CacheStats, CacheStrategy, PreparedStatementCache,
    PreparedStatementStats, QueryCache,
};

#[test]
fn test_query_cache_miss() {
    let cache = QueryCache::new();
    cache.enable();

    let result: Option<String> = cache.get("nonexistent_key");
    assert!(result.is_none());

    let stats = cache.stats();
    assert_eq!(stats.misses, 1);

    cache.clear();
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

    cache.clear();
}

#[test]
fn test_query_cache_ttl() {
    let cache = QueryCache::new();
    cache.enable();
    cache.set_default_ttl(Duration::from_millis(50));
    cache.set_strategy(CacheStrategy::TTL);

    cache
        .set(
            "ttl_key",
            &"ttl_value",
            Some(Duration::from_millis(10)),
            "model",
        )
        .unwrap();

    let result: Option<String> = cache.get("ttl_key");
    assert!(result.is_some());

    std::thread::sleep(Duration::from_millis(20));

    let result: Option<String> = cache.get("ttl_key");
    assert!(result.is_none());

    cache.clear();
}

#[test]
fn test_query_cache_max_entries_lru() {
    let cache = QueryCache::new();
    cache.enable();
    cache.set_max_entries(3);
    cache.set_strategy(CacheStrategy::LRU);

    cache.set("key1", &1, None, "model").unwrap();
    cache.set("key2", &2, None, "model").unwrap();
    cache.set("key3", &3, None, "model").unwrap();

    assert_eq!(cache.len(), 3);

    let _: Option<i32> = cache.get("key1");

    cache.set("key4", &4, None, "model").unwrap();

    let result: Option<i32> = cache.get("key2");
    assert!(result.is_none());

    let result: Option<i32> = cache.get("key1");
    assert!(result.is_some());

    cache.clear();
}

#[test]
fn test_query_cache_max_entries_fifo() {
    let cache = QueryCache::new();
    cache.enable();
    cache.set_max_entries(3);
    cache.set_strategy(CacheStrategy::FIFO);

    cache.set("key1", &1, None, "model").unwrap();
    std::thread::sleep(Duration::from_millis(1));
    cache.set("key2", &2, None, "model").unwrap();
    std::thread::sleep(Duration::from_millis(1));
    cache.set("key3", &3, None, "model").unwrap();

    assert_eq!(cache.len(), 3);

    cache.set("key4", &4, None, "model").unwrap();

    let result: Option<i32> = cache.get("key1");
    assert!(result.is_none());

    let result: Option<i32> = cache.get("key2");
    assert!(result.is_some());

    cache.clear();
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

    cache.clear();
}

#[test]
fn test_query_cache_complex_types() {
    let cache = QueryCache::new();
    cache.enable();

    let vec_data = vec![1, 2, 3, 4, 5];
    cache.set("vec_key", &vec_data, None, "model").unwrap();
    let result: Option<Vec<i32>> = cache.get("vec_key");
    assert_eq!(result, Some(vec_data));

    #[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
    struct TestStruct {
        name: String,
        value: i32,
    }

    let struct_data = TestStruct {
        name: "test".to_string(),
        value: 42,
    };
    cache
        .set("struct_key", &struct_data, None, "model")
        .unwrap();
    let result: Option<TestStruct> = cache.get("struct_key");
    assert_eq!(
        result,
        Some(TestStruct {
            name: "test".to_string(),
            value: 42
        })
    );

    cache.clear();
}

#[test]
fn test_query_cache_stats() {
    let cache = QueryCache::new();
    cache.enable();
    cache.reset_stats();

    cache.set("key", &"value", None, "model").unwrap();
    let _: Option<String> = cache.get("key");
    let _: Option<String> = cache.get("key");
    let _: Option<String> = cache.get("missing");

    let stats = cache.stats();
    assert_eq!(stats.entries, 1);
    assert_eq!(stats.hits, 2);
    assert_eq!(stats.misses, 1);
    assert!((stats.hit_ratio() - 0.666).abs() < 0.01);

    cache.reset_stats();
    let stats = cache.stats();
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.misses, 0);

    cache.clear();
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

    cache.clear();
}

#[test]
fn test_prepared_statement_cache_different_queries() {
    let cache = PreparedStatementCache::new();
    cache.enable();
    cache.clear();

    let sql1 = "SELECT * FROM users WHERE id = $1";
    let sql2 = "SELECT * FROM posts WHERE user_id = $1";

    cache.get_or_prepare(sql1);
    cache.get_or_prepare(sql2);

    assert_eq!(cache.len(), 2);

    cache.clear();
}

#[test]
fn test_prepared_statement_cache_stats() {
    let cache = PreparedStatementCache::new();
    cache.enable();
    cache.clear();
    cache.reset_stats();

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

    cache.clear();
}

#[test]
fn test_prepared_statement_record_execution() {
    let cache = PreparedStatementCache::new();
    cache.enable();
    cache.clear();

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

    cache.clear();
}

#[test]
fn test_prepared_statement_enabled_disabled() {
    let cache = PreparedStatementCache::new();
    cache.clear();

    cache.disable();
    let (_, cached) = cache.get_or_prepare("SELECT 1");
    assert!(!cached);
    assert_eq!(cache.len(), 0);

    cache.enable();
    cache.get_or_prepare("SELECT 1");
    let (_, cached) = cache.get_or_prepare("SELECT 1");
    assert!(cached);

    cache.clear();
}

#[test]
fn test_cache_key_builder_basic() {
    let key = CacheKeyBuilder::new().table("users").build();

    assert!(key.contains("users"));
}

#[test]
fn test_cache_key_builder_with_conditions() {
    let key = CacheKeyBuilder::new()
        .table("users")
        .condition("active", true)
        .condition("role", "admin")
        .build();

    assert!(key.contains("users"));
    assert!(key.contains("active"));
    assert!(key.contains("role"));
}

#[test]
fn test_cache_key_builder_with_order_and_limit() {
    let key = CacheKeyBuilder::new()
        .table("posts")
        .order("created_at", "desc")
        .limit(10)
        .offset(20)
        .build();

    assert!(key.contains("posts"));
    assert!(key.contains("created_at"));
    assert!(key.contains("desc"));
    assert!(key.contains("10"));
    assert!(key.contains("20"));
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
fn test_cache_key_builder_deterministic() {
    let key1 = CacheKeyBuilder::new()
        .table("users")
        .condition("a", 1)
        .condition("b", 2)
        .build();

    let key2 = CacheKeyBuilder::new()
        .table("users")
        .condition("a", 1)
        .condition("b", 2)
        .build();

    assert_eq!(key1, key2);
}

#[test]
fn test_cache_options_creation() {
    let options = CacheOptions::new(Duration::from_secs(300));
    assert_eq!(options.ttl, Duration::from_secs(300));
    assert!(options.key.is_none());
    assert!(options.tags.is_empty());
}

#[test]
fn test_cache_options_with_key() {
    let options = CacheOptions::new(Duration::from_secs(300)).with_key("my_custom_key");

    assert_eq!(options.key, Some("my_custom_key".to_string()));
}

#[test]
fn test_cache_options_with_tags() {
    let options =
        CacheOptions::new(Duration::from_secs(300)).with_tags(&["users", "active", "premium"]);

    assert_eq!(options.tags.len(), 3);
    assert!(options.tags.contains(&"users".to_string()));
    assert!(options.tags.contains(&"active".to_string()));
    assert!(options.tags.contains(&"premium".to_string()));
}

#[test]
fn test_cache_options_chaining() {
    let options = CacheOptions::new(Duration::from_secs(600))
        .with_key("featured_products")
        .with_tags(&["products", "featured"]);

    assert_eq!(options.ttl, Duration::from_secs(600));
    assert_eq!(options.key, Some("featured_products".to_string()));
    assert_eq!(options.tags.len(), 2);
}

#[test]
fn test_cache_stats_hit_ratio_zero_requests() {
    let stats = CacheStats {
        entries: 0,
        size_bytes: 0,
        hits: 0,
        misses: 0,
        evictions: 0,
        invalidations: 0,
    };

    assert_eq!(stats.hit_ratio(), 0.0);
}

#[test]
fn test_prepared_statement_stats_hit_ratio() {
    let stats = PreparedStatementStats {
        cached_count: 50,
        hits: 100,
        misses: 50,
        total_executions: 200,
        evictions: 0,
    };

    assert!((stats.hit_ratio() - 0.666).abs() < 0.01);
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

#[test]
fn test_query_cache_thread_safety() {
    use std::thread;

    let cache = QueryCache::new();
    cache.enable();

    let handles: Vec<_> = (0..10)
        .map(|i| {
            let cache_ref = QueryCache::global();
            thread::spawn(move || {
                let key = format!("thread_key_{}", i);
                cache_ref.set(&key, &i, None, "test").ok();
                let _: Option<i32> = cache_ref.get(&key);
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    QueryCache::global().clear();
}

#[test]
fn test_prepared_statement_cache_thread_safety() {
    use std::thread;

    let cache = PreparedStatementCache::global();
    cache.enable();
    cache.clear();

    let handles: Vec<_> = (0..10)
        .map(|i| {
            let cache_ref = PreparedStatementCache::global();
            thread::spawn(move || {
                let sql = format!("SELECT * FROM table_{}", i);
                cache_ref.get_or_prepare(&sql);
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    cache.clear();
}
