use super::*;

/// Remember `model` as read from the scope's pool.
fn remember_model<M: Model>(model: &M) {
    remember_model_from(model, crate::database::scope_origin())
}

#[tideorm::model(table = "dirty_tracking_baseline_users")]
struct BaselineUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

fn baseline_user() -> BaselineUser {
    BaselineUser {
        id: 7,
        name: "Alice".to_string(),
    }
}

#[test]
fn a_missing_baseline_is_distinguishable_from_an_unchanged_model() {
    let model = baseline_user();
    forget_model(&model);

    // No baseline: nothing was compared, so there is no field list at all.
    assert_eq!(changed_fields(&model), None);
    assert_eq!(
        original_value(&model, "name").expect("original value lookup should succeed"),
        None
    );

    remember_model(&model);

    // Baseline present and matching: an empty list, not a missing one.
    assert_eq!(changed_fields(&model), Some(Vec::new()));
    assert_eq!(
        original_value(&model, "name").expect("original value lookup should succeed"),
        Some(Some(Value::from("Alice")))
    );

    let mut edited = model.clone();
    edited.name = "Bob".to_string();
    assert_eq!(changed_fields(&edited), Some(vec!["name"]));

    forget_model(&model);
    assert_eq!(changed_fields(&model), None);
}

#[test]
fn an_unsaved_model_reports_no_baseline() {
    let mut model = baseline_user();
    model.id = 0;

    assert_eq!(changed_fields(&model), None);
    assert_eq!(
        original_value(&model, "name").expect("original value lookup should succeed"),
        None
    );
}

#[tideorm::model(table = "dirty_tracking_two_database_users")]
struct TwoDatabaseUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

/// A model does not record the database it was read from, so its baseline is
/// the one every database that gave its key agrees on, and there is none
/// where they differ. The scope's own was used, which for a model read
/// through `query_with(db)` is another database's row.
#[test]
fn a_key_read_from_two_databases_has_a_baseline_only_where_they_agree() {
    let bob = TwoDatabaseUser {
        id: 3,
        name: "Bob".to_string(),
    };
    let carol = TwoDatabaseUser {
        id: 3,
        name: "Carol".to_string(),
    };
    let remember_from = |origin, model: &TwoDatabaseUser| {
        remember_model_from(model, origin);
    };

    // Read through a database other than the scope's (none here).
    remember_from(Some(2), &bob);
    assert_eq!(changed_fields(&bob), Some(Vec::new()));

    // A second database gave the key the same row.
    remember_from(Some(1), &bob);
    assert_eq!(
        original_value(&bob, "name").expect("original value lookup should succeed"),
        Some(Some(Value::from("Bob")))
    );

    // It gave the key another row: either could be the model's.
    remember_from(Some(1), &carol);
    for model in [&bob, &carol] {
        assert_eq!(changed_fields(model), None);
        assert_eq!(
            original_value(model, "name").expect("original value lookup should succeed"),
            None
        );
    }

    // Forgetting one of them leaves the key as unknown as before: the other
    // database's baseline is not the model's either.
    forget_model_from(&carol, Some(1));
    for model in [&bob, &carol] {
        assert_eq!(changed_fields(model), None);
    }

    // Reading the row again gives the key a baseline both databases agree on.
    remember_from(Some(1), &bob);
    assert_eq!(changed_fields(&bob), Some(Vec::new()));

    invalidate_model::<TwoDatabaseUser>();
    assert_eq!(changed_fields(&bob), None);
}

fn named(name: &str) -> SnapshotValues {
    std::sync::Arc::new(HashMap::from([("name", Value::from(name))]))
}

fn store_row(key: i64) -> RowKey {
    (
        TypeId::of::<BaselineUser>(),
        vec![ValueKey::new(key.into())],
    )
}

/// The scope's pool's baseline every task shares.
const SHARED: Slot = (None, None);

