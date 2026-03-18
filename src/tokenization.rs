//! Record Tokenization
//!
//! This module provides secure tokenization for TideORM records. Tokenization
//! converts a record's primary key into an encrypted token that can be safely
//! shared externally (e.g., in URLs, API responses) without exposing the actual ID.
//!
//! ## Features
//!
//! - **Secure encryption**: Uses XChaCha20-Poly1305 authenticated encryption
//! - **Configurable at multiple levels**: Global (TideConfig), Model-specific, or default
//! - **Override priority**: Model → TideConfig → Default
//! - **Tamper detection**: Authenticated encryption rejects modified tokens
//! - **URL-safe encoding**: Base64-URL encoding for safe use in URLs
//! - **Model-specific tokens**: Tokens are bound to model type (User token ≠ Product token)
//! - **Randomized tokens**: The default encoder uses a fresh nonce for every token
//!
//! ## Configuration Hierarchy
//!
//! 1. **Model-level** - Override `token_encoder()`/`token_decoder()` in `Tokenizable`
//! 2. **TideConfig-level** - Set via `TokenConfig::set_encoder()` / `TokenConfig::set_decoder()`
//! 3. **Default** - Built-in authenticated encryption using the configured encryption key
//!
//! ## Quick Start with Derive Macro
//!
//! The easiest way to enable tokenization is using the `#[tideorm(tokenize)]` attribute:
//!
//! ```rust,ignore
//! use tideorm::prelude::*;
//!
//! #[derive(Model)]
//! #[tideorm(table = "users", tokenize)]  // Enable tokenization with derive macro
//! pub struct User {
//!     #[tideorm(primary_key, auto_increment)]
//!     pub id: i64,
//!     pub email: String,
//!     pub name: String,
//! }
//!
//! // Configure encryption key (do this once at startup)
//! TokenConfig::set_encryption_key("your-32-byte-secret-key-here-xx");
//!
//! // Now you can tokenize records
//! let user = User::find(1).await?.unwrap();
//! let token = user.tokenize()?;           // "iIBmdKYhJh4_vSKFlBTP..."
//!
//! // Decode without fetching
//! let id = User::detokenize(&token)?;     // 1
//!
//! // Or fetch directly from token
//! let same_user = User::from_token(&token).await?;
//! assert_eq!(user.id, same_user.id);
//! ```
//!
//! ## Manual Implementation
//!
//! You can also implement the `Tokenizable` trait manually:
//!
//! ```rust,ignore
//! use tideorm::prelude::*;
//! use tideorm::tokenization::Tokenizable;
//!
//! #[derive(Model)]
//! #[tideorm(table = "users")]
//! pub struct User {
//!     #[tideorm(primary_key)]
//!     pub id: i64,
//!     pub name: String,
//! }
//!
//! #[async_trait::async_trait]
//! impl Tokenizable for User {
//!     fn token_model_name() -> &'static str {
//!         "User"
//!     }
//!     
//!     fn token_primary_key(&self) -> i64 {
//!         self.id
//!     }
//!     
//!     async fn from_token(token: &str) -> tideorm::Result<Self> {
//!         let id = Self::decode_token(token)?;
//!         Self::find(id)
//!             .await?
//!             .ok_or_else(|| tideorm::Error::not_found("User not found"))
//!     }
//! }
//! ```
//!
//! ## Available Methods
//!
//! When a model implements `Tokenizable`, these methods become available:
//!
//! | Method | Description |
//! |--------|-------------|
//! | `user.tokenize()` | Convert record to token (instance method) |
//! | `user.to_token()` | Alias for `tokenize()` |
//! | `User::tokenize_id(42)` | Tokenize an ID without having the record |
//! | `User::detokenize(&token)` | Decode token to ID (doesn't fetch from DB) |
//! | `User::decode_token(&token)` | Alias for `detokenize()` |
//! | `User::from_token(&token).await` | Decode token and fetch record from DB |
//! | `user.regenerate_token()` | Generate a fresh token; default encoding uses a new random nonce |
//!
//! ## Custom Encoder/Decoder
//!
//! For models that need custom tokenization logic:
//!
//! ```rust,ignore
//! #[async_trait::async_trait]
//! impl Tokenizable for SecretDocument {
//!     fn token_model_name() -> &'static str { "SecretDocument" }
//!     fn token_primary_key(&self) -> i64 { self.id }
//!     
//!     fn token_encoder() -> Option<TokenEncoder> {
//!         Some(|record_id, _model_name| {
//!             Ok(format!("DOC-{}", record_id))
//!         })
//!     }
//!     
//!     fn token_decoder() -> Option<TokenDecoder> {
//!         Some(|token, _model_name| {
//!             Ok(token.strip_prefix("DOC-")
//!                 .and_then(|id| id.parse().ok()))
//!         })
//!     }
//!     
//!     async fn from_token(token: &str) -> tideorm::Result<Self> {
//!         let id = Self::decode_token(token)?;
//!         Self::find(id).await?.ok_or_else(|| tideorm::Error::not_found("Not found"))
//!     }
//! }
//! ```
//!
//! ## Security Notes
//!
//! - Use a high-entropy secret in production; 32+ characters is a good baseline
//! - Store keys in environment variables, never hardcode in source
//! - Changing the encryption key invalidates all existing tokens
//! - Tokens are model-specific: a User token cannot decode a Product
//! - The same record may produce different valid tokens with the default encoder

