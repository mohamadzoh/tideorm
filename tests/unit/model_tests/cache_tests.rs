#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use super::*;

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn save_invalidates_cached_queries_after_insert() {
    let _guard = model_cache_test_guard().lock().await;
    let _db = setup_model_cache_test_db().await;

    let cached_before = AutoIncrementModel::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(Duration::from_secs(60))
        .get()
        .await
        .expect("initial cached query should succeed");
    assert!(cached_before.is_empty());
    assert_eq!(QueryCache::global().stats().entries, 1);

    let saved = AutoIncrementModel {
        id: 0,
        name: "Alice".to_string(),
    }
    .save()
    .await
    .expect("save should insert a new row");

    assert!(saved.id > 0);
    assert_eq!(QueryCache::global().stats().entries, 0);

    let fresh = AutoIncrementModel::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(Duration::from_secs(60))
        .get()
        .await
        .expect("fresh cached query should succeed after save");
    assert_eq!(fresh.len(), 1);
    assert_eq!(fresh[0].name, "Alice");

    cleanup_model_cache_test_state();
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn update_invalidates_cached_queries() {
    let _guard = model_cache_test_guard().lock().await;
    let _db = setup_model_cache_test_db().await;

    let saved = AutoIncrementModel {
        id: 0,
        name: "Alice".to_string(),
    }
    .save()
    .await
    .expect("seed save should succeed");

    let cached_before = AutoIncrementModel::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(Duration::from_secs(60))
        .get()
        .await
        .expect("cached query before update should succeed");
    assert_eq!(cached_before.len(), 1);
    assert_eq!(cached_before[0].name, "Alice");
    assert_eq!(QueryCache::global().stats().entries, 1);

    let updated = AutoIncrementModel {
        id: saved.id,
        name: "Bob".to_string(),
    }
    .update()
    .await
    .expect("update should succeed");

    assert_eq!(updated.name, "Bob");
    assert_eq!(QueryCache::global().stats().entries, 0);

    let fresh = AutoIncrementModel::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(Duration::from_secs(60))
        .get()
        .await
        .expect("fresh cached query should succeed after update");
    assert_eq!(fresh.len(), 1);
    assert_eq!(fresh[0].name, "Bob");

    cleanup_model_cache_test_state();
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn delete_invalidates_cached_queries() {
    let _guard = model_cache_test_guard().lock().await;
    let _db = setup_model_cache_test_db().await;

    let saved = AutoIncrementModel {
        id: 0,
        name: "Alice".to_string(),
    }
    .save()
    .await
    .expect("seed save should succeed");

    let cached_before = AutoIncrementModel::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(Duration::from_secs(60))
        .get()
        .await
        .expect("cached query before delete should succeed");
    assert_eq!(cached_before.len(), 1);
    assert_eq!(QueryCache::global().stats().entries, 1);

    let rows_affected = saved.delete().await.expect("delete should succeed");

    assert_eq!(rows_affected, 1);
    assert_eq!(QueryCache::global().stats().entries, 0);

    let fresh = AutoIncrementModel::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(Duration::from_secs(60))
        .get()
        .await
        .expect("fresh cached query should succeed after delete");
    assert!(fresh.is_empty());

    cleanup_model_cache_test_state();
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn destroy_invalidates_cached_queries() {
    let _guard = model_cache_test_guard().lock().await;
    let _db = setup_model_cache_test_db().await;

    let saved = AutoIncrementModel {
        id: 0,
        name: "Alice".to_string(),
    }
    .save()
    .await
    .expect("seed save should succeed");

    let cached_before = AutoIncrementModel::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(Duration::from_secs(60))
        .get()
        .await
        .expect("cached query before destroy should succeed");
    assert_eq!(cached_before.len(), 1);
    assert_eq!(QueryCache::global().stats().entries, 1);

    let rows_affected = AutoIncrementModel::destroy(saved.id)
        .await
        .expect("destroy should succeed");

    assert_eq!(rows_affected, 1);
    assert_eq!(QueryCache::global().stats().entries, 0);

    let fresh = AutoIncrementModel::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(Duration::from_secs(60))
        .get()
        .await
        .expect("fresh cached query should succeed after destroy");
    assert!(fresh.is_empty());

    cleanup_model_cache_test_state();
}
