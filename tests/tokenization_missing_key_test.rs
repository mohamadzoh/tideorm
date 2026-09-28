use tideorm::tokenization::{Tokenizable, default_decode, default_encode};
use tideorm::{Error, Result};

struct MissingKeyModel {
    id: i64,
}

#[async_trait::async_trait]
impl Tokenizable for MissingKeyModel {
    type TokenPrimaryKey = i64;

    fn token_model_name() -> &'static str {
        "MissingKeyModel"
    }

    fn token_primary_key(&self) -> i64 {
        self.id
    }

    async fn from_token(token: &str) -> Result<Self> {
        let id = Self::decode_token(token)?;
        Ok(Self { id })
    }
}

/// A missing key is a configuration error, never an invalid token.
fn assert_missing_key<T: std::fmt::Debug>(result: Result<T>) {
    match result.unwrap_err() {
        Error::Tokenization { message } => {
            assert!(
                message.contains("No encryption key configured"),
                "{message}"
            );
        }
        other => panic!("expected tokenization error, got {other:?}"),
    }
}

#[test]
fn encoding_fails_without_configured_key() {
    assert_missing_key(default_encode("42", "MissingKeyModel"));
    assert_missing_key(MissingKeyModel { id: 7 }.to_token());
}

#[test]
fn decoding_fails_loudly_without_configured_key() {
    assert_missing_key(default_decode("not-a-real-token", "MissingKeyModel"));
    assert_missing_key(MissingKeyModel::decode_token("not-a-real-token"));
}
