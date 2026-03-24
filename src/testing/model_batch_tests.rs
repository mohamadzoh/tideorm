use super::BatchUpdateBuilder;
use crate::model::Model as ModelTrait;

#[tideorm::model(table = "batch_update_guard_users")]
struct BatchUpdateGuardUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[test]
fn batch_update_guard_rejects_unfiltered_updates() {
    let err = BatchUpdateBuilder::<BatchUpdateGuardUser>::new()
        .set("name", "updated")
        .ensure_explicit_filters("update")
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("requires at least one explicit filter")
    );
}

#[test]
fn batch_update_guard_accepts_where_filters() {
    assert!(
        BatchUpdateGuardUser::update_all()
            .set("name", "updated")
            .where_eq("id", 1)
            .ensure_explicit_filters("update")
            .is_ok()
    );
}

#[test]
fn batch_update_guard_accepts_or_filters() {
    assert!(
        BatchUpdateGuardUser::update_all()
            .set("name", "updated")
            .or_where_eq("name", "alice")
            .ensure_explicit_filters("update")
            .is_ok()
    );
}

#[test]
fn batch_update_guard_rejects_limit_without_where() {
    let err = BatchUpdateGuardUser::update_all()
        .set("name", "updated")
        .limit(1)
        .ensure_explicit_filters("update")
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("requires at least one explicit filter")
    );
}

#[test]
fn batch_update_set_if_applies_update_when_condition_is_true() {
    let builder = BatchUpdateBuilder::<BatchUpdateGuardUser>::new().set_if("name", "updated", true);

    assert!(matches!(
        builder.updates.get("name"),
        Some(super::UpdateValue::Value(value)) if *value == serde_json::json!("updated")
    ));
}

#[test]
fn batch_update_set_if_skips_update_when_condition_is_false() {
    let builder = BatchUpdateBuilder::<BatchUpdateGuardUser>::new().set_if("name", "updated", false);

    assert!(!builder.updates.contains_key("name"));
}
