use super::*;

use tideorm::tokenization::{TokenConfig, default_decode, default_encode};

#[test]
fn test_encode_decode_roundtrip() {
    init_test_env();

    let id = "12345";
    let model = "User";

    let token = default_encode(id, model).unwrap();
    let decoded = default_decode(&token, model).unwrap();

    assert_eq!(decoded, Some(id.to_string()));
}

#[test]
fn test_encode_decode_various_ids() {
    init_test_env();

    let test_cases = [
        ("0", "Zero"),
        ("1", "One"),
        ("100", "Hundred"),
        ("999999", "Large"),
        ("9223372036854775807", "Max"),
        ("-1", "NegativeOne"),
        ("-999999", "NegativeLarge"),
        ("-9223372036854775808", "Min"),
    ];

    for (id, model) in test_cases {
        let token = default_encode(id, model).unwrap();
        let decoded = default_decode(&token, model).unwrap();
        assert_eq!(
            decoded,
            Some(id.to_string()),
            "Failed for id={}, model={}",
            id,
            model
        );
    }
}

#[test]
fn test_token_is_url_safe() {
    init_test_env();

    for id in ["1", "42", "999", "123456789", "9223372036854775807"] {
        let token = default_encode(id, "User").unwrap();

        assert!(
            token
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "Token '{}' contains non-URL-safe characters",
            token
        );

        assert!(!token.contains('='), "Token should not contain padding '='");
        assert!(!token.contains('+'), "Token should not contain '+'");
        assert!(!token.contains('/'), "Token should not contain '/'");
    }
}

#[test]
fn test_model_specific_tokens() {
    init_test_env();

    let id = "42";

    let user_token = default_encode(id, "User").unwrap();
    let product_token = default_encode(id, "Product").unwrap();
    let order_token = default_encode(id, "Order").unwrap();

    assert_ne!(user_token, product_token);
    assert_ne!(user_token, order_token);
    assert_ne!(product_token, order_token);

    assert_eq!(
        default_decode(&user_token, "User").unwrap(),
        Some(id.to_string())
    );
    assert_eq!(
        default_decode(&product_token, "Product").unwrap(),
        Some(id.to_string())
    );
    assert_eq!(
        default_decode(&order_token, "Order").unwrap(),
        Some(id.to_string())
    );
}

#[test]
fn test_cross_model_decode_fails() {
    init_test_env();

    let id = "42";
    let user_token = default_encode(id, "User").unwrap();

    assert_eq!(default_decode(&user_token, "Product").unwrap(), None);
    assert_eq!(default_decode(&user_token, "Order").unwrap(), None);
    assert_eq!(default_decode(&user_token, "SomeOtherModel").unwrap(), None);
}

#[test]
fn test_tampered_token_fails() {
    init_test_env();

    let token = default_encode("42", "User").unwrap();

    for pos in [0, 5, 10, 15, 20, 30] {
        if pos < token.len() {
            let mut chars: Vec<char> = token.chars().collect();
            chars[pos] = if chars[pos] == 'A' { 'B' } else { 'A' };
            let tampered: String = chars.into_iter().collect();

            assert_eq!(
                default_decode(&tampered, "User").unwrap(),
                None,
                "Tampered token at position {} should fail to decode",
                pos
            );
        }
    }
}

#[test]
fn test_invalid_tokens() {
    init_test_env();

    let invalid_tokens = [
        "",
        "a",
        "abc",
        "!!!invalid!!!",
        "too-short",
        "                    ",
        "contains spaces here",
        "has\nnewline",
        "has\ttab",
    ];

    for invalid in invalid_tokens {
        assert_eq!(
            default_decode(invalid, "User").unwrap(),
            None,
            "Invalid token '{}' should fail to decode",
            invalid
        );
    }
}

#[test]
fn test_token_randomization() {
    init_test_env();

    let id = "42";
    let model = "User";

    let token1 = default_encode(id, model).unwrap();
    let token2 = default_encode(id, model).unwrap();
    let token3 = default_encode(id, model).unwrap();

    assert_ne!(token1, token2);
    assert_ne!(token2, token3);
    assert_ne!(token1, token3);

    assert_eq!(
        default_decode(&token1, model).unwrap(),
        Some(id.to_string())
    );
    assert_eq!(
        default_decode(&token2, model).unwrap(),
        Some(id.to_string())
    );
    assert_eq!(
        default_decode(&token3, model).unwrap(),
        Some(id.to_string())
    );
}

#[test]
fn test_different_ids_different_tokens() {
    init_test_env();

    let model = "User";
    let token1 = default_encode("1", model).unwrap();
    let token2 = default_encode("2", model).unwrap();
    let token3 = default_encode("3", model).unwrap();

    assert_ne!(token1, token2);
    assert_ne!(token2, token3);
    assert_ne!(token1, token3);
}

#[test]
fn test_token_length() {
    init_test_env();

    for id in [
        "0",
        "1",
        "100",
        "9223372036854775807",
        "-9223372036854775808",
    ] {
        let token = default_encode(id, "User").unwrap();
        assert!(
            token.len() >= 55,
            "Token should have a stable encrypted payload length floor, got {}",
            token.len()
        );
    }
}

#[test]
fn test_token_config_encode_decode() {
    init_test_env();

    let id = "123";
    let model = "TestModel";

    let token = TokenConfig::encode(id, model).unwrap();
    let decoded = TokenConfig::decode(&token, model).unwrap();

    assert_eq!(decoded, Some(id.to_string()));
}
