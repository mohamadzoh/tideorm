use std::sync::Once;

use serde_json::json;
use tideorm::tokenization::TokenConfig;
use tideorm::types::{CastType, CastValue, Encrypted, Hashed};

static ENCRYPTED_INIT: Once = Once::new();

fn init_encrypted_test_key() {
    ENCRYPTED_INIT.call_once(|| {
        TokenConfig::set_encryption_key("test-encryption-key-for-unit-tests-32");
    });
}

// Encrypted type tests
#[test]
fn test_encrypted_new() {
    let encrypted = Encrypted::new("secret".to_string());
    assert_eq!(encrypted.get(), "secret");
}

#[test]
fn test_encrypted_into_inner() {
    let encrypted = Encrypted::new("secret".to_string());
    let inner: String = encrypted.into_inner();
    assert_eq!(inner, "secret");
}

#[test]
fn test_encrypted_clone() {
    let encrypted = Encrypted::new("secret".to_string());
    let cloned = encrypted.clone();
    assert_eq!(cloned.get(), "secret");
}

#[test]
fn test_encrypted_inner() {
    let encrypted = Encrypted::new("secret".to_string());
    assert_eq!(encrypted.inner(), "secret");
}

#[test]
fn test_encrypted_from() {
    let encrypted: Encrypted<String> = "secret".to_string().into();
    assert_eq!(encrypted.get(), "secret");
}

#[test]
fn test_encrypted_serializes_to_ciphertext() {
    init_encrypted_test_key();

    let encrypted = Encrypted::new("secret".to_string());
    let serialized = serde_json::to_value(&encrypted).unwrap();

    let ciphertext = serialized.as_str().unwrap();
    assert!(ciphertext.starts_with("enc::"));
    assert_ne!(ciphertext, "secret");
    assert!(!ciphertext.contains("secret"));
}

#[test]
fn test_encrypted_round_trips_from_ciphertext() {
    init_encrypted_test_key();

    let encrypted = Encrypted::new("secret".to_string());
    let serialized = serde_json::to_value(&encrypted).unwrap();
    let round_trip: Encrypted<String> = serde_json::from_value(serialized).unwrap();

    assert_eq!(round_trip.get(), "secret");
}

#[test]
fn test_encrypted_rejects_plaintext_payloads() {
    let err = serde_json::from_value::<Encrypted<String>>(serde_json::json!("secret")).unwrap_err();

    assert!(
        err.to_string()
            .contains("Encrypted fields must use the encrypted payload format")
    );
}

#[test]
fn test_encrypted_rejects_tampered_ciphertext() {
    init_encrypted_test_key();

    let encrypted = Encrypted::new("secret".to_string());
    let serialized = serde_json::to_value(&encrypted).unwrap();
    let ciphertext = serialized.as_str().unwrap();
    let mut chars: Vec<char> = ciphertext.chars().collect();
    let tamper_index = chars.len().saturating_sub(4);
    chars[tamper_index] = if chars[tamper_index] == 'A' { 'B' } else { 'A' };
    let tampered: String = chars.into_iter().collect();

    let err = serde_json::from_value::<Encrypted<String>>(serde_json::json!(tampered)).unwrap_err();
    assert!(
        err.to_string().contains("Failed to decrypt field payload")
            || err.to_string().contains("Invalid encrypted field payload")
    );
}

// Hashed type tests
#[test]
fn test_hashed_new() {
    let hashed = Hashed::new("password123");
    // Hashed value is stored as hash, not plain text
    assert!(!hashed.hash().is_empty());
}

#[test]
fn test_hashed_verify() {
    let hashed = Hashed::new("password123");
    assert!(hashed.verify("password123"));
    assert!(!hashed.verify("wrongpassword"));
}

#[test]
fn test_hashed_from_str() {
    let hashed: Hashed = "password123".into();
    assert!(hashed.verify("password123"));
}

