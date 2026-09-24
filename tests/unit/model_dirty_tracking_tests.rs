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
