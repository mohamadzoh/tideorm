use super::*;
use std::sync::mpsc;

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

    let record_id = 12345i64;
    let model_name = "User";

    let token = default_encode(record_id, model_name).unwrap();
    let decoded = default_decode(&token, model_name).unwrap();

    assert_eq!(decoded, Some(record_id));
}

#[test]
fn test_encode_decode_negative_id() {
    init_test_key();

    let record_id = -99999i64;
    let model_name = "NegativeModel";

    let token = default_encode(record_id, model_name).unwrap();
    let decoded = default_decode(&token, model_name).unwrap();

    assert_eq!(decoded, Some(record_id));
}

#[test]
fn test_encode_decode_zero() {
    init_test_key();

    let record_id = 0i64;
    let model_name = "ZeroModel";

    let token = default_encode(record_id, model_name).unwrap();
    let decoded = default_decode(&token, model_name).unwrap();

    assert_eq!(decoded, Some(record_id));
}

#[test]
fn test_encode_decode_max_i64() {
    init_test_key();

    let record_id = i64::MAX;
    let model_name = "MaxModel";

    let token = default_encode(record_id, model_name).unwrap();
    let decoded = default_decode(&token, model_name).unwrap();

    assert_eq!(decoded, Some(record_id));
}

#[test]
fn test_wrong_model_fails() {
    init_test_key();

    let record_id = 42i64;
    let token = default_encode(record_id, "User").unwrap();

    let decoded = default_decode(&token, "Product").unwrap();
    assert_eq!(decoded, None);
}

#[test]
fn test_tampered_token_fails() {
    init_test_key();

    let record_id = 42i64;
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

    let record_id = 999999999i64;
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

    let token1 = default_encode(1, "User").unwrap();
    let token2 = default_encode(2, "User").unwrap();

    assert_ne!(token1, token2);
}

#[test]
fn test_same_id_generates_different_tokens() {
    init_test_key();

    let token1 = default_encode(42, "User").unwrap();
    let token2 = default_encode(42, "User").unwrap();

    assert_ne!(token1, token2);
    assert_eq!(default_decode(&token1, "User").unwrap(), Some(42));
    assert_eq!(default_decode(&token2, "User").unwrap(), Some(42));
}

#[test]
fn test_token_config_encode_decode() {
    init_test_key();

    let token = TokenConfig::encode(123, "TestModel").unwrap();
    let decoded = TokenConfig::decode(&token, "TestModel").unwrap();

    assert_eq!(decoded, Some(123));
}

#[test]
fn test_token_config_setters_overwrite_previous_values() {
    TokenConfig::reset();

    TokenConfig::set_encryption_key("first-key");
    TokenConfig::set_encryption_key("second-key");
    assert_eq!(TokenConfig::get_encryption_key().unwrap(), "second-key");

    fn encoder_one(record_id: i64, _model_name: &str) -> Result<String> {
        Ok(format!("one-{record_id}"))
    }

    fn encoder_two(record_id: i64, _model_name: &str) -> Result<String> {
        Ok(format!("two-{record_id}"))
    }

    fn decoder_one(token: &str, _model_name: &str) -> Result<Option<i64>> {
        Ok(token
            .strip_prefix("one-")
            .and_then(|value| value.parse().ok()))
    }

    fn decoder_two(token: &str, _model_name: &str) -> Result<Option<i64>> {
        Ok(token
            .strip_prefix("two-")
            .and_then(|value| value.parse().ok()))
    }

    TokenConfig::set_encoder(encoder_one);
    TokenConfig::set_encoder(encoder_two);
    TokenConfig::set_decoder(decoder_one);
    TokenConfig::set_decoder(decoder_two);

    let token = TokenConfig::encode(7, "User").unwrap();
    assert_eq!(token, "two-7");
    assert_eq!(TokenConfig::decode(&token, "User").unwrap(), Some(7));
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

    fn custom_encoder(record_id: i64, _model_name: &str) -> Result<String> {
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

        check_rx
            .recv()
            .expect("worker should receive reset signal");

        result_tx
            .send((
                TokenConfig::has_encryption_key(),
                TokenConfig::get_encryption_key(),
                TokenConfig::encode(7, "User"),
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
