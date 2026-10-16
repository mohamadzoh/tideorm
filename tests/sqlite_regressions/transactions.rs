//! Rollbacks, aborted transactions and dirty-tracking baselines.

use crate::fixtures::{ChunkProjection, chunk_projection_database};
#[cfg(feature = "dirty-tracking")]
use crate::fixtures::{User, users_and_posts};
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
            assert!(handle.exec_raw("UPDATE review_chunk_projection SET rank=4 WHERE stored_id=7").await.is_err());
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
