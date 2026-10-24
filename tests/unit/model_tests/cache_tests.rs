use super::*;

/// The names a cached read of every row returns, leaving that read cached.
async fn cached_names() -> Vec<String> {
    let rows = crate::test_support::cached_rows::<AutoIncrementModel>().await;
    assert_eq!(
        crate::test_support::cached_entries(),
        1,
        "the read should be cached"
    );
    rows.into_iter().map(|row| row.name).collect()
}

/// `save`, `update`, `delete` and `destroy` each empty the query cache, so
/// the next cached read sees the table as the write left it.
#[tokio::test]
async fn model_writes_invalidate_cached_queries() {
    let _guard = crate::test_support::global_db_lock().lock().await;
    crate::test_support::install_sqlite_global(&[
        "CREATE TABLE model_test_users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL)",
    ])
    .await;
    crate::test_support::set_query_cache(true);
    let cache_emptied_by = |write: &str| {
        assert_eq!(
            crate::test_support::cached_entries(),
            0,
            "{write} should empty the cache"
        );
    };

    assert!(cached_names().await.is_empty());
    let alice = AutoIncrementModel {
        id: 0,
        name: "Alice".to_string(),
    }
    .save()
    .await
    .expect("save should insert a new row");
    assert!(alice.id > 0);
    cache_emptied_by("save");

    assert_eq!(cached_names().await, ["Alice"]);
    let bob = AutoIncrementModel {
        id: alice.id,
        name: "Bob".to_string(),
    }
    .update()
    .await
    .expect("update should succeed");
    assert_eq!(bob.name, "Bob");
    cache_emptied_by("update");

    assert_eq!(cached_names().await, ["Bob"]);
    assert_eq!(bob.delete().await.expect("delete should succeed"), 1);
    cache_emptied_by("delete");

    assert!(cached_names().await.is_empty());
    let carol = AutoIncrementModel {
        id: 0,
        name: "Carol".to_string(),
    }
    .save()
    .await
    .expect("save should insert a new row");
    assert_eq!(cached_names().await, ["Carol"]);
    assert_eq!(
        AutoIncrementModel::destroy(carol.id)
            .await
            .expect("destroy should succeed"),
        1
    );
    cache_emptied_by("destroy");
    assert!(cached_names().await.is_empty());

    crate::test_support::reset_globals_and_cache();
}
