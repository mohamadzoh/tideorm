use tideorm::error::Result;
use tideorm::tokenization::{TokenDecoder, TokenEncoder, Tokenizable};

fn simple_encoder(id: &str, model: &str) -> tideorm::error::Result<String> {
    Ok(format!("{}-{}", model.to_lowercase(), id))
}

fn simple_decoder(token: &str, model: &str) -> Result<Option<String>> {
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
        Some(simple_encoder)
    }

    fn token_decoder() -> Option<TokenDecoder> {
        Some(simple_decoder)
    }

    async fn from_token(token: &str) -> Result<Self> {
        let id = Self::decode_token(token)?;
        Ok(CustomModel { id })
    }
}

#[test]
fn test_custom_encoder() {
    let model = CustomModel { id: 42 };
    let token = model.tokenize().unwrap();

    assert_eq!(token, "custom-42");
}

#[test]
fn test_custom_decoder() {
    let token = "custom-123";
    let decoded = CustomModel::decode_token(token).unwrap();

    assert_eq!(decoded, 123);
}

#[test]
fn test_custom_roundtrip() {
    let model = CustomModel { id: 999 };
    let token = model.tokenize().unwrap();
    let decoded = CustomModel::decode_token(&token).unwrap();

    assert_eq!(decoded, 999);
}
