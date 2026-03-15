//! Tokenization Integration Tests
//!
//! Tests for the TideORM tokenization feature that converts record IDs
//! to secure, URL-safe tokens and back.
//!
//! ## Note on Manual Implementations
//!
//! These tests use manual `Tokenizable` implementations because they run
//! without a database connection. In real applications, you should use:
//!
//! ```rust,ignore
//! #[derive(Model)]
//! #[tide(table = "users", tokenize)]  // <-- Just add `tokenize` here!
//! pub struct User {
//!     #[tide(primary_key)]
//!     pub id: i64,
//!     pub name: String,
//! }
//!
//! // Now these work automatically:
//! let token = user.tokenize()?;
//! let id = User::detokenize(&token)?;
//! let user = User::from_token(&token).await?;
//! ```

use std::sync::Once;

// One-time initialization for tests
static INIT: Once = Once::new();

fn init_test_env() {
    INIT.call_once(|| {
        // Set a consistent test encryption key
        tideorm::tokenization::TokenConfig::set_encryption_key("test-encryption-key-for-tests-32!");
    });
}

mod unit_tests {
    use super::*;
    use tideorm::tokenization::{TokenConfig, default_decode, default_encode};

    #[test]
    fn test_encode_decode_roundtrip() {
        init_test_env();

        let id = 12345i64;
        let model = "User";

        let token = default_encode(id, model).unwrap();
        let decoded = default_decode(&token, model);

        assert_eq!(decoded, Some(id));
    }

    #[test]
    fn test_encode_decode_various_ids() {
        init_test_env();

        let test_cases = [
            (0i64, "Zero"),
            (1i64, "One"),
            (100i64, "Hundred"),
            (999999i64, "Large"),
            (i64::MAX, "Max"),
            (-1i64, "NegativeOne"),
            (-999999i64, "NegativeLarge"),
            (i64::MIN, "Min"),
        ];

        for (id, model) in test_cases {
            let token = default_encode(id, model).unwrap();
            let decoded = default_decode(&token, model);
            assert_eq!(decoded, Some(id), "Failed for id={}, model={}", id, model);
        }
    }

    #[test]
    fn test_token_is_url_safe() {
        init_test_env();

        // Test multiple IDs
        for id in [1, 42, 999, 123456789, i64::MAX] {
            let token = default_encode(id, "User").unwrap();

            // Token should only contain URL-safe characters
            assert!(
                token
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                "Token '{}' contains non-URL-safe characters",
                token
            );

            // Token should not contain =, +, or / (standard Base64 chars)
            assert!(!token.contains('='), "Token should not contain padding '='");
            assert!(!token.contains('+'), "Token should not contain '+'");
            assert!(!token.contains('/'), "Token should not contain '/'");
        }
    }

    #[test]
    fn test_model_specific_tokens() {
        init_test_env();

        let id = 42i64;

        let user_token = default_encode(id, "User").unwrap();
        let product_token = default_encode(id, "Product").unwrap();
        let order_token = default_encode(id, "Order").unwrap();

        // Same ID produces different tokens for different models
        assert_ne!(user_token, product_token);
        assert_ne!(user_token, order_token);
        assert_ne!(product_token, order_token);

        // But each decodes correctly with its own model
        assert_eq!(default_decode(&user_token, "User"), Some(id));
        assert_eq!(default_decode(&product_token, "Product"), Some(id));
        assert_eq!(default_decode(&order_token, "Order"), Some(id));
    }

    #[test]
    fn test_cross_model_decode_fails() {
        init_test_env();

        let id = 42i64;
        let user_token = default_encode(id, "User").unwrap();

        // Trying to decode a User token as a Product should fail
        assert_eq!(default_decode(&user_token, "Product"), None);
        assert_eq!(default_decode(&user_token, "Order"), None);
        assert_eq!(default_decode(&user_token, "SomeOtherModel"), None);
    }

    #[test]
    fn test_tampered_token_fails() {
        init_test_env();

        let token = default_encode(42, "User").unwrap();

        // Tamper with different positions
        for pos in [0, 5, 10, 15, 20, 30] {
            if pos < token.len() {
                let mut chars: Vec<char> = token.chars().collect();
                chars[pos] = if chars[pos] == 'A' { 'B' } else { 'A' };
                let tampered: String = chars.into_iter().collect();

                assert_eq!(
                    default_decode(&tampered, "User"),
                    None,
                    "Tampered token at position {} should fail to decode",
                    pos
                );
            }
        }
    }

