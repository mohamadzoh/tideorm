use super::*;

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
    forget_model(&model).expect("forgetting an absent baseline should succeed");

    // No baseline: nothing was compared, so there is no field list at all.
    assert_eq!(
        changed_fields(&model).expect("dirty check should succeed"),
        None
    );
    assert_eq!(
        original_value(&model, "name").expect("original value lookup should succeed"),
        None
    );

    remember_model(&model).expect("remembering a baseline should succeed");

    // Baseline present and matching: an empty list, not a missing one.
    assert_eq!(
        changed_fields(&model).expect("dirty check should succeed"),
        Some(Vec::new())
    );
    assert_eq!(
        original_value(&model, "name").expect("original value lookup should succeed"),
        Some(Some(serde_json::json!("Alice")))
    );

    let mut edited = model.clone();
    edited.name = "Bob".to_string();
    assert_eq!(
        changed_fields(&edited).expect("dirty check should succeed"),
        Some(vec!["name"])
    );

    forget_model(&model).expect("forgetting a baseline should succeed");
    assert_eq!(
        changed_fields(&model).expect("dirty check should succeed"),
        None
    );
}

#[test]
fn an_unsaved_model_reports_no_baseline() {
    let mut model = baseline_user();
    model.id = 0;

    assert_eq!(
        changed_fields(&model).expect("dirty check should succeed"),
        None
    );
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
        loading_from(origin, || remember_model(model)).expect("remembering should succeed");
    };

    // Read through a database other than the scope's (none here).
    remember_from(Some(2), &bob);
    assert_eq!(
        changed_fields(&bob).expect("dirty check should succeed"),
        Some(Vec::new())
    );

    // A second database gave the key the same row.
    remember_from(Some(1), &bob);
    assert_eq!(
        original_value(&bob, "name").expect("original value lookup should succeed"),
        Some(Some(serde_json::json!("Bob")))
    );

    // It gave the key another row: either could be the model's.
    remember_from(Some(1), &carol);
    for model in [&bob, &carol] {
        assert_eq!(
            changed_fields(model).expect("dirty check should succeed"),
            None
        );
        assert_eq!(
            original_value(model, "name").expect("original value lookup should succeed"),
            None
        );
    }

    // Forgetting one of them leaves the key as unknown as before: the other
    // database's baseline is not the model's either.
    loading_from(Some(1), || forget_model(&carol)).expect("forgetting should succeed");
    for model in [&bob, &carol] {
        assert_eq!(
            changed_fields(model).expect("dirty check should succeed"),
            None
        );
    }

    // Reading the row again gives the key a baseline both databases agree on.
    remember_from(Some(1), &bob);
    assert_eq!(
        changed_fields(&bob).expect("dirty check should succeed"),
        Some(Vec::new())
    );

    invalidate_model::<TwoDatabaseUser>();
    assert_eq!(
        changed_fields(&bob).expect("dirty check should succeed"),
        None
    );
}

fn named(name: &str) -> SnapshotValues {
    HashMap::from([("name".to_string(), serde_json::json!(name))])
}

fn store_row(key: &str) -> RowKey {
    (TypeId::of::<BaselineUser>(), key.to_string())
}

#[test]
fn a_rolled_back_read_leaves_a_baseline_written_since_alone() {
    let mut store = SnapshotStore::new(10);
    let row = store_row("1");
    store.replace(row.clone(), None, Some(named("Alice")));

    // A transaction reads the row, and before it rolls back another task
    // commits a change to it.
    let (previous, standing) = store.replace(row.clone(), None, Some(named("Alice")));
    store.replace(row.clone(), None, Some(named("Bob")));
    store.restore(row.clone(), None, standing, previous);

    assert_eq!(store.get(&row), Some(&named("Bob")));
}

#[test]
fn a_rolled_back_transaction_restores_the_baseline_before_its_first_write() {
    let mut store = SnapshotStore::new(10);
    let row = store_row("1");
    store.replace(row.clone(), None, Some(named("Alice")));

    let (first_previous, first) = store.replace(row.clone(), None, Some(named("Bob")));
    let (second_previous, second) = store.replace(row.clone(), None, None);
    // Undo steps run last first.
    store.restore(row.clone(), None, second, second_previous);
    store.restore(row.clone(), None, first, first_previous);

    assert_eq!(store.get(&row), Some(&named("Alice")));
}

#[test]
fn eviction_drops_every_databases_baseline_of_a_key_together() {
    let mut store = SnapshotStore::new(2);
    let row = store_row("1");
    store.replace(row.clone(), Some(1), Some(named("Bob")));
    store.replace(row.clone(), Some(2), Some(named("Carol")));
    // Past capacity, the oldest baseline goes, and with it the key's other.
    store.replace(store_row("2"), Some(1), Some(named("Dave")));

    assert!(!store.entries.contains_key(&row));
    assert_eq!(store.order.len(), 1);
    assert_eq!(store.get(&store_row("2")), Some(&named("Dave")));
}
