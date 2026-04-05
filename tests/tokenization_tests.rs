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
//! #[tideorm(table = "users", tokenize)]
//! pub struct User {
//!     #[tideorm(primary_key)]
//!     pub id: i64,
//!     pub name: String,
//! }
//!
//! let token = user.tokenize()?;
//! let id = User::detokenize(&token)?;
//! let user = User::from_token(&token).await?;
//! ```

use std::sync::Once;

static INIT: Once = Once::new();

fn init_test_env() {
    INIT.call_once(|| {
        tideorm::tokenization::TokenConfig::set_encryption_key("test-encryption-key-for-tests-32!");
    });
}

#[path = "tokenization_tests/unit_tests.rs"]
mod unit_test_cases;

#[path = "tokenization_tests/tokenizable_trait_tests.rs"]
mod tokenizable_trait_test_cases;

#[path = "tokenization_tests/custom_encoder_tests.rs"]
mod custom_encoder_test_cases;

#[path = "tokenization_tests/security_tests.rs"]
mod security_test_cases;

#[path = "tokenization_tests/edge_cases.rs"]
mod edge_case_test_cases;
