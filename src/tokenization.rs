//! Record Tokenization
//!
//! This module turns model primary keys into signed or encrypted tokens that can
//! be shared outside the database layer.
//!
//! Use it when you want external identifiers in URLs or APIs without exposing
//! the raw primary key directly.
//!
//! If token decoding fails, the most common causes are:
//! - no encryption key or token override was configured at startup
//! - a token was generated for a different model type
//! - the token was tampered with or truncated
//!
//! The default implementation uses authenticated encryption plus URL-safe
//! encoding. Tokens are also bound to the model type, so a `User` token does
//! not decode as a different model.
//!
//! ## Configuration Hierarchy
//!
//! 1. **Model-level** - Override `token_encoder()`/`token_decoder()` in `Tokenizable`
//! 2. **TideConfig-level** - Set via `TokenConfig::set_encoder()` / `TokenConfig::set_decoder()`
//! 3. **Default** - Built-in authenticated encryption using the configured encryption key
//!
//! Typical setup:
//! - configure the encryption key once during startup if you rely on the default encoder
//! - enable `#[tideorm(tokenize)]` on models that should expose tokens externally
//! - use `decode_token()` when you only need the primary key and `from_token()` when you want the record lookup too
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
//! | `User::detokenize(&token)` | Decode token to the model's primary key type |
//! | `User::decode_token(&token)` | Alias for `detokenize()` |
//! | `User::from_token(&token).await` | Decode token and fetch record from DB |
//! | `user.regenerate_token()` | Generate a fresh token; default encoding uses a new random nonce |
//!
//! Override `token_encoder()` and `token_decoder()` on the model only when you
//! need a model-specific token format. Otherwise, keep the default encrypted
//! path so invalid or tampered tokens stay in the `Ok(None)` path instead of
//! becoming configuration errors.
//!
//! ## Security Notes
//!
//! - Use a high-entropy secret in production; 32+ characters is a good baseline
//! - Store keys in environment variables, never hardcode in source
//! - Changing the encryption key invalidates all existing tokens
//! - Tokens are model-specific: a User token cannot decode a Product
//! - The same record may produce different valid tokens with the default encoder

use parking_lot::RwLock;
#[cfg(feature = "encrypted-fields")]
use std::collections::HashMap;
use std::sync::OnceLock;

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::random;

use crate::error::{Error, Result};

// =============================================================================
// TYPE DEFINITIONS
// =============================================================================

/// Function signature for token encoders.
///
/// Receives the serialized primary-key payload plus the model name and returns
/// the external token string.
pub type TokenEncoder = fn(record_id: &str, model_name: &str) -> Result<String>;

/// Function signature for token decoders.
///
/// Returns `Ok(None)` for invalid or mismatched tokens and `Err(...)` when
/// decoding cannot proceed because configuration is missing.
pub type TokenDecoder = fn(token: &str, model_name: &str) -> Result<Option<String>>;

// =============================================================================
// GLOBAL STATE
// =============================================================================

struct TokenizationState {
    encryption_key: Option<ConfiguredEncryptionKey>,
    encoder: Option<TokenEncoder>,
    decoder: Option<TokenDecoder>,
}

static TOKENIZATION_STATE: OnceLock<RwLock<TokenizationState>> = OnceLock::new();

fn tokenization_state() -> &'static RwLock<TokenizationState> {
    TOKENIZATION_STATE.get_or_init(|| {
        RwLock::new(TokenizationState {
            encryption_key: None,
            encoder: None,
            decoder: None,
        })
    })
}

struct ConfiguredEncryptionKey {
    raw: String,
    derived: [u8; 32],
    #[cfg(feature = "encrypted-fields")]
    scoped_derived: RwLock<HashMap<String, [u8; 32]>>,
}

