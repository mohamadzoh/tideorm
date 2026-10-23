//! Concurrent lookups share one managed identity, and rolled-back reads leave none.

use crate::fixtures::{Number, User, global_memory_database, users_and_posts};
use tideorm::prelude::*;

#[tokio::test]
async fn clear_survives_outer_rollback_even_after_refilling_the_map() {
    use std::sync::Arc;
    use tideorm::entity_manager::EntityManager;
    for refill in [false, true] {
        let db = crate::fixtures::memory_database(
            "CREATE TABLE audit_numbers(id INTEGER PRIMARY KEY,value INTEGER); \
             INSERT INTO audit_numbers VALUES(1,10)",
        )
        .await;
        let manager = EntityManager::new(Arc::new(db.clone()));
        manager.find::<Number>(1).await.unwrap().unwrap();
        let m = manager.clone();
        let result: tideorm::Result<()> = db
            .transaction(move |_| {
                Box::pin(async move {
                    m.save(&Number { id: 1, value: 20 }).await?;
                    m.clear();
                    if refill {
                        m.register(Number { id: 1, value: 30 }).await;
                    }
                    Err(tideorm::Error::query("rollback outer"))
                })
            })
            .await;
        assert!(result.is_err());
        // A read registered in the rolled-back transaction is evicted too.
        db.exec_raw("UPDATE audit_numbers SET value=40")
            .await
            .unwrap();
        assert_eq!(manager.find::<Number>(1).await.unwrap().unwrap().value, 40);
    }
}

#[tokio::test]
async fn concurrent_persist_and_merge_share_one_identity() {
    use std::sync::{Arc, Barrier};
    use tideorm::entity_manager::EntityManager;
    let db =
        global_memory_database("CREATE TABLE audit_numbers(id INTEGER PRIMARY KEY,value INTEGER)")
            .await;
    for merge in [false, true] {
        for attempt in 1..=12 {
            db.exec_raw("DELETE FROM audit_numbers").await.unwrap();
            if merge {
                db.exec_raw("INSERT INTO audit_numbers VALUES(1,10)")
                    .await
                    .unwrap();
            }
            let manager = EntityManager::new(Arc::new(db.clone()));
            let barrier = Arc::new(Barrier::new(16));
            let handles = std::thread::scope(|scope| {
                let tasks = (0..16)
                    .map(|_| {
                        let manager = manager.clone();
                        let barrier = barrier.clone();
                        scope.spawn(move || {
                            barrier.wait();
                            if merge {
                                manager.merge(Number { id: 1, value: 10 }).unwrap()
                            } else {
                                manager.persist(Number { id: 1, value: 10 })
                            }
                        })
                    })
                    .collect::<Vec<_>>();
                tasks
                    .into_iter()
                    .map(|t| t.join().unwrap())
                    .collect::<Vec<_>>()
            });
            handles[0].edit(|row| row.value = 99);
            assert!(
                handles.iter().all(|h| h.get().value == 99),
                "merge={merge} attempt={attempt}"
            );
            manager.flush().await.unwrap();
            assert_eq!(Number::find_with(1, &db).await.unwrap().unwrap().value, 99);
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn concurrent_managed_lookups_share_one_identity() {
    use std::sync::Arc;
    use tideorm::entity_manager::EntityManager;
    let db=global_memory_database("CREATE TABLE audit_numbers(id INTEGER PRIMARY KEY,value INTEGER); INSERT INTO audit_numbers VALUES(1,10)").await;
    for _ in 0..12 {
        let manager = EntityManager::new(Arc::new(db.clone()));
        manager.find::<Number>(1).await.unwrap();
        let barrier = Arc::new(tokio::sync::Barrier::new(16));
        let tasks = (0..16)
            .map(|_| {
                let manager = manager.clone();
                let barrier = barrier.clone();
                tokio::spawn(async move {
                    barrier.wait().await;
                    manager.find_managed::<Number>(1).await.unwrap().unwrap()
                })
            })
            .collect::<Vec<_>>();
        let mut handles = Vec::new();
        for task in tasks {
            handles.push(task.await.unwrap());
        }
        handles[0].edit(|row| row.value = 77);
        assert!(handles.iter().all(|h| h.get().value == 77));
    }
}

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn rolled_back_reads_leave_no_entity_manager_phantoms() {
    let db = users_and_posts("committed", 10).await;
    let manager = tideorm::entity_manager::EntityManager::new(std::sync::Arc::new(db.clone()));
    manager.find::<User>(1).await.unwrap().unwrap();
    let inside = manager.clone();
    let nested = db.clone();
    let result = db.transaction(move |_| Box::pin(async move {
        nested.transaction(move |_| Box::pin(async move {
            Database::execute("INSERT INTO review_users VALUES(99, 'uncommitted', 1), (100, 'managed', 1)").await?;
            assert!(inside.find::<User>(99).await?.is_some());
            assert!(inside.find_managed::<User>(100).await?.is_some());
            Ok(())
        })).await?;
        Err::<(), _>(tideorm::Error::query("rollback parent"))
    })).await;
    assert!(result.is_err());
    assert!(manager.find::<User>(99).await.unwrap().is_none());
    assert!(manager.find_managed::<User>(100).await.unwrap().is_none());
    assert_eq!(
        manager.find::<User>(1).await.unwrap().unwrap().name,
        "committed"
    );
    // A committed read stays attached and usable.
    let inside = manager.clone();
    db.transaction(move |_| {
        Box::pin(async move {
            Database::execute("INSERT INTO review_users VALUES(99, 'committed insert', 1)").await?;
            inside.find::<User>(99).await?;
            Ok(())
        })
    })
    .await
    .unwrap();
    assert_eq!(
        manager.find::<User>(99).await.unwrap().unwrap().name,
        "committed insert"
    );
    manager.flush().await.unwrap();
}
