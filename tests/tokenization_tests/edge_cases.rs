use super::*;

use tideorm::tokenization::{default_decode, default_encode};

#[test]
fn test_empty_model_name() {
    init_test_env();

    let token = default_encode("42", "").unwrap();
    let decoded = default_decode(&token, "").unwrap();

    assert_eq!(decoded, Some("42".to_string()));
}

#[test]
fn test_long_model_name() {
    init_test_env();

    let long_name = "A".repeat(1000);
    let token = default_encode("42", &long_name).unwrap();
    let decoded = default_decode(&token, &long_name).unwrap();

    assert_eq!(decoded, Some("42".to_string()));
}

#[test]
fn test_unicode_model_name() {
    init_test_env();

    let unicode_name = "用户模型🔐";
    let token = default_encode("42", unicode_name).unwrap();
    let decoded = default_decode(&token, unicode_name).unwrap();

    assert_eq!(decoded, Some("42".to_string()));
}

#[test]
fn test_special_char_model_name() {
    init_test_env();

    for name in [
        "User<T>",
        "My::Nested::Model",
        "Model-With-Dashes",
        "model_with_underscores",
    ] {
        let token = default_encode("42", name).unwrap();
        let decoded = default_decode(&token, name).unwrap();
        assert_eq!(
            decoded,
            Some("42".to_string()),
            "Failed for model name: {}",
            name
        );
    }
}
