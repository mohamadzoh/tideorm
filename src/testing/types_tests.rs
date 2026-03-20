use super::{encrypted_field_missing_key_error, Encrypted};

#[test]
fn encrypted_missing_key_error_mentions_startup_configuration_for_serialization() {
    let err = encrypted_field_missing_key_error("serialization");
    let message = err.to_string();

    assert!(message.contains("Encrypted<T> serialization requires an encryption key"));
    assert!(message.contains("Configure one during startup"));
    assert!(message.contains("TideConfig::init().encryption_key"));
    assert!(message.contains("TokenConfig::set_encryption_key"));
}

#[test]
fn encrypted_missing_key_error_mentions_deserialization() {
    let err = encrypted_field_missing_key_error("deserialization");
    let message = err.to_string();

    assert!(message.contains("Encrypted<T> deserialization requires an encryption key"));
}

#[test]
fn encrypted_wrapper_preserves_inner_value() {
    let encrypted = Encrypted::new(String::from("secret"));
    assert_eq!(encrypted.inner(), "secret");
}
