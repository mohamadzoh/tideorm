use super::BatchUpdateBuilder;
use crate::model::Model as ModelTrait;

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use crate::{Database, QueryCache, TideConfig};
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use std::sync::{Mutex, OnceLock};
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use std::time::Duration;

#[tideorm::model(table = "batch_update_guard_users")]
struct BatchUpdateGuardUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
fn batch_cache_test_guard() -> &'static Mutex<()> {
    static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    GUARD.get_or_init(|| Mutex::new(()))
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
async fn setup_batch_cache_test_db() -> Database {
    Database::reset_global();
    TideConfig::reset();
    QueryCache::global().clear();
    QueryCache::global().enable();

    let db = Database::connect("sqlite::memory:")
        .await
        .expect("sqlite in-memory connection should succeed for batch cache tests");
    Database::set_global(db.clone()).expect("setting global database should succeed");

    db.__execute_with_params(
        "CREATE TABLE batch_update_guard_users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL)",
        vec![],
    )
    .await
    .expect("creating batch cache test schema should succeed");

    db
}

#[test]
fn batch_update_guard_rejects_unfiltered_updates() {
    let err = BatchUpdateBuilder::<BatchUpdateGuardUser>::new()
        .set("name", "updated")
        .ensure_explicit_filters("update")
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("requires at least one explicit filter")
    );
}

#[test]
fn batch_update_guard_accepts_where_filters() {
    assert!(
        BatchUpdateGuardUser::update_all()
            .set("name", "updated")
            .where_eq("id", 1)
            .ensure_explicit_filters("update")
            .is_ok()
    );
}

#[test]
fn batch_update_guard_accepts_or_filters() {
    assert!(
        BatchUpdateGuardUser::update_all()
            .set("name", "updated")
            .or_where_eq("name", "alice")
            .ensure_explicit_filters("update")
            .is_ok()
    );
}

#[test]
fn batch_update_guard_rejects_limit_without_where() {
    let err = BatchUpdateGuardUser::update_all()
        .set("name", "updated")
        .limit(1)
        .ensure_explicit_filters("update")
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("requires at least one explicit filter")
    );
}

#[test]
fn batch_update_set_if_applies_update_when_condition_is_true() {
    let builder = BatchUpdateBuilder::<BatchUpdateGuardUser>::new().set_if("name", "updated", true);

    assert!(matches!(
        builder.updates.get("name"),
        Some(super::UpdateValue::Value(value)) if *value == serde_json::json!("updated")
    ));
}

#[test]
fn batch_update_set_if_skips_update_when_condition_is_false() {
    let builder =
        BatchUpdateBuilder::<BatchUpdateGuardUser>::new().set_if("name", "updated", false);

    assert!(!builder.updates.contains_key("name"));
}

#[test]
fn batch_update_builder_accepts_typed_update_columns() {
    let builder = BatchUpdateBuilder::<BatchUpdateGuardUser>::new()
        .set(BatchUpdateGuardUser::columns.name, "updated")
        .increment(BatchUpdateGuardUser::columns.id, 1);

    assert!(builder.updates.contains_key("name"));
    assert!(builder.updates.contains_key("id"));
}

#[test]
fn batch_update_builder_accepts_typed_filter_columns() {
    let builder = BatchUpdateGuardUser::update_all()
        .where_eq(BatchUpdateGuardUser::columns.id, 1)
        .or_where_eq(BatchUpdateGuardUser::columns.name, "alice");

    assert_eq!(builder.conditions.len(), 2);
    assert_eq!(builder.conditions[0].column, "id");
    assert_eq!(builder.conditions[1].column, "__OR__name");
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn batch_execute_invalidates_cached_queries() {
    let _guard = batch_cache_test_guard()
        .lock()
        .expect("batch cache test guard should not be poisoned");
    let _db = setup_batch_cache_test_db().await;

    let saved = BatchUpdateGuardUser {
        id: 0,
        name: "Alice".to_string(),
    }
    .save()
    .await
    .expect("seed save should succeed");

    let cached_before = BatchUpdateGuardUser::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(Duration::from_secs(60))
        .get()
        .await
        .expect("cached query before batch execute should succeed");
    assert_eq!(cached_before[0].name, "Alice");
    assert_eq!(QueryCache::global().stats().entries, 1);

    let rows_affected = BatchUpdateGuardUser::update_all()
        .set("name", "Bob")
        .where_eq("id", saved.id)
        .execute()
        .await
        .expect("batch execute should succeed");

    assert_eq!(rows_affected, 1);
    assert_eq!(QueryCache::global().stats().entries, 0);

    let fresh = BatchUpdateGuardUser::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(Duration::from_secs(60))
        .get()
        .await
        .expect("fresh query should succeed after batch execute");
    assert_eq!(fresh[0].name, "Bob");

    QueryCache::global().clear();
    QueryCache::global().disable();
    Database::reset_global();
    TideConfig::reset();
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn batch_execute_returning_invalidates_cached_queries() {
    let _guard = batch_cache_test_guard()
        .lock()
        .expect("batch cache test guard should not be poisoned");
    let _db = setup_batch_cache_test_db().await;

    let saved = BatchUpdateGuardUser {
        id: 0,
        name: "Alice".to_string(),
    }
    .save()
    .await
    .expect("seed save should succeed");

    let cached_before = BatchUpdateGuardUser::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(Duration::from_secs(60))
        .get()
        .await
        .expect("cached query before batch execute_returning should succeed");
    assert_eq!(cached_before[0].name, "Alice");
    assert_eq!(QueryCache::global().stats().entries, 1);

    let returned = BatchUpdateGuardUser::update_all()
        .set("name", "Bob")
        .where_eq("id", saved.id)
        .execute_returning()
        .await
        .expect("batch execute_returning should succeed");

    assert_eq!(returned.len(), 1);
    assert_eq!(returned[0].name, "Bob");
    assert_eq!(QueryCache::global().stats().entries, 0);

    let fresh = BatchUpdateGuardUser::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(Duration::from_secs(60))
        .get()
        .await
        .expect("fresh query should succeed after batch execute_returning");
    assert_eq!(fresh[0].name, "Bob");

    QueryCache::global().clear();
    QueryCache::global().disable();
    Database::reset_global();
    TideConfig::reset();
}
