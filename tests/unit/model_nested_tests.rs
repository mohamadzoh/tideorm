use super::{NestedSave, NestedSaveBuilder, SavedRelation, apply_foreign_key};
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

#[derive(tideorm::Model, PartialEq)]
#[tideorm(table = "nested_test_profiles")]
struct NestedTestProfile {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_id: i64,
    bio: String,
}

/// The Rust field name and the DB column name differ, so this model is what
/// distinguishes "accepts both names" from "accepts only the Rust field name".
#[derive(tideorm::Model, PartialEq)]
#[tideorm(table = "nested_test_aliased_children")]
struct NestedTestAliasedChild {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    #[tideorm(column = "parent_id")]
    owner_id: i64,
    name: String,
}

#[derive(tideorm::Model, PartialEq)]
#[tideorm(table = "nested_test_string_children")]
struct NestedTestStringChild {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    parent_id: String,
    name: String,
}

/// Refuses to be deleted while `locked`, so a nested delete can have a child
/// that fails after an earlier one was already deleted.
#[derive(tideorm::Model, PartialEq)]
#[tideorm(table = "nested_test_locked_children")]
struct NestedTestLockedChild {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    parent_id: i64,
    locked: bool,
}

impl crate::callbacks::Callbacks for NestedTestLockedChild {
    fn before_delete(&self) -> crate::Result<()> {
        if self.locked {
            return Err(crate::Error::validation(
                "locked",
                "a locked child cannot be deleted",
            ));
        }

        Ok(())
    }
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

            CREATE TABLE nested_test_profiles (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id INTEGER NOT NULL,
                bio TEXT NOT NULL
            );

            CREATE TABLE nested_test_aliased_children (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                parent_id INTEGER NOT NULL,
                name TEXT NOT NULL UNIQUE
            );

            CREATE TABLE nested_test_string_children (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                parent_id TEXT NOT NULL,
                name TEXT NOT NULL
            );