use std::sync::OnceLock;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::random;
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

// =============================================================================
// TYPE DEFINITIONS
// =============================================================================

/// Token encoder function type
///
/// Takes the record's primary key (as i64) and model name, returns the encoded token.
/// The record parameter allows for model-aware encoding.
///
/// # Arguments
/// * `record_id` - The primary key value to encode
/// * `model_name` - The name of the model (e.g., "User", "Product")
///
/// # Returns
/// The encoded token string, or an error if encoding fails
pub type TokenEncoder = fn(record_id: i64, model_name: &str) -> Result<String>;

/// Token decoder function type
///
/// Takes a token and model name, returns the decoded primary key.
///
/// # Arguments
/// * `token` - The token string to decode
/// * `model_name` - The name of the model (e.g., "User", "Product")
///
/// # Returns
/// The decoded primary key (i64), or None if decoding fails validation.
/// Returns an error when decoding cannot proceed due to misconfiguration.
pub type TokenDecoder = fn(token: &str, model_name: &str) -> Result<Option<i64>>;

// =============================================================================
// GLOBAL STATE
// =============================================================================

/// Global encryption key for tokenization
static GLOBAL_ENCRYPTION_KEY: OnceLock<String> = OnceLock::new();

/// Global token encoder override
static GLOBAL_TOKEN_ENCODER: OnceLock<TokenEncoder> = OnceLock::new();

/// Global token decoder override
static GLOBAL_TOKEN_DECODER: OnceLock<TokenDecoder> = OnceLock::new();

// =============================================================================
// CONFIGURATION
// =============================================================================

/// Tokenization configuration and utilities
pub struct TokenConfig;

impl TokenConfig {
    /// Set the global encryption key for tokenization
    ///
    /// The key should be at least 32 bytes for secure encryption.
    /// This key is used by the default encoder/decoder.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// TokenConfig::set_encryption_key("your-32-byte-secret-key-here-xx");
    /// ```
    pub fn set_encryption_key(key: &str) {
        let _ = GLOBAL_ENCRYPTION_KEY.set(key.to_string());
    }

    /// Get the global encryption key
    ///
    /// Returns an error if no encryption key has been configured.
    pub fn get_encryption_key() -> Result<String> {
        GLOBAL_ENCRYPTION_KEY
            .get()
            .cloned()
            .ok_or_else(|| Error::tokenization("No encryption key configured"))
    }

    /// Check if an encryption key has been explicitly configured
    pub fn has_encryption_key() -> bool {
        GLOBAL_ENCRYPTION_KEY.get().is_some()
    }

    /// Set a custom global token encoder
    ///
    /// This encoder will be used for all models unless overridden at the model level.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// TokenConfig::set_encoder(|record_id, model_name| {
    ///     Ok(format!("{}-{}", model_name.to_lowercase(), record_id))
    /// });
    /// ```
    pub fn set_encoder(encoder: TokenEncoder) {
        let _ = GLOBAL_TOKEN_ENCODER.set(encoder);
    }

    /// Set a custom global token decoder
    ///
    /// This decoder will be used for all models unless overridden at the model level.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// TokenConfig::set_decoder(|token, model_name| {
    ///     let prefix = format!("{}-", model_name.to_lowercase());
    ///     Ok(token.strip_prefix(&prefix)
    ///         .and_then(|id| id.parse().ok()))
    /// });
    /// ```
    pub fn set_decoder(decoder: TokenDecoder) {
        let _ = GLOBAL_TOKEN_DECODER.set(decoder);
    }

    /// Get the global token encoder (or default)
    pub fn get_encoder() -> TokenEncoder {
        GLOBAL_TOKEN_ENCODER
            .get()
            .copied()
            .unwrap_or(default_encode)
    }