#[test]
fn test_hashed_from_string() {
    let hashed: Hashed = "password123".to_string().into();
    assert!(hashed.verify("password123"));
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

// CastType tests
#[test]
fn test_cast_type_from_str() {
    assert_eq!(CastType::parse_str("string"), Some(CastType::String));
    assert_eq!(CastType::parse_str("integer"), Some(CastType::Integer));
    assert_eq!(CastType::parse_str("float"), Some(CastType::Float));
    assert_eq!(CastType::parse_str("boolean"), Some(CastType::Boolean));
    assert_eq!(CastType::parse_str("json"), Some(CastType::Json));
    assert_eq!(CastType::parse_str("array"), Some(CastType::Array));
    assert_eq!(CastType::parse_str("datetime"), Some(CastType::DateTime));
    assert_eq!(CastType::parse_str("date"), Some(CastType::Date));
    assert_eq!(CastType::parse_str("time"), Some(CastType::Time));
    assert_eq!(CastType::parse_str("uuid"), Some(CastType::Uuid));
    assert_eq!(CastType::parse_str("decimal"), Some(CastType::Decimal));
    assert_eq!(CastType::parse_str("encrypted"), Some(CastType::Encrypted));
    assert_eq!(CastType::parse_str("hashed"), Some(CastType::Hashed));
    assert_eq!(
        CastType::parse_str("comma_separated"),
        Some(CastType::CommaSeparated)
    );
    assert_eq!(
        CastType::parse_str("collection"),
        Some(CastType::Collection)
    );
    assert_eq!(CastType::parse_str("unknown"), None);
}

#[test]
fn test_cast_type_display() {
    assert_eq!(CastType::String.to_string(), "string");
    assert_eq!(CastType::Integer.to_string(), "integer");
    assert_eq!(CastType::Boolean.to_string(), "boolean");
}

// CastValue tests
#[test]
fn test_cast_value_to_string() {
    let result = CastValue::cast(&json!(123), CastType::String);
    assert_eq!(result.unwrap(), json!("123"));
}

#[test]
fn test_cast_value_to_integer() {
    let result = CastValue::cast(&json!("42"), CastType::Integer);
    assert_eq!(result.unwrap(), json!(42));

    let result2 = CastValue::cast(&json!(3.14), CastType::Integer);
    assert_eq!(result2.unwrap(), json!(3));
}

#[test]
fn test_cast_value_to_float() {
    let result = CastValue::cast(&json!("3.14"), CastType::Float);
    assert_eq!(result.unwrap(), json!(3.14));

    let result2 = CastValue::cast(&json!(42), CastType::Float);
    assert_eq!(result2.unwrap(), json!(42.0));
}

#[test]
fn test_cast_value_to_boolean() {
    assert_eq!(
        CastValue::cast(&json!("true"), CastType::Boolean).unwrap(),
        json!(true)
    );
    assert_eq!(
        CastValue::cast(&json!("false"), CastType::Boolean).unwrap(),
        json!(false)
    );
    assert_eq!(
        CastValue::cast(&json!(1), CastType::Boolean).unwrap(),
        json!(true)
    );
    assert_eq!(
        CastValue::cast(&json!(0), CastType::Boolean).unwrap(),
        json!(false)
    );
    assert_eq!(
        CastValue::cast(&json!("1"), CastType::Boolean).unwrap(),
        json!(true)
    );
    assert_eq!(
        CastValue::cast(&json!("0"), CastType::Boolean).unwrap(),
        json!(false)
    );
}

#[test]
fn test_cast_value_to_array_from_array() {
    let result = CastValue::cast(&json!([1, 2, 3]), CastType::Array);
    assert_eq!(result.unwrap(), json!([1, 2, 3]));
}

#[test]
fn test_cast_value_json_passthrough() {
    let value = json!({"key": "value"});
    let result = CastValue::cast(&value, CastType::Json);
    assert_eq!(result.unwrap(), value);
}

#[test]
fn test_cast_value_decimal_passthrough() {
    let result = CastValue::cast(&json!(3.14159), CastType::Decimal);
    assert_eq!(result.unwrap(), json!(3.14159));
}

#[test]
fn test_cast_value_parse_comma_separated() {
    let result = CastValue::parse_comma_separated("a,b,c");
    assert_eq!(result, vec!["a", "b", "c"]);
}

#[test]
fn test_cast_value_format_comma_separated() {
    let result = CastValue::format_comma_separated(&["a", "b", "c"]);
    assert_eq!(result, "a,b,c");
}
