//! Attribute types and casting
//!
//! This module provides type definitions and casting utilities for model attributes.
//!
//! ## Attribute Casting
//!
//! TideORM supports automatic casting of attribute values when reading from and writing
//! to the database. This is useful for complex types like encrypted strings, JSON objects,
//! enums, dates, and more.
//!
//! ### Built-in Casters
//!
//! - `StringCaster` - Basic string type
//! - `IntCaster` - Integer types (i32, i64)
//! - `FloatCaster` - Floating point types (f32, f64)
//! - `BoolCaster` - Boolean values
//! - `JsonCaster` - JSON/JSONB columns
//! - `DateTimeCaster` - DateTime values
//! - `UuidCaster` - UUID values
//! - `DecimalCaster` - Decimal numbers
//! - `EncryptedCaster` - Encrypted string storage using TideORM's encrypted payload format
//! - `HashCaster` - Hashed values (one-way) stored as Argon2 hashes
//! - `EnumCaster` - Database enum types
//! - `ArrayCaster` - Array columns (PostgreSQL)
//! - `CommaSeparatedCaster` - Store arrays as comma-separated strings
//!
//! ### Example
//!
//! ```rust,ignore
//! use tideorm::prelude::*;
//! use tideorm::types::{Encrypted, Hashed, CommaSeparated};
//!
//! #[derive(Model)]
//! #[tideorm(table = "users")]
//! pub struct User {
//!     #[tideorm(primary_key, auto_increment)]
//!     pub id: i64,
//!     pub email: String,
//!     #[tideorm(cast = "encrypted")]
//!     pub ssn: Encrypted<String>,
//!     #[tideorm(cast = "hashed")]
//!     pub password: Hashed,
//!     #[tideorm(cast = "comma_separated")]
//!     pub tags: CommaSeparated<String>,
//! }
//! ```

use argon2::password_hash::PasswordVerifier;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::random;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;

// Re-export common types
pub use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
pub use rust_decimal::Decimal;
pub use uuid::Uuid;

/// JSON field type for storing arbitrary JSON data
pub type Json = serde_json::Value;

/// Jsonb field type (alias for Json, treated the same way)
pub type Jsonb = serde_json::Value;

/// Text type (for long strings)
pub type Text = String;

/// Array types for PostgreSQL array columns
pub type IntArray = Vec<i32>;
/// Big integer array type for PostgreSQL
pub type BigIntArray = Vec<i64>;
/// Text array type for PostgreSQL
pub type TextArray = Vec<String>;
/// Boolean array type for PostgreSQL
pub type BoolArray = Vec<bool>;
/// Float array type for PostgreSQL
pub type FloatArray = Vec<f64>;
/// JSON array type for PostgreSQL
pub type JsonArray = Vec<serde_json::Value>;

// =============================================================================
// UNIX TIMESTAMP TYPES
// =============================================================================

/// Unix timestamp stored as seconds since epoch (i64)
///
/// This provides a portable integer-based timestamp format that works across
/// all databases. Convert to/from `chrono::DateTime` as needed.
///
/// # Example
///
/// ```rust,ignore
/// use tideorm::types::UnixTimestamp;
///
/// #[derive(Model)]
/// #[tideorm(table = "events")]
/// pub struct Event {
///     #[tideorm(primary_key)]
///     pub id: i64,
///     pub created_at: UnixTimestamp,  // Stored as INTEGER in DB
/// }
///
/// let event = Event {
///     id: 1,
///     created_at: UnixTimestamp::now(),  // Current time
/// };
///
/// // Convert to/from chrono DateTime
/// let datetime = event.created_at.to_datetime();
/// let unix = UnixTimestamp::from_datetime(datetime);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnixTimestamp(pub i64);

impl UnixTimestamp {
    /// Create a new Unix timestamp from seconds since epoch
    pub fn new(seconds: i64) -> Self {
        Self(seconds)
    }

    /// Get the current time as a Unix timestamp
    pub fn now() -> Self {
        Self(chrono::Utc::now().timestamp())
    }

    /// Create from a chrono DateTime
    pub fn from_datetime(dt: DateTime<Utc>) -> Self {
        Self(dt.timestamp())
    }

    /// Convert to a chrono DateTime
    pub fn to_datetime(self) -> Option<DateTime<Utc>> {
        chrono::DateTime::from_timestamp(self.0, 0)
    }

    /// Get the raw seconds value
    pub fn as_seconds(&self) -> i64 {
        self.0
    }

    /// Check if this timestamp is in the past
    pub fn is_past(&self) -> bool {
        self.0 < chrono::Utc::now().timestamp()
    }

