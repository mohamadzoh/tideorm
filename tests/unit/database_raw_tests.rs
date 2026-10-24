use super::Database;

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn public_raw_reads_and_writes_flush_but_internal_queries_do_not() {
    let _guard = crate::test_support::global_db_lock().lock().await;
    crate::test_support::set_query_cache(true);
    let cache = crate::cache::QueryCache::global();

    let db = Database::connect("sqlite::memory:")
        .await
        .expect("sqlite in-memory connection should succeed");
    db.exec_raw("CREATE TABLE raw_cache_probe (id INTEGER PRIMARY KEY)")
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

    // Raw SQL is opaque, so a public raw read flushes too: even a SELECT can
    // write through a function or a view.
    seed();
    db.query_raw_json("SELECT id FROM raw_cache_probe")
        .await
        .expect("the raw read should succeed");
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

    crate::test_support::set_query_cache(false);
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