            CREATE TABLE nested_test_locked_children (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                parent_id INTEGER NOT NULL,
                locked BOOLEAN NOT NULL
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

fn nested_profile() -> NestedTestProfile {
    NestedTestProfile {
        id: 0,
        user_id: 0,
        bio: "profile".to_string(),
    }
}

fn aliased_child(name: &str) -> NestedTestAliasedChild {
    NestedTestAliasedChild {
        id: 0,
        owner_id: 0,
        name: name.to_string(),
    }
}

fn nested_parent() -> NestedTestParent {
    NestedTestParent {
        id: 0,
        name: "parent".to_string(),
    }
}

async fn stored_parent_name(id: i64) -> String {
    NestedTestParent::find(id)
        .await
        .expect("parent lookup should succeed")
        .expect("the parent row should still exist")
        .name
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
    assert!(
        saved_children
            .iter()
            .all(|child| child.parent_id == saved_parent.id)
    );
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
    assert_eq!(saved_related.len(), 1);

    let returned_children = saved_related[0]
        .clone()
        .into_many::<NestedTestChild>()
        .expect("saved relation should deserialize into typed child models");
    assert_eq!(returned_children.len(), 2);
    assert!(
        returned_children
            .iter()
            .all(|child| child.parent_id == saved_parent.id)
    );

    let fetched = NestedTestChild::query()
        .where_eq("parent_id", saved_parent.id)
        .order_by("id", crate::query::Order::Asc)
        .get()
        .await
        .expect("should fetch nested children saved by builder");

    assert_eq!(fetched.len(), 2);
    assert_eq!(fetched[0].name, "alpha");
    assert_eq!(fetched[1].name, "beta");
    assert!(
        fetched
            .iter()
            .all(|child| child.parent_id == saved_parent.id)
    );
}

#[tokio::test]
async fn nested_save_builder_can_be_spawned() {
    let _db = setup_nested_test_db().await;

    let builder = NestedSaveBuilder::new(NestedTestParent {
        id: 0,
        name: "parent".to_string(),
    })
    .with_one(nested_profile(), "user_id")
    .with_many(nested_children(&["alpha", "beta"]), "parent_id");

    let join = tokio::spawn(async move { builder.save().await });

    let (saved_parent, saved_related) = join
        .await
        .expect("spawned nested save task should join successfully")
        .expect("spawned nested save should succeed");

    assert!(saved_parent.id > 0);
    assert_eq!(saved_related.len(), 2);

    let saved_profile = saved_related[0]
        .clone()
        .into_one::<NestedTestProfile>()
        .expect("first saved relation should deserialize into profile");
    assert_eq!(saved_profile.user_id, saved_parent.id);

    let saved_children = saved_related[1]
        .clone()
        .into_many::<NestedTestChild>()
        .expect("second saved relation should deserialize into children");
    assert_eq!(saved_children.len(), 2);
    assert!(
        saved_children
            .iter()
            .all(|child| child.parent_id == saved_parent.id)
    );

    let profile = NestedTestProfile::query()
        .where_eq("user_id", saved_parent.id)
        .first()
        .await
        .expect("profile query should succeed")
        .expect("spawned builder should persist the one relation");
    assert_eq!(profile.bio, "profile");

    let fetched = NestedTestChild::query()
        .where_eq("parent_id", saved_parent.id)
        .order_by("id", crate::query::Order::Asc)
        .get()
        .await
        .expect("should fetch nested children saved by spawned builder");

    assert_eq!(fetched.len(), 2);
    assert!(
        fetched
            .iter()
            .all(|child| child.parent_id == saved_parent.id)
    );
}

#[test]
fn saved_relation_rejects_wrong_shape_conversions() {
    let one = SavedRelation::one(nested_profile());
    let many = SavedRelation::many(nested_children(&["x"]));

    assert!(one.clone().into_many::<NestedTestChild>().is_err());
    assert!(many.clone().into_one::<NestedTestProfile>().is_err());
    // Another model of the same shape is not what was saved.
    assert!(one.into_one::<NestedTestChild>().is_err());
    assert!(many.into_many::<NestedTestProfile>().is_err());
}

/// A child whose own serde derive leaves a field out of its JSON.
#[tideorm::model(table = "nested_test_children")]
#[derive(serde::Serialize, serde::Deserialize)]
struct NestedTestQuietChild {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    parent_id: i64,
    #[serde(skip_serializing)]
    name: String,
}

/// The builder hands back each child as it was stored. It went through the
/// child's JSON, where a `skip_serializing` field is missing, so reading the
/// result failed after the save had committed.
#[tokio::test]
async fn nested_save_builder_returns_the_saved_models_themselves() {
    let _db = setup_nested_test_db().await;

    let (_, saved) = NestedSaveBuilder::new(NestedTestParent {
        id: 0,
        name: "parent".to_string(),
    })
    .with_one(
        NestedTestQuietChild {
            id: 0,
            parent_id: 0,
            name: "kept".to_string(),
        },
        "parent_id",
    )
    .save()
    .await
    .expect("nested builder save should succeed");

    let child = saved[0]
        .clone()
        .into_one::<NestedTestQuietChild>()
        .expect("the saved child should come back");
    assert!(child.id > 0);
    assert_eq!(child.name, "kept");
}

#[test]
fn apply_foreign_key_accepts_the_db_column_name_and_the_rust_field_name() {
    let child = NestedTestAliasedChild {
        id: 0,
        owner_id: 0,
        name: "alpha".to_string(),
    };

    let by_column = apply_foreign_key(child.clone(), "parent_id", &serde_json::json!(7))
        .expect("the DB column name should resolve");
    assert_eq!(by_column.owner_id, 7);

    let by_field = apply_foreign_key(child, "owner_id", &serde_json::json!(9))
        .expect("the Rust field name should resolve");
    assert_eq!(by_field.owner_id, 9);
}

#[test]
fn apply_foreign_key_rejects_an_unknown_foreign_key_name() {
    let child = NestedTestChild {
        id: 0,
        parent_id: 0,
        name: "alpha".to_string(),
    };

    let error = apply_foreign_key(child, "parnt_id", &serde_json::json!(1))
        .expect_err("a typo must not be silently ignored");
    assert!(error.to_string().contains("parnt_id"), "{error}");
}

#[test]
fn apply_foreign_key_does_not_coerce_a_string_primary_key_to_an_integer() {
    let child = NestedTestStringChild {
        id: 0,
        parent_id: String::new(),
        name: "alpha".to_string(),
    };

    let applied = apply_foreign_key(child, "parent_id", &serde_json::json!("00420"))
        .expect("a string parent key should apply verbatim");
    assert_eq!(applied.parent_id, "00420");
}

#[tokio::test]
async fn save_with_many_accepts_the_db_column_name_for_a_renamed_field() {
    let _db = setup_nested_test_db().await;

    let parent = NestedTestParent {
        id: 0,
        name: "parent".to_string(),
    };

    let (saved_parent, saved_children) = parent
        .save_with_many(
            vec![NestedTestAliasedChild {
                id: 0,
                owner_id: 0,
                name: "alpha".to_string(),
            }],
            "parent_id",
        )
        .await
        .expect("save_with_many should accept the DB column name");

    assert!(saved_parent.id > 0);
    assert_eq!(saved_children.len(), 1);
    assert_eq!(saved_children[0].owner_id, saved_parent.id);
}

#[tokio::test]
async fn save_with_many_rejects_an_unknown_foreign_key_without_writing_the_parent() {
    let _db = setup_nested_test_db().await;

    let parent = NestedTestParent {
        id: 0,
        name: "parent".to_string(),
    };

    let error = parent
        .save_with_many(nested_children(&["alpha"]), "parnt_id")
        .await
        .expect_err("an unresolvable foreign key must be an error");
    assert!(error.to_string().contains("parnt_id"), "{error}");

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
async fn save_with_many_rolls_back_the_parent_when_a_child_fails() {
    let _db = setup_nested_test_db().await;

    let parent = NestedTestParent {
        id: 0,
        name: "parent".to_string(),
    };

    let children = vec![
        NestedTestAliasedChild {
            id: 0,
            owner_id: 0,
            name: "duplicate".to_string(),
        },
        NestedTestAliasedChild {
            id: 0,
            owner_id: 0,
            name: "duplicate".to_string(),
        },
    ];

    parent
        .save_with_many(children, "parent_id")
        .await
        .expect_err("the duplicate child must fail the whole nested save");

    assert_eq!(
        NestedTestParent::query()
            .get()
            .await
            .expect("parent query should succeed")
            .len(),
        0
    );
    assert_eq!(
        NestedTestAliasedChild::query()
            .get()
            .await
            .expect("aliased child query should succeed")
            .len(),
        0
    );
}

#[tokio::test]
async fn update_with_one_rolls_back_the_parent_when_the_child_fails() {
    let _db = setup_nested_test_db().await;

    let (parent, mut children) = nested_parent()
        .save_with_many(
            vec![aliased_child("alpha"), aliased_child("beta")],
            "parent_id",
        )
        .await
        .expect("save_with_many should seed nested children");
    let parent_id = parent.id;
    let beta = children.pop().expect("two children were saved");

    // `name` is UNIQUE, so renaming "beta" onto "alpha" fails the child update.
    NestedTestParent {
        name: "renamed".to_string(),
        ..parent
    }
    .update_with_one(NestedTestAliasedChild {
        name: "alpha".to_string(),
        ..beta
    })
    .await
    .expect_err("the clashing child must fail the whole nested update");

    assert_eq!(stored_parent_name(parent_id).await, "parent");
}

#[tokio::test]
async fn update_with_many_rolls_back_the_parent_when_a_child_fails() {
    let _db = setup_nested_test_db().await;

    let (parent, children) = nested_parent()
        .save_with_many(
            vec![aliased_child("alpha"), aliased_child("beta")],
            "parent_id",
        )
        .await
        .expect("save_with_many should seed nested children");
    let parent_id = parent.id;
    let clashing_children = children
        .into_iter()
        .map(|child| NestedTestAliasedChild {
            name: "alpha".to_string(),
            ..child
        })
        .collect();

    NestedTestParent {
        name: "renamed".to_string(),
        ..parent
    }
    .update_with_many(clashing_children)
    .await
    .expect_err("the clashing child must fail the whole nested update");

    assert_eq!(stored_parent_name(parent_id).await, "parent");
}

#[tokio::test]
async fn delete_with_many_rolls_back_every_delete_when_a_child_fails() {
    let _db = setup_nested_test_db().await;

    let unlocked = NestedTestLockedChild {
        id: 0,
        parent_id: 0,
        locked: false,
    };
    let locked = NestedTestLockedChild {
        locked: true,
        ..unlocked.clone()
    };
    let (parent, children) = nested_parent()
        .save_with_many(vec![unlocked, locked], "parent_id")
        .await
        .expect("save_with_many should seed nested children");

    parent
        .delete_with_many(children)
        .await
        .expect_err("the locked child must fail the whole nested delete");

    assert_eq!(
        NestedTestLockedChild::query()
            .get()
            .await
            .expect("child query should succeed")
            .len(),
        2,
        "the unlocked child's delete must be rolled back"
    );
    assert_eq!(
        NestedTestParent::query()
            .get()
            .await
            .expect("parent query should succeed")
            .len(),
        1
    );
}

#[tokio::test]
async fn delete_with_many_deletes_each_child_then_the_parent() {
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
    assert_eq!(GlobalProfiler::stats().total_queries, 4);
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
async fn update_with_many_updates_each_related_model() {
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
    // One UPDATE for the parent plus one per related model: nested updates run
    // through each model's own `Model::update` so lifecycle callbacks and
    // validation are dispatched at a concrete call site.
    assert_eq!(GlobalProfiler::stats().total_queries, 4);

    for (expected, actual) in updated_children.iter().zip(children_after_update.iter()) {
        assert_eq!(expected.id, actual.id);
        assert_eq!(expected.parent_id, actual.parent_id);
        assert_eq!(expected.name, actual.name);
    }
}