    /// Check if this timestamp is in the future
    pub fn is_future(&self) -> bool {
        self.0 > chrono::Utc::now().timestamp()
    }
}

impl Default for UnixTimestamp {
    fn default() -> Self {
        Self::now()
    }
}

impl From<i64> for UnixTimestamp {
    fn from(seconds: i64) -> Self {
        Self(seconds)
    }
}

impl From<UnixTimestamp> for i64 {
    fn from(ts: UnixTimestamp) -> Self {
        ts.0
    }
}

impl From<DateTime<Utc>> for UnixTimestamp {
    fn from(dt: DateTime<Utc>) -> Self {
        Self::from_datetime(dt)
    }
}

impl fmt::Display for UnixTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(dt) = self.to_datetime() {
            write!(f, "{}", dt.format("%Y-%m-%d %H:%M:%S UTC"))
        } else {
            write!(f, "{}", self.0)
        }
    }
}

/// Unix timestamp stored as milliseconds since epoch (i64)
///
/// Higher precision version of `UnixTimestamp` for sub-second accuracy.
///
/// # Example
///
/// ```rust,ignore
/// use tideorm::types::UnixTimestampMillis;
///
/// #[derive(Model)]
/// #[tideorm(table = "events")]
/// pub struct Event {
///     #[tideorm(primary_key)]
///     pub id: i64,
///     pub created_at: UnixTimestampMillis,  // Stored as BIGINT in DB
/// }
///
/// let event = Event {
///     id: 1,
///     created_at: UnixTimestampMillis::now(),  // Current time with milliseconds
/// };
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnixTimestampMillis(pub i64);

impl UnixTimestampMillis {
    /// Create a new Unix timestamp from milliseconds since epoch
    pub fn new(millis: i64) -> Self {
        Self(millis)
    }

    /// Get the current time as a Unix timestamp in milliseconds
    pub fn now() -> Self {
        Self(chrono::Utc::now().timestamp_millis())
    }

    /// Create from a chrono DateTime
    pub fn from_datetime(dt: DateTime<Utc>) -> Self {
        Self(dt.timestamp_millis())
    }

    /// Convert to a chrono DateTime
    pub fn to_datetime(self) -> Option<DateTime<Utc>> {
        chrono::DateTime::from_timestamp_millis(self.0)
    }

    /// Get the raw milliseconds value
    pub fn as_millis(&self) -> i64 {
        self.0
    }

    /// Get as seconds (losing millisecond precision)
    pub fn as_seconds(&self) -> i64 {
        self.0 / 1000
    }

    /// Convert to UnixTimestamp (seconds)
    pub fn to_unix_timestamp(self) -> UnixTimestamp {
        UnixTimestamp(self.0 / 1000)
    }

    /// Check if this timestamp is in the past
    pub fn is_past(&self) -> bool {
        self.0 < chrono::Utc::now().timestamp_millis()
    }

    /// Check if this timestamp is in the future
    pub fn is_future(&self) -> bool {
        self.0 > chrono::Utc::now().timestamp_millis()
    }
}

impl Default for UnixTimestampMillis {
    fn default() -> Self {
        Self::now()
    }
}

impl From<i64> for UnixTimestampMillis {
    fn from(millis: i64) -> Self {
        Self(millis)
    }
}

impl From<UnixTimestampMillis> for i64 {
    fn from(ts: UnixTimestampMillis) -> Self {
        ts.0
    }
}

impl From<DateTime<Utc>> for UnixTimestampMillis {
    fn from(dt: DateTime<Utc>) -> Self {
        Self::from_datetime(dt)
    }
}

impl From<UnixTimestamp> for UnixTimestampMillis {
    fn from(ts: UnixTimestamp) -> Self {
        Self(ts.0 * 1000)
    }
}

impl fmt::Display for UnixTimestampMillis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(dt) = self.to_datetime() {
            write!(f, "{}", dt.format("%Y-%m-%d %H:%M:%S%.3f UTC"))
        } else {
            write!(f, "{}", self.0)
        }
    }
}

// =============================================================================
// ENUM WRAPPER
// =============================================================================

/// Enum wrapper for database enums
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Clone, Debug, Serialize, Deserialize)]
/// pub enum Status {
///     Active,
///     Inactive,
///     Pending,
/// }
///
/// impl From<Status> for DbEnum<Status> {
///     fn from(s: Status) -> Self {
///         DbEnum(s)
///     }
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DbEnum<E>(pub E);

