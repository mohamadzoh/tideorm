use tideorm::types::Encrypted;

#[test]
fn encrypted_serialize_fails_without_configured_key() {
    let err = serde_json::to_value(&Encrypted::new("secret".to_string())).unwrap_err();
    let message = err.to_string();

    assert!(message.contains("Encrypted<T> serialization requires an encryption key"));
    assert!(message.contains("Configure one during startup"));
    assert!(message.contains("TideConfig::init().encryption_key"));
    assert!(message.contains("TokenConfig::set_encryption_key"));
}