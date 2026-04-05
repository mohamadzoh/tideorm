#[cfg(all(feature = "dirty-tracking", feature = "sqlite", feature = "runtime-tokio"))]
use super::*;

#[cfg(all(feature = "dirty-tracking", feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn loaded_model_reports_changed_fields_and_original_values() {
    let _guard = model_cache_test_guard().lock().await;
    let _db = setup_model_cache_test_db().await;

    let saved = AutoIncrementModel {
        id: 0,
        name: "Alice".to_string(),
    }
    .save()
    .await
    .expect("seed save should succeed");

    let mut loaded = AutoIncrementModel::find(saved.id)
        .await
        .expect("find should succeed")
        .expect("saved model should exist");

    assert!(loaded.changed_fields().expect("dirty check should succeed").is_empty());
    assert_eq!(
        loaded.original_value("name").expect("original value lookup should succeed"),
        Some(serde_json::json!("Alice"))
    );

    loaded.name = "Bob".to_string();

    assert_eq!(
        loaded.changed_fields().expect("dirty check should succeed"),
        vec!["name"]
    );
    assert_eq!(
        loaded.original_value("name").expect("original value lookup should succeed"),
        Some(serde_json::json!("Alice"))
    );

    cleanup_model_cache_test_state();
}

#[cfg(all(feature = "dirty-tracking", feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn update_refreshes_dirty_tracking_baseline() {
    let _guard = model_cache_test_guard().lock().await;
    let _db = setup_model_cache_test_db().await;

    let saved = AutoIncrementModel {
        id: 0,
        name: "Alice".to_string(),
    }
    .save()
    .await
    .expect("seed save should succeed");

    let mut loaded = AutoIncrementModel::find(saved.id)
        .await
        .expect("find should succeed")
        .expect("saved model should exist");
    loaded.name = "Bob".to_string();

    let updated = loaded.update().await.expect("update should succeed");

    assert!(updated.changed_fields().expect("dirty check should succeed").is_empty());
    assert_eq!(
        updated.original_value("name").expect("original value lookup should succeed"),
        Some(serde_json::json!("Bob"))
    );

    cleanup_model_cache_test_state();
}

#[cfg(all(feature = "dirty-tracking", feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn cache_hits_restore_dirty_tracking_snapshots() {
    let _guard = model_cache_test_guard().lock().await;
    let _db = setup_model_cache_test_db().await;

    AutoIncrementModel {
        id: 0,
        name: "Alice".to_string(),
    }
    .save()
    .await
    .expect("seed save should succeed");

    let first = AutoIncrementModel::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(std::time::Duration::from_secs(60))
        .get()
        .await
        .expect("initial cached query should succeed");
    assert_eq!(first.len(), 1);

    crate::model::__clear_dirty_snapshots();

    let mut cached = AutoIncrementModel::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(std::time::Duration::from_secs(60))
        .get()
        .await
        .expect("cache hit should succeed");

    cached[0].name = "Cached Bob".to_string();

    assert_eq!(
        cached[0].changed_fields().expect("dirty check should succeed"),
        vec!["name"]
    );
    assert_eq!(
        cached[0].original_value("name").expect("original value lookup should succeed"),
        Some(serde_json::json!("Alice"))
    );

    cleanup_model_cache_test_state();
}

#[cfg(all(feature = "dirty-tracking", feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn updated_model_can_continue_tracking_after_baseline_refresh() {
    let _guard = model_cache_test_guard().lock().await;
    let _db = setup_model_cache_test_db().await;

    let saved = AutoIncrementModel {
        id: 0,
        name: "Alice".to_string(),
    }
    .save()
    .await
    .expect("seed save should succeed");

    let mut first = AutoIncrementModel::find(saved.id)
        .await
        .expect("find should succeed")
        .expect("saved model should exist");
    let mut second = AutoIncrementModel::find(saved.id)
        .await
        .expect("second find should succeed")
        .expect("saved model should exist");

    first.name = "Bob".to_string();
    let mut first = first.update().await.expect("update should succeed");
    assert!(first.changed_fields().expect("dirty check should succeed").is_empty());

    first.name = "Carol".to_string();
    assert_eq!(
        first.changed_fields().expect("dirty check should succeed after update"),
        vec!["name"]
    );
    assert_eq!(
        first.original_value("name").expect("original value lookup should succeed"),
        Some(serde_json::json!("Bob"))
    );

    second.name = "Dana".to_string();

    assert_eq!(
        second.changed_fields().expect("stale copies should compare against the latest baseline"),
        vec!["name"]
    );
    assert_eq!(
        second.original_value("name").expect("latest baseline lookup should succeed"),
        Some(serde_json::json!("Bob"))
    );

    cleanup_model_cache_test_state();
}