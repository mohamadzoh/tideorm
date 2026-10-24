use super::*;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[test]
fn database_invalidation_protects_other_scopes_and_rejects_stale_fills() {
    let cache = QueryCache::new();
    cache.enable();
    let a: Arc<str> = "database-a".into();
    let b: Arc<str> = "database-b".into();
    let a_read = cache.fill_point_for_database(Some(a.clone()));
    let b_read = cache.fill_point_for_database(Some(b.clone()));
    let manual = cache.fill_point();
    cache.set("manual", &1, None, "users").unwrap();
    cache.invalidate_database(a);
    for (point, key) in [(a_read, "a"), (b_read, "b"), (manual, "unknown")] {
        cache
            .fill_tagged(point, key, &1, None, &["users".into()])
            .unwrap();
    }
    assert_eq!(cache.get::<i32>("a"), None);
    assert_eq!(cache.get::<i32>("unknown"), None);
    assert_eq!(cache.get::<i32>("manual"), None);
    assert_eq!(cache.get::<i32>("b"), Some(1));
    cache.invalidate_database(b);
    assert_eq!(cache.get::<i32>("b"), None);
}

#[test]
fn table_invalidation_folds_case_in_entries_and_inflight_reads() {
    let cache = QueryCache::new();
    cache.enable();
    let point = cache.fill_point();
    cache.set("before", &1, None, "POSTS").unwrap();
    cache.invalidate_model("Posts");
    cache
        .fill_tagged(point, "after", &1, None, &["POSTS".into()])
        .unwrap();
    assert!(cache.is_empty());
}

#[test]
fn database_history_compaction_rejects_old_fills_without_evicting_live_entries() {
    let cache = QueryCache::new();
    cache.enable();
    let stable = cache.fill_point_for_database(Some("stable".into()));
    cache
        .fill_tagged(stable, "stable", &1, None, &["users".into()])
        .unwrap();
    let old = cache.fill_point_for_database(Some("old".into()));
    cache.invalidate_database("old".into());
    for index in 0..1500 {
        cache.invalidate_database(format!("temporary-{index}").into());
    }
    cache
        .fill_tagged(old, "stale", &1, None, &["users".into()])
        .unwrap();
    assert_eq!(cache.get::<i32>("stale"), None);
    assert_eq!(cache.get::<i32>("stable"), Some(1));
}

