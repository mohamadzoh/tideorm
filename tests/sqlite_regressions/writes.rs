//! Model writes: binary and JSON primary keys, nested saves, decimal ranges,
//! upserts, batch inserts, and the NaN floats SQLite would store unreadably.

use crate::fixtures::{
    MoneyRow, Post, global_memory_database, memory_database, row_count, users_and_posts,
};
use rust_decimal::Decimal;
use tideorm::model::{NestedSave, NestedSaveBuilder};
use tideorm::prelude::*;

#[tideorm::model(table = "audit_json_keys")]
struct JsonKey {
    #[tideorm(primary_key, auto_increment = false)]
    id: Json,
    amount: i64,
}

#[tideorm::model(table = "audit_binary_keys")]
struct BinaryKey {
    #[tideorm(primary_key, auto_increment = false)]
    id: Vec<u8>,
    amount: i64,
}

/// A JSON array or a byte vector is one key, not the parts of a composite one.
#[tokio::test]
async fn json_array_and_binary_keys_are_single_keys() {
    let db = memory_database(
        "CREATE TABLE audit_json_keys(id JSON PRIMARY KEY,amount INTEGER); \
         INSERT INTO audit_json_keys VALUES('[]',1),('[1]',2),('[1,2]',3); \
         CREATE TABLE audit_binary_keys(id BLOB PRIMARY KEY,amount INTEGER); \
         INSERT INTO audit_binary_keys VALUES(X'0102',7)",
    )
    .await;
    for (key, amount) in [(json!([]), 1), (json!([1]), 2), (json!([1, 2]), 3)] {
        assert_eq!(
            JsonKey::query_with(&db)
                .find(key.clone())
                .await
                .unwrap()
                .unwrap()
                .amount,
            amount
        );
        assert_eq!(
            JsonKey::find_with(key, &db).await.unwrap().unwrap().amount,
            amount
        );
    }
    assert_eq!(
        BinaryKey::query_with(&db)
            .find(vec![1, 2])
            .await
            .unwrap()
            .unwrap()
            .amount,
        7
    );
    assert_eq!(
        BinaryKey::find_with(vec![1, 2], &db)
            .await
            .unwrap()
            .unwrap()
            .amount,
        7
    );
}

#[tokio::test]
async fn generated_key_upserts_validate_update_and_exclusion_columns() {
    #[tideorm::model(table = "audit_upsert_columns")]
    struct UpsertRow {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        name: String,
    }
    let db = memory_database("CREATE TABLE audit_upsert_columns(id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL)").await;
    db.transaction(|_| {
        Box::pin(async {
            for id in [0, 1] {
                for exclusion in [false, true] {
                    let builder = UpsertRow::on_conflict(vec!["id"]);
                    let builder = if exclusion {
                        builder.update_all_except(vec!["missing"])
                    } else {
                        builder.update_columns(vec!["missing"])
                    };
                    let error = builder
                        .insert(UpsertRow {
                            id,
                            name: "invalid".into(),
                        })
                        .await
                        .unwrap_err();
                    assert!(error.to_string().contains("missing"), "{error}");
                    assert_eq!(UpsertRow::query().count().await?, 0);
                }
            }
            let inserted = UpsertRow::on_conflict(vec!["id"])
                .update_columns(vec!["name"])
                .insert(UpsertRow {
                    id: 0,
                    name: "initial".into(),
                })
                .await?;
            assert!(inserted.id > 0);
            let updated = UpsertRow::on_conflict(vec!["id"])
                .update_columns(vec!["name"])
                .insert(UpsertRow {
                    id: inserted.id,
                    name: "updated".into(),
                })
                .await?;
            assert_eq!(updated.name, "updated");
            assert_eq!(UpsertRow::query().count().await?, 1);
            Ok(())
        })
    })
    .await
    .unwrap();
}

#[tideorm::model(table = "audit_nested_binary_parents")]
struct Parent {
    #[tideorm(primary_key)]
    id: Vec<u8>,
    name: String,
}

