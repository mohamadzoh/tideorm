use super::*;

use tideorm::error::Result;
use tideorm::tokenization::Tokenizable;

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

    async fn from_token(_token: &str) -> Result<Self> {
        let id = Self::decode_token(_token)?;
        Ok(TestUser { id })
    }
}

struct TestProduct {
    id: i64,
}

#[async_trait::async_trait]
impl Tokenizable for TestProduct {
    type TokenPrimaryKey = i64;

    fn token_model_name() -> &'static str {
        "TestProduct"
    }

    fn token_primary_key(&self) -> i64 {
        self.id
    }

    async fn from_token(token: &str) -> Result<Self> {
        let id = Self::decode_token(token)?;
        Ok(TestProduct { id })
    }
}

#[test]
fn test_tokenizable_tokenize_alias() {
    init_test_env();

    let user = TestUser { id: 42 };

    let token1 = user.to_token().unwrap();
    let token2 = user.tokenize().unwrap();

    assert_eq!(TestUser::decode_token(&token1).unwrap(), 42);
    assert_eq!(TestUser::decode_token(&token2).unwrap(), 42);
    assert_ne!(token1, token2);
}

#[test]
fn test_tokenizable_tokenize_id() {
    init_test_env();

    let token = TestUser::tokenize_id(42).unwrap();
    let decoded = TestUser::decode_token(&token).unwrap();

    assert_eq!(decoded, 42);
}

#[test]
fn test_tokenizable_detokenize() {
    init_test_env();

    let user = TestUser { id: 99 };

    let token = user.tokenize().unwrap();
    let decoded = TestUser::detokenize(&token).unwrap();

    assert_eq!(decoded, 99);
}

#[test]
fn test_tokenizable_regenerate_token() {
    init_test_env();

    let user = TestUser { id: 50 };

    let token1 = user.to_token().unwrap();
    let token2 = user.regenerate_token().unwrap();

    assert_ne!(token1, token2);
    assert_eq!(TestUser::decode_token(&token1).unwrap(), 50);
    assert_eq!(TestUser::decode_token(&token2).unwrap(), 50);
}

#[test]
fn test_tokenizable_cross_model_rejection() {
    init_test_env();

    let user = TestUser { id: 42 };
    let user_token = user.tokenize().unwrap();

    let product = TestProduct { id: 42 };
    let product_token = product.tokenize().unwrap();

    assert_ne!(user_token, product_token);
    assert!(TestProduct::decode_token(&user_token).is_err());
    assert!(TestUser::decode_token(&product_token).is_err());
}

#[test]
fn test_tokenizable_invalid_token_error() {
    init_test_env();

    let result = TestUser::decode_token("invalid-token");
    assert!(result.is_err());

    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("token"),
        "Error message should mention 'token'"
    );
}

#[tokio::test]
async fn test_tokenizable_from_token() {
    init_test_env();

    let original = TestUser { id: 77 };

    let token = original.tokenize().unwrap();
    let restored = TestUser::from_token(&token).await.unwrap();

    assert_eq!(restored.id, original.id);
}