#[test]
fn test_cache_config_default() {
    let config = CacheConfig::default();
    assert!(!config.enabled);
    assert_eq!(config.max_entries, 1000);
    assert_eq!(config.default_ttl, Duration::from_secs(60));
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
fn test_query_cache_invalidates_on_any_tagged_table() {
    let cache = QueryCache::new();
    cache.enable();

    cache
        .set_tagged(
            "joined",
            &"value",
            None,
            &["posts".to_string(), "users".to_string()],
        )
        .unwrap();
    cache.set("posts_only", &"value", None, "posts").unwrap();

    cache.invalidate_model("users");

    assert!(
        !cache.contains("joined"),
        "a write to a joined table must evict the cached join result"
    );
    assert!(cache.contains("posts_only"));
}

#[test]
fn test_query_cache_enforces_max_size_bytes() {
    let cache = QueryCache::new();
    cache.enable();
    cache.set_strategy(CacheStrategy::FIFO);
    cache.set_max_size_bytes(40);

    cache.set("first", &"a".repeat(20), None, "model").unwrap();
    cache.set("second", &"b".repeat(20), None, "model").unwrap();

    assert!(cache.stats().size_bytes <= 40);
    assert!(!cache.contains("first"));
    assert!(cache.contains("second"));

    cache.set("huge", &"c".repeat(200), None, "model").unwrap();
    assert!(
        !cache.contains("huge"),
        "a payload larger than the whole budget is never cached"
    );
}

#[test]
fn test_query_cache_byte_budget_is_part_of_the_configuration() {
    let cache = QueryCache::new();
    cache.set_max_size_bytes(64);
    assert_eq!(cache.config().max_size_bytes, 64);

    cache.apply_config(CacheConfig {
        enabled: true,
        max_size_bytes: 10,
        ..CacheConfig::default()
    });
    cache.set("fits", &"x", None, "model").unwrap();
    cache
        .set("too_big", &"x".repeat(20), None, "model")
        .unwrap();
    assert!(cache.contains("fits"));
    assert!(
        !cache.contains("too_big"),
        "the configured byte budget must apply"
    );
}

#[test]
fn test_query_cache_stats_follow_evictions_and_expiry() {
    let cache = QueryCache::new();
    cache.enable();
    cache.set_strategy(CacheStrategy::FIFO);
    cache.set_max_entries(2);

    cache.set("first", &"a", None, "model").unwrap();
    cache.set("second", &"b", None, "model").unwrap();
    cache.set("third", &"c", None, "model").unwrap();
    cache
        .set("expiring", &"d", Some(Duration::from_millis(1)), "model")
        .unwrap();
    thread::sleep(Duration::from_millis(5));
    cache.evict_expired();

    let stats = cache.stats();
    assert_eq!(stats.entries, cache.len());
    assert_eq!(stats.entries, 1);
    assert_eq!(stats.evictions, 3);
    assert_eq!(stats.size_bytes, serde_json::to_vec(&"c").unwrap().len());
    assert!(!cache.contains("first"));
    assert!(cache.contains("third"));
}

#[test]
fn init_global_applies_config_after_a_default_cache_was_installed() {
    // Reaching for the global cache is what a macro-generated model write
    // does; it installs the disabled default that `init_global` used to be
    // unable to replace.
    let previous = QueryCache::global().config();

    let installed = QueryCache::init_global(CacheConfig {
        enabled: true,
        max_entries: 17,
        key_prefix: Some("late-init".to_string()),
        ..CacheConfig::default()
    });

    assert!(
        installed.is_enabled(),
        "a late init_global must apply instead of being silently dropped"
    );
    let config = installed.config();
    assert_eq!(config.max_entries, 17);
    assert_eq!(config.key_prefix.as_deref(), Some("late-init"));

    // Leave the process-wide cache as it was found.
    QueryCache::init_global(previous);
}

/// A read that started before a write to one of its tables is not stored: its
/// rows may predate the write and would outlive it for the whole TTL.
#[test]
fn a_fill_that_started_before_an_invalidation_is_dropped() {
    let cache = QueryCache::new();
    cache.enable();
    let tables = vec!["orders".to_string()];

    let point = cache.fill_point();
    cache.invalidate_model("orders");
    cache
        .fill_tagged(point, "k", &vec![1], None, &tables)
        .unwrap();
    assert_eq!(cache.get::<Vec<i32>>("k"), None);

    // A write to a table the read did not touch leaves the fill alone.
    let point = cache.fill_point();
    cache.invalidate_model("customers");
    cache
        .fill_tagged(point, "k", &vec![2], None, &tables)
        .unwrap();
    assert_eq!(cache.get::<Vec<i32>>("k"), Some(vec![2]));

    let point = cache.fill_point();
    cache.clear();
    cache
        .fill_tagged(point, "k", &vec![3], None, &tables)
        .unwrap();
    assert_eq!(cache.get::<Vec<i32>>("k"), None);
}

/// Entries other requests cache while a transaction is open are dropped when
/// it commits, a savepoint's writes included: its commit hands them to the
/// enclosing transaction, which replays them at its own commit.
#[test]
fn a_transaction_replays_its_invalidations_when_it_commits() {
    let previous = QueryCache::global().config();
    let cache = QueryCache::init_global(CacheConfig {
        enabled: true,
        ..CacheConfig::default()
    });
    let tables = vec!["replayed_rows".to_string()];
    let cached = || cache.get::<Vec<i32>>("replayed").is_some();
    let fill = || {
        cache
            .set_tagged("replayed", &vec![1], None, &tables)
            .unwrap()
    };

    let outer = Arc::new(parking_lot::Mutex::new(PendingInvalidations::default()));
    let savepoint = Arc::new(parking_lot::Mutex::new(PendingInvalidations::new(Some(
        outer.clone(),
    ))));
    {
        let _outer = install_pending_invalidations(&outer);
        {
            let _savepoint = install_pending_invalidations(&savepoint);
            cache.invalidate_model("replayed_rows");
        }
        // Another request caches the committed rows meanwhile.
        fill();
        std::mem::take(&mut *savepoint.lock()).replay();
        assert!(!cached());
        fill();
    }
    assert!(cached(), "only the outer commit replays what is left");
    std::mem::take(&mut *outer.lock()).replay();
    assert!(!cached());

    QueryCache::init_global(previous);
}

#[test]
fn test_cache_key_builder() {
    let key = CacheKeyBuilder::new()
        .table("users")
        .condition("active", true)
        .condition("role", "admin")
        .order("created_at", "desc")
        .limit(10)
        .offset(20)
        .build();

    assert!(key.contains("t:users"));
    assert!(key.contains("active=true"));
    assert!(key.contains("role=admin"));
    assert!(key.contains("o:created_at:desc"));
    assert!(key.contains("l:10"));
    assert!(key.contains("off:20"));
}

#[test]
fn cache_keys_of_different_parts_never_coincide() {
    let pairs = [
        (
            CacheKeyBuilder::new().condition("name", "alice:o:id:ASC"),
            CacheKeyBuilder::new()
                .condition("name", "alice")
                .order("id", "ASC"),
        ),
        (
            CacheKeyBuilder::new().condition("a", "b=c"),
            CacheKeyBuilder::new().condition("a=b", "c"),
        ),
        (
            CacheKeyBuilder::new().raw("t").raw("users"),
            CacheKeyBuilder::new().table("users"),
        ),
    ];
    for (one, other) in pairs {
        assert_ne!(one.build(), other.build());
    }
}

#[test]
fn test_cache_stats_hit_ratio() {
    let mut stats = CacheStats::default();
    assert_eq!(stats.hit_ratio(), 0.0);

    stats.hits = 75;
    stats.misses = 25;
    assert!((stats.hit_ratio() - 0.75).abs() < 0.001);
}

/// A TTL past what an `Instant` can reach, such as `Duration::MAX` for "keep
/// it", keeps the entry; it expired at once, and still evicted a live one.
#[test]
fn test_a_ttl_too_long_for_an_instant_keeps_the_entry() {
    let cache = QueryCache::new();
    cache.enable();
    cache
        .set("forever", &1_i32, Some(Duration::MAX), "test_model")
        .unwrap();
    assert_eq!(cache.get::<i32>("forever"), Some(1));
}

/// A deadlock ends the whole transaction on the server, so the enclosing
/// transactions of the one it failed in learn of it when that one ends.
#[test]
fn test_a_deadlock_marks_every_enclosing_transaction() {
    let outer = Arc::new(parking_lot::Mutex::new(PendingInvalidations::default()));
    {
        let inner = Arc::new(parking_lot::Mutex::new(PendingInvalidations::new(Some(
            outer.clone(),
        ))));
        inner.lock().note_failed_statement(true);
        assert_eq!(inner.lock().aborted(), Some(Abort::Deadlock));
        assert_eq!(outer.lock().aborted(), None);
    }
    assert_eq!(outer.lock().aborted(), Some(Abort::Deadlock));

    let other = Arc::new(parking_lot::Mutex::new(PendingInvalidations::default()));
    other.lock().note_failed_statement(false);
    assert!(other.lock().statement_failed());
    assert_eq!(other.lock().aborted(), None);
}

#[test]
fn failed_decode_is_a_miss_and_evicts_payload() {
    for strategy in [CacheStrategy::LRU, CacheStrategy::FIFO, CacheStrategy::TTL] {
        let cache = QueryCache::with_config(CacheConfig {
            enabled: true,
            strategy,
            ..Default::default()
        });
        cache.set("key", &"text", None, "model").unwrap();
        assert_eq!(cache.get::<u64>("key"), None);
        assert_eq!(cache.stats().hits, 0);
        assert_eq!(cache.stats().misses, 1);
        assert!(!cache.contains("key"));
    }
}

#[test]
fn decoding_can_reenter_cache_without_holding_a_lock() {
    static CACHE: std::sync::OnceLock<QueryCache> = std::sync::OnceLock::new();
    struct Reentrant;
    impl<'de> serde::Deserialize<'de> for Reentrant {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            let _ = <u64 as serde::Deserialize>::deserialize(deserializer)?;
            CACHE
                .get()
                .unwrap()
                .set("another", &1, None, "model")
                .unwrap();
            Ok(Self)
        }
    }
    let cache = CACHE.get_or_init(QueryCache::new);
    cache.enable();
    cache.set("key", &1, None, "model").unwrap();
    assert!(cache.get::<Reentrant>("key").is_some());
}

