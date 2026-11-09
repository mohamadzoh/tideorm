use super::*;
use crate::tokenization::TokenConfig;

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
        [
            ("phone_number", "customer_phone_number"),
            ("backup_phone", "backup_phone")
        ]
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

    let ciphertext = encrypt_model_field(&phone, table, "phone_number", "customer_phone_number")
        .expect("encrypting test ciphertext should succeed")
        .expect("a string seals into a payload");
    assert_eq!(
        decrypt_model_field::<String>(
            Some(ciphertext.clone()),
            table,
            "phone_number",
            "customer_phone_number"
        )
        .expect("its own column should decrypt it"),
        Some(phone.clone())
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
        let error = decrypt_model_field::<String>(Some(payload), table, "phone_number", column)
            .expect_err(case);
        assert!(
            error
                .to_string()
                .contains("Failed to decrypt encrypted field"),
            "{case}: {error}"
        );
    }

    TokenConfig::reset();
}

/// Any serde value seals and opens; `null` is NULL, which reads back only
/// into a field that can hold it.
#[test]
fn any_serde_value_round_trips_and_null_stays_null() {
    TokenConfig::reset();
    TokenConfig::set_encryption_key(TEST_KEY);
    let table = EncryptedFieldModel::table_name();

    let sealed = encrypt_model_field(&1234_u32, table, "pin", "pin")
        .expect("encrypting a number should succeed")
        .expect("a number seals into a payload");
    let opened = decrypt_model_field::<u32>(Some(sealed), table, "pin", "pin");
    assert_eq!(opened.expect("its own column opens it"), Some(1234));
    let null = encrypt_model_field(&None::<u32>, table, "pin", "pin");
    assert_eq!(null.expect("encrypting null should succeed"), None);
    let nullable = decrypt_model_field::<Option<u32>>(None, table, "pin", "pin");
    assert_eq!(nullable.expect("NULL reads as None"), Some(None));
    let required = decrypt_model_field::<u32>(None, table, "pin", "pin");
    assert_eq!(required.expect("NULL is no error here"), None);

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

    #[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Address {
        street: String,
        floor: u8,
    }

    #[tideorm::model(table = "encrypted_profiles", encrypted = "pin, address, backup_pin")]
    struct EncryptedProfile {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        pin: u32,
        address: Address,
        backup_pin: Option<u32>,
    }

    /// A field of any serde type is encrypted: its column holds the payload,
    /// a decoded row holds the plaintext before any load step runs, and a
    /// batch `set()` takes a value of the field's type and refuses another.
    #[tokio::test]
    async fn non_string_encrypted_fields_decode_into_plaintext() {
        let _guard = crate::test_support::global_db_lock().lock().await;
        let _db = install(&[
            "CREATE TABLE encrypted_profiles (id INTEGER PRIMARY KEY AUTOINCREMENT, pin TEXT NOT NULL, address TEXT NOT NULL, backup_pin TEXT NULL)",
        ])
        .await;
        let address = Address {
            street: "Main".into(),
            floor: 3,
        };
        let created = EncryptedProfile::create(EncryptedProfile {
            pin: 1234,
            address: address.clone(),
            ..Default::default()
        })
        .await
        .expect("create failed");
        assert_eq!((created.pin, &created.address), (1234, &address));
        assert_eq!(created.backup_pin, None);

        let stored = Database::raw_json("SELECT pin, address, backup_pin FROM encrypted_profiles")
            .await
            .expect("reading the stored row should succeed");
        assert_sealed(&stored[0]["pin"]);
        assert_sealed(&stored[0]["address"]);
        assert_eq!(stored[0]["backup_pin"], serde_json::Value::Null);

        // `raw_as` only decodes the row: nothing runs after `FromQueryResult`.
        let decoded: Vec<EncryptedProfile> = Database::raw_as("SELECT * FROM encrypted_profiles")
            .await
            .expect("decoding the row should succeed");
        assert_eq!((decoded[0].pin, &decoded[0].address), (1234, &address));
        assert!(
            crate::internal::InternalModel::same_columns(&decoded[0], &created),
            "the columns compare as plaintext"
        );

        EncryptedProfile::update_all()
            .set("pin", 4321_u32)
            .set("backup_pin", 7_u32)
            .set(
                "address",
                serde_json::json!({ "street": "Side", "floor": 1 }),
            )
            .where_eq("id", created.id)
            .execute()
            .await
            .expect("batch-setting values of the fields' types should succeed");
        let updated = EncryptedProfile::find_or_fail(created.id)
            .await
            .expect("the updated row loads");
        assert_eq!((updated.pin, updated.backup_pin), (4321, Some(7)));
        assert_eq!(updated.address.street, "Side");
        assert_sealed(
            &Database::raw_json("SELECT pin FROM encrypted_profiles")
                .await
                .expect("reading the stored row should succeed")[0]["pin"],
        );

        for (field, value) in [("pin", crate::DbValue::from("4321")), ("address", 5.into())] {
            let refused = EncryptedProfile::update_all()
                .set(field, value)
                .where_eq("id", created.id)
                .execute()
                .await
                .expect_err(field);
            assert!(refused.to_string().contains("cannot hold"), "{refused}");
        }

        reset();
    }
}