impl ConfiguredEncryptionKey {
    fn new(raw: &str) -> Self {
        Self {
            raw: raw.to_string(),
            derived: derive_encryption_key(raw),
            #[cfg(feature = "encrypted-fields")]
            scoped_derived: RwLock::new(HashMap::new()),
        }
    }

    #[cfg(feature = "encrypted-fields")]
    fn derived_field_key(&self, table_name: &str, column_name: &str) -> [u8; 32] {
        let scope_cache_key = format!("{}\0{}", table_name, column_name);

        if let Some(derived) = self.scoped_derived.read().get(&scope_cache_key).copied() {
            return derived;
        }

        let derived = derive_scoped_encryption_key(&self.derived, table_name, column_name);
        self.scoped_derived
            .write()
            .entry(scope_cache_key)
            .or_insert(derived);
        derived
    }
}

fn with_current_encryption_key<T>(read: impl FnOnce(&ConfiguredEncryptionKey) -> T) -> Option<T> {
    let state = tokenization_state().read();
    state.encryption_key.as_ref().map(read)
}

// =============================================================================
// CONFIGURATION
// =============================================================================

/// Tokenization configuration and utilities
pub struct TokenConfig;

impl TokenConfig {
    /// Set the global encryption key used by the default encoder and decoder.
    ///
    /// If this key changes, previously issued default tokens stop decoding.
    pub fn set_encryption_key(key: &str) {
        let configured_key = ConfiguredEncryptionKey::new(key);
        tokenization_state().write().encryption_key = Some(configured_key);
    }

    /// Return the configured raw encryption key.
    ///
    /// Fails when no global key has been configured yet.
    pub fn get_encryption_key() -> Result<String> {
        with_current_encryption_key(|configured| configured.raw.clone())
            .ok_or_else(|| Error::tokenization("No encryption key configured"))
    }

    pub(crate) fn get_derived_encryption_key() -> Result<[u8; 32]> {
        with_current_encryption_key(|configured| configured.derived)
            .ok_or_else(|| Error::tokenization("No encryption key configured"))
    }

    #[cfg(feature = "encrypted-fields")]
    pub(crate) fn get_derived_encryption_key_for_field(
        table_name: &str,
        column_name: &str,
    ) -> Result<[u8; 32]> {
        with_current_encryption_key(|configured| {
            configured.derived_field_key(table_name, column_name)
        })
        .ok_or_else(|| Error::tokenization("No encryption key configured"))
    }

    /// Return whether a global encryption key is currently configured.
    pub fn has_encryption_key() -> bool {
        tokenization_state().read().encryption_key.is_some()
    }

    /// Set a global token encoder override.
    ///
    /// Model-level encoders still take precedence over this setting.
    pub fn set_encoder(encoder: TokenEncoder) {
        tokenization_state().write().encoder = Some(encoder);
    }

    /// Set a global token decoder override.
    ///
    /// Model-level decoders still take precedence over this setting.
    pub fn set_decoder(decoder: TokenDecoder) {
        tokenization_state().write().decoder = Some(decoder);
    }

    /// Clear the global key and any global encoder or decoder overrides.
    pub fn reset() {
        let mut state = tokenization_state().write();
        state.encryption_key = None;
        state.encoder = None;
        state.decoder = None;
    }

    /// Return the active global encoder, falling back to the default implementation.
    pub fn get_encoder() -> TokenEncoder {
        tokenization_state()
            .read()
            .encoder
            .unwrap_or(default_encode)
    }

    /// Return the active global decoder, falling back to the default implementation.
    pub fn get_decoder() -> TokenDecoder {
        tokenization_state()
            .read()
            .decoder
            .unwrap_or(default_decode)
    }

    /// Encode a serialized primary-key payload using the active global encoder.
    pub fn encode(record_id: &str, model_name: &str) -> Result<String> {
        Self::get_encoder()(record_id, model_name)
    }

    /// Decode a token using the active global decoder.
    ///
    /// Returns `Ok(None)` for invalid, tampered, or wrong-model tokens.
    pub fn decode(token: &str, model_name: &str) -> Result<Option<String>> {
        Self::get_decoder()(token, model_name)
    }
}