#[test]
fn failed_decoding_keeps_a_replacement_written_during_deserialization() {
    static CACHE: std::sync::OnceLock<QueryCache> = std::sync::OnceLock::new();
    struct ReplaceThenFail;
    impl<'de> serde::Deserialize<'de> for ReplaceThenFail {
        fn deserialize<D: serde::Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
            CACHE.get().unwrap().set("key", &42, None, "model").unwrap();
            Err(serde::de::Error::custom("old payload is unusable"))
        }
    }
    let cache = CACHE.get_or_init(QueryCache::new);
    cache.enable();
    cache.set("key", &1, None, "model").unwrap();
    assert!(cache.get::<ReplaceThenFail>("key").is_none());
    assert_eq!(cache.get::<u64>("key"), Some(42));
}

#[test]
fn values_whose_json_does_not_read_back_are_not_cached() {
    let cache = QueryCache::new();
    cache.enable();

    // JSON writes each of these as a `null`, which does not read back as it.
    cache
        .set("json_null", &vec![Some(serde_json::Value::Null)], None, "t")
        .unwrap();
    cache.set("nan", &vec![f64::NAN], None, "t").unwrap();
    cache
        .set("infinite", &(1, Some(f32::NEG_INFINITY)), None, "t")
        .unwrap();
    cache
        .set("some_none", &Some(None::<i32>), None, "t")
        .unwrap();
    assert!(cache.is_empty(), "{:?}", cache.stats());

    // A bare `null` and a `None` read back as written.
    let kept = (serde_json::Value::Null, None::<i32>, vec![Some(1.5)]);
    cache.set("kept", &kept, None, "t").unwrap();
    assert_eq!(cache.get("kept"), Some(kept));
}

