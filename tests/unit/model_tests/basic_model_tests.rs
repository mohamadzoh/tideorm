use super::*;

fn composite_key(user_id: i64, role_id: i64) -> CompositePrimaryKeyModel {
    CompositePrimaryKeyModel {
        user_id,
        role_id,
        granted_by: "system".to_string(),
    }
}

/// A key still holding its type's default (`0`, the nil UUID, or a composite
/// key with such a part) names no stored row; text has no such default.
#[test]
fn is_new_treats_a_defaulted_primary_key_as_unsaved() {
    let alice = |id| AutoIncrementModel {
        id,
        name: "Alice".to_string(),
    };
    let cases = [
        ("a zero auto-increment key", alice(0).is_new(), true),
        ("a non-zero auto-increment key", alice(42).is_new(), false),
        (
            "a text natural key",
            NaturalKeyModel {
                id: "user_123".to_string(),
            }
            .is_new(),
            false,
        ),
        (
            "a zero numeric natural key",
            NumericNaturalKeyModel { id: 0 }.is_new(),
            true,
        ),
        (
            "a non-zero numeric natural key",
            NumericNaturalKeyModel { id: 42 }.is_new(),
            false,
        ),
        (
            "a nil UUID key",
            UuidNaturalKeyModel {
                id: uuid::Uuid::nil(),
            }
            .is_new(),
            true,
        ),
        (
            "a random UUID key",
            UuidNaturalKeyModel {
                id: uuid::Uuid::new_v4(),
            }
            .is_new(),
            false,
        ),
        (
            "a composite key with a zero first part",
            composite_key(0, 9).is_new(),
            true,
        ),
        (
            "a composite key with a zero second part",
            composite_key(7, 0).is_new(),
            true,
        ),
        (
            "a composite key with no zero part",
            composite_key(7, 9).is_new(),
            false,
        ),
    ];

    for (case, is_new, expected) in cases {
        assert_eq!(is_new, expected, "{case}");
    }
}

#[test]
fn macro_generated_tokenization_round_trips_string_u64_and_uuid_keys() {
    crate::tokenization::TokenConfig::set_encryption_key("model-tokenization-test-key-32chars");

    let model = TokenizedStringKeyModel {
        id: "user:\"alpha\"\n42".to_string(),
    };
    let token = model.to_token().expect("a string key should tokenize");
    assert_eq!(
        TokenizedStringKeyModel::decode_token(&token).expect("a string key should decode"),
        model.id
    );

    let token = TokenizedU64KeyModel { id: u64::MAX }
        .to_token()
        .expect("a u64 key should tokenize");
    assert_eq!(
        TokenizedU64KeyModel::decode_token(&token).expect("a u64 key should decode"),
        u64::MAX
    );
    let token =
        TokenizedU64KeyModel::tokenize_id(u64::MAX).expect("tokenize_id should take a u64 key");
    assert_eq!(
        TokenizedU64KeyModel::decode_token(&token).expect("a u64 key should decode"),
        u64::MAX
    );

    let id = uuid::Uuid::new_v4();
    let token = TokenizedUuidKeyModel { id }
        .to_token()
        .expect("a UUID key should tokenize");
    assert_eq!(
        TokenizedUuidKeyModel::decode_token(&token).expect("a UUID key should decode"),
        id
    );
}

#[test]
fn a_renamed_primary_key_column_resolves_by_field_and_column_name() {
    use crate::model::ModelMeta;

    assert_eq!(CustomPrimaryKeyColumnModel::primary_key_name(), "user_id");
    assert_eq!(
        CustomPrimaryKeyColumnModel::canonical_field_name("id"),
        Some("id")
    );
    assert_eq!(
        CustomPrimaryKeyColumnModel::canonical_field_name("user_id"),
        Some("id")
    );
    assert_eq!(
        CustomPrimaryKeyColumnModel::canonical_field_name("missing"),
        None
    );
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn query_filters_accept_field_names_for_aliased_columns() {
    let _guard = crate::test_support::global_db_lock().lock().await;
    crate::test_support::install_sqlite_global(&[
        "CREATE TABLE model_test_custom_pk_column (user_id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
        "INSERT INTO model_test_custom_pk_column (user_id, name) VALUES (7, 'Alice')",
    ])
    .await;

    let loaded = CustomPrimaryKeyColumnModel::query()
        .where_eq("id", 7)
        .first()
        .await
        .expect("querying aliased column by field name should succeed")
        .expect("seeded aliased-column row should be present");

    assert_eq!(loaded.id, 7);
    assert_eq!(loaded.name, "Alice");

    crate::test_support::reset_globals();
}

#[test]
fn test_composite_primary_key_metadata_and_accessors() {
    assert_eq!(composite_key(7, 9).primary_key(), (7, 9));
    assert_eq!(
        <CompositePrimaryKeyModel as crate::model::ModelMeta>::primary_key_names(),
        &["user_id", "role_id"]
    );
    assert_eq!(
        <CompositePrimaryKeyModel as crate::model::ModelMeta>::primary_key_display(&(7, 9)),
        "user_id = 7 AND role_id = 9"
    );
    assert_eq!(
        <CompositePrimaryKeyModel as crate::internal::InternalModel>::primary_key_columns().len(),
        2
    );
}

#[cfg(feature = "dirty-tracking")]
#[test]
fn test_original_value_resolves_field_and_column_names_and_rejects_unknown_ones() {
    crate::model::__clear_dirty_snapshots();

    crate::model::__remember_dirty_snapshot(&CustomPrimaryKeyColumnModel {
        id: 7,
        name: "Alice".to_string(),
    });
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
    let error = changed
        .original_value("missing")
        .expect_err("unknown field lookup should fail");
    assert!(
        error
            .to_string()
            .contains("unknown field or column 'missing' for model 'model_test_custom_pk_column'"),
        "{error}"
    );

    crate::model::__clear_dirty_snapshots();
}

#[tideorm::model(table = "model_test_indexed")]
#[index("display_name")]
struct IndexedByFieldModel {
    #[tideorm(primary_key)]
    id: i64,
    #[tideorm(column = "display")]
    display_name: String,
}

#[tideorm::model(table = "model_test_field_indexed")]
struct FieldIndexedModel {
    #[tideorm(primary_key)]
    id: i64,
    #[index]
    name: String,
    #[unique_index]
    #[tideorm(column = "mail")]
    email: String,
}

/// An index covers its fields' columns, whether the struct names the field or
/// `#[index]` / `#[unique_index]` sits on it; on a field they were accepted
/// and ignored.
#[test]
fn indexes_cover_the_columns_of_the_fields_they_name() {
    use crate::model::ModelMeta;

    let indexes = IndexedByFieldModel::indexes();
    assert_eq!(indexes.len(), 1);
    assert_eq!(indexes[0].columns, ["display"]);

    let indexes = FieldIndexedModel::indexes();
    assert_eq!(indexes.len(), 1);
    assert_eq!(indexes[0].columns, ["name"]);
    let unique = FieldIndexedModel::unique_indexes();
    assert_eq!(unique.len(), 1);
    assert_eq!(unique[0].columns, ["mail"]);
    assert!(unique[0].unique);
}