// =============================================================================
// ENCRYPTION UTILITIES
// =============================================================================

const DERIVED_ENCRYPTION_KEY_LEN: usize = 32;
const TOKENIZATION_KDF_SALT: &[u8] = b"tideorm::xchacha20poly1305-key::v2";
#[cfg(feature = "encrypted-fields")]
const ENCRYPTED_FIELD_SCOPE_SALT_PREFIX: &[u8] = b"tideorm::xchacha20poly1305-field-key::v1::";

pub(crate) fn derive_encryption_key(key: &str) -> [u8; 32] {
    derive_key_with_salt(key.as_bytes(), TOKENIZATION_KDF_SALT)
}

#[cfg(feature = "encrypted-fields")]
fn derive_scoped_encryption_key(
    master_key: &[u8; 32],
    table_name: &str,
    column_name: &str,
) -> [u8; 32] {
    let mut salt = Vec::with_capacity(
        ENCRYPTED_FIELD_SCOPE_SALT_PREFIX.len() + table_name.len() + column_name.len() + 1,
    );
    salt.extend_from_slice(ENCRYPTED_FIELD_SCOPE_SALT_PREFIX);
    salt.extend_from_slice(table_name.as_bytes());
    salt.push(0);
    salt.extend_from_slice(column_name.as_bytes());

    derive_key_with_salt(master_key, &salt)
}

