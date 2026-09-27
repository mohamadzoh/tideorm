//! Encrypted column payloads for `#[tideorm(encrypted)]`.
//!
//! Fields marked `#[tideorm(encrypted)]` (feature `encrypted-fields`) are stored
//! as XChaCha20-Poly1305 payloads behind an `enc::` prefix.
//!
//! The key is derived per `(table, column)` from the configured encryption key,
//! so a ciphertext only decrypts in the exact column it was written to. Copying a
//! payload from a low-privilege encrypted column into a high-privilege one fails
//! authentication instead of silently decrypting.
//!
//! Configure the key once during startup with
//! `TideConfig::init().encryption_key("...")` or
//! `TokenConfig::set_encryption_key("...")`. Without it, encrypting or decrypting
//! an encrypted field is a configuration error rather than a silent passthrough.

use crate::error::{Error, Result};
use crate::tokenization::{TokenConfig, base64_url_decode, base64_url_encode, open, seal};

/// AAD bound into every `#[tideorm(encrypted)]` payload.
const ENCRYPTED_PAYLOAD_AAD: &[u8] = b"tideorm:encrypted-field:v1";
const ENCRYPTED_PAYLOAD_PREFIX: &str = "enc::";

pub(super) fn encrypted_field_missing_key_error(operation: &str) -> Error {
    Error::configuration(format!(
        "Encrypted field {} requires an encryption key. Configure one during startup with TideConfig::init().encryption_key(\"...\") or TokenConfig::set_encryption_key(\"...\") before using #[tideorm(encrypted)] fields.",
        operation
    ))
}

fn field_key(table_name: &str, column_name: &str, operation: &str) -> Result<[u8; 32]> {
    TokenConfig::get_derived_encryption_key_for_field(table_name, column_name)
        .map_err(|_| encrypted_field_missing_key_error(operation))
}

/// Seal `plaintext` under `key` into the `enc::`-prefixed column payload.
pub(crate) fn encode_payload(key: &[u8; 32], plaintext: &[u8]) -> Result<String> {
    let sealed = seal(key, plaintext, ENCRYPTED_PAYLOAD_AAD)
        .ok_or_else(|| Error::tokenization("Failed to encrypt field payload"))?;
    Ok(format!(
        "{}{}",
        ENCRYPTED_PAYLOAD_PREFIX,
        base64_url_encode(&sealed)
    ))
}

pub(crate) fn encrypt_json_value_for_attribute(
    value: &serde_json::Value,
    table_name: &str,
    column_name: &str,
) -> Result<String> {
    let plaintext = serde_json::to_vec(value)?;
    let key = field_key(table_name, column_name, "serialization")?;
    encode_payload(&key, &plaintext)
}

pub(crate) fn decrypt_json_value_for_attribute(
    text: &str,
    table_name: &str,
    column_name: &str,
) -> Result<serde_json::Value> {
    let body = text.strip_prefix(ENCRYPTED_PAYLOAD_PREFIX).ok_or_else(|| {
        Error::tokenization("Encrypted fields must use the encrypted payload format")
    })?;
    let key = field_key(table_name, column_name, "deserialization")?;
    let sealed = base64_url_decode(body)
        .ok_or_else(|| Error::tokenization("Invalid encrypted field payload"))?;
    let plaintext = open(&key, &sealed, ENCRYPTED_PAYLOAD_AAD)
        .ok_or_else(|| Error::tokenization("Failed to decrypt field payload"))?;
    Ok(serde_json::from_slice(&plaintext)?)
}

pub(crate) fn is_encrypted_json_value(text: &str) -> bool {
    text.starts_with(ENCRYPTED_PAYLOAD_PREFIX)
}
