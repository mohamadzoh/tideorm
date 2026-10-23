//! Rollbacks, aborted transactions and dirty-tracking baselines.

use crate::fixtures::{
    ChunkProjection, chunk_projection_database, file_database, memory_database, slow_count,
};
#[cfg(feature = "dirty-tracking")]
use crate::fixtures::{User, global_memory_database, users_and_posts};
use tideorm::prelude::*;

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
        .transaction(move |_| {
            Box::pin(async move {
                handle
                    .transaction(|_| {
                        Box::pin(async {
                            Database::execute(
                                "UPDATE review_users SET name='COMMITTED' WHERE id=42",
                            )
                            .await?;
                            User::query().where_eq("id", 42).get().await?;
                            Ok(())
                        })
                    })
                    .await?;
                Err::<(), _>(tideorm::Error::query("roll back outer"))
            })
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
        Some(Some(json!("COMMITTED")))
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
        .transaction(move |_| {
            Box::pin(async move {
                nested
                    .transaction(|_| {
                        Box::pin(async {
                            Database::execute(
                                "UPDATE review_users SET name='ROLLBACK' WHERE id=42",
                            )
                            .await?;
                            User::query().where_eq("id", 42).get().await?;
                            Ok(())
                        })
                    })
                    .await?;
                Err::<(), _>(tideorm::Error::query("roll back parent"))
            })
        })
        .await;
    assert!(outcome.is_err());
    assert_eq!(
        model.original_value("name").unwrap(),
        Some(Some(json!("COMMITTED")))
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
        Some(Some(json!("before")))
    );
    let nested = db.clone();
    let result = db
        .transaction(move |_| {
            Box::pin(async move {
                nested
                    .transaction(|_| {
                        Box::pin(async {
                            Snapshot::query().where_eq("id", 1).delete().await?;
                            Ok(())
                        })
                    })
                    .await?;
                Err::<(), _>(tideorm::Error::query("rollback bulk delete"))
            })
        })
        .await;
    assert!(result.is_err());
    assert_eq!(
        model.original_value("name").unwrap(),
        Some(Some(json!("before")))
    );
    assert!(
        Snapshot::query_with(&db)
            .where_eq("id", 1)
            .exists()
            .await
            .unwrap()
    );
    db.transaction(|_| {
        Box::pin(async {
            Snapshot::query().where_eq("id", 1).delete().await?;
            Ok(())
        })
    })
    .await
    .unwrap();
    assert_eq!(model.original_value("name").unwrap(), None);
}

#[tokio::test]
async fn sqlite_aborted_transactions_block_writes_and_recover_the_pool() {
    let db = chunk_projection_database().await;
    // A normal insert can abort the transaction through a trigger; callers
    // need not use any raw transaction-control statements themselves.
    db.exec_raw("CREATE TRIGGER abort_nine BEFORE UPDATE ON review_chunk_projection WHEN NEW.stored_id=9 BEGIN SELECT RAISE(ROLLBACK, 'abort fixture'); END").await.unwrap();
    for nested in [false, true] {
        let handle = db.clone();
        let result = db.transaction(|_| Box::pin(async move {
            handle.exec_raw("UPDATE review_chunk_projection SET rank=1 WHERE stored_id=7").await?;
            if nested {
                let inner = handle.clone();
                let failure = handle.transaction(|_| Box::pin(async move {
                    assert!(inner.exec_raw("UPDATE review_chunk_projection SET rank=2 WHERE stored_id=9").await.is_err());
                    assert!(inner.exec_raw("UPDATE review_chunk_projection SET rank=3 WHERE stored_id=7").await.is_err());
                    Ok(())
                })).await;
                assert!(failure.is_err());
            } else {
                assert!(handle.exec_raw("UPDATE review_chunk_projection SET rank=2 WHERE stored_id=9").await.is_err());
            }
            // A transaction error saying why, as the refused commit is; it
            // used to be an internal one.
            let refused = handle.exec_raw("UPDATE review_chunk_projection SET rank=4 WHERE stored_id=7").await.unwrap_err();
            assert!(matches!(refused, tideorm::Error::Transaction { .. }), "{refused:?}");
            assert!(refused.to_string().contains("SQLite roll the whole transaction back"), "{refused}");
            assert!(handle.transaction(|_| Box::pin(async { Ok(()) })).await.is_err());
            Ok(())
        })).await;
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
        db.transaction(|_| {
            Box::pin(async move {
                // ABORT, unlike ROLLBACK, leaves the surrounding transaction usable.
                assert!(
                    fresh
                        .exec_raw("INSERT INTO review_chunk_projection VALUES(7,0)")
                        .await
                        .is_err()
                );
                fresh
                    .exec_raw("UPDATE review_chunk_projection SET rank=81 WHERE stored_id=7")
                    .await?;
                Ok(())
            })
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
        db.exec_raw("UPDATE review_chunk_projection SET rank=80 WHERE stored_id=7")
            .await
            .unwrap();
    }
}

/// Once a savepoint has finished its statement and is idle, cancelling its
/// closure rolls back only its own work. Wait for that boundary explicitly:
/// cancelling after an arbitrary poll can instead hit begin or commit, where
/// the enclosing transaction must abort (covered by the other cancellation tests).
#[tokio::test]
async fn a_cancelled_idle_savepoint_rolls_back_only_its_own_work() {
    let db = memory_database("CREATE TABLE cancel_rows(id INTEGER PRIMARY KEY)").await;
    let handle = db.clone();
    db.transaction(move |_| {
        Box::pin(async move {
            handle.exec_raw("INSERT INTO cancel_rows VALUES (1)").await?;
            let inner_handle = handle.clone();
            let (ready, idle) = tokio::sync::oneshot::channel();
            let mut inner = Box::pin(handle.transaction(move |_| {
                Box::pin(async move {
                    inner_handle.exec_raw("INSERT INTO cancel_rows VALUES (2)").await?;
                    ready.send(()).unwrap();
                    std::future::pending::<tideorm::Result<()>>().await
                })
            }));
            tokio::select! {
                outcome = &mut inner => panic!("the nested transaction ended before cancellation: {outcome:?}"),
                ready = idle => ready.unwrap(),
            }
            drop(inner);
            handle.exec_raw("INSERT INTO cancel_rows VALUES (3)").await?;
            Ok(())
        })
    })
    .await
    .unwrap();
    assert_eq!(
        db.query_raw_json("SELECT id FROM cancel_rows ORDER BY id")
            .await
            .unwrap(),
        vec![json!({"id": 1}), json!({"id": 3})]
    );
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
        .transaction(move |_| {
            Box::pin(async move {
                let a_inner = a_handle.clone();
                b_handle
                    .transaction(move |_| {
                        Box::pin(async move {
                            a_inner
                                .exec_raw("INSERT INTO outer_rows VALUES (1)")
                                .await?;
                            Ok(())
                        })
                    })
                    .await?;
                Err(tideorm::Error::query("roll A back"))
            })
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

/// A savepoint of A's transaction opened inside B's belongs to A: a handle on
/// B used inside it runs on B's transaction, not on A's savepoint.
#[tokio::test]
async fn a_savepoint_opened_inside_another_databases_transaction_keeps_its_database() {
    let (a, _a_file) = file_database("savepoint-outer").await;
    let (b, _b_file) = file_database("savepoint-inner").await;
    b.exec_raw("CREATE TABLE inner_rows(id INTEGER PRIMARY KEY)")
        .await
        .unwrap();
    let (a_handle, b_handle) = (a.clone(), b.clone());
    a.transaction(move |_| {
        Box::pin(async move {
            let (a_inner, b_inner) = (a_handle.clone(), b_handle.clone());
            b_handle
                .transaction(move |_| {
                    Box::pin(async move {
                        a_inner
                            .transaction(move |_| {
                                Box::pin(async move {
                                    b_inner.exec_raw("INSERT INTO inner_rows VALUES (1)").await
                                })
                            })
                            .await
                    })
                })
                .await
        })
    })
    .await
    .unwrap();
    let rows = b.query_raw_json("SELECT id FROM inner_rows").await.unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    drop((a, b));
}

/// A nested transaction cancelled while another statement of its connection
/// runs cannot be rolled back at that moment, and the driver used to panic
/// trying. The enclosing transaction instead refuses to commit, so it keeps
/// nothing of either.
#[tokio::test]
async fn a_nested_transaction_cancelled_mid_statement_aborts_the_enclosing_one() {
    use std::time::Duration;
    let (db, _file) = file_database("cancelled-mid-statement").await;
    db.exec_raw("CREATE TABLE mid_rows(id INTEGER PRIMARY KEY)")
        .await
        .unwrap();
    let handle = db.clone();
    let result: tideorm::Result<()> = db
        .transaction(move |_| {
            Box::pin(async move {
                handle.exec_raw("INSERT INTO mid_rows VALUES (1)").await?;
                let nested = tokio::time::timeout(
                    Duration::from_millis(150),
                    handle.transaction(|_| {
                        Box::pin(async {
                            tokio::time::sleep(Duration::from_secs(10)).await;
                            Ok(())
                        })
                    }),
                );
                let slow = async {
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    handle
                        .query_raw_json(&format!("SELECT {} AS n", slow_count(20_000_000)))
                        .await
                };
                let (nested, slow) = tokio::join!(nested, slow);
                assert!(nested.is_err(), "the nested transaction was not cancelled");
                slow?;
                Ok(())
            })
        })
        .await;
    let error = result.unwrap_err().to_string();
    assert!(error.contains("cancelled"), "{error}");
    let rows = db.query_raw_json("SELECT id FROM mid_rows").await.unwrap();
    assert!(rows.is_empty(), "{rows:?}");
    drop(db);
}

/// A nested transaction whose handle leaked outside its closure leaves its
/// savepoint open. The enclosing transaction used to release that savepoint
/// as if it were its own and report a commit, while the transaction stayed
/// open until the stray handle dropped and rolled everything back; it now
/// refuses to commit.
#[tokio::test]
async fn a_leaked_savepoint_aborts_the_enclosing_transaction() {
    let db = memory_database("CREATE TABLE leak_rows(id INTEGER PRIMARY KEY)").await;
    let stray = std::sync::Arc::new(std::sync::Mutex::new(None));
    let (handle, keep) = (db.clone(), stray.clone());
    let result: tideorm::Result<()> = db
        .transaction(move |_| {
            Box::pin(async move {
                handle.exec_raw("INSERT INTO leak_rows VALUES (1)").await?;
                let leaked = handle
                    .transaction(move |_| {
                        Box::pin(async move {
                            *keep.lock().unwrap() = Some(tideorm::database::__current_db()?);
                            Ok(())
                        })
                    })
                    .await;
                assert!(leaked.is_err(), "the leak was not reported");
                assert!(
                    handle
                        .exec_raw("INSERT INTO leak_rows VALUES (2)")
                        .await
                        .is_err()
                );
                Ok(())
            })
        })
        .await;
    let error = result.unwrap_err().to_string();
    assert!(error.contains("leaked"), "{error}");
    drop(stray.lock().unwrap().take());
    let rows = db.query_raw_json("SELECT id FROM leak_rows").await.unwrap();
    assert!(rows.is_empty(), "{rows:?}");
}

/// A nested transaction or a batch insert cancelled while it waits for the
/// connection another statement holds never started, so nothing panics and
/// the enclosing transaction commits what it wrote.
#[tokio::test]
async fn a_savepoint_cancelled_while_it_waits_for_the_connection_never_starts() {
    use std::time::Duration;

    #[tideorm::model(table = "waiting_rows")]
    struct WaitingRow {
        #[tideorm(primary_key)]
        id: i64,
    }

    for batch in [false, true] {
        let (db, _file) = file_database("cancelled-while-waiting").await;
        db.exec_raw("CREATE TABLE waiting_rows(id INTEGER PRIMARY KEY)")
            .await
            .unwrap();
        let handle = db.clone();
        let result: tideorm::Result<()> = db
            .transaction(move |_| {
                Box::pin(async move {
                    handle
                        .exec_raw("INSERT INTO waiting_rows VALUES (1)")
                        .await?;
                    let slow_sql = format!("SELECT {} AS n", slow_count(20_000_000));
                    let slow = handle.query_raw_json(&slow_sql);
                    let waiting = async {
                        tokio::time::sleep(Duration::from_millis(30)).await;
                        let savepoint = async {
                            if batch {
                                WaitingRow::insert_all(vec![
                                    WaitingRow { id: 2 },
                                    WaitingRow { id: 3 },
                                ])
                                .await
                                .map(drop)
                            } else {
                                handle.transaction(|_| Box::pin(async { Ok(()) })).await
                            }
                        };
                        tokio::time::timeout(Duration::from_millis(100), savepoint).await
                    };
                    let (slow, waiting) = tokio::join!(slow, waiting);
                    assert!(waiting.is_err(), "the savepoint did not wait");
                    slow?;
                    Ok(())
                })
            })
            .await;
        result.unwrap();
        let rows = db
            .query_raw_json("SELECT id FROM waiting_rows")
            .await
            .unwrap();
        assert_eq!(rows.len(), 1, "{rows:?}");
        drop(db);
    }
}

/// Two savepoints opened side by side in one transaction take turns. sqlx
/// names a savepoint by how many are open on its connection, so one that
/// ended while a later one was still open ended that one in its place, and
/// the later one's rollback then undid the rows the first had committed.
#[tokio::test]
async fn side_by_side_savepoints_in_one_transaction_take_turns() {
    use std::time::Duration;

    #[tideorm::model(table = "side_by_side_rows")]
    struct SideRow {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        email: String,
    }
    let row = |email: &str| SideRow {
        id: 0,
        email: email.to_string(),
    };

    let db = memory_database(
        "CREATE TABLE side_by_side_rows(id INTEGER PRIMARY KEY AUTOINCREMENT,          email TEXT NOT NULL UNIQUE)",
    )
    .await;
    let handle = db.clone();
    let result: tideorm::Result<()> = db
        .transaction(move |_| {
            Box::pin(async move {
                // The first opens, writes, and waits while the second opens;
                // it commits while the second, which then fails, is open.
                let first = handle.transaction(move |_| {
                    Box::pin(async move {
                        SideRow::create(row("a@x")).await?;
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        SideRow::create(row("b@x")).await.map(drop)
                    })
                });
                let second = async {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    handle
                        .transaction(move |_| {
                            Box::pin(async move {
                                SideRow::create(row("c@x")).await?;
                                tokio::time::sleep(Duration::from_millis(40)).await;
                                SideRow::create(row("c@x")).await.map(drop)
                            })
                        })
                        .await
                };
                let (written, refused) = tokio::join!(first, second);
                written?;
                assert!(refused.is_err(), "the duplicate was written");
                Ok(())
            })
        })
        .await;
    result.unwrap();
    let mut emails: Vec<String> = db
        .query_raw_json("SELECT email FROM side_by_side_rows")
        .await
        .unwrap()
        .iter()
        .map(|row| row["email"].as_str().unwrap().to_string())
        .collect();
    emails.sort();
    assert_eq!(emails, ["a@x", "b@x"]);
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
        Some(Some(json!("first")))
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
    db.transaction(move |_| {
        Box::pin(async move {
            // Explicit auto-increment keys use per-row inserts. The first load
            // must be undone by this failed batch, even if its parent commits.
            let error = Row::insert_all(vec![
                first,
                Row {
                    id: 11,
                    name: "taken".into(),
                },
            ])
            .await
            .unwrap_err();
            assert!(error.to_string().contains("UNIQUE"), "{error}");
            assert_eq!(checked.original_value("name")?, None);
            Ok(())
        })
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