#[tideorm::model(table = "audit_nested_binary_children")]
struct Child {
    #[tideorm(primary_key)]
    id: i64,
    parent_id: Vec<u8>,
}

#[tideorm::model(table = "audit_nested_json_parents")]
struct JsonParent {
    #[tideorm(primary_key)]
    id: serde_json::Value,
    name: String,
}

#[tideorm::model(table = "audit_nested_json_children")]
struct JsonChild {
    #[tideorm(primary_key)]
    id: i64,
    parent_id: serde_json::Value,
}

#[tideorm::model(table = "audit_nested_composite_parents")]
struct CompositeParent {
    #[tideorm(primary_key)]
    left_id: i64,
    #[tideorm(primary_key)]
    right_id: i64,
    name: String,
}

/// The nested-save tables, in the global database.
async fn database() -> Database {
    global_memory_database(
        "CREATE TABLE audit_nested_binary_parents(id BLOB PRIMARY KEY,name TEXT); \
         CREATE TABLE audit_nested_binary_children(id INTEGER PRIMARY KEY,\
         parent_id BLOB REFERENCES audit_nested_binary_parents(id)); \
         CREATE TABLE audit_nested_json_parents(id JSON PRIMARY KEY,name TEXT); \
         CREATE TABLE audit_nested_json_children(id INTEGER PRIMARY KEY,parent_id JSON); \
         CREATE TABLE audit_nested_composite_parents(left_id INTEGER,right_id INTEGER,name TEXT,\
         PRIMARY KEY(left_id,right_id))",
    )
    .await
}

#[tokio::test]
async fn nested_binary_keys_preserve_bytes_across_all_entry_points() {
    let _db = database().await;
    for mode in 0..3 {
        let key = vec![0, 255, mode];
        let parent = Parent {
            id: key.clone(),
            name: "nested".into(),
        };
        let child = Child {
            id: i64::from(mode) + 1,
            parent_id: vec![],
        };
        let saved_child = match mode {
            0 => parent.save_with_one(child, "parent_id").await.unwrap().1,
            1 => parent
                .save_with_many(vec![child], "parent_id")
                .await
                .unwrap()
                .1
                .remove(0),
            _ => NestedSaveBuilder::new(parent)
                .with_one(child, "parent_id")
                .save()
                .await
                .unwrap()
                .1
                .remove(0)
                .into_one::<Child>()
                .unwrap(),
        };
        assert_eq!(saved_child.parent_id, key);
        assert!(Parent::find(key.clone()).await.unwrap().is_some());
        assert_eq!(
            Child::find(i64::from(mode) + 1)
                .await
                .unwrap()
                .unwrap()
                .parent_id,
            key
        );
    }
}

#[tokio::test]
async fn nested_json_keys_preserve_array_and_object_values() {
    let _db = database().await;
    for (kind, key) in [json!([1, 2]), json!({"a":1})].into_iter().enumerate() {
        for mode in 0..3 {
            let id = (kind * 3 + mode + 1) as i64;
            let parent = JsonParent {
                id: key.clone(),
                name: "nested".into(),
            };
            let child = JsonChild {
                id,
                parent_id: serde_json::Value::Null,
            };
            let saved_child = match mode {
                0 => parent.save_with_one(child, "parent_id").await.unwrap().1,
                1 => parent
                    .save_with_many(vec![child], "parent_id")
                    .await
                    .unwrap()
                    .1
                    .remove(0),
                _ => NestedSaveBuilder::new(parent)
                    .with_many(vec![child], "parent_id")
                    .save()
                    .await
                    .unwrap()
                    .1
                    .remove(0)
                    .into_many::<JsonChild>()
                    .unwrap()
                    .remove(0),
            };
            assert_eq!(saved_child.parent_id, key);
            assert!(JsonParent::find(key.clone()).await.unwrap().is_some());
            assert_eq!(JsonChild::find(id).await.unwrap().unwrap().parent_id, key);
        }
    }
}