/// Two tasks, both alive while their ids are taken, so the ids differ.
#[cfg(feature = "runtime-tokio")]
fn two_tasks() -> (Task, Task) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        assert_eq!(current_task(), None);
        let (go, ready) = tokio::sync::watch::channel(false);
        let spawn = |mut ready: tokio::sync::watch::Receiver<bool>| {
            tokio::spawn(async move {
                let task = current_task().unwrap();
                ready.wait_for(|go| *go).await.unwrap();
                task
            })
        };
        let (first, second) = (spawn(ready.clone()), spawn(ready));
        go.send(true).unwrap();
        (first.await.unwrap(), second.await.unwrap())
    })
}

/// Each task reads the baseline it read or wrote itself, whatever another
/// task wrote since; code outside a task, or in one that never read the row,
/// reads the shared one, the latest.
#[cfg(feature = "runtime-tokio")]
#[test]
fn each_task_reads_its_own_baseline() {
    let (first, second) = two_tasks();
    assert_ne!(first, second);
    let mut store = SnapshotStore::new(10);
    let row = store_row(1);
    for slot in [(None, Some(first)), SHARED] {
        store.replace(row.clone(), slot, Some(named("Alice")));
    }
    for slot in [(None, Some(second)), SHARED] {
        store.replace(row.clone(), slot, Some(named("Bob")));
    }

    assert_eq!(store.get(&row, Some(first)), Some(&named("Alice")));
    assert_eq!(store.get(&row, Some(second)), Some(&named("Bob")));
    assert_eq!(store.get(&row, None), Some(&named("Bob")));
}

/// An evicted task's baseline takes the shared one with it, so that task
/// does not go on to compare with another task's write.
#[cfg(feature = "runtime-tokio")]
#[test]
fn eviction_of_a_tasks_baseline_takes_the_shared_one() {
    let (first, second) = two_tasks();
    let mut store = SnapshotStore::new(3);
    let row = store_row(1);
    store.replace(row.clone(), (None, Some(first)), Some(named("Alice")));
    store.replace(row.clone(), (None, Some(second)), Some(named("Bob")));
    store.replace(row.clone(), SHARED, Some(named("Bob")));
    store.replace(store_row(2), SHARED, Some(named("Dave")));

    assert_eq!(store.get(&row, Some(first)), None);
    assert_eq!(store.get(&row, None), None);
    assert_eq!(store.get(&row, Some(second)), Some(&named("Bob")));
    assert_eq!(store.order.len(), 2);
}

#[test]
fn a_rolled_back_read_leaves_a_baseline_written_since_alone() {
    let mut store = SnapshotStore::new(10);
    let row = store_row(1);
    store.replace(row.clone(), SHARED, Some(named("Alice")));

    // A transaction reads the row, and before it rolls back another task
    // commits a change to it.
    let (previous, standing) = store.replace(row.clone(), SHARED, Some(named("Alice")));
    store.replace(row.clone(), SHARED, Some(named("Bob")));
    store.restore(row.clone(), SHARED, standing, previous);

    assert_eq!(store.get(&row, None), Some(&named("Bob")));
}

#[test]
fn a_rolled_back_transaction_restores_the_baseline_before_its_first_write() {
    let mut store = SnapshotStore::new(10);
    let row = store_row(1);
    store.replace(row.clone(), SHARED, Some(named("Alice")));

    let (first_previous, first) = store.replace(row.clone(), SHARED, Some(named("Bob")));
    let (second_previous, second) = store.replace(row.clone(), SHARED, None);
    // Undo steps run last first.
    store.restore(row.clone(), SHARED, second, second_previous);
    store.restore(row.clone(), SHARED, first, first_previous);

    assert_eq!(store.get(&row, None), Some(&named("Alice")));
}

