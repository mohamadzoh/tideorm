use super::*;
use std::sync::mpsc;

struct TokenizableProbe {
    id: i64,
}

#[async_trait::async_trait]
impl Tokenizable for TokenizableProbe {
    type TokenPrimaryKey = i64;

    fn token_model_name() -> &'static str {
        "TokenizableProbe"
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

fn init_test_key() {
    TokenConfig::set_encryption_key("test-encryption-key-for-unit-tests-32");
}

#[test]
fn test_base64_url_encode_decode() {
    let data = b"Hello, World!";
    let encoded = base64_url_encode(data);
    let decoded = base64_url_decode(&encoded).unwrap();
    assert_eq!(data.to_vec(), decoded);
}

#[test]
fn test_base64_url_various_lengths() {
    for len in 1..=32 {
        let data: Vec<u8> = (0..len).map(|i| i as u8).collect();
        let encoded = base64_url_encode(&data);
        let decoded = base64_url_decode(&encoded).unwrap();
        assert_eq!(data, decoded, "Failed for length {}", len);
    }
}

#[test]
fn test_default_encode_decode() {
    init_test_key();

    let record_id = "12345";
    let model_name = "User";

    let token = default_encode(record_id, model_name).unwrap();
    let decoded = default_decode(&token, model_name).unwrap();

    assert_eq!(decoded, Some(record_id.to_string()));
}

#[test]
fn test_encode_decode_negative_id() {
    init_test_key();

    let record_id = "-99999";
    let model_name = "NegativeModel";

    let token = default_encode(record_id, model_name).unwrap();
    let decoded = default_decode(&token, model_name).unwrap();

    assert_eq!(decoded, Some(record_id.to_string()));
}

#[test]
fn test_encode_decode_zero() {
    init_test_key();

    let record_id = "0";
    let model_name = "ZeroModel";

    let token = default_encode(record_id, model_name).unwrap();
    let decoded = default_decode(&token, model_name).unwrap();

    assert_eq!(decoded, Some(record_id.to_string()));
}

#[test]
fn test_encode_decode_max_i64() {
    init_test_key();

    let record_id = "9223372036854775807";
    let model_name = "MaxModel";

    let token = default_encode(record_id, model_name).unwrap();
    let decoded = default_decode(&token, model_name).unwrap();

    assert_eq!(decoded, Some(record_id.to_string()));
}

#[test]
fn test_wrong_model_fails() {
    init_test_key();

    let record_id = "42";
    let token = default_encode(record_id, "User").unwrap();

    let decoded = default_decode(&token, "Product").unwrap();
    assert_eq!(decoded, None);
}

#[test]
fn test_tampered_token_fails() {
    init_test_key();

    let record_id = "42";
    let token = default_encode(record_id, "User").unwrap();

    let mut chars: Vec<char> = token.chars().collect();
    if let Some(c) = chars.get_mut(10) {
        *c = if *c == 'A' { 'B' } else { 'A' };
    }
    let tampered: String = chars.into_iter().collect();

    let decoded = default_decode(&tampered, "User").unwrap();
    assert_eq!(decoded, None);
}

#[test]
fn test_invalid_base64_fails() {
    init_test_key();

    let decoded = default_decode("not-valid-base64!!!", "User").unwrap();
    assert_eq!(decoded, None);
}

#[test]
fn test_too_short_token_fails() {
    init_test_key();

    let decoded = default_decode("abc", "User").unwrap();
    assert_eq!(decoded, None);
}

#[test]
fn test_token_is_url_safe() {
    init_test_key();

    let record_id = "999999999";
    let token = default_encode(record_id, "User").unwrap();

    assert!(
        token
            .chars()
            .all(|c| { c.is_ascii_alphanumeric() || c == '-' || c == '_' })
    );
}

#[test]
fn test_different_ids_different_tokens() {
    init_test_key();

    let token1 = default_encode("1", "User").unwrap();
    let token2 = default_encode("2", "User").unwrap();

    assert_ne!(token1, token2);
}

#[test]
fn test_same_id_generates_different_tokens() {
    init_test_key();

    let token1 = default_encode("42", "User").unwrap();
    let token2 = default_encode("42", "User").unwrap();

    assert_ne!(token1, token2);
    assert_eq!(
        default_decode(&token1, "User").unwrap(),
        Some("42".to_string())
    );
    assert_eq!(
        default_decode(&token2, "User").unwrap(),
        Some("42".to_string())
    );
}

#[test]
fn test_token_config_encode_decode() {
    init_test_key();

    let token = TokenConfig::encode("123", "TestModel").unwrap();
    let decoded = TokenConfig::decode(&token, "TestModel").unwrap();

    assert_eq!(decoded, Some("123".to_string()));
}

#[test]
fn test_token_config_setters_overwrite_previous_values() {
    TokenConfig::reset();

    TokenConfig::set_encryption_key("first-key");
    TokenConfig::set_encryption_key("second-key");
    assert_eq!(TokenConfig::get_encryption_key().unwrap(), "second-key");

    fn encoder_one(record_id: &str, _model_name: &str) -> Result<String> {
        Ok(format!("one-{record_id}"))
    }

    fn encoder_two(record_id: &str, _model_name: &str) -> Result<String> {
        Ok(format!("two-{record_id}"))
    }

    fn decoder_one(token: &str, _model_name: &str) -> Result<Option<String>> {
        Ok(token.strip_prefix("one-").map(ToOwned::to_owned))
    }

    fn decoder_two(token: &str, _model_name: &str) -> Result<Option<String>> {
        Ok(token.strip_prefix("two-").map(ToOwned::to_owned))
    }

    TokenConfig::set_encoder(encoder_one);
    TokenConfig::set_encoder(encoder_two);
    TokenConfig::set_decoder(decoder_one);
    TokenConfig::set_decoder(decoder_two);

    let token = TokenConfig::encode("7", "User").unwrap();
    assert_eq!(token, "two-7");
    assert_eq!(
        TokenConfig::decode(&token, "User").unwrap(),
        Some("7".to_string())
    );
}

#[test]
fn test_tokenizable_decode_token_uses_global_decoder_override() {
    TokenConfig::reset();

    fn decoder(token: &str, _model_name: &str) -> Result<Option<String>> {
        Ok(token.strip_prefix("global-").map(ToOwned::to_owned))
    }

    TokenConfig::set_decoder(decoder);

    assert_eq!(TokenizableProbe::decode_token("global-42").unwrap(), 42);
}

#[test]
fn test_token_config_reset_clears_global_state() {
    TokenConfig::set_encryption_key("transient-key");
    assert!(TokenConfig::has_encryption_key());

    TokenConfig::reset();

    assert!(!TokenConfig::has_encryption_key());
    assert!(TokenConfig::get_encryption_key().is_err());
}

#[test]
fn test_token_config_reset_clears_state_set_on_another_thread() {
    TokenConfig::reset();

    fn custom_encoder(record_id: &str, _model_name: &str) -> Result<String> {
        Ok(format!("custom-{record_id}"))
    }

    let (ready_tx, ready_rx) = mpsc::channel();
    let (check_tx, check_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();

    let worker = std::thread::spawn(move || {
        TokenConfig::set_encryption_key("thread-key");
        TokenConfig::set_encoder(custom_encoder);
        ready_tx
            .send(())
            .expect("worker should report that token state is configured");

        check_rx.recv().expect("worker should receive reset signal");

        result_tx
            .send((
                TokenConfig::has_encryption_key(),
                TokenConfig::get_encryption_key(),
                TokenConfig::encode("7", "User"),
            ))
            .expect("worker should report token state after reset");
    });

    ready_rx
        .recv()
        .expect("main thread should observe configured token state");

    TokenConfig::reset();

    check_tx
        .send(())
        .expect("main thread should signal worker to validate reset state");

    let (has_key, key_result, encode_result) = result_rx
        .recv()
        .expect("main thread should receive worker validation results");

    worker
        .join()
        .expect("token config worker thread should finish successfully");

    assert!(!has_key);
    assert!(key_result.is_err());
    assert!(encode_result.is_err());
}

#[test]
fn test_derive_encryption_key_is_deterministic_for_same_secret() {
    let first = derive_encryption_key("same-secret");
    let second = derive_encryption_key("same-secret");

    assert_eq!(first, second);
}

#[test]
fn test_derive_encryption_key_changes_with_secret() {
    let first = derive_encryption_key("first-secret");
    let second = derive_encryption_key("second-secret");

    assert_ne!(first, second);
}

#[test]
fn test_base64_url_decode_rejects_orphan_trailing_character() {
    // Six leftover bits encode no byte, so the encoder never emits a 4k+1 length.
    assert!(base64_url_decode("A").is_none());
    assert!(base64_url_decode("AAAAA").is_none());
}

#[test]
fn test_base64_url_decode_rejects_non_canonical_trailing_bits() {
    // "QQ" is the canonical spelling of the single byte 0x41. "QR" carries the
    // same leading eight bits with a leftover bit set, so it is a re-spelling.
    assert_eq!(base64_url_decode("QQ"), Some(vec![0x41]));
    assert!(base64_url_decode("QR").is_none());
}

#[test]
fn test_token_has_exactly_one_valid_spelling() {
    init_test_key();

    let token = default_encode("42", "User").unwrap();
    assert_eq!(
        default_decode(&token, "User").unwrap(),
        Some("42".to_string())
    );

    // Appending a character used to decode to the same bytes and authenticate,
    // so a token was not a stable identity for cache keys or revocation lists.
    for suffix in ['A', 'B', '-', '_'] {
        let respelled = format!("{token}{suffix}");
        assert_ne!(respelled, token);
        assert_eq!(
            default_decode(&respelled, "User").unwrap(),
            None,
            "non-canonical token spelling '{respelled}' must not authenticate"
        );
    }
}

/// `#[tideorm(encrypted)]` derives a key per `(table, column)`.
///
/// This is the property that makes ciphertext non-portable between columns, and it is
/// why the process-wide `Encrypted<T>` wrapper was removed in 0.10: the wrapper ran at
/// the serde layer, which cannot see the table or column, so every wrapper column in a
/// process shared one key and a ciphertext could be moved from a low-privilege column
/// into a high-privilege one.
#[cfg(feature = "encrypted-fields")]
#[test]
fn test_encrypted_field_key_is_derived_per_table_and_column() {
    TokenConfig::reset();
    TokenConfig::set_encryption_key("field-domain-separation-key-32chars");

    let token_key = TokenConfig::get_derived_encryption_key().unwrap();
    let users_email = TokenConfig::get_derived_encryption_key_for_field("users", "email").unwrap();
    let users_ssn = TokenConfig::get_derived_encryption_key_for_field("users", "ssn").unwrap();
    let orders_email =
        TokenConfig::get_derived_encryption_key_for_field("orders", "email").unwrap();

    // Distinct from the token key, and from each other along both axes.
    assert_ne!(token_key, users_email);
    assert_ne!(users_email, users_ssn, "column must affect the derivation");
    assert_ne!(
        users_email, orders_email,
        "table must affect the derivation"
    );

    // Stable within one configuration, or previously written data stops decrypting.
    assert_eq!(
        users_email,
        TokenConfig::get_derived_encryption_key_for_field("users", "email").unwrap()
    );

    TokenConfig::reset();
}
