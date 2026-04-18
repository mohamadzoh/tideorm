use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::random;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Encrypted value wrapper.
///
/// Values are serialized to TideORM's encrypted payload format and must be
/// deserialized from that same encrypted format.
/// Uses XChaCha20-Poly1305 authenticated encryption.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Encrypted<T> {
    value: T,
}

const ENCRYPTED_PAYLOAD_AAD: &[u8] = b"tideorm:encrypted-field:v1";
pub(crate) const ENCRYPTED_PAYLOAD_PREFIX: &str = "enc::";

impl<T> Encrypted<T> {
    /// Create a new encrypted value
    pub fn new(value: T) -> Self {
        Self { value }
    }

    /// Get the inner value
    pub fn into_inner(self) -> T {
        self.value
    }

    /// Get a reference to the inner value
    pub fn inner(&self) -> &T {
        &self.value
    }
}

impl<T: Clone> Encrypted<T> {
    /// Get a clone of the inner value
    pub fn get(&self) -> T {
        self.value.clone()
    }
}

impl<T: fmt::Display> fmt::Display for Encrypted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "***ENCRYPTED***")
    }
}

impl<T: Serialize> Serialize for Encrypted<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let json = serde_json::to_value(&self.value).map_err(serde::ser::Error::custom)?;
        let encoded = encrypt_json_value(&json).map_err(serde::ser::Error::custom)?;
        serializer.serialize_str(&encoded)
    }
}

impl<'de, T> Deserialize<'de> for Encrypted<T>
where
    T: serde::de::DeserializeOwned,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let text = String::deserialize(deserializer)?;
        let value = decrypt_json_value(&text).map_err(serde::de::Error::custom)?;
        let value = serde_json::from_value(value).map_err(serde::de::Error::custom)?;
        Ok(Self { value })
    }
}

impl<T> From<T> for Encrypted<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<T: Default> Default for Encrypted<T> {
    fn default() -> Self {
        Self {
            value: T::default(),
        }
    }
}

pub(crate) fn encrypted_field_missing_key_error(operation: &str) -> crate::Error {
    crate::Error::configuration(format!(
        "Encrypted<T> {} requires an encryption key. Configure one during startup with TideConfig::init().encryption_key(\"...\") or TokenConfig::set_encryption_key(\"...\") before using encrypted fields.",
        operation
    ))
}

pub(crate) fn encrypt_json_value(value: &serde_json::Value) -> crate::error::Result<String> {
    let plaintext = serde_json::to_vec(value).map_err(crate::Error::from)?;
    encrypt_encrypted_payload(&plaintext)
}

pub(crate) fn encrypt_json_value_for_attribute(
    value: &serde_json::Value,
    table_name: &str,
    column_name: &str,
) -> crate::error::Result<String> {
    let plaintext = serde_json::to_vec(value).map_err(crate::Error::from)?;
    encrypt_encrypted_payload_for_attribute(&plaintext, table_name, column_name)
}

pub(crate) fn decrypt_json_value(text: &str) -> crate::error::Result<serde_json::Value> {
    let ciphertext = encrypted_payload_body(text)?;
    let plaintext = decrypt_encrypted_payload(ciphertext)?;
    serde_json::from_slice(&plaintext).map_err(crate::Error::from)
}

pub(crate) fn decrypt_json_value_for_attribute(
    text: &str,
    table_name: &str,
    column_name: &str,
) -> crate::error::Result<serde_json::Value> {
    let ciphertext = encrypted_payload_body(text)?;
    let plaintext = decrypt_encrypted_payload_for_attribute(ciphertext, table_name, column_name)?;
    serde_json::from_slice(&plaintext).map_err(crate::Error::from)
}

pub(crate) fn is_encrypted_json_value(text: &str) -> bool {
    text.starts_with(ENCRYPTED_PAYLOAD_PREFIX)
}

fn encrypted_payload_body(text: &str) -> crate::error::Result<&str> {
    text.strip_prefix(ENCRYPTED_PAYLOAD_PREFIX).ok_or_else(|| {
        crate::Error::tokenization("Encrypted fields must use the encrypted payload format")
    })
}

pub(crate) fn encrypt_encrypted_payload(plaintext: &[u8]) -> crate::error::Result<String> {
    let derived_key = crate::tokenization::TokenConfig::get_derived_encryption_key()
        .map_err(|_| encrypted_field_missing_key_error("serialization"))?;
    encrypt_encrypted_payload_with_key(plaintext, derived_key)
}

pub(crate) fn encrypt_encrypted_payload_for_attribute(
    plaintext: &[u8],
    table_name: &str,
    column_name: &str,
) -> crate::error::Result<String> {
    let derived_key = crate::tokenization::TokenConfig::get_derived_encryption_key_for_field(
        table_name,
        column_name,
    )
    .map_err(|_| encrypted_field_missing_key_error("serialization"))?;
    encrypt_encrypted_payload_with_key(plaintext, derived_key)
}

fn encrypt_encrypted_payload_with_key(
    plaintext: &[u8],
    derived_key: [u8; 32],
) -> crate::error::Result<String> {
    let cipher = XChaCha20Poly1305::new((&derived_key).into());
    let nonce_bytes: [u8; 24] = random();
    let nonce = XNonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                aad: ENCRYPTED_PAYLOAD_AAD,
            },
        )
        .map_err(|_| crate::Error::tokenization("Failed to encrypt field payload"))?;

    let mut payload = Vec::with_capacity(24 + ciphertext.len());
    payload.extend_from_slice(&nonce_bytes);
    payload.extend_from_slice(&ciphertext);

    Ok(format!(
        "{}{}",
        ENCRYPTED_PAYLOAD_PREFIX,
        crate::tokenization::base64_url_encode(&payload)
    ))
}

pub(crate) fn decrypt_encrypted_payload(encoded: &str) -> crate::error::Result<Vec<u8>> {
    let derived_key = crate::tokenization::TokenConfig::get_derived_encryption_key()
        .map_err(|_| encrypted_field_missing_key_error("deserialization"))?;
    decrypt_encrypted_payload_with_key(encoded, derived_key)
}

pub(crate) fn decrypt_encrypted_payload_for_attribute(
    encoded: &str,
    table_name: &str,
    column_name: &str,
) -> crate::error::Result<Vec<u8>> {
    let derived_key = crate::tokenization::TokenConfig::get_derived_encryption_key_for_field(
        table_name,
        column_name,
    )
    .map_err(|_| encrypted_field_missing_key_error("deserialization"))?;
    decrypt_encrypted_payload_with_key(encoded, derived_key)
}

fn decrypt_encrypted_payload_with_key(
    encoded: &str,
    derived_key: [u8; 32],
) -> crate::error::Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new((&derived_key).into());
    let payload = crate::tokenization::base64_url_decode(encoded)
        .ok_or_else(|| crate::Error::tokenization("Invalid encrypted field payload"))?;

    if payload.len() <= 24 {
        return Err(crate::Error::tokenization(
            "Invalid encrypted field payload",
        ));
    }

    let nonce = XNonce::from_slice(&payload[..24]);
    cipher
        .decrypt(
            nonce,
            Payload {
                msg: &payload[24..],
                aad: ENCRYPTED_PAYLOAD_AAD,
            },
        )
        .map_err(|_| crate::Error::tokenization("Failed to decrypt field payload"))
}