fn derive_key_with_salt(secret: &[u8], salt: &[u8]) -> [u8; 32] {
    let params = Params::new(64 * 1024, 3, 1, Some(DERIVED_ENCRYPTION_KEY_LEN)).unwrap();
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut derived = [0u8; DERIVED_ENCRYPTION_KEY_LEN];
    argon2
        .hash_password_into(secret, salt, &mut derived)
        .unwrap();
    derived
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
///
/// Only canonical unpadded base64url is accepted. A non-canonical spelling is
/// treated as a tampered payload and reported as `None`, because otherwise every
/// token would have several distinct spellings that all decode to the same bytes
/// and all authenticate, which breaks any caller that uses the token string as an
/// identity (cache key, rate-limit bucket, revocation list, unique index).
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

    // A 4k+1 length carries six leftover bits that encode no byte at all, so it
    // can never be produced by the encoder.
    if encoded.len() % 4 == 1 {
        return None;
    }

    let mut result = Vec::with_capacity(encoded.len() / 4 * 3);
    let mut bits = 0u32;
    let mut bit_count = 0u32;

    for c in encoded.chars() {
        let value = char_to_value(c)?;
        bits = (bits << 6) | (value as u32);
        bit_count += 6;
        if bit_count >= 8 {
            bit_count -= 8;
            result.push((bits >> bit_count) as u8);
        }
    }

    // The encoder zero-fills the final partial group, so any leftover bit that is
    // set means the input is a non-canonical re-spelling of the same bytes.
    if bit_count > 0 && (bits & ((1u32 << bit_count) - 1)) != 0 {
        return None;
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
/// - ciphertext: encrypted record ID payload plus authentication tag
pub fn default_encode(record_id: &str, model_name: &str) -> Result<String> {
    let key = TokenConfig::get_derived_encryption_key()?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    let nonce_bytes: [u8; 24] = random();
    let nonce = XNonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: record_id.as_bytes(),
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
pub fn default_decode(token: &str, model_name: &str) -> Result<Option<String>> {
    let key = TokenConfig::get_derived_encryption_key()?;
    let cipher = XChaCha20Poly1305::new((&key).into());

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

    Ok(String::from_utf8(plaintext).ok())
}

// =============================================================================
// TOKENIZABLE TRAIT
// =============================================================================

/// Trait for models that support tokenization
///
/// This trait is automatically implemented by TideORM's model macros
/// when tokenization is enabled via `tokenize`.
///
/// Most callers use the macro-generated implementation and only override the
/// encoder or decoder when one model needs a different external token format.
#[async_trait::async_trait]
pub trait Tokenizable: Sized + Send + Sync {
    /// The model primary key type decoded from tokens for this model.
    type TokenPrimaryKey: Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static;

    /// Return the model name bound into generated tokens.
    fn token_model_name() -> &'static str;

    /// Return the primary key value that should be encoded into the token.
    fn token_primary_key(&self) -> Self::TokenPrimaryKey;

    /// Return whether token helpers should be available for this model.
    fn tokenization_enabled() -> bool {
        true
    }

    /// Return a model-specific encoder override.
    ///
    /// Return `None` to use the global encoder path instead.
    fn token_encoder() -> Option<TokenEncoder> {
        None
    }

    /// Return a model-specific decoder override.
    ///
    /// Return `None` to use the global decoder path instead.
    fn token_decoder() -> Option<TokenDecoder> {
        None
    }

    /// Encode this record's primary key into an external token.
    ///
    /// Fails when tokenization is disabled, the key cannot be serialized, or
    /// the active encoder reports an error.
    fn to_token(&self) -> Result<String> {
        if !Self::tokenization_enabled() {
            return Err(Error::tokenization(
                "Tokenization is not enabled for this model",
            ));
        }

        let encoder = Self::token_encoder().unwrap_or_else(TokenConfig::get_encoder);

        let primary_key = self.token_primary_key();
        let payload = serde_json::to_string(&primary_key).map_err(|error| {
            Error::tokenization(format!("Failed to serialize token primary key: {error}"))
        })?;
        encoder(&payload, Self::token_model_name())
    }

    /// Alias for `to_token()`.
    fn tokenize(&self) -> Result<String> {
        self.to_token()
    }

    /// Encode one primary-key value without loading a record first.
    fn tokenize_id(id: Self::TokenPrimaryKey) -> Result<String> {
        if !Self::tokenization_enabled() {
            return Err(Error::tokenization(
                "Tokenization is not enabled for this model",
            ));
        }

        let encoder = Self::token_encoder().unwrap_or_else(TokenConfig::get_encoder);

        let payload = serde_json::to_string(&id).map_err(|error| {
            Error::tokenization(format!("Failed to serialize token primary key: {error}"))
        })?;
        encoder(&payload, Self::token_model_name())
    }

    /// Decode a token and load the matching record.
    async fn from_token(token: &str) -> Result<Self>;

    /// Alias for `decode_token()`.
    fn detokenize(token: &str) -> Result<Self::TokenPrimaryKey> {
        Self::decode_token(token)
    }

    /// Decode a token into the model primary key without loading the record.
    ///
    /// Fails when tokenization is disabled, the token does not belong to this
    /// model, or the decoded payload cannot be deserialized into the primary-key type.
    fn decode_token(token: &str) -> Result<Self::TokenPrimaryKey> {
        if !Self::tokenization_enabled() {
            return Err(Error::tokenization(
                "Tokenization is not enabled for this model",
            ));
        }

        let decoder = Self::token_decoder().unwrap_or_else(TokenConfig::get_decoder);
        let payload = decoder(token, Self::token_model_name())?
            .ok_or_else(|| Error::invalid_token("Failed to decode token"))?;

        serde_json::from_str::<Self::TokenPrimaryKey>(&payload).map_err(|error| {
            Error::invalid_token(format!(
                "Failed to deserialize decoded token payload '{}' for model {}: {}",
                payload,
                Self::token_model_name(),
                error
            ))
        })
    }

    /// Generate a fresh token for this record.
    ///
    /// With the default encoder, the token changes because a new random nonce
    /// is used each time.
    fn regenerate_token(&self) -> Result<String> {
        self.to_token()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
#[path = "../tests/unit/tokenization_tests.rs"]
mod tests;
