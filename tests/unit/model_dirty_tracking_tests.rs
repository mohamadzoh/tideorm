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

    loading_from(Some(1), || forget_model(&carol)).expect("forgetting should succeed");
    assert_eq!(
        changed_fields(&carol).expect("dirty check should succeed"),
        Some(vec!["name"])
    );

    invalidate_model::<TwoDatabaseUser>();
    assert_eq!(
        changed_fields(&bob).expect("dirty check should succeed"),
        None
    );
}
