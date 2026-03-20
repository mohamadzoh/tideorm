use crate::model::Model as ModelTrait;

#[tideorm::model(table = "model_test_users")]
struct AutoIncrementModel {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[tideorm::model(table = "model_test_tokens")]
struct NaturalKeyModel {
    #[tideorm(primary_key)]
    id: String,
}

#[tideorm::model(table = "model_test_serialization")]
struct SerializationModel {
    #[tideorm(primary_key)]
    id: i64,
    params: String,
    enabled: bool,
}

#[tideorm::model(table = "model_test_presenter_serialization")]
struct PresenterSerializationModel {
    #[tideorm(primary_key)]
    id: i64,
    params: serde_json::Value,
    title: String,
}

#[cfg(feature = "translations")]
#[tideorm::model(table = "model_test_translations", translatable = "title")]
struct TranslationSerializationModel {
    #[tideorm(primary_key)]
    id: i64,
    title: String,
    translations: Option<serde_json::Value>,
}

#[cfg(feature = "attachments")]
#[tideorm::model(table = "model_test_attachments", has_one_files = "thumbnail")]
struct FileSerializationModel {
    #[tideorm(primary_key)]
    id: i64,
    files: Option<serde_json::Value>,
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

#[cfg(feature = "translations")]
#[test]
fn test_load_language_translations_updates_model_fields() {
    let mut model = TranslationSerializationModel {
        id: 1,
        title: "Default Title".to_string(),
        translations: Some(serde_json::json!({
            "title": {
                "en": "English Title",
                "fr": "French Title"
            }
        })),
    };

    model.load_language_translations("fr").unwrap();

    assert_eq!(model.title, "French Title");
}

#[cfg(feature = "translations")]
#[test]
fn test_load_all_translations_fails_loudly() {
    let mut model = TranslationSerializationModel {
        id: 1,
        title: "Default Title".to_string(),
        translations: Some(serde_json::json!({
            "title": {
                "en": "English Title"
            }
        })),
    };

    let error = model.load_all_translations().unwrap_err();

    assert!(error.contains("not supported"));
}

#[cfg(feature = "attachments")]
#[test]
fn test_files_attribute_round_trip_updates_model() {
    let mut model = FileSerializationModel { id: 3, files: None };
    let mut files = std::collections::HashMap::new();
    files.insert(
        "thumbnail".to_string(),
        serde_json::json!({
            "key": "uploads/example.png"
        }),
    );

    model.set_files_attribute(files.clone()).unwrap();

    assert_eq!(
        model.files,
        Some(serde_json::json!({
            "thumbnail": {
                "key": "uploads/example.png"
            }
        }))
    );
    assert_eq!(model.get_files_attribute().unwrap(), files);
}
