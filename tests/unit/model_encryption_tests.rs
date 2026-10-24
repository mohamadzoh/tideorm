use super::*;
use crate::tokenization::TokenConfig;

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

/// `encrypted` names columns here; the metadata reports both spellings.
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

const TEST_KEY: &str = "encrypted-field-model-test-key-32chars";

#[test]
fn mismatched_encrypted_metadata_is_rejected_instead_of_truncated() {
    // `other` is the entry `zip` would drop, which would make it look like an
    // unencrypted column and let plaintext through.
    let mut rows = vec![serde_json::json!({ "other_column": "ciphertext" })];
    let cases = [
        (
            "a batch update of the dropped field",
            prepare_batch_update_value::<MismatchedEncryptedMeta>(
                "other",
                UpdateValue::Value(serde_json::Value::String("plaintext".to_string())),
            )
            .err(),
            "encrypted column name",
        ),
        (
            "a batch update of an unencrypted column",
            prepare_batch_update_value::<MismatchedEncryptedMeta>(
                "id",
                UpdateValue::Value(serde_json::Value::from(1)),
            )
            .err(),
            "encrypted field name",
        ),
        (
            "decrypting rows, which would hand the dropped column back as ciphertext",
            crate::model::decrypt_json_rows::<MismatchedEncryptedMeta>(&mut rows, &[]).err(),
            "encrypted column name",
        ),
    ];

    for (case, error, expected) in cases {
        let error = error.unwrap_or_else(|| panic!("{case}: a metadata mismatch must be reported"));
        assert!(error.to_string().contains(expected), "{case}: {error}");
    }
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

#[test]
fn encrypted_field_metadata_uses_canonical_field_and_column_names() {
    assert_eq!(
        EncryptedFieldModel::encrypted_fields(),
        vec!["phone_number", "backup_phone"]
    );
    assert_eq!(
        EncryptedFieldModel::encrypted_column_names(),
        vec!["customer_phone_number", "backup_phone"]
    );
    assert!(EncryptedFieldModel::has_encrypted_fields());
}

/// A payload opens only under the key of the table and column it was written
/// for: copied into another column or table it fails to decrypt, as does the
/// shape TideORM wrote before encrypted fields were keyed per
/// `(table, column)`, the same envelope sealed under the unscoped key.
#[test]
fn an_encrypted_field_decrypts_only_in_its_own_table_and_column() {
    TokenConfig::reset();
    TokenConfig::set_encryption_key(TEST_KEY);
    let table = EncryptedFieldModel::table_name();
    let phone = "+15551234567".to_string();

    let ciphertext = encrypt_model_field(
        phone.clone(),
        table,
        "phone_number",
        "customer_phone_number",
    )
    .expect("encrypting test ciphertext should succeed");
    assert_eq!(
        decrypt_model_field::<String>(
            ciphertext.clone(),
            table,
            "phone_number",
            "customer_phone_number"
        )
        .expect("its own column should decrypt it"),
        phone
    );

    let unscoped_key = TokenConfig::get_derived_encryption_key()
        .expect("the test encryption key should be configured");
    let plaintext =
        serde_json::to_vec(&serde_json::json!(phone)).expect("the plaintext should serialize");
    let unscoped = encrypted::encode_payload(&unscoped_key, &plaintext)
        .expect("an unscoped payload should be produced");

    for (case, payload, table, column) in [
        ("another column", ciphertext.clone(), table, "backup_phone"),
        (
            "another table",
            ciphertext,
            "other_table",
            "customer_phone_number",
        ),
        ("the unscoped key", unscoped, table, "customer_phone_number"),
    ] {
        let error =
            decrypt_model_field::<String>(payload, table, "phone_number", column).expect_err(case);
        assert!(
            error
                .to_string()
                .contains("Failed to decrypt encrypted field"),
            "{case}: {error}"
        );
    }

    TokenConfig::reset();
}

/// Encrypted fields written to and read from SQLite.
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
mod stored {
    use super::{EncryptedFieldModel, TEST_KEY};
    use crate::prelude::*;
    use crate::tokenization::TokenConfig;

    #[tideorm::model(table = "encrypted_contacts", encrypted = "phone")]
    struct EncryptedContact {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        name: String,
        #[tideorm(column = "phone_number")]
        phone: String,
    }

    /// Install a database built by `schema`, with the encryption key set.
    async fn install(schema: &[&str]) -> Database {
        let db = crate::test_support::install_sqlite_global(schema).await;
        TokenConfig::set_encryption_key(TEST_KEY);
        db
    }

    fn reset() {
        TokenConfig::reset();
        crate::test_support::reset_globals();
    }

    const CONTACTS: &str = "CREATE TABLE model_test_encrypted_contacts (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, customer_phone_number TEXT NOT NULL, backup_phone TEXT NULL)";

    /// The phone columns of the one stored contact, as stored.
    async fn stored_phones() -> serde_json::Value {
        Database::raw_json(
            "SELECT customer_phone_number, backup_phone FROM model_test_encrypted_contacts",
        )
        .await
        .expect("reading the stored row should succeed")
        .into_iter()
        .next()
        .expect("the stored row should be present")
    }

    fn assert_sealed(stored: &serde_json::Value) {
        let text = stored
            .as_str()
            .expect("an encrypted column should store text");
        assert!(text.starts_with("enc::"), "{text}");
    }

    #[tokio::test]
    async fn create_find_batch_update_and_upsert_encrypt_and_decrypt_fields() {
        let _guard = crate::test_support::global_db_lock().lock().await;
        let _db = install(&[CONTACTS]).await;

        let created = EncryptedFieldModel::create(EncryptedFieldModel {
            id: 0,
            name: "Alice".to_string(),
            phone_number: "+15551234567".to_string(),
            backup_phone: Some("+15557654321".to_string()),
        })
        .await
        .expect("creating encrypted-field model should succeed");
        assert_eq!(created.phone_number, "+15551234567");
        assert_eq!(created.backup_phone.as_deref(), Some("+15557654321"));
        let stored = stored_phones().await;
        assert_sealed(&stored["customer_phone_number"]);
        assert_sealed(&stored["backup_phone"]);

        let loaded = EncryptedFieldModel::find_or_fail(created.id)
            .await
            .expect("finding encrypted-field model should succeed");
        assert_eq!(loaded.phone_number, "+15551234567");
        assert_eq!(loaded.backup_phone.as_deref(), Some("+15557654321"));

        let rows_affected = EncryptedFieldModel::update_all()
            .where_eq("id", created.id)
            .set("phone_number", "+15550000000")
            .execute()
            .await
            .expect("batch-updating encrypted phone by field name should succeed");
        assert_eq!(rows_affected, 1);
        let updated = EncryptedFieldModel::find_or_fail(created.id)
            .await
            .expect("reloading updated encrypted-field model should succeed");
        assert_eq!(updated.phone_number, "+15550000000");
        assert_sealed(&stored_phones().await["customer_phone_number"]);

        let upserted = EncryptedFieldModel::insert_or_update(
            EncryptedFieldModel {
                id: updated.id,
                name: "Encrypted Alice Upsert".to_string(),
                phone_number: "+15551112222".to_string(),
                backup_phone: Some("+15553334444".to_string()),
            },
            vec!["id"],
        )
        .await
        .expect("upsert on encrypted-field model should succeed");
        assert_eq!(upserted.phone_number, "+15551112222");
        assert_eq!(upserted.backup_phone.as_deref(), Some("+15553334444"));
        assert_sealed(&stored_phones().await["customer_phone_number"]);

        let encrypted_conflict_error = EncryptedFieldModel::insert_or_update(
            EncryptedFieldModel {
                id: updated.id,
                name: "Encrypted Alice Duplicate".to_string(),
                phone_number: "+15551112222".to_string(),
                backup_phone: None,
            },
            vec!["phone_number"],
        )
        .await
        .expect_err("encrypted fields should not be usable as upsert conflict columns");
        assert!(
            encrypted_conflict_error
                .to_string()
                .contains("encrypted field 'phone_number' cannot be used")
        );

        reset();
    }

    #[tokio::test]
    async fn load_rejects_plaintext_values_without_prefix() {
        let _guard = crate::test_support::global_db_lock().lock().await;
        let _db = install(&[
            CONTACTS,
            "INSERT INTO model_test_encrypted_contacts (name, customer_phone_number, backup_phone) VALUES ('Bob', 'legacy-plain-phone', 'legacy-backup')",
        ])
        .await;

        let error = EncryptedFieldModel::first()
            .await
            .expect_err("loading plaintext data into an encrypted field should fail");

        reset();

        assert!(
            error
                .to_string()
                .contains("loaded plaintext data; expected a TideORM encrypted payload or null")
        );
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
        let _guard = crate::test_support::global_db_lock().lock().await;
        let _db = install(&[
            "CREATE TABLE encrypted_contacts (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, phone_number TEXT NOT NULL)",
        ])
        .await;
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

        // Read under another name, the column is decrypted all the same, and
        // another column read under its name is left as it is; the alias
        // came back as ciphertext, and the other column was decrypted.
        let rows = EncryptedContact::query()
            .select_raw("phone_number AS renamed")
            .select(vec!["phone AS dialed", "name AS phone_number"])
            .get_json()
            .await
            .expect("aliased get_json");
        assert_eq!(rows[0]["renamed"], serde_json::json!("555"));
        assert_eq!(rows[0]["dialed"], serde_json::json!("555"));
        assert_eq!(rows[0]["phone_number"], serde_json::json!("c"));

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

        reset();
    }
}
