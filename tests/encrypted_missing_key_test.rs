use tideorm::types::Encrypted;

#[test]
fn encrypted_serialize_fails_without_configured_key() {
    let err = serde_json::to_value(&Encrypted::new("secret".to_string())).unwrap_err();
    assert!(err.to_string().contains("No encryption key configured"));
}