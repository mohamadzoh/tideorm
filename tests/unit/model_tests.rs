use crate::model::Model as ModelTrait;
use crate::tokenization::Tokenizable as _;

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use crate::{Database, QueryCache, TideConfig};
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use std::sync::{Mutex, OnceLock};
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use std::time::Duration;

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

#[tideorm::model(table = "model_test_presenter_serialization")]
struct PresenterSerializationModel {
    #[tideorm(primary_key)]
    id: i64,
    params: serde_json::Value,
    title: String,
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

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
fn model_cache_test_guard() -> &'static Mutex<()> {
    static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    GUARD.get_or_init(|| Mutex::new(()))
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
async fn setup_model_cache_test_db() -> Database {
    Database::reset_global();
    TideConfig::reset();
    QueryCache::global().clear();
    QueryCache::global().enable();

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

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn save_invalidates_cached_queries_after_insert() {
    let _guard = model_cache_test_guard()
        .lock()
        .expect("model cache test guard should not be poisoned");
    let _db = setup_model_cache_test_db().await;

    let cached_before = AutoIncrementModel::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(Duration::from_secs(60))
        .get()
        .await
        .expect("initial cached query should succeed");
    assert!(cached_before.is_empty());
    assert_eq!(QueryCache::global().stats().entries, 1);

    let saved = AutoIncrementModel {
        id: 0,
        name: "Alice".to_string(),
    }
    .save()
    .await
    .expect("save should insert a new row");

    assert!(saved.id > 0);
    assert_eq!(QueryCache::global().stats().entries, 0);

    let fresh = AutoIncrementModel::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(Duration::from_secs(60))
        .get()
        .await
        .expect("fresh cached query should succeed after save");
    assert_eq!(fresh.len(), 1);
    assert_eq!(fresh[0].name, "Alice");

    QueryCache::global().clear();
    QueryCache::global().disable();
    Database::reset_global();
    TideConfig::reset();
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn update_invalidates_cached_queries() {
    let _guard = model_cache_test_guard()
        .lock()
        .expect("model cache test guard should not be poisoned");
    let _db = setup_model_cache_test_db().await;

    let saved = AutoIncrementModel {
        id: 0,
        name: "Alice".to_string(),
    }
    .save()
    .await
    .expect("seed save should succeed");

    let cached_before = AutoIncrementModel::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(Duration::from_secs(60))
        .get()
        .await
        .expect("cached query before update should succeed");
    assert_eq!(cached_before.len(), 1);
    assert_eq!(cached_before[0].name, "Alice");
    assert_eq!(QueryCache::global().stats().entries, 1);

    let updated = AutoIncrementModel {
        id: saved.id,
        name: "Bob".to_string(),
    }
    .update()
    .await
    .expect("update should succeed");

    assert_eq!(updated.name, "Bob");
    assert_eq!(QueryCache::global().stats().entries, 0);

    let fresh = AutoIncrementModel::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(Duration::from_secs(60))
        .get()
        .await
        .expect("fresh cached query should succeed after update");
    assert_eq!(fresh.len(), 1);
    assert_eq!(fresh[0].name, "Bob");

    QueryCache::global().clear();
    QueryCache::global().disable();
    Database::reset_global();
    TideConfig::reset();
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn delete_invalidates_cached_queries() {
    let _guard = model_cache_test_guard()
        .lock()
        .expect("model cache test guard should not be poisoned");
    let _db = setup_model_cache_test_db().await;

    let saved = AutoIncrementModel {
        id: 0,
        name: "Alice".to_string(),
    }
    .save()
    .await
    .expect("seed save should succeed");

    let cached_before = AutoIncrementModel::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(Duration::from_secs(60))
        .get()
        .await
        .expect("cached query before delete should succeed");
    assert_eq!(cached_before.len(), 1);
    assert_eq!(QueryCache::global().stats().entries, 1);

    let rows_affected = saved.delete().await.expect("delete should succeed");

    assert_eq!(rows_affected, 1);
    assert_eq!(QueryCache::global().stats().entries, 0);

    let fresh = AutoIncrementModel::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(Duration::from_secs(60))
        .get()
        .await
        .expect("fresh cached query should succeed after delete");
    assert!(fresh.is_empty());

    QueryCache::global().clear();
    QueryCache::global().disable();
    Database::reset_global();
    TideConfig::reset();
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn destroy_invalidates_cached_queries() {
    let _guard = model_cache_test_guard()
        .lock()
        .expect("model cache test guard should not be poisoned");
    let _db = setup_model_cache_test_db().await;

    let saved = AutoIncrementModel {
        id: 0,
        name: "Alice".to_string(),
    }
    .save()
    .await
    .expect("seed save should succeed");

    let cached_before = AutoIncrementModel::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(Duration::from_secs(60))
        .get()
        .await
        .expect("cached query before destroy should succeed");
    assert_eq!(cached_before.len(), 1);
    assert_eq!(QueryCache::global().stats().entries, 1);

    let rows_affected = AutoIncrementModel::destroy(saved.id)
        .await
        .expect("destroy should succeed");

    assert_eq!(rows_affected, 1);
    assert_eq!(QueryCache::global().stats().entries, 0);

    let fresh = AutoIncrementModel::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(Duration::from_secs(60))
        .get()
        .await
        .expect("fresh cached query should succeed after destroy");
    assert!(fresh.is_empty());

    QueryCache::global().clear();
    QueryCache::global().disable();
    Database::reset_global();
    TideConfig::reset();
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
fn test_load_language_translations_preserves_loaded_relations() {
    let cached_profile = TranslationRelationProfile {
        id: 10,
        user_id: 1,
        bio: "Cached profile".to_string(),
    }
    .with_relations();
    let cached_post = TranslationRelationPost {
        id: 20,
        user_id: 1,
        title: "Cached post".to_string(),
        author: Default::default(),
    }
    .with_relations();

    let mut model = TranslationRelationUser {
        id: 1,
        title: "Default Title".to_string(),
        translations: Some(serde_json::json!({
            "title": {
                "en": "English Title",
                "fr": "French Title"
            }
        })),
        profile: Default::default(),
        posts: Default::default(),
        roles: Default::default(),
    }
    .with_relations();
    model.profile.set_cached(Some(cached_profile));
    model.posts.set_cached(vec![cached_post]);

    model.load_language_translations("fr").unwrap();

    assert_eq!(model.title, "French Title");
    assert_eq!(
        model.profile.get_cached().map(|profile| profile.id),
        Some(10)
    );
    assert_eq!(model.posts.get_cached().map(|posts| posts.len()), Some(1));
    assert_eq!(model.posts.get_cached().unwrap()[0].id, 20);
}

#[cfg(feature = "translations")]
#[test]
fn test_load_language_translations_preserves_loaded_has_many_through_relations() {
    let cached_role = TranslationRelationRole {
        id: 30,
        name: "Cached role".to_string(),
    }
    .with_relations();

    let mut model = TranslationRelationUser {
        id: 1,
        title: "Default Title".to_string(),
        translations: Some(serde_json::json!({
            "title": {
                "en": "English Title",
                "fr": "French Title"
            }
        })),
        profile: Default::default(),
        posts: Default::default(),
        roles: Default::default(),
    }
    .with_relations();
    model.roles.set_cached(vec![cached_role]);

    model.load_language_translations("fr").unwrap();

    assert_eq!(model.title, "French Title");
    assert_eq!(model.roles.get_cached().map(|roles| roles.len()), Some(1));
    assert_eq!(model.roles.get_cached().unwrap()[0].id, 30);
}

#[cfg(feature = "translations")]
#[test]
fn test_with_relations_preserves_deserialized_cached_relations() {
    let model: TranslationRelationUser = serde_json::from_value(serde_json::json!({
        "id": 1,
        "title": "Default Title",
        "translations": null,
        "profile": {
            "id": 10,
            "user_id": 1,
            "bio": "Cached profile"
        },
        "posts": [
            {
                "id": 20,
                "user_id": 1,
                "title": "Cached post",
                "author": null
            }
        ],
        "roles": [
            {
                "id": 30,
                "name": "Cached role"
            }
        ]
    }))
    .expect("model should deserialize with cached relations");

    assert_eq!(model.profile.foreign_key, "user_id");
    assert_eq!(model.profile.local_key, "id");
    assert_eq!(
        model.profile.get_cached().map(|profile| profile.id),
        Some(10)
    );

    assert_eq!(model.posts.foreign_key, "user_id");
    assert_eq!(model.posts.local_key, "id");
    assert_eq!(model.posts.get_cached().map(|posts| posts.len()), Some(1));
    assert_eq!(model.posts.get_cached().unwrap()[0].id, 20);

    assert_eq!(model.roles.foreign_key, "user_id");
    assert_eq!(model.roles.related_key, "role_id");
    assert_eq!(model.roles.get_cached().map(|roles| roles.len()), Some(1));
    assert_eq!(model.roles.get_cached().unwrap()[0].id, 30);
}

#[cfg(feature = "translations")]
#[test]
fn test_model_relation_serialization_round_trip_preserves_cached_relations() {
    let cached_profile = TranslationRelationProfile {
        id: 10,
        user_id: 1,
        bio: "Cached profile".to_string(),
    }
    .with_relations();
    let cached_post = TranslationRelationPost {
        id: 20,
        user_id: 1,
        title: "Cached post".to_string(),
        author: Default::default(),
    }
    .with_relations();
    let cached_role = TranslationRelationRole {
        id: 30,
        name: "Cached role".to_string(),
    }
    .with_relations();

    let mut model = TranslationRelationUser {
        id: 1,
        title: "Default Title".to_string(),
        translations: None,
        profile: Default::default(),
        posts: Default::default(),
        roles: Default::default(),
    }
    .with_relations();
    model.profile.set_cached(Some(cached_profile));
    model.posts.set_cached(vec![cached_post]);
    model.roles.set_cached(vec![cached_role]);

    let value = serde_json::to_value(&model).expect("model should serialize with cached relations");
    let round_trip: TranslationRelationUser = serde_json::from_value(value)
        .expect("model should deserialize after serialization round trip");

    assert_eq!(
        round_trip.profile.get_cached().map(|profile| profile.id),
        Some(10)
    );
    assert_eq!(
        round_trip.posts.get_cached().map(|posts| posts.len()),
        Some(1)
    );
    assert_eq!(round_trip.posts.get_cached().unwrap()[0].id, 20);
    assert_eq!(
        round_trip.roles.get_cached().map(|roles| roles.len()),
        Some(1)
    );
    assert_eq!(round_trip.roles.get_cached().unwrap()[0].id, 30);
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

#[cfg(feature = "attachments")]
#[test]
fn test_set_files_attribute_preserves_loaded_belongs_to_relation() {
    let cached_author = AttachmentRelationUser {
        id: 7,
        name: "Cached author".to_string(),
    }
    .with_relations();
    let mut model = AttachmentRelationPost {
        id: 3,
        user_id: 7,
        files: None,
        author: Default::default(),
    }
    .with_relations();
    let mut files = std::collections::HashMap::new();
    files.insert(
        "thumbnail".to_string(),
        serde_json::json!({
            "key": "uploads/example.png"
        }),
    );
    model.author.set_cached(Some(cached_author));

    model.set_files_attribute(files).unwrap();

    assert_eq!(model.author.get_cached().map(|author| author.id), Some(7));
    assert_eq!(
        model.files,
        Some(serde_json::json!({
            "thumbnail": {
                "key": "uploads/example.png"
            }
        }))
    );
}

#[cfg(feature = "attachments")]
#[test]
fn test_with_relations_preserves_deserialized_belongs_to_cache() {
    let model: AttachmentRelationPost = serde_json::from_value(serde_json::json!({
        "id": 3,
        "user_id": 7,
        "files": null,
        "author": {
            "id": 7,
            "name": "Cached author"
        }
    }))
    .expect("attachment relation model should deserialize with cached author");

    assert_eq!(model.author.foreign_key, "user_id");
    assert_eq!(model.author.owner_key, "id");
    assert_eq!(model.author.get_cached().map(|author| author.id), Some(7));
}
