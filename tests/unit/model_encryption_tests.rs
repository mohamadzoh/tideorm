use super::*;

/// Metadata whose two encrypted lists disagree. Macro-generated models
/// cannot produce this, but the trait lets it be written by hand and the
/// failure mode is silent plaintext, so it has to be rejected.
#[derive(Clone)]
struct MismatchedEncryptedMeta;

impl ModelMeta for MismatchedEncryptedMeta {
    type PrimaryKey = i64;

    fn table_name() -> &'static str {
        "mismatched_encrypted_models"
    }

    fn primary_key_names() -> &'static [&'static str] {
        &["id"]
    }

    fn primary_key_display(primary_key: &Self::PrimaryKey) -> String {
        primary_key.to_string()
    }

    fn column_names() -> &'static [&'static str] {
        &["id", "secret_column", "other_column"]
    }

    fn field_names() -> &'static [&'static str] {
        &["id", "secret", "other"]
    }

    fn encrypted_fields() -> Vec<&'static str> {
        vec!["secret", "other"]
    }

    fn encrypted_column_names() -> Vec<&'static str> {
        vec!["secret_column"]
    }
}

#[test]
fn mismatched_encrypted_metadata_is_rejected_instead_of_truncated() {
    // `other` is the entry `zip` would drop, which would make it look like an
    // unencrypted column and let plaintext through.
    let error = prepare_batch_update_value::<MismatchedEncryptedMeta>(
        "other",
        UpdateValue::Value(serde_json::Value::String("plaintext".to_string())),
    )
    .expect_err("a metadata mismatch must not fall through to plaintext");

    assert!(
        error.to_string().contains("encrypted column name"),
        "unexpected error: {error}"
    );
}

#[test]
fn mismatched_encrypted_metadata_is_rejected_for_unrelated_columns_too() {
    let error = prepare_batch_update_value::<MismatchedEncryptedMeta>(
        "id",
        UpdateValue::Value(serde_json::Value::from(1)),
    )
    .expect_err("a metadata mismatch must be reported, not skipped");

    assert!(
        error.to_string().contains("encrypted field name"),
        "unexpected error: {error}"
    );
}

#[test]
fn crypto_errors_keep_their_class_and_name_the_field() {
    let error = annotate_crypto_error(
        Error::configuration("no key"),
        "encrypt",
        "phone_number",
        "customer_phone_number",
    );
    assert!(matches!(error, Error::Configuration { .. }), "{error:?}");
    assert!(
        error
            .to_string()
            .contains("Failed to encrypt encrypted field 'phone_number (customer_phone_number)'"),
        "{error}"
    );

    let error = annotate_crypto_error(Error::tokenization("bad tag"), "decrypt", "note", "note");
    assert!(matches!(error, Error::Tokenization { .. }), "{error:?}");
    assert!(
        error
            .to_string()
            .contains("Failed to decrypt encrypted field 'note'"),
        "{error}"
    );
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
mod stored {
    use crate::prelude::*;

    #[tideorm::model(table = "encrypted_contacts", encrypted = "phone")]
    struct EncryptedContact {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        name: String,
        #[tideorm(column = "phone_number")]
        phone: String,
    }

    #[derive(serde::Deserialize)]
    struct ContactRow {
        phone_number: String,
    }

    /// `pluck`, `value`, `get_json` and `get_as` return what `get()` does, not
    /// the stored ciphertext, and a batch `set()` of a value that is not a
    /// string is refused rather than stored where no load could read it.
    #[tokio::test]
    async fn json_reads_decrypt_and_non_string_batch_values_are_refused() {
        crate::Database::reset_global();
        crate::TideConfig::reset();
        crate::TideConfig::init()
            .database("sqlite::memory:")
            .max_connections(1)
            .encryption_key("encrypted-contacts-test-key")
            .connect()
            .await
            .expect("connect failed");
        crate::migration::Schema::new(DatabaseType::SQLite)
            .create_table("encrypted_contacts", |t| {
                t.id();
                t.string("name").not_null();
                t.text("phone_number").not_null();
            })
            .await
            .expect("create table failed");
        let contact = EncryptedContact::create(EncryptedContact {
            name: "c".into(),
            phone: "555".into(),
            ..Default::default()
        })
        .await
        .expect("create failed");

        let plucked: Vec<String> = EncryptedContact::query()
            .pluck("phone")
            .await
            .expect("pluck");
        assert_eq!(plucked, ["555"]);
        let value: Option<String> = EncryptedContact::query()
            .value("phone")
            .await
            .expect("value");
        assert_eq!(value.as_deref(), Some("555"));
        let rows = EncryptedContact::query()
            .get_json()
            .await
            .expect("get_json");
        assert_eq!(rows[0]["phone_number"], serde_json::json!("555"));
        let rows: Vec<ContactRow> = EncryptedContact::query().get_as().await.expect("get_as");
        assert_eq!(rows[0].phone_number, "555");

        let refused = EncryptedContact::update_all()
            .set("phone", 5_551_234_567_i64)
            .where_eq("id", contact.id)
            .execute()
            .await;
        assert!(refused.is_err(), "{refused:?}");
        let stored = EncryptedContact::find(contact.id)
            .await
            .expect("the row still loads")
            .expect("the row exists");
        assert_eq!(stored.phone, "555");

        crate::Database::reset_global();
        crate::TideConfig::reset();
        crate::tokenization::TokenConfig::reset();
    }
}
