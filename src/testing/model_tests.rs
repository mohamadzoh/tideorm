use crate::model::Model;

#[derive(tideorm::Model)]
#[tide(table = "model_test_users")]
struct AutoIncrementModel {
    #[tide(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[derive(tideorm::Model)]
#[tide(table = "model_test_tokens")]
struct NaturalKeyModel {
    #[tide(primary_key)]
    id: String,
}

#[test]
fn test_is_new_treats_zero_auto_increment_id_as_unsaved() {
    let model = AutoIncrementModel {
        id: 0,
        name: "Alice".to_string(),
    };

    assert!(model.is_new());
}

#[test]
fn test_is_new_treats_non_zero_auto_increment_id_as_persisted() {
    let model = AutoIncrementModel {
        id: 42,
        name: "Alice".to_string(),
    };

    assert!(!model.is_new());
}

#[test]
fn test_is_new_does_not_treat_non_numeric_natural_key_as_unsaved() {
    let model = NaturalKeyModel {
        id: "user_123".to_string(),
    };

    assert!(!model.is_new());
}