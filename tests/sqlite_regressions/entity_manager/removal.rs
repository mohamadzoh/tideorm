//! A removal deletes the row it was loaded as, never another one.

use crate::fixtures::memory_database;
use std::sync::Arc;
use tideorm::entity_manager::EntityManager;
use tideorm::prelude::*;

#[tideorm::model(table = "managed_remove_rows")]
struct Row {
    #[tideorm(primary_key)]
    id: i64,
    name: String,
}

async fn database() -> Database {
    memory_database("CREATE TABLE managed_remove_rows(id INTEGER PRIMARY KEY,name TEXT); INSERT INTO managed_remove_rows VALUES(1,'one'),(2,'two')").await
}

#[tokio::test]
async fn detached_merge_key_edit_cannot_delete_another_row() {
    let db = database().await;
    let manager = EntityManager::new(Arc::new(db.clone()));
    let row = manager
        .merge(Row {
            id: 1,
            name: "one".into(),
        })
        .unwrap();
    row.edit(|row| row.id = 2);
    manager.remove(&row);
    let error = manager.flush().await.unwrap_err().to_string();
    assert!(
        error.contains("refusing to delete a different row"),
        "{error}"
    );
    for id in [1, 2] {
        assert!(Row::find_with(id, &db).await.unwrap().is_some());
    }
    // Rollback keeps the pending removal recoverable after correcting the key.
    row.edit(|row| row.id = 1);
    manager.flush().await.unwrap();
    assert!(Row::find_with(1, &db).await.unwrap().is_none());
    assert!(Row::find_with(2, &db).await.unwrap().is_some());
}

#[tokio::test]
async fn loaded_removal_uses_its_original_snapshot() {
    let db = database().await;
    let manager = EntityManager::new(Arc::new(db.clone()));
    let row = manager.find_managed::<Row>(1).await.unwrap().unwrap();
    row.edit(|row| row.id = 2);
    manager.remove(&row);
    manager.flush().await.unwrap();
    assert!(Row::find_with(1, &db).await.unwrap().is_none());
    assert!(Row::find_with(2, &db).await.unwrap().is_some());
}

#[tokio::test]
async fn removing_an_unflushed_insert_cannot_delete_an_existing_row() {
    let db = database().await;
    let manager = EntityManager::new(Arc::new(db.clone()));
    let row = manager.persist(Row {
        id: 3,
        name: "new".into(),
    });
    row.edit(|row| row.id = 2);
    manager.remove(&row);
    manager.flush().await.unwrap();
    assert!(Row::find_with(2, &db).await.unwrap().is_some());
    assert!(Row::find_with(3, &db).await.unwrap().is_none());
}

#[tokio::test]
async fn invalid_removal_rolls_back_earlier_deletes_in_the_flush() {
    let db = database().await;
    db.exec_raw("INSERT INTO managed_remove_rows VALUES(3,'three')")
        .await
        .unwrap();
    let manager = EntityManager::new(Arc::new(db.clone()));
    let safe = manager
        .merge(Row {
            id: 1,
            name: "one".into(),
        })
        .unwrap();
    let changed = manager
        .merge(Row {
            id: 2,
            name: "two".into(),
        })
        .unwrap();
    manager.remove(&safe);
    changed.edit(|row| row.id = 3);
    manager.remove(&changed);
    assert!(manager.flush().await.is_err());
    for id in [1, 2, 3] {
        assert!(Row::find_with(id, &db).await.unwrap().is_some());
    }
    changed.edit(|row| row.id = 2);
    manager.flush().await.unwrap();
    assert!(Row::find_with(1, &db).await.unwrap().is_none());
    assert!(Row::find_with(2, &db).await.unwrap().is_none());
    assert!(Row::find_with(3, &db).await.unwrap().is_some());
}