#[test]
fn an_uncacheable_replacement_removes_the_old_value() {
    let cache = QueryCache::new();
    cache.enable();
    cache.set("key", &vec![1.0], None, "t").unwrap();
    cache.set("other", &42, None, "t").unwrap();
    cache.set("key", &vec![f64::NAN], None, "t").unwrap();
    assert_eq!(cache.get::<Vec<f64>>("key"), None);
    assert_eq!(cache.get::<i32>("other"), Some(42));
    assert_eq!(cache.stats().invalidations, 1);
}

#[test]
fn a_replacement_rejected_by_cache_policy_removes_only_its_old_value() {
    for policy in ["empty", "oversized", "zero entries"] {
        let cache = QueryCache::new();
        cache.enable();
        cache.set("key", &vec![1], None, "t").unwrap();
        cache.set("other", &42, None, "t").unwrap();
        let replacement = match policy {
            "empty" => {
                cache.set_cache_empty_results(false);
                vec![]
            }
            "oversized" => {
                cache.set_max_size_bytes(8);
                vec![123456789]
            }
            _ => {
                cache.set_max_entries(0);
                vec![2]
            }
        };
        cache
            .set_tagged("key", &replacement, None, &["t".to_string()])
            .unwrap();
        assert_eq!(cache.get::<Vec<i32>>("key"), None, "{policy}");
        assert_eq!(cache.get::<i32>("other"), Some(42), "{policy}");
        assert_eq!(cache.stats().entries, 1, "{policy}");
    }
}