#[test]
fn eviction_drops_every_databases_baseline_of_a_key_together() {
    let mut store = SnapshotStore::new(2);
    let row = store_row(1);
    store.replace(row.clone(), (Some(1), None), Some(named("Bob")));
    store.replace(row.clone(), (Some(2), None), Some(named("Carol")));
    // Past capacity, the oldest baseline goes, and with it the key's other.
    store.replace(store_row(2), (Some(1), None), Some(named("Dave")));

    assert!(!store.entries.contains_key(&row));
    assert_eq!(store.order.len(), 1);
    assert_eq!(store.get(&store_row(2), None), Some(&named("Dave")));
}

#[test]
fn bulk_invalidation_rollback_does_not_overwrite_a_later_read() {
    let mut model = baseline_user();
    model.id = 71;
    remember_model(&model);
    let pending = std::sync::Arc::new(parking_lot::Mutex::new(
        crate::cache::PendingInvalidations::new(None),
    ));
    let scope = crate::cache::install_pending_invalidations(&pending);
    invalidate_model::<BaselineUser>();
    assert_eq!(original_value(&model, "name").unwrap(), None);
    drop(scope);
    // Another task reads a newer baseline before the bulk write rolls back.
    model.name = "newer".into();
    remember_model(&model);
    drop(pending);
    assert_eq!(
        original_value(&model, "name").unwrap(),
        Some(Some(Value::from("newer")))
    );
}

#[test]
fn a_committed_bulk_invalidation_drops_all_origin_tombstones() {
    let mut model = baseline_user();
    model.id = 72;
    for origin in [Some(701), Some(702)] {
        remember_model_from(&model, origin);
    }
    let pending = std::sync::Arc::new(parking_lot::Mutex::new(
        crate::cache::PendingInvalidations::new(None),
    ));
    let scope = crate::cache::install_pending_invalidations(&pending);
    invalidate_model::<BaselineUser>();
    drop(scope);
    std::sync::Arc::try_unwrap(pending)
        .ok()
        .unwrap()
        .into_inner()
        .replay();
    remember_model_from(&model, Some(703));
    assert_eq!(
        original_value(&model, "name").unwrap(),
        Some(Some(Value::from("Alice")))
    );
}

#[tideorm::model(table = "dirty_tracking_measurements")]
struct Measurement {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    value: f64,
    ratio: Option<f32>,
}

/// Fields compare as values: a NaN equals itself, and a change to or from a
/// non-finite float shows, as does one between two of them. A NULL field's
/// original value is `None`.
#[test]
fn a_change_to_or_from_a_non_finite_float_shows() {
    let model = Measurement {
        id: 3,
        value: f64::NAN,
        ratio: None,
    };
    remember_model(&model);
    assert_eq!(changed_fields(&model), Some(Vec::new()));
    assert_eq!(
        original_value(&model, "value").unwrap(),
        Some(Some(Value::from(f64::NAN)))
    );
    assert_eq!(original_value(&model, "ratio").unwrap(), Some(None));

    let mut edited = model.clone();
    edited.value = f64::INFINITY;
    edited.ratio = Some(f32::NAN);
    assert_eq!(changed_fields(&edited), Some(vec!["value", "ratio"]));

    edited.value = 1.5;
    edited.ratio = Some(f32::NEG_INFINITY);
    remember_model(&edited);
    let mut back = edited.clone();
    back.ratio = None;
    assert_eq!(changed_fields(&back), Some(vec!["ratio"]));
    forget_model(&edited);
}

/// Baselines recorded by real reads and writes.
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
mod stored {
    use crate::model::Model;

    #[tideorm::model(table = "dirty_tracking_stored_users")]
    struct StoredUser {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        name: String,
    }

    /// Install a database with the query cache on, holding Alice.
    async fn saved_alice() -> (crate::Database, StoredUser) {
        let db = crate::test_support::install_sqlite_global(&[
            "CREATE TABLE dirty_tracking_stored_users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL)",
        ])
        .await;
        crate::test_support::set_query_cache(true);
        let alice = StoredUser {
            id: 0,
            name: "Alice".to_string(),
        }
        .save()
        .await
        .expect("seed save should succeed");
        (db, alice)
    }

