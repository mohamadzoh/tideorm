use super::*;

use crate::model::ModelMeta;

fn init_encrypted_model_test_key() {
    crate::tokenization::TokenConfig::reset();
    crate::tokenization::TokenConfig::set_encryption_key(
        "encrypted-field-model-test-key-32chars",
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

#[test]
fn encrypted_field_ciphertext_is_bound_to_table_and_column_context() {
    init_encrypted_model_test_key();

    let ciphertext = crate::model::__encrypt_model_field(
        "+15551234567".to_string(),
        EncryptedFieldModel::table_name(),
        "phone_number",
        "customer_phone_number",
    )
    .expect("encrypting test ciphertext should succeed");

    let wrong_column = crate::model::__decrypt_model_field::<String>(
        ciphertext.clone(),
        EncryptedFieldModel::table_name(),
        "phone_number",
        "backup_phone",
    )
    .expect_err("decrypting with the wrong column scope should fail");
    assert!(wrong_column.to_string().contains("Failed to decrypt encrypted field"));

    let wrong_table = crate::model::__decrypt_model_field::<String>(
        ciphertext,
        "other_table",
        "phone_number",
        "customer_phone_number",
    )
    .expect_err("decrypting with the wrong table scope should fail");
    assert!(wrong_table.to_string().contains("Failed to decrypt encrypted field"));

    crate::tokenization::TokenConfig::reset();
}

#[test]
fn encrypted_field_rejects_legacy_global_payloads() {
    init_encrypted_model_test_key();

    let legacy_ciphertext = crate::types::__encrypt_json_value(&serde_json::json!(
        "+15551234567"
    ))
    .expect("legacy encrypted payload should be produced");

    let error = crate::model::__decrypt_model_field::<String>(
        legacy_ciphertext,
        EncryptedFieldModel::table_name(),
        "phone_number",
        "customer_phone_number",
    )
    .expect_err("legacy global payloads should be rejected by the model helper");

    assert!(error.to_string().contains("Failed to decrypt encrypted field"));

    crate::tokenization::TokenConfig::reset();
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
fn encrypted_model_test_guard() -> &'static tokio::sync::Mutex<()> {
    static GUARD: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    GUARD.get_or_init(|| tokio::sync::Mutex::new(()))
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
fn prepare_encrypted_model_test_state() {
    crate::tokenization::TokenConfig::reset();
    Database::reset_global();
    TideConfig::reset();
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
fn cleanup_encrypted_model_test_state() {
    crate::tokenization::TokenConfig::reset();
    Database::reset_global();
    TideConfig::reset();
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
async fn setup_encrypted_model_test_db() -> Database {
    prepare_encrypted_model_test_state();

    let db = Database::connect("sqlite::memory:")
        .await
        .expect("sqlite in-memory connection should succeed for encrypted field tests");
    Database::set_global(db.clone()).expect("setting global database should succeed");
    crate::tokenization::TokenConfig::set_encryption_key("encrypted-field-model-test-key-32chars");

    db.__execute_with_params(
        "CREATE TABLE model_test_encrypted_contacts (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, customer_phone_number TEXT NOT NULL, backup_phone TEXT NULL)",
        vec![],
    )
    .await
    .expect("creating encrypted field test schema should succeed");

    db
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn create_find_and_batch_update_auto_encrypt_and_decrypt_fields() {
    let _guard = encrypted_model_test_guard().lock().await;
    let _db = setup_encrypted_model_test_db().await;

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

    let stored_rows = Database::raw_json(
        "SELECT customer_phone_number, backup_phone FROM model_test_encrypted_contacts",
    )
        .await
        .expect("reading stored encrypted row should succeed");
    let stored_row = stored_rows
        .first()
        .and_then(serde_json::Value::as_object)
        .expect("stored row should be present");
    let stored_phone = stored_row
        .get("customer_phone_number")
        .and_then(serde_json::Value::as_str)
        .expect("stored encrypted phone should be a string");
    let stored_backup = stored_row
        .get("backup_phone")
        .and_then(serde_json::Value::as_str)
        .expect("stored encrypted backup phone should be a string");

    assert!(stored_phone.starts_with("enc::"));
    assert!(stored_backup.starts_with("enc::"));
    assert_ne!(stored_phone, "+15551234567");
    assert_ne!(stored_backup, "+15557654321");

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

    let stored_rows = Database::raw_json(
        "SELECT customer_phone_number FROM model_test_encrypted_contacts",
    )
        .await
        .expect("reading updated encrypted row should succeed");
    let stored_phone = stored_rows
        .first()
        .and_then(serde_json::Value::as_object)
        .and_then(|row| row.get("customer_phone_number"))
        .and_then(serde_json::Value::as_str)
        .expect("updated encrypted phone should be a string");
    assert!(stored_phone.starts_with("enc::"));
    assert_ne!(stored_phone, "+15550000000");

    cleanup_encrypted_model_test_state();
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn load_rejects_plaintext_values_without_prefix() {
    let _guard = encrypted_model_test_guard().lock().await;
    let db = setup_encrypted_model_test_db().await;

    db.__execute_with_params(
        "INSERT INTO model_test_encrypted_contacts (name, customer_phone_number, backup_phone) VALUES (?, ?, ?)",
        vec![
            crate::internal::Value::String(Some("Bob".to_string())),
            crate::internal::Value::String(Some("legacy-plain-phone".to_string())),
            crate::internal::Value::String(Some("legacy-backup".to_string())),
        ],
    )
    .await
    .expect("inserting legacy plaintext row should succeed");

    let error = EncryptedFieldModel::first()
        .await
        .expect_err("loading plaintext data into an encrypted field should fail");

    cleanup_encrypted_model_test_state();

    assert!(error
        .to_string()
        .contains("loaded plaintext data; expected a TideORM encrypted payload or null"));
}