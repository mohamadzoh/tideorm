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
fn test_base64_url_various_lengths() {
    for len in 0..=32u8 {
        let data: Vec<u8> = (0..len).collect();
        let encoded = base64_url_encode(&data);
        assert_eq!(base64_url_decode(&encoded), Some(data), "length {len}");
    }
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

    let token = TokenConfig::get_encoder()("7", "User").unwrap();
    assert_eq!(token, "two-7");
    assert_eq!(
        TokenConfig::get_decoder()(&token, "User").unwrap(),
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
                TokenConfig::get_encoder()("7", "User"),
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

/// Tokens sealed by earlier releases, with argon2 0.5 and chacha20poly1305
/// 0.10, still open, and only under the key they were sealed with: a
/// dependency upgrade that changed the key derivation or the cipher would
/// invalidate every token already issued.
#[test]
fn tokens_issued_by_earlier_releases_still_open() {
    let by_name = "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHVHWDkLrobLrgKs0g40xp6eSe";
    let by_table = "v2.CQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJobdpcfwh5XWPTzj88HZ2np_9";

    TokenConfig::reset();
    TokenConfig::set_encryption_key("known-answer-key");
    assert_eq!(
        default_decode(by_name, "User").unwrap().as_deref(),
        Some("42")
    );
    assert_eq!(
        decode_for_table(by_table, (None, "users"), "User")
            .unwrap()
            .as_deref(),
        Some("42")
    );

    TokenConfig::set_encryption_key("another-key");
    assert_eq!(default_decode(by_name, "User").unwrap(), None);
    TokenConfig::reset();
}

/// Only the canonical unpadded base64url spelling of a token opens. Appending
/// a character used to decode to the same bytes and authenticate, so a token
/// was not a stable identity for cache keys or revocation lists.
#[test]
fn test_token_has_exactly_one_valid_spelling() {
    // A 4k+1 length leaves six bits that encode no byte, and "QR" is "QQ"
    // (the byte 0x41) with a leftover bit set.
    assert_eq!(base64_url_decode("A"), None);
    assert_eq!(base64_url_decode("AAAAA"), None);
    assert_eq!(base64_url_decode("QQ"), Some(vec![0x41]));
    assert_eq!(base64_url_decode("QR"), None);

    init_test_key();
    let token = default_encode("42", "User").unwrap();
    assert_eq!(
        default_decode(&token, "User").unwrap().as_deref(),
        Some("42")
    );
    for suffix in ['A', 'B', '-', '_'] {
        let respelled = format!("{token}{suffix}");
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

#[test]
fn sealed_payloads_open_only_under_the_same_key_and_aad() {
    let key = [7_u8; 32];
    let sealed = super::seal(&key, b"secret", b"aad").expect("sealing should succeed");

    assert_eq!(
        super::open(&key, &sealed, b"aad").as_deref(),
        Some(&b"secret"[..])
    );
    assert_eq!(super::open(&[8_u8; 32], &sealed, b"aad"), None);
    assert_eq!(super::open(&key, &sealed, b"other"), None);
    assert_eq!(super::open(&key, &sealed[..24], b"aad"), None);
}

/// An empty key, what an unset environment variable read with
/// `unwrap_or_default()` gives, configures no key: encrypting then fails
/// rather than sealing everything under a key anyone can derive.
#[test]
fn an_empty_encryption_key_leaves_encryption_unconfigured() {
    TokenConfig::reset();
    TokenConfig::set_encryption_key("  ");
    assert!(!TokenConfig::has_encryption_key());
    assert!(TokenConfig::get_encryption_key().is_err());

    TokenConfig::set_encryption_key("a-real-key");
    assert!(TokenConfig::has_encryption_key());
    TokenConfig::reset();
}

/// Without a key, encoding and decoding with the default codec is a
/// configuration error, never `Ok(None)`: a misconfigured process must not
/// pass for one handed an invalid token.
#[test]
fn a_missing_key_is_a_configuration_error() {
    fn assert_missing_key<T: std::fmt::Debug>(result: Result<T>) {
        match result.unwrap_err() {
            Error::Tokenization { message } => {
                assert!(
                    message.contains("No encryption key configured"),
                    "{message}"
                );
            }
            other => panic!("expected a tokenization error, got {other:?}"),
        }
    }

    TokenConfig::reset();
    assert_missing_key(default_encode("42", "TokenizableProbe"));
    assert_missing_key(TokenizableProbe { id: 7 }.to_token());
    assert_missing_key(default_decode("not-a-real-token", "TokenizableProbe"));
    assert_missing_key(TokenizableProbe::decode_token("not-a-real-token"));
}

/// A model's codec hooks run before the token state is locked, so a hook may
/// configure the global codecs without deadlocking.
#[test]
fn custom_codec_selectors_can_update_global_configuration() {
    struct ConfiguringModel;
    fn encode(payload: &str, model: &str) -> Result<String> {
        Ok(format!("{model}:{payload}"))
    }
    fn decode(token: &str, model: &str) -> Result<Option<String>> {
        Ok(token.strip_prefix(&format!("{model}:")).map(str::to_owned))
    }
    #[async_trait::async_trait]
    impl Tokenizable for ConfiguringModel {
        type TokenPrimaryKey = i64;
        fn token_model_name() -> &'static str {
            "ConfiguringModel"
        }
        fn token_primary_key(&self) -> i64 {
            42
        }
        fn token_encoder() -> Option<TokenEncoder> {
            TokenConfig::set_encoder(encode);
            None
        }
        fn token_decoder() -> Option<TokenDecoder> {
            TokenConfig::set_decoder(decode);
            None
        }
        async fn from_token(token: &str) -> Result<Self> {
            Self::decode_token(token)?;
            Ok(Self)
        }
    }
    TokenConfig::reset();
    let token = ConfiguringModel.to_token().unwrap();
    assert_eq!(token, "ConfiguringModel:42");
    assert_eq!(ConfiguringModel::decode_token(&token).unwrap(), 42);
    TokenConfig::reset();
}
