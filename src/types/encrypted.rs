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

#[cfg(feature = "encrypted-fields")]
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
#[cfg(feature = "encrypted-fields")]
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
#[cfg(feature = "encrypted-fields")]
use rand::random;

/// AAD bound into every `#[tideorm(encrypted)]` payload.
#[cfg(feature = "encrypted-fields")]
const ENCRYPTED_PAYLOAD_AAD: &[u8] = b"tideorm:encrypted-field:v1";
#[cfg(feature = "encrypted-fields")]
pub(crate) const ENCRYPTED_PAYLOAD_PREFIX: &str = "enc::";
#[cfg(feature = "encrypted-fields")]
const ENCRYPTED_PAYLOAD_NONCE_LEN: usize = 24;

#[cfg(any(test, feature = "encrypted-fields"))]
pub(crate) fn encrypted_field_missing_key_error(operation: &str) -> crate::Error {
    crate::Error::configuration(format!(
        "Encrypted field {} requires an encryption key. Configure one during startup with TideConfig::init().encryption_key(\"...\") or TokenConfig::set_encryption_key(\"...\") before using #[tideorm(encrypted)] fields.",
        operation
    ))
}

/// Encrypt a JSON value with the unscoped, process-wide encryption key.
///
/// This is the payload shape TideORM wrote before encrypted fields were keyed per
/// `(table, column)`. Only tests use it, to prove the load path still rejects it.
#[cfg(all(test, feature = "encrypted-fields"))]
pub(crate) fn encrypt_json_value(value: &serde_json::Value) -> crate::error::Result<String> {
    let plaintext = serde_json::to_vec(value).map_err(crate::Error::from)?;
    let derived_key = crate::tokenization::TokenConfig::get_derived_encryption_key()
        .map_err(|_| encrypted_field_missing_key_error("serialization"))?;
    encrypt_encrypted_payload_with_key(&plaintext, derived_key, ENCRYPTED_PAYLOAD_AAD)
}

#[cfg(feature = "encrypted-fields")]
pub(crate) fn encrypt_json_value_for_attribute(
    value: &serde_json::Value,
    table_name: &str,
    column_name: &str,
) -> crate::error::Result<String> {
    let plaintext = serde_json::to_vec(value).map_err(crate::Error::from)?;
    encrypt_encrypted_payload_for_attribute(&plaintext, table_name, column_name)
}

#[cfg(feature = "encrypted-fields")]
pub(crate) fn decrypt_json_value_for_attribute(
    text: &str,
    table_name: &str,
    column_name: &str,
) -> crate::error::Result<serde_json::Value> {
    let ciphertext = encrypted_payload_body(text)?;
    let plaintext = decrypt_encrypted_payload_for_attribute(ciphertext, table_name, column_name)?;
    serde_json::from_slice(&plaintext).map_err(crate::Error::from)
}

#[cfg(feature = "encrypted-fields")]
pub(crate) fn is_encrypted_json_value(text: &str) -> bool {
    text.starts_with(ENCRYPTED_PAYLOAD_PREFIX)
}

#[cfg(feature = "encrypted-fields")]
fn encrypted_payload_body(text: &str) -> crate::error::Result<&str> {
    text.strip_prefix(ENCRYPTED_PAYLOAD_PREFIX).ok_or_else(|| {
        crate::Error::tokenization("Encrypted fields must use the encrypted payload format")
    })
}

#[cfg(feature = "encrypted-fields")]
fn encrypt_encrypted_payload_for_attribute(
    plaintext: &[u8],
    table_name: &str,
    column_name: &str,
) -> crate::error::Result<String> {
    let derived_key = crate::tokenization::TokenConfig::get_derived_encryption_key_for_field(
        table_name,
        column_name,
    )
    .map_err(|_| encrypted_field_missing_key_error("serialization"))?;
    encrypt_encrypted_payload_with_key(plaintext, derived_key, ENCRYPTED_PAYLOAD_AAD)
}

#[cfg(feature = "encrypted-fields")]
fn encrypt_encrypted_payload_with_key(
    plaintext: &[u8],
    derived_key: [u8; 32],
    aad: &[u8],
) -> crate::error::Result<String> {
    let cipher = XChaCha20Poly1305::new((&derived_key).into());
    let nonce_bytes: [u8; ENCRYPTED_PAYLOAD_NONCE_LEN] = random();
    let nonce = XNonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| crate::Error::tokenization("Failed to encrypt field payload"))?;

    let mut payload = Vec::with_capacity(ENCRYPTED_PAYLOAD_NONCE_LEN + ciphertext.len());
    payload.extend_from_slice(&nonce_bytes);
    payload.extend_from_slice(&ciphertext);

    Ok(format!(
        "{}{}",
        ENCRYPTED_PAYLOAD_PREFIX,
        crate::tokenization::base64_url_encode(&payload)
    ))
}

#[cfg(feature = "encrypted-fields")]
fn decrypt_encrypted_payload_for_attribute(
    encoded: &str,
    table_name: &str,
    column_name: &str,
) -> crate::error::Result<Vec<u8>> {
    let derived_key = crate::tokenization::TokenConfig::get_derived_encryption_key_for_field(
        table_name,
        column_name,
    )
    .map_err(|_| encrypted_field_missing_key_error("deserialization"))?;
    let payload = decode_encrypted_payload_bytes(encoded)?;
    decrypt_encrypted_payload_with_key(&payload, derived_key, ENCRYPTED_PAYLOAD_AAD)
}

#[cfg(feature = "encrypted-fields")]
fn decode_encrypted_payload_bytes(encoded: &str) -> crate::error::Result<Vec<u8>> {
    crate::tokenization::base64_url_decode(encoded)
        .filter(|payload| payload.len() > ENCRYPTED_PAYLOAD_NONCE_LEN)
        .ok_or_else(|| crate::Error::tokenization("Invalid encrypted field payload"))
}

#[cfg(feature = "encrypted-fields")]
fn decrypt_encrypted_payload_with_key(
    payload: &[u8],
    derived_key: [u8; 32],
    aad: &[u8],
) -> crate::error::Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new((&derived_key).into());
    let nonce = XNonce::from_slice(&payload[..ENCRYPTED_PAYLOAD_NONCE_LEN]);
    cipher
        .decrypt(
            nonce,
            Payload {
                msg: &payload[ENCRYPTED_PAYLOAD_NONCE_LEN..],
                aad,
            },
        )
        .map_err(|_| crate::Error::tokenization("Failed to decrypt field payload"))
}
