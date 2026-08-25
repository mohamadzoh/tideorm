use super::*;

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
fn test_is_new_treats_zero_numeric_natural_key_as_unsaved() {
    let model = NumericNaturalKeyModel { id: 0 };

    assert!(model.is_new());
}

#[test]
fn test_is_new_does_not_treat_non_zero_numeric_natural_key_as_unsaved() {
    let model = NumericNaturalKeyModel { id: 42 };

    assert!(!model.is_new());
}

#[test]
fn test_is_new_treats_nil_uuid_natural_key_as_unsaved() {
    let model = UuidNaturalKeyModel {
        id: uuid::Uuid::nil(),
    };

    assert!(model.is_new());
}

#[test]
fn test_is_new_does_not_treat_non_nil_uuid_natural_key_as_unsaved() {
    let model = UuidNaturalKeyModel {
        id: uuid::Uuid::new_v4(),
    };

    assert!(!model.is_new());
}

#[test]
fn test_macro_generated_tokenization_round_trips_string_primary_key() {
    init_model_tokenization_test_key();

    let model = TokenizedStringKeyModel {
        id: "user:\"alpha\"\n42".to_string(),
    };

    let token = model.tokenize().expect("tokenization should succeed");
    let decoded = TokenizedStringKeyModel::decode_token(&token)
        .expect("generated tokenization should decode complex string keys");

    assert_eq!(decoded, model.id);
}

#[test]
fn test_macro_generated_tokenization_round_trips_u64_primary_key() {
    init_model_tokenization_test_key();

    let model = TokenizedU64KeyModel { id: u64::MAX };

    let token = model.tokenize().expect("tokenization should succeed");
    let decoded = TokenizedU64KeyModel::decode_token(&token)
        .expect("generated tokenization should decode u64 keys");

    assert_eq!(decoded, u64::MAX);

    let direct =
        TokenizedU64KeyModel::tokenize_id(u64::MAX).expect("tokenize_id should support u64 keys");
    assert_eq!(
        TokenizedU64KeyModel::decode_token(&direct).unwrap(),
        u64::MAX
    );
}

#[test]
fn test_macro_generated_tokenization_round_trips_uuid_primary_key() {
    init_model_tokenization_test_key();

    let id = uuid::Uuid::new_v4();
    let model = TokenizedUuidKeyModel { id };

    let token = model.tokenize().expect("tokenization should succeed");
    let decoded = TokenizedUuidKeyModel::decode_token(&token)
        .expect("generated tokenization should decode uuid keys");

    assert_eq!(decoded, id);
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

#[test]
fn test_primary_key_name_uses_database_column_name() {
    assert_eq!(
        <CustomPrimaryKeyColumnModel as crate::model::ModelMeta>::primary_key_name(),
        "user_id"
    );
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
fn aliased_column_test_guard() -> &'static tokio::sync::Mutex<()> {
    static GUARD: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    GUARD.get_or_init(|| tokio::sync::Mutex::new(()))
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
fn prepare_aliased_column_test_state() {
    Database::reset_global();
    TideConfig::reset();
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
fn cleanup_aliased_column_test_state() {
    Database::reset_global();
    TideConfig::reset();
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
async fn setup_aliased_column_test_db() -> Database {
    prepare_aliased_column_test_state();

    let db = Database::connect("sqlite::memory:")
        .await
        .expect("sqlite in-memory connection should succeed for aliased-column tests");
    Database::set_global(db.clone()).expect("setting global database should succeed");

    db.__execute_with_params(
        "CREATE TABLE model_test_custom_pk_column (user_id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
        vec![],
    )
    .await
    .expect("creating aliased-column test schema should succeed");

    db
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn query_filters_accept_field_names_for_aliased_columns() {
    let _guard = aliased_column_test_guard().lock().await;
    let db = setup_aliased_column_test_db().await;

    db.__execute_with_params(
        "INSERT INTO model_test_custom_pk_column (user_id, name) VALUES (?, ?)",
        vec![
            crate::internal::Value::BigInt(Some(7)),
            crate::internal::Value::String(Some("Alice".to_string())),
        ],
    )
    .await
    .expect("seeding aliased-column test row should succeed");

    let loaded = CustomPrimaryKeyColumnModel::query()
        .where_eq("id", 7)
        .first()
        .await
        .expect("querying aliased column by field name should succeed")
        .expect("seeded aliased-column row should be present");

    assert_eq!(loaded.id, 7);
    assert_eq!(loaded.name, "Alice");

    cleanup_aliased_column_test_state();
}

#[test]
fn test_composite_primary_key_metadata_and_accessors() {
    let model = CompositePrimaryKeyModel {
        user_id: 7,
        role_id: 9,
        granted_by: "system".to_string(),
    };

    assert_eq!(model.primary_key(), (7, 9));
    assert_eq!(
        <CompositePrimaryKeyModel as crate::model::ModelMeta>::primary_key_names(),
        &["user_id", "role_id"]
    );
    assert_eq!(
        <CompositePrimaryKeyModel as crate::model::ModelMeta>::primary_key_display(&(7, 9)),
        "user_id = 7 AND role_id = 9"
    );
    assert!(!model.is_new());

    let primary_key_columns =
        <CompositePrimaryKeyModel as crate::internal::InternalModel>::primary_key_columns();
    assert_eq!(primary_key_columns.len(), 2);

    let _ = <CompositePrimaryKeyModel as crate::internal::InternalModel>::primary_key_condition(&(
        7, 9,
    ));
}

#[test]
fn test_is_new_treats_defaulted_composite_primary_key_component_as_unsaved() {
    let model = CompositePrimaryKeyModel {
        user_id: 0,
        role_id: 9,
        granted_by: "system".to_string(),
    };

    assert!(model.is_new());

    let model = CompositePrimaryKeyModel {
        user_id: 7,
        role_id: 0,
        granted_by: "system".to_string(),
    };

    assert!(model.is_new());
}

#[cfg(feature = "dirty-tracking")]
#[test]
fn test_original_value_accepts_field_and_column_names() {
    crate::model::__clear_dirty_snapshots();

    let original = CustomPrimaryKeyColumnModel {
        id: 7,
        name: "Alice".to_string(),
    };
    crate::model::__remember_dirty_snapshot(&original)
        .expect("dirty snapshot registration should succeed");

    let changed = CustomPrimaryKeyColumnModel {
        id: 7,
        name: "Bob".to_string(),
    };

    assert_eq!(
        changed
            .original_value("name")
            .expect("field lookup should succeed"),
        Some(Some(serde_json::json!("Alice")))
    );
    assert_eq!(
        changed
            .original_value("user_id")
            .expect("column lookup should succeed"),
        Some(Some(serde_json::json!(7)))
    );

    crate::model::__clear_dirty_snapshots();
}

#[cfg(feature = "dirty-tracking")]
#[test]
fn test_original_value_rejects_unknown_fields() {
    crate::model::__clear_dirty_snapshots();

    let model = AutoIncrementModel {
        id: 1,
        name: "Alice".to_string(),
    };
    crate::model::__remember_dirty_snapshot(&model)
        .expect("dirty snapshot registration should succeed");

    let err = model
        .original_value("missing")
        .expect_err("unknown field lookup should fail");
    assert!(
        err.to_string()
            .contains("unknown field or column 'missing' for model 'model_test_users'")
    );

    crate::model::__clear_dirty_snapshots();
}
