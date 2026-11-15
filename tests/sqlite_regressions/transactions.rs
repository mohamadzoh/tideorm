//! Rollbacks, aborted transactions and dirty-tracking baselines.

use crate::fixtures::{
    ChunkProjection, chunk_projection_database, file_database, global_memory_database,
    memory_database,
};
#[cfg(feature = "dirty-tracking")]
use crate::fixtures::{User, users_and_posts};
use tideorm::prelude::*;

#[path = "transactions/savepoints.rs"]
mod savepoints;

#[cfg(feature = "dirty-tracking")]
#[tokio::test]
async fn rollback_does_not_undo_an_independent_commit() {
    let outer = users_and_posts("outer", 20).await;
    let inner = users_and_posts("inner", 10).await;
    inner
        .exec_raw("INSERT INTO review_users VALUES(42, 'BEFORE', 1)")
        .await
        .unwrap();
    User::query_with(&inner)
        .where_eq("id", 42)
        .get()
        .await
        .unwrap();
    let handle = inner.clone();
    let outcome = outer
        .transaction(async move |_| {
            handle
                .transaction(async |_| {
                    Database::execute("UPDATE review_users SET name='COMMITTED' WHERE id=42")
                        .await?;
                    User::query().where_eq("id", 42).get().await?;
                    Ok(())
                })
                .await?;
            Err::<(), _>(tideorm::Error::query("roll back outer"))
        })
        .await;
    assert!(outcome.is_err());
    let model = User {
        id: 42,
        name: "COMMITTED".into(),
        active: true,
        posts: Default::default(),
    };
    assert_eq!(
        model.original_value("name").unwrap(),
        Some(Some(DbValue::from("COMMITTED")))
    );
    assert_eq!(
        inner
            .query_raw_json("SELECT name FROM review_users WHERE id=42")
            .await
            .unwrap()[0]["name"],
        "COMMITTED"
    );
    // A savepoint on the same database must still restore its parent's baseline.
    let nested = inner.clone();
    let outcome = inner
        .transaction(async move |_| {
            nested
                .transaction(async |_| {
                    Database::execute("UPDATE review_users SET name='ROLLBACK' WHERE id=42")
                        .await?;
                    User::query().where_eq("id", 42).get().await?;
                    Ok(())
                })
                .await?;
            Err::<(), _>(tideorm::Error::query("roll back parent"))
        })
        .await;
    assert!(outcome.is_err());
    assert_eq!(
        model.original_value("name").unwrap(),
        Some(Some(DbValue::from("COMMITTED")))
    );
}

