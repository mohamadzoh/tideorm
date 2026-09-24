use crate::model::Model as ModelTrait;
use crate::tokenization::Tokenizable as _;

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use crate::{Database, QueryCache, TideConfig};
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use std::sync::OnceLock;
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use std::time::Duration;
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use tokio::sync::Mutex;

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

#[tideorm::model(table = "model_test_serialization")]
struct SerializationModel {
    #[tideorm(primary_key)]
    id: i64,
    params: String,
    enabled: bool,
}

#[tideorm::model(table = "model_test_structured_params")]
struct StructuredParamsModel {
    #[tideorm(primary_key)]
    id: i64,
    params: serde_json::Value,
    title: String,
}

#[cfg(feature = "encrypted-fields")]
#[tideorm::model(
    table = "model_test_encrypted_contacts",
    encrypted = "customer_phone_number, backup_phone"
)]
struct EncryptedFieldModel {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
    #[tideorm(column = "customer_phone_number")]
    phone_number: String,
    backup_phone: Option<String>,
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

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
fn model_cache_test_guard() -> &'static Mutex<()> {
    static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    GUARD.get_or_init(|| Mutex::new(()))
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
fn prepare_model_cache_test_state() {
    Database::reset_global();
    TideConfig::reset();

    let query_cache = QueryCache::global();
    query_cache.clear();
    query_cache.enable();
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
fn cleanup_model_cache_test_state() {
    let query_cache = QueryCache::global();
    query_cache.clear();
    query_cache.disable();

    Database::reset_global();
    TideConfig::reset();
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
async fn setup_model_cache_test_db() -> Database {
    prepare_model_cache_test_state();

    let db = Database::connect("sqlite::memory:")
        .await
        .expect("sqlite in-memory connection should succeed for model cache tests");
    Database::set_global(db.clone()).expect("setting global database should succeed");

    db.__execute_with_params(
        "CREATE TABLE model_test_users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL)",
        vec![],
    )
    .await
    .expect("creating model cache test schema should succeed");

    db
}

fn init_model_tokenization_test_key() {
    crate::tokenization::TokenConfig::set_encryption_key("model-tokenization-test-key-32chars");
}

#[cfg(feature = "translations")]
#[tideorm::model(table = "model_test_translations", translatable = "title")]
struct TranslationSerializationModel {
    #[tideorm(primary_key)]
    id: i64,
    title: String,
    translations: Option<serde_json::Value>,
}

#[cfg(feature = "translations")]
#[tideorm::model(table = "model_test_translation_profiles")]
struct TranslationRelationProfile {
    #[tideorm(primary_key)]
    id: i64,
    user_id: i64,
    bio: String,
}

#[cfg(feature = "translations")]
#[tideorm::model(table = "model_test_translation_posts")]
struct TranslationRelationPost {
    #[tideorm(primary_key)]
    id: i64,
    user_id: i64,
    title: String,

    #[tideorm(belongs_to = "TranslationRelationUser", foreign_key = "user_id")]
    author: crate::relations::BelongsTo<TranslationRelationUser>,
}

#[cfg(feature = "translations")]
#[tideorm::model(table = "model_test_translation_roles")]
struct TranslationRelationRole {
    #[tideorm(primary_key)]
    id: i64,
    name: String,
}

#[cfg(feature = "translations")]
#[tideorm::model(table = "model_test_translation_user_roles")]
struct TranslationRelationUserRole {
    #[tideorm(primary_key)]
    id: i64,
    user_id: i64,
    role_id: i64,
}

#[cfg(feature = "translations")]
#[tideorm::model(table = "model_test_translation_users", translatable = "title")]
struct TranslationRelationUser {
    #[tideorm(primary_key)]
    id: i64,
    title: String,
    translations: Option<serde_json::Value>,

    #[tideorm(has_one = "TranslationRelationProfile", foreign_key = "user_id")]
    profile: crate::relations::HasOne<TranslationRelationProfile>,

    #[tideorm(has_many = "TranslationRelationPost", foreign_key = "user_id")]
    posts: crate::relations::HasMany<TranslationRelationPost>,

    #[tideorm(
        has_many_through = "TranslationRelationRole",
        pivot = "model_test_translation_user_roles",
        foreign_key = "user_id",
        related_key = "role_id"
    )]
    roles: crate::relations::HasManyThrough<TranslationRelationRole, TranslationRelationUserRole>,
}

#[cfg(feature = "attachments")]
#[tideorm::model(table = "model_test_attachments", has_one_files = "thumbnail")]
struct FileSerializationModel {
    #[tideorm(primary_key)]
    id: i64,
    files: Option<serde_json::Value>,
}

#[cfg(feature = "attachments")]
#[tideorm::model(table = "model_test_attachment_users")]
struct AttachmentRelationUser {
    #[tideorm(primary_key)]
    id: i64,
    name: String,
}

#[cfg(feature = "attachments")]
#[tideorm::model(table = "model_test_attachment_posts", has_one_files = "thumbnail")]
struct AttachmentRelationPost {
    #[tideorm(primary_key)]
    id: i64,
    user_id: i64,
    files: Option<serde_json::Value>,

    #[tideorm(belongs_to = "AttachmentRelationUser", foreign_key = "user_id")]
    author: crate::relations::BelongsTo<AttachmentRelationUser>,
}

#[path = "model_tests/basic_model_tests.rs"]
mod basic_model_tests;

#[path = "model_tests/cache_tests.rs"]
mod cache_tests;

#[cfg(feature = "encrypted-fields")]
#[path = "model_tests/encrypted_field_tests.rs"]
mod encrypted_field_tests;

#[cfg(feature = "dirty-tracking")]
#[path = "model_tests/dirty_tracking_tests.rs"]
mod dirty_tracking_tests;

#[path = "model_tests/translation_tests.rs"]
mod translation_tests;

#[path = "model_tests/attachment_tests.rs"]
mod attachment_tests;

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
