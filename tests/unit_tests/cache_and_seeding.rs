// CACHE MODULE TESTS
// =============================================================================

#[cfg(test)]
mod cache_tests {
    use std::time::Duration;
    use tideorm::cache::{
        CacheKeyBuilder, CacheOptions, CacheStats, CacheStrategy, PreparedStatementCache,
        PreparedStatementStats, QueryCache,
    };

    // =========================================================================
    // QUERY CACHE TESTS
    // =========================================================================

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

        // Disabled by default
        cache.disable();
        cache.set("key", &"value", None, "model").ok();
        let result: Option<String> = cache.get("key");

        // Should return None when disabled
        assert!(result.is_none());

        // Enable and try again
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

        // Set value with short TTL
        cache
            .set(
                "ttl_key",
                &"ttl_value",
                Some(Duration::from_millis(10)),
                "model",
            )
            .unwrap();

        // Should be present immediately
        let result: Option<String> = cache.get("ttl_key");
        assert!(result.is_some());

        // Wait for TTL to expire
        std::thread::sleep(Duration::from_millis(20));

        // Should be expired now
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

        // Fill cache
        cache.set("key1", &1, None, "model").unwrap();
        cache.set("key2", &2, None, "model").unwrap();
        cache.set("key3", &3, None, "model").unwrap();

        assert_eq!(cache.len(), 3);

        // Access key1 to make it recently used
        let _: Option<i32> = cache.get("key1");

        // Add one more, should evict least recently used (key2)
        cache.set("key4", &4, None, "model").unwrap();

        // key2 should be evicted, key1 should still exist
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

        // Fill cache
        cache.set("key1", &1, None, "model").unwrap();
        std::thread::sleep(Duration::from_millis(1));
        cache.set("key2", &2, None, "model").unwrap();
        std::thread::sleep(Duration::from_millis(1));
        cache.set("key3", &3, None, "model").unwrap();

        assert_eq!(cache.len(), 3);

        // Add one more, should evict first in (key1)
        cache.set("key4", &4, None, "model").unwrap();

        // key1 should be evicted (FIFO)
        let result: Option<i32> = cache.get("key1");
        assert!(result.is_none());

        // key2 should still exist
        let result: Option<i32> = cache.get("key2");
        assert!(result.is_some());

