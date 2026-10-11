use super::Database;

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn public_raw_reads_and_writes_flush_but_internal_queries_do_not() {
    use crate::cache::QueryCache;

    let cache = QueryCache::global();
    let was_enabled = cache.is_enabled();
    cache.enable();

    let db = Database::connect("sqlite::memory:")
        .await
        .expect("sqlite in-memory connection should succeed");
    db.exec_raw("CREATE TABLE raw_cache_probe (id INTEGER PRIMARY KEY, deleted_at TEXT)")
        .await
        .expect("the probe table should be created");

    let seed = || {
        cache.clear();
        cache
            .set_tagged(
                "raw-cache-probe",
                &[1_i64],
                None,
                &["unrelated_table".to_string()],
            )
            .expect("seeding the cache should succeed");
        assert!(
            cache.contains("raw-cache-probe"),
            "the seeded entry should be cached"
        );
    };

    // A builder read on a soft-delete model renders `deleted_at`, which used
    // to classify the read as a write and destroy the cache on every call.
    seed();
    db.query_raw_json(
        "WITH scoped AS (SELECT id FROM raw_cache_probe WHERE deleted_at IS NULL) \
         SELECT * FROM scoped",
    )
    .await
    .expect("the CTE read should succeed");
    assert!(
        !cache.contains("raw-cache-probe"),
        "raw reads may write through functions or views"
    );

    // Statements TideORM rendered itself leave invalidation to their caller,
    // which knows the one table it wrote.
    seed();
    db.__execute_with_params("INSERT INTO raw_cache_probe (id) VALUES (1)", Vec::new())
        .await
        .expect("the internal insert should succeed");
    assert!(
        cache.contains("raw-cache-probe"),
        "an internal write must leave unrelated entries to targeted invalidation"
    );

    // Hand-written raw SQL remains unattributable, so it still flushes.
    seed();
    db.exec_raw("INSERT INTO raw_cache_probe (id) VALUES (2)")
        .await
        .expect("the raw insert should succeed");
    assert!(
        !cache.contains("raw-cache-probe"),
        "an unattributable raw write must still flush the query cache"
    );

    if !was_enabled {
        cache.disable();
    }
}

#[test]
fn non_finite_floats_stay_distinguishable_from_null() {
    assert_eq!(Database::f64_to_json(1.5), serde_json::json!(1.5));
    assert_eq!(
        Database::f64_to_json(f64::NAN),
        serde_json::Value::String("NaN".to_string())
    );
    assert_eq!(
        Database::f64_to_json(f64::INFINITY),
        serde_json::Value::String("inf".to_string())
    );
}