impl<E: Serialize> Serialize for DbEnum<E> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de, E: Deserialize<'de>> Deserialize<'de> for DbEnum<E> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        E::deserialize(deserializer).map(DbEnum)
    }
}

impl<E: fmt::Display> fmt::Display for DbEnum<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<E> From<E> for DbEnum<E> {
    fn from(e: E) -> Self {
        DbEnum(e)
    }
}

impl<E> DbEnum<E> {
    /// Get the inner value
    pub fn into_inner(self) -> E {
        self.0
    }

    /// Get a reference to the inner value
    pub fn inner(&self) -> &E {
        &self.0
    }
}

// =============================================================================
// CASTABLE TRAIT
// =============================================================================

/// Trait for types that can be cast to/from database values
pub trait Castable: Sized {
    /// Cast from a serde_json::Value
    fn from_json(value: &serde_json::Value) -> Result<Self, String>;

    /// Cast to a serde_json::Value
    fn to_json(&self) -> serde_json::Value;
}

// Implement Castable for common types
impl Castable for String {
    fn from_json(value: &serde_json::Value) -> Result<Self, String> {
        value
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "Expected string".to_string())
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::Value::String(self.clone())
    }
}

impl Castable for i32 {
    fn from_json(value: &serde_json::Value) -> Result<Self, String> {
        value
            .as_i64()
            .map(|n| n as i32)
            .ok_or_else(|| "Expected integer".to_string())
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::Value::Number((*self).into())
    }
}

impl Castable for i64 {
    fn from_json(value: &serde_json::Value) -> Result<Self, String> {
        value.as_i64().ok_or_else(|| "Expected integer".to_string())
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::Value::Number((*self).into())
    }
}

impl Castable for f64 {
    fn from_json(value: &serde_json::Value) -> Result<Self, String> {
        value.as_f64().ok_or_else(|| "Expected float".to_string())
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!(*self)
    }
}

impl Castable for bool {
    fn from_json(value: &serde_json::Value) -> Result<Self, String> {
        value
            .as_bool()
            .ok_or_else(|| "Expected boolean".to_string())
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::Value::Bool(*self)
    }
}

impl Castable for Uuid {
    fn from_json(value: &serde_json::Value) -> Result<Self, String> {
        value
            .as_str()
            .ok_or_else(|| "Expected string".to_string())
            .and_then(|s| Uuid::parse_str(s).map_err(|e| e.to_string()))
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::Value::String(self.to_string())
    }
}

impl<T: Castable> Castable for Option<T> {
    fn from_json(value: &serde_json::Value) -> Result<Self, String> {
        if value.is_null() {
            Ok(None)
        } else {
            T::from_json(value).map(Some)
        }
    }

    fn to_json(&self) -> serde_json::Value {
        match self {
            Some(v) => v.to_json(),
            None => serde_json::Value::Null,
        }
    }
}

impl<T: Castable> Castable for Vec<T> {
    fn from_json(value: &serde_json::Value) -> Result<Self, String> {
        value
            .as_array()
            .ok_or_else(|| "Expected array".to_string())
            .and_then(|arr| {
                arr.iter()
                    .map(|v| T::from_json(v))
                    .collect::<Result<Vec<_>, _>>()
            })
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::Value::Array(self.iter().map(|v| v.to_json()).collect())
    }
}

// =============================================================================
// ATTRIBUTE CASTER TRAIT
// =============================================================================

/// Trait for attribute casters that transform values when reading/writing
///
/// Implement this trait to create custom casting logic for model attributes.
pub trait AttributeCaster<T>: Sized {
    /// Cast from database value to Rust type
    fn get(value: serde_json::Value) -> Result<T, String>;

    /// Cast from Rust type to database value
    fn set(value: &T) -> serde_json::Value;
}

// =============================================================================
// ENCRYPTED TYPE
// =============================================================================

/// Encrypted value wrapper.
///
/// Values are serialized to TideORM's encrypted payload format and must be
/// deserialized from that same encrypted format.
/// Uses XChaCha20-Poly1305 authenticated encryption.
///
/// **Note**: You must configure an encryption key in TideConfig for this to work.
/// Plaintext values are not accepted during deserialization.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Model)]
/// pub struct User {
///     #[tideorm(cast = "encrypted")]
///     pub ssn: Encrypted<String>,
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Encrypted<T> {
    value: T,
}