        cache.clear();
    }

    #[test]
    fn test_query_cache_complex_types() {
        let cache = QueryCache::new();
        cache.enable();

        // Test with Vec
        let vec_data = vec![1, 2, 3, 4, 5];
        cache.set("vec_key", &vec_data, None, "model").unwrap();
        let result: Option<Vec<i32>> = cache.get("vec_key");
        assert_eq!(result, Some(vec_data));

        // Test with struct
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

        // Generate hits and misses
        cache.set("key", &"value", None, "model").unwrap();
        let _: Option<String> = cache.get("key"); // hit
        let _: Option<String> = cache.get("key"); // hit
        let _: Option<String> = cache.get("missing"); // miss

        let stats = cache.stats();
        assert_eq!(stats.entries, 1);
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert!((stats.hit_ratio() - 0.666).abs() < 0.01);

        // Reset stats
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

        // Add entries with short TTL
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

        // Wait for short TTL to expire
        std::thread::sleep(Duration::from_millis(10));

        // Evict expired entries
        cache.evict_expired();
        assert_eq!(cache.len(), 1);

        cache.clear();
    }

    // =========================================================================
    // PREPARED STATEMENT CACHE TESTS
    // =========================================================================

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

        // Generate hits and misses
        cache.get_or_prepare(sql); // miss
        cache.get_or_prepare(sql); // hit
        cache.get_or_prepare(sql); // hit
        cache.get_or_prepare("SELECT * FROM posts"); // miss

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

        // Record some executions
        cache.record_execution(sql, 100); // 100µs
        cache.record_execution(sql, 200); // 200µs
        cache.record_execution(sql, 300); // 300µs

        let stats = cache.stats();
        assert_eq!(stats.total_executions, 3);

        // Check statement info
        let statements = cache.cached_statements_info();
        assert!(!statements.is_empty());
        let stmt = &statements[0];
        assert_eq!(stmt.execution_count, 3);
        assert_eq!(stmt.avg_execution_time_us, 200); // (100+200+300)/3

        cache.clear();
    }

    #[test]
    fn test_prepared_statement_enabled_disabled() {
        let cache = PreparedStatementCache::new();
        cache.clear();

        // Disable cache
        cache.disable();
        let (_, cached) = cache.get_or_prepare("SELECT 1");
        assert!(!cached);
        assert_eq!(cache.len(), 0);

        // Enable cache
        cache.enable();
        cache.get_or_prepare("SELECT 1");
        let (_, cached) = cache.get_or_prepare("SELECT 1");
        assert!(cached);

        cache.clear();
    }

    // =========================================================================
    // CACHE KEY BUILDER TESTS
    // =========================================================================

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

        // Same inputs should produce same hash
        assert_eq!(hash1, hash2);

        let hash3 = CacheKeyBuilder::new()
            .table("users")
            .condition("id", 2)
            .build_hash();

        // Different inputs should produce different hash
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_cache_key_builder_deterministic() {
        // Same conditions in same order should produce same key
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

    // =========================================================================
    // CACHE OPTIONS TESTS
    // =========================================================================

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

    // =========================================================================
    // CACHE CONFIG TESTS
    // =========================================================================

    // =========================================================================
    // CACHE STATS TESTS
    // =========================================================================

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

    // =========================================================================
    // GLOBAL CACHE TESTS
    // =========================================================================

    #[test]
    fn test_global_query_cache() {
        // Test global cache singleton
        let cache1 = QueryCache::global();
        let cache2 = QueryCache::global();

        cache1.enable();
        cache1.set("global_test", &42, None, "test").unwrap();

        // Both should refer to the same cache
        let result: Option<i32> = cache2.get("global_test");
        assert_eq!(result, Some(42));

        cache1.clear();
    }

    #[test]
    fn test_global_prepared_statement_cache() {
        // Test global prepared statement cache singleton
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

    // =========================================================================
    // THREAD SAFETY TESTS
    // =========================================================================

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
}

// =============================================================================
// SEEDING TESTS
// =============================================================================

mod seeding_tests {
    use tideorm::seeding::{SeedInfo, SeedResult, SeedStatus};

    #[test]
    fn test_seed_result_has_rolled_back() {
        let result = SeedResult {
            executed: Vec::new(),
            skipped: Vec::new(),
            rolled_back: vec![SeedInfo {
                name: "test_seed".to_string(),
            }],
        };
        assert!(!result.has_executed());
        assert!(result.has_rolled_back());
    }

    #[test]
    fn test_seed_result_display_rolled_back() {
        let result = SeedResult {
            executed: Vec::new(),
            skipped: Vec::new(),
            rolled_back: vec![SeedInfo {
                name: "test_seeder".to_string(),
            }],
        };
        let display = format!("{}", result);
        assert!(display.contains("test_seeder"));
        assert!(display.contains("Rolled back seeds"));
    }

    #[test]
    fn test_seed_info_clone() {
        let info = SeedInfo {
            name: "test_seed".to_string(),
        };
        let cloned = info.clone();
        assert_eq!(info.name, cloned.name);
    }

    #[test]
    fn test_seed_status_clone() {
        let status = SeedStatus {
            name: "test".to_string(),
            executed: true,
            priority: 10,
        };
        let cloned = status.clone();
        assert_eq!(status.name, cloned.name);
        assert_eq!(status.executed, cloned.executed);
        assert_eq!(status.priority, cloned.priority);
    }

    #[test]
    fn test_seed_result_clone() {
        let result = SeedResult {
            executed: vec![SeedInfo {
                name: "s1".to_string(),
            }],
            skipped: vec![SeedInfo {
                name: "s2".to_string(),
            }],
            rolled_back: vec![SeedInfo {
                name: "s3".to_string(),
            }],
        };
        let cloned = result.clone();
        assert_eq!(result.executed.len(), cloned.executed.len());
        assert_eq!(result.skipped.len(), cloned.skipped.len());
        assert_eq!(result.rolled_back.len(), cloned.rolled_back.len());
    }

    #[test]
    fn test_seed_info_debug() {
        let info = SeedInfo {
            name: "test_seed".to_string(),
        };
        let debug = format!("{:?}", info);
        assert!(debug.contains("test_seed"));
    }

    #[test]
    fn test_seed_status_debug() {
        let status = SeedStatus {
            name: "test".to_string(),
            executed: true,
            priority: 100,
        };
        let debug = format!("{:?}", status);
        assert!(debug.contains("test"));
        assert!(debug.contains("true"));
        assert!(debug.contains("100"));
    }

    #[test]
    fn test_seed_result_empty_display() {
        let result = SeedResult {
            executed: Vec::new(),
            skipped: Vec::new(),
            rolled_back: Vec::new(),
        };
        let display = format!("{}", result);
        assert!(display.is_empty() || !display.contains("Executed"));
    }

    #[test]
    fn test_seed_result_multiple_seeds() {
        let result = SeedResult {
            executed: vec![
                SeedInfo {
                    name: "seed1".to_string(),
                },
                SeedInfo {
                    name: "seed2".to_string(),
                },
                SeedInfo {
                    name: "seed3".to_string(),
                },
            ],
            skipped: Vec::new(),
            rolled_back: Vec::new(),
        };
        assert_eq!(result.total(), 3);
        assert!(result.has_executed());
    }

    #[test]
    fn test_seed_status_high_priority() {
        let status = SeedStatus {
            name: "critical_seeder".to_string(),
            executed: false,
            priority: 1,
        };
        assert_eq!(status.priority, 1);
    }

    #[test]
    fn test_seed_status_low_priority() {
        let status = SeedStatus {
            name: "optional_seeder".to_string(),
            executed: false,
            priority: 1000,
        };
        assert_eq!(status.priority, 1000);
    }
}
