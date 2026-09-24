use tideorm::types::Hashed;

#[test]
fn test_hashed_uses_argon2_format() {
    let hashed = Hashed::new("secret123");
    assert!(hashed.hash().starts_with("$argon2"));
}

#[test]
fn test_hashed_verify_accepts_matching_password() {
    let hashed = Hashed::new("secret123");
    assert!(hashed.verify("secret123"));
    assert!(!hashed.verify("wrong-password"));
}

#[test]
fn test_hashed_is_salted() {
    let first = Hashed::new("secret123");
    let second = Hashed::new("secret123");

    assert_ne!(first.hash(), second.hash());
    assert!(first.verify("secret123"));
    assert!(second.verify("secret123"));
}

#[test]
fn test_hashed_verify_rejects_non_argon2_hashes() {
    let hashed = Hashed::from_hash("legacy-hash-value".to_string());

    assert!(!hashed.verify("secret123"));
    assert!(!hashed.verify("wrong-password"));
}

#[test]
fn test_hashed_from_str_and_string_hash_the_input() {
    let from_str: Hashed = "password123".into();
    let from_string: Hashed = "password123".to_string().into();

    assert!(from_str.verify("password123"));
    assert!(from_string.verify("password123"));
}

#[test]
fn test_hashed_serialize_redacts_raw_hash() {
    let hashed = Hashed::new("password123");

    let serialized = serde_json::to_value(&hashed).unwrap();

    assert_eq!(serialized, serde_json::json!("***HASHED***"));
    assert_ne!(serialized, serde_json::json!(hashed.hash()));
}

#[test]
fn test_hashed_nested_serialization_does_not_leak_argon2_hash() {
    #[derive(serde::Serialize)]
    struct UserPayload {
        password: Hashed,
    }

    let payload = UserPayload {
        password: Hashed::new("password123"),
    };

    let serialized = serde_json::to_value(&payload).unwrap();
    let password = serialized
        .get("password")
        .and_then(serde_json::Value::as_str)
        .unwrap();

    assert_eq!(password, "***HASHED***");
    assert!(!password.starts_with("$argon2"));
}

#[test]
fn test_hashed_deserialize_rejects_redacted_payload() {
    let err = serde_json::from_value::<Hashed>(serde_json::json!("***HASHED***")).unwrap_err();

    assert!(err.to_string().contains("redacted serialization format"));
}