const ENCRYPTED_PAYLOAD_AAD: &[u8] = b"tideorm:encrypted-field:v1";
const ENCRYPTED_PAYLOAD_PREFIX: &str = "enc::";

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
        let plaintext = serde_json::to_vec(&self.value).map_err(serde::ser::Error::custom)?;
        let encoded = encrypt_encrypted_payload(&plaintext).map_err(serde::ser::Error::custom)?;
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
        let ciphertext = text.strip_prefix(ENCRYPTED_PAYLOAD_PREFIX).ok_or_else(|| {
            serde::de::Error::custom("Encrypted fields must use the encrypted payload format")
        })?;
        let plaintext = decrypt_encrypted_payload(ciphertext).map_err(serde::de::Error::custom)?;
        let value = serde_json::from_slice(&plaintext).map_err(serde::de::Error::custom)?;
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

fn encrypted_field_missing_key_error(operation: &str) -> crate::Error {
    crate::Error::configuration(format!(
        "Encrypted<T> {} requires an encryption key. Configure one during startup with TideConfig::init().encryption_key(\"...\") or TokenConfig::set_encryption_key(\"...\") before using encrypted fields.",
        operation
    ))
}

fn encrypted_field_encryption_key(operation: &str) -> crate::error::Result<String> {
    crate::tokenization::TokenConfig::get_encryption_key()
        .map_err(|_| encrypted_field_missing_key_error(operation))
}

fn encrypt_encrypted_payload(plaintext: &[u8]) -> crate::error::Result<String> {
    let key = encrypted_field_encryption_key("serialization")?;
    let cipher = XChaCha20Poly1305::new((&crate::tokenization::derive_encryption_key(&key)).into());
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

fn decrypt_encrypted_payload(encoded: &str) -> crate::error::Result<Vec<u8>> {
    let key = encrypted_field_encryption_key("deserialization")?;
    let cipher = XChaCha20Poly1305::new((&crate::tokenization::derive_encryption_key(&key)).into());
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

// =============================================================================
// HASHED TYPE
// =============================================================================

/// Hashed string wrapper (one-way hash, e.g., for passwords).
///
/// Values are stored as Argon2 hashes. `verify` accepts only Argon2 hashes and
/// returns `false` for any other hash format.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Model)]
/// pub struct User {
///     #[tideorm(cast = "hashed")]
///     pub password: Hashed,
/// }
///
/// // Usage
/// let user = User { password: Hashed::from("secret123") };
/// user.save().await?;
///
/// // Verify
/// if user.password.verify("secret123") {
///     println!("Password matches!");
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Hashed {
    /// The hashed value (stored)
    hash: String,
}

impl Hashed {
    /// Create a new hashed value from plain text
    pub fn new(plain_text: &str) -> Self {
        Self {
            hash: Self::compute_hash(plain_text),
        }
    }

    /// Create from an existing Argon2 hash.
    pub fn from_hash(hash: String) -> Self {
        Self { hash }
    }

    /// Get the hash value
    pub fn hash(&self) -> &str {
        &self.hash
    }

    /// Verify a plain text value against the stored Argon2 hash.
    pub fn verify(&self, plain_text: &str) -> bool {
        let Ok(parsed_hash) = argon2::password_hash::PasswordHash::new(&self.hash) else {
            return false;
        };

        argon2::Argon2::default()
            .verify_password(plain_text.as_bytes(), &parsed_hash)
            .is_ok()
    }

    /// Compute an Argon2 hash suitable for password storage.
    fn compute_hash(input: &str) -> String {
        use argon2::password_hash::{PasswordHasher, SaltString, rand_core::OsRng};

        let salt = SaltString::generate(&mut OsRng);
        argon2::Argon2::default()
            .hash_password(input.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .expect("argon2 hashing should succeed with generated salt")
    }
}

impl From<&str> for Hashed {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for Hashed {
    fn from(s: String) -> Self {
        Self::new(&s)
    }
}

impl fmt::Display for Hashed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "***HASHED***")
    }
}

impl Serialize for Hashed {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.hash.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Hashed {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        String::deserialize(deserializer).map(|hash| Self { hash })
    }
}

// =============================================================================
// COMMA SEPARATED TYPE
// =============================================================================

/// Comma-separated string wrapper
///
/// Stores arrays as comma-separated strings in the database.
/// Useful for databases without native array support.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Model)]
/// pub struct User {
///     #[tideorm(cast = "comma_separated")]
///     pub tags: CommaSeparated<String>,
/// }
///
/// // Usage
/// let user = User { tags: vec!["admin", "user"].into() };
/// user.save().await?; // Stored as "admin,user"
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommaSeparated<T> {
    values: Vec<T>,
}

impl<T> CommaSeparated<T> {
    /// Create a new comma-separated list
    pub fn new(values: Vec<T>) -> Self {
        Self { values }
    }

    /// Get the values
    pub fn values(&self) -> &[T] {
        &self.values
    }

    /// Get mutable values
    pub fn values_mut(&mut self) -> &mut Vec<T> {
        &mut self.values
    }

    /// Into inner values
    pub fn into_inner(self) -> Vec<T> {
        self.values
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Get length
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Add a value
    pub fn push(&mut self, value: T) {
        self.values.push(value);
    }

    /// Check if contains a value
    pub fn contains(&self, value: &T) -> bool
    where
        T: PartialEq,
    {
        self.values.contains(value)
    }
}

impl<T: FromStr> CommaSeparated<T>
where
    T::Err: fmt::Debug,
{
    /// Parse from comma-separated string
    pub fn from_string(s: &str) -> Self {
        let values = s
            .split(',')
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        Self { values }
    }
}

impl<T> From<Vec<T>> for CommaSeparated<T> {
    fn from(values: Vec<T>) -> Self {
        Self::new(values)
    }
}

impl<T> FromIterator<T> for CommaSeparated<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self {
            values: iter.into_iter().collect(),
        }
    }
}