    #[test]
    fn test_invalid_tokens() {
        init_test_env();

        let invalid_tokens = [
            "",
            "a",
            "abc",
            "!!!invalid!!!",
            "too-short",
            "                    ",
            "contains spaces here",
            "has\nnewline",
            "has\ttab",
        ];

        for invalid in invalid_tokens {
            assert_eq!(
                default_decode(invalid, "User"),
                None,
                "Invalid token '{}' should fail to decode",
                invalid
            );
        }
    }

    #[test]
    fn test_token_consistency() {
        init_test_env();

        let id = 42i64;
        let model = "User";

        // Same ID and model should produce identical tokens
        let token1 = default_encode(id, model).unwrap();
        let token2 = default_encode(id, model).unwrap();
        let token3 = default_encode(id, model).unwrap();

        assert_eq!(token1, token2);
        assert_eq!(token2, token3);
    }

    #[test]
    fn test_different_ids_different_tokens() {
        init_test_env();

        let model = "User";
        let token1 = default_encode(1, model).unwrap();
        let token2 = default_encode(2, model).unwrap();
        let token3 = default_encode(3, model).unwrap();

        assert_ne!(token1, token2);
        assert_ne!(token2, token3);
        assert_ne!(token1, token3);
    }

    #[test]
    fn test_token_length() {
        init_test_env();

        // Token format: base64url(iv || encrypted_data || hmac)
        // = base64url(16 + 8 + 8) = base64url(32 bytes) = 43 chars

        for id in [0, 1, 100, i64::MAX, i64::MIN] {
            let token = default_encode(id, "User").unwrap();
            assert_eq!(
                token.len(),
                43,
                "Token should be 43 characters, got {}",
                token.len()
            );
        }
    }

    #[test]
    fn test_token_config_encode_decode() {
        init_test_env();

        let id = 123i64;
        let model = "TestModel";

        let token = TokenConfig::encode(id, model).unwrap();
        let decoded = TokenConfig::decode(&token, model);

        assert_eq!(decoded, Some(id));
    }
}

mod tokenizable_trait_tests {
    use super::*;
    use tideorm::error::Result;
    use tideorm::tokenization::Tokenizable;

    // Manual implementation of Tokenizable for testing (without database).
    // In real apps, just use `#[tide(tokenize)]` on your model - no manual impl needed!
    struct TestUser {
        id: i64,
    }

    #[async_trait::async_trait]
    impl Tokenizable for TestUser {
        fn token_model_name() -> &'static str {
            "TestUser"
        }

        fn token_primary_key(&self) -> i64 {
            self.id
        }

        async fn from_token(_token: &str) -> Result<Self> {
            // In real impl, this would fetch from DB
            // For testing, we just decode and create a placeholder
            let id = Self::decode_token(_token)?;
            Ok(TestUser { id })
        }
    }

    // Another test model (without database)
    struct TestProduct {
        id: i64,
    }

    #[async_trait::async_trait]
    impl Tokenizable for TestProduct {
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

        // Token should be URL-safe
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

        // Both methods should produce the same token
        assert_eq!(token1, token2);
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

        // With default encoder, regenerate produces the same token
        assert_eq!(token1, token2);
    }

    #[test]
    fn test_tokenizable_cross_model_rejection() {
        init_test_env();

        // Create a TestUser token
        let user = TestUser { id: 42 };
        let user_token = user.tokenize().unwrap();

        // Create a TestProduct token for the same ID
        let product = TestProduct { id: 42 };
        let product_token = product.tokenize().unwrap();

        // Tokens should be different
        assert_ne!(user_token, product_token);

        // User token should not decode as Product
        assert!(TestProduct::decode_token(&user_token).is_err());

        // Product token should not decode as User
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

        // All tokens should be unique
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

        // All tokens should decode back to correct IDs
        for (user, token) in users.iter().zip(tokens.iter()) {
            let decoded = TestUser::decode_token(token).unwrap();
            assert_eq!(decoded, user.id);
        }
    }
}

mod custom_encoder_tests {
    use tideorm::error::Result;
    use tideorm::tokenization::{TokenDecoder, TokenEncoder, Tokenizable};

    // Custom encoder that uses a simple format
    fn simple_encoder(id: i64, model: &str) -> tideorm::error::Result<String> {
        Ok(format!("{}-{}", model.to_lowercase(), id))
    }

    fn simple_decoder(token: &str, model: &str) -> Option<i64> {
        let prefix = format!("{}-", model.to_lowercase());
        token.strip_prefix(&prefix)?.parse().ok()
    }

