use tideorm::tokenization::{default_decode, default_encode, TokenConfig, Tokenizable};
use tideorm::{Error, Result};

struct MissingKeyModel {
    id: i64,
}

#[async_trait::async_trait]
impl Tokenizable for MissingKeyModel {
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

#[test]
fn encode_fails_without_configured_key() {
    let err = TokenConfig::encode(42, "MissingKeyModel").unwrap_err();

    match err {
        Error::Tokenization { message } => {
            assert!(message.contains("No encryption key configured"));
        }
        other => panic!("expected tokenization error, got {other:?}"),
    }
}

#[test]
fn default_encode_fails_without_configured_key() {
    let err = default_encode(42, "MissingKeyModel").unwrap_err();

    match err {
        Error::Tokenization { message } => {
            assert!(message.contains("No encryption key configured"));
        }
        other => panic!("expected tokenization error, got {other:?}"),
    }
}

#[test]
fn default_decode_fails_without_configured_key() {
    let err = default_decode("not-a-real-token", "MissingKeyModel").unwrap_err();

    match err {
        Error::Tokenization { message } => {
            assert!(message.contains("No encryption key configured"));
        }
        other => panic!("expected tokenization error, got {other:?}"),
    }
}

#[test]
fn decode_token_fails_loudly_without_configured_key() {
    let err = MissingKeyModel::decode_token("not-a-real-token").unwrap_err();

    match err {
        Error::Tokenization { message } => {
            assert!(message.contains("No encryption key configured"));
        }
        other => panic!("expected tokenization error, got {other:?}"),
    }
}

#[test]
fn tokenize_fails_loudly_without_configured_key() {
    let err = MissingKeyModel { id: 7 }.to_token().unwrap_err();

    match err {
        Error::Tokenization { message } => {
            assert!(message.contains("No encryption key configured"));
        }
        other => panic!("expected tokenization error, got {other:?}"),
    }
}