impl<T> IntoIterator for CommaSeparated<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.values.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a CommaSeparated<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.values.iter()
    }
}

impl<T: Serialize> Serialize for CommaSeparated<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.values.serialize(serializer)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for CommaSeparated<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Vec::<T>::deserialize(deserializer).map(|v| Self { values: v })
    }
}

impl<T: Default> Default for CommaSeparated<T> {
    fn default() -> Self {
        Self { values: Vec::new() }
    }
}

impl<T: fmt::Display> fmt::Display for CommaSeparated<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self
            .values
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",");
        write!(f, "{}", s)
    }
}

#[cfg(test)]
#[path = "testing/types_tests.rs"]
mod tests;

// =============================================================================
// COLLECTION TYPE
// =============================================================================

/// Collection wrapper for JSON array columns
///
/// Provides a convenient interface for working with JSON arrays in the database.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Model)]
/// pub struct User {
///     pub permissions: Collection<String>,
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Collection<T> {
    items: Vec<T>,
}

impl<T> Collection<T> {
    /// Create a new collection
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Create from a vector
    pub fn from_vec(items: Vec<T>) -> Self {
        Self { items }
    }

    /// Get all items
    pub fn all(&self) -> &[T] {
        &self.items
    }

    /// Get first item
    pub fn first(&self) -> Option<&T> {
        self.items.first()
    }

    /// Get last item
    pub fn last(&self) -> Option<&T> {
        self.items.last()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Get count
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// Add an item
    pub fn add(&mut self, item: T) {
        self.items.push(item);
    }

    /// Remove item at index
    pub fn remove(&mut self, index: usize) -> Option<T> {
        if index < self.items.len() {
            Some(self.items.remove(index))
        } else {
            None
        }
    }

    /// Filter items
    pub fn filter<F: Fn(&T) -> bool>(&self, predicate: F) -> Self
    where
        T: Clone,
    {
        Self {
            items: self
                .items
                .iter()
                .filter(|i| predicate(i))
                .cloned()
                .collect(),
        }
    }

    /// Map items
    pub fn map<U, F: Fn(&T) -> U>(&self, mapper: F) -> Collection<U> {
        Collection {
            items: self.items.iter().map(mapper).collect(),
        }
    }

    /// Find item
    pub fn find<F: Fn(&T) -> bool>(&self, predicate: F) -> Option<&T> {
        self.items.iter().find(|i| predicate(i))
    }

    /// Check if any matches
    pub fn any<F: Fn(&T) -> bool>(&self, predicate: F) -> bool {
        self.items.iter().any(predicate)
    }

    /// Check if all match
    pub fn every<F: Fn(&T) -> bool>(&self, predicate: F) -> bool {
        self.items.iter().all(predicate)
    }

    /// Pluck values (for collections of objects)
    pub fn pluck<U, F: Fn(&T) -> U>(&self, extractor: F) -> Vec<U> {
        self.items.iter().map(extractor).collect()
    }

    /// Sort items (returns new collection)
    pub fn sorted<F: FnMut(&T, &T) -> std::cmp::Ordering>(&self, compare: F) -> Self
    where
        T: Clone,
    {
        let mut items = self.items.clone();
        items.sort_by(compare);
        Self { items }
    }

    /// Take first n items
    pub fn take(&self, n: usize) -> Self
    where
        T: Clone,
    {
        Self {
            items: self.items.iter().take(n).cloned().collect(),
        }
    }

    /// Skip first n items
    pub fn skip(&self, n: usize) -> Self
    where
        T: Clone,
    {
        Self {
            items: self.items.iter().skip(n).cloned().collect(),
        }
    }

    /// Convert to vector
    pub fn to_vec(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.items.clone()
    }

    /// Into inner vector
    pub fn into_inner(self) -> Vec<T> {
        self.items
    }
}

impl<T> From<Vec<T>> for Collection<T> {
    fn from(items: Vec<T>) -> Self {
        Self::from_vec(items)
    }
}

impl<T> Default for Collection<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> FromIterator<T> for Collection<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self {
            items: iter.into_iter().collect(),
        }
    }
}

