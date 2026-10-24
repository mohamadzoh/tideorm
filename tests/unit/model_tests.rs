use crate::model::Model as ModelTrait;
use crate::tokenization::Tokenizable as _;

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

#[tideorm::model(table = "model_test_numeric_natural_keys")]
struct NumericNaturalKeyModel {
    #[tideorm(primary_key)]
    id: i64,
}

#[tideorm::model(table = "model_test_uuid_natural_keys")]
struct UuidNaturalKeyModel {
    #[tideorm(primary_key)]
    id: uuid::Uuid,
}

#[tideorm::model(table = "model_test_tokenized_string_keys", tokenize)]
struct TokenizedStringKeyModel {
    #[tideorm(primary_key)]
    id: String,
}

#[tideorm::model(table = "model_test_tokenized_u64_keys", tokenize)]
struct TokenizedU64KeyModel {
    #[tideorm(primary_key)]
    id: u64,
}

#[tideorm::model(table = "model_test_tokenized_uuid_keys", tokenize)]
struct TokenizedUuidKeyModel {
    #[tideorm(primary_key)]
    id: uuid::Uuid,
}

#[tideorm::model(table = "model_test_custom_pk_column")]
struct CustomPrimaryKeyColumnModel {
    #[tideorm(primary_key, column = "user_id")]
    id: i64,
    name: String,
}

#[tideorm::model(table = "model_test_user_roles")]
struct CompositePrimaryKeyModel {
    #[tideorm(primary_key)]
    user_id: i64,
    #[tideorm(primary_key)]
    role_id: i64,
    granted_by: String,
}

#[tideorm::model(table = "model_test_raw_identifiers")]
struct RawIdentifierModel {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    #[validate(min_length = 2)]
    r#type: String,
}

// The fully qualified spelling of the soft-delete timestamp used to be rejected.
#[tideorm::model(
    table = "model_test_archived_posts",
    soft_delete,
    deleted_at_column = "archived_on"
)]
struct ArchivedPostModel {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    archived_on: std::option::Option<chrono::DateTime<chrono::Utc>>,
}

#[path = "model_tests/basic_model_tests.rs"]
mod basic_model_tests;

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[path = "model_tests/cache_tests.rs"]
mod cache_tests;

#[path = "model_tests/relation_wrapper_tests.rs"]
mod relation_wrapper_tests;

/// A raw-identifier field is named `type` wherever a name is a string, and a
/// `#[validate]` on it compiles (it used to expand to `self.type`).
#[test]
fn raw_identifier_fields_round_trip_under_their_unraw_name() {
    use crate::internal::InternalModel;
    use crate::model::ModelMeta;
    use crate::validation::Validate;

    assert_eq!(RawIdentifierModel::field_names(), &["id", "type"]);
    assert_eq!(RawIdentifierModel::column_names(), &["id", "type"]);
    assert_eq!(
        RawIdentifierModel::canonical_column_name("type"),
        Some("type")
    );

    let model = RawIdentifierModel {
        id: 7,
        r#type: "note".to_string(),
    };
    assert_eq!(
        model
            .field_json_value("type")
            .expect("field lookup should succeed"),
        Some(serde_json::json!("note"))
    );

    let json = serde_json::to_value(&model).expect("model should serialize");
    assert_eq!(json, serde_json::json!({ "id": 7, "type": "note" }));
    let restored: RawIdentifierModel =
        serde_json::from_value(json).expect("model should deserialize");
    assert_eq!(restored.r#type, "note");

    let invalid = RawIdentifierModel {
        id: 7,
        r#type: "x".to_string(),
    };
    let errors = invalid
        .validate()
        .expect_err("a one-character type is too short");
    assert!(!errors.field_errors("type").is_empty(), "{errors:?}");
}

/// `deleted_at_column` is declared by `ModelMeta` alone, so an unqualified call
/// with the whole prelude in scope is not an E0034 ambiguity with `SoftDelete`.
#[test]
fn deleted_at_column_resolves_with_the_prelude_in_scope() {
    use crate::prelude::*;

    assert!(ArchivedPostModel::soft_delete_enabled());
    assert_eq!(ArchivedPostModel::deleted_at_column(), "archived_on");

    let mut post = ArchivedPostModel {
        id: 1,
        archived_on: None,
    };
    post.set_deleted_at(Some(chrono::Utc::now()));
    assert!(post.is_deleted());
}