#[test]
fn replacement_byte_budget_counts_the_new_payload_once() {
    for strategy in [CacheStrategy::LRU, CacheStrategy::FIFO, CacheStrategy::TTL] {
        let cache = QueryCache::with_config(CacheConfig {
            enabled: true,
            strategy,
            max_size_bytes: 8,
            ..Default::default()
        });
        cache.set("other", &"bb", None, "t").unwrap();
        cache.set("key", &"aa", None, "t").unwrap();
        for value in ["cc", "c"] {
            cache.set("key", &value, None, "t").unwrap();
            assert_eq!(cache.get::<String>("other").as_deref(), Some("bb"));
            assert_eq!(cache.get::<String>("key").as_deref(), Some(value));
            assert_eq!(cache.stats().size_bytes, 4 + value.len() + 2);
            assert_eq!(cache.stats().evictions, 0);
        }
        cache.set("key", &"dddd", None, "t").unwrap();
        assert_eq!(cache.get::<String>("other"), None);
        assert_eq!(cache.get::<String>("key").as_deref(), Some("dddd"));
        assert_eq!(cache.stats().size_bytes, 6);
        assert_eq!(cache.stats().evictions, 1);
    }
}

#[test]
fn replacement_obeys_a_lowered_entry_budget() {
    let cache = QueryCache::new();
    cache.enable();
    for key in ["a", "b", "c"] {
        cache.set(key, &1, None, "t").unwrap();
    }
    cache.set_max_entries(1);
    cache.set("c", &2, None, "t").unwrap();
    assert_eq!(cache.get::<i32>("c"), Some(2));
    assert_eq!(cache.stats().entries, 1);
    assert_eq!(cache.stats().evictions, 2);
}

#[test]
fn a_stale_uncacheable_fill_preserves_the_new_value() {
    let cache = QueryCache::new();
    cache.enable();
    let point = cache.fill_point();
    cache.invalidate_model("t");
    cache.set("key", &42, None, "t").unwrap();
    cache
        .fill_tagged(point, "key", &f64::NAN, None, &["t".to_string()])
        .unwrap();
    assert_eq!(cache.get::<i32>("key"), Some(42));
}

#[test]
fn zero_ttl_entries_do_not_evict_live_entries() {
    for default_ttl in [false, true] {
        let cache = QueryCache::with_config(CacheConfig {
            enabled: true,
            max_entries: 1,
            ..Default::default()
        });
        cache.set("live", &42, None, "t").unwrap();
        let ttl = if default_ttl {
            cache.set_default_ttl(Duration::ZERO);
            None
        } else {
            Some(Duration::ZERO)
        };
        cache.set("expired", &7, ttl, "t").unwrap();
        assert_eq!(cache.get::<i32>("live"), Some(42));
        assert_eq!(cache.stats().entries, 1);
        assert_eq!(cache.stats().evictions, 0);
        assert!(!cache.contains("expired"));
    }
}

#[test]
fn a_zero_ttl_replacement_removes_only_its_previous_value() {
    let cache = QueryCache::with_config(CacheConfig {
        enabled: true,
        max_size_bytes: 8,
        ..Default::default()
    });
    cache.set("other", &"bb", None, "t").unwrap();
    cache.set("key", &"aa", None, "t").unwrap();
    cache
        .set("key", &"dddd", Some(Duration::ZERO), "t")
        .unwrap();
    assert_eq!(cache.get::<String>("other").as_deref(), Some("bb"));
    assert!(!cache.contains("key"));
    assert_eq!(cache.stats().size_bytes, 4);
    assert_eq!(cache.stats().evictions, 0);
}

#[test]
fn serialization_errors_are_reported_without_discarding_the_previous_entry() {
    struct CannotSerialize;
    impl serde::Serialize for CannotSerialize {
        fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom(
                "deliberate serialization failure",
            ))
        }
    }

    let cache = QueryCache::new();
    cache.enable();
    cache.set("key", &42, None, "t").unwrap();
    let error = cache.set("key", &CannotSerialize, None, "t").unwrap_err();
    assert!(
        error
            .to_string()
            .contains("deliberate serialization failure")
    );
    // The JSON encoder refuses a map with non-string keys before the probe runs.
    let invalid_key = std::collections::BTreeMap::from([((1, 2), 3)]);
    assert!(cache.set("key", &invalid_key, None, "t").is_err());
    // An earlier lossy field must not mask a later serialization failure.
    assert!(
        cache
            .set("key", &(f64::NAN, CannotSerialize), None, "t")
            .is_err()
    );
    assert_eq!(cache.get::<i32>("key"), Some(42));
    assert_eq!(cache.stats().invalidations, 0);
}