impl<T> IntoIterator for Collection<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a Collection<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

impl<T: Serialize> Serialize for Collection<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.items.serialize(serializer)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Collection<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Vec::<T>::deserialize(deserializer).map(|v| Self { items: v })
    }
}

// =============================================================================
// CASTER REGISTRY
// =============================================================================

/// Cast type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastType {
    /// String cast
    String,
    /// Integer cast (i64)
    Integer,
    /// Float cast (f64)
    Float,
    /// Boolean cast
    Boolean,
    /// JSON/JSONB cast
    Json,
    /// Array cast (comma-separated in string databases)
    Array,
    /// DateTime cast
    DateTime,
    /// Date only cast
    Date,
    /// Time only cast
    Time,
    /// UUID cast
    Uuid,
    /// Decimal cast
    Decimal,
    /// Encrypted cast
    Encrypted,
    /// Hashed cast (one-way)
    Hashed,
    /// Comma-separated array
    CommaSeparated,
    /// Collection (JSON array)
    Collection,
    /// Custom cast (user-defined)
    Custom,
}

impl CastType {
    /// Parse from string
    pub fn parse_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "string" | "str" => Some(Self::String),
            "integer" | "int" | "i64" | "i32" => Some(Self::Integer),
            "float" | "f64" | "f32" | "double" => Some(Self::Float),
            "boolean" | "bool" => Some(Self::Boolean),
            "json" | "jsonb" => Some(Self::Json),
            "array" => Some(Self::Array),
            "datetime" | "timestamp" => Some(Self::DateTime),
            "date" => Some(Self::Date),
            "time" => Some(Self::Time),
            "uuid" => Some(Self::Uuid),
            "decimal" => Some(Self::Decimal),
            "encrypted" => Some(Self::Encrypted),
            "hashed" | "hash" => Some(Self::Hashed),
            "comma_separated" | "csv" => Some(Self::CommaSeparated),
            "collection" => Some(Self::Collection),
            _ => None,
        }
    }
}

impl fmt::Display for CastType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String => write!(f, "string"),
            Self::Integer => write!(f, "integer"),
            Self::Float => write!(f, "float"),
            Self::Boolean => write!(f, "boolean"),
            Self::Json => write!(f, "json"),
            Self::Array => write!(f, "array"),
            Self::DateTime => write!(f, "datetime"),
            Self::Date => write!(f, "date"),
            Self::Time => write!(f, "time"),
            Self::Uuid => write!(f, "uuid"),
            Self::Decimal => write!(f, "decimal"),
            Self::Encrypted => write!(f, "encrypted"),
            Self::Hashed => write!(f, "hashed"),
            Self::CommaSeparated => write!(f, "comma_separated"),
            Self::Collection => write!(f, "collection"),
            Self::Custom => write!(f, "custom"),
        }
    }
}

// =============================================================================
// CAST VALUE HELPER
// =============================================================================

/// Helper struct for casting values at runtime
pub struct CastValue;