    /// Get the global token decoder (or default)
    pub fn get_decoder() -> TokenDecoder {
        GLOBAL_TOKEN_DECODER
            .get()
            .copied()
            .unwrap_or(default_decode)
    }

    /// Encode a record ID using the global encoder
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let token = TokenConfig::encode(42, "User")?;
    /// ```
    pub fn encode(record_id: i64, model_name: &str) -> Result<String> {
        Self::get_encoder()(record_id, model_name)
    }

    /// Decode a token using the global decoder
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let id = TokenConfig::decode(&token, "User")?
    ///     .ok_or_else(|| Error::invalid_token("Invalid token"))?;
    /// ```
    pub fn decode(token: &str, model_name: &str) -> Result<Option<i64>> {
        Self::get_decoder()(token, model_name)
    }
}

// =============================================================================
// ENCRYPTION UTILITIES
// =============================================================================

pub(crate) fn derive_encryption_key(key: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hasher.finalize().into()
}

/// Base64-URL safe encoding (no padding)
pub(crate) fn base64_url_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

    let mut result = String::new();
    let mut bits = 0u32;
    let mut bit_count = 0;

    for &byte in data {
        bits = (bits << 8) | (byte as u32);
        bit_count += 8;
        while bit_count >= 6 {
            bit_count -= 6;
            result.push(ALPHABET[((bits >> bit_count) & 0x3F) as usize] as char);
        }
    }

    if bit_count > 0 {
        bits <<= 6 - bit_count;
        result.push(ALPHABET[(bits & 0x3F) as usize] as char);
    }

    result
}

/// Base64-URL safe decoding
pub(crate) fn base64_url_decode(encoded: &str) -> Option<Vec<u8>> {
    fn char_to_value(c: char) -> Option<u8> {
        match c {
            'A'..='Z' => Some(c as u8 - b'A'),
            'a'..='z' => Some(c as u8 - b'a' + 26),
            '0'..='9' => Some(c as u8 - b'0' + 52),
            '-' => Some(62),
            '_' => Some(63),
            _ => None,
        }
    }

    let mut result = Vec::new();
    let mut bits = 0u32;
    let mut bit_count = 0;

    for c in encoded.chars() {
        let value = char_to_value(c)?;
        bits = (bits << 6) | (value as u32);
        bit_count += 6;
        if bit_count >= 8 {
            bit_count -= 8;
            result.push((bits >> bit_count) as u8);
        }
    }

    Some(result)
}

// =============================================================================
// DEFAULT ENCODER/DECODER
// =============================================================================

/// Default token encoder using XChaCha20-Poly1305 authenticated encryption.
///
/// Token format: base64url(nonce || ciphertext)
/// - nonce: 24 bytes - random nonce for XChaCha20-Poly1305
/// - ciphertext: encrypted record ID plus authentication tag
pub fn default_encode(record_id: i64, model_name: &str) -> Result<String> {
    let key = TokenConfig::get_encryption_key()?;
    let cipher = XChaCha20Poly1305::new((&derive_encryption_key(&key)).into());
    let nonce_bytes: [u8; 24] = random();
    let nonce = XNonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: &record_id.to_be_bytes(),
                aad: model_name.as_bytes(),
            },
        )
        .map_err(|_| Error::tokenization("Failed to encrypt token payload"))?;

    let mut token_data = Vec::with_capacity(24 + ciphertext.len());
    token_data.extend_from_slice(&nonce_bytes);
    token_data.extend_from_slice(&ciphertext);

    Ok(base64_url_encode(&token_data))
}

/// Default token decoder.
///
/// Returns `Ok(None)` for invalid or tampered tokens and `Err(...)` when
/// tokenization is misconfigured, such as when no encryption key is set.
pub fn default_decode(token: &str, model_name: &str) -> Result<Option<i64>> {
    let key = TokenConfig::get_encryption_key()?;
    let cipher = XChaCha20Poly1305::new((&derive_encryption_key(&key)).into());

    let Some(token_data) = base64_url_decode(token) else {
        return Ok(None);
    };

    if token_data.len() <= 24 {
        return Ok(None);
    }

    let nonce = XNonce::from_slice(&token_data[..24]);
    let plaintext = match cipher.decrypt(
        nonce,
        Payload {
            msg: &token_data[24..],
            aad: model_name.as_bytes(),
        },
    ) {
        Ok(plaintext) => plaintext,
        Err(_) => return Ok(None),
    };

    if plaintext.len() != 8 {
        return Ok(None);
    }

    let Some(id_bytes) = plaintext.try_into().ok() else {
        return Ok(None);
    };

    Ok(Some(i64::from_be_bytes(id_bytes)))
}

