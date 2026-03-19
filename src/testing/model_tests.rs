use crate::model::Model;

#[derive(tideorm::Model)]
#[tideorm(table = "model_test_users")]
struct AutoIncrementModel {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[derive(tideorm::Model)]
#[tideorm(table = "model_test_tokens")]
struct NaturalKeyModel {
    #[tideorm(primary_key)]
    id: String,
}

#[derive(tideorm::Model)]
#[tideorm(table = "model_test_serialization")]
struct SerializationModel {
    #[tideorm(primary_key)]
    id: i64,
    params: String,
    enabled: bool,
}

#[derive(tideorm::Model)]
#[tideorm(table = "model_test_presenter_serialization")]
struct PresenterSerializationModel {
    #[tideorm(primary_key)]
    id: i64,
    params: serde_json::Value,
    title: String,
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

#[test]
fn test_to_hash_map_preserves_params_field() {
    let model = SerializationModel {
        id: 7,
        params: "keep me".to_string(),
        enabled: true,
    };

    let map = model.to_hash_map();

    assert_eq!(map.get("id").map(String::as_str), Some("7"));
    assert_eq!(map.get("params").map(String::as_str), Some("keep me"));
    assert_eq!(map.get("enabled").map(String::as_str), Some("true"));
}

#[test]
fn test_to_hash_map_preserves_structured_params_field() {
    let model = PresenterSerializationModel {
        id: 8,
        params: serde_json::json!({
            "view": "minimal",
            "locale": "en"
        }),
        title: "Presenter Output".to_string(),
    };

    let map = model.to_hash_map();

    assert_eq!(map.get("id").map(String::as_str), Some("8"));
    assert_eq!(
        map.get("title").map(String::as_str),
        Some("Presenter Output")
    );
    assert_eq!(map.get("params"), None);
    assert_eq!(map.get("_params"), None);
}
