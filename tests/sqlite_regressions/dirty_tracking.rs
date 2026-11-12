//! Dirty-tracking baselines are kept per async task: two tasks editing one
//! row side by side each compare with the copy they read or wrote.

use crate::fixtures::{close_global, global_memory_database};
use tideorm::prelude::*;
use tokio::sync::oneshot;

#[tideorm::model(table = "tracked_users")]
struct TrackedUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
    active: bool,
}

const USERS: &str = "CREATE TABLE tracked_users(id INTEGER PRIMARY KEY AUTOINCREMENT, \
     name TEXT NOT NULL, active BOOLEAN NOT NULL); \
     INSERT INTO tracked_users VALUES(1, 'Alice', 1)";

async fn alice() -> TrackedUser {
    TrackedUser::find(1).await.unwrap().unwrap()
}

fn original_name(model: &TrackedUser) -> Option<Option<DbValue>> {
    model.original_value("name").unwrap()
}

fn was(name: &str) -> Option<Option<DbValue>> {
    Some(Some(DbValue::from(name)))
}

/// Two requests edit one row at once: the second one's `update()` and
/// `update_all()` leave the first comparing with the row it read. The
/// baselines were one process-wide store keyed by the row, so the second
/// task's write became the first task's baseline too.
#[tokio::test]
async fn concurrent_tasks_keep_their_own_baselines() {
    let db = global_memory_database(USERS).await;
    let (loaded, first_loaded) = oneshot::channel();
    let (written, second_written) = oneshot::channel();

    let first = tokio::spawn(async move {
        let mut user = alice().await;
        loaded.send(()).unwrap();
        second_written.await.unwrap();
        user.active = false;
        (user.changed_fields().unwrap(), original_name(&user))
    });
    let second = tokio::spawn(async move {
        first_loaded.await.unwrap();
        let mut user = alice().await;
        user.name = "Bob".into();
        let user = user.update().await.unwrap();
        assert_eq!(user.changed_fields().unwrap(), Some(Vec::new()));
        TrackedUser::query()
            .where_eq(TrackedUser::columns.id, 1)
            .update_all()
            .set(TrackedUser::columns.name, "Carol")
            .execute()
            .await
            .unwrap();
        written.send(()).unwrap();
        original_name(&user)
    });

    let (changed, first_original) = first.await.unwrap();
    assert_eq!(changed, Some(vec!["active"]));
    assert_eq!(first_original, was("Alice"));
    // `update_all()` forgot the writing task's own baselines.
    assert_eq!(second.await.unwrap(), None);
    close_global(&db).await;
}

/// Code that did not read the row itself, here the test body outside any
/// task, compares with the latest baseline any task left.
#[tokio::test]
async fn a_model_read_elsewhere_compares_with_the_latest_baseline() {
    let db = global_memory_database(USERS).await;
    let user = tokio::spawn(alice()).await.unwrap();
    assert_eq!(original_name(&user), was("Alice"));

    tokio::spawn(async {
        let mut bob = alice().await;
        bob.name = "Bob".into();
        bob.update().await.unwrap();
    })
    .await
    .unwrap();
    assert_eq!(original_name(&user), was("Bob"));
    close_global(&db).await;
}

/// A rolled-back write in a task gives back the task's own baseline and the
/// shared one.
#[tokio::test]
async fn a_rollback_in_a_task_restores_its_baselines() {
    let db = global_memory_database(USERS).await;
    let tx_db = db.clone();
    let user = tokio::spawn(async move {
        let user = alice().await;
        let outcome = tx_db
            .transaction(async |_| {
                let mut bob = alice().await;
                bob.name = "Bob".into();
                assert_eq!(original_name(&bob.update().await?), was("Bob"));
                Err::<(), _>(tideorm::Error::query("roll back"))
            })
            .await;
        assert!(outcome.is_err());
        assert_eq!(original_name(&user), was("Alice"));
        user
    })
    .await
    .unwrap();
    // Outside the task, the shared baseline is back to the row as well.
    assert_eq!(original_name(&user), was("Alice"));
    close_global(&db).await;
}