    async fn find(id: i64) -> StoredUser {
        StoredUser::find(id)
            .await
            .expect("find should succeed")
            .expect("saved model should exist")
    }

    fn changed(model: &StoredUser) -> Option<Vec<&'static str>> {
        model.changed_fields().expect("dirty check should succeed")
    }

    fn original_name(model: &StoredUser) -> Option<Option<crate::DbValue>> {
        model
            .original_value("name")
            .expect("original value lookup should succeed")
    }

    /// A loaded model compares against the row it was read as, and after an
    /// update against what the update wrote, as does any stale copy of it.
    #[tokio::test]
    async fn loaded_models_track_changes_against_the_latest_stored_baseline() {
        let _guard = crate::test_support::global_db_lock().lock().await;
        let (_db, alice) = saved_alice().await;
        let was_alice = Some(Some(crate::DbValue::from("Alice")));
        let was_bob = Some(Some(crate::DbValue::from("Bob")));

        let mut first = find(alice.id).await;
        let mut second = find(alice.id).await;
        assert_eq!(changed(&first), Some(Vec::new()));
        assert_eq!(original_name(&first), was_alice);

        first.name = "Bob".to_string();
        assert_eq!(changed(&first), Some(vec!["name"]));
        assert_eq!(original_name(&first), was_alice);

        let mut first = first.update().await.expect("update should succeed");
        assert_eq!(changed(&first), Some(Vec::new()));
        assert_eq!(original_name(&first), was_bob);

        first.name = "Carol".to_string();
        assert_eq!(changed(&first), Some(vec!["name"]));
        assert_eq!(original_name(&first), was_bob);

        second.name = "Dana".to_string();
        assert_eq!(
            changed(&second),
            Some(vec!["name"]),
            "a stale copy compares against the latest baseline"
        );
        assert_eq!(original_name(&second), was_bob);

        crate::test_support::reset_globals_and_cache();
    }

    #[tokio::test]
    async fn cache_hits_restore_dirty_tracking_snapshots() {
        let _guard = crate::test_support::global_db_lock().lock().await;
        let _db = saved_alice().await;

        let first = crate::test_support::cached_rows::<StoredUser>().await;
        assert_eq!(first.len(), 1);

        crate::model::clear_dirty_snapshots();

        let mut cached = crate::test_support::cached_rows::<StoredUser>().await;
        cached[0].name = "Cached Bob".to_string();

        assert_eq!(changed(&cached[0]), Some(vec!["name"]));
        assert_eq!(
            original_name(&cached[0]),
            Some(Some(crate::DbValue::from("Alice")))
        );

        crate::test_support::reset_globals_and_cache();
    }

    /// A baseline recorded inside a transaction that rolls back is withdrawn:
    /// the row is what it was, so the baseline from before comes back. The
    /// update's "Bob" stayed, and an unchanged model read before it reported a
    /// change.
    #[tokio::test]
    async fn a_rolled_back_update_restores_the_baseline() {
        let _guard = crate::test_support::global_db_lock().lock().await;
        let (db, alice) = saved_alice().await;
        let loaded = find(alice.id).await;

        let edited = StoredUser {
            name: "Bob".to_string(),
            ..loaded.clone()
        };
        let result: crate::error::Result<()> = db
            .transaction(async move |_| {
                edited.update().await?;
                Err(crate::error::Error::query("roll it back"))
            })
            .await;
        assert!(result.is_err(), "the transaction should roll back");

        assert_eq!(changed(&loaded), Some(Vec::new()));
        assert_eq!(
            original_name(&loaded),
            Some(Some(crate::DbValue::from("Alice")))
        );

        crate::test_support::reset_globals_and_cache();
    }
}
