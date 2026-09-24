use super::Database;

#[test]
fn read_only_statements_do_not_flush_the_cache() {
    assert!(!Database::raw_sql_may_write("SELECT 1"));
    assert!(!Database::raw_sql_may_write(
        "  -- comment\n select * from users"
    ));
    assert!(!Database::raw_sql_may_write("/* hint */ EXPLAIN SELECT 1"));
    assert!(!Database::raw_sql_may_write("(SELECT 1)"));
    assert!(!Database::raw_sql_may_write(
        "WITH active AS (SELECT 1) SELECT * FROM active"
    ));
}

#[test]
fn soft_delete_columns_are_not_write_keywords() {
    // `deleted_at` and `updated_at` contain `DELETE` and `UPDATE`, and the
    // soft-delete scope renders them into the `WHERE` clause of every read.
    assert!(!Database::raw_sql_may_write(
        "WITH scoped AS (SELECT id FROM users WHERE \"deleted_at\" IS NULL) \
         SELECT * FROM scoped"
    ));
    assert!(!Database::raw_sql_may_write(
        "WITH recent AS (SELECT id, updated_at FROM users) \
         SELECT * FROM recent ORDER BY updated_at DESC"
    ));
    // Nor can a literal that merely mentions one.
    assert!(!Database::raw_sql_may_write(
        "WITH notes AS (SELECT 'delete me' AS body) SELECT * FROM notes"
    ));
    // A column list in front of `AS` is not a statement.
    assert!(!Database::raw_sql_may_write(
        "WITH scoped (id) AS (SELECT id FROM users) SELECT * FROM scoped"
    ));
}

#[test]
fn writing_statements_flush_the_cache() {
    assert!(Database::raw_sql_may_write(
        "INSERT INTO users (id) VALUES (1)"
    ));
    assert!(Database::raw_sql_may_write("update users set active = 1"));
    assert!(Database::raw_sql_may_write("DELETE FROM users"));
    assert!(Database::raw_sql_may_write(
        "CREATE TABLE users (id INTEGER)"
    ));
    assert!(Database::raw_sql_may_write(
        "WITH removed AS (DELETE FROM users RETURNING id) SELECT * FROM removed"
    ));
    // Unparseable input stays conservative.
    assert!(Database::raw_sql_may_write(""));
}

#[test]
fn data_modifying_ctes_still_flush_the_cache() {
    // The soft-delete filter inside the CTE is not what makes this a write:
    // the CTE body itself is.
    assert!(Database::raw_sql_may_write(
        "WITH removed AS (DELETE FROM users WHERE deleted_at IS NOT NULL RETURNING id) \
         SELECT * FROM removed"
    ));
    // The statement following the CTE list counts too.
    assert!(Database::raw_sql_may_write(
        "WITH stale AS (SELECT id FROM users) \
         UPDATE users SET updated_at = NULL WHERE id IN (SELECT id FROM stale)"
    ));
    // As does a nested one.
    assert!(Database::raw_sql_may_write(
        "WITH outer_rows AS (WITH inner_rows AS (INSERT INTO audit (id) VALUES (1) \
         RETURNING id) SELECT * FROM inner_rows) SELECT * FROM outer_rows"
    ));
    // A `WITH` that never reaches a statement stays conservative.
    assert!(Database::raw_sql_may_write("WITH scoped AS ("));
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn only_unattributable_raw_writes_flush_the_cache() {
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
        cache.contains("raw-cache-probe"),
        "a read must never flush the query cache"
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