impl CastValue {
    /// Cast a JSON value based on cast type
    pub fn cast(
        value: &serde_json::Value,
        cast_type: CastType,
    ) -> Result<serde_json::Value, String> {
        match cast_type {
            CastType::String => match value {
                serde_json::Value::String(s) => Ok(serde_json::Value::String(s.clone())),
                serde_json::Value::Number(n) => Ok(serde_json::Value::String(n.to_string())),
                serde_json::Value::Bool(b) => Ok(serde_json::Value::String(b.to_string())),
                serde_json::Value::Null => Ok(serde_json::Value::Null),
                _ => Ok(serde_json::Value::String(value.to_string())),
            },
            CastType::Integer => match value {
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        Ok(serde_json::json!(i))
                    } else if let Some(f) = n.as_f64() {
                        Ok(serde_json::json!(f as i64))
                    } else {
                        Err("Invalid number".to_string())
                    }
                }
                serde_json::Value::String(s) => s
                    .parse::<i64>()
                    .map(|i| serde_json::json!(i))
                    .map_err(|_| "Failed to parse integer".to_string()),
                serde_json::Value::Bool(b) => Ok(serde_json::json!(if *b { 1 } else { 0 })),
                serde_json::Value::Null => Ok(serde_json::Value::Null),
                _ => Err("Cannot cast to integer".to_string()),
            },
            CastType::Float => match value {
                serde_json::Value::Number(n) => {
                    if let Some(f) = n.as_f64() {
                        Ok(serde_json::json!(f))
                    } else {
                        Err("Invalid number".to_string())
                    }
                }
                serde_json::Value::String(s) => s
                    .parse::<f64>()
                    .map(|f| serde_json::json!(f))
                    .map_err(|_| "Failed to parse float".to_string()),
                serde_json::Value::Bool(b) => Ok(serde_json::json!(if *b { 1.0 } else { 0.0 })),
                serde_json::Value::Null => Ok(serde_json::Value::Null),
                _ => Err("Cannot cast to float".to_string()),
            },
            CastType::Boolean => match value {
                serde_json::Value::Bool(b) => Ok(serde_json::Value::Bool(*b)),
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        Ok(serde_json::Value::Bool(i != 0))
                    } else {
                        Ok(serde_json::Value::Bool(true))
                    }
                }
                serde_json::Value::String(s) => {
                    let lower = s.to_lowercase();
                    Ok(serde_json::Value::Bool(
                        lower == "true" || lower == "1" || lower == "yes" || lower == "on",
                    ))
                }
                serde_json::Value::Null => Ok(serde_json::Value::Bool(false)),
                _ => Err("Cannot cast to boolean".to_string()),
            },
            CastType::Json => {
                // JSON cast - value is already JSON
                Ok(value.clone())
            }
            CastType::Array | CastType::Collection => {
                match value {
                    serde_json::Value::Array(_) => Ok(value.clone()),
                    serde_json::Value::String(s) => {
                        // Try to parse as JSON array
                        serde_json::from_str(s).or_else(|_| {
                            // Fallback to comma-separated
                            Ok(serde_json::Value::Array(
                                s.split(',')
                                    .map(|v| serde_json::Value::String(v.trim().to_string()))
                                    .collect(),
                            ))
                        })
                    }
                    serde_json::Value::Null => Ok(serde_json::Value::Array(vec![])),
                    _ => Err("Cannot cast to array".to_string()),
                }
            }
            CastType::DateTime => {
                match value {
                    serde_json::Value::String(s) => {
                        // Validate it's a valid datetime string
                        if chrono::DateTime::parse_from_rfc3339(s).is_ok() {
                            Ok(value.clone())
                        } else {
                            Err("Invalid datetime format".to_string())
                        }
                    }
                    serde_json::Value::Null => Ok(serde_json::Value::Null),
                    _ => Err("Cannot cast to datetime".to_string()),
                }
            }
            CastType::Date => {
                match value {
                    serde_json::Value::String(s) => {
                        // Validate it's a valid date string
                        if chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() {
                            Ok(value.clone())
                        } else {
                            Err("Invalid date format".to_string())
                        }
                    }
                    serde_json::Value::Null => Ok(serde_json::Value::Null),
                    _ => Err("Cannot cast to date".to_string()),
                }
            }
            CastType::Time => {
                match value {
                    serde_json::Value::String(s) => {
                        // Validate it's a valid time string
                        if chrono::NaiveTime::parse_from_str(s, "%H:%M:%S").is_ok()
                            || chrono::NaiveTime::parse_from_str(s, "%H:%M").is_ok()
                        {
                            Ok(value.clone())
                        } else {
                            Err("Invalid time format".to_string())
                        }
                    }
                    serde_json::Value::Null => Ok(serde_json::Value::Null),
                    _ => Err("Cannot cast to time".to_string()),
                }
            }
            CastType::Uuid => match value {
                serde_json::Value::String(s) => Uuid::parse_str(s)
                    .map(|_| value.clone())
                    .map_err(|e| format!("Invalid UUID: {}", e)),
                serde_json::Value::Null => Ok(serde_json::Value::Null),
                _ => Err("Cannot cast to UUID".to_string()),
            },
            CastType::Decimal => match value {
                serde_json::Value::Number(_) => Ok(value.clone()),
                serde_json::Value::String(s) => s
                    .parse::<f64>()
                    .map(|f| serde_json::json!(f))
                    .map_err(|_| "Failed to parse decimal".to_string()),
                serde_json::Value::Null => Ok(serde_json::Value::Null),
                _ => Err("Cannot cast to decimal".to_string()),
            },
            CastType::Encrypted | CastType::Hashed => {
                // These require special handling - pass through for now
                Ok(value.clone())
            }
            CastType::CommaSeparated => match value {
                serde_json::Value::Array(arr) => {
                    let strings: Vec<String> = arr
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                    Ok(serde_json::Value::String(strings.join(",")))
                }
                serde_json::Value::String(_) => Ok(value.clone()),
                serde_json::Value::Null => Ok(serde_json::Value::String(String::new())),
                _ => Err("Cannot cast to comma-separated".to_string()),
            },
            CastType::Custom => {
                // Custom casts pass through unchanged
                Ok(value.clone())
            }
        }
    }

    /// Parse comma-separated to array
    pub fn parse_comma_separated(s: &str) -> Vec<String> {
        s.split(',')
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect()
    }

    /// Format array as comma-separated string
    pub fn format_comma_separated<T: fmt::Display>(values: &[T]) -> String {
        values
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",")
    }
}