#[tokio::test]
async fn actual_composite_keys_still_reject_and_roll_back() {
    let _db = database().await;
    for mode in 0..3 {
        let parent = CompositeParent {
            left_id: 1,
            right_id: mode,
            name: "nested".into(),
        };
        let child = Child {
            id: 1,
            parent_id: vec![],
        };
        let result = match mode {
            0 => parent.save_with_one(child, "parent_id").await.map(|_| ()),
            1 => parent
                .save_with_many(vec![child], "parent_id")
                .await
                .map(|_| ()),
            _ => NestedSaveBuilder::new(parent)
                .with_one(child, "parent_id")
                .save()
                .await
                .map(|_| ()),
        };
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("composite primary keys")
        );
        assert!(CompositeParent::find((1, mode)).await.unwrap().is_none());
    }
    assert_eq!(Child::query().count().await.unwrap(), 0);
}

#[tokio::test]
async fn decimal_model_boundaries_fail_before_autocommit_writes() {
    let db = global_memory_database(
        "CREATE TABLE resume_decimals(id INTEGER PRIMARY KEY,amount REAL,marker INTEGER)",
    )
    .await;
    let saved = MoneyRow::create(MoneyRow {
        id: 1,
        amount: Some(Decimal::ONE),
        marker: 0,
    })
    .await
    .unwrap();
    for value in [Decimal::MAX, Decimal::MIN] {
        assert!(
            MoneyRow::create(MoneyRow {
                id: 2,
                amount: Some(value),
                marker: 0
            })
            .await
            .is_err()
        );
        assert!(
            MoneyRow {
                id: 1,
                amount: Some(value),
                marker: 0
            }
            .update()
            .await
            .is_err()
        );
        assert!(
            MoneyRow::insert_or_update(
                MoneyRow {
                    id: 2,
                    amount: Some(value),
                    marker: 0
                },
                vec!["id"]
            )
            .await
            .is_err()
        );
        assert_eq!(MoneyRow::query_with(&db).count().await.unwrap(), 1);
        assert_eq!(
            MoneyRow::find_with(1, &db).await.unwrap().unwrap().amount,
            saved.amount
        );
    }
    // The nearest representable f64 below 2^96 remains usable, with either sign.
    for value in [
        79_228_162_514_264_337_593_543_950_336_f64.next_down(),
        -79_228_162_514_264_337_593_543_950_336_f64.next_down(),
    ] {
        let amount = Decimal::try_from(value).unwrap();
        MoneyRow {
            id: 1,
            amount: Some(amount),
            marker: 0,
        }
        .update()
        .await
        .unwrap();
        assert!(MoneyRow::find_with(1, &db).await.unwrap().is_some());
    }
    Database::reset_global();
}

#[tokio::test]
async fn null_related_keys_never_change_the_pivot() {
    #[tideorm::model(table = "review_nullable_pivot")]
    struct Pivot {
        #[tideorm(primary_key)]
        id: i64,
        owner_id: i64,
        related_id: Option<i64>,
    }
    let db = users_and_posts("owner", 10).await;
    Database::set_global(db.clone()).unwrap();
    db.exec_raw("CREATE TABLE review_nullable_pivot(id INTEGER PRIMARY KEY,owner_id INTEGER,related_id INTEGER, UNIQUE(owner_id,related_id))").await.unwrap();
    db.exec_raw("INSERT INTO review_nullable_pivot VALUES(1,1,10)")
        .await
        .unwrap();
    let relation = HasManyThrough::<Post, Pivot>::new(
        "owner_id",
        "related_id",
        "id",
        "id",
        "review_nullable_pivot",
    )
    .with_parent_pk(json!(1));
    assert!(relation.sync(vec![Some(10_i64), None]).await.is_err());
    assert!(relation.attach(None::<i64>).await.is_err());
    let rows = Pivot::all().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].related_id, Some(10));
    Database::reset_global();
}

