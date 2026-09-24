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
