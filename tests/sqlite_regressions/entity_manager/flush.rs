//! A save in flight racing a removal, a detach or further edits.

use crate::fixtures::memory_database;
use std::sync::{Arc, Mutex};
use tideorm::entity_manager::{EntityManager, EntityState, Managed};
use tideorm::prelude::*;

#[tideorm::model(table = "flush_behavior_rows")]
struct Row {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

type Pause = (
    tokio::sync::oneshot::Sender<()>,
    std::sync::mpsc::Receiver<()>,
);

static PAUSE: Mutex<Option<Pause>> = Mutex::new(None);

static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

impl tideorm::callbacks::Callbacks for Row {
    fn before_save(&mut self) -> tideorm::Result<()> {
        let pause = PAUSE.lock().unwrap().take();
        if let Some((entered, resume)) = pause {
            entered.send(()).unwrap();
            resume
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap();
        }
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

async fn pause_flush(
    manager: &Arc<EntityManager>,
) -> (
    tokio::task::JoinHandle<tideorm::Result<()>>,
    std::sync::mpsc::Sender<()>,
) {
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel();
    *PAUSE.lock().unwrap() = Some((entered_tx, resume_rx));
    let manager = manager.clone();
    let task = tokio::spawn(async move { manager.flush().await });
    tokio::time::timeout(std::time::Duration::from_secs(10), entered_rx)
        .await
        .unwrap()
        .unwrap();
    (task, resume_tx)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn removal_during_insert_or_update_remains_scheduled() {
    let _serial = TEST_LOCK.lock().await;
    for insert in [false, true] {
        let db = database().await;
        let manager = EntityManager::new(Arc::new(db.clone()));
        let managed = pending_row(&manager, insert).await;
        let (task, resume) = pause_flush(&manager).await;
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
            let (task, resume) = pause_flush(&manager).await;
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
    let (task, resume) = pause_flush(&manager).await;
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