#[tideorm::model(table = "stamped_rows")]
struct StampedRow {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

thread_local! {
    static DELETED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

impl tideorm::callbacks::Callbacks for StampedRow {
    fn after_delete(&self) -> tideorm::Result<()> {
        DELETED.with(|deleted| deleted.set(deleted.get() + 1));
        Ok(())
    }
}

/// An insert stamps `created_at` and `updated_at` with one instant, and
/// `after_delete` runs only when a row was deleted.
#[tokio::test]
async fn an_insert_stamps_one_instant_and_after_delete_needs_a_deleted_row() {
    let _db = global_memory_database(
        "CREATE TABLE stamped_rows(id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, \
         created_at TEXT, updated_at TEXT)",
    )
    .await;
    for name in ["a", "b", "c"] {
        let row = StampedRow::create(StampedRow {
            id: 0,
            name: name.into(),
            created_at: Default::default(),
            updated_at: Default::default(),
        })
        .await
        .unwrap();
        assert_eq!(row.created_at, row.updated_at);
    }

    let row = StampedRow::find(1).await.unwrap().unwrap();
    let copy = row.clone();
    assert_eq!(row.delete().await.unwrap(), 1);
    assert_eq!(copy.delete().await.unwrap(), 0);
    assert_eq!(DELETED.with(std::cell::Cell::get), 1);
    Database::reset_global();
}

#[tideorm::model(table = "batch_members")]
struct Member {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

/// `insert_all` stays all-or-nothing when a trigger leaves a row out of its
/// one `INSERT .. RETURNING`: that statement ran outside a transaction, so
/// the other rows stayed although the call failed on the missing one.
#[tokio::test]
async fn insert_all_keeps_no_row_of_a_batch_a_trigger_cut_short() {
    let db = global_memory_database(
        "CREATE TABLE batch_members(id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL); \
         CREATE TRIGGER batch_members_skip BEFORE INSERT ON batch_members \
         WHEN NEW.name = 'Skipped' BEGIN SELECT RAISE(IGNORE); END",
    )
    .await;
    let member = |name: &str| Member {
        id: 0,
        name: name.to_string(),
    };

    let result = Member::insert_all(vec![member("Kept"), member("Skipped")]).await;
    assert!(result.is_err(), "a row came back missing");
    assert_eq!(row_count(&db, "batch_members").await, 0);
}

#[tideorm::model(table = "audit_values")]
struct Typed {
    #[tideorm(primary_key)]
    id: i64,
    token: Uuid,
    amount: Option<f64>,
}

/// SQLite stores a NaN float, which the model then cannot read back, so every
/// model write refuses one, and a batch writes none of its rows.
#[tokio::test]
async fn model_writes_refuse_a_nan_float() {
    let db = memory_database(
        "CREATE TABLE audit_values(id INTEGER PRIMARY KEY,token BLOB,amount REAL); \
         INSERT INTO audit_values VALUES(1,X'00112233445566778899AABBCCDDEEFF',1)",
    )
    .await;
    db.transaction(|_| {
        Box::pin(async {
            assert!(
                Typed::create(Typed {
                    id: 2,
                    token: Uuid::nil(),
                    amount: Some(f64::NAN)
                })
                .await
                .is_err()
            );
            let mut row = Typed::find(1).await?.unwrap();
            row.amount = Some(f64::NAN);
            assert!(row.update().await.is_err());
            assert!(
                Typed::insert_all(vec![
                    Typed {
                        id: 2,
                        token: Uuid::nil(),
                        amount: Some(2.0)
                    },
                    Typed {
                        id: 3,
                        token: Uuid::nil(),
                        amount: Some(f64::NAN)
                    },
                ])
                .await
                .is_err()
            );
            assert!(
                Typed::insert_or_update(
                    Typed {
                        id: 1,
                        token: Uuid::nil(),
                        amount: Some(f64::NAN)
                    },
                    vec!["id"]
                )
                .await
                .is_err()
            );
            Ok(())
        })
    })
    .await
    .unwrap();
    assert_eq!(Typed::query_with(&db).count().await.unwrap(), 1);
    assert_eq!(
        Typed::find_with(1, &db).await.unwrap().unwrap().amount,
        Some(1.0)
    );
}
