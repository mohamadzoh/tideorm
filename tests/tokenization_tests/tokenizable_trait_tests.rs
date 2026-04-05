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
fn test_tokenizable_to_token() {
    init_test_env();

    let user = TestUser { id: 42 };

    let token = user.to_token().unwrap();
    assert!(!token.is_empty());

    assert!(
        token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    );
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
fn test_tokenizable_decode_token() {
    init_test_env();

    let token = TestUser::tokenize_id(123).unwrap();
    let decoded = TestUser::decode_token(&token).unwrap();

    assert_eq!(decoded, 123);
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

#[test]
fn test_multiple_users_unique_tokens() {
    init_test_env();

    let users: Vec<TestUser> = (1..=10).map(|i| TestUser { id: i }).collect();

    let tokens: Vec<String> = users.iter().map(|u| u.tokenize().unwrap()).collect();

    for i in 0..tokens.len() {
        for j in (i + 1)..tokens.len() {
            assert_ne!(
                tokens[i],
                tokens[j],
                "Tokens for users {} and {} should be different",
                i + 1,
                j + 1
            );
        }
    }

    for (user, token) in users.iter().zip(tokens.iter()) {
        let decoded = TestUser::decode_token(token).unwrap();
        assert_eq!(decoded, user.id);
    }
}

struct StringKeyModel {
    id: String,
}

#[async_trait::async_trait]
impl Tokenizable for StringKeyModel {
    type TokenPrimaryKey = String;

    fn token_model_name() -> &'static str {
        "StringKeyModel"
    }

    fn token_primary_key(&self) -> String {
        self.id.clone()
    }

    async fn from_token(token: &str) -> Result<Self> {
        Ok(Self {
            id: Self::decode_token(token)?,
        })
    }
}

struct U64KeyModel {
    id: u64,
}

#[async_trait::async_trait]
impl Tokenizable for U64KeyModel {
    type TokenPrimaryKey = u64;

    fn token_model_name() -> &'static str {
        "U64KeyModel"
    }

    fn token_primary_key(&self) -> u64 {
        self.id
    }

    async fn from_token(token: &str) -> Result<Self> {
        Ok(Self {
            id: Self::decode_token(token)?,
        })
    }
}

#[test]
fn test_tokenizable_string_primary_key_roundtrip() {
    init_test_env();

    let model = StringKeyModel {
        id: "user_abc-123".to_string(),
    };

    let token = model.tokenize().unwrap();
    let decoded = StringKeyModel::decode_token(&token).unwrap();

    assert_eq!(decoded, model.id);
}

#[test]
fn test_tokenizable_u64_primary_key_roundtrip() {
    init_test_env();

    let model = U64KeyModel { id: u64::MAX };

    let token = model.tokenize().unwrap();
    let decoded = U64KeyModel::decode_token(&token).unwrap();

    assert_eq!(decoded, u64::MAX);
}
