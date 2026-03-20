use super::{NestedSave, NestedSaveBuilder};
use crate::internal::ConnectionTrait;
use crate::model::Model;
use crate::{Database, GlobalProfiler, TideConfig};

#[derive(tideorm::Model, PartialEq)]
#[tideorm(table = "nested_test_parents")]
struct NestedTestParent {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[derive(tideorm::Model, PartialEq)]
#[tideorm(table = "nested_test_children")]
struct NestedTestChild {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    parent_id: i64,
    name: String,
}

async fn setup_nested_test_db() -> Database {
    Database::reset_global();
    TideConfig::reset();
    GlobalProfiler::disable();
    GlobalProfiler::reset();

    let db = Database::connect("sqlite::memory:")
        .await
        .expect("failed to connect to SQLite for nested model tests");
    Database::set_global(db.clone()).expect("failed to register nested test database");

    db.__internal_connection()
        .unwrap()
        .execute_unprepared(
            r#"
            CREATE TABLE nested_test_parents (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL
            );

            CREATE TABLE nested_test_children (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                parent_id INTEGER NOT NULL,
                name TEXT NOT NULL
            );
            "#,
        )
        .await
        .expect("failed to create nested test schema");

    db
}

fn nested_children(names: &[&str]) -> Vec<NestedTestChild> {
    names
        .iter()
        .map(|name| NestedTestChild {
            id: 0,
            parent_id: 0,
            name: (*name).to_string(),
        })
        .collect()
}

#[tokio::test]
async fn save_with_many_persists_children_with_parent_fk() {
    let _db = setup_nested_test_db().await;

    let parent = NestedTestParent {
        id: 0,
        name: "parent".to_string(),
    };

    let (saved_parent, saved_children) = parent
        .save_with_many(nested_children(&["alpha", "beta"]), "parent_id")
        .await
        .expect("save_with_many should succeed");

    assert!(saved_parent.id > 0);
    assert_eq!(saved_children.len(), 2);
    assert!(saved_children
        .iter()
        .all(|child| child.parent_id == saved_parent.id));
    assert!(saved_children.iter().all(|child| child.id > 0));

    let fetched = NestedTestChild::query()
        .where_eq("parent_id", saved_parent.id)
        .order_by("id", crate::query::Order::Asc)
        .get()
        .await
        .expect("should fetch nested children");
    assert_eq!(fetched, saved_children);
}

#[tokio::test]
async fn nested_save_builder_persists_related_models_with_parent_fk() {
    let _db = setup_nested_test_db().await;

    let parent = NestedTestParent {
        id: 0,
        name: "parent".to_string(),
    };

    let (saved_parent, saved_related) = NestedSaveBuilder::new(parent)
        .with_many(nested_children(&["alpha", "beta"]), "parent_id")
        .save()
        .await
        .expect("nested builder save should succeed");

    assert!(saved_parent.id > 0);
    assert_eq!(saved_related.len(), 2);

    let fetched = NestedTestChild::query()
        .where_eq("parent_id", saved_parent.id)
        .order_by("id", crate::query::Order::Asc)
        .get()
        .await
        .expect("should fetch nested children saved by builder");

    assert_eq!(fetched.len(), 2);
    assert_eq!(fetched[0].name, "alpha");
    assert_eq!(fetched[1].name, "beta");
    assert!(fetched
        .iter()
        .all(|child| child.parent_id == saved_parent.id));

    let returned_parent_ids: Vec<_> = saved_related
        .iter()
        .map(|value| value.get("parent_id").and_then(serde_json::Value::as_i64))
        .collect();
    assert_eq!(
        returned_parent_ids,
        vec![Some(saved_parent.id), Some(saved_parent.id)]
    );
}

#[tokio::test]
async fn delete_with_many_uses_bulk_delete_for_related_models() {
    let _db = setup_nested_test_db().await;

    let parent = NestedTestParent {
        id: 0,
        name: "parent".to_string(),
    };
    let (saved_parent, saved_children) = parent
        .save_with_many(nested_children(&["alpha", "beta", "gamma"]), "parent_id")
        .await
        .expect("save_with_many should seed nested children");

    GlobalProfiler::reset();
    GlobalProfiler::enable();

    let deleted = saved_parent
        .delete_with_many(saved_children)
        .await
        .expect("delete_with_many should succeed");

    GlobalProfiler::disable();

    assert_eq!(deleted, 4);
    assert_eq!(GlobalProfiler::stats().total_queries, 2);
    assert_eq!(
        NestedTestChild::query()
            .get()
            .await
            .expect("child query should succeed")
            .len(),
        0
    );
    assert_eq!(
        NestedTestParent::query()
            .get()
            .await
            .expect("parent query should succeed")
            .len(),
        0
    );
}

#[tokio::test]
async fn update_with_many_uses_bulk_upsert_for_existing_related_models() {
    let _db = setup_nested_test_db().await;

    let parent = NestedTestParent {
        id: 0,
        name: "parent".to_string(),
    };
    let (saved_parent, saved_children) = parent
        .save_with_many(nested_children(&["alpha", "beta", "gamma"]), "parent_id")
        .await
        .expect("save_with_many should seed nested children");

    let updated_parent = NestedTestParent {
        name: "parent-updated".to_string(),
        ..saved_parent
    };
    let updated_children: Vec<_> = saved_children
        .into_iter()
        .enumerate()
        .map(|(index, child)| NestedTestChild {
            name: format!("updated-{index}"),
            ..child
        })
        .collect();

    GlobalProfiler::reset();
    GlobalProfiler::enable();

    let (parent_after_update, children_after_update) = updated_parent
        .update_with_many(updated_children.clone())
        .await
        .expect("update_with_many should succeed");

    GlobalProfiler::disable();

    assert_eq!(parent_after_update.name, "parent-updated");
    assert_eq!(children_after_update.len(), 3);
    assert_eq!(GlobalProfiler::stats().total_queries, 3);

    for (expected, actual) in updated_children.iter().zip(children_after_update.iter()) {
        assert_eq!(expected.id, actual.id);
        assert_eq!(expected.parent_id, actual.parent_id);
        assert_eq!(expected.name, actual.name);
    }
}