// =============================================================================
// TOKENIZABLE TRAIT
// =============================================================================

/// Trait for models that support tokenization
///
/// This trait is automatically implemented by the `#[derive(Model)]` macro
/// when tokenization is enabled via `#[tideorm(tokenize)]`.
///
/// ## Example
///
/// ```rust,ignore
/// #[derive(Model)]
/// #[tideorm(table = "users", tokenize)]  // Enable tokenization
/// pub struct User {
///     #[tideorm(primary_key)]
///     pub id: i64,
///     pub email: String,
/// }
///
/// // Now User implements Tokenizable
/// let user = User::find(1).await?;
/// let token = user.to_token()?;
/// let restored = User::from_token(&token).await?;
/// ```
#[async_trait::async_trait]
pub trait Tokenizable: Sized + Send + Sync {
    /// Get the model name for tokenization
    fn token_model_name() -> &'static str;

    /// Get the primary key value from this record
    fn token_primary_key(&self) -> i64;

    /// Check if tokenization is enabled for this model
    fn tokenization_enabled() -> bool {
        true
    }

    /// Get the token encoder for this model
    ///
    /// Override to provide model-specific encoding logic.
    /// Returns `None` to use the global encoder.
    fn token_encoder() -> Option<TokenEncoder> {
        None
    }

    /// Get the token decoder for this model
    ///
    /// Override to provide model-specific decoding logic.
    /// Returns `None` to use the global decoder.
    fn token_decoder() -> Option<TokenDecoder> {
        None
    }

    /// Convert this record to a token
    ///
    /// Encrypts the record's primary key into a secure, URL-safe token.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let user = User::find(42).await?;
    /// let token = user.to_token()?;
    /// // token: "eyJhbGciOiJ..."
    /// ```
    fn to_token(&self) -> Result<String> {
        if !Self::tokenization_enabled() {
            return Err(Error::tokenization(
                "Tokenization is not enabled for this model",
            ));
        }

        let encoder = Self::token_encoder().unwrap_or_else(TokenConfig::get_encoder);

        encoder(self.token_primary_key(), Self::token_model_name())
    }

    /// Alias for `to_token()` - Convert this record to a token
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let user = User::find(42).await?;
    /// let token = user.tokenize()?;
    /// ```
    fn tokenize(&self) -> Result<String> {
        self.to_token()
    }

    /// Tokenize a specific ID without having the full record
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let token = User::tokenize_id(42)?;
    /// ```
    fn tokenize_id(id: i64) -> Result<String> {
        if !Self::tokenization_enabled() {
            return Err(Error::tokenization(
                "Tokenization is not enabled for this model",
            ));
        }

        let encoder = Self::token_encoder().unwrap_or_else(TokenConfig::get_encoder);

        encoder(id, Self::token_model_name())
    }

    /// Find a record from a token
    ///
    /// Decrypts the token to get the primary key, then fetches the record.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let user = User::from_token("eyJhbGciOiJ...").await?;
    /// ```
    async fn from_token(token: &str) -> Result<Self>;

    /// Alias for `decode_token()` - Decode a token to get the record ID
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let id = User::detokenize("eyJhbGciOiJ...")?;
    /// // id: 42
    /// ```
    fn detokenize(token: &str) -> Result<i64> {
        Self::decode_token(token)
    }

    /// Decode a token to get the record ID without fetching
    ///
    /// Useful when you just need the ID without loading the full record.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let id = User::decode_token("eyJhbGciOiJ...")?;
    /// // id: 42
    /// ```
    fn decode_token(token: &str) -> Result<i64> {
        if !Self::tokenization_enabled() {
            return Err(Error::tokenization(
                "Tokenization is not enabled for this model",
            ));
        }

        if let Some(decoder) = Self::token_decoder() {
            return decoder(token, Self::token_model_name())?
                .ok_or_else(|| Error::invalid_token("Failed to decode token"));
        }

        default_decode(token, Self::token_model_name())?
            .ok_or_else(|| Error::invalid_token("Failed to decode token"))
    }

    /// Regenerate a new token for this record
    ///
    /// Creates a fresh token. With the default encoder, tokens for the same
    /// record differ because a new random nonce is used each time.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let user = User::find(42).await?;
    /// let token1 = user.to_token()?;
    /// let token2 = user.regenerate_token()?;
    /// // With the default encoder: token1 != token2
    /// ```
    fn regenerate_token(&self) -> Result<String> {
        self.to_token()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
#[path = "testing/tokenization_tests.rs"]
mod tests;