// =============================================================================
// ACCESSOR TRAIT (for computed attributes)
// =============================================================================

/// Trait for models with computed/accessor attributes
///
/// Implement this to add computed properties that are calculated on the fly.
///
/// # Example
///
/// ```rust,ignore
/// impl Accessor for User {
///     fn get_accessor(&self, key: &str) -> Option<serde_json::Value> {
///         match key {
///             "full_name" => Some(serde_json::json!(format!("{} {}", self.first_name, self.last_name))),
///             "is_admin" => Some(serde_json::json!(self.role == "admin")),
///             _ => None,
///         }
///     }
///     
///     fn accessor_keys() -> Vec<&'static str> {
///         vec!["full_name", "is_admin"]
///     }
/// }
/// ```
pub trait Accessor {
    /// Get a computed attribute value
    fn get_accessor(&self, key: &str) -> Option<serde_json::Value>;

    /// List all accessor keys
    fn accessor_keys() -> Vec<&'static str> {
        vec![]
    }
}

/// Trait for models with mutator attributes
///
/// Implement this to transform values before they are stored.
///
/// # Example
///
/// ```rust,ignore
/// impl Mutator for User {
///     fn set_mutator(&mut self, key: &str, value: serde_json::Value) -> bool {
///         match key {
///             "email" => {
///                 // Always lowercase emails
///                 if let Some(email) = value.as_str() {
///                     self.email = email.to_lowercase();
///                     return true;
///                 }
///                 false
///             }
///             _ => false,
///         }
///     }
///     
///     fn mutator_keys() -> Vec<&'static str> {
///         vec!["email"]
///     }
/// }
/// ```
pub trait Mutator {
    /// Transform and set a value
    fn set_mutator(&mut self, key: &str, value: serde_json::Value) -> bool;

    /// List all mutator keys
    fn mutator_keys() -> Vec<&'static str> {
        vec![]
    }
}

// =============================================================================
// DEFAULT VALUE WRAPPER
// =============================================================================

/// Wrapper for fields with default values
///
/// Allows defining default values that are applied when the field is not set.
#[derive(Clone, Debug)]
pub struct WithDefault<T> {
    value: Option<T>,
    _marker: PhantomData<T>,
}

impl<T: Clone> WithDefault<T> {
    /// Create a new WithDefault (no value set)
    pub fn none() -> Self {
        Self {
            value: None,
            _marker: PhantomData,
        }
    }

    /// Create with a value
    pub fn some(value: T) -> Self {
        Self {
            value: Some(value),
            _marker: PhantomData,
        }
    }

    /// Get the value or the provided default
    pub fn unwrap_or(&self, default: T) -> T {
        self.value.clone().unwrap_or(default)
    }

    /// Get the value or call a function for the default
    pub fn unwrap_or_else<F: FnOnce() -> T>(&self, f: F) -> T {
        self.value.clone().unwrap_or_else(f)
    }

    /// Check if value is set
    pub fn is_some(&self) -> bool {
        self.value.is_some()
    }

    /// Check if value is not set
    pub fn is_none(&self) -> bool {
        self.value.is_none()
    }

    /// Get the inner Option
    pub fn into_option(self) -> Option<T> {
        self.value
    }
}

impl<T: Default + Clone> Default for WithDefault<T> {
    fn default() -> Self {
        Self::some(T::default())
    }
}

impl<T: Serialize + Clone> Serialize for WithDefault<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.value.serialize(serializer)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for WithDefault<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Option::<T>::deserialize(deserializer).map(|v| Self {
            value: v,
            _marker: PhantomData,
        })
    }
}
