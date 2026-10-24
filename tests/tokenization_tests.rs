//! The public tokenization API without a database: the default codec, a
//! model's codec overrides, and `Tokenizable` over hand-written impls, whose
//! tokens are bound to the struct name. A derived model's tokens are bound to
//! its table; `sqlite_regressions/tokenization.rs` covers those.

use std::sync::Once;

use tideorm::error::{Error, Result};
use tideorm::tokenization::{
    TokenConfig, TokenDecoder, TokenEncoder, Tokenizable, default_decode, default_encode,
};

/// Setting the key derives it with Argon2, so it happens once per process.
fn init_test_env() {
    static INIT: Once = Once::new();
    INIT.call_once(|| TokenConfig::set_encryption_key("test-encryption-key-for-tests-32!"));
}

#[test]
fn the_default_codec_round_trips_any_payload_and_model_name() {
    init_test_env();
    let long_name = "A".repeat(1000);
    let cases = [
        ("0", "User"),
        ("-9223372036854775808", "User"),
        ("\"0b9c6a52-uuid\"", "User"),
        ("42", ""),
        ("42", long_name.as_str()),
        ("42", "用户模型🔐"),
        ("42", "My::Nested::Model<T>"),
    ];

    for (payload, model) in cases {
        let token = default_encode(payload, model).unwrap();
        assert!(
            token
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "token {token:?} for {payload} of {model} is not URL-safe"
        );
        assert_eq!(
            default_decode(&token, model).unwrap().as_deref(),
            Some(payload),
            "{payload} of {model}"
        );
    }
}

/// Each token takes a fresh nonce: the same key gives a new token every time,
/// and each of them opens.
#[test]
fn every_encoding_gives_a_new_token() {
    init_test_env();
    let tokens: Vec<String> = (0..3)
        .map(|_| default_encode("42", "User").unwrap())
        .collect();

    assert_ne!(tokens[0], tokens[1]);
    assert_ne!(tokens[1], tokens[2]);
    assert_ne!(tokens[0], tokens[2]);
    for token in &tokens {
        assert_eq!(
            default_decode(token, "User").unwrap().as_deref(),
            Some("42")
        );
    }
}

/// A token of another model, a tampered token and a malformed one decode to
/// `Ok(None)`: an invalid token is not an error, as a missing key is.
#[test]
fn foreign_tampered_and_malformed_tokens_decode_to_none() {
    init_test_env();
    let token = default_encode("42", "User").unwrap();

    for model in ["Product", "user"] {
        assert_eq!(default_decode(&token, model).unwrap(), None, "for {model}");
    }
    for position in 0..token.len() {
        let mut tampered = token.clone().into_bytes();
        tampered[position] = if tampered[position] == b'A' {
            b'B'
        } else {
            b'A'
        };
        let tampered = String::from_utf8(tampered).unwrap();
        assert_eq!(
            default_decode(&tampered, "User").unwrap(),
            None,
            "tampered at {position}"
        );
    }
    for malformed in [
        "",
        "a",
        "abc",
        "!!!invalid!!!",
        "too-short",
        "contains spaces here",
        "has\nnewline",
    ] {
        assert_eq!(
            default_decode(malformed, "User").unwrap(),
            None,
            "{malformed:?}"
        );
    }
}

struct TestUser {
    id: i64,
}

#[async_trait::async_trait]
impl Tokenizable for TestUser {
    type TokenPrimaryKey = i64;

    fn token_model_name() -> &'static str {
        "TestUser"
    }

    fn token_primary_key(&self) -> i64 {
        self.id
    }

    async fn from_token(token: &str) -> Result<Self> {
        Ok(Self {
            id: Self::decode_token(token)?,
        })
    }
}

/// `Tokenizable` puts the serialized key through the default codec, and
/// reports a token that does not open for the model as `InvalidToken`.
#[test]
fn tokenizable_round_trips_its_key_and_rejects_other_tokens() {
    init_test_env();
    let by_id = TestUser::tokenize_id(42).unwrap();
    assert_eq!(TestUser::decode_token(&by_id).unwrap(), 42);
    let by_record = TestUser { id: 77 }.to_token().unwrap();
    assert_eq!(TestUser::decode_token(&by_record).unwrap(), 77);

    let foreign = default_encode("42", "TestProduct").unwrap();
    for bad in ["invalid-token", foreign.as_str()] {
        assert!(
            matches!(TestUser::decode_token(bad), Err(Error::InvalidToken { .. })),
            "{bad}"
        );
    }
}

fn prefix_encoder(id: &str, model: &str) -> Result<String> {
    Ok(format!("{}-{}", model.to_lowercase(), id))
}

fn prefix_decoder(token: &str, model: &str) -> Result<Option<String>> {
    let prefix = format!("{}-", model.to_lowercase());
    Ok(token.strip_prefix(&prefix).map(ToOwned::to_owned))
}

struct CustomModel {
    id: i64,
}

#[async_trait::async_trait]
impl Tokenizable for CustomModel {
    type TokenPrimaryKey = i64;

    fn token_model_name() -> &'static str {
        "custom"
    }

    fn token_primary_key(&self) -> i64 {
        self.id
    }

    fn token_encoder() -> Option<TokenEncoder> {
        Some(prefix_encoder)
    }

    fn token_decoder() -> Option<TokenDecoder> {
        Some(prefix_decoder)
    }

    async fn from_token(token: &str) -> Result<Self> {
        Ok(Self {
            id: Self::decode_token(token)?,
        })
    }
}

/// A model's own encoder and decoder replace the default codec for it.
#[test]
fn a_models_codec_overrides_replace_the_default() {
    assert_eq!(CustomModel { id: 42 }.to_token().unwrap(), "custom-42");
    assert_eq!(CustomModel::decode_token("custom-123").unwrap(), 123);
}