    // Model with custom encoder
    struct CustomModel {
        id: i64,
    }

    #[async_trait::async_trait]
    impl Tokenizable for CustomModel {
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

        // Custom encoder should produce "custom-42"
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
}

mod security_tests {
    use super::*;
    use tideorm::tokenization::default_encode;

    #[test]
    fn test_tokens_not_predictable() {
        init_test_env();

        // Sequential IDs should not have predictable token patterns
        let token1 = default_encode(1, "User").unwrap();
        let token2 = default_encode(2, "User").unwrap();
        let token3 = default_encode(3, "User").unwrap();

        // Tokens should not share common prefixes beyond IV
        // (First 22 chars are IV which is model-specific)
        let common_prefix = common_prefix_len(&token1, &token2);
        let common_prefix2 = common_prefix_len(&token2, &token3);

        // Due to XOR encryption, encrypted parts should differ
        assert!(
            common_prefix < token1.len(),
            "Tokens share too much common prefix"
        );
        assert!(
            common_prefix2 < token2.len(),
            "Tokens share too much common prefix"
        );
    }

    fn common_prefix_len(a: &str, b: &str) -> usize {
        a.chars()
            .zip(b.chars())
            .take_while(|(ca, cb)| ca == cb)
            .count()
    }

    #[test]
    fn test_token_bits_distribution() {
        init_test_env();

        // Generate many tokens and check character distribution
        let mut char_counts = std::collections::HashMap::new();

        for id in 1..=1000 {
            let token = default_encode(id, "User").unwrap();
            for c in token.chars() {
                *char_counts.entry(c).or_insert(0) += 1;
            }
        }

        // All Base64-URL characters should be represented
        let base64_chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let represented: usize = base64_chars
            .chars()
            .filter(|c| char_counts.contains_key(c))
            .count();

        // At least 80% of characters should appear (allowing for some statistical variance)
        assert!(
            represented > 50,
            "Only {} of 64 Base64 characters represented",
            represented
        );
    }

    #[test]
    fn test_no_id_leakage() {
        init_test_env();

        // The original ID should not appear in the token
        let id = 12345i64;
        let token = default_encode(id, "User").unwrap();

        // Check that ID doesn't appear in any encoding
        assert!(!token.contains(&id.to_string()));
        assert!(!token.contains(&format!("{:x}", id))); // hex
        assert!(!token.contains(&format!("{:o}", id))); // octal
    }

    #[test]
    fn test_model_name_binding() {
        init_test_env();

        // Changing model name should completely change the token
        let id = 42i64;
        let token_user = default_encode(id, "User").unwrap();
        let token_admin = default_encode(id, "Admin").unwrap();

        // Tokens should be completely different (not just a suffix change)
        let common = common_prefix_len(&token_user, &token_admin);
        assert!(
            common < 10,
            "Model-specific tokens share too much in common"
        );
    }
}

mod edge_cases {
    use super::*;
    use tideorm::tokenization::{default_decode, default_encode};

    #[test]
    fn test_empty_model_name() {
        init_test_env();

        // Empty model name should still work
        let token = default_encode(42, "").unwrap();
        let decoded = default_decode(&token, "");

        assert_eq!(decoded, Some(42));
    }

    #[test]
    fn test_long_model_name() {
        init_test_env();

        let long_name = "A".repeat(1000);
        let token = default_encode(42, &long_name).unwrap();
        let decoded = default_decode(&token, &long_name);

        assert_eq!(decoded, Some(42));
    }

    #[test]
    fn test_unicode_model_name() {
        init_test_env();

        let unicode_name = "用户模型🔐";
        let token = default_encode(42, unicode_name).unwrap();
        let decoded = default_decode(&token, unicode_name);

        assert_eq!(decoded, Some(42));
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
            let token = default_encode(42, name).unwrap();
            let decoded = default_decode(&token, name);
            assert_eq!(decoded, Some(42), "Failed for model name: {}", name);
        }
    }

    #[test]
    fn test_boundary_ids() {
        init_test_env();

        let boundary_ids = [i64::MIN, i64::MIN + 1, -1, 0, 1, i64::MAX - 1, i64::MAX];

        for id in boundary_ids {
            let token = default_encode(id, "Boundary").unwrap();
            let decoded = default_decode(&token, "Boundary");
            assert_eq!(decoded, Some(id), "Failed for boundary ID: {}", id);
        }
    }
}
