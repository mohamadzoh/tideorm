//! A save in flight racing a removal, a detach or further edits.

use super::Pause;
use crate::fixtures::memory_database;
use std::sync::Arc;
use tideorm::entity_manager::{EntityManager, EntityState, Managed};
use tideorm::prelude::*;

#[tideorm::model(table = "flush_behavior_rows")]
struct Row {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

static PAUSE: Pause = Pause::new();

static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

impl tideorm::callbacks::Callbacks for Row {
    fn before_save(&mut self) -> tideorm::Result<()> {
        PAUSE.wait();
        Ok(())
    }
}

async fn database() -> Database {
    memory_database("CREATE TABLE flush_behavior_rows(id INTEGER PRIMARY KEY,name TEXT); INSERT INTO flush_behavior_rows VALUES(1,'original')").await
}

async fn pending_row(manager: &Arc<EntityManager>, insert: bool) -> Managed<Row> {
    if insert {
        manager.persist(Row {
            id: 0,
            name: "changed".into(),
        })
    } else {
        let row = manager.find_managed::<Row>(1).await.unwrap().unwrap();
        row.edit(|row| row.name = "changed".into());
        row
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn removal_during_insert_or_update_remains_scheduled() {
    let _serial = TEST_LOCK.lock().await;
    for insert in [false, true] {
        let db = database().await;
        let manager = EntityManager::new(Arc::new(db.clone()));
        let managed = pending_row(&manager, insert).await;
        let (task, resume) = PAUSE.flush(&manager).await;
        manager.remove(&managed);
        assert_eq!(managed.state(), EntityState::Removed);
        resume.send(()).unwrap();
        task.await.unwrap().unwrap();
        assert_eq!(managed.state(), EntityState::Removed);
        let id = managed.get().id;
        assert_ne!(id, 0);
        manager.flush().await.unwrap();
        assert!(Row::find_with(id, &db).await.unwrap().is_none());
        assert!(manager.find_managed::<Row>(id).await.unwrap().is_none());
        assert_eq!(managed.state(), EntityState::Detached);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn detach_or_clear_during_save_does_not_restore_the_handle() {
    let _serial = TEST_LOCK.lock().await;
    for insert in [false, true] {
        for clear in [false, true] {
            let db = database().await;
            let manager = EntityManager::new(Arc::new(db.clone()));
            let managed = pending_row(&manager, insert).await;
            let (task, resume) = PAUSE.flush(&manager).await;
            if clear {
                manager.clear();
            } else {
                manager.detach(&managed);
            }
            assert_eq!(managed.state(), EntityState::Detached);
            resume.send(()).unwrap();
            task.await.unwrap().unwrap();
            assert_eq!(managed.state(), EntityState::Detached);
            let id = managed.get().id;
            let reattached = manager.find_managed::<Row>(id).await.unwrap().unwrap();
            reattached.edit(|row| row.name = "after reattach".into());
            assert_eq!(managed.get().name, "changed");
            manager.flush().await.unwrap();
            assert_eq!(
                Row::find_with(id, &db).await.unwrap().unwrap().name,
                "after reattach"
            );
            assert_eq!(managed.state(), EntityState::Detached);
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn edits_during_save_remain_dirty_for_the_next_flush() {
    let _serial = TEST_LOCK.lock().await;
    let db = database().await;
    let manager = EntityManager::new(Arc::new(db.clone()));
    let managed = pending_row(&manager, false).await;
    let (task, resume) = PAUSE.flush(&manager).await;
    managed.edit(|row| row.name = "later edit".into());
    resume.send(()).unwrap();
    task.await.unwrap().unwrap();
    assert_eq!(managed.state(), EntityState::Managed);
    assert_eq!(managed.get().name, "later edit");
    assert_eq!(
        Row::find_with(1, &db).await.unwrap().unwrap().name,
        "changed"
    );
    manager.flush().await.unwrap();
    assert_eq!(
        Row::find_with(1, &db).await.unwrap().unwrap().name,
        "later edit"
    );
}

/// A flush that fails undoes what it wrote and nothing else: an entity
/// persisted while it ran still flushes next time, and an edit or a removal
/// of the entity it was writing stays.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_flush_keeps_work_done_while_it_ran() {
    let _serial = TEST_LOCK.lock().await;
    for remove in [false, true] {
        let db = memory_database(
            "CREATE TABLE flush_behavior_rows(id INTEGER PRIMARY KEY, name TEXT CHECK (name <> 'bad')); \
             INSERT INTO flush_behavior_rows VALUES(1, 'original')",
        )
        .await;
        let manager = EntityManager::new(Arc::new(db.clone()));
        let failing = manager.find_managed::<Row>(1).await.unwrap().unwrap();
        failing.edit(|row| row.name = "bad".into());
        let inserted = pending_row(&manager, true).await;
        let (task, resume) = PAUSE.flush(&manager).await;
        let persisted = manager.persist(Row {
            id: 0,
            name: "persisted meanwhile".into(),
        });
        if remove {
            manager.remove(&inserted);
        } else {
            inserted.edit(|row| row.name = "edited meanwhile".into());
        }
        resume.send(()).unwrap();
        assert!(task.await.unwrap().is_err());

        assert_eq!(inserted.get().id, 0);
        let expected = if remove {
            EntityState::Removed
        } else {
            assert_eq!(inserted.get().name, "edited meanwhile");
            EntityState::New
        };
        assert_eq!(inserted.state(), expected);
        assert_eq!(persisted.state(), EntityState::New);

        failing.edit(|row| row.name = "fixed".into());
        manager.flush().await.unwrap();
        let mut names: Vec<String> = Row::query_with(&db)
            .get()
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.name)
            .collect();
        names.sort();
        let mut expected = vec!["fixed", "persisted meanwhile"];
        if !remove {
            expected.insert(0, "edited meanwhile");
        }
        assert_eq!(names, expected);
    }
}

#[tokio::test]
async fn sequential_remove_and_detach_still_work() {
    let _serial = TEST_LOCK.lock().await;
    let db = database().await;
    let manager = EntityManager::new(Arc::new(db.clone()));
    let row = pending_row(&manager, false).await;
    manager.flush().await.unwrap();
    manager.detach(&row);
    manager.flush().await.unwrap();
    assert_eq!(row.state(), EntityState::Detached);
    let row = manager.find_managed::<Row>(1).await.unwrap().unwrap();
    manager.remove(&row);
    manager.flush().await.unwrap();
    assert!(Row::find_with(1, &db).await.unwrap().is_none());
}