#[cfg(feature = "dirty-tracking")]
#[tokio::test]
async fn bulk_rollback_restores_dirty_tracking_baselines() {
    #[tideorm::model(table = "review_bulk_snapshots")]
    struct Snapshot {
        #[tideorm(primary_key)]
        id: i64,
        name: String,
    }
    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.exec_raw("CREATE TABLE review_bulk_snapshots(id INTEGER PRIMARY KEY, name TEXT)")
        .await
        .unwrap();
    db.exec_raw("INSERT INTO review_bulk_snapshots VALUES(1, 'before')")
        .await
        .unwrap();
    let model = Snapshot::find_with(1, &db).await.unwrap().unwrap();
    assert_eq!(
        model.original_value("name").unwrap(),
        Some(Some(DbValue::from("before")))
    );
    let nested = db.clone();
    let result = db
        .transaction(async move |_| {
            nested
                .transaction(async |_| {
                    Snapshot::query().where_eq("id", 1).delete().await?;
                    Ok(())
                })
                .await?;
            Err::<(), _>(tideorm::Error::query("rollback bulk delete"))
        })
        .await;
    assert!(result.is_err());
    assert_eq!(
        model.original_value("name").unwrap(),
        Some(Some(DbValue::from("before")))
    );
    assert!(
        Snapshot::query_with(&db)
            .where_eq("id", 1)
            .exists()
            .await
            .unwrap()
    );
    db.transaction(async |_| {
        Snapshot::query().where_eq("id", 1).delete().await?;
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(model.original_value("name").unwrap(), None);
}

#[tokio::test]
async fn sqlite_aborted_transactions_block_writes_and_recover_the_pool() {
    let db = chunk_projection_database().await;
    // A plain update can abort the transaction through a trigger; callers
    // need not use any raw transaction-control statements themselves.
    db.exec_raw(
        "CREATE TRIGGER abort_nine BEFORE UPDATE ON review_chunk_projection \
         WHEN NEW.stored_id=9 BEGIN SELECT RAISE(ROLLBACK, 'abort fixture'); END",
    )
    .await
    .unwrap();
    let set_rank = |rank: i64, id: i64| {
        format!("UPDATE review_chunk_projection SET rank={rank} WHERE stored_id={id}")
    };
    for nested in [false, true] {
        let handle = db.clone();
        let result = db
            .transaction(async move |_| {
                handle.exec_raw(&set_rank(1, 7)).await?;
                if nested {
                    let inner = handle.clone();
                    let failure = handle
                        .transaction(async move |_| {
                            assert!(inner.exec_raw(&set_rank(2, 9)).await.is_err());
                            assert!(inner.exec_raw(&set_rank(3, 7)).await.is_err());
                            Ok(())
                        })
                        .await;
                    assert!(failure.is_err());
                } else {
                    assert!(handle.exec_raw(&set_rank(2, 9)).await.is_err());
                }
                // A transaction error saying why, as the refused commit is; it
                // used to be an internal one.
                let refused = handle.exec_raw(&set_rank(4, 7)).await.unwrap_err();
                assert!(
                    matches!(refused, tideorm::Error::Transaction { .. }),
                    "{refused:?}"
                );
                assert!(
                    refused
                        .to_string()
                        .contains("SQLite roll the whole transaction back"),
                    "{refused}"
                );
                assert!(handle.transaction(async |_| { Ok(()) }).await.is_err());
                Ok(())
            })
            .await;
        assert!(result.is_err());
        assert_eq!(
            ChunkProjection::find_with(7, &db)
                .await
                .unwrap()
                .unwrap()
                .rank,
            80
        );
        let fresh = db.clone();
        db.transaction(async move |_| {
            // ABORT, unlike ROLLBACK, leaves the surrounding transaction usable.
            assert!(
                fresh
                    .exec_raw("INSERT INTO review_chunk_projection VALUES(7,0)")
                    .await
                    .is_err()
            );
            fresh.exec_raw(&set_rank(81, 7)).await?;
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!(
            ChunkProjection::find_with(7, &db)
                .await
                .unwrap()
                .unwrap()
                .rank,
            81
        );
        db.exec_raw(&set_rank(80, 7)).await.unwrap();
    }
}

/// A handle on database A, used inside B's transaction nested in A's, joins
/// A's transaction: A's rollback must undo its write.
#[tokio::test]
async fn outer_database_handle_joins_its_transaction_inside_another_database() {
    let (a, _a_file) = file_database("outer").await;
    let (b, _b_file) = file_database("inner").await;
    a.exec_raw("CREATE TABLE outer_rows(id INTEGER PRIMARY KEY)")
        .await
        .unwrap();
    let (a_handle, b_handle) = (a.clone(), b.clone());
    let result: tideorm::Result<()> = a
        .transaction(async move |_| {
            let a_inner = a_handle.clone();
            b_handle
                .transaction(async move |_| {
                    a_inner
                        .exec_raw("INSERT INTO outer_rows VALUES (1)")
                        .await?;
                    Ok(())
                })
                .await?;
            Err(tideorm::Error::query("roll A back"))
        })
        .await;
    assert!(result.is_err());
    let rows = a.query_raw_json("SELECT id FROM outer_rows").await.unwrap();
    assert!(
        rows.is_empty(),
        "the write escaped A's transaction: {rows:?}"
    );
    drop((a, b));
}

#[cfg(feature = "dirty-tracking")]
#[tokio::test]
async fn batch_snapshots_are_undone_when_commit_fails() {
    #[tideorm::model(table = "failed_batch_snapshots")]
    struct Child {
        #[tideorm(primary_key)]
        id: i64,
        parent_id: i64,
        name: String,
    }
    let db = global_memory_database(
        "CREATE TABLE batch_snapshot_parent(id INTEGER PRIMARY KEY); \
         CREATE TABLE failed_batch_snapshots(id INTEGER PRIMARY KEY, \
         parent_id INTEGER REFERENCES batch_snapshot_parent(id) DEFERRABLE INITIALLY DEFERRED, name TEXT)",
    ).await;
    let first = Child {
        id: 1,
        parent_id: 7,
        name: "first".into(),
    };
    assert_eq!(first.original_value("name").unwrap(), None);
    let error = Child::insert_all(vec![
        first.clone(),
        Child {
            id: 2,
            parent_id: 7,
            name: "second".into(),
        },
    ])
    .await
    .unwrap_err();
    assert!(error.to_string().contains("FOREIGN KEY"), "{error}");
    assert_eq!(Child::query_with(&db).count().await.unwrap(), 0);
    assert_eq!(first.original_value("name").unwrap(), None);

    db.exec_raw("INSERT INTO batch_snapshot_parent VALUES (7)")
        .await
        .unwrap();
    let saved = Child::insert_all(vec![
        first,
        Child {
            id: 2,
            parent_id: 7,
            name: "second".into(),
        },
    ])
    .await
    .unwrap();
    assert_eq!(
        saved[0].original_value("name").unwrap(),
        Some(Some(DbValue::from("first")))
    );
}

#[cfg(feature = "dirty-tracking")]
#[tokio::test]
async fn batch_snapshots_are_undone_before_the_enclosing_transaction_commits() {
    #[tideorm::model(table = "nested_batch_snapshots")]
    struct Row {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        name: String,
    }
    let db = global_memory_database(
        "CREATE TABLE nested_batch_snapshots(id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT UNIQUE); \
         INSERT INTO nested_batch_snapshots VALUES(99, 'taken')",
    ).await;
    let first = Row {
        id: 10,
        name: "first".into(),
    };
    let checked = first.clone();
    db.transaction(async move |_| {
        // A batch mixing a held auto-increment key with an unset one
        // inserts row by row, so the first row is written, and its
        // baseline recorded, before the second fails. The failed batch
        // must undo that baseline, though its parent commits.
        let error = Row::insert_all(vec![
            first,
            Row {
                id: 0,
                name: "taken".into(),
            },
        ])
        .await
        .unwrap_err();
        assert!(error.to_string().contains("UNIQUE"), "{error}");
        assert_eq!(checked.original_value("name")?, None);
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(Row::query_with(&db).count().await.unwrap(), 1);
    assert_eq!(
        Row {
            id: 10,
            name: "first".into()
        }
        .original_value("name")
        .unwrap(),
        None
    );
}

/// A transaction handle that leaks out of a closure that fails is reported as
/// the leak, not hidden behind the closure's own error.
#[tokio::test]
async fn a_leaked_handle_is_reported_even_when_the_closure_fails() {
    let db = memory_database("").await;
    let stray = std::sync::Arc::new(std::sync::Mutex::new(None));
    let keep = stray.clone();
    let error = db
        .transaction(async move |_| {
            *keep.lock().unwrap() = Some(tideorm::database::__current_db()?);
            Err::<(), _>(tideorm::Error::query("rollback with a leaked handle"))
        })
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("transaction handle leaked outside the transaction scope"),
        "{error}"
    );
    drop(stray.lock().unwrap().take());
}

/// A batch of rows that all take their defaults is one `INSERT .. DEFAULT
/// VALUES` per row on SQLite, run as one statement: a row that fails leaves
/// none of the batch behind, and the trigger's own error is what the caller
/// sees, also when it rolls the whole transaction back.
#[tokio::test]
async fn a_failing_default_only_batch_leaves_no_rows_and_keeps_its_error() {
    #[tideorm::model(table = "review_default_rows")]
    struct DefaultRow {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
    }
    let db = global_memory_database(
        "CREATE TABLE review_default_rows(id INTEGER PRIMARY KEY AUTOINCREMENT, label TEXT NOT NULL DEFAULT 'x'); \
         CREATE TABLE review_default_marks(id INTEGER PRIMARY KEY)",
    )
    .await;
    let rows = || (0..3).map(|_| DefaultRow { id: 0 }).collect::<Vec<_>>();
    let count = |table: &'static str| {
        let db = db.clone();
        async move {
            db.query_raw_json(&format!("SELECT COUNT(*) AS n FROM {table}"))
                .await
                .unwrap()[0]["n"]
                .as_i64()
                .unwrap()
        }
    };
    for action in ["ABORT", "ROLLBACK"] {
        db.exec_raw(&format!(
            "CREATE TRIGGER stop_third BEFORE INSERT ON review_default_rows \
             WHEN (SELECT COUNT(*) FROM review_default_rows) >= 2 \
             BEGIN SELECT RAISE({action}, 'stop fixture'); END"
        ))
        .await
        .unwrap();

        let failure = DefaultRow::insert_all(rows()).await.unwrap_err();
        assert!(failure.to_string().contains("stop fixture"), "{failure}");
        assert_eq!(count("review_default_rows").await, 0, "{action} on its own");

        let handle = db.clone();
        let outcome = db
            .transaction(async move |_| {
                handle
                    .exec_raw("INSERT INTO review_default_marks VALUES(1)")
                    .await?;
                let failure = DefaultRow::insert_all(rows()).await.unwrap_err();
                assert!(failure.to_string().contains("stop fixture"), "{failure}");
                handle
                    .exec_raw("INSERT INTO review_default_marks VALUES(2)")
                    .await?;
                Ok(())
            })
            .await;
        if action == "ABORT" {
            outcome.unwrap();
            assert_eq!(count("review_default_marks").await, 2);
        } else {
            assert!(outcome.is_err(), "SQLite rolled the transaction back");
            assert_eq!(count("review_default_marks").await, 0);
        }
        assert_eq!(
            count("review_default_rows").await,
            0,
            "{action} in a transaction"
        );

        db.exec_raw("DROP TRIGGER stop_third; DELETE FROM review_default_marks")
            .await
            .unwrap();
        assert_eq!(DefaultRow::insert_all(rows()).await.unwrap().len(), 3);
        db.exec_raw("DELETE FROM review_default_rows")
            .await
            .unwrap();
    }
}

/// `Database::scope` runs calls that name no handle on its database, and
/// `Database::atomically` commits or rolls back a future borrowing locals.
#[tokio::test]
async fn scope_and_atomically_run_handle_less_calls_on_their_database() {
    let tenant = crate::fixtures::users_and_posts("tenant", 10).await;
    let other = crate::fixtures::users_and_posts("other", 20).await;
    let name = String::from("borrowed");
    let user = |id| crate::fixtures::User {
        id,
        name: name.clone(),
        active: true,
        posts: Default::default(),
    };
    type User = crate::fixtures::User;

    let found = tenant.scope(async { User::find(1).await }).await.unwrap();
    assert_eq!(found.unwrap().name, "tenant");
    let found = other.scope(async { User::find(1).await }).await.unwrap();
    assert_eq!(found.unwrap().name, "other");

    tenant
        .atomically(async { User::create(user(2)).await })
        .await
        .unwrap();
    let failed: tideorm::Result<()> = tenant
        .atomically(async {
            User::create(user(3)).await?;
            Err(tideorm::Error::query("stop after the insert"))
        })
        .await;
    assert!(failed.is_err());

    let ids = User::query_with(&tenant)
        .order_asc("id")
        .pluck::<i64>("id")
        .await
        .unwrap();
    assert_eq!(ids, vec![1, 2]);
    assert_eq!(User::query_with(&other).count().await.unwrap(), 1);
}

/// `is_dirty` and `is_field_dirty` compare with the row as loaded, and know
/// nothing of a model built by hand.
#[cfg(feature = "dirty-tracking")]
#[tokio::test]
async fn is_dirty_compares_with_the_loaded_row() {
    let db = users_and_posts("tenant", 10).await;
    db.exec_raw("INSERT INTO review_users VALUES(7341, 'loaded', 1)")
        .await
        .unwrap();
    let mut user = User::query_with(&db)
        .where_eq(User::columns.id, 7341)
        .first()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(user.is_dirty().unwrap(), Some(false));
    assert_eq!(
        user.is_field_dirty(User::columns.name).unwrap(),
        Some(false)
    );

    user.name = "renamed".into();
    assert_eq!(user.is_dirty().unwrap(), Some(true));
    assert_eq!(user.is_field_dirty("name").unwrap(), Some(true));
    assert_eq!(
        user.is_field_dirty(User::columns.active).unwrap(),
        Some(false)
    );
    assert!(user.is_field_dirty("missing").is_err());

    let built = User {
        id: 7342,
        name: "built".into(),
        active: true,
        posts: Default::default(),
    };
    assert_eq!(built.is_dirty().unwrap(), None);
    assert_eq!(built.is_field_dirty("name").unwrap(), None);
}

/// The closure is an async closure that may hold its `&Transaction` across an
/// `.await`, and the transaction's future is `Send`, so it can be spawned on a
/// multi-threaded runtime.
#[tokio::test]
async fn a_transaction_closure_holds_its_handle_and_can_be_spawned() {
    use tideorm::orm::ConnectionTrait;

    let db = memory_database("CREATE TABLE spawned_rows(id INTEGER PRIMARY KEY)").await;
    let handle = db.clone();
    tokio::spawn(async move {
        handle
            .transaction(async |tx| {
                let connection = tx.connection();
                for sql in [
                    "INSERT INTO spawned_rows VALUES (1)",
                    "INSERT INTO spawned_rows VALUES (2)",
                ] {
                    connection.execute_unprepared(sql).await.unwrap();
                    tokio::task::yield_now().await;
                }
                Ok(())
            })
            .await
    })
    .await
    .unwrap()
    .unwrap();
    let rows = db
        .query_raw_json("SELECT COUNT(*) AS n FROM spawned_rows")
        .await
        .unwrap();
    assert_eq!(rows[0]["n"], 2);
}
